use super::super::{
    model::Identity,
    store::{ChangeKind, Database, Guard, Login, Store, Ticket},
    transaction::{Native, NativeGuard, Recovery},
    Accounts, Clients, Home, IsolatedSignIn, MakeHome, NativeAccount, NativeStores, Notify,
};
use crate::dto::AgentId;
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard},
};

static SERIAL: Mutex<()> = Mutex::new(());

pub(super) fn claude(user: &str, generation: &str) -> Login {
    claude_in(user, "team", generation)
}
pub(super) fn claude_in(user: &str, workspace: &str, generation: &str) -> Login {
    Login {
        auth: json!({"claudeAiOauth": {
            "accessToken": format!("access-{generation}"),
            "refreshToken": format!("refresh-{generation}")
        }}),
        account: json!({
            "accountUuid": user,
            "organizationUuid": workspace,
            "emailAddress": format!("{user}@example.com")
        }),
    }
}

pub(super) fn codex(user: &str, generation: &str) -> Login {
    Login {
        auth: json!({"tokens": {
            "access_token": format!("access-{generation}"),
            "refresh_token": format!("refresh-{generation}"),
            "account_id": "team",
            "id_token": super::super::codex::tests::id_token(
                &json!({"chatgpt_user_id": user, "chatgpt_account_id": "team"})
            )
        }}),
        account: serde_json::Value::Null,
    }
}

pub(super) fn generation(login: Option<&Login>) -> Option<String> {
    let auth = &login?.auth;
    let token = auth
        .pointer("/claudeAiOauth/refreshToken")
        .or_else(|| auth.pointer("/tokens/refresh_token"))?
        .as_str()?;
    Some(token.trim_start_matches("refresh-").to_owned())
}

pub(super) fn fingerprint(provider: AgentId, login: &Login) -> String {
    super::super::view(provider, login).unwrap().fingerprint()
}

pub(super) fn identity(provider: AgentId, user: &str, workspace: &str) -> Identity {
    Identity {
        provider,
        user_id: user.into(),
        workspace_id: workspace.into(),
    }
}

#[derive(Default)]
pub(super) struct NativeState {
    pub live: RefCell<Option<Login>>,
    pub verify_error: RefCell<Option<String>>,
    pub logout_error: RefCell<Option<String>>,
    pub logouts: Cell<usize>,
    pub locks: Cell<usize>,
    pub reads: Cell<usize>,
    pub resolved: RefCell<Vec<AgentId>>,
    pub signed_in: RefCell<Option<Login>>,
    pub sign_in_exit: Cell<i32>,
    pub isolated: RefCell<Vec<(AgentId, PathBuf)>>,
    pub cleaned: Cell<usize>,
    pub homes: Option<Arc<Homes>>,
}

#[derive(Default)]
pub(super) struct Homes {
    pub logins: Mutex<HashMap<PathBuf, Login>>,
    pub busy: Mutex<Vec<PathBuf>>,
    pub refuses: Mutex<Vec<PathBuf>>,
    pub drops_writes: Mutex<bool>,
    pub undeletable: Mutex<Vec<PathBuf>>,
    pub unreadable: Mutex<Vec<PathBuf>>,
    pub read: Mutex<Vec<PathBuf>>,
    pub emptied: Mutex<Vec<PathBuf>>,
    pub deleted: Mutex<Vec<PathBuf>>,
    pub answer: Mutex<Option<crate::limits::SavedReadError>>,
}

struct FakeHome(Arc<Homes>, PathBuf);
impl FakeHome {
    fn refused(&self) -> Result<(), String> {
        if self.0.refuses.lock().unwrap().contains(&self.1) {
            Err("The home refused the write.".into())
        } else {
            Ok(())
        }
    }
}
impl Home for FakeHome {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        if self.0.busy.lock().unwrap().contains(&self.1) {
            return Err("Claude is busy with this home.".into());
        }
        std::fs::create_dir_all(&self.1).unwrap();
        Ok(Box::new(()))
    }
    fn read(&self) -> Result<Option<Login>, String> {
        if self.0.unreadable.lock().unwrap().contains(&self.1) {
            return Err("The home's login could not be read.".into());
        }
        Ok(self.0.logins.lock().unwrap().get(&self.1).cloned())
    }
    fn identify(&self, login: &Login) -> Result<Identity, String> {
        super::super::view(AgentId::Claude, login)?.identity()
    }
    fn put(&self, login: &Login, _: &dyn NativeGuard) -> Result<Option<Login>, String> {
        self.refused()?;
        if *self.0.drops_writes.lock().unwrap() {
            return self.read();
        }
        self.0
            .logins
            .lock()
            .unwrap()
            .insert(self.1.clone(), login.clone());
        self.read()
    }
    fn clear(&self, _: &dyn NativeGuard) -> Result<(), String> {
        self.refused()?;
        self.0.logins.lock().unwrap().remove(&self.1);
        self.0.emptied.lock().unwrap().push(self.1.clone());
        Ok(())
    }
    fn read_usage(
        &self,
        identity: &Identity,
    ) -> Result<crate::dto::ProviderLimitsDto, crate::limits::SavedReadError> {
        self.0.read.lock().unwrap().push(self.1.clone());
        if let Some(answer) = self.0.answer.lock().unwrap().clone() {
            return Err(answer);
        }
        Ok(weekly_reading(&identity.observation_key()))
    }
    fn delete(&self, _: Box<dyn NativeGuard>) -> Result<(), String> {
        if self.0.undeletable.lock().unwrap().contains(&self.1) {
            return Err("The home could not be removed.".into());
        }
        self.0.logins.lock().unwrap().remove(&self.1);
        self.0.deleted.lock().unwrap().push(self.1.clone());
        let _ = std::fs::remove_dir_all(&self.1);
        Ok(())
    }
}

pub(super) fn weekly_reading(key: &str) -> crate::dto::ProviderLimitsDto {
    crate::dto::ProviderLimitsDto::for_test(AgentId::Claude, key).with_reading(
        crate::dto::Reading {
            windows: vec![crate::dto::LimitWindowDto {
                id: "weekly".into(),
                label: "Weekly".into(),
                kind: crate::dto::LimitWindowKind::Weekly,
                used_percent: 42.0,
                window_seconds: Some(604_800),
                resets_at: None,
                observed_at: "2026-09-19T00:00:00Z".into(),
            }],
            ..Default::default()
        },
    )
}
#[derive(Clone)]
struct FakeNative(Rc<NativeState>, AgentId);
impl Native for FakeNative {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        self.0.locks.set(self.0.locks.get() + 1);
        Ok(Box::new(()))
    }
    fn read(&self) -> Result<Option<Login>, String> {
        self.0.reads.set(self.0.reads.get() + 1);
        Ok(self.0.live.borrow().clone())
    }
    fn identify(&self, login: &Login) -> Result<Identity, String> {
        super::super::view(self.1, login)?.identity()
    }
    fn write(&self, login: Option<&Login>) -> Result<(), String> {
        *self.0.live.borrow_mut() = login.cloned();
        Ok(())
    }
    fn verify(&self) -> Result<(), String> {
        self.0.reads.set(self.0.reads.get() + 1);
        self.0.verify_error.borrow().clone().map_or(Ok(()), Err)
    }
}
impl NativeAccount for FakeNative {
    fn preflight(&self) -> Result<(), String> {
        Ok(())
    }
    fn logout(&self) -> Result<(), String> {
        self.0.logouts.set(self.0.logouts.get() + 1);
        if let Some(error) = self.0.logout_error.borrow().clone() {
            return Err(error);
        }
        *self.0.live.borrow_mut() = None;
        Ok(())
    }
}

struct FakeStores(Rc<NativeState>);
impl NativeStores for FakeStores {
    fn native(&self, provider: AgentId, _: &Path) -> Result<Box<dyn NativeAccount>, String> {
        super::super::adapter(provider)?;
        self.0.resolved.borrow_mut().push(provider);
        Ok(Box::new(FakeNative(self.0.clone(), provider)))
    }
    fn isolated(&self, provider: AgentId, dir: &Path) -> Result<Box<dyn IsolatedSignIn>, String> {
        super::super::adapter(provider)?;
        self.0.isolated.borrow_mut().push((provider, dir.into()));
        Ok(Box::new(FakeIsolated(
            FakeNative(self.0.clone(), provider),
            dir.into(),
        )))
    }
    fn homes(&self, provider: AgentId) -> Option<Box<MakeHome<'_>>> {
        let homes = self
            .0
            .homes
            .clone()
            .filter(|_| provider == AgentId::Claude)?;
        Some(Box::new(move |dir| {
            Box::new(FakeHome(homes.clone(), dir.into())) as Box<dyn Home>
        }))
    }
}

struct FakeIsolated(FakeNative, PathBuf);
impl Native for FakeIsolated {
    fn read(&self) -> Result<Option<Login>, String> {
        Ok(self.0 .0.signed_in.borrow().clone())
    }
    fn identify(&self, login: &Login) -> Result<Identity, String> {
        self.0.identify(login)
    }
    fn write(&self, _: Option<&Login>) -> Result<(), String> {
        panic!("an isolated sign-in never writes a login")
    }
    fn verify(&self) -> Result<(), String> {
        Ok(())
    }
}
impl IsolatedSignIn for FakeIsolated {
    fn sign_in(&self) -> std::process::Command {
        crate::cli_stub::CliStub::new("official-sign-in")
            .exit(self.0 .0.sign_in_exit.get())
            .cli(&self.1.join("bin"))
            .command()
    }
    fn first_usage(&self, _: &Path, _: &Identity) -> Option<crate::dto::ProviderLimitsDto> {
        None
    }
    fn clean(&self) -> Result<(), String> {
        self.0 .0.cleaned.set(self.0 .0.cleaned.get() + 1);
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct ClientState {
    pub running: Cell<bool>,
    pub asked: RefCell<Vec<(&'static str, AgentId)>>,
}
struct FakeClients(Rc<ClientState>);
impl FakeClients {
    fn ask(&self, check: &'static str, provider: AgentId) -> Result<(), String> {
        self.0.asked.borrow_mut().push((check, provider));
        if self.0.running.get() {
            Err("Close this provider's clients.".into())
        } else {
            Ok(())
        }
    }
}
impl Clients for FakeClients {
    fn activation_safe(&self, provider: AgentId) -> Result<(), String> {
        self.ask("activation safe", provider)
    }
    fn closed(&self, provider: AgentId) -> Result<(), String> {
        self.ask("closed", provider)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Heard {
    Changed(AgentId),
    Accounts,
}
pub(super) struct Recorder {
    home: PathBuf,
    pub heard: RefCell<Vec<(Heard, bool)>>,
}
struct Listener(Rc<Recorder>);
impl Listener {
    fn record(&self, heard: Heard) {
        let _now = super::super::override_lease_timeout(std::time::Duration::ZERO);
        let released = Store::lease(&self.0.home).is_ok()
            && super::super::PROVIDERS
                .into_iter()
                .all(super::super::activity::tests::idle);
        self.0.heard.borrow_mut().push((heard, released));
    }
}
impl Notify for Listener {
    fn changed(&self, provider: AgentId) {
        self.record(Heard::Changed(provider));
    }
    fn accounts(&self) {
        self.record(Heard::Accounts);
    }
}

pub(super) struct Harness {
    pub home: tempfile::TempDir,
    pub native: Rc<NativeState>,
    pub clients: Rc<ClientState>,
    pub recorder: Rc<Recorder>,
    _serial: MutexGuard<'static, ()>,
}
impl Harness {
    pub fn new() -> Self {
        let serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".on-n-off/accounts")).unwrap();
        super::super::vault::tests::unlock_fixture(&home);
        Self {
            recorder: Rc::new(Recorder {
                home: home.path().into(),
                heard: RefCell::default(),
            }),
            home,
            native: Rc::default(),
            clients: Rc::default(),
            _serial: serial,
        }
    }
    pub fn path(&self) -> &Path {
        self.home.path()
    }
    pub fn with_homes(mut self) -> Self {
        Rc::get_mut(&mut self.native)
            .expect("homes are given before anything shares the native state")
            .homes = Some(Arc::default());
        self
    }
    pub fn homes(&self) -> &Homes {
        self.native.homes.as_deref().expect("a harness with homes")
    }
    pub fn home_of(&self, id: &str) -> Option<PathBuf> {
        let home = self
            .vault()
            .profiles
            .into_iter()
            .find(|p| p.id == id)?
            .home?;
        Some(super::super::homes::dir(self.path(), &home).unwrap())
    }
    pub fn in_home(&self, id: &str) -> Option<String> {
        let dir = self.home_of(id)?;
        generation(self.homes().logins.lock().unwrap().get(&dir))
    }
    pub fn in_vault(&self, id: &str) -> Option<String> {
        let db = self.vault();
        generation(db.profiles.iter().find(|p| p.id == id)?.login.as_ref())
    }
    pub fn accounts(&self) -> Accounts {
        Accounts {
            home: self.path().into(),
            stores: Box::new(FakeStores(self.native.clone())),
            clients: Box::new(FakeClients(self.clients.clone())),
            notify: Box::new(Listener(self.recorder.clone())),
        }
    }
    pub fn signed_in(&self, login: Option<Login>) {
        *self.native.live.borrow_mut() = login;
    }
    pub fn live(&self) -> Option<String> {
        generation(self.native.live.borrow().as_ref())
    }
    pub fn heard(&self) -> Vec<(Heard, bool)> {
        self.recorder.heard.take()
    }
    pub fn seed<T>(&self, edit: impl FnOnce(&mut Database) -> T) -> T {
        Store::open(self.path(), true)
            .unwrap()
            .change(ChangeKind::Metadata, |db| Ok(edit(db)))
            .unwrap()
    }
    pub fn saved(&self, identity: Identity, login: Login) -> String {
        self.seed(|db| db.save(identity, login, None).unwrap())
    }
    pub fn interrupted(&self, target: &str, outgoing: Option<Login>) {
        let outgoing_identity = outgoing.as_ref().map(|login| {
            FakeNative(self.native.clone(), AgentId::Claude)
                .identify(login)
                .unwrap()
        });
        self.seed(|db| {
            db.begin_recovery(Recovery {
                target_id: target.into(),
                outgoing,
                outgoing_identity,
            })
        });
    }
    pub fn vault(&self) -> Database {
        Store::open_existing(self.path()).unwrap().load().unwrap()
    }
    pub fn sealed(&self) -> Option<String> {
        std::fs::read(self.path().join(".on-n-off/accounts/vault.enc"))
            .ok()
            .map(|bytes| crate::sha::sha256_hex(&bytes))
    }
    pub fn sign_in(&self) -> Ticket {
        self.vault().ticket(Guard::SignIn).unwrap()
    }
    pub fn vouches(&self, ticket: &Ticket) -> bool {
        Store::open_existing(self.path())
            .unwrap()
            .recheck(ticket)
            .is_ok()
    }
}
