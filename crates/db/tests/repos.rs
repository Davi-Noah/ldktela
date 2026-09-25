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
async fn a_private_invite_admits_exactly_one_guest() {
    let db = TestDb::migrated().await;
    let owner = seed_user(&db.pool, 70, "owner").await;
    let guest_a = seed_user(&db.pool, 71, "guest-a").await;
    let guest_b = seed_user(&db.pool, 72, "guest-b").await;
    let now = OffsetDateTime::now_utc();
    db::repo::private_calls::insert(
        &db.pool,
        Uuid::now_v7(),
        owner,
        HASH_A,
        now + Duration::minutes(10),
    )
    .await
    .expect("criando chamada");

    let (a, b) = tokio::join!(
        db::repo::private_calls::join(&db.pool, HASH_A, guest_a, now),
        db::repo::private_calls::join(&db.pool, HASH_A, guest_b, now),
    );
    let winners = [a.expect("guest a"), b.expect("guest b")]
        .into_iter()
        .flatten()
        .count();
    assert_eq!(winners, 1, "o convite deve formar uma chamada 1:1");
}

#[tokio::test]
async fn only_the_private_call_owner_can_end_it() {
    let db = TestDb::migrated().await;
    let owner = seed_user(&db.pool, 73, "owner").await;
    let guest = seed_user(&db.pool, 74, "guest").await;
    let now = OffsetDateTime::now_utc();
    let call = db::repo::private_calls::insert(
        &db.pool,
        Uuid::now_v7(),
        owner,
        HASH_A,
        now + Duration::minutes(10),
    )
    .await
    .expect("criando chamada");

    assert!(db::repo::private_calls::end(&db.pool, call.id, guest, now)
        .await
        .expect("tentativa do convidado")
        .is_none());
    assert!(db::repo::private_calls::end(&db.pool, call.id, owner, now)
        .await
        .expect("encerrando como dono")
        .is_some());
}

#[tokio::test]
async fn an_oauth_poll_secret_can_only_be_consumed_once() {
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, 75, "oauth-user").await;
    let attempt = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    db::repo::oauth_login::insert(
        &db.pool,
        attempt,
        HASH_A,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        now + Duration::minutes(5),
    )
    .await
    .expect("criando tentativa");
    assert!(db::repo::oauth_login::is_pending(&db.pool, HASH_A, now)
        .await
        .expect("validando state"));
    assert!(db::repo::oauth_login::complete(&db.pool, HASH_A, user, now)
        .await
        .expect("completando callback"));
    assert!(!db::repo::oauth_login::is_pending(&db.pool, HASH_A, now)
        .await
        .expect("state consumido"));

    let first = db::repo::oauth_login::consume(
        &db.pool,
        attempt,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        now,
    )
    .await
    .expect("primeiro poll");
    let second = db::repo::oauth_login::consume(
        &db.pool,
        attempt,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        now,
    )
    .await
    .expect("segundo poll");
    assert_eq!(first, Some(user));
    assert_eq!(second, None);
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

#[tokio::test]
async fn a_viewer_who_arrives_late_sees_the_real_time_on_air() {
    // RF-34: o inicio vem da sessao, nao do momento em que o espectador entrou.
    // Sem isto, quem chega aos vinte minutos ve "no ar ha 0s".
    let db = TestDb::migrated().await;
    let publisher = seed_user(&db.pool, 1, "quem-publica").await;
    let latecomer = seed_user(&db.pool, 2, "quem-chega-depois").await;

    db::repo::presence::join(&db.pool, publisher, 900)
        .await
        .expect("publicador entra");
    db::repo::presence::set_publishing(&db.pool, publisher, true)
        .await
        .expect("comeca a publicar");
    let session = db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, publisher)
        .await
        .expect("sessao aberta");

    db::repo::presence::join(&db.pool, latecomer, 900)
        .await
        .expect("espectador entra depois");

    let rows = db::repo::presence::list_by_channel(&db.pool, 900)
        .await
        .expect("listando");
    let publishing = rows
        .iter()
        .find(|r| r.user_id == publisher)
        .expect("publicador na lista");
    assert_eq!(
        publishing.publishing_since,
        Some(session.started_at),
        "o inicio precisa ser o da sessao"
    );

    let viewer = rows
        .iter()
        .find(|r| r.user_id == latecomer)
        .expect("espectador na lista");
    assert!(
        viewer.publishing_since.is_none(),
        "quem nao publica nao tem inicio de transmissao"
    );
}

#[tokio::test]
async fn a_closed_session_stops_reporting_time_on_air() {
    let db = TestDb::migrated().await;
    let publisher = seed_user(&db.pool, 1, "pessoa").await;
    db::repo::presence::join(&db.pool, publisher, 900)
        .await
        .expect("entrando");
    db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, publisher)
        .await
        .expect("abrindo");
    db::repo::sessions::close(&db.pool, 900, publisher, OffsetDateTime::now_utc())
        .await
        .expect("fechando");

    let rows = db::repo::presence::list_by_channel(&db.pool, 900)
        .await
        .expect("listando");
    assert!(
        rows[0].publishing_since.is_none(),
        "sessao fechada nao pode continuar contando tempo"
    );
}

#[tokio::test]
async fn two_publishers_in_one_room_each_keep_their_own_start() {
    // RF-31: a sala passa a comportar varias telas, e cada uma tem o seu tempo.
    let db = TestDb::migrated().await;
    let a = seed_user(&db.pool, 1, "a").await;
    let b = seed_user(&db.pool, 2, "b").await;
    for user in [a, b] {
        db::repo::presence::join(&db.pool, user, 900)
            .await
            .expect("entrando");
        db::repo::presence::set_publishing(&db.pool, user, true)
            .await
            .expect("publicando");
    }
    let first = db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, a)
        .await
        .expect("sessao de a");
    let second = db::repo::sessions::open(&db.pool, Uuid::now_v7(), 900, b)
        .await
        .expect("sessao de b");
    assert_ne!(first.id, second.id, "duas telas, duas sessoes");

    let rows = db::repo::presence::list_by_channel(&db.pool, 900)
        .await
        .expect("listando");
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|r| r.publishing && r.publishing_since.is_some()),
        "cada publicador precisa do proprio inicio"
    );
}

/// RF-38 a RF-40 e ADR-0024.
///
/// O apelido é estado do usuário no servidor **dele**: é a única coisa que este
/// produto escreve fora de si mesmo, e errar a restauração estraga algo que não
/// é nosso. Por isso a restauração é testada antes e com mais cuidado do que a
/// marcação.
mod live_tags {
    use super::*;
    use db::repo::live_tags;

    const GUILD: i64 = 1_436_472_447_275_761_798;
    const MEMBER: i64 = 464_986_116_957_667_330;

    #[tokio::test]
    async fn a_member_with_no_nickname_goes_back_to_having_none() {
        // O caso que o ADR-0024 chama pelo nome. Restaurar "sem apelido" como o
        // nome de usuário deixaria o apelido gravado para sempre — e ninguém
        // notaria, porque na tela fica igual.
        let db = TestDb::migrated().await;
        live_tags::remember(&db.pool, GUILD, MEMBER, None, OffsetDateTime::now_utc())
            .await
            .expect("marcar");

        let restored = live_tags::forget(&db.pool, GUILD, MEMBER)
            .await
            .expect("desmarcar");
        assert_eq!(restored.expect("estava marcado").previous_nick, None);
    }

    #[tokio::test]
    async fn the_previous_nickname_comes_back_exactly() {
        let db = TestDb::migrated().await;
        live_tags::remember(
            &db.pool,
            GUILD,
            MEMBER,
            Some("  Gabriel  "),
            OffsetDateTime::now_utc(),
        )
        .await
        .expect("marcar");

        let restored = live_tags::forget(&db.pool, GUILD, MEMBER)
            .await
            .expect("desmarcar");
        assert_eq!(
            restored.expect("estava marcado").previous_nick.as_deref(),
            Some("  Gabriel  "),
            "espaços incluídos: o apelido é do usuário, não nosso para normalizar"
        );
    }

    #[tokio::test]
    async fn tagging_twice_keeps_the_first_nickname() {
        // Sem isto, a segunda marcação gravaria "[LIVE] Gabriel" como apelido
        // anterior e o prefixo viraria permanente (ADR-0024, guarda 4).
        let db = TestDb::migrated().await;
        let now = OffsetDateTime::now_utc();
        live_tags::remember(&db.pool, GUILD, MEMBER, Some("Gabriel"), now)
            .await
            .expect("primeira");
        live_tags::remember(&db.pool, GUILD, MEMBER, Some("[LIVE] Gabriel"), now)
            .await
            .expect("segunda");

        let restored = live_tags::forget(&db.pool, GUILD, MEMBER)
            .await
            .expect("desmarcar");
        assert_eq!(
            restored.expect("estava marcado").previous_nick.as_deref(),
            Some("Gabriel")
        );
    }

    #[tokio::test]
    async fn untagging_someone_who_is_not_tagged_does_nothing() {
        let db = TestDb::migrated().await;
        assert_eq!(
            live_tags::forget(&db.pool, GUILD, 1)
                .await
                .expect("desmarcar"),
            None
        );
    }

    #[tokio::test]
    async fn the_sweep_sees_everyone_still_marked() {
        // RF-40: é o que sobra depois de uma queda, e o que a varredura de
        // arranque precisa encontrar antes de aceitar sessão nova.
        let db = TestDb::migrated().await;
        let now = OffsetDateTime::now_utc();
        live_tags::remember(&db.pool, GUILD, 1, Some("um"), now)
            .await
            .expect("um");
        live_tags::remember(&db.pool, GUILD, 2, None, now)
            .await
            .expect("dois");

        let left = live_tags::all(&db.pool).await.expect("varredura");
        assert_eq!(left.len(), 2);
        assert_eq!(left[0].previous_nick.as_deref(), Some("um"));
        assert_eq!(left[1].previous_nick, None);
    }
}
