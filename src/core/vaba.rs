use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::OnceLock;

use actix_web::middleware;
use actix_web::HttpServer;
use log::info;
use tokio::sync::Mutex;

use crate::base::Message;
use crate::base::NodeId;
use tokio::spawn;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedSender;

use super::handlers;
use super::vaba_core::VabaCore;
use super::IdempotentId;

#[derive(Debug)]
pub struct Vaba {
    //core_handle: JoinHandle<std::result::Result<(), Error>>,
    pub tx_api: UnboundedSender<Message>,
    pub node_id: NodeId,
    pub idempotent_set: Arc<Mutex<BTreeSet<IdempotentId>>>,
}

static INSTANCE: OnceLock<Arc<Vaba>> = OnceLock::new();

impl Vaba {
    pub fn get_instance() -> &'static Arc<Vaba> {
        INSTANCE.get().unwrap()
    }

    // return true if handle this message before
    pub async fn has_handled(&self, id: IdempotentId) -> bool {
        let mut idempotent_set = self.idempotent_set.lock().await;
        if idempotent_set.contains(&id) {
            true
        } else {
            idempotent_set.insert(id);
            false
        }
    }

    pub async fn start(node_id: NodeId, nodes: BTreeMap<NodeId, String>) -> std::io::Result<()> {
        let (tx_api, rx_api) = unbounded_channel();

        let address = nodes.get(&node_id).unwrap();

        let core = VabaCore::new(node_id, rx_api, nodes.clone());
        let _core_handle = spawn(core.main());
        let vaba = Vaba {
            //core_handle,
            tx_api: tx_api.clone(),
            node_id,
            idempotent_set: Arc::new(Mutex::new(BTreeSet::new())),
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
                .service(handlers::metrics)
                // node internal RPC
                .service(handlers::promote)
                .service(handlers::ack)
                .service(handlers::done)
                .service(handlers::skip_share)
                .service(handlers::skip)
                .service(handlers::share)
                .service(handlers::view_change)
                .service(handlers::response)
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
