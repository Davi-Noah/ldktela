//! Pure input validation.
//!
//! Length bounds come from the column widths in SRS §5.2; anything the
//! specification leaves open is decided here and recorded in `docs/DECISIONS.md`.
//! These functions never touch IO, so they are cheap enough to run before any
//! database round trip.

use std::fmt;

/// Why one field was rejected. Serialised as `details[].code` by the API layer
/// (`docs/api/rest-api.md` §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationCode {
    Required,
    TooShort,
    TooLong,
    TooMany,
    InvalidFormat,
    OutOfRange,
    NotAllowed,
}

impl ValidationCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "REQUIRED",
            Self::TooShort => "TOO_SHORT",
            Self::TooLong => "TOO_LONG",
            Self::TooMany => "TOO_MANY",
            Self::InvalidFormat => "INVALID_FORMAT",
            Self::OutOfRange => "OUT_OF_RANGE",
            Self::NotAllowed => "NOT_ALLOWED",
        }
    }
}

impl fmt::Display for ValidationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One rejected field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldError {
    pub field: &'static str,
    pub code: ValidationCode,
}

impl FieldError {
    pub const fn new(field: &'static str, code: ValidationCode) -> Self {
        Self { field, code }
    }
}

/// Accumulates field errors so one request reports every problem at once.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Validation {
    errors: Vec<FieldError>,
}

impl Validation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, field: &'static str, code: ValidationCode) {
        self.errors.push(FieldError::new(field, code));
    }

    pub fn check(&mut self, field: &'static str, outcome: Result<(), ValidationCode>) {
        if let Err(code) = outcome {
            self.push(field, code);
        }
    }

    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// `Ok(())` when nothing failed, otherwise every failure collected.
    pub fn finish(self) -> Result<(), Vec<FieldError>> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors)
        }
    }
}

// ---------------------------------------------------------------------------
// Limites
// ---------------------------------------------------------------------------

/// Bounds taken straight from the column widths of SRS §5.2, plus the two the
/// specification leaves open (message body and search query).
pub mod limits {
    pub const USERNAME_MIN: usize = 2;
    pub const USERNAME_MAX: usize = 32;
    pub const EMAIL_MAX: usize = 255;
    /// Not specified; chosen so Argon2id is not spent on trivially weak secrets.
    pub const PASSWORD_MIN: usize = 8;
    /// Argon2 accepts far more; the cap only stops a pathological request body.
    pub const PASSWORD_MAX: usize = 128;
    pub const DISPLAY_NAME_MAX: usize = 64;
    pub const BIO_MAX: usize = 500;
    pub const NICKNAME_MAX: usize = 64;
    pub const GUILD_NAME_MAX: usize = 100;
    pub const CATEGORY_NAME_MAX: usize = 100;
    pub const CHANNEL_NAME_MAX: usize = 100;
    pub const CHANNEL_TOPIC_MAX: usize = 1024;
    pub const ROLE_NAME_MAX: usize = 64;
    pub const INVITE_CODE_MAX: usize = 16;
    pub const EMOJI_MAX: usize = 32;
    /// `messages.content` is unbounded `TEXT`; a wire limit is still needed.
    pub const MESSAGE_CONTENT_MAX: usize = 4000;
    pub const SEARCH_QUERY_MIN: usize = 2;
    pub const SEARCH_QUERY_MAX: usize = 200;
    /// P-01 in SRS §10.1.
    pub const GROUP_DM_PARTICIPANTS_MAX: usize = 10;
    /// `docs/api/rest-api.md` §4.
    pub const PAGE_LIMIT_DEFAULT: u32 = 50;
    pub const PAGE_LIMIT_MAX: u32 = 100;
    pub const SEARCH_LIMIT_DEFAULT: u32 = 25;
}

// ---------------------------------------------------------------------------
// Validadores
// ---------------------------------------------------------------------------

/// Character count, not byte count: `VARCHAR(n)` in PostgreSQL counts characters.
fn char_len(value: &str) -> usize {
    value.chars().count()
}

/// Trims, then bounds the character count.
pub fn bounded(value: &str, min: usize, max: usize) -> Result<(), ValidationCode> {
    let len = char_len(value.trim());
    if len < min {
        return Err(if len == 0 {
            ValidationCode::Required
        } else {
            ValidationCode::TooShort
        });
    }
    if len > max {
        return Err(ValidationCode::TooLong);
    }
    Ok(())
}

/// Optional field: `None` and `Some("")` both mean "not set".
pub fn optional_max(value: Option<&str>, max: usize) -> Result<(), ValidationCode> {
    match value {
        None => Ok(()),
        Some(v) if v.trim().is_empty() => Ok(()),
        Some(v) if char_len(v.trim()) > max => Err(ValidationCode::TooLong),
        Some(_) => Ok(()),
    }
}

/// ASCII letters, digits, `_`, `.` and `-`, with at least one alphanumeric.
/// Not specified by the SRS; chosen so a username is safe in a mention and in a
/// Discord webhook `username` override.
pub fn username(value: &str) -> Result<(), ValidationCode> {
    bounded(value, limits::USERNAME_MIN, limits::USERNAME_MAX)?;
    let trimmed = value.trim();
    let allowed = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    let has_alnum = trimmed.chars().any(|c| c.is_ascii_alphanumeric());
    if allowed && has_alnum {
        Ok(())
    } else {
        Err(ValidationCode::InvalidFormat)
    }
}

/// Structural check only: one `@`, a non-empty local part, and a dotted domain.
/// Deliverability is proven by sending mail, not by a regex.
pub fn email(value: &str) -> Result<(), ValidationCode> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ValidationCode::Required);
    }
    if char_len(trimmed) > limits::EMAIL_MAX {
        return Err(ValidationCode::TooLong);
    }
    let mut parts = trimmed.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(ValidationCode::InvalidFormat);
    };
    let domain_ok = domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..");
    if local.is_empty() || !domain_ok || trimmed.contains(char::is_whitespace) {
        return Err(ValidationCode::InvalidFormat);
    }
    Ok(())
}

/// Password strength is deliberately shallow: this is a closed community of 10
/// to 30 people behind invite codes, and Argon2id carries the real cost.
pub fn password(value: &str) -> Result<(), ValidationCode> {
    let len = value.chars().count();
    if len < limits::PASSWORD_MIN {
        return Err(if len == 0 {
            ValidationCode::Required
        } else {
            ValidationCode::TooShort
        });
    }
    if len > limits::PASSWORD_MAX {
        return Err(ValidationCode::TooLong);
    }
    Ok(())
}

/// `#RRGGBB`, matching the `VARCHAR(7)` column.
pub fn hex_color(value: &str) -> Result<(), ValidationCode> {
    let bytes = value.as_bytes();
    if bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit) {
        Ok(())
    } else {
        Err(ValidationCode::InvalidFormat)
    }
}

/// A message needs either text or at least one attachment
/// (`docs/api/rest-api.md` §6.5).
pub fn message_body(content: &str, attachment_count: usize) -> Result<(), ValidationCode> {
    if char_len(content) > limits::MESSAGE_CONTENT_MAX {
        return Err(ValidationCode::TooLong);
    }
    if content.trim().is_empty() && attachment_count == 0 {
        return Err(ValidationCode::Required);
    }
    Ok(())
}

/// RF-11a: size and type are rejected before a presigned URL is issued.
pub fn attachment(
    size_bytes: i64,
    content_type: &str,
    max_bytes: i64,
    allowed_types: &[String],
) -> Result<(), ValidationCode> {
    if size_bytes <= 0 {
        return Err(ValidationCode::OutOfRange);
    }
    if size_bytes > max_bytes {
        return Err(ValidationCode::TooLong);
    }
    let normalised = content_type.trim().to_ascii_lowercase();
    if allowed_types
        .iter()
        .any(|t| t.eq_ignore_ascii_case(&normalised))
    {
        Ok(())
    } else {
        Err(ValidationCode::NotAllowed)
    }
}

/// RF-11a: at most `max` attachments in one message.
pub fn attachment_count(count: usize, max: usize) -> Result<(), ValidationCode> {
    if count > max {
        Err(ValidationCode::TooMany)
    } else {
        Ok(())
    }
}

/// Clamps a client-supplied page size into the contract's range.
pub fn page_limit(requested: Option<u32>, default: u32, max: u32) -> u32 {
    requested.unwrap_or(default).clamp(1, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_accepts_the_shapes_a_mention_can_carry() {
        assert!(username("gabriel").is_ok());
        assert!(username("gab.riel_01-x").is_ok());
        assert_eq!(username("a"), Err(ValidationCode::TooShort));
        assert_eq!(username(""), Err(ValidationCode::Required));
        assert_eq!(username(&"a".repeat(33)), Err(ValidationCode::TooLong));
        assert_eq!(username("com espaço"), Err(ValidationCode::InvalidFormat));
        assert_eq!(username("..."), Err(ValidationCode::InvalidFormat));
        assert_eq!(username("olá!"), Err(ValidationCode::InvalidFormat));
    }

    #[test]
    fn email_rejects_the_shapes_that_break_the_unique_index() {
        assert!(email("pessoa@exemplo.com.br").is_ok());
        assert_eq!(email(""), Err(ValidationCode::Required));
        assert_eq!(email("sem-arroba"), Err(ValidationCode::InvalidFormat));
        assert_eq!(email("@exemplo.com"), Err(ValidationCode::InvalidFormat));
        assert_eq!(email("a@b"), Err(ValidationCode::InvalidFormat));
        assert_eq!(email("a@@b.com"), Err(ValidationCode::InvalidFormat));
        assert_eq!(email("a b@c.com"), Err(ValidationCode::InvalidFormat));
    }

    #[test]
    fn message_body_requires_text_or_an_attachment() {
        assert!(message_body("oi", 0).is_ok());
        assert!(message_body("", 1).is_ok());
        assert_eq!(message_body("   ", 0), Err(ValidationCode::Required));
        assert_eq!(
            message_body(&"a".repeat(limits::MESSAGE_CONTENT_MAX + 1), 0),
            Err(ValidationCode::TooLong)
        );
    }

    #[test]
    fn attachment_enforces_rf11a_before_any_presign() {
        let allowed = vec!["image/webp".to_string(), "image/png".to_string()];
        assert!(attachment(1024, "image/webp", 26_214_400, &allowed).is_ok());
        assert!(attachment(1024, "IMAGE/WEBP", 26_214_400, &allowed).is_ok());
        assert_eq!(
            attachment(26_214_401, "image/webp", 26_214_400, &allowed),
            Err(ValidationCode::TooLong)
        );
        assert_eq!(
            attachment(1024, "application/x-msdownload", 26_214_400, &allowed),
            Err(ValidationCode::NotAllowed)
        );
        assert_eq!(
            attachment(0, "image/webp", 26_214_400, &allowed),
            Err(ValidationCode::OutOfRange)
        );
    }

    #[test]
    fn page_limit_clamps_into_the_contract_range() {
        assert_eq!(page_limit(None, 50, 100), 50);
        assert_eq!(page_limit(Some(0), 50, 100), 1);
        assert_eq!(page_limit(Some(500), 50, 100), 100);
        assert_eq!(page_limit(Some(25), 50, 100), 25);
    }

    #[test]
    fn validation_reports_every_failing_field_at_once() {
        let mut v = Validation::new();
        v.check("username", username("x"));
        v.check("email", email("nope"));
        v.check("password", password("123"));
        let errors = v.finish().unwrap_err();
        assert_eq!(errors.len(), 3);
        assert_eq!(errors[0].field, "username");
        assert_eq!(errors[1].code, ValidationCode::InvalidFormat);
    }

    #[test]
    fn hex_color_matches_the_varchar7_column() {
        assert!(hex_color("#5B8CFF").is_ok());
        assert!(hex_color("#5b8cff").is_ok());
        assert!(hex_color("5b8cff").is_err());
        assert!(hex_color("#5b8cf").is_err());
        assert!(hex_color("#5b8cfg").is_err());
    }
}
