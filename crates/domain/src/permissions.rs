//! Permission bitmask. Transcribed from SRS §5.3.
//!
//! The mask is 63 bits stored in a PostgreSQL `BIGINT`. Bits from 20 upward are
//! reserved for future expansion and are never set by this crate.
//!
//! On the wire the mask travels as a **decimal string**, never as a number:
//! `Number` in JavaScript loses precision above 2^53 (`docs/api/rest-api.md` §6.4).

use std::fmt;
use std::ops::{BitAnd, BitOr, BitOrAssign, Not};

/// A set of permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Permissions(i64);

macro_rules! define_permissions {
    ($($bit:expr => $name:ident),* $(,)?) => {
        impl Permissions {
            $(pub const $name: Self = Self(1 << $bit);)*

            /// Every permission this version of the protocol defines.
            ///
            /// Deliberately not `i64::MAX`: reserved bits (20..63) are not
            /// permissions yet, so `ADMINISTRATOR` must not grant them.
            pub const ALL: Self = Self($(  (1i64 << $bit) |)* 0);

            /// `(bit, name)` for every defined permission, ascending by bit.
            pub const NAMES: &'static [(u8, &'static str)] = &[
                $(($bit, stringify!($name)),)*
            ];
        }
    };
}

define_permissions! {
    0  => ADMINISTRATOR,
    1  => MANAGE_GUILD,
    2  => MANAGE_ROLES,
    3  => MANAGE_CHANNELS,
    4  => KICK_MEMBERS,
    5  => BAN_MEMBERS,
    6  => CREATE_INVITE,
    7  => VIEW_CHANNEL,
    8  => SEND_MESSAGES,
    9  => MANAGE_MESSAGES,
    10 => ATTACH_FILES,
    11 => EMBED_LINKS,
    12 => ADD_REACTIONS,
    13 => MENTION_EVERYONE,
    14 => CONNECT_VOICE,
    15 => SPEAK,
    16 => VIDEO,
    17 => SCREEN_SHARE,
    18 => MUTE_MEMBERS,
    19 => MOVE_MEMBERS,
}

impl Permissions {
    pub const NONE: Self = Self(0);

    /// The fixed grant a direct-message participant receives (SRS §5.3, step 0).
    pub const DIRECT_MESSAGE: Self = Self(
        Self::VIEW_CHANNEL.0
            | Self::SEND_MESSAGES.0
            | Self::ATTACH_FILES.0
            | Self::EMBED_LINKS.0
            | Self::ADD_REACTIONS.0
            | Self::CONNECT_VOICE.0
            | Self::SPEAK.0
            | Self::VIDEO.0
            | Self::SCREEN_SHARE.0,
    );

    /// Wraps a raw `BIGINT` read from the database.
    ///
    /// Bits outside the defined range are dropped: a stale row must not grant a
    /// permission this build does not know how to check.
    pub const fn from_bits_truncate(bits: i64) -> Self {
        Self(bits & Self::ALL.0)
    }

    /// The raw value, for persistence and for the decimal string on the wire.
    pub const fn bits(self) -> i64 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True when `self` holds every bit in `other`.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// True when `self` and `other` share at least one bit.
    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    /// `self` without the bits in `other`.
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// The `(base & ~deny) | allow` step that SRS §5.3 applies three times.
    pub const fn apply_overwrite(self, allow: Self, deny: Self) -> Self {
        Self((self.0 & !deny.0) | allow.0)
    }

    /// Names of the set bits, for logs and assertion messages.
    pub fn names(self) -> Vec<&'static str> {
        Self::NAMES
            .iter()
            .filter(|(bit, _)| self.0 & (1 << bit) != 0)
            .map(|(_, name)| *name)
            .collect()
    }
}

impl BitOr for Permissions {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Permissions {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for Permissions {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl Not for Permissions {
    type Output = Self;
    /// Complement within the defined range only.
    fn not(self) -> Self {
        Self(!self.0 & Self::ALL.0)
    }
}

impl fmt::Display for Permissions {
    /// Decimal string: the wire representation required by rest-api.md §6.4.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A permission mask string that is not a valid `BIGINT`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("máscara de permissão inválida")]
pub struct ParsePermissionsError;

impl std::str::FromStr for Permissions {
    type Err = ParsePermissionsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw: i64 = s.parse().map_err(|_| ParsePermissionsError)?;
        if raw < 0 {
            return Err(ParsePermissionsError);
        }
        Ok(Self::from_bits_truncate(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_covers_exactly_the_twenty_defined_bits() {
        assert_eq!(Permissions::NAMES.len(), 20);
        assert_eq!(Permissions::ALL.bits(), (1 << 20) - 1);
        // Bits reservados (20..63) nunca são concedidos por ALL.
        assert_eq!(Permissions::ALL.bits() & !((1 << 20) - 1), 0);
    }

    #[test]
    fn direct_message_grant_matches_the_srs_step_zero_list() {
        assert_eq!(
            Permissions::DIRECT_MESSAGE.names(),
            vec![
                "VIEW_CHANNEL",
                "SEND_MESSAGES",
                "ATTACH_FILES",
                "EMBED_LINKS",
                "ADD_REACTIONS",
                "CONNECT_VOICE",
                "SPEAK",
                "VIDEO",
                "SCREEN_SHARE",
            ]
        );
        // O passo 0 não concede moderação nem gestão.
        assert!(!Permissions::DIRECT_MESSAGE.contains(Permissions::MANAGE_MESSAGES));
        assert!(!Permissions::DIRECT_MESSAGE.contains(Permissions::MENTION_EVERYONE));
    }

    #[test]
    fn unknown_bits_from_the_database_are_dropped() {
        let stale = Permissions::from_bits_truncate((1 << 42) | Permissions::VIEW_CHANNEL.bits());
        assert_eq!(stale, Permissions::VIEW_CHANNEL);
    }

    #[test]
    fn masks_round_trip_through_the_decimal_string_wire_format() {
        let mask = Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
        assert_eq!(mask.to_string(), "384");
        assert_eq!("384".parse::<Permissions>().unwrap(), mask);
        assert!("-1".parse::<Permissions>().is_err());
        assert!("abc".parse::<Permissions>().is_err());
    }

    #[test]
    fn apply_overwrite_denies_before_it_allows() {
        let base = Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
        // allow vence deny quando os dois carregam o mesmo bit.
        let result = base.apply_overwrite(Permissions::SEND_MESSAGES, Permissions::SEND_MESSAGES);
        assert!(result.contains(Permissions::SEND_MESSAGES));
    }
}
