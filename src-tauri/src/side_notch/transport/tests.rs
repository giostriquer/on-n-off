use super::*;

#[test]
fn shutdown_reaps_an_unresponsive_child_without_the_supervisor() {
    // This child does not read stdin or cooperate with graceful shutdown.
    let child = Arc::new(Mutex::new(
        Command::new("/bin/sleep").arg("60").spawn().unwrap(),
    ));
    let lifetime = Lifetime::default();
    lifetime.0.lock().unwrap().child = Some(child.clone());
    lifetime.shutdown();
    let exited = child.lock().unwrap().try_wait().unwrap().is_some();
    if !exited {
        let mut child = child.lock().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
    }
    assert!(
        exited,
        "shutdown returned while the unresponsive helper was alive"
    );
    assert!(
        lifetime.connect().is_err(),
        "quit must prevent a later spawn"
    );
}
