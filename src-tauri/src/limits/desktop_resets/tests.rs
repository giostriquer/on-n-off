use std::path::{Path, PathBuf};

use ruzstd::encoding::{compress_to_vec, CompressionLevel};
use serde_json::{json, Value};

use super::*;
use crate::accounts::model::Identity;
use crate::accounts::{roster_for_test, SignInDeadlines};
use crate::dto::ProviderLimitsDto;
use crate::limits::{column, scoped_account};
use crate::paths;

const ORG_A: &str = "00000000-0000-4000-8000-00000000000a";
const ORG_B: &str = "00000000-0000-4000-8000-00000000000b";
const NOW_MS: i64 = 1_794_350_000_000;
const SEEN: &str = "Tue, 10 Nov 2026 20:49:33 GMT";
const SEEN_AT: &str = "2026-11-10T20:49:33Z";

fn grant(label: &str, left: u64, ends_at: &str) -> Value {
    json!({
        "id": "grant-1", "label": label, "type": "full",
        "resets_total": left, "resets_left": left,
        "starts_at": "2026-11-09T18:00:00+00:00", "ends_at": ends_at,
        "paused": false, "usable_now": true,
    })
}

fn answer(grants: Value) -> Value {
    json!({
        "five_hour": null,
        "limits": [],
        "cedar_ember": {"eligible": true, "ineligible_reason": null, "grants": grants},
    })
}

fn one_reset() -> Value {
    answer(json!([grant(
        "Launch reset",
        1,
        "2027-03-09T07:59:00+00:00"
    )]))
}

fn entry_of(key: &str, body: &[u8], date: Option<&str>) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xfcfb_6d1b_a772_5c30_u64.to_le_bytes());
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(key.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(key.as_bytes());
    bytes.extend_from_slice(&compress_to_vec(body, CompressionLevel::Fastest));
    bytes.extend_from_slice(b"\x00\x00eof-record\x00HTTP/1.1 200\x00content-encoding:zstd\x00");
    if let Some(date) = date {
        bytes.extend_from_slice(format!("date:{date}\x00").as_bytes());
    }
    bytes.extend_from_slice(b"content-type:application/json\x00\x00");
    bytes
}

fn cache_dir(home: &Path) -> PathBuf {
    let dir = paths::claude_desktop_data_for(home)
        .join("Cache")
        .join("Cache_Data");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cached(home: &Path, organization: &str, bytes: &[u8]) -> PathBuf {
    let file = cache_dir(home).join(entry_file_name(&usage_key(organization)));
    std::fs::write(&file, bytes).unwrap();
    file
}

fn desktop_saw(organization: &str, body: &Value) -> PathBuf {
    let home = paths::scratch_dir("desktop-resets");
    let key = usage_key(organization);
    cached(
        &home,
        organization,
        &entry_of(&key, body.to_string().as_bytes(), Some(SEEN)),
    );
    home
}

fn seen(home: &Path, organization: &str) -> Option<DesktopResetsDto> {
    read(&cache_dir(home), organization, NOW_MS)
}

fn counted(home: &Path, organization: &str) -> Option<(u32, Option<String>)> {
    seen(home, organization).map(|seen| (seen.available_count, seen.next_expires_at))
}

fn left(count: u32, next: Option<&str>) -> Option<(u32, Option<String>)> {
    Some((count, next.map(str::to_owned)))
}

#[test]
fn the_usage_request_desktop_caches_names_the_organization_and_asks_for_resets() {
    assert_eq!(
        usage_key(ORG_A),
        "1/0/https://claude.ai/api/organizations/00000000-0000-4000-8000-00000000000a/usage?cedar_ember=1&skip_spend=1"
    );
}

#[test]
fn an_entrys_file_is_named_by_the_first_eight_bytes_of_its_keys_sha1() {
    assert_eq!(entry_file_name(&usage_key(ORG_A)), "c69a59c27f4c6c21_0");
}

#[test]
fn a_cached_answer_gives_the_resets_left_their_soonest_end_and_when_it_was_seen() {
    let home = desktop_saw(
        ORG_A,
        &answer(json!([
            grant("Launch reset", 1, "2027-03-09T07:59:00+00:00"),
            grant("Tenure reset", 2, "2027-01-15T12:00:00+00:00"),
        ])),
    );

    assert_eq!(
        seen(&home, ORG_A),
        Some(DesktopResetsDto {
            observed_at: SEEN_AT.into(),
            available_count: 3,
            next_expires_at: Some("2027-01-15T12:00:00Z".into()),
        })
    );
}

#[test]
fn an_answer_without_its_date_was_seen_when_the_entry_was_written() {
    let home = paths::scratch_dir("desktop-resets");
    let file = cached(
        &home,
        ORG_A,
        &entry_of(&usage_key(ORG_A), one_reset().to_string().as_bytes(), None),
    );
    let written = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_794_300_000);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(written)
        .unwrap();

    assert_eq!(
        seen(&home, ORG_A).map(|resets| resets.observed_at),
        Some("2026-11-10T08:40:00Z".to_owned())
    );
}

#[test]
fn a_reset_without_an_end_is_counted_and_has_no_expiry() {
    let mut unending = grant("Open reset", 1, "");
    unending["ends_at"] = Value::Null;
    unending["starts_at"] = Value::Null;
    let home = desktop_saw(ORG_A, &answer(json!([unending])));

    assert_eq!(counted(&home, ORG_A), left(1, None));
}

#[test]
fn only_resets_that_can_still_be_used_are_counted() {
    let live = grant("Launch reset", 1, "2027-03-09T07:59:00+00:00");
    let with = |change: &dyn Fn(&mut Value)| {
        let mut other = grant("Other reset", 1, "2027-02-01T00:00:00+00:00");
        change(&mut other);
        other
    };
    for (other, why) in [
        (with(&|g| g["paused"] = json!(true)), "paused"),
        (with(&|g| g["resets_left"] = json!(0)), "none left"),
        (
            with(&|g| g["ends_at"] = json!("2026-11-10T00:00:00+00:00")),
            "already ended",
        ),
        (
            with(&|g| g["starts_at"] = json!("2026-12-01T00:00:00+00:00")),
            "not started",
        ),
        (
            with(&|g| g["ends_at"] = json!("soon")),
            "an end that is no date",
        ),
        (
            with(&|g| g["starts_at"] = json!("later")),
            "a start that is no date",
        ),
        (
            with(&|g| g["resets_left"] = json!("1")),
            "a count that is no number",
        ),
        (with(&|g| g["resets_left"] = json!(101)), "an absurd count"),
        (json!("grant"), "a grant that is no object"),
    ] {
        let home = desktop_saw(ORG_A, &answer(json!([other, live])));

        assert_eq!(
            counted(&home, ORG_A),
            left(1, Some("2027-03-09T07:59:00Z")),
            "{why}"
        );
    }
}

#[test]
fn a_reset_ending_this_very_millisecond_is_over_and_one_starting_now_counts() {
    let mut boundary = grant("Edge reset", 1, "2026-11-10T22:33:20+00:00");
    let home = desktop_saw(ORG_A, &answer(json!([boundary.clone()])));
    assert_eq!(seen(&home, ORG_A), None, "ends at now");

    boundary["ends_at"] = json!("2026-11-10T22:33:21+00:00");
    boundary["starts_at"] = json!("2026-11-10T22:33:20+00:00");
    let home = desktop_saw(ORG_A, &answer(json!([boundary])));
    assert_eq!(
        seen(&home, ORG_A).map(|seen| seen.available_count),
        Some(1),
        "starts at now"
    );
}

#[test]
fn an_answer_that_holds_no_usable_reset_shows_nothing() {
    for (body, why) in [
        (
            json!({"cedar_ember": null, "limits": []}),
            "the plain usage answer",
        ),
        (json!({"limits": []}), "no block at all"),
        (
            json!({"cedar_ember": {"eligible": false, "ineligible_reason": "surface", "grants": []}}),
            "withheld from this client",
        ),
        (
            json!({"cedar_ember": {"eligible": false, "ineligible_reason": "tier",
                "grants": [grant("Launch reset", 1, "2027-03-09T07:59:00+00:00")]}}),
            "grants on an account that cannot use them",
        ),
        (answer(json!([])), "no grants"),
        (answer(json!({"id": "grant-1"})), "grants that are no list"),
        (json!([1, 2]), "a body that is no object"),
    ] {
        let home = desktop_saw(ORG_A, &body);
        assert_eq!(seen(&home, ORG_A), None, "{why}");
    }
}

#[test]
fn an_entry_that_is_not_this_organizations_answer_shows_nothing() {
    let key = usage_key(ORG_A);
    let body = one_reset().to_string();
    let good = entry_of(&key, body.as_bytes(), Some(SEEN));
    let mut wrong_magic = good.clone();
    wrong_magic[0] ^= 0xff;
    let mut long_key = good.clone();
    long_key[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    let truncated = good[..good.len() / 2].to_vec();
    let mut oversized = good.clone();
    oversized.resize(512 * 1024 + 1, 0);

    for (bytes, why) in [
        (
            entry_of(&usage_key(ORG_B), body.as_bytes(), Some(SEEN)),
            "another organization's answer under this name",
        ),
        (wrong_magic, "not a cache entry"),
        (long_key, "a key longer than the file"),
        (truncated, "cut off inside the body"),
        (oversized, "larger than any usage answer"),
        (
            entry_of(&key, b"not json", Some(SEEN)),
            "a body that is not JSON",
        ),
        (
            [&good[..24 + key.len()], b"not zstd".as_slice()].concat(),
            "a body that is not zstd",
        ),
        (good[..20].to_vec(), "shorter than the header"),
        (Vec::new(), "empty"),
    ] {
        let home = paths::scratch_dir("desktop-resets");
        cached(&home, ORG_A, &bytes);
        assert_eq!(seen(&home, ORG_A), None, "{why}");
    }

    let home = paths::scratch_dir("desktop-resets");
    cached(&home, ORG_A, &good);
    assert!(seen(&home, ORG_A).is_some(), "the entry itself is readable");
    assert_eq!(seen(&home, ORG_B), None, "no entry for that organization");
    assert_eq!(
        read(&home.join("no-such-cache"), ORG_A, NOW_MS),
        None,
        "no cache"
    );
}

#[test]
fn a_decoded_body_larger_than_any_usage_answer_shows_nothing() {
    let home = paths::scratch_dir("desktop-resets");
    let mut body = one_reset();
    body["padding"] = json!("x".repeat(300 * 1024));
    cached(
        &home,
        ORG_A,
        &entry_of(&usage_key(ORG_A), body.to_string().as_bytes(), Some(SEEN)),
    );

    assert_eq!(seen(&home, ORG_A), None);
}

fn identity(user: &str, organization: &str) -> Identity {
    Identity {
        provider: AgentId::Claude,
        user_id: user.into(),
        workspace_id: organization.into(),
    }
}

fn saved(identity: &Identity, label: &str) -> ProviderLimitsDto {
    let mut card = ProviderLimitsDto::for_test(identity.provider, "replaced");
    card.account = Some(scoped_account(identity, Some(label.to_owned())));
    card.current_account = false;
    card.saved_profile = true;
    card
}

fn column_of(provider: AgentId, profiles: &[(Identity, &str)]) -> Vec<ColumnEntryDto> {
    let cards: Vec<ProviderLimitsDto> = profiles
        .iter()
        .map(|(identity, label)| saved(identity, label))
        .collect();
    column(
        &cards,
        &roster_for_test(provider, profiles, &[], &[]),
        &SignInDeadlines::new(),
        0,
    )
}

fn counts(column: &[ColumnEntryDto]) -> Vec<Option<u32>> {
    column
        .iter()
        .map(|entry| {
            entry
                .desktop_resets
                .as_ref()
                .map(|seen| seen.available_count)
        })
        .collect()
}

fn home_seeing(answers: &[(&str, Value)]) -> PathBuf {
    let home = paths::scratch_dir("desktop-resets");
    for (organization, body) in answers {
        cached(
            &home,
            organization,
            &entry_of(
                &usage_key(organization),
                body.to_string().as_bytes(),
                Some(SEEN),
            ),
        );
    }
    home
}

fn two_resets() -> Value {
    answer(json!([grant(
        "Launch reset",
        2,
        "2027-03-09T07:59:00+00:00"
    )]))
}

#[test]
fn each_claude_account_gets_the_resets_desktop_saw_for_its_organization() {
    let home = home_seeing(&[(ORG_A, one_reset()), (ORG_B, two_resets())]);
    let mut column = column_of(
        AgentId::Claude,
        &[
            (identity("a", ORG_A), "a@example.com"),
            (identity("b", ORG_B), "b@example.com"),
            (
                identity("c", "00000000-0000-4000-8000-00000000000c"),
                "c@example.com",
            ),
        ],
    );

    attach_desktop_resets(AgentId::Claude, &mut column, &home, NOW_MS);

    assert_eq!(counts(&column), [Some(1), Some(2), None]);
    assert_eq!(
        column[0]
            .desktop_resets
            .as_ref()
            .map(|seen| seen.observed_at.as_str()),
        Some(SEEN_AT)
    );
}

#[test]
fn accounts_that_share_an_organization_get_no_resets() {
    let home = home_seeing(&[(ORG_A, one_reset()), (ORG_B, two_resets())]);
    let mut column = column_of(
        AgentId::Claude,
        &[
            (identity("a", ORG_A), "a@example.com"),
            (identity("teammate", ORG_A), "teammate@example.com"),
            (identity("b", ORG_B), "b@example.com"),
        ],
    );

    attach_desktop_resets(AgentId::Claude, &mut column, &home, NOW_MS);

    assert_eq!(counts(&column), [None, None, Some(2)]);
}

#[test]
fn only_a_provider_with_a_desktop_app_is_given_resets() {
    let home = home_seeing(&[(ORG_A, one_reset())]);
    let codex = Identity {
        provider: AgentId::Codex,
        user_id: "a".into(),
        workspace_id: ORG_A.into(),
    };
    let mut column = column_of(AgentId::Codex, &[(codex, "a@example.com")]);

    attach_desktop_resets(AgentId::Codex, &mut column, &home, NOW_MS);

    assert_eq!(counts(&column), [None]);
}

#[test]
fn the_signed_in_account_without_a_saved_profile_is_matched_by_its_configs_organization() {
    let home = home_seeing(&[(ORG_A, one_reset())]);
    std::fs::write(
        home.join(".claude.json"),
        json!({"oauthAccount": {
            "accountUuid": "a", "organizationUuid": ORG_A, "emailAddress": "a@example.com"
        }})
        .to_string(),
    )
    .unwrap();
    let signed_in = identity("a", ORG_A);
    let mut card = saved(&signed_in, "a@example.com");
    card.current_account = true;
    card.saved_profile = false;
    let unsaved = saved(&identity("b", ORG_A), "b@example.com");
    let roster = roster_for_test(AgentId::Claude, &[], &[], &[]);

    let mut alone = column(&[card.clone()], &roster, &SignInDeadlines::new(), 0);
    attach_desktop_resets(AgentId::Claude, &mut alone, &home, NOW_MS);
    assert_eq!(counts(&alone), [Some(1)]);

    let mut beside = column(&[card, unsaved], &roster, &SignInDeadlines::new(), 0);
    attach_desktop_resets(AgentId::Claude, &mut beside, &home, NOW_MS);
    assert_eq!(
        counts(&beside),
        [Some(1), None],
        "a card with no profile that is not the signed-in one names no organization"
    );
}

#[test]
fn the_resets_are_sent_beside_when_they_were_seen() {
    let home = home_seeing(&[(ORG_A, one_reset())]);
    let mut column = column_of(AgentId::Claude, &[(identity("a", ORG_A), "a@example.com")]);
    let untouched = serde_json::to_value(&column[0]).unwrap();
    assert!(untouched.get("desktopResets").is_none(), "{untouched}");

    attach_desktop_resets(AgentId::Claude, &mut column, &home, NOW_MS);

    assert_eq!(
        serde_json::to_value(&column[0]).unwrap()["desktopResets"],
        json!({
            "observedAt": SEEN_AT,
            "availableCount": 1,
            "nextExpiresAt": "2027-03-09T07:59:00Z",
        })
    );
}

#[test]
#[ignore = "reads the saved-reset answers Claude Desktop cached in the home ON_N_OFF_PROBE_HOME names"]
fn probe_real_home_desktop_resets() {
    let home = paths::probe_home();
    let organization =
        crate::limits::claude_config::read_claude_config_account(&home.join(".claude.json"))
            .and_then(|account| account.identity.organization_id);
    let cache = paths::claude_desktop_data_for(&home)
        .join("Cache")
        .join("Cache_Data");
    let seen = organization
        .as_deref()
        .and_then(|organization| read(&cache, organization, chrono::Utc::now().timestamp_millis()));
    println!(
        "Claude Code names an organization: {}",
        organization.is_some()
    );
    println!(
        "Desktop cached a usable reset answer for it: {}",
        seen.is_some()
    );
    if let Some(seen) = seen {
        println!("resets left: {}", seen.available_count);
        println!("next expiry known: {}", seen.next_expires_at.is_some());
        println!("seen at: {}", seen.observed_at);
    }
}
