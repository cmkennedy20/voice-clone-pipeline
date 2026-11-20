//! Polyglot 12-language catalog, `TTS` engine routing rules, and `ISO` code parsing.

use std::{fmt, str::FromStr};

use crate::translation::{error::TranslationDomainError, types::TtsEngineKind};

const COSYVOICE_AND_XTTS: [TtsEngineKind; 2] = [TtsEngineKind::CosyVoice3, TtsEngineKind::XttsV2];
const XTTS_ONLY: [TtsEngineKind; 1] = [TtsEngineKind::XttsV2];

/// Supported target spoken languages for zero-shot voice-cloned translation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum TargetLanguage {
    #[default]
    #[serde(rename = "zh")]
    Mandarin,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "it")]
    Italian,
    #[serde(rename = "pt")]
    Portuguese,
    #[serde(rename = "ja")]
    Japanese,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "ar")]
    Arabic,
    #[serde(rename = "hi")]
    Hindi,
    #[serde(rename = "en")]
    English,
}

/// Serializable descriptor for a [`TargetLanguage`] exposed to `API` clients and studio UIs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LanguageMetadata {
    pub code: String,
    pub english_name: String,
    pub native_name: String,
    pub whisper_code: String,
    pub xtts_code: String,
    pub supported_engines: Vec<TtsEngineKind>,
    pub recommended_engine: TtsEngineKind,
    pub phonetic_label: String,
    pub sample_prompt: String,
}

impl LanguageMetadata {
    /// Builds canonical metadata for `lang`.
    #[must_use]
    pub fn for_language(lang: TargetLanguage) -> Self {
        Self {
            code: lang.code().to_owned(),
            english_name: lang.english_name().to_owned(),
            native_name: lang.native_name().to_owned(),
            whisper_code: lang.whisper_code().to_owned(),
            xtts_code: lang.xtts_code().to_owned(),
            supported_engines: lang.supported_engines().to_vec(),
            recommended_engine: lang.recommended_engine(),
            phonetic_label: lang.phonetic_label().to_owned(),
            sample_prompt: lang.sample_prompt().to_owned(),
        }
    }
}

impl TargetLanguage {
    /// Complete ordered catalog of all `12` supported target languages.
    pub const ALL: [Self; 12] = [
        Self::Mandarin,
        Self::Spanish,
        Self::French,
        Self::German,
        Self::Italian,
        Self::Portuguese,
        Self::Japanese,
        Self::Korean,
        Self::Russian,
        Self::Arabic,
        Self::Hindi,
        Self::English,
    ];

    /// Canonical two-letter `ISO 639-1` code stored in `PostgreSQL` and wire `JSON`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Mandarin => "zh",
            Self::Spanish => "es",
            Self::French => "fr",
            Self::German => "de",
            Self::Italian => "it",
            Self::Portuguese => "pt",
            Self::Japanese => "ja",
            Self::Korean => "ko",
            Self::Russian => "ru",
            Self::Arabic => "ar",
            Self::Hindi => "hi",
            Self::English => "en",
        }
    }

    /// Human-readable English display name.
    #[must_use]
    pub const fn english_name(self) -> &'static str {
        match self {
            Self::Mandarin => "Mandarin",
            Self::Spanish => "Spanish",
            Self::French => "French",
            Self::German => "German",
            Self::Italian => "Italian",
            Self::Portuguese => "Portuguese",
            Self::Japanese => "Japanese",
            Self::Korean => "Korean",
            Self::Russian => "Russian",
            Self::Arabic => "Arabic",
            Self::Hindi => "Hindi",
            Self::English => "English",
        }
    }

    /// Native-script endonym for UI display and `LLM` prompting.
    #[must_use]
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::Mandarin => "普通话",
            Self::Spanish => "Español",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Italian => "Italiano",
            Self::Portuguese => "Português",
            Self::Japanese => "日本語",
            Self::Korean => "한국어",
            Self::Russian => "Русский",
            Self::Arabic => "العربية",
            Self::Hindi => "हिन्दी",
            Self::English => "English",
        }
    }

    /// `Whisper` (`ggml-large-v3`) language token code for reverse-`ASR` verification.
    #[must_use]
    pub const fn whisper_code(self) -> &'static str {
        self.code()
    }

    /// `XTTS-v2` language identifier (`"zh-cn"` for Mandarin, `ISO 639-1` for all others).
    #[must_use]
    pub const fn xtts_code(self) -> &'static str {
        match self {
            Self::Mandarin => "zh-cn",
            other => other.code(),
        }
    }

    /// Zero-shot `TTS` engines capable of synthesizing `self`.
    #[must_use]
    pub const fn supported_engines(self) -> &'static [TtsEngineKind] {
        match self {
            Self::Mandarin | Self::Japanese | Self::Korean | Self::English => &COSYVOICE_AND_XTTS,
            Self::Spanish
            | Self::French
            | Self::German
            | Self::Italian
            | Self::Portuguese
            | Self::Russian
            | Self::Arabic
            | Self::Hindi => &XTTS_ONLY,
        }
    }

    /// Highest-fidelity default `TTS` engine for `self`.
    #[must_use]
    pub const fn recommended_engine(self) -> TtsEngineKind {
        match self {
            Self::Mandarin | Self::Japanese | Self::Korean => TtsEngineKind::CosyVoice3,
            Self::Spanish
            | Self::French
            | Self::German
            | Self::Italian
            | Self::Portuguese
            | Self::Russian
            | Self::Arabic
            | Self::Hindi
            | Self::English => TtsEngineKind::XttsV2,
        }
    }

    /// Label describing the romanization or pronunciation guide returned for `self`.
    #[must_use]
    pub const fn phonetic_label(self) -> &'static str {
        match self {
            Self::Mandarin => "Pinyin",
            Self::Japanese => "Romaji",
            Self::Korean => "Revised Romanization",
            Self::Russian | Self::Arabic => "Romanization",
            Self::Hindi => "IAST",
            Self::Spanish
            | Self::French
            | Self::German
            | Self::Italian
            | Self::Portuguese
            | Self::English => "Pronunciation",
        }
    }

    /// Representative English conversational prompt for quick studio testing.
    #[must_use]
    pub const fn sample_prompt(self) -> &'static str {
        match self {
            Self::Mandarin => "We are having a team meeting at 3:30 this afternoon.",
            Self::Spanish => "Hey team, see you in five minutes for the launch review.",
            Self::French => "Good morning everyone, our meeting starts this afternoon.",
            Self::German => "Let us review the deployment checklist before noon.",
            Self::Italian => "Thanks for joining, we are ready to ship the update.",
            Self::Portuguese => "Hello friends, let us connect after lunch today.",
            Self::Japanese => "Thank you for your hard work on today's release.",
            Self::Korean => "Let us check the server latency together right now.",
            Self::Russian => "Everything looks stable, we can proceed with testing.",
            Self::Arabic => "Welcome everyone, glad to have you on the voice channel.",
            Self::Hindi => "Let us wrap up the sprint review in ten minutes.",
            Self::English => "Voice cloning timbre check across local studio channels.",
        }
    }

    /// Selects `engine_override` if supported for `self`, else `preferred` if supported,
    /// falling back to [`Self::recommended_engine`].
    #[must_use]
    pub fn resolve_engine(
        self,
        preferred: TtsEngineKind,
        engine_override: Option<TtsEngineKind>,
    ) -> TtsEngineKind {
        if let Some(eng) = engine_override
            && self.supported_engines().contains(&eng)
        {
            return eng;
        }
        if self.supported_engines().contains(&preferred) {
            return preferred;
        }
        self.recommended_engine()
    }

    /// Returns serializable [`LanguageMetadata`] for `self`.
    #[must_use]
    pub fn metadata(self) -> LanguageMetadata {
        LanguageMetadata::for_language(self)
    }

    /// Returns serializable [`LanguageMetadata`] for all `12` supported languages.
    #[must_use]
    pub fn all_metadata() -> Vec<LanguageMetadata> {
        Self::ALL.into_iter().map(Self::metadata).collect()
    }
}

impl fmt::Display for TargetLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl FromStr for TargetLanguage {
    type Err = TranslationDomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "zh" | "zh-cn" | "zh_cn" | "zh-tw" | "mandarin" | "chinese" => Ok(Self::Mandarin),
            "es" | "es-es" | "es_es" | "es-mx" | "es_mx" | "spanish" | "espanol" => {
                Ok(Self::Spanish)
            }
            "fr" | "fr-fr" | "fr_fr" | "fr-ca" | "french" | "francais" => Ok(Self::French),
            "de" | "de-de" | "de_de" | "german" | "deutsch" => Ok(Self::German),
            "it" | "it-it" | "it_it" | "italian" | "italiano" => Ok(Self::Italian),
            "pt" | "pt-br" | "pt_br" | "pt-pt" | "portuguese" | "portugues" => Ok(Self::Portuguese),
            "ja" | "ja-jp" | "ja_jp" | "jp" | "japanese" => Ok(Self::Japanese),
            "ko" | "ko-kr" | "ko_kr" | "kr" | "korean" => Ok(Self::Korean),
            "ru" | "ru-ru" | "ru_ru" | "russian" => Ok(Self::Russian),
            "ar" | "ar-sa" | "ar_sa" | "arabic" => Ok(Self::Arabic),
            "hi" | "hi-in" | "hi_in" | "hindi" => Ok(Self::Hindi),
            "en" | "en-us" | "en_us" | "en-gb" | "english" => Ok(Self::English),
            _ => Err(TranslationDomainError::UnsupportedLanguage(
                s.trim().to_owned(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::TargetLanguage;
    use crate::translation::{error::TranslationDomainError, types::TtsEngineKind};

    #[test]
    fn parses_all_twelve_iso_codes_and_aliases_case_insensitively() {
        assert_eq!(TargetLanguage::ALL.len(), 12);
        assert_eq!(
            TargetLanguage::from_str("ZH-CN").unwrap(),
            TargetLanguage::Mandarin
        );
        assert_eq!(
            TargetLanguage::from_str("Spanish").unwrap(),
            TargetLanguage::Spanish
        );
        assert_eq!(
            TargetLanguage::from_str("es-MX").unwrap(),
            TargetLanguage::Spanish
        );
        assert_eq!(
            TargetLanguage::from_str("French").unwrap(),
            TargetLanguage::French
        );
        assert_eq!(
            TargetLanguage::from_str("Japanese").unwrap(),
            TargetLanguage::Japanese
        );
        assert_eq!(
            TargetLanguage::from_str("tlh"),
            Err(TranslationDomainError::UnsupportedLanguage("tlh".into()))
        );
    }

    #[test]
    fn resolves_compatible_engine_falling_back_when_cosyvoice_unsupported() {
        // Spanish only supports XTTS-v2, so CosyVoice3 profile preference or override falls back to XttsV2
        assert_eq!(
            TargetLanguage::Spanish.resolve_engine(TtsEngineKind::CosyVoice3, None),
            TtsEngineKind::XttsV2
        );
        assert_eq!(
            TargetLanguage::Spanish
                .resolve_engine(TtsEngineKind::CosyVoice3, Some(TtsEngineKind::CosyVoice3)),
            TtsEngineKind::XttsV2
        );
        // French behaves identically
        assert_eq!(
            TargetLanguage::French.resolve_engine(TtsEngineKind::CosyVoice3, None),
            TtsEngineKind::XttsV2
        );
        // Japanese & Mandarin support both CosyVoice3 and XttsV2
        assert_eq!(
            TargetLanguage::Japanese.resolve_engine(TtsEngineKind::CosyVoice3, None),
            TtsEngineKind::CosyVoice3
        );
        assert_eq!(
            TargetLanguage::Japanese
                .resolve_engine(TtsEngineKind::CosyVoice3, Some(TtsEngineKind::XttsV2)),
            TtsEngineKind::XttsV2
        );
        assert_eq!(
            TargetLanguage::Mandarin.resolve_engine(TtsEngineKind::XttsV2, None),
            TtsEngineKind::XttsV2
        );
        assert_eq!(TargetLanguage::Mandarin.xtts_code(), "zh-cn");
        assert_eq!(TargetLanguage::Spanish.xtts_code(), "es");
    }
}
