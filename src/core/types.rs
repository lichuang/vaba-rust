use std::collections::BTreeMap;

use threshold_crypto::{Signature, SignatureShare};

use crate::base::{MessageId, NodeId, Step, Value, View};

// a 4-stage proof,
// 1) if view == 1 and step == 1, proof = None
// 2) if view > 1:
//  2.1 if step = 1, proof = signature of view - 1 step 4;
//  2.2 else, proof = signature of this view step - 1
pub type Proof = Option<Signature>;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Stage {
    pub view: View,
    pub step: Step,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PromoteValue {
    pub value: Value,

    pub message_id: MessageId,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PromoteValueWithProof {
    pub value: Value,

    pub message_id: MessageId,

    pub proof: Signature,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PromoteData {
    pub value: PromoteValue,

    pub proof: Proof,

    pub view: View,
}

// proof value include: node `id` in `stage` promote `value`
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProofValue {
    pub id: NodeId,
    pub stage: Stage,
    pub value: PromoteValue,
}

#[derive(Debug)]
pub struct WaitAck {
    // promote step
    pub step: Step,

    pub data: PromoteData,

    // share sign return by the other nodes
    pub share_signs: BTreeMap<NodeId, SignatureShare>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProofSkipShare {
    pub id: NodeId,
    pub view: View,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProofCoinShare {
    pub node_id: NodeId,
    pub view: View,
}

#[derive(Clone, Eq, PartialOrd, Ord, PartialEq)]
pub struct MessageHash {
    pub node_id: NodeId,
    pub view: View,
    pub message_id: MessageId,
}
