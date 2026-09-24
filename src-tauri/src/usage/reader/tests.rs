use super::*;
use crate::paths::scratch_dir;

#[test]
fn list_finds_jsonl_by_mtime() {
    let root = scratch_dir("usage-list");
    let nested = root.join("a").join("b");
    std::fs::create_dir_all(&nested).unwrap();
    let path = nested.join("session.jsonl");
    std::fs::write(&path, "{}\n").unwrap();
    let inventory = inventory_transcript_files(&root, 0);
    assert!(inventory.complete);
    assert_eq!(inventory.files.len(), 1);
    assert_eq!(inventory.files[0].path, path);
    let _ = std::fs::remove_dir_all(&root);
}

/// Claude Code sets a replaced transcript aside as `<session>.jsonl.superseded-<ms>` rather than
/// overwriting it, and turns it dropped from the rewrite live only there.
#[test]
fn list_finds_superseded_transcripts_and_nothing_else() {
    let root = scratch_dir("usage-list-superseded");
    std::fs::create_dir_all(&root).unwrap();
    let names = [
        "session.jsonl",
        "session.jsonl.superseded-1758000000000",
        "session.jsonl.tmp",
        "notes.txt",
    ];
    for name in names {
        std::fs::write(root.join(name), "{}\n").unwrap();
    }

    let inventory = inventory_transcript_files(&root, 0);
    let mut found: Vec<String> = inventory
        .files
        .iter()
        .map(|file| {
            file.path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    found.sort();

    assert_eq!(
        found,
        ["session.jsonl", "session.jsonl.superseded-1758000000000"]
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn read_claude_file() {
    let root = scratch_dir("usage-read");
    let path = root.join("t.jsonl");
    let line = serde_json::json!({
        "type": "assistant",
        "timestamp": "2026-08-07T04:05:13.944Z",
        "sessionId": "s1",
        "message": {
            "id": "msg_1",
            "model": "claude-fable-5",
            "usage": {
                "input_tokens": 2,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
                "output_tokens": 10
            }
        }
    })
    .to_string();
    std::fs::write(&path, format!("{line}\n")).unwrap();
    let records = read_transcript_records(&path, UsageProvider::Claude).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].totals.output_tokens, 10);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn read_error_after_valid_line_rejects_partial_records() {
    let line = serde_json::json!({
        "type": "assistant",
        "timestamp": "2026-08-07T04:05:13.944Z",
        "sessionId": "s1",
        "message": {
            "id": "msg_1",
            "model": "claude-fable-5",
            "usage": {
                "input_tokens": 2,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
                "output_tokens": 10
            }
        }
    })
    .to_string();
    let lines = vec![
        Ok(line),
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "injected read failure",
        )),
    ]
    .into_iter();

    assert!(read_transcript_records_from_lines(lines, UsageProvider::Claude).is_none());
}
