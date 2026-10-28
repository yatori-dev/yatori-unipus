use super::Session;
use crate::config::Config;
use anyhow::{Context, Result};
use reqwest::{Method, RequestBuilder, Response, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

pub(crate) const UCONTENT: &str = "https://ucontent.unipus.cn";

fn platform_url(url: &Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("ucontent.unipus.cn" | "uai.unipus.cn" | "ucloud.unipus.cn")
        )
}

pub(super) fn build_clients(config: &Config) -> Result<(reqwest::Client, reqwest::Client)> {
    let builder = || {
        reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout.max(3)))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36 Edg/151.0.0.0")
    };
    let client = builder().build().context("构建 HTTP 客户端失败")?;
    let platform_client = builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("平台请求重定向次数过多")
            } else if platform_url(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("平台请求不能携带身份头重定向到外部地址")
            }
        }))
        .build()
        .context("构建平台 HTTP 客户端失败")?;
    Ok((client, platform_client))
}

impl Session {
    fn request(&self, method: Method, url: &str, authorization: Option<&str>) -> RequestBuilder {
        let platform = Url::parse(url).is_ok_and(|url| platform_url(&url));
        let state = self.read();
        let mut request = if platform {
            state.platform_client.request(method, url)
        } else {
            state.client.request(method, url)
        };
        if !platform {
            return request;
        }
        request = request
            .header("u-app-id", "39")
            .header("u-platform", "2")
            .header("appid", "undefined")
            .header("origin", UCONTENT)
            .header(
                "referer",
                "https://ucontent.unipus.cn/_explorationpc_default/pc.html",
            );
        let auth = &state.config.auth;
        for (name, value) in [
            ("u-openid", auth.open_id.as_str()),
            ("x-csrftoken", auth.open_id.as_str()),
            ("u-school", auth.u_school.as_str()),
            (
                "x-annotator-auth-token",
                auth.x_annotator_auth_token.as_str(),
            ),
        ] {
            if !value.is_empty() {
                request = request.header(name, value);
            }
        }
        if !state.jwt.is_empty() {
            request = request.header("cookie", format!("jwt={}", state.jwt));
        }
        if let Some(value) =
            authorization.or_else(|| (!state.jwt.is_empty()).then_some(state.jwt.as_str()))
        {
            request = request.header("authorization", value);
        }
        request
    }

    async fn send_with_retry(
        &self,
        url: &str,
        build: impl Fn() -> RequestBuilder,
    ) -> Result<Response> {
        let (jwt, generation) = {
            let state = self.read();
            (state.jwt.clone(), state.identity_generation)
        };
        let mut response = build().send().await.context("请求发送失败")?;
        if response.status() != StatusCode::UNAUTHORIZED
            || !Url::parse(url).is_ok_and(|url| platform_url(&url))
        {
            return Ok(response);
        }
        if self.identity_generation() != generation {
            anyhow::bail!("账号配置在请求期间已更新，未重试旧账号请求");
        }
        log::warn!("接口返回 401，尝试刷新登录凭证");
        match self.refresh_rejected_auth(&jwt).await {
            Ok(true) => {
                if self.identity_generation() != generation {
                    anyhow::bail!("账号配置已更新，未重试旧账号请求");
                }
                response = build().send().await.context("刷新后重试请求失败")?;
                if response.status() != StatusCode::UNAUTHORIZED {
                    return Ok(response);
                }
            }
            Ok(false) => {}
            Err(error) => log::warn!("自动刷新登录失败: {error:#}"),
        }
        if self.identity_generation() != generation {
            anyhow::bail!("账号配置已更新，未重试旧账号请求");
        }
        if self.refresh_annotator().await? {
            if self.identity_generation() != generation {
                anyhow::bail!("账号配置已更新，未重试旧账号请求");
            }
            response = build()
                .send()
                .await
                .context("annotator 重签后重试请求失败")?;
        }
        Ok(response)
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let response = self
            .send_with_retry(url, || {
                self.request(Method::GET, url, None)
                    .header("accept", "application/json, text/plain, */*")
            })
            .await
            .context("GET 请求失败")?;
        let status = response.status();
        let body = response.text().await.context("读取响应失败")?;
        if !status.is_success() {
            anyhow::bail!("GET {url} HTTP {status}");
        }
        let value: Value = serde_json::from_str(&body)
            .with_context(|| format!("解析JSON失败: {}", crate::text::truncate(&body, 300)))?;
        check_code(&value)?;
        serde_json::from_value(value).context("反序列化失败")
    }

    pub(crate) async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .send_with_retry(url, || self.request(Method::GET, url, None))
            .await
            .context("下载请求失败")?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("下载 {url} HTTP {status}");
        }
        Ok(response.bytes().await.context("读取下载内容失败")?.to_vec())
    }

    pub(crate) async fn post_raw_with_auth(
        &self,
        url: &str,
        body: &str,
        authorization: Option<&str>,
    ) -> Result<(StatusCode, String)> {
        let build = || {
            self.request(Method::POST, url, authorization)
                .header("accept", "application/json, text/plain, */*")
                .header("content-type", "application/json; charset=UTF-8")
                .body(body.to_string())
        };
        let response = if authorization.is_some() {
            build().send().await?
        } else {
            self.send_with_retry(url, build).await?
        };
        let status = response.status();
        Ok((status, response.text().await?))
    }

    pub(crate) async fn post_raw(&self, url: &str, body: &str) -> Result<(StatusCode, String)> {
        self.post_raw_with_auth(url, body, None).await
    }
}

fn check_code(value: &Value) -> Result<()> {
    if let Some(code) = value
        .get("code")
        .and_then(Value::as_i64)
        .filter(|code| *code != 0)
    {
        let message = value.get("msg").and_then(Value::as_str).unwrap_or_default();
        anyhow::bail!("接口返回错误 code={code} msg={message}");
    }
    Ok(())
}

pub(crate) fn progress_url(course: &str, open_id: &str) -> String {
    format!("{UCONTENT}/course/api/v2/course_progress/{course}/{open_id}/default")
}

pub(crate) fn unit_progress_url(course: &str, unit_id: &str, open_id: &str) -> String {
    format!("{UCONTENT}/course/api/v2/course_progress/{course}/{unit_id}/{open_id}/default/")
}

pub(crate) fn content_url(course: &str, group_id: &str) -> String {
    format!("{UCONTENT}/course/api/v3/content/{course}/{group_id}/default")
}

pub(crate) fn submit_url() -> &'static str {
    "https://ucontent.unipus.cn/course/api/v3/newExploration/submit"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn platform_identity_headers_are_confined_to_platform_requests() {
        let mut config = Config::default();
        config.auth.open_id = "account-1".into();
        config.auth.x_annotator_auth_token = "annotator-token".into();
        let session = Session::from_config(config).unwrap();
        let platform = session
            .request(Method::GET, "https://ucontent.unipus.cn/course", None)
            .build()
            .unwrap();
        assert_eq!(platform.headers()["u-openid"], "account-1");
        let external = session
            .request(Method::GET, "https://media.example.org/video", None)
            .build()
            .unwrap();
        for header in [
            "u-openid",
            "x-csrftoken",
            "x-annotator-auth-token",
            "cookie",
            "authorization",
        ] {
            assert!(!external.headers().contains_key(header));
        }
        let llm = session
            .http_client()
            .get("https://ucontent.unipus.cn/llm")
            .build()
            .unwrap();
        assert!(llm.headers().is_empty());
    }

    #[test]
    fn invalid_header_values_produce_request_errors() {
        let mut config = Config::default();
        config.auth.open_id = "invalid\nheader".into();
        let session = Session::from_config(config).unwrap();
        assert!(
            session
                .request(Method::GET, "https://ucontent.unipus.cn/course", None)
                .build()
                .is_err()
        );
    }
}
