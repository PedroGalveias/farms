use crate::types::{EdgeCost, NodeId};

#[derive(Clone, Copy, Debug)]
pub struct RawEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub cost: EdgeCost,
    pub speed_mm_per_s: u32,
}
