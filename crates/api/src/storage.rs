//! Object storage (Cloudflare R2, S3 API).
//!
//! Bytes never pass through the backend (RF-10): the client `PUT`s straight to a
//! presigned URL and then references the key. That keeps upload bandwidth off
//! the VM, which matters because the VM's egress budget is the scarce resource
//! (RNF-10).
//!
//! Two rules the flow depends on:
//!
//! * RF-11a is checked **before** the URL is signed. Signing first and checking
//!   afterwards hands out a URL that was never supposed to exist.
//! * The object's existence is confirmed with `HEAD` **before** the attachment
//!   row is written (SRS §6.2), so a message never renders a broken image.

use std::time::Duration;

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::error::DisplayErrorContext;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::Client;
use uuid::Uuid;

use crate::error::{AppError, UpstreamError};

/// Where a key lives and how long a signature lasts.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub endpoint: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub presign_ttl_seconds: u64,
}

pub struct Storage {
    client: Client,
    bucket: String,
    presign_ttl: Duration,
}

impl Storage {
    pub fn new(config: &StorageConfig) -> Self {
        let credentials = Credentials::from_keys(
            config.access_key_id.clone(),
            config.secret_access_key.clone(),
            None,
        );
        let s3 = aws_sdk_s3::Config::builder()
            // R2 has no regions; the SDK still requires one to sign with.
            .region(Region::new("auto"))
            .endpoint_url(config.endpoint.clone())
            .credentials_provider(credentials)
            // Path style keeps the endpoint a single host, which is what R2's
            // account endpoint and any local test server both expect.
            .force_path_style(true)
            .behavior_version(BehaviorVersion::latest())
            .build();
        Self {
            client: Client::from_conf(s3),
            bucket: config.bucket.clone(),
            presign_ttl: Duration::from_secs(config.presign_ttl_seconds),
        }
    }

    pub fn presign_ttl_seconds(&self) -> i64 {
        self.presign_ttl.as_secs() as i64
    }

    /// A key nobody can guess and no filename can escape.
    ///
    /// The UUID prefix means two people uploading `captura.webp` never collide,
    /// and the sanitised name keeps the original visible in a download dialog.
    pub fn key_for(filename: &str) -> String {
        format!("att/{}/{}", Uuid::now_v7(), sanitise_filename(filename))
    }

    /// A `PUT` URL the client uses directly. Signing is local: this makes no
    /// network call, so a storage outage does not block the request.
    pub async fn presign_put(
        &self,
        key: &str,
        content_type: &str,
        size_bytes: i64,
    ) -> Result<String, AppError> {
        let config = PresigningConfig::expires_in(self.presign_ttl)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("building presigning config: {e}")))?;
        let request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .content_length(size_bytes)
            .presigned(config)
            .await
            .map_err(|e| {
                AppError::Upstream(UpstreamError::Storage(format!(
                    "presigning put: {}",
                    DisplayErrorContext(&e)
                )))
            })?;
        Ok(request.uri().to_string())
    }

    /// Whether the object exists. Used before persisting an attachment row.
    pub async fn exists(&self, key: &str) -> Result<bool, AppError> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(err) => {
                let service = err.into_service_error();
                if service.is_not_found() {
                    Ok(false)
                } else {
                    Err(AppError::Upstream(UpstreamError::Storage(format!(
                        "head {key}: {service}"
                    ))))
                }
            }
        }
    }

    pub async fn delete(&self, key: &str) -> Result<(), AppError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                AppError::Upstream(UpstreamError::Storage(format!(
                    "delete {key}: {}",
                    DisplayErrorContext(&e)
                )))
            })?;
        Ok(())
    }

    /// Every key under `att/`, for the orphan sweep.
    ///
    /// Listing is a Class A operation and the free tier allows a million a
    /// month (RNF-09); once a day over a few thousand objects is nothing.
    pub async fn list_attachment_keys(&self) -> Result<Vec<StoredObject>, AppError> {
        let mut out = Vec::new();
        let mut continuation: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix("att/");
            if let Some(token) = &continuation {
                request = request.continuation_token(token);
            }
            let page = request.send().await.map_err(|e| {
                AppError::Upstream(UpstreamError::Storage(format!(
                    "list: {}",
                    DisplayErrorContext(&e)
                )))
            })?;

            for object in page.contents() {
                if let Some(key) = object.key() {
                    out.push(StoredObject {
                        key: key.to_owned(),
                        last_modified_secs: object
                            .last_modified()
                            .map(|t| t.secs())
                            .unwrap_or_default(),
                    });
                }
            }
            match page.next_continuation_token() {
                Some(token) if page.is_truncated().unwrap_or(false) => {
                    continuation = Some(token.to_owned());
                }
                _ => break,
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub key: String,
    /// Seconds since the epoch.
    pub last_modified_secs: i64,
}

/// Strips anything that could turn a filename into a path or a surprise.
///
/// The key is built by the server, but the filename comes from the client, and
/// `../../etc/passwd` in an object key is a bad habit even when the store is
/// flat.
fn sanitise_filename(filename: &str) -> String {
    let base = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(filename)
        .trim();
    let cleaned: String = base
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(120)
        .collect();
    let cleaned = cleaned.trim_matches('.').to_string();
    if cleaned.is_empty() {
        "arquivo".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filename_cannot_escape_its_prefix() {
        assert_eq!(sanitise_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitise_filename("C:\\Windows\\notas.txt"), "notas.txt");
        assert_eq!(sanitise_filename("captura.webp"), "captura.webp");
    }

    #[test]
    fn a_filename_of_only_punctuation_still_produces_a_key() {
        assert_eq!(sanitise_filename("..."), "arquivo");
        assert_eq!(sanitise_filename(""), "arquivo");
        assert_eq!(sanitise_filename("   "), "arquivo");
    }

    #[test]
    fn accents_and_spaces_are_handled_without_breaking_the_key() {
        // Nomes reais em português têm acento; o filtro mantém alfanumérico
        // unicode e descarta o resto, sem cortar no meio de um codepoint.
        assert_eq!(
            sanitise_filename("captura de tela.png"),
            "capturadetela.png"
        );
        assert_eq!(sanitise_filename("relatório.pdf"), "relatório.pdf");
    }

    #[test]
    fn the_key_carries_a_unique_prefix_so_two_uploads_never_collide() {
        let a = Storage::key_for("captura.webp");
        let b = Storage::key_for("captura.webp");
        assert_ne!(a, b);
        assert!(a.starts_with("att/"));
        assert!(a.ends_with("/captura.webp"));
    }
}
