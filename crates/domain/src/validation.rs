//! Pure input validation.
//!
//! There is very little left to validate: the product accepts almost nothing
//! from the client. Identity arrives as a pairing code, everything else is a
//! Discord snowflake parsed by `protocol::Snowflake`, and there is no free text
//! anywhere in the surface.

use std::fmt;

/// Why one field was rejected. Serialised as `details[].code` by the API layer
/// (`docs/rest-api.md` §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationCode {
    Required,
    TooShort,
    TooLong,
    InvalidFormat,
}

impl ValidationCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "REQUIRED",
            Self::TooShort => "TOO_SHORT",
            Self::TooLong => "TOO_LONG",
            Self::InvalidFormat => "INVALID_FORMAT",
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
// Codigo de pareamento
// ---------------------------------------------------------------------------

/// Length of a pairing code, in characters.
pub const PAIRING_CODE_LEN: usize = 8;

/// The alphabet a pairing code is drawn from.
///
/// Digits 0 and 1 and the letters I, L and O are absent on purpose: the code is
/// read off a Discord message and typed into another window by hand, and those
/// five are where transcription goes wrong. 31 symbols over 8 positions is about
/// 8.5e11 combinations, which is ample for a single-use secret that lives five
/// minutes behind an attempt limit.
pub const PAIRING_CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// Shape check only. Whether the code *exists* is a database question, and the
/// answer to that one is deliberately indistinguishable from "expired" and
/// "already used" (RF-01).
pub fn pairing_code(value: &str) -> Result<(), ValidationCode> {
    if value.is_empty() {
        return Err(ValidationCode::Required);
    }
    let len = value.chars().count();
    if len < PAIRING_CODE_LEN {
        return Err(ValidationCode::TooShort);
    }
    if len > PAIRING_CODE_LEN {
        return Err(ValidationCode::TooLong);
    }
    if !value.bytes().all(|b| PAIRING_CODE_ALPHABET.contains(&b)) {
        return Err(ValidationCode::InvalidFormat);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_well_formed_code() {
        assert_eq!(pairing_code("ABCD2345"), Ok(()));
    }

    #[test]
    fn rejects_the_ambiguous_characters() {
        // Se estes passassem, o usuario digitaria O por 0 e receberia "codigo
        // invalido" sem entender por que.
        for code in ["ABCD234O", "ABCD2340", "ABCD234I", "ABCD234L", "ABCD2341"] {
            assert_eq!(
                pairing_code(code),
                Err(ValidationCode::InvalidFormat),
                "{code} deveria ser recusado"
            );
        }
    }

    #[test]
    fn rejects_lowercase() {
        assert_eq!(pairing_code("abcd2345"), Err(ValidationCode::InvalidFormat));
    }

    #[test]
    fn reports_length_before_format() {
        assert_eq!(pairing_code(""), Err(ValidationCode::Required));
        assert_eq!(pairing_code("ABC"), Err(ValidationCode::TooShort));
        assert_eq!(pairing_code("ABCD23456"), Err(ValidationCode::TooLong));
    }

    #[test]
    fn alphabet_has_no_ambiguous_symbol() {
        for bad in b"01ILO" {
            assert!(
                !PAIRING_CODE_ALPHABET.contains(bad),
                "{} nao deveria estar no alfabeto",
                *bad as char
            );
        }
    }

    #[test]
    fn validation_collects_every_failure() {
        let mut v = Validation::new();
        v.check("code", pairing_code("abc"));
        v.push("other", ValidationCode::Required);
        let errors = v.finish().unwrap_err();
        assert_eq!(errors.len(), 2);
    }
}
