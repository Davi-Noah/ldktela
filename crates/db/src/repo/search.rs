//! Full-text search over messages (RF-17, `docs/api/rest-api.md` §6.8).
//!
//! The query rides `idx_messages_fts`, a GIN index over
//! `to_tsvector('portuguese', content)` (SRS §5.2). Portuguese, not `simple`:
//! stemming is what makes "reunião" find "reuniões", and the index has to be
//! built with the same configuration the query uses or it is not used at all.
//!
//! Two rules from the contract shape this:
//!
//! * The channel set is **passed in**, recomputed per request from current
//!   permissions. The gateway's routing index is explicitly not allowed here.
//! * Ordering is by `id` descending — recency — not by relevance. On a private
//!   server of thirty people the most recent match is nearly always the target,
//!   and it avoids having to explain a ranking.

use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::DbResult;
use crate::repo::messages::MessageRow;

/// Everything the query filters on. `channels` is the already-authorised set.
#[derive(Debug, Clone)]
pub struct SearchQuery<'a> {
    pub channels: &'a [Uuid],
    pub terms: &'a str,
    pub author_id: Option<Uuid>,
    pub since: Option<OffsetDateTime>,
    pub until: Option<OffsetDateTime>,
    /// Keyset cursor: strictly older than this id.
    pub before: Option<Uuid>,
    pub limit: u32,
}

/// One hit, with the ids on either side so the client can open the channel with
/// `around` and no second round trip.
#[derive(Debug, Clone)]
pub struct SearchHitRow {
    pub message: MessageRow,
    pub previous_message_id: Option<Uuid>,
    pub next_message_id: Option<Uuid>,
}

pub struct SearchPage {
    pub hits: Vec<SearchHitRow>,
    pub has_more: bool,
}

/// Runs the search. An empty `channels` short-circuits: with no authorised
/// channel there is nothing to look in, and issuing the query anyway would let
/// timing hint at whether the term exists somewhere.
pub async fn search(pool: &PgPool, query: SearchQuery<'_>) -> DbResult<SearchPage> {
    if query.channels.is_empty() || query.terms.trim().is_empty() {
        return Ok(SearchPage {
            hits: Vec::new(),
            has_more: false,
        });
    }
    let probe = i64::from(query.limit) + 1;

    let rows = sqlx::query!(
        r#"
        SELECT m.id, m.channel_id, m.author_id, m.content, m.reply_to_id,
               m.is_pinned, m.edited_at, m.deleted_at, m.created_at,
               (SELECT p.id FROM messages p
                 WHERE p.channel_id = m.channel_id AND p.id < m.id
                   AND p.deleted_at IS NULL
                 ORDER BY p.id DESC LIMIT 1) AS "previous_message_id?",
               (SELECT n.id FROM messages n
                 WHERE n.channel_id = m.channel_id AND n.id > m.id
                   AND n.deleted_at IS NULL
                 ORDER BY n.id ASC LIMIT 1) AS "next_message_id?"
        FROM messages m
        WHERE m.channel_id = ANY($1)
          AND m.deleted_at IS NULL
          AND to_tsvector('portuguese', m.content)
              @@ websearch_to_tsquery('portuguese', $2)
          AND ($3::uuid IS NULL OR m.author_id = $3)
          AND ($4::timestamptz IS NULL OR m.created_at >= $4)
          AND ($5::timestamptz IS NULL OR m.created_at <= $5)
          AND ($6::uuid IS NULL OR m.id < $6)
        ORDER BY m.id DESC
        LIMIT $7
        "#,
        query.channels,
        query.terms,
        query.author_id,
        query.since,
        query.until,
        query.before,
        probe,
    )
    .fetch_all(pool)
    .await?;

    let has_more = rows.len() as i64 > i64::from(query.limit);
    let hits = rows
        .into_iter()
        .take(query.limit as usize)
        .map(|r| SearchHitRow {
            message: MessageRow {
                id: r.id,
                channel_id: r.channel_id,
                author_id: r.author_id,
                content: r.content,
                reply_to_id: r.reply_to_id,
                is_pinned: r.is_pinned,
                edited_at: r.edited_at,
                deleted_at: r.deleted_at,
                created_at: r.created_at,
            },
            previous_message_id: r.previous_message_id,
            next_message_id: r.next_message_id,
        })
        .collect();

    Ok(SearchPage { hits, has_more })
}
