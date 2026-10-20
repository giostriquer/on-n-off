use super::*;
use crate::accounts::transaction::NativeGuard;

struct SecretsUntouched {
    stamp: Mutex<Option<String>>,
    reads: Mutex<usize>,
}

impl SecretsUntouched {
    fn stamped(stamp: &str) -> Self {
        Self {
            stamp: Mutex::new(Some(stamp.into())),
            reads: Mutex::new(0),
        }
    }
}

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
        *self.reads.lock().unwrap() += 1;
        Err(ReadFailure::LoginRefused)
    }
    fn login_stamp(&self) -> Option<String> {
        self.stamp.lock().unwrap().clone()
    }
    fn sign_in_deadline(&self) -> Option<model::SignInDeadline> {
        panic!("read a home's sign-in deadline")
    }
    fn delete(&self, _: Box<dyn NativeGuard>) -> Result<(), String> {
        panic!("deleted a home")
    }
}

fn homed(home: &Path) -> Profile {
    let mut p = profile();
    p.home = Some(uuid::Uuid::new_v4().to_string());
    p.login = None;
    rewrite(home, |db| db.profiles.push(p.clone()));
    p
}

#[test]
fn a_homes_generation_is_its_login_stamp_and_never_its_login() {
    let home = tempfile::tempdir().unwrap();
    let p = homed(home.path());
    let account_home = SecretsUntouched::stamped("item written at 10:00");
    let poll = |force| {
        poll_home(
            home.path(),
            &p,
            &ticket(home.path()),
            force,
            &|| Ok(open(home.path())),
            &account_home,
        )
    };
    let parked_now = || {
        parked(
            home.path(),
            &p,
            Some(&|_: &str| -> Result<Box<dyn Home>, String> {
                Ok(Box::new(SecretsUntouched {
                    stamp: Mutex::new(account_home.stamp.lock().unwrap().clone()),
                    reads: Mutex::new(0),
                }))
            }),
        )
    };

    assert!(poll(false).unwrap().is_err());
    assert_eq!(parked_now(), Some(Parked::Refused));
    super::elapse(home.path());
    assert!(poll(true).unwrap().is_err());
    assert_eq!(
        *account_home.reads.lock().unwrap(),
        1,
        "parked until the login changes"
    );

    *account_home.stamp.lock().unwrap() = Some("item written at 10:05".into());
    assert_eq!(parked_now(), None, "a changed login is usable at once");
    assert!(poll(false).unwrap().is_err());
    assert_eq!(*account_home.reads.lock().unwrap(), 2);
}
