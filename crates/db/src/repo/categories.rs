//! `categories` (RF-05: categories are a persisted entity with their own order).

use protocol::guild::Category;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::error::{missing, DbResult};

#[derive(Debug, Clone)]
pub struct CategoryRow {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub position: i32,
}

impl CategoryRow {
    pub fn to_wire(&self) -> Category {
        Category {
            id: self.id,
            guild_id: self.guild_id,
            name: self.name.clone(),
            position: self.position,
        }
    }
}

pub async fn insert(
    pool: &PgPool,
    id: Uuid,
    guild_id: Uuid,
    name: &str,
    position: i32,
) -> DbResult<CategoryRow> {
    let row = sqlx::query_as!(
        CategoryRow,
        "INSERT INTO categories (id, guild_id, name, position) VALUES ($1, $2, $3, $4) \
         RETURNING id, guild_id, name, position",
        id,
        guild_id,
        name,
        position
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn list_by_guild<'e, E: PgExecutor<'e>>(
    executor: E,
    guild_id: Uuid,
) -> DbResult<Vec<CategoryRow>> {
    let rows = sqlx::query_as!(
        CategoryRow,
        "SELECT id, guild_id, name, position FROM categories \
         WHERE guild_id = $1 ORDER BY position, id",
        guild_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows)
}

pub async fn update(
    pool: &PgPool,
    guild_id: Uuid,
    id: Uuid,
    name: Option<String>,
    position: Option<i32>,
) -> DbResult<CategoryRow> {
    missing(
        "category",
        sqlx::query_as!(
            CategoryRow,
            "UPDATE categories SET name = COALESCE($3, name), \
                                   position = COALESCE($4, position) \
             WHERE guild_id = $1 AND id = $2 \
             RETURNING id, guild_id, name, position",
            guild_id,
            id,
            name,
            position
        )
        .fetch_one(pool)
        .await,
    )
}

/// Channels in the category survive: `channels.category_id` is
/// `ON DELETE SET NULL`, so they move to the top level instead of vanishing.
pub async fn delete(pool: &PgPool, guild_id: Uuid, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!(
        "DELETE FROM categories WHERE guild_id = $1 AND id = $2",
        guild_id,
        id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}
