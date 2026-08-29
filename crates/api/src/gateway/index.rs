//! The `channel_id -> Set<user_id>` routing index
//! (`docs/protocol/websocket.md` §4.2).
//!
//! This is the **only** authorised exception to CLAUDE.md §2.7, and it is valid
//! for notification routing alone. The worst case of a stale entry here is one
//! notification too many; the worst case of a stale entry in a REST response
//! would be leaked content, which is why nothing that carries content reads it.
//!
//! Invalidation is mandatory on all five triggers of §4.2:
//! a role change, a role assignment change, a channel overwrite change, a
//! channel being created or removed, and a member joining or leaving.

use std::collections::{HashMap, HashSet};

use db::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
struct Entry {
    viewers: HashSet<Uuid>,
    /// Kept so a guild-wide invalidation can find every channel it owns without
    /// touching the database.
    guild_id: Option<Uuid>,
}

#[derive(Default)]
pub struct ViewerIndex {
    channels: RwLock<HashMap<Uuid, Entry>>,
}

impl ViewerIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Who can see this channel, computing and caching on a miss.
    pub async fn viewers(&self, pool: &PgPool, channel_id: Uuid) -> HashSet<Uuid> {
        if let Some(entry) = self.channels.read().await.get(&channel_id) {
            return entry.viewers.clone();
        }
        let viewers: HashSet<Uuid> =
            match db::repo::permissions::viewers_of_channel(pool, channel_id).await {
                Ok(list) => list.into_iter().collect(),
                Err(err) => {
                    // Failing closed is correct: a routing failure must not turn
                    // into a broadcast.
                    tracing::error!(%channel_id, error = %err, "computing channel viewers");
                    return HashSet::new();
                }
            };
        let guild_id = db::repo::channels::find_by_id(pool, channel_id)
            .await
            .ok()
            .and_then(|c| c.guild_id);
        self.channels.write().await.insert(
            channel_id,
            Entry {
                viewers: viewers.clone(),
                guild_id,
            },
        );
        viewers
    }

    /// Drops one channel's entry. Triggered by an overwrite change or by the
    /// channel being created or removed.
    pub async fn invalidate_channel(&self, channel_id: Uuid) {
        self.channels.write().await.remove(&channel_id);
    }

    /// Drops every entry of a guild. Triggered by a role change, a role
    /// assignment change, or a member joining or leaving — all of which can move
    /// the viewer set of every channel at once.
    pub async fn invalidate_guild(&self, guild_id: Uuid) {
        self.channels
            .write()
            .await
            .retain(|_, entry| entry.guild_id != Some(guild_id));
    }

    /// Drops everything. Used on shutdown and by tests.
    pub async fn clear(&self) {
        self.channels.write().await.clear();
    }

    #[cfg(test)]
    async fn len(&self) -> usize {
        self.channels.read().await.len()
    }

    #[cfg(test)]
    async fn insert_for_test(&self, channel_id: Uuid, guild_id: Option<Uuid>, viewers: &[Uuid]) {
        self.channels.write().await.insert(
            channel_id,
            Entry {
                viewers: viewers.iter().copied().collect(),
                guild_id,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invalidating_a_guild_drops_only_that_guilds_channels() {
        let index = ViewerIndex::new();
        let guild_a = Uuid::now_v7();
        let guild_b = Uuid::now_v7();
        let channel_a = Uuid::now_v7();
        let channel_b = Uuid::now_v7();
        let dm = Uuid::now_v7();

        index.insert_for_test(channel_a, Some(guild_a), &[]).await;
        index.insert_for_test(channel_b, Some(guild_b), &[]).await;
        index.insert_for_test(dm, None, &[]).await;
        assert_eq!(index.len().await, 3);

        index.invalidate_guild(guild_a).await;
        assert_eq!(index.len().await, 2, "só o canal do guild A sai");

        index.invalidate_channel(dm).await;
        assert_eq!(index.len().await, 1);

        index.clear().await;
        assert_eq!(index.len().await, 0);
    }

    #[tokio::test]
    async fn invalidating_an_unknown_channel_is_not_an_error() {
        let index = ViewerIndex::new();
        index.invalidate_channel(Uuid::now_v7()).await;
        index.invalidate_guild(Uuid::now_v7()).await;
        assert_eq!(index.len().await, 0);
    }
}
