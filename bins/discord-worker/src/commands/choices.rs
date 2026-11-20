//! Native `Discord` dropdown choice enums (`LanguageChoice` and `EngineChoice`).

use domain::translation::types::{TargetLanguage, TtsEngineKind};

/// Native 12-language `Discord` slash command dropdown mapped 1-to-1 to [`TargetLanguage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum LanguageChoice {
    #[name = "Spanish (es)"]
    Spanish,
    #[name = "Mandarin Chinese (zh)"]
    Mandarin,
    #[name = "French (fr)"]
    French,
    #[name = "German (de)"]
    German,
    #[name = "Italian (it)"]
    Italian,
    #[name = "Portuguese (pt)"]
    Portuguese,
    #[name = "Japanese (ja)"]
    Japanese,
    #[name = "Korean (ko)"]
    Korean,
    #[name = "Russian (ru)"]
    Russian,
    #[name = "Arabic (ar)"]
    Arabic,
    #[name = "Hindi (hi)"]
    Hindi,
    #[name = "English (en)"]
    English,
}

impl LanguageChoice {
    /// Complete ordered list of all 12 dropdown choices.
    pub const ALL: [Self; 12] = [
        Self::Spanish,
        Self::Mandarin,
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

    /// Converts this `Discord` dropdown choice into the domain [`TargetLanguage`].
    #[must_use]
    pub const fn target_language(self) -> TargetLanguage {
        match self {
            Self::Spanish => TargetLanguage::Spanish,
            Self::Mandarin => TargetLanguage::Mandarin,
            Self::French => TargetLanguage::French,
            Self::German => TargetLanguage::German,
            Self::Italian => TargetLanguage::Italian,
            Self::Portuguese => TargetLanguage::Portuguese,
            Self::Japanese => TargetLanguage::Japanese,
            Self::Korean => TargetLanguage::Korean,
            Self::Russian => TargetLanguage::Russian,
            Self::Arabic => TargetLanguage::Arabic,
            Self::Hindi => TargetLanguage::Hindi,
            Self::English => TargetLanguage::English,
        }
    }
}

impl From<LanguageChoice> for TargetLanguage {
    fn from(choice: LanguageChoice) -> Self {
        choice.target_language()
    }
}

/// Native `Discord` dropdown for selecting a zero-shot `TTS` voice cloning engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum EngineChoice {
    #[name = "CosyVoice 3.0 (Best for ZH/JA/KO)"]
    CosyVoice3,
    #[name = "XTTS-v2 (All 12 Languages incl. Spanish/French/German)"]
    XttsV2,
}

impl EngineChoice {
    /// Converts this `Discord` dropdown choice into the domain [`TtsEngineKind`].
    #[must_use]
    pub const fn engine_kind(self) -> TtsEngineKind {
        match self {
            Self::CosyVoice3 => TtsEngineKind::CosyVoice3,
            Self::XttsV2 => TtsEngineKind::XttsV2,
        }
    }
}

impl From<EngineChoice> for TtsEngineKind {
    fn from(choice: EngineChoice) -> Self {
        choice.engine_kind()
    }
}
