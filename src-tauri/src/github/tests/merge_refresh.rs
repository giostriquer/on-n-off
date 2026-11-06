use super::*;
use crate::dto::{CiState, MergeKind};
use crate::github_monitor::{EventKind, Watch};
use crate::side_notch::model::{frame, GithubList, NotchSettings};

fn reply(mergeable: &str, state: &str) -> Value {
    let mut value = super::reply();
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

fn read(harness: &Harness, reply: &Value, now: i64) -> GithubPrsDto {
    let github = answering(reply.clone());
    let dto = harness.read(&github, now, true);
    assert_eq!(github.requests(), 1);
    assert_eq!(dto.status, GithubStatus::Ok);
    dto
}

fn answered(github: &FakeGithub, requests: usize, pages: &[u32]) {
    assert_eq!(github.requests(), requests);
    assert_eq!(github.pages(), pages);
    assert_eq!(github.unanswered(), 0);
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
    let first = read(&harness, &reply("CONFLICTING", "DIRTY"), NOW);
    assert_conflicts(&first, true);

    for step in 1..=2 {
        let dto = read(&harness, &reply("UNKNOWN", "UNKNOWN"), NOW + step * 60);
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

    let still_conflicting = read(&harness, &reply("CONFLICTING", "DIRTY"), NOW + 180);
    assert_conflicts(&still_conflicting, true);
    let resolved = read(&harness, &reply("MERGEABLE", "CLEAN"), NOW + 240);
    assert_conflicts(&resolved, false);
    assert_eq!(
        resolved.data.mine.items[0].merge_kind,
        Some(MergeKind::Ready)
    );
    let unknown = read(&harness, &reply("UNKNOWN", "UNKNOWN"), NOW + 300);
    assert_conflicts(&unknown, false);
    assert_eq!(unknown.data.mine.items[0].merge_kind, None);
}

#[test]
fn unknown_refresh_after_restart_or_failure_keeps_the_saved_conflict_and_fresh_fields() {
    let mut harness = Harness::new("gh-conflict-restart", &[], 60);
    read(&harness, &reply("UNKNOWN", "DIRTY"), NOW);
    harness.read_memo = ReadMemo::new();
    let mut unknown = reply("UNKNOWN", "BLOCKED");
    for list in ["mine", "review", "assigned"] {
        unknown["data"][list]["nodes"][0]["title"] = json!("New title");
        unknown["data"][list]["nodes"][0]["commits"]["nodes"][0]["commit"]["statusCheckRollup"]
            ["state"] = json!("PENDING");
        unknown["data"][list]["nodes"][0]["updatedAt"] = json!("2026-08-24T21:00:00Z");
    }
    assert_conflicts(&read(&harness, &unknown, NOW + 60), true);
    let failed = harness.read(&unreachable(), NOW + 120, true);
    assert!(failed.stale);
    let dto = read(&harness, &unknown, NOW + 180);
    assert_conflicts(&dto, true);
    assert_eq!(dto.data.mine.items[0].title, "New title");
    assert_eq!(dto.data.mine.items[0].ci, CiState::Pending);
}

#[test]
fn expanding_scope_keeps_a_contributions_known_conflict_when_other_lists_are_unknown() {
    let mut harness = Harness::new("gh-conflict-scope", &["org:other"], 60);
    let mut search = super::reply();
    search["data"]["mine"] = json!({"issueCount":0,"nodes":[]});
    let mut node = fixtures::contribution_node("PR_shared", "alice", "acme/app", "OPEN");
    node["mergeable"] = json!("UNKNOWN");
    node["mergeStateStatus"] = json!("UNKNOWN");
    search["data"]["review"] = json!({"issueCount":1,"nodes":[node.clone()]});
    let mut conflicting = node.clone();
    conflicting["mergeable"] = json!("CONFLICTING");
    conflicting["mergeStateStatus"] = json!("DIRTY");
    let github = answering(search.clone())
        .page(pushed_to_shared_branch())
        .reply(fixtures::lookup_reply(json!([]), &[json!([conflicting])]))
        .reply(your_commit());
    let first = harness.read(&github, NOW, true);
    answered(&github, 3, &[1]);
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
        let github = answering(search.clone())
            .reply(fixtures::lookup_reply(json!([node.clone()]), &[json!([])]));
        let dto = harness.read(&github, NOW + step * 10, true);
        answered(&github, 2, &[]);
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
    let mut search = super::reply();
    search["data"]["mine"] = json!({"issueCount":0,"nodes":[]});
    let mut node = fixtures::contribution_node("PR_shared", "alice", "acme/app", "OPEN");
    node["mergeable"] = json!("CONFLICTING");
    node["mergeStateStatus"] = json!("DIRTY");
    let github = answering(search.clone())
        .page(pushed_to_shared_branch())
        .reply(fixtures::lookup_reply(json!([]), &[json!([node.clone()])]))
        .reply(your_commit());
    let first = harness.read(&github, NOW, true);
    answered(&github, 3, &[1]);
    assert!(first.data.mine.items.is_empty());

    node["mergeable"] = json!("UNKNOWN");
    node["mergeStateStatus"] = json!("UNKNOWN");
    let lookup = fixtures::lookup_reply(json!([node]), &[json!([])]);
    for step in 1..=2 {
        if step == 2 {
            harness.read_memo = ReadMemo::new();
            harness.settings.github_scopes.clear();
        }
        let github = answering(search.clone()).reply(lookup.clone());
        let dto = harness.read(&github, NOW + step * 10, true);
        answered(&github, 2, &[]);
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

fn read_unknown_with_contribution(harness: &Harness, now: i64) -> GithubPrsDto {
    let mut node = fixtures::contribution_node("PR_shared", "alice", "acme/app", "OPEN");
    node["mergeable"] = json!("UNKNOWN");
    node["mergeStateStatus"] = json!("UNKNOWN");
    let github =
        answering(reply("UNKNOWN", "UNKNOWN")).reply(fixtures::lookup_reply(json!([node]), &[]));
    let dto = harness.read(&github, now, true);
    answered(&github, 2, &[]);
    assert_eq!(dto.status, GithubStatus::Ok);
    assert_eq!(dto.warnings, Vec::<String>::new());
    dto
}

#[test]
fn conflicts_retained_in_files_saved_before_the_memory_hold_through_unknown_reads() {
    let mut harness = Harness::new("gh-conflict-legacy-files", &[], 60);
    write_files_saved_before_the_conflict_memory(&harness.home);

    let failed = harness.read(&unreachable(), NOW, true);
    assert!(failed.stale);
    assert_eq!(
        failed.data.mine.items[0].merge_kind,
        Some(MergeKind::Conflicts)
    );

    for step in 1..=2 {
        if step == 2 {
            harness.read_memo = ReadMemo::new();
        }
        let dto = read_unknown_with_contribution(&harness, NOW + step * 20);
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
        let mut node = fixtures::contribution_node("PR_shared", "alice", "acme/app", "OPEN");
        node["mergeable"] = json!(earlier.0);
        node["mergeStateStatus"] = json!(earlier.1);
        let github = answering(super::reply())
            .page(pushed_to_shared_branch())
            .reply(fixtures::lookup_reply(json!([]), &[json!([node.clone()])]))
            .reply(your_commit());
        harness.read(&github, NOW, true);
        answered(&github, 3, &[1]);

        let mut low = super::reply();
        low["data"]["rateLimit"] = json!({ "remaining": 10, "resetAt": rfc3339(NOW + 15) });
        let mut answer = node.clone();
        answer["mergeable"] = json!(now.0);
        answer["mergeStateStatus"] = json!(now.1);
        low["data"]["review"] = json!({"issueCount": 1, "nodes": [answer]});
        let github = FakeGithub::default().reply_leaving_paused(low);
        let dto = harness.read(&github, NOW + 10, true);
        answered(&github, 1, &[]);
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
        let mut search = super::reply();
        search["data"]["review"] = json!({"issueCount": 1, "nodes": [node.clone()]});
        let github = answering(search).reply(fixtures::lookup_reply(json!([node]), &[json!([])]));
        let dto = harness.read(&github, NOW + 20, true);
        answered(&github, 2, &[]);
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

fn search_with_review(answer: Answer) -> Value {
    let mut search = super::reply();
    search["data"]["mine"] = json!({"issueCount": 0, "nodes": []});
    search["data"]["review"] = json!({"issueCount": 1, "nodes": [shared(answer)]});
    search
}

fn watched_reads(
    first: Answer,
    then: &[(Answer, Option<Answer>)],
) -> Vec<(Vec<Option<MergeKind>>, Vec<EventKind>)> {
    let harness = Harness::new("gh-conflict-failing-lookups", &[], 60);
    let mut watch = Watch::new();
    let github = answering(search_with_review(first))
        .page(pushed_to_shared_branch())
        .reply(fixtures::lookup_reply(json!([]), &[json!([shared(first)])]))
        .reply(your_commit());
    let mut reads = vec![harness.read(&github, NOW, true)];
    answered(&github, 3, &[1]);
    let failure = json!({"errors": [{"message": "Something went wrong."}]});
    for (step, (review, lookup)) in (1..).zip(then) {
        let lookup = lookup.map_or(failure.clone(), |answer| {
            fixtures::lookup_reply(json!([shared(answer)]), &[json!([])])
        });
        let github = answering(search_with_review(*review)).reply(lookup);
        reads.push(harness.read(&github, NOW + step * 10, true));
        answered(&github, 2, &[]);
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
    write_files_saved_before_the_conflict_memory(&harness.home);
    let mut watch = Watch::new();
    let failure = json!({"errors": [{"message": "Something went wrong."}]});
    let lookup = |answer: Answer| fixtures::lookup_reply(json!([shared(answer)]), &[]);
    let reads = [
        (reply("UNKNOWN", "UNKNOWN"), failure),
        (reply("UNKNOWN", "UNKNOWN"), lookup(UNKNOWN)),
        (reply("MERGEABLE", "CLEAN"), lookup(CLEAN)),
    ];
    let mut seen = Vec::new();
    for (step, (search, lookup)) in (1..).zip(reads) {
        let github = answering(search).reply(lookup);
        let dto = harness.read(&github, NOW + step * 10, true);
        answered(&github, 2, &[]);
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
