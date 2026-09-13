//! Request id propagation.
//!
//! Every response carries `X-Request-Id`, every log line inside the request
//! carries it as a span field, and every error body echoes it
//! (`docs/api/rest-api.md` §1). An error report that cites the id can be found
//! in the log; one that cannot is untraceable.

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use tracing::Instrument;
use uuid::Uuid;

pub const HEADER: HeaderName = HeaderName::from_static("x-request-id");

tokio::task_local! {
    static CURRENT: String;
}

/// Access to the id of the request being served.
pub struct RequestId;

impl RequestId {
    /// The current request's id, or `None` outside a request.
    pub fn current() -> Option<String> {
        CURRENT.try_with(String::clone).ok()
    }
}

/// Accepts an inbound `X-Request-Id` only when it is short and alphanumeric.
///
/// The id ends up in log lines; an arbitrary client string there is a log
/// injection waiting to happen.
fn sanitise(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let acceptable = (8..=64).contains(&trimmed.len())
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    acceptable.then(|| trimmed.to_owned())
}

pub async fn propagate(request: Request, next: Next) -> Response {
    let inbound = request
        .headers()
        .get(HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(sanitise);
    let id = inbound.unwrap_or_else(|| Uuid::now_v7().simple().to_string());

    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let span = tracing::info_span!(
        "http",
        request_id = %id,
        method = %method,
        path = %path,
    );

    let started = std::time::Instant::now();
    let mut response = CURRENT
        .scope(id.clone(), next.run(request).instrument(span))
        .await;
    let elapsed_ms = started.elapsed().as_millis();
    let status = response.status().as_u16();

    // Uma linha por requisicao, sempre. Sem isto, uma chamada que o servidor
    // atende com sucesso nao deixa rastro nenhum, e diagnosticar do lado do
    // cliente vira adivinhacao: "nao aparece no log" passa a significar tanto
    // "nao chegou" quanto "chegou e deu certo".
    if status >= 500 {
        tracing::error!(%method, %path, status, elapsed_ms, request_id = %id, "requisição");
    } else if status >= 400 {
        tracing::warn!(%method, %path, status, elapsed_ms, request_id = %id, "requisição");
    } else {
        tracing::info!(%method, %path, status, elapsed_ms, request_id = %id, "requisição");
    }

    if let Ok(value) = HeaderValue::from_str(&id) {
        response.headers_mut().insert(HEADER, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_supplied_id_is_accepted_only_when_it_is_safe_to_log() {
        assert_eq!(sanitise("01J8XQABCDEF"), Some("01J8XQABCDEF".to_string()));
        assert_eq!(sanitise("with-dash_and_1"), Some("with-dash_and_1".into()));
        assert_eq!(sanitise("short"), None, "curto demais");
        assert_eq!(sanitise(&"a".repeat(65)), None, "longo demais");
        assert_eq!(
            sanitise("linha\nnova aqui"),
            None,
            "quebra de linha em log é injeção"
        );
        assert_eq!(sanitise("espaço no meio"), None);
    }

    #[test]
    fn outside_a_request_there_is_no_current_id() {
        assert_eq!(RequestId::current(), None);
    }

    #[tokio::test]
    async fn inside_the_scope_the_id_is_visible() {
        CURRENT
            .scope("abcdef123456".to_string(), async {
                assert_eq!(RequestId::current().as_deref(), Some("abcdef123456"));
            })
            .await;
    }
}
