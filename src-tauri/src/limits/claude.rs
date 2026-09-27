//! Claude subscription limits: the signed-in login's read (`claude_current`) and a saved profile's
//! (`read_saved_claude`), each asking `api.anthropic.com` for the login's profile and then its usage,
//! and the parse of what those two endpoints answer.
//!
//! The usage payload carries a normalized `limits[]` array (kind/group/percent/resets_at) plus the
//! older top-level `five_hour` / `seven_day` / `seven_day_<model>` objects. The array wins when
//! it has usable entries; the legacy keys are the fallback. Asked with `?cedar_ember=1`, the same
//! payload also reports the account's saved resets.

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::credentials::{
    read_claude_identity, ClaudeCredential, ClaudeIdentity, CredentialLookup, LoginSource,
};
use super::json::{humanize, optional_string, percent, window};
use super::pipeline::{
    finish, resolve_provider, LoadFailureKind, ProviderLoadError, ResolveOutcome,
};
use super::{
    saved_card, scoped_account, Parsed, SavedReadError, SavedReadUrls, Sources, DEFAULT_ACCOUNT,
};
use crate::accounts::claude_renew;
use crate::accounts::claude_store::{self, KeychainProbe, StorageDir};
use crate::accounts::model::Identity;
use crate::dto::{
    AgentId, LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsResetCreditsDto,
    LimitsStatus, ProviderLimitsDto, Reading,
};
use crate::http::{get_json, HttpError};

/// What `GET /api/oauth/profile` says about the login: whose it is, and the subscription status
/// Anthropic reports for its organization. The status never decides the read.
struct ClaudeProfile {
    identity: ClaudeIdentity,
    /// `organization.subscription_status` as Anthropic writes it (`active`, `past_due`, …);
    /// unknown when absent, empty or not text.
    subscription_status: Option<String>,
}

fn parse_profile(payload: &Value) -> Result<ClaudeProfile, String> {
    let account = payload
        .get("account")
        .ok_or_else(|| "missing account".to_string())?;
    let organization = payload
        .get("organization")
        .ok_or_else(|| "missing organization".to_string())?;
    Ok(ClaudeProfile {
        identity: ClaudeIdentity {
            account: LimitsAccountDto {
                legacy_id: None,
                id: optional_string(account.get("uuid"))
                    .ok_or_else(|| "missing account uuid".to_string())?,
                label: optional_string(account.get("email")),
            },
            organization_id: Some(
                optional_string(organization.get("uuid"))
                    .ok_or_else(|| "missing organization uuid".to_string())?,
            ),
        },
        subscription_status: optional_string(organization.get("subscription_status")),
    })
}

/// Everything one usage payload says about the account: its windows and its saved resets.
fn parse_usage(payload: &Value, now: DateTime<Utc>) -> Reading {
    Reading {
        windows: parse_claude(payload),
        reset_credits: parse_reset_credits(payload, now),
        ..Reading::default()
    }
}

fn parse_claude(payload: &Value) -> Vec<LimitWindowDto> {
    let normalized: Vec<LimitWindowDto> = payload
        .get("limits")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(normalized_window).collect())
        .unwrap_or_default();
    if !normalized.is_empty() {
        return normalized;
    }
    legacy_windows(payload)
}

fn normalized_window(entry: &Value) -> Option<LimitWindowDto> {
    let kind = optional_string(entry.get("kind"))?;
    let group = optional_string(entry.get("group"))?;
    let used = percent(entry.get("percent"))?;
    let resets_at = optional_string(entry.get("resets_at"));
    let scope = scope_name(entry.get("scope"));
    let (window_kind, label) = match (group.as_str(), kind.as_str()) {
        ("session", _) => (LimitWindowKind::Session, "5 hour · all models".to_string()),
        ("weekly", "weekly_all") => (LimitWindowKind::Weekly, "Weekly · all models".to_string()),
        ("weekly", other) => {
            let name = match &scope {
                Some(scope) => scope.clone(),
                None => humanize(other.strip_prefix("weekly_").unwrap_or(other)),
            };
            (LimitWindowKind::Model, format!("Weekly · {name}"))
        }
        _ => return None,
    };
    let id = match &scope {
        Some(scope) => format!("{kind}:{scope}"),
        None => kind,
    };
    Some(window(id, label, window_kind, used, resets_at))
}

/// `scope.model.display_name`, else `scope.surface`, for per-model / per-surface windows.
fn scope_name(scope: Option<&Value>) -> Option<String> {
    let scope = scope?;
    optional_string(
        scope
            .get("model")
            .and_then(|model| model.get("display_name")),
    )
    .or_else(|| optional_string(scope.get("surface")))
}

/// Why Claude Code's saved-reset status can report no reset for reasons about the account or the
/// program itself, so no grants really means none. Any other reason (`surface`, `cli_version`,
/// `mobile`, `unknown`, or one added later) describes who is asking; on-n-off is not Claude Code, so
/// those leave the count unknown.
const ACCOUNT_HOLDS_NONE: [&str; 6] = [
    "no_grant",
    "tier",
    "seat",
    "tenure",
    "other_experiment",
    "config_off",
];
/// The status could not be read at all; Claude Code treats it as a failed read.
const UNANSWERED: &str = "unavailable";

/// Anthropic's saved rate-limit resets, which Claude Code spends with `/limit-reset`: the
/// `cedar_ember` block the usage read carries when asked with `?cedar_ember=1`. The count sums
/// `resets_left` over every grant, as Claude Code's own count does, and the expiry is the soonest
/// `ends_at` still ahead of `now` among grants holding a reset.
///
/// `None` is "unknown", never zero: no block, an `unavailable` one, grants that were sent but
/// cannot be read, or no grants for a reason about the asker. Zero is only reported when the block
/// says the account holds none.
fn parse_reset_credits(payload: &Value, now: DateTime<Utc>) -> Option<LimitsResetCreditsDto> {
    let block = payload.get("cedar_ember")?.as_object()?;
    let reason = block.get("ineligible_reason").and_then(Value::as_str);
    if reason == Some(UNANSWERED) {
        return None;
    }
    let sent: &[Value] = match block.get("grants") {
        None | Some(Value::Null) => &[],
        Some(grants) => grants.as_array()?,
    };
    let grants: Vec<(u32, Option<DateTime<Utc>>)> = sent.iter().filter_map(grant).collect();
    if grants.is_empty() {
        let holds_none = sent.is_empty()
            && (block.get("eligible").and_then(Value::as_bool) == Some(true)
                || reason.is_some_and(|reason| ACCOUNT_HOLDS_NONE.contains(&reason)));
        return holds_none.then_some(LimitsResetCreditsDto {
            available_count: 0,
            next_expires_at: None,
        });
    }
    Some(LimitsResetCreditsDto {
        available_count: grants
            .iter()
            .map(|(left, _)| *left)
            .fold(0, u32::saturating_add),
        next_expires_at: grants
            .iter()
            .filter(|(left, _)| *left > 0)
            .filter_map(|(_, ends_at)| *ends_at)
            .filter(|ends_at| *ends_at > now)
            .min()
            .map(|at| at.to_rfc3339()),
    })
}

/// One grant's `resets_left` and `ends_at`. The count must be a whole number of at least zero, as
/// Claude Code's reader requires; `1.0` is one, since JSON does not tell integers from floats.
fn grant(grant: &Value) -> Option<(u32, Option<DateTime<Utc>>)> {
    let left = grant.get("resets_left")?;
    let left = left.as_u64().or_else(|| {
        left.as_f64()
            .filter(|left| left.fract() == 0.0 && *left >= 0.0)
            .map(|left| left as u64)
    })?;
    let ends_at = optional_string(grant.get("ends_at"))
        .and_then(|at| DateTime::parse_from_rfc3339(&at).ok())
        .map(|at| at.with_timezone(&Utc));
    Some((u32::try_from(left).unwrap_or(u32::MAX), ends_at))
}

fn legacy_windows(payload: &Value) -> Vec<LimitWindowDto> {
    const LEGACY: [(&str, &str, &str, LimitWindowKind); 4] = [
        (
            "five_hour",
            "session",
            "5 hour · all models",
            LimitWindowKind::Session,
        ),
        (
            "seven_day",
            "weekly_all",
            "Weekly · all models",
            LimitWindowKind::Weekly,
        ),
        (
            "seven_day_opus",
            "weekly_opus",
            "Weekly · Opus",
            LimitWindowKind::Model,
        ),
        (
            "seven_day_sonnet",
            "weekly_sonnet",
            "Weekly · Sonnet",
            LimitWindowKind::Model,
        ),
    ];
    LEGACY
        .iter()
        .filter_map(|(key, id, label, kind)| {
            let entry = payload.get(*key)?;
            let used = percent(entry.get("utilization"))?;
            let resets_at = optional_string(entry.get("resets_at"));
            Some(window(*id, *label, *kind, used, resets_at))
        })
        .collect()
}

const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Asks the usage read for the saved-reset block too. The query is the one Claude Code sends on
/// demand for `/limit-reset`; its regular read is the plain URL, which is why [`claude_usage`] falls
/// back to it. `skip_spend` leaves out the extra-usage spend figures, which on-n-off does not show.
const CLAUDE_USAGE_QUERY: &str = "cedar_ember=1&skip_spend=1";
const CLAUDE_PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";

/// The three services one Claude read talks to, together so adding a fourth costs one field and
/// not an edit at every call site.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClaudeEndpoints<'a> {
    /// Where a stale access token is renewed, before anything is asked of the other two. A saved
    /// profile's read asks only those two: a login on-n-off owns renews before it
    /// (`accounts/usage.rs`), and one it does not own is never renewed.
    pub(crate) token: &'a str,
    pub(crate) profile: &'a str,
    pub(crate) usage: &'a str,
}

/// The services a Claude read asks in the app. The account switch verifies a login against the same
/// profile endpoint (`accounts/claude.rs`).
pub(crate) const CLAUDE: ClaudeEndpoints<'static> = ClaudeEndpoints {
    token: claude_renew::TOKEN_URL,
    profile: CLAUDE_PROFILE_URL,
    usage: CLAUDE_USAGE_URL,
};

/// Claude: which account the CLI is signed into (`~/.claude.json`) decides whether the memoised
/// login may be reused; otherwise the Keychain (or the credentials file) is read. A rejected token
/// evicts the memo so the next read goes back to the Keychain.
pub(super) fn claude_current<P: Fn(&StorageDir) -> KeychainProbe>(
    force: bool,
    sources: Sources<'_, P>,
) -> ProviderLimitsDto {
    let Sources {
        home,
        memo,
        keychain,
        claude,
        now_ms,
        ..
    } = sources;
    // Where Claude Code keeps its account and its login, under whatever `CLAUDE_CONFIG_DIR` and
    // `CLAUDE_SECURESTORAGE_CONFIG_DIR` say: resolved once, for both reads below.
    let dirs = match claude_store::native_dirs(home) {
        Ok(dirs) => dirs,
        Err(why) => {
            let message = format!("Could not read the stored login: {why}");
            return finish(
                AgentId::Claude,
                LimitsStatus::Failed,
                Some(message),
                Parsed::default(),
            );
        }
    };
    let selected_identity = read_claude_identity(&dirs.config_file(home));
    let account = selected_identity
        .as_ref()
        .map(|identity| identity.account.clone())
        .unwrap_or_else(default_account);
    // Reading the Claude login includes renewing it: an access token lives eight hours and Claude
    // Code renews it only while it is running, so a longer gap is the ordinary case rather than a
    // broken login. Keeping that inside the read means the memo stores the renewed login like any
    // other, and the rejected-token retry below gets the renewal too.
    let storage = dirs.storage();
    let read_credential = || claude_renew::current_login(&storage, &keychain, now_ms, claude.token);
    let attempt = |lookup| claude_limits(lookup, &selected_identity, claude.profile, claude.usage);
    let (lookup, source) = memo.lookup(force, &account.id, now_ms, read_credential);
    let mut loaded = attempt(lookup);
    // Claude Code rotates the access token before the expiry it records, which leaves the memo
    // holding one the endpoint has already stopped accepting. That rejection says nothing about
    // the login, so read the stored login again and try once more before reporting one —
    // otherwise a signed-in user is told to sign in again until the next poll.
    //
    // A re-read that hands back no login leaves the rejection standing, because the rejection is
    // the accurate answer and the alternatives are louder falsehoods: `Missing` would report
    // "Sign in with `claude`" at a user who is signed in, and `Unreadable` a Keychain failure
    // when the prompt this retry raised went unanswered. `Expired` is the one where the discarded
    // message would have been gentler ("send a prompt to renew it"), but it describes a login
    // this attempt never sent; reporting what the endpoint actually refused is the honest answer.
    if source == LoginSource::Memo && loaded.failure == Some(LoadFailureKind::Unauthorized) {
        if let Some(fresh) = memo.refreshed(&account.id, now_ms, read_credential) {
            loaded = attempt(CredentialLookup::Found(fresh));
        }
    }
    if loaded.dto.status == LimitsStatus::Unauthenticated
        || loaded.failure == Some(LoadFailureKind::AccountMismatch)
    {
        memo.clear();
    }
    if let (Some(identity), Some(account)) = (&selected_identity, &mut loaded.dto.account) {
        if let Some(workspace) = &identity.organization_id {
            let identity = Identity {
                provider: AgentId::Claude,
                user_id: identity.account.id.clone(),
                workspace_id: workspace.clone(),
            };
            *account = scoped_account(&identity, account.label.take());
        }
    }
    loaded.dto
}

/// Claude: verify the stored token's profile, then read usage with the OAuth beta header. The plan
/// label comes from the login itself (`subscriptionType` and known Max `rateLimitTier` values).
fn claude_limits(
    lookup: CredentialLookup<ClaudeCredential>,
    selected_identity: &Option<ClaudeIdentity>,
    profile_url: &str,
    usage_url: &str,
) -> ResolveOutcome {
    let selected_account = selected_identity
        .as_ref()
        .map(|identity| identity.account.clone())
        .unwrap_or_else(default_account);
    resolve_provider(
        AgentId::Claude,
        Some(selected_account),
        lookup,
        |credential| {
            claude_read(
                credential,
                selected_identity.as_ref(),
                profile_url,
                usage_url,
            )
        },
    )
}

/// The headers every Claude request on-n-off sends with a login, given its `Authorization` value:
/// the OAuth beta header Anthropic's OAuth endpoints expect, and no cached answer. The Limits reads
/// and the account switch's verification (`accounts/claude.rs`) all send these.
pub(crate) fn claude_headers(authorization: &str) -> [(&'static str, &str); 3] {
    [
        ("Authorization", authorization),
        ("anthropic-beta", "oauth-2025-04-20"),
        ("Cache-Control", "no-cache"),
    ]
}

/// One Claude read with `credential`: its profile, which must be `expected` when an account is
/// expected, then its usage. The signed-in read expects the account `.claude.json` names, if any;
/// a saved profile's read and the first usage after a sign-in expect the profile's.
fn claude_read(
    credential: &ClaudeCredential,
    expected: Option<&ClaudeIdentity>,
    profile_url: &str,
    usage_url: &str,
) -> Result<Parsed, ProviderLoadError> {
    let bearer = format!("Bearer {}", credential.token);
    let headers = claude_headers(&bearer);
    let profile_payload = get_json(profile_url, &headers)?;
    let ClaudeProfile {
        identity: profile,
        subscription_status,
    } = parse_profile(&profile_payload).map_err(HttpError::Parse)?;
    if expected.is_some_and(|expected| {
        expected.account.id != profile.account.id
            || expected.organization_id != profile.organization_id
    }) {
        return Err(ProviderLoadError::AccountMismatch);
    }
    let usage = claude_usage(usage_url, &headers)?;
    Ok(Parsed {
        account: Some(profile.account),
        reading: Reading {
            plan: credential.plan(),
            subscription_status,
            ..usage
        },
    })
}

/// The account a saved profile's Claude read expects: the profile's user in its workspace.
fn expected_claude_identity(identity: &Identity) -> ClaudeIdentity {
    ClaudeIdentity {
        account: LimitsAccountDto {
            id: identity.user_id.clone(),
            label: None,
            legacy_id: None,
        },
        organization_id: Some(identity.workspace_id.clone()),
    }
}

/// A saved profile's Claude card, read with its login's `credential`, which must sign in as the
/// profile's user in its workspace. No CLI is started and nothing is renewed here.
pub(crate) fn read_saved_claude(
    identity: &Identity,
    credential: ClaudeCredential,
    urls: &SavedReadUrls<'_>,
) -> Result<ProviderLimitsDto, SavedReadError> {
    let parsed = claude_read(
        &credential,
        Some(&expected_claude_identity(identity)),
        urls.claude.profile,
        urls.claude.usage,
    )
    .map_err(|error| match error {
        ProviderLoadError::Http(error) => SavedReadError::Http(error),
        ProviderLoadError::AccountMismatch => SavedReadError::OtherAccount,
    })?;
    saved_card(identity, parsed)
}

/// Claude's usage read with its saved resets. The resets are optional, so a refusal of the reset
/// query never decides the read: any answer other than a transport failure retries the plain URL,
/// whose answer (a rejected login included) stands, with the resets unknown. A transport failure
/// is not retried, since the plain read would only wait on the same network.
fn claude_usage(usage_url: &str, headers: &[(&str, &str)]) -> Result<Reading, HttpError> {
    let payload = match get_json(&format!("{usage_url}?{CLAUDE_USAGE_QUERY}"), headers) {
        Err(HttpError::Network(error)) => return Err(HttpError::Network(error)),
        Err(_) => get_json(usage_url, headers)?,
        Ok(payload) => payload,
    };
    Ok(parse_usage(&payload, Utc::now()))
}

fn default_account() -> LimitsAccountDto {
    LimitsAccountDto {
        legacy_id: None,
        id: DEFAULT_ACCOUNT.to_string(),
        label: None,
    }
}

#[cfg(test)]
mod tests;
