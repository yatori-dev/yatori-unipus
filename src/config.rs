use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// SDK configuration. Groups remain flattened in JSON for existing configuration files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub timeout: u64,
    pub data_dir: PathBuf,
    #[serde(flatten)]
    pub auth: AuthConfig,
    #[serde(flatten)]
    pub course: CourseConfig,
    #[serde(flatten)]
    pub llm: LlmConfig,
    #[serde(flatten)]
    pub execution: ExecutionConfig,
    #[serde(flatten)]
    pub transcription: TranscriptionConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub username: String,
    pub password: String,
    pub refresh_token: String,
    pub open_id: String,
    pub jwt_expire: i64,
    pub rt_expire: i64,
    pub u_school: String,
    pub x_annotator_auth_token: String,
    pub annotator_key: String,
    pub annotator_iss: String,
    pub annotator_aud: String,
    pub annotator_ttl_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CourseConfig {
    pub course_id: String,
    pub class_id: String,
    pub curricula_id: String,
    pub publish_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub fallback_on_llm_failure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutionConfig {
    pub learning_strategy: String,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptionConfig {
    pub whisper_enabled: bool,
    pub whisper_model: String,
    pub whisper_language: String,
}

fn default_whisper_model() -> String {
    "base".to_string()
}

fn default_timeout() -> u64 {
    10
}

fn default_data_dir() -> PathBuf {
    PathBuf::from(".")
}

fn default_base_url() -> String {
    "https://api.moonshot.cn/v1".to_string()
}

fn default_model() -> String {
    "kimi-k2-turbo-preview".to_string()
}

fn default_learning_strategy() -> String {
    "learn_all_compulsory_course".to_string()
}

fn default_whisper_language() -> String {
    "auto".to_string()
}

fn default_fallback_on_llm_failure() -> bool {
    true
}

fn default_max_tokens() -> u32 {
    2000
}

fn default_temperature() -> f32 {
    0.3
}

fn default_interval_ms() -> u64 {
    3000
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timeout: default_timeout(),
            data_dir: default_data_dir(),
            auth: AuthConfig::default(),
            course: CourseConfig::default(),
            llm: LlmConfig::default(),
            execution: ExecutionConfig::default(),
            transcription: TranscriptionConfig::default(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: default_base_url(),
            model: default_model(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            fallback_on_llm_failure: default_fallback_on_llm_failure(),
        }
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            learning_strategy: default_learning_strategy(),
            interval_ms: default_interval_ms(),
        }
    }
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            whisper_enabled: false,
            whisper_model: default_whisper_model(),
            whisper_language: default_whisper_language(),
        }
    }
}

impl LlmConfig {
    pub fn is_enabled(&self) -> bool {
        !self.api_key.is_empty()
    }
}

impl ExecutionConfig {
    pub fn compulsory_only(&self) -> bool {
        matches!(
            self.learning_strategy.trim(),
            "learn_all_compulsory_course" | "learn_all_compusory_course"
        )
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!("配置不存在: {}", path.display());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        let mut cfg: Config =
            serde_json::from_str(&text).context("解析配置失败，请检查 JSON 格式")?;
        cfg.normalize();
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)
            .with_context(|| format!("写入临时配置失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("替换配置失败: {}", path.display()))?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let auto = !self.auth.username.is_empty() && !self.auth.password.is_empty();
        if !auto && self.auth.refresh_token.is_empty() {
            anyhow::bail!("config：请填写 username/password 或有效 refresh_token");
        }
        Ok(())
    }

    pub fn normalize(&mut self) {
        if self.timeout == 0 {
            self.timeout = default_timeout();
        }
        if self.llm.base_url.trim().is_empty() {
            self.llm.base_url = default_base_url();
        }
        if self.llm.model.trim().is_empty() {
            self.llm.model = default_model();
        }
        if self.execution.learning_strategy.trim().is_empty() {
            self.execution.learning_strategy = default_learning_strategy();
        }
        if self.llm.max_tokens == 0 {
            self.llm.max_tokens = default_max_tokens();
        }
        if !self.llm.temperature.is_finite() {
            self.llm.temperature = default_temperature();
        }
        if self.execution.interval_ms == 0 {
            self.execution.interval_ms = default_interval_ms();
        }
        if self.transcription.whisper_model.trim().is_empty() {
            self.transcription.whisper_model = default_whisper_model();
        }
        if self.transcription.whisper_language.trim().is_empty() {
            self.transcription.whisper_language = default_whisper_language();
        }
        if self.data_dir.as_os_str().is_empty() {
            self.data_dir = default_data_dir();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_credentials_or_refresh_token_only() {
        let mut cfg = Config::default();
        assert!(cfg.validate().is_err());
        cfg.auth.username = "user".into();
        cfg.auth.password = "password".into();
        assert!(cfg.validate().is_ok());
        cfg.auth = AuthConfig {
            refresh_token: "refresh-token".into(),
            ..AuthConfig::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn flattened_configuration_keeps_json_contract_and_defaults() {
        let mut cfg: Config = serde_json::from_str(r#"{"username":"u","password":"p","course_id":"course-1","api_key":"llm-key","whisper_enabled":true}"#).unwrap();
        cfg.normalize();
        assert_eq!(cfg.auth.username, "u");
        assert_eq!(cfg.course.course_id, "course-1");
        assert_eq!(cfg.llm.api_key, "llm-key");
        assert!(cfg.transcription.whisper_enabled);
        assert_eq!(cfg.timeout, 10);
        assert_eq!(cfg.llm.base_url, "https://api.moonshot.cn/v1");
        assert_eq!(cfg.llm.model, "kimi-k2-turbo-preview");
        assert_eq!(
            cfg.execution.learning_strategy,
            "learn_all_compulsory_course"
        );
        assert_eq!(cfg.execution.interval_ms, 3000);
        assert_eq!(cfg.data_dir, PathBuf::from("."));
        let value = serde_json::to_value(cfg).unwrap();
        assert_eq!(value["username"], "u");
        assert_eq!(value["course_id"], "course-1");
        assert_eq!(value["api_key"], "llm-key");
        assert!(value.get("auth").is_none());
        assert!(value.get("course").is_none());
        assert!(value.get("llm").is_none());
    }

    #[test]
    fn missing_json_fields_match_rust_defaults() {
        let empty: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(
            serde_json::to_value(empty).unwrap(),
            serde_json::to_value(Config::default()).unwrap()
        );
    }
}
