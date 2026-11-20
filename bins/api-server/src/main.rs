//! Tier 6 container & desktop entrypoint for `api-server` (`:8080` `HTTP` + embedded `SPA` + `GPU`).

use std::{fmt, process::ExitCode};

use domain::ports::ExternalPortError;
use framework::{
    config::{AppConfig, ConfigError},
    context::AppContext,
    db::pool::{connect_pg_pool, run_migrations},
    telemetry::init_telemetry,
};

const DEFAULT_MAX_DB_CONNECTIONS: u32 = 16;

#[derive(Debug)]
enum ApiServerError {
    Config(ConfigError),
    Database(String),
    Adapter(ExternalPortError),
    Io(std::io::Error),
}

impl fmt::Display for ApiServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(f, "configuration error: {err}"),
            Self::Database(err) => write!(f, "database initialization failed: {err}"),
            Self::Adapter(err) => write!(f, "production adapter initialization failed: {err}"),
            Self::Io(err) => write!(f, "network I/O error: {err}"),
        }
    }
}

impl std::error::Error for ApiServerError {}

impl From<ConfigError> for ApiServerError {
    fn from(err: ConfigError) -> Self {
        Self::Config(err)
    }
}

impl From<ExternalPortError> for ApiServerError {
    fn from(err: ExternalPortError) -> Self {
        Self::Adapter(err)
    }
}

impl From<std::io::Error> for ApiServerError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

async fn wait_for_ctrl_c() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => tracing::info!("received Ctrl-C; initiating graceful HTTP shutdown"),
        Err(err) => tracing::warn!(error = %err, "failed listening for Ctrl-C signal"),
    }
}

async fn run() -> Result<(), ApiServerError> {
    let config = AppConfig::from_env()?;
    init_telemetry(&config.rust_log);

    let bind_addr = config.api_bind_addr.clone();
    let db = connect_pg_pool(&config.database_url, DEFAULT_MAX_DB_CONNECTIONS)
        .await
        .map_err(|err| ApiServerError::Database(err.to_string()))?;
    if config.desktop.auto_migrate {
        run_migrations(&db)
            .await
            .map_err(|err| ApiServerError::Database(format!("migration failed: {err}")))?;
    }
    let ctx = AppContext::from_production_adapters(config, db)?;
    let router = api::build_router(ctx);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!(bind_addr = %bind_addr, "api-server listening");

    api::axum::serve(listener, router)
        .with_graceful_shutdown(wait_for_ctrl_c())
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(err) = run().await {
        init_telemetry("info");
        tracing::error!(error = %err, "api-server terminated with fatal error");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
