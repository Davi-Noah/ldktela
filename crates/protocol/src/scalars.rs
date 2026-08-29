//! Scalar wire types shared by every DTO.
//!
//! Two of them exist because the obvious representation is wrong on the wire:
//!
//! * `Timestamp` — RFC 3339 with offset (`docs/api/rest-api.md` §1), which is not
//!   what `time::OffsetDateTime` serialises to by default.
//! * `PermissionMask` — a **decimal string**. A 63-bit `BIGINT` does not survive
//!   `Number` in JavaScript, which loses precision above 2^53
//!   (`docs/api/rest-api.md` §6.4).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use ts_rs::TS;

/// An instant on the wire, always RFC 3339 with offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, TS)]
#[ts(export, type = "string")]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    pub const fn new(value: OffsetDateTime) -> Self {
        Self(value)
    }

    pub const fn into_inner(self) -> OffsetDateTime {
        self.0
    }
}

impl From<OffsetDateTime> for Timestamp {
    fn from(value: OffsetDateTime) -> Self {
        Self(value)
    }
}

impl From<Timestamp> for OffsetDateTime {
    fn from(value: Timestamp) -> Self {
        value.0
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let text = self
            .0
            .format(&Rfc3339)
            .map_err(|e| serde::ser::Error::custom(e.to_string()))?;
        serializer.serialize_str(&text)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        OffsetDateTime::parse(&text, &Rfc3339)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

/// A permission bitmask on the wire: decimal digits in a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, TS)]
#[ts(export, type = "string")]
pub struct PermissionMask(i64);

impl PermissionMask {
    pub const fn new(bits: i64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> i64 {
        self.0
    }
}

impl Serialize for PermissionMask {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for PermissionMask {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse::<i64>()
            .map(Self)
            .map_err(|_| serde::de::Error::custom("máscara de permissão inválida"))
    }
}

/// A Discord snowflake. `BIGINT` in the schema, decimal string on the wire for
/// the same precision reason as `PermissionMask`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, TS)]
#[ts(export, type = "string")]
pub struct Snowflake(i64);

impl Snowflake {
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Serialize for Snowflake {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Snowflake {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse::<i64>()
            .map(Self)
            .map_err(|_| serde::de::Error::custom("snowflake inválido"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn timestamp_serialises_as_rfc3339() {
        let ts = Timestamp::new(datetime!(2026-08-29 14:03:22.481 UTC));
        let json = serde_json::to_string(&ts).unwrap();
        assert_eq!(json, "\"2026-08-29T14:03:22.481Z\"");
        let back: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ts);
    }

    #[test]
    fn permission_mask_never_serialises_as_a_number() {
        // 2^53 + 1 perde precisao em Number; precisa sair como string.
        let mask = PermissionMask::new(9_007_199_254_740_993);
        assert_eq!(
            serde_json::to_string(&mask).unwrap(),
            "\"9007199254740993\""
        );
        let back: PermissionMask = serde_json::from_str("\"9007199254740993\"").unwrap();
        assert_eq!(back, mask);
    }

    #[test]
    fn permission_mask_rejects_a_json_number() {
        assert!(serde_json::from_str::<PermissionMask>("384").is_err());
    }

    #[test]
    fn snowflake_round_trips_as_a_string() {
        let id = Snowflake::new(1_234_567_890_123_456_789);
        assert_eq!(
            serde_json::to_string(&id).unwrap(),
            "\"1234567890123456789\""
        );
        assert_eq!(
            serde_json::from_str::<Snowflake>("\"1234567890123456789\"").unwrap(),
            id
        );
    }
}
