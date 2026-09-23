//! `/rooms/*` and the LiveKit webhook (RF-10 to RF-17).
//!
//! A room is a Discord voice channel (ADR-0011), addressed by its snowflake.
//! The client never picks one: it is told which room it is in and asks for a
//! token to enter it.

use crate::announce::RoomBroadcast;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use db::repo::{presence, sessions, users};
use protocol::gateway::DispatchEvent;
use protocol::room::{
    PublicationSource, RoomParticipantAdd, RoomParticipantRemove, RoomState, RoomTokenRequest,
    RoomTokenResponse, ShareStart, ShareStop,
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
    let publishes = !body.publish.is_empty();
    let access = if publishes {
        permissions::publish_access(&state, caller.id, discord_channel_id).await?
    } else {
        permissions::room_access(&state, caller.id, discord_channel_id).await?
    };

    // Uma chamada só, mesmo para o espectador: a lista vazia devolve tudo o que
    // a pessoa tinha, que é exatamente o que "parei de transmitir" significa.
    state
        .rooms
        .claim_publications(discord_channel_id, caller.id, &body.publish)
        .await?;

    let display = access
        .user
        .display_name
        .clone()
        .unwrap_or_else(|| access.user.username.clone());
    let token = state
        .rooms
        .issue_token(discord_channel_id, caller.id, &display, publishes)?;

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
        // A conexao que publica nao e uma pessoa entrando na sala (ADR-0027):
        // a pessoa ja esta la, pelo WebView. Escrever presenca aqui faria o par
        // entrar/sair da publicacao mexer em quem esta no canal.
        "participant_joined" if !event.is_publisher_connection() => {
            if let Some(user) = event.user() {
                on_join(&state, channel, user).await?;
            }
        }
        // Sair sem despublicar e como o core caindo aparece daqui. Encerra a
        // transmissao, mas nao tira a pessoa da sala. Todas as fontes de uma
        // vez: a conexao que caiu levava as duas (ADR-0038).
        "participant_left" if event.is_publisher_connection() => {
            if let Some(user) = event.user() {
                on_publisher_gone(&state, channel, user).await?;
            }
        }
        "participant_left" => {
            if let Some(user) = event.user() {
                on_leave(&state, channel, user).await?;
            }
        }
        "track_published" => {
            if let (Some(user), Some(source)) = (event.user(), event.published_source()) {
                on_share_start(&state, channel, user, source).await?;
            }
        }
        "track_unpublished" => {
            if let (Some(user), Some(source)) = (event.user(), event.published_source()) {
                on_share_stop(&state, channel, user, source).await?;
            }
        }
        "room_finished" => {
            presence::clear_channel(&state.pool, channel).await?;
            state.rooms.forget_room(channel).await;
            // A sala sumiu: o anuncio precisa saber, ou a mensagem fica no ar
            // dizendo que alguem transmite.
            state.announce.publish(RoomBroadcast {
                discord_channel_id: channel,
                publishers: Vec::new(),
                viewers: 0,
            });
        }
        _ => {}
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn on_join(state: &AppState, channel: i64, user_id: Uuid) -> Result<(), AppError> {
    presence::join(&state.pool, user_id, channel).await?;
    let user = users::find_by_id(&state.pool, user_id).await?;

    observe_room(state, channel).await?;

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
                    publications: Vec::new(),
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
    observe_room(state, channel).await?;

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

async fn on_share_start(
    state: &AppState,
    channel: i64,
    user_id: Uuid,
    source: PublicationSource,
) -> Result<(), AppError> {
    let Some(started) = presence::start_publication(&state.pool, user_id, source).await? else {
        // Webhook chegou depois de o usuario ja ter saido. Nao e erro.
        return Ok(());
    };
    // `open` devolve a sessao ja existente quando a segunda track chega, entao
    // `started_at` e o inicio real da transmissao e nao o da track de audio nem
    // o da segunda fonte. A sessao cobre a pessoa, nao a publicacao (ADR-0038).
    sessions::open(&state.pool, Uuid::now_v7(), channel, user_id).await?;
    observe_room(state, channel).await?;

    state
        .hub
        .publish_to_room(
            &state.pool,
            channel,
            DispatchEvent::ShareStart(ShareStart {
                discord_channel_id: Snowflake::new(channel),
                user_id,
                source,
                // De `room_presence`, e nao da sessao: cada fonte tem o seu
                // relogio, e a sessao comeca na primeira delas.
                started_at: Timestamp::new(started.since),
            }),
        )
        .await;
    Ok(())
}

/// The publishing connection went away, taking every source with it.
async fn on_publisher_gone(state: &AppState, channel: i64, user_id: Uuid) -> Result<(), AppError> {
    let Some(cleared) = presence::clear_publications(&state.pool, user_id).await? else {
        return Ok(());
    };
    sessions::close(&state.pool, channel, user_id, OffsetDateTime::now_utc()).await?;
    state.rooms.release_publisher(channel, user_id).await;
    observe_room(state, channel).await?;

    // Um evento por fonte que estava no ar: o espectador remove um ladrilho por
    // publicacao, e um aviso so deixaria o outro na tela para sempre.
    for source in cleared.sources() {
        state
            .hub
            .publish_to_room(
                &state.pool,
                channel,
                DispatchEvent::ShareStop(ShareStop {
                    discord_channel_id: Snowflake::new(channel),
                    user_id,
                    source,
                }),
            )
            .await;
    }
    Ok(())
}

async fn on_share_stop(
    state: &AppState,
    channel: i64,
    user_id: Uuid,
    source: PublicationSource,
) -> Result<(), AppError> {
    let Some(stopped) = presence::stop_publication(&state.pool, user_id, source).await? else {
        return Ok(());
    };
    // A sessao so fecha quando nada mais esta no ar: ela mede o periodo em que a
    // pessoa transmitiu, e fecha-la ao parar a camera encerraria a tela que
    // continua no ar (RNF-05).
    if !stopped.still_publishing {
        sessions::close(&state.pool, channel, user_id, OffsetDateTime::now_utc()).await?;
    }
    state
        .rooms
        .release_publication(channel, user_id, source)
        .await;
    observe_room(state, channel).await?;

    state
        .hub
        .publish_to_room(
            &state.pool,
            channel,
            DispatchEvent::ShareStop(ShareStop {
                discord_channel_id: Snowflake::new(channel),
                user_id,
                source,
            }),
        )
        .await;
    Ok(())
}

/// Feeds `peak_viewers` and tells the Discord side what the room looks like now.
///
/// One listing serves both: they need the same rows, and reading twice would let
/// the audience number and the announcement disagree about the same instant.
///
/// A viewer is someone in the room who is not publishing. The count is a count:
/// who watched is deliberately not recorded (RNF-08).
async fn observe_room(state: &AppState, channel: i64) -> Result<(), AppError> {
    let participants = presence::list_by_channel(&state.pool, channel).await?;
    let viewers = participants.iter().filter(|p| !p.publishing).count();
    sessions::observe_viewers(&state.pool, channel, viewers as i32).await?;

    state.announce.publish(RoomBroadcast {
        discord_channel_id: channel,
        publishers: participants
            .iter()
            .filter(|p| p.publishing)
            .map(|p| p.discord_user_id)
            .collect(),
        viewers,
    });
    Ok(())
}
