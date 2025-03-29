use threshold_crypto::SignatureShare;
use tokio::sync::oneshot;

use crate::core::{PromoteData, PromoteValue, Proof};

use super::{MessageId, NodeId, Step, Value, View, ID};

pub struct ProposalMessage {
    pub value: Value,
    pub sender: oneshot::Sender<ProposalMessageResp>,
    pub message_id: MessageId,
}

pub struct ProposalMessageResp {
    ok: bool,
    error: Option<String>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PromoteMessage {
    pub node_id: NodeId,

    pub step: Step,

    pub value: PromoteValue,

    pub proof: Proof,

    pub view: View,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PromoteMessageResp {
    pub node_id: NodeId,
    pub message_id: MessageId,
    pub share_sign: SignatureShare,
}
