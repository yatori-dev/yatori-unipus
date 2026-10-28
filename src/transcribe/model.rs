use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static DOWNLOAD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(super) struct ModelFiles {
    pub config: PathBuf,
    pub tokenizer: PathBuf,
    pub weights: PathBuf,
    pub multilingual: bool,
}

pub(super) fn load_files(name: &str) -> Result<ModelFiles> {
    let repo = match name {
        "tiny" => "openai/whisper-tiny",
        "tiny.en" => "openai/whisper-tiny.en",
        "base" => "openai/whisper-base",
        "base.en" => "openai/whisper-base.en",
        "small" => "openai/whisper-small",
        "small.en" => "openai/whisper-small.en",
        "medium" => "openai/whisper-medium",
        "medium.en" => "openai/whisper-medium.en",
        "large-v1" => "openai/whisper-large",
        "large-v2" => "openai/whisper-large-v2",
        "large" | "large-v3" => "openai/whisper-large-v3",
        "turbo" | "large-v3-turbo" => "openai/whisper-large-v3-turbo",
        _ => bail!("未知 Whisper 模型: {name}"),
    };
    let root = std::env::var_os("HF_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CACHE_HOME").map(|p| PathBuf::from(p).join("huggingface"))
        })
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|p| PathBuf::from(p).join(".cache/huggingface"))
        })
        .unwrap_or_else(|| PathBuf::from(".whisper_cache"));
    let dir = root.join("whisper-candle").join(repo.replace('/', "__"));
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("创建模型缓存目录失败: {}", dir.display()))?;
    let endpoint = std::env::var("HF_ENDPOINT").unwrap_or_else(|_| "https://huggingface.co".into());
    let base = format!("{}/{repo}/resolve/main", endpoint.trim_end_matches('/'));
    Ok(ModelFiles {
        config: fetch(&base, &dir, "config.json")?,
        tokenizer: fetch(&base, &dir, "tokenizer.json")?,
        weights: fetch(&base, &dir, "model.safetensors")?,
        multilingual: !name.ends_with(".en"),
    })
}

fn fetch(base: &str, dir: &Path, filename: &str) -> Result<PathBuf> {
    let _guard = DOWNLOAD_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("模型下载锁已失效"))?;
    let path = dir.join(filename);
    if path.is_file() && path.metadata()?.len() > 0 {
        return Ok(path);
    }
    let url = format!("{base}/{filename}");
    log::info!("下载 Whisper 模型文件: {url}");
    let mut response = ureq::get(&url)
        .call()
        .with_context(|| format!("下载 {filename} 失败"))?;
    let partial = path.with_extension("download");
    let mut output = std::io::BufWriter::new(
        std::fs::File::create(&partial)
            .with_context(|| format!("创建下载文件失败: {}", partial.display()))?,
    );
    let size = std::io::copy(&mut response.body_mut().as_reader(), &mut output)
        .with_context(|| format!("读取 {filename} 失败"))?;
    if size == 0 {
        bail!("模型文件下载为空: {filename}");
    }
    output.flush().context("写入模型缓存失败")?;
    drop(output);
    std::fs::rename(&partial, &path)
        .with_context(|| format!("保存模型缓存失败: {}", path.display()))?;
    Ok(path)
}
