use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::time::Duration;

use crate::config::LlmConfig;

#[derive(Debug)]
struct LlmError {
    retryable: bool,
    message: String,
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LlmError {}

pub(crate) async fn ask(
    client: &reqwest::Client,
    cfg: &LlmConfig,
    system: &str,
    prompt: &str,
) -> Result<String> {
    let base = normalize_base(&cfg.base_url);
    let url = format!("{}/chat/completions", base);

    let payload = json!({
        "model": cfg.model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": prompt}
        ],
        "temperature": cfg.temperature,
        "max_tokens": cfg.max_tokens,
        "thinking": {"type": "disabled"},
    });

    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..3 {
        match send_once(client, &url, &payload, cfg.api_key.as_str()).await {
            Ok(text) => return Ok(text),
            Err(e) => {
                let retryable = e
                    .chain()
                    .find_map(|cause| cause.downcast_ref::<LlmError>())
                    .is_some_and(|e| e.retryable);
                if !retryable {
                    return Err(e);
                }
                last_err = Some(e);
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("LLM 调用失败")))
}

async fn send_once(
    client: &reqwest::Client,
    url: &str,
    payload: &Value,
    api_key: &str,
) -> Result<String> {
    let resp = client
        .post(url)
        .header("authorization", format!("Bearer {}", api_key))
        .header("content-type", "application/json")
        .json(payload)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| LlmError {
            retryable: true,
            message: format!("LLM 请求失败: {e}"),
        })?;
    let status = resp.status();
    let body = resp.text().await.context("读取 LLM 响应失败")?;
    if !status.is_success() {
        return Err(LlmError {
            retryable: status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error(),
            message: format!("LLM HTTP {}: {}", status, crate::text::truncate(&body, 200)),
        }
        .into());
    }
    let v: Value = serde_json::from_str(&body).context("解析 LLM 响应失败")?;
    let msg = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"));
    let content = msg.and_then(|m| m.get("content")).and_then(|c| c.as_str());
    if let Some(s) = content {
        let s = s.trim();
        if !s.is_empty() {
            return Ok(s.to_string());
        }
    }

    let reasoning = msg
        .and_then(|m| m.get("reasoning_content"))
        .and_then(|c| c.as_str());
    if let Some(s) = reasoning {
        let s = s.trim();
        if !s.is_empty() {
            return Ok(s.to_string());
        }
    }
    Err(anyhow::anyhow!(
        "LLM 响应缺少内容: {}",
        crate::text::truncate(&body, 200)
    ))
}

fn normalize_base(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    if is_bare_host(b) {
        format!("{}/v1", b)
    } else {
        b.to_string()
    }
}

fn is_bare_host(b: &str) -> bool {
    let rest = match b
        .strip_prefix("https://")
        .or_else(|| b.strip_prefix("http://"))
    {
        Some(r) => r,
        None => return b.contains('.'),
    };
    let host = rest.split('/').next().unwrap_or(rest);
    host == rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_host() {
        assert_eq!(
            normalize_base("https://api.deepseek.com"),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(
            normalize_base("https://api.deepseek.com/"),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(
            normalize_base("https://api.moonshot.cn/v1"),
            "https://api.moonshot.cn/v1"
        );
        assert_eq!(
            normalize_base("https://api.deepseek.com:443"),
            "https://api.deepseek.com:443/v1"
        );
    }
}
