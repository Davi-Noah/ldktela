//! Background maintenance.

use std::time::Duration;

use crate::state::AppState;

/// Objects older than this with no attachment row are collected (SRS §6.2).
///
/// The grace period matters: an object uploaded seconds ago has not been
/// referenced yet because the client is still composing the message.
const ORPHAN_GRACE_SECONDS: i64 = 24 * 60 * 60;

const DAILY: Duration = Duration::from_secs(24 * 60 * 60);

/// Deletes stored objects that no attachment row points at.
///
/// Every deletion of a message takes its attachment rows with it
/// (`ON DELETE CASCADE`), and an upload that never became a message leaves an
/// object behind; without this the 10 GB free tier fills with debris (RNF-09).
pub async fn collect_orphans(state: &AppState) -> Result<u64, crate::AppError> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let objects = state.storage.list_attachment_keys().await?;

    let mut removed = 0;
    for object in objects {
        if now - object.last_modified_secs < ORPHAN_GRACE_SECONDS {
            continue;
        }
        if db::repo::engagement::is_key_referenced(&state.pool, &object.key).await? {
            continue;
        }
        state.storage.delete(&object.key).await?;
        removed += 1;
        tracing::info!(key = %object.key, "collected orphan object");
    }
    Ok(removed)
}

/// Daily orphan sweep. Runs on a plain interval rather than a cron expression:
/// the process is meant to stay up, and a missed day is harmless.
pub async fn run_orphan_collector(state: AppState) {
    let mut ticker = tokio::time::interval(DAILY);
    // The first tick fires immediately; skip it so a deploy does not sweep.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        match collect_orphans(&state).await {
            Ok(0) => tracing::debug!("orphan sweep found nothing"),
            Ok(removed) => tracing::info!(removed, "orphan sweep finished"),
            Err(err) => tracing::error!(error = %err, "orphan sweep failed"),
        }
    }
}

/// Drops refresh tokens that expired long enough ago to be useless.
pub async fn run_token_cleanup(state: AppState) {
    let mut ticker = tokio::time::interval(DAILY);
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
