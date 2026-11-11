use std::io::Read;
use std::path::Path;

use chrono::DateTime;
use ruzstd::decoding::StreamingDecoder;
use sha1::{Digest, Sha1};

const ENTRY_MAGIC: u64 = 0xfcfb_6d1b_a772_5c30;
const HEADER_BYTES: usize = 24;
const KEY_LENGTH_AT: usize = 12;
const MAX_ENTRY_BYTES: u64 = 512 * 1024;
const MAX_BODY_BYTES: u64 = 256 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CachedResponse {
    pub(super) body: Vec<u8>,
    pub(super) seen_at_ms: i64,
}

pub(super) fn response(cache: &Path, key: &str) -> Option<CachedResponse> {
    let file = std::fs::File::open(cache.join(entry_file_name(key))).ok()?;
    let written = file.metadata().ok()?.modified().ok();
    let entry = capped(file, MAX_ENTRY_BYTES)?;
    let (body, response_headers) = body_of(&entry, key)?;
    let seen_at_ms = sent_at_ms(response_headers).or_else(|| {
        let written = written?.duration_since(std::time::UNIX_EPOCH).ok()?;
        i64::try_from(written.as_millis()).ok()
    })?;
    Some(CachedResponse { body, seen_at_ms })
}

fn entry_file_name(key: &str) -> String {
    let digest = Sha1::digest(key.as_bytes());
    let mut first = [0; 8];
    first.copy_from_slice(&digest[..8]);
    format!("{:016x}_0", u64::from_le_bytes(first))
}

fn capped(source: impl Read, most: u64) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    source.take(most + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= most).then_some(bytes)
}

fn body_of<'a>(entry: &'a [u8], key: &str) -> Option<(Vec<u8>, &'a [u8])> {
    let magic = u64::from_le_bytes(entry.get(..8)?.try_into().ok()?);
    if magic != ENTRY_MAGIC {
        return None;
    }
    let key_length = entry.get(KEY_LENGTH_AT..KEY_LENGTH_AT + 4)?;
    let key_length = usize::try_from(u32::from_le_bytes(key_length.try_into().ok()?)).ok()?;
    let key_end = HEADER_BYTES.checked_add(key_length)?;
    if entry.get(HEADER_BYTES..key_end)? != key.as_bytes() {
        return None;
    }
    let mut after_key = entry.get(key_end..)?;
    let body = capped(StreamingDecoder::new(&mut after_key).ok()?, MAX_BODY_BYTES)?;
    Some((body, after_key))
}

fn sent_at_ms(response_headers: &[u8]) -> Option<i64> {
    [b"\0date:".as_slice(), b"\0Date:".as_slice()]
        .into_iter()
        .find_map(|name| {
            let at = response_headers
                .windows(name.len())
                .position(|window| window == name)?;
            let value = response_headers.get(at + name.len()..)?;
            let value = value.split(|byte| *byte == 0).next()?;
            let sent =
                DateTime::parse_from_rfc2822(std::str::from_utf8(value).ok()?.trim()).ok()?;
            Some(sent.timestamp_millis())
        })
}

#[cfg(test)]
pub(super) mod tests;
