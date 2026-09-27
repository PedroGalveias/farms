use thiserror::Error;

pub type NodeId = u32;
pub type EdgeId = u32;
pub type EdgeCost = u32; // Stored deciseconds for one edge or landmark cell.
pub type Cost = u64; // Checked in-memory total for one query.

pub const INFINITY: Cost = u64::MAX;
pub const STORED_INFINITY: EdgeCost = u32::MAX;
pub const NO_NODE: NodeId = u32::MAX;
pub const NO_EDGE: EdgeId = u32::MAX;
pub const MAX_EDGE_COST: EdgeCost = u32::MAX - 1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coord {
    pub e: f64, // LV95 easting in metres
    pub n: f64, // LV95 northing in metres
}

impl Coord {
    pub fn distance_m(self, other: Self) -> f64 {
        (self.e - other.e).hypot(self.n - other.n)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeRef {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    pub cost: EdgeCost,
    pub speed_mm_per_s: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchPath {
    pub nodes: Vec<NodeId>,
    pub edges: Vec<EdgeId>,
    pub cost: Cost,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RoutingError {
    #[error("node {0} is outside the graph")]
    InvalidNode(NodeId),
    #[error("edge {0} is outside the graph")]
    InvalidEdge(EdgeId),
    #[error("edge cost must be in 1..={MAX_EDGE_COST}, got {0}")]
    InvalidEdgeCost(EdgeCost),
    #[error("cost arithmetic overflowed or reached the reserved infinity value")]
    CostOverflow,
    #[error("a landmark distance cannot be represented in the u32 artifact table")]
    LandmarkDistanceOverflow,
    #[error("graph invariant failed: {0}")]
    InvalidGraph(&'static str),
}

pub fn add_edge_cost(a: Cost, b: EdgeCost) -> Result<Cost, RoutingError> {
    add_cost(a, Cost::from(b))
}

pub fn add_cost(a: Cost, b: Cost) -> Result<Cost, RoutingError> {
    a.checked_add(b)
        .filter(|&sum| sum != INFINITY)
        .ok_or(RoutingError::CostOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_costs_cannot_become_infinity_or_wrap() {
        assert_eq!(add_cost(0, 0), Ok(0));
        assert_eq!(add_cost(INFINITY - 2, 1), Ok(INFINITY - 1));
        assert_eq!(add_cost(INFINITY - 1, 1), Err(RoutingError::CostOverflow));
        assert_eq!(add_cost(INFINITY - 1, 2), Err(RoutingError::CostOverflow));
        assert_eq!(add_cost(INFINITY, 0), Err(RoutingError::CostOverflow));
        assert_eq!(
            add_edge_cost(0, MAX_EDGE_COST),
            Ok(Cost::from(MAX_EDGE_COST))
        );
        assert_eq!(
            add_edge_cost(INFINITY - 1, 1),
            Err(RoutingError::CostOverflow)
        );
    }
}
