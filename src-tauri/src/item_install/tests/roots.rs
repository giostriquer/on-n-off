use super::*;

#[test]
fn item_roots_per_provider_and_scope() {
    let home = scratch_dir("items-roots");
    let project = home.join("proj");
    let scope = ItemScope::Project {
        project_path: project.to_string_lossy().into_owned(),
    };
    let claude = ClaudeAdapter::at(home.join(".claude"));
    let g = claude.item_roots(&ItemScope::Global).unwrap();
    assert_eq!(g.skills, home.join(".claude/skills"));
    assert_eq!(g.agents, Some(home.join(".claude/agents")));
    let p = claude.item_roots(&scope).unwrap();
    assert_eq!(p.skills, project.join(".claude/skills"));
    assert_eq!(p.agents, Some(project.join(".claude/agents")));

    let codex = CodexAdapter::at(home.join(".codex"), home.join(".agents/skills"));
    assert_eq!(
        codex.item_roots(&ItemScope::Global).unwrap().skills,
        home.join(".codex/skills")
    );
    assert_eq!(
        codex.item_roots(&scope).unwrap().skills,
        project.join(".codex/skills")
    );
    assert!(codex.item_roots(&scope).unwrap().agents.is_none());

    let anti = AntigravityAdapter::at(home.join(".gemini"));
    assert_eq!(
        anti.item_roots(&ItemScope::Global).unwrap().skills,
        home.join(".gemini/antigravity-cli/skills")
    );
    assert_eq!(
        anti.item_roots(&scope).unwrap().skills,
        project.join(".agents/skills")
    );

    let cursor = CursorAdapter::at(home.join(".cursor"));
    assert_eq!(
        cursor.item_roots(&ItemScope::Global).unwrap().skills,
        home.join(".cursor/skills")
    );
    assert_eq!(
        cursor.item_roots(&scope).unwrap().skills,
        project.join(".cursor/skills")
    );
    let _ = fs::remove_dir_all(home);
}
