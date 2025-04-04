use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use log::error;
use log::info;
use reqwest::Client;
use threshold_crypto::Signature;
use threshold_crypto::SignatureShare;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::base::AckMessage;
use crate::base::DoneMessage;
use crate::base::Message;
use crate::base::MessageId;
use crate::base::MetricsMessageResp;
use crate::base::NodeId;
use crate::base::PromoteMessage;
use crate::base::ProposalMessageResp;
use crate::base::RespMessage;
use crate::base::ShareMessage;
use crate::base::SkipMessage;
use crate::base::SkipShareMessage;
use crate::base::Step;
use crate::base::Value;
use crate::base::View;
use crate::base::ViewChange;
use crate::core::Stage;
use crate::crypto::ThresholdCoinTossing;
use crate::crypto::ThresholdSignatureScheme;
use anyhow::Result;
use futures::FutureExt;
use tokio::sync::mpsc::UnboundedReceiver;

use super::snowflake_generator::SnowflakeGenerator;
use super::Ack;
use super::IdempotentId;
use super::MessageHash;
use super::Metrics;
use super::PromoteData;
use super::PromoteValue;
use super::PromoteValueWithProof;
use super::ProofCoinShare;
use super::ProofSkipShare;
use super::ProofValue;

const RESEND_INTERNAL_MS: u64 = 500;

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
    WaitAck(Ack),

    // promote success, notify all parties DONE
    Done(View, PromoteValue, Signature),

    // wait until skip[view] is true
    WaitSkip,

    // election leader
    ElectionLeader(BTreeMap<NodeId, SignatureShare>),

    // send view change
    ViewChange(MessageId, View, NodeId),

    // wait view change
    WaitViewChange(BTreeSet<NodeId>),
}

struct DeliverValue {
    key: Option<PromoteValueWithProof>,
    lock: Option<PromoteValueWithProof>,
    commit: Option<PromoteValueWithProof>,
}

#[derive(Debug)]
pub struct WaitRespMessage {
    pub to: NodeId,
    pub msg: String,
    pub api: String,
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

    metrics: Metrics,

    // node id -> {view, DeliverValue}
    deliver: Arc<Mutex<BTreeMap<NodeId, BTreeMap<View, DeliverValue>>>>,

    // if has skip view
    skip: BTreeSet<View>,

    // number of view send `DONE` message
    recv_done_counter: BTreeMap<View, u16>,

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

    // waiting resp message
    waiting_resp_msg: Arc<Mutex<BTreeMap<IdempotentId, WaitRespMessage>>>,

    id_generator: SnowflakeGenerator,
    last_resend_time: Arc<Mutex<u64>>,

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
            metrics: Metrics::new(),
            deliver: Arc::new(Mutex::new(BTreeMap::new())),
            skip: BTreeSet::new(),
            recv_done_counter: BTreeMap::new(),
            send_skip_share: BTreeSet::new(),
            send_skip: BTreeSet::new(),
            recv_done: BTreeSet::new(),
            recv_skip_share: BTreeSet::new(),
            skip_share_signs: BTreeMap::new(),
            view_leader: BTreeMap::new(),
            stop: false,
            decide_values: BTreeMap::new(),
            waiting_resp_msg: Arc::new(Mutex::new(BTreeMap::new())),
            id_generator: SnowflakeGenerator::new(node_id),
            last_resend_time: Arc::new(Mutex::new(0)),
            threshold_signature: ThresholdSignatureScheme::new(threshold - 1, &node_ids),
            threshold_coin_tossing: ThresholdCoinTossing::new(faulty, &node_ids),
        }
    }

    pub async fn main(mut self) -> Result<()> {
        let ret = self.main_loop().await;
        error!("out of main error: {:?}", ret);

        ret
    }

    async fn resend_message(&self) -> Result<()> {
        let timestamp = Utc::now().timestamp_millis() as u64;
        {
            let mut last_resend_time = self.last_resend_time.lock().await;
            //info!("last: {}, now: {}", last_resend_time, timestamp);
            if *last_resend_time > timestamp - RESEND_INTERNAL_MS {
                return Ok(());
            }
            *last_resend_time = timestamp;
        }
        let wait_resp = self.waiting_resp_msg.lock().await;
        for (id, wait_resp) in wait_resp.iter() {
            let msg_ts = SnowflakeGenerator::timestamp(*id);
            if msg_ts > timestamp - RESEND_INTERNAL_MS {
                break;
            }
            if let Some(address) = self.nodes.get(&wait_resp.to) {
                info!("re-send id {} msg to node {}", id, wait_resp.to);

                let uri = format!("http://{}/{}", address, wait_resp.api);
                let resp = self
                    .http_client
                    .post(uri)
                    .header("Content-Type", "application/json")
                    .body(wait_resp.msg.clone())
                    .send()
                    .await;

                if let Err(e) = resp {
                    error!(
                        "re-send response data to node {}/{} error: {:?}",
                        wait_resp.to, address, e
                    );
                }
            }
        }

        Ok(())
    }

    async fn main_loop(&mut self) -> Result<()> {
        let mut resend;
        loop {
            /*
            info!(
                "current view {} state: {:?}",
                self.state.current, self.promote_state
            );
            */
            resend = false;
            let timeout_future = sleep(Duration::from_millis(RESEND_INTERNAL_MS));
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
                },

                _ = timeout_future.fuse() => {
                    resend = true;
                    self.resend_message().await?;
                }

            }

            if !resend {
                self.resend_message().await?;
            }
        }
        Ok(())
    }

    async fn handle_message(&mut self, msg: Message) -> Result<()> {
        if let Some((idempotent_id, to)) = msg.idempotent_id_and_node_id() {
            let resp_msg = RespMessage {
                id: idempotent_id,
                node_id: self.node_id,
            };
            let json = serde_json::to_string(&resp_msg)?;

            if let Some(address) = self.nodes.get(&to) {
                info!("response msg {} to node {}", idempotent_id, to);
                let uri = format!("http://{}/{}", address, "response");
                let resp = self
                    .http_client
                    .post(uri)
                    .header("Content-Type", "application/json")
                    .body(json.to_string())
                    .send()
                    .await;

                if let Err(e) = resp {
                    error!(
                        "send response data to node {}/{} error: {:?}",
                        to, address, e
                    );
                }
            }
        }

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
            Message::Promote(promote) => {
                self.handle_promote_message(promote).await?;
            }
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

                // validate the message
                let validate = self.validate_done_message(&done)?;
                if !validate {
                    error!(
                        "recv done msg {} from node {} validate fail",
                        done.value.message_id, done.node_id
                    );
                    return Ok(());
                }

                self.recv_done.insert(msg_hash);

                // update recv view done counter
                self.recv_done_counter
                    .entry(done.view)
                    .and_modify(|done| *done += 1)
                    .or_insert(1);

                // check if has reach 2f+1 validate
                if self.threshold > *self.recv_done_counter.get(&done.view).unwrap() as usize {
                    return Ok(());
                }
                // check if this view skip-share message has been send before
                if self.send_skip_share.contains(&done.view) {
                    info!(
                        "node {} has send skip-share in view {} before",
                        self.node_id, done.view
                    );
                    return Ok(());
                }

                self.send_skip_share.insert(done.view);
                let proof_skip = ProofSkipShare {
                    // use done.node_id, now self.node_id
                    id: done.node_id,
                    view: done.view,
                };
                let value_string = serde_json::to_string(&proof_skip)?;
                let share_sign = self
                    .threshold_signature
                    .share_sign(self.node_id, &value_string)?;

                let signs = vec![(self.node_id, share_sign.clone())];
                self.skip_share_signs.insert(done.view, signs);

                // send skip-share message to all parties
                for (node_id, address) in &self.nodes {
                    if *node_id == self.node_id {
                        continue;
                    }

                    let idempotent_id = self.generate_idempotent_id();
                    let skip_share_msg = SkipShareMessage {
                        node_id: self.node_id,
                        from: done.node_id,
                        view: done.view,
                        share_proof: share_sign.clone(),
                        message_id: done.value.message_id,
                        idempotent_id,
                    };
                    let json = serde_json::to_string(&skip_share_msg)?;
                    self.send("skip-share", &json, node_id, address, idempotent_id)
                        .await?;
                }
            }
            Message::SkipShare(skip_share) => {
                let msg_hash = MessageHash {
                    node_id: skip_share.from,
                    view: skip_share.view,
                    message_id: skip_share.message_id,
                };
                // first check if recv this SKIP SHARE message before
                if self.recv_skip_share.contains(&msg_hash) {
                    return Ok(());
                }
                let validate = self.validate_skip_share_message(&skip_share)?;
                if !validate {
                    error!(
                        "recv skip_share msg {} from node {} of node {} validate fail",
                        skip_share.message_id, skip_share.node_id, skip_share.from
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
                let signature = match self.threshold_signature.threshold_sign(&node_share_signs) {
                    Ok(signature) => signature,
                    Err(e) => {
                        error!("threshold_sign skip share message error: {:?}", e);
                        return Err(e);
                    }
                };
                // send `SKIP` message to all parties
                for (node_id, address) in &self.nodes {
                    /*
                    if *node_id == self.node_id {
                        continue;
                    }
                    */
                    let idempotent_id = self.generate_idempotent_id();
                    let skip_message = SkipMessage {
                        node_id: self.node_id,
                        from: skip_share.from,
                        view: skip_share.view,
                        proof: signature.clone(),
                        message_id: skip_share.message_id,
                        idempotent_id,
                    };
                    let msg_str = serde_json::to_string(&skip_message)?;
                    self.send("skip", &msg_str, node_id, address, idempotent_id)
                        .await?;
                }

                // mark send `SKIP` message in this view
                self.send_skip.insert(skip_share.view);
                // mark skip[view] is true
                self.skip.insert(skip_share.view);

                // change to `ElectionLeader` state
                self.send_share_message(skip_share.view, skip_share.message_id)
                    .await?;
                self.promote_state = PromoteState::ElectionLeader(BTreeMap::new());
            }
            Message::Skip(skip) => {
                let view = skip.view;
                if self.skip.contains(&view) {
                    info!(
                        "node {} has seen skip of view {} msg before",
                        self.node_id, view
                    );
                    return Ok(());
                }

                let validate = self.validate_skip_message(&skip)?;
                if !validate {
                    error!(
                        "recv skip msg {} from node {} of node {} validate fail",
                        skip.message_id, skip.node_id, skip.from,
                    );

                    return Ok(());
                }

                // mark skip[view] is true
                self.skip.insert(view);
                // stop receiving promote message
                self.stop = true;

                if !self.send_skip.contains(&view) {
                    self.send_skip.insert(view);
                    // send `SKIP` message to all parties

                    for (node_id, address) in &self.nodes {
                        /*
                        if *node_id == self.node_id {
                            continue;
                        }
                        */
                        let idempotent_id = self.generate_idempotent_id();
                        let skip_message = SkipMessage {
                            node_id: self.node_id,
                            from: skip.from,
                            view: skip.view,
                            proof: skip.proof.clone(),
                            message_id: skip.message_id,
                            idempotent_id,
                        };
                        let msg_str = serde_json::to_string(&skip_message)?;
                        self.send("skip", &msg_str, node_id, address, idempotent_id)
                            .await?;
                    }
                }

                // change to `ElectionLeader` state
                self.send_share_message(skip.view, skip.message_id).await?;
                self.promote_state = PromoteState::ElectionLeader(BTreeMap::new());
            }
            Message::Share(share) => {
                if let PromoteState::ElectionLeader(coin_share_set) = &mut self.promote_state {
                    let coin_tossing = &self.threshold_coin_tossing;
                    let coin_share = ProofCoinShare { view: share.view };
                    let msg = serde_json::to_string(&coin_share)?;
                    if !coin_tossing.coin_share_validate(share.node_id, &msg, &share.proof) {
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
                    info!("elect node {} as view {} leader", leader_id, share.view);
                    // change state to VIEW-CHANGE
                    self.promote_state =
                        PromoteState::ViewChange(share.message_id, share.view, leader_id);
                }
            }
            Message::ViewChange(view_change) => {
                info!(
                    "recv view-change {:?} from node {}",
                    view_change, view_change.node_id
                );
                // update wait view-change set
                let recv_view_change_counter = if let PromoteState::WaitViewChange(wait_set) =
                    &mut self.promote_state
                {
                    wait_set.insert(view_change.node_id);
                    wait_set.len()
                } else {
                    info!("node {} recv view-change {:?} from node {}, but not in wait-view-change state", self.node_id, view_change, view_change.node_id);
                    return Ok(());
                };

                let view = view_change.view;
                let lock = &view_change.lock;
                let key = &view_change.key;
                let commit = &view_change.commit;

                // validate commit value
                if let Some(commit) = commit {
                    if let Ok(Some(value)) = self.decide_commit(&view_change, commit) {
                        self.decide_values.insert(view, value);

                        // response to the client
                        if let Some(tx) = self.proposal_sender.remove(&commit.value.message_id) {
                            let resp = ProposalMessageResp {
                                ok: true,
                                error: None,
                            };
                            if let Err(e) = tx.send(resp.clone()) {
                                error!(
                                    "resp proposal message {} to client error {:?}",
                                    commit.value.message_id, e
                                );
                            }
                            info!("response message {} to client", commit.value.message_id);
                        } else {
                            error!("cannot find channel of message {}", commit.value.message_id);
                        }
                    }
                }

                // validate lock value
                if let Some(lock_value_proof) = lock {
                    if let Ok(Some(lock)) = self.decide_lock(&view_change, lock_value_proof) {
                        info!("node {} update lock to {}", self.node_id, lock);
                        self.state.lock = Some(lock);
                    }
                }

                // validate key value
                if let Some(key_value_proof) = key {
                    if let Ok(Some(new_key)) = self.decide_key(&view_change, key_value_proof) {
                        info!("node {} update key to {:?}", self.node_id, new_key);
                        self.state.key = Some(new_key);
                    }
                }

                // if recv threshold view-change msg, move to the next view
                if recv_view_change_counter >= self.threshold {
                    self.state.current = view + 1;
                    self.promote_state = PromoteState::Init;
                }
            }
            Message::Metrics(metrics) => {
                let resp = MetricsMessageResp {
                    metrics: self.metrics.clone(),
                };

                if let Err(e) = metrics.sender.send(resp) {
                    error!("resp metrics message to client error {:?}", e);
                }
            }
            Message::Response(resp) => {
                info!("recv response msg of {}", resp.id);
                // when recv response mesage, remove the message to avoid re-send message
                let mut wait_resp = self.waiting_resp_msg.lock().await;
                assert!(wait_resp.remove(&resp.id).is_some());
            }
        }
        Ok(())
    }

    // decide commit data
    fn decide_commit(
        &self,
        view_change: &ViewChange,
        commit: &PromoteValueWithProof,
    ) -> Result<Option<PromoteValueWithProof>> {
        let view = view_change.view;
        if let Some(committed_value) = self.decide_values.get(&view) {
            // if commit value of this view before, assert they are the same value
            assert_eq!(commit, committed_value);
            return Ok(None);
        }

        let node_id = view_change.node_id;
        let leader_id = view_change.leader_id;

        let proof = &commit.proof;
        let value = commit.value.clone();
        let proof_value = ProofValue {
            stage: Stage { view, step: 3 },
            value: value.clone(),
        };
        let json = serde_json::to_string(&proof_value)?;
        if !self.threshold_signature.threshold_validate(&json, proof) {
            error!("view-change commit from node {} validate fail", node_id);
            return Ok(None);
        }

        // now the value is decided!!
        info!(
            "node {} decide value {:?} promoted by node {} in view {}",
            self.node_id, value, leader_id, view,
        );

        Ok(Some(PromoteValueWithProof {
            value: commit.value.clone(),
            proof: proof.clone(),
        }))
    }

    // return the new key data, None if no update
    fn decide_key(
        &self,
        view_change: &ViewChange,
        value_proof: &PromoteValueWithProof,
    ) -> Result<Option<(View, Signature, PromoteValue)>> {
        let view = view_change.view;
        let node_id = view_change.node_id;

        if let Some((state_view, _, _)) = self.state.key {
            // receive older promote value, ignore it
            if view <= state_view {
                info!(
                    "recv view {} view-change key data, not newer than key view {}",
                    view, state_view
                );
                return Ok(None);
            }
        }

        let proof = &value_proof.proof;
        let value = value_proof.value.clone();
        let proof_value = ProofValue {
            stage: Stage { view, step: 1 },
            value: value.clone(),
        };
        let json = serde_json::to_string(&proof_value)?;
        if !self.threshold_signature.threshold_validate(&json, proof) {
            error!("view-change key from node {} validate fail", node_id);
            Ok(None)
        } else {
            Ok(Some((
                view,
                value_proof.proof.clone(),
                value_proof.value.clone(),
            )))
        }
    }

    // return the new lock view, None if no update lock view
    fn decide_lock(
        &self,
        view_change: &ViewChange,
        value_proof: &PromoteValueWithProof,
    ) -> Result<Option<View>> {
        let view = view_change.view;
        let node_id = view_change.node_id;

        if let Some(lock_view) = self.state.lock {
            // receive older promote value, ignore it
            if view <= lock_view {
                info!(
                    "recv view {} view-change lock data, not newer than lock {}",
                    view, lock_view
                );
                return Ok(None);
            }
        }

        let proof = &value_proof.proof;
        let value = value_proof.value.clone();
        let proof_value = ProofValue {
            stage: Stage { view, step: 2 },
            value: value.clone(),
        };
        let json = serde_json::to_string(&proof_value)?;
        if !self.threshold_signature.threshold_validate(&json, proof) {
            error!("view-change lock from node {} validate fail", node_id);
            Ok(None)
        } else {
            Ok(Some(view))
        }
    }

    async fn send_share_message(&self, view: View, message_id: MessageId) -> Result<()> {
        let coin_tossing = &self.threshold_coin_tossing;
        let coin_share = ProofCoinShare { view };
        let msg = serde_json::to_string(&coin_share)?;
        let coin_share = coin_tossing.coin_share(self.node_id, &msg)?;

        for (node_id, address) in &self.nodes {
            /*
            if *node_id == self.node_id {
                continue;
            }
            */

            let idempotent_id = self.generate_idempotent_id();
            let msg = ShareMessage {
                node_id: self.node_id,
                view,
                proof: coin_share.clone(),
                message_id,
                idempotent_id,
            };
            let json = serde_json::to_string(&msg)?;
            self.send("share", &json, node_id, address, idempotent_id)
                .await?;
        }
        Ok(())
    }

    fn validate_skip_message(&self, skip: &SkipMessage) -> Result<bool> {
        let threshold_signature = &self.threshold_signature;
        let proof_skip = ProofSkipShare {
            // use from not node id
            id: skip.from,
            view: skip.view,
        };
        let json = serde_json::to_string(&proof_skip)?;

        let validate = threshold_signature.threshold_validate(&json, &skip.proof);
        Ok(validate)
    }

    async fn handle_promote_message(&mut self, promote: PromoteMessage) -> Result<()> {
        if self.stop {
            error!(
                "recv message {} from node {} but stopped",
                promote.value.message_id, promote.value.node_id
            );
            return Ok(());
        }
        let from = &promote.value.node_id;
        let address = if let Some(address) = self.nodes.get(from) {
            address
        } else {
            return Ok(());
        };
        let message_id = promote.value.message_id;
        let stage = &promote.stage;
        let step = stage.step;
        let view = stage.view;
        if !self.external_broadcast_validation(&promote)? {
            error!(
                "node {} external validate stage {:?} message {} fail",
                self.node_id, stage, message_id
            );
            return Ok(());
        }

        // calculate share sign of this node
        let proof_value = ProofValue {
            stage: Stage { view, step },
            value: promote.value.clone(),
        };
        let value_string = serde_json::to_string(&proof_value)?;
        let share_sign = self
            .threshold_signature
            .share_sign(self.node_id, &value_string)?;

        info!(
            "node {} ack node {} proof_value: {}",
            self.node_id, promote.value.node_id, value_string
        );

        // deliver promote value
        self.deliver(&promote).await;

        let idempotent_id = self.generate_idempotent_id();
        let resp = AckMessage {
            node_id: self.node_id,
            from: promote.value.node_id,
            message_id: promote.value.message_id,
            stage: promote.stage.clone(),
            share_sign,
            idempotent_id,
        };
        let json = serde_json::to_string(&resp)?;
        self.send("ack", &json, from, address, idempotent_id)
            .await?;

        Ok(())
    }

    async fn handle_ack_message(&mut self, resp: AckMessage) -> Result<()> {
        // ack only send to the promote data node
        //assert!(resp.node_id == self.node_id);
        //assert!(resp.from != self.node_id);

        let wait_promote_ack = if let PromoteState::WaitAck(wait) = &mut self.promote_state {
            wait
        } else {
            error!(
                "recv ack message {} from {} but not in Ack state",
                resp.message_id, resp.node_id
            );
            return Ok(());
        };

        if wait_promote_ack.stage != resp.stage {
            error!(
                "recv ack message {} from {} but not the same stage, expected {:?}, actual {:?}",
                resp.message_id, resp.node_id, wait_promote_ack.stage, resp.stage,
            );
            return Ok(());
        }

        if wait_promote_ack.data.value.message_id != resp.message_id {
            error!(
                "recv ack message {} from {} but not the same message, expected message id {}",
                resp.message_id, resp.node_id, wait_promote_ack.data.value.message_id,
            );
            return Ok(());
        }

        if resp.from != self.node_id {
            error!(
                "recv ack message {} from {} but not the same message, expected message id {}",
                resp.message_id, resp.node_id, wait_promote_ack.data.value.message_id,
            );

            return Ok(());
        }

        // if this view has been skip, ignore and return
        let view = wait_promote_ack.data.view;
        if self.skip.contains(&view) {
            error!(
                "recv ack message {} from {} but has been skip",
                resp.message_id, resp.node_id
            );
            return Ok(());
        }

        // validate the ack share signature
        let proof_value = ProofValue {
            stage: resp.stage.clone(),
            value: PromoteValue {
                node_id: self.node_id,
                value: wait_promote_ack.data.value.value.clone(),
                message_id: resp.message_id,
            },
        };
        let json = serde_json::to_string(&proof_value)?;
        if !self
            .threshold_signature
            .share_validate(resp.node_id, &json, &resp.share_sign)
        {
            error!(
                "recv stage {:?} ack messgae {} from {} but validate fail",
                resp.stage, resp.message_id, resp.from
            );
            return Ok(());
        }

        let msg_hash = MessageHash {
            node_id: resp.node_id,
            message_id: resp.message_id,
            view: resp.stage.view,
        };

        wait_promote_ack
            .share_signs
            .insert(msg_hash, resp.share_sign);

        // if not reach 2f+1 aggrement, just return
        if wait_promote_ack.share_signs.len() < self.threshold {
            info!(
                "recv ack message {} from {}, now {} expected {}",
                resp.message_id,
                resp.node_id,
                wait_promote_ack.share_signs.len(),
                self.threshold
            );
            return Ok(());
        }

        // now step has reach aggrement, move to the next step(step < 4) or leader election phase(step == 4)
        let mut share_signs: Vec<(NodeId, SignatureShare)> = Vec::new();
        for (msg_hash, signature) in &wait_promote_ack.share_signs {
            share_signs.push((msg_hash.node_id, signature.clone()));
        }

        let signature = self.threshold_signature.threshold_sign(&share_signs)?;
        if resp.stage.step == 4 {
            info!(
                "recv ack message {} from {}, state Done",
                resp.message_id, resp.from,
            );

            self.promote_state = PromoteState::Done(
                wait_promote_ack.data.view,
                wait_promote_ack.data.value.clone(),
                signature,
            );
        } else {
            info!(
                "recv ack message {} from {}, move to step {}",
                resp.message_id,
                resp.from,
                resp.stage.step + 1,
            );

            let data = PromoteData {
                value: wait_promote_ack.data.value.clone(),
                view: wait_promote_ack.data.view,
                proof: Some(signature),
            };
            self.promote_state = self.promote(resp.stage.step + 1, data).await?;
        }

        Ok(())
    }

    // return false if validate message fail
    fn validate_done_message(&self, done: &DoneMessage) -> Result<bool> {
        let proof_value = ProofValue {
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

    fn validate_skip_share_message(&self, skip_share: &SkipShareMessage) -> Result<bool> {
        let threshold_signature = &self.threshold_signature;
        let proof_skip = ProofSkipShare {
            // use from, not node_id
            id: skip_share.from,
            view: skip_share.view,
        };
        let value_string = serde_json::to_string(&proof_skip)?;

        Ok(threshold_signature.share_validate(
            // use skip_share.node_id, not self.node_id
            skip_share.node_id,
            &value_string,
            &skip_share.share_proof,
        ))
    }

    fn external_broadcast_validation(&self, promote: &PromoteMessage) -> Result<bool> {
        let stage = &promote.stage;
        let step = stage.step;
        let view = stage.view;
        let message_id = promote.value.message_id;
        let from = promote.value.node_id;

        let signature = if let Some(signature) = &promote.proof {
            assert!(step > 1);
            signature
        } else {
            //info!("proof is None in {} {}", view, step);
            // only view=1 and step=1 allow None proof
            if step != 1 {
                error!(
                    "recv promote message {} from node {} but with None signature in ({}, {})",
                    message_id, from, view, step
                );
            }
            return Ok(true);
        };

        let proof_value = ProofValue {
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
            "recv stage {:?} proof message {:?} from node {} validate fail",
            stage, proof_value, from
        );

        if let Some(lock) = self.state.lock {
            Ok(view >= lock)
        } else {
            Ok(true)
        }
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
        let from = promote.value.node_id;
        let view = stage.view;
        let signature = if let Some(signature) = &promote.proof {
            signature
        } else {
            error!(
                "step > 1 MUST with in proof of last step, message {} from {}",
                promote.value.message_id, from,
            );
            return;
        };
        let value_with_proof = PromoteValueWithProof {
            value: promote.value.clone(),
            proof: signature.clone(),
        };

        info!(
            "node {} deliver step {} from node {} message_id {}",
            self.node_id, step, from, promote.value.message_id
        );

        let mut deliver = self.deliver.lock().await;
        let node_view_deliver = if let Some(node_view_deliver) = deliver.get_mut(&from) {
            node_view_deliver
        } else {
            deliver.insert(from, BTreeMap::new());
            deliver.get_mut(&from).unwrap()
        };

        let deliver = if let Some(view_deliver) = node_view_deliver.get_mut(&view) {
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
                    // start recv promote message
                    self.stop = false;
                    // if there if no data to promote, break and wait proposal value
                    if let Some(state) = self.init_promote().await {
                        self.promote_state = state;
                    } else {
                        break;
                    }
                }
                PromoteState::Promote(data) => {
                    self.promote_state = self.promote(1, data.clone()).await?;
                    break;
                }
                PromoteState::Done(view, value, signature) => {
                    self.promote_state = self.done(view, value, signature).await?;
                    break;
                }
                PromoteState::ElectionLeader(_coin_share_set) => {
                    break;
                }
                PromoteState::ViewChange(message_id, view, leader_id) => {
                    let ok = self
                        .send_view_change(*message_id, *view, *leader_id)
                        .await?;
                    if !ok {
                        // move to the next view
                        //self.state.current += 1;
                        //self.promote_state = PromoteState::Init;
                        info!(
                            "cannot find leader id {} view {} deliver data, wait...",
                            leader_id, view
                        );
                    } else {
                        self.promote_state = PromoteState::WaitViewChange(BTreeSet::new());
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
        let (key, lock, commit) = {
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

            (
                deliver.key.clone(),
                deliver.lock.clone(),
                deliver.commit.clone(),
            )
        };

        for (node_id, address) in &self.nodes {
            /*
            if *node_id == self.node_id {
                continue;
            }
            */
            let idempotent_id = self.generate_idempotent_id();
            let msg = ViewChange {
                node_id: self.node_id,
                leader_id,
                view,
                message_id,
                key: key.clone(),
                lock: lock.clone(),
                commit: commit.clone(),
                idempotent_id,
            };
            let json = serde_json::to_string(&msg)?;
            self.send("view-change", &json, node_id, address, idempotent_id)
                .await?;
        }
        Ok(true)
    }

    async fn select_promote_data(&self) -> Option<PromoteData> {
        let value = {
            let mut proposal_values = self.proposal_values.lock().await;
            if proposal_values.is_empty() {
                // if there is no proposal data, return None and wait client proposal data
                return None;
            }
            proposal_values.remove(0)
        };

        Some(PromoteData {
            proof: None,
            value: PromoteValue {
                value: value.1,
                message_id: value.0,
                node_id: self.node_id,
            },
            view: self.state.current,
        })
    }

    // return None if no input values
    async fn init_promote(&self) -> Option<PromoteState> {
        if let Some(promopt_data) = self.select_promote_data().await {
            info!(
                "node {} select promote data {:?} in view {}",
                self.node_id, promopt_data, self.state.current
            );
            Some(PromoteState::Promote(promopt_data))
        } else {
            None
        }
    }

    async fn send(
        &self,
        api: &str,
        json: &str,
        to: &NodeId,
        address: &str,
        id: IdempotentId,
    ) -> Result<()> {
        match api {
            "promote" => {
                self.metrics.incr_send_promote();
            }
            "ack" => {
                self.metrics.incr_send_ack();
            }
            "done" => {
                self.metrics.incr_send_done();
            }
            "skip-share" => {
                self.metrics.incr_send_skip_share();
            }
            "skip" => {
                self.metrics.incr_send_skip();
            }
            "share" => {
                self.metrics.incr_send_share();
            }
            "view-change" => {
                self.metrics.incr_send_view_change();
            }
            _ => {}
        }
        info!(
            "node {} send {} {} msg to node {}",
            self.node_id, id, api, to
        );

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
                "send {} data to node {}/{} id {} error: {:?}",
                api, to, address, id, e
            );
        }
        let wait_message = WaitRespMessage {
            to: *to,
            msg: json.to_string(),
            api: api.to_string(),
        };
        let mut wait_resp = self.waiting_resp_msg.lock().await;
        wait_resp.insert(id, wait_message);

        Ok(())
    }

    async fn done(
        &self,
        view: &View,
        value: &PromoteValue,
        signature: &Signature,
    ) -> Result<PromoteState> {
        for (node_id, address) in &self.nodes {
            /*
            if *node_id == self.node_id {
                continue;
            }
            */
            let idempotent_id = self.generate_idempotent_id();
            let message = DoneMessage {
                node_id: self.node_id,
                value: value.clone(),
                proof: signature.clone(),
                view: *view,
                idempotent_id,
            };
            let json = serde_json::to_string(&message)?;
            self.send("done", &json, node_id, address, idempotent_id)
                .await?;
        }
        Ok(PromoteState::WaitSkip)
    }

    async fn promote(&self, step: Step, promote: PromoteData) -> Result<PromoteState> {
        // node can only promote it's own value
        assert!(promote.value.node_id == self.node_id);

        let ack = Ack {
            stage: Stage {
                view: promote.view,
                step,
            },
            data: promote.clone(),
            share_signs: BTreeMap::new(),
        };

        // calculate (view, step) value share sign of this node
        /*
        let proof_value = ProofValue {
            stage: Stage {
                view: promote.view,
                step,
            },
            value: promote.value.clone(),
        };
        let value_string = serde_json::to_string(&proof_value)?;
        let share_sign = self
            .threshold_signature
            .share_sign(self.node_id, &value_string)?;
        let msg_hash = MessageHash {
            node_id: self.node_id,
            message_id: promote.value.message_id,
            view: promote.view,
        };
        ack.share_signs.insert(msg_hash, share_sign);
        */

        for (node_id, address) in &self.nodes {
            /*
            if *node_id == self.node_id {
                self.init_view_deliver(promote.view).await?;
                continue;
            }
            */
            let idempotent_id = self.generate_idempotent_id();
            let message = PromoteMessage {
                stage: Stage {
                    step,
                    view: promote.view,
                },
                value: promote.value.clone(),
                proof: promote.proof.clone(),
                idempotent_id,
            };
            let json = serde_json::to_string(&message)?;
            self.send("promote", &json, node_id, address, idempotent_id)
                .await?;
        }
        Ok(PromoteState::WaitAck(ack))
    }

    fn generate_idempotent_id(&self) -> IdempotentId {
        self.id_generator.generate()
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

#[cfg(test)]
mod tests {

    use std::collections::BTreeSet;

    use anyhow::Result;
    use rand::Rng;
    use threshold_crypto::{Signature, SignatureShare};

    use crate::{
        base::{NodeId, Step},
        core::{PromoteValue, ProofValue, Stage},
        crypto::{ThresholdSignatureScheme, SEED},
    };

    #[test]
    fn test_4stage_promote() -> Result<()> {
        // first create nodes and threshold signature
        let node_ids: Vec<NodeId> = vec![0, 1, 2, 4];
        let faulty = node_ids.len() / 3;
        let threshold = node_ids.len() - faulty - 1;
        let threshold_signature = ThresholdSignatureScheme::new(threshold - 1, &node_ids);

        let mut value = 0;
        let mut view = 1;

        // iterator node ids, make each of them as a view promoter
        for i in &node_ids {
            let promoter = *i as NodeId;
            // for each view, when step=1 the in proof is None
            let mut in_proof: Option<Signature> = None;

            // create promote value
            let promote_value = PromoteValue {
                node_id: promoter,
                value: value.to_string(),
                message_id: value,
            };

            // the 4 step promote
            for step in 1..5 {
                println!(
                    "run view: {}, step: {}, promoter: {}, value: {}",
                    view, step, promoter, value
                );

                // if there is proof of last step, validate it
                if let Some(in_proof) = &in_proof {
                    // in proof is not None only when step > 1
                    assert!(step > 1);

                    // validate the in proof of last step
                    let proof_value = ProofValue {
                        stage: Stage {
                            step: step - 1 as Step,
                            view,
                        },
                        value: promote_value.clone(),
                    };
                    let json = serde_json::to_string(&proof_value)?;

                    assert!(threshold_signature.threshold_validate(&json, in_proof));
                } else {
                    // in proof is None only when step=1
                    assert!(step == 1);
                }

                // validate last step promote value success,
                // now each node generate their share signature of this step

                // proof value of this step
                let proof_value = ProofValue {
                    stage: Stage {
                        step: step as Step,
                        view,
                    },
                    value: promote_value.clone(),
                };
                let json = serde_json::to_string(&proof_value)?;
                let mut share_signs: Vec<(NodeId, SignatureShare)> = Vec::new();
                for i in &node_ids {
                    // each node generate their share signature of this step
                    let id = *i;
                    let share_sign = threshold_signature.share_sign(id, &json)?;
                    // assert the share signature is validate
                    assert!(threshold_signature.share_validate(id, &json, &share_sign));
                    share_signs.push((id, share_sign));

                    // collect enough share signature, create threshold signature as in proof for the next step
                    if share_signs.len() == threshold {
                        // save the in proof for the next step
                        in_proof = Some(threshold_signature.threshold_sign(&share_signs)?);
                    }
                }
            }

            // move to the next view and new promote value
            view += 1;
            value += 1;
        }
        Ok(())
    }

    #[test]
    fn test_4stage_promote_with_adversary() -> Result<()> {
        // first create nodes and threshold signature
        let node_ids: Vec<NodeId> = vec![0, 1, 2, 4, 5, 6];
        let faulty = node_ids.len() / 3;
        let threshold = node_ids.len() - faulty - 1;
        let threshold_signature = ThresholdSignatureScheme::new(threshold - 1, &node_ids);

        // select nodes as adversary randomly
        let mut adversary_ids = BTreeSet::new();
        while adversary_ids.len() < faulty {
            let mut rng = rand::thread_rng();
            let random: usize = rng.gen();
            adversary_ids.insert(node_ids[random % node_ids.len()]);
        }
        // create a fake ThresholdSignatureScheme, so adversary_id can use it to corrupt the system
        let fake_threshold_signature: ThresholdSignatureScheme = loop {
            let mut rng = rand::thread_rng();
            let fake_seed: [u8; 32] = rng.gen();
            if fake_seed == SEED {
                // continue to generate a seed different with ThresholdSignatureScheme::SEED
                continue;
            }

            break ThresholdSignatureScheme::with_seed(threshold - 1, &node_ids, fake_seed);
        };

        let mut value = 0;
        let mut view = 1;

        // iterator node ids, make each of them as a view promoter
        for i in &node_ids {
            let promoter = *i as NodeId;
            // for each view, when step=1 the in proof is None
            let mut in_proof: Option<Signature> = None;

            // create promote value
            let promote_value = PromoteValue {
                node_id: promoter,
                value: value.to_string(),
                message_id: value,
            };

            // the 4 step promote
            for step in 1..5 {
                println!(
                    "run view: {}, step: {}, promoter: {}, value: {}",
                    view, step, promoter, value
                );

                // if there is proof of last step, validate it
                if let Some(in_proof) = &in_proof {
                    // in proof is not None only when step > 1
                    assert!(step > 1);

                    // validate the in proof of last step
                    let proof_value = ProofValue {
                        stage: Stage {
                            step: step - 1 as Step,
                            view,
                        },
                        value: promote_value.clone(),
                    };
                    let json = serde_json::to_string(&proof_value)?;

                    assert!(threshold_signature.threshold_validate(&json, in_proof));
                } else {
                    // in proof is None only when step=1
                    assert!(step == 1);
                }

                // validate last step promote value success,
                // now each node generate their share signature of this step

                // proof value of this step
                let proof_value = ProofValue {
                    stage: Stage {
                        step: step as Step,
                        view,
                    },
                    value: promote_value.clone(),
                };
                let json = serde_json::to_string(&proof_value)?;
                let mut share_signs: Vec<(NodeId, SignatureShare)> = Vec::new();
                // reset the proof before nodes generating share sign loop
                in_proof = None;
                for i in &node_ids {
                    let id = *i;

                    let share_sign = if !adversary_ids.contains(&id) {
                        // each node generate their share signature of this step
                        let share_sign = threshold_signature.share_sign(id, &json)?;
                        // assert the share signature is validate
                        assert!(threshold_signature.share_validate(id, &json, &share_sign));
                        share_sign
                    } else {
                        // generate a fake share signature of this step
                        let share_sign = fake_threshold_signature.share_sign(id, &json)?;
                        // assert the fake share signature is invalidate
                        assert!(!threshold_signature.share_validate(id, &json, &share_sign));
                        share_sign
                    };

                    // only when the share signature is validate, push it into share_signs
                    if threshold_signature.share_validate(id, &json, &share_sign) {
                        share_signs.push((id, share_sign));
                    }
                    // collect enough validate share signature, create threshold signature as in proof for the next step
                    if share_signs.len() == threshold {
                        // save the in proof for the next step
                        in_proof = Some(threshold_signature.threshold_sign(&share_signs)?);
                    }
                }
                // when out of nodes generating share sign loop, assert the proof is generated
                assert!(in_proof.is_some());
            }

            // move to the next view and new promote value
            view += 1;
            value += 1;
        }
        Ok(())
    }
}
