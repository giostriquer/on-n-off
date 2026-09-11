use super::sha256_hex;

/// The published SHA-256 vectors. Nothing else in the crate pins a digest: both callers only ever
/// compare one of these strings against another, so a broken encoder would stay invisible while
/// every stored filename and every recorded item hash silently changed.
#[test]
fn matches_the_published_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// A byte under 0x10 has to keep its leading zero: `{:x}` without the width would shorten the
/// string for those inputs and collide two different inputs onto one name.
#[test]
fn pads_every_byte_to_two_digits() {
    let mut saw_low_byte = false;
    for seed in 0u8..32 {
        let hex = sha256_hex(&[seed]);
        assert_eq!(hex.len(), 64, "{seed}: {hex}");
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{seed}: {hex}"
        );
        saw_low_byte |= hex
            .as_bytes()
            .chunks(2)
            .any(|pair| pair[0] == b'0' && pair[1] != b'0');
    }
    // Without one digest carrying a byte below 0x10, the length assertion above proves nothing.
    assert!(saw_low_byte, "no digest exercised the zero padding");
}
