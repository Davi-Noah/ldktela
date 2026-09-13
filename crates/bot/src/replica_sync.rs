//! Translating serenity's model into the replica's (ADR-0010).
//!
//! Only voice channels are mirrored. A text channel never hosts a room, so
//! keeping it current would be work we never read.

use api::discord::{ChannelData, GuildData};
use domain::{Overwrite, OverwriteKind, RoleRef};
use serenity::model::channel::{GuildChannel, PermissionOverwriteType};
use serenity::model::guild::Guild;
use serenity::model::id::ChannelId;

/// Whether this channel can host a room.
///
/// Stage channels are excluded: they exist for one-to-many talks with a
/// moderated speaker list, which is a different shape from "we are all here
/// watching a screen".
pub fn is_room_channel(channel: &GuildChannel) -> bool {
    channel.kind == serenity::model::channel::ChannelType::Voice
}

pub fn channel_data(channel: &GuildChannel) -> ChannelData {
    ChannelData {
        id: channel.id.get(),
        name: channel.name.clone(),
        overwrites: channel
            .permission_overwrites
            .iter()
            .map(|ow| Overwrite {
                target_id: match ow.kind {
                    PermissionOverwriteType::Role(id) => id.get(),
                    PermissionOverwriteType::Member(id) => id.get(),
                    // Uma variante nova do Discord nao pode virar um overwrite
                    // que casa com alguem por acidente: id 0 nunca casa.
                    _ => 0,
                },
                kind: match ow.kind {
                    PermissionOverwriteType::Member(_) => OverwriteKind::Member,
                    _ => OverwriteKind::Role,
                },
                allow: ow.allow.bits(),
                deny: ow.deny.bits(),
            })
            .collect(),
    }
}

/// The whole guild, as `GUILD_CREATE` delivers it.
pub fn guild_data(guild: &Guild) -> GuildData {
    GuildData {
        id: guild.id.get(),
        owner_id: guild.owner_id.get(),
        roles: guild
            .roles
            .values()
            .map(|role| RoleRef {
                id: role.id.get(),
                permissions: role.permissions.bits(),
            })
            .collect(),
        members: guild
            .members
            .iter()
            .map(|(user_id, member)| {
                (
                    user_id.get(),
                    member.roles.iter().map(|r| r.get()).collect(),
                )
            })
            .collect(),
        channels: guild
            .channels
            .iter()
            .filter(|(_, channel)| is_room_channel(channel))
            .map(|(id, channel)| (id.get(), channel_data(channel)))
            .collect(),
    }
}

/// The voice channel a serenity voice state points at, if any.
pub fn voice_channel(channel_id: Option<ChannelId>) -> Option<i64> {
    channel_id.map(|c| i64::try_from(c.get()).unwrap_or_default())
}
