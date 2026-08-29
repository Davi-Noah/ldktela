//! Session issuance and refresh rotation with family reuse detection (RF-01a).
//!
//! A refresh token belongs to a family. Rotation consumes the presented token
//! and issues a new one **in the same family**. Presenting a token that was
//! already consumed can only mean two holders exist, so the entire family is
//! revoked and the user has to log in again.
//!
//! The `consumed_at IS NULL` guard in the `UPDATE` is what makes this safe under
//! concurrency: exactly one caller wins the race, and the loser is treated as
//! the thief — which is the correct bias, because the legitimate client can
//! always log in again while a stolen token becomes worthless.

use db::repo::refresh_tokens;
use db::PgPool;
use protocol::auth::AuthResponse;
use protocol::user::PresenceStatus;
use sqlx::PgExecutor;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::auth::token;
use crate::config::Config;
use crate::error::AppError;

/// Issues a brand new access/refresh pair, starting a new family.
///
/// Takes an executor so registration can run inside the transaction that also
/// consumes the invite.
pub async fn issue_session<'e, E: PgExecutor<'e>>(
    executor: E,
    config: &Config,
    user: &db::repo::users::UserRow,
    user_agent: Option<&str>,
) -> Result<AuthResponse, AppError> {
    let now = OffsetDateTime::now_utc();
    let id = Uuid::now_v7();
    // A nova família é identificada pelo primeiro token dela.
    issue_pair(executor, config, user, user_agent, id, id, now).await
}

/// Rotates a presented refresh token.
///
/// Returns `TokenReused` — after revoking the family — for a token that was
/// already consumed, and `Unauthorized` for one that is unknown, revoked or
/// expired.
pub async fn rotate_refresh(
    pool: &PgPool,
    config: &Config,
    presented_secret: &str,
    user_agent: Option<&str>,
) -> Result<AuthResponse, AppError> {
    let now = OffsetDateTime::now_utc();
    let hash = token::hash_refresh(presented_secret);

    let Some(row) = refresh_tokens::find_by_hash(pool, &hash).await? else {
        return Err(AppError::Unauthorized);
    };

    // Reúso explícito: o token já foi trocado antes. Derruba a família.
    if row.consumed_at.is_some() {
        let revoked = refresh_tokens::revoke_family(pool, row.family_id).await?;
        tracing::warn!(
            user_id = %row.user_id,
            family_id = %row.family_id,
            revoked,
            "refresh token reuse detected; family revoked"
        );
        return Err(AppError::TokenReused);
    }

    if row.revoked_at.is_some() || row.expires_at <= now {
        return Err(AppError::Unauthorized);
    }

    // Corrida: dois refreshes simultâneos com o mesmo token. Só um consome; o
    // perdedor é indistinguível de um ladrão, então a família cai.
    if !refresh_tokens::consume(pool, row.id).await? {
        let revoked = refresh_tokens::revoke_family(pool, row.family_id).await?;
        tracing::warn!(
            user_id = %row.user_id,
            family_id = %row.family_id,
            revoked,
            "lost the rotation race; family revoked"
        );
        return Err(AppError::TokenReused);
    }

    let user = db::repo::users::find_by_id(pool, row.user_id).await?;
    issue_pair(
        pool,
        config,
        &user,
        user_agent,
        Uuid::now_v7(),
        row.family_id,
        now,
    )
    .await
}

/// Revokes the family the presented token belongs to. Unknown tokens are a
/// no-op: logging out twice is not an error.
pub async fn revoke_session(pool: &PgPool, presented_secret: &str) -> Result<(), AppError> {
    let hash = token::hash_refresh(presented_secret);
    if let Some(row) = refresh_tokens::find_by_hash(pool, &hash).await? {
        refresh_tokens::revoke_family(pool, row.family_id).await?;
    }
    Ok(())
}

async fn issue_pair<'e, E: PgExecutor<'e>>(
    executor: E,
    config: &Config,
    user: &db::repo::users::UserRow,
    user_agent: Option<&str>,
    token_id: Uuid,
    family_id: Uuid,
    now: OffsetDateTime,
) -> Result<AuthResponse, AppError> {
    let (access_token, _jti) = token::issue_access(
        config.jwt_signing_key.as_bytes(),
        user.id,
        config.access_token_ttl_seconds,
        now,
    )
    .map_err(|e| AppError::Internal(anyhow::anyhow!("issuing access token: {e}")))?;

    let refresh = token::issue_refresh();
    refresh_tokens::insert(
        executor,
        token_id,
        family_id,
        user.id,
        &refresh.hash,
        user_agent,
        now + Duration::days(config.refresh_token_ttl_days),
    )
    .await?;

    Ok(AuthResponse {
        access_token,
        refresh_token: refresh.secret,
        expires_in: config.access_token_ttl_seconds,
        // Presence lives in the gateway; a fresh session starts offline until it
        // identifies over the WebSocket.
        user: user.to_current(PresenceStatus::Offline),
    })
}
