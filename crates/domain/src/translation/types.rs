//! Validated `MandarinSpokenScript` (`SpokenScript`) newtype and `TtsEngineKind` enumeration.

use std::{fmt, str::FromStr};

use crate::translation::error::TranslationDomainError;
pub use crate::translation::language::{LanguageMetadata, TargetLanguage};

const FORBIDDEN_UNEXPANDED_SYMBOLS: [char; 12] = [
    '$', '%', '€', '£', '¥', '￥', '#', '@', '+', '=', '％', '＄',
];

const fn is_unexpanded_digit(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '\u{FF10}'..='\u{FF19}')
}

fn is_forbidden_symbol(ch: char) -> bool {
    FORBIDDEN_UNEXPANDED_SYMBOLS.contains(&ch)
}

const fn is_cjk_hanzi_ideograph(ch: char) -> bool {
    matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
}

/// Validated spoken script guaranteed free of raw digits or unexpanded symbols.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(into = "String")]
pub struct MandarinSpokenScript(String);

/// Polyglot type alias for [`MandarinSpokenScript`] across all `12` target languages.
pub type SpokenScript = MandarinSpokenScript;

impl MandarinSpokenScript {
    /// Trims outer whitespace and validates spoken Mandarin *kǒuyǔ* hygiene invariants:
    ///
    /// 1. Non-empty after trimming.
    /// 2. Zero `ASCII` (`'0'..='9'`) or fullwidth (`'０'..='９'`) digits.
    /// 3. Zero unexpanded currency, percentage, or operator symbols (`$`, `%`, `€`, `£`, `¥`, `￥`, `#`, `@`, `+`, `=`).
    /// 4. Contains at least one `CJK` Unified Ideograph (`\u{4E00}..=\u{9FFF}` or `\u{3400}..=\u{4DBF}`).
    pub fn parse(raw: &str) -> Result<Self, TranslationDomainError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(TranslationDomainError::EmptyScript);
        }

        let mut has_hanzi = false;
        for ch in trimmed.chars() {
            if is_unexpanded_digit(ch) {
                return Err(TranslationDomainError::ContainsUnexpandedDigit { ch });
            }
            if is_forbidden_symbol(ch) {
                return Err(TranslationDomainError::ContainsForbiddenSymbol { ch });
            }
            if is_cjk_hanzi_ideograph(ch) {
                has_hanzi = true;
            }
        }

        if !has_hanzi {
            return Err(TranslationDomainError::MissingHanzi);
        }

        Ok(Self(trimmed.to_owned()))
    }

    /// Validates spoken-script hygiene for `lang`:
    ///
    /// - [`TargetLanguage::Mandarin`] enforces `CJK` Hanzi presence via [`Self::parse`].
    /// - All other languages enforce non-empty trimmed text, zero `ASCII`/fullwidth digits,
    ///   zero forbidden symbols, and at least one Unicode alphabetic character.
    pub fn parse_for_language(
        raw: &str,
        lang: TargetLanguage,
    ) -> Result<Self, TranslationDomainError> {
        if lang == TargetLanguage::Mandarin {
            return Self::parse(raw);
        }

        Self::parse_non_mandarin(raw, lang.code())
    }

    fn parse_non_mandarin(
        raw: &str,
        language: &'static str,
    ) -> Result<Self, TranslationDomainError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(TranslationDomainError::EmptyScript);
        }

        let mut has_letters = false;
        for ch in trimmed.chars() {
            if is_unexpanded_digit(ch) {
                return Err(TranslationDomainError::ContainsUnexpandedDigit { ch });
            }
            if is_forbidden_symbol(ch) {
                return Err(TranslationDomainError::ContainsForbiddenSymbol { ch });
            }
            if ch.is_alphabetic() {
                has_letters = true;
            }
        }

        if !has_letters {
            return Err(TranslationDomainError::MissingSpokenLetters { language });
        }

        Ok(Self(trimmed.to_owned()))
    }

    /// Borrows the validated spoken script slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for MandarinSpokenScript {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse_non_mandarin(&raw, "any").map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for MandarinSpokenScript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for MandarinSpokenScript {
    type Error = TranslationDomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl TryFrom<&str> for MandarinSpokenScript {
    type Error = TranslationDomainError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl FromStr for MandarinSpokenScript {
    type Err = TranslationDomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl From<MandarinSpokenScript> for String {
    fn from(value: MandarinSpokenScript) -> Self {
        value.0
    }
}

/// Supported zero-shot voice cloning engines hosted in the local `GPU` sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TtsEngineKind {
    #[serde(rename = "cosyvoice_3")]
    CosyVoice3,
    #[serde(rename = "xtts_v2")]
    XttsV2,
}

impl TtsEngineKind {
    /// Returns the canonical database and wire identifier (`"cosyvoice_3"` or `"xtts_v2"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CosyVoice3 => "cosyvoice_3",
            Self::XttsV2 => "xtts_v2",
        }
    }
}

impl fmt::Display for TtsEngineKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TtsEngineKind {
    type Err = TranslationDomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cosyvoice_3" => Ok(Self::CosyVoice3),
            "xtts_v2" => Ok(Self::XttsV2),
            other => Err(TranslationDomainError::UnknownTtsEngine(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{MandarinSpokenScript, SpokenScript, TargetLanguage, TtsEngineKind};
    use crate::translation::error::TranslationDomainError;

    #[test]
    fn parses_clean_spoken_mandarin_and_trims_outer_whitespace() {
        let script = MandarinSpokenScript::parse("  今天下午三点半我们开会。 \n").unwrap();
        assert_eq!(script.as_str(), "今天下午三点半我们开会。");
        assert_eq!(script.to_string(), "今天下午三点半我们开会。");
    }

    #[test]
    fn rejects_empty_digits_symbols_and_non_hanzi_scripts() {
        assert_eq!(
            MandarinSpokenScript::parse("   "),
            Err(TranslationDomainError::EmptyScript)
        );
        assert_eq!(
            MandarinSpokenScript::parse("今天有3个会议"),
            Err(TranslationDomainError::ContainsUnexpandedDigit { ch: '3' })
        );
        assert_eq!(
            MandarinSpokenScript::parse("今天有５个会议"),
            Err(TranslationDomainError::ContainsUnexpandedDigit { ch: '５' })
        );
        assert_eq!(
            MandarinSpokenScript::parse("成功率九十五%"),
            Err(TranslationDomainError::ContainsForbiddenSymbol { ch: '%' })
        );
        assert_eq!(
            MandarinSpokenScript::parse("Hello world only"),
            Err(TranslationDomainError::MissingHanzi)
        );
    }

    #[test]
    fn parses_multilingual_spoken_scripts_and_enforces_digit_and_letter_hygiene() {
        let es = SpokenScript::parse_for_language(
            "  ¡Hola, equipo! Nos vemos en cinco minutos. ",
            TargetLanguage::Spanish,
        )
        .unwrap();
        assert_eq!(es.as_str(), "¡Hola, equipo! Nos vemos en cinco minutos.");

        let fr = SpokenScript::parse_for_language(
            "Bonjour l'équipe, à cet après-midi.",
            TargetLanguage::French,
        )
        .unwrap();
        assert_eq!(fr.as_str(), "Bonjour l'équipe, à cet après-midi.");

        let ja = SpokenScript::parse_for_language(
            "おはようございます、よろしくお願いします。",
            TargetLanguage::Japanese,
        )
        .unwrap();
        assert_eq!(ja.as_str(), "おはようございます、よろしくお願いします。");

        assert_eq!(
            SpokenScript::parse_for_language("Nos vemos en 5 minutos", TargetLanguage::Spanish),
            Err(TranslationDomainError::ContainsUnexpandedDigit { ch: '5' })
        );
        assert_eq!(
            SpokenScript::parse_for_language("¡¿...?!", TargetLanguage::Spanish),
            Err(TranslationDomainError::MissingSpokenLetters { language: "es" })
        );
        assert_eq!(
            SpokenScript::parse_for_language("Hola mundo", TargetLanguage::Mandarin),
            Err(TranslationDomainError::MissingHanzi)
        );
    }

    #[test]
    fn serde_json_enforces_spoken_script_validation_on_deserialize() {
        let valid_json = "\"百分之九十五\"";
        let parsed: MandarinSpokenScript = serde_json::from_str(valid_json).unwrap();
        assert_eq!(parsed.as_str(), "百分之九十五");
        assert_eq!(serde_json::to_string(&parsed).unwrap(), valid_json);

        let valid_es = "\"Hola equipo\"";
        let parsed_es: SpokenScript = serde_json::from_str(valid_es).unwrap();
        assert_eq!(parsed_es.as_str(), "Hola equipo");

        let invalid: Result<MandarinSpokenScript, _> = serde_json::from_str("\"95%\"");
        assert!(invalid.is_err());
    }

    #[test]
    fn tts_engine_kind_round_trips_string_and_json() {
        assert_eq!(TtsEngineKind::CosyVoice3.as_str(), "cosyvoice_3");
        assert_eq!(TtsEngineKind::XttsV2.as_str(), "xtts_v2");
        assert_eq!(
            TtsEngineKind::from_str("cosyvoice_3").unwrap(),
            TtsEngineKind::CosyVoice3
        );
        assert_eq!(
            TtsEngineKind::from_str("xtts_v2").unwrap(),
            TtsEngineKind::XttsV2
        );
        assert_eq!(
            TtsEngineKind::from_str("piper"),
            Err(TranslationDomainError::UnknownTtsEngine("piper".to_owned()))
        );
        assert_eq!(
            serde_json::to_string(&TtsEngineKind::CosyVoice3).unwrap(),
            "\"cosyvoice_3\""
        );
    }
}
