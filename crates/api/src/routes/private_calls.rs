use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use base64::Engine as _;
use db::repo::{private_calls, users};
use domain::pairing::hash_code;
use protocol::gateway::DispatchEvent;
use protocol::private_call::{
    PrivateCallCreateResponse, PrivateCallEnded, PrivateCallJoinRequest, PrivateCallJoined,
    PrivateCallState,
};
use protocol::room::{RoomTokenRequest, RoomTokenResponse};
use rand::RngCore as _;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Path};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;

const INVITE_TTL_MINUTES: i64 = 10;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/private-calls", post(create))
        .route("/private-calls/join", post(join))
        .route("/private-calls/{id}", get(show).delete(end))
        .route("/private-calls/{id}/token", post(token))
}

fn invite_code() -> String {
    let mut bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) async fn state_of(
    state: &AppState,
    row: private_calls::PrivateCallRow,
) -> Result<PrivateCallState, AppError> {
    let owner = users::find_by_id(&state.pool, row.owner_id).await?;
    let guest = match row.guest_id {
        Some(id) => Some(users::find_by_id(&state.pool, id).await?.to_summary()),
        None => None,
    };
    Ok(PrivateCallState {
        id: row.id,
        owner: owner.to_summary(),
        guest,
    })
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn create(
    State(state): State<AppState>,
    caller: AuthUser,
) -> Result<Json<PrivateCallCreateResponse>, AppError> {
    let code = invite_code();
    let row = private_calls::insert(
        &state.pool,
        Uuid::now_v7(),
        caller.id,
        &hash_code(&code),
        OffsetDateTime::now_utc() + time::Duration::minutes(INVITE_TTL_MINUTES),
    )
    .await?;
    Ok(Json(PrivateCallCreateResponse {
        call: state_of(&state, row).await?,
        code,
    }))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn join(
    State(state): State<AppState>,
    caller: AuthUser,
    Json(body): Json<PrivateCallJoinRequest>,
) -> Result<Json<PrivateCallState>, AppError> {
    let normalized = body.code.trim();
    if normalized.len() != 22 {
        return Err(AppError::NotFound { resource: "invite" });
    }
    let Some(row) = private_calls::join(
        &state.pool,
        &hash_code(normalized),
        caller.id,
        OffsetDateTime::now_utc(),
    )
    .await?
    else {
        return Err(AppError::NotFound { resource: "invite" });
    };
    let owner_id = row.owner_id;
    let call = state_of(&state, row).await?;
    state
        .hub
        .publish_to_user(
            owner_id,
            DispatchEvent::PrivateCallJoin(Box::new(PrivateCallJoined { call: call.clone() })),
        )
        .await;
    Ok(Json(call))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn show(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<PrivateCallState>, AppError> {
    let Some(row) = private_calls::find_for_user(&state.pool, id, caller.id).await? else {
        return Err(AppError::NotFound {
            resource: "private_call",
        });
    };
    if row.ended_at.is_some() {
        return Err(AppError::NotFound {
            resource: "private_call",
        });
    }
    Ok(Json(state_of(&state, row).await?))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn token(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<RoomTokenRequest>,
) -> Result<Json<RoomTokenResponse>, AppError> {
    let Some(call) = private_calls::find_for_user(&state.pool, id, caller.id).await? else {
        return Err(AppError::NotFound {
            resource: "private_call",
        });
    };
    if call.ended_at.is_some() {
        return Err(AppError::NotFound {
            resource: "private_call",
        });
    }
    let user = users::find_by_id(&state.pool, caller.id).await?;
    if body.publish {
        state.rooms.claim_private_publisher(id, caller.id).await?;
    } else {
        state.rooms.release_private_publisher(id, caller.id).await;
    }
    let display = user
        .display_name
        .clone()
        .unwrap_or_else(|| user.username.clone());
    let token = state
        .rooms
        .issue_private_token(id, caller.id, &display, body.publish)?;
    Ok(Json(RoomTokenResponse {
        token,
        url: state.rooms.url().to_owned(),
        room: crate::livekit::private_room_name(id),
        expires_in: state.rooms.token_ttl_seconds(),
    }))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn end(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let Some(existing) = private_calls::find_for_user(&state.pool, id, caller.id).await? else {
        return Err(AppError::NotFound {
            resource: "private_call",
        });
    };
    if existing.owner_id != caller.id {
        return Err(AppError::NotFound {
            resource: "private_call",
        });
    }
    if existing.ended_at.is_some() {
        return Ok(StatusCode::NO_CONTENT);
    }
    let Some(call) =
        private_calls::end(&state.pool, id, caller.id, OffsetDateTime::now_utc()).await?
    else {
        return Ok(StatusCode::NO_CONTENT);
    };
    let mut members = vec![call.owner_id];
    if let Some(guest) = call.guest_id {
        members.push(guest);
    }
    for member in &members {
        if let Err(error) = state.rooms.remove_private_participant(id, *member).await {
            tracing::debug!(%error, %member, call_id = %id, "participant already absent");
        }
    }
    state.rooms.forget_private_room(id).await;
    state
        .hub
        .publish_to_users(
            &members,
            DispatchEvent::PrivateCallEnd(PrivateCallEnded { call_id: id }),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}
