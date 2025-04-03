use threshold_crypto::{Signature, SignatureShare};
use tokio::sync::oneshot;

use crate::{
    core::{IdempotentId, Metrics, PromoteValue, PromoteValueWithProof, Proof, Stage},
    crypto::CoinShare,
};

use super::{MessageId, NodeId, Value, View};

// message between http handlers and vaba core
pub enum Message {
    Proposal(ProposalMessage),

    Promote(PromoteMessage),

    Ack(AckMessage),

    Done(DoneMessage),

    SkipShare(SkipShareMessage),

    Skip(SkipMessage),

    Share(ShareMessage),

    ViewChange(ViewChange),

    Metrics(MetricsMessage),

    Response(RespMessage),
}

impl Message {
    pub fn idempotent_id_and_node_id(&self) -> Option<(IdempotentId, NodeId)> {
        match self {
            Message::Promote(msg) => Some((msg.idempotent_id, msg.value.node_id)),
            Message::Ack(msg) => Some((msg.idempotent_id, msg.node_id)),
            Message::Done(msg) => Some((msg.idempotent_id, msg.node_id)),
            Message::SkipShare(msg) => Some((msg.idempotent_id, msg.node_id)),
            Message::Skip(msg) => Some((msg.idempotent_id, msg.node_id)),
            Message::Share(msg) => Some((msg.idempotent_id, msg.node_id)),
            Message::ViewChange(msg) => Some((msg.idempotent_id, msg.node_id)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RespMessage {
    pub id: IdempotentId,
    pub node_id: NodeId,
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
    //pub from: NodeId,
    pub stage: Stage,

    pub value: PromoteValue,

    pub proof: Proof,

    pub idempotent_id: IdempotentId,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AckMessage {
    pub from: NodeId,
    pub node_id: NodeId,
    pub stage: Stage,
    pub message_id: MessageId,
    pub share_sign: SignatureShare,

    pub idempotent_id: IdempotentId,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DoneMessage {
    pub node_id: NodeId,

    pub value: PromoteValue,

    pub proof: Signature,

    pub view: View,

    pub idempotent_id: IdempotentId,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SkipShareMessage {
    pub node_id: NodeId,

    pub from: NodeId,

    pub message_id: MessageId,

    pub share_proof: SignatureShare,

    pub view: View,

    pub idempotent_id: IdempotentId,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SkipMessage {
    pub node_id: NodeId,

    pub from: NodeId,

    pub view: View,

    pub message_id: MessageId,

    pub proof: Signature,

    pub idempotent_id: IdempotentId,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ShareMessage {
    pub node_id: NodeId,

    pub view: View,

    pub message_id: MessageId,

    pub proof: CoinShare,

    pub idempotent_id: IdempotentId,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ViewChange {
    pub node_id: NodeId,
    pub leader_id: NodeId,

    pub view: View,

    pub message_id: MessageId,

    pub key: Option<PromoteValueWithProof>,

    pub lock: Option<PromoteValueWithProof>,

    pub commit: Option<PromoteValueWithProof>,

    pub idempotent_id: IdempotentId,
}

pub struct MetricsMessage {
    pub sender: oneshot::Sender<MetricsMessageResp>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MetricsMessageResp {
    pub metrics: Metrics,
}
