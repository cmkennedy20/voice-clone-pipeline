//! Multipart upload extraction, query conversion, and binary audio `HTTP` response builders.

use std::str::FromStr;

use axum::{
    body::Body,
    extract::Multipart,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use common::pagination::PageParams;
use domain::{
    ports::{DiscordDeliveryTarget, SynthesisTuning},
    translation::types::{TargetLanguage, TtsEngineKind},
};
use features::{
    shared_queries::profile_loader::VoiceJobId,
    voice_jobs::{
        list_voice_jobs::ListVoiceJobsFilter,
        translate_clip::{TranslateClipInput, TranslateClipOutput},
    },
};
use serde::Deserialize;

use crate::error::ApiError;

/// Default fallback profile slug when omitted on `POST /v1/voice-jobs/render`.
pub const DEFAULT_PROFILE_SLUG: &str = "default_voice";

/// Requested binary audio wire container (`audio/ogg` or `audio/wav`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderAudioFormat {
    OggOpus,
    Wav48k,
}

impl RenderAudioFormat {
    /// Parses an optional `format` query string (`ogg` default, or `wav`).
    pub fn parse(raw: Option<&str>) -> Result<Self, ApiError> {
        match raw.map(str::trim).filter(|s| !s.is_empty()) {
            None | Some("ogg") => Ok(Self::OggOpus),
            Some("wav") => Ok(Self::Wav48k),
            Some(other) => Err(ApiError::BadRequest(format!(
                "unsupported format `{other}`: expected `ogg` or `wav`"
            ))),
        }
    }

    /// Returns the `HTTP` `Content-Type` header value for this format.
    #[must_use]
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::OggOpus => "audio/ogg",
            Self::Wav48k => "audio/wav",
        }
    }

    /// Returns `true` when `48 kHz` `WAV` bytes were requested.
    #[must_use]
    pub const fn is_wav(self) -> bool {
        matches!(self, Self::Wav48k)
    }
}

/// Optional query parameters for `GET /v1/voice-jobs`.
#[derive(Debug, Default, Deserialize)]
pub struct ListJobsQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub target_language: Option<String>,
    pub profile_slug: Option<String>,
}

/// Optional query parameters for `GET /v1/voice-jobs/{id}/audio`.
#[derive(Debug, Default, Deserialize)]
pub struct AudioReplayQuery {
    pub format: Option<String>,
}

/// Optional query parameters for `POST /v1/voice-jobs/render`.
#[derive(Debug, Default, Deserialize)]
pub struct RenderQuery {
    pub profile: Option<String>,
    pub lang: Option<TargetLanguage>,
    pub format: Option<String>,
    pub engine: Option<TtsEngineKind>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub repetition_penalty: Option<f32>,
    pub speed: Option<f32>,
    pub gpt_cond_len_secs: Option<u16>,
    pub sound_norm_refs: Option<bool>,
}

impl RenderQuery {
    /// Builds a [`SynthesisTuning`] override when any synthesis query parameter is present,
    /// or `None` when omitted so the enrolled profile's saved tuning applies.
    #[must_use]
    pub fn tuning_override(&self) -> Option<SynthesisTuning> {
        let has_override = self.temperature.is_some()
            || self.top_p.is_some()
            || self.repetition_penalty.is_some()
            || self.speed.is_some()
            || self.gpt_cond_len_secs.is_some()
            || self.sound_norm_refs.is_some();
        if !has_override {
            return None;
        }

        let mut tuning = SynthesisTuning::default();
        if let Some(temperature) = self.temperature {
            tuning.temperature = temperature;
        }
        if let Some(top_p) = self.top_p {
            tuning.top_p = top_p;
        }
        if let Some(repetition_penalty) = self.repetition_penalty {
            tuning.repetition_penalty = repetition_penalty;
        }
        if let Some(speed) = self.speed {
            tuning.speed = speed;
        }
        if let Some(cond_secs) = self.gpt_cond_len_secs {
            tuning.gpt_cond_len_secs = cond_secs;
            tuning.gpt_cond_chunk_len_secs = tuning.gpt_cond_chunk_len_secs.min(cond_secs);
        }
        if let Some(sound_norm_refs) = self.sound_norm_refs {
            tuning.sound_norm_refs = sound_norm_refs;
        }
        Some(tuning)
    }
}

#[derive(Default)]
struct ExtractedRenderForm {
    profile: Option<String>,
    lang: Option<TargetLanguage>,
    source_text: Option<String>,
    audio_bytes: Option<Vec<u8>>,
}

/// Parses a `UUIDv7` [`VoiceJobId`] path parameter or returns `400 Bad Request`.
pub fn parse_voice_job_id(raw_id: &str) -> Result<VoiceJobId, ApiError> {
    raw_id
        .parse::<VoiceJobId>()
        .map_err(|err| ApiError::BadRequest(format!("invalid voice job id `{raw_id}`: {err}")))
}

/// Converts [`ListJobsQuery`] into a validated [`ListVoiceJobsFilter`].
pub fn build_list_jobs_filter(q: ListJobsQuery) -> Result<ListVoiceJobsFilter, ApiError> {
    let target_language = q
        .target_language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(TargetLanguage::from_str)
        .transpose()
        .map_err(|err| ApiError::BadRequest(err.to_string()))?;

    let page = PageParams::new(
        q.limit.unwrap_or(PageParams::DEFAULT_LIMIT),
        q.offset.unwrap_or(0),
    );
    Ok(ListVoiceJobsFilter {
        target_language,
        profile_slug: q.profile_slug,
        page,
    })
}

async fn extract_render_multipart(
    multipart: &mut Multipart,
) -> Result<ExtractedRenderForm, ApiError> {
    let mut form = ExtractedRenderForm::default();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::BadRequest(format!("invalid multipart upload: {err}")))?
    {
        let name = field.name().map(str::to_owned);
        let is_file = field.file_name().is_some();

        match name.as_deref() {
            Some("profile") if !is_file => {
                let text = field
                    .text()
                    .await
                    .map_err(|err| ApiError::BadRequest(format!("invalid profile field: {err}")))?;
                if !text.trim().is_empty() {
                    form.profile = Some(text.trim().to_owned());
                }
            }
            Some("lang" | "target_language") if !is_file => {
                let text = field
                    .text()
                    .await
                    .map_err(|err| ApiError::BadRequest(format!("invalid lang field: {err}")))?;
                if !text.trim().is_empty() {
                    form.lang = Some(
                        TargetLanguage::from_str(text.trim())
                            .map_err(|err| ApiError::BadRequest(err.to_string()))?,
                    );
                }
            }
            Some("source_text" | "text") if !is_file => {
                let text = field.text().await.map_err(|err| {
                    ApiError::BadRequest(format!("invalid source_text field: {err}"))
                })?;
                if !text.trim().is_empty() {
                    form.source_text = Some(text.trim().to_owned());
                }
            }
            Some("audio" | "file") if form.audio_bytes.is_none() => {
                let bytes = field.bytes().await.map_err(|err| {
                    ApiError::BadRequest(format!("failed reading audio bytes: {err}"))
                })?;
                form.audio_bytes = Some(bytes.to_vec());
            }
            _ if is_file && form.audio_bytes.is_none() => {
                let bytes = field.bytes().await.map_err(|err| {
                    ApiError::BadRequest(format!("failed reading audio bytes: {err}"))
                })?;
                form.audio_bytes = Some(bytes.to_vec());
            }
            _ => {}
        }
    }

    Ok(form)
}

/// Extracts `multipart/form-data` + query overrides into a [`RenderAudioFormat`] and [`TranslateClipInput`].
pub async fn build_render_input_from_multipart(
    q: RenderQuery,
    multipart: &mut Multipart,
) -> Result<(RenderAudioFormat, TranslateClipInput), ApiError> {
    let format = RenderAudioFormat::parse(q.format.as_deref())?;
    let form = extract_render_multipart(multipart).await?;

    let raw_audio = form.audio_bytes.unwrap_or_default();
    if raw_audio.is_empty() && form.source_text.is_none() {
        return Err(ApiError::BadRequest(
            "provide a non-empty audio file part or source_text field".to_owned(),
        ));
    }

    let tuning_override = q.tuning_override();
    let profile_slug = q
        .profile
        .filter(|s| !s.trim().is_empty())
        .or(form.profile)
        .unwrap_or_else(|| DEFAULT_PROFILE_SLUG.to_owned());

    let input = TranslateClipInput {
        profile_slug,
        target_language: q.lang.or(form.lang),
        engine_override: q.engine,
        max_cer_bps_override: None,
        tuning_override,
        audio_bytes: raw_audio,
        source_text: form.source_text,
        delivery: DiscordDeliveryTarget::ClientReturnOnly,
    };
    Ok((format, input))
}

/// Builds the binary audio `200 OK` [`Response`] with `x-job-id`, `x-target-language`,
/// `x-engine-used`, and `x-cer-bps` headers.
pub fn build_rendered_audio_response(
    format: RenderAudioFormat,
    out: TranslateClipOutput,
) -> Result<Response, ApiError> {
    let payload = match format {
        RenderAudioFormat::OggOpus => out.ogg_opus_bytes,
        RenderAudioFormat::Wav48k => out.wav_48k_bytes,
    };

    let cer_header = HeaderValue::from_str(&out.receipt.cer_bps.value().to_string())
        .unwrap_or_else(|_| HeaderValue::from_static("0"));
    let job_id_header = HeaderValue::from_str(&out.receipt.job_id.to_string())
        .unwrap_or_else(|_| HeaderValue::from_static("unknown"));
    let lang_header = HeaderValue::from_static(out.receipt.target_language.code());
    let engine_header = HeaderValue::from_static(out.receipt.engine_used.as_str());

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, format.content_type())
        .header("x-job-id", job_id_header)
        .header("x-target-language", lang_header)
        .header("x-engine-used", engine_header)
        .header("x-cer-bps", cer_header)
        .body(Body::from(payload))
        .map_err(|err| ApiError::Internal(format!("response build error: {err}")))
}

/// Builds a binary audio `200 OK` replay [`Response`] for `GET /v1/voice-jobs/{id}/audio`.
pub fn build_replay_audio_response(
    format: RenderAudioFormat,
    audio_bytes: Vec<u8>,
) -> Result<Response, ApiError> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, format.content_type())
        .body(Body::from(audio_bytes))
        .map_err(|err| ApiError::Internal(format!("response build error: {err}")))
}
