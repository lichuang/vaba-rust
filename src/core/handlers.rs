use actix_web::{post, web::Json, Responder};
use log::{error, info};
use tokio::sync::oneshot;

use crate::{
    base::{
        AckMessage, ClientProposalMessage, DoneMessage, Message, PromoteMessage, ProposalMessage,
        ProposalMessageResp, ShareMessage, SkipMessage, SkipShareMessage, ViewChangeMessage,
    },
    core::vaba::Vaba,
};

#[post("/proposal")]
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

    info!(
        "node {} recv promote message: {:?} from node {}",
        app.node_id, message.value.message_id, message.node_id
    );
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

#[post("/ack")]
pub async fn ack(req: Json<AckMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();

    info!(
        "node {} recv ack message: {:?} from node {}",
        app.node_id, message.message_id, message.node_id
    );
    let message_id = message.message_id;
    let send_res = app.tx_api.send(Message::Ack(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core ack message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(()))
}

#[post("/done")]
pub async fn done(req: Json<DoneMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();

    info!(
        "node {} recv done message: {:?} from node {}",
        app.node_id, message.value.message_id, message.node_id
    );
    let message_id = message.value.message_id;
    let send_res = app.tx_api.send(Message::Done(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core done message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(()))
}

#[post("/skip-share")]
pub async fn skip_share(req: Json<SkipShareMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();

    info!(
        "node {} recv skip share message: {:?} from node {}",
        app.node_id, message.message_id, message.node_id
    );
    let message_id = message.message_id;
    let send_res = app.tx_api.send(Message::SkipShare(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core skip share message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(()))
}

#[post("/skip")]
pub async fn skip(req: Json<SkipMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();

    info!(
        "node {} recv skip message: {:?} from {}",
        app.node_id, message.message_id, message.node_id
    );
    let message_id = message.message_id;
    let send_res = app.tx_api.send(Message::Skip(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core skip message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(()))
}

#[post("/share")]
pub async fn share(req: Json<ShareMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();

    info!(
        "node {} recv share message: {:?} from {}",
        app.node_id, message.message_id, message.node_id
    );
    let message_id = message.message_id;
    let send_res = app.tx_api.send(Message::Share(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core share message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(()))
}

#[post("/view-change")]
pub async fn view_change(req: Json<ViewChangeMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();

    info!(
        "node {} recv view-change message: {:?} from {}",
        app.node_id, message.message_id, message.node_id
    );
    let message_id = message.message_id;
    let send_res = app.tx_api.send(Message::ViewChangeMessage(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core view-change message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(()))
}
