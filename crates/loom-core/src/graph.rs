use crate::error::Result;
use crate::store::LoomDb;
use petgraph::algo::astar;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeMeta {
    pub relationship: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraversalEntry {
    pub symbol_id: i64,
    pub depth: usize,
    pub relationship: String,
    pub confidence: f64,
    pub direction: String,
}

#[derive(Debug, Default)]
pub struct SymbolGraph {
    graph: DiGraph<i64, EdgeMeta>,
    symbol_id_to_node: HashMap<i64, NodeIndex>,
}

impl SymbolGraph {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build_from_db(db: &LoomDb) -> Result<Self> {
        let mut graph = Self::new();
        for edge in db.get_resolved_edges()? {
            if let Some(target_id) = edge.target_id {
                graph.add_edge(
                    edge.source_id,
                    target_id,
                    edge.relationship,
                    edge.confidence,
                );
            }
        }
        Ok(graph)
    }

    pub fn add_edge(
        &mut self,
        source_id: i64,
        target_id: i64,
        relationship: String,
        confidence: f64,
    ) {
        let source = self.node_for_symbol(source_id);
        let target = self.node_for_symbol(target_id);
        if let Some(edge_index) = self.graph.find_edge(source, target) {
            let existing = self
                .graph
                .edge_weight(edge_index)
                .map(|meta| meta.confidence)
                .unwrap_or(0.0);
            if confidence <= existing {
                return;
            }
            if let Some(meta) = self.graph.edge_weight_mut(edge_index) {
                *meta = EdgeMeta {
                    relationship,
                    confidence,
                };
            }
            return;
        }
        self.graph.add_edge(
            source,
            target,
            EdgeMeta {
                relationship,
                confidence,
            },
        );
    }

    pub fn remove_node(&mut self, symbol_id: i64) {
        if let Some(node) = self.symbol_id_to_node.remove(&symbol_id) {
            self.graph.remove_node(node);
            self.rebuild_node_maps();
        }
    }

    #[must_use]
    pub fn dependents(&self, symbol_id: i64, max_depth: usize) -> Vec<TraversalEntry> {
        self.bfs(symbol_id, max_depth, Direction::Incoming)
    }

    #[must_use]
    pub fn dependencies(&self, symbol_id: i64, max_depth: usize) -> Vec<TraversalEntry> {
        self.bfs(symbol_id, max_depth, Direction::Outgoing)
    }

    #[must_use]
    pub fn shortest_path(&self, source_id: i64, target_id: i64) -> Option<Vec<i64>> {
        let source = *self.symbol_id_to_node.get(&source_id)?;
        let target = *self.symbol_id_to_node.get(&target_id)?;
        let (_cost, path) = astar(&self.graph, source, |node| node == target, |_| 1, |_| 0)?;
        path.into_iter()
            .map(|node| self.graph.node_weight(node).copied())
            .collect()
    }

    #[must_use]
    pub fn impact_radius(&self, symbol_id: i64, max_depth: usize) -> Vec<(i64, f64)> {
        let mut scored = self
            .dependents(symbol_id, max_depth)
            .into_iter()
            .map(|entry| {
                let decay = 1.0 / 2.0_f64.powi((entry.depth.saturating_sub(1)) as i32);
                (entry.symbol_id, entry.confidence * decay)
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.1.total_cmp(&left.1));
        scored
    }

    #[must_use]
    pub fn centrality(&self, top_n: usize) -> Vec<(i64, f64)> {
        if self.graph.node_count() == 0 || top_n == 0 {
            return Vec::new();
        }
        let denominator = (self.graph.node_count().saturating_sub(1)).max(1) as f64;
        let mut scores = self
            .graph
            .node_indices()
            .filter_map(|node| {
                let symbol_id = self.graph.node_weight(node).copied()?;
                let incoming = self
                    .graph
                    .edges_directed(node, Direction::Incoming)
                    .filter(|edge| edge.source() != edge.target())
                    .count() as f64;
                Some((symbol_id, incoming / denominator))
            })
            .collect::<Vec<_>>();
        scores.sort_by(|left, right| right.1.total_cmp(&left.1));
        scores.truncate(top_n);
        scores
    }

    #[must_use]
    pub fn neighbors_with_metadata(&self, symbol_id: i64, max_depth: usize) -> Vec<TraversalEntry> {
        let mut best: HashMap<i64, TraversalEntry> = HashMap::new();
        for entry in self.dependents(symbol_id, max_depth) {
            best.entry(entry.symbol_id)
                .and_modify(|existing| {
                    if entry.confidence > existing.confidence {
                        *existing = entry.clone();
                    }
                })
                .or_insert(entry);
        }
        for entry in self.dependencies(symbol_id, max_depth) {
            best.entry(entry.symbol_id)
                .and_modify(|existing| {
                    if entry.confidence > existing.confidence {
                        *existing = entry.clone();
                    }
                })
                .or_insert(entry);
        }
        best.into_values().collect()
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    fn node_for_symbol(&mut self, symbol_id: i64) -> NodeIndex {
        if let Some(node) = self.symbol_id_to_node.get(&symbol_id) {
            return *node;
        }
        let node = self.graph.add_node(symbol_id);
        self.symbol_id_to_node.insert(symbol_id, node);
        node
    }

    fn bfs(&self, symbol_id: i64, max_depth: usize, direction: Direction) -> Vec<TraversalEntry> {
        if max_depth == 0 {
            return Vec::new();
        }
        let Some(source) = self.symbol_id_to_node.get(&symbol_id).copied() else {
            return Vec::new();
        };

        let mut visited = HashSet::from([source]);
        let mut queue = VecDeque::from([(source, 0usize)]);
        let mut results = Vec::new();

        while let Some((node, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for edge in self.graph.edges_directed(node, direction) {
                let next = match direction {
                    Direction::Outgoing => edge.target(),
                    Direction::Incoming => edge.source(),
                };
                if !visited.insert(next) {
                    continue;
                }
                let next_depth = depth + 1;
                if let Some(symbol_id) = self.graph.node_weight(next).copied() {
                    results.push(TraversalEntry {
                        symbol_id,
                        depth: next_depth,
                        relationship: edge.weight().relationship.clone(),
                        confidence: edge.weight().confidence,
                        direction: match direction {
                            Direction::Incoming => "incoming",
                            Direction::Outgoing => "outgoing",
                        }
                        .to_string(),
                    });
                }
                queue.push_back((next, next_depth));
            }
        }
        results
    }

    fn rebuild_node_maps(&mut self) {
        self.symbol_id_to_node.clear();
        for node in self.graph.node_indices() {
            if let Some(symbol_id) = self.graph.node_weight(node).copied() {
                self.symbol_id_to_node.insert(symbol_id, node);
            }
        }
    }
}
