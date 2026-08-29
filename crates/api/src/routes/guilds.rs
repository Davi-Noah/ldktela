//! `/guilds/*` (`docs/api/rest-api.md` §6.3, §6.4).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, patch, post, put};
use axum::Router;
use db::repo::channels::NewGuildChannel;
use db::repo::{categories, channels, guilds, roles};
use db::types::ChannelType;
use domain::validation::{self, limits, Validation};
use domain::Permissions;
use protocol::channel::{Channel, CreateChannelRequest, ReorderChannelsRequest};
use protocol::gateway::ReadyGuild;
use protocol::guild::{
    Category, CreateCategoryRequest, CreateRoleRequest, Guild, Member, Role, UpdateCategoryRequest,
    UpdateGuildRequest, UpdateMemberRequest, UpdateRoleRequest,
};
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Path};
use crate::middleware::auth::AuthUser;
use crate::permissions::{guild_visible, require_guild, visible_channels};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/guilds", get(list))
        .route("/guilds/{id}", get(show).patch(update))
        .route("/guilds/{id}/members", get(list_members))
        .route(
            "/guilds/{id}/members/{user_id}",
            patch(update_member).delete(kick_member),
        )
        .route("/guilds/{id}/bans/{user_id}", put(ban_member))
        .route("/guilds/{id}/categories", post(create_category))
        .route(
            "/guilds/{id}/categories/{cid}",
            patch(update_category).delete(delete_category),
        )
        .route("/guilds/{id}/channels", post(create_channel))
        .route("/guilds/{id}/channels/positions", patch(reorder_channels))
        .route("/guilds/{id}/roles", get(list_roles).post(create_role))
        .route(
            "/guilds/{id}/roles/{rid}",
            patch(update_role).delete(delete_role),
        )
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn list(
    State(state): State<AppState>,
    caller: AuthUser,
) -> Result<Json<Vec<Guild>>, AppError> {
    let rows = guilds::list_for_user(&state.pool, caller.id).await?;
    Ok(Json(rows.iter().map(|g| g.to_wire()).collect()))
}

/// The structure of one guild: categories, the channels the caller can see,
/// roles and members. Messages are not here; they come per channel, on demand.
#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn show(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ReadyGuild>, AppError> {
    let access = guild_visible(&state, caller.id, id).await?;
    let visible = visible_channels(&state, caller.id, id).await?;
    // The contract gates this route on `VIEW_CHANNEL` in at least one channel.
    if visible.is_empty() {
        return Err(AppError::invisible("guild"));
    }

    let channels: Vec<Channel> = visible
        .iter()
        .map(|c| c.channel.to_wire(c.permissions.bits(), None))
        .collect();
    let categories = categories::list_by_guild(&state.pool, id).await?;
    let roles = roles::list_by_guild(&state.pool, id).await?;
    let members = guilds::list_members(&state.pool, id).await?;

    Ok(Json(ReadyGuild {
        id: access.guild.id,
        name: access.guild.name.clone(),
        icon_url: access.guild.icon_url.clone(),
        owner_id: access.guild.owner_id,
        categories: categories.iter().map(|c| c.to_wire()).collect(),
        channels,
        roles: roles.iter().map(|r| r.to_wire()).collect(),
        member_count: members.len() as i64,
        members: members.iter().map(|m| m.to_wire()).collect(),
    }))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn update(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateGuildRequest>,
) -> Result<Json<Guild>, AppError> {
    require_guild(&state, caller.id, id, Permissions::MANAGE_GUILD).await?;
    let mut v = Validation::new();
    if let Some(Some(name)) = &body.name {
        v.check("name", validation::bounded(name, 1, limits::GUILD_NAME_MAX));
    }
    v.finish()?;
    let guild = guilds::update(&state.pool, id, body.name, body.icon_url).await?;
    Ok(Json(guild.to_wire()))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn list_members(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Member>>, AppError> {
    guild_visible(&state, caller.id, id).await?;
    let rows = guilds::list_members(&state.pool, id).await?;
    Ok(Json(rows.iter().map(|m| m.to_wire()).collect()))
}

/// Nickname is the member's own; roles need `MANAGE_ROLES`.
#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn update_member(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateMemberRequest>,
) -> Result<Json<Member>, AppError> {
    let access = guild_visible(&state, caller.id, id).await?;
    if guilds::find_member(&state.pool, id, user_id)
        .await?
        .is_none()
    {
        return Err(AppError::invisible("member"));
    }

    if body.roles.is_some() && !access.permissions.contains(Permissions::MANAGE_ROLES) {
        return Err(AppError::Forbidden);
    }
    if body.nickname.is_some()
        && user_id != caller.id
        && !access.permissions.contains(Permissions::MANAGE_ROLES)
    {
        return Err(AppError::Forbidden);
    }

    if let Some(nickname) = &body.nickname {
        if let Some(value) = nickname {
            let mut v = Validation::new();
            v.check(
                "nickname",
                validation::bounded(value, 1, limits::NICKNAME_MAX),
            );
            v.finish()?;
        }
        guilds::set_nickname(&state.pool, id, user_id, nickname.as_deref()).await?;
    }
    if let Some(role_ids) = &body.roles {
        // Every role has to belong to this guild; a foreign id would silently
        // grant nothing and confuse the operator later.
        let existing = roles::list_by_guild(&state.pool, id).await?;
        for role in role_ids {
            if !existing.iter().any(|r| r.id == *role && !r.is_default) {
                return Err(AppError::Validation(vec![protocol::error::FieldError {
                    field: "roles".into(),
                    code: "NOT_ALLOWED".into(),
                }]));
            }
        }
        guilds::replace_roles(&state.pool, id, user_id, role_ids).await?;
    }

    let member = guilds::find_member(&state.pool, id, user_id)
        .await?
        .ok_or(AppError::invisible("member"))?;
    Ok(Json(member.to_wire()))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn kick_member(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let access = require_guild(&state, caller.id, id, Permissions::KICK_MEMBERS).await?;
    if user_id == access.guild.owner_id {
        return Err(AppError::Conflict {
            reason: "cannot_remove_owner",
        });
    }
    if !guilds::remove_member(&state.pool, id, user_id).await? {
        return Err(AppError::invisible("member"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn ban_member(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let access = require_guild(&state, caller.id, id, Permissions::BAN_MEMBERS).await?;
    if user_id == access.guild.owner_id {
        return Err(AppError::Conflict {
            reason: "cannot_remove_owner",
        });
    }
    guilds::ban_member(&state.pool, id, user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Categorias
// ---------------------------------------------------------------------------

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn create_category(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateCategoryRequest>,
) -> Result<(StatusCode, Json<Category>), AppError> {
    require_guild(&state, caller.id, id, Permissions::MANAGE_CHANNELS).await?;
    let mut v = Validation::new();
    v.check(
        "name",
        validation::bounded(&body.name, 1, limits::CATEGORY_NAME_MAX),
    );
    v.finish()?;
    let category = categories::insert(
        &state.pool,
        Uuid::now_v7(),
        id,
        body.name.trim(),
        body.position.unwrap_or(0),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(category.to_wire())))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn update_category(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, cid)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateCategoryRequest>,
) -> Result<Json<Category>, AppError> {
    require_guild(&state, caller.id, id, Permissions::MANAGE_CHANNELS).await?;
    let mut v = Validation::new();
    if let Some(name) = &body.name {
        v.check(
            "name",
            validation::bounded(name, 1, limits::CATEGORY_NAME_MAX),
        );
    }
    v.finish()?;
    let category = categories::update(&state.pool, id, cid, body.name, body.position).await?;
    Ok(Json(category.to_wire()))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn delete_category(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, cid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require_guild(&state, caller.id, id, Permissions::MANAGE_CHANNELS).await?;
    if !categories::delete(&state.pool, id, cid).await? {
        return Err(AppError::invisible("category"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Canais
// ---------------------------------------------------------------------------

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn create_channel(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateChannelRequest>,
) -> Result<(StatusCode, Json<Channel>), AppError> {
    let access = require_guild(&state, caller.id, id, Permissions::MANAGE_CHANNELS).await?;

    let mut v = Validation::new();
    v.check(
        "name",
        validation::bounded(&body.name, 1, limits::CHANNEL_NAME_MAX),
    );
    v.check(
        "topic",
        validation::optional_max(body.topic.as_deref(), limits::CHANNEL_TOPIC_MAX),
    );
    if body.kind.is_direct() {
        // `chk_channel_scope` would refuse anyway; refusing here names the field.
        v.push("type", domain::ValidationCode::NotAllowed);
    }
    v.finish()?;

    if let Some(category_id) = body.category_id {
        let known = categories::list_by_guild(&state.pool, id).await?;
        if !known.iter().any(|c| c.id == category_id) {
            return Err(AppError::invisible("category"));
        }
    }

    let channel = channels::insert_guild_channel(
        &state.pool,
        NewGuildChannel {
            id: Uuid::now_v7(),
            guild_id: id,
            category_id: body.category_id,
            name: body.name.trim(),
            topic: body.topic.as_deref(),
            kind: ChannelType::from(body.kind),
            position: body.position.unwrap_or(0),
        },
    )
    .await?;
    // The creator's own mask on the brand new channel, resolved like any other.
    let mask = db::repo::permissions::resolve_for_channel(&state.pool, caller.id, channel.id)
        .await?
        .unwrap_or(access.permissions);
    Ok((
        StatusCode::CREATED,
        Json(channel.to_wire(mask.bits(), None)),
    ))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn reorder_channels(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<ReorderChannelsRequest>,
) -> Result<StatusCode, AppError> {
    require_guild(&state, caller.id, id, Permissions::MANAGE_CHANNELS).await?;
    let positions: Vec<(Uuid, i32, Option<Option<Uuid>>)> = body
        .positions
        .iter()
        .map(|p| (p.id, p.position, p.category_id))
        .collect();
    channels::reorder(&state.pool, id, &positions).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Cargos
// ---------------------------------------------------------------------------

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn list_roles(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Role>>, AppError> {
    require_guild(&state, caller.id, id, Permissions::MANAGE_ROLES).await?;
    let rows = roles::list_by_guild(&state.pool, id).await?;
    Ok(Json(rows.iter().map(|r| r.to_wire()).collect()))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn create_role(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateRoleRequest>,
) -> Result<(StatusCode, Json<Role>), AppError> {
    let access = require_guild(&state, caller.id, id, Permissions::MANAGE_ROLES).await?;
    let mut v = Validation::new();
    v.check(
        "name",
        validation::bounded(&body.name, 1, limits::ROLE_NAME_MAX),
    );
    if let Some(color) = &body.color {
        v.check("color", validation::hex_color(color));
    }
    v.finish()?;

    let requested =
        Permissions::from_bits_truncate(body.permissions.map(|p| p.bits()).unwrap_or(0));
    let granted = clamp_to_own(access.permissions, requested)?;

    let role = roles::insert(
        &state.pool,
        roles::NewRole {
            id: Uuid::now_v7(),
            guild_id: id,
            name: body.name.trim(),
            color: body.color.as_deref(),
            position: body.position.unwrap_or(1),
            permissions: granted.bits(),
            hoist: body.hoist.unwrap_or(false),
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(role.to_wire())))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn update_role(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, rid)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<Json<Role>, AppError> {
    let access = require_guild(&state, caller.id, id, Permissions::MANAGE_ROLES).await?;
    let mut v = Validation::new();
    if let Some(name) = &body.name {
        v.check("name", validation::bounded(name, 1, limits::ROLE_NAME_MAX));
    }
    if let Some(Some(color)) = &body.color {
        v.check("color", validation::hex_color(color));
    }
    v.finish()?;

    let permissions = match body.permissions {
        Some(mask) => Some(
            clamp_to_own(
                access.permissions,
                Permissions::from_bits_truncate(mask.bits()),
            )?
            .bits(),
        ),
        None => None,
    };

    let role = roles::update(
        &state.pool,
        id,
        rid,
        body.name,
        body.color,
        body.position,
        permissions,
        body.hoist,
    )
    .await?;
    Ok(Json(role.to_wire()))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn delete_role(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, rid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require_guild(&state, caller.id, id, Permissions::MANAGE_ROLES).await?;
    if !roles::delete(&state.pool, id, rid).await? {
        // Either it does not exist, or it is `@everyone`, which has no delete.
        return Err(AppError::invisible("role"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Nobody may grant a permission they do not hold themselves.
///
/// Without this, `MANAGE_ROLES` is `ADMINISTRATOR`: a moderator creates a role
/// with `ADMINISTRATOR` and assigns it to themselves. The spec does not state
/// the rule, and every system that omits it gets escalated through.
fn clamp_to_own(own: Permissions, requested: Permissions) -> Result<Permissions, AppError> {
    if own.contains(Permissions::ADMINISTRATOR) {
        return Ok(requested);
    }
    if !own.contains(requested) {
        return Err(AppError::Forbidden);
    }
    Ok(requested)
}

/// The mask a fresh guild's `@everyone` carries.
///
/// Everything a member needs to take part, and nothing that moderates.
pub const DEFAULT_EVERYONE: Permissions = Permissions::from_bits_truncate(
    Permissions::VIEW_CHANNEL.bits()
        | Permissions::SEND_MESSAGES.bits()
        | Permissions::ATTACH_FILES.bits()
        | Permissions::EMBED_LINKS.bits()
        | Permissions::ADD_REACTIONS.bits()
        | Permissions::CONNECT_VOICE.bits()
        | Permissions::SPEAK.bits()
        | Permissions::VIDEO.bits()
        | Permissions::SCREEN_SHARE.bits(),
);

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::channel::ChannelType as WireChannelType;

    #[test]
    fn nobody_grants_a_permission_they_do_not_hold() {
        let moderator = Permissions::MANAGE_ROLES | Permissions::KICK_MEMBERS;
        assert!(clamp_to_own(moderator, Permissions::KICK_MEMBERS).is_ok());
        assert!(
            clamp_to_own(moderator, Permissions::ADMINISTRATOR).is_err(),
            "sem isto, MANAGE_ROLES é ADMINISTRATOR por escalada"
        );
        assert!(clamp_to_own(moderator, Permissions::BAN_MEMBERS).is_err());
    }

    #[test]
    fn an_administrator_may_grant_anything() {
        assert!(clamp_to_own(Permissions::ALL, Permissions::ALL).is_ok());
        assert!(clamp_to_own(Permissions::ADMINISTRATOR, Permissions::BAN_MEMBERS).is_ok());
    }

    #[test]
    fn the_default_everyone_mask_moderates_nothing() {
        for forbidden in [
            Permissions::ADMINISTRATOR,
            Permissions::MANAGE_GUILD,
            Permissions::MANAGE_ROLES,
            Permissions::MANAGE_CHANNELS,
            Permissions::MANAGE_MESSAGES,
            Permissions::KICK_MEMBERS,
            Permissions::BAN_MEMBERS,
            Permissions::MENTION_EVERYONE,
            Permissions::MUTE_MEMBERS,
            Permissions::MOVE_MEMBERS,
        ] {
            assert!(
                !DEFAULT_EVERYONE.contains(forbidden),
                "@everyone não pode nascer com {:?}",
                forbidden.names()
            );
        }
        assert!(DEFAULT_EVERYONE.contains(Permissions::VIEW_CHANNEL));
        assert!(DEFAULT_EVERYONE.contains(Permissions::SEND_MESSAGES));
    }

    #[test]
    fn a_direct_channel_type_is_rejected_before_the_database_check_constraint() {
        assert!(WireChannelType::Dm.is_direct());
        assert!(WireChannelType::GroupDm.is_direct());
        assert!(!WireChannelType::Text.is_direct());
    }
}
