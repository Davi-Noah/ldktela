//! Shared fixture: one PostgreSQL 16 container per test binary, one fresh
//! database per test.
//!
//! CLAUDE.md §2.10 forbids repository mocks; integration tests run against a real
//! database. Panicking here is fine — this is test-only setup code.
//!
//! Note on what is and is not shared: only the container and its base URL live
//! in the `OnceCell`. A `PgPool` must **not**, because `#[tokio::test]` gives
//! every test its own runtime and a pool spawns a reaper task on the runtime
//! that created it — once that runtime shuts down, later tests fail with
//! "a Tokio 1.x context was found, but it is being shutdown". Each test opens
//! its own short-lived maintenance connection instead.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Connection, PgConnection};
use testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

static SERVER: OnceCell<PgServer> = OnceCell::const_new();
static SWEEP: OnceCell<()> = OnceCell::const_new();
static NEXT_DB: AtomicU32 = AtomicU32::new(0);
/// Every scratch database carries this prefix so leftovers are
/// recognisable and can be swept.
const TEST_DB_PREFIX: &str = "ldkcord_test_t";
/// `CREATE DATABASE` serialises on `template1` inside PostgreSQL anyway.
static CREATING: Mutex<()> = Mutex::const_new(());

struct PgServer {
    base_url: String,
    /// `None` when the tests run against the development database from
    /// `docker/compose.dev.yml` instead of a container of their own.
    _container: Option<ContainerAsync<Postgres>>,
}

async fn base_url() -> &'static str {
    &SERVER
        .get_or_init(|| async {
            // SRS §11.7 asks for a real PostgreSQL, by container **or** by the
            // compose service. Reusing the running one matters here: every test
            // binary is its own process, so a container per binary meant a dozen
            // PostgreSQL instances alive at once during `just check`, and the
            // connection exhaustion that follows looks like a flaky test.
            if let Some(url) = std::env::var("DATABASE_URL").ok().and_then(strip_database) {
                if PgConnection::connect(&format!("{url}/postgres"))
                    .await
                    .is_ok()
                {
                    return PgServer {
                        base_url: url,
                        _container: None,
                    };
                }
            }
            let container = Postgres::default()
                .with_tag("16-alpine")
                .start()
                .await
                .expect("starting postgres container");
            let port = container
                .get_host_port_ipv4(5432)
                .await
                .expect("resolving mapped port");
            PgServer {
                base_url: format!("postgres://postgres:postgres@127.0.0.1:{port}"),
                _container: Some(container),
            }
        })
        .await
        .base_url
}

/// Drops scratch databases left by earlier runs. Best effort: one still in
/// use simply fails to drop, which is the correct outcome.
async fn sweep_leftovers(base: &str) {
    let Ok(mut conn) = PgConnection::connect(&format!("{base}/postgres")).await else {
        return;
    };
    let names: Vec<String> =
        sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE $1")
            .bind(format!("{TEST_DB_PREFIX}%"))
            .fetch_all(&mut conn)
            .await
            .unwrap_or_default();
    for name in names {
        let _ = sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{name}""#))
            .execute(&mut conn)
            .await;
    }
    let _ = conn.close().await;
}

/// Turns `postgres://user:pass@host:port/db` into `postgres://user:pass@host:port`.
fn strip_database(url: String) -> Option<String> {
    let scheme_end = url.find("://")? + 3;
    let rest = &url[scheme_end..];
    let cut = rest.find('/').map(|i| scheme_end + i).unwrap_or(url.len());
    Some(url[..cut].to_string())
}

/// Creates a fresh database and returns its URL.
pub async fn create_database() -> String {
    let base = base_url().await;
    SWEEP.get_or_init(|| sweep_leftovers(base)).await;
    // Unique per process: every test binary is its own process against the
    // same server, so a per-binary counter alone collides on the second one.
    let name = format!(
        "{}{}_{}",
        TEST_DB_PREFIX,
        std::process::id(),
        NEXT_DB.fetch_add(1, Ordering::SeqCst)
    );
    let _guard = CREATING.lock().await;
    let mut conn = PgConnection::connect(&format!("{base}/postgres"))
        .await
        .expect("connecting to the maintenance database");
    sqlx::query(&format!(r#"CREATE DATABASE "{name}""#))
        .execute(&mut conn)
        .await
        .expect("creating the test database");
    let _ = conn.close().await;
    format!("{base}/{name}")
}

pub async fn pool_for(url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(30))
        .connect(url)
        .await
        .expect("connecting to the test database")
}

/// A database of its own, with the full schema applied.
pub struct TestDb {
    pub pool: PgPool,
}

impl TestDb {
    /// Creates a fresh database and applies every migration.
    pub async fn migrated() -> Self {
        let db = Self::empty().await;
        db::MIGRATOR
            .run(&db.pool)
            .await
            .expect("applying migrations");
        db
    }

    /// Creates a fresh database with no schema.
    pub async fn empty() -> Self {
        let url = create_database().await;
        Self {
            pool: pool_for(&url).await,
        }
    }
}

/// Names of every application table. Kept in sync with SRS v2.0 §5 by hand:
/// drift here is exactly what the migration test looks for.
///
/// Five tables. If this list starts growing, the question to ask is whether the
/// thing being persisted is really ours or Discord's (ADR-0010).
pub const EXPECTED_TABLES: &[&str] = &[
    "pairing_codes",
    "refresh_tokens",
    "room_presence",
    "share_sessions",
    "users",
];

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

// ---------------------------------------------------------------------------
// Semeadura
// ---------------------------------------------------------------------------

/// A paired account. `discord_user_id` is what everything else keys on, so
/// tests pass it explicitly rather than letting a helper invent one.
pub async fn seed_user(pool: &PgPool, discord_user_id: i64, username: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, discord_user_id, username, display_name) \
         VALUES ($1, $2, $3, $3)",
    )
    .bind(id)
    .bind(discord_user_id)
    .bind(username)
    .execute(pool)
    .await
    .expect("seeding user");
    id
}
