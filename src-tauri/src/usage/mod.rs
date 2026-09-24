//! Usage scan: local Claude / Codex transcript analytics.

mod aggregate;
pub(crate) mod cache_io;
mod folding;
mod history;
mod pricing;
mod reader;
mod scan_cache;
mod source_index;
mod sources;
mod summary;
mod summary_cache;
mod transcripts;

pub use folding::{clear_usage_history, spawn_history_folding, usage_history_status};
pub use summary::read_summary;
