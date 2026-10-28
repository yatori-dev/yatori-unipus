use super::token::{jwt_exp, jwt_open_id};
use anyhow::{Context, Result, bail};
use serde_json::Value;

pub const SSO: &str = "https://sso.unipus.cn";

pub const SERVICE: &str = "https://uai.unipus.cn";

#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub jwt: String,
    pub refresh_token: String,
    pub open_id: String,

    pub jwt_expire: i64,

    pub rt_expire: i64,
}

pub fn encrypt_field(s: &str) -> String {
    use aes::Aes128;
    use cbc::cipher::{BlockModeEncrypt, KeyIvInit, block_padding::Pkcs7};
    let key = hex::decode("8AD70B641C024C7ADA2ECD082EC0334F").expect("key hex");
    let iv: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let ct = cbc::Encryptor::<Aes128>::new_from_slices(&key, &iv)
        .expect("key/iv 长度合法")
        .encrypt_padded_vec::<Pkcs7>(s.as_bytes());
    ct.iter().map(|b| format!("{:02X}", b)).collect()
}

pub(super) fn sso_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .context("构建登录客户端失败")
}

pub(super) fn parse_credentials(v: &Value, action: &str) -> Result<Credentials> {
    let code_ok = v
        .get("code")
        .map(|c| {
            c.as_str()
                .map(|s| s == "0")
                .unwrap_or_else(|| c.as_i64() == Some(0))
        })
        .unwrap_or(false);
    if !code_ok {
        let code = v.get("code").map(|c| c.to_string()).unwrap_or_default();
        let msg = v
            .get("msg")
            .or_else(|| v.get("error"))
            .and_then(|m| m.as_str())
            .unwrap_or("");
        return match code.trim_matches('"') {
            "1506" => Err(anyhow::anyhow!(
                "{}失败：触发滑块验证（极验），自动登录无法完成，请稍后重试",
                action
            )),
            "1504" | "1502" | "1509" => Err(anyhow::anyhow!(
                "{}失败：需要图形验证码（code={}），自动登录无法完成，请稍后重试",
                action,
                code
            )),
            "1507" => Err(anyhow::anyhow!(
                "{}失败：今日验证次数过多，请稍后再试",
                action
            )),
            _ => Err(anyhow::anyhow!("{}失败：code={} msg={}", action, code, msg)),
        };
    }
    let rs = v.get("rs").unwrap_or(v);
    let jwt = rs
        .get("jwt")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    if jwt.is_empty() {
        bail!(
            "{}响应缺少 jwt：{}",
            action,
            crate::text::truncate(&v.to_string(), 200)
        );
    }
    let refresh_token = rs
        .get("rt")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    let open_id = rs
        .get("openid")
        .or_else(|| rs.get("openId"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| jwt_open_id(&jwt))
        .unwrap_or_default();

    let jwt_expire = jwt_exp(&jwt)
        .or_else(|| {
            rs.get("jwtExpire")
                .and_then(|x| x.as_i64())
                .filter(|&e| e > 0)
        })
        .unwrap_or(0);
    let rt_expire = rs
        .get("rtExpire")
        .and_then(|x| x.as_i64())
        .filter(|&e| e > 0)
        .unwrap_or(0);
    Ok(Credentials {
        jwt,
        refresh_token,
        open_id,
        jwt_expire,
        rt_expire,
    })
}

pub async fn login(username: &str, password: &str) -> Result<Credentials> {
    let body = serde_json::json!({
        "username": encrypt_field(username.trim()),
        "password": encrypt_field(password),
        "remember": true,
        "service": SERVICE,
    });
    let resp = sso_client()?
        .post(format!("{}/sso/0.1/sso/cip/login", SSO))
        .header("origin", SERVICE)
        .header("referer", format!("{}/", SERVICE))
        .json(&body)
        .send()
        .await
        .context("登录请求发送失败")?;
    let status = resp.status();
    let text = resp.text().await.context("读取登录响应失败")?;
    if !status.is_success() {
        bail!(
            "登录失败：HTTP {} {}",
            status,
            crate::text::truncate(&text, 200)
        );
    }
    let v: Value = serde_json::from_str(&text)
        .with_context(|| format!("登录响应不是 JSON: {}", crate::text::truncate(&text, 200)))?;
    parse_credentials(&v, "登录")
}

pub async fn refresh(refresh_token: &str) -> Result<Credentials> {
    let body = serde_json::json!({ "rt": refresh_token });
    let resp = sso_client()?
        .post(format!("{}/sso/4.0/sso/refresh_jwt", SSO))
        .header("origin", SERVICE)
        .header("referer", format!("{}/", SERVICE))
        .json(&body)
        .send()
        .await
        .context("刷新请求发送失败")?;
    let status = resp.status();
    let text = resp.text().await.context("读取刷新响应失败")?;
    if !status.is_success() {
        bail!(
            "刷新失败：HTTP {} {}",
            status,
            crate::text::truncate(&text, 200)
        );
    }
    let v: Value = serde_json::from_str(&text)
        .with_context(|| format!("刷新响应不是 JSON: {}", crate::text::truncate(&text, 200)))?;
    parse_credentials(&v, "刷新登录")
}
