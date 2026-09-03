use super::*;
use crate::paths::scratch_dir;

#[test]
fn failed_replacement_preserves_existing_cache() {
    let root = scratch_dir("usage-atomic-write-failure");
    let path = root.join("cache.json");
    fs::write(&path, r#"{"version":1}"#).unwrap();

    let result = atomic_write_with(&path, r#"{"version":2}"#, || {
        Err(io::Error::other("injected replacement failure"))
    });
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"version":1}"#);
    assert!(!temp_sibling(&path).exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn readers_only_observe_complete_documents_during_replacement() {
    let root = scratch_dir("usage-atomic-write-readers");
    let path = root.join("cache.json");
    fs::write(&path, r#"{"generation":0}"#).unwrap();
    let reader_path = path.clone();
    let reader = std::thread::spawn(move || {
        for _ in 0..500 {
            let raw = fs::read_to_string(&reader_path).unwrap();
            assert!(serde_json::from_str::<serde_json::Value>(&raw).is_ok());
        }
    });
    for generation in 1..=100 {
        atomic_write(&path, &format!(r#"{{"generation":{generation}}}"#)).unwrap();
    }
    reader.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn concurrent_writers_use_independent_temporary_files() {
    let root = scratch_dir("usage-atomic-write-concurrent");
    let path = root.join("cache.json");
    let first_path = path.clone();
    let (ready_sender, ready_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let first = std::thread::spawn(move || {
        atomic_write_with(&first_path, r#"{"writer":1}"#, || {
            ready_sender.send(()).unwrap();
            release_receiver.recv().unwrap();
            Ok(())
        })
    });
    ready_receiver.recv().unwrap();

    let second = atomic_write(&path, r#"{"writer":2}"#);
    release_sender.send(()).unwrap();
    let first = first.join().unwrap();

    assert!(second.is_ok(), "second writer failed: {second:?}");
    assert!(first.is_ok(), "first writer failed: {first:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"writer":1}"#);
    let _ = fs::remove_dir_all(root);
}
