//! 题型策略仅依赖领域内容、LLM 配置及已准备的材料。
mod answers;
mod context;
mod prompts;

use crate::config::LlmConfig;
use crate::llm;
use crate::model::{Module, ReplyType};
use crate::transcribe::Transcriber;
use anyhow::Result;

const SYSTEM_PROMPT: &str = "你是一个专业的英语教学助手，擅长分析英语题目。\
请根据题目要求给出准确答案，注意区分不同题型：\
词汇匹配题根据英文选中文或根据中文选英文；选词填空选择最合适的单词；阅读理解基于文章内容作答。";

const DISCUSSION_FALLBACK: &str = "In my opinion, this is an interesting and meaningful topic. Based on my own experience, \
     I believe we can learn a lot from it. It has taught me to think more carefully and to \
     appreciate different perspectives. I would like to share more thoughts with my classmates.";

pub(crate) struct Solver {
    http: reqwest::Client,
    config: LlmConfig,
    transcriber: Transcriber,
}

impl Solver {
    pub(crate) fn new(http: reqwest::Client, config: LlmConfig, transcriber: Transcriber) -> Self {
        Self {
            http,
            config,
            transcriber,
        }
    }

    async fn answer<T>(
        &self,
        prompt: impl FnOnce() -> String,
        parse: impl FnOnce(&str) -> T,
        fallback: impl FnOnce() -> T,
        fallback_message: &str,
    ) -> Result<T> {
        if !self.config.is_enabled() {
            return Ok(fallback());
        }
        let prompt = prompt();
        match llm::ask(&self.http, &self.config, SYSTEM_PROMPT, &prompt).await {
            Ok(answer) => Ok(parse(&answer)),
            Err(error) if self.config.fallback_on_llm_failure => {
                log::warn!("{fallback_message}: {error:#}");
                Ok(fallback())
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn solve(&self, module: &Module) -> Result<Vec<String>> {
        use answers::*;
        if module.children.is_empty() {
            return Ok(Vec::new());
        }
        let reply_type = module.reply_type();
        let context = context::prepare(
            &self.transcriber,
            module,
            &reply_type,
            self.config.is_enabled(),
        )
        .await;
        let context = context.as_deref();
        let count = module.children.len();
        match reply_type {
            ReplyType::SingleChoice | ReplyType::MultiChoice | ReplyType::Unknown(_) => {
                let mut values = Vec::with_capacity(count);
                for child in &module.children {
                    let multi = child.reply_type == "multichoice";
                    values.push(
                        self.answer(
                            || prompts::choice(context, child, multi),
                            |answer| {
                                if multi {
                                    parse_multi(answer, &child.options)
                                } else {
                                    parse_single(answer, &child.options)
                                }
                            },
                            || random_choice(child),
                            "LLM 失败，随机作答",
                        )
                        .await?,
                    );
                }
                Ok(values)
            }
            ReplyType::FillBlank | ReplyType::TextArea => {
                self.answer(
                    || prompts::batch(context, module),
                    |answer| parse_banked(answer, count),
                    || vec!["answer".to_string(); count],
                    "LLM 失败，填空兜底",
                )
                .await
            }
            ReplyType::BankedCloze => {
                let words = word_bank(module);
                if words.is_empty() {
                    return Ok(shuffled_words(&words, count));
                }
                self.answer(
                    || prompts::banked_cloze(context, count, &words),
                    |answer| canonicalize_banked(&parse_banked(answer, count), &words, count),
                    || shuffled_words(&words, count),
                    "LLM 失败，选词填空随机兜底",
                )
                .await
            }
            ReplyType::DropdownSelection => {
                self.answer(
                    || prompts::dropdown(context, module),
                    |answer| canonicalize_dropdown(&parse_dropdown_answers(answer, count), module),
                    || module.children.iter().map(random_choice).collect(),
                    "LLM 失败，下拉选择随机兜底",
                )
                .await
            }
            ReplyType::Discussion => {
                self.answer(
                    || prompts::discussion(context, module),
                    |answer| {
                        let text = clean_discussion_text(answer);
                        vec![
                            if text.is_empty() {
                                DISCUSSION_FALLBACK.to_string()
                            } else {
                                text
                            };
                            count.max(1)
                        ]
                    },
                    || vec![DISCUSSION_FALLBACK.to_string(); count.max(1)],
                    "LLM 失败，讨论题使用兜底发言",
                )
                .await
            }
            ReplyType::Vocabulary => Ok(Vec::new()),
        }
    }
}
