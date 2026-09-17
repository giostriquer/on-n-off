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

fn is_client(executable: &str, args: &str, provider: AgentId) -> bool {
    client_name(&process(executable, args), provider).is_some()
}

#[test]
fn recognizes_provider_executables_in_any_case() {
    let extension = "/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/openai.chatgpt/bin/macos-aarch64/codex";
    for (executable, args, provider) in [
        ("claude", "claude --resume", AgentId::Claude),
        (
            "/Applications/Claude.app/Contents/MacOS/Claude",
            "",
            AgentId::Claude,
        ),
        (extension, "app-server", AgentId::Codex),
        (
            "/Applications/ChatGPT.app/Contents/Resources/codex",
            "app-server",
            AgentId::Codex,
        ),
        ("Codex.exe", "", AgentId::Codex),
    ] {
        assert!(
            is_client(executable, args, provider),
            "{executable} should be a {provider:?} client"
        );
    }
}

#[test]
fn recognizes_scripts_a_runtime_launched_after_its_flags() {
    let home = crate::paths::scratch_dir("clients-script");
    let shim = home.join("A User").join("bin").join("codex");
    std::fs::create_dir_all(shim.parent().unwrap()).unwrap();
    std::fs::write(&shim, "").unwrap();
    let shim_args = format!("node {} exec", shim.to_string_lossy());
    for (executable, args, provider, case) in [
        ("node", "node --some-flag --another-flag /Users/A User/tools/node_modules/@anthropic-ai/claude-code/cli.js", AgentId::Claude, "package entry after flags"),
        ("node", shim_args.as_str(), AgentId::Codex, "npm shim whose path has a space"),
        ("node", "node ./bin/codex exec", AgentId::Codex, "relative shim"),
        ("C:\\Program Files\\nodejs\\node.exe", "\"C:\\Program Files\\nodejs\\node.exe\" --no-warnings \"C:\\Users\\A User\\AppData\\Roaming\\npm\\node_modules\\@openai\\codex\\bin\\codex.js\" app-server", AgentId::Codex, "quoted Windows command line"),
        ("C:\\Program Files\\nodejs\\node.exe", "node --no-warnings C:\\Users\\me\\AppData\\Roaming\\npm\\node_modules\\@openai\\codex\\bin\\codex.js", AgentId::Codex, "unquoted Windows command line"),
        ("/Applications/Acme Studio.app/Contents/Frameworks/Acme Studio Helper.app/Contents/MacOS/Acme Studio Helper", "/Users/me/.npm/lib/node_modules/@anthropic-ai/claude-code/cli.js --print", AgentId::Claude, "Electron helper running the package entry"),
    ] {
        assert!(is_client(executable, args, provider), "{case}");
    }
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn ignores_helpers_under_a_framework_named_after_the_provider() {
    for executable in [
        "/Applications/ChatGPT.app/Contents/Frameworks/Codex Framework.framework/Versions/152.0.7977.83/Helpers/browser_crashpad_handler",
        "/Applications/ChatGPT.app/Contents/Frameworks/Codex Framework.framework/Versions/152.0.7977.83/Helpers/Codex (Renderer).app/Contents/MacOS/Codex (Renderer)",
        "/Applications/ChatGPT.app/Contents/Resources/codex-code-mode-host",
    ] {
        let args = format!("{executable} --database=/Users/me/Library/Application Support/com.openai.codex/Crashpad");
        assert!(!is_client(executable, &args, AgentId::Codex), "{executable}");
    }
}

#[test]
fn ignores_processes_that_only_mention_the_provider_in_arguments() {
    let home = crate::paths::scratch_dir("clients-mention");
    let tool = home.join("tool");
    std::fs::write(&tool, "").unwrap();
    let checkout = home.join("src").join("codex");
    std::fs::create_dir_all(&checkout).unwrap();
    let positional = format!(
        "node {} {}",
        tool.to_string_lossy(),
        checkout.to_string_lossy()
    );
    let app = "/Applications/Acme Orchestrator.app/Contents/MacOS/Acme Orchestrator";
    let flag = format!("{app} --provider codex");
    for (executable, args, case) in [
        (app, flag.as_str(), "flag value"),
        (
            "node",
            "node /opt/acme/worker.js --provider codex",
            "after a script",
        ),
        (
            "node",
            "node /opt/acme/worker --home /tmp/codex",
            "after an extensionless script",
        ),
        ("node", positional.as_str(), "positional path after a file"),
        (
            "bun",
            "bun run dev /Users/me/src/codex",
            "runtime subcommand",
        ),
        (
            "bun",
            "bun /opt/acme/watch.ts /Users/me/src/codex",
            "after a TypeScript script",
        ),
        ("deno", "deno run main.ts /tmp/codex", "after a deno script"),
        ("/bin/zsh", "/bin/zsh -c codex exec", "shell command"),
    ] {
        assert!(!is_client(executable, args, AgentId::Codex), "{case}");
    }
    let _ = std::fs::remove_dir_all(home);
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
        child(
            "10",
            "1",
            "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
            "",
        ),
        child(
            "11",
            "10",
            "/Applications/ChatGPT.app/Contents/Resources/codex",
            "app-server",
        ),
        child(
            "12",
            "10",
            "/Applications/ChatGPT.app/Contents/Resources/codex",
            "app-server",
        ),
        child(
            "20",
            "1",
            "/Applications/Acme Studio.app/Contents/MacOS/Acme Studio",
            "",
        ),
        child(
            "21",
            "20",
            "/Users/me/.local/bin/codex",
            "app-server --stdio",
        ),
        child(
            "30",
            "1",
            "/Applications/Terminal.app/Contents/MacOS/Terminal",
            "",
        ),
        child("31", "30", "-zsh", "-zsh"),
        child("32", "31", "node", "node ./bin/codex"),
        child("40", "40", "codex", "codex exec"),
        child(
            "50",
            "51",
            "C:\\Users\\me\\AppData\\Roaming\\npm\\codex.exe",
            "",
        ),
        child("51", "50", "C:\\Windows\\explorer.exe", ""),
    ];
    assert_eq!(
        clients(&processes, AgentId::Codex, "99"),
        [
            "Acme Studio (codex)",
            "ChatGPT",
            "Terminal (codex)",
            "codex",
            "codex.exe"
        ]
    );
}

#[test]
fn leaves_out_the_clients_on_n_off_started_itself() {
    let processes = [
        child(
            "99",
            "1",
            "/Applications/on-n-off.app/Contents/MacOS/on-n-off",
            "",
        ),
        child("100", "99", "node", "node ./bin/codex app-server"),
        child("101", "100", "/Users/me/.local/bin/codex", "app-server"),
    ];
    assert!(clients(&processes, AgentId::Codex, "99").is_empty());
}

#[test]
fn only_codex_scans_before_a_switch_and_a_failed_scan_refuses() {
    let claude = blockers(AgentId::Claude, || panic!("Claude switches never scan"));
    assert_eq!(claude, Ok(Vec::new()));
    let codex = blockers(AgentId::Codex, || {
        Ok(vec![child("5", "1", "codex", "codex")])
    });
    assert_eq!(codex, Ok(vec!["codex".to_owned()]));
    let failed = blockers(AgentId::Codex, || Err("ps failed".into())).unwrap_err();
    assert!(
        failed.starts_with("Could not check running clients."),
        "{failed}"
    );
}

#[test]
fn a_refusal_names_every_running_client() {
    let error = closed(&["Acme Studio (codex)".into(), "ChatGPT".into()]).unwrap_err();
    assert!(
        error.contains(": Acme Studio (codex), ChatGPT. on-n-off will not stop them"),
        "{error}"
    );
    assert_eq!(closed(&[]), Ok(()));
}
