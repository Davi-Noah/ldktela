//! `/channels/{id}/messages` and everything hanging off a message
//! (`docs/api/rest-api.md` §6.5).
//!
//! Every mutation here ends in the dispatch the protocol pairs it with; a
//! mutation without its event is a client that silently drifts.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::Router;
use db::repo::engagement::NewAttachment;
use db::repo::{engagement, message_view, messages};
use domain::validation::{self, limits, Validation};
use domain::{Permissions, ValidationCode};
use protocol::channel::{ReadState, UpdateReadStateRequest};
use protocol::gateway::DispatchEvent;
use protocol::message::{
    CreateMessageRequest, EditMessageRequest, Message, MessageDelete, ReactionEvent, TypingStart,
};
use protocol::page::{Page, PageQuery};
use protocol::scalars::Timestamp;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Path, Query};
use crate::middleware::auth::AuthUser;
use crate::nonce::{self, Claim};
use crate::permissions::{channel_visible, require_channel};
use crate::state::AppState;

/// RF-13: the indicator is ephemeral and expires on its own.
const TYPING_TTL_SECONDS: i64 = 5;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/channels/{id}/messages", get(list).post(create))
        .route(
            "/channels/{id}/messages/{mid}",
            axum::routing::patch(edit).delete(remove),
        )
        .route("/channels/{id}/messages/{mid}/pin", put(pin).delete(unpin))
        .route("/channels/{id}/pins", get(pins))
        .route(
            "/channels/{id}/messages/{mid}/reactions/{emoji}/@me",
            put(react).delete(unreact),
        )
        .route("/channels/{id}/typing", post(typing))
        .route("/channels/{id}/read-state", put(read_state))
}

/// Public URL of a stored object. R2 is not wired until E8, so the key is
/// rendered against the configured public base and nothing is signed here.
fn object_url(state: &AppState) -> impl Fn(&str) -> String + '_ {
    move |key: &str| {
        format!(
            "{}/{}",
            state.config.media_base_url.trim_end_matches('/'),
            key
        )
    }
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn list(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<Message>>, AppError> {
    // Permission is resolved here, at query time, never from the gateway index
    // (CLAUDE.md §2.7). An invisible channel is a 404.
    channel_visible(&state, caller.id, id).await?;

    let cursor = query.cursor().ok_or_else(|| {
        AppError::Validation(vec![protocol::error::FieldError {
            field: "before".into(),
            code: "NOT_ALLOWED".into(),
        }])
    })?;
    let limit = validation::page_limit(
        query.limit,
        limits::PAGE_LIMIT_DEFAULT,
        limits::PAGE_LIMIT_MAX,
    );

    let page = messages::page(&state.pool, id, cursor, limit).await?;
    let data =
        message_view::hydrate(&state.pool, &page.messages, caller.id, &object_url(&state)).await?;
    Ok(Json(Page::new(data, page.has_more)))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn create(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateMessageRequest>,
) -> Result<Response, AppError> {
    let access = require_channel(&state, caller.id, id, Permissions::SEND_MESSAGES).await?;

    let mut v = Validation::new();
    v.check(
        "content",
        validation::message_body(&body.content, body.attachments.len()),
    );
    v.check(
        "attachments",
        validation::attachment_count(body.attachments.len(), state.config.max_attachments),
    );
    for attachment in &body.attachments {
        v.check(
            "attachments",
            validation::attachment(
                attachment.size_bytes,
                &attachment.content_type,
                state.config.max_attachment_bytes,
                &state.config.allowed_content_types,
            ),
        );
    }
    if let Some(nonce) = &body.nonce {
        if !nonce::is_acceptable(nonce) {
            v.push("nonce", ValidationCode::TooLong);
        }
    }
    v.finish()?;

    if !body.attachments.is_empty() && !access.permissions.contains(Permissions::ATTACH_FILES) {
        return Err(AppError::Forbidden);
    }

    // A reply has to point at a live message of the same channel; otherwise the
    // preview renders a message the reader cannot open.
    if let Some(reply_to) = body.reply_to_id {
        messages::find_by_id(&state.pool, id, reply_to)
            .await
            .map_err(|_| AppError::invisible("message"))?;
    }

    // Idempotency window of §6.5: the same nonce twice is one message, and the
    // second call answers 200 instead of 201. The claim is held across the
    // insert so a simultaneous second send waits rather than duplicating.
    let mut claim = None;
    if let Some(nonce) = body.nonce.as_deref() {
        match state.nonces.claim(id, nonce).await {
            Claim::Existing(existing) => {
                let row = messages::find_by_id(&state.pool, id, existing).await?;
                let mut message =
                    message_view::hydrate_one(&state.pool, &row, caller.id, &object_url(&state))
                        .await?;
                message.nonce = Some(nonce.to_owned());
                return Ok((StatusCode::OK, Json(message)).into_response());
            }
            Claim::Fresh(claimed) => claim = Some(claimed),
        }
    }

    let created = insert_message(&state, &access, caller.id, id, &body).await;
    let row = match created {
        Ok(row) => row,
        Err(err) => {
            if let Some(claimed) = claim {
                state.nonces.release(claimed).await;
            }
            return Err(err);
        }
    };
    if let Some(claimed) = claim {
        state.nonces.fulfil(claimed, row.id).await;
    }

    let message =
        message_view::hydrate_one(&state.pool, &row, caller.id, &object_url(&state)).await?;

    // The nonce goes to the originating session only (§5), so the broadcast
    // carries `null` and the author's own copy is reconciled by REST response.
    state
        .hub
        .publish_to_channel(
            &state.pool,
            id,
            DispatchEvent::MessageCreate(Box::new(message.clone())),
        )
        .await;

    let mut response = message;
    response.nonce = body.nonce.clone();
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

/// The write half of `create`, kept separate so a failure can release the nonce.
async fn insert_message(
    state: &AppState,
    access: &crate::permissions::ChannelAccess,
    author: Uuid,
    channel_id: Uuid,
    body: &CreateMessageRequest,
) -> Result<messages::MessageRow, AppError> {
    let mut mentions = domain::mentions::extract(&body.content);
    // `@everyone` without `MENTION_EVERYONE` is text, not a mention. Recording
    // it anyway would let anyone raise a badge on every member.
    if mentions.everyone && !access.permissions.contains(Permissions::MENTION_EVERYONE) {
        mentions.everyone = false;
    }

    let mut tx = state.pool.begin().await.map_err(db::DbError::from)?;
    let row = messages::insert(
        &mut *tx,
        Uuid::now_v7(),
        channel_id,
        author,
        body.content.trim_end(),
        body.reply_to_id,
    )
    .await?;

    for attachment in &body.attachments {
        engagement::insert_attachment(
            &mut *tx,
            NewAttachment {
                id: Uuid::now_v7(),
                message_id: row.id,
                r2_key: Some(&attachment.r2_key),
                skip_reason: None,
                filename: &attachment.filename,
                content_type: &attachment.content_type,
                size_bytes: attachment.size_bytes,
                width: attachment.width,
                height: attachment.height,
                source_url: None,
            },
        )
        .await?;
    }

    if !mentions.is_empty() {
        engagement::replace_mentions(&mut tx, row.id, &mentions).await?;
    }
    tx.commit().await.map_err(db::DbError::from)?;

    if !mentions.is_empty() {
        notify_mentions(state, channel_id, author, &mentions, access).await?;
    }
    Ok(row)
}

/// Bumps the mention counter of everyone reached, excluding the author, and
/// tells each of them over their own sessions.
async fn notify_mentions(
    state: &AppState,
    channel_id: Uuid,
    author: Uuid,
    mentions: &domain::Mentions,
    access: &crate::permissions::ChannelAccess,
) -> Result<(), AppError> {
    let viewers = state.hub.viewers(&state.pool, channel_id).await;
    let reached =
        engagement::mentioned_members(&state.pool, mentions, &viewers, access.channel.guild_id)
            .await?;
    for user_id in reached {
        if user_id == author {
            continue;
        }
        let state_row = engagement::increment_mentions(&state.pool, user_id, channel_id).await?;
        state
            .hub
            .publish_to_user(user_id, DispatchEvent::ReadStateUpdate(state_row))
            .await;
    }
    Ok(())
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn edit(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, mid)): Path<(Uuid, Uuid)>,
    Json(body): Json<EditMessageRequest>,
) -> Result<Json<Message>, AppError> {
    let access = channel_visible(&state, caller.id, id).await?;
    let existing = messages::find_by_id(&state.pool, id, mid).await?;
    // Editing is the author's alone: `MANAGE_MESSAGES` deletes, it does not
    // rewrite. Putting words in someone's mouth is a different power.
    if existing.author_id != caller.id {
        return Err(AppError::Forbidden);
    }

    let mut v = Validation::new();
    v.check("content", validation::message_body(&body.content, 0));
    v.finish()?;

    let row = messages::update_content(&state.pool, id, mid, body.content.trim_end()).await?;

    let mut mentions = domain::mentions::extract(&body.content);
    if mentions.everyone && !access.permissions.contains(Permissions::MENTION_EVERYONE) {
        mentions.everyone = false;
    }
    let mut conn = state.pool.acquire().await.map_err(db::DbError::from)?;
    engagement::replace_mentions(&mut conn, mid, &mentions).await?;
    drop(conn);

    let message =
        message_view::hydrate_one(&state.pool, &row, caller.id, &object_url(&state)).await?;
    state
        .hub
        .publish_to_channel(
            &state.pool,
            id,
            DispatchEvent::MessageUpdate(Box::new(message.clone())),
        )
        .await;
    Ok(Json(message))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn remove(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, mid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let access = channel_visible(&state, caller.id, id).await?;
    let existing = messages::find_by_id(&state.pool, id, mid).await?;
    if existing.author_id != caller.id && !access.permissions.contains(Permissions::MANAGE_MESSAGES)
    {
        return Err(AppError::Forbidden);
    }

    // Logical delete (RF-12): the row survives so cross-propagation and
    // idempotency keep working.
    if !messages::soft_delete(&state.pool, id, mid).await? {
        return Err(AppError::invisible("message"));
    }
    state
        .hub
        .publish_to_channel(
            &state.pool,
            id,
            DispatchEvent::MessageDelete(MessageDelete {
                id: mid,
                channel_id: id,
            }),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn pin(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, mid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Message>, AppError> {
    set_pinned(state, caller, id, mid, true).await
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn unpin(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, mid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Message>, AppError> {
    set_pinned(state, caller, id, mid, false).await
}

async fn set_pinned(
    state: AppState,
    caller: AuthUser,
    id: Uuid,
    mid: Uuid,
    pinned: bool,
) -> Result<Json<Message>, AppError> {
    require_channel(&state, caller.id, id, Permissions::MANAGE_MESSAGES).await?;
    let row = messages::set_pinned(&state.pool, id, mid, pinned).await?;
    let message =
        message_view::hydrate_one(&state.pool, &row, caller.id, &object_url(&state)).await?;
    state
        .hub
        .publish_to_channel(
            &state.pool,
            id,
            DispatchEvent::MessageUpdate(Box::new(message.clone())),
        )
        .await;
    Ok(Json(message))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn pins(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Message>>, AppError> {
    channel_visible(&state, caller.id, id).await?;
    let rows = messages::list_pinned(&state.pool, id).await?;
    Ok(Json(
        message_view::hydrate(&state.pool, &rows, caller.id, &object_url(&state)).await?,
    ))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn react(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, mid, emoji)): Path<(Uuid, Uuid, String)>,
) -> Result<StatusCode, AppError> {
    require_channel(&state, caller.id, id, Permissions::ADD_REACTIONS).await?;
    let emoji = decode_emoji(&emoji)?;
    messages::find_by_id(&state.pool, id, mid).await?;

    if engagement::add_reaction(&state.pool, mid, caller.id, &emoji).await? {
        state
            .hub
            .publish_to_channel(
                &state.pool,
                id,
                DispatchEvent::ReactionAdd(ReactionEvent {
                    message_id: mid,
                    channel_id: id,
                    user_id: caller.id,
                    emoji,
                }),
            )
            .await;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn unreact(
    State(state): State<AppState>,
    caller: AuthUser,
    Path((id, mid, emoji)): Path<(Uuid, Uuid, String)>,
) -> Result<StatusCode, AppError> {
    // Removing your own reaction needs visibility, not `ADD_REACTIONS`: losing
    // the permission must not strand a reaction you can no longer take back.
    channel_visible(&state, caller.id, id).await?;
    let emoji = decode_emoji(&emoji)?;

    if engagement::remove_reaction(&state.pool, mid, caller.id, &emoji).await? {
        state
            .hub
            .publish_to_channel(
                &state.pool,
                id,
                DispatchEvent::ReactionRemove(ReactionEvent {
                    message_id: mid,
                    channel_id: id,
                    user_id: caller.id,
                    emoji,
                }),
            )
            .await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The emoji arrives percent-encoded in the path. Custom emoji are out of scope
/// in v1 (RF-15), so anything longer than the column is refused.
fn decode_emoji(raw: &str) -> Result<String, AppError> {
    let decoded = percent_decode(raw);
    let mut v = Validation::new();
    v.check("emoji", validation::bounded(&decoded, 1, limits::EMOJI_MAX));
    v.finish()?;
    Ok(decoded)
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn typing(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require_channel(&state, caller.id, id, Permissions::SEND_MESSAGES).await?;
    // Never persisted, never replayed on resume (§5).
    state
        .hub
        .publish_to_channel(
            &state.pool,
            id,
            DispatchEvent::TypingStart(TypingStart {
                channel_id: id,
                user_id: caller.id,
                expires_at: Timestamp::new(
                    OffsetDateTime::now_utc() + Duration::seconds(TYPING_TTL_SECONDS),
                ),
            }),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn read_state(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateReadStateRequest>,
) -> Result<Json<ReadState>, AppError> {
    channel_visible(&state, caller.id, id).await?;
    // The marker has to point at a message of this channel, or the recount runs
    // against an id from somewhere else and clears the wrong badge.
    messages::find_by_id(&state.pool, id, body.last_read_message_id)
        .await
        .map_err(|_| AppError::invisible("message"))?;

    let updated =
        engagement::mark_read(&state.pool, caller.id, id, body.last_read_message_id).await?;
    // Only the user's own sessions: it is what keeps unreads coherent across
    // machines (§5).
    state
        .hub
        .publish_to_user(caller.id, DispatchEvent::ReadStateUpdate(updated))
        .await;
    Ok(Json(updated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_percent_encoded_emoji_round_trips() {
        assert_eq!(percent_decode("%F0%9F%91%8D"), "👍");
        assert_eq!(percent_decode("%E2%9D%A4"), "❤");
        assert_eq!(percent_decode("abc"), "abc");
    }

    #[test]
    fn a_truncated_percent_escape_is_left_alone_instead_of_panicking() {
        assert_eq!(percent_decode("%F"), "%F");
        assert_eq!(percent_decode("%"), "%");
        assert_eq!(percent_decode("%ZZ"), "%ZZ");
    }

    #[test]
    fn an_oversized_emoji_is_refused_before_it_reaches_the_column() {
        assert!(decode_emoji("👍").is_ok());
        assert!(decode_emoji("").is_err());
        assert!(decode_emoji(&"a".repeat(limits::EMOJI_MAX + 1)).is_err());
    }
}
