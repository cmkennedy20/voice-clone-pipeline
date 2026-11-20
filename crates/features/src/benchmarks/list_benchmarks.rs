//! Paginated read projection (`list_benchmark_runs`) for persisted `benchmark_runs` scorecards.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use common::{
    basis_points::BasisPoints,
    pagination::{PageParams, Paged},
};
use domain::translation::types::TtsEngineKind;
use framework::context::AppContext;
use uuid::Uuid;

use crate::{
    error::AppError,
    shared_queries::profile_loader::{BenchmarkRunId, ensure_caller_is_owner},
};

/// Summary row projected from `benchmark_runs` for history tables and dashboards.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkRunSummary {
    pub id: BenchmarkRunId,
    pub engine_tested: TtsEngineKind,
    pub clips_evaluated: u16,
    pub gate0_accuracy_bps: BasisPoints,
    pub digit_hygiene_bps: BasisPoints,
    pub mean_cer_bps: BasisPoints,
    pub mean_speaker_sim_bps: BasisPoints,
    pub format_compliance_bps: BasisPoints,
    pub p95_latency_ms: u32,
    pub peak_vram_mib: u32,
    pub passed_all_gates: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
struct BenchmarkRunRow {
    id: Uuid,
    engine_tested: String,
    clips_evaluated: i16,
    gate0_accuracy_bps: i32,
    digit_hygiene_bps: i32,
    mean_cer_bps: i32,
    mean_speaker_sim_bps: i32,
    format_compliance_bps: i32,
    p95_latency_ms: i32,
    peak_vram_mib: i32,
    passed_all_gates: bool,
    created_at: DateTime<Utc>,
}

fn non_neg_u32(val: i32, field: &str) -> Result<u32, AppError> {
    u32::try_from(val).map_err(|_| AppError::Internal(format!("negative {field} stored: {val}")))
}

impl TryFrom<BenchmarkRunRow> for BenchmarkRunSummary {
    type Error = AppError;

    fn try_from(row: BenchmarkRunRow) -> Result<Self, Self::Error> {
        let engine_tested = TtsEngineKind::from_str(&row.engine_tested)
            .map_err(|err| AppError::Internal(format!("stored engine_tested invalid: {err}")))?;
        let clips_evaluated = u16::try_from(row.clips_evaluated).map_err(|_| {
            AppError::Internal(format!(
                "negative clips_evaluated stored: {}",
                row.clips_evaluated
            ))
        })?;

        Ok(Self {
            id: BenchmarkRunId::from_uuid(row.id),
            engine_tested,
            clips_evaluated,
            gate0_accuracy_bps: BasisPoints::from_raw(non_neg_u32(
                row.gate0_accuracy_bps,
                "gate0_accuracy_bps",
            )?),
            digit_hygiene_bps: BasisPoints::from_raw(non_neg_u32(
                row.digit_hygiene_bps,
                "digit_hygiene_bps",
            )?),
            mean_cer_bps: BasisPoints::from_raw(non_neg_u32(row.mean_cer_bps, "mean_cer_bps")?),
            mean_speaker_sim_bps: BasisPoints::from_raw(non_neg_u32(
                row.mean_speaker_sim_bps,
                "mean_speaker_sim_bps",
            )?),
            format_compliance_bps: BasisPoints::from_raw(non_neg_u32(
                row.format_compliance_bps,
                "format_compliance_bps",
            )?),
            p95_latency_ms: non_neg_u32(row.p95_latency_ms, "p95_latency_ms")?,
            peak_vram_mib: non_neg_u32(row.peak_vram_mib, "peak_vram_mib")?,
            passed_all_gates: row.passed_all_gates,
            created_at: row.created_at,
        })
    }
}

/// Lists persisted [`BenchmarkRunSummary`] rows ordered newest-first.
pub async fn list_benchmark_runs(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    page: PageParams,
) -> Result<Paged<BenchmarkRunSummary>, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let clamped_page = PageParams::new(page.limit, page.offset);
    let limit_i64 = i64::from(clamped_page.limit);
    let offset_i64 = i64::from(clamped_page.offset);

    let (total_i64,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM benchmark_runs")
        .fetch_one(&ctx.db)
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed counting benchmark_runs"))?;

    let total = u64::try_from(total_i64).unwrap_or(0);
    let rows = sqlx::query_as::<_, BenchmarkRunRow>(
        "SELECT id, engine_tested, clips_evaluated, gate0_accuracy_bps, digit_hygiene_bps,
                mean_cer_bps, mean_speaker_sim_bps, format_compliance_bps, p95_latency_ms,
                peak_vram_mib, passed_all_gates, created_at
           FROM benchmark_runs
          ORDER BY created_at DESC, id DESC
          LIMIT $1 OFFSET $2",
    )
    .bind(limit_i64)
    .bind(offset_i64)
    .fetch_all(&ctx.db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed listing benchmark_runs"))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(BenchmarkRunSummary::try_from(row)?);
    }

    Ok(Paged::new(items, total, clamped_page))
}
