use threshold_crypto::{Signature, SignatureShare};
use tokio::sync::oneshot;

use crate::{
    core::{PromoteValue, PromoteValueWithProof, Proof},
    crypto::CoinShare,
};

use super::{MessageId, NodeId, Step, Value, View};

// message between http handlers and vaba core
pub enum Message {
    Proposal(ProposalMessage),

    Promote(PromoteMessage),

    Ack(AckMessage),

    Done(DoneMessage),

    SkipShare(SkipShareMessage),

    Skip(SkipMessage),

    Share(ShareMessage),

    ViewChangeMessage(ViewChangeMessage),
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SkipMessage {
    pub node_id: NodeId,

    pub view: View,

    pub message_id: MessageId,

    pub proof: Signature,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ShareMessage {
    pub node_id: NodeId,

    pub view: View,

    pub message_id: MessageId,

    pub proof: CoinShare,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ViewChangeMessage {
    pub node_id: NodeId,
    pub leader_id: NodeId,

    pub view: View,

    pub message_id: MessageId,

    pub key: Option<PromoteValueWithProof>,

    pub lock: Option<PromoteValueWithProof>,

    pub commit: Option<PromoteValueWithProof>,
}
