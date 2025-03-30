use std::collections::BTreeMap;

use threshold_crypto::{Signature, SignatureShare};

use crate::base::{MessageId, NodeId, Step, Value, View};

// a 4-stage proof, while stage = 1, In == None and use External to validate
// otherwise use `Signature` of stage - 1 to validate
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Proof {
    External(String),

    In(Signature),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PromoteValue {
    pub value: Value,

    pub message_id: MessageId,
}

#[derive(Clone)]
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

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProofValue {
    pub id: NodeId,
    pub step: Step,
    pub value: PromoteValue,
}

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

#[derive(Clone, Eq, PartialOrd, Ord, PartialEq)]
pub struct MessageHash {
    pub node_id: NodeId,
    pub view: View,
    pub message_id: MessageId,
}
