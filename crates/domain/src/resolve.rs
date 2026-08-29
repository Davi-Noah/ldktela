//! Permission resolution. **Transcription of SRS §5.3, not an interpretation.**
//!
//! The eight steps below look partly redundant and are not. In particular:
//!
//! * step 4 short-circuits on `ADMINISTRATOR` **before** any channel overwrite is
//!   applied, so an overwrite can never take a permission away from an admin;
//! * step 6 folds every applicable role overwrite into a single `role_deny` and a
//!   single `role_allow` **before** applying them. Applying each role overwrite in
//!   turn produces a different answer whenever one role denies what another allows.
//!
//! Do not reorder. Do not "simplify".

use crate::permissions::Permissions;

/// One channel overwrite row (`channel_overwrites`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Overwrite {
    pub allow: Permissions,
    pub deny: Permissions,
}

impl Overwrite {
    pub fn new(allow: Permissions, deny: Permissions) -> Self {
        Self { allow, deny }
    }
}

/// Everything the algorithm needs about a guild channel. Assembled by `db`.
#[derive(Debug, Clone, Default)]
pub struct GuildContext<'a> {
    /// Step 1: the requester owns the guild.
    pub is_guild_owner: bool,
    /// Step 2: `permissions` of the `@everyone` role.
    pub everyone_permissions: Permissions,
    /// Step 3: `permissions` of every role the member holds, `@everyone` excluded.
    pub member_role_permissions: &'a [Permissions],
    /// Step 5: the channel overwrite targeting the `@everyone` role, if any.
    pub everyone_overwrite: Option<Overwrite>,
    /// Step 6: channel overwrites targeting roles the member holds.
    pub member_role_overwrites: &'a [Overwrite],
    /// Step 7: the channel overwrite targeting this member, if any.
    pub member_overwrite: Option<Overwrite>,
}

/// What kind of channel is being resolved.
#[derive(Debug, Clone)]
pub enum PermissionContext<'a> {
    /// `dm` and `group_dm`. Roles and overwrites do not apply (RF-18a).
    DirectMessage {
        /// A row in `channel_participants` with `left_at IS NULL`.
        is_active_participant: bool,
    },
    /// `text` and `voice`.
    Guild(GuildContext<'a>),
}

/// Resolves the effective permission mask for one user on one channel.
///
/// SRS §5.3, verbatim:
///
/// ```text
/// 0. Se channel.type e 'dm' ou 'group_dm':
///      participante ativo em channel_participants (left_at IS NULL) ->
///          VIEW_CHANNEL | SEND_MESSAGES | ATTACH_FILES | EMBED_LINKS |
///          ADD_REACTIONS | CONNECT_VOICE | SPEAK | VIDEO | SCREEN_SHARE
///      caso contrario -> 0 (nenhuma permissao)
///    FIM. Cargos e overwrites nao se aplicam a conversas diretas.
/// 1. Se o usuario e owner do guild -> todas as permissoes. FIM.
/// 2. base = permissions do cargo @everyone
/// 3. base |= OR das permissions de todos os cargos do membro
/// 4. Se base contem ADMINISTRATOR -> todas as permissoes. FIM.
/// 5. ow = overwrite do canal para o cargo @everyone
///    base = (base & ~ow.deny) | ow.allow
/// 6. Acumular todos os overwrites de cargo aplicaveis ao membro:
///      role_deny  = OR dos deny
///      role_allow = OR dos allow
///    base = (base & ~role_deny) | role_allow
/// 7. ow = overwrite do canal para o membro especifico
///    base = (base & ~ow.deny) | ow.allow
/// 8. Retornar base.
/// ```
pub fn resolve(context: &PermissionContext<'_>) -> Permissions {
    let guild = match context {
        // 0.
        PermissionContext::DirectMessage {
            is_active_participant,
        } => {
            return if *is_active_participant {
                Permissions::DIRECT_MESSAGE
            } else {
                Permissions::NONE
            };
        }
        PermissionContext::Guild(guild) => guild,
    };

    // 1.
    if guild.is_guild_owner {
        return Permissions::ALL;
    }

    // 2.
    let mut base = guild.everyone_permissions;

    // 3.
    for role in guild.member_role_permissions {
        base |= *role;
    }

    // 4.
    if base.contains(Permissions::ADMINISTRATOR) {
        return Permissions::ALL;
    }

    // 5.
    if let Some(ow) = guild.everyone_overwrite {
        base = base.apply_overwrite(ow.allow, ow.deny);
    }

    // 6. Acumular primeiro, aplicar depois. A ordem importa.
    let mut role_allow = Permissions::NONE;
    let mut role_deny = Permissions::NONE;
    for ow in guild.member_role_overwrites {
        role_allow |= ow.allow;
        role_deny |= ow.deny;
    }
    base = base.apply_overwrite(role_allow, role_deny);

    // 7.
    if let Some(ow) = guild.member_overwrite {
        base = base.apply_overwrite(ow.allow, ow.deny);
    }

    // 8.
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(ctx: GuildContext<'_>) -> Permissions {
        resolve(&PermissionContext::Guild(ctx))
    }

    // ---- passo 0: conversas diretas ----

    #[test]
    fn step0_active_participant_of_a_dm_gets_the_fixed_grant() {
        let mask = resolve(&PermissionContext::DirectMessage {
            is_active_participant: true,
        });
        assert_eq!(mask, Permissions::DIRECT_MESSAGE);
        assert!(mask.contains(Permissions::VIEW_CHANNEL));
        assert!(mask.contains(Permissions::SCREEN_SHARE));
    }

    #[test]
    fn step0_non_participant_of_a_dm_gets_nothing() {
        let mask = resolve(&PermissionContext::DirectMessage {
            is_active_participant: false,
        });
        assert_eq!(mask, Permissions::NONE);
        assert!(!mask.contains(Permissions::VIEW_CHANNEL));
    }

    #[test]
    fn step0_short_circuits_before_roles_and_overwrites() {
        // Quem saiu do grupo (left_at preenchido) não recupera acesso por cargo:
        // o passo 0 encerra a resolução antes de qualquer cargo ser consultado.
        let mask = resolve(&PermissionContext::DirectMessage {
            is_active_participant: false,
        });
        assert!(mask.is_empty());
    }

    // ---- passo 1: owner ----

    #[test]
    fn step1_guild_owner_gets_everything_even_with_everything_denied() {
        let deny_all = Overwrite::new(Permissions::NONE, Permissions::ALL);
        let mask = guild(GuildContext {
            is_guild_owner: true,
            everyone_permissions: Permissions::NONE,
            everyone_overwrite: Some(deny_all),
            member_overwrite: Some(deny_all),
            ..Default::default()
        });
        assert_eq!(mask, Permissions::ALL);
    }

    // ---- passo 4: ADMINISTRATOR ----

    #[test]
    fn step4_administrator_beats_every_channel_overwrite() {
        let deny_view = Overwrite::new(Permissions::NONE, Permissions::VIEW_CHANNEL);
        let mask = guild(GuildContext {
            everyone_permissions: Permissions::NONE,
            member_role_permissions: &[Permissions::ADMINISTRATOR],
            everyone_overwrite: Some(deny_view),
            member_overwrite: Some(deny_view),
            ..Default::default()
        });
        assert_eq!(mask, Permissions::ALL);
        assert!(mask.contains(Permissions::VIEW_CHANNEL));
    }

    #[test]
    fn step4_administrator_from_everyone_role_also_short_circuits() {
        let mask = guild(GuildContext {
            everyone_permissions: Permissions::ADMINISTRATOR,
            everyone_overwrite: Some(Overwrite::new(
                Permissions::NONE,
                Permissions::SEND_MESSAGES,
            )),
            ..Default::default()
        });
        assert_eq!(mask, Permissions::ALL);
    }

    // ---- passo 5: canal privado ----

    #[test]
    fn step5_private_channel_is_a_deny_of_view_channel_on_everyone() {
        let base = Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
        let mask = guild(GuildContext {
            everyone_permissions: base,
            everyone_overwrite: Some(Overwrite::new(Permissions::NONE, Permissions::VIEW_CHANNEL)),
            ..Default::default()
        });
        assert!(!mask.contains(Permissions::VIEW_CHANNEL));
        assert!(mask.contains(Permissions::SEND_MESSAGES));
    }

    // ---- passo 6: allow por cargo ----

    #[test]
    fn step6_role_allow_reopens_a_channel_closed_for_everyone() {
        let mask = guild(GuildContext {
            everyone_permissions: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
            member_role_permissions: &[Permissions::NONE],
            everyone_overwrite: Some(Overwrite::new(Permissions::NONE, Permissions::VIEW_CHANNEL)),
            member_role_overwrites: &[Overwrite::new(Permissions::VIEW_CHANNEL, Permissions::NONE)],
            ..Default::default()
        });
        assert!(mask.contains(Permissions::VIEW_CHANNEL));
    }

    #[test]
    fn step6_role_overwrites_accumulate_before_being_applied() {
        // Cargo A nega SEND_MESSAGES, cargo B permite. Acumulando primeiro,
        // allow vence porque é aplicado depois do deny na mesma operação.
        // Aplicar um overwrite por vez, na outra ordem, daria negado.
        let mask = guild(GuildContext {
            everyone_permissions: Permissions::VIEW_CHANNEL,
            member_role_overwrites: &[
                Overwrite::new(Permissions::NONE, Permissions::SEND_MESSAGES),
                Overwrite::new(Permissions::SEND_MESSAGES, Permissions::NONE),
            ],
            ..Default::default()
        });
        assert!(mask.contains(Permissions::SEND_MESSAGES));

        // E a ordem das linhas não altera o resultado, que é o ponto do passo 6.
        let reversed = guild(GuildContext {
            everyone_permissions: Permissions::VIEW_CHANNEL,
            member_role_overwrites: &[
                Overwrite::new(Permissions::SEND_MESSAGES, Permissions::NONE),
                Overwrite::new(Permissions::NONE, Permissions::SEND_MESSAGES),
            ],
            ..Default::default()
        });
        assert_eq!(mask, reversed);
    }

    // ---- passo 7: overwrite de membro ----

    #[test]
    fn step7_member_deny_overrides_a_role_allow() {
        let mask = guild(GuildContext {
            everyone_permissions: Permissions::VIEW_CHANNEL,
            member_role_overwrites: &[Overwrite::new(
                Permissions::SEND_MESSAGES,
                Permissions::NONE,
            )],
            member_overwrite: Some(Overwrite::new(
                Permissions::NONE,
                Permissions::SEND_MESSAGES,
            )),
            ..Default::default()
        });
        assert!(mask.contains(Permissions::VIEW_CHANNEL));
        assert!(!mask.contains(Permissions::SEND_MESSAGES));
    }

    #[test]
    fn step7_member_allow_overrides_a_role_deny() {
        let mask = guild(GuildContext {
            everyone_permissions: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
            member_role_overwrites: &[Overwrite::new(
                Permissions::NONE,
                Permissions::SEND_MESSAGES,
            )],
            member_overwrite: Some(Overwrite::new(
                Permissions::SEND_MESSAGES,
                Permissions::NONE,
            )),
            ..Default::default()
        });
        assert!(mask.contains(Permissions::SEND_MESSAGES));
    }

    // ---- passos 2 e 3 ----

    #[test]
    fn steps2and3_member_roles_union_with_everyone() {
        let mask = guild(GuildContext {
            everyone_permissions: Permissions::VIEW_CHANNEL,
            member_role_permissions: &[Permissions::SEND_MESSAGES, Permissions::ADD_REACTIONS],
            ..Default::default()
        });
        assert_eq!(
            mask,
            Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES | Permissions::ADD_REACTIONS
        );
    }

    #[test]
    fn a_member_with_no_roles_and_a_closed_everyone_sees_nothing() {
        let mask = guild(GuildContext {
            everyone_permissions: Permissions::NONE,
            ..Default::default()
        });
        assert_eq!(mask, Permissions::NONE);
    }
}
