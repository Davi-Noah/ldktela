//! Portão do E4: apresentar um refresh token já consumido revoga a família
//! inteira (RF-01a).

mod common;

use axum::http::StatusCode;
use common::{error_code, TestApp};
use serde_json::json;

#[tokio::test]
async fn reusing_a_consumed_refresh_token_revokes_the_entire_family() {
    let app = TestApp::spawn().await;
    let session = app.register("gabriel", "CONVITE1").await;
    let first = session["refresh_token"].as_str().unwrap().to_string();

    // Rotação normal: o primeiro token é consumido, um segundo é emitido.
    let (status, body) = app
        .post("/auth/refresh", json!({ "refresh_token": first }))
        .await;
    assert_eq!(status, StatusCode::OK);
    let second = body["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(first, second, "a rotação precisa emitir um token novo");

    // Um terceiro giro, para provar que a família tem mais de dois membros
    // vivos quando o reúso for detectado.
    let (status, body) = app
        .post("/auth/refresh", json!({ "refresh_token": second }))
        .await;
    assert_eq!(status, StatusCode::OK);
    let third = body["refresh_token"].as_str().unwrap().to_string();

    // Agora o reúso: o primeiro token já foi consumido.
    let (status, body) = app
        .post("/auth/refresh", json!({ "refresh_token": first }))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "TOKEN_REUSED");

    // A família inteira caiu: nem o token vivo mais recente funciona.
    let (status, body) = app
        .post("/auth/refresh", json!({ "refresh_token": third }))
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "o token vivo da família deveria ter sido revogado"
    );
    assert_eq!(error_code(&body), "UNAUTHENTICATED");

    // E o estado no banco confirma: nenhum token da família continua utilizável.
    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM refresh_tokens WHERE revoked_at IS NULL AND consumed_at IS NULL",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(live, 0, "sobrou token utilizável depois da revogação");

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(total, 3, "três tokens emitidos, três revogados");
}

#[tokio::test]
async fn revoking_one_family_leaves_the_other_sessions_of_the_same_user_alive() {
    let app = TestApp::spawn().await;
    let first_session = app.register("ana", "CONVITE2").await;
    let laptop = first_session["refresh_token"].as_str().unwrap().to_string();

    // Segundo login: nova família, mesma conta.
    let (status, body) = app
        .post(
            "/auth/login",
            json!({ "email": "ana@exemplo.test", "password": "senha-de-teste-123" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let desktop = body["refresh_token"].as_str().unwrap().to_string();

    // Queima a família do laptop.
    app.post("/auth/refresh", json!({ "refresh_token": laptop.clone() }))
        .await;
    let (status, _) = app
        .post("/auth/refresh", json!({ "refresh_token": laptop }))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A outra sessão continua íntegra: revogar por família, não por usuário.
    let (status, _) = app
        .post("/auth/refresh", json!({ "refresh_token": desktop }))
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn two_simultaneous_refreshes_with_the_same_token_kill_the_family() {
    let app = TestApp::spawn().await;
    let session = app.register("bruno", "CONVITE3").await;
    let token = session["refresh_token"].as_str().unwrap().to_string();

    // Não dá para distinguir uma corrida legítima de um roubo, e o viés correto
    // é derrubar a família: o cliente honesto sempre pode logar de novo.
    let a = app.post("/auth/refresh", json!({ "refresh_token": token.clone() }));
    let b = app.post("/auth/refresh", json!({ "refresh_token": token }));
    let (a, b) = tokio::join!(a, b);

    let statuses = [a.0, b.0];
    assert!(
        statuses.contains(&StatusCode::OK),
        "um dos dois precisa vencer a corrida"
    );
    assert!(
        statuses.contains(&StatusCode::UNAUTHORIZED),
        "o outro precisa ser recusado"
    );

    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM refresh_tokens WHERE revoked_at IS NULL AND consumed_at IS NULL",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(live, 0, "o perdedor da corrida derruba a família");
}

#[tokio::test]
async fn logout_revokes_the_family_and_is_idempotent() {
    let app = TestApp::spawn().await;
    let session = app.register("carla", "CONVITE4").await;
    let token = session["refresh_token"].as_str().unwrap().to_string();

    let (status, _, _) = app
        .post_full("/auth/logout", json!({ "refresh_token": token.clone() }))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = app
        .post("/auth/refresh", json!({ "refresh_token": token.clone() }))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "UNAUTHENTICATED");

    // Deslogar de novo não é erro.
    let (status, _, _) = app
        .post_full("/auth/logout", json!({ "refresh_token": token }))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn an_unknown_refresh_token_is_unauthenticated_not_reused() {
    let app = TestApp::spawn().await;
    let (status, body) = app
        .post("/auth/refresh", json!({ "refresh_token": "nunca-existiu" }))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        error_code(&body),
        "UNAUTHENTICATED",
        "TOKEN_REUSED força novo login no cliente; um token inventado não pode disparar isso"
    );
}

#[tokio::test]
async fn registration_consumes_the_invite_exactly_once() {
    let app = TestApp::spawn().await;
    app.seed_invite("UMAVEZ", 1).await;

    let body = json!({
        "invite_code": "UMAVEZ",
        "email": "primeiro@exemplo.test",
        "username": "primeiro",
        "password": "senha-de-teste-123",
    });
    let (status, _) = app.post("/auth/register", body).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = app
        .post(
            "/auth/register",
            json!({
                "invite_code": "UMAVEZ",
                "email": "segundo@exemplo.test",
                "username": "segundo",
                "password": "senha-de-teste-123",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "CONFLICT");
}

#[tokio::test]
async fn a_failed_registration_gives_the_invite_use_back() {
    let app = TestApp::spawn().await;
    app.seed_invite("DEVOLVE", 1).await;
    app.register("existente", "OUTRO1").await;

    // Username já em uso: a transação inteira reverte, inclusive o consumo.
    let (status, body) = app
        .post(
            "/auth/register",
            json!({
                "invite_code": "DEVOLVE",
                "email": "novo@exemplo.test",
                "username": "existente",
                "password": "senha-de-teste-123",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "CONFLICT");

    let uses: i32 = sqlx::query_scalar("SELECT uses FROM invites WHERE code = 'DEVOLVE'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        uses, 0,
        "o convite não pode ser queimado por um cadastro que falhou"
    );

    // E o convite continua utilizável.
    let (status, _) = app
        .post(
            "/auth/register",
            json!({
                "invite_code": "DEVOLVE",
                "email": "novo@exemplo.test",
                "username": "novo",
                "password": "senha-de-teste-123",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn registration_reports_every_invalid_field_at_once() {
    let app = TestApp::spawn().await;
    let (status, body) = app
        .post(
            "/auth/register",
            json!({
                "invite_code": "X",
                "email": "não-é-email",
                "username": "a",
                "password": "curta",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "VALIDATION_FAILED");
    let details = body["error"]["details"].as_array().unwrap();
    let fields: Vec<&str> = details
        .iter()
        .map(|d| d["field"].as_str().unwrap())
        .collect();
    assert!(fields.contains(&"email"));
    assert!(fields.contains(&"username"));
    assert!(fields.contains(&"password"));
}

#[tokio::test]
async fn login_does_not_reveal_which_emails_exist() {
    let app = TestApp::spawn().await;
    app.register("davi", "CONVITE5").await;

    let (unknown_status, unknown_body) = app
        .post(
            "/auth/login",
            json!({ "email": "ninguem@exemplo.test", "password": "senha-de-teste-123" }),
        )
        .await;
    let (wrong_status, wrong_body) = app
        .post(
            "/auth/login",
            json!({ "email": "davi@exemplo.test", "password": "senha-errada" }),
        )
        .await;

    assert_eq!(unknown_status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&unknown_body), error_code(&wrong_body));
    assert_eq!(
        unknown_body["error"]["message"], wrong_body["error"]["message"],
        "as duas respostas precisam ser indistinguíveis"
    );
}

#[tokio::test]
async fn the_access_token_opens_the_authenticated_routes_and_nothing_else_does() {
    let app = TestApp::spawn().await;
    let session = app.register("elena", "CONVITE6").await;
    let access = session["access_token"].as_str().unwrap();

    let (status, body) = app.get("/users/@me", Some(access)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["username"], "elena");
    assert_eq!(body["email"], "elena@exemplo.test");

    let (status, body) = app.get("/users/@me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "UNAUTHENTICATED");

    let (status, body) = app.get("/users/@me", Some("não-é-um-jwt")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "UNAUTHENTICATED");
}

#[tokio::test]
async fn every_response_carries_a_request_id_that_the_error_body_repeats() {
    let app = TestApp::spawn().await;
    let (status, body, headers) = app
        .post_full("/auth/refresh", json!({ "refresh_token": "inexistente" }))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let header = headers
        .get("x-request-id")
        .expect("X-Request-Id em toda resposta")
        .to_str()
        .unwrap();
    assert_eq!(
        body["error"]["request_id"], header,
        "o id do corpo tem que casar com o do cabeçalho, senão o relato não localiza o log"
    );
    assert!(!header.is_empty());
}

#[tokio::test]
async fn patching_the_profile_distinguishes_absent_from_null_over_http() {
    let app = TestApp::spawn().await;
    let session = app.register("fabio", "CONVITE7").await;
    let access = session["access_token"].as_str().unwrap();

    let (status, body) = app
        .patch(
            "/users/@me",
            access,
            json!({ "display_name": "Fábio", "bio": "biografia" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["display_name"], "Fábio");

    let (status, body) = app
        .patch("/users/@me", access, json!({ "bio": null }))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["display_name"], "Fábio", "campo ausente = não alterar");
    assert!(body["bio"].is_null(), "null = limpar");

    let (status, body) = app
        .patch("/users/@me", access, json!({ "accent_color": "vermelho" }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "VALIDATION_FAILED");
}

#[tokio::test]
async fn a_ghost_user_can_never_log_in() {
    let app = TestApp::spawn().await;
    sqlx::query(
        "INSERT INTO users (id, username, discord_user_id, is_migrated) \
         VALUES ($1, 'fantasma', 42, TRUE)",
    )
    .bind(uuid::Uuid::now_v7())
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, _) = app
        .post(
            "/auth/login",
            json!({ "email": "fantasma@exemplo.test", "password": "qualquer" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
