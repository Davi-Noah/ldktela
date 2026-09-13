//! Single binary composing the REST API, the WebSocket gateway and the Discord
//! bot.
//!
//! There is no bootstrap subcommand any more. v1 needed one because a guild had
//! to exist before anyone could be invited; now the guilds are Discord's, and
//! the only setup step is inviting the bot to one (ADR-0010).

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
        Some(other) => bail!("subcomando desconhecido: {other}. O binário não recebe argumentos."),
    }
}

async fn serve(config: api::config::Config) -> anyhow::Result<()> {
    let pool = open_database(&config).await?;

    // A crash leaves share sessions open with no SFU room behind them. Closing
    // them here keeps the egress report honest; leaving them would make every
    // future total include a session that never ended.
    match db::repo::sessions::close_all_open(&pool, time::OffsetDateTime::now_utc()).await {
        Ok(0) => {}
        Ok(closed) => tracing::warn!(closed, "closed share sessions left open by a crash"),
        Err(error) => tracing::error!(%error, "closing stale share sessions"),
    }

    let bind_addr = config.bind_addr;
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("binding {bind_addr}"))?;
    tracing::info!(addr = %bind_addr, env = ?config.app_env, "listening");

    let state = api::AppState::new(pool, config);

    tokio::spawn(api::gateway::run_session_sweeper(state.clone()));
    tokio::spawn(api::jobs::run_token_cleanup(state.clone()));
    tokio::spawn(api::jobs::run_pairing_cleanup(state.clone()));

    // The bot is not optional: without it there is no identity and no
    // authorization. If it dies the API stays up and fails closed on new
    // admissions (RF-09), which is the honest degraded state — refusing to
    // start would take running sessions down with it.
    let bot_state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = bot::run(bot_state).await {
            tracing::error!(%error, "discord bot stopped");
        }
    });

    let shutdown_state = state.clone();
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            // The system runs as a single instance (RNF-14), so a deploy drops
            // every socket. RECONNECT first: the session TTL gives the process
            // time to come back and every client resumes.
            shutdown_state.hub.broadcast_reconnect().await;
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        })
        .await
        .context("serving http")
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

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %err, "failed to listen for shutdown signal");
    }
    tracing::info!("shutdown signal received");
}

/// JSON structured logs (RNF-13). Panicking here is correct: without logging the
/// process is not operable.
fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().json().flatten_event(true))
        .init();
}
