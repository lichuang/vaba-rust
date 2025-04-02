use std::collections::BTreeMap;
use std::time::Instant;

use clap::Parser;
use reqwest::Client;
use serde_json::Value;
use tokio::spawn;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

type MessageId = u64;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    // number of values
    #[arg(short, long, default_value = "100")]
    number: u32,

    // cluster config, [id:address]+
    #[arg(short, long)]
    cluster: String,
}

struct NodeValue {
    json: String,
    sender: oneshot::Sender<()>,
}

struct NodeClient {
    pub id: u32,
    pub address: String,
    pub rx_api: UnboundedReceiver<NodeValue>,
    http_client: Client,
    uri: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProposalMessage {
    pub message_id: MessageId,
    pub value: String,
}

impl NodeClient {
    pub fn new(id: u32, address: String, rx_api: UnboundedReceiver<NodeValue>) -> Self {
        Self {
            uri: format!("http://{}/proposal", address),
            id,
            address,
            rx_api,
            http_client: Client::new(),
        }
    }

    pub async fn main(mut self) -> anyhow::Result<()> {
        while let Some(value) = self.rx_api.recv().await {
            //println!("recv value: {:?} to {:?}", value.json, self.uri);

            let resp = self
                .http_client
                .post(&self.uri)
                .header("Content-Type", "application/json")
                .body(value.json.clone())
                .send()
                .await;
            if let Err(e) = resp {
                println!(
                    "send data to node {}/{} error: {:?}",
                    self.id, self.address, e
                );
            }

            let _ = value.sender.send(());

            //println!("send value: {:?}", value.value)
        }
        Ok(())
    }
}

struct NodeHandle {
    pub tx_api: UnboundedSender<NodeValue>,
    pub handle: JoinHandle<std::result::Result<(), anyhow::Error>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let json = parse_to_json(&args.cluster).unwrap();

    let mut nodes: BTreeMap<u32, NodeHandle> = BTreeMap::new();
    let mut nodes_address: BTreeMap<u32, String> = BTreeMap::new();
    if let Value::Object(map) = json {
        for (id, value) in map {
            let id = id.parse::<u32>().unwrap();
            //nodes.insert(id, value.as_str().unwrap().to_string());
            let (tx_api, rx_api) = unbounded_channel();
            let address = value.as_str().unwrap().to_string();
            let node = NodeClient::new(id, address.clone(), rx_api);

            let handle = spawn(node.main());
            let node_handle = NodeHandle { tx_api, handle };

            nodes.insert(id, node_handle);
            nodes_address.insert(id, address);
        }
    }
    if nodes.len() <= 3 {
        println!("expected number of node id > 3, actual {}", nodes.len());
        return Ok(());
    }

    let faulty = nodes.len() / 3;
    let threshold = nodes.len() - faulty;
    println!(
        "nodes: {}, faulty: {:?}, threshold: {:?}",
        nodes.len(),
        faulty,
        threshold
    );

    let mut total_time = 0;
    for i in 0..args.number {
        println!("proposal value {}", i);
        let start = Instant::now();
        let msg = ProposalMessage {
            message_id: start.elapsed().as_secs() as u64,
            value: i.to_string(),
        };
        let json = serde_json::to_string(&msg)?;

        let mut rx_vec = Vec::with_capacity(nodes.len());
        for (id, node) in nodes.iter() {
            let (sender, rx) = oneshot::channel::<()>();
            let value = NodeValue {
                json: json.clone(),
                sender,
            };
            let _ = node.tx_api.send(value);
            rx_vec.push((id, rx));
        }

        let mut n = 0;
        for (id, rx) in rx_vec {
            //rx.try_recv()
            let v = rx.await;
            println!("recv proposal value {} resp from node {}", i, id);
            if let Err(e) = v {
                println!("recv resp from node {} error {:?}", id, e);
            }
            n += 1;
            if n >= threshold {
                break;
            }
        }

        let time = Instant::now().duration_since(start).as_millis();
        total_time += time;
    }

    let http_client = Client::new();
    let (id, address) = nodes_address.first_key_value().unwrap();
    let uri = format!("http://{}/metrics", address);

    let resp = http_client.get(&uri).send().await;
    let metrics: Metrics = match resp {
        Err(e) => {
            println!("send data to node {}/{} error: {:?}", id, address, e);
            return Ok(());
        }
        Ok(resp) => serde_json::from_str(&resp.text().await?)?,
    };

    println!(
        "total count: {}, total time: {}, avg time: {}",
        args.number,
        total_time,
        total_time as f32 / args.number as f32
    );
    println!("node {} metrics: {:?}", id, metrics);

    Ok(())
}

#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
pub struct Metrics {
    recv_proposal: u64,

    send_promote: u64,
    send_ack: u64,
    send_done: u64,
    send_skip_share: u64,
    send_skip: u64,
    send_share: u64,
    send_view_change: u64,
}

fn parse_to_json(input: &str) -> serde_json::Result<Value> {
    let trimmed = input.trim_matches(|c| c == '{' || c == '}');

    let pairs: Vec<&str> = trimmed.split(';').collect();
    let mut json_map = serde_json::Map::new();
    for pair in pairs {
        let parts: Vec<&str> = pair.split(':').collect();
        if parts.len() == 3 {
            let key = parts[0];
            let ip = parts[1];
            let port = parts[2];

            let address = format!("{}:{}", ip, port);
            json_map.insert(key.to_string(), Value::String(address));
        }
    }
    Ok(Value::Object(json_map))
}
