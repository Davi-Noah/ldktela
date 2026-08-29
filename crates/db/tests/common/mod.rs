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
static NEXT_DB: AtomicU32 = AtomicU32::new(0);
/// `CREATE DATABASE` serialises on `template1` inside PostgreSQL anyway.
static CREATING: Mutex<()> = Mutex::const_new(());

struct PgServer {
    base_url: String,
    /// Kept alive for the life of the process; Docker reclaims it on exit.
    _container: ContainerAsync<Postgres>,
}

async fn base_url() -> &'static str {
    &SERVER
        .get_or_init(|| async {
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
                _container: container,
            }
        })
        .await
        .base_url
}

/// Creates a fresh database and returns its URL.
pub async fn create_database(prefix: &str) -> String {
    let base = base_url().await;
    let name = format!("{prefix}{}", NEXT_DB.fetch_add(1, Ordering::SeqCst));
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
        .max_connections(8)
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
        let url = create_database("t").await;
        Self {
            pool: pool_for(&url).await,
        }
    }
}

/// Names of every application table. Kept in sync with SRS §5.2 by hand: drift
/// here is exactly what the migration test looks for.
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

/// A real account with a usable credential.
pub async fn seed_user(pool: &PgPool, username: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, username, password_hash, is_migrated) \
         VALUES ($1, $2, $3, 'argon2-placeholder', FALSE)",
    )
    .bind(id)
    .bind(format!("{username}@exemplo.test"))
    .bind(username)
    .execute(pool)
    .await
    .expect("seeding user");
    id
}

/// A guild with its `@everyone` role and the owner already a member.
/// Returns `(guild_id, everyone_role_id)`.
pub async fn seed_guild(pool: &PgPool, owner: Uuid, everyone_permissions: i64) -> (Uuid, Uuid) {
    let guild = Uuid::now_v7();
    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, 'guild', $2)")
        .bind(guild)
        .bind(owner)
        .execute(pool)
        .await
        .expect("seeding guild");
    let everyone = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, guild_id, name, permissions, is_default) \
         VALUES ($1, $2, '@everyone', $3, TRUE)",
    )
    .bind(everyone)
    .bind(guild)
    .bind(everyone_permissions)
    .execute(pool)
    .await
    .expect("seeding @everyone");
    join_guild(pool, guild, owner).await;
    (guild, everyone)
}

pub async fn join_guild(pool: &PgPool, guild: Uuid, user: Uuid) {
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild)
        .bind(user)
        .execute(pool)
        .await
        .expect("joining guild");
}

pub async fn seed_role(pool: &PgPool, guild: Uuid, name: &str, permissions: i64) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO roles (id, guild_id, name, permissions) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(guild)
        .bind(name)
        .bind(permissions)
        .execute(pool)
        .await
        .expect("seeding role");
    id
}

pub async fn assign_role(pool: &PgPool, guild: Uuid, user: Uuid, role: Uuid) {
    sqlx::query("INSERT INTO member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(guild)
        .bind(user)
        .bind(role)
        .execute(pool)
        .await
        .expect("assigning role");
}

pub async fn seed_text_channel(pool: &PgPool, guild: Uuid, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO channels (id, guild_id, name, type) VALUES ($1, $2, $3, 'text')")
        .bind(id)
        .bind(guild)
        .bind(name)
        .execute(pool)
        .await
        .expect("seeding channel");
    id
}

pub async fn set_overwrite(
    pool: &PgPool,
    channel: Uuid,
    target_type: &str,
    target: Uuid,
    allow: i64,
    deny: i64,
) {
    sqlx::query(
        "INSERT INTO channel_overwrites (channel_id, target_type, target_id, allow, deny) \
         VALUES ($1, $2::overwrite_target, $3, $4, $5) \
         ON CONFLICT (channel_id, target_type, target_id) \
         DO UPDATE SET allow = EXCLUDED.allow, deny = EXCLUDED.deny",
    )
    .bind(channel)
    .bind(target_type)
    .bind(target)
    .bind(allow)
    .bind(deny)
    .execute(pool)
    .await
    .expect("setting overwrite");
}
