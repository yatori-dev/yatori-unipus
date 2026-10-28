use crate::config::TranscriptionConfig;
use anyhow::Result;
use std::path::PathBuf;

mod cache;
#[cfg(feature = "whisper")]
mod ffmpeg;
#[cfg(feature = "whisper")]
mod model;
mod subtitle;
#[cfg(feature = "whisper")]
mod whisper;

/// 转写只使用通用 HTTP 客户端，不携带平台身份头。
pub(crate) struct Transcriber {
    http: reqwest::Client,
    config: TranscriptionConfig,
    data_dir: PathBuf,
}

impl Transcriber {
    pub(crate) fn new(
        http: reqwest::Client,
        config: TranscriptionConfig,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            http,
            config,
            data_dir,
        }
    }

    pub(crate) async fn transcribe(&self, url: &str) -> Result<String> {
        let entry = cache::Entry::new(&self.data_dir, url).await?;
        if let Some(text) = entry.read().await {
            return Ok(text);
        }
        let lower = url.to_ascii_lowercase();
        let text = if lower.ends_with(".vtt") || lower.ends_with(".srt") {
            let response = self.http.get(url).send().await?.error_for_status()?;
            let bytes = response.bytes().await?;
            subtitle::to_text(&String::from_utf8_lossy(&bytes))
        } else {
            if !self.config.whisper_enabled {
                return Ok(String::new());
            }
            self.infer(&entry, url, &lower).await?
        };
        entry.write(&text).await;
        Ok(text)
    }

    #[cfg(feature = "whisper")]
    async fn infer(&self, entry: &cache::Entry, url: &str, lower: &str) -> Result<String> {
        use anyhow::Context;
        use tokio::io::AsyncWriteExt;

        let extension = if lower.contains(".mp4") || lower.contains("video") {
            "mp4"
        } else if lower.contains(".m4a") {
            "m4a"
        } else if lower.contains(".wav") {
            "wav"
        } else {
            "mp3"
        };
        let media = entry.path(&format!("source.{extension}"));
        if tokio::fs::metadata(&media)
            .await
            .map(|m| m.len())
            .unwrap_or(0)
            <= 1000
        {
            let partial = entry.temporary("source");
            let mut response = self.http.get(url).send().await?.error_for_status()?;
            let mut output = tokio::fs::File::create(partial.path()).await?;
            while let Some(chunk) = response.chunk().await? {
                output.write_all(&chunk).await?;
            }
            output.flush().await?;
            let size = output.metadata().await?.len();
            drop(output);
            anyhow::ensure!(size > 0, "媒体下载为空: {url}");
            tokio::fs::rename(partial.path(), &media).await?;
        }
        let wav = entry.temporary("wav");
        let model = self.config.whisper_model.clone();
        let language = self.config.whisper_language.clone();
        log::info!("whisper 转写中: {}", crate::text::truncate(url, 80));
        let text = tokio::task::spawn_blocking(move || {
            ffmpeg::extract_wav(&media, wav.path())?;
            whisper::transcribe(wav.path(), &model, &language)
        })
        .await
        .context("媒体转写线程异常退出")??;
        log::info!("whisper 转写完成: {} 字", text.chars().count());
        Ok(text)
    }

    #[cfg(not(feature = "whisper"))]
    async fn infer(&self, _: &cache::Entry, _: &str, _: &str) -> Result<String> {
        anyhow::bail!("本地媒体转写需要启用 yatori-unipus 的 whisper feature")
    }
}
