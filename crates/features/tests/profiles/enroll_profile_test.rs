//! Layer-4 `#[sqlx::test]` integration suite for profile `CRUD`, multi-sample management,
//! synthesis tuning, and `RTX 3090` `XTTS-v2` fine-tuning workflows.

use std::sync::Arc;

use chrono::TimeDelta;
use common::pagination::PageParams;
use domain::{
    ports::{DiscordDeliveryTarget, SynthesisTuning},
    translation::types::{TargetLanguage, TtsEngineKind},
};
use features::{
    error::AppError,
    profiles::{
        delete_profile::delete_profile,
        enroll_profile::{AdditionalSampleInput, EnrollProfileInput, enroll_profile},
        finetune_profile::{
            StartProfileFinetuneInput, get_profile_finetune_status, start_profile_finetune,
        },
        list_profiles::list_profiles,
        manage_samples::{
            AddProfileSampleInput, add_profile_sample, delete_profile_sample, list_profile_samples,
            read_profile_sample_audio,
        },
        read_reference_audio::read_profile_reference_audio,
        update_profile::{AvatarPatch, UpdateProfileInput, update_profile},
    },
    voice_jobs::translate_clip::{TranslateClipInput, translate_clip},
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
        test_context::{FixedClock, build_test_context},
    },
};
use sqlx::PgPool;
use uuid::Uuid;

fn isolated_context(pool: PgPool) -> (AppContext, Arc<FixedClock>, Arc<FakeCallCounters>) {
    let (mut ctx, counters) = build_test_context(pool);
    let mut cfg: AppConfig = (*ctx.config).clone();
    cfg.storage_dir = std::env::temp_dir().join(format!("dmv-profiles-test-{}", Uuid::now_v7()));
    let clock = Arc::new(FixedClock::default());
    ctx.config = Arc::new(cfg);
    ctx.clock = Arc::clone(&clock) as Arc<dyn common::clock::Clock>;
    (ctx, clock, counters)
}

#[sqlx::test(migrations = "../../migrations")]
async fn enrolls_valid_profiles_persists_wav_and_paginates_newest_first(pool: PgPool) {
    let (ctx, clock, _) = isolated_context(pool);
    let owner_id = ctx.config.discord_owner_user_id;
    let expected_wav = synthetic_speech_wav();

    let first = enroll_profile(
        &ctx,
        owner_id,
        EnrollProfileInput {
            slug: "default_voice".to_owned(),
            display_name: "  Primary Voice  ".to_owned(),
            avatar_url: Some("https://cdn.example.com/avatar.png".to_owned()),
            reference_audio_bytes: expected_wav.clone(),
            reference_transcript_en: "  Clear English reference sample. ".to_owned(),
            preferred_engine: TtsEngineKind::CosyVoice3,
            default_target_language: TargetLanguage::Mandarin,
            additional_samples: Vec::new(),
            tuning: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(first.slug, "default_voice");
    assert_eq!(first.display_name, "Primary Voice");
    assert_eq!(first.default_target_language, TargetLanguage::Mandarin);
    assert_eq!(first.sample_count, 1);
    assert_eq!(first.samples.len(), 1);
    assert!(first.samples[0].is_primary);
    assert_eq!(first.finetune_status, "none");
    assert_eq!(
        first.reference_transcript_en,
        "Clear English reference sample."
    );
    let saved_bytes = tokio::fs::read(&first.reference_wav_path).await.unwrap();
    assert_eq!(saved_bytes, expected_wav);

    let read_back = read_profile_reference_audio(&ctx, owner_id, "default_voice")
        .await
        .unwrap();
    assert_eq!(read_back, expected_wav);

    clock.advance(TimeDelta::seconds(10));
    let second = enroll_profile(
        &ctx,
        owner_id,
        EnrollProfileInput {
            slug: "xtts_voice".to_owned(),
            display_name: "XTTS Voice".to_owned(),
            avatar_url: None,
            reference_audio_bytes: synthetic_speech_wav_ms(1_800),
            reference_transcript_en: "Second reference clip.".to_owned(),
            preferred_engine: TtsEngineKind::XttsV2,
            default_target_language: TargetLanguage::Spanish,
            additional_samples: Vec::new(),
            tuning: None,
        },
    )
    .await
    .unwrap();

    let page_one = list_profiles(&ctx, owner_id, PageParams::new(1, 0))
        .await
        .unwrap();
    assert_eq!(page_one.total, 2);
    assert!(page_one.has_more());
    assert_eq!(page_one.items, vec![second]);

    let page_two = list_profiles(&ctx, owner_id, PageParams::new(1, 1))
        .await
        .unwrap();
    assert!(!page_two.has_more());
    assert_eq!(page_two.items, vec![first]);
}

#[sqlx::test(migrations = "../../migrations")]
async fn updates_metadata_and_cascades_delete_across_associated_voice_jobs(pool: PgPool) {
    let (ctx, _clock, _) = isolated_context(pool);
    let owner_id = ctx.config.discord_owner_user_id;

    let enrolled = enroll_profile(
        &ctx,
        owner_id,
        EnrollProfileInput {
            slug: "crud_voice".to_owned(),
            display_name: "Original Name".to_owned(),
            avatar_url: Some("https://cdn.example.com/old.png".to_owned()),
            reference_audio_bytes: synthetic_speech_wav(),
            reference_transcript_en: "Original transcript.".to_owned(),
            preferred_engine: TtsEngineKind::CosyVoice3,
            default_target_language: TargetLanguage::Mandarin,
            additional_samples: Vec::new(),
            tuning: None,
        },
    )
    .await
    .unwrap();

    let updated = update_profile(
        &ctx,
        owner_id,
        "crud_voice",
        UpdateProfileInput {
            display_name: Some("  Polyglot Voice  ".to_owned()),
            avatar_url: AvatarPatch::Clear,
            preferred_engine: Some(TtsEngineKind::XttsV2),
            default_target_language: Some(TargetLanguage::Spanish),
            reference_transcript_en: Some("  Updated transcript.  ".to_owned()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.display_name, "Polyglot Voice");
    assert_eq!(updated.avatar_url, None);
    assert_eq!(updated.preferred_engine, TtsEngineKind::XttsV2);
    assert_eq!(updated.default_target_language, TargetLanguage::Spanish);
    assert_eq!(updated.reference_transcript_en, "Updated transcript.");
    assert_eq!(updated.samples[0].transcript_en, "Updated transcript.");

    let job_out = translate_clip(
        &ctx,
        owner_id,
        TranslateClipInput {
            profile_slug: "crud_voice".to_owned(),
            source_text: Some("See you this afternoon.".to_owned()),
            delivery: DiscordDeliveryTarget::ClientReturnOnly,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    delete_profile(&ctx, owner_id, "crud_voice").await.unwrap();

    assert!(
        tokio::fs::metadata(&enrolled.reference_wav_path)
            .await
            .is_err()
    );
    assert!(
        tokio::fs::metadata(&job_out.receipt.output_ogg_path)
            .await
            .is_err()
    );

    let (remaining_jobs,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM voice_jobs")
        .fetch_one(&ctx.db)
        .await
        .unwrap();
    assert_eq!(remaining_jobs, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn manages_multi_samples_promotes_primary_and_guards_final_sample(pool: PgPool) {
    let (ctx, clock, _) = isolated_context(pool);
    let owner_id = ctx.config.discord_owner_user_id;
    let primary_wav = synthetic_speech_wav_ms(1_500);
    let extra_wav = synthetic_speech_wav_ms(2_000);
    let third_wav = synthetic_speech_wav_ms(1_800);

    let enrolled = enroll_profile(
        &ctx,
        owner_id,
        EnrollProfileInput {
            slug: "studio_voice".to_owned(),
            display_name: "Studio Voice".to_owned(),
            avatar_url: None,
            reference_audio_bytes: primary_wav,
            reference_transcript_en: "Primary clean studio clip.".to_owned(),
            preferred_engine: TtsEngineKind::XttsV2,
            default_target_language: TargetLanguage::Mandarin,
            additional_samples: vec![AdditionalSampleInput {
                audio_bytes: extra_wav,
                transcript_en: "Warm conversational take.".to_owned(),
                style_label: "warm".to_owned(),
            }],
            tuning: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(enrolled.sample_count, 2);
    assert!(enrolled.total_sample_duration_ms >= 3_400);

    clock.advance(TimeDelta::seconds(5));
    let with_third = add_profile_sample(
        &ctx,
        owner_id,
        "studio_voice",
        AddProfileSampleInput {
            audio_bytes: third_wav.clone(),
            transcript_en: "Expressive third reference.".to_owned(),
            style_label: "expressive".to_owned(),
            make_primary: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(with_third.sample_count, 3);
    assert_eq!(
        with_third.reference_transcript_en,
        "Expressive third reference."
    );
    let samples = list_profile_samples(&ctx, owner_id, "studio_voice")
        .await
        .unwrap();
    assert_eq!(samples.len(), 3);
    assert!(samples[0].is_primary);
    assert_eq!(samples[0].style_label, "expressive");

    let audio_bytes = read_profile_sample_audio(&ctx, owner_id, "studio_voice", samples[0].id)
        .await
        .unwrap();
    assert_eq!(audio_bytes, third_wav);

    let after_del_primary = delete_profile_sample(&ctx, owner_id, "studio_voice", samples[0].id)
        .await
        .unwrap();
    assert_eq!(after_del_primary.sample_count, 2);
    assert!(after_del_primary.samples[0].is_primary);

    let one_left = delete_profile_sample(
        &ctx,
        owner_id,
        "studio_voice",
        after_del_primary.samples[1].id,
    )
    .await
    .unwrap();
    assert_eq!(one_left.sample_count, 1);

    let cannot_delete_last =
        delete_profile_sample(&ctx, owner_id, "studio_voice", one_left.samples[0].id)
            .await
            .unwrap_err();
    assert!(matches!(cannot_delete_last, AppError::Validation(_)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn runs_finetune_syncs_checkpoint_and_toggles_back_to_zero_shot(pool: PgPool) {
    let (ctx, _clock, counters) = isolated_context(pool);
    let owner_id = ctx.config.discord_owner_user_id;
    let custom_tuning = SynthesisTuning {
        temperature: 0.55,
        repetition_penalty: 6.5,
        gpt_cond_len_secs: 42,
        sound_norm_refs: false,
        ..SynthesisTuning::default()
    };

    enroll_profile(
        &ctx,
        owner_id,
        EnrollProfileInput {
            slug: "finetune_voice".to_owned(),
            display_name: "FineTune Voice".to_owned(),
            avatar_url: None,
            reference_audio_bytes: synthetic_speech_wav(),
            reference_transcript_en: "Primary reference.".to_owned(),
            preferred_engine: TtsEngineKind::XttsV2,
            default_target_language: TargetLanguage::Mandarin,
            additional_samples: Vec::new(),
            tuning: Some(custom_tuning),
        },
    )
    .await
    .unwrap();

    assert_eq!(
        get_profile_finetune_status(&ctx, owner_id, "finetune_voice")
            .await
            .unwrap(),
        None
    );

    let started_job = start_profile_finetune(
        &ctx,
        owner_id,
        "finetune_voice",
        StartProfileFinetuneInput {
            epochs: Some(8),
            learning_rate: Some(5e-6),
        },
    )
    .await
    .unwrap();
    assert_eq!(started_job.status, "running");
    assert_eq!(started_job.epochs, 8);

    let polled_job = get_profile_finetune_status(&ctx, owner_id, "finetune_voice")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(polled_job.status, "succeeded");
    assert_eq!(polled_job.progress_bps, 10_000);
    assert!((polled_job.last_loss.unwrap() - 0.38).abs() < 1e-6);
    assert!(polled_job.checkpoint_dir.is_some());
    assert_eq!(counters.finetune_start_calls(), 1);
    assert_eq!(counters.finetune_poll_calls(), 1);

    translate_clip(
        &ctx,
        owner_id,
        TranslateClipInput {
            profile_slug: "finetune_voice".to_owned(),
            source_text: Some("Good evening everyone.".to_owned()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let synth_call = counters.last_synthesis_call().unwrap();
    assert_eq!(synth_call.tuning, custom_tuning);
    assert_eq!(
        synth_call.finetuned_checkpoint_dir,
        polled_job.checkpoint_dir
    );

    let reset = update_profile(
        &ctx,
        owner_id,
        "finetune_voice",
        UpdateProfileInput {
            use_finetuned_checkpoint: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(reset.finetune_status, "none");
    assert_eq!(reset.finetuned_checkpoint_dir, None);
}

#[sqlx::test(migrations = "../../migrations")]
async fn rejects_duplicate_slug_with_conflict_without_overwriting_existing_wav(pool: PgPool) {
    let (ctx, _clock, _) = isolated_context(pool);
    let owner_id = ctx.config.discord_owner_user_id;
    let wav_one = synthetic_speech_wav_ms(1_600);
    let wav_two = synthetic_speech_wav_ms(2_200);

    let enrolled = enroll_profile(
        &ctx,
        owner_id,
        EnrollProfileInput {
            slug: "dup_voice".to_owned(),
            display_name: "Voice One".to_owned(),
            avatar_url: None,
            reference_audio_bytes: wav_one.clone(),
            reference_transcript_en: "First clip.".to_owned(),
            preferred_engine: TtsEngineKind::CosyVoice3,
            default_target_language: TargetLanguage::Mandarin,
            additional_samples: Vec::new(),
            tuning: None,
        },
    )
    .await
    .unwrap();

    let err = enroll_profile(
        &ctx,
        owner_id,
        EnrollProfileInput {
            slug: "dup_voice".to_owned(),
            display_name: "Voice Duplicate".to_owned(),
            avatar_url: None,
            reference_audio_bytes: wav_two,
            reference_transcript_en: "Duplicate clip.".to_owned(),
            preferred_engine: TtsEngineKind::XttsV2,
            default_target_language: TargetLanguage::Mandarin,
            additional_samples: Vec::new(),
            tuning: None,
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, AppError::Conflict(_)));
    let disk_bytes = tokio::fs::read(&enrolled.reference_wav_path).await.unwrap();
    assert_eq!(disk_bytes, wav_one);

    let (row_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM voice_profiles")
        .fetch_one(&ctx.db)
        .await
        .unwrap();
    assert_eq!(row_count, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn rejects_silent_clipped_and_malformed_slug_inputs_at_validation_boundary(pool: PgPool) {
    let (ctx, _clock, _) = isolated_context(pool);
    let owner_id = ctx.config.discord_owner_user_id;

    let silent_err = enroll_profile(
        &ctx,
        owner_id,
        EnrollProfileInput {
            slug: "silent_ref".to_owned(),
            display_name: "Silent".to_owned(),
            avatar_url: None,
            reference_audio_bytes: synthetic_silent_short_wav(),
            reference_transcript_en: "Silent clip.".to_owned(),
            preferred_engine: TtsEngineKind::CosyVoice3,
            default_target_language: TargetLanguage::Mandarin,
            additional_samples: Vec::new(),
            tuning: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(silent_err, AppError::Validation(_)));

    let clipped_err = enroll_profile(
        &ctx,
        owner_id,
        EnrollProfileInput {
            slug: "clipped_ref".to_owned(),
            display_name: "Clipped".to_owned(),
            avatar_url: None,
            reference_audio_bytes: synthetic_clipped_wav(),
            reference_transcript_en: "Clipped clip.".to_owned(),
            preferred_engine: TtsEngineKind::CosyVoice3,
            default_target_language: TargetLanguage::Mandarin,
            additional_samples: Vec::new(),
            tuning: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(clipped_err, AppError::Validation(_)));

    let slug_err = enroll_profile(
        &ctx,
        owner_id,
        EnrollProfileInput {
            slug: "Invalid-Slug".to_owned(),
            display_name: "Bad Slug".to_owned(),
            avatar_url: None,
            reference_audio_bytes: synthetic_speech_wav(),
            reference_transcript_en: "Valid audio.".to_owned(),
            preferred_engine: TtsEngineKind::CosyVoice3,
            default_target_language: TargetLanguage::Mandarin,
            additional_samples: Vec::new(),
            tuning: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(slug_err, AppError::Validation(_)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn forbids_non_owner_from_enrolling_or_listing_profiles(pool: PgPool) {
    let (ctx, _clock, _) = isolated_context(pool);
    let intruder_id = 123_456_789_u64;

    let enroll_err = enroll_profile(
        &ctx,
        intruder_id,
        EnrollProfileInput {
            slug: "intruder_voice".to_owned(),
            display_name: "Intruder".to_owned(),
            avatar_url: None,
            reference_audio_bytes: synthetic_speech_wav(),
            reference_transcript_en: "Unauthorized.".to_owned(),
            preferred_engine: TtsEngineKind::CosyVoice3,
            default_target_language: TargetLanguage::Mandarin,
            additional_samples: Vec::new(),
            tuning: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(enroll_err, AppError::Forbidden(_)));

    let list_err = list_profiles(&ctx, intruder_id, PageParams::default())
        .await
        .unwrap_err();
    assert!(matches!(list_err, AppError::Forbidden(_)));
}
