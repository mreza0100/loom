use crate::{
    embedder::{build_symbol_text, Embedder},
    graph::SymbolGraph,
    models::{
        decode_file_handle_path, file_handle, response_budget, response_envelope, symbol_handle,
        BehaviorFactHit, CoupledHit, CoupledSymbol, EvidenceCoverageItem, EvidencePackResponse,
        FileAnchor, FileRoleCard, ImpactResponse, InspectPage, InspectResponse, InspectSnippet,
        LexicalEvidence, NeighborhoodResponse, RelatedResponse, SearchResponse, Symbol, SymbolHit,
        SymbolQuery, EVIDENCE_PACK_CONTRACT, IMPACT_CONTRACT, INSPECT_CONTRACT,
        NEIGHBORHOOD_CONTRACT, RELATED_CONTRACT, SEARCH_CONTRACT,
    },
    search::scoring::{compute_evolutionary, compute_semantic, compute_structural, fuse_signals},
    store::LoomDb,
    LoomConfig, Result,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const RRF_K: f64 = 60.0;
const MAX_STRUCTURAL_RESULTS: usize = 30;
const MAX_SUMMARY_CHARS: usize = 160;

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
    lexical_evidence: Option<LexicalEvidence>,
    reason_codes: BTreeSet<String>,
    coupled: Vec<CoupledSymbol>,
}

impl Candidate {
    fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            score: 0.0,
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
                exact_hits: Vec::new(),
                beyond_grep: Vec::new(),
            });
        }

        let candidate_limit = if kind.is_some() {
            limit.saturating_mul(10)
        } else {
            limit.saturating_mul(3)
        };
        let fts_results = self.db.search_fts_with_evidence(query, candidate_limit)?;
        let fact_results = self.db.search_behavior_facts(query, candidate_limit)?;
        let embedding = self.embedder.embed_single(query)?;
        let vec_results = self.db.search_vectors(&embedding, candidate_limit)?;

        let mut candidates = BTreeMap::<i64, Candidate>::new();
        let mut lexical_seed_ids = Vec::new();
        for (rank, result) in fts_results.into_iter().enumerate() {
            let Some(symbol_id) = result.symbol.id else {
                continue;
            };
            lexical_seed_ids.push(symbol_id);
            let candidate = candidates
                .entry(symbol_id)
                .or_insert_with(|| Candidate::new(result.symbol.clone()));
            candidate.score += rrf_score(rank) * kind_boost(&candidate.symbol.kind);
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
            candidate.score += rrf_score(rank) * kind_boost(&candidate.symbol.kind);
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
            candidate.score += rrf_score(rank);
            candidate.score += rrf_score(rank) * (kind_boost(&candidate.symbol.kind) - 1.0);
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
            }
        }

        let mut hits = Vec::new();
        for mut candidate in candidates.into_values() {
            if kind.is_some_and(|expected| candidate.symbol.kind != expected) {
                continue;
            }
            let mut coupled = self.find_coupled(&candidate.symbol)?;
            coupled.truncate(self.config.top_coupled);
            candidate.coupled = coupled;
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
        let truncated = exact_hits.len() > limit || beyond_grep.len() > limit;
        exact_hits.truncate(limit);
        beyond_grep.truncate(limit);
        let returned = exact_hits.len() + beyond_grep.len();

        let envelope = response_envelope(SEARCH_CONTRACT, index_revision, limit, truncated, true);
        Ok(SearchResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget(
                "results",
                limit,
                returned,
                total_before_truncate.saturating_sub(returned),
                truncated,
            ),
            exact_hits,
            beyond_grep,
        })
    }

    pub fn related(
        &self,
        symbol: &str,
        file: Option<&str>,
        kind: Option<&str>,
    ) -> Result<RelatedResponse> {
        let index_revision = self.db.index_revision()?;
        let Some(target) = self
            .db
            .get_symbol_by_name_fuzzy(symbol, file)?
            .into_iter()
            .next()
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
                results: Vec::new(),
            });
        };
        let mut coupled = self.find_coupled(&target)?;
        if let Some(kind) = kind {
            coupled.retain(|entry| entry.symbol.kind == kind);
        }
        let results = coupled_to_hits(coupled, &index_revision);
        let envelope =
            response_envelope(RELATED_CONTRACT, index_revision, results.len(), false, true);
        Ok(RelatedResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget("results", results.len(), results.len(), 0, false),
            query: symbol_query(symbol, file, kind),
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
        let Some(target) = self
            .db
            .get_symbol_by_name_fuzzy(symbol, file)?
            .into_iter()
            .next()
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
                results: Vec::new(),
            });
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
        let results = coupled_to_hits(impact, &index_revision);
        let envelope =
            response_envelope(IMPACT_CONTRACT, index_revision, results.len(), false, true);
        Ok(ImpactResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget("results", results.len(), results.len(), 0, false),
            query: symbol_query(symbol, file, kind),
            results,
        })
    }

    pub fn neighborhood(&self, file: &str, line: i64) -> Result<NeighborhoodResponse> {
        let index_revision = self.db.index_revision()?;
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
            reason_codes: vec!["anchor".to_string()],
            lexical_evidence: None,
            coupled: coupled_to_hits(coupled, &index_revision),
        };
        let envelope = response_envelope(
            NEIGHBORHOOD_CONTRACT,
            index_revision,
            coupled_hits.len(),
            false,
            true,
        );
        Ok(NeighborhoodResponse {
            contract: envelope.contract,
            version: envelope.version,
            index_revision: envelope.index_revision,
            limit: envelope.limit,
            truncated: envelope.truncated,
            inspect_required: envelope.inspect_required,
            budget: response_budget("results", coupled_hits.len(), coupled_hits.len(), 0, false),
            file: file.to_string(),
            line,
            anchor: Some(anchor_hit),
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
        let line_budget = line_budget.max(1);
        let char_budget = char_budget.max(1);
        let parsed = parse_handle(handle)?;
        if parsed.index_revision != index_revision {
            return Ok(stale_inspect_response(
                handle,
                &parsed.kind,
                &index_revision,
                line_budget,
                format!(
                    "stale handle for {}; rerun search and inspect the fresh handle",
                    parsed.index_revision
                ),
            ));
        }

        match parsed.target {
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
            HandleTarget::File(file) => self.inspect_file(
                handle,
                &file,
                &index_revision,
                line_budget,
                char_budget,
                line_offset,
            ),
        }
    }

    pub fn evidence_pack(&self, query: &str, budget_tokens: usize) -> Result<EvidencePackResponse> {
        if budget_tokens == 0 {
            return Err(crate::LoomError::InvalidInput(
                "budget_tokens must be greater than zero".to_string(),
            ));
        }
        let index_revision = self.db.index_revision()?;
        let result_limit = (budget_tokens / 120).clamp(2, 8);
        let mut search = self.search(query, result_limit, None)?;
        let behavior_facts = self.db.search_behavior_facts(query, result_limit)?;
        let role_cards = self.role_cards_for_evidence(&search, &behavior_facts)?;
        let char_budget = budget_tokens.saturating_mul(4).clamp(240, 12_000);
        let mut selected = Vec::new();
        selected.extend(
            search
                .exact_hits
                .iter()
                .take(2)
                .map(|hit| hit.handle.clone()),
        );
        selected.extend(
            search
                .beyond_grep
                .iter()
                .take(3)
                .map(|hit| hit.handle.clone()),
        );
        selected.sort();
        selected.dedup();

        let per_snippet_budget = if selected.is_empty() {
            char_budget
        } else {
            (char_budget / selected.len()).clamp(160, 1_600)
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
        if inspected_snippets.is_empty() {
            omitted.push("no source snippets were inspected for this query".to_string());
        }

        let missing_concepts = missing_concepts(query, &search);
        let coverage_checklist =
            evidence_coverage(&search, &inspected_snippets, &behavior_facts, &role_cards);
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
            budget_tokens,
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
            inspect_required: envelope.inspect_required,
            budget: response_budget(
                "tokens",
                budget_tokens,
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
        self.db.get_role_cards_for_files(&files)
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
                candidate.score +=
                    compute_structural(&entry.relationship, entry.confidence, entry.depth);
                candidate
                    .reason_codes
                    .insert(format!("graph:{}", entry.relationship));
            }
        }
        Ok(())
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
        reason_codes: candidate.reason_codes.into_iter().collect(),
        lexical_evidence: candidate.lexical_evidence,
        coupled: coupled_to_hits(candidate.coupled, index_revision),
    }
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
            }
        })
        .collect()
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
