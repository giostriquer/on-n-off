use super::sha256_hex;

/// The published SHA-256 vectors. Nothing else in the crate pins a digest: both callers only ever
/// compare one of these strings against another, so a broken encoder would stay invisible while
/// every stored filename and every recorded item hash silently changed. The "abc" digest carries
/// bytes below 0x10 (`01`, `03`, `00`), so an encoder that drops their leading zero fails it too.
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
