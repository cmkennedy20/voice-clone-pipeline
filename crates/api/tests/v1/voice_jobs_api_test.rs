//! Layer-5 black-box `HTTP` transport tests using `tower::ServiceExt::oneshot` against
//! migrated `PostgreSQL` (`#[sqlx::test]`).

use std::sync::Arc;

use api::{
    build_router,
    error::{PROBLEM_JSON_CONTENT_TYPE, ProblemDetails},
    extractors::DISCORD_USER_ID_HEADER,
    middleware::request_id::REQUEST_ID_HEADER,
};
use axum::{
    Router,
    body::Body,
    http::{Request, Response, StatusCode, header},
};
use common::{basis_points::BasisPoints, pagination::Paged};
use domain::{
    ports::DiscordDeliveryTarget,
    translation::types::{LanguageMetadata, TargetLanguage, TtsEngineKind},
};
use features::{
    profiles::{
        enroll_profile::EnrollProfileInput,
        finetune_profile::{StartProfileFinetuneInput, VoiceFinetuneJobRecord},
        manage_samples::AddProfileSampleInput,
        update_profile::UpdateProfileInput,
    },
    shared_queries::{
        profile_loader::VoiceProfileRecord, sample_queries::VoiceProfileSampleRecord,
    },
    voice_jobs::translate_clip::{TranslateClipInput, VoiceJobReceipt},
};
use framework::{
    clients::FakeCallCounters,
    testing::{
        audio_fixtures::{synthetic_silent_short_wav, synthetic_speech_wav},
        test_context::{build_test_context, create_isolated_test_pool},
    },
};
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

const BEARER_HEADER: &str = "Bearer test-owner-bearer-token";
const MULTIPART_BOUNDARY: &str = "----dmv-oneshot-boundary-998877";

async fn build_isolated_router() -> (Router, Arc<FakeCallCounters>) {
    let pool = create_isolated_test_pool().await.unwrap();
    let (mut ctx, counters) = build_test_context(pool);
    Arc::make_mut(&mut ctx.config).storage_dir =
        std::env::temp_dir().join(format!("dmv-api-suite-{}", Uuid::now_v7()));
    (build_router(ctx), counters)
}

async fn read_body_bytes(response: Response<Body>) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

async fn assert_problem_response(
    response: Response<Body>,
    expected_status: StatusCode,
    expected_problem_urn: &str,
) -> ProblemDetails {
    assert_eq!(response.status(), expected_status);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some(PROBLEM_JSON_CONTENT_TYPE)
    );

    let bytes = read_body_bytes(response).await;
    let problem: ProblemDetails = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(problem.problem_type, expected_problem_urn);
    assert_eq!(problem.status, expected_status.as_u16());
    assert_ne!(problem.title, "");
    assert_ne!(problem.detail, "");
    problem
}

fn multipart_wav_request(uri: &str, wav_bytes: &[u8]) -> Request<Body> {
    let mut payload = Vec::with_capacity(wav_bytes.len() + 256);
    payload.extend_from_slice(
        format!(
            "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"clip.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    payload.extend_from_slice(wav_bytes);
    payload.extend_from_slice(format!("\r\n--{MULTIPART_BOUNDARY}--\r\n").as_bytes());

    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::AUTHORIZATION, BEARER_HEADER)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
        )
        .body(Body::from(payload))
        .unwrap()
}

fn multipart_text_request(uri: &str, source_text: &str) -> Request<Body> {
    let payload = format!(
        "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"source_text\"\r\n\r\n{source_text}\r\n--{MULTIPART_BOUNDARY}--\r\n"
    );
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::AUTHORIZATION, BEARER_HEADER)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
        )
        .body(Body::from(payload))
        .unwrap()
}

async fn enroll_default_profile(app: &Router) -> VoiceProfileRecord {
    let input = EnrollProfileInput {
        slug: "default_voice".to_owned(),
        display_name: "Default Voice".to_owned(),
        avatar_url: Some("https://cdn.discordapp.com/embed/avatars/0.png".to_owned()),
        reference_audio_bytes: synthetic_speech_wav(),
        reference_transcript_en: "Reference studio sample in a quiet room.".to_owned(),
        preferred_engine: TtsEngineKind::CosyVoice3,
        default_target_language: TargetLanguage::Mandarin,
        additional_samples: Vec::new(),
        tuning: None,
    };

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/profiles")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&input).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = read_body_bytes(resp).await;
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn health_live_and_ready_return_200_and_propagate_request_id() {
    let (app, _) = build_isolated_router().await;

    let live_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(live_resp.status(), StatusCode::OK);
    let generated_id = live_resp
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_owned();
    assert_eq!(Uuid::parse_str(&generated_id).unwrap().get_version_num(), 7);
    let live_json: serde_json::Value =
        serde_json::from_slice(&read_body_bytes(live_resp).await).unwrap();
    assert_eq!(live_json["status"], "ok");

    let ready_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health/ready")
                .header(REQUEST_ID_HEADER, "health-check-fixed-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ready_resp.status(), StatusCode::OK);
    assert_eq!(
        ready_resp
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok()),
        Some("health-check-fixed-id")
    );
    let ready_json: serde_json::Value =
        serde_json::from_slice(&read_body_bytes(ready_resp).await).unwrap();
    assert_eq!(ready_json["status"], "ready");
}

#[tokio::test]
async fn missing_or_wrong_bearer_token_returns_401_rfc7807_problem_json() {
    let (app, _) = build_isolated_router().await;

    let missing_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/voice-jobs")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_problem_response(
        missing_resp,
        StatusCode::UNAUTHORIZED,
        "urn:voice-clone-pipeline:problem:unauthorized",
    )
    .await;

    let wrong_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/voice-jobs")
                .header(header::AUTHORIZATION, "Bearer forged-token-value")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_problem_response(
        wrong_resp,
        StatusCode::UNAUTHORIZED,
        "urn:voice-clone-pipeline:problem:unauthorized",
    )
    .await;
}

#[tokio::test]
async fn non_owner_discord_user_id_returns_403_and_malformed_header_returns_400() {
    let (app, _) = build_isolated_router().await;

    let forbidden_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/voice-jobs")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .header(DISCORD_USER_ID_HEADER, "999")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_problem_response(
        forbidden_resp,
        StatusCode::FORBIDDEN,
        "urn:voice-clone-pipeline:problem:forbidden",
    )
    .await;

    let bad_header_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/voice-jobs")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .header(DISCORD_USER_ID_HEADER, "not-an-integer")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_problem_response(
        bad_header_resp,
        StatusCode::BAD_REQUEST,
        "urn:voice-clone-pipeline:problem:bad-request",
    )
    .await;
}

#[tokio::test]
async fn enroll_profile_returns_201_duplicate_returns_409_and_list_returns_paged() {
    let (app, _) = build_isolated_router().await;

    let created = enroll_default_profile(&app).await;
    assert_eq!(created.slug, "default_voice");
    assert_eq!(created.preferred_engine, TtsEngineKind::CosyVoice3);

    let dup_input = EnrollProfileInput {
        slug: "default_voice".to_owned(),
        display_name: "Duplicate Attempt".to_owned(),
        avatar_url: None,
        reference_audio_bytes: synthetic_speech_wav(),
        reference_transcript_en: "Another sample.".to_owned(),
        preferred_engine: TtsEngineKind::XttsV2,
        default_target_language: TargetLanguage::Mandarin,
        additional_samples: Vec::new(),
        tuning: None,
    };
    let conflict_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/profiles")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&dup_input).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_problem_response(
        conflict_resp,
        StatusCode::CONFLICT,
        "urn:voice-clone-pipeline:problem:conflict",
    )
    .await;

    let list_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/profiles?limit=10&offset=0")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let paged: Paged<VoiceProfileRecord> =
        serde_json::from_slice(&read_body_bytes(list_resp).await).unwrap();
    assert_eq!(paged.total, 1);
    assert_eq!(paged.items.len(), 1);
    assert_eq!(paged.items[0].slug, "default_voice");
}

#[tokio::test]
async fn gate0_silent_wav_multipart_upload_returns_422_without_invoking_gpu() {
    let (app, counters) = build_isolated_router().await;
    enroll_default_profile(&app).await;

    let silent_wav = synthetic_silent_short_wav();
    let resp = app
        .oneshot(multipart_wav_request(
            "/api/v1/voice-jobs/render",
            &silent_wav,
        ))
        .await
        .unwrap();

    assert_problem_response(
        resp,
        StatusCode::UNPROCESSABLE_ENTITY,
        "urn:voice-clone-pipeline:problem:validation-failed",
    )
    .await;
    assert_eq!(
        counters.asr_en_calls(),
        0,
        "Gate 0 must reject silent clips before touching GPU ASR"
    );
}

#[tokio::test]
async fn render_endpoint_streams_ogg_on_api_v1_and_wav_on_v1_with_cer_header() {
    let (app, _) = build_isolated_router().await;
    enroll_default_profile(&app).await;

    let speech_wav = synthetic_speech_wav();

    let ogg_resp = app
        .clone()
        .oneshot(multipart_wav_request(
            "/api/v1/voice-jobs/render",
            &speech_wav,
        ))
        .await
        .unwrap();
    assert_eq!(ogg_resp.status(), StatusCode::OK);
    assert_eq!(
        ogg_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("audio/ogg")
    );
    assert_eq!(
        ogg_resp
            .headers()
            .get("x-cer-bps")
            .and_then(|v| v.to_str().ok()),
        Some("0")
    );
    let ogg_bytes = read_body_bytes(ogg_resp).await;
    assert!(ogg_bytes.len() > 32);
    assert_eq!(&ogg_bytes[0..4], b"OggS");

    let wav_resp = app
        .oneshot(multipart_wav_request(
            "/v1/voice-jobs/render?format=wav",
            &speech_wav,
        ))
        .await
        .unwrap();
    assert_eq!(wav_resp.status(), StatusCode::OK);
    assert_eq!(
        wav_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("audio/wav")
    );
    assert_eq!(
        wav_resp
            .headers()
            .get("x-cer-bps")
            .and_then(|v| v.to_str().ok()),
        Some("0")
    );
    let wav_bytes = read_body_bytes(wav_resp).await;
    assert!(wav_bytes.len() > 44);
    assert_eq!(&wav_bytes[0..4], b"RIFF");
    assert_eq!(&wav_bytes[8..12], b"WAVE");
}

#[tokio::test]
async fn post_voice_job_returns_201_and_get_by_id_returns_200_receipt() {
    let (app, _) = build_isolated_router().await;
    let profile = enroll_default_profile(&app).await;

    let create_body = TranslateClipInput {
        profile_slug: "default_voice".to_owned(),
        audio_bytes: synthetic_speech_wav(),
        delivery: DiscordDeliveryTarget::ClientReturnOnly,
        ..Default::default()
    };

    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/voice-jobs")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let created: VoiceJobReceipt =
        serde_json::from_slice(&read_body_bytes(create_resp).await).unwrap();
    assert_eq!(created.profile_id, profile.id);
    assert_eq!(created.cer_bps, BasisPoints::ZERO);
    assert_eq!(created.attempts_used, 1);
    assert_eq!(created.spoken_hanzi, "今天下午三点半我们开会。");

    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/voice-jobs/{}", created.job_id))
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let fetched: VoiceJobReceipt =
        serde_json::from_slice(&read_body_bytes(get_resp).await).unwrap();
    assert_eq!(fetched.job_id, created.job_id);
    assert_eq!(fetched.spoken_hanzi, created.spoken_hanzi);
    assert_eq!(fetched.english_transcript, created.english_transcript);

    let missing_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/voice-jobs/018f1234-5678-7abc-8def-0123456789ab")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_problem_response(
        missing_resp,
        StatusCode::NOT_FOUND,
        "urn:voice-clone-pipeline:problem:not-found",
    )
    .await;
}

async fn verify_spanish_render_and_replay(app: &Router, counters: &FakeCallCounters) {
    let es_render = app
        .clone()
        .oneshot(multipart_text_request(
            "/api/v1/voice-jobs/render?profile=default_voice&lang=es&format=wav",
            "Hey team, see you at three thirty!",
        ))
        .await
        .unwrap();
    assert_eq!(es_render.status(), StatusCode::OK);
    assert_eq!(
        es_render
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("audio/wav")
    );
    assert_eq!(
        es_render
            .headers()
            .get("x-target-language")
            .and_then(|v| v.to_str().ok()),
        Some("es")
    );
    assert_eq!(
        es_render
            .headers()
            .get("x-engine-used")
            .and_then(|v| v.to_str().ok()),
        Some("xtts_v2")
    );
    assert_eq!(
        es_render
            .headers()
            .get("x-cer-bps")
            .and_then(|v| v.to_str().ok()),
        Some("0")
    );
    let job_id_str = es_render
        .headers()
        .get("x-job-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_owned();
    let es_wav = read_body_bytes(es_render).await;
    assert_eq!(&es_wav[0..4], b"RIFF");
    assert_eq!(&es_wav[8..12], b"WAVE");
    assert_eq!(counters.asr_en_calls(), 0);

    let jobs_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/voice-jobs?target_language=es")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(jobs_resp.status(), StatusCode::OK);
    let paged_jobs: Paged<VoiceJobReceipt> =
        serde_json::from_slice(&read_body_bytes(jobs_resp).await).unwrap();
    assert_eq!(paged_jobs.total, 1);
    assert_eq!(paged_jobs.items[0].job_id.to_string(), job_id_str);
    assert_eq!(paged_jobs.items[0].target_language, TargetLanguage::Spanish);
    assert_eq!(paged_jobs.items[0].engine_used, TtsEngineKind::XttsV2);

    let replay_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/voice-jobs/{job_id_str}/audio?format=ogg"))
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay_resp.status(), StatusCode::OK);
    assert_eq!(
        replay_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("audio/ogg")
    );
    let replay_ogg = read_body_bytes(replay_resp).await;
    assert_eq!(&replay_ogg[0..4], b"OggS");
}

async fn verify_profile_patch_reference_audio_and_delete(
    app: Router,
    enrolled: &VoiceProfileRecord,
) {
    let patch_body = UpdateProfileInput {
        display_name: Some("Studio Voice Updated".to_owned()),
        default_target_language: Some(TargetLanguage::Spanish),
        ..Default::default()
    };
    let patch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/profiles/default_voice")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&patch_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let patched: VoiceProfileRecord =
        serde_json::from_slice(&read_body_bytes(patch_resp).await).unwrap();
    assert_eq!(patched.id, enrolled.id);
    assert_eq!(patched.display_name, "Studio Voice Updated");
    assert_eq!(patched.default_target_language, TargetLanguage::Spanish);
    assert_eq!(patched.avatar_url, enrolled.avatar_url);

    let ref_audio_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/profiles/default_voice/reference-audio")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ref_audio_resp.status(), StatusCode::OK);
    assert_eq!(
        ref_audio_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("audio/wav")
    );
    assert_eq!(
        read_body_bytes(ref_audio_resp).await,
        synthetic_speech_wav()
    );

    let del_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/profiles/default_voice")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

    let after_del_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/profiles/default_voice/reference-audio")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(after_del_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn polyglot_endpoints_support_languages_config_spanish_text_render_replay_and_profile_crud() {
    let (app, counters) = build_isolated_router().await;
    let enrolled = enroll_default_profile(&app).await;

    let langs_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/languages")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(langs_resp.status(), StatusCode::OK);
    let langs: Vec<LanguageMetadata> =
        serde_json::from_slice(&read_body_bytes(langs_resp).await).unwrap();
    assert_eq!(langs.len(), 12);
    assert!(langs.iter().any(|l| l.code == "es"));
    assert!(langs.iter().any(|l| l.code == "zh"));

    let cfg_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/config")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cfg_resp.status(), StatusCode::OK);
    let cfg_json: serde_json::Value =
        serde_json::from_slice(&read_body_bytes(cfg_resp).await).unwrap();
    assert_eq!(cfg_json["service_name"], "voice-clone-pipeline");
    assert_eq!(cfg_json["discord_owner_user_id"], 777_888_999);
    assert_eq!(cfg_json["discord_owner_user_id_str"], "777888999");
    assert_eq!(cfg_json["discord_application_id"], 111_222_333);
    assert_eq!(cfg_json["discord_application_id_str"], "111222333");
    assert_eq!(cfg_json["default_target_language"], "zh");
    assert_eq!(cfg_json["max_cer_bps"], 1_200);
    assert_eq!(cfg_json["max_tts_attempts"], 3);

    verify_spanish_render_and_replay(&app, &counters).await;
    verify_profile_patch_reference_audio_and_delete(app, &enrolled).await;
}

async fn add_and_verify_second_sample(app: &Router) -> features::profiles::VoiceSampleId {
    let second_wav = framework::testing::audio_fixtures::synthetic_speech_wav_ms(1_800);
    let add_sample = AddProfileSampleInput {
        audio_bytes: second_wav.clone(),
        transcript_en: "Warm casual second sample.".to_owned(),
        style_label: "casual".to_owned(),
        make_primary: false,
    };
    let add_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/profiles/default_voice/samples")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&add_sample).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(add_resp.status(), StatusCode::CREATED);
    let updated_profile: VoiceProfileRecord =
        serde_json::from_slice(&read_body_bytes(add_resp).await).unwrap();
    assert_eq!(updated_profile.sample_count, 2);

    let list_samples_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/profiles/default_voice/samples")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_samples_resp.status(), StatusCode::OK);
    let samples: Vec<VoiceProfileSampleRecord> =
        serde_json::from_slice(&read_body_bytes(list_samples_resp).await).unwrap();
    assert_eq!(samples.len(), 2);
    let second_sample_id = samples[1].id;

    let sample_audio_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/profiles/default_voice/samples/{second_sample_id}/audio"
                ))
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sample_audio_resp.status(), StatusCode::OK);
    assert_eq!(
        sample_audio_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("audio/wav")
    );
    assert_eq!(read_body_bytes(sample_audio_resp).await, second_wav);
    second_sample_id
}

async fn run_and_poll_finetune_via_api(app: &Router) -> VoiceFinetuneJobRecord {
    let initial_ft = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/profiles/default_voice/finetune")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial_ft.status(), StatusCode::OK);
    let initial_job: Option<VoiceFinetuneJobRecord> =
        serde_json::from_slice(&read_body_bytes(initial_ft).await).unwrap();
    assert!(initial_job.is_none());

    let ft_start_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/profiles/default_voice/finetune")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&StartProfileFinetuneInput {
                        epochs: Some(6),
                        learning_rate: Some(5e-6),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ft_start_resp.status(), StatusCode::ACCEPTED);
    let started: VoiceFinetuneJobRecord =
        serde_json::from_slice(&read_body_bytes(ft_start_resp).await).unwrap();
    assert_eq!(started.sample_count, 2);

    let ft_poll_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/profiles/default_voice/finetune")
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ft_poll_resp.status(), StatusCode::OK);
    let finished: Option<VoiceFinetuneJobRecord> =
        serde_json::from_slice(&read_body_bytes(ft_poll_resp).await).unwrap();
    let finished = finished.unwrap();
    assert_eq!(finished.status, "succeeded");
    assert_eq!(finished.progress_bps, 10_000);
    assert!(finished.checkpoint_dir.is_some());
    finished
}

#[tokio::test]
async fn multi_sample_crud_finetune_polling_and_render_tuning_query_overrides_work_end_to_end() {
    let (app, counters) = build_isolated_router().await;
    let enrolled = enroll_default_profile(&app).await;
    assert_eq!(enrolled.sample_count, 1);

    let second_sample_id = add_and_verify_second_sample(&app).await;
    let finished = run_and_poll_finetune_via_api(&app).await;

    let render_resp = app
        .clone()
        .oneshot(multipart_text_request(
            "/api/v1/voice-jobs/render?profile=default_voice&format=wav&temperature=0.45&speed=1.10&sound_norm_refs=false",
            "See you at three thirty!",
        ))
        .await
        .unwrap();
    assert_eq!(render_resp.status(), StatusCode::OK);
    let synth_call = counters.last_synthesis_call().unwrap();
    assert!((synth_call.tuning.temperature - 0.45).abs() < 1e-5);
    assert!((synth_call.tuning.speed - 1.10).abs() < 1e-5);
    assert!(!synth_call.tuning.sound_norm_refs);
    assert_eq!(synth_call.reference_samples.len(), 2);
    assert_eq!(synth_call.finetuned_checkpoint_dir, finished.checkpoint_dir);

    let del_sample_resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/api/v1/profiles/default_voice/samples/{second_sample_id}"
                ))
                .header(header::AUTHORIZATION, BEARER_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_sample_resp.status(), StatusCode::OK);
    let after_del: VoiceProfileRecord =
        serde_json::from_slice(&read_body_bytes(del_sample_resp).await).unwrap();
    assert_eq!(after_del.sample_count, 1);
}
