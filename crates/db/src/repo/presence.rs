//! `room_presence` (RF-12).
//!
//! Who is in which room right now. The table is `UNLOGGED`: without a socket
//! there is no presence, so none of this should survive a crash.

use protocol::room::{Publication, PublicationSource, RoomParticipant};
use protocol::scalars::{Snowflake, Timestamp};
use protocol::user::UserSummary;
use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::DbResult;

/// One participant, joined with the profile needed to render them.
#[derive(Debug, Clone)]
pub struct ParticipantRow {
    pub user_id: Uuid,
    pub discord_user_id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub publishing: bool,
    pub joined_at: OffsetDateTime,
    /// When each publication went live (RF-34, ADR-0038). `None` when that
    /// source is not on the air.
    pub screen_since: Option<OffsetDateTime>,
    pub camera_since: Option<OffsetDateTime>,
}

impl ParticipantRow {
    pub fn to_wire(&self) -> RoomParticipant {
        // A ordem e fixa — tela, depois camera — porque ela chega ao espectador
        // como ordem de ladrilho: variar por consulta remexeria a grade sem
        // nada ter mudado.
        let publications = [
            (PublicationSource::Screen, self.screen_since),
            (PublicationSource::Camera, self.camera_since),
        ]
        .into_iter()
        .filter_map(|(source, since)| {
            since.map(|since| Publication {
                source,
                since: Timestamp::new(since),
            })
        })
        .collect();

        RoomParticipant {
            user: UserSummary {
                id: self.user_id,
                discord_user_id: Snowflake::new(self.discord_user_id),
                username: self.username.clone(),
                display_name: self.display_name.clone(),
                avatar_url: self.avatar_url.clone(),
            },
            publications,
        }
    }
}

/// Put a user in a room, or move them to a different one.
///
/// A user is in at most one room, because they are in at most one Discord voice
/// channel — hence the primary key on `user_id` and the upsert here.
pub async fn join<'e, E: PgExecutor<'e>>(
    executor: E,
    user_id: Uuid,
    discord_channel_id: i64,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO room_presence (user_id, discord_channel_id)
        VALUES ($1, $2)
        ON CONFLICT (user_id) DO UPDATE
           SET discord_channel_id = EXCLUDED.discord_channel_id,
               publishing         = FALSE,
               screen_since       = NULL,
               camera_since       = NULL,
               joined_at          = NOW()
        "#,
        user_id,
        discord_channel_id,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Remove a user from whatever room they were in. Returns the channel they left,
/// so the caller knows who to notify without a second query.
pub async fn leave<'e, E: PgExecutor<'e>>(executor: E, user_id: Uuid) -> DbResult<Option<i64>> {
    let channel = sqlx::query_scalar!(
        r#"DELETE FROM room_presence WHERE user_id = $1 RETURNING discord_channel_id"#,
        user_id,
    )
    .fetch_optional(executor)
    .await?;
    Ok(channel)
}

/// One publication going live. Returns the channel and the moment it started,
/// or `None` if the user is not in a room — which happens when a LiveKit webhook
/// arrives after the user already left, and is not an error.
///
/// `COALESCE` makes it idempotent: a publisher republishes the same source when
/// switching window or preset, and resetting the clock would tell every viewer
/// the transmission had just begun (RF-34).
pub async fn start_publication<'e, E: PgExecutor<'e>>(
    executor: E,
    user_id: Uuid,
    source: PublicationSource,
) -> DbResult<Option<StartedPublication>> {
    // Duas consultas e nao uma com nome de coluna dinamico: a macro do SQLx so
    // confere o que esta escrito, e um `format!` aqui abriria mao da conferencia
    // em troca de quatro linhas.
    let row = match source {
        PublicationSource::Screen => {
            sqlx::query_as!(
                StartedPublication,
                r#"
                UPDATE room_presence
                   SET screen_since = COALESCE(screen_since, NOW()),
                       publishing   = TRUE
                 WHERE user_id = $1
                RETURNING discord_channel_id, screen_since AS "since!"
                "#,
                user_id,
            )
            .fetch_optional(executor)
            .await?
        }
        PublicationSource::Camera => {
            sqlx::query_as!(
                StartedPublication,
                r#"
                UPDATE room_presence
                   SET camera_since = COALESCE(camera_since, NOW()),
                       publishing   = TRUE
                 WHERE user_id = $1
                RETURNING discord_channel_id, camera_since AS "since!"
                "#,
                user_id,
            )
            .fetch_optional(executor)
            .await?
        }
    };
    Ok(row)
}

/// The channel the publication belongs to, and when it went live.
#[derive(Debug, Clone, Copy)]
pub struct StartedPublication {
    pub discord_channel_id: i64,
    pub since: OffsetDateTime,
}

/// One publication ending. `still_publishing` says whether the person is still
/// transmitting the *other* source, which is what decides if the share session
/// closes and if the `[LIVE]` tag comes off (ADR-0024).
pub async fn stop_publication<'e, E: PgExecutor<'e>>(
    executor: E,
    user_id: Uuid,
    source: PublicationSource,
) -> DbResult<Option<StoppedPublication>> {
    let row = match source {
        PublicationSource::Screen => {
            sqlx::query_as!(
                StoppedPublication,
                r#"
                UPDATE room_presence
                   SET screen_since = NULL,
                       publishing   = (camera_since IS NOT NULL)
                 WHERE user_id = $1
                RETURNING discord_channel_id, publishing AS "still_publishing!"
                "#,
                user_id,
            )
            .fetch_optional(executor)
            .await?
        }
        PublicationSource::Camera => {
            sqlx::query_as!(
                StoppedPublication,
                r#"
                UPDATE room_presence
                   SET camera_since = NULL,
                       publishing   = (screen_since IS NOT NULL)
                 WHERE user_id = $1
                RETURNING discord_channel_id, publishing AS "still_publishing!"
                "#,
                user_id,
            )
            .fetch_optional(executor)
            .await?
        }
    };
    Ok(row)
}

/// The channel the publication belonged to, and whether anything is left.
#[derive(Debug, Clone, Copy)]
pub struct StoppedPublication {
    pub discord_channel_id: i64,
    pub still_publishing: bool,
}

/// Stops everything this person transmits, and says what was on the air.
///
/// It exists for the events that end a transmission without a per-track
/// webhook: the publishing connection dropping, and access being revoked. The
/// caller needs the list to emit one `SHARE_STOP` per source — a stop nobody
/// announces leaves a tile on every viewer's screen forever.
///
/// `RETURNING` sees the **new** row, which is always empty here, so the previous
/// state is read in a CTE that runs before the update.
pub async fn clear_publications<'e, E: PgExecutor<'e>>(
    executor: E,
    user_id: Uuid,
) -> DbResult<Option<ClearedPublications>> {
    let row = sqlx::query_as!(
        ClearedPublications,
        r#"
        WITH before AS (
            SELECT discord_channel_id, screen_since, camera_since
              FROM room_presence
             WHERE user_id = $1
        ), cleared AS (
            UPDATE room_presence
               SET screen_since = NULL,
                   camera_since = NULL,
                   publishing   = FALSE
             WHERE user_id = $1
            RETURNING user_id
        )
        SELECT b.discord_channel_id,
               (b.screen_since IS NOT NULL) AS "had_screen!",
               (b.camera_since IS NOT NULL) AS "had_camera!"
          FROM before b, cleared c
        "#,
        user_id,
    )
    .fetch_optional(executor)
    .await?;
    Ok(row)
}

/// What the person was transmitting when everything was stopped at once.
#[derive(Debug, Clone, Copy)]
pub struct ClearedPublications {
    pub discord_channel_id: i64,
    pub had_screen: bool,
    pub had_camera: bool,
}

impl ClearedPublications {
    /// The sources that were live, in tile order.
    pub fn sources(&self) -> Vec<PublicationSource> {
        [
            (PublicationSource::Screen, self.had_screen),
            (PublicationSource::Camera, self.had_camera),
        ]
        .into_iter()
        .filter_map(|(source, was_live)| was_live.then_some(source))
        .collect()
    }
}

pub async fn list_by_channel(
    pool: &PgPool,
    discord_channel_id: i64,
) -> DbResult<Vec<ParticipantRow>> {
    let rows = sqlx::query_as!(
        ParticipantRow,
        r#"
        -- O inicio de cada publicacao vem de `room_presence`, e nao da sessao
        -- aberta: desde o ADR-0038 sao duas fontes com relogios proprios, e a
        -- sessao cobre o periodo inteiro em que a pessoa esteve ao vivo.
        SELECT p.user_id,
               u.discord_user_id,
               u.username,
               u.display_name,
               u.avatar_url,
               p.publishing,
               p.joined_at,
               p.screen_since,
               p.camera_since
          FROM room_presence p
          JOIN users u ON u.id = p.user_id
         WHERE p.discord_channel_id = $1
         ORDER BY p.joined_at
        "#,
        discord_channel_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Which room a user is in, if any.
pub async fn channel_of(pool: &PgPool, user_id: Uuid) -> DbResult<Option<i64>> {
    let channel = sqlx::query_scalar!(
        r#"SELECT discord_channel_id FROM room_presence WHERE user_id = $1"#,
        user_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(channel)
}

/// How many publications of one source a room holds. Feeds the admission guard,
/// which counts per source: the ceilings are separate because a face costs a
/// fraction of a screen (ADR-0038).
pub async fn publisher_count(
    pool: &PgPool,
    discord_channel_id: i64,
    source: PublicationSource,
) -> DbResult<i64> {
    let count = match source {
        PublicationSource::Screen => {
            sqlx::query_scalar!(
                r#"
                SELECT COUNT(*) AS "count!"
                  FROM room_presence
                 WHERE discord_channel_id = $1 AND screen_since IS NOT NULL
                "#,
                discord_channel_id,
            )
            .fetch_one(pool)
            .await?
        }
        PublicationSource::Camera => {
            sqlx::query_scalar!(
                r#"
                SELECT COUNT(*) AS "count!"
                  FROM room_presence
                 WHERE discord_channel_id = $1 AND camera_since IS NOT NULL
                "#,
                discord_channel_id,
            )
            .fetch_one(pool)
            .await?
        }
    };
    Ok(count)
}

/// Every room that currently has someone in it.
///
/// Feeds the revocation sweep: after a role changes, only occupied rooms are
/// worth rechecking, and there are rarely more than a handful.
pub async fn occupied_channels(pool: &PgPool) -> DbResult<Vec<i64>> {
    let rows = sqlx::query_scalar!(r#"SELECT DISTINCT discord_channel_id FROM room_presence"#)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Empty a room. Used when LiveKit reports the room finished.
pub async fn clear_channel(pool: &PgPool, discord_channel_id: i64) -> DbResult<u64> {
    let result = sqlx::query!(
        r#"DELETE FROM room_presence WHERE discord_channel_id = $1"#,
        discord_channel_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
