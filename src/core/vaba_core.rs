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
use crate::base::DoneMessage;
use crate::base::Message;
use crate::base::MessageId;
use crate::base::NodeId;
use crate::base::PromoteMessage;
use crate::base::ProposalMessageResp;
use crate::base::ShareMessage;
use crate::base::SkipMessage;
use crate::base::SkipShareMessage;
use crate::base::Step;
use crate::base::Value;
use crate::base::View;
use crate::base::ViewChangeMessage;
use crate::core::Stage;
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
use super::ProofCoinShare;
use super::ProofSkipShare;
use super::ProofValue;
use super::WaitAck;

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
    pub key: Option<(View, Signature, PromoteValue)>,

    // current view of VABA
    pub current: View,
}

// state machine state of promote value progress
#[derive(Debug)]
enum PromoteState {
    Init,

    // 4 step promote data
    Promote(PromoteData),

    // wait promote with 2f + 1 ack return
    WaitAck(WaitAck),

    // promote success, notify all parties DONE
    Done(View, PromoteValue, Signature),

    // wait until skip[view] is true
    WaitSkip(View),

    // election leader
    ElectionLeader(BTreeMap<NodeId, SignatureShare>),

    // send view change
    ViewChange(MessageId, View, NodeId),

    // wait view change
    WaitViewChange(MessageId, View, BTreeSet<NodeId>),
}

struct DeliverValue {
    key: Option<PromoteValueWithProof>,
    lock: Option<PromoteValueWithProof>,
    commit: Option<PromoteValueWithProof>,
}

pub struct VabaCore {
    node_id: NodeId,

    // threshold = 2 * faulty + 1
    // faulty = total / 3
    threshold: usize,
    faulty: usize,

    state: VabaState,

    promote_state: PromoteState,

    // (node id, node address)
    nodes: BTreeMap<NodeId, String>,

    rx_api: UnboundedReceiver<Message>,

    http_client: Client,

    proposal_values: Arc<Mutex<Vec<(MessageId, Value)>>>,
    //proposal_values: Vec<(MessageId, Value)>,

    //proposal_sender: Arc<Mutex<BTreeMap<MessageId, oneshot::Sender<ProposalMessageResp>>>>,
    proposal_sender: BTreeMap<MessageId, oneshot::Sender<ProposalMessageResp>>,

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

    // view's leader id
    view_leader: BTreeMap<View, NodeId>,

    // true if stop receiving promote value in current view
    stop: bool,

    // decide values
    decide_values: BTreeMap<View, PromoteValueWithProof>,

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
        nodes: BTreeMap<NodeId, String>,
    ) -> Self {
        let total = nodes.len();
        let faulty = total / 3;
        // threshold = 2 * f + 1, f = number of faulty nodes
        let threshold = total - faulty;
        let node_ids: Vec<u64> = nodes.keys().cloned().collect();
        Self {
            node_id,
            threshold,
            faulty,
            state: VabaState::default(),
            promote_state: PromoteState::Init,
            rx_api,
            http_client: Client::new(),
            nodes,
            proposal_values: Arc::new(Mutex::new(Vec::new())),
            proposal_sender: BTreeMap::new(),
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
            view_leader: BTreeMap::new(),
            stop: false,
            decide_values: BTreeMap::new(),
            threshold_signature: ThresholdSignatureScheme::new(threshold, &node_ids),
            threshold_coin_tossing: ThresholdCoinTossing::new(threshold, &node_ids),
        }
    }

    pub fn stop(&self) -> Result<()> {
        Ok(())
    }

    pub async fn main(mut self) -> Result<()> {
        self.main_loop().await
    }

    async fn main_loop(&mut self) -> Result<()> {
        loop {
            info!(
                "current view {} state: {:?}",
                self.state.current, self.promote_state
            );
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
            Message::Proposal(proposal) => {
                self.metrics.incr_recv_proposal();

                {
                    let mut proposal_values = self.proposal_values.lock().await;
                    proposal_values.push((proposal.message_id, proposal.value.clone()));
                }
                self.proposal_sender
                    .insert(proposal.message_id, proposal.sender);
            }
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
                for (node_id, address) in &self.nodes {
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
                    .and_modify(|f| f.push(node_share_sign.clone()))
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
                for (node_id, address) in &self.nodes {
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
                self.send_share_message(skip_share.node_id, skip_share.view, skip_share.message_id)
                    .await?;
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

                if !self.send_skip.contains(&view) {
                    self.send_skip.insert(view);
                    // send `SKIP` message to all parties
                    let skip_message = SkipMessage {
                        node_id: skip.node_id,
                        view: skip.view,
                        proof: skip.proof.clone(),
                        message_id: skip.message_id,
                    };
                    let msg_str = serde_json::to_string(&skip_message)?;
                    for (node_id, address) in &self.nodes {
                        if *node_id == self.node_id {
                            continue;
                        }

                        self.send("skip", &msg_str, node_id, address).await?;
                    }
                }

                // change to `ElectionLeader` state
                self.send_share_message(skip.node_id, skip.view, skip.message_id)
                    .await?;
                self.promote_state = PromoteState::ElectionLeader(BTreeMap::new());
            }
            Message::Share(share) => {
                if let PromoteState::ElectionLeader(coin_share_set) = &mut self.promote_state {
                    let coin_tossing = &self.threshold_coin_tossing;
                    let coin_share = ProofCoinShare {
                        node_id: share.node_id,
                        view: share.view,
                    };
                    let msg = serde_json::to_string(&coin_share)?;
                    if !coin_tossing.coin_share_validate(self.node_id, &msg, &share.proof) {
                        error!(
                            "recv share msg {} from node {} validate fail",
                            share.message_id, share.node_id
                        );

                        return Ok(());
                    }

                    coin_share_set.insert(share.node_id, share.proof);
                    if coin_share_set.len() < self.faulty + 1 {
                        return Ok(());
                    }
                    let share_signs: Vec<(NodeId, SignatureShare)> =
                        coin_share_set.clone().into_iter().collect();
                    let leader_id = coin_tossing.coin_toss(&msg, &share_signs)?;
                    // save view leader id
                    self.view_leader.insert(share.view, leader_id);
                    // change state to VIEW-CHANGE
                    self.promote_state =
                        PromoteState::ViewChange(share.message_id, share.view, leader_id);
                }
            }
            Message::ViewChangeMessage(view_change) => {
                let leader_id = view_change.leader_id;
                let view = view_change.view;
                let lock = &view_change.lock;
                let key = &view_change.key;
                let commit = &view_change.commit;
                let threshold_signature = &self.threshold_signature;

                // validate commit value
                if let Some(commit) = commit {
                    let proof = &commit.proof;
                    let value = PromoteValue {
                        value: commit.value.clone(),
                        message_id: commit.message_id,
                    };
                    let proof_value = ProofValue {
                        id: leader_id,
                        stage: Stage {
                            view: view_change.view,
                            step: 3,
                        },
                        value: value.clone(),
                    };
                    let json = serde_json::to_string(&proof_value)?;
                    if threshold_signature.threshold_validate(&json, proof) {
                        // now the value is decided!!
                        info!(
                            "node {} decide value {:?} promoted by node {}",
                            self.node_id, value, leader_id
                        );
                        assert!(!self.decide_values.contains_key(&view));
                        self.decide_values.insert(
                            view,
                            PromoteValueWithProof {
                                value: commit.value.clone(),
                                message_id: commit.message_id,
                                proof: proof.clone(),
                            },
                        );
                        // response to the client
                        if let Some(tx) = self.proposal_sender.remove(&commit.message_id) {
                            let resp = ProposalMessageResp {
                                ok: true,
                                error: None,
                            };
                            if let Err(e) = tx.send(resp.clone()) {
                                error!(
                                    "resp proposal message {} to client error {:?}",
                                    commit.message_id, e
                                );
                            }
                        } else {
                            error!("cannot find channel of message {}", commit.message_id);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn send_share_message(
        &self,
        node_id: NodeId,
        view: View,
        message_id: MessageId,
    ) -> Result<()> {
        let coin_tossing = &self.threshold_coin_tossing;
        let coin_share = ProofCoinShare { node_id, view };
        let msg = serde_json::to_string(&coin_share)?;
        let coin_share = coin_tossing.coin_share(self.node_id, &msg)?;

        let msg = ShareMessage {
            node_id,
            view,
            proof: coin_share,
            message_id,
        };
        let json = serde_json::to_string(&msg)?;

        for (node_id, address) in &self.nodes {
            if *node_id == self.node_id {
                continue;
            }

            self.send("share", &json, node_id, address).await?;
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

    async fn handle_promote_message(&mut self, promote: PromoteMessage) -> Result<()> {
        if self.stop {
            return Ok(());
        }
        let from = &promote.node_id;
        let address = if let Some(address) = self.nodes.get(from) {
            address
        } else {
            return Ok(());
        };
        let message_id = promote.value.message_id;
        let stage = &promote.stage;
        let step = stage.step;
        let view = stage.view;
        // check if handled this message id
        if self.seen.contains(&message_id) {
            info!(
                "node {} has seen message {} before",
                self.node_id, message_id
            );
            return Ok(());
        }
        if !self.external_provable_broadcast_validate(&promote)? {
            error!(
                "node {} external validate stage {:?} message {} fail",
                self.node_id, stage, message_id
            );
            return Ok(());
        }

        // save message id
        self.seen.insert(message_id);

        // calculate share sign of this node
        let proof_value = ProofValue {
            id: promote.node_id,
            // use last step signature as in proof
            stage: Stage {
                view: view,
                step: step - 1,
            },
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
            step,
            share_sign,
        };
        let json = serde_json::to_string(&resp)?;

        let uri = format!("http://{}/ack", address);
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
            self.promote_state = PromoteState::Done(
                wait_promote_ack.data.view,
                wait_promote_ack.data.value.clone(),
                signature,
            );
        } else {
            let data = PromoteData {
                value: wait_promote_ack.data.value.clone(),
                view: wait_promote_ack.data.view,
                proof: Some(signature),
            };
            self.promote(resp.step + 1, data).await?;
        }

        Ok(())
    }

    // return false if validate message fail
    fn handle_done_message(&self, done: &DoneMessage) -> Result<bool> {
        let proof_value = ProofValue {
            id: done.node_id,
            stage: Stage {
                view: done.view,
                step: 4,
            },
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

    fn external_vbba_validate(&self, _promote: &PromoteMessage) -> bool {
        // for now external proof always validate true
        true
    }

    fn external_provable_broadcast_validate(&self, promote: &PromoteMessage) -> Result<bool> {
        let stage = &promote.stage;
        let step = stage.step;
        let view = stage.view;
        let message_id = promote.value.message_id;
        let from = promote.node_id;

        let signature = if let Some(signature) = &promote.proof {
            signature
        } else {
            info!("proof is None in {} {}", view, step);
            // only view=1 and step=1 allow None proof
            if view != 1 && step != 1 {
                error!(
                    "recv promote message {} from node {} but with None signature in ({}, {})",
                    message_id, from, view, step
                );
            }
            return Ok(true);
        };

        let proof_value = ProofValue {
            id: promote.node_id,
            // use last step signature as in proof
            stage: Stage {
                view,
                step: step - 1,
            },
            value: promote.value.clone(),
        };
        let value_string = serde_json::to_string(&proof_value)?;
        let validate = self
            .threshold_signature
            .threshold_validate(&value_string, signature);
        if validate {
            return Ok(true);
        }

        error!(
            "recv promote message {} from node {} validate fail",
            message_id, from
        );

        /*
        if let Some(lock) = self.state.lock {
            Ok(view >= lock)
        } else {
            Ok(true)
        }
        */

        Ok(false)
    }

    async fn deliver(&self, promote: &PromoteMessage) {
        let stage = &promote.stage;
        info!(
            "deliver stage {:?} message {}",
            stage, promote.value.message_id
        );
        let step = stage.step;
        if step == 1 {
            return;
        }
        let from = promote.node_id;
        let view = stage.view;
        let signature = if let Some(signature) = &promote.proof {
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

    // update promote state
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
                PromoteState::Done(view, value, signature) => {
                    self.promote_state = self.done(view, value, signature).await?;
                    break;
                }
                PromoteState::ElectionLeader(coin_share_set) => {
                    break;
                }
                PromoteState::ViewChange(message_id, view, leader_id) => {
                    let ok = self
                        .send_view_change(*message_id, *view, *leader_id)
                        .await?;
                    if !ok {
                        // move to the next view
                        self.state.current += 1;
                        self.promote_state = PromoteState::Init;
                    } else {
                        self.promote_state =
                            PromoteState::WaitViewChange(*message_id, *view, BTreeSet::new());
                    }
                    break;
                }
                _ => break,
            }
        }
        Ok(())
    }

    async fn send_view_change(
        &self,
        message_id: MessageId,
        view: View,
        leader_id: NodeId,
    ) -> Result<bool> {
        let msg = {
            let deliver = self.deliver.lock().await;
            let view_deliver = if let Some(view_deliver) = deliver.get(&leader_id) {
                view_deliver
            } else {
                error!("cannot find node {} deliver data", leader_id);
                return Ok(false);
            };

            let deliver = if let Some(deliver) = view_deliver.get(&view) {
                deliver
            } else {
                error!("cannot find node {} view {} deliver data", leader_id, view);
                return Ok(false);
            };

            ViewChangeMessage {
                leader_id: leader_id,
                node_id: self.node_id,
                view,
                message_id,
                key: deliver.key.clone(),
                lock: deliver.lock.clone(),
                commit: deliver.commit.clone(),
            }
        };
        let json = serde_json::to_string(&msg)?;
        for (node_id, address) in &self.nodes {
            if *node_id == self.node_id {
                continue;
            }

            self.send("view-change", &json, node_id, address).await?;
        }
        Ok(true)
    }

    async fn select_promote_data(&self) -> Option<PromoteData> {
        let promote_value = if let Some((last_view, proof, promote)) = &self.state.key {
            let proof = if let Some(value_with_proof) = self.decide_values.get(last_view) {
                Some(value_with_proof.proof.clone())
            } else {
                error!("last view is {} but not found delivered value", last_view);
                assert!(false);
                return None;
            };
            PromoteData {
                value: promote.clone(),
                proof,
                view: *last_view,
            }
        } else {
            let current_view = self.state.current;
            let last_view = current_view - 1;
            let value = {
                let mut proposal_values = self.proposal_values.lock().await;
                if proposal_values.is_empty() {
                    return None;
                }
                proposal_values.remove(0)
            };

            let proof = if current_view == 1 {
                None
            } else if let Some(value_with_proof) = self.decide_values.get(&last_view) {
                Some(value_with_proof.proof.clone())
            } else {
                error!("last view is {} but not found delivered value", last_view);
                assert!(false);
                return None;
            };

            PromoteData {
                proof,
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
                self.node_id, promopt_data, self.state.current
            );
            self.promote_state = PromoteState::Promote(promopt_data);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn send(&self, api: &str, json: &str, to: &NodeId, address: &str) -> Result<()> {
        match api {
            "promote" => {
                self.metrics.incr_send_promote();
            }
            _ => {}
        }

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
        for (node_id, address) in &self.nodes {
            if *node_id == self.node_id {
                continue;
            }
            self.send("done", &json, node_id, address).await?;
        }
        Ok(PromoteState::WaitSkip(*view))
    }

    async fn promote(&self, step: Step, promote: PromoteData) -> Result<PromoteState> {
        let mut promote_resp = WaitAck {
            step,
            data: promote.clone(),
            share_signs: BTreeMap::new(),
        };
        let last_stage = Stage {
            view: promote.view,
            step: step - 1,
        };
        // calculate share sign of this node
        let proof_value = ProofValue {
            id: self.node_id,
            // use last step signature as in proof
            stage: last_stage,
            value: promote.value.clone(),
        };
        let value_string = serde_json::to_string(&proof_value)?;
        let share_sign = self
            .threshold_signature
            .share_sign(self.node_id, &value_string)?;
        promote_resp.share_signs.insert(self.node_id, share_sign);

        let message = PromoteMessage {
            stage: Stage {
                step,
                view: promote.view,
            },
            node_id: self.node_id,
            value: promote.value.clone(),
            proof: promote.proof.clone(),
        };
        let json = serde_json::to_string(&message)?;
        info!("promote step {} data {:?}", step, promote);
        for (node_id, address) in &self.nodes {
            if *node_id == self.node_id {
                continue;
            }
            self.send("promote", &json, node_id, address).await?;
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
