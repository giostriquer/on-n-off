//! Which cards say they are a saved profile Limits polls (`saved_profile`): every polled profile's,
//! whatever its poll came to, and no other.
use super::*;

/// A remembered card of the account `key` that no read this poll answered for.
fn history(key: &str) -> ProviderLimitsDto {
    ProviderLimitsDto {
        current_account: false,
        ..ProviderLimitsDto::for_test(AgentId::Claude, key).with_reading(Reading {
            windows: reading(&profile()).reading.windows,
            ..Reading::default()
        })
    }
}

/// `entries` after the saved polls of `home`'s Claude profiles, each fetched as `fetch` says, with
/// `native` the CLI's own subscription.
fn polled(
    home: &Path,
    entries: &mut Vec<ProviderLimitsDto>,
    native: Result<Option<model::Identity>, String>,
    fetch: &(dyn Fn(&Profile) -> FetchResult + Sync),
) {
    refresh_with(
        home,
        AgentId::Claude,
        false,
        entries,
        native,
        &|| Ok(open(home)),
        fetch,
    );
}

/// The flag of the card for `key` in `entries`.
fn flag(entries: &[ProviderLimitsDto], key: &str) -> bool {
    entries
        .iter()
        .find(|card| {
            card.account
                .as_ref()
                .is_some_and(|account| account.id == key)
        })
        .expect("a card for the account")
        .saved_profile
}

#[test]
fn a_polled_profile_is_a_saved_profile_whether_it_answered_failed_or_was_held_back() {
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    let key = p.identity.observation_key();

    let mut answered = vec![];
    polled(home.path(), &mut answered, Ok(None), &|p| FetchResult {
        login: p.login.clone(),
        result: Ok(reading(p)),
    });
    assert!(flag(&answered, &key), "answered");

    // The next read, inside the poll interval, shows the snapshot the answer left.
    let mut held_back = crate::limits::remembered(home.path(), AgentId::Claude);
    assert!(!flag(&held_back, &key));
    polled(home.path(), &mut held_back, Ok(None), &|_| {
        panic!("a poll inside its interval fetches nothing")
    });
    assert!(flag(&held_back, &key), "held back");

    let other = tempfile::tempdir().unwrap();
    let p = stored(other.path());
    let mut failed = vec![];
    polled(other.path(), &mut failed, Ok(None), &|p| FetchResult {
        login: p.login.clone(),
        result: Err(HttpError::Network("offline".into()).into()),
    });
    assert_eq!(failed[0].status, LimitsStatus::Failed);
    assert!(flag(&failed, &p.identity.observation_key()), "failed");
}

/// The signed-in card, a saved profile without a login, and history no saved profile polls.
#[test]
fn cards_no_saved_poll_reads_are_not_saved_profiles() {
    let home = tempfile::tempdir().unwrap();
    let signed_in = stored(home.path());
    let mut logged_out = signed_in.clone();
    logged_out.id = "logged-out".into();
    logged_out.identity.user_id = "logged-out".into();
    logged_out.login = None;
    rewrite(home.path(), |db| db.profiles.push(logged_out.clone()));
    let mut entries = vec![
        ProviderLimitsDto::for_test(AgentId::Claude, &signed_in.identity.observation_key()),
        history(&logged_out.identity.observation_key()),
        history("legacy-user"),
    ];

    polled(
        home.path(),
        &mut entries,
        Ok(Some(signed_in.identity.clone())),
        &|_| panic!("nothing here is polled"),
    );

    assert!(entries.iter().all(|card| !card.saved_profile));
}

/// A native store that could not be read, or a recovery still pending, polls no saved profile, so
/// its card stays a remembered reading.
#[test]
fn a_profile_left_unpolled_is_not_a_saved_profile() {
    for skipped in ["native store unreadable", "recovery pending"] {
        let home = tempfile::tempdir().unwrap();
        let p = stored(home.path());
        let native = if skipped == "recovery pending" {
            rewrite(home.path(), |db| {
                db.begin_recovery(super::super::super::transaction::Recovery {
                    target_id: p.id.clone(),
                    outgoing: None,
                    outgoing_identity: None,
                })
            });
            Ok(None)
        } else {
            Err("Cannot read native credentials.".to_string())
        };
        let key = p.identity.observation_key();
        let mut entries = vec![history(&key)];

        polled(home.path(), &mut entries, native, &|_| {
            panic!("{skipped}: nothing is polled")
        });

        assert!(!flag(&entries, &key), "{skipped}");
    }
}
