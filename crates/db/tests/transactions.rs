//! Escritas relacionadas são transacionais, e os guards de concorrência têm um
//! único vencedor (CLAUDE.md §7).

mod common;

use common::{seed_user, TestDb};
use db::repo::{invites, refresh_tokens, users};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[tokio::test]
async fn registration_is_atomic_between_the_invite_and_the_user() {
    let db = TestDb::migrated().await;
    let admin = seed_user(&db.pool, "admin").await;
    invites::insert(&db.pool, Uuid::now_v7(), "CODIGO01", admin, None, 1, None)
        .await
        .unwrap();

    // Cadastro que falha depois de consumir o convite: o username já existe.
    let mut tx = db.pool.begin().await.unwrap();
    invites::consume(&mut *tx, "CODIGO01").await.unwrap();
    let duplicate = users::insert_real(
        &mut *tx,
        Uuid::now_v7(),
        "outro@exemplo.test",
        "admin",
        "hash",
    )
    .await;
    assert!(duplicate.is_err(), "username duplicado deveria falhar");
    drop(tx); // sem commit: rollback

    let invite = invites::find_by_code(&db.pool, "CODIGO01")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        invite.uses, 0,
        "o convite não pode ficar consumido por um cadastro que falhou"
    );
    assert!(invite.is_valid(OffsetDateTime::now_utc()));

    // Agora o caminho feliz, na mesma transação.
    let mut tx = db.pool.begin().await.unwrap();
    invites::consume(&mut *tx, "CODIGO01").await.unwrap();
    users::insert_real(
        &mut *tx,
        Uuid::now_v7(),
        "novo@exemplo.test",
        "novo",
        "hash",
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let invite = invites::find_by_code(&db.pool, "CODIGO01")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invite.uses, 1);
    assert!(!invite.is_valid(OffsetDateTime::now_utc()));
}

#[tokio::test]
async fn a_single_use_invite_has_exactly_one_winner_under_concurrency() {
    let db = TestDb::migrated().await;
    let admin = seed_user(&db.pool, "admin").await;
    invites::insert(&db.pool, Uuid::now_v7(), "UMAVEZ", admin, None, 1, None)
        .await
        .unwrap();

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let pool = db.pool.clone();
        tasks.push(tokio::spawn(async move {
            invites::consume(&pool, "UMAVEZ").await.is_ok()
        }));
    }
    let mut winners = 0;
    for t in tasks {
        if t.await.unwrap() {
            winners += 1;
        }
    }
    assert_eq!(winners, 1, "oito tentativas simultâneas, um único uso");

    let invite = invites::find_by_code(&db.pool, "UMAVEZ")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invite.uses, 1);
}

#[tokio::test]
async fn an_expired_or_revoked_invite_cannot_be_consumed() {
    let db = TestDb::migrated().await;
    let admin = seed_user(&db.pool, "admin").await;
    let past = OffsetDateTime::now_utc() - Duration::hours(1);
    invites::insert(
        &db.pool,
        Uuid::now_v7(),
        "EXPIRADO",
        admin,
        None,
        10,
        Some(past),
    )
    .await
    .unwrap();
    assert!(invites::consume(&db.pool, "EXPIRADO").await.is_err());

    invites::insert(&db.pool, Uuid::now_v7(), "REVOGADO", admin, None, 10, None)
        .await
        .unwrap();
    invites::revoke(&db.pool, "REVOGADO").await.unwrap();
    assert!(invites::consume(&db.pool, "REVOGADO").await.is_err());
}

#[tokio::test]
async fn consuming_a_refresh_token_twice_has_exactly_one_winner() {
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, "usuario").await;
    let family = Uuid::now_v7();
    let id = Uuid::now_v7();
    refresh_tokens::insert(
        &db.pool,
        id,
        family,
        user,
        &"a".repeat(64),
        None,
        OffsetDateTime::now_utc() + Duration::days(30),
    )
    .await
    .unwrap();

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let pool = db.pool.clone();
        tasks.push(tokio::spawn(async move {
            refresh_tokens::consume(&pool, id).await.unwrap()
        }));
    }
    let mut winners = 0;
    for t in tasks {
        if t.await.unwrap() {
            winners += 1;
        }
    }
    assert_eq!(
        winners, 1,
        "o guard consumed_at IS NULL é o que torna a rotação segura"
    );
}

#[tokio::test]
async fn revoking_a_family_kills_every_live_token_in_it() {
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, "usuario").await;
    let family = Uuid::now_v7();
    let other_family = Uuid::now_v7();
    let expires = OffsetDateTime::now_utc() + Duration::days(30);

    for i in 0..3 {
        refresh_tokens::insert(
            &db.pool,
            Uuid::now_v7(),
            family,
            user,
            &format!("{i}").repeat(64),
            None,
            expires,
        )
        .await
        .unwrap();
    }
    let survivor = Uuid::now_v7();
    refresh_tokens::insert(
        &db.pool,
        survivor,
        other_family,
        user,
        &"z".repeat(64),
        None,
        expires,
    )
    .await
    .unwrap();

    assert_eq!(
        refresh_tokens::revoke_family(&db.pool, family)
            .await
            .unwrap(),
        3
    );
    for token in refresh_tokens::list_family(&db.pool, family).await.unwrap() {
        assert!(token.revoked_at.is_some());
        assert!(!token.is_usable(OffsetDateTime::now_utc()));
    }
    let other = refresh_tokens::list_family(&db.pool, other_family)
        .await
        .unwrap();
    assert!(
        other[0].is_usable(OffsetDateTime::now_utc()),
        "revogar uma família não pode derrubar as outras sessões do usuário"
    );

    // Revogar de novo não conta ninguém: já estão todos revogados.
    assert_eq!(
        refresh_tokens::revoke_family(&db.pool, family)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn expired_tokens_are_collected_after_the_grace_period() {
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, "usuario").await;
    let long_gone = Uuid::now_v7();
    refresh_tokens::insert(
        &db.pool,
        long_gone,
        Uuid::now_v7(),
        user,
        &"a".repeat(64),
        None,
        OffsetDateTime::now_utc() - Duration::days(40),
    )
    .await
    .unwrap();
    let recent = Uuid::now_v7();
    refresh_tokens::insert(
        &db.pool,
        recent,
        Uuid::now_v7(),
        user,
        &"b".repeat(64),
        None,
        OffsetDateTime::now_utc() - Duration::days(1),
    )
    .await
    .unwrap();

    assert_eq!(
        refresh_tokens::delete_expired(&db.pool, 7).await.unwrap(),
        1
    );
    assert!(refresh_tokens::find_by_hash(&db.pool, &"a".repeat(64))
        .await
        .unwrap()
        .is_none());
    assert!(refresh_tokens::find_by_hash(&db.pool, &"b".repeat(64))
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn a_ghost_and_a_real_account_may_share_a_username() {
    let db = TestDb::migrated().await;
    users::insert_real(
        &db.pool,
        Uuid::now_v7(),
        "pessoa@exemplo.test",
        "pessoa",
        "hash",
    )
    .await
    .unwrap();
    users::insert_ghost(&db.pool, Uuid::now_v7(), "pessoa", None, 42, None)
        .await
        .expect("o índice único de username é parcial em is_migrated = FALSE");

    // Mas duas contas reais, não.
    let clash = users::insert_real(
        &db.pool,
        Uuid::now_v7(),
        "outra@exemplo.test",
        "PESSOA",
        "hash",
    )
    .await;
    assert!(
        clash.is_err(),
        "o índice é sobre lower(username): a colisão é insensível a caixa"
    );

    assert!(users::find_real_by_username(&db.pool, "PESSOA")
        .await
        .unwrap()
        .is_some());
    assert!(users::find_by_discord_id(&db.pool, 42)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn patching_a_profile_distinguishes_absent_from_null() {
    let db = TestDb::migrated().await;
    let id = Uuid::now_v7();
    users::insert_real(&db.pool, id, "p@exemplo.test", "pessoa", "hash")
        .await
        .unwrap();

    let row = users::update_profile(
        &db.pool,
        id,
        Some(Some("Pessoa".into())),
        None,
        Some(Some("bio".into())),
        None,
    )
    .await
    .unwrap();
    assert_eq!(row.display_name.as_deref(), Some("Pessoa"));
    assert_eq!(row.bio.as_deref(), Some("bio"));

    // Campo ausente não altera; null limpa.
    let row = users::update_profile(&db.pool, id, None, None, Some(None), None)
        .await
        .unwrap();
    assert_eq!(
        row.display_name.as_deref(),
        Some("Pessoa"),
        "campo ausente = não alterar"
    );
    assert_eq!(row.bio, None, "null = limpar");
}
