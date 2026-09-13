//! Background maintenance.
//!
//! Two sweeps, both of expired credentials. There is no object storage to
//! collect any more: the product stores no files (ADR-0006).

use std::time::Duration;

use crate::state::AppState;

const DAILY: Duration = Duration::from_secs(24 * 60 * 60);
/// Pairing codes are short-lived and numerous relative to their usefulness, so
/// they are swept far more often than daily.
const HOURLY: Duration = Duration::from_secs(60 * 60);

/// Drops refresh tokens that expired long enough ago to be useless.
pub async fn run_token_cleanup(state: AppState) {
    let mut ticker = tokio::time::interval(DAILY);
    // The first tick fires immediately; skip it so a deploy does not sweep.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        match db::repo::refresh_tokens::delete_expired(&state.pool, 7).await {
            Ok(0) => {}
            Ok(removed) => tracing::info!(removed, "expired refresh tokens deleted"),
            Err(err) => tracing::error!(error = %err, "refresh token cleanup failed"),
        }
    }
}

/// Drops pairing codes that are spent or past their window.
///
/// They are inert once consumed or expired, but they are also the input to the
/// issuance rate limit, so leaving them forever would make that count grow
/// without bound.
pub async fn run_pairing_cleanup(state: AppState) {
    let mut ticker = tokio::time::interval(HOURLY);
    ticker.tick().await;
    loop {
        ticker.tick().await;
        let now = time::OffsetDateTime::now_utc();
        match db::repo::pairing::delete_expired(&state.pool, now).await {
            Ok(0) => {}
            Ok(removed) => tracing::debug!(removed, "spent pairing codes deleted"),
            Err(err) => tracing::error!(error = %err, "pairing cleanup failed"),
        }
    }
}
