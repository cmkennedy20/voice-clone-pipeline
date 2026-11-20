//! `USER_INSTALL` + `GUILD_INSTALL` slash commands (`/voice_clone`, `/mandarin_voice`,
//! `/voice_settings`, `/voice_profiles`) and `HTTP` relay to `api-server`.

pub mod choices;
pub mod management;
pub mod relay;

use std::borrow::Cow;

pub use choices::{EngineChoice, LanguageChoice};
use domain::ports::DiscordDeliveryTarget;
use framework::auth::owner_token::verify_discord_owner;
pub use management::{configured_voice_profiles_command, configured_voice_settings_command};
pub use relay::{
    RelayPayloadSpec, WorkerError, WorkerState, build_relay_input, resolve_slash_delivery,
};
pub(crate) use relay::{download_attachment_bytes, relay_voice_job};
use serenity::all::{Attachment, InstallationContext, InteractionContext, ResolvedValue};

use crate::dm_relay::format_voice_bubble_caption;

pub(crate) const DEFAULT_PROFILE_SLUG: &str = "default_voice";

pub type AppCtx<'a> = poise::ApplicationContext<'a, WorkerState, WorkerError>;

struct SlashVoiceRequest {
    language: Option<LanguageChoice>,
    text: Option<String>,
    audio: Option<Attachment>,
    profile: Option<String>,
    voice_bubble: bool,
    private_only: bool,
    spoiler_phonetics: bool,
}

fn extract_optional_bool_arg(ctx: AppCtx<'_>, name: &str) -> Option<bool> {
    ctx.args.iter().find_map(|arg| {
        if arg.name == name
            && let ResolvedValue::Boolean(flag) = arg.value
        {
            Some(flag)
        } else {
            None
        }
    })
}

async fn handle_slash_voice_request(
    ctx: AppCtx<'_>,
    req: SlashVoiceRequest,
) -> Result<(), WorkerError> {
    let state = ctx.data();
    let caller_id = ctx.author().id.get();

    if verify_discord_owner(caller_id, state.owner_user_id).is_err() {
        ctx.send(
            poise::CreateReply::default()
                .content("Unauthorized: this personal voice app is restricted to its owner.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    if req.private_only {
        ctx.defer_ephemeral().await?;
    } else {
        ctx.defer().await?;
    }

    let app_id = if state.application_id != 0 {
        state.application_id
    } else {
        ctx.interaction.application_id.get()
    };
    let delivery = resolve_slash_delivery(
        ctx.interaction.context,
        req.voice_bubble,
        req.private_only,
        ctx.channel_id().get(),
        app_id,
        ctx.interaction.token.clone(),
        false,
    );
    let used_bot_bubble = matches!(delivery, DiscordDeliveryTarget::BotVoiceBubble { .. });

    let relay_result = async {
        let audio_bytes = match &req.audio {
            Some(att) => download_attachment_bytes(&state.http, &att.url).await?,
            None => Vec::new(),
        };
        let source_text = req
            .text
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        if audio_bytes.is_empty() && source_text.is_none() {
            return Err(WorkerError::Relay(
                "provide either an `audio` attachment or `text` prompt".to_owned(),
            ));
        }
        let input = build_relay_input(RelayPayloadSpec {
            profile: req.profile.as_deref(),
            target_language: req.language.map(LanguageChoice::target_language),
            audio_bytes,
            source_text,
            delivery,
        });
        relay_voice_job(state, caller_id, &input).await
    }
    .await;

    match relay_result {
        Ok(receipt) => {
            if req.spoiler_phonetics || used_bot_bubble {
                let _ = ctx.say(format_voice_bubble_caption(&receipt)).await;
            } else {
                let _ = ctx.say("\u{200B}").await;
            }
            tracing::info!(
                job_id = %receipt.job_id,
                lang = %receipt.target_language,
                engine = %receipt.engine_used,
                cer_bps = receipt.cer_bps.value(),
                latency_ms = receipt.total_latency_ms,
                "completed Discord slash voice clone delivery"
            );
        }
        Err(err) => {
            tracing::warn!(error = %err, "Discord voice clone relay failed");
            let _ = ctx.say(format!("Voice synthesis failed: {err}")).await;
        }
    }
    Ok(())
}

/// Translate an English voice clip or text prompt into your cloned voice across 12 languages.
#[poise::command(
    slash_command,
    install_context = "Guild|User",
    interaction_context = "Guild|BotDm|PrivateChannel"
)]
pub async fn voice_clone(
    ctx: AppCtx<'_>,
    #[description = "Target language (omit to use your profile's saved default language)"]
    language: Option<LanguageChoice>,
    #[description = "Direct English text prompt (alternative to uploading audio)"] text: Option<
        String,
    >,
    #[description = "English voice/audio clip (.ogg, .wav, .mp3, .m4a, .flac)"] audio: Option<
        Attachment,
    >,
    #[description = "Enrolled voice profile slug (default: default_voice)"] profile: Option<String>,
    #[description = "Post a native flags=8192 voice waveform bubble (1-on-1 Bot DM or bot channels)"]
    voice_bubble: Option<bool>,
    #[description = "Only show the synthesized clip to you (ephemeral response)"]
    private_only: Option<bool>,
) -> Result<(), WorkerError> {
    let spoiler_phonetics = extract_optional_bool_arg(ctx, "spoiler_phonetics").unwrap_or(true);
    handle_slash_voice_request(
        ctx,
        SlashVoiceRequest {
            language,
            text,
            audio,
            profile,
            voice_bubble: voice_bubble.unwrap_or(false),
            private_only: private_only.unwrap_or(false),
            spoiler_phonetics,
        },
    )
    .await
}

/// Alias command translating an English voice clip or text prompt into your cloned Mandarin voice.
#[poise::command(
    slash_command,
    install_context = "Guild|User",
    interaction_context = "Guild|BotDm|PrivateChannel"
)]
pub async fn mandarin_voice(
    ctx: AppCtx<'_>,
    #[description = "Direct English text prompt (alternative to uploading audio)"] text: Option<
        String,
    >,
    #[description = "English voice/audio clip (.ogg, .wav, .mp3, .m4a, .flac)"] audio: Option<
        Attachment,
    >,
    #[description = "Enrolled voice profile slug (default: default_voice)"] profile: Option<String>,
    #[description = "Only show the synthesized clip to you (ephemeral response)"]
    private_only: Option<bool>,
    #[description = "Attach spoiler-tagged Pinyin alongside the audio clip (default: true)"]
    spoiler_pinyin: Option<bool>,
) -> Result<(), WorkerError> {
    handle_slash_voice_request(
        ctx,
        SlashVoiceRequest {
            language: Some(LanguageChoice::Mandarin),
            text,
            audio,
            profile,
            voice_bubble: false,
            private_only: private_only.unwrap_or(false),
            spoiler_phonetics: spoiler_pinyin.unwrap_or(true),
        },
    )
    .await
}

/// Speak an English text prompt in your cloned voice (`text` auto-focused immediately).
#[poise::command(
    slash_command,
    install_context = "Guild|User",
    interaction_context = "Guild|BotDm|PrivateChannel"
)]
pub async fn speak(
    ctx: AppCtx<'_>,
    #[description = "English sentence to translate and speak in your cloned voice"] text: String,
    #[description = "Target language (omit to use your profile's saved default language)"]
    language: Option<LanguageChoice>,
    #[description = "Enrolled voice profile slug (default: auto-selects your profile)"]
    profile: Option<String>,
    #[description = "Only show the synthesized clip to you (ephemeral response)"]
    private_only: Option<bool>,
    #[description = "Post a native flags=8192 voice waveform bubble (bot channels/DMs)"]
    voice_bubble: Option<bool>,
) -> Result<(), WorkerError> {
    handle_slash_voice_request(
        ctx,
        SlashVoiceRequest {
            language,
            text: Some(text),
            audio: None,
            profile,
            voice_bubble: voice_bubble.unwrap_or(false),
            private_only: private_only.unwrap_or(false),
            spoiler_phonetics: true,
        },
    )
    .await
}

pub(crate) fn with_all_install_and_interaction_contexts(
    mut cmd: poise::Command<WorkerState, WorkerError>,
) -> poise::Command<WorkerState, WorkerError> {
    cmd.install_context = Some(vec![InstallationContext::User, InstallationContext::Guild]);
    cmd.interaction_context = Some(vec![
        InteractionContext::Guild,
        InteractionContext::BotDm,
        InteractionContext::PrivateChannel,
    ]);
    cmd
}

/// Returns `/voice_clone` configured for `USER_INSTALL` + `GUILD_INSTALL` across contexts `0..=2`.
pub fn configured_voice_clone_command() -> poise::Command<WorkerState, WorkerError> {
    let mut cmd = with_all_install_and_interaction_contexts(voice_clone());
    cmd.parameters.push(poise::CommandParameter {
        name: Cow::Borrowed("spoiler_phonetics"),
        name_localizations: Cow::Borrowed(&[]),
        description: Some(Cow::Borrowed(
            "Attach spoiler-tagged phonetic guide alongside the audio clip (default: true)",
        )),
        description_localizations: Cow::Borrowed(&[]),
        required: false,
        channel_types: None,
        type_setter: Some(<bool as poise::SlashArgument>::create),
        choices: Cow::Borrowed(&[]),
        autocomplete_callback: None,
        __non_exhaustive: (),
    });
    cmd
}

/// Returns `/mandarin_voice` configured for `USER_INSTALL` + `GUILD_INSTALL` across contexts `0..=2`.
pub fn configured_mandarin_voice_command() -> poise::Command<WorkerState, WorkerError> {
    with_all_install_and_interaction_contexts(mandarin_voice())
}

/// Returns `/speak` configured for `USER_INSTALL` + `GUILD_INSTALL` across contexts `0..=2`.
pub fn configured_speak_command() -> poise::Command<WorkerState, WorkerError> {
    with_all_install_and_interaction_contexts(speak())
}

/// Returns all 5 slash commands configured for `USER_INSTALL` + `GUILD_INSTALL`.
pub fn configured_commands() -> Vec<poise::Command<WorkerState, WorkerError>> {
    vec![
        configured_speak_command(),
        configured_voice_clone_command(),
        configured_mandarin_voice_command(),
        configured_voice_settings_command(),
        configured_voice_profiles_command(),
    ]
}

/// Builds the `poise::Framework` registering slash commands and the 1-on-1 `DM` event listener.
pub fn build_framework(state: WorkerState) -> poise::Framework<WorkerState, WorkerError> {
    let options = poise::FrameworkOptions {
        commands: configured_commands(),
        event_handler: |framework_ctx, event| {
            Box::pin(crate::dm_relay::handle_gateway_event(
                framework_ctx.serenity_context,
                event,
                framework_ctx.user_data,
            ))
        },
        ..Default::default()
    };
    poise::Framework::builder()
        .options(options)
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                tracing::info!(bot_user = %ready.user.name, "registering global USER_INSTALL slash commands");
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(state)
            })
        })
        .build()
}

#[cfg(test)]
mod tests {
    use common::{
        basis_points::BasisPoints,
        clock::{Clock, SystemClock},
        pagination::{PageParams, Paged},
        secret::Secret,
    };
    use domain::{
        ports::DiscordDeliveryTarget,
        translation::types::{TargetLanguage, TtsEngineKind},
    };
    use features::{
        profiles::{AvatarPatch, VoiceProfileId, VoiceProfileRecord},
        shared_queries::profile_loader::VoiceJobId,
        voice_jobs::translate_clip::VoiceJobReceipt,
    };
    use poise::ChoiceParameter;
    use serenity::all::{InstallationContext, InteractionContext};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::{
        EngineChoice, LanguageChoice, RelayPayloadSpec, WorkerState, build_relay_input,
        configured_commands,
        management::{
            build_profile_patch_input, fetch_enrolled_profiles, format_profiles_summary,
            format_updated_profile_reply, patch_profile_settings, resolve_valid_profile_slug,
        },
        relay_voice_job, resolve_slash_delivery,
    };

    #[test]
    fn maps_all_twelve_language_choices_and_engine_choices_cleanly() {
        let expected = [
            (
                LanguageChoice::Spanish,
                TargetLanguage::Spanish,
                "Spanish (es)",
            ),
            (
                LanguageChoice::Mandarin,
                TargetLanguage::Mandarin,
                "Mandarin Chinese (zh)",
            ),
            (
                LanguageChoice::French,
                TargetLanguage::French,
                "French (fr)",
            ),
            (
                LanguageChoice::German,
                TargetLanguage::German,
                "German (de)",
            ),
            (
                LanguageChoice::Italian,
                TargetLanguage::Italian,
                "Italian (it)",
            ),
            (
                LanguageChoice::Portuguese,
                TargetLanguage::Portuguese,
                "Portuguese (pt)",
            ),
            (
                LanguageChoice::Japanese,
                TargetLanguage::Japanese,
                "Japanese (ja)",
            ),
            (
                LanguageChoice::Korean,
                TargetLanguage::Korean,
                "Korean (ko)",
            ),
            (
                LanguageChoice::Russian,
                TargetLanguage::Russian,
                "Russian (ru)",
            ),
            (
                LanguageChoice::Arabic,
                TargetLanguage::Arabic,
                "Arabic (ar)",
            ),
            (LanguageChoice::Hindi, TargetLanguage::Hindi, "Hindi (hi)"),
            (
                LanguageChoice::English,
                TargetLanguage::English,
                "English (en)",
            ),
        ];

        assert_eq!(LanguageChoice::ALL.len(), 12);
        assert_eq!(LanguageChoice::list().len(), 12);
        for (choice, target, label) in expected {
            assert_eq!(TargetLanguage::from(choice), target);
            assert_eq!(choice.name(), label);
        }

        assert_eq!(
            TtsEngineKind::from(EngineChoice::CosyVoice3),
            TtsEngineKind::CosyVoice3
        );
        assert_eq!(
            EngineChoice::CosyVoice3.name(),
            "CosyVoice 3.0 (Best for ZH/JA/KO)"
        );
        assert_eq!(
            TtsEngineKind::from(EngineChoice::XttsV2),
            TtsEngineKind::XttsV2
        );
        assert_eq!(
            EngineChoice::XttsV2.name(),
            "XTTS-v2 (All 12 Languages incl. Spanish/French/German)"
        );
    }

    #[test]
    fn registers_all_four_slash_commands_with_user_and_guild_contexts() {
        let commands = configured_commands();
        let names: Vec<&str> = commands.iter().map(|cmd| cmd.name.as_ref()).collect();
        assert_eq!(
            names,
            vec![
                "speak",
                "voice_clone",
                "mandarin_voice",
                "voice_settings",
                "voice_profiles",
            ]
        );

        for cmd in &commands {
            assert_eq!(
                cmd.install_context,
                Some(vec![InstallationContext::User, InstallationContext::Guild]),
                "unexpected install_context on /{}",
                cmd.name
            );
            assert_eq!(
                cmd.interaction_context,
                Some(vec![
                    InteractionContext::Guild,
                    InteractionContext::BotDm,
                    InteractionContext::PrivateChannel,
                ]),
                "unexpected interaction_context on /{}",
                cmd.name
            );
        }

        let clone_params: Vec<&str> = commands[1]
            .parameters
            .iter()
            .map(|param| param.name.as_ref())
            .collect();
        assert_eq!(
            clone_params,
            vec![
                "language",
                "text",
                "audio",
                "profile",
                "voice_bubble",
                "private_only",
                "spoiler_phonetics",
            ]
        );
    }

    #[test]
    fn preserves_none_target_language_for_profile_defaults_and_resolves_safe_delivery() {
        let followup = resolve_slash_delivery(
            Some(InteractionContext::PrivateChannel),
            true,
            false,
            777,
            42,
            "tok-friend-dm".to_owned(),
            true,
        );
        assert!(matches!(
            followup,
            DiscordDeliveryTarget::UserInstallFollowup {
                application_id: 42,
                attach_spoiler_pinyin: true,
                ..
            }
        ));

        let bot_dm_bubble = resolve_slash_delivery(
            Some(InteractionContext::BotDm),
            true,
            false,
            888,
            42,
            "tok-bot-dm".to_owned(),
            true,
        );
        assert_eq!(
            bot_dm_bubble,
            DiscordDeliveryTarget::BotVoiceBubble {
                channel_id: 888,
                reply_to_message_id: None,
            }
        );

        let input = build_relay_input(RelayPayloadSpec {
            profile: None,
            target_language: None,
            audio_bytes: Vec::new(),
            source_text: Some("See you in five minutes.".to_owned()),
            delivery: followup,
        });
        assert_eq!(input.profile_slug, "default_voice");
        assert_eq!(input.target_language, None);
        assert_eq!(
            input.source_text.as_deref(),
            Some("See you in five minutes.")
        );
    }

    #[test]
    fn builds_profile_patch_and_formats_profile_management_summaries() {
        assert_eq!(resolve_valid_profile_slug(None).unwrap(), "default_voice");
        assert!(resolve_valid_profile_slug(Some("../bad")).is_err());

        let patch =
            build_profile_patch_input(Some(LanguageChoice::Spanish), Some(EngineChoice::XttsV2));
        assert_eq!(patch.avatar_url, AvatarPatch::Keep);
        assert_eq!(patch.default_target_language, Some(TargetLanguage::Spanish));
        assert_eq!(patch.preferred_engine, Some(TtsEngineKind::XttsV2));

        let empty_msg = format_profiles_summary(&[], 0);
        assert!(empty_msg.contains("No voice profiles are enrolled yet."));
        assert!(empty_msg.contains("default_voice"));

        let record = VoiceProfileRecord {
            id: VoiceProfileId::new_v7(),
            slug: "default_voice".to_owned(),
            display_name: "Alex".to_owned(),
            avatar_url: None,
            reference_wav_path: "/data/voice_profiles/default_voice.wav".to_owned(),
            reference_transcript_en: "Testing reference transcript.".to_owned(),
            preferred_engine: TtsEngineKind::CosyVoice3,
            default_target_language: TargetLanguage::Spanish,
            owner_discord_user_id: 123_456,
            tuning: domain::ports::SynthesisTuning::default(),
            finetune_status: "none".to_owned(),
            finetuned_checkpoint_dir: None,
            finetune_error: None,
            finetuned_at: None,
            samples: Vec::new(),
            sample_count: 1,
            total_sample_duration_ms: 15_000,
            created_at: SystemClock.now_utc(),
        };
        let updated_msg = format_updated_profile_reply(&record);
        assert!(updated_msg.contains("Spanish (`es`)"));
        assert!(updated_msg.contains("auto-routes to `xtts_v2`"));

        let list_msg = format_profiles_summary(&[record], 1);
        assert!(list_msg.contains("Enrolled voice profiles (**1** total):"));
        assert!(list_msg.contains("`default_voice`"));
    }

    fn sample_profile_and_receipt() -> (VoiceProfileRecord, VoiceJobReceipt) {
        let profile = VoiceProfileRecord {
            id: VoiceProfileId::new_v7(),
            slug: "default_voice".to_owned(),
            display_name: "Alex".to_owned(),
            avatar_url: None,
            reference_wav_path: "/data/voice_profiles/default_voice.wav".to_owned(),
            reference_transcript_en: "Reference line.".to_owned(),
            preferred_engine: TtsEngineKind::XttsV2,
            default_target_language: TargetLanguage::Spanish,
            owner_discord_user_id: 777_888,
            tuning: domain::ports::SynthesisTuning::default(),
            finetune_status: "none".to_owned(),
            finetuned_checkpoint_dir: None,
            finetune_error: None,
            finetuned_at: None,
            samples: Vec::new(),
            sample_count: 1,
            total_sample_duration_ms: 15_000,
            created_at: SystemClock.now_utc(),
        };
        let receipt = VoiceJobReceipt {
            job_id: VoiceJobId::new_v7(),
            profile_id: profile.id,
            profile_slug: "default_voice".to_owned(),
            target_language: TargetLanguage::Spanish,
            delivery_mode: "bot_voice_bubble".to_owned(),
            english_transcript: "Hey team".to_owned(),
            spoken_text: "¡Hola equipo!".to_owned(),
            spoken_hanzi: "¡Hola equipo!".to_owned(),
            phonetic_guide: "OH-lah eh-KEE-poh".to_owned(),
            pinyin: "OH-lah eh-KEE-poh".to_owned(),
            english_back_gloss: "Hey team!".to_owned(),
            back_transcribed_text: "¡Hola equipo!".to_owned(),
            back_transcribed_zh: "¡Hola equipo!".to_owned(),
            engine_used: TtsEngineKind::XttsV2,
            cer_bps: BasisPoints::from_raw(80),
            attempts_used: 1,
            duration_ms: 1_500,
            total_latency_ms: 620,
            output_ogg_path: "/data/voice_jobs/out.ogg".to_owned(),
            discord_message_id: Some(555_666),
            created_at: SystemClock.now_utc(),
        };
        (profile, receipt)
    }

    #[tokio::test]
    async fn relays_voice_jobs_and_profile_endpoints_with_bearer_and_owner_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (profile_record, job_receipt) = sample_profile_and_receipt();
        let paged_profiles = Paged::new(vec![profile_record.clone()], 1, PageParams::default());

        let responses = vec![
            serde_json::to_string(&profile_record).unwrap(),
            serde_json::to_string(&paged_profiles).unwrap(),
            serde_json::to_string(&job_receipt).unwrap(),
        ];

        let server = tokio::spawn(async move {
            let mut captured = Vec::new();
            for body in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = vec![0_u8; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                captured.push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(resp.as_bytes()).await.unwrap();
            }
            captured
        });

        let state = WorkerState {
            api_base_url: format!("http://{addr}"),
            bearer_token: Secret::new("owner-secret-token".to_owned()),
            application_id: 99,
            owner_user_id: 777_888,
            http: reqwest::Client::new(),
        };

        let patch =
            build_profile_patch_input(Some(LanguageChoice::Spanish), Some(EngineChoice::XttsV2));
        let updated = patch_profile_settings(&state, 777_888, "default_voice", &patch)
            .await
            .unwrap();
        assert_eq!(updated.default_target_language, TargetLanguage::Spanish);

        let listed = fetch_enrolled_profiles(&state, 777_888).await.unwrap();
        assert_eq!(listed.total, 1);
        assert_eq!(listed.items[0].slug, "default_voice");

        let relay_in = build_relay_input(RelayPayloadSpec {
            profile: None,
            target_language: None,
            audio_bytes: Vec::new(),
            source_text: Some("Hey team".to_owned()),
            delivery: DiscordDeliveryTarget::BotVoiceBubble {
                channel_id: 111_222,
                reply_to_message_id: Some(333_444),
            },
        });
        let receipt = relay_voice_job(&state, 777_888, &relay_in).await.unwrap();
        assert_eq!(receipt.discord_message_id, Some(555_666));

        let requests = server.await.unwrap();
        assert!(requests[0].starts_with("PATCH /api/v1/profiles/default_voice HTTP/1.1"));
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("authorization: bearer owner-secret-token")
        );
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("x-discord-user-id: 777888")
        );
        assert!(!requests[0].contains("avatar_url"));
        assert!(requests[1].starts_with("GET /api/v1/profiles?limit=20&offset=0 HTTP/1.1"));
        assert!(requests[2].starts_with("POST /api/v1/voice-jobs HTTP/1.1"));
        assert!(requests[2].contains("\"mode\":\"bot_voice_bubble\""));
    }
}
