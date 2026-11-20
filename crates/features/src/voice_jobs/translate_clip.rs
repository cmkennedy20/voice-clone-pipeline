//! Polyglot audio/text voice cloning, reverse-`ASR` `CER` verification, and `Discord` delivery workflow.

use std::time::Instant;

use common::basis_points::BasisPoints;
use domain::{
    audio::types::Pcm24kMono,
    ports::{
        DiscordDeliveryTarget, ReferenceSampleRef, SynthesisTuning, TranslatedMandarin,
        VoiceSynthesisRequest,
    },
    translation::{
        expand_english_numerals_for_tts,
        types::{TargetLanguage, TtsEngineKind},
        verifier::evaluate_cer_gate_for_language,
    },
};
use framework::context::AppContext;

pub use crate::voice_jobs::job_storage::VoiceJobReceipt;
use crate::{
    error::AppError,
    shared_queries::profile_loader::{
        VoiceJobId, VoiceProfileRecord, deterministic_attempt_seed, discord_id_to_i64,
        ensure_caller_is_owner, load_owned_profile_by_slug, now_utc_micros,
    },
    voice_jobs::{
        job_storage::{persist_voice_job_row, write_dual_audio_files},
        tuning_and_refs::{
            PreparedSource, prepare_input_source, resolve_synthesis_tuning_and_samples,
        },
    },
};

const fn default_delivery_target() -> DiscordDeliveryTarget {
    DiscordDeliveryTarget::ClientReturnOnly
}

/// Input command for translating an English voice clip or text prompt into verified cloned speech.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TranslateClipInput {
    pub profile_slug: String,
    #[serde(default)]
    pub target_language: Option<TargetLanguage>,
    #[serde(default)]
    pub engine_override: Option<TtsEngineKind>,
    #[serde(default)]
    pub max_cer_bps_override: Option<BasisPoints>,
    #[serde(default)]
    pub tuning_override: Option<SynthesisTuning>,
    #[serde(default, with = "crate::shared_queries::bytes_serde")]
    pub audio_bytes: Vec<u8>,
    #[serde(default)]
    pub source_text: Option<String>,
    #[serde(default = "default_delivery_target")]
    pub delivery: DiscordDeliveryTarget,
}

impl Default for TranslateClipInput {
    fn default() -> Self {
        Self {
            profile_slug: String::new(),
            target_language: None,
            engine_override: None,
            max_cer_bps_override: None,
            tuning_override: None,
            audio_bytes: Vec::new(),
            source_text: None,
            delivery: default_delivery_target(),
        }
    }
}

/// Complete workflow result pairing the persisted [`VoiceJobReceipt`] with dual-encoded audio bytes.
#[derive(Debug, Clone)]
pub struct TranslateClipOutput {
    pub receipt: VoiceJobReceipt,
    pub ogg_opus_bytes: Vec<u8>,
    pub wav_48k_bytes: Vec<u8>,
}

struct GpuPipelineParams<'a> {
    profile: &'a VoiceProfileRecord,
    reference_samples: &'a [ReferenceSampleRef],
    source: &'a PreparedSource,
    target_lang: TargetLanguage,
    engine: TtsEngineKind,
    max_cer: BasisPoints,
    tuning: SynthesisTuning,
}

struct VerifiedGpuOutput {
    english_transcript: String,
    translated: TranslatedMandarin,
    pcm_24k: Pcm24kMono,
    back_transcribed_text: String,
    cer_bps: BasisPoints,
    attempts_used: u8,
}

async fn run_gpu_inference_pipeline(
    ctx: &AppContext,
    params: &GpuPipelineParams<'_>,
) -> Result<VerifiedGpuOutput, AppError> {
    let _gpu_permit = ctx
        .gpu_semaphore
        .acquire()
        .await
        .map_err(|err| AppError::Internal(format!("GPU semaphore closed: {err}")))?;

    let english_transcript = match params.source {
        PreparedSource::Audio { pcm_16k, .. } => {
            let raw_english = ctx.asr.transcribe_english(pcm_16k).await?;
            let trimmed = raw_english.trim().to_owned();
            if trimmed.is_empty() {
                return Err(AppError::Validation(
                    "English ASR produced an empty transcript".to_owned(),
                ));
            }
            trimmed
        }
        PreparedSource::Text {
            english_transcript, ..
        } => english_transcript.clone(),
    };

    let sanitized_en = expand_english_numerals_for_tts(&english_transcript);
    let translated = match ctx
        .translator
        .translate_to_spoken_language(&sanitized_en, params.target_lang)
        .await
    {
        Ok(ok) => ok,
        Err(first_err) if first_err.to_string().contains("spoken script hygiene") => {
            let nudged = format!("{sanitized_en} (spell every number out as spoken words)");
            ctx.translator
                .translate_to_spoken_language(&nudged, params.target_lang)
                .await?
        }
        Err(err) => return Err(AppError::from(err)),
    };

    let max_attempts = ctx.config.max_tts_attempts.max(1);
    let mut last_cer_bps = BasisPoints::ONE_HUNDRED_PERCENT;

    for attempt in 1..=max_attempts {
        let seed = deterministic_attempt_seed(params.source.sha256(), attempt);
        let pcm_24k = ctx
            .tts
            .synthesize_cloned_voice(VoiceSynthesisRequest {
                profile_slug: &params.profile.slug,
                reference_wav_path: &params.profile.reference_wav_path,
                reference_transcript_en: &params.profile.reference_transcript_en,
                reference_samples: params.reference_samples,
                engine: params.engine,
                target_language: params.target_lang,
                script: &translated.spoken_hanzi,
                seed,
                tuning: params.tuning,
                finetuned_checkpoint_dir: params.profile.finetuned_checkpoint_dir.as_deref(),
            })
            .await?;
        let back_text = ctx
            .asr
            .transcribe_in_language(&pcm_24k, params.target_lang)
            .await?;
        let verdict = evaluate_cer_gate_for_language(
            translated.spoken_text(),
            &back_text,
            params.target_lang,
            params.max_cer,
        );
        last_cer_bps = verdict.cer_bps;
        if verdict.passed {
            return Ok(VerifiedGpuOutput {
                english_transcript,
                translated,
                pcm_24k,
                back_transcribed_text: back_text,
                cer_bps: verdict.cer_bps,
                attempts_used: attempt,
            });
        }
        tracing::warn!(
            attempt,
            max_attempts,
            language = params.target_lang.code(),
            cer_bps = verdict.cer_bps.value(),
            max_cer_bps = params.max_cer.value(),
            "synthesized clip failed reverse-ASR CER gate; shifting seed"
        );
    }

    Err(AppError::Internal(format!(
        "all {max_attempts} TTS synthesis attempts failed CER gate (last CER: {last_cer_bps}, ceiling: {})",
        params.max_cer
    )))
}

/// Executes the polyglot `Gate 0 / Text -> EN ASR -> Spoken Target -> Seed-Shifted TTS + Reverse-ASR CER -> Opus/WAV` pipeline.
pub async fn translate_clip(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    input: TranslateClipInput,
) -> Result<TranslateClipOutput, AppError> {
    let started_at = Instant::now();
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let profile =
        load_owned_profile_by_slug(&ctx.db, &input.profile_slug, caller_discord_user_id).await?;

    let target_lang = input
        .target_language
        .unwrap_or(profile.default_target_language);
    let effective_preferred_engine = if profile.finetuned_checkpoint_dir.is_some() {
        TtsEngineKind::XttsV2
    } else {
        profile.preferred_engine
    };
    let engine = target_lang.resolve_engine(effective_preferred_engine, input.engine_override);
    let max_cer = input.max_cer_bps_override.unwrap_or(ctx.config.max_cer_bps);

    // Gate 0, tuning validation, and source preparation run strictly BEFORE acquiring GPU semaphore.
    let (tuning, reference_samples) =
        resolve_synthesis_tuning_and_samples(&profile, input.tuning_override)?;
    let source = prepare_input_source(ctx, &input.audio_bytes, input.source_text.as_deref())?;
    let verified = run_gpu_inference_pipeline(
        ctx,
        &GpuPipelineParams {
            profile: &profile,
            reference_samples: &reference_samples,
            source: &source,
            target_lang,
            engine,
            max_cer,
            tuning,
        },
    )
    .await?;

    let bundle = ctx
        .codec
        .encode_bundle(&verified.pcm_24k)
        .map_err(|err| AppError::Internal(err.to_string()))?;

    let job_id = VoiceJobId::new_v7();
    let (output_ogg_path, output_wav_path) =
        write_dual_audio_files(ctx, job_id, &bundle.ogg_opus_bytes, &bundle.wav_48k_bytes).await?;

    let discord_message_id = ctx
        .discord
        .publish(&input.delivery, &bundle, &verified.translated)
        .await?;
    let discord_channel_id = match &input.delivery {
        DiscordDeliveryTarget::BotVoiceBubble { channel_id, .. } => {
            Some(discord_id_to_i64(*channel_id, "discord_channel_id")?)
        }
        DiscordDeliveryTarget::ClientReturnOnly
        | DiscordDeliveryTarget::UserInstallFollowup { .. }
        | DiscordDeliveryTarget::ProfileWebhook { .. } => None,
    };

    let spoken_text = verified.translated.spoken_text().to_owned();
    let phonetic_guide = verified.translated.phonetic_guide().to_owned();
    let back_transcribed_text = verified.back_transcribed_text;
    let total_latency_ms = u32::try_from(started_at.elapsed().as_millis()).unwrap_or(u32::MAX);

    let receipt = VoiceJobReceipt {
        job_id,
        profile_id: profile.id,
        profile_slug: profile.slug,
        target_language: target_lang,
        delivery_mode: input.delivery.mode_slug().to_owned(),
        english_transcript: verified.english_transcript,
        spoken_hanzi: spoken_text.clone(),
        spoken_text,
        pinyin: phonetic_guide.clone(),
        phonetic_guide,
        english_back_gloss: verified.translated.english_back_gloss,
        back_transcribed_zh: back_transcribed_text.clone(),
        back_transcribed_text,
        engine_used: engine,
        cer_bps: verified.cer_bps,
        attempts_used: verified.attempts_used,
        duration_ms: bundle.duration_ms,
        total_latency_ms,
        output_ogg_path,
        discord_message_id,
        created_at: now_utc_micros(ctx),
    };

    persist_voice_job_row(
        &ctx.db,
        &receipt,
        source.sha256(),
        &output_wav_path,
        discord_channel_id,
    )
    .await?;

    Ok(TranslateClipOutput {
        receipt,
        ogg_opus_bytes: bundle.ogg_opus_bytes,
        wav_48k_bytes: bundle.wav_48k_bytes,
    })
}
