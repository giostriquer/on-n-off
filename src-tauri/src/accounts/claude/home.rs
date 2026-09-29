//! A saved Claude account's home (`accounts::homes`): a private Claude config dir holding the
//! account's login while it is not the signed-in one. Claude Code renews that login itself when
//! on-n-off asks it there for the account's usage (`limits::claude_cli`). The home's own scoped
//! Keychain entry holds the login on macOS, and its credentials file does on Windows, as Claude
//! Code keeps any login there.
use super::*;

pub(in crate::accounts) struct ClaudeHome {
    dir: PathBuf,
    store: ClaudeNative,
}

impl ClaudeHome {
    /// The home in `dir`, created by its first write.
    pub(super) fn at(dir: &Path) -> Self {
        Self {
            dir: dir.into(),
            store: ClaudeNative::home(dir),
        }
    }
}

impl Home for ClaudeHome {
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
    ) -> Result<ProviderLimitsDto, crate::limits::SavedReadError> {
        crate::limits::claude_cli::read_usage(
            &|| self.store.command(),
            &self.store.config_file,
            identity,
        )
    }

    /// The Keychain entry first, then the directory, which takes Claude Code's lock directories
    /// with it: releasing the locks then finds nothing left to remove.
    fn delete(&self, locks: Box<dyn NativeGuard>) -> Result<(), String> {
        IsolatedSignIn::clean(&self.store)?;
        let removed = match fs::remove_dir_all(&self.dir) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err("Cannot remove the saved account's home.".into())
            }
            _ => Ok(()),
        };
        drop(locks);
        removed
    }
}
