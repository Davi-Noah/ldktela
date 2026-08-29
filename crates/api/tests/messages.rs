//! Portão do E7: enviar o mesmo nonce duas vezes produz **uma** mensagem.

mod common;

use axum::http::StatusCode;
use common::{error_code, TestApp};
use domain::Permissions;
use serde_json::json;
use uuid::Uuid;

fn everyone_mask() -> i64 {
    (Permissions::VIEW_CHANNEL
        | Permissions::SEND_MESSAGES
        | Permissions::ADD_REACTIONS
        | Permissions::ATTACH_FILES)
        .bits()
}

struct Scene {
    app: TestApp,
    guild: Uuid,
    everyone_role: Uuid,
    channel: Uuid,
    owner_token: String,
    owner_id: Uuid,
    member_token: String,
    member_id: Uuid,
}

async fn scene() -> Scene {
    let app = TestApp::spawn().await;
    let owner = app.register("dono", "MSGOWNER1").await;
    let owner_id = app.user_id_by_username("dono").await;
    let (guild, everyone_role) = app.seed_guild(owner_id, everyone_mask()).await;
    let member = app.register_into("membro", "MSGMEMBR1", Some(guild)).await;
    let member_id = app.user_id_by_username("membro").await;
    let channel = app.seed_channel(guild, "geral").await;

    Scene {
        owner_token: owner["access_token"].as_str().unwrap().to_string(),
        member_token: member["access_token"].as_str().unwrap().to_string(),
        app,
        guild,
        everyone_role,
        channel,
        owner_id,
        member_id,
    }
}

#[tokio::test]
async fn the_same_nonce_twice_produces_exactly_one_message() {
    let s = scene().await;
    let body = json!({ "content": "olá", "nonce": "01J8XQNONCE" });

    let (first_status, first) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            body.clone(),
        )
        .await;
    assert_eq!(first_status, StatusCode::CREATED, "{first}");
    assert_eq!(first["nonce"], "01J8XQNONCE", "o nonce volta a quem enviou");

    // O reenvio depois de um timeout de rede: mesmo nonce, mesmo canal.
    let (second_status, second) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            body,
        )
        .await;
    assert_eq!(
        second_status,
        StatusCode::OK,
        "repetir o nonce responde 200, não 201: {second}"
    );
    assert_eq!(
        second["id"], first["id"],
        "o segundo envio precisa devolver a mensagem já criada"
    );

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE channel_id = $1")
        .bind(s.channel)
        .fetch_one(&s.app.pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "existe uma mensagem, não duas");
}

#[tokio::test]
async fn two_simultaneous_sends_of_one_nonce_still_produce_one_message() {
    let s = scene().await;
    let body = json!({ "content": "corrida", "nonce": "01J8XQRACE" });
    let path = format!("/channels/{}/messages", s.channel);

    let a = s.app.post_auth(&path, &s.member_token, body.clone());
    let b = s.app.post_auth(&path, &s.member_token, body);
    let (a, b) = tokio::join!(a, b);

    for (status, body) in [&a, &b] {
        assert!(
            status.is_success(),
            "os dois envios respondem sucesso: {body}"
        );
    }

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE channel_id = $1")
        .bind(s.channel)
        .fetch_one(&s.app.pool)
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "duas chamadas simultâneas com o mesmo nonce não podem criar duas mensagens"
    );
    assert_eq!(a.1["id"], b.1["id"]);
}

#[tokio::test]
async fn a_different_nonce_is_a_different_message() {
    let s = scene().await;
    for nonce in ["um", "dois"] {
        let (status, _) = s
            .app
            .post_auth(
                &format!("/channels/{}/messages", s.channel),
                &s.member_token,
                json!({ "content": "oi", "nonce": nonce }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE channel_id = $1")
        .bind(s.channel)
        .fetch_one(&s.app.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn a_message_without_content_or_attachment_is_refused() {
    let s = scene().await;
    let (status, body) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({ "content": "   " }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "VALIDATION_FAILED");
}

#[tokio::test]
async fn an_invisible_channel_answers_404_on_every_message_route() {
    // Metade do portão do E5 que ficou devendo: a regra de vazamento vale para
    // mensagem, não só para canal.
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

    // O dono escreve lá dentro.
    let (status, message) = s
        .app
        .post_auth(
            &format!("/channels/{private}/messages"),
            &s.owner_token,
            json!({ "content": "segredo" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{message}");
    let mid = message["id"].as_str().unwrap();

    // Para o membro, o canal e a mensagem não existem.
    let (status, body) = s
        .app
        .get(
            &format!("/channels/{private}/messages"),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "NOT_FOUND");

    for (method, path) in [
        ("GET", format!("/channels/{private}/pins")),
        ("DELETE", format!("/channels/{private}/messages/{mid}")),
    ] {
        let (status, _) = match method {
            "GET" => s.app.get(&path, Some(&s.member_token)).await,
            _ => s.app.delete(&path, &s.member_token).await,
        };
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }

    let (status, _) = s
        .app
        .post_auth(
            &format!("/channels/{private}/messages"),
            &s.member_token,
            json!({ "content": "invadindo" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // E o conteúdo nunca aparece numa listagem que ele possa fazer.
    let (status, page) = s
        .app
        .get(
            &format!("/channels/{}/messages", s.channel),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!page.to_string().contains("segredo"));
}

#[tokio::test]
async fn editing_belongs_to_the_author_and_deleting_also_to_a_moderator() {
    let s = scene().await;
    let (_, message) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({ "content": "original" }),
        )
        .await;
    let mid = message["id"].as_str().unwrap();

    // O dono tem MANAGE_MESSAGES, mas editar não é dele.
    let (status, _) = s
        .app
        .patch(
            &format!("/channels/{}/messages/{mid}", s.channel),
            &s.owner_token,
            json!({ "content": "reescrito por outro" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "MANAGE_MESSAGES apaga, não reescreve"
    );

    let (status, edited) = s
        .app
        .patch(
            &format!("/channels/{}/messages/{mid}", s.channel),
            &s.member_token,
            json!({ "content": "corrigido" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["content"], "corrigido");
    assert!(
        !edited["edited_at"].is_null(),
        "edited_at precisa ser marcado"
    );

    // Apagar, sim: o dono é moderador do canal.
    let (status, _) = s
        .app
        .delete(
            &format!("/channels/{}/messages/{mid}", s.channel),
            &s.owner_token,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Exclusão lógica: a linha continua no banco (RF-12).
    let deleted: bool =
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM messages WHERE id = $1::uuid")
            .bind(mid)
            .fetch_one(&s.app.pool)
            .await
            .unwrap();
    assert!(deleted);
}

#[tokio::test]
async fn mentions_are_extracted_on_the_server_and_raise_the_recipients_counter() {
    let s = scene().await;
    let content = format!("bom dia <@{}>", s.owner_id);

    let (status, message) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({ "content": content }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{message}");
    let mid = message["id"].as_str().unwrap();

    let mentioned: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mentions WHERE message_id = $1::uuid AND user_id = $2",
    )
    .bind(mid)
    .bind(s.owner_id)
    .fetch_one(&s.app.pool)
    .await
    .unwrap();
    assert_eq!(mentioned, 1);

    let count: i32 = sqlx::query_scalar(
        "SELECT mention_count FROM read_states WHERE user_id = $1 AND channel_id = $2",
    )
    .bind(s.owner_id)
    .bind(s.channel)
    .fetch_one(&s.app.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);

    // O autor não se auto-notifica.
    let author_state: Option<i32> = sqlx::query_scalar(
        "SELECT mention_count FROM read_states WHERE user_id = $1 AND channel_id = $2",
    )
    .bind(s.member_id)
    .bind(s.channel)
    .fetch_optional(&s.app.pool)
    .await
    .unwrap();
    assert!(author_state.is_none());

    // Marcar como lido zera o contador.
    let (status, read) = s
        .app
        .put(
            &format!("/channels/{}/read-state", s.channel),
            &s.owner_token,
            json!({ "last_read_message_id": mid }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["mention_count"], 0);
    assert_eq!(read["last_read_message_id"], mid);
}

#[tokio::test]
async fn a_client_supplied_mention_list_has_no_effect_because_there_is_none() {
    // O corpo não tem campo de menções por contrato (§6.5); mandar um é ignorado
    // pelo desserializador, e o que vale é o que o servidor extraiu do texto.
    let s = scene().await;
    let (status, message) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({
                "content": "sem mencionar ninguem",
                "mentions": [s.owner_id.to_string()],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{message}");
    let mid = message["id"].as_str().unwrap();

    let mentioned: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mentions WHERE message_id = $1::uuid")
            .bind(mid)
            .fetch_one(&s.app.pool)
            .await
            .unwrap();
    assert_eq!(mentioned, 0);
}

#[tokio::test]
async fn everyone_without_the_permission_is_text_not_a_mention() {
    let s = scene().await;
    let (status, message) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({ "content": "@everyone atenção" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{message}");
    let mid = message["id"].as_str().unwrap();

    let everyone: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mentions WHERE message_id = $1::uuid AND is_everyone",
    )
    .bind(mid)
    .fetch_one(&s.app.pool)
    .await
    .unwrap();
    assert_eq!(
        everyone, 0,
        "sem MENTION_EVERYONE, qualquer um levantaria badge em todo mundo"
    );
    assert_eq!(
        message["content"], "@everyone atenção",
        "o texto fica intacto"
    );
}

#[tokio::test]
async fn a_reaction_is_idempotent_and_removable() {
    let s = scene().await;
    let (_, message) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({ "content": "reaja" }),
        )
        .await;
    let mid = message["id"].as_str().unwrap();
    let path = format!(
        "/channels/{}/messages/{mid}/reactions/%F0%9F%91%8D/@me",
        s.channel
    );

    for _ in 0..3 {
        let (status, _) = s.app.put(&path, &s.owner_token, json!({})).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM reactions WHERE message_id = $1::uuid")
            .bind(mid)
            .fetch_one(&s.app.pool)
            .await
            .unwrap();
    assert_eq!(count, 1, "reagir três vezes é uma reação");

    // A agregação sai no objeto de mensagem.
    let (status, page) = s
        .app
        .get(
            &format!("/channels/{}/messages", s.channel),
            Some(&s.owner_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let reaction = &page["data"][0]["reactions"][0];
    assert_eq!(reaction["emoji"], "👍");
    assert_eq!(reaction["count"], 1);
    assert_eq!(reaction["me"], true);

    let (status, _) = s.app.delete(&path, &s.owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM reactions WHERE message_id = $1::uuid")
            .bind(mid)
            .fetch_one(&s.app.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn a_reply_carries_a_preview_and_survives_the_original_being_deleted() {
    let s = scene().await;
    let (_, original) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.owner_token,
            json!({ "content": "pergunta original" }),
        )
        .await;
    let original_id = original["id"].as_str().unwrap();

    let (status, reply) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({ "content": "resposta", "reply_to_id": original_id }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    assert_eq!(reply["reply_to"]["author_username"], "dono");
    assert_eq!(reply["reply_to"]["excerpt"], "pergunta original");

    s.app
        .delete(
            &format!("/channels/{}/messages/{original_id}", s.channel),
            &s.owner_token,
        )
        .await;

    let (_, page) = s
        .app
        .get(
            &format!("/channels/{}/messages", s.channel),
            Some(&s.member_token),
        )
        .await;
    let remaining = &page["data"][0];
    assert_eq!(remaining["content"], "resposta");
    assert_eq!(
        remaining["reply_to"]["excerpt"], "mensagem apagada",
        "o cabeçalho da resposta continua renderizando"
    );
}

#[tokio::test]
async fn a_reply_cannot_point_at_a_message_from_another_channel() {
    let s = scene().await;
    let other = s.app.seed_channel(s.guild, "outro").await;
    let (_, message) = s
        .app
        .post_auth(
            &format!("/channels/{other}/messages"),
            &s.member_token,
            json!({ "content": "lá" }),
        )
        .await;
    let mid = message["id"].as_str().unwrap();

    let (status, _) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({ "content": "aqui", "reply_to_id": mid }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn pagination_over_http_is_keyset_and_reports_has_more() {
    let s = scene().await;
    let mut ids = Vec::new();
    for i in 0..7 {
        let (_, message) = s
            .app
            .post_auth(
                &format!("/channels/{}/messages", s.channel),
                &s.member_token,
                json!({ "content": format!("m{i}") }),
            )
            .await;
        ids.push(message["id"].as_str().unwrap().to_string());
    }

    let (status, page) = s
        .app
        .get(
            &format!("/channels/{}/messages?limit=3", s.channel),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["has_more"], true);
    let first: Vec<&str> = page["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    assert_eq!(first, vec!["m6", "m5", "m4"], "mais recentes primeiro");

    let cursor = page["data"][2]["id"].as_str().unwrap();
    let (_, page) = s
        .app
        .get(
            &format!("/channels/{}/messages?before={cursor}&limit=3", s.channel),
            Some(&s.member_token),
        )
        .await;
    let second: Vec<&str> = page["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    assert_eq!(second, vec!["m3", "m2", "m1"]);

    // before e after juntos não fazem sentido e são recusados.
    let (status, body) = s
        .app
        .get(
            &format!(
                "/channels/{}/messages?before={cursor}&after={cursor}",
                s.channel
            ),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "VALIDATION_FAILED");
}

#[tokio::test]
async fn an_attachment_over_the_limit_is_refused_before_anything_is_written() {
    let s = scene().await;
    let (status, body) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({
                "content": "",
                "attachments": [{
                    "r2_key": "att/x/grande.webp",
                    "filename": "grande.webp",
                    "content_type": "image/webp",
                    "size_bytes": 26_214_401i64,
                }],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "VALIDATION_FAILED");

    let (status, body) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({
                "content": "",
                "attachments": [{
                    "r2_key": "att/x/programa.exe",
                    "filename": "programa.exe",
                    "content_type": "application/x-msdownload",
                    "size_bytes": 1024,
                }],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments")
        .fetch_one(&s.app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn pinning_needs_manage_messages_and_shows_up_in_pins() {
    let s = scene().await;
    let (_, message) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({ "content": "fixe isto" }),
        )
        .await;
    let mid = message["id"].as_str().unwrap();
    let path = format!("/channels/{}/messages/{mid}/pin", s.channel);

    let (status, _) = s.app.put(&path, &s.member_token, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, pinned) = s.app.put(&path, &s.owner_token, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{pinned}");
    assert_eq!(pinned["is_pinned"], true);

    let (status, pins) = s
        .app
        .get(
            &format!("/channels/{}/pins", s.channel),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pins.as_array().unwrap().len(), 1);

    let (status, _) = s.app.delete(&path, &s.owner_token).await;
    assert_eq!(status, StatusCode::OK);
    let (_, pins) = s
        .app
        .get(
            &format!("/channels/{}/pins", s.channel),
            Some(&s.member_token),
        )
        .await;
    assert!(pins.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn typing_answers_204_and_needs_send_messages() {
    let s = scene().await;
    let (status, _) = s
        .app
        .post_auth(
            &format!("/channels/{}/typing", s.channel),
            &s.member_token,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Sem SEND_MESSAGES no canal, o indicador é negado.
    s.app
        .set_overwrite(
            s.channel,
            "role",
            s.everyone_role,
            0,
            Permissions::SEND_MESSAGES.bits(),
        )
        .await;
    let (status, _) = s
        .app
        .post_auth(
            &format!("/channels/{}/typing", s.channel),
            &s.member_token,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_read_marker_from_another_channel_is_refused() {
    let s = scene().await;
    let other = s.app.seed_channel(s.guild, "outro").await;
    let (_, message) = s
        .app
        .post_auth(
            &format!("/channels/{other}/messages"),
            &s.member_token,
            json!({ "content": "lá" }),
        )
        .await;
    let mid = message["id"].as_str().unwrap();

    let (status, _) = s
        .app
        .put(
            &format!("/channels/{}/read-state", s.channel),
            &s.member_token,
            json!({ "last_read_message_id": mid }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "recontar menções contra um id de outro canal limparia o badge errado"
    );
}
