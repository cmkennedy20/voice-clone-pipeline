//! Layer-7 `RTX 3090` 6-metric release suite runner (`run_rtx3090_suite`) and `benchmark_runs` ledger writer.

use std::time::Instant;

use chrono::{DateTime, Utc};
use common::basis_points::BasisPoints;
use domain::{
    audio::types::{Pcm24kMono, RenderedAudioBundle},
    ports::{AudioCodecEngine, VoiceSynthesisRequest},
    translation::{
        types::{MandarinSpokenScript, TargetLanguage, TtsEngineKind},
        verifier::evaluate_cer_gate_for_language,
    },
};
use framework::context::AppContext;

use crate::{
    benchmarks::{
        ffprobe_check::verify_ffprobe_dual_bundle,
        manifest::{GoldenUtteranceSpec, evaluate_gate0_traps, parse_golden_manifest},
        scorecard::{
            ClipEvaluationOutcome, ScorecardRenderParams, aggregate_suite_metrics,
            render_scorecard_markdown,
        },
    },
    error::AppError,
    shared_queries::profile_loader::{
        BenchmarkRunId, VoiceProfileRecord, deterministic_attempt_seed, ensure_caller_is_owner,
        load_owned_profile_by_slug, now_utc_micros,
    },
};

/// Input configuration for executing the 15-utterance + 3-trap `RTX 3090` benchmark suite.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunBenchmarkSuiteInput {
    pub profile_slug: String,
    pub engine_override: Option<TtsEngineKind>,
    pub manifest_json: String,
    pub peak_vram_mib_override: Option<u32>,
}

/// Persisted 6-metric (`M1–M6`) evaluation report and rendered Markdown scorecard.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkSuiteReport {
    pub run_id: BenchmarkRunId,
    pub engine_tested: TtsEngineKind,
    pub clips_evaluated: u16,
    pub gate0_accuracy_bps: BasisPoints,
    pub digit_hygiene_bps: BasisPoints,
    pub mean_cer_bps: BasisPoints,
    pub max_clip_cer_bps: BasisPoints,
    pub mean_speaker_sim_bps: BasisPoints,
    pub format_compliance_bps: BasisPoints,
    pub p95_latency_ms: u32,
    pub peak_vram_mib: u32,
    pub passed_all_gates: bool,
    pub scorecard_markdown: String,
    pub created_at: DateTime<Utc>,
}

async fn verify_bundle_format_compliance(
    codec: &dyn AudioCodecEngine,
    pcm_24k: &Pcm24kMono,
    bundle: &RenderedAudioBundle,
) -> bool {
    let has_ogg_magic = bundle.ogg_opus_bytes.starts_with(b"OggS");
    let has_riff_wave_magic = bundle.wav_48k_bytes.starts_with(b"RIFF")
        && bundle.wav_48k_bytes.get(8..12) == Some(b"WAVE");
    let waveform_valid =
        (1..=256).contains(&bundle.waveform.bin_count()) && bundle.waveform.decode_bins().is_ok();
    if !has_ogg_magic || !has_riff_wave_magic || !waveform_valid {
        return false;
    }

    let expected_ms = pcm_24k.duration_ms();
    if bundle.duration_ms.abs_diff(expected_ms) >= 40 {
        return false;
    }
    let Ok((_, ogg_metrics)) = codec.decode_and_measure(&bundle.ogg_opus_bytes) else {
        return false;
    };
    let Ok((_, wav_metrics)) = codec.decode_and_measure(&bundle.wav_48k_bytes) else {
        return false;
    };
    if ogg_metrics.total_duration_ms.abs_diff(expected_ms) >= 40
        || wav_metrics.total_duration_ms.abs_diff(expected_ms) >= 40
    {
        return false;
    }
    verify_ffprobe_dual_bundle(&bundle.ogg_opus_bytes, &bundle.wav_48k_bytes).await
}

async fn evaluate_single_utterance(
    ctx: &AppContext,
    profile: &VoiceProfileRecord,
    engine_tested: TtsEngineKind,
    utterance: &GoldenUtteranceSpec,
    max_clip_cer_bps: BasisPoints,
) -> Result<ClipEvaluationOutcome, AppError> {
    let clip_start = Instant::now();
    let gpu_permit = ctx
        .gpu_semaphore
        .acquire()
        .await
        .map_err(|err| AppError::Internal(format!("GPU semaphore closed: {err}")))?;

    let translated = ctx
        .translator
        .translate_to_spoken_language(&utterance.english_prompt, TargetLanguage::Mandarin)
        .await?;
    let spoken = translated.spoken_text();
    let digit_hygiene_ok = MandarinSpokenScript::parse(spoken).is_ok()
        && utterance
            .forbidden_substrings
            .iter()
            .all(|sub| !spoken.contains(sub.as_str()));

    let max_attempts = ctx.config.max_tts_attempts.max(1);
    let reference_samples = profile.reference_sample_refs();
    let mut best_attempt: Option<(Pcm24kMono, BasisPoints, u8)> = None;

    for attempt in 1..=max_attempts {
        let seed = deterministic_attempt_seed(&utterance.id, attempt);
        let pcm_24k = ctx
            .tts
            .synthesize_cloned_voice(VoiceSynthesisRequest {
                profile_slug: &profile.slug,
                reference_wav_path: &profile.reference_wav_path,
                reference_transcript_en: &profile.reference_transcript_en,
                reference_samples: &reference_samples,
                engine: engine_tested,
                target_language: TargetLanguage::Mandarin,
                script: &translated.spoken_hanzi,
                seed,
                tuning: profile.tuning,
                finetuned_checkpoint_dir: profile.finetuned_checkpoint_dir.as_deref(),
            })
            .await?;
        let back_zh = ctx
            .asr
            .transcribe_in_language(&pcm_24k, TargetLanguage::Mandarin)
            .await?;
        let verdict = evaluate_cer_gate_for_language(
            spoken,
            &back_zh,
            TargetLanguage::Mandarin,
            max_clip_cer_bps,
        );
        let should_replace = best_attempt
            .as_ref()
            .is_none_or(|(_, best_cer, _)| verdict.cer_bps < *best_cer);
        if should_replace {
            best_attempt = Some((pcm_24k, verdict.cer_bps, attempt));
        }
        if verdict.passed {
            break;
        }
    }

    let Some((pcm_24k, cer_bps, attempts_used)) = best_attempt else {
        return Err(AppError::Internal(
            "benchmark utterance produced zero synthesis attempts".to_owned(),
        ));
    };

    let bundle = ctx
        .codec
        .encode_bundle(&pcm_24k)
        .map_err(|err| AppError::Internal(err.to_string()))?;
    let speaker_sim_bps = ctx
        .speaker_sim
        .score_similarity_bps(&profile.reference_wav_path, &bundle.wav_48k_bytes)
        .await?;
    drop(gpu_permit);

    let format_compliant =
        verify_bundle_format_compliance(ctx.codec.as_ref(), &pcm_24k, &bundle).await;
    let latency_ms = u32::try_from(clip_start.elapsed().as_millis()).unwrap_or(u32::MAX);

    Ok(ClipEvaluationOutcome {
        id: utterance.id.clone(),
        category: utterance.category.clone(),
        digit_hygiene_ok,
        cer_bps,
        speaker_sim_bps,
        format_compliant,
        latency_ms,
        attempts_used,
    })
}

async fn probe_peak_vram_mib(override_mib: Option<u32>) -> u32 {
    if let Some(mib) = override_mib {
        return mib;
    }
    let output = tokio::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.used", "--format=csv,noheader,nounits"])
        .output()
        .await;
    let Ok(out) = output else {
        return 0;
    };
    if !out.status.success() {
        return 0;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .max()
        .unwrap_or(0)
}

async fn persist_benchmark_run(
    db: &sqlx::PgPool,
    report: &BenchmarkSuiteReport,
) -> Result<(), AppError> {
    let clips_i16 = i16::try_from(report.clips_evaluated).unwrap_or(i16::MAX);
    let gate0_i32 = i32::try_from(report.gate0_accuracy_bps.value()).unwrap_or(i32::MAX);
    let hygiene_i32 = i32::try_from(report.digit_hygiene_bps.value()).unwrap_or(i32::MAX);
    let cer_i32 = i32::try_from(report.mean_cer_bps.value()).unwrap_or(i32::MAX);
    let sim_i32 = i32::try_from(report.mean_speaker_sim_bps.value()).unwrap_or(i32::MAX);
    let fmt_i32 = i32::try_from(report.format_compliance_bps.value()).unwrap_or(i32::MAX);
    let p95_i32 = i32::try_from(report.p95_latency_ms).unwrap_or(i32::MAX);
    let vram_i32 = i32::try_from(report.peak_vram_mib).unwrap_or(i32::MAX);

    sqlx::query(
        "INSERT INTO benchmark_runs (
            id, engine_tested, clips_evaluated, gate0_accuracy_bps, digit_hygiene_bps,
            mean_cer_bps, mean_speaker_sim_bps, format_compliance_bps, p95_latency_ms,
            peak_vram_mib, passed_all_gates, created_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(report.run_id.as_uuid())
    .bind(report.engine_tested.as_str())
    .bind(clips_i16)
    .bind(gate0_i32)
    .bind(hygiene_i32)
    .bind(cer_i32)
    .bind(sim_i32)
    .bind(fmt_i32)
    .bind(p95_i32)
    .bind(vram_i32)
    .bind(report.passed_all_gates)
    .bind(report.created_at)
    .execute(db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed inserting benchmark_runs row"))?;

    Ok(())
}

/// Evaluates `M1–M6` across the golden manifest, persists `benchmark_runs`, and renders the Markdown scorecard.
pub async fn run_rtx3090_suite(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    input: RunBenchmarkSuiteInput,
) -> Result<BenchmarkSuiteReport, AppError> {
    let RunBenchmarkSuiteInput {
        profile_slug,
        engine_override,
        manifest_json,
        peak_vram_mib_override,
    } = input;

    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let profile =
        load_owned_profile_by_slug(&ctx.db, &profile_slug, caller_discord_user_id).await?;
    let engine_tested = engine_override.unwrap_or(profile.preferred_engine);
    let manifest = parse_golden_manifest(&manifest_json)?;

    // M1 evaluates Gate-0 traps before acquiring any GPU permit.
    let gate0_accuracy_bps = evaluate_gate0_traps(&manifest.gate0_traps);
    let max_clip_cer_bps = BasisPoints::from_raw(manifest.thresholds.max_clip_cer_bps);

    let mut outcomes = Vec::with_capacity(manifest.utterances.len());
    for utterance in &manifest.utterances {
        let outcome =
            evaluate_single_utterance(ctx, &profile, engine_tested, utterance, max_clip_cer_bps)
                .await?;
        outcomes.push(outcome);
    }

    let peak_vram_mib = probe_peak_vram_mib(peak_vram_mib_override).await;
    let metrics = aggregate_suite_metrics(
        gate0_accuracy_bps,
        &outcomes,
        peak_vram_mib,
        &manifest.thresholds,
    );
    let run_id = BenchmarkRunId::new_v7();
    let scorecard_markdown = render_scorecard_markdown(&ScorecardRenderParams {
        run_id,
        engine_tested,
        manifest_version: &manifest.manifest_version,
        metrics: &metrics,
        thresholds: &manifest.thresholds,
        clips: &outcomes,
    });

    let report = BenchmarkSuiteReport {
        run_id,
        engine_tested,
        clips_evaluated: metrics.clips_evaluated,
        gate0_accuracy_bps: metrics.gate0_accuracy_bps,
        digit_hygiene_bps: metrics.digit_hygiene_bps,
        mean_cer_bps: metrics.mean_cer_bps,
        max_clip_cer_bps: metrics.max_clip_cer_bps,
        mean_speaker_sim_bps: metrics.mean_speaker_sim_bps,
        format_compliance_bps: metrics.format_compliance_bps,
        p95_latency_ms: metrics.p95_latency_ms,
        peak_vram_mib: metrics.peak_vram_mib,
        passed_all_gates: metrics.passed_all_gates,
        scorecard_markdown,
        created_at: now_utc_micros(ctx),
    };

    persist_benchmark_run(&ctx.db, &report).await?;
    Ok(report)
}
