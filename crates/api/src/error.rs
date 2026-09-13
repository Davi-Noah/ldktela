//! The single error type in every handler signature (CLAUDE.md §6).
//!
//! Handlers return `Result<T, AppError>`; nothing else. `Internal` never leaks a
//! detail to the client: the real message goes to the log with the `request_id`,
//! and the response carries only the code and that id.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use domain::validation::FieldError as DomainFieldError;
use protocol::error::{ErrorBody, ErrorCode, ErrorResponse, FieldError};

use crate::middleware::request_id::RequestId;

/// Upstream dependencies that can fail without it being our bug.
#[derive(Debug, thiserror::Error)]
pub enum UpstreamError {
    #[error("livekit: {0}")]
    LiveKit(String),
    #[error("discord: {0}")]
    Discord(String),
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unauthorized")]
    Unauthorized,
    /// The access token is well formed but expired. The client refreshes once
    /// and retries (`docs/rest-api.md` §2).
    #[error("token expired")]
    TokenExpired,
    /// A consumed refresh token was presented. The family is already revoked by
    /// the time this is returned.
    #[error("token reused")]
    TokenReused,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound { resource: &'static str },
    #[error("validation failed")]
    Validation(Vec<FieldError>),
    #[error("conflict")]
    Conflict { reason: &'static str },
    #[error("rate limited")]
    RateLimited { retry_after_ms: u64 },
    #[error("payload too large")]
    PayloadTooLarge,
    /// The room already holds the maximum number of publishers (RNF-05).
    #[error("room capacity")]
    RoomCapacity,
    /// The Discord replica is too far behind to vouch for access (RF-09).
    /// Failing closed here is the whole point: an admission granted from stale
    /// state can outlive the permission it was based on by hours.
    #[error("replica stale")]
    ReplicaStale,
    #[error("upstream failure")]
    Upstream(#[from] UpstreamError),
    #[error("internal")]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    /// Anything invisible answers `404`, never `403`: a `403` confirms the
    /// resource exists, which is enough to map a private server
    /// (`docs/rest-api.md` §3).
    pub const fn invisible(resource: &'static str) -> Self {
        Self::NotFound { resource }
    }

    pub const fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized | Self::TokenExpired | Self::TokenReused => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::Conflict { .. } | Self::RoomCapacity => StatusCode::CONFLICT,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            // 503 e nao 409: nao ha conflito de estado, o servico e que nao
            // pode responder com seguranca agora. O cliente deve tentar de novo.
            Self::ReplicaStale => StatusCode::SERVICE_UNAVAILABLE,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Unauthorized => ErrorCode::Unauthenticated,
            Self::TokenExpired => ErrorCode::TokenExpired,
            Self::TokenReused => ErrorCode::TokenReused,
            Self::Forbidden => ErrorCode::Forbidden,
            Self::NotFound { .. } => ErrorCode::NotFound,
            Self::Validation(_) => ErrorCode::ValidationFailed,
            Self::Conflict { .. } => ErrorCode::Conflict,
            Self::RateLimited { .. } => ErrorCode::RateLimited,
            Self::PayloadTooLarge => ErrorCode::PayloadTooLarge,
            Self::RoomCapacity => ErrorCode::RoomCapacity,
            Self::ReplicaStale => ErrorCode::ReplicaStale,
            Self::Upstream(_) => ErrorCode::UpstreamFailure,
            Self::Internal(_) => ErrorCode::Internal,
        }
    }

    /// Portuguese, displayable, and free of internal detail.
    fn message(&self) -> String {
        match self {
            Self::Unauthorized => "Autenticação necessária.".into(),
            Self::TokenExpired => "Sua sessão expirou. Renovando…".into(),
            Self::TokenReused => "Sua sessão foi encerrada por segurança. Entre novamente.".into(),
            Self::Forbidden => "Você não tem permissão para esta ação.".into(),
            Self::NotFound { .. } => "Não encontrado.".into(),
            Self::Validation(_) => "Alguns campos estão inválidos.".into(),
            Self::Conflict { .. } => "A operação conflita com o estado atual.".into(),
            Self::RateLimited { .. } => "Muitas requisições. Tente novamente em instantes.".into(),
            Self::PayloadTooLarge => "Requisição grande demais.".into(),
            Self::RoomCapacity => {
                "A sala já tem o número máximo de telas sendo compartilhadas.".into()
            }
            Self::ReplicaStale => {
                "Sem contato com o Discord no momento. Tente novamente em instantes.".into()
            }
            Self::Upstream(_) => "Um serviço externo falhou. Tente novamente.".into(),
            Self::Internal(_) => "Erro interno.".into(),
        }
    }

    fn details(&self) -> Option<Vec<FieldError>> {
        match self {
            Self::Validation(fields) => Some(fields.clone()),
            _ => None,
        }
    }
}

/// Validation failures from `domain` become wire field errors.
impl From<Vec<DomainFieldError>> for AppError {
    fn from(errors: Vec<DomainFieldError>) -> Self {
        Self::Validation(
            errors
                .into_iter()
                .map(|e| FieldError {
                    field: e.field.to_string(),
                    code: e.code.as_str().to_string(),
                })
                .collect(),
        )
    }
}

/// `db::DbError::NotFound` becomes a `404`; everything else is internal.
///
/// Note that this conversion never produces `Forbidden`: visibility is decided
/// by the route, before the repository is called.
impl From<db::DbError> for AppError {
    fn from(error: db::DbError) -> Self {
        match error {
            db::DbError::NotFound(resource) => Self::NotFound { resource },
            db::DbError::Conflict(reason) => Self::Conflict { reason },
            db::DbError::Sqlx(e) => Self::Internal(anyhow::Error::new(e)),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // The request id is put on the response by the middleware; here it is
        // only echoed into the body, so an operator can grep the log for it.
        let request_id = RequestId::current().unwrap_or_default();

        if let Self::Internal(err) = &self {
            tracing::error!(error = ?err, %request_id, "internal error");
        }
        if let Self::Upstream(err) = &self {
            tracing::warn!(error = %err, %request_id, "upstream failure");
        }

        let status = self.status();
        let retry_after = match &self {
            Self::RateLimited { retry_after_ms } => Some(retry_after_ms.div_ceil(1000).max(1)),
            _ => None,
        };

        let body = ErrorResponse::new(ErrorBody {
            code: self.code().as_str().to_string(),
            message: self.message(),
            details: self.details(),
            request_id: request_id.clone(),
        });

        let mut response = (status, Json(body)).into_response();
        if let Some(seconds) = retry_after {
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_resources_answer_404_never_403() {
        assert_eq!(
            AppError::invisible("channel").status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(AppError::invisible("channel").code(), ErrorCode::NotFound);
    }

    #[test]
    fn internal_errors_never_carry_their_cause_to_the_client() {
        let err = AppError::Internal(anyhow::anyhow!(
            "SELECT password_hash FROM users WHERE email = 'vitima@exemplo.test'"
        ));
        let message = err.message();
        assert_eq!(message, "Erro interno.");
        assert!(!message.contains("SELECT"));
        assert!(!message.contains("password_hash"));
    }

    #[test]
    fn upstream_errors_do_not_name_the_upstream() {
        let err = AppError::Upstream(UpstreamError::LiveKit(
            "401 from https://sfu.exemplo.internal/twirp".into(),
        ));
        assert!(!err.message().contains("sfu.exemplo.internal"));
        assert_eq!(err.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn every_status_matches_the_contract_table() {
        use AppError::*;
        let cases = [
            (Validation(vec![]), 400, ErrorCode::ValidationFailed),
            (Unauthorized, 401, ErrorCode::Unauthenticated),
            (TokenExpired, 401, ErrorCode::TokenExpired),
            (TokenReused, 401, ErrorCode::TokenReused),
            (Forbidden, 403, ErrorCode::Forbidden),
            (NotFound { resource: "x" }, 404, ErrorCode::NotFound),
            (Conflict { reason: "x" }, 409, ErrorCode::Conflict),
            (
                RateLimited { retry_after_ms: 1 },
                429,
                ErrorCode::RateLimited,
            ),
            (PayloadTooLarge, 413, ErrorCode::PayloadTooLarge),
            (RoomCapacity, 409, ErrorCode::RoomCapacity),
            (ReplicaStale, 503, ErrorCode::ReplicaStale),
        ];
        for (error, status, code) in cases {
            assert_eq!(error.status().as_u16(), status, "{error:?}");
            assert_eq!(error.code(), code, "{error:?}");
        }
    }

    #[test]
    fn domain_validation_errors_become_wire_field_errors() {
        use domain::validation::{FieldError as DField, ValidationCode};
        let err: AppError = vec![
            DField::new("username", ValidationCode::TooShort),
            DField::new("email", ValidationCode::InvalidFormat),
        ]
        .into();
        let details = err.details().expect("details on VALIDATION_FAILED");
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].field, "username");
        assert_eq!(details[0].code, "TOO_SHORT");
        assert_eq!(details[1].code, "INVALID_FORMAT");
    }
}
