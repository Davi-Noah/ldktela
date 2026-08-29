//! A fake S3, for the network boundary only.
//!
//! CLAUDE.md §2.10 forbids repository mocks so that the SQL is exercised for
//! real. It does not forbid faking an external network boundary — and it should
//! not: reaching Cloudflare from a test suite would be slow, flaky and needs a
//! credential nobody should hand to CI.
//!
//! This speaks enough of the S3 API for the attachment flow: `PUT`, `HEAD`,
//! `DELETE` and `ListObjectsV2`.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, put};
use axum::Router;

#[derive(Clone, Debug)]
pub struct StoredObject {
    pub bytes: usize,
    /// Seconds since the epoch, overridable so a test can age an object.
    pub last_modified: i64,
}

#[derive(Clone, Default)]
pub struct FakeS3 {
    objects: Arc<Mutex<HashMap<String, StoredObject>>>,
}

impl FakeS3 {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts an object directly, so a test can arrange state without signing.
    pub fn insert(&self, key: &str, bytes: usize, last_modified: i64) {
        self.objects.lock().expect("fake s3 lock").insert(
            key.to_string(),
            StoredObject {
                bytes,
                last_modified,
            },
        );
    }

    pub fn contains(&self, key: &str) -> bool {
        self.objects.lock().expect("fake s3 lock").contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.objects.lock().expect("fake s3 lock").len()
    }

    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .objects
            .lock()
            .expect("fake s3 lock")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    /// Binds an ephemeral port and serves until the process ends.
    pub async fn spawn() -> (Self, String) {
        let state = Self::new();
        let router = Router::new()
            .route("/{bucket}", get(list_objects))
            // The SDK signs ListObjectsV2 against the bucket with a trailing
            // slash; without this route it lands on axum's own 404 and the
            // failure reads as a service error.
            .route("/{bucket}/", get(list_objects_trailing))
            .route(
                "/{bucket}/{*key}",
                put(put_object).head(head_object).delete(delete_object),
            )
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binding fake s3");
        let addr = listener.local_addr().expect("fake s3 addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (state, format!("http://{addr}"))
    }
}

async fn put_object(
    State(state): State<FakeS3>,
    Path((_bucket, key)): Path<(String, String)>,
    body: Bytes,
) -> StatusCode {
    state.insert(
        &key,
        body.len(),
        time::OffsetDateTime::now_utc().unix_timestamp(),
    );
    StatusCode::OK
}

async fn head_object(
    State(state): State<FakeS3>,
    Path((_bucket, key)): Path<(String, String)>,
) -> (StatusCode, HeaderMap) {
    let objects = state.objects.lock().expect("fake s3 lock");
    match objects.get(&key) {
        Some(object) => {
            let mut headers = HeaderMap::new();
            headers.insert(
                axum::http::header::CONTENT_LENGTH,
                object.bytes.to_string().parse().expect("length header"),
            );
            (StatusCode::OK, headers)
        }
        None => (StatusCode::NOT_FOUND, HeaderMap::new()),
    }
}

async fn delete_object(
    State(state): State<FakeS3>,
    Path((_bucket, key)): Path<(String, String)>,
) -> StatusCode {
    state.objects.lock().expect("fake s3 lock").remove(&key);
    StatusCode::NO_CONTENT
}

async fn list_objects_trailing(
    state: State<FakeS3>,
    bucket: Path<String>,
    params: Query<HashMap<String, String>>,
) -> (
    StatusCode,
    [(axum::http::HeaderName, &'static str); 1],
    String,
) {
    list_objects(state, bucket, params).await
}

async fn list_objects(
    State(state): State<FakeS3>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> (
    StatusCode,
    [(axum::http::HeaderName, &'static str); 1],
    String,
) {
    let prefix = params.get("prefix").cloned().unwrap_or_default();
    let objects = state.objects.lock().expect("fake s3 lock");
    let mut entries: Vec<(&String, &StoredObject)> = objects
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    let mut body = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">"#,
    );
    body.push_str(&format!("<Name>{bucket}</Name>"));
    body.push_str(&format!("<Prefix>{prefix}</Prefix>"));
    body.push_str(&format!("<KeyCount>{}</KeyCount>", entries.len()));
    body.push_str("<MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated>");
    for (key, object) in entries {
        let modified = time::OffsetDateTime::from_unix_timestamp(object.last_modified)
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        body.push_str(&format!(
            "<Contents><Key>{key}</Key><LastModified>{modified}</LastModified>\
             <Size>{}</Size><StorageClass>STANDARD</StorageClass></Contents>",
            object.bytes
        ));
    }
    body.push_str("</ListBucketResult>");

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/xml")],
        body,
    )
}
