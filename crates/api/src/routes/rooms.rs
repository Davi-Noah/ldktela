//! `/rooms/*` and the LiveKit webhook (RF-10 to RF-17).
//!
//! A room is a Discord voice channel (ADR-0011), addressed by its snowflake.
//! The client never picks one: it is told which room it is in and asks for a
//! token to enter it.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use db::repo::{presence, sessions, users};
use protocol::gateway::DispatchEvent;
use protocol::room::{
    RoomParticipantAdd, RoomParticipantRemove, RoomState, RoomTokenRequest, RoomTokenResponse,
    ShareStart, ShareStop,
};
use protocol::scalars::{Snowflake, Timestamp};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Path};
use crate::middleware::auth::AuthUser;
use crate::permissions;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/rooms/{discord_channel_id}", get(room))
        .route("/rooms/{discord_channel_id}/token", post(token))
        .route("/internal/livekit/webhook", post(webhook))
}

/// Builds the full state of a room. Callers have already checked access.
pub async fn state_of(
    state: &AppState,
    discord_channel_id: i64,
    discord_guild_id: i64,
    channel_name: String,
) -> Result<RoomState, AppError> {
    let participants = presence::list_by_channel(&state.pool, discord_channel_id).await?;
    Ok(RoomState {
        discord_channel_id: Snowflake::new(discord_channel_id),
        discord_guild_id: Snowflake::new(discord_guild_id),
        channel_name,
        participants: participants.iter().map(|p| p.to_wire()).collect(),
    })
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn room(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(discord_channel_id): Path<i64>,
) -> Result<Json<RoomState>, AppError> {
    let access = permissions::room_access(&state, caller.id, discord_channel_id).await?;
    Ok(Json(
        state_of(
            &state,
            discord_channel_id,
            access.discord_guild_id,
            access.channel_name,
        )
        .await?,
    ))
}

/// Issues a LiveKit token for the room.
///
/// A publish token additionally needs Discord's own "Video" right and a free
/// slot in the publisher guard. Asking for a viewer token gives back any slot
/// the caller held: someone who stopped sharing must not keep occupying a seat.
#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn token(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(discord_channel_id): Path<i64>,
    Json(body): Json<RoomTokenRequest>,
) -> Result<Json<RoomTokenResponse>, AppError> {
    let access = if body.publish {
        permissions::publish_access(&state, caller.id, discord_channel_id).await?
    } else {
        permissions::room_access(&state, caller.id, discord_channel_id).await?
    };

    if body.publish {
        state
            .rooms
            .claim_publisher(discord_channel_id, caller.id)
            .await?;
    } else {
        state
            .rooms
            .release_publisher(discord_channel_id, caller.id)
            .await;
    }

    let display = access
        .user
        .display_name
        .clone()
        .unwrap_or_else(|| access.user.username.clone());
    let token = state
        .rooms
        .issue_token(discord_channel_id, caller.id, &display, body.publish)?;

    Ok(Json(RoomTokenResponse {
        token,
        url: state.rooms.url().to_owned(),
        room: crate::livekit::room_name(discord_channel_id),
        expires_in: state.rooms.token_ttl_seconds(),
    }))
}

/// LiveKit webhook. Authenticated by signature, never by bearer token.
///
/// This is the only source of `SHARE_START`, and the only thing that writes
/// presence: `room_presence` means "connected to our SFU room", which is a
/// stronger and more useful statement than "in the Discord voice channel".
#[tracing::instrument(skip(state, headers, body))]
async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<StatusCode, AppError> {
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    let Some(event) = state.rooms.verify_webhook(&body, authorization) else {
        return Err(AppError::Unauthorized);
    };

    // Assinado e valido, mas de uma sala que nao e nossa. Recusar faria o
    // LiveKit reenviar para sempre.
    let Some(channel) = event.channel() else {
        return Ok(StatusCode::NO_CONTENT);
    };

    match event.event.as_str() {
        "participant_joined" => {
            if let Some(user) = event.user() {
                on_join(&state, channel, user).await?;
            }
        }
        "participant_left" => {
            if let Some(user) = event.user() {
                on_leave(&state, channel, user).await?;
            }
        }
        "track_published" if event.is_screen_video() => {
            if let Some(user) = event.user() {
                on_share_start(&state, channel, user).await?;
            }
        }
        "track_unpublished" if event.is_screen_video() => {
            if let Some(user) = event.user() {
                on_share_stop(&state, channel, user).await?;
            }
        }
        "room_finished" => {
            presence::clear_channel(&state.pool, channel).await?;
            state.rooms.forget_room(channel).await;
        }
        _ => {}
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn on_join(state: &AppState, channel: i64, user_id: Uuid) -> Result<(), AppError> {
    presence::join(&state.pool, user_id, channel).await?;
    let user = users::find_by_id(&state.pool, user_id).await?;

    record_peak_viewers(state, channel).await?;

    state
        .hub
        .publish_to_room(
            &state.pool,
            channel,
            DispatchEvent::RoomParticipantAdd(Box::new(RoomParticipantAdd {
                discord_channel_id: Snowflake::new(channel),
                participant: protocol::room::RoomParticipant {
                    user: user.to_summary(),
                    // Quem acaba de entrar na sala ainda nao publica; o
                    // SHARE_START vem depois, com o inicio real.
                    publishing: false,
                    publishing_since: None,
                },
            })),
        )
        .await;
    Ok(())
}

async fn on_leave(state: &AppState, channel: i64, user_id: Uuid) -> Result<(), AppError> {
    // Quem sai ainda precisa receber o evento, e depois do DELETE ele nao esta
    // mais no conjunto de destinatarios.
    let mut recipients = state.hub.room_members(&state.pool, channel).await;
    if !recipients.contains(&user_id) {
        recipients.push(user_id);
    }

    presence::leave(&state.pool, user_id).await?;
    sessions::close(&state.pool, channel, user_id, OffsetDateTime::now_utc()).await?;
    state.rooms.release_publisher(channel, user_id).await;

    state
        .hub
        .publish_to_users(
            &recipients,
            DispatchEvent::RoomParticipantRemove(RoomParticipantRemove {
                discord_channel_id: Snowflake::new(channel),
                user_id,
            }),
        )
        .await;
    Ok(())
}

async fn on_share_start(state: &AppState, channel: i64, user_id: Uuid) -> Result<(), AppError> {
    if presence::set_publishing(&state.pool, user_id, true)
        .await?
        .is_none()
    {
        // Webhook chegou depois de o usuario ja ter saido. Nao e erro.
        return Ok(());
    }
    // `open` devolve a sessao ja existente quando a segunda track chega, entao
    // `started_at` e o inicio real da transmissao e nao o da track de audio.
    let session = sessions::open(&state.pool, Uuid::now_v7(), channel, user_id).await?;
    record_peak_viewers(state, channel).await?;

    state
        .hub
        .publish_to_room(
            &state.pool,
            channel,
            DispatchEvent::ShareStart(ShareStart {
                discord_channel_id: Snowflake::new(channel),
                user_id,
                started_at: Timestamp::new(session.started_at),
            }),
        )
        .await;
    Ok(())
}

async fn on_share_stop(state: &AppState, channel: i64, user_id: Uuid) -> Result<(), AppError> {
    presence::set_publishing(&state.pool, user_id, false).await?;
    sessions::close(&state.pool, channel, user_id, OffsetDateTime::now_utc()).await?;
    state.rooms.release_publisher(channel, user_id).await;

    state
        .hub
        .publish_to_room(
            &state.pool,
            channel,
            DispatchEvent::ShareStop(ShareStop {
                discord_channel_id: Snowflake::new(channel),
                user_id,
            }),
        )
        .await;
    Ok(())
}

/// Feeds `peak_viewers`, which is the only audience number the product keeps.
///
/// A viewer is someone in the room who is not publishing. The count is a count:
/// who watched is deliberately not recorded (RNF-08).
async fn record_peak_viewers(state: &AppState, channel: i64) -> Result<(), AppError> {
    let participants = presence::list_by_channel(&state.pool, channel).await?;
    let viewers = participants.iter().filter(|p| !p.publishing).count();
    sessions::observe_viewers(&state.pool, channel, viewers as i32).await?;
    Ok(())
}
