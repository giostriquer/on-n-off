use super::*;

#[test]
fn unpack_reads_commit_sha_and_strips_top_level_folder() {
    let bytes = tarball_with_root(
        "skills-3f2a1b",
        Some(SHA_A),
        &[("README.md", "hello"), ("skills/tdd/SKILL.md", "body")],
    );
    let tarball = fetch::unpack(&bytes).unwrap();
    assert_eq!(tarball.commit_sha, SHA_A);
    assert_eq!(tarball.files.get("README.md").unwrap(), b"hello");
    assert_eq!(tarball.files.get("skills/tdd/SKILL.md").unwrap(), b"body");
    assert!(!tarball.files.keys().any(|k| k.starts_with("skills-3f2a1b")));
}

#[test]
fn unpack_rejects_missing_commit_sha() {
    let bytes = tarball_with_root("r-x", None, &[("a", "b")]);
    let error = fetch::unpack(&bytes).unwrap_err();
    assert!(error.message.contains("commit"), "{error:?}");
}

#[test]
fn unpack_rejects_path_traversal() {
    for evil in ["../../evil.md", r"skills\..\..\evil.md", "C:/evil.md"] {
        let bytes = tarball_with_root("r-x", Some(SHA_A), &[(evil, "x"), ("ok.md", "y")]);
        let error = fetch::unpack(&bytes).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Parse, "{evil}");
    }
    assert!(write::normalize_upstream_path("C:/x").is_err());
}

#[test]
fn unpack_rejects_a_single_file_over_the_per_file_cap() {
    // Zeros compress ~1000:1, so the body stays far below the compressed cap.
    let big = "0".repeat(20 * 1024 * 1024 + 1);
    let bytes = tarball_with_root("r-x", Some(SHA_A), &[("big.bin", big.as_str())]);
    let error = fetch::unpack(&bytes).unwrap_err();
    assert!(error.message.contains("too large"), "{error:?}");
}

#[test]
fn unpack_skips_symlinks_and_directories() {
    let mut builder = tar::Builder::new(Vec::new());
    let mut dir = tar::Header::new_ustar();
    dir.set_entry_type(tar::EntryType::Directory);
    dir.set_size(0);
    dir.set_mode(0o755);
    dir.set_cksum();
    builder
        .append_data(&mut dir, "r-x/skills/", &[][..])
        .unwrap();
    let mut link = tar::Header::new_ustar();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_size(0);
    link.set_mode(0o644);
    link.set_link_name("../../etc/passwd").unwrap();
    link.set_cksum();
    builder
        .append_data(&mut link, "r-x/skills/link", &[][..])
        .unwrap();
    let mut file = tar::Header::new_ustar();
    file.set_size(1);
    file.set_mode(0o644);
    file.set_cksum();
    builder
        .append_data(&mut file, "r-x/skills/a.md", &b"a"[..])
        .unwrap();
    let mut pax = tar::Header::new_ustar();
    let record = format!("{} comment={SHA_A}\n", 9 + SHA_A.len() + 1 + 2);
    pax.set_entry_type(tar::EntryType::XGlobalHeader);
    pax.set_size(record.len() as u64);
    pax.set_cksum();
    let mut all = tar::Builder::new(Vec::new());
    all.append_data(&mut pax, "pax_global_header", record.as_bytes())
        .unwrap();
    let rest = builder.into_inner().unwrap();
    let mut merged = all.into_inner().unwrap();
    // Drop the trailing 1024-byte end-of-archive marker of the first builder before appending.
    merged.truncate(merged.len() - 1024);
    merged.extend_from_slice(&rest);
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&merged).unwrap();
    let bytes = gz.finish().unwrap();

    let tarball = fetch::unpack(&bytes).unwrap();
    assert_eq!(tarball.commit_sha, SHA_A);
    assert_eq!(tarball.files.len(), 1);
    assert!(tarball.files.contains_key("skills/a.md"));
}

#[test]
fn unpack_rejects_oversized_bodies() {
    let too_big = vec![0u8; fetch::MAX_TARBALL_BYTES + 1];
    let error = fetch::unpack(&too_big).unwrap_err();
    assert!(error.message.contains("large"), "{error:?}");
}

#[test]
fn upstream_sha_validates_and_trims() {
    let fetcher = FakeFetcher::default();
    let url = fetch::commit_sha_url("o", "r", "main");
    fetcher.route(&url, format!("{SHA_A}\n").into_bytes());
    assert_eq!(
        fetch::upstream_sha(&fetcher, "o", "r", "main").unwrap(),
        SHA_A
    );
    fetcher.route(&url, b"<html>rate limited</html>".to_vec());
    assert!(fetch::upstream_sha(&fetcher, "o", "r", "main").is_err());
}
