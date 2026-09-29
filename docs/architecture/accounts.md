# Account profiles

`accounts/` implements opt-in saved subscription logins and explicit activation in the default
native Claude/Codex CLI store. It adds no proxy, request router, inference traffic or automatic
account rotation.

## Ownership

A profile has a random local UUID and a stable provider/user/workspace identity. Email is the account name; an optional free-text category is separate display metadata.
Neither field is an identity key. Active native storage is authoritative. A saved active credential is a
non-refreshing shadow; switching away captures the latest native generation. Inactive access-token
expiry does not discard a renewable login. A saved Claude account that is not the signed-in one
keeps its login in its own home (below), where Claude Code renews it; Limits reads it there through
Claude Code's own usage report, never with the token. Limits polls saved Codex accounts, and any
Claude login still in the vault, with access-only HTTP requests without changing the active native
login, each through its provider's Limits reader, which the adapter dispatches (`read_usage`). At
most two saved reads per provider run at once, with per-account backoff and provider retry-after
handling. Rejected shared credentials, and logins that now sign in as a different account, wait for
a changed generation and renew nothing; an expired Claude shadow still in the vault sends no request.
The previous numeric reading and its observation time remain visible, and every card a poll
produces is marked as a saved profile's (`saved_profile`), so it never reads as a remembered one.

Only a Codex login created by Add / sign in again in an isolated home has private renewal
ownership: a provider renews privately only when its adapter says so (`renews_privately`), and
Claude's never does, since its saved logins wait in homes. The flag defaults to false in older
vaults. Capturing a native login clears it; activation clears it durably before publishing any
credential to a native client, including failed activation. A private login can renew in the
encrypted vault, using the provider's OAuth grant; a renewal of a provider that does not renew
privately is refused before anything is recorded. This is separate from native renewal: shared
credentials, including inactive native shadows, never renew independently. A running client can
therefore retain its credential generation.

Each private renewal holds a per-profile cross-process lease and first writes an encrypted intent.
An ambiguous response or crash leaves that intent, preventing another redemption of the same
possibly consumed token. A complete reply is encrypted before vault publication; the next poll
can recover it without another grant. Publication checks that the profile still holds the renewed
generation and still owns its renewal, and deliberately not the sign-in epoch: the refresh token is
already spent, so an account change that left this profile alone must not discard the renewed
login. An unfinished renewal blocks activation of its source generation until recovery
or a new sign-in. No renewal secret enters ordinary backups or DTOs. Usage publication separately
checks the sign-in epoch and the login it read with while holding the vault lease briefly; HTTP
runs without the vault lease. Any account change, removal, logout and reauthentication included,
rejects late usage.

Ownership covers credentials issued and managed through the app's normal flows. It cannot
coordinate copies manually exported to an unrelated client or machine. Codex profiles saved from a
native login and profiles already activated remain access-only until explicitly signed in again.

## Homes

A saved account's home (`accounts/homes.rs`) is a private store of the provider's own client, kept
under `.on-n-off/accounts/homes/<id>`, where the account's one login waits while it is not the
signed-in one. Only Claude has homes (`accounts/claude/home.rs`): a persistent Claude config dir.
Claude Code renews the login there whenever Limits asks it for the account's usage
(`limits::claude_cli`), so on-n-off sends no request with that login and sends no grant for it.

Every saved Claude login lives in exactly one Claude Code store at a time: the default one for the
signed-in account, or the account's home. The issuer replaces a refresh token each time it renews
one, so a second copy would stop working the first time the other renewed, and switching to it
would sign the user out. Two moves keep the one copy, each under the home's own Claude Code locks:

- **Checking out**, inside a switch's account change: the target's login comes from its home into
  the vault, which is persisted, and only then is the home emptied as Claude Code signs out
  (`claudeAiOauth` emptied, the `oauthAccount` record kept). A home that cannot be emptied stops the
  switch before anything is replaced. The switch then publishes the login as any saved login.
- **Checking in**, before every read of saved accounts: each saved account that is not the
  signed-in one, is not archived and holds a login in the vault gets a home id recorded first, then
  its login written there and read back, then its vault login cleared. Each vault step is a
  publication under the read's ticket (the login it moves, the sign-in epoch), so an account change
  meanwhile leaves the move for the next read. A pending recovery moves nothing.

A crash between the halves of either move leaves the one login in both: nothing renews a home
while its profile still holds a vault login, since only a profile without one is read from its
home. The next check-in clears the vault's copy. When the two differ, the vault's was put there
since the home got its copy, by a switch away from the account, a save of it or automatic
remembering, from the native store, which is authoritative: it replaces the home's, at check-in
and at check-out alike, and a check-out still empties the home. A home holding another account's
login is left alone, and its profile keeps its vault login. A login a private renewal of an earlier
version finished but never published moves in with its renewed login, without another grant, and
the renewal record goes; one whose outcome is unknown stays in the vault.

On macOS a home's login is its own scoped Keychain entry, `Claude Code-credentials-<hash of its
config dir>`, created by the first write under Claude Code's own account name
(`PendingWrite::prove_in_home`); a home that keeps a login in a credentials file takes no new one. On
Windows it is the home's `.credentials.json`, the file Claude Code keeps any login in there.

A new sign-in (Add or Sign in again) and Sign out (for every saved workspace of the signed-out
user) set the profile's home aside, and Remove takes the profile with it. A home no profile names
then goes at the next read: every read lists the homes on disk before it reads the vault, and
tears down each one no profile names, under the home's own Claude Code locks, its Keychain entry
first, then the directory. A home's id is recorded in the vault before anything creates it, so one
listed first and named nowhere is one nothing will use again; one its client holds, or that could
not be removed, goes at a later read.

The exception is a version before homes. It reads a vault with homes, since every field it does
not know is skipped, but its next write of the vault drops the field that names them, and such
writes are routine (remembering a renewed native login is one). That leaves a homed account with
no login anywhere but its home. So the read first looks at what an unnamed home holds: a login
that signs in as a saved account with neither a login nor a home is taken back by that account,
unless Sign out left the account so, which it records on the profile (`signed_out`) in the same
vault change; every other home is torn down. A home whose login cannot be read right now is left
for a later read, since it may be the account's only login. The rule has one limit: a version
before homes drops `signed_out` with `home`, so a sign-out it made, or one whose vault it rewrote
before the next read, leaves nothing to tell by, and the signed-out account takes its homes back;
their reads then say whether that sign-out ended their logins. The teardown never
runs `claude auth logout`: whether a logout ends only its own login or every login of the account
is unproven, and a logout that ended the others would sign the user out elsewhere.

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

`accounts/store.rs` owns how the vault changes. An account change (save, remove, use, recover,
sign out, or a sign-in's publication) goes through `Store::change`: a pending recovery refuses it
(recovery instead requires one), it bumps the persisted sign-in epoch, persists, and releases the
vault lease before returning, so the caller announces it after release. `change_then` runs what must
follow under the same lease once the change is durable: a switch's native write, the official
logout. A category edit is a metadata change, allowed during recovery and bumping nothing. Turning
remembering on bumps the epoch but is allowed during recovery, since it changes no login. A change
that changed nothing writes nothing. Save current and sign out ask `Store::gate` first, read-only and
without creating a vault, so a pending recovery refuses them before they verify or read the native
login; the change gates again under its lease.

Work too slow to hold the lease (an isolated sign-in, a remembered login's verification, a usage
read, a private renewal) takes a `Ticket` first, and a pending recovery refuses every ticket. What
the ticket guards is rechecked under the lease afterwards, in one of three ways:

- a sign-in's ticket guards the sign-in epoch, and its publication is itself an account change:
  `Store::change` with `ChangeKind::SignIn` rechecks the ticket, then bumps the epoch;
- `Store::publish` rechecks a remembered login's ticket (the epoch) and a private renewal's (the
  profile's login and renewal ownership, and deliberately not the epoch), and bumps nothing;
- `Store::recheck` serves a usage reading, which is published outside the vault: its ticket guards
  the epoch and the login generation the reading was made with.

The epoch and the recovery journal are private to the store.

## Provider adapters

Everything account code does differently for Claude and Codex sits behind one accounts-local seam,
separate from the catalog's `AgentAdapter` (which accounts touch only through
`supports_accounts`, the IPC gate). `accounts::adapter` is the one place that tells the providers
apart; it gives the provider's `Adapter`, `claude::Claude` or `codex::Codex`, and generic account
code asks that rather than which provider it has. An adapter:

- resolves the provider's native stores, which the operations reach through the `Accounts`
  context's `NativeStores` resolver: `ClaudeNative` over `claude_store.rs`, `CodexNative` over
  `codex_store.rs`. The user's own store is the transaction's `Native` and a `NativeAccount`: its
  preflight (custom homes, environment credentials, managed policy), its sign-out, and the
  signed-in subscription whose saved accounts' usage is read beside it (an API-key Codex login
  has none). A private one in a directory is an `IsolatedSignIn`, with the official sign-in
  command, the first usage reading and the cleanup of anything the sign-in left outside that
  directory;
- reads a login through its typed view, `ClaudeLogin` or `CodexLogin` (`accounts::view`), so
  nothing else reads a login's JSON: identity and email, the credential generation's fingerprint
  and whether a saved login is due to renew. A view only reads; the bare credentials document
  Claude's store holds, without its account record, gives its credential through
  `ClaudeLogin::credential_in` instead. A `Login` keeps its stored shape, `{auth, account}`, so the vault, the
  recovery journal and the renewal journal written by earlier versions still load, and
  `model.rs` keeps the one fingerprint layout every version has hashed. Whole documents are still
  written raw: Codex's `auth.json` verbatim, Claude's `claudeAiOauth` merged into its credentials
  document beside the `oauthAccount` record `ConfigIo` patches;
- reads a saved profile's usage with its login (`read_usage`), through the provider's Limits
  reader, asking the services it is given (`limits::SavedReadUrls`): Claude's credential to
  `limits::read_saved_claude`, the signed-in Claude read expecting the profile's identity, unless
  the login's renewal is due, which reads as expired without a request; Codex's access token to
  `limits::read_saved_codex`, the `wham/usage` body app-server itself reads, without starting a
  CLI. A login without an access token is refused before either. Claude's first usage after a
  sign-in is Claude Code's own report in the isolated home (`limits::claude_cli`); Codex's runs
  app-server there;
- renews a never-activated private login at its token endpoint: Claude's grant sent from
  `claude_renew.rs`, Codex's built and folded by its login and sent from `usage_renew.rs`;
- says how its client processes are recognized and whether they refuse an ordinary switch.

Sign-in, the cleanup of abandoned sign-in homes, automatic remembering and the saved accounts'
usage refresh are methods on the same `Accounts` context as the six operations, so they resolve
native stores and reach running clients and listeners the same way. A remembering poll announces
the account list once when it saved a login or changed the provider's notice. Vault access stays
generic: a saved Codex profile's claims are found in the vault by `accounts::saved_codex_claims`
and read through the Codex view.

`accounts/claude_renew.rs` remains the only Claude token-redemption implementation. It keeps the
existing expiry, native-lock, preflight and stranded-token behavior, writing the active native
store under its locks. Where Claude's login lives and how it is locked belongs to
`accounts/claude_store.rs`, which the renewal, Limits and the account switch share: Claude
Code's dirs as it resolves them (`CLAUDE_CONFIG_DIR` untrimmed and NFC-normalized, with
`CLAUDE_SECURESTORAGE_CONFIG_DIR` moving the credentials, the locks and the Keychain entry's name),
the store Claude Code's own read would use (a Keychain item that parses, token or not, else the
credentials file; an unreadable Keychain refuses every write), the Keychain item under Claude
Code's own account name first, and one lock protocol. A renewal whose refresh lock the heartbeat
finds taken away sends no grant. An emptied `claudeAiOauth`, Claude Code's sign-out, is no native
login. The same grant/parser implementation also serves private vault renewal
under the saved-account journal. Limits consumes access-only projections. The one projection that
carries a credential is `codex_store::metadata_and_access`: the signed-in Codex login's identity
and its access token alone (never its refresh or id token, never the login JSON), wrapped in
`model::AccessToken`, which has no `Debug`, `Clone` or serialization and reads back only as an
`Authorization` header value. Its one caller is the app-server read's identity check after the
handshake (`limits/codex_app_server.rs`). It hands the token to the gate both Codex reads ask their
backend figures through (`limits/codex.rs` `backend_figures`), which uses it only for that card's
account, one read-only GET per figure: the term from `/backend-api/subscriptions` (`limits/renewal.rs`)
and, on a workspace plan, what was spent from
`/backend-api/wham/usage/daily-workspace-user-token-usage-breakdown` (`limits/credits_spent.rs`).
That exception to "Codex alone makes requests for the signed-in account" is the user's decision
(2026-09-24, extended to the term on 2026-09-25). Native Codex delegates
renewal to its official app-server; private saved Codex credentials use the JSON refresh grant
without starting a CLI or writing auth.json. Which backend holds Codex's login, and how its
keyring item is named and reached, is `accounts/codex_store.rs`'s question, as Claude's store is
`claude_store.rs`'s. Limits reads a saved login's credential only through the typed views and
the signed-in Codex login only through `codex_store`'s projections.

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
  without forced renewal; Claude runs `claude -p /usage` there, which makes no model request.
  It rereads the credential afterward so official-client rotation is not stranded, rejects any
  user/workspace change, and publishes only a matching observation after the vault save and existing
  cancellation/epoch checks. Usage failure retains the login and previous history. Successful
  publication refreshes the shared Limits reading so the new card appears without waiting for a poll.
  The current CLI login does not change. Reauthentication offers the new generation for activation.
  Either retires the profile's home: the new login moves into a new one at the next read.
- **Use** checks the incoming login out of its home when it has one, preserves the outgoing login in
  a durable encrypted journal, publishes the incoming credential and narrow account identity,
  verifies native readback, then clears the journal. The outgoing login moves into its own home at
  the next read.
- **Recover** restores a known outgoing credential after an interrupted write. Recovery recognizes
  known credential bytes before interpreting partially written identity metadata. Unknown rotated
  bytes require native verification; unrelated identities are never overwritten.
- **Sign out** uses the official logout command and invalidates saved logins for the same user
  before revocation. It is intentionally distinct from switching.
- **Remove** removes the app-owned profile only and excludes that stable identity from automatic
  remembering until explicitly saved/added again, and retires its home. It does not sign out a
  native client.

An in-process reservation and a cross-process shared/exclusive activity lease exclude provider
reads from account activation. A separate lease serializes vault access. Existing vault keys are unlocked before acquiring the shared storage lease, so an OS prompt does
not block another provider's file transaction. Initial key/vault creation remains serialized.
Brief contention waits on blocking workers (up to ten seconds); it never bypasses the lease or replaces its lock file. Every account change releases its leases, the vault lease and the provider's activity reservation, before it is announced. Every file lease is a `FileLease` (`file_lease.rs`), which unlocks explicitly when dropped: on Unix the lock belongs to the open file, and a child process that another thread spawns meanwhile shares it until the child execs. Native Claude locks
(Claude Code's refresh lock, its legacy lock beside the config dir's real path, and the config
file's lock) cover the outgoing reread, durable journal and publication; the credential write goes
through `claude_store::begin`, the one writer the renewal uses too, which takes Claude Code's
`.storage-write.lock` and reads the store under it; its proof, made before either half of the
change is written, refuses a linked credentials file. A lock that cannot be taken at all is reported with its reason;
only one another process holds reads as Claude being busy. They are released for verification/renewal, while the
exclusive activity lease remains held through completion or recovery: the locked write consumes
the lock guard and returns the store as read back under it, so verification, which may renew and
take the same locks, cannot run while they are held. Every held lock has a heartbeat. Claude verification compares the authenticated
user and organization with the exact resolved native identity file, including legacy configuration. All network
and filesystem work happens on blocking workers, outside UI/state mutexes. Cancellation is scoped
to an operation UUID, including cancel-before-start. The persisted sign-in epoch rejects a late
sign-in publication after any account change in any manager instance, and so does a pending
recovery.
Pending reauthentication is preserved when switching away; Save current refuses to overwrite it.

Remembering consent lives in a private metadata preference beside the vault. It is checked before
credential access and again under the vault lease before publication. Enable proves the protected
vault is usable first. Disabling needs only the publication file lease and consent write, so it
works even if the vault is damaged or unavailable. The UI reads consent independently from account
credentials. Publication rechecks consent; re-enabling advances the persisted sign-in epoch,
so an earlier check cannot save after disable/re-enable. Turning it off retains existing profiles. Discovery
verification releases the vault lease; publication reacquires it, rechecks consent, then its ticket
(epoch and recovery), exclusions and exact native credential equality, then holds the native lock
through encrypted save.
Logout suppresses the outgoing credential fingerprint so an unchanged failed logout is not silently
remembered again. A later verified native sign-in with a new credential can be remembered.

The encrypted journal is persisted before any native write. ConfigIo owns narrow oauthAccount
configuration publication, validation and rollback; the account journal is its protected backup
participant. Claude credential publication merges only `claudeAiOauth`, preserving current MCP
OAuth entries. Every account change replaces the shared Limits reading, and the Codex subscription
dates read from the logins, through `read_revision` once its leases are released: save current and
remove for the provider whose card they add or remove. Use and sign out do so even when their
native half fails, because a rollback or a failed logout may still have changed the native login;
a sign-in does so only once published, and a change refused before it wrote anything is not
announced. A category edit announces only the account list.

## Archived accounts

Archive account puts a Limits account away without losing it: the saved profile and its remembered
reading stay, so it comes back without signing in again. Only the user archives an account, from a
card's menu or, when its subscription is known to have ended without renewing, from the card's
footer in place of Use account. Nothing archives one on a timer, at expiry, or for disuse. The
signed-in card cannot be archived.

- **Storage.** The archived ids live in plaintext in `<home>/.on-n-off/limits/archived.json`,
  per provider, beside the snapshots and keyed by the ids Forget takes: a card's own id and the
  legacy ids merged into it, so its legacy history is archived with it. Writes hold the snapshot
  lock and replace the file whole. The file needs no vault key, so the popover and a locked vault
  honour it, and archiving a history card never creates a vault. A missing or malformed file
  archives nothing; the next write replaces a malformed one.
- **Dormant.** Saved polls skip an archived profile entirely: no move into a home, no usage read
  and no renewal of any kind. A private renewal runs only inside a poll, so it never runs for an archived login, and the
  renewal rules above are unchanged. If a provider expires an unused login meanwhile, unarchiving
  shows the usual sign-in-again state.
- **Shown.** The shared read flags archived cards (`archived`), after the legacy history they
  replaced is hidden, and the account list flags archived profiles. The Limits screen lists them
  collapsed under each provider's cards, with Unarchive and Remove account; the popover shows none.
- **What unarchives.** Unarchive, which then reads the provider again, forced, so the account comes
  back polled. Save account, Add account and Sign in again, as they undo a Remove. And becoming the
  signed-in account, whether the switch happened in on-n-off or in the provider's CLI: a provider
  read unarchives the account it names under the cache lock, before its saved polls and before it
  flags the cards, and announces the account list outside the lock, only when the archive changed.
  Its legacy id comes along only while that history is the one the account replaced, by the rule
  that hides it (the same email), since in a shared workspace it can hold another member's.
  Automatic remembering never unarchives.
- **Removed.** Remove account on an archived account works as it does on a card, through the same
  sequence and under the same blocked rule: never while the account list is loading or failed,
  when no card knows its saved login. Forget takes every id whose snapshot it deletes out of the
  archive.

## Native scope and limitations

The guaranteed target is the default native CLI home. Custom native homes, selected Codex config
profiles, ephemeral/alternate Codex backends, environment credentials and detected forced-login
policies currently defer to the official client. A Claude home chosen by `CLAUDE_CONFIG_DIR`, and a
store `CLAUDE_SECURESTORAGE_CONFIG_DIR` moved (to another dir, or to a scoped Keychain entry by
naming the default dir), are such custom homes: account changes refuse them, while Limits and the
renewal follow them where Claude Code keeps the login. Set but empty, that variable leaves the
default store in place. Existing model/endpoints/API-key configuration is
never rewritten to simulate a switch. Native Codex file/keyring/auto backends are selected from
config; unreadable protected storage is not treated as a missing login. macOS native account reads use the same system `security` reader as Limits, finding
Claude Code's item under its own account name before the account a service-only lookup names,
with a bounded subprocess deadline, and native account writes and
removals go through the same tool (`accounts/keychain.rs`), never the process's own ad-hoc-signed
Keychain identity, so one "Always Allow" survives updates and Claude Code's own refreshes. Claude's Keychain entry is
scoped, its name derived from the NFC-normalized storage path, for an isolated sign-in and wherever
`CLAUDE_CONFIG_DIR` or `CLAUDE_SECURESTORAGE_CONFIG_DIR` chose the store; an isolated sign-in never
inherits the latter.
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
that merely names the provider, and names each client after the app bundle it runs in or was
started from. Only the scan behind the prompt leaves out processes on-n-off started: the checks that
gate a change run under the account-change lease, which already keeps on-n-off's own reads from
running, and Windows keeps a dead parent's pid on its children and reuses pids.

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

New usage and subscription observation keys contain both user and workspace. Legacy observations remain
historical, with no inferred ownership. Unattributed session usage cannot advance new scoped
profiles. on-n-off does not read Claude Desktop's organization-only usage samples at all.
Subscription terms appear directly on account cards. The term (whether the plan renews) is read during
the usage read with the login's own access token: the signed-in login's after the app-server identity
check, a saved profile's from the vault, without changing the active CLI; it is remembered with the
card's other figures. The fallback date comes from the ID token of the signed-in login or of the saved
profile, read locally, and its queries share one cache that account changes invalidate.

## References and verification

The implementation follows selected invariants from [CodexBar](https://github.com/steipete/CodexBar/tree/d394565751ab6b3c27f0c9c1e7879647ac5d029d),
[claude-swap](https://github.com/realiti4/claude-swap/tree/7187ce83b444c6af7b61ec8ee092623566a2d8fa),
and [CC Switch](https://github.com/farion1231/cc-switch/tree/1d5d90f4aba88447d422a16cdec5282ec5331fd7).
Third-party notices are retained in the repository notice file. No manager/proxy runtime is embedded.

Focused tests use disposable homes, fake native verification boundaries, real file publication,
authenticated encryption and browser/HTTP fixtures. They cover identity separation, newer outgoing
generations, wrong-user reauthentication, cancellation, partial publication, recovery, MCP
preservation, corrupted vaults and browser workspace membership. Tests do not use personal accounts.
The account operations, sign-in, remembering and the saved accounts' usage refresh run on
`Accounts` (`accounts/mod.rs`), which tests build from a scratch home whose vault the fixture key
unlocks, a fake native store that reads each provider's logins by that provider's rules, a fake
isolated sign-in whose official client is a stub, fake running clients and a notifier that
records what it heard; a test build fails any test that reaches the OS credential store. Each
adapter's native store is tested beside it (`claude/tests/`, `codex/tests/`), with a scratch home,
an injected environment and managed-policy paths, and a fake `security`.

Real browser sign-in, provider revocation, macOS Keychain interoperability and Windows credential
storage require designated-account validation on those platforms. Fixture tests and a read-only
app boot do not prove these live operations. Full CI belongs to the PR, not local execution.

A vault that cannot be read right now is a retryable IPC error for the subscription read, not an
absent date.

Account management is embedded in Limits cards. The Limits header contains one Add account picker for Claude and Codex;
each card owns its usage, subscription date, primary action and optional category. Saved identities
join observations only by exact observation key, and remain visible when usage reads fail.
The picker and Settings share the global automatic-saving opt-in preference. Inactive Remove account removes
the saved login and usage card; active Remove saved login keeps the native login and usage card.
Operations remain busy through readback, and a sign-out confirmation is checked against the
latest known native observation key before invoking the provider action.

A failed account-list unlock exposes a Retry action beside the error. It unlocks the existing
vault and reloads accounts; it does not start sign-in, create a vault, or change a CLI login.

Card headers never display workspace IDs. Saved profiles keep their verified workspace identity;
accounts that share an email are told apart by their plan badge when usage reports one, and by the
optional category otherwise.
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


