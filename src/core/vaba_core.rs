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

use crate::base::AckMessage;
use crate::base::ClusterConfig;
use crate::base::DoneMessage;
use crate::base::Message;
use crate::base::MessageId;
use crate::base::NodeId;
use crate::base::PromoteMessage;
use crate::base::ProposalMessage;
use crate::base::ProposalMessageResp;
use crate::base::SkipMessage;
use crate::base::SkipShareMessage;
use crate::base::Step;
use crate::base::Value;
use crate::base::View;
use crate::base::ID;
use crate::crypto::ThresholdCoinTossing;
use crate::crypto::ThresholdSignatureScheme;
use anyhow::Result;
use futures::FutureExt;
use tokio::sync::mpsc::UnboundedReceiver;

use super::MessageHash;
use super::Metrics;
use super::PromoteData;
use super::PromoteValue;
use super::PromoteValueWithProof;
use super::Proof;
use super::ProofCoinShare;
use super::ProofSkipShare;
use super::ProofValue;
use super::WaitAck;

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

    // 4 step promote data
    Promote(PromoteData),

    // wait promote with 2f + 1 ack return
    WaitAck(WaitAck),

    // promote success, notify all parties DONE
    Done((View, PromoteValue, Signature)),

    // wait until skip[view] is true
    WaitSkip(View),

    // election leader
    ElectionLeader(BTreeMap<NodeId, SignatureShare>),
}

struct DeliverValue {
    key: Option<PromoteValueWithProof>,
    lock: Option<PromoteValueWithProof>,
    commit: Option<PromoteValueWithProof>,
}

pub struct VabaCore {
    node_id: NodeId,

    id: ID,

    // threshold = 2 * faulty + 1
    threshold: usize,
    faulty: usize,

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

    // if has skip view
    skip: BTreeSet<View>,

    // number of view send `DONE` message
    broadcast_done: BTreeMap<View, u16>,

    // number of view has recv validate `SKIP-SHARE` message
    broadcast_skip: BTreeMap<View, u16>,

    // if recv `DONE` message id  in view from node id
    recv_done: BTreeSet<MessageHash>,

    // if recv `SKIP-SHARE` message id  in view from node id
    recv_skip_share: BTreeSet<MessageHash>,

    // if send `SKIP-SHARE` message in view
    send_skip_share: BTreeSet<View>,

    // if send `SKIP` message in view
    send_skip: BTreeSet<View>,

    // view share signs vector
    skip_share_signs: BTreeMap<View, Vec<(NodeId, SignatureShare)>>,

    // true if stop receiving promote value in current view
    stop: bool,

    threshold_signature: ThresholdSignatureScheme,
    threshold_coin_tossing: ThresholdCoinTossing,
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
        let faulty = (total as f32 / 3 as f32) as usize;
        // threshold = 2 * f + 1, f = number of faulty nodes
        let threshold = 2 * faulty + 1;
        let core = Self {
            node_id,
            id: format!("node_{}", node_id),
            threshold,
            faulty,
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
            skip: BTreeSet::new(),
            broadcast_done: BTreeMap::new(),
            broadcast_skip: BTreeMap::new(),
            send_skip_share: BTreeSet::new(),
            send_skip: BTreeSet::new(),
            recv_done: BTreeSet::new(),
            recv_skip_share: BTreeSet::new(),
            skip_share_signs: BTreeMap::new(),
            stop: false,
            threshold_signature: ThresholdSignatureScheme::new(threshold, total),
            threshold_coin_tossing: ThresholdCoinTossing::new(threshold, total),
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
            self.update().await?;

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
            Message::Ack(ack) => self.handle_ack_message(ack).await?,
            Message::Done(done) => {
                let msg_hash = MessageHash {
                    node_id: done.node_id,
                    view: done.view,
                    message_id: done.value.message_id,
                };
                // first check if recv this DONE message before
                if self.recv_done.contains(&msg_hash) {
                    return Ok(());
                }
                self.recv_done.insert(msg_hash);
                // validate the message
                let validate = self.handle_done_message(&done)?;
                if !validate {
                    error!(
                        "recv done msg {} from node {} validate fail",
                        done.value.message_id, done.node_id
                    );
                    return Ok(());
                }

                self.broadcast_done
                    .entry(done.view)
                    .and_modify(|done| *done += 1)
                    .or_insert(1);

                // check if has reach 2f+1 validate
                if self.threshold > *self.broadcast_done.get(&done.view).unwrap() as usize {
                    return Ok(());
                }
                // check if this view skip-share message has been send before
                if self.send_skip_share.contains(&done.view) {
                    return Ok(());
                }

                self.send_skip_share.insert(done.view);
                let proof_skip = ProofSkipShare {
                    id: done.node_id,
                    view: done.view,
                };
                let value_string = serde_json::to_string(&proof_skip)?;
                let share_sign = self
                    .threshold_signature
                    .share_sign(self.node_id, &value_string)?;

                let signs = vec![(self.node_id, share_sign.clone())];
                self.skip_share_signs.insert(done.view, signs);

                let skip_share_msg = SkipShareMessage {
                    node_id: self.node_id,
                    view: done.view,
                    share_proof: share_sign,
                    message_id: done.value.message_id,
                };
                let json = serde_json::to_string(&skip_share_msg)?;

                // send skip-share message to all parties
                for (node_id, address) in &self.cluster.nodes {
                    if *node_id == self.node_id {
                        continue;
                    }
                    self.send("skip-share", &json, node_id, address).await?;
                }
            }
            Message::SkipShare(skip_share) => {
                let msg_hash = MessageHash {
                    node_id: skip_share.node_id,
                    view: skip_share.view,
                    message_id: skip_share.message_id,
                };
                // first check if recv this SKIP SHARE message before
                if self.recv_skip_share.contains(&msg_hash) {
                    return Ok(());
                }
                let validate = self.handle_skip_share_message(&skip_share)?;
                if !validate {
                    error!(
                        "recv skip_share msg {} from node {} validate fail",
                        skip_share.message_id, skip_share.node_id
                    );

                    return Ok(());
                }
                let node_share_sign = (skip_share.node_id, skip_share.share_proof);
                self.skip_share_signs
                    .entry(skip_share.view)
                    .and_modify(|f| f.push((node_share_sign.clone())))
                    .or_insert(vec![node_share_sign]);

                let node_share_signs = self.skip_share_signs.get(&skip_share.view).unwrap();
                if self.threshold > node_share_signs.len() {
                    return Ok(());
                }
                let signature = self.threshold_signature.threshold_sign(&node_share_signs)?;
                // send `SKIP` message to all parties
                let skip_message = SkipMessage {
                    node_id: skip_share.node_id,
                    view: skip_share.view,
                    proof: signature,
                    message_id: skip_share.message_id,
                };
                let msg_str = serde_json::to_string(&skip_message)?;
                for (node_id, address) in &self.cluster.nodes {
                    if *node_id == self.node_id {
                        continue;
                    }

                    self.send("skip", &msg_str, node_id, address).await?;
                }

                // mark send `SKIP` message in this view
                self.send_skip.insert(skip_share.view);
                // mark skip[view] is true
                self.skip.insert(skip_share.view);
                // stop receiving promote message
                self.stop = true;
                // change to `ElectionLeader` state
                self.promote_state = PromoteState::ElectionLeader(BTreeMap::new());
            }
            Message::Skip(skip) => {
                let validate = self.handle_skip_message(&skip)?;
                if !validate {
                    error!(
                        "recv skip msg {} from node {} validate fail",
                        skip.message_id, skip.node_id
                    );

                    return Ok(());
                }
                let view = skip.view;
                if self.skip.contains(&view) {
                    return Ok(());
                }
                // mark skip[view] is true
                self.skip.insert(view);
                if self.send_skip.contains(&view) {
                    return Ok(());
                }
                self.send_skip.insert(view);

                // send `SKIP` message to all parties
                let skip_message = SkipMessage {
                    node_id: skip.node_id,
                    view: skip.view,
                    proof: skip.proof.clone(),
                    message_id: skip.message_id,
                };
                let msg_str = serde_json::to_string(&skip_message)?;
                for (node_id, address) in &self.cluster.nodes {
                    if *node_id == self.node_id {
                        continue;
                    }

                    self.send("skip", &msg_str, node_id, address).await?;
                }
            }
        }
        Ok(())
    }

    fn handle_skip_message(&self, skip: &SkipMessage) -> Result<bool> {
        let threshold_signature = &self.threshold_signature;
        let proof_skip = ProofSkipShare {
            id: skip.node_id,
            view: skip.view,
        };
        let json = serde_json::to_string(&proof_skip)?;

        let validate = threshold_signature.threshold_validate(&json, &skip.proof);
        Ok(validate)
    }

    async fn handle_proposal_message(&self, message: ProposalMessage) -> Result<()> {
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
        if self.stop {
            return Ok(());
        }
        let from = &promote.node_id;
        let address = if let Some(address) = self.cluster.nodes.get(from) {
            address
        } else {
            return Ok(());
        };
        // check if handled this message id
        if self.seen.contains(&promote.value.message_id) {
            return Ok(());
        }
        if !self.external_provable_broadcast_validate(&promote)? {
            return Ok(());
        }

        // save message id
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

        let resp = AckMessage {
            node_id: self.node_id,
            message_id: promote.value.message_id,
            step: promote.step,
            share_sign,
        };
        let json = serde_json::to_string(&resp)?;

        let uri = format!("http://{}/promote-ack", address);
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

        self.metrics.incr_send_promote_ack();

        Ok(())
    }

    async fn handle_ack_message(&mut self, resp: AckMessage) -> Result<()> {
        let wait_promote_ack = if let PromoteState::WaitAck(wait) = &mut self.promote_state {
            wait
        } else {
            error!(
                "recv promote resp message {} from {} but not in WaitAck state",
                resp.message_id, resp.node_id
            );
            return Ok(());
        };

        if wait_promote_ack.step != resp.step {
            error!(
                "recv promote resp message {} from {} but not the same step, expected {}, actual {}",
                resp.message_id, resp.node_id, wait_promote_ack.step, resp.step,
            );
            return Ok(());
        }

        if wait_promote_ack.data.value.message_id != resp.message_id {
            error!(
                "recv promote resp message {} from {} but not the same message, expected message id {}",
                resp.message_id, resp.node_id, wait_promote_ack.data.value.message_id,
            );
            return Ok(());
        }

        // if this view has been skip, ignore and return
        let view = wait_promote_ack.data.view;
        if self.skip.contains(&view) {
            return Ok(());
        }

        wait_promote_ack
            .share_signs
            .insert(resp.node_id, resp.share_sign);

        // if not reach 2f+1 aggrement, just return
        if wait_promote_ack.share_signs.len() < self.threshold {
            return Ok(());
        }

        // now step has reach aggrement, move to the next step(step < 4) or leader election phase(step == 4)
        let share_signs: Vec<(NodeId, SignatureShare)> =
            wait_promote_ack.share_signs.clone().into_iter().collect();
        let signature = self.threshold_signature.threshold_sign(&share_signs)?;
        if resp.step == 4 {
            self.promote_state = PromoteState::Done((
                wait_promote_ack.data.view,
                wait_promote_ack.data.value.clone(),
                signature,
            ));
        } else {
            let data = PromoteData {
                value: wait_promote_ack.data.value.clone(),
                view: wait_promote_ack.data.view,
                proof: Proof::In(signature),
            };
            self.promote(resp.step + 1, data).await?;
        }

        Ok(())
    }

    // return false if validate message fail
    fn handle_done_message(&self, done: &DoneMessage) -> Result<bool> {
        let proof_value = ProofValue {
            id: done.node_id,
            step: 4,
            value: done.value.clone(),
        };
        let value_string = serde_json::to_string(&proof_value)?;
        let validate = self
            .threshold_signature
            .threshold_validate(&value_string, &done.proof);

        Ok(validate)
    }

    fn handle_skip_share_message(&self, skip_share: &SkipShareMessage) -> Result<bool> {
        let threshold_signature = &self.threshold_signature;
        let proof_skip = ProofSkipShare {
            id: skip_share.node_id,
            view: skip_share.view,
        };
        let value_string = serde_json::to_string(&proof_skip)?;

        Ok(
            threshold_signature.share_validate(
                self.node_id,
                &value_string,
                &skip_share.share_proof,
            ),
        )
    }

    //
    fn elect(&self, view: &View) -> Result<()> {
        let coin_tossing = &self.threshold_coin_tossing;
        let coin_share = ProofCoinShare {
            id: self.node_id,
            view: *view,
        };
        let msg = serde_json::to_string(&coin_share)?;
        let coin_share = coin_tossing.coin_share(self.node_id, &msg)?;

        Ok(())
    }

    fn external_vbba_validate(&self, _promote: &PromoteMessage) -> bool {
        // for now external proof always validate true
        true
    }

    fn check_key(&self, promote: &PromoteMessage, external_proof: &String) -> bool {
        if !self.external_vbba_validate(promote) {
            return false;
        }

        let view = promote.view;
        //if view != 1 {}

        if let Some(lock) = self.state.lock {
            if view >= lock {
                return true;
            }
        }
        false
    }

    fn external_provable_broadcast_validate(&self, promote: &PromoteMessage) -> Result<bool> {
        let step = promote.step;

        if step == 1 {
            // step = 1 case
            if let Proof::External(external_proof) = &promote.proof {
                if self.check_key(promote, external_proof) {
                    return Ok(true);
                }
            } else {
                error!(
                    "step 1 MUST with external proof, message from {} id {}",
                    promote.node_id, promote.value.message_id
                );
                return Ok(false);
            }
        } else {
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

            return Ok(validate);
        }

        Ok(false)
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
                "step > 1 MUST with in proof of last step, message {} from {}",
                promote.value.message_id, promote.node_id,
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
    async fn update(&mut self) -> Result<()> {
        loop {
            let view_state = &self.promote_state;
            match view_state {
                PromoteState::Init => {
                    // if there if no data to promote, break and wait proposal value
                    if !self.init_promote().await? {
                        break;
                    }
                    self.stop = false;
                }
                PromoteState::Promote(data) => {
                    self.promote_state = self.promote(1, data.clone()).await?;
                    break;
                }
                PromoteState::Done((view, value, signature)) => {
                    self.promote_state = self.done(view, value, signature).await?;
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

    async fn send(&self, api: &str, json: &str, to: &NodeId, address: &str) -> Result<()> {
        let uri = format!("http://{}/{}", address, api);
        let resp = self
            .http_client
            .post(uri)
            .header("Content-Type", "application/json")
            .body(json.to_string())
            .send()
            .await;
        if let Err(e) = resp {
            error!(
                "send {} data to node {}/{} error: {:?}",
                api, to, address, e
            );
        }

        Ok(())
    }

    async fn done(
        &self,
        view: &View,
        value: &PromoteValue,
        signature: &Signature,
    ) -> Result<PromoteState> {
        let message = DoneMessage {
            node_id: self.node_id,
            value: value.clone(),
            proof: signature.clone(),
            view: *view,
        };
        let json = serde_json::to_string(&message)?;
        for (node_id, address) in &self.cluster.nodes {
            if *node_id == self.node_id {
                continue;
            }
            self.send("done", &json, node_id, address).await?;
            self.metrics.incr_send_done();
        }
        Ok(PromoteState::WaitSkip(*view))
    }

    async fn promote(&self, step: Step, promote: PromoteData) -> Result<PromoteState> {
        let mut promote_resp = WaitAck {
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

        let message = PromoteMessage {
            step,
            node_id: self.node_id,
            value: promote.value.clone(),
            view: promote.view,
            proof: promote.proof.clone(),
        };
        let json = serde_json::to_string(&message)?;
        for (node_id, address) in &self.cluster.nodes {
            if *node_id == self.node_id {
                continue;
            }
            self.send("promote", &json, node_id, address).await?;

            self.metrics.incr_send_promote();
        }
        //self.promote_state = PromoteState::WaitAck(promote_resp);
        Ok(PromoteState::WaitAck(promote_resp))
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
