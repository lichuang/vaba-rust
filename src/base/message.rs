use threshold_crypto::SignatureShare;
use tokio::sync::oneshot;

use crate::core::PromoteData;

use super::{MessageId, Value, ID};

pub struct ProposalMessage {
    pub value: Value,
    pub sender: oneshot::Sender<ProposalMessageResp>,
    pub message_id: MessageId,
}

pub struct ProposalMessageResp {
    ok: bool,
    error: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PromoteMessage {
    pub id: ID,
    pub data: PromoteData,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PromoteMessageResp {
    pub id: ID,
    pub share_sign: SignatureShare,
}
