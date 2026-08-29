//! Database access: connection pool, embedded migrations and repositories.

use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};

/// Migrations embedded in the binary so a deploy cannot drift from the schema.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// Opens the connection pool. Fails fast: the process cannot serve without it.
pub async fn connect(database_url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await
}
