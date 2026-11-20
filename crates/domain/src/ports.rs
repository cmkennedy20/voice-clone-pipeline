//! Narrow external capability ports and Discord delivery target models.

use common::{basis_points::BasisPoints, secret::Secret};
use serde::{Deserialize, Serialize};

pub mod finetune;

pub use crate::translation::{
    language::{LanguageMetadata, TargetLanguage},
    tuning::SynthesisTuning,
    types::SpokenScript,
};
use crate::{
    audio::types::{Pcm16kMono, Pcm24kMono, RawAudioMetrics, RenderedAudioBundle},
    translation::types::{MandarinSpokenScript, TtsEngineKind},
};
pub use finetune::{FinetuneJobProgress, FinetuneStartRequest, ReferenceSampleRef, VoiceFinetuner};

/// Compliant Discord delivery modes supporting personal-account audio bridges and bot/app flows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum DiscordDeliveryTarget {
    /// Mode A: Returns raw `.ogg` / `48 kHz` `.wav` bytes directly to `BlackHole`, `VB-Cable`,
    /// or `iOS` Shortcut so playback/upload happens from the user's real Discord client.
    ClientReturnOnly,
    /// Mode B: Responds to a `USER_INSTALL` interaction (`contexts = [0, 1, 2]`)
    /// usable inside private 1:1 DMs, Group DMs, or external servers without adding a bot.
    UserInstallFollowup {
        #[serde(with = "common::snowflake::u64_or_string")]
        application_id: u64,
        interaction_token: Secret<String>,
        attach_spoiler_pinyin: bool,
    },
    /// Mode C1: Posts via channel webhook with custom `username` and `avatar_url`.
    ProfileWebhook {
        webhook_url: Secret<String>,
        username: String,
        avatar_url: Option<String>,
    },
    /// Mode C2: Posts a native `IS_VOICE_MESSAGE` (`flags: 8192`) waveform bubble via Bot `REST` `API`.
    BotVoiceBubble {
        #[serde(with = "common::snowflake::u64_or_string")]
        channel_id: u64,
        #[serde(default, with = "common::snowflake::option_u64_or_string")]
        reply_to_message_id: Option<u64>,
    },
}

impl DiscordDeliveryTarget {
    /// Returns the `snake_case` delivery mode slug matching the `voice_jobs.delivery_mode` schema.
    #[must_use]
    pub const fn mode_slug(&self) -> &'static str {
        match self {
            Self::ClientReturnOnly => "client_return_only",
            Self::UserInstallFollowup { .. } => "user_install_followup",
            Self::ProfileWebhook { .. } => "profile_webhook",
            Self::BotVoiceBubble { .. } => "bot_voice_bubble",
        }
    }
}

/// Structured `LLM` translation output containing spoken script, phonetic guide, and English gloss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslatedMandarin {
    pub spoken_hanzi: MandarinSpokenScript,
    pub pinyin: String,
    pub english_back_gloss: String,
}

/// Language-neutral alias for [`TranslatedMandarin`].
pub type TranslatedScript = TranslatedMandarin;

impl TranslatedMandarin {
    /// Returns the validated spoken script text in the target language.
    #[must_use]
    pub fn spoken_text(&self) -> &str {
        self.spoken_hanzi.as_str()
    }

    /// Returns the romanization or phonetic pronunciation guide.
    #[must_use]
    pub fn phonetic_guide(&self) -> &str {
        &self.pinyin
    }
}

/// Parameters for a single zero-shot or fine-tuned voice-cloning synthesis pass.
#[derive(Debug, Clone, Copy)]
pub struct VoiceSynthesisRequest<'a> {
    pub profile_slug: &'a str,
    pub reference_wav_path: &'a str,
    pub reference_transcript_en: &'a str,
    pub reference_samples: &'a [ReferenceSampleRef],
    pub engine: TtsEngineKind,
    pub target_language: TargetLanguage,
    pub script: &'a SpokenScript,
    pub seed: u64,
    pub tuning: SynthesisTuning,
    pub finetuned_checkpoint_dir: Option<&'a str>,
}

/// Adapter-boundary failures returned by external `DSP`, `GPU` inference, and Discord ports.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExternalPortError {
    #[error("audio codec failure: {0}")]
    AudioCodec(String),
    #[error("ASR inference failure: {0}")]
    Asr(String),
    #[error("LLM translation failure: {0}")]
    Translation(String),
    #[error("TTS sidecar synthesis failure: {0}")]
    Tts(String),
    #[error("Discord publish failure: {0}")]
    Discord(String),
}

/// Synchronous `DSP` demux/`VAD` measurement and `48 kHz` `Opus`/`WAV` encoding port.
pub trait AudioCodecEngine: Send + Sync {
    fn decode_and_measure(
        &self,
        raw_bytes: &[u8],
    ) -> Result<(Pcm16kMono, RawAudioMetrics), ExternalPortError>;

    fn encode_bundle(&self, pcm_24k: &Pcm24kMono)
    -> Result<RenderedAudioBundle, ExternalPortError>;
}

/// Asynchronous speech-to-text port for English input `ASR` and multilingual reverse-`ASR` verification.
#[async_trait::async_trait]
pub trait SpeechRecognizer: Send + Sync {
    async fn transcribe_english(&self, pcm: &Pcm16kMono) -> Result<String, ExternalPortError>;

    async fn transcribe_mandarin(&self, pcm: &Pcm24kMono) -> Result<String, ExternalPortError> {
        self.transcribe_in_language(pcm, TargetLanguage::Mandarin)
            .await
    }

    async fn transcribe_in_language(
        &self,
        pcm: &Pcm24kMono,
        lang: TargetLanguage,
    ) -> Result<String, ExternalPortError> {
        match lang {
            TargetLanguage::Mandarin => self.transcribe_mandarin(pcm).await,
            other => Err(ExternalPortError::Asr(format!(
                "reverse-ASR not configured for '{}'",
                other.code()
            ))),
        }
    }
}

/// Asynchronous English-to-target-language spoken translation port.
#[async_trait::async_trait]
pub trait MandarinTranslator: Send + Sync {
    async fn translate_to_spoken_language(
        &self,
        english: &str,
        target_language: TargetLanguage,
    ) -> Result<TranslatedMandarin, ExternalPortError>;

    async fn translate_english_to_spoken_zh(
        &self,
        english: &str,
    ) -> Result<TranslatedMandarin, ExternalPortError> {
        self.translate_to_spoken_language(english, TargetLanguage::Mandarin)
            .await
    }
}

/// Asynchronous zero-shot multilingual voice cloning port.
#[async_trait::async_trait]
pub trait VoiceSynthesizer: Send + Sync {
    async fn synthesize_cloned_voice(
        &self,
        request: VoiceSynthesisRequest<'_>,
    ) -> Result<Pcm24kMono, ExternalPortError>;

    async fn synthesize_cloned_zh(
        &self,
        profile_slug: &str,
        reference_wav_path: &str,
        reference_transcript_en: &str,
        engine: TtsEngineKind,
        script: &MandarinSpokenScript,
        seed: u64,
    ) -> Result<Pcm24kMono, ExternalPortError> {
        self.synthesize_cloned_voice(VoiceSynthesisRequest {
            profile_slug,
            reference_wav_path,
            reference_transcript_en,
            reference_samples: &[],
            engine,
            target_language: TargetLanguage::Mandarin,
            script,
            seed,
            tuning: SynthesisTuning::default(),
            finetuned_checkpoint_dir: None,
        })
        .await
    }
}

/// Asynchronous Discord multi-mode voice delivery port.
#[async_trait::async_trait]
pub trait DiscordPublisher: Send + Sync {
    async fn publish(
        &self,
        target: &DiscordDeliveryTarget,
        bundle: &RenderedAudioBundle,
        translated: &TranslatedMandarin,
    ) -> Result<Option<u64>, ExternalPortError>;
}

/// Asynchronous speaker cosine-similarity port (`ECAPA-TDNN` / cepstral) used by `Layer 7` (`M4`).
#[async_trait::async_trait]
pub trait SpeakerSimilarityScorer: Send + Sync {
    /// Scores cosine speaker similarity in [`BasisPoints`] (`0..=10_000`) between an enrolled
    /// reference `WAV` file and synthesized candidate `WAV` bytes.
    async fn score_similarity_bps(
        &self,
        reference_wav_path: &str,
        candidate_wav_bytes: &[u8],
    ) -> Result<BasisPoints, ExternalPortError>;
}

#[cfg(test)]
mod tests {
    use common::secret::Secret;

    use super::{DiscordDeliveryTarget, SpokenScript, TargetLanguage, TranslatedMandarin};

    #[test]
    fn delivery_target_debug_redacts_embedded_secrets_and_round_trips_serde() {
        let target = DiscordDeliveryTarget::UserInstallFollowup {
            application_id: 123_456,
            interaction_token: Secret::new("interaction-secret-xyz".to_owned()),
            attach_spoiler_pinyin: true,
        };

        let dbg_str = format!("{target:?}");
        assert!(!dbg_str.contains("interaction-secret-xyz"));
        assert!(dbg_str.contains("[REDACTED]"));
        assert_eq!(target.mode_slug(), "user_install_followup");

        let json = serde_json::to_string(&target).unwrap();
        let decoded: DiscordDeliveryTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, target);
    }

    #[test]
    fn delivery_target_accepts_19_digit_snowflake_strings_and_u64_and_null() {
        const SNOWFLAKE: u64 = 1_234_567_890_123_456_789_u64;

        let followup_from_str: DiscordDeliveryTarget = serde_json::from_str(
            r#"{
                "mode": "user_install_followup",
                "application_id": "1234567890123456789",
                "interaction_token": "tok-abc",
                "attach_spoiler_pinyin": false
            }"#,
        )
        .unwrap();
        assert_eq!(
            followup_from_str,
            DiscordDeliveryTarget::UserInstallFollowup {
                application_id: SNOWFLAKE,
                interaction_token: Secret::new("tok-abc".to_owned()),
                attach_spoiler_pinyin: false,
            }
        );

        let bubble_from_str: DiscordDeliveryTarget = serde_json::from_str(
            r#"{
                "mode": "bot_voice_bubble",
                "channel_id": "1234567890123456789",
                "reply_to_message_id": "9876543210987654321"
            }"#,
        )
        .unwrap();
        assert_eq!(
            bubble_from_str,
            DiscordDeliveryTarget::BotVoiceBubble {
                channel_id: SNOWFLAKE,
                reply_to_message_id: Some(9_876_543_210_987_654_321_u64),
            }
        );

        let bubble_null_reply: DiscordDeliveryTarget = serde_json::from_str(
            r#"{
                "mode": "bot_voice_bubble",
                "channel_id": 1234567890123456789,
                "reply_to_message_id": null
            }"#,
        )
        .unwrap();
        assert_eq!(
            bubble_null_reply,
            DiscordDeliveryTarget::BotVoiceBubble {
                channel_id: SNOWFLAKE,
                reply_to_message_id: None,
            }
        );
    }

    #[test]
    fn translated_script_accessors_expose_spoken_text_and_phonetic_guide() {
        let script =
            SpokenScript::parse_for_language("Hola equipo", TargetLanguage::Spanish).unwrap();
        let translated = TranslatedMandarin {
            spoken_hanzi: script,
            pinyin: "OH-lah eh-KEE-poh".to_owned(),
            english_back_gloss: "Hello team".to_owned(),
        };
        assert_eq!(translated.spoken_text(), "Hola equipo");
        assert_eq!(translated.phonetic_guide(), "OH-lah eh-KEE-poh");
    }
}
