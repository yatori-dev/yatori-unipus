use serde_json::Value;

pub fn jwt_payload(token: &str) -> Option<Value> {
    use base64::Engine;
    let part = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(part)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(part))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn jwt_exp(token: &str) -> Option<i64> {
    jwt_payload(token)?.get("exp")?.as_i64()
}

pub fn jwt_open_id(token: &str) -> Option<String> {
    jwt_payload(token)?
        .get("openId")?
        .as_str()
        .map(|s| s.to_string())
}

pub fn jwt_username(token: &str) -> Option<String> {
    jwt_payload(token)?
        .get("username")?
        .as_str()
        .map(|s| s.to_string())
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn jwt_fresh_enough(token: &str) -> bool {
    jwt_exp(token).is_some_and(|e| e - now_ts() > 24 * 3600)
}
