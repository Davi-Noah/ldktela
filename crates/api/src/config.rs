//! Process configuration read from the environment.
//!
//! Variable names are normative (`.env.example`); do not invent variants.
//! Fields are added as stages start using them, never speculatively.

use std::net::SocketAddr;

/// Missing or unparseable configuration. Only ever surfaced at startup, where
/// failing fast is the correct behaviour.
#[derive(Debug, thiserror::Error)]
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

#[derive(Debug, Clone)]
pub struct Config {
    pub app_env: AppEnv,
    pub bind_addr: SocketAddr,
    pub public_base_url: String,
    pub database_url: String,
    pub database_max_connections: u32,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            app_env: match required("APP_ENV")?.as_str() {
                "development" => AppEnv::Development,
                "production" => AppEnv::Production,
                other => {
                    return Err(ConfigError::Invalid {
                        name: "APP_ENV",
                        reason: format!("expected development|production, got {other}"),
                    })
                }
            },
            bind_addr: parse("BIND_ADDR", &required("BIND_ADDR")?)?,
            public_base_url: required("PUBLIC_BASE_URL")?,
            database_url: required("DATABASE_URL")?,
            database_max_connections: parse(
                "DATABASE_MAX_CONNECTIONS",
                &required("DATABASE_MAX_CONNECTIONS")?,
            )?,
        })
    }
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    std::env::var(name).map_err(|_| ConfigError::Missing(name))
}

fn parse<T>(name: &'static str, raw: &str) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    raw.parse::<T>().map_err(|e| ConfigError::Invalid {
        name,
        reason: e.to_string(),
    })
}
