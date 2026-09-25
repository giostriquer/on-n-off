use super::*;

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0, "hex comes in whole bytes: {hex}");
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect()
}

fn hex_argument(command: &str) -> &str {
    command
        .rsplit_once("-X \"")
        .unwrap()
        .1
        .trim_end()
        .trim_end_matches('"')
}

/// The secret reaches `security` on stdin, hex-encoded and decodable back to the same bytes.
/// Nothing about it can then be read out of the process table, and no quoting in the JSON can
/// escape into the command.
#[test]
fn the_write_hex_encodes_the_secret_instead_of_quoting_it() {
    let service = crate::accounts::claude_store::CLAUDE_KEYCHAIN_SERVICE;
    let secret = br#"{"a":"b\"c"}"#;
    let command = write_command(service, "me", secret).unwrap();
    assert!(
        command.starts_with(&format!(
            r#"add-generic-password -U -a "me" -s "{service}" -X ""#
        )),
        "{command}"
    );
    assert!(
        command.ends_with("\"\n"),
        "one command, one line: {command:?}"
    );
    assert_eq!(hex_to_bytes(hex_argument(&command)), secret);
    assert!(
        !command.contains(r#"b\"c"#),
        "no JSON reaches the command line"
    );

    let odd = [0u8, 0xff, b'"', b'\n', 0x7f];
    assert_eq!(
        hex_to_bytes(hex_argument(&write_command("s", "a", &odd).unwrap())),
        odd,
        "bytes outside ASCII and the quote itself survive"
    );
    assert!(write_command("s", "a", b"").unwrap().ends_with("-X \"\"\n"));
}

/// A delete names the account as well as the service, or Codex's other homes are fair game.
#[test]
fn the_delete_names_the_exact_entry() {
    assert_eq!(
        delete_command("Codex Auth", "cli|0123456789abcdef").unwrap(),
        "delete-generic-password -a \"cli|0123456789abcdef\" -s \"Codex Auth\"\n"
    );
}

/// An identifier the command syntax cannot carry is refused, never escaped by guesswork.
#[test]
fn an_identifier_that_needs_escaping_is_refused() {
    for bad in ["", "a\"b", "a\\b", "a\nb", "a\rb"] {
        assert!(
            write_command(bad, "a", b"x").is_err(),
            "service {bad:?} must be refused"
        );
        assert!(
            delete_command("s", bad).is_err(),
            "account {bad:?} must be refused"
        );
        assert!(
            delete_command(bad, "a").is_err(),
            "service {bad:?} must be refused on delete too"
        );
    }
    assert!(delete_command("Claude Code-credentials", "cli|x y").is_ok());
}

/// `security`'s answers, mapped before the caller names the operation: success is silent, a
/// refusal keeps the tool's words on one line, silence gets a reason of its own, and a prompt
/// nobody answered is named as such.
#[test]
fn the_tool_outcome_is_read_into_one_reason() {
    let exited = |success: bool, stderr: &str| CommandOutcome::Exited {
        success,
        stdout: String::new(),
        stderr: stderr.to_string(),
    };
    assert_eq!(interpret(exited(true, "ignored on success")), Ok(()));
    assert_eq!(
        interpret(exited(
            false,
            "security: first line.\n\n  delete-generic-password: returned -25300\n"
        )),
        Err("security: first line. delete-generic-password: returned -25300".to_string())
    );
    assert_eq!(
        interpret(exited(false, "  \n")),
        Err("security exited with an error".to_string())
    );
    assert_eq!(
        interpret(CommandOutcome::TimedOut),
        Err("not answered in time".to_string())
    );
}

/// Both spellings of "no such item" count, and nothing else does.
#[test]
fn only_a_missing_item_counts_as_not_found() {
    assert!(not_found(
        "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain. delete-generic-password: returned -25300"
    ));
    assert!(not_found("returned -25300"));
    assert!(!not_found("User interaction is not allowed."));
    assert!(!not_found(""));
}

/// `find-generic-password -w`'s answers: the secret, trimmed; no item, in either form; or a failure
/// that says how the user can unblock it.
#[test]
fn a_password_read_is_mapped_to_the_secret_no_item_or_why_not() {
    assert_eq!(
        interpret_password(true, "  {\"a\":1}\n", ""),
        Ok(Some("{\"a\":1}".to_string()))
    );
    assert_eq!(interpret_password(true, "\n", ""), Ok(None));
    assert_eq!(
        interpret_password(
            false,
            "",
            "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain."
        ),
        Ok(None)
    );
    let denied = interpret_password(
        false,
        "",
        "security: SecKeychainItemCopyContent: User canceled the operation.",
    )
    .unwrap_err();
    assert!(denied.contains("User canceled"), "{denied}");
    assert!(denied.contains("click Allow"), "{denied}");
}

/// The production entry points, driven through the test runner: what they send is exactly the
/// command the builders produce, a missing item is a completed delete, and any other refusal is
/// wrapped as a terminated sentence naming the operation.
#[cfg(target_os = "macos")]
#[test]
fn write_and_delete_send_their_commands_and_read_the_answers() {
    let ok: Runner = |_| CommandOutcome::Exited {
        success: true,
        stdout: String::new(),
        stderr: String::new(),
    };
    let (result, sent) = with_test_runner(ok, || write("svc", "acct", b"{}"));
    assert_eq!(result, Ok(()));
    assert_eq!(sent, vec![write_command("svc", "acct", b"{}").unwrap()]);

    let missing: Runner = |_| CommandOutcome::Exited {
        success: false,
        stdout: String::new(),
        stderr: "delete-generic-password: returned -25300".to_string(),
    };
    let (result, sent) = with_test_runner(missing, || delete("svc", "acct"));
    assert_eq!(result, Ok(()), "an item already gone is deleted");
    assert_eq!(sent, vec![delete_command("svc", "acct").unwrap()]);

    let refused: Runner = |_| CommandOutcome::Exited {
        success: false,
        stdout: String::new(),
        stderr: "User interaction is not allowed.\n".to_string(),
    };
    let (result, _) = with_test_runner(refused, || delete("svc", "acct"));
    assert_eq!(
        result,
        Err("Keychain delete failed (User interaction is not allowed).".to_string())
    );
    let (result, _) =
        with_test_runner(|_| CommandOutcome::TimedOut, || write("svc", "acct", b"{}"));
    assert_eq!(
        result,
        Err("Keychain write failed (not answered in time).".to_string())
    );
    let (result, sent) = with_test_runner(ok, || write("svc", "a\"b", b"{}"));
    assert!(result
        .unwrap_err()
        .starts_with("Keychain write failed (Keychain identifier"));
    assert!(
        sent.is_empty(),
        "nothing is sent for an identifier that was refused"
    );
    let (result, sent) = with_test_runner(missing, || delete("svc", "a\"b"));
    assert!(result
        .unwrap_err()
        .starts_with("Keychain delete failed (Keychain identifier"));
    assert!(
        sent.is_empty(),
        "a refused identifier is reported before the tool is asked, never read as a missing item"
    );
}

/// The production write and delete, driven against a throwaway entry of our own so no real login
/// is at stake. The entry is created the way Claude Code creates its own — by `security` — and the
/// check that matters is the access list afterwards: only the tool's identity may be on it. An
/// in-process write would have needed this test binary to be allowed, and would have left its hash
/// on the item's partition list, which is the state that made every account switch prompt.
///
/// `cargo test --manifest-path src-tauri/Cargo.toml rehearse_the_keychain_write -- --ignored`
#[cfg(target_os = "macos")]
#[test]
#[ignore = "writes a throwaway Keychain entry; not part of CI"]
fn rehearse_the_keychain_write_and_delete_through_security() {
    with_real_keychain(rehearse_the_keychain_write_and_delete);
}

#[cfg(target_os = "macos")]
fn rehearse_the_keychain_write_and_delete() {
    use std::process::Command;

    let entry = ThrowawayEntry {
        service: "on-n-off keychain write rehearsal",
        account: "on-n-off-test",
    };
    let (service, account) = (entry.service, entry.account);
    let security = |args: &[&str]| {
        Command::new("/usr/bin/security")
            .args(args)
            .output()
            .unwrap()
    };
    let read = || {
        let output = security(&["find-generic-password", "-a", account, "-s", service, "-w"]);
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    // `dump-keychain -a` prints every item as a `keychain: "<path>"` block whose attributes
    // include `"svce"<blob>="<service>"` and whose `access:` entries list each trusted
    // application's path and code requirement (`cdhash H"…"` for an ad-hoc-signed binary) plus a
    // `partition_id` entry naming `cdhash:…` / `apple-tool:` partitions. That is the shape matched
    // here, as of macOS 26.
    let access_list = || {
        let dump = security(&["dump-keychain", "-a"]);
        String::from_utf8_lossy(&dump.stdout)
            .split("keychain: ")
            .find(|item| item.contains(&format!("\"svce\"<blob>=\"{service}\"")))
            .map(str::to_owned)
            .expect("the rehearsal entry is in the login keychain")
    };

    // A quote and a space in the payload: the two things hex encoding exists to survive.
    let first = r#"{"claudeAiOauth":{"accessToken":"one","note":"a \"quoted\" word"}}"#;
    let second = r#"{"claudeAiOauth":{"accessToken":"two"}}"#;
    write(service, account, first.as_bytes()).unwrap();
    assert_eq!(
        read().as_deref(),
        Some(first),
        "the entry round-trips byte for byte"
    );
    write(service, account, second.as_bytes()).unwrap();
    assert_eq!(
        read().as_deref(),
        Some(second),
        "-U replaces an entry that already exists rather than failing or duplicating it"
    );

    let access = access_list();
    assert!(
        access.contains("/usr/bin/security"),
        "the tool is the identity on the item:\n{access}"
    );
    assert!(
        !access.contains("cdhash:") && !access.contains("target/debug"),
        "this process never touched the item with its own identity:\n{access}"
    );

    // The account lookup, against output `security` really produced rather than a fixture.
    let attributes = security(&["find-generic-password", "-s", service]);
    assert_eq!(
        crate::accounts::claude_store::parse_keychain_account(&String::from_utf8_lossy(
            &attributes.stdout
        ))
        .as_deref(),
        Some(account),
        "the parser reads `security`'s own output, not just a hand-written fixture"
    );

    delete(service, account).unwrap();
    assert_eq!(read(), None, "the delete removes the entry");
    delete(service, account).unwrap();
    assert_eq!(
        read(),
        None,
        "removing an entry already gone is not an error"
    );
    drop(entry);
}
