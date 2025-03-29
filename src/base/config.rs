use std::collections::BTreeMap;

use super::NodeId;

pub struct ClusterConfig {
    // (node id, node address)
    pub nodes: BTreeMap<NodeId, String>,
}
