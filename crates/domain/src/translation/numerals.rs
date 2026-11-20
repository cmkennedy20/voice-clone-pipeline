//! Pure English numeral, clock-time, currency, and percentage verbalizer so `LLM` translation
//! prompts never leak raw Arabic digits (`0..=9`) into synthesized target-language scripts.

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];

const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

fn spell_below_thousand(n: u64) -> String {
    if n < 20 {
        return ONES[usize::try_from(n).unwrap_or(0)].to_owned();
    }
    if n < 100 {
        let ten = TENS[usize::try_from(n / 10).unwrap_or(0)];
        let rem = n % 10;
        return if rem == 0 {
            ten.to_owned()
        } else {
            format!("{ten}-{}", ONES[usize::try_from(rem).unwrap_or(0)])
        };
    }
    let hundred = ONES[usize::try_from(n / 100).unwrap_or(0)];
    let rem = n % 100;
    if rem == 0 {
        format!("{hundred} hundred")
    } else {
        format!("{hundred} hundred {}", spell_below_thousand(rem))
    }
}

/// Converts an unsigned integer into spoken English words (falling back to digit-by-digit above trillions).
#[must_use]
pub fn spell_u64_en(mut n: u64) -> String {
    if n == 0 {
        return "zero".to_owned();
    }
    let scales: [(u64, &str); 4] = [
        (1_000_000_000_000, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ];
    let mut parts = Vec::new();
    for (scale_val, scale_name) in scales {
        if n >= scale_val {
            let chunk = n / scale_val;
            n %= scale_val;
            if chunk >= 1_000 {
                parts.push(format!("{} {scale_name}", spell_u64_en(chunk)));
            } else {
                parts.push(format!("{} {scale_name}", spell_below_thousand(chunk)));
            }
        }
    }
    if n > 0 {
        parts.push(spell_below_thousand(n));
    }
    parts.join(" ")
}

fn spell_digits_individually(digits: &str) -> String {
    digits
        .chars()
        .filter_map(|c| c.to_digit(10))
        .map(|d| ONES[usize::try_from(d).unwrap_or(0)])
        .collect::<Vec<_>>()
        .join(" ")
}

fn spell_number_str(digits: &str) -> String {
    digits
        .parse::<u64>()
        .map_or_else(|_| spell_digits_individually(digits), spell_u64_en)
}

fn normalize_fullwidth_digit(ch: char) -> char {
    if ('\u{FF10}'..='\u{FF19}').contains(&ch) {
        char::from_u32(u32::from(ch) - 0xFF10 + u32::from(b'0')).unwrap_or(ch)
    } else {
        ch
    }
}

fn consume_digits(chars: &[char], start: usize) -> (String, usize) {
    let mut out = String::new();
    let mut idx = start;
    while idx < chars.len() {
        let ch = chars[idx];
        if ch.is_ascii_digit() {
            out.push(ch);
            idx += 1;
        } else if ch == ','
            && !out.is_empty()
            && idx + 3 < chars.len()
            && chars[idx + 1..=idx + 3].iter().all(char::is_ascii_digit)
            && (idx + 4 == chars.len() || !chars[idx + 4].is_ascii_digit())
        {
            idx += 1;
        } else {
            break;
        }
    }
    (out, idx)
}

fn try_ordinal_suffix(word_num: &str, suffix: &str) -> Option<String> {
    let lower = suffix.to_ascii_lowercase();
    if !matches!(lower.as_str(), "st" | "nd" | "rd" | "th") {
        return None;
    }
    if let Some((prefix, last)) = word_num.rsplit_once([' ', '-']) {
        let sep = if word_num.contains('-') && prefix.len() + 1 + last.len() == word_num.len() {
            word_num.as_bytes()[prefix.len()] as char
        } else {
            ' '
        };
        let ord_last = word_to_ordinal(last);
        Some(format!("{prefix}{sep}{ord_last}"))
    } else {
        Some(word_to_ordinal(word_num))
    }
}

fn word_to_ordinal(word: &str) -> String {
    match word {
        "one" => "first".to_owned(),
        "two" => "second".to_owned(),
        "three" => "third".to_owned(),
        "five" => "fifth".to_owned(),
        "eight" => "eighth".to_owned(),
        "nine" => "ninth".to_owned(),
        "twelve" => "twelfth".to_owned(),
        other if other.ends_with('y') => format!("{}ieth", &other[..other.len() - 1]),
        other => format!("{other}th"),
    }
}

/// Expands embedded digits, clock times (`8:30`), decimals (`3.5`), currency (`$20`),
/// percentages (`95%`), and ordinals (`1st`) in an English prompt into spoken words.
#[must_use]
pub fn expand_english_numerals_for_tts(input: &str) -> String {
    let chars: Vec<char> = input.chars().map(normalize_fullwidth_digit).collect();
    let mut out = String::with_capacity(input.len() + 16);
    let mut idx = 0;

    while idx < chars.len() {
        let ch = chars[idx];

        // Currency prefix ($20, €15, £5)
        if matches!(ch, '$' | '€' | '£' | '¥')
            && idx + 1 < chars.len()
            && chars[idx + 1].is_ascii_digit()
        {
            let unit = match ch {
                '$' => "dollars",
                '€' => "euros",
                '£' => "pounds",
                _ => "yuan",
            };
            let (digits, next_idx) = consume_digits(&chars, idx + 1);
            let spoken = spell_number_str(&digits);
            if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                out.push(' ');
            }
            out.push_str(&spoken);
            out.push(' ');
            out.push_str(unit);
            idx = next_idx;
            if idx < chars.len()
                && !chars[idx].is_whitespace()
                && !chars[idx].is_ascii_punctuation()
            {
                out.push(' ');
            }
            continue;
        }

        if ch.is_ascii_digit() {
            let (digits, mut next_idx) = consume_digits(&chars, idx);
            if !out.is_empty()
                && !out.ends_with(char::is_whitespace)
                && !out.ends_with(['(', '[', '"', '\''])
            {
                out.push(' ');
            }

            // Clock time: H:MM or HH:MM
            if (1..=2).contains(&digits.len())
                && next_idx + 2 < chars.len()
                && chars[next_idx] == ':'
                && chars[next_idx + 1].is_ascii_digit()
                && chars[next_idx + 2].is_ascii_digit()
                && (next_idx + 3 == chars.len() || !chars[next_idx + 3].is_ascii_digit())
            {
                let mins: String = chars[next_idx + 1..=next_idx + 2].iter().collect();
                let hr_word = spell_number_str(&digits);
                if mins == "00" {
                    out.push_str(&hr_word);
                    out.push_str(" o'clock");
                } else if let Some(rest) = mins.strip_prefix('0') {
                    out.push_str(&hr_word);
                    out.push_str(" oh ");
                    out.push_str(&spell_number_str(rest));
                } else {
                    out.push_str(&hr_word);
                    out.push(' ');
                    out.push_str(&spell_number_str(&mins));
                }
                idx = next_idx + 3;
            } else if next_idx + 1 < chars.len()
                && chars[next_idx] == '.'
                && chars[next_idx + 1].is_ascii_digit()
            {
                // Decimal: 3.14 -> three point one four
                let (frac, after_frac) = consume_digits(&chars, next_idx + 1);
                out.push_str(&spell_number_str(&digits));
                out.push_str(" point ");
                out.push_str(&spell_digits_individually(&frac));
                idx = after_frac;
            } else {
                let mut spoken = spell_number_str(&digits);
                // Check ordinal suffix (st, nd, rd, th)
                if next_idx + 1 < chars.len()
                    && chars[next_idx].is_ascii_alphabetic()
                    && chars[next_idx + 1].is_ascii_alphabetic()
                    && (next_idx + 2 == chars.len() || !chars[next_idx + 2].is_ascii_alphanumeric())
                {
                    let suf: String = chars[next_idx..=next_idx + 1].iter().collect();
                    if let Some(ord) = try_ordinal_suffix(&spoken, &suf) {
                        spoken = ord;
                        next_idx += 2;
                    }
                }
                out.push_str(&spoken);
                idx = next_idx;
            }

            if idx < chars.len() && chars[idx] == '%' {
                out.push_str(" percent");
                idx += 1;
            }
            if idx < chars.len() && chars[idx].is_ascii_alphabetic() {
                out.push(' ');
            }
            continue;
        }

        out.push(ch);
        idx += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::expand_english_numerals_for_tts;

    #[test]
    fn expands_digits_clock_times_currency_percentages_and_ordinals() {
        assert_eq!(
            expand_english_numerals_for_tts("See you at 8!"),
            "See you at eight!"
        );
        assert_eq!(
            expand_english_numerals_for_tts(
                "Meeting at 7:30 or 8:05 for $20 (95% sure) on the 1st."
            ),
            "Meeting at seven thirty or eight oh five for twenty dollars (ninety-five percent sure) on the first."
        );
        assert!(
            !expand_english_numerals_for_tts("Order 8 bowls in 15 mins at 3.5x")
                .chars()
                .any(|c| c.is_ascii_digit())
        );
    }
}
