use crate::api::parser::parse_group;
use crate::api::session::{Session, content_url};
use crate::model::ParsedGroup;
use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, KeyInit};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
struct ContentWrapper {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub k: Option<String>,
}

/// 解密后的内容；浏览材料不一定是 JSON，也不一定包含题目模块。
#[derive(Debug)]
pub struct ContentDocument {
    pub plain: String,
    pub json: Option<Value>,
    pub group: Option<ParsedGroup>,
}

impl ContentDocument {
    fn from_plain(plain: String) -> Self {
        let json = parse_decrypted(&plain).ok();
        let group = json.as_ref().and_then(|value| parse_group(value).ok());
        Self { plain, json, group }
    }
}

/// 获取和解密失败必须由调用方处理，非题目材料则保留原文。
pub(crate) async fn fetch_document(session: &Session, group_id: &str) -> Result<ContentDocument> {
    let url = content_url(&session.course_id(), group_id);
    let content: ContentWrapper = session.get_json(&url).await?;
    let plain = decrypt_content(
        &content.content.unwrap_or_default(),
        &content.k.unwrap_or_default(),
    )?;
    Ok(ContentDocument::from_plain(plain))
}

fn decrypt_content(content: &str, k: &str) -> Result<String> {
    let hex = content.strip_prefix("unipus.").unwrap_or(content).trim();
    if hex.is_empty() {
        return Ok(String::new());
    }
    let cipher_bytes = hex::decode(hex).context("content 不是合法 hex")?;

    let key_string = format!("1a2b3c4d{}", k);
    let mut key_arr = [0u8; 16];
    for (i, b) in key_string.as_bytes().iter().take(16).enumerate() {
        key_arr[i] = *b;
    }
    let cipher = Aes128::new(&key_arr.into());
    let mut out = Vec::with_capacity(cipher_bytes.len());
    for chunk in cipher_bytes.chunks(16) {
        if chunk.len() != 16 {
            break;
        }
        let mut block: aes::Block = Default::default();
        block.copy_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        out.extend_from_slice(&block);
    }
    while out.last() == Some(&0u8) {
        out.pop();
    }
    String::from_utf8(out).context("解密内容不是合法 UTF-8")
}

fn parse_decrypted(json: &str) -> Result<Value> {
    serde_json::from_str(json).context("解密后的 content 不是合法 JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{BlockCipherEncrypt, KeyInit};

    fn encrypt_ecb(plain: &[u8], k: &str) -> Vec<u8> {
        let mut key_arr = [0u8; 16];
        let key_string = format!("1a2b3c4d{}", k);
        for (i, b) in key_string.as_bytes().iter().take(16).enumerate() {
            key_arr[i] = *b;
        }
        let cipher = Aes128::new(&key_arr.into());
        let padded = plain.to_vec();
        let mut out = Vec::new();
        for chunk in padded.chunks(16) {
            let mut block: aes::Block = Default::default();
            block.copy_from_slice(chunk);
            cipher.encrypt_block(&mut block);
            out.extend_from_slice(&block);
        }
        out
    }

    #[test]
    fn decrypt_real_zero_pad() {
        let mut plain = (0..40u8).collect::<Vec<u8>>();
        plain.extend_from_slice(&[0u8; 8]);
        let enc = encrypt_ecb(&plain, "20260808");
        let hex = enc.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let dec = decrypt_content(&format!("unipus.{}", hex), "20260808").unwrap();
        assert_eq!(dec.as_bytes(), &(0..40u8).collect::<Vec<u8>>()[..]);
    }

    #[test]
    fn decrypt_escapes_empty() {
        assert_eq!(decrypt_content("", "x").unwrap(), "");
    }

    #[test]
    fn content_wrapper_ignores_unused_metadata_and_defaults_null_content() {
        for metadata in [
            serde_json::Value::Null,
            serde_json::json!(0.75),
            serde_json::json!("139753"),
        ] {
            let wrapper: ContentWrapper = serde_json::from_value(serde_json::json!({
                "content": null, "k": null, "publish_version": metadata,
            }))
            .unwrap();
            assert!(wrapper.content.is_none());
            assert!(wrapper.k.is_none());
        }
    }

    #[test]
    fn browser_material_retains_original_text_without_questions() {
        let raw = "<p>Read this material.</p>";
        let document = ContentDocument::from_plain(raw.into());
        assert_eq!(document.plain, raw);
        assert!(document.json.is_none());
        assert!(document.group.is_none());

        let document = ContentDocument::from_plain("{}".into());
        assert!(document.json.is_some());
        assert!(document.group.is_none());
    }
}
