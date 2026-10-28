use crate::api::{
    course::{fetch_course_progress, fetch_home_courses, fetch_unit},
    login::{ensure_course, ensure_login, ensure_profile},
    session::Session,
};
use crate::archive::{self, Archive, ArchiveOptions, ArchiveSummary, Preview};
use crate::config::{AuthConfig, Config};
use crate::course::{CourseProgress, HomeCourse, UnitProgress};
use crate::execution::{
    self, RunOptions, RunSummary,
    planner::{Plan, plan_course},
};
use crate::reporter::Reporter;
use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// U 校园 SDK 入口。默认只保存内存配置；查询与执行均显式调用。
pub struct Client {
    session: Session,
}

impl Client {
    /// 构造客户端，不登录、不访问平台，也不创建归档。
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            session: Session::from_config(config)?,
        })
    }

    pub fn from_credentials(
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Self> {
        Self::new(Config {
            auth: AuthConfig {
                username: username.into(),
                password: password.into(),
                ..AuthConfig::default()
            },
            ..Config::default()
        })
    }

    /// 显式启用会话配置写回；刷新凭证和课程选择会写入此文件。
    pub fn with_config_path(config: Config, path: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            session: Session::new(config, path.into())?,
        })
    }

    pub fn from_config_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        Self::with_config_path(Config::load(path)?, path.to_path_buf())
    }

    /// 返回当前配置快照，包含续期后的 refresh token。
    pub fn config(&self) -> Config {
        self.session.cfg()
    }

    pub fn update_config(&mut self, config: Config) -> Result<()> {
        self.session.update_config(config)
    }

    /// 登录或刷新凭证，并补全账号和所选课程的身份信息。
    pub async fn authenticate(&mut self) -> Result<()> {
        ensure_login(&self.session).await?;
        ensure_profile(&self.session).await?;
        Ok(())
    }

    pub async fn courses(&self) -> Result<Vec<HomeCourse>> {
        fetch_home_courses(&self.session).await
    }

    pub fn select_course(&mut self, course: &HomeCourse) -> Result<()> {
        self.session.select_course(course)
    }

    pub async fn select_course_by_id(&mut self, course_id: &str) -> Result<HomeCourse> {
        let course = self
            .courses()
            .await?
            .into_iter()
            .find(|course| course.course_id == course_id)
            .ok_or_else(|| anyhow::anyhow!("未找到课程: {course_id}"))?;
        self.select_course(&course)?;
        Ok(course)
    }

    pub async fn course_progress(&self) -> Result<CourseProgress> {
        ensure_course(&self.session)?;
        fetch_course_progress(&self.session).await
    }

    pub async fn unit_progress(&self, unit_id: &str) -> Result<UnitProgress> {
        ensure_course(&self.session)?;
        fetch_unit(&self.session, unit_id).await
    }

    pub async fn plan(&self) -> Result<Plan> {
        ensure_course(&self.session)?;
        plan_course(&self.session).await
    }

    /// 执行所选任务，可能提交答案、标记已看或发布讨论回复。
    /// `unit_ids` 为空时执行当前课程全部单元。
    pub async fn run(
        &mut self,
        options: &RunOptions,
        reporter: &dyn Reporter,
        cancel: &CancellationToken,
    ) -> Result<RunSummary> {
        ensure_course(&self.session)?;
        if options.unit_ids.is_empty() {
            execution::run_course(&mut self.session, options.with_names, reporter, cancel).await
        } else {
            execution::run_course_units(
                &mut self.session,
                &options.unit_ids,
                options.with_names,
                reporter,
                cancel,
            )
            .await
        }
    }

    /// 抓取内容并归档到本地 SQLite，不提交答案。
    pub async fn archive(
        &self,
        options: &ArchiveOptions,
        reporter: &dyn Reporter,
        cancel: &CancellationToken,
    ) -> Result<ArchiveSummary> {
        ensure_course(&self.session)?;
        archive::archive_course(&self.session, options, reporter, cancel).await
    }

    /// 打开当前账号的本地归档，不请求平台。
    pub fn archive_store(&self) -> Result<Archive> {
        let config = self.session.cfg();
        Archive::open(config.data_dir, &config.auth.open_id)
    }

    /// 获取远端任务原文和解析结果，不提交答案。
    pub async fn preview(&self, group_id: &str) -> Result<Preview> {
        ensure_course(&self.session)?;
        archive::load_preview(&self.session, group_id).await
    }

    pub async fn transcribe(&self, media_url: &str) -> Result<String> {
        let config = self.session.cfg();
        crate::transcribe::Transcriber::new(
            self.session.http_client(),
            config.transcription,
            config.data_dir,
        )
        .transcribe(media_url)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_futures_can_be_sent_to_the_host_runtime() {
        fn assert_send(_: impl Send) {}
        let mut client = Client::new(Config::default()).unwrap();
        let cancel = CancellationToken::new();
        let run_options = RunOptions::default();
        let archive_options = ArchiveOptions::default();
        assert_send(client.authenticate());
        assert_send(client.courses());
        assert_send(client.plan());
        assert_send(client.run(&run_options, &(), &cancel));
        assert_send(client.archive(&archive_options, &(), &cancel));
        assert_send(client.preview("group-1"));
        assert_send(client.transcribe("https://example.org/subtitles.vtt"));
    }

    #[test]
    fn credentials_constructor_keeps_credentials_in_memory() {
        let client = Client::from_credentials("user", "password").unwrap();
        assert_eq!(client.config().auth.username, "user");
        assert_eq!(client.config().auth.password, "password");
        assert_eq!(client.config().data_dir, PathBuf::from("."));
    }

    #[test]
    fn select_course_clears_stale_identity() {
        let mut client = Client::new(Config {
            course: crate::config::CourseConfig {
                course_id: "old-course".into(),
                class_id: "old-class".into(),
                curricula_id: "old-curricula".into(),
                ..crate::config::CourseConfig::default()
            },
            ..Config::default()
        })
        .unwrap();
        let course = HomeCourse {
            name: "Personal course".into(),
            course_id: "course-1".into(),
            class_id: String::new(),
            curricula_id: String::new(),
            group_label: "个人学习".into(),
        };
        client.select_course(&course).unwrap();
        assert_eq!(client.config().course.course_id, "course-1");
        assert!(client.config().course.class_id.is_empty());
        assert!(client.config().course.curricula_id.is_empty());
    }
}
