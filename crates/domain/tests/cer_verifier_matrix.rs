//! Layer 2 Golden Matrix Suite — Mandarin CER calculations and `MandarinSpokenScript` guards.

use common::basis_points::BasisPoints;
use domain::translation::{
    error::TranslationDomainError,
    types::MandarinSpokenScript,
    verifier::{CerVerdict, evaluate_cer_gate},
};

struct CerMatrixCase {
    name: &'static str,
    expected_hanzi: &'static str,
    back_transcribed_zh: &'static str,
    max_cer_bps: u32,
    cer_bps: u32,
    edit_distance: usize,
    reference_len: usize,
    passed: bool,
}

impl CerMatrixCase {
    const fn pass(
        name: &'static str,
        expected_hanzi: &'static str,
        back_transcribed_zh: &'static str,
        max_cer_bps: u32,
        cer_bps: u32,
        edit_distance: usize,
        reference_len: usize,
    ) -> Self {
        Self {
            name,
            expected_hanzi,
            back_transcribed_zh,
            max_cer_bps,
            cer_bps,
            edit_distance,
            reference_len,
            passed: true,
        }
    }

    const fn fail(
        name: &'static str,
        expected_hanzi: &'static str,
        back_transcribed_zh: &'static str,
        max_cer_bps: u32,
        cer_bps: u32,
        edit_distance: usize,
        reference_len: usize,
    ) -> Self {
        Self {
            name,
            expected_hanzi,
            back_transcribed_zh,
            max_cer_bps,
            cer_bps,
            edit_distance,
            reference_len,
            passed: false,
        }
    }
}

const CER_GOLDEN_CASES: [CerMatrixCase; 26] = [
    CerMatrixCase::pass(
        "01_identical_short_greeting_zero_cer",
        "你好世界",
        "你好世界",
        1_200,
        0,
        0,
        4,
    ),
    CerMatrixCase::pass(
        "02_identical_with_heavy_cjk_punctuation_and_quotes_zero_cer",
        "今天下午三点半我们准时开会",
        "“今天，下午三点半——我们准时开会！”",
        0,
        0,
        0,
        13,
    ),
    CerMatrixCase::pass(
        "03_identical_with_ascii_punctuation_and_newlines_zero_cer",
        "请把文件发给我谢谢",
        "  请把文件, 发给我... 谢谢!\n\t",
        1_200,
        0,
        0,
        9,
    ),
    CerMatrixCase::pass(
        "04_one_of_ten_substitution_equals_1000bps_passes_1200_gate",
        "今天天气真的非常不错",
        "今天天气真的非常不措",
        1_200,
        1_000,
        1,
        10,
    ),
    CerMatrixCase::fail(
        "05_one_of_eight_substitution_equals_1250bps_fails_1200_gate",
        "我们明天早上出发",
        "我们明天早上初发",
        1_200,
        1_250,
        1,
        8,
    ),
    CerMatrixCase::pass(
        "06_three_of_twenty_five_substitutions_equals_exact_1200bps_passes",
        "今天晚上八点钟我们在老地方见面一起吃顿热乎的火锅吧",
        "今天晚上八点中我们在老地芳见面一起吃顿热呼的火锅吧",
        1_200,
        1_200,
        3,
        25,
    ),
    CerMatrixCase::pass(
        "07_one_of_nine_insertion_rounds_to_1111bps_passes_1200_gate",
        "这个方案我已经看了",
        "这个方案我已经看过了",
        1_200,
        1_111,
        1,
        9,
    ),
    CerMatrixCase::pass(
        "08_one_of_ten_deletion_equals_1000bps_passes_1200_gate",
        "我现在正在去机场路上",
        "我现在正在去机场路",
        1_200,
        1_000,
        1,
        10,
    ),
    CerMatrixCase::fail(
        "09_two_of_ten_deletions_equals_2000bps_fails_1200_gate",
        "我现在正在去机场路上",
        "我现在正在去机场",
        1_200,
        2_000,
        2,
        10,
    ),
    CerMatrixCase::fail(
        "10_complete_hallucination_longer_than_reference_exceeds_10000bps",
        "你好世界",
        "感谢大家收看本期视频点个关注",
        1_200,
        35_000,
        14,
        4,
    ),
    CerMatrixCase::fail(
        "11_complete_hallucination_same_length_scores_10000bps_and_fails",
        "早安朋友",
        "完全不同",
        1_200,
        10_000,
        4,
        4,
    ),
    CerMatrixCase::fail(
        "12_empty_asr_output_scores_10000bps_and_fails",
        "请稍等一下",
        "",
        1_200,
        10_000,
        5,
        5,
    ),
    CerMatrixCase::fail(
        "13_punctuation_only_asr_output_is_treated_as_empty_and_fails_permissive_gate",
        "请稍等一下",
        "……，。！？   ",
        15_000,
        10_000,
        5,
        5,
    ),
    CerMatrixCase::pass(
        "14_both_empty_strings_pass_with_zero_cer",
        "",
        "",
        0,
        0,
        0,
        0,
    ),
    CerMatrixCase::fail(
        "15_empty_reference_with_hallucinated_asr_fails_with_10000bps",
        "   ",
        "字幕由社区提供",
        1_200,
        10_000,
        7,
        0,
    ),
    CerMatrixCase::fail(
        "16_whisper_arabic_digit_normalization_mismatch_penalizes_unaligned_digits",
        "百分之九十五",
        "95%",
        1_200,
        10_000,
        6,
        6,
    ),
    CerMatrixCase::pass(
        "17_spoken_date_kouyu_exact_match_passes",
        "两千零二十六年十一月十八日",
        "两千零二十六年，十一月十八日。",
        1_000,
        0,
        0,
        13,
    ),
    CerMatrixCase::pass(
        "18_spoken_date_single_er_liang_variant_1_of_13_equals_769bps_passes_1000_gate",
        "两千零二十六年十一月十八日",
        "二千零二十六年十一月十八日",
        1_000,
        769,
        1,
        13,
    ),
    CerMatrixCase::fail(
        "19_one_of_three_homophone_error_rounds_to_3333bps_fails",
        "没问题",
        "煤问题",
        1_200,
        3_333,
        1,
        3,
    ),
    CerMatrixCase::fail(
        "20_two_of_three_errors_rounds_half_up_to_6667bps_fails",
        "没问题",
        "煤文题",
        1_200,
        6_667,
        2,
        3,
    ),
    CerMatrixCase::fail(
        "21_transposition_of_two_adjacent_hanzi_costs_2_edits_on_10_chars",
        "我们需要马上处理这事",
        "我们需要上马处理这事",
        1_200,
        2_000,
        2,
        10,
    ),
    CerMatrixCase::pass(
        "22_single_modal_particle_substitution_on_12_chars_is_833bps_passes",
        "我已经把测试全部跑通了呀",
        "我已经把测试全都跑通了呀",
        1_000,
        833,
        1,
        12,
    ),
    CerMatrixCase::fail(
        "23_repeated_stutter_hallucination_triples_length_to_20000bps",
        "好的明白",
        "好的明白好的明白好的明白",
        1_200,
        20_000,
        8,
        4,
    ),
    CerMatrixCase::pass(
        "24_one_of_sixteen_error_is_625bps_passes_strict_1000_gate",
        "今天晚上的语音克隆测试非常顺利呀",
        "今天晚上的语音克隆测试非常顺力呀",
        1_000,
        625,
        1,
        16,
    ),
    CerMatrixCase::pass(
        "25_ideographic_space_and_book_title_marks_stripped_cleanly",
        "我最近在读三体这本书",
        "我最近\u{3000}在读《三体》【这本书】！",
        0,
        0,
        0,
        10,
    ),
    CerMatrixCase::pass(
        "26_exact_1000bps_passes_scorecard_m3_mean_threshold",
        "服务器显存占用很稳定",
        "服务器显存站用很稳定",
        1_000,
        1_000,
        1,
        10,
    ),
];

#[test]
fn mandarin_cer_gate_golden_matrix() {
    assert!(CER_GOLDEN_CASES.len() >= 25);
    for case in &CER_GOLDEN_CASES {
        let actual = evaluate_cer_gate(
            case.expected_hanzi,
            case.back_transcribed_zh,
            BasisPoints::from_raw(case.max_cer_bps),
        );
        let expected = CerVerdict {
            cer_bps: BasisPoints::from_raw(case.cer_bps),
            edit_distance: case.edit_distance,
            reference_len: case.reference_len,
            passed: case.passed,
        };
        assert_eq!(actual, expected, "failed CER matrix case `{}`", case.name);
    }
}

struct ScriptHygieneCase {
    name: &'static str,
    raw: &'static str,
    expected: Result<&'static str, TranslationDomainError>,
}

const SCRIPT_HYGIENE_CASES: [ScriptHygieneCase; 20] = [
    ScriptHygieneCase {
        name: "valid_spoken_date_kouyu",
        raw: "两千零二十六年十一月十八日",
        expected: Ok("两千零二十六年十一月十八日"),
    },
    ScriptHygieneCase {
        name: "valid_spoken_percentage_kouyu",
        raw: "百分之九十五",
        expected: Ok("百分之九十五"),
    },
    ScriptHygieneCase {
        name: "valid_spoken_currency_kouyu_with_trimmed_spaces",
        raw: "  一共是三百五十块五毛钱。 ",
        expected: Ok("一共是三百五十块五毛钱。"),
    },
    ScriptHygieneCase {
        name: "valid_cjk_extension_a_ideograph",
        raw: "㐀",
        expected: Ok("㐀"),
    },
    ScriptHygieneCase {
        name: "rejects_empty_string",
        raw: "",
        expected: Err(TranslationDomainError::EmptyScript),
    },
    ScriptHygieneCase {
        name: "rejects_whitespace_only",
        raw: " \t\n\u{3000} ",
        expected: Err(TranslationDomainError::EmptyScript),
    },
    ScriptHygieneCase {
        name: "rejects_ascii_zero",
        raw: "零和0不一样",
        expected: Err(TranslationDomainError::ContainsUnexpandedDigit { ch: '0' }),
    },
    ScriptHygieneCase {
        name: "rejects_ascii_year_digits",
        raw: "2026年十一月十八日",
        expected: Err(TranslationDomainError::ContainsUnexpandedDigit { ch: '2' }),
    },
    ScriptHygieneCase {
        name: "rejects_fullwidth_digit",
        raw: "一共９个苹果",
        expected: Err(TranslationDomainError::ContainsUnexpandedDigit { ch: '９' }),
    },
    ScriptHygieneCase {
        name: "rejects_dollar_symbol",
        raw: "价格是$五十",
        expected: Err(TranslationDomainError::ContainsForbiddenSymbol { ch: '$' }),
    },
    ScriptHygieneCase {
        name: "rejects_percent_symbol",
        raw: "九十五%",
        expected: Err(TranslationDomainError::ContainsForbiddenSymbol { ch: '%' }),
    },
    ScriptHygieneCase {
        name: "rejects_euro_symbol",
        raw: "一百€",
        expected: Err(TranslationDomainError::ContainsForbiddenSymbol { ch: '€' }),
    },
    ScriptHygieneCase {
        name: "rejects_pound_symbol",
        raw: "五十£",
        expected: Err(TranslationDomainError::ContainsForbiddenSymbol { ch: '£' }),
    },
    ScriptHygieneCase {
        name: "rejects_halfwidth_yen_symbol",
        raw: "¥一百元",
        expected: Err(TranslationDomainError::ContainsForbiddenSymbol { ch: '¥' }),
    },
    ScriptHygieneCase {
        name: "rejects_fullwidth_yen_symbol",
        raw: "￥一百元",
        expected: Err(TranslationDomainError::ContainsForbiddenSymbol { ch: '￥' }),
    },
    ScriptHygieneCase {
        name: "rejects_hash_symbol",
        raw: "标签#一",
        expected: Err(TranslationDomainError::ContainsForbiddenSymbol { ch: '#' }),
    },
    ScriptHygieneCase {
        name: "rejects_at_and_plus_symbols",
        raw: "一+一等于二",
        expected: Err(TranslationDomainError::ContainsForbiddenSymbol { ch: '+' }),
    },
    ScriptHygieneCase {
        name: "rejects_equals_sign",
        raw: "结果=成功",
        expected: Err(TranslationDomainError::ContainsForbiddenSymbol { ch: '=' }),
    },
    ScriptHygieneCase {
        name: "rejects_latin_only_text_without_hanzi",
        raw: "ni hao shi jie",
        expected: Err(TranslationDomainError::MissingHanzi),
    },
    ScriptHygieneCase {
        name: "rejects_punctuation_only_without_hanzi",
        raw: "，。！？",
        expected: Err(TranslationDomainError::MissingHanzi),
    },
];

#[test]
fn mandarin_spoken_script_digit_and_symbol_hygiene_matrix() {
    for case in &SCRIPT_HYGIENE_CASES {
        let actual = MandarinSpokenScript::parse(case.raw);
        match case.expected {
            Ok(expected_str) => {
                let script = actual.unwrap_or_else(|err| {
                    panic!("expected `{}` to succeed, got {err:?}", case.name)
                });
                assert_eq!(script.as_str(), expected_str, "case `{}`", case.name);
            }
            Err(ref expected_err) => {
                assert_eq!(
                    actual.as_ref().err(),
                    Some(expected_err),
                    "case `{}`",
                    case.name
                );
            }
        }
    }
}
