use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::paths::user_home;

use super::files::{FoldChecks, UsageFiles};

const FIRST_FOLD_DELAY: Duration = Duration::from_secs(90);
const FOLD_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

pub fn spawn_history_folding() {
    let spawned = std::thread::Builder::new()
        .name("usage-history".into())
        .spawn(|| {
            std::thread::sleep(FIRST_FOLD_DELAY);
            let mut checks = FoldChecks::default();
            loop {
                let checked =
                    user_home().and_then(|home| UsageFiles::of(&home).fold(now_ms(), &mut checks));
                if let Err(error) = checked {
                    eprintln!("usage history: {}", error.message);
                }
                std::thread::sleep(FOLD_CHECK_INTERVAL);
            }
        });
    if let Err(error) = spawned {
        eprintln!("usage history: could not start folding: {error}");
    }
}
