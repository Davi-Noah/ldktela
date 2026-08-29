//! Access tokens (JWT) and refresh tokens (opaque, rotating).
//!
//! The access token is a 15-minute HS256 JWT carrying `sub` and `jti`
//! (`docs/api/rest-api.md` §2). The refresh token is **not** a JWT: it is 256
//! bits of randomness, stored as a SHA-256 digest, so a database dump does not
//! hand over live sessions.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use protocol::auth::AccessTokenClaims;
use rand::RngCore;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("token expired")]
    Expired,
    #[error("token invalid")]
    Invalid,
}

/// Signs an access token valid for `ttl_seconds`.
pub fn issue_access(
    signing_key: &[u8],
    user_id: Uuid,
    ttl_seconds: i64,
    now: OffsetDateTime,
) -> Result<(String, Uuid), TokenError> {
    let jti = Uuid::now_v7();
    let claims = AccessTokenClaims {
        sub: user_id.to_string(),
        jti: jti.to_string(),
        iat: now.unix_timestamp(),
        exp: now.unix_timestamp() + ttl_seconds,
    };
    let token = jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(signing_key),
    )
    .map_err(|_| TokenError::Invalid)?;
    Ok((token, jti))
}

/// Verifies signature and expiry, and returns the claims.
pub fn verify_access(signing_key: &[u8], token: &str) -> Result<AccessTokenClaims, TokenError> {
    let mut validation = Validation::default();
    validation.set_required_spec_claims(&["exp", "sub"]);
    validation.leeway = 5;
    jsonwebtoken::decode::<AccessTokenClaims>(
        token,
        &DecodingKey::from_secret(signing_key),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|e| match e.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => TokenError::Expired,
        _ => TokenError::Invalid,
    })
}

/// A fresh opaque refresh token: the secret to hand to the client, and the
/// digest to store.
pub struct RefreshToken {
    /// Given to the client once, never stored.
    pub secret: String,
    /// 64 hex characters, matching `refresh_tokens.token_hash CHAR(64)`.
    pub hash: String,
}

pub fn issue_refresh() -> RefreshToken {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let secret = URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_refresh(&secret);
    RefreshToken { secret, hash }
}

/// SHA-256, hex, lowercase.
///
/// A refresh token is 256 bits of uniform randomness, so it needs a fast digest,
/// not a password KDF: there is no dictionary to attack, and Argon2 here would
/// add ~100 ms to every refresh for nothing.
pub fn hash_refresh(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    const KEY: &[u8] = b"dev-only-not-a-real-signing-key-0123456789";

    #[test]
    fn an_access_token_round_trips_with_its_claims() {
        let user = Uuid::now_v7();
        let now = OffsetDateTime::now_utc();
        let (token, jti) = issue_access(KEY, user, 900, now).unwrap();
        let claims = verify_access(KEY, &token).unwrap();
        assert_eq!(claims.sub, user.to_string());
        assert_eq!(claims.jti, jti.to_string());
        assert_eq!(claims.exp - claims.iat, 900);
    }

    #[test]
    fn a_token_signed_with_another_key_is_invalid() {
        let (token, _) = issue_access(KEY, Uuid::now_v7(), 900, OffsetDateTime::now_utc()).unwrap();
        assert!(matches!(
            verify_access(b"outra-chave-completamente-diferente", &token),
            Err(TokenError::Invalid)
        ));
    }

    #[test]
    fn an_expired_token_is_distinguishable_from_an_invalid_one() {
        // O cliente renova uma vez em TOKEN_EXPIRED e desloga em UNAUTHENTICATED;
        // confundir os dois transforma renovação em logout.
        let long_ago = OffsetDateTime::now_utc() - Duration::hours(2);
        let (token, _) = issue_access(KEY, Uuid::now_v7(), 900, long_ago).unwrap();
        assert!(matches!(
            verify_access(KEY, &token),
            Err(TokenError::Expired)
        ));
    }

    #[test]
    fn a_tampered_token_is_invalid() {
        let (token, _) = issue_access(KEY, Uuid::now_v7(), 900, OffsetDateTime::now_utc()).unwrap();
        let mut parts: Vec<&str> = token.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(
            br#"{"sub":"00000000-0000-0000-0000-000000000000","jti":"x","iat":0,"exp":9999999999}"#,
        );
        parts[1] = &forged;
        assert!(verify_access(KEY, &parts.join(".")).is_err());
    }

    #[test]
    fn refresh_tokens_are_unique_and_hash_to_64_hex_characters() {
        let a = issue_refresh();
        let b = issue_refresh();
        assert_ne!(a.secret, b.secret);
        assert_ne!(a.hash, b.hash);
        assert_eq!(a.hash.len(), 64);
        assert!(a.hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash_refresh(&a.secret), a.hash, "o hash é determinístico");
        assert_ne!(
            a.hash, a.secret,
            "o segredo nunca pode ser igual ao que fica no banco"
        );
    }
}
