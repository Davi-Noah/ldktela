//! `/channels/{id}` and its permission overwrites
//! (`docs/api/rest-api.md` §6.3, §6.4).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{patch, put};
use axum::Router;
use db::repo::{categories, channels, guilds, roles};
use db::types::OverwriteTarget;
use domain::validation::{self, limits, Validation};
use domain::Permissions;
use protocol::channel::{Channel, UpdateChannelRequest};
use protocol::gateway::DispatchEvent;
use protocol::guild::{ChannelOverwrite, PutOverwriteRequest};
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Path};
use crate::middleware::auth::AuthUser;
use crate::permissions::require_channel;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/channels/{id}", patch(update).delete(delete))
        .route(
            "/channels/{id}/permissions/{target_type}/{target_id}",
            put(put_overwrite).delete(delete_overwrite),
        )
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn update(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateChannelRequest>,
) -> Result<Json<Channel>, AppError> {
    let access = require_channel(&state, caller.id, id, Permissions::MANAGE_CHANNELS).await?;

    let mut v = Validation::new();
    if let Some(name) = &body.name {
        v.check(
            "name",
            validation::bounded(name, 1, limits::CHANNEL_NAME_MAX),
        );
    }
    if let Some(Some(topic)) = &body.topic {
        v.check(
            "topic",
            validation::optional_max(Some(topic), limits::CHANNEL_TOPIC_MAX),
        );
    }
    v.finish()?;

    // A category has to belong to the same guild, or the sidebar tree breaks.
    if let Some(Some(category_id)) = body.category_id {
        let guild_id = access
            .channel
            .guild_id
            .ok_or(AppError::invisible("category"))?;
        let known = categories::list_by_guild(&state.pool, guild_id).await?;
        if !known.iter().any(|c| c.id == category_id) {
            return Err(AppError::invisible("category"));
        }
    }

    let updated = channels::update(
        &state.pool,
        id,
        body.name,
        body.topic,
        body.category_id,
        body.position,
    )
    .await?;
    state
        .hub
        .publish_to_channel(
            &state.pool,
            id,
            DispatchEvent::ChannelUpdate(Box::new(updated.to_wire(0, None))),
        )
        .await;
    Ok(Json(updated.to_wire(access.permissions.bits(), None)))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn delete(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let access = require_channel(&state, caller.id, id, Permissions::MANAGE_CHANNELS).await?;
    if access.channel.kind.is_direct() {
        // A direct conversation is left, not deleted (RF-18b).
        return Err(AppError::Conflict {
            reason: "direct_channel_not_deletable",
        });
    }
    // The recipient set has to be captured first: after the row is deleted
    // there is nothing left to resolve it from.
    let recipients = state.hub.viewers(&state.pool, id).await;
    let wire = access.channel.to_wire(0, None);
    if !channels::delete(&state.pool, id).await? {
        return Err(AppError::invisible("channel"));
    }
    // Trigger 4 of the five in websocket.md 4.2.
    state
        .hub
        .invalidate_channel(&state.pool, id, access.channel.guild_id)
        .await;
    state
        .hub
        .publish_to_users(&recipients, DispatchEvent::ChannelDelete(Box::new(wire)))
        .await;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_target(raw: &str) -> Result<OverwriteTarget, AppError> {
    match raw {
        "role" => Ok(OverwriteTarget::Role),
        "member" => Ok(OverwriteTarget::Member),
        _ => Err(AppError::invisible("overwrite")),
    }
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn put_overwrite(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, target_type, target_id)): Path<(Uuid, String, Uuid)>,
    Json(body): Json<PutOverwriteRequest>,
) -> Result<Json<ChannelOverwrite>, AppError> {
    let access = require_channel(&state, caller.id, id, Permissions::MANAGE_ROLES).await?;
    let target = parse_target(&target_type)?;
    let guild_id = access
        .channel
        .guild_id
        .ok_or(AppError::BridgeNotAllowed)
        .map_err(|_| AppError::Conflict {
            reason: "direct_channel_has_no_overwrites",
        })?;

    // The target must exist in this guild; otherwise the overwrite is a row
    // nobody can explain six months later.
    match target {
        OverwriteTarget::Role => {
            roles::find_by_id(&state.pool, guild_id, target_id)
                .await
                .map_err(|_| AppError::invisible("role"))?;
        }
        OverwriteTarget::Member => {
            if guilds::find_member(&state.pool, guild_id, target_id)
                .await?
                .is_none()
            {
                return Err(AppError::invisible("member"));
            }
        }
    }

    let allow = Permissions::from_bits_truncate(body.allow.bits());
    let deny = Permissions::from_bits_truncate(body.deny.bits());
    if allow.intersects(deny) {
        return Err(AppError::Validation(vec![protocol::error::FieldError {
            field: "allow".into(),
            code: "NOT_ALLOWED".into(),
        }]));
    }
    // Same escalation guard as role creation: an overwrite is a permission grant.
    if !access.permissions.contains(Permissions::ADMINISTRATOR)
        && !access.permissions.contains(allow)
    {
        return Err(AppError::Forbidden);
    }

    let row = roles::put_overwrite(
        &state.pool,
        id,
        target,
        target_id,
        allow.bits(),
        deny.bits(),
    )
    .await?;
    // Trigger 3: a channel overwrite changed. This also emits
    // PERMISSIONS_STALE, without which a demoted user keeps seeing controls
    // the server will refuse.
    state
        .hub
        .invalidate_channel(&state.pool, id, Some(guild_id))
        .await;
    Ok(Json(row.to_wire()))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn delete_overwrite(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, target_type, target_id)): Path<(Uuid, String, Uuid)>,
) -> Result<StatusCode, AppError> {
    let access = require_channel(&state, caller.id, id, Permissions::MANAGE_ROLES).await?;
    let target = parse_target(&target_type)?;
    if !roles::delete_overwrite(&state.pool, id, target, target_id).await? {
        return Err(AppError::invisible("overwrite"));
    }
    state
        .hub
        .invalidate_channel(&state.pool, id, access.channel.guild_id)
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_overwrite_target_is_a_404_not_a_400() {
        // `/permissions/foo/{id}` names a resource that does not exist; telling
        // the caller their enum was wrong would be a shape they can probe.
        assert!(matches!(
            parse_target("foo"),
            Err(AppError::NotFound { .. })
        ));
        assert_eq!(parse_target("role").unwrap(), OverwriteTarget::Role);
        assert_eq!(parse_target("member").unwrap(), OverwriteTarget::Member);
    }
}
