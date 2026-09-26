//! The account operations over a scratch home: its vault unlocked by a fixture key, a fake native
//! store that reads each provider's logins by that provider's rules, fake running clients and a
//! notifier that records what it heard.
use super::super::{
    model::Identity,
    store::{ChangeKind, Database, Guard, Login, Store, Ticket},
    transaction::{Native, NativeGuard, Recovery},
    Accounts, Clients, IsolatedSignIn, NativeAccount, Notify,
};
use crate::dto::AgentId;
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Mutex, MutexGuard},
};

/// The operations take the process-wide provider reservation, so these tests run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// A Claude login for `user` in the shared test workspace; `generation` names its tokens.
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

/// A Codex login for `user` in the shared test workspace, shaped as Codex writes `auth.json`;
/// `generation` names its tokens.
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

/// Which generation `login` is, by the name `claude` or `codex` gave its tokens: `Login` has no
/// `Debug`, so assertions compare this instead.
pub(super) fn generation(login: Option<&Login>) -> Option<String> {
    let auth = &login?.auth;
    let token = auth
        .pointer("/claudeAiOauth/refreshToken")
        .or_else(|| auth.pointer("/tokens/refresh_token"))?
        .as_str()?;
    Some(token.trim_start_matches("refresh-").to_owned())
}

/// The generation `login` is, as `provider` fingerprints it.
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

/// The native store's state, shared between a test and the store the operations resolve.
#[derive(Default)]
pub(super) struct NativeState {
    pub live: RefCell<Option<Login>>,
    pub verify_error: RefCell<Option<String>>,
    pub logout_error: RefCell<Option<String>>,
    pub logouts: Cell<usize>,
    /// How often the native locks were taken and the native login read: the Keychain on macOS.
    pub locks: Cell<usize>,
    pub reads: Cell<usize>,
    /// Which provider's native store each operation resolved.
    pub resolved: RefCell<Vec<AgentId>>,
    /// The login the official client leaves in an isolated sign-in's store.
    pub signed_in: RefCell<Option<Login>>,
    /// How the official sign-in exits.
    pub sign_in_exit: Cell<i32>,
    /// The directories isolated stores were made in, and how many were cleaned.
    pub isolated: RefCell<Vec<PathBuf>>,
    pub cleaned: Cell<usize>,
}
/// The native store of the provider it was resolved for, reading logins by that provider's rules.
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
    /// Verification reads the native login, and for Claude may renew and rewrite it under
    /// Claude Code's lock, so it counts as a read.
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
    fn isolated(&self, dir: &Path) -> Result<Box<dyn IsolatedSignIn>, String> {
        self.0.isolated.borrow_mut().push(dir.into());
        Ok(Box::new(FakeIsolated(self.clone(), dir.into())))
    }
}

/// An isolated sign-in over the fake store: the official client is a stub that exits as the state
/// says, and leaves `NativeState::signed_in` behind.
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
    fn first_usage(
        &self,
        _: &Path,
        _: &Login,
        _: &Identity,
    ) -> Option<crate::dto::ProviderLimitsDto> {
        None
    }
    fn clean(&self) -> Result<(), String> {
        self.0 .0.cleaned.set(self.0 .0.cleaned.get() + 1);
        Ok(())
    }
}

/// Running clients: which checks were asked, and whether they refuse.
#[derive(Default)]
pub(super) struct ClientState {
    pub running: Cell<bool>,
    pub asked: RefCell<Vec<&'static str>>,
}
struct FakeClients(Rc<ClientState>);
impl FakeClients {
    fn ask(&self, check: &'static str) -> Result<(), String> {
        self.0.asked.borrow_mut().push(check);
        if self.0.running.get() {
            Err("Close this provider's clients.".into())
        } else {
            Ok(())
        }
    }
}
impl Clients for FakeClients {
    fn activation_safe(&self, _: AgentId) -> Result<(), String> {
        self.ask("activation safe")
    }
    fn closed(&self, _: AgentId) -> Result<(), String> {
        self.ask("closed")
    }
}

/// What the notifier heard, and whether every lease the operation held was already free when it
/// did: the vault lease and each provider's in-process read or change reservation. A change still
/// held would refuse the Limits read the notification starts; a read still held would keep the
/// next account change refused through it.
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
    pub fn accounts(&self) -> Accounts {
        let native = self.native.clone();
        Accounts {
            home: self.path().into(),
            // Only a provider with an adapter has a native store, as in the live resolver.
            native: Box::new(move |provider, _| {
                super::super::adapter(provider)?;
                native.resolved.borrow_mut().push(provider);
                Ok(Box::new(FakeNative(native.clone(), provider)))
            }),
            clients: Box::new(FakeClients(self.clients.clone())),
            notify: Box::new(Listener(self.recorder.clone())),
        }
    }
    pub fn signed_in(&self, login: Option<Login>) {
        *self.native.live.borrow_mut() = login;
    }
    /// The generation the CLI is signed in with.
    pub fn live(&self) -> Option<String> {
        generation(self.native.live.borrow().as_ref())
    }
    pub fn heard(&self) -> Vec<(Heard, bool)> {
        self.recorder.heard.take()
    }
    /// Change the vault as a fixture, outside any operation under test: a write no rule refuses
    /// and that rejects nothing in flight.
    pub fn seed<T>(&self, edit: impl FnOnce(&mut Database) -> T) -> T {
        Store::open(self.path(), true)
            .unwrap()
            .change(ChangeKind::Metadata, |db| Ok(edit(db)))
            .unwrap()
    }
    /// Save `login` as a profile of `identity` and give back its id.
    pub fn saved(&self, identity: Identity, login: Login) -> String {
        self.seed(|db| db.save(identity, login, None).unwrap())
    }
    /// Leave an interrupted switch to `target`, from `outgoing`, awaiting recovery.
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
    /// A digest of the sealed vault on disk: any write changes it, since every seal takes a new
    /// nonce.
    pub fn sealed(&self) -> Option<String> {
        std::fs::read(self.path().join(".on-n-off/accounts/vault.enc"))
            .ok()
            .map(|bytes| crate::sha::sha256_hex(&bytes))
    }
    /// The ticket a sign-in starting now would publish against.
    pub fn sign_in(&self) -> Ticket {
        self.vault().ticket(Guard::SignIn).unwrap()
    }
    /// Whether a sign-in that took `ticket` could still publish.
    pub fn vouches(&self, ticket: &Ticket) -> bool {
        Store::open_existing(self.path())
            .unwrap()
            .recheck(ticket)
            .is_ok()
    }
}
