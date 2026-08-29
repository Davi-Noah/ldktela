//! `messages` — insertion and keyset pagination.
//!
//! **`OFFSET` is never used** (CLAUDE.md §2.1). Every page is bounded by a
//! message id: `(channel_id, id)` is the index, and ids are UUIDv7, so id order
//! is time order. This is what keeps a page stable while other people are
//! writing to the channel: an insert that lands after the cursor cannot shift
//! the rows before it, which is exactly what `OFFSET` fails to guarantee.

use protocol::page::Cursor;
use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{missing, DbResult};

/// One row of `messages`, before authors, attachments and reactions are joined.
#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub author_id: Uuid,
    pub content: String,
    pub reply_to_id: Option<Uuid>,
    pub is_pinned: bool,
    pub edited_at: Option<OffsetDateTime>,
    pub deleted_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

/// A page of messages plus whether older rows remain beyond it.
#[derive(Debug, Clone)]
pub struct MessagePage {
    /// Ordered by id descending, except when the cursor was `after`
    /// (`docs/api/rest-api.md` §4).
    pub messages: Vec<MessageRow>,
    pub has_more: bool,
}

pub async fn insert<'e, E: PgExecutor<'e>>(
    executor: E,
    id: Uuid,
    channel_id: Uuid,
    author_id: Uuid,
    content: &str,
    reply_to_id: Option<Uuid>,
) -> DbResult<MessageRow> {
    let row = sqlx::query_as!(
        MessageRow,
        r#"
        INSERT INTO messages (id, channel_id, author_id, content, reply_to_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, channel_id, author_id, content, reply_to_id,
                  is_pinned, edited_at, deleted_at, created_at
        "#,
        id,
        channel_id,
        author_id,
        content,
        reply_to_id,
    )
    .fetch_one(executor)
    .await?;
    Ok(row)
}

pub async fn find_by_id<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
    id: Uuid,
) -> DbResult<MessageRow> {
    missing(
        "message",
        sqlx::query_as!(
            MessageRow,
            r#"
            SELECT id, channel_id, author_id, content, reply_to_id,
                   is_pinned, edited_at, deleted_at, created_at
            FROM messages
            WHERE channel_id = $1 AND id = $2 AND deleted_at IS NULL
            "#,
            channel_id,
            id
        )
        .fetch_one(executor)
        .await,
    )
}

/// Fetches one page.
///
/// `limit` is the page size the caller already clamped to the contract range.
/// One extra row is read to answer `has_more` without a second query, and the
/// extra row is dropped before returning.
pub async fn page<'e, E: PgExecutor<'e> + Copy>(
    executor: E,
    channel_id: Uuid,
    cursor: Cursor,
    limit: u32,
) -> DbResult<MessagePage> {
    let probe = i64::from(limit) + 1;
    match cursor {
        Cursor::Latest => {
            let mut rows = newest(executor, channel_id, probe).await?;
            let has_more = rows.len() as i64 > i64::from(limit);
            rows.truncate(limit as usize);
            Ok(MessagePage {
                messages: rows,
                has_more,
            })
        }
        Cursor::Before(before) => {
            let mut rows = older_than(executor, channel_id, before, probe).await?;
            let has_more = rows.len() as i64 > i64::from(limit);
            rows.truncate(limit as usize);
            Ok(MessagePage {
                messages: rows,
                has_more,
            })
        }
        Cursor::After(after) => {
            let mut rows = newer_than(executor, channel_id, after, probe).await?;
            let has_more = rows.len() as i64 > i64::from(limit);
            rows.truncate(limit as usize);
            Ok(MessagePage {
                messages: rows,
                has_more,
            })
        }
        Cursor::Around(around) => {
            // limit/2 each side plus the anchor itself, so the caller can open a
            // channel positioned on a search hit or a reply target.
            let half = i64::from(limit) / 2;
            let newer = newer_than(executor, channel_id, around, half).await?;
            let mut older = older_than(executor, channel_id, around, half + 1).await?;
            let has_more = older.len() as i64 > half;
            older.truncate(half as usize);

            let anchor = sqlx::query_as!(
                MessageRow,
                r#"
                SELECT id, channel_id, author_id, content, reply_to_id,
                       is_pinned, edited_at, deleted_at, created_at
                FROM messages
                WHERE channel_id = $1 AND id = $2 AND deleted_at IS NULL
                "#,
                channel_id,
                around
            )
            .fetch_optional(executor)
            .await?;

            // Resposta ordenada por id decrescente: mais novos primeiro.
            let mut messages: Vec<MessageRow> = newer.into_iter().rev().collect();
            messages.extend(anchor);
            messages.extend(older);
            Ok(MessagePage { messages, has_more })
        }
    }
}

async fn newest<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
    limit: i64,
) -> DbResult<Vec<MessageRow>> {
    let rows = sqlx::query_as!(
        MessageRow,
        r#"
        SELECT id, channel_id, author_id, content, reply_to_id,
               is_pinned, edited_at, deleted_at, created_at
        FROM messages
        WHERE channel_id = $1 AND deleted_at IS NULL
        ORDER BY id DESC
        LIMIT $2
        "#,
        channel_id,
        limit
    )
    .fetch_all(executor)
    .await?;
    Ok(rows)
}

/// Strictly older than `before`, newest first.
async fn older_than<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
    before: Uuid,
    limit: i64,
) -> DbResult<Vec<MessageRow>> {
    let rows = sqlx::query_as!(
        MessageRow,
        r#"
        SELECT id, channel_id, author_id, content, reply_to_id,
               is_pinned, edited_at, deleted_at, created_at
        FROM messages
        WHERE channel_id = $1 AND deleted_at IS NULL AND id < $2
        ORDER BY id DESC
        LIMIT $3
        "#,
        channel_id,
        before,
        limit
    )
    .fetch_all(executor)
    .await?;
    Ok(rows)
}

/// Strictly newer than `after`, oldest first — the one case where the response
/// is ascending (`docs/api/rest-api.md` §4).
async fn newer_than<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
    after: Uuid,
    limit: i64,
) -> DbResult<Vec<MessageRow>> {
    let rows = sqlx::query_as!(
        MessageRow,
        r#"
        SELECT id, channel_id, author_id, content, reply_to_id,
               is_pinned, edited_at, deleted_at, created_at
        FROM messages
        WHERE channel_id = $1 AND deleted_at IS NULL AND id > $2
        ORDER BY id ASC
        LIMIT $3
        "#,
        channel_id,
        after,
        limit
    )
    .fetch_all(executor)
    .await?;
    Ok(rows)
}

/// Edit. `edited_at` is set by the database so the client cannot backdate it.
pub async fn update_content<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
    id: Uuid,
    content: &str,
) -> DbResult<MessageRow> {
    missing(
        "message",
        sqlx::query_as!(
            MessageRow,
            r#"
            UPDATE messages SET content = $3, edited_at = NOW()
            WHERE channel_id = $1 AND id = $2 AND deleted_at IS NULL
            RETURNING id, channel_id, author_id, content, reply_to_id,
                      is_pinned, edited_at, deleted_at, created_at
            "#,
            channel_id,
            id,
            content
        )
        .fetch_one(executor)
        .await,
    )
}

/// Logical delete (RF-12). The row stays so cross-propagation and idempotency
/// keep working.
pub async fn soft_delete<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
    id: Uuid,
) -> DbResult<bool> {
    let result = sqlx::query!(
        "UPDATE messages SET deleted_at = NOW() \
         WHERE channel_id = $1 AND id = $2 AND deleted_at IS NULL",
        channel_id,
        id
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn set_pinned<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
    id: Uuid,
    pinned: bool,
) -> DbResult<MessageRow> {
    missing(
        "message",
        sqlx::query_as!(
            MessageRow,
            r#"
            UPDATE messages SET is_pinned = $3
            WHERE channel_id = $1 AND id = $2 AND deleted_at IS NULL
            RETURNING id, channel_id, author_id, content, reply_to_id,
                      is_pinned, edited_at, deleted_at, created_at
            "#,
            channel_id,
            id,
            pinned
        )
        .fetch_one(executor)
        .await,
    )
}

pub async fn list_pinned(pool: &PgPool, channel_id: Uuid) -> DbResult<Vec<MessageRow>> {
    let rows = sqlx::query_as!(
        MessageRow,
        r#"
        SELECT id, channel_id, author_id, content, reply_to_id,
               is_pinned, edited_at, deleted_at, created_at
        FROM messages
        WHERE channel_id = $1 AND is_pinned AND deleted_at IS NULL
        ORDER BY id DESC
        "#,
        channel_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
