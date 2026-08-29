//! Portão do E5: quem não tem `VIEW_CHANNEL` recebe **404**, nunca 403.
//!
//! A regra de vazamento do `docs/api/rest-api.md` §3 existe porque um 403
//! confirma que o canal existe e quem está nele — o suficiente para mapear a
//! estrutura de um servidor privado.

mod common;

use axum::http::StatusCode;
use common::{error_code, TestApp};
use domain::Permissions;
use serde_json::json;
use uuid::Uuid;

/// Everything a member needs to take part, and nothing that moderates.
fn everyone_mask() -> i64 {
    (Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES).bits()
}

struct Scene {
    app: TestApp,
    guild: Uuid,
    everyone_role: Uuid,
    owner_token: String,
    owner_id: Uuid,
    member_token: String,
    member_id: Uuid,
    outsider_token: String,
}

/// A guild with an owner, a plain member and a user who is in no guild at all.
async fn scene() -> Scene {
    let app = TestApp::spawn().await;

    let owner_session = app.register("dono", "SEEDOWNER").await;
    let owner_id = app.user_id_by_username("dono").await;
    let (guild, everyone_role) = app.seed_guild(owner_id, everyone_mask()).await;

    let member_session = app.register_into("membro", "SEEDMEMBR", Some(guild)).await;
    let member_id = app.user_id_by_username("membro").await;
    let outsider_session = app.register("forasteiro", "SEEDOUT").await;

    Scene {
        owner_token: owner_session["access_token"].as_str().unwrap().to_string(),
        member_token: member_session["access_token"].as_str().unwrap().to_string(),
        outsider_token: outsider_session["access_token"]
            .as_str()
            .unwrap()
            .to_string(),
        app,
        guild,
        everyone_role,
        owner_id,
        member_id,
    }
}

#[tokio::test]
async fn a_channel_denied_by_overwrite_answers_404_not_403() {
    let s = scene().await;
    let open = s.app.seed_channel(s.guild, "geral").await;
    let private = s.app.seed_channel(s.guild, "privado").await;
    s.app
        .set_overwrite(
            private,
            "role",
            s.everyone_role,
            0,
            Permissions::VIEW_CHANNEL.bits(),
        )
        .await;

    // O canal aberto o membro enxerga; o privado, não — e a diferença entre os
    // dois não pode aparecer no status.
    let (status, _) = s
        .app
        .patch(&format!("/channels/{open}"), &s.member_token, json!({}))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "canal visível, ação negada: 403"
    );

    let (status, body) = s
        .app
        .patch(&format!("/channels/{private}"), &s.member_token, json!({}))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "canal invisível precisa ser indistinguível de inexistente"
    );
    assert_eq!(error_code(&body), "NOT_FOUND");

    // E um id que não existe responde igual ao canal invisível.
    let (status, other) = s
        .app
        .patch(
            &format!("/channels/{}", Uuid::now_v7()),
            &s.member_token,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["message"], other["error"]["message"]);
}

#[tokio::test]
async fn deleting_and_overwriting_an_invisible_channel_answer_404_too() {
    let s = scene().await;
    let private = s.app.seed_channel(s.guild, "privado").await;
    s.app
        .set_overwrite(
            private,
            "role",
            s.everyone_role,
            0,
            Permissions::VIEW_CHANNEL.bits(),
        )
        .await;

    let (status, _) = s
        .app
        .delete(&format!("/channels/{private}"), &s.member_token)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = s
        .app
        .put(
            &format!("/channels/{private}/permissions/role/{}", s.everyone_role),
            &s.member_token,
            json!({ "allow": "0", "deny": "0" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = s
        .app
        .delete(
            &format!("/channels/{private}/permissions/role/{}", s.everyone_role),
            &s.member_token,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_guild_the_caller_does_not_belong_to_answers_404() {
    let s = scene().await;
    s.app.seed_channel(s.guild, "geral").await;

    let (status, body) = s
        .app
        .get(&format!("/guilds/{}", s.guild), Some(&s.outsider_token))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "NOT_FOUND");

    let (status, _) = s
        .app
        .get(
            &format!("/guilds/{}/members", s.guild),
            Some(&s.outsider_token),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a lista de membros de um guild alheio não pode confirmar que ele existe"
    );
}

#[tokio::test]
async fn a_member_with_no_visible_channel_cannot_see_the_guild_either() {
    // O contrato condiciona GET /guilds/{id} a VIEW_CHANNEL em ao menos um canal.
    let s = scene().await;
    let only = s.app.seed_channel(s.guild, "privado").await;
    s.app
        .set_overwrite(
            only,
            "role",
            s.everyone_role,
            0,
            Permissions::VIEW_CHANNEL.bits(),
        )
        .await;

    let (status, _) = s
        .app
        .get(&format!("/guilds/{}", s.guild), Some(&s.member_token))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // O dono continua enxergando: o passo 1 do §5.3 ignora overwrites.
    let (status, body) = s
        .app
        .get(&format!("/guilds/{}", s.guild), Some(&s.owner_token))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["channels"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_guild_view_lists_only_the_channels_the_caller_can_see() {
    let s = scene().await;
    s.app.seed_channel(s.guild, "geral").await;
    let private = s.app.seed_channel(s.guild, "privado").await;
    s.app
        .set_overwrite(
            private,
            "role",
            s.everyone_role,
            0,
            Permissions::VIEW_CHANNEL.bits(),
        )
        .await;

    let (status, body) = s
        .app
        .get(&format!("/guilds/{}", s.guild), Some(&s.member_token))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let names: Vec<&str> = body["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["geral"], "o canal privado não pode ser listado");

    // A máscara resolvida vai junto, como string decimal.
    let mask = body["channels"][0]["permissions"].as_str().unwrap();
    assert_eq!(mask, everyone_mask().to_string());
}

#[tokio::test]
async fn permission_masks_travel_as_decimal_strings_in_both_directions() {
    let s = scene().await;
    let channel = s.app.seed_channel(s.guild, "geral").await;

    // Number em vez de string precisa ser recusado: é o bug clássico da
    // categoria, e aceitar silenciosamente perde precisão acima de 2^53.
    let (status, _) = s
        .app
        .put(
            &format!("/channels/{channel}/permissions/role/{}", s.everyone_role),
            &s.owner_token,
            json!({ "allow": 256, "deny": 0 }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let allow = Permissions::SEND_MESSAGES.bits().to_string();
    let (status, body) = s
        .app
        .put(
            &format!("/channels/{channel}/permissions/role/{}", s.everyone_role),
            &s.owner_token,
            json!({ "allow": allow, "deny": "0" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allow"], allow);
    assert!(body["allow"].is_string());
}

#[tokio::test]
async fn a_role_may_not_grant_a_permission_its_creator_lacks() {
    let s = scene().await;
    s.app.seed_channel(s.guild, "geral").await;

    // O membro ganha MANAGE_ROLES e nada mais.
    let (status, role) = s
        .app
        .post_auth(
            &format!("/guilds/{}/roles", s.guild),
            &s.owner_token,
            json!({
                "name": "moderacao",
                "permissions": Permissions::MANAGE_ROLES.bits().to_string(),
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{role}");
    let role_id = role["id"].as_str().unwrap();

    let (status, _) = s
        .app
        .patch(
            &format!("/guilds/{}/members/{}", s.guild, s.member_id),
            &s.owner_token,
            json!({ "roles": [role_id] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Com MANAGE_ROLES, ele não pode fabricar um cargo ADMINISTRATOR.
    let (status, body) = s
        .app
        .post_auth(
            &format!("/guilds/{}/roles", s.guild),
            &s.member_token,
            json!({
                "name": "escalada",
                "permissions": Permissions::ADMINISTRATOR.bits().to_string(),
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "sem esse guard, MANAGE_ROLES é ADMINISTRATOR: {body}"
    );

    // Mas pode criar um cargo com o que ele próprio tem.
    let (status, _) = s
        .app
        .post_auth(
            &format!("/guilds/{}/roles", s.guild),
            &s.member_token,
            json!({
                "name": "ajudante",
                "permissions": Permissions::MANAGE_ROLES.bits().to_string(),
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn the_everyone_role_cannot_be_deleted() {
    let s = scene().await;
    s.app.seed_channel(s.guild, "geral").await;
    let (status, _) = s
        .app
        .delete(
            &format!("/guilds/{}/roles/{}", s.guild, s.everyone_role),
            &s.owner_token,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "apagar @everyone deixaria o guild sem máscara base"
    );
}

#[tokio::test]
async fn a_member_may_rename_themselves_but_not_someone_else() {
    let s = scene().await;
    s.app.seed_channel(s.guild, "geral").await;

    let (status, body) = s
        .app
        .patch(
            &format!("/guilds/{}/members/{}", s.guild, s.member_id),
            &s.member_token,
            json!({ "nickname": "Membrinho" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["nickname"], "Membrinho");

    let (status, _) = s
        .app
        .patch(
            &format!("/guilds/{}/members/{}", s.guild, s.owner_id),
            &s.member_token,
            json!({ "nickname": "Dono Falso" }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // E não pode se promover.
    let (status, _) = s
        .app
        .patch(
            &format!("/guilds/{}/members/{}", s.guild, s.member_id),
            &s.member_token,
            json!({ "roles": [] }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_owner_can_never_be_kicked_or_banned() {
    let s = scene().await;
    s.app.seed_channel(s.guild, "geral").await;
    let (status, _) = s
        .app
        .delete(
            &format!("/guilds/{}/members/{}", s.guild, s.owner_id),
            &s.owner_token,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = s
        .app
        .put(
            &format!("/guilds/{}/bans/{}", s.guild, s.owner_id),
            &s.owner_token,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn banning_a_member_removes_their_access_immediately() {
    let s = scene().await;
    s.app.seed_channel(s.guild, "geral").await;

    let (status, _) = s
        .app
        .get(&format!("/guilds/{}", s.guild), Some(&s.member_token))
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = s
        .app
        .put(
            &format!("/guilds/{}/bans/{}", s.guild, s.member_id),
            &s.owner_token,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = s
        .app
        .get(&format!("/guilds/{}", s.guild), Some(&s.member_token))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "o token continua válido; a permissão é recalculada na consulta"
    );
}

#[tokio::test]
async fn a_direct_channel_cannot_be_created_through_the_guild_route() {
    let s = scene().await;
    let (status, body) = s
        .app
        .post_auth(
            &format!("/guilds/{}/channels", s.guild),
            &s.owner_token,
            json!({ "name": "conversa", "type": "dm" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "VALIDATION_FAILED");
}

#[tokio::test]
async fn channel_reordering_is_all_or_nothing() {
    let s = scene().await;
    let a = s.app.seed_channel(s.guild, "a").await;
    let b = s.app.seed_channel(s.guild, "b").await;

    // Um id que não pertence ao guild derruba o lote inteiro.
    let (status, _) = s
        .app
        .patch(
            &format!("/guilds/{}/channels/positions", s.guild),
            &s.owner_token,
            json!({ "positions": [
                { "id": a, "position": 5 },
                { "id": Uuid::now_v7(), "position": 6 },
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let position: i32 = sqlx::query_scalar("SELECT position FROM channels WHERE id = $1")
        .bind(a)
        .fetch_one(&s.app.pool)
        .await
        .unwrap();
    assert_eq!(position, 0, "a transação inteira precisa ter revertido");

    let (status, _) = s
        .app
        .patch(
            &format!("/guilds/{}/channels/positions", s.guild),
            &s.owner_token,
            json!({ "positions": [
                { "id": a, "position": 5 },
                { "id": b, "position": 6 },
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn an_invite_binds_the_new_account_to_its_guild() {
    let s = scene().await;
    s.app.seed_channel(s.guild, "geral").await;

    let (status, invite) = s
        .app
        .post_auth(
            "/invites",
            &s.owner_token,
            json!({ "guild_id": s.guild, "max_uses": 1 }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{invite}");
    let code = invite["code"].as_str().unwrap();

    let (status, preview) = s.app.get(&format!("/invites/{code}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["valid"], true);
    assert_eq!(preview["guild_name"], "guild");

    let (status, session) = s
        .app
        .post(
            "/auth/register",
            json!({
                "invite_code": code,
                "email": "novato@exemplo.test",
                "username": "novato",
                "password": "senha-de-teste-123",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{session}");
    let token = session["access_token"].as_str().unwrap();

    let (status, guilds) = s.app.get("/guilds", Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        guilds.as_array().unwrap().len(),
        1,
        "o convite precisa colocar a conta nova dentro do guild"
    );
}

#[tokio::test]
async fn an_unknown_invite_code_looks_the_same_as_an_expired_one() {
    let app = TestApp::spawn().await;
    let (status, unknown) = app.get("/invites/NAOEXISTE", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unknown["valid"], false);
    assert!(unknown["guild_name"].is_null());

    app.seed_invite("GASTADO", 1, None).await;
    sqlx::query("UPDATE invites SET revoked_at = NOW() WHERE code = 'GASTADO'")
        .execute(&app.pool)
        .await
        .unwrap();
    let (status, revoked) = app.get("/invites/GASTADO", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(revoked["valid"], false);
    assert!(revoked["guild_name"].is_null());
}

#[tokio::test]
async fn creating_an_invite_requires_create_invite_in_that_guild() {
    let s = scene().await;
    s.app.seed_channel(s.guild, "geral").await;

    let (status, _) = s
        .app
        .post_auth("/invites", &s.member_token, json!({ "guild_id": s.guild }))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Um guild em que o solicitante nem entra some do mapa.
    let (status, _) = s
        .app
        .post_auth(
            "/invites",
            &s.outsider_token,
            json!({ "guild_id": s.guild }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_overwrite_cannot_allow_and_deny_the_same_bit() {
    let s = scene().await;
    let channel = s.app.seed_channel(s.guild, "geral").await;
    let both = Permissions::SEND_MESSAGES.bits().to_string();
    let (status, body) = s
        .app
        .put(
            &format!("/channels/{channel}/permissions/role/{}", s.everyone_role),
            &s.owner_token,
            json!({ "allow": both, "deny": both }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "VALIDATION_FAILED");
}
