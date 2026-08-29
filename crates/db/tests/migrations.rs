//! Portão do E1: as migrations aplicam, revertem e reaplicam sem resíduo.

mod common;

use common::{application_enums, application_tables, TestDb, EXPECTED_TABLES};

#[tokio::test]
async fn migrations_apply_revert_and_reapply_leaving_no_residue() {
    let db = TestDb::empty().await;

    // Aplica tudo.
    db::MIGRATOR.run(&db.pool).await.expect("first run");
    let tables = application_tables(&db.pool).await;
    assert_eq!(
        tables, EXPECTED_TABLES,
        "conjunto de tabelas diverge do SRS §5.2"
    );
    assert_eq!(
        application_enums(&db.pool).await,
        vec!["channel_type", "message_origin", "overwrite_target"],
        "conjunto de enums diverge do SRS §5.2"
    );

    // Reverte tudo. Falha aqui significa ordem de DROP errada em algum .down.sql.
    db::MIGRATOR
        .undo(&db.pool, 0)
        .await
        .expect("reverting every migration");
    assert!(
        application_tables(&db.pool).await.is_empty(),
        "reverter deixou tabelas para trás"
    );
    assert!(
        application_enums(&db.pool).await.is_empty(),
        "reverter deixou tipos enum para trás"
    );

    // Reaplica: prova que o down não destruiu a capacidade de subir de novo.
    db::MIGRATOR.run(&db.pool).await.expect("second run");
    assert_eq!(application_tables(&db.pool).await, EXPECTED_TABLES);
}

#[tokio::test]
async fn schema_enforces_the_srs_constraints_that_carry_meaning() {
    let db = TestDb::migrated().await;

    // chk_real_user_credentials: conta real exige email e hash de senha.
    let err = sqlx::query(
        "INSERT INTO users (id, username, is_migrated) VALUES ($1, 'sem-credencial', FALSE)",
    )
    .bind(uuid::Uuid::now_v7())
    .execute(&db.pool)
    .await
    .expect_err("usuário real sem credencial deveria violar a CHECK");
    assert!(
        err.to_string().contains("chk_real_user_credentials"),
        "violação inesperada: {err}"
    );

    // Ghost user (RF-26) não precisa de email nem senha.
    sqlx::query("INSERT INTO users (id, username, is_migrated) VALUES ($1, 'fantasma', TRUE)")
        .bind(uuid::Uuid::now_v7())
        .execute(&db.pool)
        .await
        .expect("ghost user deveria ser aceito");

    // idx_users_username: unicidade de username só vale para contas reais.
    sqlx::query("INSERT INTO users (id, username, is_migrated) VALUES ($1, 'fantasma', TRUE)")
        .bind(uuid::Uuid::now_v7())
        .execute(&db.pool)
        .await
        .expect("dois ghosts podem repetir username");

    let owner = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, username, password_hash, is_migrated) \
         VALUES ($1, 'dono@exemplo.test', 'dono', 'x', FALSE)",
    )
    .bind(owner)
    .execute(&db.pool)
    .await
    .expect("conta real com credencial");

    let guild = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, 'guild', $2)")
        .bind(guild)
        .bind(owner)
        .execute(&db.pool)
        .await
        .expect("guild");

    // chk_channel_scope: canal de texto exige guild_id.
    let err = sqlx::query("INSERT INTO channels (id, name, type) VALUES ($1, 'orfao', 'text')")
        .bind(uuid::Uuid::now_v7())
        .execute(&db.pool)
        .await
        .expect_err("canal de texto sem guild deveria violar a CHECK");
    assert!(
        err.to_string().contains("chk_channel_scope"),
        "violação inesperada: {err}"
    );

    // chk_channel_scope: conversa direta exige a ausência de guild_id.
    let err = sqlx::query(
        "INSERT INTO channels (id, guild_id, name, type) VALUES ($1, $2, 'dm-em-guild', 'dm')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(guild)
    .execute(&db.pool)
    .await
    .expect_err("dm com guild_id deveria violar a CHECK");
    assert!(
        err.to_string().contains("chk_channel_scope"),
        "violação inesperada: {err}"
    );

    // chk_bridge_scope (RF-18a): ponte nunca se aplica a conversa direta.
    let err = sqlx::query(
        "INSERT INTO channels (id, name, type, bridge_enabled) VALUES ($1, 'dm', 'dm', TRUE)",
    )
    .bind(uuid::Uuid::now_v7())
    .execute(&db.pool)
    .await
    .expect_err("ponte em dm deveria violar a CHECK");
    assert!(
        err.to_string().contains("chk_bridge_scope"),
        "violação inesperada: {err}"
    );

    // idx_roles_default: um único cargo @everyone por guild.
    sqlx::query(
        "INSERT INTO roles (id, guild_id, name, is_default) VALUES ($1, $2, '@everyone', TRUE)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(guild)
    .execute(&db.pool)
    .await
    .expect("primeiro @everyone");

    let err = sqlx::query(
        "INSERT INTO roles (id, guild_id, name, is_default) VALUES ($1, $2, '@everyone2', TRUE)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(guild)
    .execute(&db.pool)
    .await
    .expect_err("segundo cargo padrão deveria violar o índice único");
    assert!(
        err.to_string().contains("idx_roles_default"),
        "violação inesperada: {err}"
    );
}

#[tokio::test]
async fn voice_states_is_unlogged_as_the_srs_requires() {
    let db = TestDb::migrated().await;
    // relpersistence: 'u' = unlogged, 'p' = permanent.
    let persistence: String = sqlx::query_scalar(
        "SELECT relpersistence::text FROM pg_class WHERE relname = 'voice_states'",
    )
    .fetch_one(&db.pool)
    .await
    .expect("consultando pg_class");
    assert_eq!(persistence, "u", "voice_states precisa ser UNLOGGED");
}

#[tokio::test]
async fn full_text_index_uses_the_portuguese_configuration() {
    let db = TestDb::migrated().await;
    let definition: String =
        sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE indexname = 'idx_messages_fts'")
            .fetch_one(&db.pool)
            .await
            .expect("consultando pg_indexes");
    assert!(
        definition.contains("portuguese"),
        "o índice GIN de busca precisa usar a configuração 'portuguese' (RF-17): {definition}"
    );
    assert!(
        definition.contains("gin"),
        "o índice de busca precisa ser GIN: {definition}"
    );
}
