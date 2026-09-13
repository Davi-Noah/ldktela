//! In-memory mirror of the Discord state that authorization depends on.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use domain::{ChannelRef, DiscordPermissions, GuildRef, MemberRef, Overwrite, RoleRef};
use tokio::sync::RwLock;

/// One voice channel, with the overwrites that decide who gets in.
#[derive(Debug, Clone)]
pub struct ChannelData {
    pub id: u64,
    pub name: String,
    pub overwrites: Vec<Overwrite>,
}

/// One guild and everything under it that bears on a permission decision.
#[derive(Debug, Clone, Default)]
pub struct GuildData {
    pub id: u64,
    pub owner_id: u64,
    pub roles: Vec<RoleRef>,
    /// Discord user id -> the ids of the roles they hold.
    pub members: HashMap<u64, Vec<u64>>,
    /// Voice channels only. Text channels never host a room, so mirroring them
    /// would be state we keep current for nothing.
    pub channels: HashMap<u64, ChannelData>,
}

/// Why a lookup could not be answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    /// The gateway is connected; the answer is current.
    Fresh,
    /// The gateway is down and has been for longer than the grace window. New
    /// admissions must be refused (RF-09).
    Stale,
}

#[derive(Debug, Default)]
struct State {
    guilds: HashMap<u64, GuildData>,
    /// Channel id -> guild id. Without it every permission check would scan
    /// every guild.
    channel_index: HashMap<u64, u64>,
    connected: bool,
    /// When the gateway last dropped. `None` before the first connection ever.
    disconnected_since: Option<Instant>,
}

/// The replica. Cheap to read, written only by the bot's gateway handler.
#[derive(Debug)]
pub struct Replica {
    state: RwLock<State>,
    /// How long the gateway may be down before admissions start failing closed.
    grace: Duration,
}

impl Replica {
    pub fn new(grace: Duration) -> Self {
        Self {
            state: RwLock::new(State::default()),
            grace,
        }
    }

    // -----------------------------------------------------------------------
    // Escrita: so o handler do gateway do bot chama estes.
    // -----------------------------------------------------------------------

    /// Install a guild wholesale, as `GUILD_CREATE` delivers it.
    pub async fn replace_guild(&self, guild: GuildData) {
        let mut state = self.state.write().await;
        // Um GUILD_CREATE de reconexao pode trazer menos canais que antes; as
        // entradas velhas do indice precisam sair, ou um canal apagado
        // continuaria resolvendo.
        state.channel_index.retain(|_, g| *g != guild.id);
        for id in guild.channels.keys() {
            state.channel_index.insert(*id, guild.id);
        }
        state.guilds.insert(guild.id, guild);
    }

    pub async fn remove_guild(&self, guild_id: u64) {
        let mut state = self.state.write().await;
        state.channel_index.retain(|_, g| *g != guild_id);
        state.guilds.remove(&guild_id);
    }

    pub async fn upsert_member(&self, guild_id: u64, user_id: u64, role_ids: Vec<u64>) {
        let mut state = self.state.write().await;
        if let Some(guild) = state.guilds.get_mut(&guild_id) {
            guild.members.insert(user_id, role_ids);
        }
    }

    pub async fn remove_member(&self, guild_id: u64, user_id: u64) {
        let mut state = self.state.write().await;
        if let Some(guild) = state.guilds.get_mut(&guild_id) {
            guild.members.remove(&user_id);
        }
    }

    pub async fn upsert_role(&self, guild_id: u64, role: RoleRef) {
        let mut state = self.state.write().await;
        if let Some(guild) = state.guilds.get_mut(&guild_id) {
            match guild.roles.iter_mut().find(|r| r.id == role.id) {
                Some(existing) => *existing = role,
                None => guild.roles.push(role),
            }
        }
    }

    pub async fn remove_role(&self, guild_id: u64, role_id: u64) {
        let mut state = self.state.write().await;
        if let Some(guild) = state.guilds.get_mut(&guild_id) {
            guild.roles.retain(|r| r.id != role_id);
            for roles in guild.members.values_mut() {
                roles.retain(|r| *r != role_id);
            }
        }
    }

    pub async fn upsert_channel(&self, guild_id: u64, channel: ChannelData) {
        let mut state = self.state.write().await;
        if !state.guilds.contains_key(&guild_id) {
            return;
        }
        state.channel_index.insert(channel.id, guild_id);
        if let Some(guild) = state.guilds.get_mut(&guild_id) {
            guild.channels.insert(channel.id, channel);
        }
    }

    pub async fn remove_channel(&self, guild_id: u64, channel_id: u64) {
        let mut state = self.state.write().await;
        state.channel_index.remove(&channel_id);
        if let Some(guild) = state.guilds.get_mut(&guild_id) {
            guild.channels.remove(&channel_id);
        }
    }

    pub async fn set_connected(&self, connected: bool) {
        let mut state = self.state.write().await;
        state.connected = connected;
        state.disconnected_since = if connected {
            None
        } else {
            Some(Instant::now())
        };
    }

    // -----------------------------------------------------------------------
    // Leitura
    // -----------------------------------------------------------------------

    /// Whether the replica may still be trusted for a new admission.
    ///
    /// A short blip does not refuse anyone — reconnects are routine and a
    /// two-second gap says nothing about permissions. Past the grace window the
    /// answer becomes "we no longer know", and not knowing means no.
    pub async fn staleness(&self) -> Staleness {
        let state = self.state.read().await;
        if state.connected {
            return Staleness::Fresh;
        }
        match state.disconnected_since {
            Some(since) if since.elapsed() <= self.grace => Staleness::Fresh,
            // Nunca conectou: nao ha nada em que confiar.
            _ => Staleness::Stale,
        }
    }

    /// Resolve what a Discord account may do in a voice channel.
    ///
    /// `None` means the channel is not in the replica at all — unknown to us,
    /// which the caller turns into a 404, exactly as an invisible channel would.
    pub async fn permissions(
        &self,
        discord_user_id: u64,
        discord_channel_id: u64,
    ) -> Option<DiscordPermissions> {
        let state = self.state.read().await;
        let guild_id = *state.channel_index.get(&discord_channel_id)?;
        let guild = state.guilds.get(&guild_id)?;
        let channel = guild.channels.get(&discord_channel_id)?;

        // Nao ser membro nao e "sem permissao": e nao ter relacao nenhuma com o
        // guild. O algoritmo pressupoe um membro, entao o caso e tratado aqui.
        let role_ids = guild.members.get(&discord_user_id)?;

        Some(domain::resolve(
            GuildRef {
                id: guild.id,
                owner_id: guild.owner_id,
                roles: &guild.roles,
            },
            MemberRef {
                user_id: discord_user_id,
                role_ids,
            },
            ChannelRef {
                id: channel.id,
                overwrites: &channel.overwrites,
            },
        ))
    }

    /// Guild id and channel name, for the room payload.
    pub async fn channel_info(&self, discord_channel_id: u64) -> Option<(u64, String)> {
        let state = self.state.read().await;
        let guild_id = *state.channel_index.get(&discord_channel_id)?;
        let guild = state.guilds.get(&guild_id)?;
        let channel = guild.channels.get(&discord_channel_id)?;
        Some((guild_id, channel.name.clone()))
    }

    /// Number of guilds mirrored. Observability only (RNF-13).
    pub async fn guild_count(&self) -> usize {
        self.state.read().await.guilds.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUILD: u64 = 100;
    const OWNER: u64 = 1;
    const MEMBER: u64 = 2;
    const OUTSIDER: u64 = 3;
    const CHANNEL: u64 = 900;
    const VIEW_CONNECT: u64 = DiscordPermissions::VIEW_CHANNEL | DiscordPermissions::CONNECT;

    fn guild_with(everyone: u64, overwrites: Vec<Overwrite>) -> GuildData {
        let mut channels = HashMap::new();
        channels.insert(
            CHANNEL,
            ChannelData {
                id: CHANNEL,
                name: "jogos".into(),
                overwrites,
            },
        );
        let mut members = HashMap::new();
        members.insert(OWNER, vec![]);
        members.insert(MEMBER, vec![]);
        GuildData {
            id: GUILD,
            owner_id: OWNER,
            roles: vec![RoleRef {
                id: GUILD,
                permissions: everyone,
            }],
            members,
            channels,
        }
    }

    async fn fresh(replica: &Replica) {
        replica.set_connected(true).await;
    }

    #[tokio::test]
    async fn resolves_through_the_channel_index() {
        let replica = Replica::new(Duration::from_secs(60));
        fresh(&replica).await;
        replica
            .replace_guild(guild_with(VIEW_CONNECT, vec![]))
            .await;

        let perms = replica.permissions(MEMBER, CHANNEL).await.expect("membro");
        assert!(perms.can_join_room());
    }

    #[tokio::test]
    async fn a_non_member_gets_nothing() {
        let replica = Replica::new(Duration::from_secs(60));
        fresh(&replica).await;
        replica
            .replace_guild(guild_with(VIEW_CONNECT, vec![]))
            .await;

        assert!(
            replica.permissions(OUTSIDER, CHANNEL).await.is_none(),
            "quem nao e membro nao tem permissao zero: nao tem relacao nenhuma"
        );
    }

    #[tokio::test]
    async fn an_unknown_channel_is_indistinguishable_from_an_invisible_one() {
        let replica = Replica::new(Duration::from_secs(60));
        fresh(&replica).await;
        assert!(replica.permissions(MEMBER, 12345).await.is_none());
    }

    #[tokio::test]
    async fn removing_a_member_revokes_immediately() {
        let replica = Replica::new(Duration::from_secs(60));
        fresh(&replica).await;
        replica
            .replace_guild(guild_with(VIEW_CONNECT, vec![]))
            .await;
        assert!(replica.permissions(MEMBER, CHANNEL).await.is_some());

        replica.remove_member(GUILD, MEMBER).await;
        assert!(replica.permissions(MEMBER, CHANNEL).await.is_none());
    }

    #[tokio::test]
    async fn removing_a_role_also_detaches_it_from_members() {
        // Sem isso o membro guardaria um id de cargo morto, e um cargo novo que
        // reusasse o id herdaria as permissoes por acidente.
        let replica = Replica::new(Duration::from_secs(60));
        fresh(&replica).await;
        let mut guild = guild_with(0, vec![]);
        guild.roles.push(RoleRef {
            id: 500,
            permissions: VIEW_CONNECT,
        });
        guild.members.insert(MEMBER, vec![500]);
        replica.replace_guild(guild).await;
        assert!(replica
            .permissions(MEMBER, CHANNEL)
            .await
            .expect("membro")
            .can_join_room());

        replica.remove_role(GUILD, 500).await;
        assert!(!replica
            .permissions(MEMBER, CHANNEL)
            .await
            .expect("ainda membro")
            .can_join_room());
    }

    #[tokio::test]
    async fn a_reconnect_that_drops_a_channel_clears_the_index() {
        let replica = Replica::new(Duration::from_secs(60));
        fresh(&replica).await;
        replica
            .replace_guild(guild_with(VIEW_CONNECT, vec![]))
            .await;
        assert!(replica.channel_info(CHANNEL).await.is_some());

        let mut without = guild_with(VIEW_CONNECT, vec![]);
        without.channels.clear();
        replica.replace_guild(without).await;

        assert!(
            replica.channel_info(CHANNEL).await.is_none(),
            "o canal sumiu do guild mas continuou resolvendo pelo indice"
        );
    }

    #[tokio::test]
    async fn removing_a_guild_takes_its_channels_out_of_the_index() {
        let replica = Replica::new(Duration::from_secs(60));
        fresh(&replica).await;
        replica
            .replace_guild(guild_with(VIEW_CONNECT, vec![]))
            .await;
        replica.remove_guild(GUILD).await;
        assert!(replica.permissions(MEMBER, CHANNEL).await.is_none());
        assert_eq!(replica.guild_count().await, 0);
    }

    #[tokio::test]
    async fn a_replica_that_never_connected_is_stale() {
        let replica = Replica::new(Duration::from_secs(60));
        assert_eq!(replica.staleness().await, Staleness::Stale);
    }

    #[tokio::test]
    async fn a_brief_disconnect_stays_within_the_grace_window() {
        let replica = Replica::new(Duration::from_secs(60));
        replica.set_connected(true).await;
        replica.set_connected(false).await;
        assert_eq!(
            replica.staleness().await,
            Staleness::Fresh,
            "uma reconexao de dois segundos nao pode recusar todo mundo"
        );
    }

    #[tokio::test]
    async fn a_long_disconnect_goes_stale() {
        let replica = Replica::new(Duration::ZERO);
        replica.set_connected(true).await;
        replica.set_connected(false).await;
        assert_eq!(replica.staleness().await, Staleness::Stale);
    }

    #[tokio::test]
    async fn reconnecting_makes_it_fresh_again() {
        let replica = Replica::new(Duration::ZERO);
        replica.set_connected(false).await;
        assert_eq!(replica.staleness().await, Staleness::Stale);
        replica.set_connected(true).await;
        assert_eq!(replica.staleness().await, Staleness::Fresh);
    }
}
