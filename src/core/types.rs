use threshold_crypto::Signature;

use crate::base::{MessageId, Step, Value, View};

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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PromoteData {
    pub value: PromoteValue,

    pub proof: Proof,

    pub view: View,
}
