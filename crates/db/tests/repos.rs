//! Repository behaviour that only a real database can prove: atomicity of the
//! pairing consume, and the presence/session bookkeeping the room depends on.

mod common;

use common::{seed_user, TestDb};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

async fn insert_code(pool: &db::PgPool, hash: &str, discord_user: i64, ttl: Duration) {
    db::repo::pairing::insert(
        pool,
        Uuid::now_v7(),
        hash,
        discord_user,
        1234,
        OffsetDateTime::now_utc() + ttl,
    )
    .await
    .expect("inserindo codigo");
}

#[tokio::test]
async fn a_pairing_code_can_only_be_consumed_once() {
    let db = TestDb::migrated().await;
    insert_code(&db.pool, HASH_A, 42, Duration::minutes(5)).await;
    let now = OffsetDateTime::now_utc();

    let first = db::repo::pairing::consume(&db.pool, HASH_A, now)
        .await
        .expect("consumindo");
    assert_eq!(first.map(|c| c.discord_user_id), Some(42));

    let second = db::repo::pairing::consume(&db.pool, HASH_A, now)
        .await
        .expect("segunda tentativa");
    assert!(second.is_none(), "o codigo foi aceito duas vezes");
}

#[tokio::test]
async fn concurrent_consumers_of_one_code_produce_exactly_one_winner() {
    // O guard esta no WHERE do UPDATE, entao o Postgres serializa as duas
    // tentativas. Um "SELECT depois UPDATE" deixaria as duas passarem.
    let db = TestDb::migrated().await;
    insert_code(&db.pool, HASH_A, 42, Duration::minutes(5)).await;
    let now = OffsetDateTime::now_utc();

    let (a, b) = tokio::join!(
        db::repo::pairing::consume(&db.pool, HASH_A, now),
        db::repo::pairing::consume(&db.pool, HASH_A, now),
    );
    let winners = [a.expect("a"), b.expect("b")].into_iter().flatten().count();
    assert_eq!(winners, 1, "exatamente um consumidor deveria vencer");
}

#[tokio::test]
async fn an_expired_code_is_indistinguishable_from_a_missing_one() {
    let db = TestDb::migrated().await;
    insert_code(&db.pool, HASH_A, 42, Duration::minutes(-1)).await;
    let now = OffsetDateTime::now_utc();

    let expired = db::repo::pairing::consume(&db.pool, HASH_A, now)
        .await
        .expect("codigo expirado");
    let unknown = db::repo::pairing::consume(&db.pool, "0".repeat(64).as_str(), now)
        .await
        .expect("codigo inexistente");
    assert!(expired.is_none());
    assert!(unknown.is_none());
}

#[tokio::test]
async fn request_count_only_sees_the_window() {
    let db = TestDb::migrated().await;
    insert_code(&db.pool, HASH_A, 42, Duration::minutes(5)).await;
    let now = OffsetDateTime::now_utc();

    let inside = db::repo::pairing::recent_request_count(&db.pool, 42, now - Duration::minutes(1))
        .await
        .expect("dentro da janela");
    let outside = db::repo::pairing::recent_request_count(&db.pool, 42, now + Duration::minutes(1))
        .await
        .expect("fora da janela");
    assert_eq!(inside, 1);
    assert_eq!(outside, 0);
}

#[tokio::test]
async fn a_user_is_in_at_most_one_room() {
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, 1, "pessoa").await;

    db::repo::presence::join(&db.pool, user, 900)
        .await
        .expect("entrando na primeira sala");
    db::repo::presence::join(&db.pool, user, 901)
        .await
        .expect("mudando de sala");

    assert_eq!(
        db::repo::presence::channel_of(&db.pool, user)
            .await
            .expect("consultando"),
        Some(901)
    );
    assert!(db::repo::presence::list_by_channel(&db.pool, 900)
        .await
        .expect("sala antiga")
        .is_empty());
}

#[tokio::test]
async fn moving_rooms_clears_the_publishing_flag() {
    // Sem isso, quem estava compartilhando e trocou de canal de voz apareceria
    // como publicando numa sala onde nao ha track nenhuma.
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, 1, "pessoa").await;

    db::repo::presence::join(&db.pool, user, 900)
        .await
        .expect("entrando");
    db::repo::presence::set_publishing(&db.pool, user, true)
        .await
        .expect("publicando");
    db::repo::presence::join(&db.pool, user, 901)
        .await
        .expect("mudando de sala");

    let participants = db::repo::presence::list_by_channel(&db.pool, 901)
        .await
        .expect("listando");
    assert_eq!(participants.len(), 1);
    assert!(!participants[0].publishing);
}

#[tokio::test]
async fn leaving_reports_the_room_that_was_left() {
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, 1, "pessoa").await;
    db::repo::presence::join(&db.pool, user, 900)
        .await
        .expect("entrando");

    assert_eq!(
        db::repo::presence::leave(&db.pool, user)
            .await
            .expect("saindo"),
        Some(900),
        "quem sai precisa dizer de onde, ou o fan-out nao sabe quem avisar"
    );
    assert_eq!(
        db::repo::presence::leave(&db.pool, user)
            .await
            .expect("saindo de novo"),
        None
    );
}

#[tokio::test]
async fn setting_publishing_on_an_absent_user_is_not_an_error() {
    // Um webhook do LiveKit pode chegar depois de o usuario ja ter saido.
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, 1, "pessoa").await;
    assert_eq!(
        db::repo::presence::set_publishing(&db.pool, user, true)
            .await
            .expect("webhook atrasado"),
        None
    );
}

#[tokio::test]
async fn publisher_count_feeds_the_admission_guard() {
    let db = TestDb::migrated().await;
    let a = seed_user(&db.pool, 1, "a").await;
    let b = seed_user(&db.pool, 2, "b").await;
    for user in [a, b] {
        db::repo::presence::join(&db.pool, user, 900)
            .await
            .expect("entrando");
    }
    db::repo::presence::set_publishing(&db.pool, a, true)
        .await
        .expect("a publica");

    assert_eq!(
        db::repo::presence::publisher_count(&db.pool, 900)
            .await
            .expect("contando"),
        1
    );
}

#[tokio::test]
async fn peak_viewers_never_goes_down() {
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, 1, "pessoa").await;
    db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, user)
        .await
        .expect("abrindo");

    db::repo::sessions::observe_viewers(&db.pool, 900, 5)
        .await
        .expect("cinco assistindo");
    db::repo::sessions::observe_viewers(&db.pool, 900, 2)
        .await
        .expect("dois assistindo");

    let peak: i32 = sqlx::query_scalar("SELECT peak_viewers FROM share_sessions")
        .fetch_one(&db.pool)
        .await
        .expect("consultando pico");
    assert_eq!(peak, 5, "o pico e o maximo, nao o ultimo valor");
}

#[tokio::test]
async fn egress_since_sums_only_the_window() {
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, 1, "pessoa").await;
    let session = db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, user)
        .await
        .expect("abrindo");
    db::repo::sessions::add_egress(&db.pool, session.id, 1_000)
        .await
        .expect("primeiro lote");
    db::repo::sessions::add_egress(&db.pool, session.id, 500)
        .await
        .expect("segundo lote");

    let now = OffsetDateTime::now_utc();
    assert_eq!(
        db::repo::sessions::egress_since(&db.pool, now - Duration::hours(1))
            .await
            .expect("dentro da janela"),
        1_500
    );
    assert_eq!(
        db::repo::sessions::egress_since(&db.pool, now + Duration::hours(1))
            .await
            .expect("fora da janela"),
        0,
        "sem sessao na janela o total e zero, nao um erro de SUM sobre vazio"
    );
}

#[tokio::test]
async fn startup_closes_sessions_a_crash_left_open() {
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, 1, "pessoa").await;
    db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, user)
        .await
        .expect("abrindo");

    let closed = db::repo::sessions::close_all_open(&db.pool, OffsetDateTime::now_utc())
        .await
        .expect("varrendo");
    assert_eq!(closed, 1);
    assert_eq!(
        db::repo::sessions::close_all_open(&db.pool, OffsetDateTime::now_utc())
            .await
            .expect("segunda varredura"),
        0
    );
}

#[tokio::test]
async fn upserting_a_discord_profile_keeps_the_original_id() {
    // O id local e alvo de chave estrangeira; trocar por causa de um avatar novo
    // levaria presenca e sessoes junto.
    let db = TestDb::migrated().await;
    let first = db::repo::users::upsert_from_discord(
        &db.pool,
        Uuid::now_v7(),
        4242,
        "pessoa",
        Some("Pessoa"),
        None,
    )
    .await
    .expect("primeiro pareamento");

    let second = db::repo::users::upsert_from_discord(
        &db.pool,
        Uuid::now_v7(),
        4242,
        "pessoa-renomeada",
        Some("Outra"),
        Some("https://cdn.example/a.png"),
    )
    .await
    .expect("segundo pareamento");

    assert_eq!(first.id, second.id, "o id local mudou entre pareamentos");
    assert_eq!(second.username, "pessoa-renomeada");
    assert_eq!(
        second.avatar_url.as_deref(),
        Some("https://cdn.example/a.png")
    );
}
