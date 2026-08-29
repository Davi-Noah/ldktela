//! Portão do E3: a paginação por keyset é estável sob inserção concorrente.

mod common;

use std::collections::HashSet;

use common::{seed_guild, seed_text_channel, seed_user, TestDb};
use db::repo::messages;
use protocol::page::Cursor;
use uuid::Uuid;

/// Walks a channel backwards with `before` cursors and returns every id seen,
/// in the order the pages produced them.
async fn walk_backwards(pool: &sqlx::PgPool, channel: Uuid, page_size: u32) -> Vec<Uuid> {
    let mut seen = Vec::new();
    let mut cursor = Cursor::Latest;
    loop {
        let page = messages::page(pool, channel, cursor, page_size)
            .await
            .expect("paginating");
        let Some(last) = page.messages.last().map(|m| m.id) else {
            break;
        };
        seen.extend(page.messages.iter().map(|m| m.id));
        if !page.has_more {
            break;
        }
        cursor = Cursor::Before(last);
    }
    seen
}

#[tokio::test]
async fn keyset_pagination_is_stable_under_concurrent_inserts() {
    let db = TestDb::migrated().await;
    let author = seed_user(&db.pool, "autor").await;
    let (guild, _) = seed_guild(&db.pool, author, 0).await;
    let channel = seed_text_channel(&db.pool, guild, "geral").await;

    // 200 mensagens já existentes: o conjunto que a paginação precisa devolver
    // inteiro, exatamente uma vez, aconteça o que acontecer durante a varredura.
    let mut preexisting = Vec::new();
    for i in 0..200 {
        let id = Uuid::now_v7();
        messages::insert(&db.pool, id, channel, author, &format!("antiga {i}"), None)
            .await
            .expect("inserting");
        preexisting.push(id);
    }

    // Escritores concorrentes: quatro tarefas inserindo enquanto paginamos.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut writers = Vec::new();
    for w in 0..4 {
        let pool = db.pool.clone();
        let stop = stop.clone();
        writers.push(tokio::spawn(async move {
            let mut written = 0u32;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let id = Uuid::now_v7();
                if messages::insert(&pool, id, channel, author, &format!("nova {w}"), None)
                    .await
                    .is_err()
                {
                    break;
                }
                written += 1;
                tokio::task::yield_now().await;
            }
            written
        }));
    }

    let seen = walk_backwards(&db.pool, channel, 25).await;

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut concurrent_writes = 0u32;
    for w in writers {
        concurrent_writes += w.await.expect("writer task");
    }
    assert!(
        concurrent_writes > 0,
        "o teste não exerceu concorrência: nenhuma escrita ocorreu durante a paginação"
    );

    // 1. Nenhuma duplicata. Com OFFSET, cada insercao concorrente empurra a
    //    janela e repete uma linha na página seguinte.
    let unique: HashSet<Uuid> = seen.iter().copied().collect();
    assert_eq!(
        unique.len(),
        seen.len(),
        "a paginação devolveu {} ids repetidos",
        seen.len() - unique.len()
    );

    // 2. Nenhuma lacuna no conjunto pré-existente. Com OFFSET, o mesmo empurrão
    //    faz uma linha antiga nunca aparecer.
    for (i, id) in preexisting.iter().enumerate() {
        assert!(
            unique.contains(id),
            "a mensagem pré-existente #{i} ({id}) não apareceu em nenhuma página"
        );
    }

    // 3. Ordem estritamente decrescente por id dentro e entre páginas.
    for pair in seen.windows(2) {
        assert!(
            pair[0] > pair[1],
            "ordem quebrada: {} veio antes de {}",
            pair[0],
            pair[1]
        );
    }
}

#[tokio::test]
async fn uuid_v7_is_strictly_increasing_even_when_generated_concurrently() {
    // A estabilidade do keyset depende disto: se dois ids gerados em threads
    // diferentes no mesmo milissegundo saírem fora de ordem, uma linha nova pode
    // cair antes de um cursor já ultrapassado e sumir da paginação.
    let mut handles = Vec::new();
    for _ in 0..4 {
        handles.push(std::thread::spawn(|| {
            (0..5_000).map(|_| Uuid::now_v7()).collect::<Vec<_>>()
        }));
    }
    let mut all: Vec<Uuid> = Vec::new();
    for h in handles {
        all.extend(h.join().expect("generator thread"));
    }
    let unique: HashSet<Uuid> = all.iter().copied().collect();
    assert_eq!(unique.len(), all.len(), "UUIDv7 repetiu um valor");
}

#[tokio::test]
async fn cursors_walk_the_channel_in_the_directions_the_contract_defines() {
    let db = TestDb::migrated().await;
    let author = seed_user(&db.pool, "autor").await;
    let (guild, _) = seed_guild(&db.pool, author, 0).await;
    let channel = seed_text_channel(&db.pool, guild, "geral").await;

    let mut ids = Vec::new();
    for i in 0..20 {
        let id = Uuid::now_v7();
        messages::insert(&db.pool, id, channel, author, &format!("m{i}"), None)
            .await
            .expect("inserting");
        ids.push(id);
    }

    // Latest: mais recentes primeiro.
    let latest = messages::page(&db.pool, channel, Cursor::Latest, 5)
        .await
        .unwrap();
    assert!(latest.has_more);
    assert_eq!(
        latest.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
        ids.iter().rev().take(5).copied().collect::<Vec<_>>()
    );

    // Before é exclusivo.
    let before = messages::page(&db.pool, channel, Cursor::Before(ids[10]), 3)
        .await
        .unwrap();
    assert_eq!(
        before.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![ids[9], ids[8], ids[7]]
    );

    // After é exclusivo e devolve em ordem crescente.
    let after = messages::page(&db.pool, channel, Cursor::After(ids[10]), 3)
        .await
        .unwrap();
    assert_eq!(
        after.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![ids[11], ids[12], ids[13]]
    );

    // Around: limit/2 de cada lado, com a âncora no meio, decrescente.
    let around = messages::page(&db.pool, channel, Cursor::Around(ids[10]), 6)
        .await
        .unwrap();
    assert_eq!(
        around.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![ids[13], ids[12], ids[11], ids[10], ids[9], ids[8], ids[7]]
    );

    // Fim do canal: has_more falso.
    let oldest = messages::page(&db.pool, channel, Cursor::Before(ids[2]), 10)
        .await
        .unwrap();
    assert!(!oldest.has_more);
    assert_eq!(oldest.messages.len(), 2);
}

#[tokio::test]
async fn deleted_messages_leave_the_pages_without_leaving_the_table() {
    let db = TestDb::migrated().await;
    let author = seed_user(&db.pool, "autor").await;
    let (guild, _) = seed_guild(&db.pool, author, 0).await;
    let channel = seed_text_channel(&db.pool, guild, "geral").await;

    let mut ids = Vec::new();
    for i in 0..5 {
        let id = Uuid::now_v7();
        messages::insert(&db.pool, id, channel, author, &format!("m{i}"), None)
            .await
            .unwrap();
        ids.push(id);
    }

    assert!(messages::soft_delete(&db.pool, channel, ids[2])
        .await
        .unwrap());
    // Deletar duas vezes não é erro, mas também não afeta nada.
    assert!(!messages::soft_delete(&db.pool, channel, ids[2])
        .await
        .unwrap());

    let page = messages::page(&db.pool, channel, Cursor::Latest, 50)
        .await
        .unwrap();
    assert_eq!(page.messages.len(), 4);
    assert!(!page.messages.iter().any(|m| m.id == ids[2]));

    // A linha continua no banco: o mapeamento cruzado da ponte depende disso.
    let still_there: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE id = $1")
        .bind(ids[2])
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(still_there, 1);
}
