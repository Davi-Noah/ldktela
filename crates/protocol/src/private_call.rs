use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::room::RoomTokenRequest;
use crate::user::UserSummary;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PrivateCallState {
    pub id: Uuid,
    pub owner: UserSummary,
    #[ts(optional)]
    pub guest: Option<UserSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PrivateCallCreateResponse {
    pub call: PrivateCallState,
    pub code: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct PrivateCallJoinRequest {
    pub code: String,
}

pub type PrivateCallTokenRequest = RoomTokenRequest;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PrivateCallJoined {
    pub call: PrivateCallState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PrivateCallEnded {
    pub call_id: Uuid,
}
