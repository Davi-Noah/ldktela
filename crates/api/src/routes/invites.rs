//! `/invites` (`docs/api/rest-api.md` §6.2, RF-02).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use db::repo::{guilds, invites};
use domain::validation::{self, limits, Validation};
use domain::Permissions;
use protocol::guild::{CreateInviteRequest, Invite, InvitePreview};
use rand::Rng;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Path};
use crate::middleware::auth::AuthUser;
use crate::permissions::require_guild;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/invites", post(create).get(list))
        .route("/invites/{code}", get(preview).delete(revoke))
}

/// Unambiguous alphabet: no `O`/`0`, no `I`/`1`. Invite codes get read aloud.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 10;

fn generate_code() -> String {
    let mut rng = rand::rng();
    (0..CODE_LEN)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn create(
    State(state): State<AppState>,
    caller: AuthUser,
    Json(body): Json<CreateInviteRequest>,
) -> Result<(StatusCode, Json<Invite>), AppError> {
    // The contract leaves the scope of a guild-less invite undefined; requiring
    // one keeps `CREATE_INVITE` checkable and gives registration a guild to join.
    let guild_id = body.guild_id.ok_or_else(|| {
        AppError::Validation(vec![protocol::error::FieldError {
            field: "guild_id".into(),
            code: "REQUIRED".into(),
        }])
    })?;
    require_guild(&state, caller.id, guild_id, Permissions::CREATE_INVITE).await?;

    let max_uses = body.max_uses.unwrap_or(1);
    if !(1..=100).contains(&max_uses) {
        return Err(AppError::Validation(vec![protocol::error::FieldError {
            field: "max_uses".into(),
            code: "OUT_OF_RANGE".into(),
        }]));
    }
    let expires_at = body
        .expires_in
        .map(|seconds| OffsetDateTime::now_utc() + Duration::seconds(seconds));

    let invite = invites::insert(
        &state.pool,
        Uuid::now_v7(),
        &generate_code(),
        caller.id,
        Some(guild_id),
        max_uses,
        expires_at,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(invite.to_wire())))
}

/// Invites of every guild where the caller has `MANAGE_GUILD`.
#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn list(
    State(state): State<AppState>,
    caller: AuthUser,
) -> Result<Json<Vec<Invite>>, AppError> {
    let mut managed = Vec::new();
    for guild in guilds::list_for_user(&state.pool, caller.id).await? {
        if require_guild(&state, caller.id, guild.id, Permissions::MANAGE_GUILD)
            .await
            .is_ok()
        {
            managed.push(guild.id);
        }
    }
    let all = invites::list(&state.pool).await?;
    Ok(Json(
        all.iter()
            .filter(|i| i.guild_id.is_some_and(|g| managed.contains(&g)))
            .map(|i| i.to_wire())
            .collect(),
    ))
}

/// Public: pre-validates a code during sign-up. Carries only validity and the
/// guild name, so an unauthenticated caller learns nothing else.
#[tracing::instrument(skip(state))]
async fn preview(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> Result<Json<InvitePreview>, AppError> {
    let mut v = Validation::new();
    v.check(
        "code",
        validation::bounded(&code, 1, limits::INVITE_CODE_MAX),
    );
    v.finish()?;

    let Some(invite) = invites::find_by_code(&state.pool, &code).await? else {
        // An unknown code and an expired one look the same on purpose.
        return Ok(Json(InvitePreview {
            code,
            valid: false,
            guild_name: None,
            expires_at: None,
        }));
    };
    let valid = invite.is_valid(OffsetDateTime::now_utc());
    let guild_name = match (valid, invite.guild_id) {
        (true, Some(guild_id)) => guilds::find_by_id(&state.pool, guild_id)
            .await
            .ok()
            .map(|g| g.name),
        _ => None,
    };
    Ok(Json(InvitePreview {
        code: invite.code.clone(),
        valid,
        guild_name,
        expires_at: invite.to_wire().expires_at,
    }))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn revoke(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(code): Path<String>,
) -> Result<StatusCode, AppError> {
    let Some(invite) = invites::find_by_code(&state.pool, &code).await? else {
        return Err(AppError::invisible("invite"));
    };
    let Some(guild_id) = invite.guild_id else {
        return Err(AppError::invisible("invite"));
    };
    require_guild(&state, caller.id, guild_id, Permissions::MANAGE_GUILD).await?;
    invites::revoke(&state.pool, &code).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn generated_codes_fit_the_column_and_avoid_ambiguous_glyphs() {
        let code = generate_code();
        assert_eq!(code.len(), CODE_LEN);
        assert!(code.len() <= limits::INVITE_CODE_MAX);
        assert!(
            !code.contains(['O', '0', 'I', '1']),
            "convites são lidos em voz alta: {code}"
        );
    }

    #[test]
    fn generated_codes_do_not_repeat_in_practice() {
        let codes: HashSet<String> = (0..2000).map(|_| generate_code()).collect();
        assert_eq!(
            codes.len(),
            2000,
            "colisão em 2000 códigos de 10 caracteres"
        );
    }
}
