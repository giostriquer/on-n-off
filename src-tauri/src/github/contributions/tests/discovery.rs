use super::*;

#[test]
fn a_pr_someone_else_opened_on_a_branch_you_pushed_joins_once_it_carries_a_commit_you_wrote() {
    let path = store_path("contrib-found");
    let github = one_push()
        .reply(lookup_reply(json!([]), &[json!([pr("PR_alice", "alice")])]))
        .reply(commits_reply(&[vec![
            commit(1, &["alice"]),
            commit(1, &["octocat"]),
        ]]));

    let found = read_at(&github, &path, NOW);

    assert_eq!(found.warnings, Vec::<String>::new());
    assert_eq!(ids(&found.open), vec!["PR_alice"]);
    assert_eq!(found.open[0].author, "alice");
    assert!(found.open[0].contributed);
    assert!(found.merged.is_empty());
    assert_eq!(
        *github.pages_asked.borrow(),
        vec![("octocat".to_string(), 1)]
    );
    assert_eq!(github.ids_sent(1), json!(["PR_alice"]));
}

#[test]
fn a_merge_commit_of_yours_is_not_a_commit_you_wrote() {
    let path = store_path("contrib-merge-commit");
    let github = one_push()
        .reply(lookup_reply(json!([]), &[json!([pr("PR_alice", "alice")])]))
        .reply(commits_reply(&[vec![
            commit(1, &["alice"]),
            commit(2, &["octocat"]),
        ]]));

    assert!(read_at(&github, &path, NOW).open.is_empty());
}

#[test]
fn a_commit_you_co_authored_counts() {
    let path = store_path("contrib-co-author");
    let github = one_push()
        .reply(lookup_reply(json!([]), &[json!([pr("PR_alice", "alice")])]))
        .reply(commits_reply(&[vec![commit(1, &["alice", "OctoCat"])]]));

    assert_eq!(ids(&read_at(&github, &path, NOW).open), vec!["PR_alice"]);
}

#[test]
fn a_pr_you_opened_is_left_to_the_mine_search() {
    let path = store_path("contrib-own");
    let github = one_push().reply(lookup_reply(json!([]), &[json!([pr("PR_own", "OctoCat")])]));

    assert!(read_at(&github, &path, NOW).open.is_empty());
    assert_eq!(github.requests(), 1, "nothing to check");
}

#[test]
fn a_fork_pr_with_the_same_branch_name_is_not_on_the_branch_you_pushed() {
    let path = store_path("contrib-fork");
    let fork = contribution_node("PR_fork", "alice", "alice/app", "OPEN");
    let github = one_push().reply(lookup_reply(json!([]), &[json!([fork])]));

    assert!(read_at(&github, &path, NOW).open.is_empty());
    assert_eq!(github.requests(), 1, "nothing to check");
}

#[test]
fn the_head_repository_matches_the_pushed_one_in_any_case() {
    let path = store_path("contrib-head-case");
    let node = contribution_node("PR_alice", "alice", "ACME/App", "OPEN");
    let github = one_push()
        .reply(lookup_reply(json!([]), &[json!([node])]))
        .reply(commits_reply(&[yours()]));

    assert_eq!(ids(&read_at(&github, &path, NOW).open), vec!["PR_alice"]);
}

#[test]
fn a_pr_from_the_repository_default_branch_is_not_a_contribution() {
    let path = store_path("contrib-default-branch");
    let github = FakeGithub::default()
        .page(json!([push(10, "acme/app", "refs/heads/main", NOW - 60)]))
        .reply(lookup_reply(
            json!([]),
            &[json!([pr("PR_release", "alice")])],
        ));

    assert!(read_at(&github, &path, NOW).open.is_empty());
    assert_eq!(github.requests(), 1, "nothing to check");

    let next = FakeGithub::default();
    read_at(&next, &path, NOW + 30);
    assert_eq!(
        next.requests(),
        0,
        "the default branch is not looked up again"
    );
}

#[test]
fn only_branch_pushes_to_a_named_repository_are_looked_up() {
    let path = store_path("contrib-tags");
    let github = FakeGithub::default().page(json!([
        push(14, "acme/app", "refs/tags/v1.0.0", NOW - 60),
        push(13, "app", "refs/heads/fix/x", NOW - 60),
        push(12, "/app", "refs/heads/fix/x", NOW - 60),
        { "id": "11", "type": "IssuesEvent", "repo": { "name": "acme/app" }, "payload": {}, "created_at": at(NOW) },
        { "id": "not-a-number", "type": "PushEvent", "repo": { "name": "acme/app" },
          "payload": { "ref": "refs/heads/fix/x" }, "created_at": at(NOW) },
    ]));

    let found = read_at(&github, &path, NOW);

    assert_eq!(found.warnings, Vec::<String>::new());
    assert_eq!(github.requests(), 0, "no branch, no lookup");
}

#[test]
fn an_events_reply_that_is_not_a_list_is_reported_and_read_again_at_once() {
    let path = store_path("contrib-events-shape");
    let github = FakeGithub::default().page(json!({ "message": "Not Found" }));

    let found = read_at(&github, &path, NOW);

    assert_eq!(
        found.warnings,
        vec!["Your recent pushes were not read. GitHub sent an unexpected reply.".to_string()]
    );
    let again = FakeGithub::default();
    read_at(&again, &path, NOW + 1);
    assert_eq!(again.pages(), vec![1]);
}

#[test]
fn every_repository_and_branch_name_travels_as_a_variable() {
    let path = store_path("contrib-variables");
    let github = FakeGithub::default()
        .page(json!([push(
            10,
            "acme/app",
            "refs/heads/fix/\"quoted\" { x }",
            NOW - 60
        )]))
        .reply(empty_lookup());

    read_at(&github, &path, NOW);

    let body = github.body(0);
    let query = body["query"].as_str().unwrap();
    assert!(!query.contains("quoted"), "{query}");
    assert!(!query.contains("acme"), "{query}");
    assert_eq!(body["variables"]["o0"], "acme");
    assert_eq!(body["variables"]["n0"], "app");
    assert_eq!(body["variables"]["h0"], "fix/\"quoted\" { x }");
    assert_eq!(body["variables"]["ids"], json!([]));
    assert!(query.contains("defaultBranchRef { name }"));
    assert!(query.contains("headRefName: $h0, states: OPEN, first: 5"));
}

#[test]
fn events_are_read_at_most_once_a_minute() {
    let path = store_path("contrib-events-window");
    let first = one_push().reply(empty_lookup());
    read_at(&first, &path, NOW);

    let soon = FakeGithub::default().reply(empty_lookup());
    read_at(&soon, &path, NOW + 59);
    assert!(soon.pages().is_empty());

    let later = FakeGithub::default().reply(empty_lookup());
    read_at(&later, &path, NOW + 60);
    assert_eq!(later.pages(), vec![1]);
}

#[test]
fn a_page_that_reaches_the_newest_event_read_is_the_last_one_and_counts_whole() {
    let path = store_path("contrib-events-cursor");
    let first = one_push().reply(empty_lookup());
    read_at(&first, &path, NOW);

    let mut page: Vec<Value> = (102..=200)
        .rev()
        .map(|id| push(id, "acme/app", "refs/heads/fix/y", NOW + 30))
        .collect();
    page.push(push(5, "acme/app", "refs/heads/fix/late", NOW - 30));
    let later = FakeGithub::default()
        .page(Value::Array(page))
        .reply(lookup_reply(json!([]), &[json!([]), json!([]), json!([])]));
    read_at(&later, &path, NOW + 60);

    assert_eq!(later.pages(), vec![1]);
    assert_eq!(
        later.branches_looked_up(0),
        vec!["acme/app:fix/y", "acme/app:fix/late", "acme/app:fix/x"]
    );
}

#[test]
fn a_full_page_of_events_reads_the_next_up_to_three() {
    let path = store_path("contrib-pages");
    let full = |from: u64| -> Value {
        Value::Array(
            (0..100)
                .map(|offset| push(from - offset, "acme/app", "refs/heads/fix/x", NOW - 60))
                .collect(),
        )
    };
    let github = FakeGithub::default()
        .page(full(1_000))
        .page(full(900))
        .page(full(800))
        .page(full(700))
        .reply(empty_lookup());

    read_at(&github, &path, NOW);

    assert_eq!(github.pages(), vec![1, 2, 3]);
}

#[test]
fn the_newest_event_read_survives_a_read_with_nothing_new() {
    let path = store_path("contrib-events-quiet");
    let first = one_push().reply(empty_lookup());
    read_at(&first, &path, NOW);
    let quiet = FakeGithub::default().reply(empty_lookup());
    read_at(&quiet, &path, NOW + 60);

    let page: Vec<Value> = (10..=109)
        .rev()
        .map(|id| push(id, "acme/app", "refs/heads/fix/x", NOW + 90))
        .collect();
    let full = FakeGithub::default()
        .page(Value::Array(page))
        .reply(empty_lookup());
    read_at(&full, &path, NOW + 120);

    assert_eq!(full.pages(), vec![1], "event 10 was read before");
}

#[test]
fn a_failed_events_read_still_looks_up_the_branches_already_remembered() {
    let path = store_path("contrib-events-failed");
    let first = one_push().reply(empty_lookup());
    read_at(&first, &path, NOW);

    let github = FakeGithub::default()
        .failing_page("Could not reach GitHub (HTTP 503).")
        .reply(empty_lookup());
    let found = read_at(&github, &path, NOW + 60);

    assert_eq!(
        found.warnings,
        vec!["Your recent pushes were not read. Could not reach GitHub (HTTP 503).".to_string()]
    );
    assert_eq!(github.branches_looked_up(0), vec!["acme/app:fix/x"]);

    let retry = FakeGithub::default().reply(empty_lookup());
    read_at(&retry, &path, NOW + 61);
    assert_eq!(retry.pages(), vec![1], "a failed read is retried at once");
}

#[test]
fn the_same_branch_name_in_two_repositories_is_two_branches() {
    let path = store_path("contrib-two-repos");
    let github = FakeGithub::default()
        .page(json!([
            push(11, "acme/lib", "refs/heads/fix/x", NOW - 30),
            push(10, "acme/app", "refs/heads/fix/x", NOW - 60),
        ]))
        .reply(lookup_reply(json!([]), &[json!([]), json!([])]));

    read_at(&github, &path, NOW);

    assert_eq!(
        github.branches_looked_up(0),
        vec!["acme/lib:fix/x", "acme/app:fix/x"]
    );
}

#[test]
fn at_most_fifty_branches_are_looked_up_newest_first() {
    let path = store_path("contrib-cap");
    let pushes: Vec<Value> = (1..=51)
        .rev()
        .map(|n| {
            push(
                n,
                "acme/app",
                &format!("refs/heads/b-{n}"),
                NOW - 1_000 + n as i64,
            )
        })
        .collect();
    let github = FakeGithub::default()
        .page(Value::Array(pushes))
        .reply(lookup_reply(json!([]), &vec![json!([]); 50]));

    read_at(&github, &path, NOW);

    let looked_up = github.branches_looked_up(0);
    assert_eq!(looked_up.len(), 50);
    assert_eq!(looked_up[0], "acme/app:b-51");
    assert_eq!(looked_up[49], "acme/app:b-2");
}

#[test]
fn a_branch_is_looked_up_until_thirty_days_after_it_was_pushed() {
    let path = store_path("contrib-branch-age");
    let first = FakeGithub::default()
        .page(json!([push(10, "acme/app", "refs/heads/fix/x", NOW)]))
        .reply(empty_lookup());
    read_at(&first, &path, NOW);

    let last_day = FakeGithub::default().reply(empty_lookup());
    read_at(&last_day, &path, NOW + 30 * DAY - 1);
    assert_eq!(last_day.branches_looked_up(0), vec!["acme/app:fix/x"]);

    let after = FakeGithub::default();
    read_at(&after, &path, NOW + 30 * DAY);
    assert_eq!(after.requests(), 0);
}

#[test]
fn a_branch_pushed_again_is_kept_thirty_days_from_the_latest_push() {
    let path = store_path("contrib-branch-repush");
    let first = FakeGithub::default()
        .page(json!([push(10, "acme/app", "refs/heads/fix/x", NOW)]))
        .reply(empty_lookup());
    read_at(&first, &path, NOW);
    let again = FakeGithub::default()
        .page(json!([push(
            11,
            "acme/app",
            "refs/heads/fix/x",
            NOW + 20 * DAY
        )]))
        .reply(empty_lookup());
    read_at(&again, &path, NOW + 20 * DAY);

    let later = FakeGithub::default().reply(empty_lookup());
    read_at(&later, &path, NOW + 50 * DAY - 1);

    assert_eq!(later.branches_looked_up(0), vec!["acme/app:fix/x"]);
}
