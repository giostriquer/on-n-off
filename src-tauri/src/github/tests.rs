use super::*;
use crate::cli::AgentCli;
use crate::dto::GithubStatus;
use crate::http::{refused_url, serve_sequence};
use crate::paths::{github_prs_path_for, scratch_dir};
use crate::settings::AppSettings;
use connection::HttpMemo;
use fixtures::{authorization, gh, gh_runs, FakeGithub, REPLY};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const NOW: i64 = 1_787_000_000;

mod merge_refresh;

fn settings(scopes: &[&str], poll_seconds: u16) -> AppSettings {
    AppSettings {
        github_scopes: scopes.iter().map(|scope| scope.to_string()).collect(),
        github_poll_seconds: poll_seconds,
        ..AppSettings::default()
    }
}

struct Harness {
    home: PathBuf,
    settings: AppSettings,
    read_memo: ReadMemo,
    http: HttpMemo,
}

impl Harness {
    fn new(prefix: &str, scopes: &[&str], poll_seconds: u16) -> Self {
        Self {
            home: scratch_dir(prefix),
            settings: settings(scopes, poll_seconds),
            read_memo: ReadMemo::new(),
            http: HttpMemo::new(),
        }
    }

    fn read(&self, github: &dyn Github, now_secs: i64, force: bool) -> GithubPrsDto {
        self.read_revisioned(github, now_secs, force).0
    }

    fn read_revisioned(
        &self,
        github: &dyn Github,
        now_secs: i64,
        force: bool,
    ) -> (GithubPrsDto, Reading) {
        read_prs_in(
            &Sources {
                home: &self.home,
                settings: &self.settings,
                read_memo: &self.read_memo,
                github,
                now_secs,
            },
            force,
        )
    }

    fn read_http(&self, cli: &AgentCli, url: &str, now_secs: i64, force: bool) -> GithubPrsDto {
        self.read_with_events(cli, url, &refused_url(), now_secs, force)
            .0
    }

    fn read_with_events(
        &self,
        cli: &AgentCli,
        url: &str,
        events_url: &str,
        now_secs: i64,
        force: bool,
    ) -> (GithubPrsDto, Reading) {
        let github = HttpGithub {
            memo: &self.http,
            cli,
            graphql_url: url,
            events_url,
            now_secs,
        };
        self.read_revisioned(&github, now_secs, force)
    }
}

fn reply() -> Value {
    serde_json::from_str(REPLY).unwrap()
}

fn answering(reply: Value) -> FakeGithub {
    FakeGithub::default().reply(reply)
}

fn unreachable() -> FakeGithub {
    FakeGithub::default().failing_reply("Could not reach GitHub (network error: refused).")
}

fn reply_with_budget(remaining: u64, reset_epoch_secs: i64) -> String {
    let mut value = reply();
    value["data"]["rateLimit"] =
        json!({ "remaining": remaining, "resetAt": rfc3339(reset_epoch_secs).unwrap() });
    value.to_string()
}

fn assert_no_token(dto: &GithubPrsDto, home: &Path, token: &str) {
    let json = serde_json::to_string(dto).unwrap();
    assert!(!json.contains(token), "the DTO carries the token: {json}");
    if let Ok(raw) = fs::read_to_string(github_prs_path_for(home)) {
        assert!(
            !raw.contains(token),
            "the snapshot carries the token: {raw}"
        );
    }
}

#[test]
fn a_successful_read_fills_the_lists_and_remembers_them() {
    let harness = Harness::new("gh-read-ok", &["org:acme"], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);

    let dto = harness.read_http(&gh, &url, NOW, false);

    assert_eq!(dto.status, GithubStatus::Ok);
    assert_eq!(dto.hint, None);
    assert!(!dto.stale);
    assert_eq!(dto.data.viewer.as_deref(), Some("octocat"));
    assert_eq!(dto.data.fetched_at.as_deref(), Some("2026-08-17T20:53:20Z"));
    assert_eq!(dto.data.scope, vec!["org:acme".to_string()]);
    assert_eq!(dto.data.mine.items.len(), 1);
    assert_eq!(dto.data.review_requested.items.len(), 2);
    assert_eq!(dto.data.assigned.total, 0);
    assert_eq!(dto.data.rate_limit.as_ref().unwrap().remaining, 4998);

    let requests = server.join().unwrap();
    assert_eq!(authorization(&requests[0]), "Bearer gho_t");
    let body: Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(
        body["variables"]["mine"],
        "is:pr is:open author:@me org:acme"
    );
    assert_eq!(
        body["variables"]["merged"],
        "is:pr is:merged author:@me sort:updated-desc org:acme"
    );
    assert_eq!(dto.data.merged.items[0].id, "PR_merged1");
    assert_no_token(&dto, &harness.home, "gho_t");
    assert_eq!(
        snapshot::load(&github_prs_path_for(&harness.home)).map(|snapshot| snapshot.data),
        Some(dto.data)
    );
}

#[test]
fn the_memory_window_ends_five_seconds_before_the_poll_interval() {
    let harness = Harness::new("gh-read-cache-edge", &[], 60);
    let first = harness.read(&answering(reply()), NOW, false);

    let silent = FakeGithub::default();
    assert_eq!(harness.read(&silent, NOW + 54, false), first);
    assert_eq!(silent.requests(), 0);
    let github = answering(reply());
    assert_eq!(
        harness.read(&github, NOW + 55, false).status,
        GithubStatus::Ok
    );
    assert_eq!(github.requests(), 1);
}

#[test]
fn a_fresh_read_is_served_from_memory_until_the_poll_interval_nears() {
    let harness = Harness::new("gh-read-cache", &[], 60);
    let first = harness.read(&answering(reply()), NOW, false);

    let cached = harness.read(&unreachable(), NOW + 30, false);
    assert_eq!(cached, first, "inside the window nothing is fetched");

    let expired = harness.read(&unreachable(), NOW + 56, false);
    assert_eq!(expired.status, GithubStatus::Network);
    assert!(expired.stale, "the snapshot backs the failed refresh");
    assert_eq!(expired.data.mine.items, first.data.mine.items);
    assert_eq!(expired.data.fetched_at, first.data.fetched_at);
}

#[test]
fn only_a_read_that_replaces_the_remembered_result_is_announced() {
    let harness = Harness::new("gh-read-revision", &[], 60);
    let (_, first) = harness.read_revisioned(&answering(reply()), NOW, false);
    assert_eq!(
        first,
        Reading::Replaced(1),
        "the first read replaces nothing-yet"
    );

    let (_, memoised) = harness.read_revisioned(&unreachable(), NOW + 30, false);
    assert_eq!(
        memoised,
        Reading::Unchanged(1),
        "the read a consumer makes in answer to an announcement is memory-served and announces \
         nothing, which is what ends the exchange"
    );

    let (_, failed) = harness.read_revisioned(&unreachable(), NOW + 1, true);
    assert_eq!(
        failed,
        Reading::Unchanged(1),
        "a failure discards the remembered result rather than replacing it with a newer one; \
         announcing would send every consumer back to GitHub for the request that just failed"
    );

    let (_, paused) = harness.read_revisioned(&FakeGithub::paused(), NOW + 2, true);
    assert_eq!(
        paused,
        Reading::Unchanged(1),
        "a paused read replaces nothing"
    );

    let (_, refreshed) = harness.read_revisioned(&answering(reply()), NOW + 2, true);
    assert_eq!(
        refreshed,
        Reading::Replaced(2),
        "the Pull requests screen's refresh replaced the result, so consumers re-read at once"
    );
}

#[test]
fn a_failed_refresh_does_not_leave_the_previous_result_live() {
    let harness = Harness::new("gh-read-failed-refresh", &[], 60);
    harness.read(&answering(reply()), NOW, false);

    let failed = harness.read(&unreachable(), NOW + 1, true);
    assert_eq!(failed.status, GithubStatus::Network);
    assert!(failed.stale);

    let next_poll = harness.read(&unreachable(), NOW + 2, false);
    assert_eq!(
        next_poll.status,
        GithubStatus::Network,
        "a failure must not be papered over by the result it replaced"
    );
    assert!(next_poll.stale);
}

#[test]
fn a_paused_read_is_served_from_memory_not_from_disk() {
    let harness = Harness::new("gh-read-paused-memory", &[], 60);
    let first = harness.read(&answering(reply()), NOW, false);
    fs::remove_file(github_prs_path_for(&harness.home)).unwrap();

    for later in [NOW + 56, NOW + 57] {
        let github = FakeGithub::paused();
        let paused = harness.read(&github, later, false);
        assert_eq!(paused.status, GithubStatus::RateLimited);
        assert_eq!(
            paused.hint.as_deref(),
            Some("GitHub rate limit reached — polling resumes in 5 min.")
        );
        assert!(paused.stale);
        assert_eq!(
            paused.data.mine.items, first.data.mine.items,
            "a pause keeps the remembered result"
        );
        assert_eq!(github.requests(), 0);
    }
}

#[test]
fn a_paused_read_after_a_restart_shows_the_snapshot() {
    let harness = Harness::new("gh-read-paused-snapshot", &[], 60);
    let first = harness.read(&answering(reply()), NOW, false);
    let restarted = Harness {
        home: harness.home.clone(),
        ..Harness::new("gh-read-paused-snapshot-restarted", &[], 60)
    };

    let github = FakeGithub::paused();
    let paused = restarted.read(&github, NOW + 120, true);

    assert_eq!(paused.status, GithubStatus::RateLimited);
    assert!(
        paused.stale,
        "the snapshot backs a paused read with no memory"
    );
    assert_eq!(paused.data.mine.items, first.data.mine.items);
    assert_eq!(github.requests(), 0);
}

#[test]
fn a_fresh_read_is_served_from_memory_even_while_reads_are_paused() {
    let harness = Harness::new("gh-read-fresh-while-paused", &[], 60);
    let (first, _) = harness.read_revisioned(&answering(reply()), NOW, false);

    let github = FakeGithub::paused();
    let (served, reading) = harness.read_revisioned(&github, NOW + 30, false);

    assert_eq!(served, first, "the memory window comes before the pause");
    assert_eq!(reading, Reading::Unchanged(1));
    assert_eq!(github.requests(), 0);
}

#[test]
fn force_skips_the_memory() {
    let harness = Harness::new("gh-read-force", &[], 60);
    harness.read(&answering(reply()), NOW, false);

    let github = answering(reply());
    let forced = harness.read(&github, NOW + 1, true);

    assert_eq!(forced.status, GithubStatus::Ok);
    assert_eq!(github.requests(), 1);
}

#[test]
fn a_failure_after_a_restart_shows_the_snapshot_as_stale() {
    let harness = Harness::new("gh-read-missing", &["org:acme"], 60);
    let first = harness.read(&answering(reply()), NOW, false);

    let later = Harness {
        home: harness.home.clone(),
        settings: settings(&["org:other"], 60),
        read_memo: ReadMemo::new(),
        http: HttpMemo::new(),
    };
    let missing = Failure::new(
        GithubStatus::GhMissing,
        "Install the GitHub CLI (`gh`), sign in with `gh auth login`, then refresh.",
    );
    let dto = later.read(
        &FakeGithub::default().failing_with(missing.clone()),
        NOW + 120,
        false,
    );

    assert_eq!(dto.status, GithubStatus::GhMissing);
    assert!(dto.stale);
    assert_eq!(dto.hint, Some(missing.hint));
    assert_eq!(dto.data.mine.items, first.data.mine.items);
    assert_eq!(dto.data.fetched_at, first.data.fetched_at);
    assert_eq!(dto.data.viewer, first.data.viewer);
    assert_eq!(
        dto.data.scope, first.data.scope,
        "the lists reflect the scope they were read with"
    );
}

#[test]
fn a_token_rejected_twice_is_reported_without_the_token() {
    let harness = Harness::new("gh-read-401-twice", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_x");
    let (url, server) = serve_sequence(&[
        ("401 Unauthorized", &[], "{}"),
        ("401 Unauthorized", &[], "{}"),
    ]);

    let dto = harness.read_http(&gh, &url, NOW, false);

    assert_eq!(server.join().unwrap().len(), 2);
    assert_eq!(dto.status, GithubStatus::TokenRejected);
    assert_no_token(&dto, &harness.home, "gho_x");
    assert!(
        dto.hint.as_deref().unwrap().contains("gh auth login"),
        "{:?}",
        dto.hint
    );
}

#[test]
fn a_nearly_spent_budget_pauses_polling_with_the_last_read_shown() {
    let harness = Harness::new("gh-read-low-budget", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let body = reply_with_budget(10, NOW + 120);
    let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);

    let first = harness.read_http(&gh, &url, NOW, false);
    server.join().unwrap();
    assert_eq!(first.status, GithubStatus::Ok);

    let paused = harness.read_http(&gh, &refused_url(), NOW + 5, true);
    assert_eq!(paused.status, GithubStatus::RateLimited);
    assert!(paused.stale);
    assert_eq!(paused.data.mine.items, first.data.mine.items);
    assert!(
        paused.hint.as_deref().unwrap().contains("2 min"),
        "{:?}",
        paused.hint
    );
}

#[test]
fn an_unreadable_or_failed_reply_reports_network() {
    let harness = Harness::new("gh-read-network", &[], 60);

    let refused = harness.read(&unreachable(), NOW, true);
    assert_eq!(refused.status, GithubStatus::Network);
    assert_eq!(
        refused.hint.as_deref(),
        Some("Could not reach GitHub (network error: refused).")
    );

    let unreadable = harness.read(
        &answering(json!({"data":null,"errors":[{"message":"Bad query"}]})),
        NOW,
        true,
    );
    assert_eq!(unreadable.status, GithubStatus::Network);
    assert!(
        unreadable.hint.as_deref().unwrap().contains("Bad query"),
        "{:?}",
        unreadable.hint
    );
    assert!(
        !unreadable.stale,
        "nothing succeeded, so there is no snapshot"
    );
}

#[test]
fn the_memory_window_follows_the_current_poll_interval() {
    let mut harness = Harness::new("gh-read-ttl", &[], 300);
    harness.read(&answering(reply()), NOW, false);

    harness.settings = settings(&[], 30);
    let github = answering(reply());
    let dto = harness.read(&github, NOW + 40, false);

    assert_eq!(dto.status, GithubStatus::Ok);
    assert_eq!(
        github.requests(),
        1,
        "a shorter interval shrinks the window"
    );
}

#[test]
fn a_saved_pull_request_interval_replaces_the_remembered_one() {
    set_poll_seconds(120);
    assert_eq!(POLL_SECONDS.load(Ordering::Acquire), 120);
}

fn pushed_to_shared_branch() -> Value {
    json!([{
        "id": "10", "type": "PushEvent", "repo": { "name": "acme/app" },
        "payload": { "ref": "refs/heads/fix/shared", "head": "head1", "before": "head0" },
        "created_at": rfc3339(NOW - 60).unwrap(),
    }])
}

fn shared_branch_lookup() -> Value {
    fixtures::lookup_reply(
        json!([]),
        &[json!([fixtures::contribution_node(
            "PR_shared",
            "alice",
            "acme/app",
            "OPEN"
        )])],
    )
}

fn your_commit() -> Value {
    fixtures::commits_reply(&[vec![fixtures::commit(1, &["octocat"])]])
}

fn events_url(url: &str) -> String {
    url.replace("/graphql", "/users")
}

fn mine(dto: &GithubPrsDto) -> Vec<(&str, &str, bool)> {
    dto.data
        .mine
        .items
        .iter()
        .map(|pr| (pr.id.as_str(), pr.author.as_str(), pr.contributed))
        .collect()
}

fn read_contribution(harness: &Harness, gh: &AgentCli) {
    let (events, lookup, commit) = (
        pushed_to_shared_branch().to_string(),
        shared_branch_lookup().to_string(),
        your_commit().to_string(),
    );
    let (url, server) = serve_sequence(&[
        ("200 OK", &[], REPLY),
        ("200 OK", &[], &events),
        ("200 OK", &[], &lookup),
        ("200 OK", &[], &commit),
    ]);
    let (dto, _) = harness.read_with_events(gh, &url, &events_url(&url), NOW, false);
    server.join().unwrap();
    assert_eq!(dto.warnings, Vec::<String>::new());
}

#[test]
fn a_pull_request_someone_else_opened_with_a_commit_of_yours_is_read_into_mine() {
    let harness = Harness::new("gh-read-contributed", &["org:acme"], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let (events, lookup, commit) = (
        pushed_to_shared_branch().to_string(),
        shared_branch_lookup().to_string(),
        your_commit().to_string(),
    );
    let (url, server) = serve_sequence(&[
        ("200 OK", &[], REPLY),
        ("200 OK", &[], &events),
        ("200 OK", &[], &lookup),
        ("200 OK", &[], &commit),
    ]);

    let (dto, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW, false);

    assert_eq!(dto.status, GithubStatus::Ok);
    assert_eq!(dto.warnings, Vec::<String>::new());
    assert_eq!(
        mine(&dto),
        vec![("PR_mine1", "octocat", false), ("PR_shared", "alice", true)]
    );
    assert_eq!(dto.data.mine.total, 2);
    let requests = server.join().unwrap();
    assert!(
        requests[1]
            .head
            .starts_with("GET /users/octocat/events?per_page=100&page=1 "),
        "{}",
        requests[1].head
    );
    assert_eq!(authorization(&requests[1]), "Bearer gho_t");
    assert_eq!(
        crate::http::head_header(&requests[1].head, "accept"),
        Some("application/vnd.github+json")
    );
    assert_eq!(authorization(&requests[2]), "Bearer gho_t");
    assert_no_token(&dto, &harness.home, "gho_t");
    let remembered = fs::read_to_string(
        harness
            .home
            .join(".on-n-off")
            .join("github")
            .join("contributions.json"),
    )
    .unwrap();
    assert!(!remembered.contains("gho_t"));
    assert_eq!(
        snapshot::load(&github_prs_path_for(&harness.home)).map(|snapshot| snapshot.data),
        Some(dto.data)
    );
}

#[test]
fn after_a_restart_a_failed_contribution_lookup_shows_the_contributions_last_read() {
    let first = Harness::new("gh-read-contributed-kept", &[], 60);
    let gh = gh(&first.home.join("cli"), "gho_t");
    read_contribution(&first, &gh);
    let restarted = Harness {
        home: first.home.clone(),
        ..Harness::new("gh-read-contributed-kept-again", &[], 60)
    };

    let (url, server) = serve_sequence(&[
        ("200 OK", &[], REPLY),
        ("200 OK", &[], "[]"),
        ("502 Bad Gateway", &[], "{}"),
    ]);
    let (dto, _) = restarted.read_with_events(&gh, &url, &events_url(&url), NOW + 60, true);
    server.join().unwrap();

    assert_eq!(dto.status, GithubStatus::Ok);
    assert!(!dto.stale);
    assert_eq!(
        mine(&dto),
        vec![("PR_mine1", "octocat", false), ("PR_shared", "alice", true)]
    );
    assert_eq!(dto.data.mine.total, 2);
    assert_eq!(
        dto.warnings,
        vec![
            "Pull requests on your pushed branches were not looked up. Could not reach GitHub (HTTP 502)."
                .to_string()
        ]
    );
}

#[test]
fn a_low_rate_budget_shows_the_contributions_last_read_without_asking_github_again() {
    let harness = Harness::new("gh-read-contributed-budget", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    read_contribution(&harness, &gh);

    let low = reply_with_budget(10, NOW + 3_600);
    let (url, server) = serve_sequence(&[("200 OK", &[], &low)]);
    let (dto, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW + 120, true);

    assert_eq!(server.join().unwrap().len(), 1);
    assert_eq!(dto.warnings, Vec::<String>::new());
    assert_eq!(
        mine(&dto),
        vec![("PR_mine1", "octocat", false), ("PR_shared", "alice", true)]
    );
}

#[test]
fn a_rate_limited_contribution_lookup_pauses_the_next_read() {
    let harness = Harness::new("gh-read-lookup-rate-limited", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let events = pushed_to_shared_branch().to_string();
    let reset = format!("X-RateLimit-Reset: {}", NOW + 300);
    let (url, server) = serve_sequence(&[
        ("200 OK", &[], REPLY),
        ("200 OK", &[], &events),
        (
            "403 Forbidden",
            &["X-RateLimit-Remaining: 0", &reset],
            r#"{"message":"API rate limit exceeded"}"#,
        ),
    ]);
    let (dto, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW, false);
    assert_eq!(server.join().unwrap().len(), 3);
    assert_eq!(dto.status, GithubStatus::Ok);
    assert_eq!(
        dto.warnings,
        vec![
            "Pull requests on your pushed branches were not looked up. GitHub rate limit reached — polling resumes in 5 min."
                .to_string()
        ]
    );

    let paused = harness.read_http(&gh, &refused_url(), NOW + 10, true);
    assert_eq!(paused.status, GithubStatus::RateLimited);
    assert_eq!(
        paused.hint.as_deref(),
        Some("GitHub rate limit reached — polling resumes in 5 min.")
    );
    assert!(paused.stale);
    assert_eq!(paused.data.mine.items, dto.data.mine.items);
}

#[test]
fn an_events_read_github_refuses_is_a_warning_and_is_asked_again_on_the_next_read() {
    let cases: [(&str, &[&str], &str); 2] = [
        (
            "503 Service Unavailable",
            &[],
            "Could not reach GitHub (HTTP 503).",
        ),
        (
            "403 Forbidden",
            &["X-RateLimit-Remaining: 12"],
            "Could not reach GitHub (HTTP 403).",
        ),
    ];
    for (index, (status_line, headers, why)) in cases.into_iter().enumerate() {
        let harness = Harness::new(&format!("gh-read-events-refused-{index}"), &[], 60);
        let cli_dir = harness.home.join("cli");
        let gh = gh(&cli_dir, "gho_t");
        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY), (status_line, headers, "{}")]);
        let (dto, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW, false);
        assert_eq!(server.join().unwrap().len(), 2, "{status_line}");
        assert_eq!(dto.status, GithubStatus::Ok, "{status_line}");
        assert_eq!(
            dto.warnings,
            vec![format!("Your recent pushes were not read. {why}")],
            "{status_line}"
        );

        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY), ("200 OK", &[], "[]")]);
        let (next, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW + 60, true);
        let requests = server.join().unwrap();
        assert!(
            requests[1]
                .head
                .starts_with("GET /users/octocat/events?per_page=100&page=1 "),
            "{status_line}: {}",
            requests[1].head
        );
        assert_eq!(next.warnings, Vec::<String>::new(), "{status_line}");
        assert_eq!(gh_runs(&cli_dir), 1, "{status_line}");
    }
}

#[test]
fn a_rate_limited_events_read_pauses_only_the_events_read() {
    let harness = Harness::new("gh-read-events-limited", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let reset = format!("X-RateLimit-Reset: {}", NOW + 300);
    let (url, server) = serve_sequence(&[
        ("200 OK", &[], REPLY),
        (
            "403 Forbidden",
            &["X-RateLimit-Remaining: 0", &reset],
            r#"{"message":"API rate limit exceeded"}"#,
        ),
    ]);
    let (first, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW, false);
    assert_eq!(server.join().unwrap().len(), 2);
    assert_eq!(first.status, GithubStatus::Ok);
    assert_eq!(
        first.warnings,
        vec![
            "Your recent pushes were not read. GitHub rate limit reached — your events are read again in 5 min."
                .to_string()
        ]
    );

    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    let (paused, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW + 60, true);
    assert_eq!(server.join().unwrap().len(), 1, "only the search is asked");
    assert_eq!(paused.status, GithubStatus::Ok);
    assert!(!paused.stale);
    assert_eq!(
        paused.warnings,
        vec![
            "Your recent pushes were not read. GitHub rate limit reached — your events are read again in 4 min."
                .to_string()
        ]
    );

    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY), ("200 OK", &[], "[]")]);
    let (resumed, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW + 300, true);
    assert_eq!(server.join().unwrap().len(), 2);
    assert_eq!(resumed.warnings, Vec::<String>::new());
}

#[test]
fn a_viewer_login_github_would_never_issue_is_not_put_in_the_events_path() {
    let harness = Harness::new("gh-read-contributed-login", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let mut reply: Value = serde_json::from_str(REPLY).unwrap();
    reply["data"]["viewer"]["login"] = json!("../octocat");
    let reply = reply.to_string();
    let (url, server) = serve_sequence(&[("200 OK", &[], &reply)]);

    let (dto, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW, false);

    assert_eq!(server.join().unwrap().len(), 1);
    assert_eq!(dto.status, GithubStatus::Ok);
    assert_eq!(dto.warnings, Vec::<String>::new());
    assert_eq!(mine(&dto), vec![("PR_mine1", "octocat", false)]);
}
