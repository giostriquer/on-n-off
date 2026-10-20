use serde_json::{json, Value};

use super::MintResponse;

pub(in crate::limits::muse) fn mint_body() -> Value {
    json!({
        "is_subs_active": true,
        "require_payment": false,
        "subs_tier_name": "Wonder",
        "subs_usage": {
            "tier": "27681631238169137",
            "window": {"used_percent": 41, "window_duration_mins": 300, "resets_at": 1_760_000_000i64},
            "weekly": {"used_percent": 22, "resets_at": 1_760_500_000i64}
        }
    })
}

pub(in crate::limits::muse) fn mint_response(body: Value) -> MintResponse {
    serde_json::from_value(body).expect("mint response")
}
