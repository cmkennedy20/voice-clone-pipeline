//! `PostgreSQL` connection pool builder, embedded schema migrator, and readiness check.

use std::time::Duration;

use common::secret::Secret;
use sqlx::{PgPool, postgres::PgPoolOptions};

/// Connects a bounded `PostgreSQL` connection pool using `rustls` transport.
pub async fn connect_pg_pool(
    database_url: &Secret<String>,
    max_connections: u32,
) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections.max(1))
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url.expose_secret())
        .await
}

/// Applies embedded `SQLx` schema migrations idempotently without external `sqlx-cli`.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(pool).await
}

/// Verifies `PostgreSQL` connectivity with a lightweight `SELECT 1` round-trip.
pub async fn check_db_ready(pool: &PgPool) -> Result<(), sqlx::Error> {
    let _: (i32,) = sqlx::query_as("SELECT 1").fetch_one(pool).await?;
    Ok(())
}
