//! Portão do E8: com um servidor S3 falso, um arquivo acima do limite é
//! recusado **antes** de qualquer URL ser assinada.

mod common;

use axum::http::StatusCode;
use common::{error_code, TestApp};
use domain::Permissions;
use serde_json::json;
use uuid::Uuid;

fn everyone_mask() -> i64 {
    (Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES | Permissions::ATTACH_FILES).bits()
}

struct Scene {
    app: TestApp,
    guild: Uuid,
    everyone_role: Uuid,
    channel: Uuid,
    owner_token: String,
    member_token: String,
}

async fn scene() -> Scene {
    let app = TestApp::spawn().await;
    let owner = app.register("dono", "ATTOWNER1").await;
    let owner_id = app.user_id_by_username("dono").await;
    let (guild, everyone_role) = app.seed_guild(owner_id, everyone_mask()).await;
    let member = app.register_into("membro", "ATTMEMBR1", Some(guild)).await;
    let channel = app.seed_channel(guild, "geral").await;

    Scene {
        owner_token: owner["access_token"].as_str().unwrap().to_string(),
        member_token: member["access_token"].as_str().unwrap().to_string(),
        app,
        guild,
        everyone_role,
        channel,
    }
}

#[tokio::test]
async fn a_file_over_the_limit_is_refused_before_a_url_is_signed() {
    let s = scene().await;
    let (status, body) = s
        .app
        .post_auth(
            "/attachments/presign",
            &s.member_token,
            json!({
                "channel_id": s.channel,
                "filename": "video.mp4",
                "content_type": "video/mp4",
                "size_bytes": 26_214_401i64,
            }),
        )
        .await;

    assert_eq!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "25 MB é o limite do RF-11a: {body}"
    );
    assert_eq!(error_code(&body), "PAYLOAD_TOO_LARGE");
    assert!(
        body["error"]["details"].is_null() || body.get("upload_url").is_none(),
        "nenhuma URL pode ser devolvida junto com a recusa"
    );
    assert!(
        body.get("upload_url").is_none() && body.get("r2_key").is_none(),
        "assinar e depois recusar entregaria uma URL válida por todo o TTL: {body}"
    );

    // E nada foi escrito no armazenamento.
    assert_eq!(s.app.s3.len(), 0);
}

#[tokio::test]
async fn a_disallowed_content_type_is_refused_before_a_url_is_signed() {
    let s = scene().await;
    let (status, body) = s
        .app
        .post_auth(
            "/attachments/presign",
            &s.member_token,
            json!({
                "channel_id": s.channel,
                "filename": "programa.exe",
                "content_type": "application/x-msdownload",
                "size_bytes": 1024,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "VALIDATION_FAILED");
    assert_eq!(body["error"]["details"][0]["field"], "content_type");
    assert!(body.get("upload_url").is_none());
}

#[tokio::test]
async fn a_valid_request_gets_a_key_and_a_signed_url() {
    let s = scene().await;
    let (status, body) = s
        .app
        .post_auth(
            "/attachments/presign",
            &s.member_token,
            json!({
                "channel_id": s.channel,
                "filename": "captura.webp",
                "content_type": "image/webp",
                "size_bytes": 40_213,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let key = body["r2_key"].as_str().unwrap();
    assert!(key.starts_with("att/"), "{key}");
    assert!(key.ends_with("/captura.webp"), "{key}");
    assert_eq!(body["expires_in"], 300);

    let url = body["upload_url"].as_str().unwrap();
    assert!(url.contains(key), "a URL precisa apontar para a chave");
    assert!(
        url.contains("X-Amz-Signature"),
        "a URL precisa estar assinada: {url}"
    );
    assert!(
        url.contains("X-Amz-Expires=300"),
        "o TTL da assinatura é o configurado: {url}"
    );
}

#[tokio::test]
async fn a_filename_cannot_escape_the_prefix_through_the_key() {
    let s = scene().await;
    let (status, body) = s
        .app
        .post_auth(
            "/attachments/presign",
            &s.member_token,
            json!({
                "channel_id": s.channel,
                "filename": "../../../etc/passwd",
                "content_type": "image/png",
                "size_bytes": 10,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let key = body["r2_key"].as_str().unwrap();
    assert!(!key.contains(".."), "{key}");
    assert!(key.starts_with("att/"), "{key}");
    assert!(key.ends_with("/passwd"), "{key}");
}

#[tokio::test]
async fn presigning_needs_attach_files_and_an_invisible_channel_answers_404() {
    // A metade do portão do E5 que faltava: a regra de vazamento vale para anexo.
    let s = scene().await;

    // Sem ATTACH_FILES: 403, porque o canal é visível.
    s.app
        .set_overwrite(
            s.channel,
            "role",
            s.everyone_role,
            0,
            Permissions::ATTACH_FILES.bits(),
        )
        .await;
    let (status, _) = s
        .app
        .post_auth(
            "/attachments/presign",
            &s.member_token,
            json!({
                "channel_id": s.channel,
                "filename": "x.webp",
                "content_type": "image/webp",
                "size_bytes": 10,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Canal invisível: 404, indistinguível de inexistente.
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
        .post_auth(
            "/attachments/presign",
            &s.member_token,
            json!({
                "channel_id": private,
                "filename": "x.webp",
                "content_type": "image/webp",
                "size_bytes": 10,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "NOT_FOUND");

    let (status, unknown) = s
        .app
        .post_auth(
            "/attachments/presign",
            &s.member_token,
            json!({
                "channel_id": Uuid::now_v7(),
                "filename": "x.webp",
                "content_type": "image/webp",
                "size_bytes": 10,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["message"], unknown["error"]["message"]);
}

#[tokio::test]
async fn a_message_referencing_an_object_that_was_never_uploaded_is_refused() {
    let s = scene().await;
    let (_, presign) = s
        .app
        .post_auth(
            "/attachments/presign",
            &s.member_token,
            json!({
                "channel_id": s.channel,
                "filename": "captura.webp",
                "content_type": "image/webp",
                "size_bytes": 40_213,
            }),
        )
        .await;
    let key = presign["r2_key"].as_str().unwrap();

    // O cliente assinou mas nunca fez o PUT.
    let (status, body) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({
                "content": "",
                "attachments": [{
                    "r2_key": key,
                    "filename": "captura.webp",
                    "content_type": "image/webp",
                    "size_bytes": 40_213,
                }],
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "sem o HEAD, a mensagem renderiza uma imagem quebrada para sempre: {body}"
    );

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments")
        .fetch_one(&s.app.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn a_message_referencing_an_uploaded_object_persists_the_attachment() {
    let s = scene().await;
    let (_, presign) = s
        .app
        .post_auth(
            "/attachments/presign",
            &s.member_token,
            json!({
                "channel_id": s.channel,
                "filename": "captura.webp",
                "content_type": "image/webp",
                "size_bytes": 40_213,
            }),
        )
        .await;
    let key = presign["r2_key"].as_str().unwrap().to_string();

    // O PUT do cliente, simulado escrevendo direto no falso.
    s.app.s3.insert(&key, 40_213, 0);

    let (status, message) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({
                "content": "",
                "attachments": [{
                    "r2_key": key,
                    "filename": "captura.webp",
                    "content_type": "image/webp",
                    "size_bytes": 40_213,
                    "width": 1280,
                    "height": 720,
                }],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{message}");

    let attachment = &message["attachments"][0];
    assert_eq!(attachment["filename"], "captura.webp");
    // Dimensões persistidas para reservar espaço e evitar reflow (RNF-04).
    assert_eq!(attachment["width"], 1280);
    assert_eq!(attachment["height"], 720);
    assert!(
        attachment["url"]
            .as_str()
            .unwrap()
            .ends_with(&format!("/{key}")),
        "a URL pública precisa apontar para a chave: {attachment}"
    );
    assert!(attachment["skip_reason"].is_null());
}

#[tokio::test]
async fn an_unmigrated_attachment_renders_as_a_placeholder_instead_of_a_broken_link() {
    // RF-25a: `r2_key` nulo com `skip_reason` preenchido. É o que a migração do
    // E14 grava para vídeo fora do orçamento, e a renderização não pode quebrar.
    let s = scene().await;
    let (_, message) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.member_token,
            json!({ "content": "com anexo antigo" }),
        )
        .await;
    let mid = message["id"].as_str().unwrap();

    sqlx::query(
        "INSERT INTO attachments \
           (id, message_id, r2_key, skip_reason, filename, content_type, size_bytes) \
         VALUES ($1, $2::uuid, NULL, 'video_fora_do_orcamento', 'aula.mp4', 'video/mp4', 900000)",
    )
    .bind(Uuid::now_v7())
    .bind(mid)
    .execute(&s.app.pool)
    .await
    .unwrap();

    let (status, page) = s
        .app
        .get(
            &format!("/channels/{}/messages", s.channel),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let attachment = &page["data"][0]["attachments"][0];
    assert!(attachment["url"].is_null(), "sem URL para o que não migrou");
    assert_eq!(attachment["skip_reason"], "video_fora_do_orcamento");
    assert_eq!(attachment["filename"], "aula.mp4");
    assert_eq!(attachment["size_bytes"], 900_000);
}

#[tokio::test]
async fn the_orphan_sweep_deletes_only_unreferenced_objects_past_the_grace_period() {
    let s = scene().await;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let day = 24 * 60 * 60;

    // 1. Órfão antigo: ninguém referencia, passou da carência.
    s.app
        .s3
        .insert("att/orfao-antigo/x.webp", 10, now - day - 60);
    // 2. Órfão recente: ninguém referencia, mas ainda está dentro da carência —
    //    o usuário pode estar escrevendo a mensagem agora.
    s.app.s3.insert("att/orfao-novo/x.webp", 10, now - 60);
    // 3. Referenciado e antigo: precisa sobreviver.
    let referenced = "att/referenciado/x.webp";
    s.app.s3.insert(referenced, 10, now - day - 60);

    let (_, message) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.owner_token,
            json!({
                "content": "",
                "attachments": [{
                    "r2_key": referenced,
                    "filename": "x.webp",
                    "content_type": "image/webp",
                    "size_bytes": 10,
                }],
            }),
        )
        .await;
    assert_eq!(message["attachments"].as_array().unwrap().len(), 1);

    let removed = api::jobs::collect_orphans(&s.app.state).await.unwrap();
    assert_eq!(removed, 1, "só o órfão antigo sai");
    assert!(!s.app.s3.contains("att/orfao-antigo/x.webp"));
    assert!(
        s.app.s3.contains("att/orfao-novo/x.webp"),
        "um objeto recém-enviado ainda não tem mensagem: apagá-lo é uma corrida"
    );
    assert!(
        s.app.s3.contains(referenced),
        "apagar um objeto referenciado quebraria a mensagem"
    );
}

#[tokio::test]
async fn deleting_a_message_leaves_its_object_for_the_sweep_to_collect() {
    let s = scene().await;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let key = "att/sera-apagado/x.webp";
    s.app.s3.insert(key, 10, now - 25 * 60 * 60);

    let (_, message) = s
        .app
        .post_auth(
            &format!("/channels/{}/messages", s.channel),
            &s.owner_token,
            json!({
                "content": "",
                "attachments": [{
                    "r2_key": key,
                    "filename": "x.webp",
                    "content_type": "image/webp",
                    "size_bytes": 10,
                }],
            }),
        )
        .await;
    let mid = message["id"].as_str().unwrap();

    // Exclusão de mensagem é lógica: a linha de anexo continua, então o objeto
    // segue referenciado. É deliberado — a propagação cruzada da ponte depende
    // da linha, e o anexo volta se a mensagem for restaurada.
    s.app
        .delete(
            &format!("/channels/{}/messages/{mid}", s.channel),
            &s.owner_token,
        )
        .await;
    assert_eq!(api::jobs::collect_orphans(&s.app.state).await.unwrap(), 0);
    assert!(s.app.s3.contains(key));

    // Já a remoção física do canal leva a mensagem e o anexo em cascata, e aí o
    // objeto vira órfão de verdade.
    s.app
        .delete(&format!("/channels/{}", s.channel), &s.owner_token)
        .await;
    assert_eq!(api::jobs::collect_orphans(&s.app.state).await.unwrap(), 1);
    assert!(!s.app.s3.contains(key));
}
