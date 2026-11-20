//! `WorkerError`, `WorkerState`, and `HTTP` relay client helpers targeting `api-server`.

use std::{fmt, time::Duration};

use common::{pagination::Paged, secret::Secret};
use domain::{
    ports::DiscordDeliveryTarget,
    translation::{
        error::TranslationDomainError, expand_english_numerals_for_tts, types::TargetLanguage,
    },
};
use features::{
    profiles::VoiceProfileRecord,
    voice_jobs::translate_clip::{TranslateClipInput, VoiceJobReceipt},
};
use framework::config::{AppConfig, ConfigError};
use reqwest::{StatusCode, header::AUTHORIZATION};
use serenity::all::InteractionContext;

use super::DEFAULT_PROFILE_SLUG;

/// Typed error enum for the CPU-only `Discord` gateway & `HTTP` relay worker.
#[derive(Debug)]
pub enum WorkerError {
    Config(ConfigError),
    Http(reqwest::Error),
    Discord(Box<serenity::Error>),
    Translation(TranslationDomainError),
    Relay(String),
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(f, "configuration error: {err}"),
            Self::Http(err) => write!(f, "HTTP relay error: {err}"),
            Self::Discord(err) => write!(f, "Discord gateway/REST error: {err}"),
            Self::Translation(err) => write!(f, "invalid language selection: {err}"),
            Self::Relay(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for WorkerError {}

impl From<ConfigError> for WorkerError {
    fn from(err: ConfigError) -> Self {
        Self::Config(err)
    }
}

impl From<reqwest::Error> for WorkerError {
    fn from(err: reqwest::Error) -> Self {
        Self::Http(err)
    }
}

impl From<serenity::Error> for WorkerError {
    fn from(err: serenity::Error) -> Self {
        Self::Discord(Box::new(err))
    }
}

impl From<TranslationDomainError> for WorkerError {
    fn from(err: TranslationDomainError) -> Self {
        Self::Translation(err)
    }
}

/// Shared CPU-only relay state stored inside `poise::Framework`.
#[derive(Debug, Clone)]
pub struct WorkerState {
    pub api_base_url: String,
    pub bearer_token: Secret<String>,
    pub application_id: u64,
    pub owner_user_id: u64,
    pub http: reqwest::Client,
}

impl WorkerState {
    pub fn from_config(config: &AppConfig) -> Result<Self, WorkerError> {
        let api_base_url = config
            .api_server_internal_url
            .trim()
            .trim_end_matches('/')
            .to_owned();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .build()?;
        Ok(Self {
            api_base_url,
            bearer_token: config.owner_api_bearer_token.clone(),
            application_id: config.discord_application_id,
            owner_user_id: config.discord_owner_user_id,
            http,
        })
    }
}

/// Parameters for constructing a `POST /api/v1/voice-jobs` relay payload.
pub struct RelayPayloadSpec<'a> {
    pub profile: Option<&'a str>,
    pub target_language: Option<TargetLanguage>,
    pub audio_bytes: Vec<u8>,
    pub source_text: Option<String>,
    pub delivery: DiscordDeliveryTarget,
}

/// Builds the `JSON` payload forwarded to `POST /api/v1/voice-jobs`.
#[must_use]
pub fn build_relay_input(spec: RelayPayloadSpec<'_>) -> TranslateClipInput {
    let profile_slug = spec
        .profile
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_PROFILE_SLUG)
        .to_owned();
    let normalized_text = spec.source_text.map(|raw| {
        if raw
            .chars()
            .any(|c| c.is_ascii_digit() || ('\u{FF10}'..='\u{FF19}').contains(&c))
        {
            expand_english_numerals_for_tts(&raw)
        } else {
            raw
        }
    });
    TranslateClipInput {
        profile_slug,
        target_language: spec.target_language,
        engine_override: None,
        max_cer_bps_override: None,
        tuning_override: None,
        audio_bytes: spec.audio_bytes,
        source_text: normalized_text,
        delivery: spec.delivery,
    }
}

/// Selects `BotVoiceBubble` when explicitly requested in a bot-accessible context, or
/// `UserInstallFollowup` everywhere else (friend `PrivateChannel` `DM`s, `Group` `DM`s, ephemeral).
#[must_use]
pub fn resolve_slash_delivery(
    interaction_context: Option<InteractionContext>,
    voice_bubble: bool,
    private_only: bool,
    channel_id: u64,
    application_id: u64,
    interaction_token: String,
    attach_spoiler_pinyin: bool,
) -> DiscordDeliveryTarget {
    if voice_bubble
        && !private_only
        && interaction_context != Some(InteractionContext::PrivateChannel)
    {
        DiscordDeliveryTarget::BotVoiceBubble {
            channel_id,
            reply_to_message_id: None,
        }
    } else {
        DiscordDeliveryTarget::UserInstallFollowup {
            application_id,
            interaction_token: Secret::new(interaction_token),
            attach_spoiler_pinyin,
        }
    }
}

pub(crate) async fn download_attachment_bytes(
    http: &reqwest::Client,
    url: &str,
) -> Result<Vec<u8>, WorkerError> {
    let response = http.get(url).send().await?;
    if !response.status().is_success() {
        return Err(WorkerError::Relay(format!(
            "attachment download failed with HTTP {}",
            response.status()
        )));
    }
    Ok(response.bytes().await?.to_vec())
}

async fn post_voice_job_once(
    state: &WorkerState,
    caller_user_id: u64,
    input: &TranslateClipInput,
) -> Result<reqwest::Response, WorkerError> {
    Ok(state
        .http
        .post(format!("{}/api/v1/voice-jobs", state.api_base_url))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", state.bearer_token.expose_secret()),
        )
        .header("x-discord-user-id", caller_user_id.to_string())
        .json(input)
        .send()
        .await?)
}

async fn lookup_fallback_profile_slug(state: &WorkerState, caller_user_id: u64) -> Option<String> {
    let resp = state
        .http
        .get(format!(
            "{}/api/v1/profiles?limit=1&offset=0",
            state.api_base_url
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", state.bearer_token.expose_secret()),
        )
        .header("x-discord-user-id", caller_user_id.to_string())
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let paged: Paged<VoiceProfileRecord> = resp.json().await.ok()?;
    paged.items.into_iter().next().map(|p| p.slug)
}

pub(crate) async fn relay_voice_job(
    state: &WorkerState,
    caller_user_id: u64,
    input: &TranslateClipInput,
) -> Result<VoiceJobReceipt, WorkerError> {
    let mut active_input = input.clone();
    let mut response = post_voice_job_once(state, caller_user_id, &active_input).await?;

    if response.status() == StatusCode::NOT_FOUND
        && active_input.profile_slug == DEFAULT_PROFILE_SLUG
        && let Some(fallback_slug) = lookup_fallback_profile_slug(state, caller_user_id).await
        && fallback_slug != DEFAULT_PROFILE_SLUG
    {
        active_input.profile_slug = fallback_slug;
        response = post_voice_job_once(state, caller_user_id, &active_input).await?;
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status == StatusCode::INTERNAL_SERVER_ERROR
            && body.contains("spoken script hygiene")
            && let Some(src) = active_input.source_text.take()
        {
            let mut retry_in = active_input;
            retry_in.source_text = Some(format!(
                "{} (spell out every number in spoken words with zero digits)",
                expand_english_numerals_for_tts(&src)
            ));
            let retry_resp = post_voice_job_once(state, caller_user_id, &retry_in).await?;
            if retry_resp.status().is_success() {
                return Ok(retry_resp.json().await?);
            }
        }
        return Err(WorkerError::Relay(format!(
            "api-server returned HTTP {status}: {body}"
        )));
    }
    Ok(response.json().await?)
}
