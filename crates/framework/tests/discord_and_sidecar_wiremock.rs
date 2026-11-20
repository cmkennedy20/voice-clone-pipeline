#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use common::{basis_points::BasisPoints, secret::Secret};
use domain::{
    ports::{
        AudioCodecEngine, DiscordDeliveryTarget, DiscordPublisher, ExternalPortError,
        FinetuneStartRequest, MandarinTranslator, ReferenceSampleRef, SpokenScript,
        SynthesisTuning, TargetLanguage, TranslatedMandarin, VoiceFinetuner, VoiceSynthesisRequest,
        VoiceSynthesizer,
    },
    translation::types::{MandarinSpokenScript, TtsEngineKind},
};
use framework::{
    clients::{
        discord_publisher::HttpDiscordPublisher, qwen_llama::QwenLlamaTranslator,
        tts_sidecar::HttpTtsSidecarClient,
    },
    context::SpeakerSimilarityScorer,
    dsp::opus_and_wav::RealAudioCodecEngine,
    testing::audio_fixtures::{synthetic_24k_pcm, synthetic_speech_wav_ms},
};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, header_exists, method, path, query_param},
};

fn sample_translation() -> TranslatedMandarin {
    TranslatedMandarin {
        spoken_hanzi: MandarinSpokenScript::parse("你好世界").unwrap(),
        pinyin: "nǐ hǎo shì jiè".into(),
        english_back_gloss: "Hello world".into(),
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|win| win == needle)
}

fn encode_16khz_pcm16_wav(duration_ms: u32) -> Vec<u8> {
    let sample_count = usize::try_from((u64::from(duration_ms) * 16_000) / 1_000).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for idx in 0..sample_count {
            let phase = u16::try_from((idx * 440) % 16_000).unwrap();
            let angle = std::f32::consts::TAU * (f32::from(phase) / 16_000.0_f32);
            let sample = if angle.sin() >= 0.0 {
                8_000_i16
            } else {
                -8_000_i16
            };
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
    }
    cursor.into_inner()
}

#[tokio::test]
async fn bot_voice_bubble_omits_auth_on_gcs_put_and_sets_flag_8192_without_content() {
    let server = MockServer::start().await;
    let codec = RealAudioCodecEngine::new();
    let bundle = codec.encode_bundle(&synthetic_24k_pcm(1_500)).unwrap();

    // 1. Pre-signed upload slot request
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/42/attachments"))
        .and(header("authorization", "Bot test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "attachments": [{
                "upload_url": format!("{}/gcs-upload/slot-0", server.uri()),
                "upload_filename": "gcs/voice-message.ogg"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    // 2. GCS PUT must have audio/ogg and NO Discord Authorization header (otherwise GCS 403s)
    Mock::given(method("PUT"))
        .and(path("/gcs-upload/slot-0"))
        .and(header("content-type", "audio/ogg"))
        .respond_with(|req: &wiremock::Request| {
            assert!(
                !req.headers.contains_key("authorization"),
                "GCS pre-signed PUT must NOT leak Bot Authorization header"
            );
            assert_eq!(
                &req.body[0..4],
                b"OggS",
                "Uploaded bytes must be real Ogg container"
            );
            ResponseTemplate::new(200)
        })
        .expect(1)
        .mount(&server)
        .await;

    // 3. Final message POST must set flags=8192 and omit `content`/`embeds`
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/42/messages"))
        .and(header_exists("authorization"))
        .respond_with(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["flags"], 8192);
            assert!(
                body.get("content").is_none(),
                "flags=8192 payload cannot include content"
            );
            assert!(
                body.get("embeds").is_none(),
                "flags=8192 payload cannot include embeds"
            );
            assert!(
                body.get("message_reference").is_none(),
                "message_reference must be omitted when reply_to_message_id is None"
            );
            assert!(body["attachments"][0]["waveform"].as_str().unwrap().len() >= 4);
            ResponseTemplate::new(200).set_body_json(json!({ "id": "99887766" }))
        })
        .expect(1)
        .mount(&server)
        .await;

    let publisher =
        HttpDiscordPublisher::with_base_url(server.uri(), Secret::new("test-token".into()));
    let msg_id = publisher
        .publish(
            &DiscordDeliveryTarget::BotVoiceBubble {
                channel_id: 42,
                reply_to_message_id: None,
            },
            &bundle,
            &sample_translation(),
        )
        .await
        .unwrap();

    assert_eq!(msg_id, Some(99_887_766));
}

#[tokio::test]
async fn bot_voice_bubble_includes_message_reference_when_replying() {
    let server = MockServer::start().await;
    let codec = RealAudioCodecEngine::new();
    let bundle = codec.encode_bundle(&synthetic_24k_pcm(1_000)).unwrap();

    Mock::given(method("POST"))
        .and(path("/api/v10/channels/88/attachments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "attachments": [{
                "upload_url": format!("{}/gcs-upload/slot-reply", server.uri()),
                "upload_filename": "gcs/reply-voice.ogg"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/gcs-upload/slot-reply"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v10/channels/88/messages"))
        .respond_with(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["flags"], 8192);
            assert!(body.get("content").is_none());
            assert_eq!(body["message_reference"]["message_id"], "12345000");
            ResponseTemplate::new(200).set_body_json(json!({ "id": "55443322" }))
        })
        .expect(1)
        .mount(&server)
        .await;

    let publisher =
        HttpDiscordPublisher::with_base_url(server.uri(), Secret::new("test-token".into()));
    let msg_id = publisher
        .publish(
            &DiscordDeliveryTarget::BotVoiceBubble {
                channel_id: 88,
                reply_to_message_id: Some(12_345_000),
            },
            &bundle,
            &sample_translation(),
        )
        .await
        .unwrap();

    assert_eq!(msg_id, Some(55_443_322));
}

#[tokio::test]
async fn user_install_followup_uploads_multipart_ogg_and_spoiler_pinyin() {
    let server = MockServer::start().await;
    let codec = RealAudioCodecEngine::new();
    let bundle = codec.encode_bundle(&synthetic_24k_pcm(1_000)).unwrap();

    Mock::given(method("POST"))
        .and(path("/api/v10/webhooks/555666/ix-secret-token"))
        .and(query_param("wait", "true"))
        .respond_with(|req: &wiremock::Request| {
            let ct = req
                .headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(ct.starts_with("multipart/form-data; boundary="));
            let body_lossy = String::from_utf8_lossy(&req.body);
            assert!(body_lossy.contains("name=\"payload_json\""));
            assert!(body_lossy.contains("你好世界"));
            assert!(body_lossy.contains("filename=\"voice-message.ogg\""));
            assert!(body_lossy.contains("Content-Type: audio/ogg"));
            assert!(contains_subslice(&req.body, b"OggS"));
            ResponseTemplate::new(200).set_body_json(json!({ "id": "44556677" }))
        })
        .expect(1)
        .mount(&server)
        .await;

    let publisher =
        HttpDiscordPublisher::with_base_url(server.uri(), Secret::new("unused-bot-token".into()));
    let msg_id = publisher
        .publish(
            &DiscordDeliveryTarget::UserInstallFollowup {
                application_id: 555_666,
                interaction_token: Secret::new("ix-secret-token".into()),
                attach_spoiler_pinyin: true,
            },
            &bundle,
            &sample_translation(),
        )
        .await
        .unwrap();

    assert_eq!(msg_id, Some(44_556_677));
}

#[tokio::test]
async fn profile_webhook_sends_custom_username_and_avatar_url_override() {
    let server = MockServer::start().await;
    let codec = RealAudioCodecEngine::new();
    let bundle = codec.encode_bundle(&synthetic_24k_pcm(1_000)).unwrap();

    Mock::given(method("POST"))
        .and(path("/api/webhooks/777/wh-token-xyz"))
        .and(query_param("wait", "true"))
        .respond_with(|req: &wiremock::Request| {
            let body_lossy = String::from_utf8_lossy(&req.body);
            assert!(body_lossy.contains("\"username\":\"Speaker (Mandarin)\""));
            assert!(body_lossy.contains("\"avatar_url\":\"https://cdn.example.com/avatar.png\""));
            assert!(body_lossy.contains("filename=\"voice-message.ogg\""));
            assert!(contains_subslice(&req.body, b"OggS"));
            ResponseTemplate::new(200).set_body_json(json!({ "id": "33221100" }))
        })
        .expect(1)
        .mount(&server)
        .await;

    let publisher =
        HttpDiscordPublisher::with_base_url(server.uri(), Secret::new("unused-bot-token".into()));
    let msg_id = publisher
        .publish(
            &DiscordDeliveryTarget::ProfileWebhook {
                webhook_url: Secret::new(format!("{}/api/webhooks/777/wh-token-xyz", server.uri())),
                username: "Speaker (Mandarin)".into(),
                avatar_url: Some("https://cdn.example.com/avatar.png".into()),
            },
            &bundle,
            &sample_translation(),
        )
        .await
        .unwrap();

    assert_eq!(msg_id, Some(33_221_100));
}

#[tokio::test]
async fn client_return_only_makes_zero_http_requests() {
    let server = MockServer::start().await;
    let codec = RealAudioCodecEngine::new();
    let bundle = codec.encode_bundle(&synthetic_24k_pcm(800)).unwrap();

    let publisher =
        HttpDiscordPublisher::with_base_url(server.uri(), Secret::new("bot-token".into()));
    let out = publisher
        .publish(
            &DiscordDeliveryTarget::ClientReturnOnly,
            &bundle,
            &sample_translation(),
        )
        .await
        .unwrap();

    assert_eq!(out, None);
    let received = server.received_requests().await.unwrap();
    assert!(
        received.is_empty(),
        "ClientReturnOnly must never emit network requests"
    );
}

#[tokio::test]
async fn qwen_llama_translator_recovers_from_http_503_and_parses_json() {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_clone = Arc::clone(&attempts);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["response_format"]["type"], "json_object");
            let temp = body["temperature"].as_f64().unwrap();
            assert!((temp - 0.2).abs() < 1e-5);
            let sys_prompt = body["messages"][0]["content"].as_str().unwrap();
            assert!(sys_prompt.contains("/no_think"));

            let call_idx = attempts_clone.fetch_add(1, Ordering::SeqCst);
            if call_idx == 0 {
                ResponseTemplate::new(503).set_body_string("model warming up")
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{
                        "message": {
                            "content": "<think></think>\n```json\n{\"spoken_hanzi\": \"今天下午三点半我们开会。\", \"pinyin\": \"jīn tiān xià wǔ sān diǎn bàn wǒ men kāi huì.\", \"english_back_gloss\": \"We have a meeting at 3:30 PM today.\"}\n```"
                        }
                    }]
                }))
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let translator = QwenLlamaTranslator::new(server.uri());
    let translated = translator
        .translate_english_to_spoken_zh("We have a meeting at 3:30 this afternoon.")
        .await
        .unwrap();

    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(translated.spoken_hanzi.as_str(), "今天下午三点半我们开会。");
    assert_eq!(
        translated.pinyin,
        "jīn tiān xià wǔ sān diǎn bàn wǒ men kāi huì."
    );
}

#[tokio::test]
async fn qwen_llama_translator_rejects_unexpanded_digits_from_llm() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": "{\"spoken_hanzi\": \"今天下午3点半我们开会。\", \"pinyin\": \"jīn tiān\", \"english_back_gloss\": \"Meeting at 3:30.\"}"
                }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let translator = QwenLlamaTranslator::new(server.uri());
    let err = translator
        .translate_english_to_spoken_zh("Meeting at 3:30.")
        .await
        .unwrap_err();

    assert!(matches!(err, ExternalPortError::Translation(_)));
    assert!(err.to_string().contains("hygiene"));
}

#[tokio::test]
async fn tts_sidecar_retries_http_503_decodes_wav_and_scores_speaker_similarity() {
    let server = MockServer::start().await;
    let synth_attempts = Arc::new(AtomicUsize::new(0));
    let synth_attempts_clone = Arc::clone(&synth_attempts);
    let wav_24k = synthetic_speech_wav_ms(1_200);

    Mock::given(method("POST"))
        .and(path("/synthesize"))
        .respond_with(move |req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["profile_slug"], "default_voice");
            assert_eq!(body["engine"], "cosyvoice_3");
            assert_eq!(body["spoken_hanzi"], "今天下午三点半我们开会。");
            assert_eq!(body["seed"], 77);

            let idx = synth_attempts_clone.fetch_add(1, Ordering::SeqCst);
            if idx == 0 {
                ResponseTemplate::new(503).set_body_string("CUDA engine prewarming")
            } else {
                ResponseTemplate::new(200).set_body_raw(wav_24k.clone(), "audio/wav")
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let candidate_bytes = encode_16khz_pcm16_wav(1_000);
    let expected_cand = candidate_bytes.clone();

    Mock::given(method("POST"))
        .and(path("/speaker-similarity"))
        .respond_with(move |req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["reference_wav_path"], "/storage/profiles/default.wav");
            let b64 = body["candidate_wav_base64"].as_str().unwrap();
            let decoded = STANDARD.decode(b64).unwrap();
            assert_eq!(decoded, expected_cand);

            ResponseTemplate::new(200).set_body_json(json!({
                "similarity_bps": 8425,
                "backend": "ecapa_tdnn"
            }))
        })
        .expect(1)
        .mount(&server)
        .await;

    let client = HttpTtsSidecarClient::new(server.uri());
    let script = MandarinSpokenScript::parse("今天下午三点半我们开会。").unwrap();
    let pcm = client
        .synthesize_cloned_zh(
            "default_voice",
            "/storage/profiles/default.wav",
            "Reference clip.",
            TtsEngineKind::CosyVoice3,
            &script,
            77,
        )
        .await
        .unwrap();

    assert_eq!(synth_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(pcm.duration_ms(), 1_200);
    assert_eq!(pcm.samples().len(), 28_800);

    let sim_bps = client
        .score_similarity_bps("/storage/profiles/default.wav", &candidate_bytes)
        .await
        .unwrap();
    assert_eq!(sim_bps, BasisPoints::from_raw(8_425));
}

#[tokio::test]
async fn tts_sidecar_resamples_non_24khz_wav_into_24khz_pcm() {
    let server = MockServer::start().await;
    let wav_16k = encode_16khz_pcm16_wav(1_000);

    Mock::given(method("POST"))
        .and(path("/synthesize"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(wav_16k, "audio/wav"))
        .expect(1)
        .mount(&server)
        .await;

    let client = HttpTtsSidecarClient::new(server.uri());
    let script = MandarinSpokenScript::parse("你好世界").unwrap();
    let pcm = client
        .synthesize_cloned_zh(
            "default_voice",
            "/storage/profiles/default.wav",
            "Reference clip.",
            TtsEngineKind::XttsV2,
            &script,
            1,
        )
        .await
        .unwrap();

    assert!((950..=1_050).contains(&pcm.duration_ms()));
}

#[tokio::test]
async fn qwen_and_tts_sidecar_support_spanish_spoken_text_wire_contract() {
    let server = MockServer::start().await;
    let wav_24k = synthetic_speech_wav_ms(900);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            let sys_prompt = body["messages"][0]["content"].as_str().unwrap();
            assert!(sys_prompt.contains("/no_think"));
            assert!(sys_prompt.contains("Spanish (Español)"));
            ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "content": "{\"spoken_text\": \"Hola equipo, nos vemos en cinco minutos.\", \"phonetic_guide\": \"OH-lah eh-KEE-poh\", \"english_back_gloss\": \"Hi team, see you in five minutes.\"}"
                    }
                }]
            }))
        })
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/synthesize"))
        .respond_with(move |req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["target_language"], "es");
            assert_eq!(body["engine"], "xtts_v2");
            assert_eq!(
                body["spoken_text"],
                "Hola equipo, nos vemos en cinco minutos."
            );
            assert_eq!(
                body["spoken_hanzi"],
                "Hola equipo, nos vemos en cinco minutos."
            );
            assert_eq!(
                body["reference_wav_paths"],
                json!(["/storage/profiles/default.wav"])
            );
            assert_eq!(body["reference_transcripts"], json!(["Reference clip."]));
            assert!(body["finetuned_checkpoint_dir"].is_null());
            assert_eq!(body["tuning"]["top_k"], 50);
            ResponseTemplate::new(200).set_body_raw(wav_24k.clone(), "audio/wav")
        })
        .expect(1)
        .mount(&server)
        .await;

    let translator = QwenLlamaTranslator::new(server.uri());
    let translated = translator
        .translate_to_spoken_language("Hi team, see you in 5 minutes.", TargetLanguage::Spanish)
        .await
        .unwrap();
    assert_eq!(
        translated.spoken_text(),
        "Hola equipo, nos vemos en cinco minutos."
    );
    assert_eq!(translated.phonetic_guide(), "OH-lah eh-KEE-poh");

    let script = SpokenScript::parse_for_language(
        "Hola equipo, nos vemos en cinco minutos.",
        TargetLanguage::Spanish,
    )
    .unwrap();
    let client = HttpTtsSidecarClient::new(server.uri());
    let pcm = client
        .synthesize_cloned_voice(VoiceSynthesisRequest {
            profile_slug: "default_voice",
            reference_wav_path: "/storage/profiles/default.wav",
            reference_transcript_en: "Reference clip.",
            reference_samples: &[],
            engine: TtsEngineKind::XttsV2,
            target_language: TargetLanguage::Spanish,
            script: &script,
            seed: 9,
            tuning: SynthesisTuning::default(),
            finetuned_checkpoint_dir: None,
        })
        .await
        .unwrap();
    assert_eq!(pcm.duration_ms(), 900);
}

#[tokio::test]
async fn tts_sidecar_sends_multi_sample_tuning_and_checkpoint_dir() {
    let server = MockServer::start().await;
    let wav_24k = synthetic_speech_wav_ms(1_000);

    Mock::given(method("POST"))
        .and(path("/synthesize"))
        .respond_with(move |req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(
                body["reference_wav_paths"],
                json!(["/data/voice/p1.wav", "/data/voice/s2.wav"])
            );
            assert_eq!(
                body["reference_transcripts"],
                json!(["First studio take.", "Second conversational take."])
            );
            assert_eq!(
                body["finetuned_checkpoint_dir"],
                "/data/voice/finetuned_xtts"
            );
            assert!((body["tuning"]["temperature"].as_f64().unwrap() - 0.50).abs() < 1e-5);
            assert_eq!(body["tuning"]["gpt_cond_len_secs"], 45);
            assert_eq!(body["tuning"]["sound_norm_refs"], false);
            ResponseTemplate::new(200).set_body_raw(wav_24k.clone(), "audio/wav")
        })
        .expect(1)
        .mount(&server)
        .await;

    let client = HttpTtsSidecarClient::new(server.uri());
    let samples = vec![
        ReferenceSampleRef {
            wav_path: "/data/voice/p1.wav".to_owned(),
            transcript_en: "First studio take.".to_owned(),
        },
        ReferenceSampleRef {
            wav_path: "/data/voice/s2.wav".to_owned(),
            transcript_en: "Second conversational take.".to_owned(),
        },
    ];
    let script = MandarinSpokenScript::parse("你好世界").unwrap();
    let tuning = SynthesisTuning {
        temperature: 0.50,
        gpt_cond_len_secs: 45,
        sound_norm_refs: false,
        ..SynthesisTuning::default()
    };
    let pcm = client
        .synthesize_cloned_voice(VoiceSynthesisRequest {
            profile_slug: "default_voice",
            reference_wav_path: "/data/voice/p1.wav",
            reference_transcript_en: "First studio take.",
            reference_samples: &samples,
            engine: TtsEngineKind::XttsV2,
            target_language: TargetLanguage::Mandarin,
            script: &script,
            seed: 42,
            tuning,
            finetuned_checkpoint_dir: Some("/data/voice/finetuned_xtts"),
        })
        .await
        .unwrap();
    assert_eq!(pcm.duration_ms(), 1_000);
}

#[tokio::test]
async fn tts_sidecar_starts_and_polls_finetune_lifecycle() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/finetune"))
        .respond_with(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["job_id"], "ft-job-01");
            assert_eq!(body["profile_slug"], "default_voice");
            assert_eq!(body["samples"].as_array().unwrap().len(), 2);
            assert_eq!(body["samples"][0]["wav_path"], "/data/voice/p1.wav");
            assert_eq!(body["output_dir"], "/data/voice/finetuned_xtts");
            assert_eq!(body["epochs"], 8);
            assert!((body["learning_rate"].as_f64().unwrap() - 5e-6).abs() < 1e-12);
            ResponseTemplate::new(202).set_body_json(json!({
                "job_id": "ft-job-01",
                "profile_slug": "default_voice",
                "status": "running",
                "epochs": 8,
                "current_epoch": 1,
                "progress_bps": 1250,
                "last_loss": 0.91,
                "checkpoint_dir": null,
                "error_message": null
            }))
        })
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/finetune/ft-job-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "job_id": "ft-job-01",
            "profile_slug": "default_voice",
            "status": "succeeded",
            "epochs": 8,
            "current_epoch": 8,
            "progress_bps": 10000,
            "last_loss": 0.34,
            "checkpoint_dir": "/data/voice/finetuned_xtts",
            "error_message": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = HttpTtsSidecarClient::new(server.uri());
    let samples = vec![
        ReferenceSampleRef {
            wav_path: "/data/voice/p1.wav".to_owned(),
            transcript_en: "First studio take.".to_owned(),
        },
        ReferenceSampleRef {
            wav_path: "/data/voice/s2.wav".to_owned(),
            transcript_en: "Second conversational take.".to_owned(),
        },
    ];
    let started = client
        .start_finetune(&FinetuneStartRequest {
            job_id: "ft-job-01".to_owned(),
            profile_slug: "default_voice".to_owned(),
            samples,
            output_dir: "/data/voice/finetuned_xtts".to_owned(),
            epochs: 8,
            learning_rate: 5e-6,
        })
        .await
        .unwrap();
    assert_eq!(started.status, "running");
    assert_eq!(started.progress_bps, 1_250);

    let polled = client.poll_finetune("ft-job-01").await.unwrap();
    assert_eq!(polled.status, "succeeded");
    assert_eq!(polled.progress_bps, 10_000);
    assert_eq!(
        polled.checkpoint_dir.as_deref(),
        Some("/data/voice/finetuned_xtts")
    );
}
