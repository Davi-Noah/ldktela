//! Single binary composing the REST API, the WebSocket gateway and the bridge.

use std::process::ExitCode;

use anyhow::Context;
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

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?
        .block_on(serve(config))
}

async fn serve(config: api::config::Config) -> anyhow::Result<()> {
    let pool = db::connect(&config.database_url, config.database_max_connections)
        .await
        .context("connecting to postgres")?;
    db::MIGRATOR
        .run(&pool)
        .await
        .context("applying migrations")?;

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
