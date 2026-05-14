use crate::{
    embedder::{build_symbol_text, Embedder},
    graph::{SymbolGraph, TraversalEntry},
    models::{
        behavior_fact_handle, decode_file_handle_path, file_handle, response_budget,
        response_envelope, symbol_handle, BehaviorFact, BehaviorFactHit, Callsite, Continuation,
        CoupledHit, CoupledSymbol, EvidenceCoverageItem, EvidenceFactHit, EvidencePackResponse,
        FileAnchor, FileRoleCard, GraphProvenance, ImpactResponse, InspectPage, InspectResponse,
        InspectSnippet, LexicalEvidence, NeighborhoodResponse, NextToolSuggestion, QueryIntent,
        RelatedResponse, SearchResponse, SignalScores, Symbol, SymbolHit, SymbolListResponse,
        SymbolQuery, EVIDENCE_PACK_CONTRACT, IMPACT_CONTRACT, INSPECT_CONTRACT,
        NEIGHBORHOOD_CONTRACT, RELATED_CONTRACT, SEARCH_CONTRACT, SYMBOLS_CONTRACT,
    },
    search::scoring::{compute_evolutionary, compute_semantic, compute_structural, fuse_signals},
    store::LoomDb,
    LoomConfig, Result,
};
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const RRF_K: f64 = 60.0;
const MAX_STRUCTURAL_RESULTS: usize = 30;
const MAX_SUMMARY_CHARS: usize = 160;
const MAX_SEARCH_RESULTS: usize = 100;
const MAX_SYMBOL_RESULTS: usize = 256;
const MAX_EXPANSION_RESULTS: usize = 12;
const MAX_INSPECT_LINES: usize = 32;
const MAX_INSPECT_CHARS: usize = 2_500;
const MAX_EVIDENCE_BUDGET_TOKENS: usize = 3_000;
const MAX_EVIDENCE_RESULTS: usize = 4;
const MAX_EVIDENCE_CARD_ITEMS: usize = 5;
const MAX_FILE_LINE_SCAN_FILES: usize = 512;
const MAX_FILE_LINE_SCAN_BYTES: u64 = 2_000_000;

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

#[derive(Debug, Clone)]
struct Candidate {
    symbol: Symbol,
    score: f64,
    signal_scores: SignalScores,
    lexical_evidence: Option<LexicalEvidence>,
    reason_codes: BTreeSet<String>,
    coupled: Vec<CoupledSymbol>,
}

impl Candidate {
    fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            score: 0.0,
            signal_scores: SignalScores::default(),
            lexical_evidence: None,
            reason_codes: BTreeSet::new(),
            coupled: Vec::new(),
        }
    }
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

    pub fn search(&self, query: &str, limit: usize, kind: Option<&str>) -> Result<SearchResponse> {
        let index_revision = self.db.index_revision()?;
        if limit == 0 {
            let envelope = response_envelope(SEARCH_CONTRACT, index_revision, limit, false, true);
            return Ok(SearchResponse {
                contract: envelope.contract,
                version: envelope.version,
                index_revision: envelope.index_revision,
                limit: envelope.limit,
                truncated: envelope.truncated,
                inspect_required: envelope.inspect_required,
                budget: response_budget("results", limit, 0, 0, false),
                continuation: None,
                next_tool_suggestions: vec![tool_suggestion(
                    "search",
                    "retry with a narrower code identifier, symbol name, or domain phrase",
                    [("query", query)],
                )],
                query_intent: classify_query_intent(query),
                exact_hits: Vec::new(),
                beyond_grep: Vec::new(),
            });
        }

        let requested_limit = limit;
        let limit = limit.min(MAX_SEARCH_RESULTS);
        let candidate_limit = if kind.is_some() {
            limit.saturating_mul(10)
        } else {
            limit.saturating_mul(3)
        };
        let fts_results = self.db.search_fts_with_evidence(query, candidate_limit)?;
        let fact_results = self.db.search_behavior_facts(query, candidate_limit)?;
        let symbol_results = self.db.list_symbols(query, None, None, candidate_limit)?;
        let token_symbol_results =
            self.db
                .list_symbols_by_ordered_tokens(query, None, None, candidate_limit)?;
        let lexical_result_count = fts_results.len()
            + fact_results.len()
            + symbol_results.len()
            + token_symbol_results.len();
        let file_line_results =
            if should_run_file_line_scan(query, lexical_result_count, candidate_limit) {
                self.search_file_lines(query, candidate_limit)?
            } else {
                Vec::new()
            };
        let embedding = self.embedder.embed_single(query)?;
        let vec_results = self.db.search_vectors(&embedding, candidate_limit)?;

        let mut candidates = BTreeMap::<i64, Candidate>::new();
        let mut lexical_seed_ids = Vec::new();
        for (rank, symbol) in symbol_results.into_iter().enumerate() {
            let Some(symbol_id) = symbol.id else {
                continue;
            };
            lexical_seed_ids.push(symbol_id);
            let evidence = symbol_query_evidence(&symbol, query, rank);
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(symbol.clone()));
            let signal = (1.0 + rrf_score(rank)) * kind_boost(&candidate.symbol.kind);
            candidate.score += signal;
            candidate.signal_scores.symbol += signal;
            candidate.lexical_evidence = Some(evidence);
            candidate
                .reason_codes
                .insert(symbol_reason_code(&candidate.symbol, query));
            candidate.reason_codes.insert("exact:symbol".to_string());
        }
        for (rank, symbol) in token_symbol_results.into_iter().enumerate() {
            let Some(symbol_id) = symbol.id else {
                continue;
            };
            lexical_seed_ids.push(symbol_id);
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(symbol.clone()));
            let signal = (0.8 + rrf_score(rank)) * kind_boost(&candidate.symbol.kind);
            candidate.score += signal;
            candidate.signal_scores.symbol += signal;
            if candidate.lexical_evidence.is_none() {
                candidate.lexical_evidence =
                    Some(symbol_query_token_evidence(&candidate.symbol, query, rank));
            }
            candidate
                .reason_codes
                .insert("symbol:ordered_tokens".to_string());
            candidate.reason_codes.insert("exact:symbol".to_string());
        }
        for (rank, (symbol, evidence)) in file_line_results.into_iter().enumerate() {
            let Some(symbol_id) = symbol.id else {
                continue;
            };
            lexical_seed_ids.push(symbol_id);
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(symbol.clone()));
            let signal = (0.7 + rrf_score(rank)) * kind_boost(&candidate.symbol.kind);
            candidate.score += signal;
            candidate.signal_scores.lexical += signal;
            candidate.lexical_evidence = Some(evidence.clone());
            candidate.reason_codes.insert("exact:file_line".to_string());
            candidate
                .reason_codes
                .insert(format!("lexical:{}", evidence.match_kind));
        }
        for (rank, result) in fts_results.into_iter().enumerate() {
            let Some(symbol_id) = result.symbol.id else {
                continue;
            };
            lexical_seed_ids.push(symbol_id);
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(result.symbol.clone()));
            let signal = rrf_score(rank) * kind_boost(&candidate.symbol.kind);
            candidate.score += signal;
            candidate.signal_scores.lexical += signal;
            candidate.lexical_evidence = Some(result.evidence.clone());
            candidate
                .reason_codes
                .insert(format!("exact:{}", result.evidence.field));
            candidate
                .reason_codes
                .insert(format!("lexical:{}", result.evidence.match_kind));
        }

        for (rank, result) in fact_results.into_iter().enumerate() {
            let Some(symbol_id) = result.fact.enclosing_symbol_id else {
                continue;
            };
            let Some(symbol) = self.db.get_symbol_by_id(symbol_id)? else {
                continue;
            };
            lexical_seed_ids.push(symbol_id);
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(symbol.clone()));
            let signal = rrf_score(rank) * kind_boost(&candidate.symbol.kind);
            candidate.score += signal;
            candidate.signal_scores.behavior += signal;
            if candidate.lexical_evidence.is_none() {
                candidate.lexical_evidence = Some(result.lexical_evidence.clone());
            }
            candidate
                .reason_codes
                .insert(format!("fact:{}", result.fact.fact_type));
            candidate
                .reason_codes
                .insert(format!("exact:fact:{}", result.lexical_evidence.field));
        }

        for (rank, (symbol_id, _distance)) in vec_results.into_iter().enumerate() {
            let Some(symbol) = self.db.get_symbol_by_id(symbol_id)? else {
                continue;
            };
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(symbol.clone()));
            let signal = rrf_score(rank) * kind_boost(&candidate.symbol.kind);
            candidate.score += signal;
            candidate.signal_scores.semantic += signal;
            candidate.reason_codes.insert("semantic".to_string());
        }

        self.add_graph_candidates(&mut candidates, &lexical_seed_ids)?;
        let normalized = normalize_scores(
            candidates
                .iter()
                .map(|(symbol_id, candidate)| (*symbol_id, candidate.score))
                .collect(),
        );
        for (symbol_id, score) in normalized {
            if let Some(candidate) = candidates.get_mut(&symbol_id) {
                candidate.score = score;
                candidate.signal_scores.total = score;
            }
        }

        let mut hits = Vec::new();
        for mut candidate in candidates.into_values() {
            if kind.is_some_and(|expected| !kind_matches(&candidate.symbol.kind, expected)) {
                continue;
            }
            if self.config.top_coupled > 0 {
                let mut coupled = self.find_coupled(&candidate.symbol)?;
                coupled.truncate(self.config.top_coupled);
                candidate.coupled = coupled;
            }
            hits.push(candidate_to_hit(candidate, &index_revision));
        }
        sort_hits(&mut hits);

        let mut exact_hits = Vec::new();
        let mut beyond_grep = Vec::new();
        for hit in hits {
            if hit.lexical_evidence.is_some() {
                exact_hits.push(hit);
            } else {
                beyond_grep.push(hit);
            }
        }
        assign_symbol_ranks(&mut exact_hits);
        assign_symbol_ranks(&mut beyond_grep);
        let total_before_truncate = exact_hits.len() + beyond_grep.len();
        let exact_limit = exact_hits.len().min(limit);
        exact_hits.truncate(limit);
        beyond_grep.truncate(limit.saturating_sub(exact_limit));
        let returned = exact_hits.len() + beyond_grep.len();
        let truncated = total_before_truncate > returned || requested_limit > limit;
        let omitted = total_before_truncate.saturating_sub(returned);
        let continuation = continuation_for("search", truncated, returned, omitted);
        let next_tool_suggestions = search_next_tool_suggestions(&exact_hits, &beyond_grep);

        let envelope = response_envelope(SEARCH_CONTRACT, index_revision, limit, truncated, true);
        Ok(SearchResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget("results", limit, returned, omitted, truncated),
            continuation,
            next_tool_suggestions,
            query_intent: classify_query_intent(query),
            exact_hits,
            beyond_grep,
        })
    }

    pub fn symbols(
        &self,
        query: &str,
        file_prefix: Option<&str>,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<SymbolListResponse> {
        let index_revision = self.db.index_revision()?;
        let effective_limit = limit.min(MAX_SYMBOL_RESULTS);
        let fetch_limit = effective_limit.saturating_add(1);
        let mut relaxed_kind = false;
        let mut symbols = self
            .db
            .list_symbols(query, file_prefix, kind, fetch_limit)?;
        if symbols.is_empty() && function_method_kind(kind).is_some() {
            symbols =
                self.db
                    .list_symbols(query, file_prefix, None, fetch_limit.saturating_mul(4))?;
            if let Some(expected) = kind {
                symbols.retain(|symbol| kind_matches(&symbol.kind, expected));
            }
            relaxed_kind = !symbols.is_empty();
        }
        let truncated = symbols.len() > effective_limit || limit > effective_limit;
        symbols.truncate(effective_limit);
        let total_returned = symbols.len();
        let mut results = symbols
            .into_iter()
            .enumerate()
            .map(|(index, symbol)| {
                symbol_list_hit(
                    symbol,
                    &index_revision,
                    index + 1,
                    query,
                    file_prefix,
                    kind,
                    relaxed_kind,
                )
            })
            .collect::<Vec<_>>();
        sort_hits(&mut results);
        assign_symbol_ranks(&mut results);
        let envelope = response_envelope(
            SYMBOLS_CONTRACT,
            index_revision,
            effective_limit,
            truncated,
            true,
        );
        Ok(SymbolListResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget(
                "results",
                effective_limit,
                total_returned,
                usize::from(truncated),
                truncated,
            ),
            query: query.to_string(),
            file_prefix: file_prefix.map(ToString::to_string),
            kind: kind.map(ToString::to_string),
            continuation: continuation_for(
                "symbols",
                truncated,
                results.len(),
                usize::from(truncated),
            ),
            next_tool_suggestions: symbol_next_tool_suggestions(&results),
            results,
        })
    }

    pub fn related(
        &self,
        symbol: &str,
        file: Option<&str>,
        kind: Option<&str>,
    ) -> Result<RelatedResponse> {
        let index_revision = self.db.index_revision()?;
        let Some(target) =
            select_target_symbol(self.db.get_symbol_by_name_fuzzy(symbol, file)?, kind)
        else {
            let envelope = response_envelope(RELATED_CONTRACT, index_revision, 0, false, true);
            return Ok(RelatedResponse {
                contract: envelope.contract,
                version: envelope.version,
                index_revision: envelope.index_revision,
                limit: envelope.limit,
                truncated: envelope.truncated,
                inspect_required: envelope.inspect_required,
                budget: response_budget("results", 0, 0, 0, false),
                query: symbol_query(symbol, file, kind),
                continuation: None,
                next_tool_suggestions: vec![tool_suggestion(
                    "symbols",
                    "enumerate similarly named symbols or method suffixes before falling back to text search",
                    [("query", symbol)],
                )],
                results: Vec::new(),
            });
        };
        let mut coupled = self.find_coupled(&target)?;
        let total_before_truncate = coupled.len();
        coupled.truncate(MAX_EXPANSION_RESULTS);
        let results = coupled_to_hits(coupled, &index_revision);
        let truncated = total_before_truncate > results.len();
        let envelope = response_envelope(
            RELATED_CONTRACT,
            index_revision,
            MAX_EXPANSION_RESULTS,
            truncated,
            true,
        );
        Ok(RelatedResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget(
                "results",
                MAX_EXPANSION_RESULTS,
                results.len(),
                total_before_truncate.saturating_sub(results.len()),
                truncated,
            ),
            query: symbol_query(symbol, file, kind),
            continuation: continuation_for(
                "related",
                truncated,
                results.len(),
                total_before_truncate.saturating_sub(results.len()),
            ),
            next_tool_suggestions: coupled_next_tool_suggestions(&results),
            results,
        })
    }

    pub fn impact(
        &self,
        symbol: &str,
        file: Option<&str>,
        kind: Option<&str>,
    ) -> Result<ImpactResponse> {
        let index_revision = self.db.index_revision()?;
        let Some(target) =
            select_target_symbol(self.db.get_symbol_by_name_fuzzy(symbol, file)?, kind)
        else {
            let envelope = response_envelope(IMPACT_CONTRACT, index_revision, 0, false, true);
            return Ok(ImpactResponse {
                contract: envelope.contract,
                version: envelope.version,
                index_revision: envelope.index_revision,
                limit: envelope.limit,
                truncated: envelope.truncated,
                inspect_required: envelope.inspect_required,
                budget: response_budget("results", 0, 0, 0, false),
                query: symbol_query(symbol, file, kind),
                continuation: None,
                next_tool_suggestions: vec![tool_suggestion(
                    "symbols",
                    "resolve the target symbol before running impact",
                    [("query", symbol)],
                )],
                results: Vec::new(),
            });
        };
        let mut impact = Vec::new();
        let mut seen = BTreeSet::from_iter(target.id);
        if let (Some(graph), Some(target_id)) = (&self.graph, target.id) {
            for entry in graph.dependents(target_id, 3) {
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
                let evolutionary = self.evolutionary_score(&target.file, &symbol.file)?;
                let fused = fuse_signals(structural, 0.0, evolutionary, &self.config);
                impact.push(CoupledSymbol {
                    symbol,
                    score: fused.combined,
                    reason: fused.breakdown(),
                    provenance: vec![traversal_provenance(&entry, "graph.dependents")],
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
                provenance: vec![GraphProvenance {
                    relationship: edge.relationship,
                    direction: "incoming".to_string(),
                    depth: 1,
                    confidence: edge.confidence,
                    source: "resolved_edge".to_string(),
                }],
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
                provenance: vec![GraphProvenance {
                    relationship: edge.relationship,
                    direction: "incoming".to_string(),
                    depth: 1,
                    confidence: edge.confidence,
                    source: "unresolved_name_edge".to_string(),
                }],
            });
        }

        impact.sort_by(|left, right| right.score.total_cmp(&left.score));
        let total_before_truncate = impact.len();
        impact.truncate(MAX_EXPANSION_RESULTS);
        let results = coupled_to_hits(impact, &index_revision);
        let truncated = total_before_truncate > results.len();
        let envelope = response_envelope(
            IMPACT_CONTRACT,
            index_revision,
            MAX_EXPANSION_RESULTS,
            truncated,
            true,
        );
        Ok(ImpactResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget(
                "results",
                MAX_EXPANSION_RESULTS,
                results.len(),
                total_before_truncate.saturating_sub(results.len()),
                truncated,
            ),
            query: symbol_query(symbol, file, kind),
            continuation: continuation_for(
                "impact",
                truncated,
                results.len(),
                total_before_truncate.saturating_sub(results.len()),
            ),
            next_tool_suggestions: coupled_next_tool_suggestions(&results),
            results,
        })
    }

    pub fn neighborhood(&self, file: &str, line: i64) -> Result<NeighborhoodResponse> {
        let index_revision = self.db.index_revision()?;
        let colocated = self.db.get_colocated_symbols(file)?;
        let anchor = most_specific_symbol_for_line(&colocated, line)
            .cloned()
            .or_else(|| {
                colocated
                    .iter()
                    .min_by_key(|symbol| (symbol.line - line).abs())
                    .cloned()
            });
        let Some(anchor) = anchor else {
            let envelope = response_envelope(NEIGHBORHOOD_CONTRACT, index_revision, 0, false, true);
            return Ok(NeighborhoodResponse {
                contract: envelope.contract,
                version: envelope.version,
                index_revision: envelope.index_revision,
                limit: envelope.limit,
                truncated: envelope.truncated,
                inspect_required: envelope.inspect_required,
                budget: response_budget("results", 0, 0, 0, false),
                file: file.to_string(),
                line,
                anchor: None,
                continuation: None,
                next_tool_suggestions: vec![tool_suggestion(
                    "symbols",
                    "find an anchor symbol first, then retry neighborhood with its file and line",
                    [("query", file)],
                )],
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
                provenance: vec![GraphProvenance {
                    relationship: "co_located".to_string(),
                    direction: "same_file".to_string(),
                    depth: 0,
                    confidence: 0.4,
                    source: "file_layout".to_string(),
                }],
            });
        }
        coupled.sort_by(|left, right| right.score.total_cmp(&left.score));
        let total_before_truncate = coupled.len();
        coupled.truncate(MAX_EXPANSION_RESULTS);
        let truncated = total_before_truncate > coupled.len();
        let coupled_hits = coupled_to_hits(coupled.clone(), &index_revision);
        let anchor_hit = SymbolHit {
            handle: symbol_handle(&index_revision, &anchor),
            file_handle: file_handle(&index_revision, &anchor.file),
            rank: 1,
            name: anchor.name.clone(),
            kind: anchor.kind.clone(),
            language: anchor.language.clone(),
            anchor: FileAnchor::from_symbol(&anchor),
            summary: symbol_summary(&anchor),
            symbol: anchor,
            score: 1.0,
            signal_scores: SignalScores {
                graph: 1.0,
                total: 1.0,
                ..SignalScores::default()
            },
            reason_codes: vec!["anchor".to_string()],
            lexical_evidence: None,
            coupled: coupled_to_hits(coupled, &index_revision),
        };
        let envelope = response_envelope(
            NEIGHBORHOOD_CONTRACT,
            index_revision,
            MAX_EXPANSION_RESULTS,
            truncated,
            true,
        );
        Ok(NeighborhoodResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget(
                "results",
                MAX_EXPANSION_RESULTS,
                coupled_hits.len(),
                total_before_truncate.saturating_sub(coupled_hits.len()),
                truncated,
            ),
            file: file.to_string(),
            line,
            anchor: Some(anchor_hit),
            continuation: continuation_for(
                "neighborhood",
                truncated,
                coupled_hits.len(),
                total_before_truncate.saturating_sub(coupled_hits.len()),
            ),
            next_tool_suggestions: coupled_next_tool_suggestions(&coupled_hits),
            coupled: coupled_hits,
        })
    }

    pub fn inspect(
        &self,
        handle: &str,
        line_budget: usize,
        char_budget: usize,
        line_offset: usize,
    ) -> Result<InspectResponse> {
        let index_revision = self.db.index_revision()?;
        let line_budget = line_budget.clamp(1, MAX_INSPECT_LINES);
        let char_budget = char_budget.clamp(1, MAX_INSPECT_CHARS);
        let parsed = parse_handle(handle)?;
        let stale_revision = parsed.index_revision != index_revision;
        let stale_source_revision = parsed.index_revision.clone();

        let response = match parsed.target {
            HandleTarget::Symbol(symbol_id) => {
                let Some(symbol) = self.db.get_symbol_by_id(symbol_id)? else {
                    return Ok(stale_inspect_response(
                        handle,
                        "symbol",
                        &index_revision,
                        line_budget,
                        "symbol handle no longer resolves; rerun search".to_string(),
                    ));
                };
                self.inspect_symbol(handle, symbol, line_budget, char_budget, line_offset)
            }
            HandleTarget::BehaviorFact(fact_id) => {
                let Some(fact) = self.db.get_behavior_fact_by_id(fact_id)? else {
                    return Ok(stale_inspect_response(
                        handle,
                        "fact",
                        &index_revision,
                        line_budget,
                        "behavior fact handle no longer resolves; rerun search or evidence_pack"
                            .to_string(),
                    ));
                };
                self.inspect_behavior_fact(handle, fact, line_budget, char_budget, line_offset)
            }
            HandleTarget::Callsite(callsite_id) => {
                let Some(callsite) = self.db.get_callsite_by_id(callsite_id)? else {
                    return Ok(stale_inspect_response(
                        handle,
                        "callsite",
                        &index_revision,
                        line_budget,
                        "callsite handle no longer resolves; rerun search or related".to_string(),
                    ));
                };
                self.inspect_callsite(handle, callsite, line_budget, char_budget, line_offset)
            }
            HandleTarget::File(file) => self.inspect_file(
                handle,
                &file,
                &index_revision,
                line_budget,
                char_budget,
                line_offset,
            ),
        }?;
        Ok(mark_stale_recovered(
            response,
            stale_revision,
            &stale_source_revision,
        ))
    }

    pub fn evidence_pack(&self, query: &str, budget_tokens: usize) -> Result<EvidencePackResponse> {
        if budget_tokens == 0 {
            return Err(crate::LoomError::InvalidInput(
                "budget_tokens must be greater than zero".to_string(),
            ));
        }
        let index_revision = self.db.index_revision()?;
        let effective_budget_tokens = budget_tokens.min(MAX_EVIDENCE_BUDGET_TOKENS);
        let result_limit = (effective_budget_tokens / 180).clamp(2, MAX_EVIDENCE_RESULTS);
        let mut search = self.search(query, result_limit, None)?;
        let raw_behavior_facts = self.db.search_behavior_facts(query, result_limit)?;
        let role_cards = self.role_cards_for_evidence(&search, &raw_behavior_facts)?;
        let char_budget = effective_budget_tokens.saturating_mul(3).clamp(240, 4_000);
        let mut selected = Vec::new();
        selected.extend(
            search
                .exact_hits
                .iter()
                .take(1)
                .map(|hit| hit.handle.clone()),
        );
        selected.extend(
            search
                .beyond_grep
                .iter()
                .take(1)
                .map(|hit| hit.handle.clone()),
        );
        selected.extend(
            raw_behavior_facts
                .iter()
                .take(1)
                .map(|hit| behavior_fact_handle(&index_revision, &hit.fact)),
        );
        selected.sort();
        selected.dedup();

        let per_snippet_budget = if selected.is_empty() {
            char_budget
        } else {
            (char_budget / selected.len()).clamp(160, 1_000)
        };
        let mut inspected_snippets = Vec::new();
        let mut omitted = Vec::new();
        let mut returned_chars = 0usize;
        let mut truncated = search.truncated;
        for handle in selected {
            let inspected = self.inspect(&handle, 12, per_snippet_budget, 0)?;
            if inspected.truncated {
                truncated = true;
            }
            if let Some(snippet) = inspected.snippet {
                returned_chars += snippet.chars;
                inspected_snippets.push(snippet);
            } else if let Some(error) = inspected.error {
                omitted.push(format!("{handle}: {error}"));
            }
        }

        if search.truncated {
            omitted.push("search results were truncated before evidence packing".to_string());
        }
        if budget_tokens > effective_budget_tokens {
            omitted.push(format!(
                "evidence budget capped at {effective_budget_tokens} tokens to keep MCP payload bounded"
            ));
        }
        if inspected_snippets.is_empty() {
            omitted.push("no source snippets were inspected for this query".to_string());
        }

        let missing_concepts = missing_concepts(query, &search);
        let coverage_checklist = evidence_coverage(
            &search,
            &inspected_snippets,
            &raw_behavior_facts,
            &role_cards,
        );
        let behavior_facts = raw_behavior_facts
            .into_iter()
            .map(|hit| evidence_fact_hit(hit, &index_revision))
            .collect::<Vec<_>>();
        let display_text = format!(
            "Evidence pack for `{query}`: {} exact, {} beyond-grep, {} facts, {} role cards, {} snippets.",
            search.exact_hits.len(),
            search.beyond_grep.len(),
            behavior_facts.len(),
            role_cards.len(),
            inspected_snippets.len()
        );
        let returned_units = (returned_chars / 4)
            .saturating_add(behavior_facts.len().saturating_mul(12))
            .saturating_add(role_cards.len().saturating_mul(24))
            .max(inspected_snippets.len());
        let omitted_count = omitted.len() + missing_concepts.len();
        let envelope = response_envelope(
            EVIDENCE_PACK_CONTRACT,
            index_revision,
            effective_budget_tokens,
            truncated,
            false,
        );

        search.exact_hits.truncate(result_limit);
        search.beyond_grep.truncate(result_limit);
        let next_tool_suggestions = evidence_next_tool_suggestions(&search);
        Ok(EvidencePackResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget(
                "tokens",
                effective_budget_tokens,
                returned_units,
                omitted_count,
                truncated,
            ),
            query: query.to_string(),
            exact_hits: search.exact_hits,
            beyond_grep: search.beyond_grep,
            behavior_facts,
            role_cards,
            inspected_snippets,
            coverage_checklist,
            omitted,
            missing_concepts,
            next_tool_suggestions,
            display_text,
        })
    }

    fn role_cards_for_evidence(
        &self,
        search: &SearchResponse,
        behavior_facts: &[BehaviorFactHit],
    ) -> Result<Vec<FileRoleCard>> {
        let mut files = BTreeSet::new();
        files.extend(
            search
                .exact_hits
                .iter()
                .take(3)
                .map(|hit| hit.symbol.file.clone()),
        );
        files.extend(
            search
                .beyond_grep
                .iter()
                .take(3)
                .map(|hit| hit.symbol.file.clone()),
        );
        files.extend(
            behavior_facts
                .iter()
                .take(3)
                .map(|hit| hit.fact.file.clone()),
        );
        let files = files.into_iter().collect::<Vec<_>>();
        let mut cards = self.db.get_role_cards_for_files(&files)?;
        cards.truncate(2);
        for card in &mut cards {
            compact_role_card(card);
        }
        Ok(cards)
    }

    fn inspect_symbol(
        &self,
        handle: &str,
        symbol: Symbol,
        line_budget: usize,
        char_budget: usize,
        line_offset: usize,
    ) -> Result<InspectResponse> {
        let index_revision = self.db.index_revision()?;
        let anchor = FileAnchor::from_symbol(&symbol);
        let path = self.contained_path(&symbol.file)?;
        let offset = i64::try_from(line_offset).unwrap_or(i64::MAX);
        let start_line = symbol.line.saturating_add(offset).max(1);
        let anchor_end = symbol.end_line.max(symbol.line).max(start_line);
        let requested_end = start_line
            .saturating_add(line_budget.saturating_sub(1) as i64)
            .min(anchor_end);
        let snippet = read_snippet(&path, &anchor, start_line, requested_end, char_budget)?;
        let truncated = snippet.truncated || requested_end < anchor_end;
        let next_line_offset = truncated.then_some(
            line_offset
                + snippet
                    .snippet
                    .as_ref()
                    .map(|snippet| (snippet.end_line - start_line + 1).max(1) as usize)
                    .unwrap_or(0),
        );
        Ok(inspect_response(InspectResponseParts {
            handle,
            handle_kind: "symbol",
            index_revision,
            limit: line_budget,
            truncated,
            stale: false,
            error: None,
            anchor: Some(anchor),
            snippet: snippet.snippet,
            page: InspectPage {
                line_offset,
                next_line_offset,
                refused: false,
                refusal_reason: None,
            },
        }))
    }

    fn inspect_behavior_fact(
        &self,
        handle: &str,
        fact: BehaviorFact,
        line_budget: usize,
        char_budget: usize,
        line_offset: usize,
    ) -> Result<InspectResponse> {
        let index_revision = self.db.index_revision()?;
        let anchor = FileAnchor {
            file: fact.file.clone(),
            line: fact.line,
            end_line: fact.end_line,
        };
        self.inspect_line_span(LineSpanInspection {
            handle,
            handle_kind: "fact",
            index_revision: &index_revision,
            file: &fact.file,
            anchor,
            line_budget,
            char_budget,
            line_offset,
        })
    }

    fn inspect_callsite(
        &self,
        handle: &str,
        callsite: Callsite,
        line_budget: usize,
        char_budget: usize,
        line_offset: usize,
    ) -> Result<InspectResponse> {
        let index_revision = self.db.index_revision()?;
        let anchor = FileAnchor {
            file: callsite.file.clone(),
            line: callsite.line,
            end_line: callsite.end_line,
        };
        self.inspect_line_span(LineSpanInspection {
            handle,
            handle_kind: "callsite",
            index_revision: &index_revision,
            file: &callsite.file,
            anchor,
            line_budget,
            char_budget,
            line_offset,
        })
    }

    fn inspect_line_span(&self, parts: LineSpanInspection<'_>) -> Result<InspectResponse> {
        let path = self.contained_path(parts.file)?;
        let offset = i64::try_from(parts.line_offset).unwrap_or(i64::MAX);
        let start_line = parts.anchor.line.saturating_add(offset).max(1);
        let anchor_end = parts.anchor.end_line.max(parts.anchor.line).max(start_line);
        let requested_end = start_line
            .saturating_add(parts.line_budget.saturating_sub(1) as i64)
            .min(anchor_end);
        let snippet = read_snippet(
            &path,
            &parts.anchor,
            start_line,
            requested_end,
            parts.char_budget,
        )?;
        let truncated = snippet.truncated || requested_end < anchor_end;
        let returned_lines = snippet
            .snippet
            .as_ref()
            .map(|snippet| (snippet.end_line - start_line + 1).max(1) as usize)
            .unwrap_or(0);
        Ok(inspect_response(InspectResponseParts {
            handle: parts.handle,
            handle_kind: parts.handle_kind,
            index_revision: parts.index_revision.to_string(),
            limit: parts.line_budget,
            truncated,
            stale: false,
            error: None,
            anchor: Some(parts.anchor),
            snippet: snippet.snippet,
            page: InspectPage {
                line_offset: parts.line_offset,
                next_line_offset: truncated.then_some(parts.line_offset + returned_lines),
                refused: false,
                refusal_reason: None,
            },
        }))
    }

    fn inspect_file(
        &self,
        handle: &str,
        file: &str,
        index_revision: &str,
        line_budget: usize,
        char_budget: usize,
        line_offset: usize,
    ) -> Result<InspectResponse> {
        if self.db.get_file_hash(file)?.is_none() {
            return Ok(stale_inspect_response(
                handle,
                "file",
                index_revision,
                line_budget,
                "file handle no longer resolves; rerun search".to_string(),
            ));
        }
        let path = self.contained_path(file)?;
        let offset = i64::try_from(line_offset).unwrap_or(i64::MAX);
        let start_line = 1_i64.saturating_add(offset);
        let requested_end = start_line.saturating_add(line_budget.saturating_sub(1) as i64);
        let anchor = FileAnchor {
            file: file.to_string(),
            line: start_line,
            end_line: requested_end,
        };
        let snippet = read_snippet(&path, &anchor, start_line, requested_end, char_budget)?;
        let truncated = snippet.truncated || snippet.has_more;
        let returned_lines = snippet
            .snippet
            .as_ref()
            .map(|snippet| (snippet.end_line - start_line + 1).max(1) as usize)
            .unwrap_or(0);
        Ok(inspect_response(InspectResponseParts {
            handle,
            handle_kind: "file",
            index_revision: index_revision.to_string(),
            limit: line_budget,
            truncated,
            stale: false,
            error: None,
            anchor: Some(anchor),
            snippet: snippet.snippet,
            page: InspectPage {
                line_offset,
                next_line_offset: truncated.then_some(line_offset + returned_lines),
                refused: false,
                refusal_reason: None,
            },
        }))
    }

    fn contained_path(&self, file: &str) -> Result<PathBuf> {
        let relative = Path::new(file);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(crate::LoomError::InvalidInput(
                "indexed file path must be repo-relative and stay inside target_dir".to_string(),
            ));
        }
        let root = self.config.target_dir.canonicalize().map_err(|source| {
            crate::LoomError::IndexerIo {
                path: self.config.target_dir.display().to_string(),
                source,
            }
        })?;
        let path = root.join(relative);
        let canonical = path
            .canonicalize()
            .map_err(|source| crate::LoomError::IndexerIo {
                path: path.display().to_string(),
                source,
            })?;
        if !canonical.starts_with(&root) {
            return Err(crate::LoomError::InvalidInput(
                "indexed file path escaped target_dir".to_string(),
            ));
        }
        Ok(canonical)
    }

    fn add_graph_candidates(
        &self,
        candidates: &mut BTreeMap<i64, Candidate>,
        seed_ids: &[i64],
    ) -> Result<()> {
        let Some(graph) = &self.graph else {
            return Ok(());
        };
        for seed_id in seed_ids {
            for entry in graph.neighbors_with_metadata(*seed_id, 1) {
                let Some(symbol) = self.db.get_symbol_by_id(entry.symbol_id)? else {
                    continue;
                };
                if is_generic_target(&symbol.name) {
                    continue;
                }
                let candidate = candidates
                    .entry(entry.symbol_id)
                    .or_insert_with(|| Candidate::new(symbol));
                let signal = compute_structural(&entry.relationship, entry.confidence, entry.depth);
                candidate.score += signal;
                candidate.signal_scores.graph += signal;
                candidate
                    .reason_codes
                    .insert(format!("graph:{}", entry.relationship));
            }
        }
        Ok(())
    }

    fn search_file_lines(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(Symbol, LexicalEvidence)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let query_lower = query.trim().to_lowercase();
        let terms = lexical_query_terms(query);
        if query_lower.chars().count() < 3 && terms.is_empty() {
            return Ok(Vec::new());
        }

        let mut matches = Vec::new();
        for file in self
            .db
            .list_indexed_files()?
            .into_iter()
            .take(MAX_FILE_LINE_SCAN_FILES)
        {
            let path = self.contained_path(&file)?;
            let Ok(file_handle) = File::open(&path) else {
                continue;
            };
            if file_handle
                .metadata()
                .is_ok_and(|metadata| metadata.len() > MAX_FILE_LINE_SCAN_BYTES)
            {
                continue;
            }
            let reader = BufReader::new(file_handle);
            let mut file_matches = Vec::new();
            for (line_index, line) in reader.lines().enumerate() {
                let Ok(line) = line else {
                    continue;
                };
                let Some(match_kind) = line_match_kind(&query_lower, &terms, &line) else {
                    continue;
                };
                file_matches.push((line_index as i64 + 1, line, match_kind));
                if file_matches.len() >= limit {
                    break;
                }
            }
            if file_matches.is_empty() {
                continue;
            }
            let colocated = self.db.get_colocated_symbols(&file)?;
            for (line, text, match_kind) in file_matches {
                let Some(symbol) = containing_symbol_for_line(&colocated, line) else {
                    continue;
                };
                matches.push((
                    symbol.clone(),
                    LexicalEvidence {
                        snippet: bounded_line_evidence(&text),
                        matched_text: query.to_string(),
                        rank: matches.len() as f64,
                        field: "file_line".to_string(),
                        reason: format!("exact file-line scan at {file}:{line}"),
                        match_kind: match_kind.to_string(),
                        sanitized_query: query.to_string(),
                    },
                ));
                if matches.len() >= limit {
                    return Ok(matches);
                }
            }
        }
        Ok(matches)
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
                        provenance: vec![traversal_provenance(&entry, "graph.neighbors")],
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
                    provenance: Vec::new(),
                });
            }
        }

        coupled.sort_by(|left, right| right.score.total_cmp(&left.score));
        Ok(coupled)
    }

    fn evolutionary_score(&self, file_a: &str, file_b: &str) -> Result<f64> {
        let Some(cochange) = self.db.get_cochange(file_a, file_b)? else {
            return Ok(0.0);
        };
        Ok(compute_evolutionary(
            cochange.frequency,
            cochange.recency,
            10,
        ))
    }
}

fn candidate_to_hit(candidate: Candidate, index_revision: &str) -> SymbolHit {
    let file = candidate.symbol.file.clone();
    SymbolHit {
        handle: symbol_handle(index_revision, &candidate.symbol),
        file_handle: file_handle(index_revision, &file),
        rank: 0,
        name: candidate.symbol.name.clone(),
        kind: candidate.symbol.kind.clone(),
        language: candidate.symbol.language.clone(),
        anchor: FileAnchor::from_symbol(&candidate.symbol),
        summary: symbol_summary(&candidate.symbol),
        symbol: candidate.symbol,
        score: candidate.score,
        signal_scores: candidate.signal_scores,
        reason_codes: candidate.reason_codes.into_iter().collect(),
        lexical_evidence: candidate.lexical_evidence,
        coupled: coupled_to_hits(candidate.coupled, index_revision),
    }
}

fn symbol_list_hit(
    symbol: Symbol,
    index_revision: &str,
    rank: usize,
    query: &str,
    file_prefix: Option<&str>,
    kind: Option<&str>,
    relaxed_kind: bool,
) -> SymbolHit {
    let mut reason_codes = Vec::new();
    let score = if symbol.name == query {
        reason_codes.push("symbol:exact".to_string());
        1.0
    } else if symbol
        .name
        .strip_suffix(query)
        .is_some_and(|prefix| prefix.ends_with('.'))
    {
        reason_codes.push("symbol:method_suffix".to_string());
        0.92
    } else {
        reason_codes.push("symbol:contains".to_string());
        0.72
    };
    if file_prefix.is_some_and(|prefix| symbol.file.starts_with(prefix)) {
        reason_codes.push("file_prefix".to_string());
    }
    if kind.is_some_and(|expected| symbol.kind == expected) {
        reason_codes.push("kind".to_string());
    } else if relaxed_kind && function_method_kind(kind).is_some() {
        reason_codes.push("kind:relaxed-function-method".to_string());
    }
    SymbolHit {
        handle: symbol_handle(index_revision, &symbol),
        file_handle: file_handle(index_revision, &symbol.file),
        rank,
        name: symbol.name.clone(),
        kind: symbol.kind.clone(),
        language: symbol.language.clone(),
        anchor: FileAnchor::from_symbol(&symbol),
        summary: symbol_summary(&symbol),
        symbol,
        score,
        signal_scores: SignalScores {
            symbol: score,
            total: score,
            ..SignalScores::default()
        },
        reason_codes,
        lexical_evidence: None,
        coupled: Vec::new(),
    }
}

fn symbol_query_evidence(symbol: &Symbol, query: &str, rank: usize) -> LexicalEvidence {
    LexicalEvidence {
        snippet: symbol.name.clone(),
        matched_text: query.to_string(),
        rank: rank as f64,
        field: "symbol".to_string(),
        reason: "symbol lookup matched indexed symbol name".to_string(),
        match_kind: symbol_match_kind(symbol, query).to_string(),
        sanitized_query: query.to_string(),
    }
}

fn symbol_query_token_evidence(symbol: &Symbol, query: &str, rank: usize) -> LexicalEvidence {
    LexicalEvidence {
        snippet: symbol.name.clone(),
        matched_text: query.to_string(),
        rank: rank as f64,
        field: "symbol".to_string(),
        reason: "ordered query tokens matched indexed symbol name or context".to_string(),
        match_kind: "ordered_tokens".to_string(),
        sanitized_query: query.to_string(),
    }
}

fn symbol_reason_code(symbol: &Symbol, query: &str) -> String {
    format!("symbol:{}", symbol_match_kind(symbol, query))
}

fn symbol_match_kind(symbol: &Symbol, query: &str) -> &'static str {
    if symbol.name == query {
        "exact"
    } else if symbol
        .name
        .strip_suffix(query)
        .is_some_and(|prefix| prefix.ends_with('.'))
    {
        "method_suffix"
    } else {
        "contains"
    }
}

fn lexical_query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for piece in query
        .split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
    {
        push_query_identifier_terms(piece, &mut terms);
    }
    terms.dedup();
    terms
}

fn push_query_identifier_terms(piece: &str, terms: &mut Vec<String>) {
    let chars = piece.chars().collect::<Vec<_>>();
    let mut pieces = Vec::new();
    let mut start = 0;
    for index in 1..chars.len() {
        let prev = chars[index - 1];
        let current = chars[index];
        let next = chars.get(index + 1).copied();
        let starts_word = (current.is_uppercase()
            && (prev.is_lowercase() || prev.is_ascii_digit()))
            || (prev.is_uppercase()
                && current.is_uppercase()
                && next.is_some_and(char::is_lowercase));
        if starts_word {
            push_query_identifier_piece(&chars[start..index], &mut pieces);
            start = index;
        }
    }
    push_query_identifier_piece(&chars[start..], &mut pieces);
    if pieces.len() > 1 {
        terms.extend(pieces);
    } else {
        let lower = piece.to_lowercase();
        if lower.chars().count() >= 2 {
            terms.push(lower);
        }
    }
}

fn push_query_identifier_piece(piece: &[char], terms: &mut Vec<String>) {
    let term = piece.iter().collect::<String>().to_lowercase();
    if term.chars().count() >= 2 && terms.last().is_none_or(|last| last != &term) {
        terms.push(term);
    }
}

fn should_run_file_line_scan(query: &str, lexical_count: usize, limit: usize) -> bool {
    if limit == 0 {
        return false;
    }
    let trimmed = query.trim();
    let terms = lexical_query_terms(trimmed);
    if trimmed.chars().count() < 3 || terms.is_empty() {
        return false;
    }
    let code_like = trimmed
        .chars()
        .any(|character| matches!(character, ':' | '_' | '.' | '/' | '\\' | '(' | ')' | '-'));
    let specific_phrase = terms.len() >= 3 && trimmed.chars().count() >= 12;
    lexical_count < limit || code_like || specific_phrase
}

fn classify_query_intent(query: &str) -> QueryIntent {
    let trimmed = query.trim();
    let terms = lexical_query_terms(trimmed);
    let mut reasons = Vec::new();
    let mut intent = "conceptual";
    let mut confidence: f64 = 0.55;

    if trimmed.contains('/') || trimmed.contains('\\') {
        intent = "path";
        confidence = 0.85;
        reasons.push("contains path separator".to_string());
    }
    if trimmed.contains("::") || trimmed.contains('.') || trimmed.contains('_') {
        intent = "symbol";
        confidence = confidence.max(0.82);
        reasons.push("contains code identifier punctuation".to_string());
    }
    if !trimmed.is_empty()
        && trimmed.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
        && trimmed
            .chars()
            .any(|character| character.is_ascii_uppercase())
    {
        intent = "operational_fact";
        confidence = 0.9;
        reasons.push("looks like constant/env/config name".to_string());
    }
    if !trimmed.contains(' ') && trimmed.chars().any(char::is_uppercase) {
        intent = "symbol";
        confidence = confidence.max(0.78);
        reasons.push("looks like camel or pascal identifier".to_string());
    }
    if terms.len() >= 4 && reasons.is_empty() {
        intent = "conceptual";
        confidence = 0.7;
        reasons.push("multi-term natural language query".to_string());
    }
    if reasons.is_empty() {
        reasons.push("default query interpretation".to_string());
    }

    QueryIntent {
        intent: intent.to_string(),
        confidence,
        reasons,
    }
}

fn line_match_kind(query_lower: &str, terms: &[String], line: &str) -> Option<&'static str> {
    let line_lower = line.to_lowercase();
    if !query_lower.is_empty() && line_lower.contains(query_lower) {
        return Some("exact_phrase");
    }
    if ordered_terms_match(&line_lower, terms) {
        return Some("ordered_tokens");
    }
    None
}

fn ordered_terms_match(line_lower: &str, terms: &[String]) -> bool {
    if terms.len() < 2 {
        return false;
    }
    let mut remaining = line_lower;
    for term in terms {
        let Some(index) = remaining.find(term) else {
            return false;
        };
        remaining = &remaining[index + term.len()..];
    }
    true
}

fn containing_symbol_for_line(symbols: &[Symbol], line: i64) -> Option<&Symbol> {
    most_specific_symbol_for_line(symbols, line)
}

fn most_specific_symbol_for_line(symbols: &[Symbol], line: i64) -> Option<&Symbol> {
    symbols
        .iter()
        .filter(|symbol| symbol.line <= line && line <= symbol.end_line)
        .min_by_key(|symbol| {
            (
                symbol.end_line.saturating_sub(symbol.line),
                Reverse(symbol.line),
            )
        })
}

fn bounded_line_evidence(line: &str) -> String {
    let trimmed = line.trim();
    let mut snippet = trimmed.chars().take(180).collect::<String>();
    if trimmed.chars().count() > 180 {
        snippet.push_str("...");
    }
    snippet
}

fn traversal_provenance(entry: &TraversalEntry, source: &str) -> GraphProvenance {
    GraphProvenance {
        relationship: entry.relationship.clone(),
        direction: entry.direction.clone(),
        depth: entry.depth,
        confidence: entry.confidence,
        source: source.to_string(),
    }
}

fn function_method_kind(kind: Option<&str>) -> Option<&'static str> {
    match kind {
        Some("function") => Some("method"),
        Some("method") => Some("function"),
        _ => None,
    }
}

fn kind_matches(actual: &str, expected: &str) -> bool {
    actual == expected
        || function_method_kind(Some(expected)).is_some_and(|alternate| actual == alternate)
}

fn continuation_for(
    tool: &str,
    truncated: bool,
    returned: usize,
    omitted: usize,
) -> Option<Continuation> {
    truncated.then(|| Continuation {
        cursor: format!("{tool}:offset:{returned}"),
        omitted,
        next_request_hint: format!(
            "narrow the query or inspect returned handles; {tool} caps broad payloads by design"
        ),
    })
}

fn tool_suggestion<'a>(
    tool: &str,
    reason: &str,
    args: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> NextToolSuggestion {
    NextToolSuggestion {
        tool: tool.to_string(),
        reason: reason.to_string(),
        args: args
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    }
}

fn search_next_tool_suggestions(
    exact_hits: &[SymbolHit],
    beyond_grep: &[SymbolHit],
) -> Vec<NextToolSuggestion> {
    let mut suggestions = Vec::new();
    if let Some(hit) = exact_hits.first().or_else(|| beyond_grep.first()) {
        suggestions.push(tool_suggestion(
            "inspect",
            "inspect the top handle for bounded source evidence",
            [("handle", hit.handle.as_str())],
        ));
        suggestions.push(tool_suggestion(
            "related",
            "expand the top symbol only if relationship context is needed",
            [
                ("symbol", hit.name.as_str()),
                ("file", hit.anchor.file.as_str()),
            ],
        ));
    }
    if exact_hits.is_empty() && !beyond_grep.is_empty() {
        suggestions.push(tool_suggestion(
            "symbols",
            "try exact symbol enumeration with a concrete identifier from the semantic result",
            [("query", beyond_grep[0].name.as_str())],
        ));
    }
    suggestions
}

fn symbol_next_tool_suggestions(results: &[SymbolHit]) -> Vec<NextToolSuggestion> {
    let Some(hit) = results.first() else {
        return Vec::new();
    };
    vec![
        tool_suggestion(
            "inspect",
            "inspect the selected symbol instead of reading the whole file",
            [("handle", hit.handle.as_str())],
        ),
        tool_suggestion(
            "impact",
            "check callers/dependents before editing this symbol",
            [
                ("symbol", hit.name.as_str()),
                ("file", hit.anchor.file.as_str()),
            ],
        ),
    ]
}

fn coupled_next_tool_suggestions(results: &[CoupledHit]) -> Vec<NextToolSuggestion> {
    let Some(hit) = results.first() else {
        return Vec::new();
    };
    vec![tool_suggestion(
        "inspect",
        "inspect the strongest related handle for bounded source evidence",
        [("handle", hit.handle.as_str())],
    )]
}

fn evidence_next_tool_suggestions(search: &SearchResponse) -> Vec<NextToolSuggestion> {
    search_next_tool_suggestions(&search.exact_hits, &search.beyond_grep)
}

fn select_target_symbol(symbols: Vec<Symbol>, kind: Option<&str>) -> Option<Symbol> {
    let Some(kind) = kind else {
        return symbols.into_iter().next();
    };
    let alternate = function_method_kind(Some(kind));
    symbols
        .iter()
        .find(|symbol| symbol.kind == kind)
        .cloned()
        .or_else(|| {
            alternate.and_then(|alternate| {
                symbols
                    .iter()
                    .find(|symbol| symbol.kind == alternate)
                    .cloned()
            })
        })
}

fn coupled_to_hits(coupled: Vec<CoupledSymbol>, index_revision: &str) -> Vec<CoupledHit> {
    coupled
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let symbol = entry.symbol;
            let reason = entry.reason;
            CoupledHit {
                handle: symbol_handle(index_revision, &symbol),
                file_handle: file_handle(index_revision, &symbol.file),
                rank: index + 1,
                name: symbol.name.clone(),
                kind: symbol.kind.clone(),
                language: symbol.language.clone(),
                anchor: FileAnchor::from_symbol(&symbol),
                summary: symbol_summary(&symbol),
                symbol,
                score: entry.score,
                reason_codes: vec![reason_code_from_reason(&reason)],
                reason,
                provenance: entry.provenance,
            }
        })
        .collect()
}

fn evidence_fact_hit(hit: BehaviorFactHit, index_revision: &str) -> EvidenceFactHit {
    let anchor = FileAnchor {
        file: hit.fact.file.clone(),
        line: hit.fact.line,
        end_line: hit.fact.end_line,
    };
    let name = hit.fact.value.clone();
    let kind = hit.fact.fact_type.clone();
    let reason_codes = vec![
        format!("fact:{}", hit.fact.fact_type),
        format!("exact:fact:{}", hit.lexical_evidence.field),
    ];
    let summary = format!(
        "{} `{}` in {}:{}",
        hit.fact.fact_type, hit.fact.value, hit.fact.file, hit.fact.line
    );
    EvidenceFactHit {
        handle: behavior_fact_handle(index_revision, &hit.fact),
        file_handle: file_handle(index_revision, &hit.fact.file),
        name,
        kind,
        anchor,
        summary,
        fact: hit.fact,
        lexical_evidence: hit.lexical_evidence,
        reason_codes,
    }
}

fn assign_symbol_ranks(hits: &mut [SymbolHit]) {
    for (index, hit) in hits.iter_mut().enumerate() {
        hit.rank = index + 1;
        for (coupled_index, coupled) in hit.coupled.iter_mut().enumerate() {
            coupled.rank = coupled_index + 1;
        }
    }
}

fn reason_code_from_reason(reason: &str) -> String {
    if reason.contains("structural") {
        "structural".to_string()
    } else if reason.contains("semantic") {
        "semantic".to_string()
    } else if reason.contains("evolutionary") {
        "evolutionary".to_string()
    } else {
        "coupled".to_string()
    }
}

fn symbol_summary(symbol: &Symbol) -> String {
    let summary = symbol
        .context
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map_or_else(
            || format!("{} {}", symbol.kind, symbol.name),
            ToString::to_string,
        );
    bounded_chars(&summary, MAX_SUMMARY_CHARS)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedHandle {
    kind: String,
    index_revision: String,
    target: HandleTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HandleTarget {
    Symbol(i64),
    BehaviorFact(i64),
    Callsite(i64),
    File(String),
}

fn parse_handle(handle: &str) -> Result<ParsedHandle> {
    let mut parts = handle.splitn(3, ':');
    let Some(kind) = parts.next() else {
        return invalid_handle();
    };
    let Some(index_revision) = parts.next() else {
        return invalid_handle();
    };
    let Some(target) = parts.next() else {
        return invalid_handle();
    };
    match kind {
        "symbol" => {
            if target.starts_with("unindexed:") {
                return Err(crate::LoomError::InvalidInput(
                    "unindexed symbol handles cannot be inspected; rerun search after indexing"
                        .to_string(),
                ));
            }
            let symbol_id = target.parse::<i64>().map_err(|_| {
                crate::LoomError::InvalidInput(
                    "symbol handle must end with a numeric symbol id".to_string(),
                )
            })?;
            Ok(ParsedHandle {
                kind: "symbol".to_string(),
                index_revision: index_revision.to_string(),
                target: HandleTarget::Symbol(symbol_id),
            })
        }
        "fact" => {
            if target.starts_with("unindexed:") {
                return Err(crate::LoomError::InvalidInput(
                    "unindexed fact handles cannot be inspected; rerun evidence_pack after indexing"
                        .to_string(),
                ));
            }
            let fact_id = target.parse::<i64>().map_err(|_| {
                crate::LoomError::InvalidInput(
                    "fact handle must end with a numeric behavior fact id".to_string(),
                )
            })?;
            Ok(ParsedHandle {
                kind: "fact".to_string(),
                index_revision: index_revision.to_string(),
                target: HandleTarget::BehaviorFact(fact_id),
            })
        }
        "callsite" => {
            if target.starts_with("unindexed:") {
                return Err(crate::LoomError::InvalidInput(
                    "unindexed callsite handles cannot be inspected; rerun related after indexing"
                        .to_string(),
                ));
            }
            let callsite_id = target.parse::<i64>().map_err(|_| {
                crate::LoomError::InvalidInput(
                    "callsite handle must end with a numeric callsite id".to_string(),
                )
            })?;
            Ok(ParsedHandle {
                kind: "callsite".to_string(),
                index_revision: index_revision.to_string(),
                target: HandleTarget::Callsite(callsite_id),
            })
        }
        "file" => {
            let file = decode_file_handle_path(target).ok_or_else(|| {
                crate::LoomError::InvalidInput(
                    "file handle path is not valid hex; rerun search to obtain a handle"
                        .to_string(),
                )
            })?;
            Ok(ParsedHandle {
                kind: "file".to_string(),
                index_revision: index_revision.to_string(),
                target: HandleTarget::File(file),
            })
        }
        _ => invalid_handle(),
    }
}

fn invalid_handle() -> Result<ParsedHandle> {
    Err(crate::LoomError::InvalidInput(
        "handle must be symbol:{index_revision}:{symbol_id} or file:{index_revision}:{hex_path}"
            .to_string(),
    ))
}

#[derive(Debug, Clone)]
struct SnippetRead {
    snippet: Option<InspectSnippet>,
    truncated: bool,
    has_more: bool,
}

fn read_snippet(
    path: &Path,
    anchor: &FileAnchor,
    start_line: i64,
    requested_end: i64,
    char_budget: usize,
) -> Result<SnippetRead> {
    let file = File::open(path).map_err(|source| crate::LoomError::IndexerIo {
        path: path.display().to_string(),
        source,
    })?;
    let reader = BufReader::new(file);
    let mut text = String::new();
    let mut chars = 0usize;
    let mut end_line = start_line.saturating_sub(1);
    let mut truncated = false;
    let mut has_more = false;

    for line in reader.lines().enumerate() {
        let (index, line) = line;
        let line_number = i64::try_from(index + 1).unwrap_or(i64::MAX);
        let line = line.map_err(|source| crate::LoomError::IndexerIo {
            path: path.display().to_string(),
            source,
        })?;
        if line_number < start_line {
            continue;
        }
        if line_number > requested_end {
            has_more = true;
            break;
        }

        let separator_chars = usize::from(!text.is_empty());
        let line_chars = line.chars().count();
        if chars + separator_chars + line_chars > char_budget {
            if !text.is_empty() && chars < char_budget {
                text.push('\n');
                chars += 1;
            }
            let remaining = char_budget.saturating_sub(chars);
            if remaining > 0 {
                text.push_str(&line.chars().take(remaining).collect::<String>());
                chars = text.chars().count();
                end_line = line_number;
            }
            truncated = true;
            break;
        }

        if !text.is_empty() {
            text.push('\n');
            chars += 1;
        }
        text.push_str(&line);
        chars += line_chars;
        end_line = line_number;
    }

    if text.is_empty() {
        return Ok(SnippetRead {
            snippet: None,
            truncated,
            has_more,
        });
    }

    Ok(SnippetRead {
        snippet: Some(InspectSnippet {
            anchor: anchor.clone(),
            start_line,
            end_line,
            text,
            chars,
        }),
        truncated,
        has_more,
    })
}

struct InspectResponseParts<'a> {
    handle: &'a str,
    handle_kind: &'a str,
    index_revision: String,
    limit: usize,
    truncated: bool,
    stale: bool,
    error: Option<String>,
    anchor: Option<FileAnchor>,
    snippet: Option<InspectSnippet>,
    page: InspectPage,
}

struct LineSpanInspection<'a> {
    handle: &'a str,
    handle_kind: &'a str,
    index_revision: &'a str,
    file: &'a str,
    anchor: FileAnchor,
    line_budget: usize,
    char_budget: usize,
    line_offset: usize,
}

fn inspect_response(parts: InspectResponseParts<'_>) -> InspectResponse {
    let returned = parts.snippet.as_ref().map_or(0, |snippet| {
        (snippet.end_line - snippet.start_line + 1).max(1) as usize
    });
    let display_text = if let Some(snippet) = &parts.snippet {
        format!(
            "{}:{}-{}",
            snippet.anchor.file, snippet.start_line, snippet.end_line
        )
    } else {
        parts
            .error
            .clone()
            .unwrap_or_else(|| "no snippet returned".to_string())
    };
    let omitted = usize::from(parts.snippet.is_none());
    let envelope = response_envelope(
        INSPECT_CONTRACT,
        parts.index_revision,
        parts.limit,
        parts.truncated,
        false,
    );
    InspectResponse {
        contract: envelope.contract,
        version: envelope.version,
        index_revision: envelope.index_revision,
        limit: envelope.limit,
        truncated: envelope.truncated,
        inspect_required: envelope.inspect_required,
        budget: response_budget(
            "lines",
            envelope.limit,
            returned,
            omitted,
            envelope.truncated,
        ),
        handle: parts.handle.to_string(),
        handle_kind: parts.handle_kind.to_string(),
        stale: parts.stale,
        error: parts.error,
        anchor: parts.anchor,
        snippet: parts.snippet,
        page: parts.page,
        display_text,
    }
}

fn stale_inspect_response(
    handle: &str,
    handle_kind: &str,
    index_revision: &str,
    limit: usize,
    error: String,
) -> InspectResponse {
    inspect_response(InspectResponseParts {
        handle,
        handle_kind,
        index_revision: index_revision.to_string(),
        limit,
        truncated: false,
        stale: true,
        error: Some(error),
        anchor: None,
        snippet: None,
        page: InspectPage {
            line_offset: 0,
            next_line_offset: None,
            refused: false,
            refusal_reason: None,
        },
    })
}

fn mark_stale_recovered(
    mut response: InspectResponse,
    stale_revision: bool,
    source_revision: &str,
) -> InspectResponse {
    if stale_revision {
        response.stale = true;
        response.error = Some(format!(
            "stale handle from {source_revision}; recovered against current index by stable path/id, rerun search for a fresh handle before relying on exact identity"
        ));
    }
    response
}

fn evidence_coverage(
    search: &SearchResponse,
    inspected_snippets: &[InspectSnippet],
    behavior_facts: &[BehaviorFactHit],
    role_cards: &[FileRoleCard],
) -> Vec<EvidenceCoverageItem> {
    let has_graph = search.beyond_grep.iter().any(|hit| {
        hit.reason_codes
            .iter()
            .any(|reason| reason.starts_with("graph:"))
    });
    vec![
        EvidenceCoverageItem {
            item: "exact_matches".to_string(),
            status: if search.exact_hits.is_empty() {
                "missing".to_string()
            } else {
                "included".to_string()
            },
            detail: format!("{} exact lexical hits", search.exact_hits.len()),
        },
        EvidenceCoverageItem {
            item: "beyond_grep".to_string(),
            status: if search.beyond_grep.is_empty() {
                "missing".to_string()
            } else {
                "included".to_string()
            },
            detail: format!("{} semantic or graph hits", search.beyond_grep.len()),
        },
        EvidenceCoverageItem {
            item: "graph_neighbors".to_string(),
            status: if has_graph {
                "included".to_string()
            } else {
                "not_found".to_string()
            },
            detail: "graph-derived results are marked with graph:* reason codes".to_string(),
        },
        EvidenceCoverageItem {
            item: "behavior_facts".to_string(),
            status: if behavior_facts.is_empty() {
                "not_found".to_string()
            } else {
                "included".to_string()
            },
            detail: format!("{} indexed behavior facts", behavior_facts.len()),
        },
        EvidenceCoverageItem {
            item: "role_cards".to_string(),
            status: if role_cards.is_empty() {
                "not_found".to_string()
            } else {
                "included".to_string()
            },
            detail: format!("{} file role cards", role_cards.len()),
        },
        EvidenceCoverageItem {
            item: "source_snippets".to_string(),
            status: if inspected_snippets.is_empty() {
                "missing".to_string()
            } else {
                "included".to_string()
            },
            detail: format!("{} inspected snippets", inspected_snippets.len()),
        },
    ]
}

fn missing_concepts(query: &str, search: &SearchResponse) -> Vec<String> {
    let mut missing = Vec::new();
    if search.exact_hits.is_empty() {
        missing.push(format!(
            "no exact indexed lexical symbol hits for `{query}`; current exact search is symbol-FTS, not whole-file grep"
        ));
    }
    if search.exact_hits.is_empty() && search.beyond_grep.is_empty() {
        missing.push("no semantic or graph indexed evidence found".to_string());
    }
    missing
}

fn bounded_chars(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}

fn compact_role_card(card: &mut FileRoleCard) {
    card.content_hash.clear();
    card.primary_responsibility = bounded_chars(&card.primary_responsibility, MAX_SUMMARY_CHARS);
    card.exported_symbols.truncate(MAX_EVIDENCE_CARD_ITEMS);
    card.imported_dependencies.truncate(MAX_EVIDENCE_CARD_ITEMS);
    card.behavior_facts.truncate(MAX_EVIDENCE_CARD_ITEMS);
    card.tests_touching.truncate(MAX_EVIDENCE_CARD_ITEMS);
    card.top_related_files.truncate(MAX_EVIDENCE_CARD_ITEMS);
}

fn sort_hits(hits: &mut [SymbolHit]) {
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.symbol.file.cmp(&right.symbol.file))
            .then_with(|| left.symbol.line.cmp(&right.symbol.line))
            .then_with(|| left.symbol.name.cmp(&right.symbol.name))
            .then_with(|| left.handle.cmp(&right.handle))
    });
}

fn symbol_query(symbol: &str, file: Option<&str>, kind: Option<&str>) -> SymbolQuery {
    SymbolQuery {
        symbol: symbol.to_string(),
        file: file.map(ToString::to_string),
        kind: kind.map(ToString::to_string),
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
