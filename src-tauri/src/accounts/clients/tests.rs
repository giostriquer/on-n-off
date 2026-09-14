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
