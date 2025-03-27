use tokio::sync::oneshot;

use super::Value;

pub struct ProposalMessage {
    pub value: Value,
    pub sender: oneshot::Sender<ProposalMessageResp>,
}

pub struct ProposalMessageResp {
    ok: bool,
    error: Option<String>,
}
