//! The migrations apply, revert and reapply leaving no residue, and the schema
//! enforces the invariants the SRS relies on.

mod common;

use common::{application_enums, application_tables, TestDb, EXPECTED_TABLES};
use uuid::Uuid;

#[tokio::test]
async fn migrations_apply_revert_and_reapply_leaving_no_residue() {
    let db = TestDb::empty().await;

    db::MIGRATOR.run(&db.pool).await.expect("first run");
    assert_eq!(
        application_tables(&db.pool).await,
        EXPECTED_TABLES,
        "conjunto de tabelas diverge do SRS v2.0 §5"
    );
    assert!(
        application_enums(&db.pool).await.is_empty(),
        "o schema novo nao tem enum: os que existiam descreviam mensagens e ponte"
    );

    // Falha aqui significa ordem de DROP errada em algum .down.sql.
    db::MIGRATOR
        .undo(&db.pool, 0)
        .await
        .expect("reverting every migration");
    assert!(
        application_tables(&db.pool).await.is_empty(),
        "reverter deixou tabelas para tras"
    );

    // Reaplica: prova que o down nao destruiu a capacidade de subir de novo.
    db::MIGRATOR.run(&db.pool).await.expect("second run");
    assert_eq!(application_tables(&db.pool).await, EXPECTED_TABLES);
}

#[tokio::test]
async fn room_presence_is_unlogged() {
    let db = TestDb::migrated().await;
    // relpersistence: 'u' = unlogged, 'p' = permanent.
    let persistence: String = sqlx::query_scalar(
        "SELECT relpersistence::text FROM pg_class WHERE relname = 'room_presence'",
    )
    .fetch_one(&db.pool)
    .await
    .expect("consultando pg_class");
    assert_eq!(
        persistence, "u",
        "presenca e estado efemero e nao deve gerar WAL"
    );
}

#[tokio::test]
async fn a_discord_account_cannot_be_paired_twice() {
    let db = TestDb::migrated().await;
    common::seed_user(&db.pool, 4242, "pessoa").await;

    let err =
        sqlx::query("INSERT INTO users (id, discord_user_id, username) VALUES ($1, 4242, 'clone')")
            .bind(Uuid::now_v7())
            .execute(&db.pool)
            .await
            .expect_err("o mesmo discord_user_id nao pode virar duas contas");
    assert!(
        err.to_string().contains("users_discord_user_id_key"),
        "violacao inesperada: {err}"
    );
}

#[tokio::test]
async fn a_publisher_has_at_most_one_open_session_per_channel() {
    let db = TestDb::migrated().await;
    let user = common::seed_user(&db.pool, 1, "pessoa").await;

    // Compartilhar tela COM audio publica duas tracks, entao `open` e chamado
    // duas vezes para a mesma sessao. Sem o indice parcial, isso viraria duas
    // linhas e o relatorio de egress contaria a sessao em dobro.
    let first = db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, user)
        .await
        .expect("primeira track");
    let second = db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, user)
        .await
        .expect("segunda track da mesma sessao");
    assert_eq!(first.id, second.id, "a segunda track abriu uma sessao nova");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM share_sessions")
        .fetch_one(&db.pool)
        .await
        .expect("contando sessoes");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn closing_a_session_frees_the_channel_for_the_next_one() {
    let db = TestDb::migrated().await;
    let user = common::seed_user(&db.pool, 1, "pessoa").await;
    let now = time::OffsetDateTime::now_utc();

    let first = db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, user)
        .await
        .expect("abrindo");
    db::repo::sessions::close(&db.pool, 900, user, now)
        .await
        .expect("fechando");

    let second = db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, user)
        .await
        .expect("reabrindo depois de fechar");
    assert_ne!(
        first.id, second.id,
        "depois de fechar, a proxima sessao precisa ser uma linha nova"
    );
}

#[tokio::test]
async fn deleting_a_user_takes_their_rows_with_them() {
    let db = TestDb::migrated().await;
    let user = common::seed_user(&db.pool, 7, "pessoa").await;
    db::repo::presence::join(&db.pool, user, 900)
        .await
        .expect("entrando na sala");
    db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, user)
        .await
        .expect("abrindo sessao");

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user)
        .execute(&db.pool)
        .await
        .expect("removendo usuario");

    let presence: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM room_presence")
        .fetch_one(&db.pool)
        .await
        .expect("contando presenca");
    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM share_sessions")
        .fetch_one(&db.pool)
        .await
        .expect("contando sessoes");
    assert_eq!(presence, 0, "presenca orfa ficaria visivel numa sala");
    assert_eq!(sessions, 0);
}
