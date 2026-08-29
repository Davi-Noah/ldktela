//! Single binary composing the REST API, the WebSocket gateway and the bridge.
//!
//! `server` serves. `server bootstrap --guild <nome> --owner <username>` creates
//! the first guild, because the REST contract has no guild-creation endpoint and
//! a private platform needs exactly one guild to exist before anyone can be
//! invited into it.

use std::process::ExitCode;

use anyhow::{bail, Context};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("fatal: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    init_tracing();
    let config = api::config::Config::from_env().context("loading configuration")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => runtime.block_on(serve(config)),
        Some("bootstrap") => runtime.block_on(bootstrap(config, &args[1..])),
        Some(other) => bail!("subcomando desconhecido: {other}. Use `bootstrap` ou nenhum."),
    }
}

async fn serve(config: api::config::Config) -> anyhow::Result<()> {
    let pool = open_database(&config).await?;
    let bind_addr = config.bind_addr;
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("binding {bind_addr}"))?;
    tracing::info!(addr = %bind_addr, env = ?config.app_env, "listening");

    let state = api::AppState::new(pool, config);
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serving http")
}

/// Creates the first guild with its `@everyone` role, a `geral` text channel and
/// the owner as a member. Idempotent by guild name: running it twice does not
/// create a second guild.
async fn bootstrap(config: api::config::Config, args: &[String]) -> anyhow::Result<()> {
    let guild_name = flag(args, "--guild").context("faltou --guild <nome>")?;
    let owner_username = flag(args, "--owner").context("faltou --owner <username>")?;

    let pool = open_database(&config).await?;
    let owner = db::repo::users::find_real_by_username(&pool, &owner_username)
        .await?
        .with_context(|| format!("usuário {owner_username} não existe"))?;

    let existing = db::repo::guilds::list_for_user(&pool, owner.id).await?;
    if let Some(guild) = existing.iter().find(|g| g.name == guild_name) {
        println!("guild já existe: {} ({})", guild.name, guild.id);
        return Ok(());
    }

    let mut tx = pool.begin().await?;
    let guild = db::repo::guilds::insert(&mut *tx, uuid::Uuid::now_v7(), &guild_name, owner.id)
        .await
        .context("creating guild")?;
    db::repo::roles::insert_default(
        &mut *tx,
        uuid::Uuid::now_v7(),
        guild.id,
        api::routes::guilds::DEFAULT_EVERYONE.bits(),
    )
    .await
    .context("creating @everyone")?;
    db::repo::guilds::add_member(&mut *tx, guild.id, owner.id).await?;
    db::repo::channels::insert_guild_channel(
        &mut *tx,
        db::repo::channels::NewGuildChannel {
            id: uuid::Uuid::now_v7(),
            guild_id: guild.id,
            category_id: None,
            name: "geral",
            topic: None,
            kind: db::types::ChannelType::Text,
            position: 0,
        },
    )
    .await
    .context("creating the default channel")?;
    tx.commit().await?;

    println!("guild criado: {} ({})", guild.name, guild.id);
    println!("dono: {} ({})", owner.username, owner.id);
    Ok(())
}

async fn open_database(config: &api::config::Config) -> anyhow::Result<db::PgPool> {
    let pool = db::connect(&config.database_url, config.database_max_connections)
        .await
        .context("connecting to postgres")?;
    db::MIGRATOR
        .run(&pool)
        .await
        .context("applying migrations")?;
    Ok(pool)
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %err, "failed to listen for shutdown signal");
    }
    tracing::info!("shutdown signal received");
}

/// JSON structured logs (RNF-14). Panicking here is correct: without logging the
/// process is not operable.
fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().json().flatten_event(true))
        .init();
}
