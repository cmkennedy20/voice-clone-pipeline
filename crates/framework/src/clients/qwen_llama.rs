//! Structured `JSON` client for local `llama-server` (`Qwen3-8B` `/no_think` `CUDA`).

use std::time::Duration;

use domain::ports::{
    ExternalPortError, MandarinTranslator, SpokenScript, TargetLanguage, TranslatedMandarin,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

const MAX_ATTEMPTS: usize = 3;
const RETRY_BACKOFF_MS: [u64; MAX_ATTEMPTS] = [50, 100, 0];

fn build_system_prompt(target_language: TargetLanguage) -> String {
    if target_language == TargetLanguage::Mandarin {
        return "/no_think\n\
You are a native Mandarin spoken-dialogue translator. Translate the user's English utterance into \
natural conversational Mandarin kouyu (普通话口语) suitable for voice synthesis.\n\
Strict rules:\n\
1. Expand EVERY number, year, clock time, date, currency amount, decimal, and percentage into \
spoken Hanzi words (e.g. '3:30' -> '三点半', '95%' -> '百分之九十五', '$20' -> '二十美元').\n\
2. NEVER output ASCII digits (0-9), fullwidth digits (０-９), or symbols ($, %, €, £, ¥, #, @, +, =).\n\
3. Respond ONLY with a valid JSON object matching:\n\
{\"spoken_text\": \"...\", \"phonetic_guide\": \"...\", \"english_back_gloss\": \"...\"}"
            .to_owned();
    }

    format!(
        "/no_think\n\
You are a native {en_name} ({native_name}) spoken-dialogue translator. Translate the user's \
English utterance into natural conversational spoken {en_name} ({native_name}) suitable for \
voice synthesis.\n\
Strict rules:\n\
1. Expand EVERY number, year, clock time, date, currency amount, decimal, and percentage into \
spoken {en_name} words.\n\
2. NEVER output ASCII digits (0-9), fullwidth digits (０-９), or symbols ($, %, €, £, ¥, #, @, +, =).\n\
3. Provide a concise {phonetic_label} guide in `phonetic_guide`.\n\
4. Respond ONLY with a valid JSON object matching:\n\
{{\"spoken_text\": \"...\", \"phonetic_guide\": \"...\", \"english_back_gloss\": \"...\"}}",
        en_name = target_language.english_name(),
        native_name = target_language.native_name(),
        phonetic_label = target_language.phonetic_label(),
    )
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    temperature: f32,
    response_format: ResponseFormat<'a>,
    messages: [ChatMessage<'a>; 2],
}

#[derive(Debug, Serialize)]
struct ResponseFormat<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct RawTranslationJson {
    #[serde(alias = "spoken_hanzi")]
    spoken_text: String,
    #[serde(alias = "pinyin")]
    phonetic_guide: String,
    english_back_gloss: String,
}

fn is_transient_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT
    )
}

fn strip_think_and_markdown_fences(raw: &str) -> &str {
    let mut current = raw.trim();
    while let Some(think_start) = current.find("<think>") {
        if let Some(rel_end) = current[think_start..].find("</think>") {
            let after_end = think_start + rel_end + "</think>".len();
            if think_start == 0 {
                current = current[after_end..].trim();
                continue;
            }
        }
        break;
    }

    if let Some(after_open) = current.strip_prefix("```") {
        let body = after_open
            .strip_prefix("json")
            .or_else(|| after_open.strip_prefix("JSON"))
            .unwrap_or(after_open)
            .trim_start();
        if let Some(unwrapped) = body.strip_suffix("```") {
            return unwrapped.trim();
        }
    }

    current
}

/// `OpenAI`-compatible `HTTP` client targeting the local `Qwen3-8B` `llama.cpp` server.
#[derive(Debug, Clone)]
pub struct QwenLlamaTranslator {
    base_url: String,
    http: reqwest::Client,
}

impl QwenLlamaTranslator {
    /// Creates a translator client pointing at `base_url` (e.g. `"http://qwen-translator:8081"`).
    #[must_use]
    pub fn new(mut base_url: String) -> Self {
        while base_url.ends_with('/') {
            base_url.pop();
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { base_url, http }
    }

    /// Probes `GET {base_url}/health` on the local `llama-server` container.
    pub async fn check_health(&self) -> Result<(), ExternalPortError> {
        let url = format!("{}/health", self.base_url);
        let resp =
            self.http.get(&url).send().await.map_err(|err| {
                ExternalPortError::Translation(format!("health check failed: {err}"))
            })?;
        if !resp.status().is_success() {
            return Err(ExternalPortError::Translation(format!(
                "translator unhealthy: HTTP {}",
                resp.status()
            )));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl MandarinTranslator for QwenLlamaTranslator {
    async fn translate_to_spoken_language(
        &self,
        english: &str,
        target_language: TargetLanguage,
    ) -> Result<TranslatedMandarin, ExternalPortError> {
        let trimmed_en = english.trim();
        if trimmed_en.is_empty() {
            return Err(ExternalPortError::Translation(
                "cannot translate empty English transcript".to_owned(),
            ));
        }

        let url = format!("{}/v1/chat/completions", self.base_url);
        let system_prompt = build_system_prompt(target_language);
        let user_prompt = format!("/no_think\n{trimmed_en}");
        let req_body = ChatCompletionRequest {
            temperature: 0.2,
            response_format: ResponseFormat {
                kind: "json_object",
            },
            messages: [
                ChatMessage {
                    role: "system",
                    content: &system_prompt,
                },
                ChatMessage {
                    role: "user",
                    content: &user_prompt,
                },
            ],
        };

        let mut last_err = String::from("translator request exhausted retries");

        for (attempt, &backoff_ms) in RETRY_BACKOFF_MS.iter().enumerate() {
            let send_res = self.http.post(&url).json(&req_body).send().await;
            match send_res {
                Ok(resp) if resp.status().is_success() => {
                    let completion: ChatCompletionResponse = resp.json().await.map_err(|err| {
                        ExternalPortError::Translation(format!(
                            "invalid chat completion JSON envelope: {err}"
                        ))
                    })?;
                    let raw_content = completion
                        .choices
                        .first()
                        .map(|choice| choice.message.content.as_str())
                        .ok_or_else(|| {
                            ExternalPortError::Translation(
                                "chat completion returned zero choices".to_owned(),
                            )
                        })?;

                    let cleaned = strip_think_and_markdown_fences(raw_content);
                    let parsed: RawTranslationJson =
                        serde_json::from_str(cleaned).map_err(|err| {
                            ExternalPortError::Translation(format!(
                                "failed parsing LLM JSON payload '{cleaned}': {err}"
                            ))
                        })?;

                    let spoken_hanzi =
                        SpokenScript::parse_for_language(&parsed.spoken_text, target_language)
                            .map_err(|domain_err| {
                                ExternalPortError::Translation(format!(
                                    "LLM violated spoken script hygiene: {domain_err}"
                                ))
                            })?;

                    return Ok(TranslatedMandarin {
                        spoken_hanzi,
                        pinyin: parsed.phonetic_guide.trim().to_owned(),
                        english_back_gloss: parsed.english_back_gloss.trim().to_owned(),
                    });
                }
                Ok(resp) if is_transient_status(resp.status()) && attempt + 1 < MAX_ATTEMPTS => {
                    last_err = format!("transient HTTP {} from translator", resp.status());
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ExternalPortError::Translation(format!(
                        "translator HTTP {status}: {body}"
                    )));
                }
                Err(err)
                    if (err.is_timeout() || err.is_connect()) && attempt + 1 < MAX_ATTEMPTS =>
                {
                    last_err = format!("transient translator transport error: {err}");
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(err) => {
                    return Err(ExternalPortError::Translation(format!(
                        "translator request error: {err}"
                    )));
                }
            }
        }

        Err(ExternalPortError::Translation(last_err))
    }
}

#[cfg(test)]
mod tests {
    use domain::ports::TargetLanguage;

    use super::{build_system_prompt, strip_think_and_markdown_fences};

    #[test]
    fn strips_think_tags_and_json_code_fences() {
        let decorated =
            "<think>\nreasoning...\n</think>\n```json\n{\"spoken_hanzi\":\"你好\"}\n```";
        assert_eq!(
            strip_think_and_markdown_fences(decorated),
            "{\"spoken_hanzi\":\"你好\"}"
        );
    }

    #[test]
    fn builds_no_think_multilingual_prompts_with_native_names() {
        let es_prompt = build_system_prompt(TargetLanguage::Spanish);
        assert!(es_prompt.starts_with("/no_think\n"));
        assert!(es_prompt.contains("Spanish (Español)"));
        assert!(es_prompt.contains("\"spoken_text\""));
    }
}
