//! Discord's channel permission algorithm, as a pure function (ADR-0010).
//!
//! We do not own permissions any more: the guild does. This module is the
//! transcription of Discord's own resolution order, run against the in-memory
//! replica the bot maintains.
//!
//! ## Why the constants are written out here
//!
//! `CLAUDE.md` §7 says permission bits come from serenity, never hand-written.
//! They are hand-written anyway in this file, for one reason: `domain` must not
//! depend on `sqlx`, `axum` or `tokio` (§3), and the workspace's serenity is
//! configured with `client` + `gateway`, which drag all three in transitively.
//!
//! The rule survives in a different form: `crates/bot` has serenity, and holds a
//! test that asserts every constant below equals serenity's. If Discord ever
//! renumbers a bit, that test fails — the values cannot drift in silence.

/// A resolved Discord permission bitmask for one member in one channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiscordPermissions(u64);

impl DiscordPermissions {
    pub const ADMINISTRATOR: u64 = 1 << 3;
    /// "Video" in the Discord UI: the right to go live in a voice channel.
    pub const STREAM: u64 = 1 << 9;
    pub const VIEW_CHANNEL: u64 = 1 << 10;
    pub const CONNECT: u64 = 1 << 20;

    pub const NONE: Self = Self(0);

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, bit: u64) -> bool {
        self.0 & bit == bit
    }

    /// Everything the product ever asks. Kept as one place so a new question
    /// cannot be answered by reading a bit directly at a call site.
    pub const fn can_view(self) -> bool {
        self.contains(Self::VIEW_CHANNEL)
    }

    /// Whether the member may be in the room at all.
    ///
    /// `CONNECT` without `VIEW_CHANNEL` is meaningless in Discord's own UI, and
    /// treating it as sufficient here would let someone into a room whose
    /// channel they cannot see.
    pub const fn can_join_room(self) -> bool {
        self.can_view() && self.contains(Self::CONNECT)
    }

    /// Whether the member may publish a screen. Discord's own "Video" right is
    /// respected: if the guild says you cannot go live here, neither do we.
    pub const fn can_publish(self) -> bool {
        self.can_join_room() && self.contains(Self::STREAM)
    }
}

/// A role in the replica. The `@everyone` role is the one whose id equals the
/// guild id — that is Discord's convention, not ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleRef {
    pub id: u64,
    pub permissions: u64,
}

/// A guild in the replica.
#[derive(Debug, Clone, Copy)]
pub struct GuildRef<'a> {
    pub id: u64,
    pub owner_id: u64,
    pub roles: &'a [RoleRef],
}

/// A member in the replica.
#[derive(Debug, Clone, Copy)]
pub struct MemberRef<'a> {
    pub user_id: u64,
    pub role_ids: &'a [u64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverwriteKind {
    Role,
    Member,
}

/// A channel permission overwrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overwrite {
    pub target_id: u64,
    pub kind: OverwriteKind,
    pub allow: u64,
    pub deny: u64,
}

/// A channel in the replica.
#[derive(Debug, Clone, Copy)]
pub struct ChannelRef<'a> {
    pub id: u64,
    pub overwrites: &'a [Overwrite],
}

/// Resolve what `member` may do in `channel`.
///
/// The order below is Discord's and is not negotiable — it is the same shape as
/// the algorithm the v1 SRS §5.3 specified for our own RBAC, which is no
/// coincidence: that one was modelled on this one.
pub fn resolve(
    guild: GuildRef<'_>,
    member: MemberRef<'_>,
    channel: ChannelRef<'_>,
) -> DiscordPermissions {
    // 1. O dono do guild tem tudo, e nenhum overwrite o alcanca.
    if member.user_id == guild.owner_id {
        return DiscordPermissions::from_bits(u64::MAX);
    }

    // 2. Base: @everyone, cujo id e o id do guild.
    let mut base = guild
        .roles
        .iter()
        .find(|r| r.id == guild.id)
        .map_or(0, |r| r.permissions);

    // 3. Uniao com os cargos do membro.
    for role_id in member.role_ids {
        if let Some(role) = guild.roles.iter().find(|r| r.id == *role_id) {
            base |= role.permissions;
        }
    }

    // 4. ADMINISTRATOR curto-circuita antes de qualquer overwrite.
    if base & DiscordPermissions::ADMINISTRATOR == DiscordPermissions::ADMINISTRATOR {
        return DiscordPermissions::from_bits(u64::MAX);
    }

    // 5. Overwrite de @everyone no canal.
    if let Some(ow) = channel
        .overwrites
        .iter()
        .find(|o| o.kind == OverwriteKind::Role && o.target_id == guild.id)
    {
        base = (base & !ow.deny) | ow.allow;
    }

    // 6. Overwrites dos cargos do membro, acumulados antes de aplicar: um allow
    //    em qualquer cargo vence um deny em outro cargo.
    let mut role_allow = 0u64;
    let mut role_deny = 0u64;
    for ow in channel.overwrites.iter().filter(|o| {
        o.kind == OverwriteKind::Role
            && o.target_id != guild.id
            && member.role_ids.contains(&o.target_id)
    }) {
        role_allow |= ow.allow;
        role_deny |= ow.deny;
    }
    base = (base & !role_deny) | role_allow;

    // 7. Overwrite do membro especifico, que vence tudo.
    if let Some(ow) = channel
        .overwrites
        .iter()
        .find(|o| o.kind == OverwriteKind::Member && o.target_id == member.user_id)
    {
        base = (base & !ow.deny) | ow.allow;
    }

    DiscordPermissions::from_bits(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUILD: u64 = 100;
    const OWNER: u64 = 1;
    const MEMBER: u64 = 2;
    const ROLE_A: u64 = 200;
    const ROLE_B: u64 = 201;
    const CHANNEL: u64 = 300;

    const VIEW_CONNECT: u64 = DiscordPermissions::VIEW_CHANNEL | DiscordPermissions::CONNECT;

    fn guild(roles: &[RoleRef]) -> GuildRef<'_> {
        GuildRef {
            id: GUILD,
            owner_id: OWNER,
            roles,
        }
    }

    fn channel(overwrites: &[Overwrite]) -> ChannelRef<'_> {
        ChannelRef {
            id: CHANNEL,
            overwrites,
        }
    }

    #[test]
    fn owner_bypasses_every_deny() {
        let roles = [RoleRef {
            id: GUILD,
            permissions: 0,
        }];
        let overwrites = [Overwrite {
            target_id: GUILD,
            kind: OverwriteKind::Role,
            allow: 0,
            deny: u64::MAX,
        }];
        let perms = resolve(
            guild(&roles),
            MemberRef {
                user_id: OWNER,
                role_ids: &[],
            },
            channel(&overwrites),
        );
        assert!(perms.can_join_room());
    }

    #[test]
    fn administrator_bypasses_channel_overwrites() {
        let roles = [
            RoleRef {
                id: GUILD,
                permissions: 0,
            },
            RoleRef {
                id: ROLE_A,
                permissions: DiscordPermissions::ADMINISTRATOR,
            },
        ];
        let overwrites = [Overwrite {
            target_id: GUILD,
            kind: OverwriteKind::Role,
            allow: 0,
            deny: DiscordPermissions::VIEW_CHANNEL,
        }];
        let perms = resolve(
            guild(&roles),
            MemberRef {
                user_id: MEMBER,
                role_ids: &[ROLE_A],
            },
            channel(&overwrites),
        );
        assert!(perms.can_join_room());
    }

    #[test]
    fn everyone_deny_makes_a_private_channel() {
        let roles = [RoleRef {
            id: GUILD,
            permissions: VIEW_CONNECT,
        }];
        let overwrites = [Overwrite {
            target_id: GUILD,
            kind: OverwriteKind::Role,
            allow: 0,
            deny: DiscordPermissions::VIEW_CHANNEL,
        }];
        let perms = resolve(
            guild(&roles),
            MemberRef {
                user_id: MEMBER,
                role_ids: &[],
            },
            channel(&overwrites),
        );
        assert!(!perms.can_view());
        assert!(!perms.can_join_room());
    }

    #[test]
    fn role_allow_reopens_a_private_channel() {
        let roles = [
            RoleRef {
                id: GUILD,
                permissions: VIEW_CONNECT,
            },
            RoleRef {
                id: ROLE_A,
                permissions: 0,
            },
        ];
        let overwrites = [
            Overwrite {
                target_id: GUILD,
                kind: OverwriteKind::Role,
                allow: 0,
                deny: DiscordPermissions::VIEW_CHANNEL,
            },
            Overwrite {
                target_id: ROLE_A,
                kind: OverwriteKind::Role,
                allow: DiscordPermissions::VIEW_CHANNEL,
                deny: 0,
            },
        ];
        let perms = resolve(
            guild(&roles),
            MemberRef {
                user_id: MEMBER,
                role_ids: &[ROLE_A],
            },
            channel(&overwrites),
        );
        assert!(perms.can_join_room());
    }

    #[test]
    fn allow_on_one_role_beats_deny_on_another() {
        // Os overwrites de cargo sao acumulados e so entao aplicados; se fossem
        // aplicados em sequencia, a ordem da lista decidiria o resultado.
        let roles = [
            RoleRef {
                id: GUILD,
                permissions: VIEW_CONNECT,
            },
            RoleRef {
                id: ROLE_A,
                permissions: 0,
            },
            RoleRef {
                id: ROLE_B,
                permissions: 0,
            },
        ];
        let overwrites = [
            Overwrite {
                target_id: ROLE_A,
                kind: OverwriteKind::Role,
                allow: 0,
                deny: DiscordPermissions::CONNECT,
            },
            Overwrite {
                target_id: ROLE_B,
                kind: OverwriteKind::Role,
                allow: DiscordPermissions::CONNECT,
                deny: 0,
            },
        ];
        let member = MemberRef {
            user_id: MEMBER,
            role_ids: &[ROLE_A, ROLE_B],
        };
        assert!(resolve(guild(&roles), member, channel(&overwrites)).can_join_room());

        // A ordem inversa na lista precisa dar o mesmo resultado.
        let reversed = [overwrites[1], overwrites[0]];
        assert!(resolve(guild(&roles), member, channel(&reversed)).can_join_room());
    }

    #[test]
    fn member_overwrite_wins_over_roles() {
        let roles = [
            RoleRef {
                id: GUILD,
                permissions: VIEW_CONNECT,
            },
            RoleRef {
                id: ROLE_A,
                permissions: VIEW_CONNECT,
            },
        ];
        let overwrites = [
            Overwrite {
                target_id: ROLE_A,
                kind: OverwriteKind::Role,
                allow: DiscordPermissions::CONNECT,
                deny: 0,
            },
            Overwrite {
                target_id: MEMBER,
                kind: OverwriteKind::Member,
                allow: 0,
                deny: DiscordPermissions::CONNECT,
            },
        ];
        let perms = resolve(
            guild(&roles),
            MemberRef {
                user_id: MEMBER,
                role_ids: &[ROLE_A],
            },
            channel(&overwrites),
        );
        assert!(perms.can_view());
        assert!(!perms.can_join_room());
    }

    #[test]
    fn connect_without_view_is_not_enough_to_join() {
        let roles = [RoleRef {
            id: GUILD,
            permissions: DiscordPermissions::CONNECT,
        }];
        let perms = resolve(
            guild(&roles),
            MemberRef {
                user_id: MEMBER,
                role_ids: &[],
            },
            channel(&[]),
        );
        assert!(perms.contains(DiscordPermissions::CONNECT));
        assert!(!perms.can_join_room());
    }

    #[test]
    fn publishing_needs_the_stream_right() {
        let roles = [RoleRef {
            id: GUILD,
            permissions: VIEW_CONNECT,
        }];
        let member = MemberRef {
            user_id: MEMBER,
            role_ids: &[],
        };
        assert!(!resolve(guild(&roles), member, channel(&[])).can_publish());

        let with_stream = [RoleRef {
            id: GUILD,
            permissions: VIEW_CONNECT | DiscordPermissions::STREAM,
        }];
        assert!(resolve(guild(&with_stream), member, channel(&[])).can_publish());
    }

    #[test]
    fn a_role_the_member_does_not_have_is_ignored() {
        let roles = [
            RoleRef {
                id: GUILD,
                permissions: 0,
            },
            RoleRef {
                id: ROLE_A,
                permissions: VIEW_CONNECT,
            },
        ];
        let perms = resolve(
            guild(&roles),
            MemberRef {
                user_id: MEMBER,
                role_ids: &[],
            },
            channel(&[]),
        );
        assert!(!perms.can_join_room());
    }
}
