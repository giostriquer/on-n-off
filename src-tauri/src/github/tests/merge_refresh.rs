use super::*;
use crate::dto::{CiState, MergeKind};
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
    for list in [
        &dto.data.mine,
        &dto.data.review_requested,
        &dto.data.assigned,
    ] {
        assert_eq!(
            list.items[0].merge_kind == Some(MergeKind::Conflicts),
            expected
        );
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
        for pr in dto
            .data
            .mine
            .items
            .iter()
            .chain(&dto.data.review_requested.items)
            .chain(&dto.data.assigned.items)
        {
            assert_eq!(pr.mergeable, crate::dto::Mergeability::Unknown);
            assert_eq!(pr.merge_state, crate::dto::MergeState::Unknown);
        }
        assert_eq!(dto.data.fetched_at, rfc3339(NOW + step * 60));
        assert!(!dto.stale);
        assert_eq!(
            snapshot::load(&github_prs_path_for(&harness.home)),
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
fn unknown_does_not_invent_conflicts_or_reuse_them_for_another_revision_or_viewer() {
    for field in [
        "headRefOid",
        "baseRefName",
        "id",
        "viewer",
        "missingHead",
        "legacy",
    ] {
        let harness = Harness::new("gh-conflict-identity", &[], 60);
        let gh = gh(&harness.home.join("cli"), "gho_t");
        let mut first = reply("CONFLICTING", "DIRTY");
        let mut unknown = reply("UNKNOWN", "UNKNOWN");
        if field == "viewer" {
            unknown["data"]["viewer"]["login"] = json!("alice");
        } else {
            for list in ["mine", "review", "assigned"] {
                if field == "legacy" {
                    first["data"][list]["nodes"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("headRefOid");
                } else if field == "missingHead" {
                    unknown["data"][list]["nodes"][0]["headRefOid"] = Value::Null;
                } else {
                    unknown["data"][list]["nodes"][0][field] = json!(format!("changed-{list}"));
                }
            }
        }
        assert_conflicts(&read(&harness, &gh, &unknown, NOW), false);
        read(&harness, &gh, &first, NOW + 60);
        assert_conflicts(&read(&harness, &gh, &unknown, NOW + 120), false);
    }
}

#[test]
fn a_definite_answer_clears_an_unknown_contribution_in_every_list_and_its_saved_history() {
    let mut harness = Harness::new("gh-conflict-overlap", &[], 60);
    let gh = gh(&harness.home.join("cli"), "gho_t");
    let mut search: Value = serde_json::from_str(REPLY).unwrap();
    search["data"]["mine"] = json!({"issueCount":0,"nodes":[]});
    let mut node = fixtures::contribution_node("PR_shared", "alice", "acme/app", "OPEN");
    node["mergeable"] = json!("CONFLICTING");
    node["mergeStateStatus"] = json!("DIRTY");
    search["data"]["review"] = json!({"issueCount":1,"nodes":[node.clone()]});
    let first_search = search.to_string();
    let events = pushed_to_shared_branch();
    let lookup = fixtures::lookup_reply(json!([]), &[json!([node.clone()])]).to_string();
    let commits = your_commit();
    let (url, server) = serve_sequence(&[
        ("200 OK", &[], &first_search),
        ("200 OK", &[], &events),
        ("200 OK", &[], &lookup),
        ("200 OK", &[], &commits),
    ]);
    let (first, _) = harness.read_with_events(&gh, &url, &events_url(&url), NOW, true);
    server.join().unwrap();
    assert_eq!(
        first.data.mine.items[0].merge_kind,
        Some(MergeKind::Conflicts)
    );
    assert_eq!(
        first.data.review_requested.items[0].merge_kind,
        Some(MergeKind::Conflicts)
    );

    node["mergeable"] = json!("UNKNOWN");
    node["mergeStateStatus"] = json!("UNKNOWN");
    for step in 1..=3 {
        if step == 3 {
            harness.read_memo = ReadMemo::new();
        }
        search["data"]["review"]["nodes"][0] = node.clone();
        if step == 1 {
            search["data"]["review"]["nodes"][0]["mergeable"] = json!("MERGEABLE");
            search["data"]["review"]["nodes"][0]["mergeStateStatus"] = json!("CLEAN");
        }
        let search_body = search.to_string();
        let lookup = fixtures::lookup_reply(json!([node.clone()]), &[json!([])]).to_string();
        let (url, server) =
            serve_sequence(&[("200 OK", &[], &search_body), ("200 OK", &[], &lookup)]);
        let (dto, _) =
            harness.read_with_events(&gh, &url, &events_url(&url), NOW + step * 10, true);
        server.join().unwrap();
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
        assert_eq!(reading.segments.len(), 1);
        assert!(!reading.segments[0].passing_with_conflicts);
        assert_eq!(reading.attention.conflicts, 0);
        for list in &reading.lists {
            assert!(!list.items[0]
                .badges
                .iter()
                .any(|badge| badge.label == "Conflicts"));
        }
        for pr in dto
            .data
            .mine
            .items
            .iter()
            .chain(&dto.data.review_requested.items)
        {
            assert_ne!(
                pr.merge_kind,
                Some(MergeKind::Conflicts),
                "step {step}: {}",
                pr.id
            );
        }
        let stored = contributions::read(
            None,
            &paths::github_contributions_path_for(&harness.home),
            "octocat",
            &[],
            &[],
            NOW + step * 10,
        );
        assert_ne!(
            stored.open[0].merge_kind,
            Some(MergeKind::Conflicts),
            "saved contribution at step {step}"
        );
    }
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
    assert_eq!(first.data.review_requested.items[0].merge_kind, None);

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
        for pr in dto
            .data
            .mine
            .items
            .iter()
            .chain(&dto.data.review_requested.items)
        {
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
