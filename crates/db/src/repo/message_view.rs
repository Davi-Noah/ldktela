//! Turning `messages` rows into the wire object of `docs/api/rest-api.md` §7.
//!
//! A message carries its author, its reply preview, its attachments and its
//! aggregated reactions. Fetching those per message would be four queries per
//! row; these functions load a whole page in four queries total.

use std::collections::HashMap;

use protocol::message::{Attachment, BridgeInfo, Message, Reaction, ReplyPreview};
use protocol::scalars::Timestamp;
use protocol::user::UserSummary;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbResult;
use crate::repo::messages::MessageRow;
use crate::types::MessageOrigin;

/// How much of a replied-to message the preview shows.
const EXCERPT_CHARS: usize = 120;

/// Hydrates a page of rows into wire objects.
///
/// `viewer` decides the `me` flag on each reaction; `nonce_for` is echoed only
/// on the message whose id matches, because a nonce goes to the originating
/// session alone (§6.5).
pub async fn hydrate(
    pool: &PgPool,
    rows: &[MessageRow],
    viewer: Uuid,
    public_url: &(dyn Fn(&str) -> String + Send + Sync),
) -> DbResult<Vec<Message>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let reply_ids: Vec<Uuid> = rows.iter().filter_map(|r| r.reply_to_id).collect();
    let mut author_ids: Vec<Uuid> = rows.iter().map(|r| r.author_id).collect();
    author_ids.sort_unstable();
    author_ids.dedup();

    let authors = load_authors(pool, &author_ids).await?;
    let attachments = load_attachments(pool, &ids, public_url).await?;
    let reactions = load_reactions(pool, &ids, viewer).await?;
    let replies = load_reply_previews(pool, &reply_ids).await?;
    let bridged = load_bridge_origins(pool, &ids).await?;

    Ok(rows
        .iter()
        .map(|row| Message {
            id: row.id,
            channel_id: row.channel_id,
            author: authors.get(&row.author_id).cloned().unwrap_or_else(|| {
                // The author row is `ON DELETE` restricted, so this is only
                // reachable if someone deleted a user by hand.
                UserSummary {
                    id: row.author_id,
                    username: "desconhecido".into(),
                    display_name: None,
                    avatar_url: None,
                    accent_color: None,
                    is_migrated: false,
                }
            }),
            content: row.content.clone(),
            reply_to: row.reply_to_id.and_then(|id| replies.get(&id).cloned()),
            attachments: attachments.get(&row.id).cloned().unwrap_or_default(),
            reactions: reactions.get(&row.id).cloned().unwrap_or_default(),
            is_pinned: row.is_pinned,
            edited_at: row.edited_at.map(Timestamp::new),
            created_at: Timestamp::new(row.created_at),
            nonce: None,
            bridge: bridged.get(&row.id).map(|origin| BridgeInfo {
                origin: (*origin).into(),
            }),
        })
        .collect())
}

/// Convenience for the single-message case.
pub async fn hydrate_one(
    pool: &PgPool,
    row: &MessageRow,
    viewer: Uuid,
    public_url: &(dyn Fn(&str) -> String + Send + Sync),
) -> DbResult<Message> {
    let mut list = hydrate(pool, std::slice::from_ref(row), viewer, public_url).await?;
    Ok(list.remove(0))
}

async fn load_authors(pool: &PgPool, ids: &[Uuid]) -> DbResult<HashMap<Uuid, UserSummary>> {
    let rows = sqlx::query!(
        "SELECT id, username, display_name, avatar_url, accent_color, is_migrated \
         FROM users WHERE id = ANY($1)",
        ids
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                r.id,
                UserSummary {
                    id: r.id,
                    username: r.username,
                    display_name: r.display_name,
                    avatar_url: r.avatar_url,
                    accent_color: r.accent_color,
                    is_migrated: r.is_migrated,
                },
            )
        })
        .collect())
}

async fn load_attachments(
    pool: &PgPool,
    ids: &[Uuid],
    public_url: &(dyn Fn(&str) -> String + Send + Sync),
) -> DbResult<HashMap<Uuid, Vec<Attachment>>> {
    let rows = sqlx::query!(
        "SELECT id, message_id, r2_key, skip_reason, filename, content_type, \
                size_bytes, width, height \
         FROM attachments WHERE message_id = ANY($1) ORDER BY id",
        ids
    )
    .fetch_all(pool)
    .await?;
    let mut out: HashMap<Uuid, Vec<Attachment>> = HashMap::new();
    for r in rows {
        out.entry(r.message_id).or_default().push(Attachment {
            id: r.id,
            filename: r.filename,
            content_type: r.content_type,
            size_bytes: r.size_bytes,
            width: r.width,
            height: r.height,
            // `r2_key` null means the attachment was never migrated (RF-25a);
            // the client renders a placeholder rather than a broken image.
            url: r.r2_key.as_deref().map(public_url),
            skip_reason: r.skip_reason,
        });
    }
    Ok(out)
}

async fn load_reactions(
    pool: &PgPool,
    ids: &[Uuid],
    viewer: Uuid,
) -> DbResult<HashMap<Uuid, Vec<Reaction>>> {
    let rows = sqlx::query!(
        r#"
        SELECT message_id, emoji,
               COUNT(*) AS "count!",
               bool_or(user_id = $2) AS "me!"
        FROM reactions
        WHERE message_id = ANY($1)
        GROUP BY message_id, emoji
        ORDER BY MIN(created_at)
        "#,
        ids,
        viewer
    )
    .fetch_all(pool)
    .await?;
    let mut out: HashMap<Uuid, Vec<Reaction>> = HashMap::new();
    for r in rows {
        out.entry(r.message_id).or_default().push(Reaction {
            emoji: r.emoji,
            count: r.count,
            me: r.me,
        });
    }
    Ok(out)
}

async fn load_reply_previews(pool: &PgPool, ids: &[Uuid]) -> DbResult<HashMap<Uuid, ReplyPreview>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query!(
        "SELECT m.id, u.username, m.content, m.deleted_at \
         FROM messages m JOIN users u ON u.id = m.author_id \
         WHERE m.id = ANY($1)",
        ids
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let excerpt = if r.deleted_at.is_some() {
                // The referenced message is gone; the header still renders, so
                // the reply does not lose its context entirely.
                "mensagem apagada".to_string()
            } else {
                r.content.chars().take(EXCERPT_CHARS).collect()
            };
            (
                r.id,
                ReplyPreview {
                    id: r.id,
                    author_username: r.username,
                    excerpt,
                },
            )
        })
        .collect())
}

async fn load_bridge_origins(
    pool: &PgPool,
    ids: &[Uuid],
) -> DbResult<HashMap<Uuid, MessageOrigin>> {
    let rows = sqlx::query!(
        r#"
        SELECT internal_message_id, origin AS "origin: MessageOrigin"
        FROM message_mappings WHERE internal_message_id = ANY($1)
        "#,
        ids
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.internal_message_id, r.origin))
        .collect())
}
