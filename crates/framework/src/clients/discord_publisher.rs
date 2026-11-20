//! Multi-mode `Discord` `REST` publisher (`USER_INSTALL` followup, profile webhook, `flags: 8192`).

use std::{path::PathBuf, time::Duration};

use common::secret::Secret;
use domain::{
    audio::types::RenderedAudioBundle,
    ports::{DiscordDeliveryTarget, DiscordPublisher, ExternalPortError, TranslatedMandarin},
};
use reqwest::{
    header::{AUTHORIZATION, CONTENT_TYPE},
    multipart::{Form, Part},
};
use serde::Deserialize;
use serde_json::json;

const DEFAULT_DISCORD_BASE_URL: &str = "https://discord.com";
const VOICE_MESSAGE_FILENAME: &str = "voice-message.ogg";
const VOICE_MESSAGE_FLAG: u64 = 8_192;

#[derive(Debug, Deserialize)]
struct DiscordMessageCreated {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AttachmentSlotResponse {
    attachments: Vec<AttachmentSlot>,
}

#[derive(Debug, Deserialize)]
struct AttachmentSlot {
    upload_url: String,
    upload_filename: String,
}

fn append_wait_true_query(raw_url: &str) -> String {
    if raw_url.contains("wait=true") {
        raw_url.to_owned()
    } else if raw_url.contains('?') {
        format!("{raw_url}&wait=true")
    } else {
        format!("{raw_url}?wait=true")
    }
}

fn parse_snowflake_id(raw_id: &str) -> Result<u64, ExternalPortError> {
    raw_id.trim().parse::<u64>().map_err(|err| {
        ExternalPortError::Discord(format!("invalid Discord snowflake id '{raw_id}': {err}"))
    })
}

fn build_ogg_file_part(ogg_bytes: Vec<u8>) -> Result<Part, ExternalPortError> {
    Part::bytes(ogg_bytes)
        .file_name(VOICE_MESSAGE_FILENAME)
        .mime_str("audio/ogg")
        .map_err(|err| ExternalPortError::Discord(format!("failed building audio/ogg part: {err}")))
}

/// Concrete `Discord` `REST` v10 publisher supporting compliant user-install, webhook, and bot
/// `IS_VOICE_MESSAGE` (`flags: 8192`) waveform delivery.
#[derive(Debug, Clone)]
pub struct HttpDiscordPublisher {
    base_url: String,
    bot_token: Secret<String>,
    studio_env_path: Option<PathBuf>,
    http: reqwest::Client,
}

impl HttpDiscordPublisher {
    /// Creates a publisher targeting `"https://discord.com"` with the given `Bot` token.
    #[must_use]
    pub fn new(bot_token: Secret<String>) -> Self {
        Self::with_base_url(DEFAULT_DISCORD_BASE_URL.to_owned(), bot_token)
    }

    /// Attaches an optional `studio.env` override path so live Web UI pairing updates the `Bot` token immediately.
    #[must_use]
    pub fn with_studio_env_path(mut self, path: Option<PathBuf>) -> Self {
        self.studio_env_path = path;
        self
    }

    /// Creates a publisher with a custom `base_url` (used by `Wiremock` contract suites).
    #[must_use]
    pub fn with_base_url(mut base_url: String, bot_token: Secret<String>) -> Self {
        while base_url.ends_with('/') {
            base_url.pop();
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url,
            bot_token,
            studio_env_path: None,
            http,
        }
    }

    fn bot_auth_header(&self) -> String {
        if let Some(path) = self.studio_env_path.as_deref()
            && let Ok(map) = crate::desktop::dotenv::read_dotenv_file(path)
            && let Some(token) = map.get("DISCORD_BOT_TOKEN")
            && !token.trim().is_empty()
        {
            return format!("Bot {}", token.trim());
        }
        format!("Bot {}", self.bot_token.expose_secret())
    }

    async fn post_multipart_message(
        &self,
        url: &str,
        payload_json: &serde_json::Value,
        ogg_bytes: Vec<u8>,
    ) -> Result<u64, ExternalPortError> {
        let form = Form::new()
            .text("payload_json", payload_json.to_string())
            .part("files[0]", build_ogg_file_part(ogg_bytes)?);

        let resp = self
            .http
            .post(url)
            .multipart(form)
            .send()
            .await
            .map_err(|err| {
                ExternalPortError::Discord(format!("Discord multipart POST failed: {err}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ExternalPortError::Discord(format!(
                "Discord multipart POST HTTP {status}: {body}"
            )));
        }

        let created: DiscordMessageCreated = resp.json().await.map_err(|err| {
            ExternalPortError::Discord(format!("invalid Discord message JSON: {err}"))
        })?;
        parse_snowflake_id(&created.id)
    }

    async fn publish_bot_voice_bubble(
        &self,
        channel_id: u64,
        reply_to_message_id: Option<u64>,
        bundle: &RenderedAudioBundle,
    ) -> Result<u64, ExternalPortError> {
        let auth_value = self.bot_auth_header();

        // Step 1: Request pre-signed GCS attachment slot from Discord REST v10.
        let slot_url = format!(
            "{}/api/v10/channels/{channel_id}/attachments",
            self.base_url
        );
        let slot_body = json!({
            "files": [{
                "filename": VOICE_MESSAGE_FILENAME,
                "file_size": bundle.ogg_opus_bytes.len(),
                "id": "0"
            }]
        });

        let slot_resp = self
            .http
            .post(&slot_url)
            .header(AUTHORIZATION, &auth_value)
            .json(&slot_body)
            .send()
            .await
            .map_err(|err| {
                ExternalPortError::Discord(format!("attachment slot request failed: {err}"))
            })?;

        if !slot_resp.status().is_success() {
            let status = slot_resp.status();
            let text = slot_resp.text().await.unwrap_or_default();
            return Err(ExternalPortError::Discord(format!(
                "attachment slot HTTP {status}: {text}"
            )));
        }

        let mut parsed_slots: AttachmentSlotResponse = slot_resp.json().await.map_err(|err| {
            ExternalPortError::Discord(format!("invalid attachment slot JSON: {err}"))
        })?;
        if parsed_slots.attachments.is_empty() {
            return Err(ExternalPortError::Discord(
                "Discord returned empty attachments list".to_owned(),
            ));
        }
        let slot = parsed_slots.attachments.swap_remove(0);

        // Step 2: Upload raw Ogg/Opus bytes to GCS upload_url with ZERO Authorization header.
        let put_resp = self
            .http
            .put(&slot.upload_url)
            .header(CONTENT_TYPE, "audio/ogg")
            .body(bundle.ogg_opus_bytes.clone())
            .send()
            .await
            .map_err(|err| ExternalPortError::Discord(format!("GCS upload PUT failed: {err}")))?;

        if !put_resp.status().is_success() {
            let status = put_resp.status();
            let text = put_resp.text().await.unwrap_or_default();
            return Err(ExternalPortError::Discord(format!(
                "GCS upload PUT HTTP {status}: {text}"
            )));
        }

        // Step 3: Finalize channel message with flags=8192 and strictly NO content/embeds fields.
        let msg_url = format!("{}/api/v10/channels/{channel_id}/messages", self.base_url);
        let duration_secs = f64::from(bundle.duration_ms) / 1_000.0_f64;
        let mut msg_payload = json!({
            "flags": VOICE_MESSAGE_FLAG,
            "attachments": [{
                "id": "0",
                "filename": VOICE_MESSAGE_FILENAME,
                "uploaded_filename": slot.upload_filename,
                "duration_secs": duration_secs,
                "waveform": bundle.waveform.as_str()
            }]
        });

        if let Some(reply_id) = reply_to_message_id
            && let Some(map) = msg_payload.as_object_mut()
        {
            map.insert(
                "message_reference".to_owned(),
                json!({ "message_id": reply_id.to_string() }),
            );
        }

        let msg_resp = self
            .http
            .post(&msg_url)
            .header(AUTHORIZATION, &auth_value)
            .json(&msg_payload)
            .send()
            .await
            .map_err(|err| {
                ExternalPortError::Discord(format!("voice message POST failed: {err}"))
            })?;

        if !msg_resp.status().is_success() {
            let status = msg_resp.status();
            let text = msg_resp.text().await.unwrap_or_default();
            return Err(ExternalPortError::Discord(format!(
                "voice message POST HTTP {status}: {text}"
            )));
        }

        let created: DiscordMessageCreated = msg_resp.json().await.map_err(|err| {
            ExternalPortError::Discord(format!("invalid voice message response JSON: {err}"))
        })?;
        parse_snowflake_id(&created.id)
    }
}

#[async_trait::async_trait]
impl DiscordPublisher for HttpDiscordPublisher {
    async fn publish(
        &self,
        target: &DiscordDeliveryTarget,
        bundle: &RenderedAudioBundle,
        translated: &TranslatedMandarin,
    ) -> Result<Option<u64>, ExternalPortError> {
        match target {
            DiscordDeliveryTarget::ClientReturnOnly => Ok(None),

            DiscordDeliveryTarget::UserInstallFollowup {
                application_id,
                interaction_token,
                attach_spoiler_pinyin,
            } => {
                let url = format!(
                    "{}/api/v10/webhooks/{application_id}/{}?wait=true",
                    self.base_url,
                    interaction_token.expose_secret()
                );
                let payload_json = if *attach_spoiler_pinyin {
                    json!({ "content": translated.spoken_text() })
                } else {
                    json!({})
                };
                let msg_id = self
                    .post_multipart_message(&url, &payload_json, bundle.ogg_opus_bytes.clone())
                    .await?;
                Ok(Some(msg_id))
            }

            DiscordDeliveryTarget::ProfileWebhook {
                webhook_url,
                username,
                avatar_url,
            } => {
                let url = append_wait_true_query(webhook_url.expose_secret());
                let mut payload = serde_json::Map::new();
                payload.insert("username".to_owned(), json!(username));
                if let Some(avatar) = avatar_url {
                    payload.insert("avatar_url".to_owned(), json!(avatar));
                }
                let payload_value = serde_json::Value::Object(payload);
                let msg_id = self
                    .post_multipart_message(&url, &payload_value, bundle.ogg_opus_bytes.clone())
                    .await?;
                Ok(Some(msg_id))
            }

            DiscordDeliveryTarget::BotVoiceBubble {
                channel_id,
                reply_to_message_id,
            } => {
                let msg_id = self
                    .publish_bot_voice_bubble(*channel_id, *reply_to_message_id, bundle)
                    .await?;
                Ok(Some(msg_id))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::append_wait_true_query;

    #[test]
    fn appends_wait_true_preserving_existing_query_parameters() {
        assert_eq!(
            append_wait_true_query("https://discord.com/api/webhooks/1/tok"),
            "https://discord.com/api/webhooks/1/tok?wait=true"
        );
        assert_eq!(
            append_wait_true_query("https://discord.com/api/webhooks/1/tok?thread_id=9"),
            "https://discord.com/api/webhooks/1/tok?thread_id=9&wait=true"
        );
        assert_eq!(
            append_wait_true_query("https://discord.com/api/webhooks/1/tok?wait=true"),
            "https://discord.com/api/webhooks/1/tok?wait=true"
        );
    }
}
