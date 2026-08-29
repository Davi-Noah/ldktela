//! `/dms` (`docs/api/rest-api.md` §6.7, RF-18, RF-18a, RF-18b).
//!
//! Direct conversations reuse `channels` with a null `guild_id`, so every
//! message, attachment, reaction and read-state route works on them unchanged.
//! What differs is access: there are no roles and no overwrites, and permission
//! resolution short-circuits at step 0 of SRS §5.3 on `channel_participants`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::Router;
use db::repo::{channels, users};
use db::types::ChannelType;
use domain::validation::limits;
use domain::Permissions;
use protocol::channel::{AddDmParticipantRequest, Channel, CreateDmRequest, DmParticipantEvent};
use protocol::gateway::DispatchEvent;
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Path};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/dms", get(list).post(create))
        .route("/dms/{id}/participants", post(add_participant))
        .route("/dms/{id}/participants/{uid}", delete(remove_participant))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn list(
    State(state): State<AppState>,
    caller: AuthUser,
) -> Result<Json<Vec<Channel>>, AppError> {
    let rows = channels::list_direct_for_user(&state.pool, caller.id).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let participants = channels::participants(&state.pool, row.id).await?;
        out.push(row.to_wire(Permissions::DIRECT_MESSAGE.bits(), Some(participants)));
    }
    Ok(Json(out))
}

/// `POST /dms`. With a single recipient this **resolves** the existing 1:1
/// channel instead of creating another (RF-18).
#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn create(
    State(state): State<AppState>,
    caller: AuthUser,
    Json(body): Json<CreateDmRequest>,
) -> Result<(StatusCode, Json<Channel>), AppError> {
    let mut recipients: Vec<Uuid> = body.recipient_ids.clone();
    recipients.retain(|id| *id != caller.id);
    recipients.sort_unstable();
    recipients.dedup();

    if recipients.is_empty() {
        return Err(AppError::Validation(vec![protocol::error::FieldError {
            field: "recipient_ids".into(),
            code: "REQUIRED".into(),
        }]));
    }
    // The caller counts towards the limit (P-01: ten participants).
    if recipients.len() + 1 > limits::GROUP_DM_PARTICIPANTS_MAX {
        return Err(AppError::Conflict {
            reason: "dm_participant_limit",
        });
    }
    // Every recipient has to exist. A ghost user has no session and no way to
    // read the conversation, so it cannot be a recipient either.
    for recipient in &recipients {
        let user = users::find_by_id(&state.pool, *recipient)
            .await
            .map_err(|_| AppError::invisible("user"))?;
        if user.is_migrated {
            return Err(AppError::invisible("user"));
        }
    }

    let is_pair = recipients.len() == 1;
    if is_pair {
        // Resolving before creating is the whole of RF-18. Doing it inside the
        // transaction below would still race two simultaneous opens, which is
        // why the insert re-checks under the same transaction.
        if let Some(existing) =
            channels::find_direct_between(&state.pool, caller.id, recipients[0]).await?
        {
            let row = channels::find_by_id(&state.pool, existing).await?;
            let participants = channels::participants(&state.pool, existing).await?;
            return Ok((
                StatusCode::OK,
                Json(row.to_wire(Permissions::DIRECT_MESSAGE.bits(), Some(participants))),
            ));
        }
    }

    let mut tx = state.pool.begin().await.map_err(db::DbError::from)?;
    if is_pair {
        // Serialise on the canonical pair before looking again. Without the
        // lock both transactions read before either commits and one pair ends
        // up with two channels.
        channels::lock_direct_pair(&mut tx, caller.id, recipients[0]).await?;
        if let Some(existing) =
            channels::find_direct_between(&mut *tx, caller.id, recipients[0]).await?
        {
            tx.rollback().await.map_err(db::DbError::from)?;
            let row = channels::find_by_id(&state.pool, existing).await?;
            let participants = channels::participants(&state.pool, existing).await?;
            return Ok((
                StatusCode::OK,
                Json(row.to_wire(Permissions::DIRECT_MESSAGE.bits(), Some(participants))),
            ));
        }
    }

    let kind = if is_pair {
        ChannelType::Dm
    } else {
        ChannelType::GroupDm
    };
    // The name is a fallback for a group with no title; the client renders
    // participant names for a 1:1.
    let channel =
        channels::insert_direct_channel(&mut *tx, Uuid::now_v7(), "conversa", kind).await?;
    channels::add_participant(&mut *tx, channel.id, caller.id, Some(caller.id)).await?;
    for recipient in &recipients {
        channels::add_participant(&mut *tx, channel.id, *recipient, Some(caller.id)).await?;
    }
    tx.commit().await.map_err(db::DbError::from)?;

    let participants = channels::participants(&state.pool, channel.id).await?;
    let wire = channel.to_wire(
        Permissions::DIRECT_MESSAGE.bits(),
        Some(participants.clone()),
    );

    // A new conversation is routed by participation, so the index has no stale
    // entry to drop; the event goes straight to the people in it.
    let audience: Vec<Uuid> = participants.iter().map(|p| p.id).collect();
    state
        .hub
        .publish_to_users(
            &audience,
            DispatchEvent::DmChannelCreate(Box::new(wire.clone())),
        )
        .await;

    Ok((StatusCode::CREATED, Json(wire)))
}

/// Only the creator adds people (RF-18b). The creator is whoever `added_by`
/// names on the first participant row.
#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn add_participant(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<AddDmParticipantRequest>,
) -> Result<StatusCode, AppError> {
    let channel = direct_channel_visible_to(&state, caller.id, id).await?;
    if channel.kind != ChannelType::GroupDm {
        return Err(AppError::Conflict {
            reason: "not_a_group",
        });
    }
    if channels::direct_creator(&state.pool, id).await? != Some(caller.id) {
        return Err(AppError::Forbidden);
    }

    let existing = channels::participants(&state.pool, id).await?;
    if existing.iter().any(|p| p.id == body.user_id) {
        return Ok(StatusCode::NO_CONTENT);
    }
    if existing.len() + 1 > limits::GROUP_DM_PARTICIPANTS_MAX {
        return Err(AppError::Conflict {
            reason: "dm_participant_limit",
        });
    }
    let user = users::find_by_id(&state.pool, body.user_id)
        .await
        .map_err(|_| AppError::invisible("user"))?;
    if user.is_migrated {
        return Err(AppError::invisible("user"));
    }

    channels::add_participant(&state.pool, id, body.user_id, Some(caller.id)).await?;
    announce_participants(&state, id, DispatchKind::Added, body.user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The creator removes anyone; anyone removes themselves (RF-18b).
#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn remove_participant(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, uid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    direct_channel_visible_to(&state, caller.id, id).await?;
    let is_self = uid == caller.id;
    if !is_self && channels::direct_creator(&state.pool, id).await? != Some(caller.id) {
        return Err(AppError::Forbidden);
    }

    if !channels::remove_participant(&state.pool, id, uid).await? {
        return Err(AppError::invisible("participant"));
    }
    // The leaver has to be told too, and they are no longer in the recipient
    // set, so the event is addressed explicitly.
    announce_participants(&state, id, DispatchKind::Removed, uid).await?;
    Ok(StatusCode::NO_CONTENT)
}

enum DispatchKind {
    Added,
    Removed,
}

async fn announce_participants(
    state: &AppState,
    channel_id: Uuid,
    kind: DispatchKind,
    user_id: Uuid,
) -> Result<(), AppError> {
    // The membership of a direct conversation is its permission set, so the
    // routing entry for this channel is now wrong.
    state
        .hub
        .invalidate_channel(&state.pool, channel_id, None)
        .await;

    let mut audience = channels::participant_ids(&state.pool, channel_id).await?;
    if !audience.contains(&user_id) {
        audience.push(user_id);
    }
    let payload = DmParticipantEvent {
        channel_id,
        user_id,
    };
    let event = match kind {
        DispatchKind::Added => DispatchEvent::DmParticipantAdd(payload),
        DispatchKind::Removed => DispatchEvent::DmParticipantRemove(payload),
    };
    state.hub.publish_to_users(&audience, event).await;
    Ok(())
}

/// A direct channel the caller actively participates in.
///
/// Anything else — a guild channel, a conversation they left, one that does not
/// exist — is a `404`. Distinguishing them would confirm who talks to whom.
async fn direct_channel_visible_to(
    state: &AppState,
    user_id: Uuid,
    channel_id: Uuid,
) -> Result<channels::ChannelRow, AppError> {
    let channel = channels::find_by_id(&state.pool, channel_id)
        .await
        .map_err(|_| AppError::invisible("channel"))?;
    if !channel.kind.is_direct() {
        return Err(AppError::invisible("channel"));
    }
    let participants = channels::participant_ids(&state.pool, channel_id).await?;
    if !participants.contains(&user_id) {
        return Err(AppError::invisible("channel"));
    }
    Ok(channel)
}
