use std::collections::BTreeMap;

use threshold_crypto::{Signature, SignatureShare};

use crate::base::{MessageId, NodeId, Step, Value, View};

// a 4-stage proof,
// 1) if step == 1, proof = None
// 2) if step > 1: proof = signature of this view step - 1
pub type Proof = Option<Signature>;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stage {
    pub view: View,
    pub step: Step,
}

// a promote value include: (node_id, value, message id)
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PromoteValue {
    pub node_id: NodeId,

    pub value: Value,

    pub message_id: MessageId,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PromoteValueWithProof {
    pub value: PromoteValue,

    pub proof: Signature,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PromoteData {
    pub value: PromoteValue,

    pub proof: Proof,

    pub view: View,
}

// proof value include: node `id` in `stage` promote `value`
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ProofValue {
    pub stage: Stage,
    pub value: PromoteValue,
}

#[derive(Debug)]
pub struct Ack {
    // promote stage
    pub stage: Stage,

    pub data: PromoteData,

    // share sign return by the other nodes
    pub share_signs: BTreeMap<MessageHash, SignatureShare>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProofSkipShare {
    pub id: NodeId,
    pub view: View,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProofCoinShare {
    //pub node_id: NodeId,
    pub view: View,
}

#[derive(Clone, Debug, Eq, PartialOrd, Ord, PartialEq)]
pub struct MessageHash {
    pub node_id: NodeId,
    pub view: View,
    pub message_id: MessageId,
}
