# on-n-off

A desktop app that reads what coding agents keep on disk: plugins, skills, MCP servers, token
usage, subscription limits and pull requests. It shows them in one place.

Where each term lives in code: the Glossary in
[`docs/architecture/README.md`](docs/architecture/README.md#glossary).

## Language

### Limits

**Quota window**:
A rolling rate limit a provider enforces on an account: the five-hour session, the week, or a
single model's.
_Avoid_: limit, bucket

**Reading**:
Everything one read reported about one provider account: its quota windows, its figures and its
account details.
_Avoid_: snapshot, observation set

**Headline window**:
The quota window a card or the notch leads with: its weekly window. A card that has none leads with
nothing; a read that reports other windows but misses it keeps the last one read.
_Avoid_: hero, primary

**Figure**:
A value on a reading other than a quota window: the credit balance, workspace credits, credits
spent, banked resets, the subscription term or a reset offer.
_Avoid_: metric, extra

**Account details**:
The plan a read reports. It describes the account; a reading that holds nothing else has observed
nothing.
_Avoid_: metadata

**Remembered reading**:
The last reading kept for an account, shown when the account is neither the signed-in one, a saved
profile Limits is polling, nor an archived account: one the user switched away from without saving
it, or a saved profile Limits cannot poll now.
_Avoid_: cache, snapshot (the file that stores it)

**Archived account**:
An account the user has put away: hidden from Limits and not polled, its saved login and last
reading kept so it can be brought back without signing in again. Only the user archives or
unarchives an account: by Unarchive, by adding it again, or by signing in to it. Unrelated to
Codex's archived sessions, which Usage reads.
_Avoid_: hidden, disabled, paused

**Banked reset alert**:
A Codex account's opt-in to act when one of its banked resets is worth using: the account has run
low, at the share the user set or below, and its limit is not about to renew by itself. It either
notifies, or, set to use the reset automatically, says so and uses it ten minutes later unless the
user cancels it on the account's card. Either way a banked reset is spent only with 10% or less of
the limit left, or the alert's lower share, and an alert acts at most once a weekly cycle.
_Avoid_: auto-reset

### Accounts

**Native store**:
Where a provider's own CLI keeps its signed-in login: a Keychain item or a credentials file.
_Avoid_: credential source

**Saved profile**:
A login on-n-off keeps for a provider account, so the user can switch back to it: in the encrypted
vault, or in the account's home.
_Avoid_: saved account, vault entry

**Account home**:
A private store of the provider's own client, kept for one saved profile, where its one login waits
while it is not the signed-in one and where that client renews it. Only Claude has them.
_Avoid_: shadow, isolated home (that is a sign-in's, removed once it is done)

**Account change**:
An action that changes which logins are saved or which one the provider's CLI uses: save, remove,
use, sign out, sign in.

### Usage

**Transcript source**:
One transcript file a provider wrote, found by walking that provider's transcript directories.
_Avoid_: log, session file

**Watermark**:
The instant before which usage is counted only from folded usage, never from transcripts.

**Folded usage**:
Usage kept as 15-minute rows once its transcripts are old enough, so it outlives their deletion.
It is user data, not a cache.
_Avoid_: history cache
