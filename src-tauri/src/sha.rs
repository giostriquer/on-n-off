//! SHA-256 as lowercase hex.
//!
//! `sha2` 0.11 returns a plain byte array whose type no longer implements `LowerHex`, so the
//! encoding lives here rather than in a `{:x}` format string at each call site.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests;
