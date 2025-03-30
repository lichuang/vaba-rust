use threshold_crypto::{Signature, SignatureShare};
use tokio::sync::oneshot;

use crate::core::{PromoteValue, Proof};

use super::{MessageId, NodeId, Step, Value, View};

// message between http handlers and vaba core
pub enum Message {
    Proposal(ProposalMessage),

    Promote(PromoteMessage),

    Ack(AckMessage),

    Done(DoneMessage),

    SkipShare(SkipShareMessage),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ClientProposalMessage {
    pub message_id: MessageId,
    pub value: Value,
}

pub struct ProposalMessage {
    pub value: Value,
    pub sender: oneshot::Sender<ProposalMessageResp>,
    pub message_id: MessageId,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalMessageResp {
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PromoteMessage {
    pub node_id: NodeId,

    pub step: Step,

    pub value: PromoteValue,

    pub proof: Proof,

    pub view: View,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AckMessage {
    pub node_id: NodeId,
    pub step: Step,
    pub message_id: MessageId,
    pub share_sign: SignatureShare,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DoneMessage {
    pub node_id: NodeId,

    pub value: PromoteValue,

    pub proof: Signature,

    pub view: View,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SkipShareMessage {
    pub node_id: NodeId,

    pub message_id: MessageId,

    pub share_proof: SignatureShare,

    pub view: View,
}
