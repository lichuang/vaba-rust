use actix_web::{
    post,
    web::{Data, Json},
    Responder,
};
use tokio::sync::oneshot;

use crate::{
    base::{ClientProposalMessage, ProposalMessage, ProposalMessageResp},
    core::vaba::Vaba,
};

#[post("/prosoal")]
pub async fn proposal(req: Json<ClientProposalMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();

    let (tx, rx) = oneshot::channel::<ProposalMessageResp>();
    let message = ProposalMessage {
        value: req.value.clone(),
        sender: tx,
        message_id: req.message_id,
    };
    let send_res = app.tx_api.send(message);
    if let Err(e) = send_res {
        let resp = ProposalMessageResp {
            ok: false,
            error: Some(e.to_string()),
        };
        return Ok(Json(resp));
    }
    let resp = rx.await;
    let resp = match resp {
        Ok(resp) => resp,
        Err(e) => ProposalMessageResp {
            ok: false,
            error: Some(e.to_string()),
        },
    };

    Ok(Json(resp))
}
