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
