//! `/auth/*` (`docs/api/rest-api.md` §6.1).

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use db::repo::{invites, users};
use domain::validation::{self, Validation};
use protocol::auth::{AuthResponse, LoginRequest, RefreshRequest, RegisterRequest};
use protocol::user::PresenceStatus;
use uuid::Uuid;

use crate::auth::{password, session};
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
}

fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(200).collect())
}

/// Registration consumes the invite and creates the account in one transaction:
/// a failure anywhere gives the invite back (RF-02).
#[tracing::instrument(skip(state, body), fields(username = %body.username))]
async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), AppError> {
    let mut v = Validation::new();
    v.check("invite_code", validation::bounded(&body.invite_code, 1, 16));
    v.check("email", validation::email(&body.email));
    v.check("username", validation::username(&body.username));
    v.check("password", validation::password(&body.password));
    v.finish()?;

    let hash = password::hash(&state.config.argon2, &body.password)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("hashing password: {e}")))?;

    let mut tx = state.pool.begin().await.map_err(db::DbError::from)?;

    // O convite é consumido antes da criação da conta, com o guard de uso na
    // própria cláusula WHERE. Se a criação falhar, o rollback devolve o uso.
    invites::consume(&mut *tx, body.invite_code.trim())
        .await
        .map_err(|_| AppError::Conflict {
            reason: "invite_consumed",
        })?;

    let user = users::insert_real(
        &mut *tx,
        Uuid::now_v7(),
        body.email.trim(),
        body.username.trim(),
        &hash,
    )
    .await
    .map_err(|e| {
        if e.is_unique_violation("idx_users_username") {
            AppError::Conflict {
                reason: "username_taken",
            }
        } else if e.is_unique_violation("idx_users_email") {
            AppError::Conflict {
                reason: "email_taken",
            }
        } else {
            AppError::from(e)
        }
    })?;

    let response = session::issue_session(
        &mut *tx,
        &state.config,
        &user,
        user_agent(&headers).as_deref(),
    )
    .await?;

    tx.commit().await.map_err(db::DbError::from)?;
    tracing::info!(user_id = %user.id, "account created");
    Ok((StatusCode::CREATED, Json(response)))
}

#[tracing::instrument(skip(state, body))]
async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let Some(user) = users::find_by_email(&state.pool, body.email.trim()).await? else {
        // Gasta o mesmo Argon2 de um login real: sem isto, o tempo de resposta
        // diz quais e-mails existem.
        password::verify_dummy(&state.config.argon2, &body.password);
        return Err(AppError::Unauthorized);
    };

    let Some(stored) = user.password_hash.as_deref() else {
        // Ghost user: sem senha, nunca autentica.
        password::verify_dummy(&state.config.argon2, &body.password);
        return Err(AppError::Unauthorized);
    };

    if !password::verify(&state.config.argon2, &body.password, stored) {
        return Err(AppError::Unauthorized);
    }

    let response = session::issue_session(
        &state.pool,
        &state.config,
        &user,
        user_agent(&headers).as_deref(),
    )
    .await?;
    tracing::info!(user_id = %user.id, "login");
    Ok(Json(response))
}

/// Rotation with reuse detection (RF-01a). See `auth::session::rotate_refresh`.
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

/// Presence is not persisted; a freshly authenticated user is offline until the
/// gateway sees them. Kept next to the routes that build `AuthResponse` so the
/// two never drift.
pub const INITIAL_PRESENCE: PresenceStatus = PresenceStatus::Offline;
