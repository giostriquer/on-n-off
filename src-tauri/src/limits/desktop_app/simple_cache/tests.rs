use std::path::{Path, PathBuf};

use ruzstd::encoding::{compress_to_vec, CompressionLevel};

use super::*;
use crate::paths;

const KEY: &str = "1/0/https://claude.ai/api/organizations/00000000-0000-4000-8000-00000000000a/usage?cedar_ember=1&skip_spend=1";
const OTHER_KEY: &str = "1/0/https://claude.ai/api/organizations/00000000-0000-4000-8000-00000000000b/usage?cedar_ember=1&skip_spend=1";
pub(in crate::limits::desktop_app) const SENT: &str = "Tue, 10 Nov 2026 20:49:33 GMT";
const SENT_MS: i64 = 1_794_343_773_000;

pub(in crate::limits::desktop_app) fn entry_of(
    key: &str,
    body: &[u8],
    date: Option<&str>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xfcfb_6d1b_a772_5c30_u64.to_le_bytes());
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(key.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(key.as_bytes());
    bytes.extend_from_slice(&compress_to_vec(body, CompressionLevel::Fastest));
    bytes.extend_from_slice(b"\x00\x00eof-record\x00HTTP/1.1 200\x00content-encoding:zstd\x00");
    if let Some(date) = date {
        bytes.extend_from_slice(format!("date:{date}\x00").as_bytes());
    }
    bytes.extend_from_slice(b"content-type:application/json\x00\x00");
    bytes
}

pub(in crate::limits::desktop_app) fn cached(cache: &Path, key: &str, bytes: &[u8]) -> PathBuf {
    std::fs::create_dir_all(cache).unwrap();
    let file = cache.join(entry_file_name(key));
    std::fs::write(&file, bytes).unwrap();
    file
}

fn cache_holding(bytes: &[u8]) -> PathBuf {
    let cache = paths::scratch_dir("simple-cache");
    cached(&cache, KEY, bytes);
    cache
}

#[test]
fn an_entrys_file_is_named_by_the_first_eight_bytes_of_its_keys_sha1() {
    assert_eq!(entry_file_name(KEY), "c69a59c27f4c6c21_0");
}

#[test]
fn a_cached_response_gives_its_decoded_body_and_when_the_server_sent_it() {
    let cache = cache_holding(&entry_of(KEY, br#"{"answer":42}"#, Some(SENT)));

    assert_eq!(
        response(&cache, KEY),
        Some(CachedResponse {
            body: br#"{"answer":42}"#.to_vec(),
            seen_at_ms: SENT_MS,
        })
    );
}

#[test]
fn a_date_header_written_in_title_case_is_read_too() {
    let mut entry = entry_of(KEY, b"{}", None);
    entry.extend_from_slice(format!("\x00Date:{SENT}\x00").as_bytes());

    assert_eq!(
        response(&cache_holding(&entry), KEY).map(|cached| cached.seen_at_ms),
        Some(SENT_MS)
    );
}

#[test]
fn a_response_without_a_readable_date_was_seen_when_its_entry_was_written() {
    for (date, why) in [
        (None, "no date header"),
        (Some("soon"), "a date that is no date"),
    ] {
        let cache = paths::scratch_dir("simple-cache");
        let file = cached(&cache, KEY, &entry_of(KEY, b"{}", date));
        let written = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_794_300_000);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(written)
            .unwrap();

        assert_eq!(
            response(&cache, KEY).map(|cached| cached.seen_at_ms),
            Some(1_794_300_000_000),
            "{why}"
        );
    }
}

#[test]
fn an_entry_that_is_not_this_keys_response_gives_nothing() {
    let good = entry_of(KEY, b"{}", Some(SENT));
    let mut wrong_magic = good.clone();
    wrong_magic[0] ^= 0xff;
    let mut long_key = good.clone();
    long_key[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut short_key = good.clone();
    short_key[12..16].copy_from_slice(&u32::try_from(KEY.len() - 1).unwrap().to_le_bytes());
    let mut oversized = good.clone();
    oversized.resize(512 * 1024 + 1, 0);

    for (bytes, why) in [
        (
            entry_of(OTHER_KEY, b"{}", Some(SENT)),
            "another key's response under this name",
        ),
        (wrong_magic, "not a cache entry"),
        (long_key, "a key longer than the file"),
        (short_key, "a stored key that is only a prefix of this one"),
        (good[..good.len() / 2].to_vec(), "cut off inside the body"),
        (oversized, "larger than the entry limit"),
        (
            [&good[..24 + KEY.len()], b"not zstd".as_slice()].concat(),
            "a body that is not zstd",
        ),
        (good[..20].to_vec(), "shorter than the header"),
        (Vec::new(), "empty"),
    ] {
        assert_eq!(response(&cache_holding(&bytes), KEY), None, "{why}");
    }

    let cache = cache_holding(&good);
    assert!(
        response(&cache, KEY).is_some(),
        "the entry itself is readable"
    );
    assert_eq!(response(&cache, OTHER_KEY), None, "no entry for that key");
    assert_eq!(
        response(&cache.join("no-such-cache"), KEY),
        None,
        "no cache"
    );
}

#[test]
fn an_entry_at_the_size_limit_is_read_and_a_body_past_its_limit_is_not() {
    let mut at_limit = entry_of(KEY, b"{}", Some(SENT));
    at_limit.resize(512 * 1024, 0);
    assert!(response(&cache_holding(&at_limit), KEY).is_some());

    let body = vec![b'x'; 256 * 1024];
    let fits = entry_of(KEY, &body, Some(SENT));
    assert_eq!(
        response(&cache_holding(&fits), KEY).map(|cached| cached.body.len()),
        Some(256 * 1024)
    );
    let too_long = entry_of(KEY, &[body, vec![b'x']].concat(), Some(SENT));
    assert_eq!(response(&cache_holding(&too_long), KEY), None);
}

#[test]
fn an_entry_cut_off_inside_its_body_gives_nothing_and_one_cut_after_it_gives_the_body() {
    let body = br#"{"answer":42}"#;
    let whole = entry_of(KEY, body, Some(SENT));
    let body_starts = 24 + KEY.len();
    let body_ends = body_starts + compress_to_vec(body.as_slice(), CompressionLevel::Fastest).len();
    let cache = paths::scratch_dir("simple-cache");

    let read: Vec<Option<Vec<u8>>> = (0..=whole.len())
        .map(|length| {
            cached(&cache, KEY, &whole[..length]);
            response(&cache, KEY).map(|cached| cached.body)
        })
        .collect();

    let first_read = read.iter().position(Option::is_some).unwrap();
    assert!(
        first_read > body_starts && first_read <= body_ends,
        "first readable at {first_read}, body at {body_starts}..{body_ends}"
    );
    assert!(read[..first_read].iter().all(Option::is_none));
    assert!(read[first_read..]
        .iter()
        .all(|read| read.as_deref() == Some(body.as_slice())));
}
