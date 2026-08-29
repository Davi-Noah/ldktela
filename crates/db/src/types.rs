//! Rust mirrors of the PostgreSQL enum types.
//!
//! These live here rather than in `protocol` because they carry `sqlx::Type`, and
//! `protocol` must not depend on `sqlx` (CLAUDE.md §3).

use protocol::channel::ChannelType as WireChannelType;
use protocol::guild::OverwriteTarget as WireOverwriteTarget;
use protocol::message::MessageOrigin as WireMessageOrigin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "channel_type", rename_all = "snake_case")]
pub enum ChannelType {
    Text,
    Voice,
    Dm,
    GroupDm,
}

impl ChannelType {
    /// Direct conversations short-circuit permission resolution at step 0.
    pub const fn is_direct(self) -> bool {
        matches!(self, Self::Dm | Self::GroupDm)
    }
}

impl From<ChannelType> for WireChannelType {
    fn from(value: ChannelType) -> Self {
        match value {
            ChannelType::Text => Self::Text,
            ChannelType::Voice => Self::Voice,
            ChannelType::Dm => Self::Dm,
            ChannelType::GroupDm => Self::GroupDm,
        }
    }
}

impl From<WireChannelType> for ChannelType {
    fn from(value: WireChannelType) -> Self {
        match value {
            WireChannelType::Text => Self::Text,
            WireChannelType::Voice => Self::Voice,
            WireChannelType::Dm => Self::Dm,
            WireChannelType::GroupDm => Self::GroupDm,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "overwrite_target", rename_all = "lowercase")]
pub enum OverwriteTarget {
    Role,
    Member,
}

impl From<OverwriteTarget> for WireOverwriteTarget {
    fn from(value: OverwriteTarget) -> Self {
        match value {
            OverwriteTarget::Role => Self::Role,
            OverwriteTarget::Member => Self::Member,
        }
    }
}

impl From<WireOverwriteTarget> for OverwriteTarget {
    fn from(value: WireOverwriteTarget) -> Self {
        match value {
            WireOverwriteTarget::Role => Self::Role,
            WireOverwriteTarget::Member => Self::Member,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "message_origin", rename_all = "lowercase")]
pub enum MessageOrigin {
    Internal,
    Discord,
}

impl From<MessageOrigin> for WireMessageOrigin {
    fn from(value: MessageOrigin) -> Self {
        match value {
            MessageOrigin::Internal => Self::Internal,
            MessageOrigin::Discord => Self::Discord,
        }
    }
}

impl From<WireMessageOrigin> for MessageOrigin {
    fn from(value: WireMessageOrigin) -> Self {
        match value {
            WireMessageOrigin::Internal => Self::Internal,
            WireMessageOrigin::Discord => Self::Discord,
        }
    }
}
