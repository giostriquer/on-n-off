use super::*;

fn process(executable: &str, args: &str) -> Process {
    child("0", "0", executable, args)
}

fn child(pid: &str, parent: &str, executable: &str, args: &str) -> Process {
    Process {
        pid: pid.into(),
        parent: parent.into(),
        executable: executable.into(),
        args: args.into(),
    }
}

#[test]
fn recognizes_cli_launchers_after_node_flags_and_paths_with_spaces() {
    assert!(conflicts(&process("node", "node --some-flag --another-flag /Users/A User/tools/node_modules/@anthropic-ai/claude-code/cli.js"), AgentId::Claude));
    assert!(conflicts(
        &process("claude", "claude --resume"),
        AgentId::Claude
    ));
    let extension = "/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/openai.chatgpt/bin/macos-aarch64/codex";
    assert!(conflicts(
        &process(extension, &format!("{extension} app-server")),
        AgentId::Codex
    ));
    assert!(conflicts(&process("/Applications/ChatGPT.app/Contents/Resources/codex", "/Applications/ChatGPT.app/Contents/Resources/codex app-server --analytics-default-enabled"), AgentId::Codex));
    assert!(conflicts(&process("/Users/A User/.nvm/versions/node/v24.0.0/bin/node", "/Users/A User/.nvm/versions/node/v24.0.0/bin/node /Users/A User/.nvm/versions/node/v24.0.0/bin/codex exec"), AgentId::Codex));
    assert!(conflicts(&process("C:\\Program Files\\nodejs\\node.exe", "\"C:\\Program Files\\nodejs\\node.exe\" \"C:\\Users\\A User\\AppData\\Roaming\\npm\\node_modules\\@openai\\codex\\bin\\codex.js\" app-server"), AgentId::Codex));
    assert!(conflicts(&process("codex.exe", ""), AgentId::Codex));
    assert!(!conflicts(
        &process(
            "/Applications/Safari.app/Contents/MacOS/Safari",
            "/Applications/Safari.app/Contents/MacOS/Safari"
        ),
        AgentId::Codex
    ));
}

#[test]
fn ignores_helpers_under_a_framework_named_after_the_provider() {
    let handler = "/Applications/ChatGPT.app/Contents/Frameworks/Codex Framework.framework/Versions/152.0.7977.83/Helpers/browser_crashpad_handler";
    assert!(!conflicts(&process(handler, &format!("{handler} --monitor-self --database=/Users/me/Library/Application Support/com.openai.codex/Crashpad")), AgentId::Codex));
}

#[test]
fn ignores_processes_that_only_mention_the_provider_in_arguments() {
    let app = "/Applications/Acme Orchestrator.app/Contents/MacOS/Acme Orchestrator";
    assert!(!conflicts(
        &process(app, &format!("{app} --provider codex")),
        AgentId::Codex
    ));
    assert!(!conflicts(
        &process("node", "node /opt/acme/worker.js --provider codex"),
        AgentId::Codex
    ));
    assert!(!conflicts(
        &process("node", "node /opt/acme/worker --home /tmp/codex"),
        AgentId::Codex
    ));
    assert!(!conflicts(
        &process("/bin/zsh", "/bin/zsh -c codex exec"),
        AgentId::Codex
    ));
}

#[test]
fn reads_ps_columns_by_pid_and_keeps_spaces_in_executables() {
    let executables = "    1     0 /sbin/launchd\n  880     1 /Applications/Acme App.app/Contents/MacOS/Acme App\n 1262   880 codex\n";
    let args =
        "  880 /Applications/Acme App.app/Contents/MacOS/Acme App --flag\n    1 /sbin/launchd\n";
    let mut processes = ps_processes(executables, args);
    processes.sort_by(|a, b| a.executable.cmp(&b.executable));
    assert_eq!(
        processes,
        [
            child(
                "880",
                "1",
                "/Applications/Acme App.app/Contents/MacOS/Acme App",
                "/Applications/Acme App.app/Contents/MacOS/Acme App --flag"
            ),
            child("1", "0", "/sbin/launchd", "/sbin/launchd"),
            child("1262", "880", "codex", ""),
        ]
    );
}

#[test]
fn reads_cim_fields_and_falls_back_to_the_name_without_a_path() {
    let output = "7\t4\tC:\\Program Files\\Acme\\acme.exe\tacme.exe\t\"C:\\Program Files\\Acme\\acme.exe\" --codex\r\n9\t7\t\tcodex.exe\t\r\n\r\n";
    assert_eq!(
        cim_processes(output),
        [
            child(
                "7",
                "4",
                "C:\\Program Files\\Acme\\acme.exe",
                "\"C:\\Program Files\\Acme\\acme.exe\" --codex"
            ),
            child("9", "7", "codex.exe", ""),
        ]
    );
}

#[test]
fn names_each_client_after_the_app_it_runs_in_or_was_started_from() {
    let processes = [
        child("10", "1", "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT", ""),
        child("11", "10", "/Applications/ChatGPT.app/Contents/Resources/codex", "/Applications/ChatGPT.app/Contents/Resources/codex app-server"),
        child("12", "10", "/Applications/ChatGPT.app/Contents/Resources/codex", "/Applications/ChatGPT.app/Contents/Resources/codex app-server"),
        child("20", "1", "/Applications/Acme Studio.app/Contents/MacOS/Acme Studio", ""),
        child("21", "20", "/Users/me/.local/bin/codex", "/Users/me/.local/bin/codex app-server --stdio"),
        child("30", "1", "/Applications/Terminal.app/Contents/MacOS/Terminal", ""),
        child("31", "30", "-zsh", "-zsh"),
        child("32", "31", "node", "node /Users/me/.nvm/versions/node/v24.0.0/bin/codex"),
        child("40", "40", "codex", "codex exec"),
        child("50", "1", "/Applications/Acme Studio.app/Contents/Frameworks/Codex Framework.framework/Helpers/browser_crashpad_handler", ""),
    ];
    assert_eq!(
        clients(&processes, AgentId::Codex),
        [
            "Acme Studio (codex)",
            "ChatGPT",
            "Terminal (codex)",
            "codex"
        ]
    );
    let error = closed(&clients(&processes, AgentId::Codex)).unwrap_err();
    assert!(
        error.contains(": Acme Studio (codex), ChatGPT, Terminal (codex), codex."),
        "{error}"
    );
    assert_eq!(closed(&[]), Ok(()));
}

#[test]
fn claude_activation_is_never_blocked_by_running_clients() {
    assert_eq!(activation_blockers(AgentId::Claude), Ok(Vec::new()));
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
