//! The Discord side of a live session (S8): the `[LIVE]` tag on the publisher's
//! nickname.
//!
//! There is no message in the channel. It used to be one, per session, and it
//! mentioned the publisher: people were notified about their own screen share
//! (ADR-0037).
//!
//! The tag is driven by the room snapshots `api` publishes, never by guessing
//! from gateway events. A snapshot is the whole state of the room, so a consumer
//! that missed one still converges, and two changes a moment apart collapse into
//! a single edit instead of two — which is what keeps the edit rate limit out of
//! the path of someone joining and leaving repeatedly.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use api::announce::RoomBroadcast;
use api::AppState;
use db::repo::live_tags;
use serenity::builder::EditMember;
use serenity::model::id::{GuildId, UserId};
use serenity::prelude::*;
use time::OffsetDateTime;
use tokio::sync::broadcast::error::RecvError;

/// Discord caps a nickname at 32 characters, prefix included.
const PREFIX: &str = "[🔴LIVE] ";
/// The prefix before the emoji. A nickname still carrying it is already tagged;
/// tagging it again would read "[🔴LIVE] [LIVE] ...".
const LEGACY_PREFIX: &str = "[LIVE] ";
/// Counted in UTF-16 units, the way Discord's client counts: an emoji outside
/// the BMP, like the one in the prefix, is two of them.
const NICK_LIMIT: usize = 32;

/// How long snapshots pile up before one edit goes out.
///
/// Discord rate limits member edits. Joining and leaving a voice channel is
/// something people do in bursts, and each one is a new snapshot — without
/// coalescing, a lively room would spend the session rate limited and a
/// nickname would lag behind the share it is meant to announce.
const FLUSH: Duration = Duration::from_secs(2);

/// Why a member cannot be renamed. Both cases are Discord's rules, not ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameVerdict {
    Allowed,
    /// The server owner. No permission exists that lets a bot rename them.
    Owner,
    /// Their highest role sits at or above the bot's. Fixable by moving the
    /// bot's role up, which is why the log says so.
    AboveBot,
}

/// Whether the bot may rename this member (ADR-0024, guard 1).
///
/// Checked *before* trying, so the failure is a log line for the operator rather
/// than a rejected request per share, and never anything the room sees.
pub fn may_rename(
    target_id: u64,
    target_top_role: u16,
    bot_top_role: u16,
    owner_id: u64,
) -> RenameVerdict {
    if target_id == owner_id {
        return RenameVerdict::Owner;
    }
    if target_top_role >= bot_top_role {
        return RenameVerdict::AboveBot;
    }
    RenameVerdict::Allowed
}

/// The tagged nickname, or `None` when it is already tagged.
///
/// Idempotent (ADR-0024, guard 4): tagging twice must not stack prefixes, which
/// would also poison the saved nickname on the second pass.
pub fn tagged_nickname(current: Option<&str>, username: &str) -> Option<String> {
    let base = current.unwrap_or(username);
    if base.starts_with(PREFIX) || base.starts_with(LEGACY_PREFIX) {
        return None;
    }
    // Orcado em unidades UTF-16, e nao em bytes: `PREFIX.len()` sao bytes, e
    // com o emoji o prefixo tem 11 bytes para 8 caracteres — os apelidos
    // passaram a ser cortados tres caracteres antes da hora. UTF-16 e o que o
    // cliente do Discord conta, e e o unico corte que nunca estoura o limite.
    let room = NICK_LIMIT - PREFIX.encode_utf16().count();
    // Corta por caractere inteiro: cortar no meio produz um apelido que o
    // Discord recusa, e a maioria dos apelidos daqui tem acento.
    let mut used = 0;
    let trimmed: String = base
        .chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= room
        })
        .collect();
    Some(format!("{PREFIX}{trimmed}"))
}

/// Starts the task that keeps Discord in step with the rooms.
pub fn spawn(state: AppState, ctx: Context) {
    tokio::spawn(async move {
        if let Err(error) = sweep_stale_tags(&state, &ctx).await {
            tracing::error!(%error, "não consegui limpar as tags [LIVE] de uma execução anterior");
        }
        run(state, ctx).await;
    });
}

/// RF-40: undo whatever a crash left marked, before accepting a new session.
///
/// Runs before the loop starts, not alongside it: a nickname left dirty is
/// someone else's server showing our bug, and a new session must not be able to
/// interleave with the cleanup and have its own tag undone.
async fn sweep_stale_tags(state: &AppState, ctx: &Context) -> anyhow::Result<()> {
    let left = live_tags::all(&state.pool).await?;
    if left.is_empty() {
        return Ok(());
    }
    tracing::info!(count = left.len(), "limpando tags [LIVE] de uma queda");
    for tag in left {
        restore_nickname(state, ctx, tag.discord_guild_id, tag.discord_user_id).await;
    }
    Ok(())
}

async fn run(state: AppState, ctx: Context) {
    let mut snapshots = state.announce.subscribe();
    let mut pending: HashMap<i64, RoomBroadcast> = HashMap::new();
    let mut tagged: HashSet<(i64, i64)> = HashSet::new();
    let mut flush = tokio::time::interval(FLUSH);
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            received = snapshots.recv() => match received {
                // A mais nova vence: o estado antigo de uma sala nao tem valor.
                Ok(snapshot) => {
                    pending.insert(snapshot.discord_channel_id, snapshot);
                }
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "anúncio atrasado; seguindo pelo estado mais novo");
                }
                Err(RecvError::Closed) => return,
            },
            _ = flush.tick() => {
                for (_, snapshot) in pending.drain() {
                    apply(&state, &ctx, &snapshot, &mut tagged).await;
                }
            }
        }
    }
}

async fn apply(
    state: &AppState,
    ctx: &Context,
    snapshot: &RoomBroadcast,
    tagged: &mut HashSet<(i64, i64)>,
) {
    let channel = snapshot.discord_channel_id;
    let Some(guild_id) = state
        .replica
        .channel_info(channel.unsigned_abs())
        .await
        .map(|(guild, _)| i64::try_from(guild).unwrap_or_default())
    else {
        // Canal fora da réplica: não é nosso, e renomear alguém num servidor que
        // não pediu seria escrever nele.
        return;
    };

    update_tags(state, ctx, guild_id, snapshot, tagged).await;
}

async fn update_tags(
    state: &AppState,
    ctx: &Context,
    guild_id: i64,
    snapshot: &RoomBroadcast,
    tagged: &mut HashSet<(i64, i64)>,
) {
    let wanted: HashSet<i64> = snapshot.publishers.iter().copied().collect();

    for user in &wanted {
        if tagged.insert((guild_id, *user)) {
            apply_nickname(state, ctx, guild_id, *user).await;
        }
    }

    let stale: Vec<i64> = tagged
        .iter()
        .filter(|(guild, user)| *guild == guild_id && !wanted.contains(user))
        .map(|(_, user)| *user)
        .collect();
    for user in stale {
        tagged.remove(&(guild_id, user));
        restore_nickname(state, ctx, guild_id, user).await;
    }
}

async fn apply_nickname(state: &AppState, ctx: &Context, guild_id: i64, user_id: i64) {
    let guild = GuildId::new(guild_id.unsigned_abs());
    let mut member = match guild
        .member(&ctx.http, UserId::new(user_id.unsigned_abs()))
        .await
    {
        Ok(member) => member,
        Err(error) => {
            tracing::warn!(%error, guild_id, user_id, "membro não encontrado para marcar");
            return;
        }
    };

    match verdict(ctx, guild, &member) {
        RenameVerdict::Allowed => {}
        RenameVerdict::Owner => {
            tracing::info!(
                guild_id,
                user_id,
                "dono do servidor não recebe a tag [LIVE]: o Discord não permite, \
                 e não há contorno (ADR-0024)"
            );
            return;
        }
        RenameVerdict::AboveBot => {
            tracing::info!(
                guild_id,
                user_id,
                "cargo acima do bot: sem tag [LIVE]. Mova o cargo do bot para o \
                 topo da hierarquia para corrigir"
            );
            return;
        }
    }

    let Some(nickname) = tagged_nickname(member.nick.as_deref(), &member.user.name) else {
        return; // Já marcado.
    };

    // Guardar antes de renomear: se o processo cair entre as duas coisas, sobra
    // um registro sem tag (inofensivo) em vez de uma tag sem registro, que
    // deixaria o apelido sujo para sempre.
    if let Err(error) = live_tags::remember(
        &state.pool,
        guild_id,
        user_id,
        member.nick.as_deref(),
        OffsetDateTime::now_utc(),
    )
    .await
    {
        tracing::error!(%error, guild_id, user_id, "não consegui guardar o apelido anterior");
        return;
    }

    if let Err(error) = member
        .edit(&ctx.http, EditMember::new().nickname(nickname))
        .await
    {
        tracing::warn!(%error, guild_id, user_id, "não consegui aplicar a tag [LIVE]");
        let _ = live_tags::forget(&state.pool, guild_id, user_id).await;
    }
}

async fn restore_nickname(state: &AppState, ctx: &Context, guild_id: i64, user_id: i64) {
    let saved = match live_tags::forget(&state.pool, guild_id, user_id).await {
        Ok(Some(saved)) => saved,
        // Não estava marcado: desmarcar é idempotente (ADR-0024, guarda 4).
        Ok(None) => return,
        Err(error) => {
            tracing::error!(%error, guild_id, user_id, "não consegui ler o apelido anterior");
            return;
        }
    };

    let guild = GuildId::new(guild_id.unsigned_abs());
    // String vazia é como o Discord representa "sem apelido". Guardar `None` e
    // restaurar o nome de usuário deixaria um apelido onde não havia nenhum.
    let nickname = saved.previous_nick.unwrap_or_default();
    if let Err(error) = guild
        .edit_member(
            &ctx.http,
            UserId::new(user_id.unsigned_abs()),
            EditMember::new().nickname(nickname),
        )
        .await
    {
        tracing::warn!(%error, guild_id, user_id, "não consegui restaurar o apelido");
    }
}

/// Reads the hierarchy out of the cache and asks `may_rename`.
///
/// Falls back to refusing when the guild is not cached: not renaming is always
/// recoverable, renaming someone we should not have is not.
fn verdict(
    ctx: &Context,
    guild_id: GuildId,
    member: &serenity::model::guild::Member,
) -> RenameVerdict {
    let Some(guild) = ctx.cache.guild(guild_id) else {
        return RenameVerdict::AboveBot;
    };
    let top = |member: &serenity::model::guild::Member| -> u16 {
        member
            .roles
            .iter()
            .filter_map(|role| guild.roles.get(role))
            .map(|role| role.position)
            .max()
            .unwrap_or(0)
    };
    let Some(bot) = guild.members.get(&ctx.cache.current_user().id) else {
        return RenameVerdict::AboveBot;
    };
    may_rename(
        member.user.id.get(),
        top(member),
        top(bot),
        guild.owner_id.get(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: u64 = 1;
    const MEMBER: u64 = 2;

    #[test]
    fn the_server_owner_is_never_renamed() {
        // Limitação do Discord, sem contorno nenhum — nem movendo cargo, nem com
        // permissão de administrador. No servidor de teste, o dono é o dono do
        // projeto, então este é o caso que mais aparece.
        assert_eq!(may_rename(OWNER, 0, 99, OWNER), RenameVerdict::Owner);
    }

    #[test]
    fn a_role_at_or_above_the_bot_blocks_the_rename() {
        assert_eq!(may_rename(MEMBER, 5, 5, OWNER), RenameVerdict::AboveBot);
        assert_eq!(may_rename(MEMBER, 6, 5, OWNER), RenameVerdict::AboveBot);
    }

    #[test]
    fn a_member_below_the_bot_can_be_renamed() {
        assert_eq!(may_rename(MEMBER, 4, 5, OWNER), RenameVerdict::Allowed);
    }

    #[test]
    fn tagging_twice_does_not_stack_the_prefix() {
        // Guarda 4 do ADR-0024. Sem isto o apelido viraria "[LIVE] [LIVE] ..."
        // e o apelido salvo na segunda vez ja viria marcado.
        let tagged = format!("{PREFIX}Gabriel");
        assert_eq!(tagged_nickname(Some(&tagged), "gabriel.2352"), None);
    }

    /// Quem ficou marcado pela versao anterior, sem o emoji, nao pode ganhar o
    /// prefixo novo por cima do velho.
    #[test]
    fn the_prefix_without_the_emoji_still_counts_as_tagged() {
        assert_eq!(
            tagged_nickname(Some("[LIVE] Gabriel"), "gabriel.2352"),
            None
        );
    }

    #[test]
    fn someone_without_a_nickname_is_tagged_over_their_username() {
        assert_eq!(
            tagged_nickname(None, "gabriel.2352"),
            Some(format!("{PREFIX}gabriel.2352"))
        );
    }

    #[test]
    fn a_long_nickname_is_cut_to_fit_the_prefix() {
        let long = "a".repeat(40);
        let tagged = tagged_nickname(Some(&long), "x").expect("marcado");
        assert_eq!(tagged.encode_utf16().count(), NICK_LIMIT);
        assert!(tagged.starts_with(PREFIX));
    }

    #[test]
    fn cutting_a_long_nickname_does_not_split_a_character() {
        // Cortar UTF-8 por byte produz apelido que o Discord recusa, e apelido
        // com acento e a regra por aqui, nao a excecao.
        let long = "ãé".repeat(30);
        let tagged = tagged_nickname(Some(&long), "x").expect("marcado");
        assert_eq!(tagged.encode_utf16().count(), NICK_LIMIT);
        assert!(tagged.is_char_boundary(tagged.len()));
    }

    /// Um emoji no apelido vale duas unidades, como o do prefixo. Cortar pela
    /// contagem de caracteres deixaria este apelido um acima do limite.
    #[test]
    fn an_emoji_in_the_nickname_is_budgeted_as_two() {
        let long = "🎮".repeat(20);
        let tagged = tagged_nickname(Some(&long), "x").expect("marcado");
        assert!(tagged.encode_utf16().count() <= NICK_LIMIT, "{tagged}");
    }
}
