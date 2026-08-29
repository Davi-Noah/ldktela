//! SRS §5.3 resolvido contra o banco real, não só contra structs em memória.

mod common;

use common::{
    assign_role, join_guild, seed_guild, seed_role, seed_text_channel, seed_user, set_overwrite,
    TestDb,
};
use db::repo::permissions;
use domain::Permissions;
use uuid::Uuid;

fn base_mask() -> i64 {
    (Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES).bits()
}

#[tokio::test]
async fn a_plain_member_gets_the_everyone_mask() {
    let db = TestDb::migrated().await;
    let owner = seed_user(&db.pool, "dono").await;
    let member = seed_user(&db.pool, "membro").await;
    let (guild, _) = seed_guild(&db.pool, owner, base_mask()).await;
    join_guild(&db.pool, guild, member).await;
    let channel = seed_text_channel(&db.pool, guild, "geral").await;

    let mask = permissions::resolve_for_channel(&db.pool, member, channel)
        .await
        .unwrap()
        .expect("canal existe");
    assert_eq!(mask.bits(), base_mask());
}

#[tokio::test]
async fn the_owner_bypasses_every_deny() {
    let db = TestDb::migrated().await;
    let owner = seed_user(&db.pool, "dono").await;
    let (guild, everyone) = seed_guild(&db.pool, owner, 0).await;
    let channel = seed_text_channel(&db.pool, guild, "privado").await;
    set_overwrite(
        &db.pool,
        channel,
        "role",
        everyone,
        0,
        Permissions::ALL.bits(),
    )
    .await;

    let mask = permissions::resolve_for_channel(&db.pool, owner, channel)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(mask, Permissions::ALL);
}

#[tokio::test]
async fn a_private_channel_hides_itself_from_everyone_and_opens_for_one_role() {
    let db = TestDb::migrated().await;
    let owner = seed_user(&db.pool, "dono").await;
    let outsider = seed_user(&db.pool, "forasteiro").await;
    let insider = seed_user(&db.pool, "convidado").await;
    let (guild, everyone) = seed_guild(&db.pool, owner, base_mask()).await;
    join_guild(&db.pool, guild, outsider).await;
    join_guild(&db.pool, guild, insider).await;

    let channel = seed_text_channel(&db.pool, guild, "privado").await;
    // Canal privado = deny VIEW_CHANNEL no overwrite de @everyone.
    set_overwrite(
        &db.pool,
        channel,
        "role",
        everyone,
        0,
        Permissions::VIEW_CHANNEL.bits(),
    )
    .await;
    // …e allow VIEW_CHANNEL para um cargo.
    let staff = seed_role(&db.pool, guild, "staff", 0).await;
    assign_role(&db.pool, guild, insider, staff).await;
    set_overwrite(
        &db.pool,
        channel,
        "role",
        staff,
        Permissions::VIEW_CHANNEL.bits(),
        0,
    )
    .await;

    let outside = permissions::resolve_for_channel(&db.pool, outsider, channel)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !outside.contains(Permissions::VIEW_CHANNEL),
        "quem não tem o cargo não pode ver o canal"
    );

    let inside = permissions::resolve_for_channel(&db.pool, insider, channel)
        .await
        .unwrap()
        .unwrap();
    assert!(inside.contains(Permissions::VIEW_CHANNEL));
    assert!(inside.contains(Permissions::SEND_MESSAGES));
}

#[tokio::test]
async fn a_member_overwrite_beats_the_role_that_allowed_it() {
    let db = TestDb::migrated().await;
    let owner = seed_user(&db.pool, "dono").await;
    let member = seed_user(&db.pool, "membro").await;
    let (guild, _) = seed_guild(&db.pool, owner, base_mask()).await;
    join_guild(&db.pool, guild, member).await;
    let channel = seed_text_channel(&db.pool, guild, "geral").await;

    let staff = seed_role(&db.pool, guild, "staff", 0).await;
    assign_role(&db.pool, guild, member, staff).await;
    set_overwrite(
        &db.pool,
        channel,
        "role",
        staff,
        Permissions::SEND_MESSAGES.bits(),
        0,
    )
    .await;
    set_overwrite(
        &db.pool,
        channel,
        "member",
        member,
        0,
        Permissions::SEND_MESSAGES.bits(),
    )
    .await;

    let mask = permissions::resolve_for_channel(&db.pool, member, channel)
        .await
        .unwrap()
        .unwrap();
    assert!(mask.contains(Permissions::VIEW_CHANNEL));
    assert!(
        !mask.contains(Permissions::SEND_MESSAGES),
        "o passo 7 vem depois do passo 6"
    );
}

#[tokio::test]
async fn administrator_short_circuits_before_any_overwrite_is_read() {
    let db = TestDb::migrated().await;
    let owner = seed_user(&db.pool, "dono").await;
    let admin = seed_user(&db.pool, "admin").await;
    let (guild, everyone) = seed_guild(&db.pool, owner, 0).await;
    join_guild(&db.pool, guild, admin).await;
    let channel = seed_text_channel(&db.pool, guild, "privado").await;
    set_overwrite(
        &db.pool,
        channel,
        "role",
        everyone,
        0,
        Permissions::ALL.bits(),
    )
    .await;
    set_overwrite(
        &db.pool,
        channel,
        "member",
        admin,
        0,
        Permissions::ALL.bits(),
    )
    .await;

    let role = seed_role(&db.pool, guild, "adm", Permissions::ADMINISTRATOR.bits()).await;
    assign_role(&db.pool, guild, admin, role).await;

    let mask = permissions::resolve_for_channel(&db.pool, admin, channel)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(mask, Permissions::ALL);
}

#[tokio::test]
async fn a_non_member_of_the_guild_sees_nothing() {
    let db = TestDb::migrated().await;
    let owner = seed_user(&db.pool, "dono").await;
    let stranger = seed_user(&db.pool, "estranho").await;
    let (guild, _) = seed_guild(&db.pool, owner, Permissions::ALL.bits()).await;
    let channel = seed_text_channel(&db.pool, guild, "geral").await;

    let mask = permissions::resolve_for_channel(&db.pool, stranger, channel)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(mask, Permissions::NONE);
    assert!(!permissions::can_view(&db.pool, stranger, channel)
        .await
        .unwrap());
}

#[tokio::test]
async fn a_banned_member_loses_access_without_losing_the_row() {
    let db = TestDb::migrated().await;
    let owner = seed_user(&db.pool, "dono").await;
    let banned = seed_user(&db.pool, "banido").await;
    let (guild, _) = seed_guild(&db.pool, owner, Permissions::ALL.bits()).await;
    join_guild(&db.pool, guild, banned).await;
    let channel = seed_text_channel(&db.pool, guild, "geral").await;

    assert!(permissions::can_view(&db.pool, banned, channel)
        .await
        .unwrap());

    sqlx::query("UPDATE guild_members SET banned_at = NOW() WHERE guild_id = $1 AND user_id = $2")
        .bind(guild)
        .bind(banned)
        .execute(&db.pool)
        .await
        .unwrap();

    assert!(!permissions::can_view(&db.pool, banned, channel)
        .await
        .unwrap());
}

#[tokio::test]
async fn direct_messages_short_circuit_at_step_zero() {
    let db = TestDb::migrated().await;
    let a = seed_user(&db.pool, "ana").await;
    let b = seed_user(&db.pool, "bruno").await;
    let outsider = seed_user(&db.pool, "carla").await;

    let channel = Uuid::now_v7();
    sqlx::query("INSERT INTO channels (id, name, type) VALUES ($1, 'dm', 'dm')")
        .bind(channel)
        .execute(&db.pool)
        .await
        .unwrap();
    for user in [a, b] {
        sqlx::query("INSERT INTO channel_participants (channel_id, user_id) VALUES ($1, $2)")
            .bind(channel)
            .bind(user)
            .execute(&db.pool)
            .await
            .unwrap();
    }

    let inside = permissions::resolve_for_channel(&db.pool, a, channel)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(inside, Permissions::DIRECT_MESSAGE);

    let outside = permissions::resolve_for_channel(&db.pool, outsider, channel)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outside, Permissions::NONE);

    // Quem sai perde o acesso, e as mensagens continuam para os demais.
    assert!(db::repo::channels::remove_participant(&db.pool, channel, b)
        .await
        .unwrap());
    let left = permissions::resolve_for_channel(&db.pool, b, channel)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(left, Permissions::NONE);
    assert!(permissions::can_view(&db.pool, a, channel).await.unwrap());
}

#[tokio::test]
async fn a_missing_channel_resolves_to_none_not_to_an_empty_mask() {
    let db = TestDb::migrated().await;
    let user = seed_user(&db.pool, "alguem").await;
    let resolved = permissions::resolve_for_channel(&db.pool, user, Uuid::now_v7())
        .await
        .unwrap();
    assert!(
        resolved.is_none(),
        "canal inexistente e canal invisível são estados diferentes para o repositório; \
         é a rota que colapsa os dois em 404"
    );
}

#[tokio::test]
async fn guild_level_permissions_ignore_channel_overwrites() {
    let db = TestDb::migrated().await;
    let owner = seed_user(&db.pool, "dono").await;
    let member = seed_user(&db.pool, "membro").await;
    let (guild, everyone) = seed_guild(&db.pool, owner, Permissions::CREATE_INVITE.bits()).await;
    join_guild(&db.pool, guild, member).await;
    let channel = seed_text_channel(&db.pool, guild, "geral").await;
    set_overwrite(
        &db.pool,
        channel,
        "role",
        everyone,
        0,
        Permissions::CREATE_INVITE.bits(),
    )
    .await;

    let guild_mask = permissions::resolve_for_guild(&db.pool, member, guild)
        .await
        .unwrap();
    assert!(
        guild_mask.contains(Permissions::CREATE_INVITE),
        "um overwrite de canal não pode remover uma permissão de guild"
    );

    assert_eq!(
        permissions::resolve_for_guild(&db.pool, owner, guild)
            .await
            .unwrap(),
        Permissions::ALL
    );
}
