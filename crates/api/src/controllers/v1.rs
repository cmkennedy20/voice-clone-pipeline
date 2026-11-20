//! Version 1 (`v1`) `HTTP` controller routing (`config`, `languages`, `agent`, `system`, `profiles`, `voice-jobs`, `benchmarks`).

use axum::Router;
use framework::context::AppContext;

pub mod benchmarks_controller;
pub mod desktop_controller;
pub mod profile_samples_controller;
pub mod profiles_controller;
pub mod system_controller;
pub mod voice_jobs_controller;

/// Builds the versioned `v1` router tree mounted under both `/api/v1` and `/v1`.
pub fn routes() -> Router<AppContext> {
    Router::new()
        .merge(system_controller::routes())
        .merge(desktop_controller::routes())
        .nest("/profiles", profiles_controller::routes())
        .nest("/voice-jobs", voice_jobs_controller::routes())
        .nest("/benchmarks", benchmarks_controller::routes())
}
