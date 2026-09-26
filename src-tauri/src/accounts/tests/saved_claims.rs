//! A saved Codex profile's claims, as the subscription dates read them.
use super::super::{
    codex::tests::id_token,
    model::Identity,
    saved_codex_claims,
    store::{ChangeKind, Database, Login, Store},
};
use crate::dto::AgentId;
use serde_json::{json, Value};

/// A scratch home whose vault the fixture key unlocks.
fn vault_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".on-n-off/accounts")).unwrap();
    super::super::vault::tests::unlock_fixture(&home);
    home
}

fn codex_identity(user: &str) -> Identity {
    Identity {
        provider: AgentId::Codex,
        user_id: user.into(),
        workspace_id: "team".into(),
    }
}

/// A saved profile's claims come only from the exact saved Codex login they are asked for, and
/// never carry its tokens.
#[test]
fn codex_claims_come_only_from_the_exact_saved_login_after_vault_reload() {
    let home = vault_home();
    let change = |edit: &dyn Fn(&mut Database)| {
        Store::open(home.path(), true)
            .unwrap()
            .change(ChangeKind::Metadata, |db| {
                edit(db);
                Ok(())
            })
            .unwrap();
    };
    let key = codex_identity("inactive").observation_key();
    change(&|db| {
        db.save(
            codex_identity("inactive"),
            Login {
                auth: json!({"tokens": {
                    "id_token": id_token(&json!({"chatgpt_plan_type": "pro"})),
                    "refresh_token": "never-projected"
                }}),
                account: Value::Null,
            },
            None,
        )
        .unwrap();
    });
    let claims = saved_codex_claims(home.path(), &key).unwrap().unwrap();
    assert_eq!(claims["chatgpt_plan_type"], "pro");
    assert!(claims.get("refresh_token").is_none() && claims.get("id_token").is_none());
    assert!(
        saved_codex_claims(home.path(), &codex_identity("other-user").observation_key())
            .unwrap()
            .is_none()
    );
    let mut other_workspace = codex_identity("inactive");
    other_workspace.workspace_id = "other".into();
    assert!(
        saved_codex_claims(home.path(), &other_workspace.observation_key())
            .unwrap()
            .is_none()
    );
    let mut claude = codex_identity("inactive");
    claude.provider = AgentId::Claude;
    change(&|db| {
        db.profiles[0].identity = claude.clone();
    });
    assert!(
        saved_codex_claims(home.path(), &claude.observation_key())
            .unwrap()
            .is_none(),
        "only a Codex profile's claims are read"
    );
    change(&|db| {
        db.profiles[0].identity = codex_identity("inactive");
        db.profiles[0].login = None;
    });
    assert!(
        saved_codex_claims(home.path(), &key).unwrap().is_none(),
        "a profile awaiting sign-in has no token to read"
    );
    change(&|db| db.profiles.clear());
    assert!(saved_codex_claims(home.path(), &key).unwrap().is_none());
    assert!(
        saved_codex_claims(home.path(), "legacy-workspace")
            .unwrap()
            .is_none(),
        "a legacy key names no profile"
    );
}
