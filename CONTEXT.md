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
The plan and subscription status a read reports. They describe the account; a reading that holds
nothing else has observed nothing.
_Avoid_: metadata

**Remembered reading**:
The last reading kept for an account, shown when the account is neither the signed-in one nor a
saved profile Limits is polling: one the user switched away from without saving it, or a saved
profile Limits cannot poll now.
_Avoid_: cache, snapshot (the file that stores it)

### Accounts

**Native store**:
Where a provider's own CLI keeps its signed-in login: a Keychain item or a credentials file.
_Avoid_: credential source

**Saved profile**:
A login on-n-off keeps encrypted for a provider account, so the user can switch back to it.
_Avoid_: saved account, vault entry

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
