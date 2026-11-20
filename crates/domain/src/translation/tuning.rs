//! Validated `XTTS-v2` / `CosyVoice` synthesis tuning parameters and partial-`JSON` defaults.

use serde::{Deserialize, Serialize};

/// Per-profile and per-render synthesis conditioning and sampling parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SynthesisTuning {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub repetition_penalty: f32,
    pub length_penalty: f32,
    pub speed: f32,
    pub gpt_cond_len_secs: u16,
    pub gpt_cond_chunk_len_secs: u16,
    pub max_ref_len_secs: u16,
    pub sound_norm_refs: bool,
}

impl Default for SynthesisTuning {
    fn default() -> Self {
        Self {
            temperature: 0.62,
            top_p: 0.80,
            top_k: 50,
            repetition_penalty: 5.0,
            length_penalty: 1.0,
            speed: 1.0,
            gpt_cond_len_secs: 30,
            gpt_cond_chunk_len_secs: 4,
            max_ref_len_secs: 60,
            sound_norm_refs: true,
        }
    }
}

impl SynthesisTuning {
    /// Validates finite float invariants, inclusive parameter bounds, and conditioning chunk rules.
    pub fn validate(&self) -> Result<(), String> {
        if !self.temperature.is_finite() || !(0.05..=1.20).contains(&self.temperature) {
            return Err(format!(
                "temperature {} must be finite and within 0.05..=1.20",
                self.temperature
            ));
        }
        if !self.top_p.is_finite() || !(0.10..=1.00).contains(&self.top_p) {
            return Err(format!(
                "top_p {} must be finite and within 0.10..=1.00",
                self.top_p
            ));
        }
        if !(1..=200).contains(&self.top_k) {
            return Err(format!("top_k {} must be within 1..=200", self.top_k));
        }
        if !self.repetition_penalty.is_finite()
            || !(1.00..=15.00).contains(&self.repetition_penalty)
        {
            return Err(format!(
                "repetition_penalty {} must be finite and within 1.00..=15.00",
                self.repetition_penalty
            ));
        }
        if !self.length_penalty.is_finite() || !(0.50..=2.00).contains(&self.length_penalty) {
            return Err(format!(
                "length_penalty {} must be finite and within 0.50..=2.00",
                self.length_penalty
            ));
        }
        if !self.speed.is_finite() || !(0.50..=2.00).contains(&self.speed) {
            return Err(format!(
                "speed {} must be finite and within 0.50..=2.00",
                self.speed
            ));
        }
        if !(3..=60).contains(&self.gpt_cond_len_secs) {
            return Err(format!(
                "gpt_cond_len_secs {} must be within 3..=60",
                self.gpt_cond_len_secs
            ));
        }
        if !(2..=30).contains(&self.gpt_cond_chunk_len_secs) {
            return Err(format!(
                "gpt_cond_chunk_len_secs {} must be within 2..=30",
                self.gpt_cond_chunk_len_secs
            ));
        }
        if self.gpt_cond_chunk_len_secs > self.gpt_cond_len_secs {
            return Err(format!(
                "gpt_cond_chunk_len_secs ({}) cannot exceed gpt_cond_len_secs ({})",
                self.gpt_cond_chunk_len_secs, self.gpt_cond_len_secs
            ));
        }
        if !(6..=120).contains(&self.max_ref_len_secs) {
            return Err(format!(
                "max_ref_len_secs {} must be within 6..=120",
                self.max_ref_len_secs
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SynthesisTuning;

    #[test]
    fn default_tuning_and_partial_json_hydration_produce_valid_parameters() {
        let defaults = SynthesisTuning::default();
        assert_eq!(defaults.validate(), Ok(()));
        assert!((defaults.temperature - 0.62).abs() < 1e-6);
        assert!((defaults.top_p - 0.80).abs() < 1e-6);
        assert_eq!(defaults.top_k, 50);
        assert!((defaults.repetition_penalty - 5.0).abs() < 1e-6);
        assert!((defaults.length_penalty - 1.0).abs() < 1e-6);
        assert!((defaults.speed - 1.0).abs() < 1e-6);
        assert_eq!(defaults.gpt_cond_len_secs, 30);
        assert_eq!(defaults.gpt_cond_chunk_len_secs, 4);
        assert_eq!(defaults.max_ref_len_secs, 60);
        assert!(defaults.sound_norm_refs);

        let from_empty: SynthesisTuning = serde_json::from_str("{}").unwrap();
        assert_eq!(from_empty, defaults);

        let partial: SynthesisTuning =
            serde_json::from_str(r#"{"temperature": 0.45, "sound_norm_refs": false}"#).unwrap();
        assert!((partial.temperature - 0.45).abs() < 1e-6);
        assert!(!partial.sound_norm_refs);
        assert_eq!(partial.top_k, 50);
        assert_eq!(partial.gpt_cond_len_secs, 30);
        assert_eq!(partial.validate(), Ok(()));
    }

    #[test]
    fn accepts_exact_inclusive_lower_and_upper_boundaries() {
        let lower = SynthesisTuning {
            temperature: 0.05,
            top_p: 0.10,
            top_k: 1,
            repetition_penalty: 1.00,
            length_penalty: 0.50,
            speed: 0.50,
            gpt_cond_len_secs: 3,
            gpt_cond_chunk_len_secs: 2,
            max_ref_len_secs: 6,
            sound_norm_refs: false,
        };
        assert_eq!(lower.validate(), Ok(()));

        let upper = SynthesisTuning {
            temperature: 1.20,
            top_p: 1.00,
            top_k: 200,
            repetition_penalty: 15.00,
            length_penalty: 2.00,
            speed: 2.00,
            gpt_cond_len_secs: 60,
            gpt_cond_chunk_len_secs: 30,
            max_ref_len_secs: 120,
            sound_norm_refs: true,
        };
        assert_eq!(upper.validate(), Ok(()));

        let equal_chunk_and_cond = SynthesisTuning {
            gpt_cond_len_secs: 12,
            gpt_cond_chunk_len_secs: 12,
            ..SynthesisTuning::default()
        };
        assert_eq!(equal_chunk_and_cond.validate(), Ok(()));
    }

    #[test]
    fn rejects_non_finite_out_of_bounds_and_inverted_chunk_lengths() {
        let bad_cases = [
            SynthesisTuning {
                temperature: f32::NAN,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                temperature: 0.04,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                temperature: 1.21,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                top_p: 0.09,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                top_p: f32::INFINITY,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                top_k: 0,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                top_k: 201,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                repetition_penalty: 0.99,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                repetition_penalty: 15.01,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                length_penalty: 0.49,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                speed: 2.01,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                gpt_cond_len_secs: 2,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                gpt_cond_chunk_len_secs: 1,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                gpt_cond_len_secs: 6,
                gpt_cond_chunk_len_secs: 8,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                max_ref_len_secs: 5,
                ..SynthesisTuning::default()
            },
            SynthesisTuning {
                max_ref_len_secs: 121,
                ..SynthesisTuning::default()
            },
        ];

        for (idx, candidate) in bad_cases.iter().enumerate() {
            assert!(
                candidate.validate().is_err(),
                "expected invalid tuning case #{idx} ({candidate:?}) to fail validation"
            );
        }
    }
}
