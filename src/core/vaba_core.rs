use std::collections::BTreeMap;
use std::sync::Arc;

use log::info;
use threshold_crypto::Signature;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use crate::base::ClusterConfig;
use crate::base::MessageId;
use crate::base::NodeId;
use crate::base::ProposalMessage;
use crate::base::ProposalMessageResp;
use crate::base::Result;
use crate::base::Step;
use crate::base::Value;
use crate::base::View;
use crate::crypto::ThresholdSignatureScheme;
use tokio::sync::mpsc::UnboundedReceiver;

use super::PromoteData;
use super::PromoteValue;

struct KeyState {
    pub proof: Signature,
    pub value: PromoteValue,
}

struct VabaState {
    // the highest view number for which the party ever received
    // a view-change message that includes a lock that was delivered
    // in the Proposal-Promotion of the chosen leader of this view.
    pub lock: Option<View>,

    // The KEY variable stores the 3-tuple: view, proof and value,
    // derived from the maximum view for which the party ever
    // received a view-change message that includes a key
    // (composing of value and proof) that was delivered
    // in the ProposalPromotion of the chosen leader of this view.
    pub key: Option<KeyState>,

    // current view of VABA
    pub current: View,
}

// state machine state of promote value progress
enum PromoteState {
    Init,

    Promote(PromoteData),

    // step and step's PromoteData
    WaitPromoteResponse((Step, PromoteData)),
}

pub struct VabaCore {
    id: NodeId,

    state: VabaState,

    promote_state: PromoteState,

    cluster: ClusterConfig,

    rx_api: UnboundedReceiver<ProposalMessage>,

    rx_shutdown: oneshot::Receiver<()>,

    proposal_values: Arc<Mutex<Vec<(MessageId, Value)>>>,

    proposal_sender: Arc<Mutex<BTreeMap<MessageId, oneshot::Sender<ProposalMessageResp>>>>,

    threshold_signature: ThresholdSignatureScheme,
}

impl Default for VabaState {
    fn default() -> Self {
        Self {
            lock: None,
            key: None,
            current: 1,
        }
    }
}

impl VabaCore {
    pub fn new(
        id: NodeId,
        rx_api: UnboundedReceiver<ProposalMessage>,
        rx_shutdown: oneshot::Receiver<()>,
        cluster: ClusterConfig,
    ) -> Self {
        let total = cluster.nodes.len();
        let threshold = 2 * (total as f32 / 3 as f32) as usize + 1;
        let core = Self {
            id,
            state: VabaState::default(),
            promote_state: PromoteState::Init,
            rx_api,
            rx_shutdown,
            cluster,
            proposal_values: Arc::new(Mutex::new(Vec::new())),
            proposal_sender: Arc::new(Mutex::new(BTreeMap::new())),
            threshold_signature: ThresholdSignatureScheme::new(threshold, total),
        };

        core
    }

    pub fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn proposal(&self, message: ProposalMessage) -> Result<()> {
        {
            let mut proposal_values = self.proposal_values.lock().await;
            proposal_values.push((message.message_id, message.value.clone()));
        }
        {
            let mut proposal_sender = self.proposal_sender.lock().await;
            proposal_sender.insert(message.message_id, message.sender);
        }
        Ok(())
    }

    pub async fn main(mut self) -> Result<()> {
        Ok(())
    }

    fn external_proof_of_value(&self, value: &Value) -> String {
        value.clone()
    }

    // core of VABA algorithm
    async fn vaba_core(&mut self) -> Result<()> {
        loop {
            let view_state = &self.promote_state;
            match view_state {
                PromoteState::Init => {
                    // if there if no data to promote, break and wait proposal value
                    if !self.init_promote().await? {
                        break;
                    }
                }
                PromoteState::Promote(data) => {
                    self.promote_data(1, data.clone()).await?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn select_promote_data(&self) -> Option<PromoteData> {
        let current_view = self.state.current;
        let promote_value = if let Some(key) = &self.state.key {
            PromoteData {
                step: 1,
                value: key.value.clone(),
                proof: super::Proof::External(self.external_proof_of_value(&key.value.value)),
                view: current_view,
            }
        } else {
            let mut proposal_values = self.proposal_values.lock().await;
            if proposal_values.is_empty() {
                return None;
            }
            let value = proposal_values.remove(0);
            PromoteData {
                step: 1,
                proof: super::Proof::External(self.external_proof_of_value(&value.1)),
                value: PromoteValue {
                    value: value.1,
                    message_id: value.0,
                },
                view: current_view,
            }
        };

        return Some(promote_value);
    }

    // return false if no input values
    async fn init_promote(&mut self) -> Result<bool> {
        if let Some(promopt_data) = self.select_promote_data().await {
            info!(
                "node {} select promote data {:?} in view {}",
                self.id, promopt_data, self.state.current
            );
            self.promote_state = PromoteState::Promote(promopt_data);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    //
    async fn promote_data(&mut self, step: Step, promote_data: PromoteData) -> Result<()> {
        self.promote_state = PromoteState::WaitPromoteResponse((step, promote_data.clone()));
        Ok(())
    }
}
