use std::collections::BTreeMap;

use super::NodeId;

#[derive(Clone, Debug)]
pub struct ClusterConfig {
    // (node id, node address)
    pub nodes: BTreeMap<NodeId, String>,
}
