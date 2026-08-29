//! Portão do E10: termo exato em canal sem `VIEW_CHANNEL` não retorna resultado.

mod common;

use axum::http::StatusCode;
use common::{error_code, TestApp};
use domain::Permissions;
use serde_json::json;
use uuid::Uuid;

fn everyone_mask() -> i64 {
    (Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES).bits()
}

struct Scene {
    app: TestApp,
    guild: Uuid,
    everyone_role: Uuid,
    public_channel: Uuid,
    private_channel: Uuid,
    owner_token: String,
    owner_id: Uuid,
    member_token: String,
}

/// Um guild com um canal aberto e um canal negado por overwrite, ambos com
/// mensagens contendo o mesmo termo.
async fn scene() -> Scene {
    let app = TestApp::spawn().await;
    let owner = app.register("dono", "SRCOWNER1").await;
    let owner_id = app.user_id_by_username("dono").await;
    let (guild, everyone_role) = app.seed_guild(owner_id, everyone_mask()).await;
    let member = app.register_into("membro", "SRCMEMBR1", Some(guild)).await;

    let public_channel = app.seed_channel(guild, "geral").await;
    let private_channel = app.seed_channel(guild, "diretoria").await;
    app.set_overwrite(
        private_channel,
        "role",
        everyone_role,
        0,
        Permissions::VIEW_CHANNEL.bits(),
    )
    .await;

    Scene {
        owner_token: owner["access_token"].as_str().unwrap().to_string(),
        member_token: member["access_token"].as_str().unwrap().to_string(),
        app,
        guild,
        everyone_role,
        public_channel,
        private_channel,
        owner_id,
    }
}

async fn post(s: &Scene, channel: Uuid, token: &str, content: &str) -> String {
    let (status, message) = s
        .app
        .post_auth(
            &format!("/channels/{channel}/messages"),
            token,
            json!({ "content": content }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{message}");
    message["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn an_exact_term_in_a_channel_without_view_channel_returns_nothing() {
    let s = scene().await;
    // O mesmo termo nos dois canais, para que a diferença seja só a permissão.
    post(&s, s.public_channel, &s.owner_token, "orçamento aprovado").await;
    post(
        &s,
        s.private_channel,
        &s.owner_token,
        "orçamento sigiloso da diretoria",
    )
    .await;

    // O dono enxerga os dois.
    let (status, page) = s
        .app
        .get(
            &format!("/search?q=orçamento&guild_id={}", s.guild),
            Some(&s.owner_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["data"].as_array().unwrap().len(), 2);

    // O membro enxerga só o canal aberto, mesmo com o termo exato.
    let (status, page) = s
        .app
        .get(
            &format!("/search?q=orçamento&guild_id={}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let hits = page["data"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "só o canal visível pode aparecer");
    assert_eq!(hits[0]["message"]["content"], "orçamento aprovado");
    assert!(
        !page.to_string().contains("sigiloso"),
        "nada do canal invisível pode vazar na resposta: {page}"
    );

    // E buscar o termo que só existe lá dentro não devolve nada.
    let (status, page) = s
        .app
        .get(
            &format!("/search?q=sigiloso&guild_id={}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(page["data"].as_array().unwrap().is_empty());
    assert_eq!(page["has_more"], false);
}

#[tokio::test]
async fn naming_the_private_channel_directly_answers_404_not_an_empty_page() {
    let s = scene().await;
    post(&s, s.private_channel, &s.owner_token, "orçamento sigiloso").await;

    let (status, body) = s
        .app
        .get(
            &format!("/search?q=orçamento&channel_id={}", s.private_channel),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "uma página vazia confirmaria que o canal existe: {body}"
    );
    assert_eq!(error_code(&body), "NOT_FOUND");

    let (status, unknown) = s
        .app
        .get(
            &format!("/search?q=orçamento&channel_id={}", Uuid::now_v7()),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["message"], unknown["error"]["message"]);
}

#[tokio::test]
async fn losing_access_removes_results_that_were_visible_a_moment_ago() {
    // A permissão é recalculada a cada consulta (RF-17), nunca lida de cache.
    let s = scene().await;
    post(&s, s.public_channel, &s.owner_token, "relatório trimestral").await;

    let (_, page) = s
        .app
        .get(
            &format!("/search?q=relatório&guild_id={}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(page["data"].as_array().unwrap().len(), 1);

    // O canal aberto é fechado agora.
    s.app
        .set_overwrite(
            s.public_channel,
            "role",
            s.everyone_role,
            0,
            Permissions::VIEW_CHANNEL.bits(),
        )
        .await;

    let (status, page) = s
        .app
        .get(
            &format!("/search?q=relatório&guild_id={}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page["data"].as_array().unwrap().is_empty(),
        "a busca não pode servir do índice de fan-out: {page}"
    );
}

#[tokio::test]
async fn search_is_stemmed_with_the_portuguese_configuration() {
    let s = scene().await;
    post(
        &s,
        s.public_channel,
        &s.owner_token,
        "marcamos os projetos de segunda",
    )
    .await;

    // Verbo e substantivo flexionados casam com a raiz — é para isto que o
    // índice usa 'portuguese' e não 'simple'.
    for term in ["marcamos", "marcar", "marcado", "projeto", "projetos"] {
        let (status, page) = s
            .app
            .get(
                &format!("/search?q={term}&guild_id={}", s.guild),
                Some(&s.member_token),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            page["data"].as_array().unwrap().len(),
            1,
            "{term} precisa casar com a mesma raiz: {page}"
        );
    }

    // E um termo que não está lá não casa.
    let (_, page) = s
        .app
        .get(
            &format!("/search?q=churrasco&guild_id={}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert!(page["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn the_stemmer_does_not_unify_ao_plurals_nor_missing_accents() {
    // Limitação real do stemmer Snowball de português, medida e não suposta:
    // "reunião" vira 'reuniã' e "reuniões" vira 'reuniõ'; "orçamento" vira
    // 'orçament' e "orcamento" (sem cedilha) vira 'orcament'. Este teste existe
    // para que a limitação apareça como decisão registrada e não como surpresa
    // em produção — é exatamente o caso que o P-02 do SRS §10.1 antecipa ao
    // deixar o índice de trigrama pronto e desativado.
    let s = scene().await;
    post(&s, s.public_channel, &s.owner_token, "reunião de orçamento").await;

    let (_, exact) = s
        .app
        .get(
            &format!("/search?q=reunião&guild_id={}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(exact["data"].as_array().unwrap().len(), 1);

    for term in ["reuniões", "orcamento"] {
        let (_, page) = s
            .app
            .get(
                &format!("/search?q={term}&guild_id={}", s.guild),
                Some(&s.member_token),
            )
            .await;
        assert!(
            page["data"].as_array().unwrap().is_empty(),
            "se {term} passar a casar, o stemmer mudou e o P-02 pode ser revisto: {page}"
        );
    }
}

#[tokio::test]
async fn results_come_newest_first_and_paginate_by_keyset() {
    let s = scene().await;
    let mut ids = Vec::new();
    for i in 0..7 {
        ids.push(
            post(
                &s,
                s.public_channel,
                &s.owner_token,
                &format!("nota {i} sobre projeto"),
            )
            .await,
        );
    }

    let (status, page) = s
        .app
        .get(
            &format!("/search?q=projeto&guild_id={}&limit=3", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["has_more"], true);
    let first: Vec<&str> = page["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["message"]["content"].as_str().unwrap())
        .collect();
    assert_eq!(
        first,
        vec![
            "nota 6 sobre projeto",
            "nota 5 sobre projeto",
            "nota 4 sobre projeto"
        ],
        "ordenação é por recência, não por relevância"
    );

    let cursor = page["data"][2]["message"]["id"].as_str().unwrap();
    let (_, page) = s
        .app
        .get(
            &format!(
                "/search?q=projeto&guild_id={}&limit=3&before={cursor}",
                s.guild
            ),
            Some(&s.member_token),
        )
        .await;
    let second: Vec<&str> = page["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["message"]["content"].as_str().unwrap())
        .collect();
    assert_eq!(
        second,
        vec![
            "nota 3 sobre projeto",
            "nota 2 sobre projeto",
            "nota 1 sobre projeto"
        ]
    );
}

#[tokio::test]
async fn a_hit_carries_its_neighbours_so_the_client_can_open_with_around() {
    let s = scene().await;
    let first = post(&s, s.public_channel, &s.owner_token, "antes").await;
    let target = post(&s, s.public_channel, &s.owner_token, "alvo procurado").await;
    let last = post(&s, s.public_channel, &s.owner_token, "depois").await;

    let (status, page) = s
        .app
        .get(
            &format!("/search?q=alvo&guild_id={}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let hit = &page["data"][0];
    assert_eq!(hit["message"]["id"], target);
    assert_eq!(hit["previous_message_id"], first);
    assert_eq!(hit["next_message_id"], last);
}

#[tokio::test]
async fn the_author_and_period_filters_narrow_the_result() {
    let s = scene().await;
    post(&s, s.public_channel, &s.owner_token, "assunto do dono").await;
    post(&s, s.public_channel, &s.member_token, "assunto do membro").await;

    let (status, page) = s
        .app
        .get(
            &format!(
                "/search?q=assunto&guild_id={}&author_id={}",
                s.guild, s.owner_id
            ),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let hits = page["data"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["message"]["author"]["username"], "dono");

    // Um período no futuro não devolve nada.
    let future = "2099-01-01T00:00:00Z";
    let (_, page) = s
        .app
        .get(
            &format!("/search?q=assunto&guild_id={}&since={future}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert!(page["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_deleted_message_stops_being_findable() {
    let s = scene().await;
    let id = post(
        &s,
        s.public_channel,
        &s.owner_token,
        "erro que quero apagar",
    )
    .await;

    let (_, page) = s
        .app
        .get(
            &format!("/search?q=apagar&guild_id={}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(page["data"].as_array().unwrap().len(), 1);

    s.app
        .delete(
            &format!("/channels/{}/messages/{id}", s.public_channel),
            &s.owner_token,
        )
        .await;

    let (_, page) = s
        .app
        .get(
            &format!("/search?q=apagar&guild_id={}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert!(
        page["data"].as_array().unwrap().is_empty(),
        "o índice é parcial em deleted_at IS NULL: {page}"
    );
}

#[tokio::test]
async fn search_without_a_scope_or_with_both_is_refused() {
    let s = scene().await;
    for query in [
        "/search?q=teste".to_string(),
        format!(
            "/search?q=teste&guild_id={}&channel_id={}",
            s.guild, s.public_channel
        ),
    ] {
        let (status, body) = s.app.get(&query, Some(&s.member_token)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(error_code(&body), "VALIDATION_FAILED");
    }

    // Termo curto demais também é recusado, antes de tocar o índice.
    let (status, body) = s
        .app
        .get(
            &format!("/search?q=a&guild_id={}", s.guild),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_direct_conversation_is_searchable_only_by_its_participants() {
    let s = scene().await;
    let member_id = s.app.user_id_by_username("membro").await;
    let (status, dm) = s
        .app
        .post_auth(
            "/dms",
            &s.owner_token,
            json!({ "recipient_ids": [member_id] }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{dm}");
    let dm_id: Uuid = dm["id"].as_str().unwrap().parse().unwrap();
    post(&s, dm_id, &s.owner_token, "combinado particular").await;

    // Participante encontra.
    let (status, page) = s
        .app
        .get(
            &format!("/search?q=combinado&channel_id={dm_id}"),
            Some(&s.member_token),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["data"].as_array().unwrap().len(), 1);

    // Um terceiro não: a conversa é invisível para ele.
    s.app.register("estranho", "SRCEXTRA1").await;
    let outsider = s.app.register("carla", "SRCCARLA1").await;
    let outsider_token = outsider["access_token"].as_str().unwrap();
    let (status, _) = s
        .app
        .get(
            &format!("/search?q=combinado&channel_id={dm_id}"),
            Some(outsider_token),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
