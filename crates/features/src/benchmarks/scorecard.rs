//! Pure `M1–M6` metric aggregation, `P95` percentile calculation, and Markdown scorecard formatter.

use std::fmt::Write as _;

use common::basis_points::BasisPoints;
use domain::translation::types::TtsEngineKind;

use crate::{
    benchmarks::manifest::ManifestThresholds, shared_queries::profile_loader::BenchmarkRunId,
};

#[derive(Debug, Clone)]
pub(crate) struct ClipEvaluationOutcome {
    pub id: String,
    pub category: String,
    pub digit_hygiene_ok: bool,
    pub cer_bps: BasisPoints,
    pub speaker_sim_bps: BasisPoints,
    pub format_compliant: bool,
    pub latency_ms: u32,
    pub attempts_used: u8,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AggregatedMetrics {
    pub clips_evaluated: u16,
    pub gate0_accuracy_bps: BasisPoints,
    pub digit_hygiene_bps: BasisPoints,
    pub mean_cer_bps: BasisPoints,
    pub max_clip_cer_bps: BasisPoints,
    pub mean_speaker_sim_bps: BasisPoints,
    pub format_compliance_bps: BasisPoints,
    pub p95_latency_ms: u32,
    pub peak_vram_mib: u32,
    pub passed_all_gates: bool,
}

pub(crate) struct ScorecardRenderParams<'a> {
    pub run_id: BenchmarkRunId,
    pub engine_tested: TtsEngineKind,
    pub manifest_version: &'a str,
    pub metrics: &'a AggregatedMetrics,
    pub thresholds: &'a ManifestThresholds,
    pub clips: &'a [ClipEvaluationOutcome],
}

fn compute_p95_ms(latencies: &[u32]) -> u32 {
    if latencies.is_empty() {
        return 0;
    }
    let mut sorted = latencies.to_vec();
    sorted.sort_unstable();
    let count = sorted.len();
    let rank = (count.saturating_mul(95)).div_ceil(100);
    let idx = rank.saturating_sub(1).min(count - 1);
    sorted.get(idx).copied().unwrap_or(0)
}

fn mean_bps_round_half_up(sum_bps: u64, count: u64) -> BasisPoints {
    if count == 0 {
        return BasisPoints::ZERO;
    }
    let mean = (sum_bps.saturating_add(count / 2)) / count;
    BasisPoints::from_raw(u32::try_from(mean).unwrap_or(u32::MAX))
}

pub(crate) fn aggregate_suite_metrics(
    gate0_accuracy_bps: BasisPoints,
    clips: &[ClipEvaluationOutcome],
    peak_vram_mib: u32,
    thresholds: &ManifestThresholds,
) -> AggregatedMetrics {
    let count_u64 = u64::try_from(clips.len()).unwrap_or(0);
    let clips_evaluated = u16::try_from(clips.len()).unwrap_or(u16::MAX);

    let mut hygiene_hits: u64 = 0;
    let mut format_hits: u64 = 0;
    let mut cer_sum: u64 = 0;
    let mut max_clip_cer_bps = BasisPoints::ZERO;
    let mut sim_sum: u64 = 0;
    let mut latencies = Vec::with_capacity(clips.len());

    for clip in clips {
        if clip.digit_hygiene_ok {
            hygiene_hits = hygiene_hits.saturating_add(1);
        }
        if clip.format_compliant {
            format_hits = format_hits.saturating_add(1);
        }
        cer_sum = cer_sum.saturating_add(u64::from(clip.cer_bps.value()));
        if clip.cer_bps > max_clip_cer_bps {
            max_clip_cer_bps = clip.cer_bps;
        }
        sim_sum = sim_sum.saturating_add(u64::from(clip.speaker_sim_bps.value()));
        latencies.push(clip.latency_ms);
    }

    let digit_hygiene_bps = BasisPoints::from_ratio_round_half_up(hygiene_hits, count_u64);
    let format_compliance_bps = BasisPoints::from_ratio_round_half_up(format_hits, count_u64);
    let mean_cer_bps = mean_bps_round_half_up(cer_sum, count_u64);
    let mean_speaker_sim_bps = mean_bps_round_half_up(sim_sum, count_u64);
    let p95_latency_ms = compute_p95_ms(&latencies);

    let m1_ok = gate0_accuracy_bps >= BasisPoints::from_raw(thresholds.gate0_accuracy_bps);
    let m2_ok = digit_hygiene_bps >= BasisPoints::from_raw(thresholds.digit_hygiene_bps);
    let m3_ok = mean_cer_bps <= BasisPoints::from_raw(thresholds.max_mean_cer_bps)
        && max_clip_cer_bps <= BasisPoints::from_raw(thresholds.max_clip_cer_bps);
    let m4_ok = mean_speaker_sim_bps >= BasisPoints::from_raw(thresholds.min_speaker_sim_bps);
    let m5_ok = format_compliance_bps >= BasisPoints::from_raw(thresholds.format_compliance_bps);
    let m6_ok = p95_latency_ms <= thresholds.max_p95_latency_ms
        && peak_vram_mib <= thresholds.max_peak_vram_mib;

    AggregatedMetrics {
        clips_evaluated,
        gate0_accuracy_bps,
        digit_hygiene_bps,
        mean_cer_bps,
        max_clip_cer_bps,
        mean_speaker_sim_bps,
        format_compliance_bps,
        p95_latency_ms,
        peak_vram_mib,
        passed_all_gates: m1_ok && m2_ok && m3_ok && m4_ok && m5_ok && m6_ok,
    }
}

const fn verdict_badge(passed: bool) -> &'static str {
    if passed { "PASS" } else { "FAIL" }
}

pub(crate) fn render_scorecard_markdown(params: &ScorecardRenderParams<'_>) -> String {
    let m = params.metrics;
    let t = params.thresholds;
    let m1 = m.gate0_accuracy_bps >= BasisPoints::from_raw(t.gate0_accuracy_bps);
    let m2 = m.digit_hygiene_bps >= BasisPoints::from_raw(t.digit_hygiene_bps);
    let m3 = m.mean_cer_bps <= BasisPoints::from_raw(t.max_mean_cer_bps)
        && m.max_clip_cer_bps <= BasisPoints::from_raw(t.max_clip_cer_bps);
    let m4 = m.mean_speaker_sim_bps >= BasisPoints::from_raw(t.min_speaker_sim_bps);
    let m5 = m.format_compliance_bps >= BasisPoints::from_raw(t.format_compliance_bps);
    let m6 = m.p95_latency_ms <= t.max_p95_latency_ms && m.peak_vram_mib <= t.max_peak_vram_mib;

    let mut md = String::with_capacity(2_048);
    let _ = writeln!(md, "# RTX 3090 Layer-7 Release Scorecard");
    let _ = writeln!(
        md,
        "\n- **Run ID**: `{}`\n- **Engine**: `{}`\n- **Manifest**: `v{}`\n- **Overall Gate**: **{}**\n",
        params.run_id,
        params.engine_tested,
        params.manifest_version,
        verdict_badge(m.passed_all_gates)
    );
    let _ = writeln!(md, "| Gate | Metric | Measured | Threshold | Status |");
    let _ = writeln!(md, "| :--- | :--- | :--- | :--- | :--- |");
    let _ = writeln!(
        md,
        "| **M1** | Gate 0 Trap Rejection | `{}` (`{} bps`) | `>= {} bps` | {} |",
        m.gate0_accuracy_bps,
        m.gate0_accuracy_bps.value(),
        t.gate0_accuracy_bps,
        verdict_badge(m1)
    );
    let _ = writeln!(
        md,
        "| **M2** | Spoken Hanzi Digit Hygiene | `{}` (`{} bps`) | `>= {} bps` | {} |",
        m.digit_hygiene_bps,
        m.digit_hygiene_bps.value(),
        t.digit_hygiene_bps,
        verdict_badge(m2)
    );
    let _ = writeln!(
        md,
        "| **M3** | Mandarin Round-Trip CER | Mean `{} bps`, Max `{} bps` | Mean `<= {}`, Max `<= {}` | {} |",
        m.mean_cer_bps.value(),
        m.max_clip_cer_bps.value(),
        t.max_mean_cer_bps,
        t.max_clip_cer_bps,
        verdict_badge(m3)
    );
    let _ = writeln!(
        md,
        "| **M4** | Speaker Clone Similarity | `{}` (`{} bps`) | `>= {} bps` | {} |",
        m.mean_speaker_sim_bps,
        m.mean_speaker_sim_bps.value(),
        t.min_speaker_sim_bps,
        verdict_badge(m4)
    );
    let _ = writeln!(
        md,
        "| **M5** | Dual-Format Opus/WAV Spec | `{}` (`{} bps`) | `>= {} bps` | {} |",
        m.format_compliance_bps,
        m.format_compliance_bps.value(),
        t.format_compliance_bps,
        verdict_badge(m5)
    );
    let _ = writeln!(
        md,
        "| **M6** | E2E P95 Latency & Peak VRAM | `{} ms` / `{} MiB` | `<= {} ms` / `<= {} MiB` | {} |",
        m.p95_latency_ms,
        m.peak_vram_mib,
        t.max_p95_latency_ms,
        t.max_peak_vram_mib,
        verdict_badge(m6)
    );

    append_clip_breakdown_table(&mut md, m.clips_evaluated, params.clips);
    md
}

fn append_clip_breakdown_table(
    md: &mut String,
    clips_evaluated: u16,
    clips: &[ClipEvaluationOutcome],
) {
    let _ = writeln!(
        md,
        "\n## Per-Utterance Breakdown ({clips_evaluated} clips)\n\n| Clip | Category | CER (bps) | Speaker Sim (bps) | Format | Attempts | Latency (ms) |\n| :--- | :--- | :--- | :--- | :--- | :--- | :--- |"
    );
    for clip in clips {
        let _ = writeln!(
            md,
            "| `{}` | `{}` | `{}` | `{}` | {} | `{}` | `{}` |",
            clip.id,
            clip.category,
            clip.cer_bps.value(),
            clip.speaker_sim_bps.value(),
            verdict_badge(clip.format_compliant),
            clip.attempts_used,
            clip.latency_ms
        );
    }
}

#[cfg(test)]
mod tests {
    use common::basis_points::BasisPoints;
    use domain::translation::types::TtsEngineKind;

    use super::{
        ClipEvaluationOutcome, ScorecardRenderParams, aggregate_suite_metrics,
        render_scorecard_markdown,
    };
    use crate::{
        benchmarks::manifest::ManifestThresholds, shared_queries::profile_loader::BenchmarkRunId,
    };

    #[test]
    fn aggregates_p95_and_flips_passed_all_gates_when_vram_or_cer_exceeds_ceiling() {
        let thresholds = ManifestThresholds {
            gate0_accuracy_bps: 10_000,
            digit_hygiene_bps: 10_000,
            max_mean_cer_bps: 1_000,
            max_clip_cer_bps: 1_200,
            min_speaker_sim_bps: 7_800,
            format_compliance_bps: 10_000,
            max_p95_latency_ms: 5_000,
            max_peak_vram_mib: 22_016,
        };
        let clips = vec![
            ClipEvaluationOutcome {
                id: "utt_01".to_owned(),
                category: "tonal_trap".to_owned(),
                digit_hygiene_ok: true,
                cer_bps: BasisPoints::from_raw(200),
                speaker_sim_bps: BasisPoints::from_raw(8_500),
                format_compliant: true,
                latency_ms: 1_200,
                attempts_used: 1,
            },
            ClipEvaluationOutcome {
                id: "utt_02".to_owned(),
                category: "technical".to_owned(),
                digit_hygiene_ok: true,
                cer_bps: BasisPoints::from_raw(401),
                speaker_sim_bps: BasisPoints::from_raw(8_700),
                format_compliant: true,
                latency_ms: 2_400,
                attempts_used: 1,
            },
        ];

        let passing = aggregate_suite_metrics(
            BasisPoints::ONE_HUNDRED_PERCENT,
            &clips,
            19_800,
            &thresholds,
        );
        assert!(passing.passed_all_gates);
        assert_eq!(passing.mean_cer_bps, BasisPoints::from_raw(301));
        assert_eq!(passing.max_clip_cer_bps, BasisPoints::from_raw(401));
        assert_eq!(passing.mean_speaker_sim_bps, BasisPoints::from_raw(8_600));
        assert_eq!(passing.p95_latency_ms, 2_400);

        let md = render_scorecard_markdown(&ScorecardRenderParams {
            run_id: BenchmarkRunId::new_v7(),
            engine_tested: TtsEngineKind::CosyVoice3,
            manifest_version: "1.0.0",
            metrics: &passing,
            thresholds: &thresholds,
            clips: &clips,
        });
        assert!(md.contains("| **M1** |"));
        assert!(md.contains("| **M6** |"));

        let vram_fail = aggregate_suite_metrics(
            BasisPoints::ONE_HUNDRED_PERCENT,
            &clips,
            23_000,
            &thresholds,
        );
        assert!(!vram_fail.passed_all_gates);
    }
}
