//! Loads the inputs of SRS §5.3 and runs the algorithm.
//!
//! **This is the only place permission resolution touches the database.** The
//! algorithm itself lives in `domain::resolve` and is never duplicated here.
//!
//! Every content-bearing query calls this at query time. CLAUDE.md §2.7 forbids
//! trusting a permission cache for anything but gateway routing.

use domain::{resolve, GuildContext, Overwrite, PermissionContext, Permissions};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::error::DbResult;
use crate::types::{ChannelType, OverwriteTarget};

/// What the channel row plus the requester's guild standing tells us.
#[derive(Debug, Clone)]
struct ChannelFacts {
    kind: ChannelType,
    guild_id: Option<Uuid>,
    guild_owner_id: Option<Uuid>,
    is_member: bool,
    is_active_participant: bool,
    everyone_role_id: Option<Uuid>,
    everyone_permissions: Option<i64>,
}

/// Resolved permissions for one user on one channel.
///
/// `None` means the channel does not exist. A channel the user cannot see
/// resolves to `Some(Permissions::NONE)`; the caller turns both into `404`
/// (`docs/api/rest-api.md` §3).
pub async fn resolve_for_channel<'e, E>(
    executor: E,
    user_id: Uuid,
    channel_id: Uuid,
) -> DbResult<Option<Permissions>>
where
    E: PgExecutor<'e> + Copy,
{
    let Some(facts) = load_facts(executor, user_id, channel_id).await? else {
        return Ok(None);
    };

    // Passo 0: conversas diretas terminam aqui.
    if facts.kind.is_direct() {
        return Ok(Some(resolve(&PermissionContext::DirectMessage {
            is_active_participant: facts.is_active_participant,
        })));
    }

    let Some(guild_id) = facts.guild_id else {
        // chk_channel_scope garante que isto nao acontece; se acontecer, negar.
        return Ok(Some(Permissions::NONE));
    };

    // Quem nao e membro do guild (ou esta banido) nao ve nada. Isso nao esta no
    // §5.3 porque o algoritmo pressupoe um membro; a checagem e do chamador.
    if !facts.is_member {
        return Ok(Some(Permissions::NONE));
    }

    let role_permissions = member_role_permissions(executor, guild_id, user_id).await?;
    let member_role_ids: Vec<Uuid> = role_permissions.iter().map(|(id, _)| *id).collect();
    let overwrites = channel_overwrites(executor, channel_id).await?;

    let everyone_role_id = facts.everyone_role_id;
    let mut everyone_overwrite = None;
    let mut member_overwrite = None;
    let mut member_role_overwrites = Vec::new();

    for ow in &overwrites {
        match ow.target_type {
            OverwriteTarget::Member if ow.target_id == user_id => {
                member_overwrite = Some(Overwrite::new(
                    Permissions::from_bits_truncate(ow.allow),
                    Permissions::from_bits_truncate(ow.deny),
                ));
            }
            OverwriteTarget::Role if Some(ow.target_id) == everyone_role_id => {
                everyone_overwrite = Some(Overwrite::new(
                    Permissions::from_bits_truncate(ow.allow),
                    Permissions::from_bits_truncate(ow.deny),
                ));
            }
            OverwriteTarget::Role if member_role_ids.contains(&ow.target_id) => {
                member_role_overwrites.push(Overwrite::new(
                    Permissions::from_bits_truncate(ow.allow),
                    Permissions::from_bits_truncate(ow.deny),
                ));
            }
            _ => {}
        }
    }

    let member_role_permissions: Vec<Permissions> = role_permissions
        .iter()
        .map(|(_, bits)| Permissions::from_bits_truncate(*bits))
        .collect();

    let mask = resolve(&PermissionContext::Guild(GuildContext {
        is_guild_owner: facts.guild_owner_id == Some(user_id),
        everyone_permissions: Permissions::from_bits_truncate(
            facts.everyone_permissions.unwrap_or(0),
        ),
        member_role_permissions: &member_role_permissions,
        everyone_overwrite,
        member_role_overwrites: &member_role_overwrites,
        member_overwrite,
    }));
    Ok(Some(mask))
}

/// Convenience for the common guard: does this user see this channel at all?
pub async fn can_view<'e, E>(executor: E, user_id: Uuid, channel_id: Uuid) -> DbResult<bool>
where
    E: PgExecutor<'e> + Copy,
{
    Ok(resolve_for_channel(executor, user_id, channel_id)
        .await?
        .is_some_and(|mask| mask.contains(Permissions::VIEW_CHANNEL)))
}

/// Guild-level permissions, with no channel in play: used by routes such as
/// `MANAGE_GUILD` and `CREATE_INVITE` that are not scoped to a channel.
///
/// Steps 0 and 5 to 7 do not apply, because there is no channel to overwrite.
pub async fn resolve_for_guild<'e, E>(
    executor: E,
    user_id: Uuid,
    guild_id: Uuid,
) -> DbResult<Permissions>
where
    E: PgExecutor<'e> + Copy,
{
    let row = sqlx::query!(
        r#"
        SELECT g.owner_id,
               (gm.user_id IS NOT NULL AND gm.banned_at IS NULL) AS "is_member!",
               er.permissions AS "everyone_permissions?"
        FROM guilds g
        LEFT JOIN guild_members gm ON gm.guild_id = g.id AND gm.user_id = $2
        LEFT JOIN roles er ON er.guild_id = g.id AND er.is_default
        WHERE g.id = $1
        "#,
        guild_id,
        user_id
    )
    .fetch_optional(executor)
    .await?;

    let Some(row) = row else {
        return Ok(Permissions::NONE);
    };
    if row.owner_id == user_id {
        return Ok(Permissions::ALL);
    }
    if !row.is_member {
        return Ok(Permissions::NONE);
    }

    let role_permissions = member_role_permissions(executor, guild_id, user_id).await?;
    let mut base = Permissions::from_bits_truncate(row.everyone_permissions.unwrap_or(0));
    for (_, bits) in &role_permissions {
        base |= Permissions::from_bits_truncate(*bits);
    }
    if base.contains(Permissions::ADMINISTRATOR) {
        return Ok(Permissions::ALL);
    }
    Ok(base)
}

async fn load_facts<'e, E: PgExecutor<'e>>(
    executor: E,
    user_id: Uuid,
    channel_id: Uuid,
) -> DbResult<Option<ChannelFacts>> {
    let row = sqlx::query!(
        r#"
        SELECT c.type AS "kind: ChannelType",
               c.guild_id,
               g.owner_id AS "guild_owner_id?",
               (gm.user_id IS NOT NULL AND gm.banned_at IS NULL) AS "is_member!",
               (cp.user_id IS NOT NULL AND cp.left_at IS NULL) AS "is_active_participant!",
               er.id AS "everyone_role_id?",
               er.permissions AS "everyone_permissions?"
        FROM channels c
        LEFT JOIN guilds g               ON g.id = c.guild_id
        LEFT JOIN guild_members gm       ON gm.guild_id = c.guild_id AND gm.user_id = $2
        LEFT JOIN roles er               ON er.guild_id = c.guild_id AND er.is_default
        LEFT JOIN channel_participants cp ON cp.channel_id = c.id AND cp.user_id = $2
        WHERE c.id = $1
        "#,
        channel_id,
        user_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|r| ChannelFacts {
        kind: r.kind,
        guild_id: r.guild_id,
        guild_owner_id: r.guild_owner_id,
        is_member: r.is_member,
        is_active_participant: r.is_active_participant,
        everyone_role_id: r.everyone_role_id,
        everyone_permissions: r.everyone_permissions,
    }))
}

/// `(role_id, permissions)` for every role the member holds. The `@everyone`
/// role is not in `member_roles`, and does not need to be: step 2 reads it
/// separately and step 3 is an OR, so a duplicate would be harmless anyway.
async fn member_role_permissions<'e, E: PgExecutor<'e>>(
    executor: E,
    guild_id: Uuid,
    user_id: Uuid,
) -> DbResult<Vec<(Uuid, i64)>> {
    let rows = sqlx::query!(
        r#"
        SELECT r.id, r.permissions
        FROM member_roles mr
        JOIN roles r ON r.id = mr.role_id
        WHERE mr.guild_id = $1 AND mr.user_id = $2
        ORDER BY r.position, r.id
        "#,
        guild_id,
        user_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows.into_iter().map(|r| (r.id, r.permissions)).collect())
}

struct OverwriteRow {
    target_type: OverwriteTarget,
    target_id: Uuid,
    allow: i64,
    deny: i64,
}

/// Every overwrite on the channel. A channel carries a handful at most, so
/// fetching all of them and filtering in Rust beats three round trips.
async fn channel_overwrites<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
) -> DbResult<Vec<OverwriteRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT target_type AS "target_type: OverwriteTarget", target_id, allow, deny
        FROM channel_overwrites WHERE channel_id = $1
        "#,
        channel_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| OverwriteRow {
            target_type: r.target_type,
            target_id: r.target_id,
            allow: r.allow,
            deny: r.deny,
        })
        .collect())
}
