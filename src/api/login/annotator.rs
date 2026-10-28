use super::sso::sso_client;
use super::token::jwt_payload;
use crate::config::AuthConfig;
use anyhow::{Context, Result, bail};

pub const DEFAULT_ANNOTATOR_KEY: &str = "a824b379f126b8b7aa5e33dee83fb0a05aa7462c";
pub const DEFAULT_ANNOTATOR_ISS: &str = "c4f772063dcfa98e9c50";
pub const DEFAULT_ANNOTATOR_AUD: &str = "edx.unipus.cn";

pub const DEFAULT_ANNOTATOR_TTL_MS: u64 = 31_536_000_000;

pub const EXPLORATION_PC_URL: &str = "https://ucontent.unipus.cn/_explorationpc_default/pc.html";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotatorParams {
    pub key: String,
    pub iss: String,
    pub aud: String,
    pub ttl_ms: u64,
}

impl AnnotatorParams {
    pub fn from_config(cfg: &AuthConfig) -> Self {
        Self {
            key: if cfg.annotator_key.is_empty() {
                DEFAULT_ANNOTATOR_KEY.to_string()
            } else {
                cfg.annotator_key.clone()
            },
            iss: if cfg.annotator_iss.is_empty() {
                DEFAULT_ANNOTATOR_ISS.to_string()
            } else {
                cfg.annotator_iss.clone()
            },
            aud: if cfg.annotator_aud.is_empty() {
                DEFAULT_ANNOTATOR_AUD.to_string()
            } else {
                cfg.annotator_aud.clone()
            },
            ttl_ms: if cfg.annotator_ttl_ms == 0 {
                DEFAULT_ANNOTATOR_TTL_MS
            } else {
                cfg.annotator_ttl_ms
            },
        }
    }

    #[cfg(test)]
    pub fn is_builtin(&self) -> bool {
        self.key == DEFAULT_ANNOTATOR_KEY
            && self.iss == DEFAULT_ANNOTATOR_ISS
            && self.aud == DEFAULT_ANNOTATOR_AUD
            && self.ttl_ms == DEFAULT_ANNOTATOR_TTL_MS
    }
}

fn base64url_nopad(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn generate_annotator_token(open_id: &str, params: &AnnotatorParams) -> String {
    build_annotator_token(open_id, now_ms() + params.ttl_ms, params)
}

pub fn build_annotator_token(open_id: &str, exp_ms: u64, params: &AnnotatorParams) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    let header = base64url_nopad(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload_json = format!(
        "{{\"open_id\":\"{}\",\"name\":\"\",\"email\":\"\",\"administrator\":false,\"exp\":{},\"iss\":\"{}\",\"aud\":\"{}\"}}",
        open_id, exp_ms, params.iss, params.aud
    );
    let payload = base64url_nopad(payload_json.as_bytes());
    let signing_input = format!("{}.{}", header, payload);
    let mut mac = Hmac::<Sha256>::new_from_slice(params.key.as_bytes()).expect("hmac key");
    mac.update(signing_input.as_bytes());
    let sig = base64url_nopad(&mac.finalize().into_bytes());
    format!("{}.{}", signing_input, sig)
}

pub fn annotator_token_ok(token: &str) -> bool {
    let Some(payload) = jwt_payload(token) else {
        return false;
    };
    let Some(exp_ms) = payload.get("exp").and_then(|x| x.as_u64()) else {
        return false;
    };
    exp_ms.saturating_sub(now_ms()) > 30 * 24 * 3600 * 1000
}

pub fn ensure_annotator_token(cfg: &mut AuthConfig) -> bool {
    if annotator_token_ok(&cfg.x_annotator_auth_token) {
        return false;
    }
    if cfg.open_id.is_empty() {
        return false;
    }
    let params = AnnotatorParams::from_config(cfg);
    cfg.x_annotator_auth_token = generate_annotator_token(&cfg.open_id, &params);
    log::info!("已本地签发 x_annotator_auth_token（有效期 1 年）");
    true
}

pub async fn extract_annotator_params() -> Result<AnnotatorParams> {
    let client = sso_client()?;
    let html = client
        .get(EXPLORATION_PC_URL)
        .send()
        .await
        .context("获取 ucontent 页面失败")?
        .text()
        .await
        .context("读取 ucontent 页面失败")?;
    let files = extract_script_srcs(&html);
    if files.is_empty() {
        bail!("未在 ucontent 页面中找到前端脚本");
    }
    let base = "https://ucontent.unipus.cn/_explorationpc_default/";
    let mut last_err = String::new();
    for file in files {
        let url = if file.starts_with("http") {
            file.clone()
        } else {
            format!("{}{}", base, file.trim_start_matches('/'))
        };
        let Ok(resp) = client.get(&url).send().await else {
            continue;
        };
        let Ok(js) = resp.text().await else {
            continue;
        };
        match parse_annotator_params(&js) {
            Some(p) => {
                log::info!(
                    "已从 bundle 提取 annotator 参数: iss={} aud={} ttl={}ms key={}…",
                    p.iss,
                    p.aud,
                    p.ttl_ms,
                    &p.key[..p.key.len().min(8)]
                );
                return Ok(p);
            }
            None => last_err = file,
        }
    }
    bail!(
        "未在 bundle 中找到 annotator 签发参数（最后检查: {}）",
        last_err
    )
}

pub(super) fn extract_script_srcs(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("src=") {
        rest = &rest[i + 4..];
        let trimmed = rest.trim_start();
        let quote = trimmed.chars().next().unwrap_or(' ');
        if quote != '"' && quote != '\'' {
            continue;
        }
        rest = &trimmed[1..];
        if let Some(end) = rest.find(quote) {
            let src = &rest[..end];
            rest = &rest[end..];
            if src.ends_with(".js") {
                out.push(src.to_string());
            }
        }
    }
    out
}

pub fn parse_annotator_params(js: &str) -> Option<AnnotatorParams> {
    let window = match js.find("_generateJwtToken") {
        Some(i) => &js[i..js.floor_char_boundary((i + 4000).min(js.len()))],
        None => js,
    };
    let iss_marker = if window.contains("iss:\"") {
        "iss:\""
    } else {
        "\"iss\":\""
    };
    let aud_marker = if window.contains("aud:\"") {
        "aud:\""
    } else {
        "\"aud\":\""
    };
    let iss = find_quoted_after(window, iss_marker)?;
    let aud = find_quoted_after(window, aud_marker)?;
    let ttl_ms = find_number_after(window, "exp:Date.now()+").unwrap_or(DEFAULT_ANNOTATOR_TTL_MS);

    let key_start = window.find(iss_marker).unwrap_or(0);
    let key = find_hex_string(&window[key_start..], 32).filter(|k| *k != iss)?;
    if key.len() < 16 || iss.is_empty() || aud.is_empty() {
        return None;
    }
    Some(AnnotatorParams {
        key,
        iss,
        aud,
        ttl_ms,
    })
}

fn find_quoted_after(s: &str, marker: &str) -> Option<String> {
    let i = s.find(marker)? + marker.len();
    let rest = &s[i..];
    let end = rest.find('"')?;
    let v = rest[..end].to_string();
    (!v.is_empty()).then_some(v)
}

fn find_number_after(s: &str, marker: &str) -> Option<u64> {
    let i = s.find(marker)? + marker.len();
    let rest = &s[i..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+'))
        .unwrap_or(rest.len());
    rest[..end].parse::<f64>().ok().map(|v| v as u64)
}

fn find_hex_string(s: &str, min_len: usize) -> Option<String> {
    let mut rest = s;
    while let Some(i) = rest.find('"') {
        rest = &rest[i + 1..];
        let Some(end) = rest.find('"') else { break };
        let cand = &rest[..end];
        if cand.len() >= min_len && cand.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(cand.to_string());
        }
        rest = &rest[end..];
    }
    None
}

pub async fn renew_annotator(cfg: &mut AuthConfig) -> bool {
    let mut changed = false;
    match extract_annotator_params().await {
        Ok(p) => {
            if p.key != cfg.annotator_key
                || p.iss != cfg.annotator_iss
                || p.aud != cfg.annotator_aud
                || p.ttl_ms != cfg.annotator_ttl_ms
            {
                cfg.annotator_key = p.key;
                cfg.annotator_iss = p.iss;
                cfg.annotator_aud = p.aud;
                cfg.annotator_ttl_ms = p.ttl_ms;
                changed = true;
                log::info!("annotator 签发参数已更新（来源 bundle）");
            }
        }
        Err(e) => log::warn!("从 bundle 提取 annotator 参数失败（沿用现有参数）: {:#}", e),
    }
    if !cfg.open_id.is_empty() {
        let params = AnnotatorParams::from_config(cfg);
        cfg.x_annotator_auth_token = generate_annotator_token(&cfg.open_id, &params);
        changed = true;
    }
    changed
}
