//! The gateway event handler: everything Discord tells us, turned into replica
//! writes and client events.

use api::AppState;
use protocol::gateway::DispatchEvent;
use protocol::room::{RoomLeave, RoomLeaveReason};
use protocol::scalars::Snowflake;
use serenity::async_trait;
use serenity::gateway::ShardStageUpdateEvent;
use serenity::model::application::Interaction;
use serenity::model::channel::GuildChannel;
use serenity::model::event::ResumedEvent;
use serenity::model::gateway::Ready;
use serenity::model::guild::{Guild, Member, Role, UnavailableGuild};
use serenity::model::id::{GuildId, RoleId};
use serenity::model::user::User;
use serenity::model::voice::VoiceState;
use serenity::prelude::*;

use crate::{pairing, replica_sync, revoke};

pub struct Handler {
    state: AppState,
}

impl Handler {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// A voice channel changed shape: refresh it and recheck who is inside.
    async fn refresh_channel(&self, guild_id: GuildId, channel: &GuildChannel) {
        if !replica_sync::is_room_channel(channel) {
            return;
        }
        self.state
            .replica
            .upsert_channel(guild_id.get(), replica_sync::channel_data(channel))
            .await;
        revoke::sweep_channel(
            &self.state,
            i64::try_from(channel.id.get()).unwrap_or_default(),
        )
        .await;
    }

    /// Fetch the member list over REST and merge it into the replica.
    ///
    /// `GUILD_CREATE` is not a reliable source of members: Discord truncates it
    /// for guilds above `large_threshold`, and delivers only the bot itself when
    /// the members intent is off. Both end the same way — someone gets a 404
    /// joining a room and nothing says why. Asking explicitly removes the
    /// guesswork, and for the guild sizes this product targets it is one call.
    async fn backfill_members(&self, ctx: &Context, guild_id: GuildId) {
        const PAGE: u64 = 1000;
        let mut after: Option<serenity::model::id::UserId> = None;
        let mut total = 0usize;

        loop {
            let page = match guild_id.members(&ctx.http, Some(PAGE), after).await {
                Ok(page) => page,
                Err(error) => {
                    tracing::error!(
                        %error,
                        guild = guild_id.get(),
                        "não consegui listar os membros pela API. Se for 403, o \
                         SERVER MEMBERS INTENT está desligado no portal do Discord"
                    );
                    return;
                }
            };
            if page.is_empty() {
                break;
            }
            after = page.last().map(|m| m.user.id);
            total += page.len();
            for member in &page {
                self.state
                    .replica
                    .upsert_member(
                        guild_id.get(),
                        member.user.id.get(),
                        member.roles.iter().map(|r| r.get()).collect(),
                    )
                    .await;
            }
            if (page.len() as u64) < PAGE {
                break;
            }
        }

        tracing::info!(
            guild = guild_id.get(),
            members = total,
            "membros carregados"
        );
    }

    /// Tell one user's client that it may now open a room.
    async fn announce_room(&self, discord_user_id: u64, discord_channel_id: i64) {
        let Ok(discord_id) = i64::try_from(discord_user_id) else {
            return;
        };
        // Quem nunca pareou nao tem sessao para avisar.
        let Ok(Some(user)) =
            db::repo::users::find_by_discord_id(&self.state.pool, discord_id).await
        else {
            return;
        };

        let Ok(channel) = u64::try_from(discord_channel_id) else {
            return;
        };
        let allowed = self
            .state
            .replica
            .permissions(discord_user_id, channel)
            .await
            .is_some_and(|p| p.can_join_room());
        if !allowed {
            return;
        }

        let Some((guild, name)) = self.state.replica.channel_info(channel).await else {
            return;
        };
        let Ok(room) = api::routes::rooms::state_of(
            &self.state,
            discord_channel_id,
            i64::try_from(guild).unwrap_or_default(),
            name,
        )
        .await
        else {
            return;
        };

        self.state
            .hub
            .publish_to_user(user.id, DispatchEvent::RoomJoin(Box::new(room)))
            .await;
    }

    /// Tell one user's client that the room is over for them.
    async fn announce_leave(&self, discord_user_id: u64, discord_channel_id: i64) {
        let Ok(discord_id) = i64::try_from(discord_user_id) else {
            return;
        };
        let Ok(Some(user)) =
            db::repo::users::find_by_discord_id(&self.state.pool, discord_id).await
        else {
            return;
        };
        self.state
            .hub
            .publish_to_user(
                user.id,
                DispatchEvent::RoomLeave(RoomLeave {
                    discord_channel_id: Snowflake::new(discord_channel_id),
                    reason: RoomLeaveReason::Left,
                }),
            )
            .await;
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        tracing::info!(bot = %ready.user.name, guilds = ready.guilds.len(), "discord connected");

        // O comando e registrado por guild, no `guild_create`. Aqui so limpamos
        // os globais: uma versao anterior registrava `/tela` globalmente, e um
        // comando global convive com o de guild — o usuario ve DOIS `/tela`
        // identicos. Limpar na conexao conserta sozinho quem rodou aquela versao,
        // e nao custa nada para quem nunca rodou.
        match serenity::model::application::Command::set_global_commands(&ctx.http, vec![]).await {
            Ok(removed) if !removed.is_empty() => {
                tracing::info!("comandos globais antigos removidos");
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "limpando comandos globais"),
        }

        self.state.replica.set_connected(true).await;
    }

    async fn resume(&self, _ctx: Context, _: ResumedEvent) {
        self.state.replica.set_connected(true).await;
    }

    async fn shard_stage_update(&self, _ctx: Context, event: ShardStageUpdateEvent) {
        // Qualquer estagio que nao seja "conectado" abre a janela de carencia do
        // RF-09; passar dela, admissoes novas param.
        let connected = matches!(event.new, serenity::gateway::ConnectionStage::Connected);
        self.state.replica.set_connected(connected).await;
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, _is_new: Option<bool>) {
        let data = replica_sync::guild_data(&guild);
        let (members, channels, roles) =
            (data.members.len(), data.channels.len(), data.roles.len());
        self.state.replica.replace_guild(data).await;

        // Contagens no log de propósito. Espelhar menos membros do que o guild
        // tem é o sintoma de duas falhas diferentes, e as duas só aparecem muito
        // depois, como um 404 inexplicável ao entrar numa sala:
        //   - SERVER MEMBERS INTENT desligado: o Discord manda só o próprio bot;
        //   - guild acima do large_threshold: manda uma parte.
        // Comparar com `member_count`, que vem sempre, cobre as duas sem chutar
        // um limiar.
        let expected = guild.member_count;
        tracing::info!(
            guild = guild.id.get(),
            name = %guild.name,
            members,
            expected,
            voice_channels = channels,
            roles,
            "guild espelhado"
        );
        if (members as u64) < expected {
            tracing::info!(
                guild = guild.id.get(),
                members,
                expected,
                "GUILD_CREATE veio incompleto; buscando os membros pela API"
            );
            self.backfill_members(&ctx, guild.id).await;
        }

        // Registro por guild, e nao global, porque comando global leva ate uma
        // hora para propagar: o usuario digitaria `/tela` e nao veria nada, sem
        // nenhum sinal de que a causa e propagacao. Por guild aparece na hora.
        // `set_commands` e idempotente — substitui o conjunto, nao acumula.
        match guild
            .id
            .set_commands(&ctx.http, vec![pairing::command()])
            .await
        {
            Ok(_) => tracing::info!(guild = guild.id.get(), "/tela registrado"),
            Err(error) => tracing::error!(%error, guild = guild.id.get(), "registering /tela"),
        }

        revoke::sweep_guild(&self.state, guild.id.get()).await;
    }

    async fn guild_delete(
        &self,
        _ctx: Context,
        incomplete: UnavailableGuild,
        _full: Option<Guild>,
    ) {
        // `unavailable` e uma queda do lado do Discord, nao uma remocao: apagar
        // a replica ali derrubaria todo mundo por um incidente deles.
        if incomplete.unavailable {
            return;
        }
        self.state.replica.remove_guild(incomplete.id.get()).await;
        revoke::sweep_guild(&self.state, incomplete.id.get()).await;
    }

    async fn guild_member_addition(&self, _ctx: Context, member: Member) {
        self.state
            .replica
            .upsert_member(
                member.guild_id.get(),
                member.user.id.get(),
                member.roles.iter().map(|r| r.get()).collect(),
            )
            .await;
    }

    async fn guild_member_update(
        &self,
        _ctx: Context,
        _old: Option<Member>,
        new: Option<Member>,
        _event: serenity::model::event::GuildMemberUpdateEvent,
    ) {
        let Some(member) = new else {
            return;
        };
        self.state
            .replica
            .upsert_member(
                member.guild_id.get(),
                member.user.id.get(),
                member.roles.iter().map(|r| r.get()).collect(),
            )
            .await;
        revoke::sweep_guild(&self.state, member.guild_id.get()).await;
    }

    async fn guild_member_removal(
        &self,
        _ctx: Context,
        guild_id: GuildId,
        user: User,
        _member: Option<Member>,
    ) {
        self.state
            .replica
            .remove_member(guild_id.get(), user.id.get())
            .await;
        revoke::sweep_guild(&self.state, guild_id.get()).await;
    }

    async fn guild_role_create(&self, _ctx: Context, role: Role) {
        self.state
            .replica
            .upsert_role(
                role.guild_id.get(),
                domain::RoleRef {
                    id: role.id.get(),
                    permissions: role.permissions.bits(),
                },
            )
            .await;
    }

    async fn guild_role_update(&self, _ctx: Context, _old: Option<Role>, role: Role) {
        self.state
            .replica
            .upsert_role(
                role.guild_id.get(),
                domain::RoleRef {
                    id: role.id.get(),
                    permissions: role.permissions.bits(),
                },
            )
            .await;
        revoke::sweep_guild(&self.state, role.guild_id.get()).await;
    }

    async fn guild_role_delete(
        &self,
        _ctx: Context,
        guild_id: GuildId,
        role_id: RoleId,
        _role: Option<Role>,
    ) {
        self.state
            .replica
            .remove_role(guild_id.get(), role_id.get())
            .await;
        revoke::sweep_guild(&self.state, guild_id.get()).await;
    }

    async fn channel_create(&self, _ctx: Context, channel: GuildChannel) {
        self.refresh_channel(channel.guild_id, &channel).await;
    }

    async fn channel_update(&self, _ctx: Context, _old: Option<GuildChannel>, new: GuildChannel) {
        self.refresh_channel(new.guild_id, &new).await;
    }

    async fn channel_delete(
        &self,
        _ctx: Context,
        channel: GuildChannel,
        _messages: Option<Vec<serenity::model::channel::Message>>,
    ) {
        self.state
            .replica
            .remove_channel(channel.guild_id.get(), channel.id.get())
            .await;
        revoke::sweep_channel(
            &self.state,
            i64::try_from(channel.id.get()).unwrap_or_default(),
        )
        .await;
    }

    /// The whole zero-click room join (ADR-0011): Discord says where the user
    /// is, and the client follows.
    async fn voice_state_update(&self, _ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        let before = old
            .as_ref()
            .and_then(|s| replica_sync::voice_channel(s.channel_id));
        let after = replica_sync::voice_channel(new.channel_id);
        if before == after {
            // Mudou mudo, surdez ou transmissao no Discord; nada disso e nosso.
            return;
        }

        let user = new.user_id.get();
        if let Some(left) = before {
            self.announce_leave(user, left).await;
        }
        if let Some(joined) = after {
            self.announce_room(user, joined).await;
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Interaction::Command(command) = interaction else {
            return;
        };
        if command.data.name == pairing::COMMAND_NAME {
            pairing::handle(&self.state, &ctx, &command).await;
        }
    }
}
