use super::*;
use crate::cli::AgentCli;
use crate::cli_stub::CliStub;
use crate::dto::GithubStatus;
use crate::http::{refused_url, serve_sequence, CapturedRequest};
use crate::paths::{github_prs_path_for, scratch_dir};
use crate::settings::AppSettings;
use fixtures::REPLY;
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

fn gh(dir: &Path, token: &str) -> AgentCli {
    CliStub::new("gh")
        .log_args("args.txt", true)
        .stdout(token)
        .cli(dir)
}

fn gh_runs(dir: &Path) -> usize {
    fs::read_to_string(dir.join("args.txt"))
        .map(|log| log.lines().count())
        .unwrap_or(0)
}

fn missing_gh(dir: &Path) -> AgentCli {
    AgentCli::new(dir.join("gh").to_string_lossy().as_ref())
}

struct Harness {
    home: PathBuf,
    settings: AppSettings,
    token_memo: GhTokenMemo,
    read_memo: ReadMemo,
}

impl Harness {
    fn new(prefix: &str, scopes: &[&str], poll_seconds: u16) -> Self {
        Self {
            home: scratch_dir(prefix),
            settings: settings(scopes, poll_seconds),
            token_memo: GhTokenMemo::new(),
            read_memo: ReadMemo::new(),
        }
    }

    fn read(&self, cli: &AgentCli, url: &str, now_secs: i64, force: bool) -> GithubPrsDto {
        self.read_revisioned(cli, url, now_secs, force).0
    }

    fn read_revisioned(
        &self,
        cli: &AgentCli,
        url: &str,
        now_secs: i64,
        force: bool,
    ) -> (GithubPrsDto, Reading) {
        self.read_with_events(cli, url, &refused_url(), now_secs, force)
    }

    fn read_with_events(
        &self,
        cli: &AgentCli,
        url: &str,
        events_url: &str,
        now_secs: i64,
        force: bool,
    ) -> (GithubPrsDto, Reading) {
        read_prs_in(
            &Sources {
                home: &self.home,
                settings: &self.settings,
                token_memo: &self.token_memo,
                read_memo: &self.read_memo,
                cli,
                url,
                events_url,
                now_secs,
            },
            force,
        )
    }
}

fn authorization(request: &CapturedRequest) -> String {
    crate::http::head_header(&request.head, "authorization")
        .unwrap_or_default()
        .to_string()
}

fn reply_with_budget(remaining: u64, reset_epoch_secs: i64) -> String {
    reply_with_budget_text(remaining, &rfc3339(reset_epoch_secs).unwrap())
}

fn reply_with_budget_text(remaining: u64, reset_at: &str) -> String {
    let mut value: Value = serde_json::from_str(REPLY).unwrap();
    value["data"]["rateLimit"] = json!({ "remaining": remaining, "resetAt": reset_at });
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

    let dto = harness.read(&gh, &url, NOW, false);

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
        snapshot::load(&github_prs_path_for(&harness.home)),
        Some(dto.data)
    );
}

#[test]
fn the_memory_window_ends_five_seconds_before_the_poll_interval() {
    let harness = Harness::new("gh-read-cache-edge", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    let first = harness.read(&gh, &url, NOW, false);
    server.join().unwrap();

    assert_eq!(harness.read(&gh, &refused_url(), NOW + 54, false), first);
    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    assert_eq!(
        harness.read(&gh, &url, NOW + 55, false).status,
        GithubStatus::Ok
    );
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn a_fresh_read_is_served_from_memory_until_the_poll_interval_nears() {
    let harness = Harness::new("gh-read-cache", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    let first = harness.read(&gh, &url, NOW, false);
    server.join().unwrap();

    let cached = harness.read(&gh, &refused_url(), NOW + 30, false);
    assert_eq!(cached, first, "inside the window nothing is fetched");

    let expired = harness.read(&gh, &refused_url(), NOW + 56, false);
    assert_eq!(expired.status, GithubStatus::Network);
    assert!(expired.stale, "the snapshot backs the failed refresh");
    assert_eq!(expired.data.mine.items, first.data.mine.items);
    assert_eq!(expired.data.fetched_at, first.data.fetched_at);
}

#[test]
fn only_a_read_that_replaces_the_remembered_result_is_announced() {
    let harness = Harness::new("gh-read-revision", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    let (_, first) = harness.read_revisioned(&gh, &url, NOW, false);
    server.join().unwrap();
    assert_eq!(
        first,
        Reading::Replaced(1),
        "the first read replaces nothing-yet"
    );

    let (_, memoised) = harness.read_revisioned(&gh, &refused_url(), NOW + 30, false);
    assert_eq!(
        memoised,
        Reading::Unchanged(1),
        "the read a consumer makes in answer to an announcement is memory-served and announces \
         nothing, which is what ends the exchange"
    );

    let (_, failed) = harness.read_revisioned(&gh, &refused_url(), NOW + 1, true);
    assert_eq!(
        failed,
        Reading::Unchanged(1),
        "a failure discards the remembered result rather than replacing it with a newer one; \
         announcing would send every consumer back to GitHub for the request that just failed"
    );

    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    let (_, refreshed) = harness.read_revisioned(&gh, &url, NOW + 2, true);
    server.join().unwrap();
    assert_eq!(
        refreshed,
        Reading::Replaced(2),
        "the Pull requests screen's refresh replaced the result, so consumers re-read at once"
    );
}

#[test]
fn a_failed_refresh_does_not_leave_the_previous_result_live() {
    let harness = Harness::new("gh-read-failed-refresh", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    harness.read(&gh, &url, NOW, false);
    server.join().unwrap();

    let failed = harness.read(&gh, &refused_url(), NOW + 1, true);
    assert_eq!(failed.status, GithubStatus::Network);
    assert!(failed.stale);

    let next_poll = harness.read(&gh, &refused_url(), NOW + 2, false);
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
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let body = reply_with_budget(3, NOW + 600);
    let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);
    let first = harness.read(&gh, &url, NOW, false);
    server.join().unwrap();
    fs::remove_file(github_prs_path_for(&harness.home)).unwrap();

    let paused = harness.read(&gh, &refused_url(), NOW + 56, false);
    assert_eq!(paused.status, GithubStatus::RateLimited);
    assert!(paused.stale);
    assert_eq!(paused.data.mine.items, first.data.mine.items);
}

#[test]
fn force_skips_the_memory_but_not_the_token_memo() {
    let harness = Harness::new("gh-read-force", &[], 60);
    let cli_dir = harness.home.join("cli");
    let gh = gh(&cli_dir, "gho_t");
    let (url_a, server_a) = serve_sequence(&[("200 OK", &[], REPLY)]);
    harness.read(&gh, &url_a, NOW, false);
    server_a.join().unwrap();

    let (url_b, server_b) = serve_sequence(&[("200 OK", &[], REPLY)]);
    let forced = harness.read(&gh, &url_b, NOW + 1, true);

    assert_eq!(forced.status, GithubStatus::Ok);
    assert_eq!(server_b.join().unwrap().len(), 1);
    assert_eq!(gh_runs(&cli_dir), 1, "`gh auth token` ran once per app run");
}

#[test]
fn a_missing_gh_shows_the_snapshot_as_stale() {
    let harness = Harness::new("gh-read-missing", &["org:acme"], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    let first = harness.read(&gh, &url, NOW, false);
    server.join().unwrap();

    let later = Harness {
        home: harness.home.clone(),
        settings: settings(&["org:other"], 60),
        token_memo: GhTokenMemo::new(),
        read_memo: ReadMemo::new(),
    };
    let dto = later.read(
        &missing_gh(&harness.home.join("nowhere")),
        &url,
        NOW + 120,
        false,
    );

    assert_eq!(dto.status, GithubStatus::GhMissing);
    assert!(dto.stale);
    assert!(
        dto.hint.as_deref().unwrap().contains("gh auth login"),
        "{:?}",
        dto.hint
    );
    assert_eq!(dto.data.mine.items, first.data.mine.items);
    assert_eq!(dto.data.fetched_at, first.data.fetched_at);
    assert_eq!(dto.data.viewer, first.data.viewer);
    assert_eq!(
        dto.data.scope, first.data.scope,
        "the lists reflect the scope they were read with"
    );
}

#[test]
fn a_signed_out_gh_says_so_without_leaking_its_stderr() {
    let harness = Harness::new("gh-read-signed-out", &[], 60);
    let gh = CliStub::new("gh")
        .stderr("no oauth token found: secret-path")
        .exit(1)
        .cli(&harness.home.join("cli"));

    let dto = harness.read(&gh, &refused_url(), NOW, false);

    assert_eq!(dto.status, GithubStatus::GhNotLoggedIn);
    assert!(!dto.stale);
    assert!(dto.data.mine.items.is_empty());
    assert!(
        dto.hint.as_deref().unwrap().contains("gh auth login"),
        "{:?}",
        dto.hint
    );
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("secret-path"), "{json}");
}

#[test]
fn a_rejected_token_is_re_read_from_gh_once() {
    let harness = Harness::new("gh-read-401-retry", &[], 60);
    let stale_gh = gh(&harness.home.join("stale"), "gho_stale");
    let (url_a, server_a) = serve_sequence(&[("200 OK", &[], REPLY)]);
    harness.read(&stale_gh, &url_a, NOW, false);
    server_a.join().unwrap();

    let fresh_dir = harness.home.join("fresh");
    let fresh_gh = gh(&fresh_dir, "gho_fresh");
    let (url_b, server_b) = serve_sequence(&[
        ("401 Unauthorized", &[], r#"{"message":"Bad credentials"}"#),
        ("200 OK", &[], REPLY),
    ]);
    let dto = harness.read(&fresh_gh, &url_b, NOW + 1, true);

    assert_eq!(dto.status, GithubStatus::Ok);
    let requests = server_b.join().unwrap();
    assert_eq!(authorization(&requests[0]), "Bearer gho_stale");
    assert_eq!(authorization(&requests[1]), "Bearer gho_fresh");
    assert_eq!(gh_runs(&fresh_dir), 1);
}

#[test]
fn a_token_rejected_twice_is_reported_and_forgotten() {
    let harness = Harness::new("gh-read-401-twice", &[], 60);
    let cli_dir = harness.home.join("cli");
    let gh = gh(&cli_dir, "gho_x");
    let (url, server) = serve_sequence(&[
        ("401 Unauthorized", &[], "{}"),
        ("401 Unauthorized", &[], "{}"),
    ]);

    let dto = harness.read(&gh, &url, NOW, false);

    assert_eq!(dto.status, GithubStatus::TokenRejected);
    assert_no_token(&dto, &harness.home, "gho_x");
    assert!(
        dto.hint.as_deref().unwrap().contains("gh auth login"),
        "{:?}",
        dto.hint
    );
    assert_eq!(server.join().unwrap().len(), 2);
    assert_eq!(gh_runs(&cli_dir), 2);
    assert_eq!(
        harness
            .token_memo
            .lookup(|| Err(auth::TokenError::NotLoggedIn)),
        Err(auth::TokenError::NotLoggedIn),
        "the rejected token is not kept"
    );
}

#[test]
fn an_exhausted_rate_limit_pauses_polling_until_it_resets() {
    let harness = Harness::new("gh-read-rate-limited", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let reset = (NOW + 300).to_string();
    let (url, server) = serve_sequence(&[(
        "403 Forbidden",
        &[
            "X-RateLimit-Remaining: 0",
            &format!("X-RateLimit-Reset: {reset}"),
        ],
        r#"{"message":"API rate limit exceeded"}"#,
    )]);

    let dto = harness.read(&gh, &url, NOW, false);
    server.join().unwrap();
    assert_eq!(dto.status, GithubStatus::RateLimited);
    assert!(
        dto.hint.as_deref().unwrap().contains("5 min"),
        "{:?}",
        dto.hint
    );

    let paused = harness.read(&gh, &refused_url(), NOW + 10, true);
    assert_eq!(
        paused.status,
        GithubStatus::RateLimited,
        "no request while paused"
    );
    assert!(
        paused.hint.as_deref().unwrap().contains("5 min"),
        "{:?}",
        paused.hint
    );

    assert_eq!(
        harness.read(&gh, &refused_url(), NOW + 299, true).status,
        GithubStatus::RateLimited,
        "still paused one second before the reset"
    );
    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    let resumed = harness.read(&gh, &url, NOW + 300, false);
    assert_eq!(resumed.status, GithubStatus::Ok);
    server.join().unwrap();
}

#[test]
fn rate_limit_replies_without_a_usable_reset_pause_for_one_minute() {
    let past_reset = (NOW - 10).to_string();
    let cases: [(&str, Vec<String>); 2] = [
        ("429 Too Many Requests", vec![]),
        (
            "403 Forbidden",
            vec![
                "X-RateLimit-Remaining: 0".to_string(),
                format!("X-RateLimit-Reset: {past_reset}"),
            ],
        ),
    ];
    for (index, (status_line, headers)) in cases.iter().enumerate() {
        let harness = Harness::new(&format!("gh-read-fallback-{index}"), &[], 60);
        let gh = gh(&harness.home.join("cli"), "gho_t");
        let headers: Vec<&str> = headers.iter().map(String::as_str).collect();
        let (url, server) = serve_sequence(&[(status_line, &headers, "{}")]);

        let dto = harness.read(&gh, &url, NOW, false);
        server.join().unwrap();
        assert_eq!(dto.status, GithubStatus::RateLimited, "{status_line}");
        assert!(
            dto.hint.as_deref().unwrap().contains("1 min"),
            "{:?}",
            dto.hint
        );
        assert_eq!(
            harness.read(&gh, &refused_url(), NOW + 30, true).status,
            GithubStatus::RateLimited,
            "{status_line}: paused for a minute"
        );
        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
        assert_eq!(
            harness.read(&gh, &url, NOW + 60, false).status,
            GithubStatus::Ok,
            "{status_line}: resumes after a minute"
        );
        server.join().unwrap();
    }
}

#[test]
fn a_nearly_spent_budget_pauses_polling_too() {
    let harness = Harness::new("gh-read-low-budget", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let body = reply_with_budget(10, NOW + 120);
    let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);

    let first = harness.read(&gh, &url, NOW, false);
    server.join().unwrap();
    assert_eq!(first.status, GithubStatus::Ok);

    let paused = harness.read(&gh, &refused_url(), NOW + 5, true);
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
fn the_low_budget_pause_starts_below_fifty_points() {
    let fine = Harness::new("gh-read-budget-50", &[], 60);
    let gh_fine = gh(&fine.home.join("cli"), "gho_t");
    let body = reply_with_budget(50, NOW + 600);
    let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);
    assert_eq!(
        fine.read(&gh_fine, &url, NOW, false).status,
        GithubStatus::Ok
    );
    server.join().unwrap();
    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    assert_eq!(
        fine.read(&gh_fine, &url, NOW + 56, false).status,
        GithubStatus::Ok,
        "fifty points left is not a pause"
    );
    assert_eq!(server.join().unwrap().len(), 1);

    let low = Harness::new("gh-read-budget-49", &[], 60);
    let gh_low = gh(&low.home.join("cli"), "gho_t");
    let body = reply_with_budget(49, NOW + 600);
    let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);
    assert_eq!(low.read(&gh_low, &url, NOW, false).status, GithubStatus::Ok);
    server.join().unwrap();
    let paused = low.read(&gh_low, &refused_url(), NOW + 56, false);
    assert_eq!(paused.status, GithubStatus::RateLimited);
    assert!(paused.stale);
    assert!(
        paused.hint.as_deref().unwrap().contains("10 min"),
        "{:?}",
        paused.hint
    );
}

#[test]
fn an_unreadable_reset_instant_with_a_low_budget_pauses_for_one_minute() {
    let harness = Harness::new("gh-read-budget-unreadable", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let body = reply_with_budget_text(3, "soon");
    let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);
    assert_eq!(harness.read(&gh, &url, NOW, false).status, GithubStatus::Ok);
    server.join().unwrap();
    assert_eq!(
        harness.read(&gh, &refused_url(), NOW + 30, true).status,
        GithubStatus::RateLimited
    );
    let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
    assert_eq!(
        harness.read(&gh, &url, NOW + 60, false).status,
        GithubStatus::Ok
    );
    server.join().unwrap();
}

#[test]
fn network_and_unreadable_replies_report_network() {
    let harness = Harness::new("gh-read-network", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");

    let refused = harness.read(&gh, &refused_url(), NOW, true);
    assert_eq!(refused.status, GithubStatus::Network);
    assert!(
        refused
            .hint
            .as_deref()
            .unwrap()
            .contains("Could not reach GitHub"),
        "{:?}",
        refused.hint
    );

    let (url, server) = serve_sequence(&[("200 OK", &[], "<html>")]);
    assert_eq!(
        harness.read(&gh, &url, NOW, true).status,
        GithubStatus::Network
    );
    server.join().unwrap();

    let (url, server) = serve_sequence(&[(
        "200 OK",
        &[],
        r#"{"data":null,"errors":[{"message":"Bad query"}]}"#,
    )]);
    let unreadable = harness.read(&gh, &url, NOW, true);
    server.join().unwrap();
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
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let (url_a, server_a) = serve_sequence(&[("200 OK", &[], REPLY)]);
    harness.read(&gh, &url_a, NOW, false);
    server_a.join().unwrap();

    harness.settings = settings(&[], 30);
    let (url_b, server_b) = serve_sequence(&[("200 OK", &[], REPLY)]);
    let dto = harness.read(&gh, &url_b, NOW + 40, false);

    assert_eq!(dto.status, GithubStatus::Ok);
    assert_eq!(
        server_b.join().unwrap().len(),
        1,
        "a shorter interval shrinks the window"
    );
}

#[test]
fn a_saved_pull_request_interval_replaces_the_remembered_one() {
    set_poll_seconds(120);
    assert_eq!(POLL_SECONDS.load(Ordering::Acquire), 120);
}

fn pushed_to_shared_branch() -> String {
    json!([{
        "id": "10", "type": "PushEvent", "repo": { "name": "acme/app" },
        "payload": { "ref": "refs/heads/fix/shared", "head": "head1", "before": "head0" },
        "created_at": rfc3339(NOW - 60).unwrap(),
    }])
    .to_string()
}

fn shared_branch_lookup() -> String {
    fixtures::lookup_reply(
        json!([]),
        &[json!([fixtures::contribution_node(
            "PR_shared",
            "alice",
            "acme/app",
            "OPEN"
        )])],
    )
    .to_string()
}

fn your_commit() -> String {
    fixtures::commits_reply(&[vec![fixtures::commit(1, &["octocat"])]]).to_string()
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
        pushed_to_shared_branch(),
        shared_branch_lookup(),
        your_commit(),
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
        pushed_to_shared_branch(),
        shared_branch_lookup(),
        your_commit(),
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
        snapshot::load(&github_prs_path_for(&harness.home)),
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
