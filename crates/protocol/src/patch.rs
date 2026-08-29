//! Three-state fields for `PATCH` bodies.
//!
//! `docs/api/rest-api.md` §1: in a `PATCH`, an absent field means "do not
//! change" and an explicit `null` means "clear". `Option<T>` cannot express both,
//! and serde collapses `null` into `None` by default — hence the helper.

use serde::{Deserialize, Deserializer};

/// Absent, explicitly cleared, or set to a value.
pub type Patch<T> = Option<Option<T>>;

/// Deserialises `null` as `Some(None)` instead of `None`.
///
/// Use with `#[serde(default, deserialize_with = "crate::patch::double_option")]`.
pub fn double_option<'de, T, D>(deserializer: D) -> Result<Patch<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Body {
        #[serde(default, deserialize_with = "double_option")]
        bio: Patch<String>,
    }

    #[test]
    fn absent_null_and_value_are_three_distinct_states() {
        assert_eq!(
            serde_json::from_str::<Body>("{}").unwrap(),
            Body { bio: None },
            "campo ausente = não alterar"
        );
        assert_eq!(
            serde_json::from_str::<Body>(r#"{"bio":null}"#).unwrap(),
            Body { bio: Some(None) },
            "null = limpar"
        );
        assert_eq!(
            serde_json::from_str::<Body>(r#"{"bio":"oi"}"#).unwrap(),
            Body {
                bio: Some(Some("oi".into()))
            }
        );
    }
}
