//! Pure character-level Levenshtein `CER` calculator and multilingual reverse-`ASR` verification gate.

use common::basis_points::BasisPoints;

use crate::translation::language::TargetLanguage;

/// Structured outcome of comparing synthesized reverse-`ASR` against expected spoken script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CerVerdict {
    pub cer_bps: BasisPoints,
    pub edit_distance: usize,
    pub reference_len: usize,
    pub passed: bool,
}

const fn is_indic_or_arabic_spoken_mark(ch: char) -> bool {
    matches!(
        ch,
        '\u{0901}'..='\u{0903}'
            | '\u{093A}'..='\u{094F}'
            | '\u{0951}'..='\u{0957}'
            | '\u{0962}'..='\u{0963}'
    )
}

/// Retains spoken `CJK` ideographs and lowercased alphanumeric characters while stripping all
/// punctuation (including `¿`, `¡`, `«`, `»`, `،`, `؟`, `।`, `CJK`, and `ASCII`), symbols,
/// emojis, and whitespace.
#[must_use]
pub fn extract_spoken_cjk_tokens(text: &str) -> Vec<char> {
    text.chars()
        .filter(|ch| ch.is_alphanumeric() || is_indic_or_arabic_spoken_mark(*ch))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Extracts normalized character tokens for `lang` (lowercasing alphabetic scripts and stripping
/// all punctuation and whitespace).
#[must_use]
pub fn extract_spoken_tokens_for_language(text: &str, _lang: TargetLanguage) -> Vec<char> {
    extract_spoken_cjk_tokens(text)
}

/// Computes exact character-level Levenshtein edit distance in `O(min(N, M))` space.
#[must_use]
pub fn levenshtein_distance(a: &[char], b: &[char]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let mut prev: Vec<usize> = (0..=short.len()).collect();
    let mut curr: Vec<usize> = vec![0; short.len() + 1];

    for (i, &long_ch) in long.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &short_ch) in short.iter().enumerate() {
            let substitution_cost = usize::from(long_ch != short_ch);
            let deletion = prev[j + 1] + 1;
            let insertion = curr[j] + 1;
            let substitution = prev[j] + substitution_cost;
            curr[j + 1] = deletion.min(insertion).min(substitution);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[short.len()]
}

fn score_token_sequences(
    expected: &[char],
    actual: &[char],
    max_cer_bps: BasisPoints,
) -> CerVerdict {
    if expected.is_empty() {
        return if actual.is_empty() {
            CerVerdict {
                cer_bps: BasisPoints::ZERO,
                edit_distance: 0,
                reference_len: 0,
                passed: true,
            }
        } else {
            CerVerdict {
                cer_bps: BasisPoints::ONE_HUNDRED_PERCENT,
                edit_distance: actual.len(),
                reference_len: 0,
                passed: false,
            }
        };
    }

    if actual.is_empty() {
        return CerVerdict {
            cer_bps: BasisPoints::ONE_HUNDRED_PERCENT,
            edit_distance: expected.len(),
            reference_len: expected.len(),
            passed: false,
        };
    }

    let edit_distance = levenshtein_distance(expected, actual);
    let dist_u64 = u64::try_from(edit_distance).unwrap_or(u64::MAX);
    let ref_u64 = u64::try_from(expected.len()).unwrap_or(u64::MAX);
    let cer_bps = BasisPoints::from_ratio_round_half_up(dist_u64, ref_u64);

    CerVerdict {
        cer_bps,
        edit_distance,
        reference_len: expected.len(),
        passed: cer_bps.is_within(max_cer_bps),
    }
}

/// Evaluates reverse-`ASR` transcription accuracy for `lang` against `expected` in [`BasisPoints`].
#[must_use]
pub fn evaluate_cer_gate_for_language(
    expected: &str,
    back_transcribed: &str,
    lang: TargetLanguage,
    max_cer_bps: BasisPoints,
) -> CerVerdict {
    let exp_tokens = extract_spoken_tokens_for_language(expected, lang);
    let act_tokens = extract_spoken_tokens_for_language(back_transcribed, lang);
    score_token_sequences(&exp_tokens, &act_tokens, max_cer_bps)
}

/// Evaluates reverse-`ASR` Mandarin transcription accuracy against `expected_hanzi` in [`BasisPoints`].
#[must_use]
pub fn evaluate_cer_gate(
    expected_hanzi: &str,
    back_transcribed_zh: &str,
    max_cer_bps: BasisPoints,
) -> CerVerdict {
    evaluate_cer_gate_for_language(
        expected_hanzi,
        back_transcribed_zh,
        TargetLanguage::Mandarin,
        max_cer_bps,
    )
}

#[cfg(test)]
mod proptests {
    use common::basis_points::BasisPoints;
    use proptest::prelude::*;

    use super::{
        evaluate_cer_gate, evaluate_cer_gate_for_language, extract_spoken_cjk_tokens,
        levenshtein_distance,
    };
    use crate::translation::{language::TargetLanguage, types::MandarinSpokenScript};

    #[test]
    fn empty_back_transcription_always_fails_even_when_max_cer_is_100_percent() {
        let verdict = evaluate_cer_gate("你好世界", "  ，。！？ ", BasisPoints::from_raw(10_000));
        assert!(!verdict.passed);
        assert_eq!(verdict.cer_bps, BasisPoints::ONE_HUNDRED_PERCENT);
        assert_eq!(verdict.edit_distance, 4);
        assert_eq!(verdict.reference_len, 4);
    }

    #[test]
    fn token_extractor_strips_all_cjk_spanish_arabic_hindi_and_ascii_punctuation() {
        let tokens = extract_spoken_cjk_tokens("“你好，世界！”——（测试）【一】…,.!?;:'\"()[]- ");
        assert_eq!(tokens, vec!['你', '好', '世', '界', '测', '试', '一']);

        let es_verdict = evaluate_cer_gate_for_language(
            "¡Hola, equipo! Nos vemos en cinco minutos.",
            "hola equipo nos vemos en cinco minutos",
            TargetLanguage::Spanish,
            BasisPoints::ZERO,
        );
        assert!(es_verdict.passed);
        assert_eq!(es_verdict.cer_bps, BasisPoints::ZERO);
        assert_eq!(es_verdict.edit_distance, 0);

        let polyglot_punct = extract_spoken_cjk_tokens("¿¡«Bonjour»، ؟ ।!");
        assert_eq!(polyglot_punct, vec!['b', 'o', 'n', 'j', 'o', 'u', 'r']);
    }

    proptest! {
        #[test]
        fn identical_hanzi_always_scores_zero_cer_regardless_of_punctuation(
            cjk in "[\\u4e00-\\u9fa5]{1,80}",
            punct in "[，。！？、\\s]{0,10}"
        ) {
            let decorated = format!("{punct}{cjk}{punct}");
            let verdict = evaluate_cer_gate(&cjk, &decorated, BasisPoints::ZERO);
            prop_assert!(verdict.passed);
            prop_assert_eq!(verdict.cer_bps, BasisPoints::ZERO);
        }

        #[test]
        fn spoken_script_rejects_any_embedded_ascii_digit(
            prefix in "[\\u4e00-\\u9fa5]{1,20}",
            digit in "[0-9]",
            suffix in "[\\u4e00-\\u9fa5]{1,20}"
        ) {
            let raw = format!("{prefix}{digit}{suffix}");
            prop_assert!(MandarinSpokenScript::parse(&raw).is_err());
        }

        #[test]
        fn levenshtein_distance_is_symmetric(
            a in proptest::collection::vec(proptest::char::any(), 0..=40),
            b in proptest::collection::vec(proptest::char::any(), 0..=40)
        ) {
            prop_assert_eq!(levenshtein_distance(&a, &b), levenshtein_distance(&b, &a));
        }

        #[test]
        fn levenshtein_distance_satisfies_triangle_inequality(
            a in proptest::collection::vec(proptest::char::any(), 0..=30),
            b in proptest::collection::vec(proptest::char::any(), 0..=30),
            c in proptest::collection::vec(proptest::char::any(), 0..=30)
        ) {
            let d_ac = levenshtein_distance(&a, &c);
            let d_ab = levenshtein_distance(&a, &b);
            let d_bc = levenshtein_distance(&b, &c);
            prop_assert!(d_ac <= d_ab + d_bc);
        }
    }
}
