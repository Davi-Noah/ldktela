//! Process configuration read from the environment.
//!
//! Variable names are normative (`.env.example`); do not invent variants.
//! Fields are added as stages start using them, never speculatively.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;

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

/// WebSocket gateway limits (`docs/websocket.md` §3.2, §3.3, §7).
#[derive(Debug, Clone, Copy)]
pub struct GatewayConfig {
    pub heartbeat_interval_ms: u64,
    /// How long a disconnected session stays resumable.
    pub session_ttl_ms: u64,
    /// Dispatches kept per session for replay.
    pub resume_buffer_size: usize,
    pub max_connections_per_user: usize,
}

/// Everything about talking to Discord (ADR-0009, ADR-0010).
#[derive(Debug, Clone)]
pub struct DiscordConfig {
    /// Bot token. Never logged, never sent anywhere but Discord.
    pub bot_token: String,
    /// How long the gateway may be down before admissions fail closed (P-02).
    pub replica_grace: Duration,
    /// Life of a pairing code (P-03).
    pub pairing_code_ttl: Duration,
    /// Codes one Discord account may request per hour, before the bot refuses.
    pub pairing_max_per_hour: i64,
    /// Guilds this instance serves, or `None` for every guild the bot is in.
    ///
    /// A hosted instance pays for the bandwidth of everyone it serves, so it
    /// names who it serves (ADR-0035). Self-hosting leaves this empty, which is
    /// why absent means "all" and not "none".
    pub allowed_guilds: Option<HashSet<u64>>,
    pub oauth: Option<DiscordOAuthConfig>,
}

#[derive(Debug, Clone)]
pub struct DiscordOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
}

impl DiscordConfig {
    /// Whether this instance serves `guild_id`.
    pub fn serves(&self, guild_id: u64) -> bool {
        self.allowed_guilds
            .as_ref()
            .is_none_or(|allowed| allowed.contains(&guild_id))
    }
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

    pub gateway: GatewayConfig,
    pub discord: DiscordConfig,
    pub rooms: crate::livekit::RoomConfig,
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

        let max_publishers: usize = parse(source, "ROOM_MAX_PUBLISHERS")?;
        if max_publishers == 0 {
            return Err(ConfigError::Invalid {
                name: "ROOM_MAX_PUBLISHERS",
                reason: "zero publicadores torna o produto inútil; use ao menos 1".into(),
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
            gateway: GatewayConfig {
                heartbeat_interval_ms: parse(source, "WS_HEARTBEAT_INTERVAL_MS")?,
                session_ttl_ms: parse(source, "WS_SESSION_TTL_MS")?,
                resume_buffer_size: parse(source, "WS_RESUME_BUFFER_SIZE")?,
                max_connections_per_user: parse(source, "WS_MAX_CONNECTIONS_PER_USER")?,
            },
            discord: DiscordConfig {
                bot_token: required(source, "DISCORD_BOT_TOKEN")?,
                replica_grace: Duration::from_secs(parse(source, "DISCORD_REPLICA_GRACE_SECONDS")?),
                pairing_code_ttl: Duration::from_secs(parse(source, "PAIRING_CODE_TTL_SECONDS")?),
                pairing_max_per_hour: parse(source, "PAIRING_MAX_CODES_PER_HOUR")?,
                allowed_guilds: allowed_guilds(source)?,
                oauth: discord_oauth(source)?,
            },
            rooms: crate::livekit::RoomConfig {
                url: required(source, "LIVEKIT_URL")?,
                api_key: required(source, "LIVEKIT_API_KEY")?,
                api_secret: required(source, "LIVEKIT_API_SECRET")?,
                token_ttl_seconds: parse(source, "ROOM_TOKEN_TTL_SECONDS")?,
                max_publishers,
            },
        })
    }
}

/// `DISCORD_ALLOWED_GUILDS`: IDs separated by commas, or absent for every guild.
fn allowed_guilds(source: &dyn Source) -> Result<Option<HashSet<u64>>, ConfigError> {
    const NAME: &str = "DISCORD_ALLOWED_GUILDS";
    let raw = source.get(NAME).unwrap_or_default();
    if raw.trim().is_empty() {
        return Ok(None);
    }

    let allowed = raw
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| {
            id.parse::<u64>().map_err(|_| ConfigError::Invalid {
                name: NAME,
                reason: format!("'{id}' não é um ID de servidor do Discord"),
            })
        })
        .collect::<Result<HashSet<u64>, _>>()?;

    // Escrito e vazio quer dizer que alguém tentou listar algo e errou a
    // sintaxe. Servir ninguém e servir todos são os dois extremos: parar é a
    // única resposta que não escolhe um deles por conta própria.
    if allowed.is_empty() {
        return Err(ConfigError::Invalid {
            name: NAME,
            reason: "não tem nenhum ID; deixe a variável vazia para servir todos".into(),
        });
    }
    Ok(Some(allowed))
}

fn discord_oauth(source: &dyn Source) -> Result<Option<DiscordOAuthConfig>, ConfigError> {
    let non_empty = |name| source.get(name).filter(|value| !value.trim().is_empty());
    let client_id = non_empty("DISCORD_OAUTH_CLIENT_ID");
    let client_secret = non_empty("DISCORD_OAUTH_CLIENT_SECRET");
    let redirect_url = non_empty("DISCORD_OAUTH_REDIRECT_URL");
    match (client_id, client_secret, redirect_url) {
        (None, None, None) => Ok(None),
        (Some(client_id), Some(client_secret), Some(redirect_url)) => {
            Ok(Some(DiscordOAuthConfig {
                client_id,
                client_secret,
                redirect_url,
            }))
        }
        _ => Err(ConfigError::Invalid {
            name: "DISCORD_OAUTH_CLIENT_ID",
            reason: "configure as três variáveis DISCORD_OAUTH_* ou nenhuma".into(),
        }),
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
            ("WS_HEARTBEAT_INTERVAL_MS", "30000"),
            ("WS_SESSION_TTL_MS", "90000"),
            ("WS_RESUME_BUFFER_SIZE", "500"),
            ("WS_MAX_CONNECTIONS_PER_USER", "4"),
            ("DISCORD_BOT_TOKEN", "dev-only-not-a-real-token"),
            ("DISCORD_OAUTH_CLIENT_ID", "123456789012345678"),
            ("DISCORD_OAUTH_CLIENT_SECRET", "dev-only-oauth-secret"),
            (
                "DISCORD_OAUTH_REDIRECT_URL",
                "http://localhost:8080/api/v1/auth/discord/callback",
            ),
            ("DISCORD_REPLICA_GRACE_SECONDS", "60"),
            ("PAIRING_CODE_TTL_SECONDS", "300"),
            ("PAIRING_MAX_CODES_PER_HOUR", "10"),
            ("LIVEKIT_URL", "ws://localhost:7880"),
            ("LIVEKIT_API_KEY", "devkey"),
            (
                "LIVEKIT_API_SECRET",
                "dev-only-not-a-real-key-0123456789abcdef",
            ),
            ("ROOM_TOKEN_TTL_SECONDS", "3600"),
            ("ROOM_MAX_PUBLISHERS", "2"),
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
        assert_eq!(config.gateway.resume_buffer_size, 500);
        assert_eq!(config.gateway.session_ttl_ms, 90_000);
        assert_eq!(config.rooms.max_publishers, 2);
        assert_eq!(config.discord.replica_grace, Duration::from_secs(60));
        assert_eq!(config.discord.pairing_code_ttl, Duration::from_secs(300));
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
    fn the_bot_token_is_required() {
        // Sem ele nao ha identidade nem autorizacao: o produto sobe e nao serve
        // ninguem. Melhor falhar no boot do que servir 503 para todo mundo.
        let mut source = valid();
        source.0.remove("DISCORD_BOT_TOKEN");
        assert_eq!(
            Config::from_source(&source).unwrap_err(),
            ConfigError::Missing("DISCORD_BOT_TOKEN")
        );
    }

    #[test]
    fn oauth_can_be_disabled_without_disabling_server_mode() {
        let mut source = valid();
        source.0.remove("DISCORD_OAUTH_CLIENT_ID");
        source.0.remove("DISCORD_OAUTH_CLIENT_SECRET");
        source.0.remove("DISCORD_OAUTH_REDIRECT_URL");
        let config = Config::from_source(&source).expect("modo servidor continua disponível");
        assert!(config.discord.oauth.is_none());
    }

    #[test]
    fn a_partial_oauth_configuration_is_refused() {
        let mut source = valid();
        source.0.remove("DISCORD_OAUTH_CLIENT_SECRET");
        let error = Config::from_source(&source).unwrap_err();
        assert!(matches!(
            error,
            ConfigError::Invalid {
                name: "DISCORD_OAUTH_CLIENT_ID",
                ..
            }
        ));
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
    fn zero_publishers_is_refused() {
        let mut source = valid();
        source.0.insert("ROOM_MAX_PUBLISHERS", "0".into());
        assert!(Config::from_source(&source).is_err());
    }

    #[test]
    fn an_unknown_app_env_is_refused_rather_than_defaulted() {
        let mut source = valid();
        source.0.insert("APP_ENV", "staging".into());
        assert!(Config::from_source(&source).is_err());
    }

    #[test]
    fn without_an_allow_list_every_guild_is_served() {
        let config = Config::from_source(&valid()).unwrap();
        assert_eq!(config.discord.allowed_guilds, None);
        assert!(config.discord.serves(230_754_607_679_275_010));
    }

    #[test]
    fn only_the_listed_guilds_are_served() {
        let mut source = valid();
        source.0.insert(
            "DISCORD_ALLOWED_GUILDS",
            " 230754607679275010, 1436472446168600698 ".into(),
        );
        let discord = Config::from_source(&source).unwrap().discord;
        assert!(discord.serves(230_754_607_679_275_010));
        assert!(discord.serves(1_436_472_446_168_600_698));
        assert!(!discord.serves(999_999_999_999_999_999));
    }

    #[test]
    fn a_malformed_guild_id_stops_the_process_instead_of_serving_nobody() {
        let mut source = valid();
        source.0.insert(
            "DISCORD_ALLOWED_GUILDS",
            "230754607679275010, nao-e-id".into(),
        );
        assert!(matches!(
            Config::from_source(&source).unwrap_err(),
            ConfigError::Invalid {
                name: "DISCORD_ALLOWED_GUILDS",
                ..
            }
        ));
    }
}
