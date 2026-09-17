# Account profiles

`accounts/` implements opt-in saved subscription logins and explicit activation in the default
native Claude/Codex CLI store. It adds no proxy, request router, inference traffic or automatic
account rotation.

## Ownership

A profile has a random local UUID and a stable provider/user/workspace identity. Email is the account name; an optional free-text category is separate display metadata.
Neither field is an identity key. Active native storage is authoritative. A saved active credential is a
non-refreshing shadow; switching away captures the latest native generation. Inactive access-token
expiry does not discard a renewable login. The first version does not refresh inactive profiles.

The account vault is XChaCha20-Poly1305 authenticated ciphertext, atomically replaced using private
staging files. A fresh random nonce protects each write. A 32-byte key is stored with the OS:
macOS Keychain or Windows Credential Manager. Large OAuth payloads never have to fit into one
Windows credential entry. A damaged vault or unavailable key fails closed. The 32-byte encryption key stays in memory after
one unlock per storage root and app session; it is read from the OS store again after restart.
Concurrent readers share that unlock, and background reads retain a denial until an explicit
account operation retries. Native OAuth payloads are not cached by this mechanism. Metadata and
credentials have separate in-memory types; only metadata DTOs cross IPC. The entire persisted
registry, including recovery records, is encrypted. This feature never uses ConfigIo's ordinary
backup store for tokens.

`accounts/claude_renew.rs` remains the only Claude token-redemption implementation. It keeps the
existing expiry, native-lock, preflight and stranded-token behavior, writing only the active native
store. Limits consumes access-only projections. Codex delegates renewal to its official app-server.

## Operations

- **Remember accounts on this device** is off by default. One global opt-in enables a background
  check for native Claude/Codex logins about every 30 seconds, independently of notification settings.
  Unchanged saved logins and pending reauthentication are skipped. A new/changed native login is
  verified before saving; no native activation occurs. Access expiry can use the existing native
  renewal owner. Switching again before a check can cause the previous login to be missed.
- **Save current** verifies native readback and saves its latest renewable login. It also explicitly
  reenrolls an account excluded by Remove.
- **Edit category** accepts optional free text (up to 100 characters). Blank clears it. Categories
  survive refresh, reauthentication and switching. The provider email remains the primary name;
  missing email is shown as unavailable. Older custom names migrate to categories when loaded.
- **Add / sign in again** launches official CLI authentication in an isolated native home. It checks
  the resulting identity, verifies the expected profile during reauthentication, then saves it.
  Before publication it attempts a usage/plan read using that isolated login: Codex uses app-server
  without forced renewal; Claude uses its access-only projection and verified profile/usage endpoints.
  It rereads the credential afterward so official-client rotation is not stranded, rejects any
  user/workspace change, and publishes only a matching observation after the vault save and existing
  cancellation/epoch checks. Usage failure retains the login and previous history. Successful
  publication refreshes the shared Limits reading so the new card appears without waiting for a poll.
  The current CLI login does not change. Reauthentication offers the new generation for activation.
- **Use** preserves the outgoing login in a durable encrypted journal, publishes the incoming
  credential and narrow account identity, verifies native readback, then clears the journal.
- **Recover** restores a known outgoing credential after an interrupted write. Recovery recognizes
  known credential bytes before interpreting partially written identity metadata. Unknown rotated
  bytes require native verification; unrelated identities are never overwritten.
- **Sign out** uses the official logout command and invalidates saved logins for the same user
  before revocation. It is intentionally distinct from switching.
- **Remove** removes the app-owned profile only and excludes that stable identity from automatic
  remembering until explicitly saved/added again. It does not sign out a native client.

An in-process reservation and a cross-process shared/exclusive activity lease exclude provider
reads from account activation. A separate lease serializes vault access. Existing vault keys are unlocked before acquiring the shared storage lease, so an OS prompt does
not block another provider's file transaction. Initial key/vault creation remains serialized.
Brief contention waits on blocking workers (up to ten seconds); it never bypasses the lease or replaces its lock file. Completed saves release their leases before announcing account changes. Native Claude locks cover
the outgoing reread, durable journal and publication. They are released for verification/renewal,
while the exclusive activity lease remains held through completion or recovery. The native locks
have a heartbeat while Keychain access is pending. Claude verification compares the authenticated
user and organization with the exact resolved native identity file, including legacy configuration. All network
and filesystem work happens on blocking workers, outside UI/state mutexes. Cancellation is scoped
to an operation UUID, including cancel-before-start. A persisted generation rejects late sign-in
publication after logout, removal or activation in any manager instance, and during recovery.
Pending reauthentication is preserved when switching away; Save current refuses to overwrite it.

Remembering consent lives in a private metadata preference beside the vault. It is checked before
credential access and again under the vault lease before publication. Enable proves the protected
vault is usable first. Disabling needs only the publication file lease and consent write, so it
works even if the vault is damaged or unavailable. The UI reads consent independently from account
credentials. Publication rechecks consent; re-enabling advances the persisted operation generation,
so an earlier check cannot save after disable/re-enable. Turning it off retains existing profiles. Discovery
verification releases the vault lease; publication reacquires it, rechecks consent, epoch, recovery,
exclusions and exact native credential equality, then holds the native lock through encrypted save.
Logout suppresses the outgoing credential fingerprint so an unchanged failed logout is not silently
remembered again. A later verified native sign-in with a new credential can be remembered.

The encrypted journal is persisted before any native write. ConfigIo owns narrow oauthAccount
configuration publication, validation and rollback; the account journal is its protected backup
participant. Claude credential publication merges only `claudeAiOauth`, preserving current MCP
OAuth entries. Successful account changes replace the shared limits reading and cancel in-flight
billing work through `read_revision`; a late account A observation cannot become account B's data.

## Native scope and limitations

The guaranteed target is the default native CLI home. Custom native homes, selected Codex config
profiles, ephemeral/alternate Codex backends, environment credentials and detected forced-login
policies currently defer to the official client. Existing model/endpoints/API-key configuration is
never rewritten to simulate a switch. Native Codex file/keyring/auto backends are selected from
config; unreadable protected storage is not treated as a missing login. macOS native account reads use the same system `security` reader as Limits, with the exact
resolved service and account and a bounded subprocess deadline. Claude uses scoped Keychain
entries only for isolated sign-in, deriving their names from the raw NFC-normalized home path.
On macOS, real Claude sign-ins retain the OS home so Security can locate the login Keychain;
`CLAUDE_CONFIG_DIR` isolates the CLI configuration and selects its scoped credential entry.
Disposable file-backed tests still redirect the OS home and never use the real Keychain.

Ordinary Claude activation allows running clients: Claude Code handles native credential changes.
It still acquires the native refresh locks, preserves outgoing credentials in the protected journal,
and verifies the incoming identity. This does not move an already in-flight request to another account
or claim immediate uptake by Claude Desktop or IDE sessions.

Codex activation, and sign-out or explicit crash recovery for either provider, require known CLI,
desktop and IDE agent processes to be closed. The conservative process preflight never terminates
user processes. It cannot prevent an external client starting afterward; keep clients closed until
these operations finish. Restart Codex clients after switching.

The preflight identifies a client by its executable, or by the script a JavaScript runtime
launched (the first file after its flags, or the package's own entry script), never by an argument
that merely names the provider. It leaves out processes on-n-off started, whose provider reads run
under the account-change lease, and names each client after the app bundle it runs in or was
started from.

Before an ordinary Codex switch the account card lists those clients and lets the person switch
anyway. Running Codex clients never pick up a replaced `auth.json`: they keep the previous account
until restarted, and signing out or in from one can revoke saved logins, so the confirmation says
both. Codex clients match a login by workspace and take no lock, so switching beside them refuses
two accounts in one workspace and an outgoing access token within ten minutes of renewal.

Every activation also re-reads the outgoing login once its journal is durable, saving a generation
a client rotated and refusing to publish over another account; a failure before publication clears
the journal. It reads the published bytes back under the native locks before verification, which
refreshes whatever is on disk. When a client replaced them, or verification fails beside running
clients, only bytes this change wrote or replaced are restored; anything else, including a login a
client signed out, keeps the journal for explicit recovery with clients closed. These checks narrow
the races, not close them: a client whose refresh was already in flight can still write during
verification, and clients older than Codex 0.117 renew without checking the account on disk.
Sign-out and recovery never offer the choice.

Claude's [quickstart](https://code.claude.com/docs/en/quickstart) documents `/login` inside a running
session. Its [2.1.178 changelog](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#21178)
also records a fix for credentials refreshed outside a session. The exception applies only to ordinary activation;
revocation and abandoned-login cleanup retain their original process checks.

Isolated login directories have a lease and a provider marker. Completion/cancellation removes
only their scoped secrets. Abandoned directories are recovered only after the lease is free and
the provider has no running clients; failures retain the private directory for later recovery.

New usage/billing observation keys contain both user and workspace. Legacy observations remain
historical, with no inferred ownership. Unattributed session usage cannot advance new scoped
profiles. Organization-only Claude Desktop samples are also excluded from user-scoped history.
Billing dates appear directly on account cards; Connect/Retry billing lives in account controls.
Saved profiles expose an observation key and an identity-only billing projection. The browser
reader can verify inactive saved accounts without using their OAuth credentials or changing the
active CLI. Automatic browser reads begin for saved profiles (or a native account with prior
billing) and retain a persisted 24-hour attempt cooldown, including first failures. Interactive
browser access remains an explicit account action. Billing metadata queries share one cache
between Limits and account controls.
Browser billing verifies stable user identity and authenticated membership when the
browser default workspace differs, then repeats the session check after reading the date.
Inaccessible browser profiles produce inconclusive errors, not instructions asserting wrong login.

## References and verification

The implementation follows selected invariants from [CodexBar](https://github.com/steipete/CodexBar/tree/d394565751ab6b3c27f0c9c1e7879647ac5d029d),
[claude-swap](https://github.com/realiti4/claude-swap/tree/7187ce83b444c6af7b61ec8ee092623566a2d8fa),
and [CC Switch](https://github.com/farion1231/cc-switch/tree/1d5d90f4aba88447d422a16cdec5282ec5331fd7).
Third-party notices are retained in the repository notice file. No manager/proxy runtime is embedded.

Focused tests use disposable homes, fake native verification boundaries, real file publication,
authenticated encryption and browser/HTTP fixtures. They cover identity separation, newer outgoing
generations, wrong-user reauthentication, cancellation, partial publication, recovery, MCP
preservation, corrupted vaults and browser workspace membership. Tests do not use personal accounts.

Real browser sign-in, provider revocation, macOS Keychain interoperability and Windows credential
storage require designated-account validation on those platforms. Fixture tests and a read-only
app boot do not prove these live operations. Full CI belongs to the PR, not local execution.

Billing eligibility errors remain retryable IPC errors rather than absent-account results.
The final saved-identity lookup holds the account-operation lease through metadata publication,
so profile removal either finishes before that lookup or waits until publication has finished.
The UI rereads eligibility on mount and account-change events; browser access remains subject
to the persisted daily cooldown.

Account management is embedded in Limits cards. The Limits header contains one Add account picker for Claude and Codex;
each card owns its usage, billing date, primary action and optional category. Saved identities
join observations only by exact observation key, and remain visible when usage reads fail.
The picker and Settings share the global automatic-saving opt-in preference. Inactive Remove account removes
the saved login and usage card; active Remove saved login keeps the native login and usage card.
Operations remain busy through readback, and a sign-out confirmation is checked against the
latest known native observation key before invoking the provider action.

A failed account-list unlock exposes a Retry action beside the error. It unlocks the existing
vault and reloads accounts; it does not start sign-in, create a vault, or change a CLI login.

A duplicate legacy email alone does not expose a workspace ID in the header. Workspace labels are
shown only for saved profiles with the same email and distinct verified workspace identities.
When a saved profile has an actual scoped usage observation, the card list also suppresses its
legacy observation using the verified provider, legacy identity key and matching email. This
join does not depend on the snapshot's optional `legacyId`, which older app versions can drop
when rewriting history. It does not transfer old quotas or delete history, and preserves current
legacy observations and accounts without a verified scoped replacement.
The confirmed Remove account action retains that association and explicitly forgets matched
legacy history before its scoped snapshot, so the next read cannot resurrect the removed card.
Legacy deletion passes an optional expected email through the existing Forget command. Storage
rechecks that email under the snapshot write lock and refuses changed or unreadable history;
older callers that omit the guard retain the existing command behavior.

Manual billing requests wait for an existing import, releasing the state mutex while waiting,
and take priority over new automatic checks. The wait is bounded to 90 seconds; account changes
cancel pending requests. Final identity validation and generation checks still govern publication.
Billing errors occupy a full-width line below aligned account action buttons.
