//! `/channels/{id}/voice-token`, `/voice-states/@me` and the LiveKit webhook
//! (`docs/api/rest-api.md` §6.9, RF-19 to RF-23).

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{patch, post};
use axum::Router;
use db::repo::{channels, users, voice_states};
use db::types::ChannelType;
use domain::Permissions;
use protocol::gateway::DispatchEvent;
use protocol::voice::{UpdateVoiceStateRequest, VoiceState, VoiceTokenRequest, VoiceTokenResponse};
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Path};
use crate::middleware::auth::AuthUser;
use crate::permissions::require_channel;
use crate::state::AppState;
use crate::voice;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/channels/{id}/voice-token", post(issue_token))
        .route("/voice-states/@me", patch(update_own_state))
        // Authenticated by the LiveKit signature, not by a bearer token (§6.9).
        .route("/internal/livekit/webhook", post(livekit_webhook))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn issue_token(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<VoiceTokenRequest>,
) -> Result<Json<VoiceTokenResponse>, AppError> {
    let access = require_channel(&state, caller.id, id, Permissions::CONNECT_VOICE).await?;
    if access.channel.kind == ChannelType::Text {
        return Err(AppError::Conflict {
            reason: "not_a_voice_channel",
        });
    }
    if body.publish_camera && !access.permissions.contains(Permissions::VIDEO) {
        return Err(AppError::Forbidden);
    }

    // RNF-10 guard, applied before the token is signed: once a fourth camera is
    // live the egress is already spent.
    if body.publish_camera {
        state.voice.claim_camera(id, caller.id).await?;
    } else {
        // Asking for a listener token gives the slot back, so someone who turns
        // their camera off does not hold the room's fourth seat.
        state.voice.release_camera(id, caller.id).await;
    }

    let user = users::find_by_id(&state.pool, caller.id).await?;
    let display = user
        .display_name
        .clone()
        .unwrap_or_else(|| user.username.clone());
    let token = state
        .voice
        .issue_token(id, caller.id, &display, body.publish_camera)?;

    Ok(Json(VoiceTokenResponse {
        token,
        url: state.voice.url().to_string(),
        room: voice::room_name(id),
        expires_in: state.voice.token_ttl_seconds(),
    }))
}

/// The client is the source of these two flags and nothing else (§5): joining,
/// leaving and streaming all come from the webhook.
#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn update_own_state(
    State(state): State<AppState>,
    caller: AuthUser,
    Json(body): Json<UpdateVoiceStateRequest>,
) -> Result<Json<VoiceState>, AppError> {
    let Some(row) =
        voice_states::set_flags(&state.pool, caller.id, body.self_mute, body.self_deaf).await?
    else {
        // Not in a voice channel: there is no state to change.
        return Err(AppError::invisible("voice state"));
    };
    broadcast(&state, row).await?;
    Ok(Json(row))
}

/// The **only** source of `VOICE_STATE_UPDATE` (§6.9).
///
/// The body is verified against LiveKit's signature before anything is read
/// from it. An unsigned body is not "ignored quietly" out of politeness: without
/// the check anyone could place anyone in any voice channel, and the state is
/// visible to the whole guild (RF-20).
#[tracing::instrument(skip(state, headers, body))]
async fn livekit_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> StatusCode {
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    let Some(event) = state.voice.verify_webhook(&body, authorization) else {
        // 401 rather than 400: the body may be perfectly well formed and simply
        // not ours. LiveKit retries on failure, which is what we want.
        return StatusCode::UNAUTHORIZED;
    };

    if let Err(err) = apply_webhook(&state, event).await {
        tracing::error!(error = %err, "applying livekit webhook");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::NO_CONTENT
}

async fn apply_webhook(
    state: &AppState,
    event: crate::voice::WebhookEvent,
) -> Result<(), AppError> {
    let Some(channel_id) = event.room.as_deref().and_then(voice::channel_of_room) else {
        // A room that is not one of ours; nothing to record.
        return Ok(());
    };
    let participant = event
        .participant_identity
        .as_deref()
        .and_then(|id| id.parse::<Uuid>().ok());

    match event.event.as_str() {
        "participant_joined" => {
            let Some(user_id) = participant else {
                return Ok(());
            };
            // The channel has to still exist and still be a voice channel; a
            // stale room name must not resurrect a deleted channel's state.
            let Ok(channel) = channels::find_by_id(&state.pool, channel_id).await else {
                return Ok(());
            };
            if channel.kind == ChannelType::Text {
                return Ok(());
            }
            let row = voice_states::join(&state.pool, user_id, channel_id, &event.event).await?;
            broadcast(state, row).await?;
        }
        "participant_left" => {
            let Some(user_id) = participant else {
                return Ok(());
            };
            state.voice.release_camera(channel_id, user_id).await;
            if voice_states::leave(&state.pool, user_id).await? {
                broadcast(
                    state,
                    VoiceState {
                        user_id,
                        // `null` means left (§5).
                        channel_id: None,
                        self_mute: false,
                        self_deaf: false,
                        streaming: false,
                    },
                )
                .await?;
            }
        }
        "track_published" | "track_unpublished" => {
            let Some(user_id) = participant else {
                return Ok(());
            };
            let source = event.track_source.as_deref().unwrap_or_default();
            if source.contains("microphone") {
                // Feeds the RNF-10 idle-room timeout.
                state.voice.mark_audio(channel_id).await;
            }
            if source.contains("screen_share") {
                let streaming = event.event == "track_published";
                if let Some(row) =
                    voice_states::set_streaming(&state.pool, user_id, streaming).await?
                {
                    broadcast(state, row).await?;
                }
            }
        }
        "room_finished" => {
            state.voice.forget_room(channel_id).await;
            for user_id in voice_states::clear_channel(&state.pool, channel_id).await? {
                broadcast(
                    state,
                    VoiceState {
                        user_id,
                        channel_id: None,
                        self_mute: false,
                        self_deaf: false,
                        streaming: false,
                    },
                )
                .await?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Voice state is visible to the whole guild, including people who are not in
/// the room — that is the point of RF-20.
async fn broadcast(state: &AppState, row: VoiceState) -> Result<(), AppError> {
    // On leave the row is already gone, so the audience comes from the user's
    // guilds instead of from the channel.
    if let Some(channel_id) = row.channel_id {
        let channel = channels::find_by_id(&state.pool, channel_id).await?;
        if let Some(guild_id) = channel.guild_id {
            state
                .hub
                .publish_to_guild(&state.pool, guild_id, DispatchEvent::VoiceStateUpdate(row))
                .await;
            return Ok(());
        }
        // Voice in a direct conversation reaches its participants (P-03).
        let audience = channels::participant_ids(&state.pool, channel_id).await?;
        state
            .hub
            .publish_to_users(&audience, DispatchEvent::VoiceStateUpdate(row))
            .await;
        return Ok(());
    }

    for guild in db::repo::guilds::list_for_user(&state.pool, row.user_id).await? {
        state
            .hub
            .publish_to_guild(&state.pool, guild.id, DispatchEvent::VoiceStateUpdate(row))
            .await;
    }
    Ok(())
}
