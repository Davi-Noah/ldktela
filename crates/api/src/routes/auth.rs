//! `/auth/*` (`docs/rest-api.md` §6.1, RF-01, RF-02).
//!
//! There is no register and no login. Identity arrives as a pairing code the
//! bot handed out inside Discord (ADR-0009); from the token pair onwards
//! everything is the v1 machinery, unchanged.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use db::repo::{pairing, users};
use domain::pairing::hash_code;
use domain::validation::{self, Validation};
use protocol::auth::{AuthResponse, PairRequest, RefreshRequest};
use time::OffsetDateTime;

use crate::auth::session;
use crate::error::AppError;
use crate::extract::Json;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/pair", post(pair))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
}

fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(200).collect())
}

/// Exchange a pairing code for a session.
///
/// Every failure — malformed, unknown, expired, already used — answers
/// `401` with the same body. The client cannot tell them apart, and neither can
/// the server past the repository call, which is what makes that guarantee real
/// rather than a promise (`db::repo::pairing::consume`).
///
/// There is no attempt limiter here. The code is eight characters over a
/// 31-symbol alphabet, single use, and lives five minutes: guessing one inside
/// its window needs a request rate no residential connection produces. The
/// limit that does exist is on *issuance*, in the bot, where it stops someone
/// from farming codes for an account they control.
#[tracing::instrument(skip(state, body))]
async fn pair(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PairRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let code = body.code.trim().to_ascii_uppercase();

    let mut v = Validation::new();
    v.check("code", validation::pairing_code(&code));
    // Formato errado sai como 401 junto com todo o resto: dizer "formato
    // invalido" contaria ao atacante que o alfabeto e o tamanho importam.
    if v.finish().is_err() {
        return Err(AppError::Unauthorized);
    }

    let Some(consumed) =
        pairing::consume(&state.pool, &hash_code(&code), OffsetDateTime::now_utc()).await?
    else {
        return Err(AppError::Unauthorized);
    };

    // O bot cria a linha de `users` no momento em que emite o codigo, entao ela
    // existe aqui. Nao existir e inconsistencia nossa, nao entrada do usuario.
    let Some(user) = users::find_by_discord_id(&state.pool, consumed.discord_user_id).await? else {
        return Err(AppError::Internal(anyhow::anyhow!(
            "codigo consumido sem usuario correspondente: discord_user_id={}",
            consumed.discord_user_id
        )));
    };

    let response = session::issue_session(
        &state.pool,
        &state.config,
        &user,
        user_agent(&headers).as_deref(),
    )
    .await?;
    tracing::info!(user_id = %user.id, guild = consumed.discord_guild_id, "paired");
    Ok(Json(response))
}

/// Rotation with reuse detection (RF-02). See `auth::session::rotate_refresh`.
#[tracing::instrument(skip(state, body))]
async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RefreshRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let response = session::rotate_refresh(
        &state.pool,
        &state.config,
        &body.refresh_token,
        user_agent(&headers).as_deref(),
    )
    .await?;
    Ok(Json(response))
}

#[tracing::instrument(skip(state, body))]
async fn logout(
    State(state): State<AppState>,
    Json(body): Json<RefreshRequest>,
) -> Result<StatusCode, AppError> {
    session::revoke_session(&state.pool, &body.refresh_token).await?;
    Ok(StatusCode::NO_CONTENT)
}
