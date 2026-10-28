use super::model;
use anyhow::{Context, Result, anyhow, bail, ensure};
use candle_core::{Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{self as w, model::Whisper};
use rand::distr::{Distribution, weighted::WeightedIndex};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tokenizers::Tokenizer;

struct Engine {
    model: Whisper,
    tokenizer: Tokenizer,
    filters: Vec<f32>,
    multilingual: bool,
    special: SpecialTokens,
}

struct SpecialTokens {
    sot: u32,
    eot: u32,
    previous: u32,
    transcribe: u32,
    no_timestamps: u32,
    no_speech: u32,
    blank: Vec<u32>,
    languages: Vec<(String, u32)>,
}

struct Decoded {
    tokens: Vec<u32>,
    text: String,
    average_logprob: f64,
    no_speech: f64,
    compression_ratio: f64,
    temperature: f64,
}

pub(super) fn transcribe(path: &Path, name: &str, language: &str) -> Result<String> {
    static CACHE: OnceLock<Mutex<Option<(String, Engine)>>> = OnceLock::new();
    let pcm = read_pcm(path)?;
    if pcm.is_empty() {
        return Ok(String::new());
    }
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| anyhow!("Whisper 模型缓存锁已失效"))?;
    if cache.as_ref().is_none_or(|(cached, _)| cached != name) {
        log::info!("加载 Whisper 模型: {name}");
        *cache = Some((name.to_owned(), Engine::load(name)?));
    }
    let (_, engine) = cache.as_mut().context("Whisper 模型缓存为空")?;
    engine.transcribe(&pcm, language)
}

fn read_pcm(path: &Path) -> Result<Vec<f32>> {
    let mut wav = hound::WavReader::open(path)
        .with_context(|| format!("打开转码后的 WAV 失败: {}", path.display()))?;
    let spec = wav.spec();
    ensure!(
        spec.sample_rate == w::SAMPLE_RATE as u32
            && spec.channels == 1
            && spec.sample_format == hound::SampleFormat::Int
            && spec.bits_per_sample == 16,
        "Whisper 需要 16 kHz 单声道 PCM16 WAV，请检查 FFmpeg 转码结果"
    );
    wav.samples::<i16>()
        .map(|sample| Ok(sample.context("读取 WAV 样本失败")? as f32 / 32768.0))
        .collect()
}

impl Engine {
    fn load(name: &str) -> Result<Self> {
        let files = model::load_files(name)?;
        let config: w::Config = serde_json::from_reader(std::fs::File::open(&files.config)?)
            .context("解析 Whisper 模型配置失败")?;
        let filters = mel_filters(config.num_mel_bins)?;
        let tokenizer = Tokenizer::from_file(&files.tokenizer)
            .map_err(|e| anyhow!("加载 Whisper tokenizer 失败: {e}"))?;
        ensure!(config.max_target_positions >= 16, "Whisper 上下文长度过小");
        let special = SpecialTokens::load(&tokenizer, config.vocab_size)?;

        let weights = unsafe {
            VarBuilder::from_mmaped_safetensors(&[&files.weights], w::DTYPE, &Device::Cpu)
        }
        .context("映射 Whisper 模型权重失败")?;
        let model = Whisper::load(&weights, config).context("加载 Candle Whisper 模型失败")?;
        Ok(Self {
            model,
            tokenizer,
            filters,
            multilingual: files.multilingual,
            special,
        })
    }

    fn transcribe(&mut self, pcm: &[f32], language: &str) -> Result<String> {
        self.model.reset_kv_cache();
        let bins = self.model.config.num_mel_bins;
        let mel = w::audio::pcm_to_mel(&self.model.config, pcm, &self.filters);
        let frames = mel.len() / bins;
        let mel = Tensor::from_vec(mel, (1, bins, frames), &Device::Cpu)?;
        let first = mel.narrow(2, 0, frames.min(w::N_FRAMES))?;
        let language = self.language_token(&first, language)?;
        let content_frames = pcm.len().div_ceil(w::HOP_LENGTH);
        let mut history = Vec::new();
        let mut text = String::new();
        for seek in (0..content_frames).step_by(w::N_FRAMES) {
            let window = mel.narrow(2, seek, (frames - seek).min(w::N_FRAMES))?;
            let features = self.model.encoder.forward(&window, true)?;
            let decoded = self.decode_window(&features, language, &history)?;
            if decoded.no_speech > w::NO_SPEECH_THRESHOLD
                && decoded.average_logprob < w::LOGPROB_THRESHOLD
            {
                continue;
            }
            text.push_str(&decoded.text);
            if decoded.temperature > 0.5 {
                history.clear();
            } else {
                history.extend(decoded.tokens.into_iter().filter(|t| *t < self.special.eot));
                let max_history = self.model.config.max_target_positions / 2 - 1;
                if history.len() > max_history {
                    history.drain(..history.len() - max_history);
                }
            }
            let processed = ((seek + w::N_FRAMES) * w::HOP_LENGTH).min(pcm.len());
            log::debug!(
                "Whisper 已处理至 {:.1}s",
                processed as f64 / w::SAMPLE_RATE as f64
            );
        }
        self.model.reset_kv_cache();
        Ok(text.trim().to_owned())
    }

    fn language_token(&mut self, mel: &Tensor, requested: &str) -> Result<Option<u32>> {
        let requested = requested.trim();
        if !self.multilingual {
            ensure!(
                requested.is_empty()
                    || requested.eq_ignore_ascii_case("auto")
                    || requested.eq_ignore_ascii_case("en"),
                "英语专用 Whisper 模型不支持语言 {requested}"
            );
            return Ok(None);
        }
        if !requested.is_empty() && !requested.eq_ignore_ascii_case("auto") {
            let language = requested.to_ascii_lowercase();
            return self
                .special
                .languages
                .iter()
                .find(|(code, _)| code == &language)
                .map(|(_, id)| Some(*id))
                .with_context(|| format!("模型不支持 Whisper 语言: {requested}"));
        }
        let features = self.model.encoder.forward(mel, true)?;
        let input = Tensor::new(&[[self.special.sot]], &Device::Cpu)?;
        let output = self.model.decoder.forward(&input, &features, true)?;
        let logits = self.model.decoder.final_linear(&output)?.i((0, 0))?;
        let logits = logits.to_vec1::<f32>()?;
        let (code, token) = self
            .special
            .languages
            .iter()
            .filter(|(_, id)| (*id as usize) < logits.len())
            .max_by(|(_, a), (_, b)| logits[*a as usize].total_cmp(&logits[*b as usize]))
            .context("tokenizer 中没有可用的语言 token")?;
        log::info!("Whisper 检测语种: {code}");
        Ok(Some(*token))
    }

    fn decode_window(
        &mut self,
        features: &Tensor,
        language: Option<u32>,
        history: &[u32],
    ) -> Result<Decoded> {
        for (index, temperature) in w::TEMPERATURES.into_iter().enumerate() {
            let decoded = self.decode(features, language, history, temperature)?;
            if index == w::TEMPERATURES.len() - 1
                || decoded.no_speech > w::NO_SPEECH_THRESHOLD
                || (decoded.average_logprob >= w::LOGPROB_THRESHOLD
                    && decoded.compression_ratio <= w::COMPRESSION_RATIO_THRESHOLD)
            {
                return Ok(decoded);
            }
        }
        bail!("Whisper 解码未产生结果")
    }

    fn decode(
        &mut self,
        features: &Tensor,
        language: Option<u32>,
        history: &[u32],
        temperature: f64,
    ) -> Result<Decoded> {
        let mut tokens = Vec::new();
        if temperature <= 0.5 && !history.is_empty() {
            tokens.push(self.special.previous);
            tokens.extend_from_slice(history);
        }
        let sot_index = tokens.len();
        tokens.push(self.special.sot);
        if let Some(language) = language {
            tokens.push(language);
        }
        tokens.extend([self.special.transcribe, self.special.no_timestamps]);
        let sample_begin = tokens.len();
        let sample_limit = (self.model.config.max_target_positions / 2)
            .min(self.model.config.max_target_positions - sample_begin);
        let mut sum_logprob = 0.0;
        let mut no_speech = 0.0;
        for step in 0..sample_limit {
            let input = Tensor::new(tokens.as_slice(), &Device::Cpu)?.unsqueeze(0)?;
            let output = self.model.decoder.forward(&input, features, step == 0)?;
            if step == 0 {
                let logits = self
                    .model
                    .decoder
                    .final_linear(&output.narrow(1, sot_index, 1)?)?
                    .i((0, 0))?
                    .to_vec1::<f32>()?;
                no_speech = token_logprob(&logits, self.special.no_speech as usize)?.exp();
            }
            let last = output.dim(1)? - 1;
            let mut logits = self
                .model
                .decoder
                .final_linear(&output.narrow(1, last, 1)?)?
                .i((0, 0))?
                .to_vec1::<f32>()?;
            for &id in &self.model.config.suppress_tokens {
                if let Some(value) = logits.get_mut(id as usize) {
                    *value = f32::NEG_INFINITY;
                }
            }

            for value in &mut logits[self.special.sot as usize..] {
                *value = f32::NEG_INFINITY;
            }
            if step == 0 {
                logits[self.special.eot as usize] = f32::NEG_INFINITY;
                for &id in &self.special.blank {
                    logits[id as usize] = f32::NEG_INFINITY;
                }
            }
            let token = sample_token(&logits, temperature)?;
            sum_logprob += token_logprob(&logits, token as usize)?;
            tokens.push(token);
            if token == self.special.eot {
                break;
            }
        }
        let generated = tokens[sample_begin..].to_vec();
        let text = self
            .tokenizer
            .decode(&generated, true)
            .map_err(|e| anyhow!("解码 Whisper 文本失败: {e}"))?;
        let mut compressed = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        compressed.write_all(text.as_bytes())?;
        let compressed = compressed.finish()?;
        let compression_ratio = text.len() as f64 / compressed.len().max(1) as f64;
        Ok(Decoded {
            average_logprob: sum_logprob / generated.len().max(1) as f64,
            tokens: generated,
            text,
            no_speech,
            compression_ratio,
            temperature,
        })
    }
}

impl SpecialTokens {
    fn load(tokenizer: &Tokenizer, vocab_size: usize) -> Result<Self> {
        let token = |name: &str| {
            tokenizer
                .token_to_id(name)
                .filter(|id| (*id as usize) < vocab_size)
                .with_context(|| format!("Whisper tokenizer 缺少 {name}"))
        };
        let sot = token(w::SOT_TOKEN)?;
        let translate = token(w::TRANSLATE_TOKEN)?;
        let mut languages: Vec<_> = tokenizer
            .get_vocab(true)
            .into_iter()
            .filter_map(|(text, id)| {
                let code = text.strip_prefix("<|")?.strip_suffix("|>")?;
                (id > sot
                    && id < translate
                    && (id as usize) < vocab_size
                    && (2..=3).contains(&code.len())
                    && code.bytes().all(|c| c.is_ascii_lowercase()))
                .then(|| (code.to_owned(), id))
            })
            .collect();
        languages.sort_by_key(|(_, id)| *id);
        Ok(Self {
            sot,
            eot: token(w::EOT_TOKEN)?,
            previous: token("<|startofprev|>")?,
            transcribe: token(w::TRANSCRIBE_TOKEN)?,
            no_timestamps: token(w::NO_TIMESTAMPS_TOKEN)?,
            no_speech: w::NO_SPEECH_TOKENS
                .iter()
                .find_map(|name| tokenizer.token_to_id(name))
                .filter(|id| (*id as usize) < vocab_size)
                .context("Whisper tokenizer 缺少无语音 token")?,
            blank: tokenizer
                .encode(" ", false)
                .map_err(|e| anyhow!("编码 Whisper 空白 token 失败: {e}"))?
                .get_ids()
                .to_vec(),
            languages,
        })
    }
}

fn token_logprob(logits: &[f32], token: usize) -> Result<f64> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max) as f64;
    ensure!(max.is_finite(), "Whisper 没有可选 token");
    let sum: f64 = logits.iter().map(|value| (*value as f64 - max).exp()).sum();
    let value = *logits.get(token).context("Whisper token 超出词表")? as f64;
    Ok(value - max - sum.ln())
}

fn sample_token(logits: &[f32], temperature: f64) -> Result<u32> {
    let (best, &max) = logits
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .context("Whisper 输出词表为空")?;
    ensure!(max.is_finite(), "Whisper 没有可选 token");
    if temperature == 0.0 {
        return Ok(best as u32);
    }
    let weights: Vec<_> = logits
        .iter()
        .map(|value| ((*value as f64 - max as f64) / temperature).exp())
        .collect();
    let distribution = WeightedIndex::new(&weights).context("构造 Whisper 采样分布失败")?;
    Ok(distribution.sample(&mut rand::rng()) as u32)
}

fn mel_filters(bins: usize) -> Result<Vec<f32>> {
    let bytes: &[u8] = match bins {
        80 => include_bytes!("assets/melfilters.bytes"),
        128 => include_bytes!("assets/melfilters128.bytes"),
        _ => bail!("不支持 Whisper mel 维度: {bins}"),
    };
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect())
}
