use std::collections::BTreeMap;

use clap::Parser;
use reqwest::Client;
use serde_json::Value;
use tokio::spawn;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

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
    value: u32,
    sender: oneshot::Sender<u32>,
}

struct NodeClient {
    pub id: u32,
    pub address: String,
    pub rx_api: UnboundedReceiver<NodeValue>,
    pub http_client: Client,
}

impl NodeClient {
    pub fn new(id: u32, address: String, rx_api: UnboundedReceiver<NodeValue>) -> Self {
        Self {
            id,
            address,
            rx_api,
            http_client: Client::new(),
        }
    }

    pub async fn main(mut self) -> anyhow::Result<()> {
        while let Some(value) = self.rx_api.recv().await {
            println!("recv value: {:?}", value.value);

            let _ = value.sender.send(value.value);

            println!("send value: {:?}", value.value)
        }
        Ok(())
    }
}

struct NodeHandle {
    pub tx_api: UnboundedSender<NodeValue>,
    pub handle: JoinHandle<std::result::Result<(), anyhow::Error>>,
}

fn parse_to_json(input: &str) -> serde_json::Result<Value> {
    // 移除首尾的 `{` 和 `}`
    let trimmed = input.trim_matches(|c| c == '{' || c == '}');

    // 按 `;` 分割键值对
    let pairs: Vec<&str> = trimmed.split(';').collect();
    // 构建 JSON 对象
    let mut json_map = serde_json::Map::new();
    for pair in pairs {
        let parts: Vec<&str> = pair.split(':').collect();
        if parts.len() == 3 {
            let key = parts[0];
            let ip = parts[1];
            let port = parts[2];

            // 组合 IP 和端口
            let address = format!("{}:{}", ip, port);
            json_map.insert(key.to_string(), Value::String(address));
        }
    }
    Ok(Value::Object(json_map))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let json = parse_to_json(&args.cluster).unwrap();

    let mut nodes: BTreeMap<u32, NodeHandle> = BTreeMap::new();
    if let Value::Object(map) = json {
        for (id, value) in map {
            let id = id.parse::<u32>().unwrap();
            //nodes.insert(id, value.as_str().unwrap().to_string());
            let (tx_api, rx_api) = unbounded_channel();
            let node = NodeClient::new(id, value.as_str().unwrap().to_string(), rx_api);

            let handle = spawn(node.main());
            let node_handle = NodeHandle { tx_api, handle };

            nodes.insert(id, node_handle);
        }
    }

    for (id, node) in nodes.iter() {
        let (sender, rx) = oneshot::channel::<u32>();
        let value = NodeValue { value: 1, sender };
        let _ = node.tx_api.send(value);
        let v = rx.await?;
        println!("recv {} from node {}", v, id);
    }
    for i in 0..args.number {
        //let mut handles = Vec::with_capacity(nodes.len());
        for node in nodes.values() {}
    }

    Ok(())
}
