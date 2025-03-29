use std::sync::atomic::AtomicU32;

use crate::base::ClusterConfig;
use crate::base::MessageId;
use crate::base::NodeId;
use crate::base::ProposalMessage;
use crate::base::ProposalMessageResp;
use crate::base::Value;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use tokio::spawn;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::vaba_core::VabaCore;

pub struct Vaba {
    core_handle: JoinHandle<std::result::Result<(), Error>>,
    tx_api: UnboundedSender<ProposalMessage>,
    tx_shutdown: oneshot::Sender<()>,
    node_id: NodeId,
}

fn generate_message_id(node_id: NodeId, msg_id: u32) -> u64 {
    ((node_id as u64) << 32) | (msg_id as u64)
}

impl Vaba {
    pub fn new(node_id: NodeId, cluster: ClusterConfig) -> Self {
        let (tx_api, rx_api) = unbounded_channel();
        let (tx_shutdown, rx_shutdown) = oneshot::channel::<()>();

        let core = VabaCore::new(node_id, rx_api, rx_shutdown, cluster);
        let core_handle = spawn(core.main());
        Self {
            core_handle,
            tx_api,
            tx_shutdown,
            node_id,
        }
    }

    pub async fn proposal(&self, message_id: MessageId, value: Value) -> Result<()> {
        let (tx, rx) = oneshot::channel::<ProposalMessageResp>();
        let message = ProposalMessage {
            value: value.clone(),
            sender: tx,
            message_id,
        };
        let send_res = self.tx_api.send(message);
        let resp = rx
            .await
            .context(format!("fail to recv response of proposal value {}", value))?;

        Ok(())
    }
}
