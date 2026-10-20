use super::*;

const ROLLOUT: &str = r#"{"timestamp":"2026-08-20T06:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":96.0,"window_minutes":10080,"resets_at":1787614473},"secondary":null}}}"#;

fn remembered(provider: AgentId) -> ProviderLimitsDto {
    ProviderLimitsDto {
        current_account: false,
        ..ProviderLimitsDto::for_test(provider, "acct-old")
            .labelled("old@example.com")
            .with_reading(Reading {
                windows: vec![LimitWindowDto {
                    id: "primary".to_string(),
                    label: "Weekly · all models".to_string(),
                    kind: LimitWindowKind::Weekly,
                    used_percent: 86.0,
                    resets_at: Some("2026-08-24T23:34:33Z".to_string()),
                    window_seconds: Some(604_800),
                    observed_at: "2026-08-20T02:39:06.754Z".to_string(),
                    role: None,
                    starts_on_first_message: false,
                }],
                ..Reading::default()
            })
    }
}

fn weekly_used(cards: &[ProviderLimitsDto]) -> Option<f64> {
    cards
        .iter()
        .find(|card| {
            card.account
                .as_ref()
                .is_some_and(|account| account.id == "acct-old")
        })
        .map(|card| card.reading.windows[0].used_percent)
}

#[test]
fn a_read_merges_session_rollouts_only_for_a_provider_whose_facts_name_them() {
    let now_ms = chrono::DateTime::parse_from_rfc3339("2026-08-20T14:00:00Z")
        .unwrap()
        .timestamp_millis();
    for provider in crate::provider_catalogue::WITH_LIMITS {
        let home = scratch_dir(&format!("limits-rollouts-{}", provider.key()));
        let sessions = home.join(".codex/sessions/2026/08/20");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(sessions.join("rollout.jsonl"), ROLLOUT).unwrap();
        let store = SnapshotStore::for_home(&home);
        store.save(&remembered(provider)).unwrap();
        let signed_in = signed_in_card(provider, "acct-now", Reading::default());

        let cards = read_accounts(
            provider,
            Sources {
                home: &home,
                now_ms,
            },
            |read_home| {
                assert_eq!(read_home, home.as_path());
                signed_in
            },
        );

        let merged = provider.limits_facts().session_rollouts.is_some();
        let expected = if merged { 96.0 } else { 86.0 };
        assert_eq!(weekly_used(&cards), Some(expected), "{provider:?}");
        assert_eq!(
            weekly_used(&store.load(provider)),
            Some(expected),
            "{provider:?} on disk"
        );
        let _ = fs::remove_dir_all(&home);
    }
}
