//! `GET /search` (`docs/api/rest-api.md` §6.8, RF-17).

use axum::extract::State;
use axum::routing::get;
use axum::Router;
use db::repo::{channels, message_view, permissions, search};
use domain::validation::{self, limits, Validation};
use domain::Permissions;
use protocol::page::Page;
use protocol::search::{SearchHit, SearchQuery};
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Query};
use crate::middleware::auth::AuthUser;
use crate::permissions::guild_visible;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/search", get(search_messages))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn search_messages(
    State(state): State<AppState>,
    caller: AuthUser,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Page<SearchHit>>, AppError> {
    let mut v = Validation::new();
    v.check(
        "q",
        validation::bounded(&query.q, limits::SEARCH_QUERY_MIN, limits::SEARCH_QUERY_MAX),
    );
    // Scope is mandatory: there is no global search, and one scope at a time.
    match (query.guild_id, query.channel_id) {
        (None, None) | (Some(_), Some(_)) => {
            v.push("guild_id", domain::ValidationCode::Required);
        }
        _ => {}
    }
    v.finish()?;

    // The searchable channel set is recomputed here, from current permissions.
    // The gateway's routing index is explicitly off limits (§6.8): its worst
    // case is one notification too many, which would be leaked content here.
    let channels = searchable_channels(&state, caller.id, &query).await?;

    let limit = validation::page_limit(
        query.limit,
        limits::SEARCH_LIMIT_DEFAULT,
        limits::PAGE_LIMIT_MAX,
    );
    let page = search::search(
        &state.pool,
        search::SearchQuery {
            channels: &channels,
            terms: query.q.trim(),
            author_id: query.author_id,
            since: query.since.map(Into::into),
            until: query.until.map(Into::into),
            before: query.before,
            limit,
        },
    )
    .await?;

    let rows: Vec<db::repo::messages::MessageRow> =
        page.hits.iter().map(|h| h.message.clone()).collect();
    let messages = message_view::hydrate(
        &state.pool,
        &rows,
        caller.id,
        &crate::routes::messages::object_url(&state),
    )
    .await?;

    let data = messages
        .into_iter()
        .zip(page.hits.iter())
        .map(|(message, hit)| SearchHit {
            message,
            previous_message_id: hit.previous_message_id,
            next_message_id: hit.next_message_id,
        })
        .collect();
    Ok(Json(Page::new(data, page.has_more)))
}

/// Channels the caller may search right now.
///
/// A `channel_id` scope narrows to one channel and still has to pass the same
/// visibility check — otherwise naming a private channel directly would be a way
/// around the guild scope.
async fn searchable_channels(
    state: &AppState,
    user_id: Uuid,
    query: &SearchQuery,
) -> Result<Vec<Uuid>, AppError> {
    if let Some(channel_id) = query.channel_id {
        // Invisible answers 404, the same as a channel that does not exist:
        // returning an empty result set would confirm the channel is real.
        crate::permissions::channel_visible(state, user_id, channel_id).await?;
        return Ok(vec![channel_id]);
    }

    let guild_id = query.guild_id.expect("scope was validated above");
    guild_visible(state, user_id, guild_id).await?;

    let mut visible = Vec::new();
    for channel in channels::list_by_guild(&state.pool, guild_id).await? {
        let Some(mask) = permissions::resolve_for_channel(&state.pool, user_id, channel.id).await?
        else {
            continue;
        };
        if mask.contains(Permissions::VIEW_CHANNEL) {
            visible.push(channel.id);
        }
    }
    Ok(visible)
}
