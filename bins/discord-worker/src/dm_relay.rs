//! Conversational 1-on-1 `Discord` `Bot` `DM` voice & text listener (`flags: 8192` waveform relay).

use std::str::FromStr;

use domain::{ports::DiscordDeliveryTarget, translation::types::TargetLanguage};
use features::voice_jobs::translate_clip::VoiceJobReceipt;
use framework::auth::owner_token::verify_discord_owner;
use serenity::all::{Context, FullEvent, Message};

use crate::commands::{
    RelayPayloadSpec, WorkerError, WorkerState, build_relay_input, download_attachment_bytes,
    relay_voice_job,
};

const AUDIO_EXTENSIONS: [&str; 10] = [
    ".ogg", ".oga", ".opus", ".wav", ".mp3", ".m4a", ".flac", ".aac", ".webm", ".mp4",
];

/// Parses a 1-on-1 `DM` text message into an optional [`TargetLanguage`] override and prompt body.
///
/// Supports `"es: Hola..."`, `"[es] Hola..."`, and `"spanish: Hola..."` prefixes while leaving
/// `URL` schemes (such as `"https://..."`) and plain English sentences untouched.
#[must_use]
pub fn parse_dm_text_prompt(raw: &str) -> (Option<TargetLanguage>, String) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, String::new());
    }

    if let Some(after_open) = trimmed.strip_prefix('[')
        && let Some((tag, after_close)) = after_open.split_once(']')
    {
        let remainder = after_close
            .trim_start()
            .strip_prefix(':')
            .unwrap_or(after_close)
            .trim();
        if !remainder.is_empty()
            && let Ok(lang) = TargetLanguage::from_str(tag)
        {
            return (Some(lang), remainder.to_owned());
        }
    }

    if let Some((prefix, after_colon)) = trimmed.split_once(':')
        && !after_colon.starts_with("//")
    {
        let remainder = after_colon.trim();
        if !remainder.is_empty()
            && let Ok(lang) = TargetLanguage::from_str(prefix)
        {
            return (Some(lang), remainder.to_owned());
        }
    }

    (None, trimmed.to_owned())
}

/// Extracts an optional [`TargetLanguage`] hint from message text accompanying a `DM` audio clip.
#[must_use]
pub fn parse_dm_audio_language_hint(raw: &str) -> Option<TargetLanguage> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_colon = trimmed.trim_end_matches(':').trim();
    let candidate = without_colon
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(without_colon);
    if let Ok(lang) = TargetLanguage::from_str(candidate) {
        return Some(lang);
    }

    parse_dm_text_prompt(trimmed).0
}

/// Returns `true` when attachment `MIME` metadata or filename indicates an audio/voice clip.
#[must_use]
pub fn is_audio_attachment_meta(content_type: Option<&str>, filename: &str) -> bool {
    if let Some(mime) = content_type {
        let base_mime = mime
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if base_mime.starts_with("audio/")
            || base_mime == "application/ogg"
            || base_mime == "video/ogg"
        {
            return true;
        }
    }

    let lower_name = filename.trim().to_ascii_lowercase();
    AUDIO_EXTENSIONS.iter().any(|ext| lower_name.ends_with(ext))
}

/// Formats the companion caption posted with a voice clip using pure target-language script only.
#[must_use]
pub fn format_voice_bubble_caption(receipt: &VoiceJobReceipt) -> String {
    receipt.spoken_text.clone()
}

/// Strips leading or inline `<@bot_id>` / `<@!bot_id>` mention tokens from message text.
#[must_use]
pub fn strip_bot_mention_tokens(raw: &str, bot_user_id: u64) -> String {
    if bot_user_id == 0 {
        return raw.trim().to_owned();
    }
    let plain = format!("<@{bot_user_id}>");
    let nick = format!("<@!{bot_user_id}>");
    raw.replace(&plain, " ")
        .replace(&nick, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

async fn relay_direct_message(
    ctx: &Context,
    new_message: &Message,
    state: &WorkerState,
) -> Result<(), WorkerError> {
    let caller_id = new_message.author.id.get();
    if new_message.author.bot || verify_discord_owner(caller_id, state.owner_user_id).is_err() {
        return Ok(());
    }

    let current_bot_id = ctx.cache.current_user().id.get();
    let bot_id = if current_bot_id != 0 {
        current_bot_id
    } else {
        state.application_id
    };
    let is_dm = new_message.guild_id.is_none();
    let mentions_bot = bot_id != 0 && new_message.mentions.iter().any(|u| u.id.get() == bot_id);
    if !is_dm && !mentions_bot {
        return Ok(());
    }

    let cleaned_content = if mentions_bot {
        strip_bot_mention_tokens(&new_message.content, bot_id)
    } else {
        new_message.content.clone()
    };

    let audio_attachment = new_message
        .attachments
        .iter()
        .find(|att| is_audio_attachment_meta(att.content_type.as_deref(), &att.filename));

    let outcome = async {
        let (target_language, audio_bytes, source_text) = if let Some(att) = audio_attachment {
            let _ = new_message.channel_id.broadcast_typing(&ctx.http).await;
            let bytes = download_attachment_bytes(&state.http, &att.url).await?;
            let lang = parse_dm_audio_language_hint(&cleaned_content);
            (lang, bytes, None)
        } else {
            let (lang, prompt) = parse_dm_text_prompt(&cleaned_content);
            if prompt.is_empty() {
                return Ok(None);
            }
            let _ = new_message.channel_id.broadcast_typing(&ctx.http).await;
            (lang, Vec::new(), Some(prompt))
        };

        let input = build_relay_input(RelayPayloadSpec {
            profile: None,
            target_language,
            audio_bytes,
            source_text,
            delivery: DiscordDeliveryTarget::BotVoiceBubble {
                channel_id: new_message.channel_id.get(),
                reply_to_message_id: Some(new_message.id.get()),
            },
        });
        let receipt = relay_voice_job(state, caller_id, &input).await?;
        Ok::<Option<VoiceJobReceipt>, WorkerError>(Some(receipt))
    }
    .await;

    match outcome {
        Ok(None) => {}
        Ok(Some(receipt)) => {
            let caption = format_voice_bubble_caption(&receipt);
            let _ = new_message.channel_id.say(&ctx.http, caption).await;
            tracing::info!(
                job_id = %receipt.job_id,
                lang = %receipt.target_language,
                engine = %receipt.engine_used,
                cer_bps = receipt.cer_bps.value(),
                latency_ms = receipt.total_latency_ms,
                "completed 1-on-1 DM native voice bubble relay"
            );
        }
        Err(err) => {
            tracing::warn!(error = %err, "1-on-1 DM voice relay failed");
            let _ = new_message
                .reply(&ctx.http, format!("Voice synthesis failed: {err}"))
                .await;
        }
    }
    Ok(())
}

/// `Poise` gateway event callback routing owner 1-on-1 `Bot` `DM` messages into `api-server`.
pub async fn handle_gateway_event(
    ctx: &Context,
    event: &FullEvent,
    state: &WorkerState,
) -> Result<(), WorkerError> {
    if let FullEvent::Message { new_message } = event {
        relay_direct_message(ctx, new_message, state).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use common::{
        basis_points::BasisPoints,
        clock::{Clock, SystemClock},
    };
    use domain::translation::types::{TargetLanguage, TtsEngineKind};
    use features::{
        profiles::VoiceProfileId, shared_queries::profile_loader::VoiceJobId,
        voice_jobs::translate_clip::VoiceJobReceipt,
    };

    use super::{
        format_voice_bubble_caption, is_audio_attachment_meta, parse_dm_audio_language_hint,
        parse_dm_text_prompt,
    };

    #[test]
    fn parses_dm_text_language_prefixes_without_misparsing_urls_or_plain_sentences() {
        assert_eq!(
            parse_dm_text_prompt("es: Hola friend"),
            (Some(TargetLanguage::Spanish), "Hola friend".to_owned())
        );
        assert_eq!(
            parse_dm_text_prompt("zh: Let's eat"),
            (Some(TargetLanguage::Mandarin), "Let's eat".to_owned())
        );
        assert_eq!(
            parse_dm_text_prompt("[fr] Good morning team"),
            (Some(TargetLanguage::French), "Good morning team".to_owned())
        );
        assert_eq!(
            parse_dm_text_prompt("spanish: Hey team, five minutes!"),
            (
                Some(TargetLanguage::Spanish),
                "Hey team, five minutes!".to_owned()
            )
        );
        assert_eq!(
            parse_dm_text_prompt("URL https://example.com"),
            (None, "URL https://example.com".to_owned())
        );
        assert_eq!(
            parse_dm_text_prompt("https://example.com/path"),
            (None, "https://example.com/path".to_owned())
        );
        assert_eq!(
            parse_dm_text_prompt("Meeting at 3pm"),
            (None, "Meeting at 3pm".to_owned())
        );
        assert_eq!(parse_dm_text_prompt("es:   "), (None, "es:".to_owned()));
    }

    #[test]
    fn parses_dm_audio_caption_language_hints_and_detects_voice_attachments() {
        assert_eq!(parse_dm_audio_language_hint(""), None);
        assert_eq!(
            parse_dm_audio_language_hint("es"),
            Some(TargetLanguage::Spanish)
        );
        assert_eq!(
            parse_dm_audio_language_hint("es:"),
            Some(TargetLanguage::Spanish)
        );
        assert_eq!(
            parse_dm_audio_language_hint("[ja]"),
            Some(TargetLanguage::Japanese)
        );
        assert_eq!(
            parse_dm_audio_language_hint("de: quick note"),
            Some(TargetLanguage::German)
        );

        assert!(is_audio_attachment_meta(
            Some("audio/ogg; codecs=opus"),
            "voice-message.ogg"
        ));
        assert!(is_audio_attachment_meta(
            Some("application/octet-stream"),
            "recording.m4a"
        ));
        assert!(is_audio_attachment_meta(None, "sample.wav"));
        assert!(!is_audio_attachment_meta(
            Some("image/png"),
            "screenshot.png"
        ));
    }

    #[test]
    fn formats_spoiler_voice_bubble_caption_with_spoken_text_phonetics_and_cer() {
        let receipt = VoiceJobReceipt {
            job_id: VoiceJobId::new_v7(),
            profile_id: VoiceProfileId::new_v7(),
            profile_slug: "default_voice".to_owned(),
            target_language: TargetLanguage::Spanish,
            delivery_mode: "bot_voice_bubble".to_owned(),
            english_transcript: "Hello friend".to_owned(),
            spoken_text: "¡Hola amigo!".to_owned(),
            spoken_hanzi: "¡Hola amigo!".to_owned(),
            phonetic_guide: "OH-lah ah-MEE-goh".to_owned(),
            pinyin: "OH-lah ah-MEE-goh".to_owned(),
            english_back_gloss: "Hello friend!".to_owned(),
            back_transcribed_text: "¡Hola amigo!".to_owned(),
            back_transcribed_zh: "¡Hola amigo!".to_owned(),
            engine_used: TtsEngineKind::XttsV2,
            cer_bps: BasisPoints::from_raw(125),
            attempts_used: 1,
            duration_ms: 1_800,
            total_latency_ms: 940,
            output_ogg_path: "/tmp/out.ogg".to_owned(),
            discord_message_id: Some(999),
            created_at: SystemClock.now_utc(),
        };

        assert_eq!(format_voice_bubble_caption(&receipt), "¡Hola amigo!");
    }
}
