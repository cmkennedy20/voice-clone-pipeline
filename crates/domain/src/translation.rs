//! Multilingual spoken-script guards, target language catalog, `TTS` engine routing, and `CER` verification.

pub mod error;
pub mod language;
pub mod numerals;
pub mod tuning;
pub mod types;
pub mod verifier;

pub use language::{LanguageMetadata, TargetLanguage};
pub use numerals::expand_english_numerals_for_tts;
pub use tuning::SynthesisTuning;
pub use types::{MandarinSpokenScript, SpokenScript, TtsEngineKind};
