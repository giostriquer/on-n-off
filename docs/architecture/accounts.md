# Account profiles

`accounts/` implements opt-in saved subscription logins and explicit activation in the default
native Claude/Codex/Muse CLI store. It adds no proxy, request router, inference traffic or automatic
account rotation.

## Ownership

A profile has a random local UUID and a stable provider/user/workspace identity. Email is the account name; an optional free-text category is separate display metadata, kept per account rather than per profile (below).
Neither field is an identity key. Active native storage is authoritative. A saved active credential is a
non-refreshing shadow; switching away captures the latest native generation. Inactive access-token
expiry does not discard a renewable login. A saved Claude account that is not the signed-in one
keeps its login in its own home (below), where Claude Code renews it; Limits reads it there through
Claude Code's own usage report, never with the token. A Claude login still in the vault, one the last
read could not move into its home, is never sent: its read fails until a later read moves it. Limits
polls saved Codex accounts with access-only HTTP requests without changing the active native login,
through Codex's Limits reader, which the adapter dispatches (`read_usage`). At most two saved reads
per provider run at once, with the Limits failure backoff per account
(`limits_refresh::failure_backoff`) and provider retry-after handling, whether the login is in the
vault or in its home. A login its provider refused, and one that now signs in as a different
account, waits for a changed generation and renews nothing, in its home as in the vault (Login
state, below).
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
signed-in one. Claude and Muse have homes; Codex has none. Claude's (`accounts/claude/home.rs`) is
a persistent Claude config dir. Claude Code renews the login there whenever Limits asks it for the
account's usage (`limits::claude_cli`), so on-n-off sends no request with that login and sends no
grant for it. Muse's (`MuseHome`, `accounts/muse.rs`) keeps its login inline in the home's own
`auth.json` and never renews (Provider adapters, below).

Renewal never outlives the sign-in itself: Anthropic ends each one at the `refreshTokenExpiresAt`
Claude Code stores with it, and only signing in again starts a new one. The account listing reads
that date from the default store (`NativeAccount::sign_in_deadline`) and from each saved login,
credited only to the account the login's `oauthAccount` names: a login in the vault from its view
(`LoginView::sign_in_deadline`), already decrypted, and a login in its home from the date its
profile records (`sign_in_end` on `Profile`, `SignInEnd` in `accounts/login_state.rs`), which is
not a secret. on-n-off records it wherever it already holds the login: every save (save current,
remembering, a sign-in's publication, a switch's capture of the outgoing login), the move into a
home at check-in, the move out of it at check-out, and a forgotten home taken back. A profile an
older version wrote has no record; its home is read once (`Home::sign_in_deadline`, through the
store's own read) and what that read found, a date or none, is recorded with a metadata change
(`ChangeKind::Metadata`), so a home is read at most once, never once per listing. A read that
fails, a Keychain prompt denied or unanswered or a file that cannot be read, records none too, on
purpose, so it never asks again; that home shows no date until on-n-off next holds its login. The limit: a
login replaced in its home outside on-n-off, by running `claude /login` with that config dir, keeps
the date recorded for the login it replaced until on-n-off next holds the login. A sign-in awaiting use is newer than
whatever the default store still holds for its account, so it reads only its own login; otherwise
the default store's date comes first. The card warns through the last seven days ([PROVIDERS.md](../../PROVIDERS.md)). The date survives
Claude Code emptying a login Anthropic refused, so a sign-in that has already ended still says when
for a saved account, in the vault or its home. It does not for a signed-in account with no saved
login: once Claude Code empties its ended login, the default store holds no login, so the account
is no longer the signed-in one and lists as `missing`, its card offering Sign in without the
Sign-in ended tag.

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
user) set the profile's home aside, and Remove takes the profile with it; Remove account also
retires the home in the same call (Remove account, below). A home no profile names
then goes at the next read of its own provider (Owners, below): every read lists the homes on disk
before it reads the vault, and tears down each one no profile names that its provider owns, through
that provider's home: a Claude home under its own Claude Code locks, its Keychain entry first, then
the directory; a Muse home as a directory, since it keeps nothing outside it. A home's id is
recorded in the vault before anything creates it, so one listed first and named nowhere is one
nothing will use again; one its client holds, or that could not be removed, goes at a later read.

The exception is a version before homes. It reads a vault with homes, since every field it does
not know is skipped, but its next write of the vault drops the field that names them, and such
writes are routine (remembering a renewed native login is one). That leaves a homed account with
no login anywhere but its home. So the read first looks at what an unnamed home holds: a login
that signs in as a saved account with neither a login nor a home is taken back by that account,
unless Sign out left the account so, which it records on the profile (`signed_out`) in the same
vault change; every other home the provider owns is torn down. A home whose login cannot be read
right now is left for a later read, since it may be the account's only login. The rule has one
limit: a version before homes drops `signed_out` with `home`, so a sign-out it made, or one whose
vault it rewrote before the signed-out homes went, leaves nothing to tell by, and the signed-out account takes its homes back;
their reads then say whether that sign-out ended their logins. The teardown never
runs `claude auth logout`: whether a logout ends only its own login or every login of the account
is unproven, and a logout that ended the others would sign the user out elsewhere.

**Owners.** Every provider's homes share `accounts/homes/`, and each provider's read runs on its
own. Each home therefore belongs to one provider, and only that provider's home moves a login into
it or out of it, takes it back, or tears it down. Before this rule, a Muse read took an unnamed
Claude home for an empty Muse home and deleted only its directory, which left the home's
`Claude Code-credentials-<hash>` Keychain entry behind with nothing that would ever find it again
(the service name hashes a path that no longer exists); and a Claude read deleted a Muse home that
a Muse read would have taken back, its account's only login.

A home records its provider in its own `provider.json`, the provider's name and nothing secret,
the file and format a sign-in's isolated home records its provider in. Check-in writes it under the
home's locks before the first login goes in, and a home taken back gets it too. One rule decides a
home's owner (`homes::owner`), asked by check-in, check-out, the take-back and teardown of an
unnamed home, and Remove account's retire, each before it takes the home's locks:

- The provider its record names, whatever else the home holds.
- With no record, the provider whose profile names the home (for Remove account, the profile it
  just removed).
- With no record and no profile naming it, a home from a version before records, the one provider
  whose layout it holds, found by looking at directories alone (`Home::recognizes`): a Claude home
  is the directory with a `.claude` config dir in it, which Claude Code's locks create before
  anything is written there, and a Muse home the one with a `config/muse` dir. Neither reads a
  login, a file's contents or a Keychain item, and a linked directory does not count.

A home whose record cannot be read (malformed, or naming a provider this version does not know),
with no layout, or with both layouts belongs to no provider. No read takes its locks, reads its
login or deletes it, and it stays on disk until it is removed by hand: a home whose owner cannot be
told may hold an account's only login, and the wrong provider's teardown would leave a Keychain
entry behind. A check-in or a switch refuses a home another provider owns ("This account's home
belongs to another provider, so it was left alone."), and the account's login stays in the vault.
Keychain entries an earlier version left behind this way are not found: on-n-off never lists the
Keychain, and the path each one hashes is gone.

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

## Login state

Every listed profile carries one login state (`loginState` on `ProfileDto`, `LoginState` in
`accounts/login_state.rs`), and so does the profile on each Limits column entry; it is all a Limits
card reads to ask for a new sign-in, for every provider:

- `usable`: nothing says otherwise;
- `ending`, with the date its sign-in ends: the adapter knows the date (Claude's
  `refreshTokenExpiresAt`, Homes above; Codex and Muse report none) and it has not passed;
- `ended`, with that date, once it has passed;
- `refused`: the saved poll parked the login because its provider refused it or found it signed
  out;
- `otherAccount`: the saved poll parked it because it now answers for a different account;
- `missing`: the profile keeps neither a vault login nor a home.

When more than one applies, the first of missing, ended, another account, refused and ending wins,
so a sign-in that ended still says when after the refusal its end brought. The signed-in
account's state is its date alone, `usable`, `ending` or `ended`, never `missing`, `refused` or
`otherAccount` (`Standing::SignedIn`): the saved poll never reads it, its own read names what
failed, and a signed-in account with no saved login (signed out in on-n-off, then signed in again
in the CLI) still shows when its sign-in ends. Whether a date has passed is decided here, when the
account list is read and whenever the Limits column is served; the UI reads the account list again
just after the nearest `ending` date it has not been read since, and when the window shows again
after such a date passed while it was hidden ([shared-reads.md](shared-reads.md)).

The Limits column states each profile as the account list would (`Roster::in_column`,
`accounts/roster.rs`, through the one `stated` the listing uses too), from facts it already holds and
with no login, Keychain item or stamp read, when the column is served:

- signed in for the account of the read the column is served with (its current card), not the
  native store's;
- the signed-in login's date only from the ends the account list last handed over
  (`Notify::listed`), never from the saved copy: a login that reached its end and was signed in
  again in the terminal has a later end than the copy, so it shows no date until the account list
  hands its native one over;
- a saved login's date from those ends, else the end recorded on the profile (`sign_in_end`,
  recorded when a login is saved and when a home's is first read), never by opening the login;
- the verdict the saved poll parked, looked up in memory as the column is served
  (`usage::parked_for`), for the login the roster listed (`LoginKey`: the vault login's fingerprint,
  taken from the vault already unlocked for the roster, or the profile's home). The verdict is not
  part of the roster, so a reread that overtakes the poll and wins the roster cannot drop it, and the
  read that ran the poll announces the column. A new login in the same home shows once the saved
  poll reads it, which the Limits read that follows the account change runs.

The saved poll is one path for a login in the vault and one in its home (`accounts/usage.rs`):
each attempt is keyed by the profile and kept with the generation it read: the vault login's
fingerprint, or the home's id with its login stamp (`Home::login_stamp`). The poll and the listing
resolve where each profile's login is kept once (`usage::kept`: the vault login, or its home) and
take its generation from that. A rate limit holds the
next attempt until its Retry-After, any other failure the Limits failure backoff, and a refusal or
another account parks the login: no attempt, forced or not, reads it again until its generation
changes, by a new sign-in, a switch that captured a renewed login, or the provider's client
renewing it in its home.

A login stamp is evidence that changes whenever the home's login is written, and it never reads the
login itself. A Claude home's is the attributes of its Keychain item (`find-generic-password`
without `-w`: its account and modification date, which ask macOS for no access to the secret), under
Claude Code's own account name first, then any, beside its `.credentials.json`'s metadata; a Muse
home's is its `auth.json`'s metadata. File metadata is the file's id, size and modification time,
read without opening the file. Reading the login instead would read Claude's item with `-w` from a
background poll: Claude Code rewrites the item whenever it renews the login in the home, and macOS
then resets the item's partition list to Claude's signing team, so that read would raise a
"security wants to use" prompt. The vault login's fingerprint comes from the vault already
unlocked for the poll. A stamp that cannot be taken mints no generation: the poll reads nothing and
replays the verdict it has, and the listing keeps the parked verdict. The listing takes the poll's
verdict only while the profile's generation is still the one parked (`usage::parked`), so a changed
login is usable at once. A Muse login refused (401) in its home used to be read again on the backoff, while
the same refusal in the vault waited for a new login.

The poll's attempts live in memory, as its holds always have, and are not kept across a restart:
the first saved poll after a restart finds nothing held, reads every saved login and parks a
refused one again, so the state comes back within one Limits read; until then the listing says
usable, or ending or ended from the date. A poll that parks or clears a verdict changes the roster
its saved refresh lists for the Limits column, so the Limits read that ran it carries the new state
with its own announcement; the account list is not announced for it, since nothing outside Limits
shows a login state, and reads the verdicts when it is next read.

## Provider adapters

Everything account code does differently for Claude, Codex and Muse sits behind one accounts-local seam,
separate from the catalog's `AgentAdapter` (which accounts touch only through
`supports_accounts`, the IPC gate). `accounts::adapter` is the one place that tells the providers
apart; it gives the provider's `Adapter`, `claude::Claude`, `codex::Codex` or `muse::Muse`, and generic account
code asks that rather than which provider it has. An adapter:

- resolves the provider's native stores, which the operations reach through the `Accounts`
  context's `NativeStores` resolver: `ClaudeNative` over `claude_store.rs`, `CodexNative` over
  `codex_store.rs`. The user's own store is the transaction's `Native` and a `NativeAccount`: its
  preflight (custom homes, environment credentials, managed policy), its sign-out, and the
  signed-in subscription whose saved accounts' usage is read beside it (an API-key Codex login
  has none). A private one in a directory is an `IsolatedSignIn`, with the official sign-in
  command, the page that command prints for the user to open when it opens none itself (only
  Muse's does; the `Accounts` context's `Browser` opens it once, and a page that cannot open ends
  the sign-in at once), the first usage reading and the cleanup of anything the sign-in left
  outside that directory;
- reads a login through its typed view, `ClaudeLogin` or `CodexLogin` (`accounts::view`), so
  nothing else reads a login's JSON: identity and email, the credential generation's fingerprint
  and whether a saved login is due to renew. A view only reads; the bare credentials document
  Claude's store holds, without its account record, is read directly once, by `ClaudeNative::read`,
  which checks that it still holds an access token. A `Login` keeps its stored shape, `{auth, account}`, so the vault, the
  recovery journal and the renewal journal written by earlier versions still load, and
  `model.rs` keeps the one fingerprint layout every version has hashed. Whole documents are still
  written raw: Codex's `auth.json` verbatim, Claude's `claudeAiOauth` merged into its credentials
  document beside the `oauthAccount` record `ConfigIo` patches;
- reads a saved profile's usage with its login still in the vault (`read_usage`), through the
  provider's Limits reader, asking the services it is given (`limits::SavedReadUrls`): Codex's
  access token to `limits::read_saved_codex`, the `wham/usage` body app-server itself reads,
  without starting a CLI, and a login without an access token is refused first. Claude sends
  nothing: a saved Claude account is read in its home (`Home::read_usage`, Claude Code's own report
  through `limits::claude_cli`), and one whose login is still in the vault reads as not moved yet,
  for the next read to try again. Claude's first usage after a sign-in is Claude Code's own report
  in the isolated home (`limits::claude_cli`); Codex's runs app-server there;
- renews a never-activated private login at its token endpoint (`token_url`): Codex's grant, built
  and folded by its login and sent from `usage_renew.rs`. Claude has no token endpoint
  (`token_url` is `None`) and refuses a private renewal, since it never renews privately;
- says how its client processes are recognized and whether they refuse an ordinary switch.

Sign-in, the cleanup of abandoned sign-in homes, automatic remembering and the saved accounts'
usage refresh are methods on the same `Accounts` context as the six operations, so they resolve
native stores and reach running clients and listeners the same way. A remembering poll announces
the account list once when it saved a login or changed the provider's notice. Vault access stays
generic: a saved Codex profile's claims are found in the vault by `accounts::saved_codex_claims`
and read through the Codex view.

on-n-off redeems no Claude token and sends no Claude grant: Claude Code renews every Claude login
itself, the signed-in one whenever it runs and a saved account's in its home. Where Claude's login
lives and how it is locked belongs to `accounts/claude_store.rs`, which the account operations,
automatic remembering and the homes share: Claude Code's dirs as it resolves them
(`CLAUDE_CONFIG_DIR` untrimmed and NFC-normalized, with `CLAUDE_SECURESTORAGE_CONFIG_DIR` moving
the credentials, the locks and the Keychain entry's name), the store Claude Code's own read would
use (a Keychain item that parses, token or not, else the credentials file; an unreadable Keychain
refuses every write), the Keychain item under Claude Code's own account name first, and one lock
protocol, which a write takes as Claude Code takes it around its own renewals. An emptied
`claudeAiOauth`, Claude Code's sign-out, is no native login. The Limits readers (`limits/`) read no
Claude credential at all; for Codex and Muse they consume access-only projections. The one projection that
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

Muse is the second exception, also the user's decision (2026-10-16). `accounts/muse_store.rs`
reads Muse Code's signed-in device login, inline in its `auth.json` or else its macOS Keychain
item. For the Muse Limits read (`limits/muse.rs`) it yields the login's email, which names the
account on every path, and one `MuseCredential`: the access token, as `model::AccessToken`; an
API key; an expired login; or a Keychain login it could not read. A login entry with no credential
is no login. The mint read sends one POST per read to the key-mint endpoint the CLI calls itself
(`limits/muse/mint.rs`) and keeps only the quota fields of the answer; Limits never starts Muse.
Saved accounts use the same store for whole logins, as the next paragraph describes.

Muse saved accounts are a later decision of the same day, reversing "Muse has no saved accounts"
because every provider should offer the same account actions. `accounts/muse.rs` is the third
adapter. A Muse login's identity is its lowercased `user_email` (Muse's `auth.json` carries no user
id); its secret is `{access_token, api_key}` and the rest of `providers.meta` is its
metadata. Muse 1.4.10 stores the signed-in login either inline in
`auth.json` (file backend, Windows) or in the macOS Keychain item `ai.meta.dev.credentials` /
`meta` with `storage: "keychain"` metadata; neither holds a refresh token, so a Muse login never
renews and on-n-off never renews one: an expired saved login asks for Sign in again.
`accounts/muse_store.rs` owns both files: one parse of `auth.json` (an inline secret, else the
Keychain item when the metadata says `storage: "keychain"` or the login is OAuth), one Keychain
lookup and one writer. The lookup is cached in memory by `auth.json`'s path, content and
modification time, and the cache keeps only the parsed `{access_token, api_key}`; a store that
cannot reach the Keychain (Windows, a disposable QA home, a saved login's home) never reads it
from the cache. A denied lookup is an error for accounts, never "no login". A login with any
credential is a login: an API-key login fails identification, so a switch is refused rather than
overwriting it. Limits and accounts tell the two kinds apart by one rule,
`muse_store::credential_of`, which reads the secrets: an `access_token` makes a device login
whatever `mechanism` says, and an `api_key` alone makes an API-key login. Every store yields the same saved secret,
`{access_token, api_key}`. Activation writes it back to the store Muse already uses: the same
Keychain item through `/usr/bin/security`, keeping its `secret_schema_version` (1 when there is
no item yet), or inline; `auth.json` is replaced atomically with other providers' entries kept,
as Codex's credential file is, without Muse's `.auth.json.lock` (Muse never renews, and a switch
beside a running Muse asks first). A malformed document or item is refused, never repaired.
macOS resets an item's partition list to its writer's, so after that write the list holds only
`apple-tool:` and the next Muse process, signed by Meta, asks once for its own login; "Always
Allow" adds Muse's team until the next activation (verified 2026-10-19 in the `securityd` log).
Muse has no command that stores a device login (`muse auth set` takes only API keys), so on-n-off
cannot write the item as Muse. on-n-off therefore never starts Muse in the background: the Muse
Limits card comes from `auth.json`, the Keychain item and the mint read alone.
A saved, non-active Muse login lives in its own home under `accounts/homes/`, run with `HOME`,
`XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `TBH_CREDENTIAL_BACKEND=file`, so its login stays inline in
that home's `auth.json` and never touches the global Keychain item (verified 2026-10-16 with a
real `muse login`); its usage is one key-mint read with that home's token, and the home is removed
as a directory, never with `muse logout`: nothing of it lives outside the directory. Sign-in runs
`muse login` in an isolated home of that
shape. Without a terminal Muse opens no browser and asks for no Enter: it prints its device-code
page (`https://auth.meta.com/oauth/device/?code=…`) and the code on stdout and waits, so on-n-off
opens that page itself (verified 2026-10-17, Muse 1.4.10, with stdin closed and with an Enter
sent). Running `muse-bin-*` processes keep their login, so a switch
beside one asks first, as Codex does.

## Operations

- **Remember accounts on this device** is off by default. One global opt-in enables a background
  check for native Claude/Codex/Muse logins about every 30 seconds, independently of notification settings.
  Unchanged saved logins and pending reauthentication are skipped. A new/changed native login is
  verified before saving; no native activation occurs. Codex's verification can let app-server,
  the native renewal owner, renew an expired login; Claude's is local and accepts an expired one,
  which Claude Code renews itself. Switching again before a check can cause the previous login to
  be missed.
- **Save current** verifies native readback and saves its latest renewable login. It also explicitly
  reenrolls an account excluded by Remove.
- **Edit category** accepts optional free text (up to 100 characters). Blank clears it. Categories
  survive refresh, reauthentication and switching. The provider email remains the primary name;
  missing email is shown as unavailable. Older custom names migrate to categories when loaded.
  A category belongs to the account, not to a saved login: the vault's `categories` map holds one
  text per provider and card account id (the observation key), so every Limits card can have one,
  for every provider, saved or not. `read_account_categories` and `set_account_category` need no
  account adapter, and the setter answers with the stored text. The Limits cards take their
  categories from the Limits read's roster (the Limits column, below), which a saved edit, a
  forget and Remove account reread (`Notify::roster`, `limits_refresh::account_actions`); the UI no
  longer reads the categories itself. `Database` keeps a saved login's
  profile copy in step with the map on every set and forget, and a login saved after its card was
  named takes the category, so an older app version still shows it; a profile category from an
  older version moves into the map when the vault loads. Remove account drops the categories of
  every id the card holds, legacy ids included, in the vault change that removes its saved login
  (Remove account, below); the older Forget command drops those of every id it forgets.
- **Add / sign in again** launches official CLI authentication in an isolated native home. It checks
  the resulting identity, verifies the expected profile during reauthentication, then saves it.
  Before publication it attempts a usage/plan read using that isolated login: Codex uses app-server
  without forced renewal; Claude runs `claude -p /usage --safe-mode` there, which makes no model
  request.
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
  native client. This is Remove saved login on the signed-in card; Remove account on any other
  card removes the profile together with its usage history in one operation (Remove account,
  below).

An in-process reservation and a cross-process shared/exclusive activity lease exclude provider
reads from account activation. A separate lease serializes vault access. Existing vault keys are unlocked before acquiring the shared storage lease, so an OS prompt does
not block another provider's file transaction. Initial key/vault creation remains serialized.
Brief contention waits on blocking workers (up to ten seconds); it never bypasses the lease or replaces its lock file. Every account change releases its leases, the vault lease and the provider's activity reservation, before it is announced. Every file lease is a `FileLease` (`file_lease.rs`), which unlocks explicitly when dropped: on Unix the lock belongs to the open file, and a child process that another thread spawns meanwhile shares it until the child execs. Native Claude locks
(Claude Code's refresh lock, its legacy lock beside the config dir's real path, and the config
file's lock) cover the outgoing reread, durable journal and publication; the credential write goes
through `claude_store::begin`, the one writer of Claude's credentials, which takes Claude Code's
`.storage-write.lock` and reads the store under it; its proof, made before either half of the
change is written, refuses a linked credentials file. A lock that cannot be taken at all is reported with its reason;
only one another process holds reads as Claude being busy. They are released for verification, while the
exclusive activity lease remains held through completion or recovery: the locked write consumes
the lock guard and returns the store as read back under it, so verification, which runs the
provider's own client, cannot run while they are held: a client that took the same locks to renew
would wait on on-n-off while on-n-off waited on it. Every held lock has a heartbeat. Claude verification is local: it
identifies the published login from the exact resolved native identity file, including legacy
configuration, then runs `claude auth status --json`, which reads only what is stored and asks
Anthropic nothing, and requires `loggedIn: true`, that login's organization as `orgId` and, when
the login names an email, that email as `email`; the store read again must hold the same login. It
never renews: an expired incoming login is accepted and Claude Code renews it on its next run, and a
login Anthropic refuses shows as a failed read at the next Limits poll. A local check rather than a
`/usage` run is the user's decision (2026-09-29). All network
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
Remove saved login for the provider whose card they add or remove. Remove account edits the
reading in place instead of reading the provider again (below). Use and sign out do so even when their
native half fails, because a rollback or a failed logout may still have changed the native login;
a sign-in does so only once published, and a change refused before it wrote anything is not
announced. A category edit or forget announces the account list and rereads the provider's Limits
roster, which announces the Limits read when the roster changed.

## Archived accounts

Archive account puts a Limits account away without losing it: the saved profile and its remembered
reading stay, so it comes back without signing in again. Only the user archives an account, from a
card's menu or, when its Codex subscription is known to have ended without renewing, from the card's
footer in place of Use account. Claude reports no subscription term, so a Claude card offers it
only from its menu. Nothing archives one on a timer, at expiry, or for disuse. The
signed-in card cannot be archived.

- **Storage.** The archived ids live in plaintext in `<home>/.on-n-off/limits/archived.json`,
  per provider, beside the snapshots and keyed by account id. Archive account archives a card's
  own id and the legacy ids its column entry folded into it (`replaced`), which Rust reads from
  the column it serves (Remove account, below), so its legacy history is archived with it. Writes hold the snapshot
  lock and replace the file whole. The file needs no vault key, so the popover and a locked vault
  honour it, and archiving a history card never creates a vault. A missing or malformed file
  archives nothing; the next write replaces a malformed one.
- **Dormant.** Saved polls skip an archived profile entirely: no move into a home, no usage read
  and no renewal of any kind. A private renewal runs only inside a poll, so it never runs for an archived login, and the
  renewal rules above are unchanged. If a provider expires an unused login meanwhile, unarchiving
  shows the usual sign-in-again state.
- **Shown.** The shared read flags archived cards (`archived`), and its column folds the legacy
  history they replaced into them and flags a saved profile no read answered for as archived when
  its profile is, unless it is the native login (`limits::column`). The account list flags archived
  profiles too. The Limits screen lists them collapsed under each provider's cards, with Unarchive
  and Remove account; the popover shows none.
- **What unarchives.** Unarchive, which then reads the provider again, forced, so the account comes
  back polled. Save account, Add account and Sign in again, as they undo a Remove. And becoming the
  signed-in account, whether the switch happened in on-n-off or in the provider's CLI: a provider
  read unarchives the account it names under the cache lock, before its saved polls and before it
  flags the cards, and announces the account list outside the lock, only when the archive changed.
  Its legacy id comes along only while that history is the one the account replaced, by the rule
  that hides it (the same email), since in a shared workspace it can hold another member's.
  Automatic remembering never unarchives.
- **Removed.** Remove account on an archived account is the same operation as on a card, under
  the same blocked rule: never while the account list is loading, failed or waiting for recovery,
  when the vault the removal changes may not open. It takes every id whose snapshot it deletes out
  of the archive.

## Remove account

Remove account and Archive account are one Rust operation each, which the Limits screen calls
once with the card's provider and account id: `remove_limits_account` and
`set_limits_account_archived` (`limits_refresh/account_actions.rs`). Rust works out what the card
holds from the Limits column it serves: the card's own id and the legacy history its entry folded
in (`replaced`), never ids the UI passes. The signed-in card is refused, as it offers neither. A
remembered card with no saved login is the same call.

Before it changes anything, while it plans the removal under the Limits cache lock, the snapshot
store checks that every replaced legacy snapshot still carries the email the column folded it by
(`SnapshotStore::check_card_history`); a legacy with no email to check, and one that changed
hands, refuse the whole removal, so the saved login stays. Remove account then changes two
stores, in this order:

1. **The vault.** One account change, under the vault lease and its atomic replacement, removes
   the saved profile whose observation key is the card's id, keeps its identity out of automatic
   remembering as Remove does, and drops the categories of every id the card holds
   (`accounts::forget_account_at`). The profile is found in the vault it opens, not in the column.
   Without one it is a metadata change, which bumps no sign-in epoch and is allowed during
   recovery; without a vault it changes nothing and creates none. It opens the vault as Remove
   did, with this session's key or the OS store's, and reads no native login, no home and no
   login secret. If this step fails, nothing has changed and nothing is announced: a vault that
   does not open refuses even a card with no saved login, which the blocked rule above keeps from
   being asked. Once the vault lease is released, and outside the cache lock, the home the removed
   profile named is retired with that profile's own provider's home, once the owner rule says the
   home is that provider's (Owners, in Homes above): under the home's own locks it deletes the
   home's login (on macOS Claude's scoped Keychain entry) first, then the directory, reading no
   login on the way (`Accounts::retire_home`). A home that cannot go now, because its client holds
   it, it cannot be deleted or it records another provider, is left for a later read of its owner,
   which tears down every home no profile names that it owns (Homes, above).
2. **The usage history**, under the Limits cache lock, so no read of the provider runs beside it:
   the snapshot store checks every replaced legacy snapshot's email again, the final guard,
   then deletes them, the card's own snapshot and any legacy that one supersedes on disk, and
   takes every deleted id out of the archive, all under its write lock
   (`SnapshotStore::forget_card`). History that changed hands since the plan is refused before
   anything is deleted. The cache drops the cards it deleted.

When the history step fails after the saved login went, the one error says both ("The saved
login was removed, but its usage history was not: …"), adding why the home was left when it
was, and the history stays as a remembered card without its saved login, which Remove account
removes. A home left behind while everything else went is no error. Then the roster is reread
with the session's key, when the vault changed, and the Limits read and the account list are each
announced at most once: the Limits read when the column changed, the account list when the vault
did. The provider's usage is not read again, so the card goes as soon as the announcement is
answered, and discovery is not woken.

Archive account writes the archive under the cache lock and flags the cached cards, rereads the
roster when the archive changed, and announces the same way. Unarchive then reads the provider
again, forced (`AccountStores::read_again`); that read announces its own replacement, and the
unarchive announces only what it did not, so each source is still announced once. The UI patches
no cache; it waits for the announcement. The earlier commands stay registered with their shapes
and their `api.ts` wrappers, though no screen calls them: `forget_limits_snapshot` forgets one
snapshot and the legacy it supersedes, as before, and drops every card it forgot from the cache;
`set_limits_archived` archives the ids it is given through the same path as Archive account.

## Native scope and limitations

The guaranteed target is the default native CLI home. Custom native homes, selected Codex config
profiles, ephemeral/alternate Codex backends, environment credentials and detected forced-login
policies currently defer to the official client. A Claude home chosen by `CLAUDE_CONFIG_DIR`, and a
store `CLAUDE_SECURESTORAGE_CONFIG_DIR` moved (to another dir, or to a scoped Keychain entry by
naming the default dir), are such custom homes: account changes refuse them, while Limits follows
them, running Claude Code's report there. Set but empty, that variable leaves the
default store in place. Existing model/endpoints/API-key configuration is
never rewritten to simulate a switch. Native Codex file/keyring/auto backends are selected from
config; unreadable protected storage is not treated as a missing login. macOS native account reads use the system `security` tool, finding
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
reads whatever is on disk. When a client replaced them, or verification fails beside running
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
isolated sign-in whose official client is a stub, fake running clients, a notifier that
records what it heard and a browser that records the pages it opened and can refuse one; a test build fails any test that reaches the OS credential store. Each
adapter's native store is tested beside it (`claude/tests/`, `codex/tests/`), with a scratch home,
an injected environment and managed-policy paths, and a fake `security`.

Real browser sign-in, provider revocation, macOS Keychain interoperability and Windows credential
storage require designated-account validation on those platforms. Fixture tests and a read-only
app boot do not prove these live operations. Full CI belongs to the PR, not local execution.

A vault that cannot be read right now is a retryable IPC error for the subscription read, not an
absent date.

Account management is embedded in Limits cards. The Limits header contains one Add account picker for Claude, Codex and Muse;
each provider in it waits by the rule its cards' account actions wait by (`blocked`, `AccountManager.tsx`):
while that provider's account list is loading, cannot be read or needs recovery, and while one of
its account changes runs. Each card owns its usage, subscription date, primary action and optional category. A saved
profile's login state decides, for every provider, whether its card asks for a new sign-in: ended,
refused or another account make Sign in again the primary action, with the state's sign-in tag and
its message in place of the read's, and a refused login or another account keeps Use account in
the ⋯ menu; ending shows its date through the last seven days; missing offers Sign in. The card frame
(`LimitCardFrame`) draws each card's ⋯ menu and footer from the card action plan
(`cardActionPlan.ts`), which decides every account action for every provider: Check limits now
always, Edit category on every card with an account, the saved-login items and the footer's
primary action wherever there is an account manager, and Archive account and Remove account on
every card that is not signed in.
Remove account and Archive account each send one command naming the card's account, from a card
and from the archived list alike (Remove account, above); on a card with no saved login they act
on its usage history only.
Saved identities
join observations only by exact observation key, and remain visible when usage reads fail.

### The Limits column

Both Limits surfaces, the screen and the menu-bar popover, render one column per provider, and
that column is the Limits read's (`read_limits`, served by `limits_refresh::read_column` as
`ColumnEntryDto`s and built by `limits::column`). Each entry is a `ProviderLimitsDto` with the
account facts a card needs beside it: its saved profile (`profile`, the same `ProfileDto` the account
list gives), its category (`category`), the legacy history it replaced (`replaced`, which Remove
account and Archive account take with it, read by Rust from the column it serves) and, for a saved profile no read answered for yet, an
entry of its own (`unread`). The card model (`limitColumn`) therefore joins nothing, and the popover
lists the same accounts, counts and copy as the screen, with no account manager and no vault of its
own.

The saved profiles and categories are the provider's roster (`Roster`, `accounts/roster.rs`),
which the Limits cache owns under a lock of its own and two writers replace
([shared-reads.md](shared-reads.md#the-mechanism)). The saved refresh the read already makes
(`accounts::usage::refresh_and_list`) lists it from the vault it loaded to poll, with no second
decrypt; one that could not poll because the provider's own login could not be read (a malformed
config, a denied Keychain probe, a login that names no account) rereads it with the session's key
(`Roster::read`), so the saved profiles stay listed. Every vault write rereads it too
(`limits_refresh::roster_changed`): an account change (`account_changed`, before the provider is
read again), automatic remembering when it saves a login or is turned on, unlock, a category edit or
forget (all through `Notify::roster`), Remove account, archive and unarchive, and the account list once it has read
the vault (`Notify::listed`), which is what unlocks it at startup when nothing else has. One builder lists a profile for the account list and
the roster alike (`Roster::of`, given each profile's facts: the listing's read its native login,
deadlines and verdicts; the roster's are its recorded end alone). Who is signed
in is not part of the roster: the column marks active the profile of the account the latest native
read names, and none when that read names nobody (not installed, signed out), so a switch made in
the CLI moves the active dot and the actions with the next native read.

Building and rereading the roster add no credential or Keychain read to the Limits path: the
reread opens the vault only with the key this session has already unlocked
(`Store::open_unlocked`, through `SessionKeys::peek`), never asking the OS store, and neither reads
a native login or a home's login. A sign-in's end needs a login's credentials (a home's Keychain
item on macOS), so the column carries the end the account list last read
(`Roster::sign_in_deadlines`, as instants, which `Accounts::list` hands over through `Notify::listed` to
`limits_refresh::accounts_listed`, with the roster reread, announcing the Limits read once and only
when either changed): the screen, whose account manager lists the
accounts, shows it as before, and the popover shows it whenever the account list has been read in
this session.
The picker and Settings share the global automatic-saving opt-in preference. Inactive Remove account removes
the saved login and usage card; active Remove saved login keeps the native login and usage card.
Operations remain busy through readback, and a sign-out confirmation is checked against the
latest known native observation key before invoking the provider action.

A failed account-list unlock exposes a Retry action beside the error. It unlocks the existing
vault and reloads accounts; it does not start sign-in, create a vault, or change a CLI login.

Card headers never display workspace IDs. Saved profiles keep their verified workspace identity;
accounts that share an email are told apart by their plan badge when usage reports one, and by the
optional category otherwise.
When a scoped account has a successful reading, the column folds into it the legacy observation it
replaces, by one rule (`snapshots::supersedes`, which Forget also uses): the scoped card's legacy id
names the legacy card, which is not the signed-in read, and both carry the same email, compared
trimmed and ignoring ASCII case only (a non-ASCII letter's case is not folded). The legacy key has
one owner, `Identity::legacy_key`: the workspace id for Codex, the user id for Claude, `email:` and
the lowercased email for Muse, and none for providers without saved accounts. Every scoped card, the
current Codex card and a saved account's failed first read included, and every listed profile carry
it as `legacyId`; when a scoped snapshot lost its own, which older app versions can drop when
rewriting history, the column takes the saved profile's. The Codex sessions merge never advances a
folded legacy card (`codex_sessions::merge_recent`). The fold does not transfer old quotas or delete
history, and preserves current legacy observations and accounts without a verified scoped
replacement. Muse is the one
exception to the transfer rule, because its legacy key is the whole identity
(`Identity::legacy_key_is_whole`): when a Muse card's first successful scoped reading has no weekly
window, the snapshot store fills it from the legacy snapshot with the same legacy id and email, the
way a later reading keeps its own last weekly. Meta omits Muse's usage while the 5-hour window is
idle, so without this the first reading after the id change would show no weekly at all.
Remove account takes that association with it: it deletes the matched legacy history with the
scoped snapshot, so the next read cannot resurrect the removed card. Each legacy snapshot goes only
while it still carries the email the column folded it by: the snapshot store rechecks every one
under its write lock before it deletes any, and refuses changed or unreadable history. The older
Forget command keeps its optional expected email, with the same recheck, for a caller that passes
one.


