use anyhow::{Context, Result};
use std::path::Path;
use std::process::Stdio;

fn which(cmd: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(cmd);
        if cand.is_file() {
            return Some(cand);
        }
        let exe = dir.join(format!("{}.exe", cmd));
        if exe.is_file() {
            return Some(exe);
        }
    }
    None
}

pub(super) fn extract_wav(media: &Path, wav: &Path) -> Result<()> {
    let ffmpeg = which("ffmpeg").ok_or_else(|| anyhow::anyhow!("未找到 ffmpeg"))?;
    let out = std::process::Command::new(ffmpeg)
        .arg("-y")
        .arg("-i")
        .arg(media)
        .arg("-ar")
        .arg("16000")
        .arg("-ac")
        .arg("1")
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg("-f")
        .arg("wav")
        .arg(wav)
        .stdout(Stdio::null())
        .output()
        .context("运行 ffmpeg 失败")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!(
            "ffmpeg 转码失败 status={} stderr={}",
            out.status,
            stderr.trim()
        );
    }
    Ok(())
}
