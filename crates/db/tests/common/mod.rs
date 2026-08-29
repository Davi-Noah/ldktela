//! Shared fixture: a throwaway PostgreSQL 16 container per test binary.
//!
//! CLAUDE.md §2.10 forbids repository mocks; integration tests run against a real
//! database. Panicking here is fine — this is test-only setup code.

#![allow(dead_code)]

use sqlx::postgres::{PgPool, PgPoolOptions};
use testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres;

/// A running database plus its pool. Dropping it stops the container, so keep it
/// alive for the whole test.
pub struct TestDb {
    pub pool: PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestDb {
    /// Starts an empty database. No migrations are applied.
    pub async fn empty() -> Self {
        let container = Postgres::default()
            .with_tag("16-alpine")
            .start()
            .await
            .expect("starting postgres container");
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("resolving mapped port");
        let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connecting to test database");
        Self {
            pool,
            _container: container,
        }
    }

    /// Starts a database with the full schema applied.
    pub async fn migrated() -> Self {
        let db = Self::empty().await;
        db::MIGRATOR
            .run(&db.pool)
            .await
            .expect("applying migrations");
        db
    }
}

/// Names of every application table, in no particular order. Kept in sync with
/// SRS §5.2 by hand: a drift here is the point of the test that reads it.
pub const EXPECTED_TABLES: &[&str] = &[
    "attachments",
    "bridge_outbox",
    "categories",
    "channel_overwrites",
    "channel_participants",
    "channel_webhooks",
    "channels",
    "guild_members",
    "guilds",
    "invites",
    "member_roles",
    "mentions",
    "message_mappings",
    "messages",
    "reactions",
    "read_states",
    "refresh_tokens",
    "roles",
    "users",
    "voice_states",
];

/// Tables present in `public`, excluding SQLx bookkeeping.
pub async fn application_tables(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT tablename FROM pg_tables \
         WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations' \
         ORDER BY tablename",
    )
    .fetch_all(pool)
    .await
    .expect("listing tables")
}

/// User-defined enum types in `public`.
pub async fn application_enums(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT t.typname FROM pg_type t \
         JOIN pg_namespace n ON n.oid = t.typnamespace \
         WHERE n.nspname = 'public' AND t.typtype = 'e' \
         ORDER BY t.typname",
    )
    .fetch_all(pool)
    .await
    .expect("listing enum types")
}
