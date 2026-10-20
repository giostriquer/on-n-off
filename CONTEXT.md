# on-n-off

A desktop app that reads what coding agents keep on disk: plugins, skills, MCP servers, token
usage, subscription limits and pull requests. It shows them in one place.

Where each term lives in code: the Glossary in
[`docs/architecture/README.md`](docs/architecture/README.md#glossary).

## Language

### Providers

**Provider catalogue**:
Every provider on-n-off knows, in tab order, with one row of facts about each: its key, names and
CLI, and which of Limits, Usage and saved accounts it has, beside the side notch's order. Every list
of providers the Rust side and the UI keep is drawn from it; the notch helper still keeps its own.
_Avoid_: registry, provider list

### Limits

**Quota window**:
A rolling rate limit a provider enforces on an account: the five-hour session, the week, or a
single model's.
_Avoid_: limit, bucket

**Reset instant**:
When a quota window renews, as its provider reported it. Two within two seconds of each other are
the same instant, and one names the window's current cycle.
_Avoid_: reset string, reset timestamp

**Window at an instant**:
What a quota window reads at a given moment, alike on every surface: 0% used once its reset instant
has passed, reached only at 100% used, unreadable outside 0–100%, and idle while it is unused, has
no reset and starts with the first message.
_Avoid_: live percent, current window

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
The plan a read reports, with the label a card shows for it and its capacity: how many times a base
plan's limits it holds, which weighs usage left when cards are ordered. It describes the account; a
reading that holds nothing else has observed nothing.
_Avoid_: metadata

**Read outcome**:
What one Limits read came to: a reading, or one failure of a closed set (not installed, signed out,
unsupported, a refused login, another account, an account changed during the read, a rate limit,
a failed request, or unavailable). It decides the card's status and message, whether the card keeps the account the
login names (every failure but not installed and signed out does, so its remembered reading stands
in), and how long the next read waits.
_Avoid_: error, result, read failure (one of its kinds)

**Remembered reading**:
The last reading kept for an account, shown when the account is neither the signed-in one, a saved
profile Limits is polling, nor an archived account: one the user switched away from without saving
it, or a saved profile Limits cannot poll now.
_Avoid_: cache, snapshot (the file that stores it)

**Signed-in state**:
Whether a card's account is the one the provider's CLI uses: confirmed when its saved profile, or
without one its read, says so and the account list names it; unconfirmed while the account list has
yet to name it, as just after a switch; no otherwise. Every action that acts on the signed-in login
waits for confirmed. A card's place in its column follows the read instead, so a signed-out read
still leads.
_Avoid_: active, current

**Card action plan**:
Which actions a Limits card offers, in order, and whether each can be used now: its menu, its
footer's main button, the Switch anyway confirmation and its banked reset controls. Decided once,
for every provider and card state.
_Avoid_: card menu (only part of it)

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
user cancels it on the account's card. An automatic spend happens only with 5% or less of the limit
left, or the alert's lower share, and an alert acts at most once a weekly cycle. A reset used by
hand from the card is never refused for what is left; above that share its confirmation warns.
_Avoid_: auto-reset

**Failure backoff**:
How long a Limits read waits after failing before it is made again: its poll interval doubled for
each failure in a row, at most sixteen intervals and never more than an hour, or a rate limit's
Retry-After when that is longer. The signed-in account's read, each saved profile and each Codex
backend figure count their own failures; a success starts the count over, and a read that found
nobody signed in, nothing installed or no subscription to read counts none. A backend figure starts one doubling behind the others: one
interval after its first failure, where they wait two.
_Avoid_: retry delay, cooldown

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

### Pull requests

**Pull request context**:
Whether a pull request is work or personal. It is work when GitHub reports its repository in an
organization, owned by one or a private fork of one's repository, and personal when a user account
owns it. A pull request read before GitHub reported this has neither.
_Avoid_: scope, workspace

### Side notch

**Rail cell**:
One slot of the side notch's rail: a selected provider's rings, or the pull request ring, in rail
order. A cell whose provider or pull requests have not been read yet keeps its slot and is checking.
_Avoid_: tile, entry

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
