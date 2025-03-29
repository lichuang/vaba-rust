use actix_web::{post, web::Json, Responder};
use log::{error, info};
use tokio::sync::oneshot;

use crate::{
    base::{
        ClientProposalMessage, Message, PromoteAckMessage, PromoteMessage, ProposalMessage,
        ProposalMessageResp,
    },
    core::vaba::Vaba,
};

#[post("/prosoal")]
pub async fn proposal(req: Json<ClientProposalMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let msg = req.into_inner();

    info!("node {} recv proposal message: {:?}", app.node_id, msg);

    let (tx, rx) = oneshot::channel::<ProposalMessageResp>();
    let message_id = msg.message_id;
    let message = ProposalMessage {
        message_id: msg.message_id,
        value: msg.value,
        sender: tx,
    };
    let send_res = app.tx_api.send(Message::Proposal(message));
    if let Err(e) = send_res {
        let resp = ProposalMessageResp {
            ok: false,
            error: Some(e.to_string()),
        };
        error!(
            "send to node {} core proposal message {} error {:?}",
            app.node_id, message_id, e
        );
        return Ok(Json(resp));
    }
    let resp = rx.await;
    let resp = match resp {
        Ok(resp) => resp,
        Err(e) => {
            error!(
                "recv resp from node {} core proposal message {} error {:?}",
                app.node_id, message_id, e
            );
            ProposalMessageResp {
                ok: false,
                error: Some(e.to_string()),
            }
        }
    };

    Ok(Json(resp))
}

#[post("/promote")]
pub async fn promote(req: Json<PromoteMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();

    info!("node {} recv promote message: {:?}", app.node_id, message);
    let message_id = message.value.message_id;
    let send_res = app.tx_api.send(Message::Promote(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core promote message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(()))
}

#[post("/promote-ack")]
pub async fn promote_ack(req: Json<PromoteAckMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();

    info!(
        "node {} recv promote resp message: {:?}",
        app.node_id, message
    );
    let message_id = message.message_id;
    let send_res = app.tx_api.send(Message::PromoteAck(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core promote resp message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(()))
}
