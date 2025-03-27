use crate::base::Error;
use crate::base::ProposalMessage;
use crate::base::ProposalMessageResp;
use crate::base::Result;
use crate::base::Value;
use tokio::spawn;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::vaba_core::VabaCore;

pub struct Vbba {
    core_handle: JoinHandle<std::result::Result<(), Error>>,
    tx_api: UnboundedSender<ProposalMessage>,
    tx_shutdown: oneshot::Sender<()>,
}

impl Vbba {
    pub fn new() -> Self {
        let (tx_api, rx_api) = unbounded_channel();
        let (tx_shutdown, rx_shutdown) = oneshot::channel::<()>();

        let core = VabaCore::new(rx_api, rx_shutdown);
        let core_handle = spawn(core.main());
        Self {
            core_handle,
            tx_api,
            tx_shutdown,
        }
    }

    pub async fn proposal(&self, value: Value) -> Result<()> {
        let (tx, rx) = oneshot::channel::<ProposalMessageResp>();
        let message = ProposalMessage {
            value: value.clone(),
            sender: tx,
        };
        let send_res = self.tx_api.send(message);
        let resp = rx.await.map_err(Error::oneshot_recv_error(format!(
            "fail to recv response of proposal value {}",
            value
        )))?;

        Ok(())
    }
}
