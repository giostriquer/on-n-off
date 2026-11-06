use std::cell::Cell;

thread_local! {
    static TRANSCRIPT_PARSE_COUNT: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn note_transcript_parse() {
    TRANSCRIPT_PARSE_COUNT.set(TRANSCRIPT_PARSE_COUNT.get() + 1);
}

pub(crate) fn reset_transcript_parse_count() {
    TRANSCRIPT_PARSE_COUNT.set(0);
}

pub(crate) fn transcript_parse_count() -> usize {
    TRANSCRIPT_PARSE_COUNT.get()
}

thread_local! {
    static LIVE_TRANSCRIPT: std::cell::RefCell<Option<(String, String)>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) fn with_live_transcript<R>(
    path: &std::path::Path,
    line: &str,
    f: impl FnOnce() -> R,
) -> R {
    LIVE_TRANSCRIPT.set(Some((super::normalize_path(path), line.to_string())));
    let result = f();
    LIVE_TRANSCRIPT.set(None);
    result
}

pub(super) fn keep_writing_if_live(path: &std::path::Path) {
    LIVE_TRANSCRIPT.with_borrow(|live| {
        if let Some((live_path, line)) = live {
            if *live_path == super::normalize_path(path) {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
                writeln!(file, "{line}").unwrap();
            }
        }
    });
}
