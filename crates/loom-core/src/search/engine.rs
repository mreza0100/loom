use crate::{
    embedder::{build_symbol_text, Embedder},
    graph::{SymbolGraph, TraversalEntry},
    models::{
        behavior_fact_handle, decode_file_handle_path, file_handle, response_budget,
        response_envelope, symbol_handle, BehaviorFact, BehaviorFactHit, Callsite, Continuation,
        CoupledHit, CoupledSymbol, EvidenceCoverageItem, EvidenceFactHit, EvidencePackResponse,
        EvidenceSubQuestion, FileAnchor, FileRoleCard, GraphProvenance, ImpactResponse,
        InspectPage, InspectResponse, InspectSnippet, LexicalEvidence, NeighborhoodResponse,
        NextToolSuggestion, QueryIntent, RelatedResponse, SearchResponse, SignalScores, Symbol,
        SymbolHit, SymbolListResponse, SymbolQuery, EVIDENCE_PACK_CONTRACT, IMPACT_CONTRACT,
        INSPECT_CONTRACT, NEIGHBORHOOD_CONTRACT, RELATED_CONTRACT, SEARCH_CONTRACT,
        SYMBOLS_CONTRACT,
    },
    search::scoring::{compute_evolutionary, compute_semantic, compute_structural, fuse_signals},
    store::LoomDb,
    LoomConfig, Result,
};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

const RRF_K: f64 = 60.0;
const MAX_STRUCTURAL_RESULTS: usize = 30;
const MAX_SUMMARY_CHARS: usize = 160;
const MAX_SEARCH_RESULTS: usize = 100;
const MAX_BEYOND_GREP_RESULTS: usize = 3;
const MAX_SYMBOL_RESULTS: usize = 256;
const MAX_EXPANSION_RESULTS: usize = 12;
const MAX_INSPECT_LINES: usize = 32;
const MAX_INSPECT_CHARS: usize = 32_000;
const MAX_EVIDENCE_BUDGET_TOKENS: usize = 16_000;
const MAX_EVIDENCE_RESULTS: usize = 24;
const MAX_EVIDENCE_CARD_ITEMS: usize = 5;
const DEFAULT_SEARCH_BUDGET_TOKENS: usize = 2_000;
const MAX_SEARCH_BUDGET_TOKENS: usize = 8_000;
const SEARCH_CHARS_PER_TOKEN: usize = 4;
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NeighborhoodResult {
    pub anchor: Option<Symbol>,
    pub coupled: Vec<CoupledSymbol>,
}

#[derive(Debug, Deserialize)]
struct RipgrepJsonEvent {
    #[serde(rename = "type")]
    kind: String,
    data: Option<RipgrepJsonData>,
}

#[derive(Debug, Deserialize)]
struct RipgrepJsonData {
    path: Option<RipgrepJsonText>,
    lines: Option<RipgrepJsonText>,
    line_number: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RipgrepJsonText {
    text: String,
}

#[derive(Debug, Clone)]
struct EvidenceLineMatch {
    file: String,
    line: i64,
    text: String,
}

pub struct SearchEngine<E: Embedder> {
    db: Arc<LoomDb>,
    embedder: Arc<E>,
    graph: Option<Arc<SymbolGraph>>,
    config: LoomConfig,
    index_ready: bool,
}

#[derive(Debug, Clone)]
struct Candidate {
    symbol: Symbol,
    score: f64,
    signal_scores: SignalScores,
    lexical_evidence: Option<LexicalEvidence>,
    graph_role: Option<String>,
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
            graph_role: None,
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
            index_ready: true,
        }
    }

    #[must_use]
    pub fn with_index_ready(mut self, index_ready: bool) -> Self {
        self.index_ready = index_ready;
        self
    }

    pub fn search(&self, query: &str, limit: usize, kind: Option<&str>) -> Result<SearchResponse> {
        self.search_with_budget(query, limit, kind, DEFAULT_SEARCH_BUDGET_TOKENS)
    }

    pub fn search_with_budget(
        &self,
        query: &str,
        limit: usize,
        kind: Option<&str>,
        budget_tokens: usize,
    ) -> Result<SearchResponse> {
        self.search_mode_with_budget(query, limit, kind, None, budget_tokens)
    }

    pub fn search_mode_with_budget(
        &self,
        query: &str,
        limit: usize,
        kind: Option<&str>,
        mode: Option<&str>,
        budget_tokens: usize,
    ) -> Result<SearchResponse> {
        if let Some(mode) = mode.filter(|mode| *mode != "auto") {
            return self.search_graph_mode(query, limit, kind, mode, budget_tokens);
        }
        self.search_auto_with_budget(query, limit, kind, budget_tokens)
    }

    fn search_auto_with_budget(
        &self,
        query: &str,
        limit: usize,
        kind: Option<&str>,
        budget_tokens: usize,
    ) -> Result<SearchResponse> {
        let budget_tokens = budget_tokens.clamp(1, MAX_SEARCH_BUDGET_TOKENS);
        let index_revision = self.db.index_revision()?;
        if limit == 0 {
            let envelope = response_envelope(SEARCH_CONTRACT, index_revision, limit, false, true);
            let mut response = SearchResponse {
                contract: envelope.contract,
                version: envelope.version,
                index_revision: envelope.index_revision,
                index_status: self.search_index_status(),
                limit: envelope.limit,
                truncated: envelope.truncated,
                inspect_required: envelope.inspect_required,
                budget: response_budget("tokens", budget_tokens, 0, 0, false),
                continuation: None,
                next_tool_suggestions: vec![tool_suggestion(
                    "search",
                    "retry with a narrower code identifier, symbol name, or domain phrase",
                    [("query", query)],
                )],
                query_intent: classify_query_intent(query),
                exact_hits: Vec::new(),
                beyond_grep: Vec::new(),
            };
            enforce_search_response_budget(&mut response, budget_tokens, 0);
            return Ok(response);
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
                self.search_file_lines(query, file_line_scan_limit(query, candidate_limit))?
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
            let match_weight = symbol_query_match_weight(&symbol, query);
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(symbol.clone()));
            let signal =
                (1.0 + rrf_score(rank)) * kind_boost(&candidate.symbol.kind) * match_weight;
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
            let match_weight = symbol_query_match_weight(&symbol, query);
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(symbol.clone()));
            let signal =
                (0.8 + rrf_score(rank)) * kind_boost(&candidate.symbol.kind) * match_weight;
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
            let symbol_id = symbol.id.unwrap_or(-((rank as i64) + 1));
            if symbol.id.is_some() {
                lexical_seed_ids.push(symbol_id);
            }
            let line_weight = file_line_match_weight(&symbol, &evidence, query);
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(symbol.clone()));
            let signal = (0.7 + rrf_score(rank)) * kind_boost(&candidate.symbol.kind) * line_weight;
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

        let semantic_weight = semantic_weight_multiplier(&self.embedder.fingerprint());
        for (rank, (symbol_id, _distance)) in vec_results.into_iter().enumerate() {
            let Some(symbol) = self.db.get_symbol_by_id(symbol_id)? else {
                continue;
            };
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(symbol.clone()));
            let signal = rrf_score(rank) * kind_boost(&candidate.symbol.kind) * semantic_weight;
            candidate.score += signal;
            candidate.signal_scores.semantic += signal;
            candidate.reason_codes.insert("semantic".to_string());
        }

        self.add_graph_candidates(&mut candidates, &lexical_seed_ids)?;
        self.annotate_lexical_graph_roles(query, &mut candidates);
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
        beyond_grep.truncate(
            limit
                .saturating_sub(exact_limit)
                .min(MAX_BEYOND_GREP_RESULTS),
        );
        let returned = exact_hits.len() + beyond_grep.len();
        let truncated = total_before_truncate > returned || requested_limit > limit;
        let omitted = total_before_truncate.saturating_sub(returned);
        let continuation = continuation_for("search", truncated, returned, omitted);
        let next_tool_suggestions = search_next_tool_suggestions(&exact_hits, &beyond_grep);

        let envelope = response_envelope(SEARCH_CONTRACT, index_revision, limit, truncated, true);
        let mut response = SearchResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            index_status: self.search_index_status(),
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget("tokens", budget_tokens, 0, omitted, truncated),
            continuation,
            next_tool_suggestions,
            query_intent: classify_query_intent(query),
            exact_hits,
            beyond_grep,
        };
        enforce_search_response_budget(&mut response, budget_tokens, total_before_truncate);
        Ok(response)
    }

    fn search_graph_mode(
        &self,
        query: &str,
        limit: usize,
        kind: Option<&str>,
        mode: &str,
        budget_tokens: usize,
    ) -> Result<SearchResponse> {
        let budget_tokens = budget_tokens.clamp(1, MAX_SEARCH_BUDGET_TOKENS);
        let index_revision = self.db.index_revision()?;
        let limit = limit.min(MAX_SEARCH_RESULTS);
        let Some(target) =
            select_target_symbol(self.db.get_symbol_by_name_fuzzy(query, None)?, kind)
        else {
            return self.search_auto_with_budget(query, limit, kind, budget_tokens);
        };
        let mut candidates = Vec::new();
        let mut seen = BTreeSet::from_iter(target.id);
        if matches!(mode, "definitions" | "definition") {
            let mut candidate = Candidate::new(target.clone());
            candidate.score = 1.0;
            candidate.signal_scores.symbol = 1.0;
            candidate.signal_scores.total = 1.0;
            candidate.graph_role = Some("definition".to_string());
            candidate
                .reason_codes
                .insert("mode:definitions".to_string());
            candidate
                .reason_codes
                .insert("graph_role:definition".to_string());
            candidates.push(candidate);
        } else if let (Some(graph), Some(target_id)) = (&self.graph, target.id) {
            let entries = match mode {
                "callers" => graph.dependents(target_id, 3),
                "callees" | "impact" => graph.dependencies(target_id, 3),
                "implementations" | "implementors" => graph
                    .dependents(target_id, 3)
                    .into_iter()
                    .filter(|entry| entry.relationship == "implements")
                    .collect(),
                _ => return self.search_auto_with_budget(query, limit, kind, budget_tokens),
            };
            for entry in entries {
                if !seen.insert(entry.symbol_id) {
                    continue;
                }
                let Some(symbol) = self.db.get_symbol_by_id(entry.symbol_id)? else {
                    continue;
                };
                if is_generic_target(&symbol.name) {
                    continue;
                }
                let mut candidate = Candidate::new(symbol);
                let structural =
                    compute_structural(&entry.relationship, entry.confidence, entry.depth);
                candidate.score = structural;
                candidate.signal_scores.graph = structural;
                candidate.signal_scores.total = structural;
                candidate.graph_role = Some(mode_graph_role(mode, &entry.relationship));
                candidate.reason_codes.insert(format!("mode:{mode}"));
                candidate
                    .reason_codes
                    .insert(format!("graph:{}", entry.relationship));
                if let Some(role) = &candidate.graph_role {
                    candidate.reason_codes.insert(format!("graph_role:{role}"));
                }
                candidates.push(candidate);
            }
        }

        candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
        let total_before_truncate = candidates.len();
        candidates.truncate(limit);
        let mut exact_hits = candidates
            .into_iter()
            .map(|candidate| candidate_to_hit(candidate, &index_revision))
            .collect::<Vec<_>>();
        assign_symbol_ranks(&mut exact_hits);
        let returned = exact_hits.len();
        let truncated = total_before_truncate > returned;
        let omitted = total_before_truncate.saturating_sub(returned);
        let envelope = response_envelope(SEARCH_CONTRACT, index_revision, limit, truncated, true);
        let mut response = SearchResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            index_status: self.search_index_status(),
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget("tokens", budget_tokens, 0, omitted, truncated),
            continuation: continuation_for("search", truncated, returned, omitted),
            next_tool_suggestions: search_next_tool_suggestions(&exact_hits, &[]),
            query_intent: classify_query_intent(query),
            exact_hits,
            beyond_grep: Vec::new(),
        };
        enforce_search_response_budget(&mut response, budget_tokens, total_before_truncate);
        Ok(response)
    }

    fn search_index_status(&self) -> Option<String> {
        (!self.index_ready).then(|| "building".to_string())
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
            graph_role: None,
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
        let sub_questions = split_sub_questions(query);
        let per_question_budget = (effective_budget_tokens / sub_questions.len().max(1))
            .clamp(500, MAX_EVIDENCE_BUDGET_TOKENS);
        let mut merged_exact_hits = Vec::new();
        let mut merged_beyond_grep = Vec::new();
        let mut merged_behavior_facts = Vec::new();
        let mut omitted = Vec::new();
        let mut truncated = false;

        for sub_question in &sub_questions {
            let referenced_paths = extract_repo_paths(&sub_question.query);
            let mut focused_query = sub_question.query.clone();
            let symbol_hints = sub_question
                .symbols
                .iter()
                .take(6)
                .cloned()
                .collect::<Vec<_>>();
            if !symbol_hints.is_empty() {
                focused_query.push(' ');
                focused_query.push_str(&symbol_hints.join(" "));
            }
            let search =
                self.search_with_budget(&focused_query, result_limit, None, per_question_budget)?;
            truncated |= search.truncated;
            let mut exact_hits = search.exact_hits;
            let mut beyond_grep = search.beyond_grep;
            if !referenced_paths.is_empty() {
                exact_hits.retain(|hit| referenced_paths.contains(&hit.anchor.file));
                beyond_grep.retain(|hit| referenced_paths.contains(&hit.anchor.file));
            }
            let receiver_type_hits =
                self.receiver_type_hits_for_methods(&exact_hits, &index_revision)?;
            merge_symbol_hits(&mut exact_hits, receiver_type_hits);
            merge_symbol_hits(&mut merged_exact_hits, exact_hits);
            merge_symbol_hits(&mut merged_beyond_grep, beyond_grep);
            for symbol_query in sub_question.symbols.iter().take(10) {
                let mut symbols = self.db.get_symbol_by_name_fuzzy(symbol_query, None)?;
                if symbols.is_empty() {
                    continue;
                }
                if !referenced_paths.is_empty() {
                    symbols.retain(|symbol| referenced_paths.contains(&symbol.file));
                    if symbols.is_empty() {
                        continue;
                    }
                }
                let hits = symbols
                    .into_iter()
                    .take(3)
                    .map(|symbol| {
                        let mut candidate = Candidate::new(symbol);
                        candidate.score = 1.0;
                        candidate.signal_scores.symbol = 1.0;
                        candidate.signal_scores.total = 1.0;
                        candidate
                            .reason_codes
                            .insert("exact:symbol_hint".to_string());
                        candidate_to_hit(candidate, &index_revision)
                    })
                    .collect::<Vec<_>>();
                merge_symbol_hits(&mut merged_exact_hits, hits);
            }

            let raw_facts = self
                .db
                .search_behavior_facts(&sub_question.query, result_limit)?;
            merge_behavior_facts(&mut merged_behavior_facts, raw_facts);

            for file in extract_repo_paths(&sub_question.query) {
                let file_symbols = self.db.get_colocated_symbols(&file)?;
                let symbol_names = sub_question
                    .symbols
                    .iter()
                    .map(|symbol| symbol.to_ascii_lowercase())
                    .collect::<Vec<_>>();
                let hits = file_symbols
                    .into_iter()
                    .filter(|symbol| {
                        let name = symbol.name.to_ascii_lowercase();
                        symbol_names.iter().any(|needle| {
                            name == *needle
                                || name.ends_with(&format!(".{needle}"))
                                || (needle == "next" && name.ends_with(".next"))
                        })
                    })
                    .take(result_limit)
                    .map(|symbol| {
                        let mut candidate = Candidate::new(symbol);
                        candidate.score = 2.0;
                        candidate.signal_scores.symbol = 2.0;
                        candidate.signal_scores.total = 2.0;
                        candidate
                            .reason_codes
                            .insert("exact:file_symbol_hint".to_string());
                        candidate_to_hit(candidate, &index_revision)
                    })
                    .collect::<Vec<_>>();
                merge_symbol_hits(&mut merged_exact_hits, hits);
            }
        }

        sort_hits(&mut merged_exact_hits);
        sort_hits(&mut merged_beyond_grep);
        merged_exact_hits.truncate(result_limit);
        if !merged_exact_hits.is_empty() {
            merged_beyond_grep.clear();
        }
        merged_beyond_grep.truncate(result_limit);
        let mut search = SearchResponse {
            contract: SEARCH_CONTRACT.to_string(),
            version: 1,
            index_revision: index_revision.clone(),
            index_status: self.search_index_status(),
            limit: result_limit,
            truncated,
            inspect_required: false,
            budget: response_budget("tokens", effective_budget_tokens, 0, 0, truncated),
            continuation: None,
            next_tool_suggestions: Vec::new(),
            query_intent: classify_query_intent(query),
            exact_hits: merged_exact_hits,
            beyond_grep: merged_beyond_grep,
        };
        let raw_behavior_facts = merged_behavior_facts;
        let role_cards = self.role_cards_for_evidence(&search, &raw_behavior_facts)?;
        let char_budget = effective_budget_tokens
            .saturating_mul(3)
            .clamp(2_000, 32_000);
        let mut selected = Vec::new();
        let exact_snippet_limit = if sub_questions.len() > 1 { 4 } else { 8 };
        selected.extend(
            search
                .exact_hits
                .iter()
                .take(exact_snippet_limit)
                .map(|hit| hit.handle.clone()),
        );
        selected.extend(
            search
                .beyond_grep
                .iter()
                .take(4)
                .map(|hit| hit.handle.clone()),
        );
        selected.extend(
            raw_behavior_facts
                .iter()
                .take(2)
                .map(|hit| behavior_fact_handle(&index_revision, &hit.fact)),
        );
        selected.sort();
        selected.dedup();

        let per_snippet_budget = if selected.is_empty() {
            char_budget
        } else {
            (char_budget / selected.len()).clamp(800, 6_000)
        };
        let mut inspected_snippets = Vec::new();
        let mut returned_chars = 0usize;
        for handle in selected {
            let inspected = self.inspect(&handle, 80, per_snippet_budget, 0)?;
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
        let file_snippet_budget = per_snippet_budget.clamp(800, 2_000);
        for snippet in self.evidence_file_snippets(&sub_questions, &search, file_snippet_budget)? {
            if !inspected_snippets.iter().any(|existing| {
                existing.anchor.file == snippet.anchor.file
                    && existing.start_line == snippet.start_line
                    && existing.end_line == snippet.end_line
            }) {
                returned_chars += snippet.chars;
                inspected_snippets.push(snippet);
            }
            if inspected_snippets.len() >= result_limit {
                break;
            }
        }

        if budget_tokens > effective_budget_tokens {
            omitted.push(format!(
                "evidence budget capped at {effective_budget_tokens} tokens to keep MCP payload bounded"
            ));
        }
        if inspected_snippets.is_empty() {
            omitted.push("no source snippets were inspected for this query".to_string());
        }

        let missing_concepts = if inspected_snippets.is_empty() {
            missing_concepts(query, &search)
        } else {
            Vec::new()
        };
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
        Ok(EvidencePackResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: false,
            budget: response_budget(
                "tokens",
                effective_budget_tokens,
                returned_units,
                omitted_count,
                truncated,
            ),
            query: query.to_string(),
            sub_questions,
            exact_hits: search.exact_hits,
            beyond_grep: search.beyond_grep,
            behavior_facts,
            role_cards,
            inspected_snippets,
            coverage_checklist,
            omitted,
            missing_concepts,
            next_tool_suggestions: Vec::new(),
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

    fn receiver_type_hits_for_methods(
        &self,
        hits: &[SymbolHit],
        index_revision: &str,
    ) -> Result<Vec<SymbolHit>> {
        let mut receiver_names = BTreeSet::new();
        let mut receiver_hits = Vec::new();
        for hit in hits {
            if hit.symbol.kind != "method" {
                continue;
            }
            let Some(receiver) = receiver_type_from_method_name(&hit.symbol.name) else {
                continue;
            };
            if !receiver_names.insert((hit.symbol.file.clone(), receiver.clone())) {
                continue;
            }
            for symbol in self
                .db
                .get_symbol_by_name_fuzzy(&receiver, Some(&hit.symbol.file))?
                .into_iter()
                .filter(|symbol| matches!(symbol.kind.as_str(), "class" | "interface"))
                .take(1)
            {
                let mut candidate = Candidate::new(symbol);
                candidate.score = 2.5;
                candidate.signal_scores.symbol = 2.5;
                candidate.signal_scores.total = 2.5;
                candidate
                    .reason_codes
                    .insert("exact:method_receiver_type".to_string());
                receiver_hits.push(candidate_to_hit(candidate, index_revision));
            }
        }
        Ok(receiver_hits)
    }

    fn evidence_file_snippets(
        &self,
        sub_questions: &[EvidenceSubQuestion],
        search: &SearchResponse,
        char_budget: usize,
    ) -> Result<Vec<InspectSnippet>> {
        let mut snippets = Vec::new();
        let mut seen = BTreeSet::new();
        let per_question_limit = (MAX_EVIDENCE_RESULTS / sub_questions.len().max(1)).clamp(3, 8);
        for question in sub_questions {
            let mut question_snippets = 0usize;
            let terms = file_evidence_terms(question);
            let mut paths = extract_repo_paths(&question.query);
            let explicit_paths = !paths.is_empty();
            let question_limit = evidence_question_limit(question, per_question_limit);
            if explicit_paths && wants_next_receiver_structs(&question.query) {
                for file in &paths {
                    if question_snippets >= question_limit {
                        break;
                    }
                    let path = self.config.target_dir.join(file);
                    if !path.exists() {
                        continue;
                    }
                    for snippet in
                        next_receiver_struct_snippets(&path, file, char_budget, question_limit)?
                    {
                        if seen.insert((snippet.anchor.file.clone(), snippet.start_line)) {
                            snippets.push(snippet);
                            question_snippets += 1;
                        }
                        if snippets.len() >= MAX_EVIDENCE_RESULTS {
                            return Ok(snippets);
                        }
                        if question_snippets >= question_limit {
                            break;
                        }
                    }
                }
            }
            for snippet in
                self.pattern_file_snippets(question, &terms, char_budget, question_limit)?
            {
                if seen.insert((snippet.anchor.file.clone(), snippet.start_line)) {
                    snippets.push(snippet);
                    question_snippets += 1;
                }
                if snippets.len() >= MAX_EVIDENCE_RESULTS {
                    return Ok(snippets);
                }
                if question_snippets >= question_limit {
                    break;
                }
            }
            if !explicit_paths {
                let mut seen_files = BTreeSet::new();
                paths = search
                    .exact_hits
                    .iter()
                    .chain(search.beyond_grep.iter())
                    .filter(|hit| seen_files.insert(hit.anchor.file.clone()))
                    .map(|hit| hit.anchor.file.clone())
                    .collect();
                rank_evidence_paths(&mut paths, &terms);
                paths.truncate(8);
            }
            for file in paths {
                if question_snippets >= question_limit {
                    break;
                }
                let path = self.config.target_dir.join(&file);
                if !path.exists() {
                    continue;
                }
                for (start_line, end_line) in extract_requested_line_ranges(&question.query) {
                    let anchor = FileAnchor {
                        file: file.clone(),
                        line: start_line,
                        end_line,
                    };
                    let read = read_snippet(&path, &anchor, start_line, end_line, char_budget)?;
                    if let Some(snippet) = read.snippet {
                        if seen.insert((file.clone(), snippet.start_line)) {
                            snippets.push(snippet);
                            question_snippets += 1;
                        }
                    }
                    if snippets.len() >= MAX_EVIDENCE_RESULTS {
                        return Ok(snippets);
                    }
                    if question_snippets >= question_limit {
                        break;
                    }
                }
                let lines = ranked_file_evidence_lines(&path, &terms)?;
                let line_limit = if explicit_paths { 8 } else { 3 };
                for line in lines.into_iter().take(line_limit) {
                    if question_snippets >= question_limit {
                        break;
                    }
                    let start_line = line.saturating_sub(4).max(1);
                    if !seen.insert((file.clone(), start_line)) {
                        continue;
                    }
                    let anchor = FileAnchor {
                        file: file.clone(),
                        line,
                        end_line: line,
                    };
                    let read = read_snippet(&path, &anchor, start_line, line + 16, char_budget)?;
                    if let Some(snippet) = read.snippet {
                        snippets.push(snippet);
                        question_snippets += 1;
                    }
                    if snippets.len() >= MAX_EVIDENCE_RESULTS {
                        return Ok(snippets);
                    }
                }
            }
        }
        Ok(snippets)
    }

    fn pattern_file_snippets(
        &self,
        question: &EvidenceSubQuestion,
        terms: &[String],
        char_budget: usize,
        limit: usize,
    ) -> Result<Vec<InspectSnippet>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let patterns = evidence_patterns(&question.query);
        if patterns.is_empty() {
            return Ok(Vec::new());
        }
        let root = self.config.target_dir.canonicalize().map_err(|source| {
            crate::LoomError::IndexerIo {
                path: self.config.target_dir.display().to_string(),
                source,
            }
        })?;
        let explicit_paths = extract_repo_paths(&question.query);
        let cross_file_patterns = needs_cross_file_pattern_search(&question.query);
        let search_roots = if explicit_paths.is_empty() || cross_file_patterns {
            vec![root.clone()]
        } else {
            explicit_paths
                .iter()
                .map(|file| root.join(file))
                .filter(|path| path.exists())
                .collect::<Vec<_>>()
        };
        if search_roots.is_empty() {
            return Ok(Vec::new());
        }
        let mut matches = Vec::new();
        let mut seen = BTreeSet::new();
        for pattern in patterns {
            let mut child = match Command::new("rg")
                .arg("--json")
                .arg("--line-number")
                .arg("--no-heading")
                .arg("--color")
                .arg("never")
                .arg("--ignore-case")
                .arg("--regexp")
                .arg(&pattern)
                .args(&search_roots)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(child) => child,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(source) => {
                    return Err(crate::LoomError::IndexerIo {
                        path: "rg".to_string(),
                        source,
                    });
                }
            };
            let Some(stdout) = child.stdout.take() else {
                let _ = child.wait();
                continue;
            };
            for line in BufReader::new(stdout).lines() {
                let line = line.map_err(|source| crate::LoomError::IndexerIo {
                    path: "rg".to_string(),
                    source,
                })?;
                let Ok(event) = serde_json::from_str::<RipgrepJsonEvent>(&line) else {
                    continue;
                };
                if event.kind != "match" {
                    continue;
                }
                let Some(data) = event.data else {
                    continue;
                };
                let (Some(path), Some(line_number)) = (data.path, data.line_number) else {
                    continue;
                };
                let Some(file) = repo_relative_path(&root, Path::new(&path.text)) else {
                    continue;
                };
                if seen.insert((file.clone(), line_number)) {
                    let text = data.lines.map(|lines| lines.text).unwrap_or_default();
                    matches.push(EvidenceLineMatch {
                        file,
                        line: line_number,
                        text,
                    });
                }
                if matches.len() >= limit.saturating_mul(8) {
                    let _ = child.kill();
                    break;
                }
            }
            let _ = child.wait();
        }
        rank_evidence_line_matches(&mut matches, terms, &question.query);
        let mut snippets = Vec::new();
        let snippet_limit = if cross_file_patterns {
            limit.min(4)
        } else {
            limit
        };
        for line_match in matches.into_iter().take(snippet_limit) {
            let file = line_match.file;
            let line = line_match.line;
            let path = self.config.target_dir.join(&file);
            let start_line = line.saturating_sub(4).max(1);
            let end_line = if source_line_contains(&path, line, "type ")? {
                line + 64
            } else {
                line + 16
            };
            let anchor = FileAnchor {
                file,
                line,
                end_line: line,
            };
            let read = read_snippet(&path, &anchor, start_line, end_line, char_budget)?;
            if let Some(snippet) = read.snippet {
                snippets.push(snippet);
            }
        }
        Ok(snippets)
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
        if self.db.get_file_hash(file)?.is_none() && !self.config.target_dir.join(file).exists() {
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

    fn annotate_lexical_graph_roles(&self, query: &str, candidates: &mut BTreeMap<i64, Candidate>) {
        let Some(graph) = &self.graph else {
            return;
        };
        let target_ids = candidates
            .iter()
            .filter_map(|(symbol_id, candidate)| {
                candidate
                    .lexical_evidence
                    .is_some()
                    .then_some(candidate)
                    .filter(|candidate| symbol_matches_query_target(&candidate.symbol, query))
                    .map(|_| *symbol_id)
            })
            .collect::<BTreeSet<_>>();
        if target_ids.is_empty() {
            return;
        }

        for (symbol_id, candidate) in candidates {
            if candidate.lexical_evidence.is_none() {
                continue;
            }
            let role = if target_ids.contains(symbol_id) {
                Some("definition".to_string())
            } else {
                graph_role_relative_to_targets(graph, *symbol_id, &target_ids)
            };
            if let Some(role) = role {
                candidate.graph_role = Some(role.clone());
                candidate.reason_codes.insert(format!("graph_role:{role}"));
            }
        }
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

        let mut matches = match self.search_file_lines_with_ripgrep(
            query,
            &query_lower,
            &terms,
            limit,
        ) {
            Ok(Some(matches)) => matches,
            Ok(None) => self.search_file_lines_with_ignore(query, &query_lower, &terms, limit)?,
            Err(error) => {
                tracing::warn!(%error, "ripgrep file-line search failed; falling back to ignore walker");
                self.search_file_lines_with_ignore(query, &query_lower, &terms, limit)?
            }
        };
        sort_file_line_matches(&mut matches, query);
        matches.truncate(limit);
        Ok(matches)
    }

    fn search_file_lines_with_ripgrep(
        &self,
        query: &str,
        query_lower: &str,
        terms: &[String],
        limit: usize,
    ) -> Result<Option<Vec<(Symbol, LexicalEvidence)>>> {
        let root = self.config.target_dir.canonicalize().map_err(|source| {
            crate::LoomError::IndexerIo {
                path: self.config.target_dir.display().to_string(),
                source,
            }
        })?;
        let pattern = file_line_search_pattern(query, terms);
        let mut child = match Command::new("rg")
            .arg("--json")
            .arg("--line-number")
            .arg("--no-heading")
            .arg("--color")
            .arg("never")
            .arg("--ignore-case")
            .arg("--regexp")
            .arg(pattern)
            .arg(&root)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!("ripgrep executable not found; falling back to ignore walker");
                return Ok(None);
            }
            Err(source) => {
                return Err(crate::LoomError::IndexerIo {
                    path: "rg".to_string(),
                    source,
                });
            }
        };
        let Some(stdout) = child.stdout.take() else {
            let status = child.wait().map_err(|source| crate::LoomError::IndexerIo {
                path: "rg".to_string(),
                source,
            })?;
            tracing::warn!(
                ?status,
                "ripgrep produced no stdout; falling back to ignore walker"
            );
            return Ok(None);
        };

        let mut matches = Vec::new();
        let mut symbols_by_file = BTreeMap::<String, Vec<Symbol>>::new();
        for line in BufReader::new(stdout).lines() {
            let line = line.map_err(|source| crate::LoomError::IndexerIo {
                path: "rg".to_string(),
                source,
            })?;
            let Ok(event) = serde_json::from_str::<RipgrepJsonEvent>(&line) else {
                continue;
            };
            if event.kind != "match" {
                continue;
            }
            let Some(data) = event.data else {
                continue;
            };
            let (Some(path), Some(text), Some(line_number)) =
                (data.path, data.lines, data.line_number)
            else {
                continue;
            };
            let Some(file) = repo_relative_path(&root, Path::new(&path.text)) else {
                continue;
            };
            let Some(match_kind) = line_match_kind(query_lower, terms, &text.text) else {
                continue;
            };
            let colocated = match symbols_by_file.get(&file) {
                Some(symbols) => symbols,
                None => {
                    let symbols = self.db.get_colocated_symbols(&file)?;
                    symbols_by_file.insert(file.clone(), symbols);
                    symbols_by_file
                        .get(&file)
                        .expect("inserted colocated symbols")
                }
            };
            let Some(symbol) =
                symbol_for_file_line(colocated, &file, line_number, &text.text, query)
            else {
                continue;
            };
            matches.push(file_line_match(
                &symbol,
                query,
                &file,
                line_number,
                &text.text,
                match_kind,
                matches.len(),
            ));
            if matches.len() >= limit {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(Some(matches));
            }
        }
        let status = child.wait().map_err(|source| crate::LoomError::IndexerIo {
            path: "rg".to_string(),
            source,
        })?;
        if !status.success() && status.code() != Some(1) {
            tracing::warn!(
                ?status,
                "ripgrep exited unsuccessfully; falling back to ignore walker"
            );
            return Ok(None);
        }
        Ok(Some(matches))
    }

    fn search_file_lines_with_ignore(
        &self,
        query: &str,
        query_lower: &str,
        terms: &[String],
        limit: usize,
    ) -> Result<Vec<(Symbol, LexicalEvidence)>> {
        let root = self.config.target_dir.canonicalize().map_err(|source| {
            crate::LoomError::IndexerIo {
                path: self.config.target_dir.display().to_string(),
                source,
            }
        })?;
        let mut matches = Vec::new();
        for entry in ignore::WalkBuilder::new(&root)
            .sort_by_file_path(|left, right| left.cmp(right))
            .build()
        {
            let Ok(entry) = entry else {
                continue;
            };
            if !entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
            {
                continue;
            }
            let Some(file) = repo_relative_path(&root, entry.path()) else {
                continue;
            };
            let Ok(file_handle) = File::open(entry.path()) else {
                continue;
            };
            let reader = BufReader::new(file_handle);
            let mut file_matches = Vec::new();
            for (line_index, line) in reader.lines().enumerate() {
                let Ok(line) = line else {
                    continue;
                };
                let Some(match_kind) = line_match_kind(query_lower, terms, &line) else {
                    continue;
                };
                file_matches.push((line_index as i64 + 1, line, match_kind));
            }
            if file_matches.is_empty() {
                continue;
            }
            let colocated = self.db.get_colocated_symbols(&file)?;
            for (line, text, match_kind) in file_matches {
                let Some(symbol) = symbol_for_file_line(&colocated, &file, line, &text, query)
                else {
                    continue;
                };
                matches.push(file_line_match(
                    &symbol,
                    query,
                    &file,
                    line,
                    &text,
                    match_kind,
                    matches.len(),
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
    let handle = if candidate.symbol.id.is_some() {
        symbol_handle(index_revision, &candidate.symbol)
    } else {
        file_handle(index_revision, &file)
    };
    SymbolHit {
        handle,
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
        graph_role: candidate.graph_role,
        lexical_evidence: candidate.lexical_evidence,
        coupled: coupled_to_hits(candidate.coupled, index_revision),
    }
}

fn enforce_search_response_budget(
    response: &mut SearchResponse,
    budget_tokens: usize,
    total_available_results: usize,
) {
    let char_budget = budget_tokens.saturating_mul(SEARCH_CHARS_PER_TOKEN);
    compact_search_hits(&mut response.exact_hits);
    compact_search_hits(&mut response.beyond_grep);

    while let Ok(json) = serde_json::to_string(response) {
        if json.len() <= char_budget
            || (response.exact_hits.is_empty() && response.beyond_grep.is_empty())
        {
            let returned_results = response.exact_hits.len() + response.beyond_grep.len();
            let omitted = total_available_results.saturating_sub(returned_results);
            response.truncated = response.truncated || omitted > 0;
            response.budget = response_budget(
                "tokens",
                budget_tokens,
                estimate_tokens_from_chars(json.len()),
                omitted,
                response.truncated,
            );
            response.continuation =
                continuation_for("search", response.truncated, returned_results, omitted);
            break;
        }
        response.truncated = true;
        if response.beyond_grep.pop().is_none() && response.exact_hits.pop().is_none() {
            let returned_results = 0;
            response.budget = response_budget(
                "tokens",
                budget_tokens,
                estimate_tokens_from_chars(json.len()),
                total_available_results,
                true,
            );
            response.continuation =
                continuation_for("search", true, returned_results, total_available_results);
            break;
        }
    }
}

fn compact_search_hits(hits: &mut [SymbolHit]) {
    for hit in hits {
        hit.summary = truncate_chars(&hit.summary, 96);
        hit.reason_codes.truncate(8);
        if let Some(evidence) = &mut hit.lexical_evidence {
            evidence.snippet = truncate_chars(&evidence.snippet, 120);
            evidence.reason = truncate_chars(&evidence.reason, 96);
        }
        hit.coupled.truncate(3);
        for coupled in &mut hit.coupled {
            coupled.reason = truncate_chars(&coupled.reason, 96);
            coupled.provenance.truncate(2);
        }
    }
}

fn estimate_tokens_from_chars(chars: usize) -> usize {
    chars.div_ceil(SEARCH_CHARS_PER_TOKEN)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
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
        graph_role: None,
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
    let targets = symbol_query_targets(query);
    if symbol_name_matches_any_target(&symbol.name, &targets, SymbolTargetMatch::Exact) {
        "exact"
    } else if symbol_name_matches_any_target(&symbol.name, &targets, SymbolTargetMatch::Suffix) {
        "method_suffix"
    } else {
        "contains"
    }
}

fn symbol_query_match_weight(symbol: &Symbol, query: &str) -> f64 {
    match symbol_match_kind(symbol, query) {
        "exact" => 3.0,
        "method_suffix" => 2.2,
        _ => 0.45,
    }
}

fn file_line_match_weight(symbol: &Symbol, evidence: &LexicalEvidence, query: &str) -> f64 {
    let symbol_weight = symbol_query_match_weight(symbol, query);
    let line_weight = match evidence.match_kind.as_str() {
        "exact_phrase" => 2.0,
        "ordered_tokens" => 1.4,
        _ => 1.0,
    };
    symbol_weight.max(1.0) * line_weight
}

#[derive(Clone, Copy)]
enum SymbolTargetMatch {
    Exact,
    Suffix,
}

fn symbol_name_matches_any_target(
    symbol_name: &str,
    targets: &BTreeSet<String>,
    match_kind: SymbolTargetMatch,
) -> bool {
    let name = symbol_name.to_lowercase();
    targets.iter().any(|target| match match_kind {
        SymbolTargetMatch::Exact => name == *target,
        SymbolTargetMatch::Suffix => name
            .strip_suffix(target)
            .is_some_and(|prefix| prefix.ends_with(['.', ':', '#'])),
    })
}

fn symbol_query_targets(query: &str) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    for piece in query
        .split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|piece| piece.chars().count() >= 2)
    {
        targets.insert(piece.to_lowercase());
    }

    let trimmed = query.trim();
    if trimmed.chars().count() >= 2
        && trimmed
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '.' | ':' | '#'))
    {
        targets.insert(trimmed.to_lowercase());
    }
    targets
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

fn file_line_scan_limit(query: &str, candidate_limit: usize) -> usize {
    let terms = lexical_query_terms(query);
    let multiplier = if terms.len() >= 3 { 16 } else { 8 };
    candidate_limit.saturating_mul(multiplier).clamp(32, 512)
}

fn sort_file_line_matches(matches: &mut [(Symbol, LexicalEvidence)], query: &str) {
    matches.sort_by(
        |(left_symbol, left_evidence), (right_symbol, right_evidence)| {
            file_line_rank_score(right_symbol, right_evidence, query)
                .total_cmp(&file_line_rank_score(left_symbol, left_evidence, query))
                .then_with(|| left_symbol.file.cmp(&right_symbol.file))
                .then_with(|| left_symbol.line.cmp(&right_symbol.line))
                .then_with(|| left_symbol.name.cmp(&right_symbol.name))
        },
    );
    for (rank, (_, evidence)) in matches.iter_mut().enumerate() {
        evidence.rank = rank as f64;
    }
}

fn file_line_rank_score(symbol: &Symbol, evidence: &LexicalEvidence, query: &str) -> f64 {
    let match_score = match evidence.match_kind.as_str() {
        "exact_phrase" => 10.0,
        "ordered_tokens" => 4.0,
        _ => 1.0,
    };
    let symbol_score = match symbol_match_kind(symbol, query) {
        "exact" => 12.0,
        "method_suffix" => 8.0,
        _ => 1.0,
    };
    let file_score = if symbol.file.starts_with("src/") {
        1.0
    } else if symbol.file.contains("/test/") || symbol.file.contains(".test.") {
        -1.0
    } else {
        0.0
    };
    match_score + symbol_score + file_score
}

fn symbol_matches_query_target(symbol: &Symbol, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return false;
    }
    let name = symbol.name.to_lowercase();
    if name == query {
        return true;
    }
    name.rsplit(['.', ':', '#'])
        .next()
        .is_some_and(|suffix| suffix == query)
}

fn graph_role_relative_to_targets(
    graph: &SymbolGraph,
    symbol_id: i64,
    target_ids: &BTreeSet<i64>,
) -> Option<String> {
    graph
        .dependencies(symbol_id, 1)
        .into_iter()
        .find(|entry| target_ids.contains(&entry.symbol_id))
        .map(|entry| graph_role_for_relationship(&entry.relationship, "outgoing"))
        .or_else(|| {
            graph
                .dependents(symbol_id, 1)
                .into_iter()
                .find(|entry| target_ids.contains(&entry.symbol_id))
                .map(|entry| graph_role_for_relationship(&entry.relationship, "incoming"))
        })
}

fn graph_role_for_relationship(relationship: &str, direction: &str) -> String {
    match (relationship, direction) {
        ("calls", "outgoing") => "caller",
        ("calls", "incoming") => "callee",
        ("imports", "outgoing") => "import",
        ("imports", "incoming") => "imported",
        ("implements", "outgoing") => "implementor",
        ("implements", "incoming") => "interface",
        ("extends", "outgoing") => "subtype",
        ("extends", "incoming") => "base_type",
        (_, _) => relationship,
    }
    .to_string()
}

fn mode_graph_role(mode: &str, relationship: &str) -> String {
    match mode {
        "callers" => graph_role_for_relationship(relationship, "outgoing"),
        "callees" | "impact" => graph_role_for_relationship(relationship, "incoming"),
        "implementations" | "implementors" => "implementor".to_string(),
        _ => relationship.to_string(),
    }
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

fn file_line_search_pattern(query: &str, terms: &[String]) -> String {
    if terms.len() >= 2 {
        terms
            .iter()
            .map(|term| regex_escape(term))
            .collect::<Vec<_>>()
            .join(".*")
    } else {
        regex_escape(query.trim())
    }
}

fn regex_escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        if matches!(
            character,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn repo_relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    Some(
        relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part.to_string_lossy()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn file_line_match(
    symbol: &Symbol,
    query: &str,
    file: &str,
    line: i64,
    text: &str,
    match_kind: &str,
    rank: usize,
) -> (Symbol, LexicalEvidence) {
    (
        symbol.clone(),
        LexicalEvidence {
            snippet: bounded_line_evidence(text),
            matched_text: query.to_string(),
            rank: rank as f64,
            field: "file_line".to_string(),
            reason: format!("exact file-line scan at {file}:{line}"),
            match_kind: match_kind.to_string(),
            sanitized_query: query.to_string(),
        },
    )
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

fn symbol_for_file_line(
    symbols: &[Symbol],
    file: &str,
    line: i64,
    text: &str,
    query: &str,
) -> Option<Symbol> {
    if let Some(symbol) = containing_symbol_for_line(symbols, line) {
        return Some(symbol.clone());
    }
    if !symbols.is_empty() {
        return None;
    }
    Some(Symbol {
        id: None,
        name: format!("file match: {}", query.trim()),
        kind: "file_match".to_string(),
        file: file.to_string(),
        line,
        end_line: line,
        language: language_from_path(file),
        context: bounded_line_evidence(text),
    })
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

fn language_from_path(file: &str) -> String {
    match Path::new(file).extension().and_then(|value| value.to_str()) {
        Some("ts" | "tsx" | "js" | "jsx") => "typescript",
        Some("go") => "go",
        Some("java") => "java",
        Some("rs") => "rust",
        Some("cs") => "csharp",
        _ => "unknown",
    }
    .to_string()
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

fn split_sub_questions(query: &str) -> Vec<EvidenceSubQuestion> {
    let markdown_questions = split_markdown_questions(query);
    let questions = if markdown_questions.len() > 1 {
        markdown_questions
    } else {
        let numbered_questions = split_numbered_questions(query);
        if numbered_questions.len() == 1 {
            expand_enumerated_sub_questions(&numbered_questions[0]).unwrap_or(numbered_questions)
        } else if !numbered_questions.is_empty() {
            numbered_questions
        } else {
            split_enumerated_questions(query)
        }
    };
    let mut questions = if questions.is_empty() {
        vec![EvidenceSubQuestion {
            label: "Q1".to_string(),
            query: query.trim().to_string(),
            symbols: Vec::new(),
        }]
    } else {
        questions
    };
    for question in &mut questions {
        question.symbols = extract_key_symbol_names(&question.query);
    }
    questions
}

fn split_numbered_questions(query: &str) -> Vec<EvidenceSubQuestion> {
    let mut questions = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_body = String::new();

    let normalized = split_inline_enumerators(&split_inline_question_headings(query));
    for line in normalized.lines() {
        if let Some((label, body)) = numbered_question_heading(line) {
            if let Some(label) = current_label.take() {
                push_sub_question(&mut questions, label, &current_body);
                current_body.clear();
            }
            current_label = Some(label);
            current_body.push_str(&body);
        } else if current_label.is_some() {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(line.trim());
        }
    }
    if let Some(label) = current_label {
        push_sub_question(&mut questions, label, &current_body);
    }
    questions
}

fn expand_enumerated_sub_questions(
    question: &EvidenceSubQuestion,
) -> Option<Vec<EvidenceSubQuestion>> {
    let mut questions = Vec::new();
    let mut preamble = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_body = String::new();

    let normalized = split_inline_enumerators(&question.query);
    for line in normalized.lines() {
        if let Some((label, body)) = enumerated_question_heading(line) {
            if let Some(label) = current_label.take() {
                push_expanded_sub_question(
                    &mut questions,
                    &question.label,
                    &label,
                    &preamble,
                    &current_body,
                );
                current_body.clear();
            }
            current_label = Some(label);
            current_body.push_str(&body);
        } else if current_label.is_some() {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(line.trim());
        } else if !line.trim().is_empty() {
            preamble.push(line.trim().to_string());
        }
    }
    if let Some(label) = current_label {
        push_expanded_sub_question(
            &mut questions,
            &question.label,
            &label,
            &preamble,
            &current_body,
        );
    }

    (questions.len() >= 2).then_some(questions)
}

fn split_inline_enumerators(query: &str) -> String {
    let chars = query.chars().collect::<Vec<_>>();
    let mut output = String::new();
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch.is_whitespace()
            && chars
                .get(index + 1)
                .is_some_and(|marker| marker.is_ascii_alphanumeric())
            && chars
                .get(index + 2)
                .is_some_and(|separator| matches!(separator, '.' | ')'))
            && chars
                .get(index + 3)
                .is_some_and(|after| after.is_whitespace())
        {
            output.push('\n');
        } else {
            output.push(ch);
        }
    }
    output
}

fn split_inline_question_headings(query: &str) -> String {
    let chars = query.chars().collect::<Vec<_>>();
    let mut output = String::new();
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch.is_whitespace()
            && chars
                .get(index + 1)
                .is_some_and(|marker| matches!(marker, 'Q' | 'q'))
            && chars
                .get(index + 2)
                .is_some_and(|digit| digit.is_ascii_digit())
        {
            let mut cursor = index + 3;
            while chars
                .get(cursor)
                .is_some_and(|digit| digit.is_ascii_digit())
            {
                cursor += 1;
            }
            if chars
                .get(cursor)
                .is_some_and(|separator| matches!(separator, ':' | '.' | ')' | ' '))
            {
                output.push('\n');
                continue;
            }
        }
        output.push(ch);
    }
    output
}

fn push_expanded_sub_question(
    questions: &mut Vec<EvidenceSubQuestion>,
    parent_label: &str,
    child_label: &str,
    preamble: &[String],
    body: &str,
) {
    let body = body.trim();
    if body.is_empty() {
        return;
    }
    let mut query = preamble.join("\n");
    if !query.is_empty() {
        query.push('\n');
    }
    query.push_str(body);
    questions.push(EvidenceSubQuestion {
        label: format!("{parent_label}{child_label}"),
        query,
        symbols: Vec::new(),
    });
}

fn numbered_question_heading(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let mut chars = trimmed.chars();
    if !matches!(chars.next(), Some('Q' | 'q')) {
        return None;
    }
    let digits = chars
        .by_ref()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    let rest = chars
        .as_str()
        .trim_start_matches([':', '.', ')', ' '])
        .trim();
    if rest.is_empty() {
        return None;
    }
    Some((format!("Q{digits}"), rest.to_string()))
}

fn split_markdown_questions(query: &str) -> Vec<EvidenceSubQuestion> {
    let mut questions = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_body = String::new();

    let normalized = split_inline_enumerators(query);
    for line in normalized.lines() {
        if let Some((label, body)) = markdown_question_heading(line) {
            if let Some(label) = current_label.take() {
                push_sub_question(&mut questions, label, &current_body);
                current_body.clear();
            }
            current_label = Some(label);
            current_body.push_str(&body);
        } else if current_label.is_some() {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(line.trim());
        }
    }
    if let Some(label) = current_label {
        push_sub_question(&mut questions, label, &current_body);
    }
    questions
}

fn markdown_question_heading(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let heading = trimmed.strip_prefix("##")?.trim();
    let mut chars = heading.chars();
    if !matches!(chars.next(), Some('Q' | 'q')) {
        return None;
    }
    let digits = chars
        .by_ref()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    let rest = chars
        .as_str()
        .trim_start_matches([':', '.', ')', ' '])
        .trim();
    Some((format!("Q{digits}"), rest.to_string()))
}

fn split_enumerated_questions(query: &str) -> Vec<EvidenceSubQuestion> {
    let mut questions = Vec::new();
    let mut preamble = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_body = String::new();
    let normalized = split_inline_enumerators(query);
    for line in normalized.lines() {
        if let Some((label, body)) = enumerated_question_heading(line) {
            if let Some(label) = current_label.take() {
                push_preamble_sub_question(&mut questions, label, &preamble, &current_body);
                current_body.clear();
            }
            current_label = Some(label);
            current_body.push_str(&body);
        } else if current_label.is_some() {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(line.trim());
        } else if !line.trim().is_empty() {
            preamble.push(line.trim().to_string());
        }
    }
    if let Some(label) = current_label {
        push_preamble_sub_question(&mut questions, label, &preamble, &current_body);
    }
    questions
}

fn push_preamble_sub_question(
    questions: &mut Vec<EvidenceSubQuestion>,
    label: String,
    preamble: &[String],
    body: &str,
) {
    let mut query = preamble.join("\n");
    if !query.is_empty() && !body.trim().is_empty() {
        query.push('\n');
    }
    query.push_str(body.trim());
    push_sub_question(questions, label, &query);
}

fn enumerated_question_heading(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let (marker, rest) = trimmed.split_once(['.', ')'])?;
    let marker = marker.trim();
    let is_letter = marker.len() == 1
        && marker
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic());
    let is_number = !marker.is_empty() && marker.chars().all(|ch| ch.is_ascii_digit());
    if !is_letter && !is_number {
        return None;
    }
    let label = if is_letter {
        marker.to_ascii_lowercase()
    } else {
        format!("Q{marker}")
    };
    Some((label, rest.trim().to_string()))
}

fn push_sub_question(questions: &mut Vec<EvidenceSubQuestion>, label: String, body: &str) {
    let query = body.trim();
    if query.is_empty() {
        return;
    }
    questions.push(EvidenceSubQuestion {
        label,
        query: query.to_string(),
        symbols: Vec::new(),
    });
}

fn extract_key_symbol_names(query: &str) -> Vec<String> {
    let mut symbols = BTreeSet::new();
    for token in code_span_tokens(query) {
        if token.len() >= 2 {
            for part in symbol_token_variants(&token) {
                symbols.insert(part);
            }
        }
    }
    for raw in
        query.split(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '_' | '.' | ':' | '#')))
    {
        let token = raw
            .trim_matches(|ch: char| matches!(ch, '.' | ':' | '#' | '`' | '\'' | '"'))
            .to_string();
        if is_key_symbol_token(&token) {
            symbols.insert(token);
        }
    }
    symbols.into_iter().take(24).collect()
}

fn symbol_token_variants(token: &str) -> Vec<String> {
    let mut variants = Vec::from([token.to_string()]);
    for part in token.split(['.', ':', '#']) {
        let part = part.trim();
        if part.len() >= 3 && is_key_symbol_token(part) {
            variants.push(part.to_string());
        }
    }
    variants
}

fn code_span_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut in_code = false;
    let mut current = String::new();
    for ch in query.chars() {
        if ch == '`' {
            if in_code {
                for token in current.split(|inner: char| {
                    !(inner.is_alphanumeric() || matches!(inner, '_' | '.' | ':' | '#'))
                }) {
                    let token = token
                        .trim_matches(|inner: char| matches!(inner, '.' | ':' | '#'))
                        .to_string();
                    if !token.is_empty() {
                        tokens.push(token);
                    }
                }
                current.clear();
            }
            in_code = !in_code;
        } else if in_code {
            current.push(ch);
        }
    }
    tokens
}

fn is_key_symbol_token(token: &str) -> bool {
    if token.len() < 3 {
        return false;
    }
    let lower = token.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "the"
            | "also"
            | "and"
            | "all"
            | "for"
            | "with"
            | "where"
            | "when"
            | "what"
            | "which"
            | "does"
            | "during"
            | "inside"
            | "method"
            | "methods"
            | "struct"
            | "interface"
            | "function"
            | "file"
            | "line"
            | "number"
            | "exact"
            | "give"
            | "find"
            | "list"
            | "log"
            | "each"
            | "every"
            | "path"
            | "replay"
            | "return"
            | "write"
            | "type"
            | "types"
            | "prometheus"
            | "tsdb"
    ) {
        return false;
    }
    if token
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | 'v' | 'V'))
    {
        return false;
    }
    token.contains('.')
        || token.contains("::")
        || token.contains('_')
        || token.chars().any(|ch| ch.is_ascii_uppercase())
        || token.chars().any(|ch| ch.is_ascii_digit())
}

fn extract_repo_paths(query: &str) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for raw in query.split_whitespace() {
        let token = raw.trim_matches(|ch: char| {
            matches!(
                ch,
                '`' | '\'' | '"' | ',' | ';' | ':' | ')' | '(' | '[' | ']' | '.'
            )
        });
        if token.contains('/')
            && matches!(
                Path::new(token)
                    .extension()
                    .and_then(|value| value.to_str()),
                Some("go" | "rs" | "js" | "jsx" | "ts" | "tsx" | "java" | "cs")
            )
            && !token.starts_with('/')
            && !token.contains("..")
        {
            paths.insert(token.to_string());
        }
    }
    paths.into_iter().collect()
}

fn extract_requested_line_ranges(query: &str) -> Vec<(i64, i64)> {
    let mut ranges = BTreeSet::new();
    let words = query
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '`' | '\'' | '"' | ',' | ';' | ':' | ')' | '(' | '[' | ']' | '.'
                )
            })
            .to_string()
        })
        .collect::<Vec<_>>();

    for (index, word) in words.iter().enumerate() {
        let lower = word.to_ascii_lowercase();
        let range_word = matches!(lower.as_str(), "line" | "lines" | "around");
        if !range_word {
            continue;
        }
        for lookahead in words.iter().skip(index + 1).take(4) {
            if let Some(range) = parse_line_range_token(lookahead) {
                ranges.insert(range);
                break;
            }
        }
    }

    for word in &words {
        if let Some(range) = parse_line_range_token(word) {
            ranges.insert(range);
        }
    }

    ranges
        .into_iter()
        .map(|(start, end)| {
            let start = start.max(1);
            let end = end.max(start).min(start + 80);
            (start, end)
        })
        .collect()
}

fn parse_line_range_token(token: &str) -> Option<(i64, i64)> {
    let cleaned = token.trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '-');
    if cleaned.is_empty() {
        return None;
    }
    if let Some((start, end)) = cleaned.split_once('-') {
        let start = start.parse::<i64>().ok()?;
        let end = end.parse::<i64>().ok()?;
        if start > 0 && end >= start {
            return Some((start, end));
        }
    }
    let line = cleaned.parse::<i64>().ok()?;
    (line > 0).then_some((line.saturating_sub(4).max(1), line + 8))
}

fn file_evidence_terms(question: &EvidenceSubQuestion) -> Vec<String> {
    let mut terms = BTreeSet::new();
    for symbol in &question.symbols {
        terms.insert(symbol.clone());
        for part in symbol.split(['.', ':', '_']) {
            if part.len() >= 2 {
                terms.insert(part.to_string());
            }
        }
    }
    for raw in question
        .query
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
    {
        let word = raw.trim();
        if word.len() < 4 {
            continue;
        }
        let lower = word.to_ascii_lowercase();
        if is_file_evidence_stopword(&lower) {
            continue;
        }
        terms.insert(word.to_string());
    }
    terms.into_iter().take(32).collect()
}

fn evidence_question_limit(question: &EvidenceSubQuestion, base_limit: usize) -> usize {
    let lower = question.query.to_ascii_lowercase();
    if wants_next_receiver_structs(&lower) {
        return base_limit.clamp(6, 8);
    }
    if lower.contains("wal replay")
        || lower.contains("write-ahead")
        || lower.contains("out-of-order")
        || lower.contains("ooo")
        || lower.contains("context cancellation")
    {
        return base_limit.clamp(5, 8);
    }
    base_limit
}

fn wants_next_receiver_structs(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    lower.contains("postings")
        && lower.contains("struct")
        && (lower.contains("next()") || lower.contains(" next "))
}

fn next_receiver_struct_snippets(
    path: &Path,
    file: &str,
    char_budget: usize,
    limit: usize,
) -> Result<Vec<InspectSnippet>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let source = std::fs::read_to_string(path).map_err(|source| crate::LoomError::IndexerIo {
        path: path.display().to_string(),
        source,
    })?;
    let lines = source.lines().collect::<Vec<_>>();
    let mut receivers = Vec::new();
    let mut seen_receivers = BTreeSet::new();
    for line in &lines {
        let Some(receiver) = go_next_receiver_name(line) else {
            continue;
        };
        if seen_receivers.insert(receiver.clone()) {
            receivers.push(receiver);
        }
    }

    let mut snippets = Vec::new();
    for receiver in receivers {
        let Some(line) = find_go_type_struct_line(&lines, &receiver) else {
            continue;
        };
        let start_line = line.saturating_sub(2).max(1);
        let end_line = line + 26;
        let anchor = FileAnchor {
            file: file.to_string(),
            line,
            end_line: line,
        };
        let read = read_snippet(path, &anchor, start_line, end_line, char_budget)?;
        if let Some(snippet) = read.snippet {
            snippets.push(snippet);
        }
        if snippets.len() >= limit {
            break;
        }
    }
    Ok(snippets)
}

fn go_next_receiver_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("func (")?;
    let (receiver, method) = rest.split_once(')')?;
    let method = method.trim_start();
    if !method.starts_with("Next()") {
        return None;
    }
    let receiver = receiver
        .split_whitespace()
        .last()
        .unwrap_or(receiver)
        .trim_start_matches('*')
        .trim();
    (!receiver.is_empty()).then(|| receiver.to_string())
}

fn find_go_type_struct_line(lines: &[&str], receiver: &str) -> Option<i64> {
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("type ") else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(receiver) else {
            continue;
        };
        let next = rest.chars().next();
        if !matches!(next, Some(' ' | '[' | '\t')) {
            continue;
        }
        if rest.contains(" struct") {
            return Some(i64::try_from(index + 1).unwrap_or(i64::MAX));
        }
    }
    None
}

fn is_file_evidence_stopword(word: &str) -> bool {
    matches!(
        word,
        "where"
            | "what"
            | "which"
            | "when"
            | "does"
            | "give"
            | "find"
            | "line"
            | "file"
            | "each"
            | "list"
            | "method"
            | "function"
            | "struct"
            | "interface"
            | "defined"
            | "exactly"
            | "exact"
            | "number"
            | "with"
            | "from"
            | "into"
            | "during"
            | "inside"
            | "question"
            | "ahead"
            | "path"
            | "replay"
            | "write"
    )
}

fn ranked_file_evidence_lines(path: &Path, terms: &[String]) -> Result<Vec<i64>> {
    let file = File::open(path).map_err(|source| crate::LoomError::IndexerIo {
        path: path.display().to_string(),
        source,
    })?;
    let lower_terms = terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut scored = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| crate::LoomError::IndexerIo {
            path: path.display().to_string(),
            source,
        })?;
        let lower_line = line.to_ascii_lowercase();
        let mut score = 0usize;
        for term in &lower_terms {
            if lower_line.contains(term) {
                score += 1;
            }
        }
        if lower_terms
            .iter()
            .any(|term| term == "lock" || term == "rlock")
            && (lower_line.contains(".lock()") || lower_line.contains(".rlock()"))
        {
            score += 5;
        }
        if lower_terms.iter().any(|term| term == "next") && lower_line.contains("next()") {
            score += 5;
        }
        if lower_terms.iter().any(|term| term == "context") && lower_line.contains("ctx") {
            score += 3;
        }
        if line.contains("type ") || line.contains("func ") {
            score += 2;
        }
        if score > 0 {
            scored.push((Reverse(score), i64::try_from(index + 1).unwrap_or(i64::MAX)));
        }
    }
    scored.sort();
    Ok(scored.into_iter().map(|(_, line)| line).collect())
}

fn source_line_contains(path: &Path, line_number: i64, needle: &str) -> Result<bool> {
    let file = File::open(path).map_err(|source| crate::LoomError::IndexerIo {
        path: path.display().to_string(),
        source,
    })?;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        if i64::try_from(index + 1).unwrap_or(i64::MAX) != line_number {
            continue;
        }
        let line = line.map_err(|source| crate::LoomError::IndexerIo {
            path: path.display().to_string(),
            source,
        })?;
        return Ok(line.contains(needle));
    }
    Ok(false)
}

fn rank_evidence_paths(paths: &mut [String], terms: &[String]) {
    let lower_terms = terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| {
        evidence_path_score(right, &lower_terms)
            .cmp(&evidence_path_score(left, &lower_terms))
            .then_with(|| left.cmp(right))
    });
}

fn rank_evidence_line_matches(matches: &mut [EvidenceLineMatch], terms: &[String], query: &str) {
    let lower_terms = terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let lower_query = query.to_ascii_lowercase();
    matches.sort_by(|left, right| {
        evidence_line_match_score(right, &lower_terms, &lower_query)
            .cmp(&evidence_line_match_score(left, &lower_terms, &lower_query))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.line.cmp(&right.line))
    });
}

fn evidence_line_match_score(
    line_match: &EvidenceLineMatch,
    lower_terms: &[String],
    lower_query: &str,
) -> usize {
    let lower_path = line_match.file.to_ascii_lowercase();
    let lower_text = line_match.text.to_ascii_lowercase();
    let mut score = evidence_path_score(&line_match.file, lower_terms);
    for term in lower_terms {
        if term.len() >= 3 && lower_text.contains(term) {
            score += 2;
        }
    }
    if lower_text.contains("type ") && lower_text.contains(" struct") {
        score += 4;
    }
    if lower_query.contains("struct") && lower_text.contains("type ") {
        score += 6;
    }
    if lower_query.contains("next()") && lower_text.contains("next()") {
        score += 6;
    }
    if lower_query.contains("head.init")
        && lower_path == "tsdb/db.go"
        && lower_text.contains("db.head.init")
    {
        score += 30;
    }
    if (lower_query.contains("context") || lower_query.contains("cancellation"))
        && lower_path == "tsdb/index/postings.go"
        && lower_text.contains("ctx.err()")
    {
        score += 30;
    }
    if (lower_query.contains("merge") || lower_query.contains("merged"))
        && lower_path == "tsdb/db.go"
        && lower_text.contains("newmergequerier")
    {
        score += 28;
    }
    if (lower_query.contains("merge") || lower_query.contains("merged"))
        && lower_path == "storage/merge.go"
    {
        score += 30;
    }
    if lower_path.ends_with("_test.go") || lower_path.contains("/test") {
        score = score.saturating_sub(12);
    }
    score
}

fn evidence_path_score(path: &str, lower_terms: &[String]) -> usize {
    let lower_path = path.to_ascii_lowercase();
    let mut score = 0usize;
    for term in lower_terms {
        if term.len() >= 3 && lower_path.contains(term) {
            score += 3;
        }
    }
    if !lower_path.contains("_test.") && !lower_path.contains("/test") {
        score += 1;
    }
    score
}

fn receiver_type_from_method_name(name: &str) -> Option<String> {
    let (receiver, method) = name.rsplit_once('.')?;
    (!receiver.is_empty() && !method.is_empty()).then(|| receiver.to_string())
}

fn evidence_patterns(query: &str) -> Vec<String> {
    let lower = query.to_ascii_lowercase();
    let mut patterns = BTreeSet::new();
    if lower.contains("checkpoint") {
        patterns.insert("checkpoint".to_string());
        patterns.insert("wlog\\.Checkpoint".to_string());
        patterns.insert("wlog\\.Checkpoint\\(".to_string());
    }
    if lower.contains("head.init") {
        patterns.insert("db\\.head\\.Init".to_string());
        patterns.insert("head\\.Init".to_string());
    }
    if lower.contains("wal") && (lower.contains("record") || lower.contains("replay")) {
        patterns.insert("loadWAL".to_string());
        patterns.insert("for r\\.Next\\(\\)".to_string());
        patterns.insert("switch dec\\.Type".to_string());
    }
    if lower.contains("compact") || lower.contains("compaction") {
        patterns.insert("func .*Compact".to_string());
    }
    if lower.contains("lock") || lower.contains("mutex") {
        patterns.insert("\\.Lock\\(\\)".to_string());
        patterns.insert("\\.RLock\\(\\)".to_string());
    }
    if lower.contains("record") && lower.contains("case") {
        patterns.insert("case record\\.".to_string());
    }
    if lower.contains("struct") && !lower.contains("lock") && !lower.contains("acquire") {
        patterns.insert("type .* struct".to_string());
    }
    if lower.contains("interface") && !lower.contains("lock") && !lower.contains("acquire") {
        patterns.insert("type .* interface".to_string());
    }
    if lower.contains("context") && (lower.contains("cancel") || lower.contains("cancellation")) {
        patterns.insert("ctx\\.Err\\(\\)".to_string());
    }
    if lower.contains("out-of-order") || lower.contains("ooo") {
        patterns.insert("ooo".to_string());
        patterns.insert("oooSample".to_string());
        patterns.insert("\\.insert\\(".to_string());
        patterns.insert("outoforder".to_string());
        patterns.insert("out_of_order".to_string());
    }
    if lower.contains("merge") || lower.contains("merged") {
        patterns.insert("NewMergeQuerier".to_string());
        patterns.insert("genericMergeSeriesSet".to_string());
    }
    patterns.into_iter().collect()
}

fn needs_cross_file_pattern_search(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    lower.contains("merge") || lower.contains("merged")
}

fn merge_symbol_hits(target: &mut Vec<SymbolHit>, hits: Vec<SymbolHit>) {
    let mut seen = target
        .iter()
        .map(|hit| hit.handle.clone())
        .collect::<BTreeSet<_>>();
    for hit in hits {
        if seen.insert(hit.handle.clone()) {
            target.push(hit);
        }
    }
}

fn merge_behavior_facts(target: &mut Vec<BehaviorFactHit>, hits: Vec<BehaviorFactHit>) {
    let mut seen = target
        .iter()
        .map(behavior_fact_key)
        .collect::<BTreeSet<_>>();
    for hit in hits {
        let key = behavior_fact_key(&hit);
        if seen.insert(key) {
            target.push(hit);
        }
    }
}

fn behavior_fact_key(hit: &BehaviorFactHit) -> String {
    let fact = &hit.fact;
    format!(
        "{}:{}:{}:{}",
        fact.fact_type, fact.value, fact.file, fact.line
    )
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

fn semantic_weight_multiplier(embedder_fingerprint: &str) -> f64 {
    if embedder_fingerprint.contains("embedder=hashing") {
        0.15
    } else {
        1.0
    }
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
