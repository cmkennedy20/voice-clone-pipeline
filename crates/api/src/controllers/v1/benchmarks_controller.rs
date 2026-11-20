//! Thin `v1` `RTX 3090` 6-metric release benchmark suite controllers (`GET /`, `POST /run`).

use axum::{
    Json, Router,
    extract::{
        Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    routing::{get, post},
};
use common::pagination::{PageParams, Paged};
use features::benchmarks::{
    list_benchmarks::{BenchmarkRunSummary, list_benchmark_runs},
    run_rtx3090_suite::{BenchmarkSuiteReport, RunBenchmarkSuiteInput, run_rtx3090_suite},
};
use framework::context::AppContext;
use serde::Deserialize;

use crate::{error::ApiError, extractors::AuthenticatedOwner};

/// Optional pagination query parameters for `GET /v1/benchmarks`.
#[derive(Debug, Default, Deserialize)]
pub struct BenchmarkPageQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Mounts `GET /` and `POST /run` under `/v1/benchmarks`.
pub fn routes() -> Router<AppContext> {
    Router::new()
        .route("/", get(get_benchmark_runs))
        .route("/run", post(post_run_benchmark_suite))
}

async fn get_benchmark_runs(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    query: Result<Query<BenchmarkPageQuery>, QueryRejection>,
) -> Result<(StatusCode, Json<Paged<BenchmarkRunSummary>>), ApiError> {
    let Query(q) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let page = PageParams::new(
        q.limit.unwrap_or(PageParams::DEFAULT_LIMIT),
        q.offset.unwrap_or(0),
    );
    let runs = list_benchmark_runs(&ctx, owner.discord_user_id, page).await?;
    Ok((StatusCode::OK, Json(runs)))
}

async fn post_run_benchmark_suite(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    payload: Result<Json<RunBenchmarkSuiteInput>, JsonRejection>,
) -> Result<(StatusCode, Json<BenchmarkSuiteReport>), ApiError> {
    let Json(input) = payload.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let report = run_rtx3090_suite(&ctx, owner.discord_user_id, input).await?;
    Ok((StatusCode::OK, Json(report)))
}
