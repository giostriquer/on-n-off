use super::*;
use crate::dto::{CiState, MergeKind};
use crate::github_monitor::{EventKind, Watch};
use crate::side_notch::model::{frame, GithubList, NotchSettings};

fn reply(mergeable: &str, state: &str) -> Value {
    let mut value: Value = serde_json::from_str(REPLY).unwrap();
    let mut pr = value["data"]["mine"]["nodes"][0].clone();
    pr["headRefOid"] = json!("head1");
    pr["baseRefName"] = json!("dev");
    pr["mergeable"] = json!(mergeable);
    pr["mergeStateStatus"] = json!(state);
    pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"] = json!("SUCCESS");
    for (index, list) in ["mine", "review", "assigned"].into_iter().enumerate() {
        pr["id"] = json!(format!("PR_{list}"));
        pr["number"] = json!(41 + index);
        pr["url"] = json!(format!("https://github.com/acme/app/pull/{}", 41 + index));
        value["data"][list] = json!({"issueCount": 1, "nodes": [pr.clone()]});
    }
    value
}

fn read(harness: &Harness, cli: &AgentCli, reply: &Value, now: i64) -> GithubPrsDto {
    let body = reply.to_string();
    let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);
    let dto = harness.read(cli, &url, now, true);
    assert_eq!(server.join().unwrap().len(), 1);
    assert_eq!(dto.status, GithubStatus::Ok);
    dto
}

fn assert_conflicts(dto: &GithubPrsDto, expected: bool) {
    assert_eq!(dto.data.open_prs().count(), 3);
    for pr in dto.data.open_prs() {
        assert_eq!(pr.merge_kind == Some(MergeKind::Conflicts), expected);
    }
    let mut settings = NotchSettings::default();
    settings.pull_requests.lists = vec![
        GithubList::Mine,
        GithubList::ReviewRequested,
        GithubList::Assigned,
    ];
    let rail = frame::rail(&settings, [], Some(dto), None);
    let cell = rail
        .cells
        .iter()
        .find_map(|cell| match cell {
            frame::CellData::PullRequests(cell) => cell.reading.as_ref(),
            _ => None,
        })
        .unwrap();
    assert_eq!(cell.segments.len(), 3);
    for segment in &cell.segments {
        assert_eq!(
            segment.passing_with_conflicts,
            expected && segment.ci == CiState::Success
        );
    }
    assert_eq!(cell.attention.conflicts, if expected { 3 } else { 0 });
    for list in &cell.lists {
        assert_eq!(
            list.items[0]
                .badges
                .iter()
                .any(|badge| badge.label == "Conflicts"),
            expected
        );
    }
}

#[test]
fn unknown_refreshes_keep_the_conflict_badge_and_ring_until_github_resolves_them() {
    let harness = Harness::new("gh-conflict-refresh", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let first = read(&harness, &gh, &reply("CONFLICTING", "DIRTY"), NOW);
    assert_conflicts(&first, true);

    for step in 1..=2 {
        let dto = read(&harness, &gh, &reply("UNKNOWN", "UNKNOWN"), NOW + step * 60);
        assert_conflicts(&dto, true);
        for pr in dto.data.open_prs() {
            assert_eq!(pr.mergeable, crate::dto::Mergeability::Unknown);
            assert_eq!(pr.merge_state, crate::dto::MergeState::Unknown);
        }
        assert_eq!(dto.data.fetched_at, rfc3339(NOW + step * 60));
        assert!(!dto.stale);
        assert_eq!(
            snapshot::load(&github_prs_path_for(&harness.home)).map(|snapshot| snapshot.data),
            Some(dto.data)
        );
    }

    let still_conflicting = read(&harness, &gh, &reply("CONFLICTING", "DIRTY"), NOW + 180);
    assert_conflicts(&still_conflicting, true);
    let resolved = read(&harness, &gh, &reply("MERGEABLE", "CLEAN"), NOW + 240);
    assert_conflicts(&resolved, false);
    assert_eq!(
        resolved.data.mine.items[0].merge_kind,
        Some(MergeKind::Ready)
    );
    let unknown = read(&harness, &gh, &reply("UNKNOWN", "UNKNOWN"), NOW + 300);
    assert_conflicts(&unknown, false);
    assert_eq!(unknown.data.mine.items[0].merge_kind, None);
}

#[test]
fn unknown_refresh_after_restart_or_failure_keeps_the_saved_conflict_and_fresh_fields() {
    let mut harness = Harness::new("gh-conflict-restart", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    read(&harness, &gh, &reply("UNKNOWN", "DIRTY"), NOW);
    harness.read_memo = ReadMemo::new();
    let mut unknown = reply("UNKNOWN", "BLOCKED");
    for list in ["mine", "review", "assigned"] {
        unknown["data"][list]["nodes"][0]["title"] = json!("New title");
        unknown["data"][list]["nodes"][0]["commits"]["nodes"][0]["commit"]["statusCheckRollup"]
            ["state"] = json!("PENDING");
        unknown["data"][list]["nodes"][0]["updatedAt"] = json!("2026-08-24T21:00:00Z");
    }
    assert_conflicts(&read(&harness, &gh, &unknown, NOW + 60), true);
    let failed = harness.read(&gh, &refused_url(), NOW + 120, true);
    assert!(failed.stale);
    let dto = read(&harness, &gh, &unknown, NOW + 180);
    assert_conflicts(&dto, true);
    assert_eq!(dto.data.mine.items[0].title, "New title");
    assert_eq!(dto.data.mine.items[0].ci, CiState::Pending);
}

#[test]
fn expanding_scope_keeps_a_contributions_known_conflict_when_other_lists_are_unknown() {
    let mut harness = Harness::new("gh-conflict-scope", &["org:other"], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let mut search: Value = serde_json::from_str(REPLY).unwrap();
    search["data"]["mine"] = json!({"issueCount":0,"nodes":[]});
    let mut node = fixtures::contribution_node("PR_shared", "alice", "acme/app", "OPEN");
    node["mergeable"] = json!("UNKNOWN");
    node["mergeStateStatus"] = json!("UNKNOWN");
    search["data"]["review"] = json!({"issueCount":1,"nodes":[node.clone()]});
    let search_body = search.to_string();
    let mut conflicting = node.clone();
    conflicting["mergeable"] = json!("CONFLICTING");
    conflicting["mergeStateStatus"] = json!("DIRTY");
    let events = pushed_to_shared_branch();
    let lookup = fixtures::lookup_reply(json!([]), &[json!([conflicting])]).to_string();
    let commits = your_commit();
    let (url, server) = serve_sequence(&[
        ("200 OK", &[], &search_body),
        ("200 OK", &[], &events),
        ("200 OK", &[], &lookup),
        ("200 OK", &[], &commits),
    ]);
    let (first, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW, true);
    server.join().unwrap();
    assert!(first.data.mine.items.is_empty());
    assert_eq!(
        first.data.review_requested.items[0].merge_kind,
        Some(MergeKind::Conflicts)
    );

    harness.settings.github_scopes.clear();
    for step in 1..=2 {
        if step == 2 {
            harness.read_memo = ReadMemo::new();
        }
        let lookup = fixtures::lookup_reply(json!([node.clone()]), &[json!([])]).to_string();
        let (url, server) =
            serve_sequence(&[("200 OK", &[], &search_body), ("200 OK", &[], &lookup)]);
        let (dto, _) =
            harness.read_with_events(&gh, &url, &events_url(&url), NOW + step * 10, true);
        server.join().unwrap();
        assert_eq!(dto.data.open_prs().count(), 2);
        for pr in dto.data.open_prs() {
            assert_eq!(pr.merge_kind, Some(MergeKind::Conflicts), "step {step}");
        }
        let mut settings = NotchSettings::default();
        settings.pull_requests.lists = vec![GithubList::Mine, GithubList::ReviewRequested];
        let rail = frame::rail(&settings, [], Some(&dto), None);
        let reading = rail
            .cells
            .iter()
            .find_map(|cell| match cell {
                frame::CellData::PullRequests(cell) => cell.reading.as_ref(),
                _ => None,
            })
            .unwrap();
        assert_eq!(reading.attention.conflicts, 1);
        assert_eq!(reading.segments.len(), 1);
        for list in &reading.lists {
            assert!(list.items[0]
                .badges
                .iter()
                .any(|badge| badge.label == "Conflicts"));
        }
    }
}

#[test]
fn a_known_conflict_outlives_a_restart_while_no_read_lists_it() {
    let mut harness = Harness::new("gh-conflict-unlisted-restart", &["org:other"], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let mut search: Value = serde_json::from_str(REPLY).unwrap();
    search["data"]["mine"] = json!({"issueCount":0,"nodes":[]});
    let search = search.to_string();
    let mut node = fixtures::contribution_node("PR_shared", "alice", "acme/app", "OPEN");
    node["mergeable"] = json!("CONFLICTING");
    node["mergeStateStatus"] = json!("DIRTY");
    let events = pushed_to_shared_branch();
    let lookup = fixtures::lookup_reply(json!([]), &[json!([node.clone()])]).to_string();
    let commits = your_commit();
    let (url, server) = serve_sequence(&[
        ("200 OK", &[], &search),
        ("200 OK", &[], &events),
        ("200 OK", &[], &lookup),
        ("200 OK", &[], &commits),
    ]);
    let (first, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW, true);
    server.join().unwrap();
    assert!(first.data.mine.items.is_empty());

    node["mergeable"] = json!("UNKNOWN");
    node["mergeStateStatus"] = json!("UNKNOWN");
    let lookup = fixtures::lookup_reply(json!([node]), &[json!([])]).to_string();
    for step in 1..=2 {
        if step == 2 {
            harness.read_memo = ReadMemo::new();
            harness.settings.github_scopes.clear();
        }
        let (url, server) = serve_sequence(&[("200 OK", &[], &search), ("200 OK", &[], &lookup)]);
        let (dto, _) =
            harness.read_with_events(&gh, &url, &events_url(&url), NOW + step * 10, true);
        server.join().unwrap();
        let mine: Vec<_> = dto
            .data
            .mine
            .items
            .iter()
            .map(|pr| (pr.id.as_str(), pr.merge_kind))
            .collect();
        let expected = match step {
            1 => Vec::new(),
            _ => vec![("PR_shared", Some(MergeKind::Conflicts))],
        };
        assert_eq!(mine, expected, "step {step}");
    }
}

fn legacy_row(id: &str, base: &str, contributed: bool) -> String {
    format!(
        r#"{{"id":"{id}","number":12,"title":"Shared work","url":"https://github.com/acme/app/pull/12","repo":"acme/app","author":"alice","isDraft":false,"ci":"success","headRef":"fix/x","headRefOid":"head1","baseRef":"{base}","updatedAt":"2026-08-24T20:00:00Z","mergeable":"unknown","mergeState":"unknown","autoMerge":false,"mergeKind":"conflicts"{}}}"#,
        if contributed {
            r#","contributed":true"#
        } else {
            ""
        }
    )
}

fn write_files_saved_before_the_conflict_memory(home: &Path) {
    let prs = github_prs_path_for(home);
    fs::create_dir_all(prs.parent().unwrap()).unwrap();
    fs::write(
        &prs,
        format!(
            r#"{{"schemaVersion":1,"data":{{"viewer":"octocat","fetchedAt":"2026-08-24T20:00:00Z","scope":[],"mine":{{"total":1,"items":[{}]}},"reviewRequested":{{"total":0,"items":[]}},"assigned":{{"total":0,"items":[]}},"merged":{{"total":0,"items":[]}}}}}}"#,
            legacy_row("PR_mine", "dev", false)
        ),
    )
    .unwrap();
    fs::write(
        paths::github_contributions_path_for(home),
        format!(
            r#"{{"schemaVersion":1,"viewer":"octocat","eventsReadAt":{NOW},"branches":[],"tracked":[{{"id":"PR_shared","repo":"acme/app","branch":"fix/x","verified":true,"head":"head1","checkedHead":"head1","last":{}}}]}}"#,
            legacy_row("PR_shared", "main", true)
        ),
    )
    .unwrap();
}

fn read_unknown_with_contribution(harness: &Harness, gh: &AgentCli, now: i64) -> GithubPrsDto {
    let search = reply("UNKNOWN", "UNKNOWN").to_string();
    let mut node = fixtures::contribution_node("PR_shared", "alice", "acme/app", "OPEN");
    node["mergeable"] = json!("UNKNOWN");
    node["mergeStateStatus"] = json!("UNKNOWN");
    let lookup = fixtures::lookup_reply(json!([node]), &[]).to_string();
    let (url, server) = serve_sequence(&[("200 OK", &[], &search), ("200 OK", &[], &lookup)]);
    let (dto, _) = harness.read_with_events(gh, &url, &refused_url(), now, true);
    assert_eq!(server.join().unwrap().len(), 2);
    assert_eq!(dto.status, GithubStatus::Ok);
    assert_eq!(dto.warnings, Vec::<String>::new());
    dto
}

#[test]
fn conflicts_retained_in_files_saved_before_the_memory_hold_through_unknown_reads() {
    let mut harness = Harness::new("gh-conflict-legacy-files", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    write_files_saved_before_the_conflict_memory(&harness.home);

    let failed = harness.read(&gh, &refused_url(), NOW, true);
    assert!(failed.stale);
    assert_eq!(
        failed.data.mine.items[0].merge_kind,
        Some(MergeKind::Conflicts)
    );

    for step in 1..=2 {
        if step == 2 {
            harness.read_memo = ReadMemo::new();
        }
        let dto = read_unknown_with_contribution(&harness, &gh, NOW + step * 20);
        let kinds: Vec<_> = dto
            .data
            .open_prs()
            .map(|pr| (pr.id.as_str(), pr.merge_kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("PR_mine", Some(MergeKind::Conflicts)),
                ("PR_shared", Some(MergeKind::Conflicts)),
                ("PR_review", None),
                ("PR_assigned", None),
            ],
            "step {step}"
        );
    }
}

fn shared_rows(dto: &GithubPrsDto) -> Vec<(Option<MergeKind>, Option<bool>)> {
    dto.data
        .open_prs()
        .filter(|pr| pr.id == "PR_shared")
        .map(|pr| (pr.merge_kind, merge::facts(pr).conflicts))
        .collect()
}

#[test]
fn a_contributions_earlier_answer_never_beats_this_reads_answer_for_its_revision() {
    let conflicts = (Some(MergeKind::Conflicts), Some(true));
    let ready = (Some(MergeKind::Ready), Some(false));
    let carried = (Some(MergeKind::Conflicts), None);
    let nothing = (None, None);
    for (earlier, now, then) in [
        (("MERGEABLE", "CLEAN"), ("CONFLICTING", "DIRTY"), carried),
        (("CONFLICTING", "DIRTY"), ("MERGEABLE", "CLEAN"), nothing),
    ] {
        let harness = Harness::new("gh-conflict-earlier-contribution", &[], 60);
        let gh = gh(&harness.home.join("cli"), "gho_t");
        let mut node = fixtures::contribution_node("PR_shared", "alice", "acme/app", "OPEN");
        node["mergeable"] = json!(earlier.0);
        node["mergeStateStatus"] = json!(earlier.1);
        let events = pushed_to_shared_branch();
        let lookup = fixtures::lookup_reply(json!([]), &[json!([node.clone()])]).to_string();
        let commits = your_commit();
        let (url, server) = serve_sequence(&[
            ("200 OK", &[], REPLY),
            ("200 OK", &[], &events),
            ("200 OK", &[], &lookup),
            ("200 OK", &[], &commits),
        ]);
        harness.read_with_events(&gh, &url, &events_url(&url), NOW, true);
        server.join().unwrap();

        let mut low: Value = serde_json::from_str(REPLY).unwrap();
        low["data"]["rateLimit"] = json!({ "remaining": 10, "resetAt": rfc3339(NOW + 15) });
        let mut answered = node.clone();
        answered["mergeable"] = json!(now.0);
        answered["mergeStateStatus"] = json!(now.1);
        low["data"]["review"] = json!({"issueCount": 1, "nodes": [answered]});
        let low = low.to_string();
        let (url, server) = serve_sequence(&[("200 OK", &[], &low)]);
        let (dto, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW + 10, true);
        assert_eq!(server.join().unwrap().len(), 1);
        let expected = if now.0 == "CONFLICTING" {
            conflicts
        } else {
            ready
        };
        assert_eq!(
            shared_rows(&dto),
            vec![expected, expected],
            "{earlier:?} then {now:?}"
        );

        node["mergeable"] = json!("UNKNOWN");
        node["mergeStateStatus"] = json!("UNKNOWN");
        let mut search: Value = serde_json::from_str(REPLY).unwrap();
        search["data"]["review"] = json!({"issueCount": 1, "nodes": [node.clone()]});
        let search = search.to_string();
        let lookup = fixtures::lookup_reply(json!([node]), &[json!([])]).to_string();
        let (url, server) = serve_sequence(&[("200 OK", &[], &search), ("200 OK", &[], &lookup)]);
        let (dto, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW + 20, true);
        assert_eq!(server.join().unwrap().len(), 2);
        assert_eq!(
            shared_rows(&dto),
            vec![then, then],
            "{earlier:?} then {now:?}, unknown"
        );
    }
}

type Answer = (&'static str, &'static str);

const CLEAN: Answer = ("MERGEABLE", "CLEAN");
const CONFLICTING: Answer = ("CONFLICTING", "DIRTY");
const UNKNOWN: Answer = ("UNKNOWN", "UNKNOWN");

fn shared(answer: Answer) -> Value {
    let mut node = fixtures::contribution_node("PR_shared", "alice", "acme/app", "OPEN");
    node["mergeable"] = json!(answer.0);
    node["mergeStateStatus"] = json!(answer.1);
    node
}

fn search_with_review(answer: Answer) -> String {
    let mut search: Value = serde_json::from_str(REPLY).unwrap();
    search["data"]["mine"] = json!({"issueCount": 0, "nodes": []});
    search["data"]["review"] = json!({"issueCount": 1, "nodes": [shared(answer)]});
    search.to_string()
}

fn watched_reads(
    first: Answer,
    then: &[(Answer, Option<Answer>)],
) -> Vec<(Vec<Option<MergeKind>>, Vec<EventKind>)> {
    let harness = Harness::new("gh-conflict-failing-lookups", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let mut watch = Watch::new();
    let search = search_with_review(first);
    let events = pushed_to_shared_branch();
    let lookup = fixtures::lookup_reply(json!([]), &[json!([shared(first)])]).to_string();
    let commits = your_commit();
    let (url, server) = serve_sequence(&[
        ("200 OK", &[], &search),
        ("200 OK", &[], &events),
        ("200 OK", &[], &lookup),
        ("200 OK", &[], &commits),
    ]);
    let (dto, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW, true);
    server.join().unwrap();
    let mut reads = vec![dto];
    let failure = json!({"errors": [{"message": "Something went wrong."}]}).to_string();
    for (step, (review, lookup)) in (1..).zip(then) {
        let search = search_with_review(*review);
        let lookup = lookup.map_or(failure.clone(), |answer| {
            fixtures::lookup_reply(json!([shared(answer)]), &[json!([])]).to_string()
        });
        let (url, server) = serve_sequence(&[("200 OK", &[], &search), ("200 OK", &[], &lookup)]);
        let (dto, _) =
            harness.read_with_events(&gh, &url, &events_url(&url), NOW + step * 10, true);
        assert_eq!(server.join().unwrap().len(), 2);
        reads.push(dto);
    }
    reads
        .iter()
        .map(|dto| {
            let kinds = dto
                .data
                .open_prs()
                .filter(|pr| pr.id == "PR_shared")
                .map(|pr| pr.merge_kind)
                .collect();
            (kinds, watch.observe(dto))
        })
        .collect()
}

#[test]
fn failing_contribution_lookups_never_bring_back_an_answer_older_than_the_last_one_shown() {
    let conflicts = Some(MergeKind::Conflicts);
    let ready = Some(MergeKind::Ready);
    let announced = |kinds: &[EventKind]| kinds.to_vec();
    let both = |kind| vec![kind, kind];
    assert_eq!(
        watched_reads(
            CLEAN,
            &[
                (CONFLICTING, None),
                (UNKNOWN, None),
                (UNKNOWN, Some(UNKNOWN)),
                (CONFLICTING, Some(CONFLICTING)),
            ],
        ),
        vec![
            (both(ready), announced(&[])),
            (both(conflicts), announced(&[EventKind::Conflicts])),
            (both(conflicts), announced(&[])),
            (both(conflicts), announced(&[])),
            (both(conflicts), announced(&[])),
        ],
        "a conflict after a clean answer"
    );
    assert_eq!(
        watched_reads(
            CONFLICTING,
            &[
                (CLEAN, None),
                (UNKNOWN, None),
                (UNKNOWN, Some(UNKNOWN)),
                (CLEAN, Some(CLEAN)),
            ],
        ),
        vec![
            (both(conflicts), announced(&[])),
            (both(ready), announced(&[EventKind::ReadyToMerge])),
            (both(ready), announced(&[])),
            (both(None), announced(&[])),
            (both(ready), announced(&[])),
        ],
        "a clean answer after a conflict"
    );
}

#[test]
fn a_conflict_saved_before_the_memory_never_becomes_a_raw_answer_github_did_not_give() {
    let harness = Harness::new("gh-conflict-legacy-lookup-fails", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    write_files_saved_before_the_conflict_memory(&harness.home);
    let mut watch = Watch::new();
    let failure = json!({"errors": [{"message": "Something went wrong."}]}).to_string();
    let lookup = |answer: Answer| fixtures::lookup_reply(json!([shared(answer)]), &[]).to_string();
    let reads = [
        (reply("UNKNOWN", "UNKNOWN"), failure),
        (reply("UNKNOWN", "UNKNOWN"), lookup(UNKNOWN)),
        (reply("MERGEABLE", "CLEAN"), lookup(CLEAN)),
    ];
    let mut seen = Vec::new();
    for (step, (search, lookup)) in (1..).zip(&reads) {
        let search = search.to_string();
        let (url, server) = serve_sequence(&[("200 OK", &[], &search), ("200 OK", &[], lookup)]);
        let (dto, _) =
            harness.read_with_events(&gh, &url, &events_url(&url), NOW + step * 10, true);
        assert_eq!(server.join().unwrap().len(), 2);
        let shared: Vec<_> = dto
            .data
            .open_prs()
            .filter(|pr| pr.id == "PR_shared")
            .map(|pr| (pr.merge_kind, pr.mergeable, pr.merge_state))
            .collect();
        seen.push((shared, watch.observe(&dto)));
    }
    let carried = (
        Some(MergeKind::Conflicts),
        crate::dto::Mergeability::Unknown,
        crate::dto::MergeState::Unknown,
    );
    let ready = (
        Some(MergeKind::Ready),
        crate::dto::Mergeability::Mergeable,
        crate::dto::MergeState::Clean,
    );
    assert_eq!(
        seen,
        vec![
            (vec![carried], Vec::new()),
            (vec![carried], Vec::new()),
            (vec![ready], Vec::new()),
        ]
    );
}
