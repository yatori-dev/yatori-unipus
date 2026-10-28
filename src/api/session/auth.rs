use super::Session;
use crate::api::login;
use crate::config::Config;
use anyhow::{Context, Result};

impl Session {
    pub(crate) async fn refresh_auth(&self) -> Result<bool> {
        self.refresh_credentials(None).await
    }

    pub(crate) async fn refresh_rejected_auth(&self, rejected_jwt: &str) -> Result<bool> {
        self.refresh_credentials(Some(rejected_jwt)).await
    }

    async fn refresh_credentials(&self, rejected_jwt: Option<&str>) -> Result<bool> {
        let _guard = self.refresh_lock.lock().await;
        let (cfg, jwt, generation) = {
            let state = self.read();
            (
                state.config.auth.clone(),
                state.jwt.clone(),
                state.identity_generation,
            )
        };
        let switched = login::account_switched(&cfg, &jwt);
        if let Some(rejected) = rejected_jwt {
            // Another clone may already have replaced the credential used by this request.
            if !switched && !jwt.is_empty() && jwt != rejected {
                return Ok(true);
            }
        } else if !switched && !jwt.is_empty() && login::jwt_fresh_enough(&jwt) {
            return Ok(false);
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut credentials = None;
        if !switched && !cfg.refresh_token.is_empty() && (cfg.rt_expire == 0 || cfg.rt_expire > now)
        {
            match login::refresh(&cfg.refresh_token).await {
                Ok(value) => credentials = Some(value),
                Err(error) => log::warn!("refresh_token 刷新失败: {error:#}"),
            }
        }
        if credentials.is_none() && !cfg.username.is_empty() && !cfg.password.is_empty() {
            credentials = Some(
                login::login(&cfg.username, &cfg.password)
                    .await
                    .context("自动登录失败")?,
            );
        }
        let Some(credentials) = credentials else {
            return Ok(false);
        };
        self.update_credentials(&credentials, generation)?;
        log::info!(
            "登录凭证已刷新（jwt 过期时间 {}）",
            crate::time_util::format_unix_minute(credentials.jwt_expire)
        );
        Ok(true)
    }

    pub(crate) async fn refresh_annotator(&self) -> Result<bool> {
        let _guard = self.annotator_lock.lock().await;
        let (mut auth, generation) = {
            let state = self.read();
            (state.config.auth.clone(), state.identity_generation)
        };
        if !login::renew_annotator(&mut auth).await {
            return Ok(false);
        }
        self.commit_config(Some(generation), |cfg, _| {
            if cfg.auth.open_id != auth.open_id {
                anyhow::bail!("账号身份在签发期间已更新，未应用旧身份的 annotator token");
            }
            cfg.auth.x_annotator_auth_token = auth.x_annotator_auth_token;
            cfg.auth.annotator_key = auth.annotator_key;
            cfg.auth.annotator_iss = auth.annotator_iss;
            cfg.auth.annotator_aud = auth.annotator_aud;
            cfg.auth.annotator_ttl_ms = auth.annotator_ttl_ms;
            Ok(true)
        })
    }

    pub(crate) fn merge_profile(
        &self,
        baseline: &Config,
        profile: &Config,
        generation: u64,
    ) -> Result<bool> {
        self.commit_config(Some(generation), |cfg, _| {
            let mut changed = false;
            if cfg.auth.u_school == baseline.auth.u_school
                && cfg.auth.u_school != profile.auth.u_school
            {
                cfg.auth.u_school = profile.auth.u_school.clone();
                changed = true;
            }
            if cfg.course.course_id == baseline.course.course_id {
                if cfg.course.class_id == baseline.course.class_id
                    && cfg.course.class_id != profile.course.class_id
                {
                    cfg.course.class_id = profile.course.class_id.clone();
                    changed = true;
                }
                if cfg.course.curricula_id == baseline.course.curricula_id
                    && cfg.course.curricula_id != profile.course.curricula_id
                {
                    cfg.course.curricula_id = profile.course.curricula_id.clone();
                    changed = true;
                }
                if cfg.course.publish_version == baseline.course.publish_version
                    && cfg.course.publish_version != profile.course.publish_version
                {
                    cfg.course.publish_version = profile.course.publish_version.clone();
                    changed = true;
                }
            }
            Ok(changed)
        })
    }
}
