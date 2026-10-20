use super::*;
use crate::accounts::transaction::NativeGuard;
use std::sync::Arc;

#[derive(Default)]
struct HomeState {
    stamp: Mutex<String>,
    reads: Mutex<usize>,
}

struct SecretsUntouched(Arc<HomeState>);

impl Home for SecretsUntouched {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        panic!("a poll took a home's locks")
    }
    fn read(&self) -> Result<Option<Login>, String> {
        panic!("read a home's login secret")
    }
    fn identify(&self, _: &Login) -> Result<model::Identity, String> {
        panic!("identified a home's login")
    }
    fn put(&self, _: &Login, _: &dyn NativeGuard) -> Result<Option<Login>, String> {
        panic!("wrote a home's login")
    }
    fn clear(&self, _: &dyn NativeGuard) -> Result<(), String> {
        panic!("emptied a home")
    }
    fn read_usage(&self, _: &model::Identity) -> Result<Accepted, ReadFailure> {
        *self.0.reads.lock().unwrap() += 1;
        Err(ReadFailure::LoginRefused)
    }
    fn login_stamp(&self) -> Result<String, String> {
        Ok(self.0.stamp.lock().unwrap().clone())
    }
    fn sign_in_deadline(&self) -> Option<model::SignInDeadline> {
        panic!("read a home's sign-in deadline")
    }
    fn delete(&self, _: Box<dyn NativeGuard>) -> Result<(), String> {
        panic!("deleted a home")
    }
}

#[test]
fn a_homes_generation_is_its_login_stamp_and_never_its_login() {
    let home = tempfile::tempdir().unwrap();
    let mut p = profile();
    let id = uuid::Uuid::new_v4().to_string();
    p.home = Some(id.clone());
    p.login = None;
    rewrite(home.path(), |db| db.profiles.push(p.clone()));
    let state = Arc::new(HomeState::default());
    *state.stamp.lock().unwrap() = "item written at 10:00".into();
    let kept = || Kept::Home(&id, Box::new(SecretsUntouched(state.clone())));
    let poll_once = |force| {
        poll(
            home.path(),
            &p,
            &kept(),
            &ticket(home.path()),
            force,
            &|| Ok(open(home.path())),
            &|_| panic!("a home is never fetched from the vault"),
        )
    };
    let reads = || *state.reads.lock().unwrap();

    assert!(poll_once(false).unwrap().is_err());
    assert_eq!(
        parked(home.path(), &p, Some(&kept())),
        Some(Parked::Refused)
    );
    super::elapse(home.path());
    assert!(
        poll_once(true).unwrap().is_err(),
        "held, its verdict replayed"
    );
    assert_eq!(reads(), 1, "parked until the login changes");

    *state.stamp.lock().unwrap() = "item written at 10:05".into();
    assert_eq!(
        parked(home.path(), &p, Some(&kept())),
        None,
        "a changed login is usable at once"
    );
    assert!(poll_once(false).unwrap().is_err());
    assert_eq!(
        parked(home.path(), &p, Some(&kept())),
        Some(Parked::Refused),
        "refused again, for the new login"
    );
    assert_eq!(reads(), 2);
}
