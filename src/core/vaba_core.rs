use std::sync::Arc;

use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::sync::Notify;

use crate::base::Proof;
use crate::base::ProposalMessage;
use crate::base::Result;
use crate::base::Value;
use crate::base::View;
use tokio::sync::mpsc::UnboundedReceiver;

struct KeyState {
    pub view: View,
    pub proof: Proof,
    pub value: Value,
}

struct VabaState {
    // the highest view number for which the party ever received
    // a view-change message that includes a lock that was delivered
    // in the Proposal-Promotion of the chosen leader of this view.
    pub lock: View,

    // The KEY variable stores the 3-tuple: view, proof and value,
    // derived from the maximum view for which the party ever
    // received a view-change message that includes a key
    // (composing of value and proof) that was delivered
    // in the ProposalPromotion of the chosen leader of this view.
    pub key: Option<KeyState>,
}

pub struct VabaCore {
    state: VabaState,

    rx_api: UnboundedReceiver<ProposalMessage>,

    rx_shutdown: oneshot::Receiver<()>,

    proposal_values: Arc<Mutex<Vec<ProposalMessage>>>,
}

impl Default for VabaState {
    fn default() -> Self {
        Self { lock: 0, key: None }
    }
}

impl VabaCore {
    pub fn new(
        rx_api: UnboundedReceiver<ProposalMessage>,
        rx_shutdown: oneshot::Receiver<()>,
    ) -> Self {
        let core = Self {
            state: VabaState::default(),
            rx_api,
            rx_shutdown,
            proposal_values: Arc::new(Mutex::new(Vec::new())),
        };

        core
    }

    pub fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn proposal(&self, message: ProposalMessage) -> Result<()> {
        let mut proposal_values = self.proposal_values.lock().await;
        proposal_values.push(message);
        Ok(())
    }

    pub async fn main(mut self) -> Result<()> {
        Ok(())
    }

    // core of VABA algorithm
    async fn core(&mut self) -> Result<()> {
        Ok(())
    }
}
