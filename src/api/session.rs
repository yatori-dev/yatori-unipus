mod auth;
mod transport;

pub(crate) use transport::{content_url, progress_url, submit_url, unit_progress_url};

use crate::api::login::Credentials;
use crate::config::Config;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock, RwLockReadGuard};

struct SessionState {
    config: Config,
    jwt: String,
    client: reqwest::Client,
    platform_client: reqwest::Client,
    identity_generation: u64,
    course_names: BTreeMap<(String, String), String>,
    unit_labels: BTreeMap<(String, String, String), String>,
}

/// Shared state and synchronization belong to the session, including all its clones.
#[derive(Clone)]
pub(crate) struct Session {
    state: Arc<RwLock<SessionState>>,
    config_path: Option<PathBuf>,
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
    annotator_lock: Arc<tokio::sync::Mutex<()>>,
}

pub(super) struct CacheScope {
    account: String,
    course: String,
    generation: u64,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.read();
        f.debug_struct("Session")
            .field("config_path", &self.config_path)
            .field("jwt_len", &state.jwt.len())
            .field(
                "has_refresh_token",
                &!state.config.auth.refresh_token.is_empty(),
            )
            .finish()
    }
}

impl Session {
    pub(crate) fn new(cfg: Config, config_path: PathBuf) -> Result<Self> {
        Self::build(cfg, Some(config_path))
    }

    pub(crate) fn from_config(cfg: Config) -> Result<Self> {
        Self::build(cfg, None)
    }

    fn build(mut config: Config, config_path: Option<PathBuf>) -> Result<Self> {
        config.normalize();
        let (client, platform_client) = transport::build_clients(&config)?;
        Ok(Self {
            state: Arc::new(RwLock::new(SessionState {
                config,
                jwt: String::new(),
                client,
                platform_client,
                identity_generation: 0,
                course_names: BTreeMap::new(),
                unit_labels: BTreeMap::new(),
            })),
            config_path,
            refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
            annotator_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    fn read(&self) -> RwLockReadGuard<'_, SessionState> {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn cfg(&self) -> Config {
        self.read().config.clone()
    }

    pub(crate) fn data_dir(&self) -> PathBuf {
        self.read().config.data_dir.clone()
    }

    /// An ordinary client without platform headers or a shared cookie jar.
    pub(crate) fn http_client(&self) -> reqwest::Client {
        self.read().client.clone()
    }

    pub(crate) fn is_authenticated(&self) -> bool {
        !self.read().jwt.is_empty()
    }

    pub(super) fn identity_generation(&self) -> u64 {
        self.read().identity_generation
    }

    /// Merge owned fields against the latest state and publish only after persistence succeeds.
    fn commit_config(
        &self,
        generation: Option<u64>,
        update: impl FnOnce(&mut Config, &mut String) -> Result<bool>,
    ) -> Result<bool> {
        let mut state = self
            .state
            .write()
            .map_err(|_| anyhow::anyhow!("会话状态锁已失效"))?;
        if generation.is_some_and(|generation| generation != state.identity_generation) {
            anyhow::bail!("账号配置在请求期间已更新，未应用旧账号的响应");
        }
        let mut config = state.config.clone();
        let mut jwt = state.jwt.clone();
        if !update(&mut config, &mut jwt)? {
            return Ok(false);
        }
        if let Some(path) = &self.config_path {
            config.save(path).context("写回会话配置失败")?;
        }
        state.config = config;
        state.jwt = jwt;
        Ok(true)
    }

    fn update_credentials(&self, credentials: &Credentials, generation: u64) -> Result<bool> {
        self.commit_config(Some(generation), |cfg, jwt| {
            let auth = &mut cfg.auth;
            let open_id = if credentials.open_id.is_empty() {
                crate::api::login::jwt_open_id(&credentials.jwt).unwrap_or_default()
            } else {
                credentials.open_id.clone()
            };
            if auth.open_id != open_id {
                auth.x_annotator_auth_token.clear();
            }
            auth.open_id = open_id;
            auth.refresh_token = credentials.refresh_token.clone();
            auth.jwt_expire = credentials.jwt_expire;
            auth.rt_expire = credentials.rt_expire;
            crate::api::login::ensure_annotator_token(auth);
            *jwt = credentials.jwt.clone();
            Ok(true)
        })
    }

    pub(crate) fn update_config(&mut self, mut config: Config) -> Result<()> {
        config.normalize();
        let (client, platform_client) = transport::build_clients(&config)?;
        let mut state = self
            .state
            .write()
            .map_err(|_| anyhow::anyhow!("会话状态锁已失效"))?;
        let old = &state.config.auth;
        let account_changed =
            config.auth.username != old.username || config.auth.password != old.password;
        let identity_changed = account_changed || config.auth.open_id != old.open_id;
        if account_changed {
            // A caller often edits a snapshot; discard inherited credentials of its old account.
            if config.auth.refresh_token == old.refresh_token {
                config.auth.refresh_token.clear();
                config.auth.rt_expire = 0;
            }
            if config.auth.open_id == old.open_id {
                config.auth.open_id.clear();
                config.auth.u_school.clear();
            }
            if config.auth.x_annotator_auth_token == old.x_annotator_auth_token {
                config.auth.x_annotator_auth_token.clear();
            }
            config.auth.jwt_expire = 0;
        } else if config.auth.open_id != old.open_id
            && config.auth.x_annotator_auth_token == old.x_annotator_auth_token
        {
            config.auth.x_annotator_auth_token.clear();
        }
        if let Some(path) = &self.config_path {
            config.save(path)?;
        }
        if identity_changed {
            state.jwt.clear();
            state.identity_generation = state.identity_generation.wrapping_add(1);
            state.course_names.clear();
            state.unit_labels.clear();
        }
        state.config = config;
        state.client = client;
        state.platform_client = platform_client;
        Ok(())
    }

    pub(crate) fn select_course(&self, course: &crate::course::HomeCourse) -> Result<()> {
        if course.course_id.trim().is_empty() {
            anyhow::bail!("课程 ID 不能为空");
        }
        self.commit_config(None, |cfg, _| {
            crate::api::course::apply_course_selection(&mut cfg.course, course);
            Ok(true)
        })?;
        Ok(())
    }

    pub(crate) fn set_publish_version(&mut self, version: &str) -> Result<()> {
        self.commit_config(None, |cfg, _| {
            if version.is_empty() || version == cfg.course.publish_version {
                return Ok(false);
            }
            cfg.course.publish_version = version.to_string();
            Ok(true)
        })?;
        Ok(())
    }

    pub(crate) fn course_id(&self) -> String {
        self.read().config.course.course_id.clone()
    }

    pub(crate) fn open_id(&self) -> String {
        self.read().config.auth.open_id.clone()
    }

    pub(crate) fn jwt(&self) -> String {
        self.read().jwt.clone()
    }

    pub(super) fn cache_scope(&self, course: &str) -> CacheScope {
        let state = self.read();
        CacheScope {
            account: format!(
                "{}\0{}",
                state.config.auth.username, state.config.auth.open_id
            ),
            course: course.to_string(),
            generation: state.identity_generation,
        }
    }

    pub(super) fn cached_course_name(&self, scope: &CacheScope) -> Option<String> {
        self.read()
            .course_names
            .get(&(scope.account.clone(), scope.course.clone()))
            .cloned()
    }

    pub(super) fn cache_course_name(&self, scope: &CacheScope, name: &str) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if scope.generation == state.identity_generation {
            state.course_names.insert(
                (scope.account.clone(), scope.course.clone()),
                name.to_string(),
            );
        }
    }

    pub(super) fn cached_unit_label(&self, scope: &CacheScope, unit: &str) -> Option<String> {
        self.read()
            .unit_labels
            .get(&(
                scope.account.clone(),
                scope.course.clone(),
                unit.to_string(),
            ))
            .cloned()
    }

    pub(super) fn cache_unit_label(&self, scope: &CacheScope, unit: &str, label: &str) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if scope.generation == state.identity_generation {
            state.unit_labels.insert(
                (
                    scope.account.clone(),
                    scope.course.clone(),
                    unit.to_string(),
                ),
                label.to_string(),
            );
        }
    }
}

#[cfg(test)]
mod tests;
