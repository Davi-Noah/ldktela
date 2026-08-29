//! Portão do E9: quem não participa recebe **404** e não aparece no fan-out.
//!
//! O SRS §9 (F4a) diz 403 nesse aceite, mas o `docs/api/rest-api.md` §3 manda
//! 404 para todo recurso invisível, com a justificativa de não confirmar
//! existência. O conflito está registrado em `docs/DECISIONS.md`; aqui vale a
//! regra de vazamento, que é a norma mais específica.

mod common;

use axum::http::StatusCode;
use common::error_code;
use common::gateway::RunningApp;
use serde_json::json;
use uuid::Uuid;

struct Scene {
    running: RunningApp,
    ana: Uuid,
    ana_token: String,
    bruno: Uuid,
    bruno_token: String,
    carla: Uuid,
    carla_token: String,
}

async fn scene() -> Scene {
    let running = RunningApp::spawn().await;
    // Os três precisam compartilhar um guild para se enxergarem na aplicação;
    // a conversa direta em si não depende disso.
    let ana = running.app.register("ana", "DMANA0001").await;
    let ana_id = running.app.user_id_by_username("ana").await;
    let (guild, _) = running.app.seed_guild(ana_id, 0).await;
    let bruno = running
        .app
        .register_into("bruno", "DMBRUNO01", Some(guild))
        .await;
    let carla = running
        .app
        .register_into("carla", "DMCARLA01", Some(guild))
        .await;

    Scene {
        ana_token: ana["access_token"].as_str().unwrap().to_string(),
        bruno_token: bruno["access_token"].as_str().unwrap().to_string(),
        carla_token: carla["access_token"].as_str().unwrap().to_string(),
        ana: ana_id,
        bruno: running.app.user_id_by_username("bruno").await,
        carla: running.app.user_id_by_username("carla").await,
        running,
    }
}

#[tokio::test]
async fn a_non_participant_gets_404_and_never_appears_in_the_fan_out() {
    let s = scene().await;

    // Carla conecta no gateway antes de a conversa existir.
    let mut carla_socket = s.running.connect().await;
    carla_socket.identify(&s.carla_token).await;
    carla_socket.drain(200).await;

    let mut bruno_socket = s.running.connect().await;
    bruno_socket.identify(&s.bruno_token).await;
    bruno_socket.drain(200).await;

    // Ana abre uma conversa com Bruno.
    let (status, channel) = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": [s.bruno] }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{channel}");
    let dm = channel["id"].as_str().unwrap();

    // Bruno é avisado; Carla não.
    let bruno_frames = bruno_socket.drain(600).await;
    assert!(
        bruno_frames.iter().any(|f| f["t"] == "DM_CHANNEL_CREATE"),
        "o participante precisa saber que a conversa existe"
    );
    let carla_frames = carla_socket.drain(600).await;
    assert!(
        !carla_frames.iter().any(|f| f["t"] == "DM_CHANNEL_CREATE"),
        "a existência de uma conversa alheia não pode vazar: {carla_frames:?}"
    );

    // Ana escreve. Bruno recebe a mensagem; Carla não recebe nada.
    let (status, message) = s
        .running
        .app
        .post_auth(
            &format!("/channels/{dm}/messages"),
            &s.ana_token,
            json!({ "content": "assunto privado" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{message}");

    let bruno_frames = bruno_socket.drain(600).await;
    assert!(
        bruno_frames
            .iter()
            .any(|f| f["t"] == "MESSAGE_CREATE" && f["d"]["content"] == "assunto privado"),
        "o participante precisa receber a mensagem"
    );
    let carla_frames = carla_socket.drain(600).await;
    assert!(
        !carla_frames
            .iter()
            .any(|f| f.to_string().contains("assunto privado")),
        "nenhum frame pode citar o conteúdo: {carla_frames:?}"
    );

    // E pelo REST, para Carla a conversa não existe.
    let (status, body) = s
        .running
        .app
        .get(&format!("/channels/{dm}/messages"), Some(&s.carla_token))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "NOT_FOUND");

    let (status, unknown) = s
        .running
        .app
        .get(
            &format!("/channels/{}/messages", Uuid::now_v7()),
            Some(&s.carla_token),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body["error"]["message"], unknown["error"]["message"],
        "invisível e inexistente têm que ser indistinguíveis"
    );

    // Nem escrever, nem listar entre as próprias conversas.
    let (status, _) = s
        .running
        .app
        .post_auth(
            &format!("/channels/{dm}/messages"),
            &s.carla_token,
            json!({ "content": "invadindo" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, dms) = s.running.app.get("/dms", Some(&s.carla_token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(dms.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn opening_the_same_pair_twice_resolves_to_one_channel() {
    let s = scene().await;
    let (first_status, first) = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": [s.bruno] }))
        .await;
    assert_eq!(first_status, StatusCode::CREATED);

    // A segunda tentativa, do outro lado do par, resolve o canal existente.
    let (second_status, second) = s
        .running
        .app
        .post_auth("/dms", &s.bruno_token, json!({ "recipient_ids": [s.ana] }))
        .await;
    assert_eq!(
        second_status,
        StatusCode::OK,
        "resolver não é criar: {second}"
    );
    assert_eq!(second["id"], first["id"]);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE type = 'dm'")
        .fetch_one(&s.running.app.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn two_simultaneous_opens_of_one_pair_still_produce_one_channel() {
    let s = scene().await;
    let a = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": [s.bruno] }));
    let b = s
        .running
        .app
        .post_auth("/dms", &s.bruno_token, json!({ "recipient_ids": [s.ana] }));
    let (a, b) = tokio::join!(a, b);
    assert!(a.0.is_success() && b.0.is_success(), "{a:?} {b:?}");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE type = 'dm'")
        .fetch_one(&s.running.app.pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "dois cliques simultâneos são uma conversa");
    assert_eq!(a.1["id"], b.1["id"]);
}

#[tokio::test]
async fn a_group_containing_the_pair_is_not_mistaken_for_their_one_to_one() {
    let s = scene().await;
    // Grupo com os três.
    let (status, group) = s
        .running
        .app
        .post_auth(
            "/dms",
            &s.ana_token,
            json!({ "recipient_ids": [s.bruno, s.carla] }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{group}");
    assert_eq!(group["type"], "group_dm");

    // Abrir o 1:1 entre Ana e Bruno cria um canal novo, não devolve o grupo.
    let (status, pair) = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": [s.bruno] }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{pair}");
    assert_ne!(pair["id"], group["id"]);
    assert_eq!(pair["type"], "dm");
}

#[tokio::test]
async fn a_group_stops_at_ten_participants() {
    let s = scene().await;
    // Ana mais nove convidados são dez; o décimo convidado estoura.
    let mut recipients = vec![s.bruno, s.carla];
    for i in 0..8 {
        let name = format!("extra{i}");
        s.running.app.register(&name, &format!("DMEXTRA{i}0")).await;
        recipients.push(s.running.app.user_id_by_username(&name).await);
    }
    assert_eq!(recipients.len(), 10);

    let (status, body) = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": recipients }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "CONFLICT");

    // Nove convidados mais a criadora cabem.
    recipients.pop();
    let (status, body) = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": recipients }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["participants"].as_array().unwrap().len(), 10);
}

#[tokio::test]
async fn leaving_ends_access_and_leaves_the_messages_for_the_others() {
    let s = scene().await;
    let (_, group) = s
        .running
        .app
        .post_auth(
            "/dms",
            &s.ana_token,
            json!({ "recipient_ids": [s.bruno, s.carla] }),
        )
        .await;
    let dm = group["id"].as_str().unwrap();

    s.running
        .app
        .post_auth(
            &format!("/channels/{dm}/messages"),
            &s.carla_token,
            json!({ "content": "tchau" }),
        )
        .await;

    // Carla sai por conta própria.
    let (status, _) = s
        .running
        .app
        .delete(
            &format!("/dms/{dm}/participants/{}", s.carla),
            &s.carla_token,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Para ela a conversa sumiu.
    let (status, _) = s
        .running
        .app
        .get(&format!("/channels/{dm}/messages"), Some(&s.carla_token))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Para os outros, as mensagens dela continuam lá (RF-18b).
    let (status, page) = s
        .running
        .app
        .get(&format!("/channels/{dm}/messages"), Some(&s.ana_token))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["data"][0]["content"], "tchau");
    assert_eq!(page["data"][0]["author"]["username"], "carla");
}

#[tokio::test]
async fn only_the_creator_adds_and_removes_other_people() {
    let s = scene().await;
    let (_, group) = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": [s.bruno] }))
        .await;
    // Um par vira grupo? Não: adicionar em `dm` é conflito, não silêncio.
    let dm = group["id"].as_str().unwrap();
    let (status, _) = s
        .running
        .app
        .post_auth(
            &format!("/dms/{dm}/participants"),
            &s.ana_token,
            json!({ "user_id": s.carla }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (_, group) = s
        .running
        .app
        .post_auth(
            "/dms",
            &s.ana_token,
            json!({ "recipient_ids": [s.bruno, s.carla] }),
        )
        .await;
    let gid = group["id"].as_str().unwrap();

    // Bruno não é o criador: não adiciona nem remove terceiros.
    s.running.app.register("davi", "DMDAVI001").await;
    let davi = s.running.app.user_id_by_username("davi").await;
    let (status, _) = s
        .running
        .app
        .post_auth(
            &format!("/dms/{gid}/participants"),
            &s.bruno_token,
            json!({ "user_id": davi }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = s
        .running
        .app
        .delete(
            &format!("/dms/{gid}/participants/{}", s.carla),
            &s.bruno_token,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Mas sair sozinho, sim.
    let (status, _) = s
        .running
        .app
        .delete(
            &format!("/dms/{gid}/participants/{}", s.bruno),
            &s.bruno_token,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_direct_conversation_can_never_be_bridged() {
    let s = scene().await;
    let (_, channel) = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": [s.bruno] }))
        .await;
    let dm = channel["id"].as_str().unwrap();
    assert_eq!(channel["bridge_enabled"], false);

    // RF-18a é estrutural: a CHECK do banco recusa mesmo por escrita direta.
    let forced = sqlx::query("UPDATE channels SET bridge_enabled = TRUE WHERE id = $1::uuid")
        .bind(dm)
        .execute(&s.running.app.pool)
        .await;
    assert!(
        forced.is_err(),
        "chk_bridge_scope é a segunda barreira e precisa recusar"
    );
}

#[tokio::test]
async fn a_direct_channel_grants_exactly_the_step_zero_mask() {
    let s = scene().await;
    let (_, channel) = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": [s.bruno] }))
        .await;

    let expected = domain::Permissions::DIRECT_MESSAGE.bits().to_string();
    assert_eq!(
        channel["permissions"], expected,
        "o passo 0 do §5.3 concede um conjunto fixo, nem mais nem menos"
    );
    // Sem moderação: cargos e overwrites não se aplicam.
    let mask: i64 = channel["permissions"].as_str().unwrap().parse().unwrap();
    let mask = domain::Permissions::from_bits_truncate(mask);
    assert!(!mask.contains(domain::Permissions::MANAGE_MESSAGES));
    assert!(!mask.contains(domain::Permissions::MENTION_EVERYONE));
    assert!(mask.contains(domain::Permissions::SCREEN_SHARE));
}

#[tokio::test]
async fn a_ghost_user_cannot_be_a_recipient() {
    let s = scene().await;
    sqlx::query(
        "INSERT INTO users (id, username, discord_user_id, is_migrated) \
         VALUES ($1, 'fantasma', 99, TRUE)",
    )
    .bind(Uuid::now_v7())
    .execute(&s.running.app.pool)
    .await
    .unwrap();
    let ghost: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = 'fantasma'")
        .fetch_one(&s.running.app.pool)
        .await
        .unwrap();

    let (status, _) = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": [ghost] }))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "um ghost user não tem sessão nem como ler a conversa"
    );
    let _ = s.ana;
}

#[tokio::test]
async fn a_conversation_with_only_yourself_is_refused() {
    let s = scene().await;
    let (status, body) = s
        .running
        .app
        .post_auth("/dms", &s.ana_token, json!({ "recipient_ids": [s.ana] }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "VALIDATION_FAILED");
}
