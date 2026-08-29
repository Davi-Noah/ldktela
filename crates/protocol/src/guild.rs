//! Guilds, categories, roles, members and invites
//! (`docs/api/rest-api.md` §6.2 to §6.4, RF-05, RF-07, RF-07a, RF-07b).

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::patch::double_option;
use crate::scalars::{PermissionMask, Snowflake, Timestamp};
use crate::user::UserSummary;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Guild {
    pub id: Uuid,
    pub name: String,
    pub icon_url: Option<String>,
    pub owner_id: Uuid,
    pub discord_guild_id: Option<Snowflake>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct UpdateGuildRequest {
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub icon_url: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Category {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub position: i32,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateCategoryRequest {
    pub name: String,
    pub position: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct UpdateCategoryRequest {
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub position: Option<i32>,
}

/// A role. `permissions` travels as a decimal string (§6.4).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Role {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub position: i32,
    pub permissions: PermissionMask,
    /// The implicit `@everyone` role.
    pub is_default: bool,
    pub hoist: bool,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateRoleRequest {
    pub name: String,
    #[ts(optional)]
    pub color: Option<String>,
    #[ts(optional)]
    pub position: Option<i32>,
    #[ts(optional)]
    pub permissions: Option<PermissionMask>,
    #[ts(optional)]
    pub hoist: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct UpdateRoleRequest {
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub color: Option<Option<String>>,
    #[ts(optional)]
    pub position: Option<i32>,
    #[ts(optional)]
    pub permissions: Option<PermissionMask>,
    #[ts(optional)]
    pub hoist: Option<bool>,
}

/// A guild member. `roles` holds role ids; the role objects come from the guild.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Member {
    pub user: UserSummary,
    pub guild_id: Uuid,
    pub nickname: Option<String>,
    pub roles: Vec<Uuid>,
    pub joined_at: Timestamp,
    pub banned_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct UpdateMemberRequest {
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub nickname: Option<Option<String>>,
    /// Requires `MANAGE_ROLES`; the nickname alone can be changed by the member.
    #[ts(optional)]
    pub roles: Option<Vec<Uuid>>,
}

/// Overwrite target, mirroring the `overwrite_target` enum in the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum OverwriteTarget {
    Role,
    Member,
}

/// `PUT /channels/{id}/permissions/{target_type}/{target_id}`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChannelOverwrite {
    pub channel_id: Uuid,
    pub target_type: OverwriteTarget,
    pub target_id: Uuid,
    pub allow: PermissionMask,
    pub deny: PermissionMask,
}

#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct PutOverwriteRequest {
    pub allow: PermissionMask,
    pub deny: PermissionMask,
}

// ---------------------------------------------------------------------------
// Convites (RF-02, §6.2)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Invite {
    pub id: Uuid,
    pub code: String,
    pub created_by: Uuid,
    pub guild_id: Option<Uuid>,
    pub max_uses: i32,
    pub uses: i32,
    pub expires_at: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

/// `GET /invites/{code}` — public. Carries only validity and the guild name, so
/// an unauthenticated caller learns nothing about the server structure.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InvitePreview {
    pub code: String,
    pub valid: bool,
    pub guild_name: Option<String>,
    pub expires_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateInviteRequest {
    #[ts(optional)]
    pub guild_id: Option<Uuid>,
    #[ts(optional)]
    pub max_uses: Option<i32>,
    /// Lifetime in seconds. Absent means it never expires.
    #[ts(optional, type = "number")]
    pub expires_in: Option<i64>,
}
