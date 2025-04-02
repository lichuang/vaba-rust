mod base;
mod core;
mod crypto;

use std::collections::BTreeMap;

use base::NodeId;
use chrono::Local;
use clap::Parser;
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    id: u64,

    // cluster config, [id:address]+
    #[arg(short, long)]
    nodes: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let json = parse_to_json(&args.nodes).unwrap();

    let mut nodes: BTreeMap<NodeId, String> = BTreeMap::new();
    if let Value::Object(map) = json {
        for (id, value) in map {
            let id = id.parse::<NodeId>().unwrap();
            let address = value.as_str().unwrap().to_string();

            nodes.insert(id, address);
        }
    }

    if !nodes.contains_key(&args.id) {
        println!("node id {:?} not contain in the node map", args.id);
        return Ok(());
    }
    if nodes.len() <= 3 {
        println!("expected number of node id > 3, actual {}", nodes.len());
        return Ok(());
    }
    setup_logger(args.id).unwrap();
    core::Vaba::start(args.id, nodes).await?;

    Ok(())
}

fn setup_logger(id: NodeId) -> Result<(), Box<dyn std::error::Error>> {
    let log_file_name = format!("node-{}.log", id);

    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_name)?;

    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .format(|buf, record| {
            writeln!(
                buf,
                "[{} {}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .target(env_logger::Target::Pipe(Box::new(log_file))) // 输出到文件
        .init();

    Ok(())
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
