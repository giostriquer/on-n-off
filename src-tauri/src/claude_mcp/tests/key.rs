use std::fs;
use std::path::{Path, PathBuf};

use super::views::{document, state, states, Home};
use crate::dto::McpServerDto;

fn linked_worktree(common: &Path, worktree: &Path, name: &str) {
    let gitdir = common.join("worktrees").join(name);
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    fs::write(
        gitdir.join("gitdir"),
        format!("{}\n", worktree.join(".git").display()),
    )
    .unwrap();
    fs::create_dir_all(worktree).unwrap();
    fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", gitdir.display()),
    )
    .unwrap();
}

type Layout = fn(&Path) -> (PathBuf, PathBuf);

#[test]
fn a_view_inside_a_repository_reads_and_writes_the_entry_claude_code_keys_by_its_root() {
    let cases: [(&str, Layout); 5] = [
        ("subdirectory", |acme| {
            let root = acme.join("webapp");
            fs::create_dir_all(root.join(".git")).unwrap();
            (root.clone(), root.join("packages").join("api"))
        }),
        ("linked worktree", |acme| {
            let root = acme.join("webapp");
            fs::create_dir_all(root.join(".git")).unwrap();
            let worktree = acme.join("webapp-feature");
            linked_worktree(&root.join(".git"), &worktree, "feature");
            (root, worktree)
        }),
        ("inside a linked worktree", |acme| {
            let root = acme.join("webapp");
            fs::create_dir_all(root.join(".git")).unwrap();
            let worktree = acme.join("webapp-feature");
            linked_worktree(&root.join(".git"), &worktree, "feature");
            (root, worktree.join("src"))
        }),
        ("worktree of a bare repository", |acme| {
            let bare = acme.join("webapp.git");
            fs::create_dir_all(&bare).unwrap();
            fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").unwrap();
            let worktree = acme.join("webapp-main");
            linked_worktree(&bare, &worktree, "main");
            (bare, worktree)
        }),
        ("linked worktree with relative links", |acme| {
            let root = acme.join("webapp");
            let gitdir = root.join(".git").join("worktrees").join("rel");
            fs::create_dir_all(&gitdir).unwrap();
            fs::write(gitdir.join("commondir"), "../..\n").unwrap();
            fs::write(gitdir.join("gitdir"), "../../../../webapp-rel/.git\n").unwrap();
            let worktree = acme.join("webapp-rel");
            fs::create_dir_all(&worktree).unwrap();
            fs::write(
                worktree.join(".git"),
                "gitdir: ../webapp/.git/worktrees/rel\n",
            )
            .unwrap();
            (root, worktree)
        }),
    ];
    for (what, layout) in cases {
        let home = Home::new("repository-key");
        let (root, view) = layout(&home.root.join("acme"));
        fs::create_dir_all(&view).unwrap();
        let key = |path: &Path| path.to_string_lossy().into_owned();
        let seeded = serde_json::json!({
            "mcpServers": {
                "github": { "command": "npx" },
                "notes": { "command": "notes" }
            },
            "projects": {
                key(&root): { "disabledMcpServers": ["github"] },
                key(&view): { "disabledMcpServers": ["notes"] }
            }
        });
        home.write(&home.claude_json(), seeded.clone());
        let adapter = home.adapter();
        let changes = crate::change::Changes::beside(&home.root);

        let shown = adapter.list_scope(Some(&key(&view))).unwrap();
        let answer = changes
            .make(
                &*adapter,
                crate::change::Request::mcp("notes", false),
                Some(&key(&view)),
            )
            .unwrap_or_else(|error| panic!("{what}: {error:?}"));

        let switchable: Vec<_> = shown
            .mcp_servers
            .iter()
            .map(|server| (server.id.as_str(), server.enabled, server.togglable))
            .collect();
        assert_eq!(
            switchable,
            [("github", false, true), ("notes", true, true)],
            "{what}"
        );
        let mut expected = seeded;
        expected["projects"][key(&root)]["disabledMcpServers"] =
            serde_json::json!(["github", "notes"]);
        assert_eq!(document(&home), expected, "{what}");
        assert_eq!(
            states(&answer),
            [state("github", false), state("notes", false)],
            "{what}"
        );
    }
}

#[test]
fn a_view_outside_any_repository_reads_the_entry_of_its_own_folder() {
    let home = Home::new("folder-key");
    let parent = home.root.join("acme");
    home.write(
        &home.claude_json(),
        serde_json::json!({
            "mcpServers": { "github": { "command": "npx" } },
            "projects": {
                parent.to_string_lossy(): { "disabledMcpServers": [] },
                home.key(): { "disabledMcpServers": ["github"] }
            }
        }),
    );

    let view = home.adapter().list_scope(Some(&home.key())).unwrap();

    assert_eq!(states(&view), [state("github", false)]);
}

#[test]
fn a_switch_settles_its_entry_before_the_edit_so_the_edit_reads_no_file() {
    let home = Home::new("settled-key");
    let root = home.root.join("acme").join("webapp");
    fs::create_dir_all(root.join(".git")).unwrap();
    let worktree = home.root.join("acme").join("webapp-feature");
    linked_worktree(&root.join(".git"), &worktree, "feature");
    let key = |path: &Path| path.to_string_lossy().into_owned();
    let mut claude_json = serde_json::json!({
        "projects": {
            key(&root): { "disabledMcpServers": [] },
            key(&worktree): { "disabledMcpServers": [] }
        }
    });
    let github = McpServerDto {
        id: "github".into(),
        name: "github".into(),
        system: "stdio".into(),
        source: "npx".into(),
        enabled: true,
        togglable: true,
        origin: String::new(),
        plugin_id: None,
        projects: Vec::new(),
    };

    let edit = crate::claude_mcp::switch(Some(&worktree), &github, false).unwrap();
    fs::remove_file(worktree.join(".git")).unwrap();
    edit(&mut claude_json).unwrap();

    assert_eq!(
        claude_json["projects"][key(&root)]["disabledMcpServers"],
        serde_json::json!(["github"])
    );
    assert_eq!(
        claude_json["projects"][key(&worktree)]["disabledMcpServers"],
        serde_json::json!([])
    );
}

fn servers(home: &Home, projects: serde_json::Value) -> serde_json::Value {
    let seeded = serde_json::json!({
        "mcpServers": {
            "github": { "command": "npx" },
            "notes": { "command": "notes" }
        },
        "projects": projects
    });
    home.write(&home.claude_json(), seeded.clone());
    seeded
}

#[test]
fn a_worktree_whose_links_do_not_check_out_keeps_its_own_folder_as_its_key() {
    let cases: [(&str, Layout); 3] = [
        ("worktree moved after it was linked", |acme| {
            let root = acme.join("webapp");
            fs::create_dir_all(root.join(".git")).unwrap();
            let linked = acme.join("webapp-feature");
            linked_worktree(&root.join(".git"), &linked, "feature");
            let moved = acme.join("webapp-moved");
            fs::rename(&linked, &moved).unwrap();
            (root, moved)
        }),
        ("git dir outside the common dir's worktrees", |acme| {
            let root = acme.join("webapp");
            fs::create_dir_all(root.join(".git")).unwrap();
            let gitdir = acme.join("elsewhere").join("feature");
            fs::create_dir_all(&gitdir).unwrap();
            fs::write(
                gitdir.join("commondir"),
                format!("{}\n", root.join(".git").display()),
            )
            .unwrap();
            let worktree = acme.join("webapp-feature");
            fs::create_dir_all(&worktree).unwrap();
            fs::write(
                gitdir.join("gitdir"),
                format!("{}\n", worktree.join(".git").display()),
            )
            .unwrap();
            fs::write(
                worktree.join(".git"),
                format!("gitdir: {}\n", gitdir.display()),
            )
            .unwrap();
            (root, worktree)
        }),
        ("common dir not named .git that holds a .git", |acme| {
            let common = acme.join("webapp-common");
            fs::create_dir_all(common.join(".git")).unwrap();
            let worktree = acme.join("webapp-feature");
            linked_worktree(&common, &worktree, "feature");
            (common, worktree)
        }),
    ];
    for (what, layout) in cases {
        let home = Home::new("own-key");
        let (target, view) = layout(&home.root.join("acme"));
        let key = |path: &Path| path.to_string_lossy().into_owned();
        let seeded = servers(
            &home,
            serde_json::json!({
                key(&target): { "disabledMcpServers": ["github"] },
                key(&view): { "disabledMcpServers": ["notes"] }
            }),
        );
        let adapter = home.adapter();
        let changes = crate::change::Changes::beside(&home.root);

        let shown = adapter.list_scope(Some(&key(&view))).unwrap();
        changes
            .make(
                &*adapter,
                crate::change::Request::mcp("github", false),
                Some(&key(&view)),
            )
            .unwrap_or_else(|error| panic!("{what}: {error:?}"));

        let rows: Vec<_> = shown
            .mcp_servers
            .iter()
            .map(|server| (server.id.as_str(), server.enabled, server.togglable))
            .collect();
        assert_eq!(
            rows,
            [("github", true, true), ("notes", false, true)],
            "{what}"
        );
        let mut expected = seeded;
        expected["projects"][key(&view)]["disabledMcpServers"] =
            serde_json::json!(["notes", "github"]);
        assert_eq!(document(&home), expected, "{what}");
    }
}

#[test]
fn an_entry_kept_for_a_subfolder_or_worktree_is_never_read_in_place_of_the_root() {
    let cases: [(&str, Layout); 2] = [
        ("subdirectory", |acme| {
            let root = acme.join("webapp");
            fs::create_dir_all(root.join(".git")).unwrap();
            (root.clone(), root.join("packages").join("api"))
        }),
        ("linked worktree", |acme| {
            let root = acme.join("webapp");
            fs::create_dir_all(root.join(".git")).unwrap();
            let worktree = acme.join("webapp-feature");
            linked_worktree(&root.join(".git"), &worktree, "feature");
            (root, worktree)
        }),
    ];
    for (what, layout) in cases {
        let home = Home::new("legacy-key");
        let (_, view) = layout(&home.root.join("acme"));
        fs::create_dir_all(&view).unwrap();
        let view = view.to_string_lossy().into_owned();
        let seeded = servers(
            &home,
            serde_json::json!({ view.clone(): { "disabledMcpServers": ["github"] } }),
        );
        let adapter = home.adapter();
        let changes = crate::change::Changes::beside(&home.root);

        let shown = adapter.list_scope(Some(&view)).unwrap();
        let refused = changes
            .make(
                &*adapter,
                crate::change::Request::mcp("github", true),
                Some(&view),
            )
            .unwrap_err();

        let rows: Vec<_> = shown
            .mcp_servers
            .iter()
            .map(|server| (server.id.as_str(), server.enabled, server.togglable))
            .collect();
        assert_eq!(
            rows,
            [("github", true, false), ("notes", true, false)],
            "{what}"
        );
        assert_eq!(
            refused.message, "mcp server is not togglable: github",
            "{what}"
        );
        assert_eq!(document(&home), seeded, "{what}");
    }
}
