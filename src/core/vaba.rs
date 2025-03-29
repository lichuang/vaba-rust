use std::sync::Arc;
use std::sync::OnceLock;

use actix_web::middleware;
use actix_web::middleware::Logger;
use actix_web::HttpServer;

use crate::base::ClusterConfig;
use crate::base::Message;
use crate::base::NodeId;
use anyhow::Error;
use tokio::spawn;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::handlers;
use super::vaba_core::VabaCore;

#[derive(Debug)]
pub struct Vaba {
    core_handle: JoinHandle<std::result::Result<(), Error>>,
    pub tx_api: UnboundedSender<Message>,
    tx_shutdown: oneshot::Sender<()>,
    pub node_id: NodeId,
    cluster: ClusterConfig,
}

static INSTANCE: OnceLock<Arc<Vaba>> = OnceLock::new();

impl Vaba {
    pub fn get_instance() -> &'static Arc<Vaba> {
        INSTANCE.get().unwrap()
    }

    pub async fn start(node_id: NodeId, cluster: ClusterConfig) -> std::io::Result<()> {
        let (tx_api, rx_api) = unbounded_channel();
        let (tx_shutdown, rx_shutdown) = oneshot::channel::<()>();

        let core = VabaCore::new(node_id, rx_api, rx_shutdown, cluster.clone());
        let core_handle = spawn(core.main());
        let vaba = Vaba {
            core_handle,
            tx_api: tx_api.clone(),
            tx_shutdown,
            node_id,
            cluster: cluster.clone(),
        };
        INSTANCE.set(Arc::new(vaba)).unwrap();
        // Start the actix-web server.
        let server = HttpServer::new(move || {
            actix_web::App::new()
                .wrap(Logger::default())
                .wrap(Logger::new("%a %{User-Agent}i"))
                .wrap(middleware::Compress::default())
                // client RPC
                .service(handlers::proposal)
                // node internal RPC
                .service(handlers::promote)
                .service(handlers::promote_ack)
        });

        let node_id = node_id;
        let address = cluster.nodes.get(&node_id).unwrap();
        let x = server.bind(address)?;

        x.run().await
    }
}
