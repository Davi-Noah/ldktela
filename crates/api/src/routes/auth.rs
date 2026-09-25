//! `/auth/*` (`docs/rest-api.md` §6.1, RF-01, RF-02).
//!
//! There is no register and no login. Identity arrives as a pairing code the
//! bot handed out inside Discord (ADR-0009); from the token pair onwards
//! everything is the v1 machinery, unchanged.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use base64::Engine as _;
use db::repo::{oauth_login, pairing, users};
use domain::pairing::hash_code;
use domain::validation::{self, Validation};
use protocol::auth::{
    AuthResponse, OAuthCompleteRequest, OAuthStartResponse, PairRequest, RefreshRequest,
};
use rand::RngCore as _;
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::auth::session;
use crate::error::AppError;
use crate::extract::Json;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/pair", post(pair))
        .route("/auth/discord/start", post(oauth_start))
        .route("/auth/discord/callback", get(oauth_callback))
        .route("/auth/discord/complete", post(oauth_complete))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
}

const OAUTH_ATTEMPT_TTL_SECONDS: i64 = 300;

fn random_secret() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[tracing::instrument(skip(state))]
async fn oauth_start(State(state): State<AppState>) -> Result<Json<OAuthStartResponse>, AppError> {
    let oauth = state
        .config
        .discord
        .oauth
        .as_ref()
        .ok_or(AppError::NotFound {
            resource: "discord_oauth",
        })?;
    let attempt_id = Uuid::now_v7();
    let oauth_state = random_secret();
    let poll_secret = random_secret();
    let now = OffsetDateTime::now_utc();
    oauth_login::insert(
        &state.pool,
        attempt_id,
        &hash_code(&oauth_state),
        &hash_code(&poll_secret),
        now + time::Duration::seconds(OAUTH_ATTEMPT_TTL_SECONDS),
    )
    .await?;

    let mut url = reqwest::Url::parse("https://discord.com/oauth2/authorize")
        .map_err(|error| AppError::Internal(anyhow::Error::new(error)))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &oauth.client_id)
        .append_pair("scope", "identify")
        .append_pair("state", &oauth_state)
        .append_pair("redirect_uri", &oauth.redirect_url)
        .append_pair("prompt", "consent");

    Ok(Json(OAuthStartResponse {
        authorize_url: url.into(),
        attempt_id,
        poll_secret,
        expires_in: OAUTH_ATTEMPT_TTL_SECONDS,
    }))
}

#[derive(Deserialize)]
struct OAuthCallbackQuery {
    code: String,
    state: String,
}

#[derive(Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct DiscordOAuthUser {
    id: String,
    username: String,
    global_name: Option<String>,
    avatar: Option<String>,
}

#[tracing::instrument(skip(state, query))]
async fn oauth_callback(
    State(state): State<AppState>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Html<&'static str>, AppError> {
    let oauth = state
        .config
        .discord
        .oauth
        .as_ref()
        .ok_or(AppError::NotFound {
            resource: "discord_oauth",
        })?;
    if !oauth_login::is_pending(
        &state.pool,
        &hash_code(&query.state),
        OffsetDateTime::now_utc(),
    )
    .await?
    {
        return Err(AppError::Unauthorized);
    }
    let client = reqwest::Client::new();
    let token = client
        .post("https://discord.com/api/oauth2/token")
        .basic_auth(&oauth.client_id, Some(&oauth.client_secret))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", query.code.as_str()),
            ("redirect_uri", oauth.redirect_url.as_str()),
        ])
        .send()
        .await
        .map_err(|error| AppError::Upstream(crate::UpstreamError::Discord(error.to_string())))?
        .error_for_status()
        .map_err(|error| AppError::Upstream(crate::UpstreamError::Discord(error.to_string())))?
        .json::<OAuthTokenResponse>()
        .await
        .map_err(|error| AppError::Upstream(crate::UpstreamError::Discord(error.to_string())))?;

    let profile = client
        .get("https://discord.com/api/v10/users/@me")
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|error| AppError::Upstream(crate::UpstreamError::Discord(error.to_string())))?
        .error_for_status()
        .map_err(|error| AppError::Upstream(crate::UpstreamError::Discord(error.to_string())))?
        .json::<DiscordOAuthUser>()
        .await
        .map_err(|error| AppError::Upstream(crate::UpstreamError::Discord(error.to_string())))?;

    let discord_user_id = profile.id.parse::<i64>().map_err(|error| {
        AppError::Upstream(crate::UpstreamError::Discord(format!(
            "invalid user id: {error}"
        )))
    })?;
    let avatar_url = profile.avatar.as_ref().map(|hash| {
        format!(
            "https://cdn.discordapp.com/avatars/{}/{hash}.png",
            profile.id
        )
    });
    let user = users::upsert_from_discord(
        &state.pool,
        Uuid::now_v7(),
        discord_user_id,
        &profile.username,
        profile.global_name.as_deref(),
        avatar_url.as_deref(),
    )
    .await?;
    let completed = oauth_login::complete(
        &state.pool,
        &hash_code(&query.state),
        user.id,
        OffsetDateTime::now_utc(),
    )
    .await?;
    if !completed {
        return Err(AppError::Unauthorized);
    }
    Ok(Html(
        "<!doctype html><meta charset=utf-8><title>ldktela</title><p>Conta conectada. Você já pode fechar esta janela e voltar ao ldktela.</p>",
    ))
}

#[tracing::instrument(skip(state, body, headers))]
async fn oauth_complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<OAuthCompleteRequest>,
) -> Result<Response, AppError> {
    let Some(user_id) = oauth_login::consume(
        &state.pool,
        body.attempt_id,
        &hash_code(&body.poll_secret),
        OffsetDateTime::now_utc(),
    )
    .await?
    else {
        return Ok(StatusCode::ACCEPTED.into_response());
    };
    let user = users::find_by_id(&state.pool, user_id).await?;
    let response = session::issue_session(
        &state.pool,
        &state.config,
        &user,
        user_agent(&headers).as_deref(),
    )
    .await?;
    Ok(Json(response).into_response())
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
#[tracing::instrument(skip(state, body, headers))]
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
#[tracing::instrument(skip(state, body, headers))]
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
