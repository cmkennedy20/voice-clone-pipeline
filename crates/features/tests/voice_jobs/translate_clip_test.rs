//! Layer-4 `#[sqlx::test]` integration suite for `translate_clip`, `list_voice_jobs`,
//! `read_voice_job_audio`, `get_voice_job`, `run_rtx3090_suite`, and `list_benchmark_runs`.

use std::sync::Arc;

use common::{basis_points::BasisPoints, pagination::PageParams};
use domain::{
    ports::DiscordDeliveryTarget,
    translation::types::{TargetLanguage, TtsEngineKind},
};
use features::{
    benchmarks::{
        list_benchmarks::list_benchmark_runs,
        run_rtx3090_suite::{RunBenchmarkSuiteInput, run_rtx3090_suite},
    },
    error::AppError,
    shared_queries::profile_loader::{VoiceJobId, load_owned_profile_by_slug},
    voice_jobs::{
        get_voice_job::get_voice_job,
        list_voice_jobs::{ListVoiceJobsFilter, list_voice_jobs, read_voice_job_audio},
        translate_clip::{TranslateClipInput, translate_clip},
    },
};
use framework::{
    clients::FakeCallCounters,
    config::AppConfig,
    context::AppContext,
    testing::{
        audio_fixtures::{
            synthetic_clipped_wav, synthetic_silent_short_wav, synthetic_speech_wav,
            synthetic_speech_wav_ms,
        },
        test_context::{
            build_test_context, build_test_context_healing_on_attempt,
            build_test_context_with_always_mumbling_tts,
        },
    },
};
use sqlx::PgPool;
use uuid::Uuid;

fn with_isolated_storage(mut ctx: AppContext) -> AppContext {
    let mut cfg: AppConfig = (*ctx.config).clone();
    cfg.storage_dir = std::env::temp_dir().join(format!("dmv-jobs-test-{}", Uuid::now_v7()));
    ctx.config = Arc::new(cfg);
    ctx
}

async fn seed_owner_profile(db: &PgPool, slug: &str, owner_id: i64) {
    sqlx::query(
        "INSERT INTO voice_profiles (
            id, slug, display_name, reference_wav_path, reference_transcript_en,
            preferred_engine, owner_discord_user_id, created_at
         ) VALUES ($1, $2, 'Alex', '/tmp/ref.wav', 'Reference clip.', 'cosyvoice_3', $3, now())",
    )
    .bind(Uuid::now_v7())
    .bind(slug)
    .bind(owner_id)
    .execute(db)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn aborts_without_publishing_or_db_commit_when_all_3_tts_seeds_fail_cer(pool: PgPool) {
    let (raw_ctx, counters) = build_test_context_with_always_mumbling_tts(pool);
    let ctx = with_isolated_storage(raw_ctx);
    let owner_id: i64 = 777_888_999;
    seed_owner_profile(&ctx.db, "default_voice", owner_id).await;

    let err = translate_clip(
        &ctx,
        u64::try_from(owner_id).unwrap(),
        TranslateClipInput {
            profile_slug: "default_voice".into(),
            audio_bytes: synthetic_speech_wav(),
            delivery: DiscordDeliveryTarget::BotVoiceBubble {
                channel_id: 42,
                reply_to_message_id: None,
            },
            ..Default::default()
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, AppError::Internal(_)));
    assert_eq!(counters.tts_calls(), 3, "Must attempt all 3 seed shifts");
    let seeds = counters.seeds_seen();
    assert_eq!(seeds.len(), 3);
    assert_ne!(seeds[0], seeds[1]);
    assert_ne!(seeds[1], seeds[2]);
    assert_eq!(
        counters.discord_publishes(),
        0,
        "Must NEVER publish unverified audio to Discord"
    );

    let saved_jobs: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM voice_jobs")
        .fetch_one(&ctx.db)
        .await
        .unwrap();
    assert_eq!(saved_jobs.0, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn self_heals_on_second_seed_persists_receipt_and_round_trips_get_voice_job(pool: PgPool) {
    let (raw_ctx, counters) = build_test_context_healing_on_attempt(pool, 2);
    let ctx = with_isolated_storage(raw_ctx);
    let owner_id = ctx.config.discord_owner_user_id;
    seed_owner_profile(&ctx.db, "default_voice", i64::try_from(owner_id).unwrap()).await;

    let out = translate_clip(
        &ctx,
        owner_id,
        TranslateClipInput {
            profile_slug: "default_voice".to_owned(),
            audio_bytes: synthetic_speech_wav(),
            delivery: DiscordDeliveryTarget::BotVoiceBubble {
                channel_id: 88_001,
                reply_to_message_id: Some(77_001),
            },
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(out.receipt.attempts_used, 2);
    assert_eq!(out.receipt.cer_bps, BasisPoints::ZERO);
    assert_eq!(out.receipt.target_language, TargetLanguage::Mandarin);
    assert_eq!(out.receipt.profile_slug, "default_voice");
    assert_eq!(out.receipt.delivery_mode, "bot_voice_bubble");
    assert_eq!(out.receipt.discord_message_id, Some(900_001));
    assert_eq!(&out.ogg_opus_bytes[..4], b"OggS");
    assert_eq!(&out.wav_48k_bytes[..4], b"RIFF");

    let seeds = counters.seeds_seen();
    assert_eq!(seeds.len(), 2);
    assert_ne!(seeds[0], seeds[1]);
    assert_eq!(counters.discord_publishes(), 1);

    let loaded = get_voice_job(&ctx, owner_id, out.receipt.job_id)
        .await
        .unwrap();
    assert_eq!(loaded, out.receipt);

    let missing = get_voice_job(&ctx, owner_id, VoiceJobId::new_v7())
        .await
        .unwrap_err();
    assert!(matches!(missing, AppError::NotFound(_)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn translates_direct_source_text_to_spanish_auto_routing_cosyvoice_profile_to_xtts(
    pool: PgPool,
) {
    let (raw_ctx, counters) = build_test_context(pool);
    let ctx = with_isolated_storage(raw_ctx);
    let owner_id = ctx.config.discord_owner_user_id;
    seed_owner_profile(&ctx.db, "default_voice", i64::try_from(owner_id).unwrap()).await;

    let empty_err = translate_clip(
        &ctx,
        owner_id,
        TranslateClipInput {
            profile_slug: "default_voice".to_owned(),
            source_text: Some("   ".to_owned()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(empty_err, AppError::Validation(_)));
    assert_eq!(counters.translator_calls(), 0);
    assert_eq!(counters.tts_calls(), 0);

    let out = translate_clip(
        &ctx,
        owner_id,
        TranslateClipInput {
            profile_slug: "default_voice".to_owned(),
            target_language: Some(TargetLanguage::Spanish),
            source_text: Some(
                "  Hello team, we have a meeting at three thirty in the afternoon.  ".to_owned(),
            ),
            delivery: DiscordDeliveryTarget::ClientReturnOnly,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(
        counters.asr_en_calls(),
        0,
        "Direct text mode must bypass English ASR"
    );
    assert_eq!(out.receipt.target_language, TargetLanguage::Spanish);
    assert_eq!(out.receipt.engine_used, TtsEngineKind::XttsV2);
    assert_eq!(out.receipt.cer_bps, BasisPoints::ZERO);
    assert_eq!(
        out.receipt.spoken_text,
        "Hola equipo, tenemos una reunión a las tres y media de la tarde."
    );
    assert_eq!(out.receipt.spoken_hanzi, out.receipt.spoken_text);
    assert_ne!(out.receipt.phonetic_guide, "");

    let replay_ogg = read_voice_job_audio(&ctx, owner_id, out.receipt.job_id, false)
        .await
        .unwrap();
    assert_eq!(replay_ogg, out.ogg_opus_bytes);
    let replay_wav = read_voice_job_audio(&ctx, owner_id, out.receipt.job_id, true)
        .await
        .unwrap();
    assert_eq!(replay_wav, out.wav_48k_bytes);

    let es_jobs = list_voice_jobs(
        &ctx,
        owner_id,
        ListVoiceJobsFilter {
            target_language: Some(TargetLanguage::Spanish),
            profile_slug: Some("default_voice".to_owned()),
            page: PageParams::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!(es_jobs.total, 1);
    assert_eq!(es_jobs.items, vec![out.receipt]);
}

#[sqlx::test(migrations = "../../migrations")]
async fn gate0_rejects_silent_and_clipped_audio_before_any_gpu_call(pool: PgPool) {
    let (raw_ctx, counters): (AppContext, Arc<FakeCallCounters>) = build_test_context(pool);
    let ctx = with_isolated_storage(raw_ctx);
    let owner_id = ctx.config.discord_owner_user_id;
    seed_owner_profile(&ctx.db, "default_voice", i64::try_from(owner_id).unwrap()).await;

    for bad_wav in [synthetic_silent_short_wav(), synthetic_clipped_wav()] {
        let err = translate_clip(
            &ctx,
            owner_id,
            TranslateClipInput {
                profile_slug: "default_voice".to_owned(),
                audio_bytes: bad_wav,
                delivery: DiscordDeliveryTarget::ClientReturnOnly,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    assert_eq!(counters.asr_en_calls(), 0);
    assert_eq!(counters.translator_calls(), 0);
    assert_eq!(counters.tts_calls(), 0);
    assert_eq!(counters.asr_zh_calls(), 0);
    assert_eq!(counters.discord_publishes(), 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn forbids_non_owner_access_and_foreign_profile_loads(pool: PgPool) {
    let (raw_ctx, _counters) = build_test_context(pool);
    let ctx = with_isolated_storage(raw_ctx);
    let owner_id = ctx.config.discord_owner_user_id;
    let intruder_id = 444_555_666_u64;
    seed_owner_profile(&ctx.db, "default_voice", i64::try_from(owner_id).unwrap()).await;
    seed_owner_profile(&ctx.db, "other_user_voice", 999_111_222).await;

    let clip_err = translate_clip(
        &ctx,
        intruder_id,
        TranslateClipInput {
            profile_slug: "default_voice".to_owned(),
            audio_bytes: synthetic_speech_wav(),
            delivery: DiscordDeliveryTarget::ClientReturnOnly,
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(clip_err, AppError::Forbidden(_)));

    let get_err = get_voice_job(&ctx, intruder_id, VoiceJobId::new_v7())
        .await
        .unwrap_err();
    assert!(matches!(get_err, AppError::Forbidden(_)));

    let foreign_profile_err = load_owned_profile_by_slug(&ctx.db, "other_user_voice", owner_id)
        .await
        .unwrap_err();
    assert!(matches!(foreign_profile_err, AppError::Forbidden(_)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn serializes_concurrent_gpu_stages_behind_single_permit_semaphore(pool: PgPool) {
    let (raw_ctx, counters) = build_test_context(pool);
    let ctx = with_isolated_storage(raw_ctx);
    let owner_id = ctx.config.discord_owner_user_id;
    seed_owner_profile(&ctx.db, "default_voice", i64::try_from(owner_id).unwrap()).await;

    let mk_input = |duration_ms: u32| TranslateClipInput {
        profile_slug: "default_voice".to_owned(),
        audio_bytes: synthetic_speech_wav_ms(duration_ms),
        delivery: DiscordDeliveryTarget::ClientReturnOnly,
        ..Default::default()
    };

    let f1 = translate_clip(&ctx, owner_id, mk_input(1_500));
    let f2 = translate_clip(&ctx, owner_id, mk_input(1_700));
    let f3 = translate_clip(&ctx, owner_id, mk_input(1_900));
    let (r1, r2, r3) = tokio::join!(f1, f2, f3);

    assert!(r1.is_ok());
    assert!(r2.is_ok());
    assert!(r3.is_ok());
    assert_eq!(counters.tts_calls(), 3);
    assert_eq!(counters.discord_publishes(), 0);
    assert_eq!(
        counters.max_concurrent_gpu_calls(),
        1,
        "Semaphore(1) must serialize all concurrent GPU calls"
    );

    let (job_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM voice_jobs")
        .fetch_one(&ctx.db)
        .await
        .unwrap();
    assert_eq!(job_count, 3);
}

#[sqlx::test(migrations = "../../migrations")]
async fn runs_rtx3090_benchmark_suite_on_golden_manifest_and_persists_passing_report(pool: PgPool) {
    let (raw_ctx, counters) = build_test_context(pool);
    let ctx = with_isolated_storage(raw_ctx);
    let owner_id = ctx.config.discord_owner_user_id;
    seed_owner_profile(&ctx.db, "default_voice", i64::try_from(owner_id).unwrap()).await;

    let manifest_json = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../evals/golden_manifest.json"
    ))
    .to_owned();

    let report = run_rtx3090_suite(
        &ctx,
        owner_id,
        RunBenchmarkSuiteInput {
            profile_slug: "default_voice".to_owned(),
            engine_override: Some(TtsEngineKind::XttsV2),
            manifest_json,
            peak_vram_mib_override: Some(19_800),
        },
    )
    .await
    .unwrap();

    assert_eq!(report.engine_tested, TtsEngineKind::XttsV2);
    assert_eq!(report.clips_evaluated, 15);
    assert_eq!(report.gate0_accuracy_bps, BasisPoints::ONE_HUNDRED_PERCENT);
    assert_eq!(report.digit_hygiene_bps, BasisPoints::ONE_HUNDRED_PERCENT);
    assert_eq!(report.mean_cer_bps, BasisPoints::ZERO);
    assert_eq!(report.max_clip_cer_bps, BasisPoints::ZERO);
    assert_eq!(report.mean_speaker_sim_bps, BasisPoints::from_raw(8_650));
    assert_eq!(
        report.format_compliance_bps,
        BasisPoints::ONE_HUNDRED_PERCENT
    );
    assert_eq!(report.peak_vram_mib, 19_800);
    assert!(report.passed_all_gates);
    assert!(
        report
            .scorecard_markdown
            .contains("# RTX 3090 Layer-7 Release Scorecard")
    );
    assert_eq!(counters.translator_calls(), 15);
    assert_eq!(counters.tts_calls(), 15);

    let listed = list_benchmark_runs(&ctx, owner_id, PageParams::default())
        .await
        .unwrap();
    assert_eq!(listed.total, 1);
    assert_eq!(listed.items[0].id, report.run_id);
    assert_eq!(listed.items[0].engine_tested, TtsEngineKind::XttsV2);
    assert!(listed.items[0].passed_all_gates);
}

#[sqlx::test(migrations = "../../migrations")]
async fn translate_clip_applies_tuning_override_and_rejects_invalid_tuning_before_gpu(
    pool: PgPool,
) {
    let (raw_ctx, counters) = build_test_context(pool);
    let ctx = with_isolated_storage(raw_ctx);
    let owner_id = ctx.config.discord_owner_user_id;
    seed_owner_profile(&ctx.db, "default_voice", i64::try_from(owner_id).unwrap()).await;

    let bad_tuning = domain::ports::SynthesisTuning {
        temperature: 2.50,
        ..domain::ports::SynthesisTuning::default()
    };
    let err = translate_clip(
        &ctx,
        owner_id,
        TranslateClipInput {
            profile_slug: "default_voice".to_owned(),
            source_text: Some("Check invalid tuning.".to_owned()),
            tuning_override: Some(bad_tuning),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AppError::Validation(_)));
    assert_eq!(counters.tts_calls(), 0);

    let override_tuning = domain::ports::SynthesisTuning {
        temperature: 0.48,
        speed: 1.15,
        sound_norm_refs: false,
        ..domain::ports::SynthesisTuning::default()
    };
    translate_clip(
        &ctx,
        owner_id,
        TranslateClipInput {
            profile_slug: "default_voice".to_owned(),
            source_text: Some("Check valid tuning override.".to_owned()),
            tuning_override: Some(override_tuning),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let recorded = counters.last_synthesis_call().unwrap();
    assert_eq!(recorded.tuning, override_tuning);
    assert_eq!(recorded.reference_samples.len(), 1);
    assert_eq!(recorded.reference_samples[0].wav_path, "/tmp/ref.wav");
}
