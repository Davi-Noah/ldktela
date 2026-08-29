//! Portão do E6, dois testes:
//!
//! 1. desconectar, gerar eventos, reconectar com `RESUME` e provar que nada
//!    faltou;
//! 2. provar que evento de canal privado não chega a quem não enxerga o canal.

mod common;

use common::gateway::RunningApp;
use domain::Permissions;
use serde_json::json;
use uuid::Uuid;

fn everyone_mask() -> i64 {
    (Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES).bits()
}

/// A guild with an owner, a member, and one channel each of open and private.
struct Scene {
    running: RunningApp,
    guild: Uuid,
    everyone_role: Uuid,
    owner_token: String,
    owner_id: Uuid,
    member_token: String,
    member_id: Uuid,
}

async fn scene() -> Scene {
    let running = RunningApp::spawn().await;
    let owner = running.app.register("dono", "GWOWNER01").await;
    let owner_id = running.app.user_id_by_username("dono").await;
    let (guild, everyone_role) = running.app.seed_guild(owner_id, everyone_mask()).await;
    let member = running
        .app
        .register_into("membro", "GWMEMBER1", Some(guild))
        .await;
    let member_id = running.app.user_id_by_username("membro").await;

    Scene {
        owner_token: owner["access_token"].as_str().unwrap().to_string(),
        member_token: member["access_token"].as_str().unwrap().to_string(),
        running,
        guild,
        everyone_role,
        owner_id,
        member_id,
    }
}

#[tokio::test]
async fn a_resume_replays_exactly_the_dispatches_missed_while_disconnected() {
    let s = scene().await;
    let mut client = s.running.connect().await;
    let ready = client.identify(&s.member_token).await;
    let session_id: Uuid = ready["d"]["session_id"].as_str().unwrap().parse().unwrap();
    let mut last_seq = ready["s"].as_u64().unwrap();
    assert_eq!(last_seq, 1, "READY é a sequência 1");

    // Consome o que chegar até aqui (o próprio PRESENCE_UPDATE da identificação).
    for frame in client.drain(300).await {
        last_seq = frame["s"].as_u64().unwrap_or(last_seq).max(last_seq);
    }

    // Cai a conexão, sem handshake de fechamento — como uma queda de rede.
    client.kill().await;

    // Três eventos acontecem enquanto o cliente está fora.
    for name in ["um", "dois", "tres"] {
        let (status, body) = s
            .running
            .app
            .post_auth(
                &format!("/guilds/{}/categories", s.guild),
                &s.owner_token,
                json!({ "name": name }),
            )
            .await;
        assert_eq!(status, axum::http::StatusCode::CREATED, "{body}");
    }

    // Reconecta e retoma a partir do último `s` conhecido.
    let mut resumed = s.running.connect().await;
    let first = resumed.resume(&s.member_token, session_id, last_seq).await;
    assert_ne!(
        first["op"], 6,
        "a sessão precisa continuar retomável dentro do TTL: {first}"
    );

    // O primeiro frame do replay é o mais antigo perdido, e as sequências são
    // contíguas a partir de last_seq + 1.
    let mut frames = vec![first];
    frames.extend(resumed.drain(500).await);

    let categories: Vec<&str> = frames
        .iter()
        .filter(|f| f["t"] == "CATEGORY_CREATE")
        .map(|f| f["d"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        categories,
        vec!["um", "dois", "tres"],
        "o replay precisa devolver os três eventos, na ordem original"
    );

    let seqs: Vec<u64> = frames.iter().filter_map(|f| f["s"].as_u64()).collect();
    for pair in seqs.windows(2) {
        assert_eq!(
            pair[1],
            pair[0] + 1,
            "as sequências do replay precisam ser contíguas: {seqs:?}"
        );
    }
    assert_eq!(
        seqs.first().copied(),
        Some(last_seq + 1),
        "o replay começa exatamente onde o cliente parou"
    );

    let resumed_frame = frames
        .iter()
        .find(|f| f["t"] == "RESUMED")
        .expect("o replay termina em RESUMED");
    assert!(
        resumed_frame["d"]["replayed"].as_u64().unwrap() >= 3,
        "RESUMED precisa contar o que foi reenviado"
    );
}

#[tokio::test]
async fn an_event_from_a_private_channel_never_reaches_who_cannot_see_it() {
    let s = scene().await;
    let private = s.running.app.seed_channel(s.guild, "privado").await;
    s.running
        .app
        .set_overwrite(
            private,
            "role",
            s.everyone_role,
            0,
            Permissions::VIEW_CHANNEL.bits(),
        )
        .await;

    let mut member = s.running.connect().await;
    let ready = member.identify(&s.member_token).await;
    let visible: Vec<&str> = ready["d"]["guilds"][0]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(
        visible.is_empty(),
        "READY só inclui canais com VIEW_CHANNEL: {visible:?}"
    );

    let mut owner = s.running.connect().await;
    owner.identify(&s.owner_token).await;
    member.drain(200).await;
    owner.drain(200).await;

    // O dono renomeia o canal privado. É um CHANNEL_UPDATE do canal invisível.
    let (status, body) = s
        .running
        .app
        .patch(
            &format!("/channels/{private}"),
            &s.owner_token,
            json!({ "name": "ainda-privado" }),
        )
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");

    let owner_frames = owner.drain(600).await;
    assert!(
        owner_frames.iter().any(|f| f["t"] == "CHANNEL_UPDATE"),
        "quem enxerga o canal precisa receber o evento"
    );

    let member_frames = member.drain(600).await;
    let leaked: Vec<&serde_json::Value> = member_frames
        .iter()
        .filter(|f| f["t"] == "CHANNEL_UPDATE")
        .collect();
    assert!(
        leaked.is_empty(),
        "o nome de um canal invisível vazou pelo fan-out: {leaked:?}"
    );
    assert!(
        !member_frames
            .iter()
            .any(|f| f.to_string().contains("ainda-privado")),
        "nenhum frame pode citar o canal invisível"
    );
}

#[tokio::test]
async fn granting_a_role_makes_a_private_channel_appear_and_invalidates_the_index() {
    let s = scene().await;
    let private = s.running.app.seed_channel(s.guild, "privado").await;
    s.running
        .app
        .set_overwrite(
            private,
            "role",
            s.everyone_role,
            0,
            Permissions::VIEW_CHANNEL.bits(),
        )
        .await;

    let mut member = s.running.connect().await;
    member.identify(&s.member_token).await;
    member.drain(200).await;

    // Um evento no canal privado não chega — o índice diz que ele não vê.
    s.running
        .app
        .patch(
            &format!("/channels/{private}"),
            &s.owner_token,
            json!({ "topic": "antes" }),
        )
        .await;
    assert!(
        !member
            .drain(400)
            .await
            .iter()
            .any(|f| f["t"] == "CHANNEL_UPDATE"),
        "ainda não deveria enxergar"
    );

    // O dono cria um cargo com allow de VIEW_CHANNEL e o atribui ao membro.
    let (_, role) = s
        .running
        .app
        .post_auth(
            &format!("/guilds/{}/roles", s.guild),
            &s.owner_token,
            json!({ "name": "convidados", "permissions": "0" }),
        )
        .await;
    let role_id = role["id"].as_str().unwrap();
    s.running
        .app
        .put(
            &format!("/channels/{private}/permissions/role/{role_id}"),
            &s.owner_token,
            json!({ "allow": Permissions::VIEW_CHANNEL.bits().to_string(), "deny": "0" }),
        )
        .await;
    s.running
        .app
        .patch(
            &format!("/guilds/{}/members/{}", s.guild, s.member_id),
            &s.owner_token,
            json!({ "roles": [role_id] }),
        )
        .await;

    // PERMISSIONS_STALE precisa ter chegado: é o que manda o cliente recarregar.
    let frames = member.drain(800).await;
    assert!(
        frames.iter().any(|f| f["t"] == "PERMISSIONS_STALE"),
        "alteração de cargo e de overwrite obriga PERMISSIONS_STALE: {frames:?}"
    );

    // E agora o evento do canal chega, porque o índice foi invalidado.
    s.running
        .app
        .patch(
            &format!("/channels/{private}"),
            &s.owner_token,
            json!({ "topic": "depois" }),
        )
        .await;
    let frames = member.drain(800).await;
    assert!(
        frames
            .iter()
            .any(|f| f["t"] == "CHANNEL_UPDATE" && f["d"]["topic"] == "depois"),
        "sem invalidação nos cinco gatilhos, o índice fica preso na visão antiga: {frames:?}"
    );
}

#[tokio::test]
async fn a_session_id_from_another_user_cannot_be_resumed() {
    let s = scene().await;
    let mut owner = s.running.connect().await;
    let ready = owner.identify(&s.owner_token).await;
    let owner_session: Uuid = ready["d"]["session_id"].as_str().unwrap().parse().unwrap();

    let mut attacker = s.running.connect().await;
    let response = attacker.resume(&s.member_token, owner_session, 0).await;
    assert_eq!(
        response["op"], 6,
        "retomar sessão alheia entregaria o histórico de eventos dela: {response}"
    );
}

#[tokio::test]
async fn a_resume_beyond_the_buffer_is_refused_instead_of_replaying_a_gap() {
    let s = scene().await;
    let mut client = s.running.connect().await;
    let ready = client.identify(&s.member_token).await;
    let session_id: Uuid = ready["d"]["session_id"].as_str().unwrap().parse().unwrap();
    client.kill().await;

    let mut resumed = s.running.connect().await;
    // O cliente afirma ter visto mais do que jamais foi enviado.
    let response = resumed.resume(&s.member_token, session_id, 9_999).await;
    assert_eq!(response["op"], 6);
    assert_eq!(response["d"]["resumable"], false);
}

#[tokio::test]
async fn heartbeats_are_acknowledged_and_an_unknown_opcode_closes_the_socket() {
    let s = scene().await;
    let mut client = s.running.connect().await;
    client.identify(&s.member_token).await;
    client.drain(200).await;

    client.send(json!({ "op": 4 })).await;
    let ack = client.recv().await;
    assert_eq!(ack["op"], 5, "HEARTBEAT precisa ser confirmado");

    // Opcode de servidor vindo do cliente é frame malformado.
    client.send(json!({ "op": 0, "t": "READY", "d": {} })).await;
    assert!(
        client.try_recv().await.is_none(),
        "o socket precisa fechar em frame inválido"
    );
}

#[tokio::test]
async fn an_unidentified_connection_is_closed_and_a_bad_token_never_gets_a_ready() {
    let s = scene().await;
    let mut client = s.running.connect().await;
    let hello = client.recv().await;
    assert_eq!(hello["op"], 1);
    assert_eq!(hello["d"]["heartbeat_interval_ms"], 30_000);
    assert_eq!(hello["d"]["session_ttl_ms"], 90_000);

    client
        .send(json!({
            "op": 2,
            "d": { "token": "nao-e-um-jwt", "client": { "version": "0.1.0", "os": "windows" } }
        }))
        .await;
    assert!(
        client.try_recv().await.is_none(),
        "token inválido fecha com 4001, sem READY"
    );
}

#[tokio::test]
async fn a_client_below_the_minimum_version_is_closed_before_identifying() {
    let s = scene().await;
    let mut client = s.running.connect().await;
    client.recv().await;
    client
        .send(json!({
            "op": 2,
            "d": { "token": s.member_token, "client": { "version": "0.0.1", "os": "windows" } }
        }))
        .await;
    assert!(client.try_recv().await.is_none());
}

#[tokio::test]
async fn presence_reaches_the_guild_and_invisible_is_reported_as_offline() {
    let s = scene().await;
    let mut owner = s.running.connect().await;
    owner.identify(&s.owner_token).await;
    owner.drain(200).await;

    let mut member = s.running.connect().await;
    member.identify(&s.member_token).await;

    // O dono vê o membro entrar.
    let frames = owner.drain(600).await;
    let presence = frames
        .iter()
        .find(|f| f["t"] == "PRESENCE_UPDATE" && f["d"]["user_id"] == s.member_id.to_string())
        .expect("PRESENCE_UPDATE do membro");
    assert_eq!(presence["d"]["status"], "online");

    // O membro se declara invisível.
    let (status, body) = s
        .running
        .app
        .patch(
            "/users/@me/presence",
            &s.member_token,
            json!({ "status": "invisible" }),
        )
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");
    assert_eq!(body["status"], "invisible", "para si mesmo, invisible");

    let frames = owner.drain(600).await;
    let presence = frames
        .iter()
        .find(|f| f["t"] == "PRESENCE_UPDATE" && f["d"]["user_id"] == s.member_id.to_string())
        .expect("PRESENCE_UPDATE do membro");
    assert_eq!(
        presence["d"]["status"], "offline",
        "PRESENCE_UPDATE nunca revela invisible a terceiros"
    );
    let _ = s.owner_id;
}

#[tokio::test]
async fn every_session_of_the_same_user_receives_the_same_events() {
    let s = scene().await;
    let mut laptop = s.running.connect().await;
    let mut desktop = s.running.connect().await;
    laptop.identify(&s.member_token).await;
    desktop.identify(&s.member_token).await;
    laptop.drain(200).await;
    desktop.drain(200).await;

    s.running
        .app
        .post_auth(
            &format!("/guilds/{}/categories", s.guild),
            &s.owner_token,
            json!({ "name": "duas-telas" }),
        )
        .await;

    for (name, client) in [("laptop", &mut laptop), ("desktop", &mut desktop)] {
        let frames = client.drain(600).await;
        assert!(
            frames
                .iter()
                .any(|f| f["t"] == "CATEGORY_CREATE" && f["d"]["name"] == "duas-telas"),
            "{name} não recebeu o evento; é isto que sincroniza as máquinas"
        );
    }
}
