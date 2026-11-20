//! Domain error variants for spoken scripts, target languages, and `TTS` engine identifiers.

/// Validation errors for [`crate::translation::types::MandarinSpokenScript`],
/// [`crate::translation::language::TargetLanguage`], and [`crate::translation::types::TtsEngineKind`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranslationDomainError {
    #[error("spoken script cannot be empty")]
    EmptyScript,

    #[error("spoken script contains unexpanded digit '{ch}'")]
    ContainsUnexpandedDigit { ch: char },

    #[error("spoken script contains forbidden unexpanded symbol '{ch}'")]
    ContainsForbiddenSymbol { ch: char },

    #[error("Mandarin spoken script must contain at least one CJK Hanzi ideograph")]
    MissingHanzi,

    #[error("spoken script for language '{language}' must contain at least one alphabetic letter")]
    MissingSpokenLetters { language: &'static str },

    #[error("unknown TTS engine '{0}'")]
    UnknownTtsEngine(String),

    #[error("unsupported target language '{0}'")]
    UnsupportedLanguage(String),
}
