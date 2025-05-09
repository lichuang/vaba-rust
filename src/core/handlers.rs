use actix_web::{get, post, web::Json, Responder};
use log::{error, info};
use tokio::sync::oneshot;

use crate::{
    base::{
        AckMessage, ClientProposalMessage, DoneMessage, Message, MetricsMessage,
        MetricsMessageResp, PromoteMessage, ProposalMessage, ProposalMessageResp, RespMessage,
        ShareMessage, SkipMessage, SkipShareMessage, ViewChange,
    },
    core::{vaba::Vaba, Metrics},
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
    let idempotent_id = message.idempotent_id;

    if app.has_handled(idempotent_id).await {
        return Ok(Json(RespMessage {
            id: idempotent_id,
            node_id: app.node_id,
        }));
    }
    info!(
        "node {} recv promote message: {:?} from node {}",
        app.node_id, message.value.message_id, message.value.node_id
    );
    let message_id = message.value.message_id;
    let send_res = app.tx_api.send(Message::Promote(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core promote message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(RespMessage {
        id: idempotent_id,
        node_id: app.node_id,
    }))
}

#[post("/ack")]
pub async fn ack(req: Json<AckMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();
    let idempotent_id = message.idempotent_id;

    if app.has_handled(idempotent_id).await {
        return Ok(Json(RespMessage {
            id: idempotent_id,
            node_id: app.node_id,
        }));
    }

    info!(
        "node {} recv ack message: {:?} from node {}",
        app.node_id, message.message_id, message.from
    );
    let message_id = message.message_id;
    let send_res = app.tx_api.send(Message::Ack(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core ack message {} error {:?}",
            app.node_id,
            message_id,
            e.to_string()
        );
    }

    Ok(Json(RespMessage {
        id: idempotent_id,
        node_id: app.node_id,
    }))
}

#[post("/done")]
pub async fn done(req: Json<DoneMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();
    let idempotent_id = message.idempotent_id;

    if app.has_handled(idempotent_id).await {
        return Ok(Json(RespMessage {
            id: idempotent_id,
            node_id: app.node_id,
        }));
    }

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

    Ok(Json(RespMessage {
        id: idempotent_id,
        node_id: app.node_id,
    }))
}

#[post("/skip-share")]
pub async fn skip_share(req: Json<SkipShareMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();
    let idempotent_id = message.idempotent_id;

    if app.has_handled(idempotent_id).await {
        return Ok(Json(RespMessage {
            id: idempotent_id,
            node_id: app.node_id,
        }));
    }

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

    Ok(Json(RespMessage {
        id: idempotent_id,
        node_id: app.node_id,
    }))
}

#[post("/skip")]
pub async fn skip(req: Json<SkipMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();
    let idempotent_id = message.idempotent_id;

    if app.has_handled(idempotent_id).await {
        return Ok(Json(RespMessage {
            id: idempotent_id,
            node_id: app.node_id,
        }));
    }

    info!(
        "node {} recv skip message: {:?} from {}",
        app.node_id, idempotent_id, message.node_id
    );
    let message_id = message.message_id;
    let send_res = app.tx_api.send(Message::Skip(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core skip message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(RespMessage {
        id: idempotent_id,
        node_id: app.node_id,
    }))
}

#[post("/share")]
pub async fn share(req: Json<ShareMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();
    let idempotent_id = message.idempotent_id;

    if app.has_handled(idempotent_id).await {
        return Ok(Json(RespMessage {
            id: idempotent_id,
            node_id: app.node_id,
        }));
    }

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

    Ok(Json(RespMessage {
        id: idempotent_id,
        node_id: app.node_id,
    }))
}

#[post("/view-change")]
pub async fn view_change(req: Json<ViewChange>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();
    let idempotent_id = message.idempotent_id;

    if app.has_handled(idempotent_id).await {
        return Ok(Json(RespMessage {
            id: idempotent_id,
            node_id: app.node_id,
        }));
    }

    info!(
        "node {} recv view-change message: {:?} from {}",
        app.node_id, message.message_id, message.node_id
    );
    let message_id = message.message_id;
    let send_res = app.tx_api.send(Message::ViewChange(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core view-change message {} error {:?}",
            app.node_id, message_id, e
        );
    }

    Ok(Json(RespMessage {
        id: idempotent_id,
        node_id: app.node_id,
    }))
}

#[post("/response")]
pub async fn response(req: Json<RespMessage>) -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();
    let message = req.into_inner();
    let id = message.id;

    info!("node {} recv response message: {:?}", app.node_id, message);
    let send_res = app.tx_api.send(Message::Response(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core response message {} error {:?}",
            app.node_id, id, e
        );
    }

    Ok(Json(()))
}

#[get("/metrics")]
pub async fn metrics() -> actix_web::Result<impl Responder> {
    let app = Vaba::get_instance();

    let (tx, rx) = oneshot::channel::<MetricsMessageResp>();
    let message = MetricsMessage { sender: tx };

    info!("node {} recv metrics message", app.node_id);
    let send_res = app.tx_api.send(Message::Metrics(message));
    if let Err(e) = send_res {
        error!(
            "send to node {} core metrics message error {:?}",
            app.node_id, e
        );
    }

    let resp = rx.await;
    let metrics = match resp {
        Ok(resp) => resp.metrics,
        Err(e) => {
            error!(
                "recv resp from node {} core metrics error {:?}",
                app.node_id, e
            );
            Metrics::default()
        }
    };

    Ok(Json(metrics))
}
