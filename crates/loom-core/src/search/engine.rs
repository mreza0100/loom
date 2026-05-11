use crate::{
    embedder::{build_symbol_text, Embedder},
    graph::SymbolGraph,
    models::{CoupledSymbol, SearchResult, Symbol},
    search::scoring::{compute_evolutionary, compute_semantic, compute_structural, fuse_signals},
    store::LoomDb,
    LoomConfig, Result,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const RRF_K: f64 = 60.0;
const MAX_STRUCTURAL_RESULTS: usize = 30;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NeighborhoodResult {
    pub anchor: Option<Symbol>,
    pub coupled: Vec<CoupledSymbol>,
}

pub struct SearchEngine<E: Embedder> {
    db: Arc<LoomDb>,
    embedder: Arc<E>,
    graph: Option<Arc<SymbolGraph>>,
    config: LoomConfig,
}

impl<E: Embedder> SearchEngine<E> {
    #[must_use]
    pub fn new(
        db: Arc<LoomDb>,
        embedder: Arc<E>,
        graph: Option<Arc<SymbolGraph>>,
        config: LoomConfig,
    ) -> Self {
        Self {
            db,
            embedder,
            graph,
            config,
        }
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        kind: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        let candidate_limit = if kind.is_some() {
            limit.saturating_mul(10)
        } else {
            limit.saturating_mul(3)
        };
        let fts_results = self.db.search_fts(query, candidate_limit)?;
        let embedding = self.embedder.embed_single(query)?;
        let vec_results = self.db.search_vectors(&embedding, candidate_limit)?;

        let mut scores = BTreeMap::<i64, f64>::new();
        let mut symbols = BTreeMap::<i64, Symbol>::new();
        for (rank, symbol) in fts_results.into_iter().enumerate() {
            let Some(symbol_id) = symbol.id else {
                continue;
            };
            *scores.entry(symbol_id).or_default() += rrf_score(rank) * kind_boost(&symbol.kind);
            symbols.insert(symbol_id, symbol);
        }

        for (rank, (symbol_id, _distance)) in vec_results.into_iter().enumerate() {
            *scores.entry(symbol_id).or_default() += rrf_score(rank);
            if let std::collections::btree_map::Entry::Vacant(entry) = symbols.entry(symbol_id) {
                if let Some(symbol) = self.db.get_symbol_by_id(symbol_id)? {
                    *scores.entry(symbol_id).or_default() +=
                        rrf_score(rank) * (kind_boost(&symbol.kind) - 1.0);
                    entry.insert(symbol);
                }
            }
        }

        let mut ranked = normalize_scores(scores.into_iter().collect());
        ranked.sort_by(|left, right| right.1.total_cmp(&left.1));
        let mut results = Vec::new();
        for (symbol_id, score) in ranked {
            let Some(symbol) = symbols.get(&symbol_id).cloned() else {
                continue;
            };
            if kind.is_some_and(|expected| symbol.kind != expected) {
                continue;
            }
            let mut coupled = self.find_coupled(&symbol)?;
            coupled.truncate(self.config.top_coupled);
            results.push(SearchResult {
                coupled,
                symbol,
                score,
            });
            if results.len() == limit {
                break;
            }
        }
        Ok(results)
    }

    pub fn related(
        &self,
        symbol: &str,
        file: Option<&str>,
        kind: Option<&str>,
    ) -> Result<Vec<CoupledSymbol>> {
        let Some(target) = self
            .db
            .get_symbol_by_name_fuzzy(symbol, file)?
            .into_iter()
            .next()
        else {
            return Ok(Vec::new());
        };
        let mut coupled = self.find_coupled(&target)?;
        if let Some(kind) = kind {
            coupled.retain(|entry| entry.symbol.kind == kind);
        }
        Ok(coupled)
    }

    pub fn impact(
        &self,
        symbol: &str,
        file: Option<&str>,
        kind: Option<&str>,
    ) -> Result<Vec<CoupledSymbol>> {
        let Some(target) = self
            .db
            .get_symbol_by_name_fuzzy(symbol, file)?
            .into_iter()
            .next()
        else {
            return Ok(Vec::new());
        };
        let mut impact = Vec::new();
        let mut seen = BTreeSet::from_iter(target.id);
        if let (Some(graph), Some(target_id)) = (&self.graph, target.id) {
            for (symbol_id, score) in graph.impact_radius(target_id, 3) {
                if !seen.insert(symbol_id) {
                    continue;
                }
                let Some(symbol) = self.db.get_symbol_by_id(symbol_id)? else {
                    continue;
                };
                if is_generic_target(&symbol.name) {
                    continue;
                }
                let evolutionary = self.evolutionary_score(&target.file, &symbol.file)?;
                let fused = fuse_signals(score, 0.0, evolutionary, &self.config);
                impact.push(CoupledSymbol {
                    symbol,
                    score: fused.combined,
                    reason: fused.breakdown(),
                });
            }
        }

        for edge in target
            .id
            .map_or(Ok(Vec::new()), |id| self.db.get_edges_to(id))?
        {
            if !seen.insert(edge.source_id) {
                continue;
            }
            let Some(symbol) = self.db.get_symbol_by_id(edge.source_id)? else {
                continue;
            };
            if is_generic_target(&symbol.name) {
                continue;
            }
            impact.push(CoupledSymbol {
                symbol,
                score: 0.8,
                reason: format!("{} (structural)", edge.relationship),
            });
        }

        for edge in self.db.get_edges_to_by_name(&target.name)? {
            if edge.target_id.is_some() && edge.target_id != target.id {
                continue;
            }
            if !seen.insert(edge.source_id) {
                continue;
            }
            let Some(symbol) = self.db.get_symbol_by_id(edge.source_id)? else {
                continue;
            };
            if is_generic_target(&symbol.name) {
                continue;
            }
            impact.push(CoupledSymbol {
                symbol,
                score: 0.8,
                reason: format!("{} (structural)", edge.relationship),
            });
        }

        if let Some(kind) = kind {
            impact.retain(|entry| entry.symbol.kind == kind);
        }
        impact.sort_by(|left, right| right.score.total_cmp(&left.score));
        Ok(impact)
    }

    pub fn neighborhood(&self, file: &str, line: i64) -> Result<NeighborhoodResult> {
        let colocated = self.db.get_colocated_symbols(file)?;
        let anchor = colocated
            .iter()
            .find(|symbol| symbol.line <= line && line <= symbol.end_line)
            .cloned()
            .or_else(|| {
                colocated
                    .iter()
                    .min_by_key(|symbol| (symbol.line - line).abs())
                    .cloned()
            });
        let Some(anchor) = anchor else {
            return Ok(NeighborhoodResult {
                anchor: None,
                coupled: Vec::new(),
            });
        };

        let mut coupled = self.find_coupled(&anchor)?;
        let existing = coupled
            .iter()
            .filter_map(|entry| entry.symbol.id)
            .collect::<BTreeSet<_>>();
        for symbol in colocated {
            if symbol.id == anchor.id || symbol.id.is_some_and(|id| existing.contains(&id)) {
                continue;
            }
            coupled.push(CoupledSymbol {
                symbol,
                score: 0.4,
                reason: "co-located".to_string(),
            });
        }
        coupled.sort_by(|left, right| right.score.total_cmp(&left.score));
        Ok(NeighborhoodResult {
            anchor: Some(anchor),
            coupled,
        })
    }

    fn find_coupled(&self, target: &Symbol) -> Result<Vec<CoupledSymbol>> {
        let mut coupled = Vec::new();
        let mut seen = BTreeSet::from_iter(target.id);
        let mut structural_scores = BTreeMap::<i64, f64>::new();

        if let (Some(graph), Some(target_id)) = (&self.graph, target.id) {
            for entry in graph.neighbors_with_metadata(target_id, 2) {
                if !seen.insert(entry.symbol_id) {
                    continue;
                }
                let Some(symbol) = self.db.get_symbol_by_id(entry.symbol_id)? else {
                    continue;
                };
                if is_generic_target(&symbol.name) {
                    continue;
                }
                let structural =
                    compute_structural(&entry.relationship, entry.confidence, entry.depth);
                structural_scores.insert(entry.symbol_id, structural);
                let evolutionary = self.evolutionary_score(&target.file, &symbol.file)?;
                let fused = fuse_signals(structural, 0.0, evolutionary, &self.config);
                if fused.combined >= self.config.coupling_threshold {
                    coupled.push(CoupledSymbol {
                        symbol,
                        score: fused.combined,
                        reason: fused.breakdown(),
                    });
                }
                if coupled.len() >= MAX_STRUCTURAL_RESULTS {
                    break;
                }
            }
        }

        if let Some(target_id) = target.id {
            let text = build_symbol_text(&target.name, &target.kind, &target.context);
            let embedding = self.embedder.embed_single(&text)?;
            for (symbol_id, distance) in self.db.search_vectors(&embedding, 20)? {
                let semantic = compute_semantic(distance);
                if semantic <= self.config.coupling_threshold {
                    continue;
                }
                if seen.contains(&symbol_id) {
                    if let Some(entry) = coupled
                        .iter_mut()
                        .find(|entry| entry.symbol.id == Some(symbol_id))
                    {
                        let structural = structural_scores.get(&symbol_id).copied().unwrap_or(0.0);
                        let evolutionary =
                            self.evolutionary_score(&target.file, &entry.symbol.file)?;
                        let fused = fuse_signals(structural, semantic, evolutionary, &self.config);
                        entry.score = fused.combined;
                        entry.reason = fused.breakdown();
                    }
                    continue;
                }
                if symbol_id == target_id {
                    continue;
                }
                let Some(symbol) = self.db.get_symbol_by_id(symbol_id)? else {
                    continue;
                };
                if is_generic_target(&symbol.name) {
                    continue;
                }
                seen.insert(symbol_id);
                let evolutionary = self.evolutionary_score(&target.file, &symbol.file)?;
                let fused = fuse_signals(0.0, semantic, evolutionary, &self.config);
                coupled.push(CoupledSymbol {
                    symbol,
                    score: fused.combined,
                    reason: fused.breakdown(),
                });
            }
        }

        coupled.sort_by(|left, right| right.score.total_cmp(&left.score));
        Ok(coupled)
    }

    fn evolutionary_score(&self, file_a: &str, file_b: &str) -> Result<f64> {
        Ok(compute_evolutionary(
            self.db.get_cochange_frequency(file_a, file_b)?,
            10,
        ))
    }
}

fn rrf_score(rank: usize) -> f64 {
    1.0 / (RRF_K + rank as f64)
}

fn kind_boost(kind: &str) -> f64 {
    match kind {
        "function" | "class" => 1.5,
        "method" => 1.3,
        "variable" => 0.5,
        _ => 1.0,
    }
}

fn normalize_scores(results: Vec<(i64, f64)>) -> Vec<(i64, f64)> {
    let Some(max_score) = results
        .iter()
        .map(|(_, score)| *score)
        .max_by(f64::total_cmp)
    else {
        return results;
    };
    if max_score <= 0.0 {
        return results;
    }
    let theoretical_max = (1.0 / RRF_K) * 1.5 + (1.0 / RRF_K);
    let divisor = max_score.max(theoretical_max);
    results
        .into_iter()
        .map(|(symbol_id, score)| (symbol_id, (score / divisor).min(1.0)))
        .collect()
}

fn is_generic_target(name: &str) -> bool {
    let short = name.rsplit('.').next().unwrap_or(name);
    matches!(
        short,
        "map"
            | "filter"
            | "reduce"
            | "forEach"
            | "find"
            | "some"
            | "every"
            | "then"
            | "catch"
            | "finally"
            | "call"
            | "apply"
            | "bind"
            | "log"
            | "warn"
            | "error"
            | "info"
            | "debug"
            | "require"
    )
}
