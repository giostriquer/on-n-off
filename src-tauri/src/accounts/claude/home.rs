use super::*;

pub(in crate::accounts) struct ClaudeHome {
    dir: PathBuf,
    store: ClaudeNative,
}

impl ClaudeHome {
    pub(super) fn at(dir: &Path) -> Self {
        Self {
            dir: dir.into(),
            store: ClaudeNative::home(dir),
        }
    }
}

impl Home for ClaudeHome {
    fn dir(&self) -> &Path {
        &self.dir
    }

    fn recognizes(&self) -> bool {
        native::real_dir(&self.store.config_home)
    }

    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        self.store.lock()
    }

    fn read(&self) -> Result<Option<Login>, String> {
        self.store.read()
    }

    fn identify(&self, login: &Login) -> Result<Identity, String> {
        self.store.identify(login)
    }

    fn put(&self, login: &Login, locks: &dyn NativeGuard) -> Result<Option<Login>, String> {
        self.store.publish(Change::Login(Some(login)), locks)?;
        self.store.read()
    }

    fn clear(&self, locks: &dyn NativeGuard) -> Result<(), String> {
        self.store.publish(Change::SignedOut, locks)
    }

    fn read_usage(
        &self,
        identity: &Identity,
    ) -> Result<crate::limits::Accepted, crate::limits::ReadFailure> {
        crate::limits::claude_cli::read_usage(
            &|| self.store.command(),
            &self.store.config_file,
            identity,
        )
    }

    fn login_stamp(&self) -> Result<String, String> {
        self.store.login_stamp()
    }

    fn sign_in_deadline(&self) -> Option<SignInDeadline> {
        NativeAccount::sign_in_deadline(&self.store)
    }

    fn delete(&self, locks: Box<dyn NativeGuard>) -> Result<(), String> {
        IsolatedSignIn::clean(&self.store)?;
        super::super::homes::remove(&self.dir, locks)
    }
}
