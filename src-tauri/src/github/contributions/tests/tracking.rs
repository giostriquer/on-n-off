use std::fs;

use super::*;

#[test]
fn a_verified_pr_is_never_checked_again_and_an_unverified_one_only_when_its_head_moves() {
    let path = store_path("contrib-verify-once");
    let first = one_push()
        .reply(lookup_reply(
            json!([]),
            &[json!([pr("PR_yes", "alice"), pr("PR_no", "bob")])],
        ))
        .reply(commits_reply(&[yours(), theirs()]));
    assert_eq!(ids(&read_at(&first, &path, NOW).open), vec!["PR_yes"]);

    let both = json!([pr("PR_yes", "alice"), pr("PR_no", "bob")]);
    let quiet = FakeGithub::default().reply(lookup_reply(both.clone(), &[both]));
    assert_eq!(ids(&read_at(&quiet, &path, NOW + 30).open), vec!["PR_yes"]);
    assert_eq!(quiet.requests(), 1, "no head moved, nothing to check");

    let both_moved = json!([
        moved(pr("PR_yes", "alice"), "head2"),
        moved(pr("PR_no", "bob"), "head2"),
    ]);
    let pushed = FakeGithub::default()
        .reply(lookup_reply(both_moved.clone(), &[both_moved]))
        .reply(commits_reply(&[yours()]));
    let found = read_at(&pushed, &path, NOW + 40);

    assert_eq!(pushed.ids_sent(1), json!(["PR_no"]));
    assert_eq!(ids(&found.open), vec!["PR_yes", "PR_no"]);
}

#[test]
fn a_verified_contribution_outlives_its_branch_and_an_unverified_one_does_not() {
    let path = store_path("contrib-outlives");
    let first = FakeGithub::default()
        .page(json!([push(10, "acme/app", "refs/heads/fix/x", NOW)]))
        .reply(lookup_reply(
            json!([]),
            &[json!([pr("PR_yes", "alice"), pr("PR_no", "bob")])],
        ))
        .reply(commits_reply(&[yours(), theirs()]));
    read_at(&first, &path, NOW);
    let both = json!([pr("PR_yes", "alice"), pr("PR_no", "bob")]);
    let other_branch = FakeGithub::default()
        .page(json!([push(
            11,
            "acme/app",
            "refs/heads/fix/other",
            NOW + 20 * DAY
        )]))
        .reply(lookup_reply(both.clone(), &[json!([]), both]));
    read_at(&other_branch, &path, NOW + 20 * DAY);

    let aged =
        FakeGithub::default().reply(lookup_reply(json!([pr("PR_yes", "alice")]), &[json!([])]));
    let found = read_at(&aged, &path, NOW + 31 * DAY);

    assert_eq!(aged.branches_looked_up(0), vec!["acme/app:fix/other"]);
    assert_eq!(aged.ids_sent(0), json!(["PR_yes"]));
    assert_eq!(ids(&found.open), vec!["PR_yes"]);
}

#[test]
fn a_merged_contribution_moves_to_the_merged_list_and_a_closed_one_is_forgotten() {
    let path = store_path("contrib-merged");
    let first = one_push()
        .reply(lookup_reply(
            json!([]),
            &[json!([pr("PR_merged", "alice"), pr("PR_closed", "bob")])],
        ))
        .reply(commits_reply(&[yours(), yours()]));
    assert_eq!(read_at(&first, &path, NOW).open.len(), 2);

    let closed = contribution_node("PR_closed", "bob", "acme/app", "CLOSED");
    let after = FakeGithub::default().reply(lookup_reply(
        json!([merged_pr("PR_merged", NOW + 20), closed]),
        &[json!([])],
    ));
    let found = read_at(&after, &path, NOW + 30);

    assert!(found.open.is_empty());
    assert_eq!(ids(&found.merged), vec!["PR_merged"]);
    assert!(found.merged[0].contributed);
    assert_eq!(after.ids_sent(0), json!(["PR_merged", "PR_closed"]));

    let next = FakeGithub::default().reply(lookup_reply(
        json!([merged_pr("PR_merged", NOW + 20)]),
        &[json!([])],
    ));
    read_at(&next, &path, NOW + 60);
    assert_eq!(next.ids_sent(0), json!(["PR_merged"]));
}

#[test]
fn a_merged_contribution_is_kept_for_fourteen_days() {
    let path = store_path("contrib-merged-age");
    found_alice(&path);
    let merged = || lookup_reply(json!([merged_pr("PR_alice", NOW)]), &[json!([])]);
    read_at(&FakeGithub::default().reply(merged()), &path, NOW + 30);

    let last_day = FakeGithub::default().reply(merged());
    let found = read_at(&last_day, &path, NOW + 14 * DAY - 1);
    assert_eq!(last_day.ids_sent(0), json!(["PR_alice"]));
    assert_eq!(ids(&found.merged), vec!["PR_alice"]);

    let after = FakeGithub::default().reply(empty_lookup());
    let found = read_at(&after, &path, NOW + 14 * DAY);
    assert_eq!(after.ids_sent(0), json!([]));
    assert!(found.merged.is_empty());
}

#[test]
fn a_merge_github_gives_no_time_is_dated_when_first_seen() {
    let path = store_path("contrib-merged-undated");
    found_alice(&path);
    let undated = || {
        let mut node = merged_pr("PR_alice", NOW);
        node["mergedAt"] = Value::Null;
        lookup_reply(json!([node]), &[json!([])])
    };
    read_at(&FakeGithub::default().reply(undated()), &path, NOW + 30);
    read_at(
        &FakeGithub::default().reply(undated()),
        &path,
        NOW + 7 * DAY,
    );

    let last_day = FakeGithub::default().reply(undated());
    read_at(&last_day, &path, NOW + 30 + 14 * DAY - 1);
    assert_eq!(last_day.ids_sent(0), json!(["PR_alice"]));

    let after = FakeGithub::default().reply(empty_lookup());
    read_at(&after, &path, NOW + 30 + 14 * DAY);
    assert_eq!(after.ids_sent(0), json!([]));
}

#[test]
fn a_read_without_github_still_forgets_a_merge_older_than_fourteen_days() {
    let path = store_path("contrib-merged-offline-age");
    found_alice(&path);
    let merged = lookup_reply(json!([merged_pr("PR_alice", NOW)]), &[json!([])]);
    read_at(&FakeGithub::default().reply(merged), &path, NOW + 30);

    assert_eq!(
        ids(&read(None, &path, "octocat", &[], NOW + 14 * DAY - 1).merged),
        vec!["PR_alice"]
    );
    assert!(read(None, &path, "octocat", &[], NOW + 14 * DAY)
        .merged
        .is_empty());
}

#[test]
fn a_failed_lookup_keeps_the_contributions_last_read_and_says_why() {
    let path = store_path("contrib-lookup-failed");
    let first = one_push()
        .reply(lookup_reply(
            json!([]),
            &[json!([pr("PR_open", "alice"), pr("PR_merging", "bob")])],
        ))
        .reply(commits_reply(&[yours(), yours()]));
    read_at(&first, &path, NOW);
    let merging = FakeGithub::default().reply(lookup_reply(
        json!([pr("PR_open", "alice"), merged_pr("PR_merging", NOW + 20)]),
        &[json!([])],
    ));
    read_at(&merging, &path, NOW + 30);

    let failing = FakeGithub::default().failing_reply("Could not reach GitHub (HTTP 502).");
    let found = read_at(&failing, &path, NOW + 45);

    assert_eq!(ids(&found.open), vec!["PR_open"]);
    assert_eq!(ids(&found.merged), vec!["PR_merging"]);
    assert_eq!(
        found.warnings,
        vec![
            "Pull requests on your pushed branches were not looked up. Could not reach GitHub (HTTP 502)."
                .to_string()
        ]
    );
}

#[test]
fn a_reply_with_only_errors_is_a_failed_lookup() {
    let path = store_path("contrib-lookup-errors");
    found_alice(&path);
    let github = FakeGithub::default()
        .reply(json!({ "errors": [{ "message": "API rate limit exceeded." }] }));

    let found = read_at(&github, &path, NOW + 30);

    assert_eq!(ids(&found.open), vec!["PR_alice"]);
    assert_eq!(
        found.warnings,
        vec![
            "Pull requests on your pushed branches were not looked up. GitHub answered: API rate limit exceeded."
                .to_string()
        ]
    );
}

#[test]
fn a_repository_that_is_gone_drops_its_branch_and_keeps_the_others() {
    let path = store_path("contrib-gone-repo");
    let mut reply = lookup_reply(json!([]), &[json!([]), json!([pr("PR_alice", "alice")])]);
    reply["data"]["b0"] = Value::Null;
    reply["errors"] =
        json!([{ "message": "Could not resolve to a Repository with the name 'acme/gone'." }]);
    let github = FakeGithub::default()
        .page(json!([
            push(11, "acme/gone", "refs/heads/fix/x", NOW - 30),
            push(10, "acme/app", "refs/heads/fix/y", NOW - 60),
        ]))
        .reply(reply)
        .reply(commits_reply(&[yours()]));

    let found = read_at(&github, &path, NOW);

    assert_eq!(found.warnings, Vec::<String>::new());
    assert_eq!(ids(&found.open), vec!["PR_alice"]);
    let next =
        FakeGithub::default().reply(lookup_reply(json!([pr("PR_alice", "alice")]), &[json!([])]));
    read_at(&next, &path, NOW + 30);
    assert_eq!(next.branches_looked_up(0), vec!["acme/app:fix/y"]);
}

#[test]
fn a_failed_check_says_why_and_is_retried_on_the_next_read() {
    let path = store_path("contrib-check-failed");
    let github = one_push()
        .reply(lookup_reply(json!([]), &[json!([pr("PR_alice", "alice")])]))
        .failing_reply("Could not reach GitHub (HTTP 502).");

    let found = read_at(&github, &path, NOW);

    assert!(found.open.is_empty());
    assert_eq!(
        found.warnings,
        vec![
            "Your commits on pull requests were not checked. Could not reach GitHub (HTTP 502)."
                .to_string()
        ]
    );
    let retry = FakeGithub::default()
        .reply(lookup_reply(
            json!([pr("PR_alice", "alice")]),
            &[json!([pr("PR_alice", "alice")])],
        ))
        .reply(commits_reply(&[yours()]));
    assert_eq!(
        ids(&read_at(&retry, &path, NOW + 30).open),
        vec!["PR_alice"]
    );
}

#[test]
fn at_most_ten_pull_requests_are_checked_per_read() {
    let path = store_path("contrib-check-cap");
    let prs: Vec<Value> = (0..12).map(|n| pr(&format!("PR_{n}"), "alice")).collect();
    let github = one_push()
        .reply(lookup_reply(json!([]), &[Value::Array(prs)]))
        .reply(commits_reply(&vec![yours(); 10]));

    let found = read_at(&github, &path, NOW);

    assert_eq!(github.ids_sent(1).as_array().unwrap().len(), 10);
    assert_eq!(found.open.len(), 10);
}

#[test]
fn without_github_the_last_read_is_shown_and_nothing_is_read() {
    let path = store_path("contrib-offline");
    found_alice(&path);
    let before = fs::read_to_string(&path).unwrap();

    let found = read(None, &path, "octocat", &[], NOW + 60);

    assert_eq!(ids(&found.open), vec!["PR_alice"]);
    assert_eq!(found.warnings, Vec::<String>::new());
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn the_scope_applies_when_a_contribution_is_shown() {
    let path = store_path("contrib-scope");
    found_alice(&path);

    let other = read(None, &path, "octocat", &["org:other".to_string()], NOW + 1);
    let acme = read(None, &path, "octocat", &["org:acme".to_string()], NOW + 1);

    assert!(other.open.is_empty());
    assert_eq!(ids(&acme.open), vec!["PR_alice"]);
}

#[test]
fn the_record_belongs_to_one_github_account_in_any_case() {
    let path = store_path("contrib-viewer");
    found_alice(&path);

    let same = read(None, &path, "OctoCat", &[], NOW + 1);
    assert_eq!(ids(&same.open), vec!["PR_alice"]);

    let other = FakeGithub::default();
    let found = read(Some(&other as &dyn Github), &path, "hubot", &[], NOW + 2);
    assert!(found.open.is_empty());
    assert_eq!(*other.pages_asked.borrow(), vec![("hubot".to_string(), 1)]);
    assert_eq!(other.requests(), 0, "the other account's branches are gone");
}

#[test]
fn an_unreadable_record_starts_from_nothing() {
    let path = store_path("contrib-unreadable");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{ not json").unwrap();
    let github = one_push().reply(empty_lookup());

    let found = read_at(&github, &path, NOW);

    assert_eq!(found.warnings, Vec::<String>::new());
    assert_eq!(github.requests(), 1);
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("\"viewer\":\"octocat\""));
}
