use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::OnceLock;

use actix_web::middleware;
use actix_web::middleware::Logger;
use actix_web::HttpServer;
use log::info;

use crate::base::Message;
use crate::base::NodeId;
use anyhow::Error;
use tokio::spawn;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

use super::handlers;
use super::vaba_core::VabaCore;

#[derive(Debug)]
pub struct Vaba {
    core_handle: JoinHandle<std::result::Result<(), Error>>,
    pub tx_api: UnboundedSender<Message>,
    pub node_id: NodeId,
}

static INSTANCE: OnceLock<Arc<Vaba>> = OnceLock::new();

impl Vaba {
    pub fn get_instance() -> &'static Arc<Vaba> {
        INSTANCE.get().unwrap()
    }

    pub async fn start(node_id: NodeId, nodes: BTreeMap<NodeId, String>) -> std::io::Result<()> {
        let (tx_api, rx_api) = unbounded_channel();

        let address = nodes.get(&node_id).unwrap();

        let core = VabaCore::new(node_id, rx_api, nodes.clone());
        let core_handle = spawn(core.main());
        let vaba = Vaba {
            core_handle,
            tx_api: tx_api.clone(),
            node_id,
        };
        INSTANCE.set(Arc::new(vaba)).unwrap();
        // Start the actix-web server.
        let server = HttpServer::new(move || {
            actix_web::App::new()
                //.wrap(Logger::default())
                //.wrap(Logger::new("%a %{User-Agent}i"))
                .wrap(middleware::Compress::default())
                // client RPC
                .service(handlers::proposal)
                // node internal RPC
                .service(handlers::promote)
                .service(handlers::ack)
                .service(handlers::done)
                .service(handlers::skip_share)
                .service(handlers::share)
                .service(handlers::view_change)
        });

        let x = server.bind(address)?;
        info!(
            "node {:?} start listening on address {:?} ..",
            node_id, address
        );
        let ret = x.run().await;

        info!("node {} stopped", node_id);

        ret
    }
}
