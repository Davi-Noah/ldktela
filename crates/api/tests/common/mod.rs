//! HTTP-level fixture: a real PostgreSQL 16 behind a real axum router.
//!
//! Requests go through the actual router, so middleware, extractors and the
//! error body shape are exercised, not bypassed.
//!
//! Only the container and its base URL are shared. A `PgPool` must not be:
//! `#[tokio::test]` gives every test its own runtime, and a pool spawns a reaper
//! task on the runtime that created it, so a shared pool dies as soon as the
//! first test finishes.

#![allow(dead_code)]

pub mod fake_s3;
pub mod gateway;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Connection, PgConnection};
use testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, OnceCell};
use tower::ServiceExt;

use api::config::{AppEnv, Argon2Config, Config, GatewayConfig};
use api::state::AppState;
use api::storage::StorageConfig;
use api::voice::VoiceConfig;
use uuid::Uuid;

static SERVER: OnceCell<PgServer> = OnceCell::const_new();
static SWEEP: OnceCell<()> = OnceCell::const_new();
static NEXT_DB: AtomicU32 = AtomicU32::new(0);
/// Every scratch database carries this prefix so leftovers are
/// recognisable and can be swept.
const TEST_DB_PREFIX: &str = "ldkcord_test_a";
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

async fn create_database() -> String {
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

/// Argon2 at RNF-06 cost is ~100 ms per hash; an HTTP test that registers and
/// logs in a dozen times would spend all its time there. The parameters are
/// exercised for real by `crates/api/src/auth/password.rs`.
pub fn test_config(database_url: String, storage_endpoint: String) -> Config {
    Config {
        app_env: AppEnv::Development,
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        public_base_url: "http://localhost:8080".into(),
        database_url,
        database_max_connections: 8,
        jwt_signing_key: "dev-only-not-a-real-key-0123456789abcdef".into(),
        access_token_ttl_seconds: 900,
        refresh_token_ttl_days: 30,
        argon2: Argon2Config {
            memory_kib: 8192,
            iterations: 1,
            parallelism: 1,
        },
        gateway: GatewayConfig {
            heartbeat_interval_ms: 30_000,
            session_ttl_ms: 90_000,
            resume_buffer_size: 500,
            max_connections_per_user: 4,
        },
        media_base_url: "https://media.exemplo.test".into(),
        max_attachment_bytes: 26_214_400,
        max_attachments: 10,
        allowed_content_types: vec![
            "image/webp".into(),
            "image/png".into(),
            "image/jpeg".into(),
            "image/gif".into(),
            "video/mp4".into(),
        ],
        storage: StorageConfig {
            endpoint: storage_endpoint,
            bucket: "comms-media".into(),
            access_key_id: "dev-only-not-a-real-key".into(),
            secret_access_key: "dev-only-not-a-real-key".into(),
            presign_ttl_seconds: 300,
        },
        voice: VoiceConfig {
            url: "ws://localhost:7880".into(),
            api_key: "devkey".into(),
            api_secret: "dev-only-not-a-real-key-0123456789abcdef".into(),
            token_ttl_seconds: 3600,
            max_camera_publishers: 3,
            idle_room_timeout_seconds: 900,
        },
    }
}

pub struct TestApp {
    pub router: Router,
    pub pool: PgPool,
    /// The fake object store the app is pointed at.
    pub s3: fake_s3::FakeS3,
    pub state: AppState,
}

impl TestApp {
    pub async fn spawn() -> Self {
        let url = create_database().await;
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(30))
            .connect(&url)
            .await
            .expect("connecting to the test database");
        db::MIGRATOR.run(&pool).await.expect("applying migrations");

        let (s3, endpoint) = fake_s3::FakeS3::spawn().await;
        let state = AppState::new(pool.clone(), test_config(url, endpoint));
        Self {
            router: api::router(state.clone()),
            pool,
            s3,
            state,
        }
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Value, axum::http::HeaderMap) {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router responded");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("reading body")
            .to_bytes();
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, json, headers)
    }

    pub async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/v1{path}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, json, _) = self.send(request).await;
        (status, json)
    }

    pub async fn post_full(
        &self,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value, axum::http::HeaderMap) {
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/v1{path}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        self.send(request).await
    }

    pub async fn get(&self, path: &str, token: Option<&str>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method("GET")
            .uri(format!("/api/v1{path}"));
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        let (status, json, _) = self.send(builder.body(Body::empty()).unwrap()).await;
        (status, json)
    }

    pub async fn patch(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/api/v1{path}"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, json, _) = self.send(request).await;
        (status, json)
    }

    /// Creates an invite bound to a guild.
    pub async fn seed_invite(&self, code: &str, max_uses: i32, guild_id: Option<Uuid>) {
        let admin = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO users (id, email, username, password_hash, is_migrated)              VALUES ($1, $2, $3, 'x', FALSE) ON CONFLICT DO NOTHING",
        )
        .bind(admin)
        .bind(format!("admin-{code}@exemplo.test"))
        .bind(format!("admin{}", code.to_lowercase()))
        .execute(&self.pool)
        .await
        .expect("seeding admin");
        sqlx::query(
            "INSERT INTO invites (id, code, created_by, guild_id, max_uses)              VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(code)
        .bind(admin)
        .bind(guild_id)
        .bind(max_uses)
        .execute(&self.pool)
        .await
        .expect("seeding invite");
    }

    /// Registers an account and returns the auth response body.
    pub async fn register(&self, username: &str, code: &str) -> Value {
        self.register_into(username, code, None).await
    }

    /// Registers an account that joins `guild_id` through the invite.
    pub async fn register_into(&self, username: &str, code: &str, guild_id: Option<Uuid>) -> Value {
        self.seed_invite(code, 1, guild_id).await;
        let (status, body) = self
            .post(
                "/auth/register",
                serde_json::json!({
                    "invite_code": code,
                    "email": format!("{username}@exemplo.test"),
                    "username": username,
                    "password": "senha-de-teste-123",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "registro falhou: {body}");
        body
    }
}

/// Reads `error.code` from a response body, failing loudly if the shape is wrong.
pub fn error_code(body: &Value) -> &str {
    body["error"]["code"]
        .as_str()
        .unwrap_or_else(|| panic!("corpo de erro fora do formato do contrato: {body}"))
}

/// Structural seeding for the routes under test. These write directly because
/// the REST contract has no guild-creation endpoint (see `docs/DECISIONS.md`).
impl TestApp {
    pub async fn seed_guild(&self, owner: Uuid, everyone_permissions: i64) -> (Uuid, Uuid) {
        let guild = Uuid::now_v7();
        sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, 'guild', $2)")
            .bind(guild)
            .bind(owner)
            .execute(&self.pool)
            .await
            .expect("seeding guild");
        let everyone = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO roles (id, guild_id, name, permissions, is_default)              VALUES ($1, $2, '@everyone', $3, TRUE)",
        )
        .bind(everyone)
        .bind(guild)
        .bind(everyone_permissions)
        .execute(&self.pool)
        .await
        .expect("seeding @everyone");
        self.join_guild(guild, owner).await;
        (guild, everyone)
    }

    pub async fn join_guild(&self, guild: Uuid, user: Uuid) {
        sqlx::query(
            "INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)              ON CONFLICT DO NOTHING",
        )
        .bind(guild)
        .bind(user)
        .execute(&self.pool)
        .await
        .expect("joining guild");
    }

    pub async fn seed_channel(&self, guild: Uuid, name: &str) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO channels (id, guild_id, name, type) VALUES ($1, $2, $3, 'text')")
            .bind(id)
            .bind(guild)
            .bind(name)
            .execute(&self.pool)
            .await
            .expect("seeding channel");
        id
    }

    pub async fn set_overwrite(
        &self,
        channel: Uuid,
        target_type: &str,
        target: Uuid,
        allow: i64,
        deny: i64,
    ) {
        sqlx::query(
            "INSERT INTO channel_overwrites (channel_id, target_type, target_id, allow, deny)              VALUES ($1, $2::overwrite_target, $3, $4, $5)              ON CONFLICT (channel_id, target_type, target_id)              DO UPDATE SET allow = EXCLUDED.allow, deny = EXCLUDED.deny",
        )
        .bind(channel)
        .bind(target_type)
        .bind(target)
        .bind(allow)
        .bind(deny)
        .execute(&self.pool)
        .await
        .expect("setting overwrite");
    }

    /// The user id carried by an access token, without decoding the JWT here.
    pub async fn user_id_by_username(&self, username: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
            .bind(username)
            .fetch_one(&self.pool)
            .await
            .expect("user exists")
    }

    pub async fn put(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("PUT")
            .uri(format!("/api/v1{path}"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, json, _) = self.send(request).await;
        (status, json)
    }

    pub async fn delete(&self, path: &str, token: &str) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1{path}"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let (status, json, _) = self.send(request).await;
        (status, json)
    }

    pub async fn post_auth(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/v1{path}"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, json, _) = self.send(request).await;
        (status, json)
    }
}

/// Posts a LiveKit webhook body, optionally with an `Authorization` header.
///
/// The body is sent as text: the endpoint verifies the signature over the raw
/// bytes before parsing, so it must not go through a JSON extractor.
impl TestApp {
    pub async fn post_webhook(&self, body: &str, authorization: Option<&str>) -> StatusCode {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/api/v1/internal/livekit/webhook")
            .header("content-type", "application/webhook+json");
        if let Some(auth) = authorization {
            builder = builder.header("authorization", auth);
        }
        let (status, _, _) = self
            .send(builder.body(Body::from(body.to_owned())).unwrap())
            .await;
        status
    }
}
