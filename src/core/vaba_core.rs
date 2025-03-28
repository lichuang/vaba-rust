use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

use log::error;
use log::info;
use reqwest::Client;
use threshold_crypto::Signature;
use threshold_crypto::SignatureShare;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use crate::base::ClusterConfig;
use crate::base::Message;
use crate::base::MessageId;
use crate::base::NodeId;
use crate::base::PromoteAckMessage;
use crate::base::PromoteMessage;
use crate::base::ProposalMessage;
use crate::base::ProposalMessageResp;
use crate::base::Step;
use crate::base::Value;
use crate::base::View;
use crate::base::ID;
use crate::crypto::ThresholdSignatureScheme;
use anyhow::Result;
use futures::FutureExt;
use tokio::sync::mpsc::UnboundedReceiver;

use super::Metrics;
use super::PromoteData;
use super::PromoteValue;
use super::PromoteValueWithProof;
use super::Proof;
use super::ProofValue;
use super::WaitPromoteAck;

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
    WaitPromoteAck(WaitPromoteAck),
}

struct DeliverValue {
    key: Option<PromoteValueWithProof>,
    lock: Option<PromoteValueWithProof>,
    commit: Option<PromoteValueWithProof>,
}

pub struct VabaCore {
    node_id: NodeId,

    id: ID,

    threshold: usize,

    state: VabaState,

    promote_state: PromoteState,

    cluster: ClusterConfig,

    rx_api: UnboundedReceiver<Message>,

    rx_shutdown: oneshot::Receiver<()>,

    http_client: Client,

    proposal_values: Arc<Mutex<Vec<(MessageId, Value)>>>,

    proposal_sender: Arc<Mutex<BTreeMap<MessageId, oneshot::Sender<ProposalMessageResp>>>>,

    // save all the message ids have seen
    seen: BTreeSet<MessageId>,

    metrics: Metrics,

    // node id -> {view, DeliverValue}
    deliver: Arc<Mutex<BTreeMap<NodeId, BTreeMap<View, DeliverValue>>>>,

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
        node_id: NodeId,
        rx_api: UnboundedReceiver<Message>,
        rx_shutdown: oneshot::Receiver<()>,
        cluster: ClusterConfig,
    ) -> Self {
        let total = cluster.nodes.len();
        // threshold = 2 * f + 1, f = number of faulty nodes
        let threshold = 2 * (total as f32 / 3 as f32) as usize + 1;
        let core = Self {
            node_id,
            id: format!("node_{}", node_id),
            threshold,
            state: VabaState::default(),
            promote_state: PromoteState::Init,
            rx_api,
            rx_shutdown,
            http_client: Client::new(),
            cluster,
            proposal_values: Arc::new(Mutex::new(Vec::new())),
            proposal_sender: Arc::new(Mutex::new(BTreeMap::new())),
            seen: BTreeSet::new(),
            metrics: Metrics::new(),
            deliver: Arc::new(Mutex::new(BTreeMap::new())),
            threshold_signature: ThresholdSignatureScheme::new(threshold, total),
        };

        core
    }

    pub fn stop(&self) -> Result<()> {
        Ok(())
    }

    pub async fn main(mut self) -> Result<()> {
        self.main_loop().await
    }

    async fn main_loop(&mut self) -> Result<()> {
        loop {
            self.vaba_core().await?;

            futures::select_biased! {
                msg_res = self.rx_api.recv().fuse() => {
                    match msg_res {
                        Some(msg) => self.handle_message(msg).await?,
                        None => {
                            info!("all rx_api senders are dropped");
                            break;
                        }
                    };
                }
            }
        }
        Ok(())
    }

    async fn handle_message(&mut self, msg: Message) -> Result<()> {
        match msg {
            Message::Proposal(proposal) => self.handle_proposal_message(proposal).await?,
            Message::Promote(promote) => self.handle_promote_message(promote).await?,
            Message::PromoteAck(promote_ack) => {
                self.handle_promote_ack_message(promote_ack).await?
            }
        }
        Ok(())
    }

    async fn handle_proposal_message(&mut self, message: ProposalMessage) -> Result<()> {
        self.metrics.incr_recv_proposal();
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

    async fn handle_promote_message(&mut self, promote: PromoteMessage) -> Result<()> {
        self.metrics.incr_recv_promote();
        let from = &promote.node_id;
        let address = if let Some(address) = self.cluster.nodes.get(from) {
            address
        } else {
            return Ok(());
        };
        if self.seen.contains(&promote.value.message_id) {
            return Ok(());
        }
        if !self.external_provable_broadcast_validate(&promote)? {
            return Ok(());
        }

        self.seen.insert(promote.value.message_id);

        // calculate share sign of this node
        let proof_value = ProofValue {
            id: promote.node_id,
            // use last step signature as in proof
            step: promote.step - 1,
            value: promote.value.clone(),
        };
        let value_string = serde_json::to_string(&proof_value)?;
        let share_sign = self
            .threshold_signature
            .share_sign(self.node_id, &value_string)?;

        // deliver promote value
        self.deliver(&promote).await;

        let resp = PromoteAckMessage {
            node_id: self.node_id,
            message_id: promote.value.message_id,
            step: promote.step,
            share_sign,
        };
        let json = serde_json::to_string(&resp)?;

        let uri = format!("http://{}/promote_respp", address);
        let resp = self
            .http_client
            .post(uri)
            .header("Content-Type", "application/json")
            .body(json.clone())
            .send()
            .await;
        if let Err(e) = resp {
            error!(
                "send promote data to node {}/{} error: {:?}",
                from, address, e
            );
        }

        self.metrics.incr_send_promote_resp();

        Ok(())
    }

    async fn handle_promote_ack_message(&mut self, resp: PromoteAckMessage) -> Result<()> {
        let wait_promote_resp = if let PromoteState::WaitPromoteAck(wait) = &mut self.promote_state
        {
            wait
        } else {
            error!(
                "recv promote resp message {} from {} but not in WaitPromoteAck state",
                resp.message_id, resp.node_id
            );
            return Ok(());
        };

        if wait_promote_resp.step != resp.step {
            error!(
                "recv promote resp message {} from {} but not the same step, expected {}, actual {}",
                resp.message_id, resp.node_id, wait_promote_resp.step, resp.step,
            );
            return Ok(());
        }

        if wait_promote_resp.data.value.message_id != resp.message_id {
            error!(
                "recv promote resp message {} from {} but not the same message, expected message id {}",
                resp.message_id, resp.node_id, wait_promote_resp.data.value.message_id,
            );
            return Ok(());
        }

        wait_promote_resp
            .share_signs
            .insert(resp.node_id, resp.share_sign);

        // if not reach 2f+1 aggrement, just return
        if wait_promote_resp.share_signs.len() < self.threshold {
            return Ok(());
        }

        // now step has reach aggrement, move to the next step(step < 4) or leader election phase(step == 4)
        let share_signs: Vec<(NodeId, SignatureShare)> =
            wait_promote_resp.share_signs.clone().into_iter().collect();
        let signature = self.threshold_signature.threshold_sign(&share_signs)?;
        if resp.step == 4 {
        } else {
        }

        Ok(())
    }

    fn external_provable_broadcast_validate(&self, promote: &PromoteMessage) -> Result<bool> {
        let step = promote.step;

        // step = 1 case
        if step == 1 {
            if let Proof::External(external_proof) = &promote.proof {
                // for now external proof always validate true
                return Ok(true);
            } else {
                error!(
                    "step 1 MUST with external proof, message from {} id {}",
                    promote.node_id, promote.value.message_id
                );
                return Ok(false);
            }
        }

        // step > 1 cases
        let signature = if let Proof::In(signature) = &promote.proof {
            signature
        } else {
            error!(
                "step > 1 MUST with in proof of last step, message from {} id {}",
                promote.node_id, promote.value.message_id
            );
            return Ok(false);
        };
        let proof_value = ProofValue {
            id: promote.node_id,
            // use last step signature as in proof
            step: step - 1,
            value: promote.value.clone(),
        };
        let value_string = serde_json::to_string(&proof_value)?;
        let validate = self
            .threshold_signature
            .threshold_validate(&value_string, signature);

        Ok(validate)
    }

    async fn deliver(&self, promote: &PromoteMessage) {
        let step = promote.step;
        if step == 1 {
            return;
        }
        let from = promote.node_id;
        let view = promote.view;
        let signature = if let Proof::In(signature) = &promote.proof {
            signature
        } else {
            error!(
                "step > 1 MUST with in proof of last step, message from {} id {}",
                promote.node_id, promote.value.message_id
            );
            return;
        };
        let value_with_proof = PromoteValueWithProof {
            value: promote.value.value.clone(),
            message_id: promote.value.message_id.clone(),
            proof: signature.clone(),
        };

        info!(
            "node {} deliver step {} from node {} message_id {}",
            self.node_id, step, promote.node_id, promote.value.message_id
        );

        let mut deliver = self.deliver.lock().await;
        let mut node_view_deliver = if let Some(node_view_deliver) = deliver.get_mut(&from) {
            node_view_deliver
        } else {
            deliver.insert(from, BTreeMap::new());
            deliver.get_mut(&from).unwrap()
        };

        let mut deliver = if let Some(view_deliver) = node_view_deliver.get_mut(&view) {
            view_deliver
        } else {
            node_view_deliver.insert(view, DeliverValue::new());
            node_view_deliver.get_mut(&view).unwrap()
        };

        if step == 2 {
            deliver.key = Some(value_with_proof);
        } else if step == 3 {
            deliver.lock = Some(value_with_proof);
        } else {
            deliver.commit = Some(value_with_proof);
        }
    }

    // for now external proof always validate true
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
                    self.promote(1, data.clone()).await?;
                    break;
                }
                _ => break,
            }
        }
        Ok(())
    }

    async fn select_promote_data(&self) -> Option<PromoteData> {
        let current_view = self.state.current;
        let promote_value = if let Some(key) = &self.state.key {
            PromoteData {
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

    async fn promote(&mut self, step: Step, promote: PromoteData) -> Result<()> {
        let mut promote_resp = WaitPromoteAck {
            step,
            data: promote.clone(),
            share_signs: BTreeMap::new(),
        };
        // calculate share sign of this node
        let proof_value = ProofValue {
            id: self.node_id,
            // use last step signature as in proof
            step: step - 1,
            value: promote.value.clone(),
        };
        let value_string = serde_json::to_string(&proof_value)?;
        let share_sign = self
            .threshold_signature
            .share_sign(self.node_id, &value_string)?;
        promote_resp.share_signs.insert(self.node_id, share_sign);

        self.promote_state = PromoteState::WaitPromoteAck(promote_resp);
        let message = PromoteMessage {
            step,
            node_id: self.node_id,
            value: promote.value.clone(),
            view: promote.view,
            proof: promote.proof.clone(),
        };
        let json = serde_json::to_string(&message)?;
        for (node_id, address) in &self.cluster.nodes {
            let uri = format!("http://{}/promote", address);
            let resp = self
                .http_client
                .post(uri)
                .header("Content-Type", "application/json")
                .body(json.clone())
                .send()
                .await;
            if let Err(e) = resp {
                error!(
                    "send promote data to node {}/{} error: {:?}",
                    node_id, address, e
                );
            }

            self.metrics.incr_send_promote();
        }
        Ok(())
    }
}

impl DeliverValue {
    pub fn new() -> Self {
        Self {
            key: None,
            lock: None,
            commit: None,
        }
    }
}
