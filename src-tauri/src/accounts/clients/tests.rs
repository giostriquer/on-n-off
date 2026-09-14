use super::*;
#[test]
fn recognizes_cli_launchers_after_node_flags_and_paths_with_spaces() {
    assert!(conflicts("node --some-flag --another-flag /Users/A User/tools/node_modules/@anthropic-ai/claude-code/cli.js",AgentId::Claude));
    assert!(conflicts("/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/openai.chatgpt/bin/macos-aarch64/codex app-server",AgentId::Codex));
    assert!(!conflicts(
        "/Applications/Safari.app/Contents/MacOS/Safari",
        AgentId::Codex
    ));
}

#[test]
fn claude_activation_does_not_require_closed_clients() {
    let result = activation_preflight(AgentId::Claude, |_| Err("Claude Code is running".into()));
    assert_eq!(result, Ok(()));
}

#[test]
fn codex_activation_still_requires_closed_clients() {
    let result = activation_preflight(AgentId::Codex, |provider| {
        assert_eq!(provider, AgentId::Codex);
        Err("Codex is running".into())
    });
    assert_eq!(result, Err("Codex is running".into()));
}

#[test]
fn codex_activation_proceeds_after_clients_exit() {
    assert_eq!(activation_preflight(AgentId::Codex, |_| Ok(())), Ok(()));
}
