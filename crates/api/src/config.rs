//! Process configuration read from the environment.
//!
//! Variable names are normative (`.env.example`); do not invent variants.
//! Fields are added as stages start using them, never speculatively.

use std::net::SocketAddr;

/// Missing or unparseable configuration. Only ever surfaced at startup, where
/// failing fast is the correct behaviour.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("missing environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for {name}: {reason}")]
    Invalid { name: &'static str, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Production,
}

/// Argon2id cost (RNF-06). Not tunable downwards without discussion.
#[derive(Debug, Clone, Copy)]
pub struct Argon2Config {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub app_env: AppEnv,
    pub bind_addr: SocketAddr,
    pub public_base_url: String,
    pub database_url: String,
    pub database_max_connections: u32,

    /// HMAC key for the access token. Never logged, never sent anywhere.
    pub jwt_signing_key: String,
    pub access_token_ttl_seconds: i64,
    pub refresh_token_ttl_days: i64,
    pub argon2: Argon2Config,
    pub gateway: GatewayConfig,

    /// Public base of stored media, used to render `attachment.url`.
    pub media_base_url: String,
    /// RF-11a.
    pub max_attachment_bytes: i64,
    pub max_attachments: usize,
    pub allowed_content_types: Vec<String>,
}

/// WebSocket gateway limits (`docs/protocol/websocket.md` §3.2, §3.3, §7).
#[derive(Debug, Clone, Copy)]
pub struct GatewayConfig {
    pub heartbeat_interval_ms: u64,
    /// How long a disconnected session stays resumable.
    pub session_ttl_ms: u64,
    /// Dispatches kept per session for replay.
    pub resume_buffer_size: usize,
    pub max_connections_per_user: usize,
}

/// Anything that can answer "what is the value of this variable".
pub trait Source {
    fn get(&self, name: &str) -> Option<String>;
}

/// The process environment.
pub struct Env;

impl Source for Env {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_source(&Env)
    }

    /// The whole parse, with the environment behind a trait so it is testable
    /// without mutating process state from several threads at once.
    pub fn from_source(source: &dyn Source) -> Result<Self, ConfigError> {
        let jwt_signing_key = required(source, "JWT_SIGNING_KEY")?;
        // A short key makes the signature decorative. Refuse at startup rather
        // than serve tokens anyone can forge.
        if jwt_signing_key.len() < 32 {
            return Err(ConfigError::Invalid {
                name: "JWT_SIGNING_KEY",
                reason: "precisa de ao menos 32 caracteres; gere com openssl rand -base64 48"
                    .into(),
            });
        }

        let argon2 = Argon2Config {
            memory_kib: parse(source, "ARGON2_MEMORY_KIB")?,
            iterations: parse(source, "ARGON2_ITERATIONS")?,
            parallelism: parse(source, "ARGON2_PARALLELISM")?,
        };
        if argon2.memory_kib < 65_536 || argon2.iterations < 3 || argon2.parallelism < 4 {
            return Err(ConfigError::Invalid {
                name: "ARGON2_MEMORY_KIB",
                reason: "abaixo do mínimo do RNF-06 (64 MiB, 3 iterações, paralelismo 4)".into(),
            });
        }

        Ok(Self {
            app_env: match required(source, "APP_ENV")?.as_str() {
                "development" => AppEnv::Development,
                "production" => AppEnv::Production,
                other => {
                    return Err(ConfigError::Invalid {
                        name: "APP_ENV",
                        reason: format!("expected development|production, got {other}"),
                    })
                }
            },
            bind_addr: parse(source, "BIND_ADDR")?,
            public_base_url: required(source, "PUBLIC_BASE_URL")?,
            database_url: required(source, "DATABASE_URL")?,
            database_max_connections: parse(source, "DATABASE_MAX_CONNECTIONS")?,
            jwt_signing_key,
            access_token_ttl_seconds: parse(source, "ACCESS_TOKEN_TTL_SECONDS")?,
            refresh_token_ttl_days: parse(source, "REFRESH_TOKEN_TTL_DAYS")?,
            argon2,
            gateway: GatewayConfig {
                heartbeat_interval_ms: parse(source, "WS_HEARTBEAT_INTERVAL_MS")?,
                session_ttl_ms: parse(source, "WS_SESSION_TTL_MS")?,
                resume_buffer_size: parse(source, "WS_RESUME_BUFFER_SIZE")?,
                max_connections_per_user: parse(source, "WS_MAX_CONNECTIONS_PER_USER")?,
            },
            media_base_url: required(source, "R2_PUBLIC_BASE_URL")?,
            max_attachment_bytes: parse(source, "MAX_ATTACHMENT_BYTES")?,
            max_attachments: parse(source, "MAX_ATTACHMENTS_PER_MESSAGE")?,
            allowed_content_types: required(source, "ALLOWED_CONTENT_TYPES")?
                .split(',')
                .map(|t| t.trim().to_ascii_lowercase())
                .filter(|t| !t.is_empty())
                .collect(),
        })
    }
}

fn required(source: &dyn Source, name: &'static str) -> Result<String, ConfigError> {
    source.get(name).ok_or(ConfigError::Missing(name))
}

fn parse<T>(source: &dyn Source, name: &'static str) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    required(source, name)?
        .parse::<T>()
        .map_err(|e| ConfigError::Invalid {
            name,
            reason: e.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Map(HashMap<&'static str, String>);

    impl Source for Map {
        fn get(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }

    fn valid() -> Map {
        let mut m = HashMap::new();
        for (k, v) in [
            ("APP_ENV", "development"),
            ("BIND_ADDR", "0.0.0.0:8080"),
            ("PUBLIC_BASE_URL", "http://localhost:8080"),
            ("DATABASE_URL", "postgres://comms:comms@localhost/comms"),
            ("DATABASE_MAX_CONNECTIONS", "10"),
            ("JWT_SIGNING_KEY", "dev-only-not-a-real-key-0123456789abcd"),
            ("ACCESS_TOKEN_TTL_SECONDS", "900"),
            ("REFRESH_TOKEN_TTL_DAYS", "30"),
            ("ARGON2_MEMORY_KIB", "65536"),
            ("ARGON2_ITERATIONS", "3"),
            ("ARGON2_PARALLELISM", "4"),
            ("WS_HEARTBEAT_INTERVAL_MS", "30000"),
            ("WS_SESSION_TTL_MS", "90000"),
            ("WS_RESUME_BUFFER_SIZE", "500"),
            ("WS_MAX_CONNECTIONS_PER_USER", "4"),
            ("R2_PUBLIC_BASE_URL", "https://media.exemplo.com"),
            ("MAX_ATTACHMENT_BYTES", "26214400"),
            ("MAX_ATTACHMENTS_PER_MESSAGE", "10"),
            (
                "ALLOWED_CONTENT_TYPES",
                "image/webp,image/png,image/jpeg,image/gif,video/mp4",
            ),
        ] {
            m.insert(k, v.to_string());
        }
        Map(m)
    }

    #[test]
    fn the_example_environment_parses() {
        let config = Config::from_source(&valid()).expect("o .env.example precisa ser válido");
        assert_eq!(config.app_env, AppEnv::Development);
        assert_eq!(config.access_token_ttl_seconds, 900);
        assert_eq!(config.argon2.memory_kib, 65_536);
        assert_eq!(config.gateway.resume_buffer_size, 500);
        assert_eq!(config.gateway.session_ttl_ms, 90_000);
        assert_eq!(config.max_attachment_bytes, 26_214_400);
        assert_eq!(config.max_attachments, 10);
        assert!(config
            .allowed_content_types
            .contains(&"image/webp".to_string()));
    }

    #[test]
    fn a_missing_variable_names_itself() {
        let mut source = valid();
        source.0.remove("DATABASE_URL");
        assert_eq!(
            Config::from_source(&source).unwrap_err(),
            ConfigError::Missing("DATABASE_URL")
        );
    }

    #[test]
    fn a_signing_key_too_short_to_matter_is_refused_at_startup() {
        let mut source = valid();
        source.0.insert("JWT_SIGNING_KEY", "curta".into());
        let err = Config::from_source(&source).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                name: "JWT_SIGNING_KEY",
                ..
            }
        ));
    }

    #[test]
    fn argon2_below_rnf06_is_refused() {
        // Reduzir o custo em produção por engano é indetectável em runtime:
        // logins continuam funcionando, só que baratos de quebrar.
        for (key, value) in [
            ("ARGON2_MEMORY_KIB", "4096"),
            ("ARGON2_ITERATIONS", "1"),
            ("ARGON2_PARALLELISM", "1"),
        ] {
            let mut source = valid();
            source.0.insert(key, value.into());
            assert!(
                Config::from_source(&source).is_err(),
                "{key}={value} deveria ser recusado"
            );
        }
    }

    #[test]
    fn an_unknown_app_env_is_refused_rather_than_defaulted() {
        let mut source = valid();
        source.0.insert("APP_ENV", "staging".into());
        assert!(Config::from_source(&source).is_err());
    }
}
