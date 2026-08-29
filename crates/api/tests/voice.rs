//! Portão do E11, dois testes:
//!
//! 1. webhook sem assinatura válida é recusado e não muda estado;
//! 2. o quarto publicador de câmera recebe **409 VOICE_CAPACITY**.

mod common;

use axum::http::StatusCode;
use common::{error_code, TestApp};
use domain::Permissions;
use livekit_api::access_token::AccessToken;
use serde_json::json;
use uuid::Uuid;

/// A chave e o segredo do fixture, iguais aos de `common::test_config`.
const API_KEY: &str = "devkey";
const API_SECRET: &str = "dev-only-not-a-real-key-0123456789abcdef";

fn everyone_mask() -> i64 {
    (Permissions::VIEW_CHANNEL
        | Permissions::SEND_MESSAGES
        | Permissions::CONNECT_VOICE
        | Permissions::SPEAK
        | Permissions::VIDEO)
        .bits()
}

struct Scene {
    app: TestApp,
    guild: Uuid,
    everyone_role: Uuid,
    voice_channel: Uuid,
    text_channel: Uuid,
    owner_token: String,
    owner_id: Uuid,
}

async fn scene() -> Scene {
    let app = TestApp::spawn().await;
    let owner = app.register("dono", "VOZOWNER1").await;
    let owner_id = app.user_id_by_username("dono").await;
    let (guild, everyone_role) = app.seed_guild(owner_id, everyone_mask()).await;

    let voice_channel = Uuid::now_v7();
    sqlx::query("INSERT INTO channels (id, guild_id, name, type) VALUES ($1, $2, 'sala', 'voice')")
        .bind(voice_channel)
        .bind(guild)
        .execute(&app.pool)
        .await
        .expect("seeding voice channel");
    let text_channel = app.seed_channel(guild, "geral").await;

    Scene {
        owner_token: owner["access_token"].as_str().unwrap().to_string(),
        app,
        guild,
        everyone_role,
        voice_channel,
        text_channel,
        owner_id,
    }
}

/// Registers a member who can join voice, returning `(id, token)`.
async fn member(s: &Scene, name: &str, code: &str) -> (Uuid, String) {
    let session = s.app.register_into(name, code, Some(s.guild)).await;
    (
        s.app.user_id_by_username(name).await,
        session["access_token"].as_str().unwrap().to_string(),
    )
}

/// Signs a webhook body the way LiveKit does: a JWT whose `sha256` claim is the
/// base64 digest of the body.
fn sign_webhook(body: &str, secret: &str) -> String {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(body.as_bytes());
    let encoded = base64::engine::general_purpose::STANDARD.encode(digest);
    AccessToken::with_api_key(API_KEY, secret)
        .with_sha256(&encoded)
        .with_ttl(std::time::Duration::from_secs(300))
        .to_jwt()
        .expect("signing webhook")
}

#[tokio::test]
async fn a_webhook_without_a_valid_signature_is_refused_and_changes_nothing() {
    let s = scene().await;
    let (member_id, _) = member(&s, "membro", "VOZMEMBR1").await;
    let body = json!({
        "event": "participant_joined",
        "room": { "name": format!("channel-{}", s.voice_channel) },
        "participant": { "identity": member_id.to_string() },
    })
    .to_string();

    // 1. Sem cabeçalho de autorização.
    let status = s.app.post_webhook(&body, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 2. Com um cabeçalho que não é um token.
    let status = s.app.post_webhook(&body, Some("nao-e-um-jwt")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 3. Assinado com outro segredo.
    let forged = sign_webhook(&body, "outro-segredo-completamente-diferente");
    let status = s.app.post_webhook(&body, Some(&forged)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 4. Assinatura válida, mas para outro corpo — o clássico replay.
    let other_body = json!({ "event": "room_finished" }).to_string();
    let mismatched = sign_webhook(&other_body, API_SECRET);
    let status = s.app.post_webhook(&body, Some(&mismatched)).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a assinatura cobre o corpo; trocar o corpo tem que invalidar"
    );

    // Nenhum estado de voz foi criado por nenhuma delas.
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_states")
        .fetch_one(&s.app.pool)
        .await
        .unwrap();
    assert_eq!(
        rows, 0,
        "o webhook é a única fonte de VOICE_STATE_UPDATE; aceitar corpo não assinado \
         deixaria qualquer um colocar qualquer pessoa em qualquer sala"
    );

    // E com a assinatura correta o estado aparece.
    let valid = sign_webhook(&body, API_SECRET);
    let status = s.app.post_webhook(&body, Some(&valid)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_states")
        .fetch_one(&s.app.pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn the_fourth_camera_publisher_is_refused_with_voice_capacity() {
    let s = scene().await;
    let mut publishers = vec![(s.owner_id, s.owner_token.clone())];
    for i in 0..3 {
        publishers.push(member(&s, &format!("cam{i}"), &format!("VOZCAM{i}001")).await);
    }

    // Os três primeiros ganham câmera.
    for (_, token) in publishers.iter().take(3) {
        let (status, body) = s
            .app
            .post_auth(
                &format!("/channels/{}/voice-token", s.voice_channel),
                token,
                json!({ "publish_camera": true }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["room"], format!("channel-{}", s.voice_channel));
        assert_eq!(body["expires_in"], 3600);
    }

    // O quarto, não.
    let (status, body) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.voice_channel),
            &publishers[3].1,
            json!({ "publish_camera": true }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "VOICE_CAPACITY");

    // Mas ele entra como ouvinte sem problema: a sala não está cheia de gente,
    // está cheia de câmera.
    let (status, body) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.voice_channel),
            &publishers[3].1,
            json!({ "publish_camera": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // E quando um dos três desliga a câmera, a vaga volta.
    let (status, _) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.voice_channel),
            &publishers[0].1,
            json!({ "publish_camera": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.voice_channel),
            &publishers[3].1,
            json!({ "publish_camera": true }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_camera_slot_is_released_when_the_participant_leaves() {
    let s = scene().await;
    let mut ids = vec![s.owner_id];
    let mut tokens = vec![s.owner_token.clone()];
    for i in 0..3 {
        let (id, token) = member(&s, &format!("sai{i}"), &format!("VOZSAI{i}001")).await;
        ids.push(id);
        tokens.push(token);
    }
    for token in tokens.iter().take(3) {
        s.app
            .post_auth(
                &format!("/channels/{}/voice-token", s.voice_channel),
                token,
                json!({ "publish_camera": true }),
            )
            .await;
    }

    let leave = json!({
        "event": "participant_left",
        "room": { "name": format!("channel-{}", s.voice_channel) },
        "participant": { "identity": ids[0].to_string() },
    })
    .to_string();
    let status = s
        .app
        .post_webhook(&leave, Some(&sign_webhook(&leave, API_SECRET)))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.voice_channel),
            &tokens[3],
            json!({ "publish_camera": true }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "quem saiu da sala liberou a vaga: {body}"
    );
}

#[tokio::test]
async fn the_token_is_scoped_to_the_room_and_lasts_at_most_an_hour() {
    let s = scene().await;
    let (status, body) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.voice_channel),
            &s.owner_token,
            json!({ "publish_camera": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["expires_in"].as_i64().unwrap() <= 3600, "RNF-07");

    let jwt = body["token"].as_str().unwrap();
    let claims = livekit_api::access_token::Claims::from_unverified(jwt).unwrap();
    assert_eq!(claims.video.room, format!("channel-{}", s.voice_channel));
    assert!(claims.video.room_join);
    assert!(!claims.video.room_admin);
    assert!(!claims.video.room_create);
    assert_eq!(claims.sub, s.owner_id.to_string());
}

#[tokio::test]
async fn connect_voice_is_required_and_an_invisible_channel_answers_404() {
    let s = scene().await;
    let (_, member_token) = member(&s, "semvoz", "VOZSEMV01").await;

    // Sem CONNECT_VOICE: 403, porque o canal é visível.
    s.app
        .set_overwrite(
            s.voice_channel,
            "role",
            s.everyone_role,
            0,
            Permissions::CONNECT_VOICE.bits(),
        )
        .await;
    let (status, _) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.voice_channel),
            &member_token,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Canal invisível: 404.
    s.app
        .set_overwrite(
            s.voice_channel,
            "role",
            s.everyone_role,
            0,
            (Permissions::CONNECT_VOICE | Permissions::VIEW_CHANNEL).bits(),
        )
        .await;
    let (status, body) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.voice_channel),
            &member_token,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "NOT_FOUND");
}

#[tokio::test]
async fn a_text_channel_has_no_voice_token() {
    let s = scene().await;
    let (status, body) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.text_channel),
            &s.owner_token,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn requesting_a_camera_without_the_video_permission_is_forbidden() {
    let s = scene().await;
    let (_, member_token) = member(&s, "semvideo", "VOZSEMVD1").await;
    s.app
        .set_overwrite(
            s.voice_channel,
            "role",
            s.everyone_role,
            0,
            Permissions::VIDEO.bits(),
        )
        .await;

    let (status, _) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.voice_channel),
            &member_token,
            json!({ "publish_camera": true }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Sem câmera, entra normalmente.
    let (status, _) = s
        .app
        .post_auth(
            &format!("/channels/{}/voice-token", s.voice_channel),
            &member_token,
            json!({ "publish_camera": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn joining_and_leaving_move_the_voice_state_and_screen_share_flips_streaming() {
    let s = scene().await;
    let (member_id, _) = member(&s, "membro", "VOZMEMBR2").await;
    let room = format!("channel-{}", s.voice_channel);

    let send = |event: serde_json::Value| {
        let body = event.to_string();
        let signature = sign_webhook(&body, API_SECRET);
        (body, signature)
    };

    let (body, sig) = send(json!({
        "event": "participant_joined",
        "room": { "name": room },
        "participant": { "identity": member_id.to_string() },
    }));
    assert_eq!(
        s.app.post_webhook(&body, Some(&sig)).await,
        StatusCode::NO_CONTENT
    );
    let channel: Uuid =
        sqlx::query_scalar("SELECT channel_id FROM voice_states WHERE user_id = $1")
            .bind(member_id)
            .fetch_one(&s.app.pool)
            .await
            .unwrap();
    assert_eq!(channel, s.voice_channel);

    // Compartilhar tela liga `streaming`; parar desliga.
    for (event, expected) in [("track_published", true), ("track_unpublished", false)] {
        let (body, sig) = send(json!({
            "event": event,
            "room": { "name": room },
            "participant": { "identity": member_id.to_string() },
            "track": { "source": 3 },
        }));
        assert_eq!(
            s.app.post_webhook(&body, Some(&sig)).await,
            StatusCode::NO_CONTENT
        );
        let streaming: bool =
            sqlx::query_scalar("SELECT streaming FROM voice_states WHERE user_id = $1")
                .bind(member_id)
                .fetch_one(&s.app.pool)
                .await
                .unwrap();
        assert_eq!(streaming, expected, "após {event}");
    }

    let (body, sig) = send(json!({
        "event": "participant_left",
        "room": { "name": room },
        "participant": { "identity": member_id.to_string() },
    }));
    assert_eq!(
        s.app.post_webhook(&body, Some(&sig)).await,
        StatusCode::NO_CONTENT
    );
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_states")
        .fetch_one(&s.app.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn a_webhook_for_a_room_that_is_not_ours_is_accepted_and_ignored() {
    let s = scene().await;
    let body = json!({
        "event": "participant_joined",
        "room": { "name": "sala-de-outro-sistema" },
        "participant": { "identity": s.owner_id.to_string() },
    })
    .to_string();
    let status = s
        .app
        .post_webhook(&body, Some(&sign_webhook(&body, API_SECRET)))
        .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "assinado e válido, só não é nosso: recusar faria o LiveKit reenviar para sempre"
    );
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_states")
        .fetch_one(&s.app.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn the_client_owns_mute_and_deaf_and_nothing_else() {
    let s = scene().await;
    let room = format!("channel-{}", s.voice_channel);
    let body = json!({
        "event": "participant_joined",
        "room": { "name": room },
        "participant": { "identity": s.owner_id.to_string() },
    })
    .to_string();
    s.app
        .post_webhook(&body, Some(&sign_webhook(&body, API_SECRET)))
        .await;

    let (status, state) = s
        .app
        .patch(
            "/voice-states/@me",
            &s.owner_token,
            json!({ "self_mute": true }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert_eq!(state["self_mute"], true);
    assert_eq!(state["self_deaf"], false, "campo ausente não altera");
    assert_eq!(
        state["streaming"], false,
        "streaming vem do webhook, não do cliente"
    );

    // Fora de um canal de voz não há estado para mudar.
    let (_, other_token) = member(&s, "forasala", "VOZFORA01").await;
    let (status, _) = s
        .app
        .patch(
            "/voice-states/@me",
            &other_token,
            json!({ "self_mute": true }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ready_carries_the_voice_state_of_people_already_in_a_room() {
    let s = scene().await;
    let (member_id, _) = member(&s, "jaesta", "VOZJAEST1").await;
    let body = json!({
        "event": "participant_joined",
        "room": { "name": format!("channel-{}", s.voice_channel) },
        "participant": { "identity": member_id.to_string() },
    })
    .to_string();
    s.app
        .post_webhook(&body, Some(&sign_webhook(&body, API_SECRET)))
        .await;

    // RF-20: quem nem está conectado à sala precisa ver quem está.
    let ready = api::gateway::ready::build(&s.app.state, s.owner_id, Uuid::now_v7()).await;
    assert_eq!(ready.voice_states.len(), 1);
    assert_eq!(ready.voice_states[0].user_id, member_id);
    assert_eq!(ready.voice_states[0].channel_id, Some(s.voice_channel));
}
