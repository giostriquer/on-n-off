use super::*;
use crate::adapter::scratch_adapter as adapter_at;

fn place(skills: &str, agents: Option<&str>) -> Option<ItemPlace> {
    Some(ItemPlace {
        skills: skills.into(),
        agents: agents.map(str::to_string),
    })
}

#[test]
fn an_agent_shows_where_its_items_go_from_its_own_item_roots() {
    let home = crate::paths::scratch_dir("on-n-off-capabilities-places");
    let muse_skills = if cfg!(windows) {
        "~/AppData/Roaming/muse/skills"
    } else {
        "~/.config/muse/skills"
    };
    let cases = [
        (
            AgentId::Claude,
            ("~/.claude/skills", Some("~/.claude/agents")),
            (".claude/skills", Some(".claude/agents")),
        ),
        (
            AgentId::Codex,
            ("~/.codex/skills", None),
            (".codex/skills", None),
        ),
        (
            AgentId::Antigravity,
            ("~/.gemini/antigravity-cli/skills", None),
            (".agents/skills", None),
        ),
        (
            AgentId::Cursor,
            ("~/.cursor/skills", None),
            (".cursor/skills", None),
        ),
        (AgentId::Muse, (muse_skills, None), (".agents/skills", None)),
    ];
    for (id, global, project) in cases {
        let adapter = adapter_at(id, &home);
        assert_eq!(
            ItemPlaces::of(&*adapter, Some(&home)),
            ItemPlaces {
                global: place(global.0, global.1),
                project: place(project.0, project.1),
            },
            "{id:?}"
        );
    }

    let elsewhere = ItemPlaces::of(&*adapter_at(AgentId::Claude, &home), None);
    assert_eq!(
        elsewhere.global.unwrap().skills,
        home.join(".claude").join("skills").display().to_string()
    );
    let _ = std::fs::remove_dir_all(home);
}
