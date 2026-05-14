use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONTRACT_VERSION: u32 = 1;
pub const SEARCH_CONTRACT: &str = "loom.search.response";
pub const SYMBOLS_CONTRACT: &str = "loom.symbols.response";
pub const RELATED_CONTRACT: &str = "loom.related.response";
pub const IMPACT_CONTRACT: &str = "loom.impact.response";
pub const NEIGHBORHOOD_CONTRACT: &str = "loom.neighborhood.response";
pub const INSPECT_CONTRACT: &str = "loom.inspect.response";
pub const EVIDENCE_PACK_CONTRACT: &str = "loom.evidence_pack.response";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: i64,
    pub end_line: i64,
    pub language: String,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAnchor {
    pub file: String,
    pub line: i64,
    pub end_line: i64,
}

impl FileAnchor {
    #[must_use]
    pub fn from_symbol(symbol: &Symbol) -> Self {
        Self {
            file: symbol.file.clone(),
            line: symbol.line,
            end_line: symbol.end_line,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedEdge {
    pub source_name: String,
    pub target_name: String,
    pub relationship: String,
    pub target_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedBehaviorFact {
    pub fact_type: String,
    pub value: String,
    pub line: i64,
    pub end_line: i64,
    pub enclosing_symbol_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BehaviorFact {
    pub id: Option<i64>,
    pub fact_type: String,
    pub value: String,
    pub file: String,
    pub line: i64,
    pub end_line: i64,
    pub enclosing_symbol_id: Option<i64>,
    pub enclosing_symbol_name: Option<String>,
    pub occurrence_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BehaviorFactHit {
    pub fact: BehaviorFact,
    pub lexical_evidence: LexicalEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceFactHit {
    pub handle: String,
    pub file_handle: String,
    pub name: String,
    pub kind: String,
    pub anchor: FileAnchor,
    pub summary: String,
    pub fact: BehaviorFact,
    pub lexical_evidence: LexicalEvidence,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedCallsite {
    pub line: i64,
    pub end_line: i64,
    pub callee: String,
    pub receiver: Option<String>,
    pub unresolved_target: String,
    pub argument_summaries: Vec<String>,
    pub imported_aliases: Vec<String>,
    pub enclosing_symbol_name: Option<String>,
    pub confidence: f64,
    pub generic: bool,
    pub downweighted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Callsite {
    pub id: Option<i64>,
    pub file: String,
    pub line: i64,
    pub end_line: i64,
    pub callee: String,
    pub receiver: Option<String>,
    pub unresolved_target: String,
    pub resolved_target_id: Option<i64>,
    pub argument_summaries: Vec<String>,
    pub imported_aliases: Vec<String>,
    pub enclosing_symbol_id: Option<i64>,
    pub enclosing_symbol_name: Option<String>,
    pub confidence: f64,
    pub generic: bool,
    pub downweighted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedAlias {
    pub line: i64,
    pub end_line: i64,
    pub local_name: String,
    pub imported_name: String,
    pub source: String,
    pub alias_kind: String,
    pub enclosing_symbol_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasRecord {
    pub id: Option<i64>,
    pub file: String,
    pub line: i64,
    pub end_line: i64,
    pub local_name: String,
    pub imported_name: String,
    pub source: String,
    pub alias_kind: String,
    pub enclosing_symbol_id: Option<i64>,
    pub enclosing_symbol_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRoleCard {
    pub file: String,
    pub content_hash: String,
    pub primary_responsibility: String,
    pub exported_symbols: Vec<String>,
    pub imported_dependencies: Vec<String>,
    pub behavior_facts: Vec<String>,
    pub centrality: f64,
    pub tests_touching: Vec<String>,
    pub top_related_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: Option<i64>,
    pub source_id: i64,
    pub target_id: Option<i64>,
    pub target_name: String,
    pub target_file: Option<String>,
    pub relationship: String,
    pub confidence: f64,
    pub original_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileState {
    pub path: String,
    pub content_hash: String,
    pub last_indexed: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CouplingScore {
    pub structural: f64,
    pub semantic: f64,
    pub evolutionary: f64,
    pub combined: f64,
}

impl CouplingScore {
    #[must_use]
    pub fn breakdown(&self) -> String {
        let mut parts = vec![
            format!("structural={:.2}", self.structural),
            format!("semantic={:.2}", self.semantic),
        ];
        if self.evolutionary > 0.0 {
            parts.push(format!("evolutionary={:.2}", self.evolutionary));
        }
        parts.join(" + ")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphProvenance {
    pub relationship: String,
    pub direction: String,
    pub depth: usize,
    pub confidence: f64,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoupledSymbol {
    pub symbol: Symbol,
    pub score: f64,
    pub reason: String,
    pub provenance: Vec<GraphProvenance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub symbol: Symbol,
    pub score: f64,
    pub coupled: Vec<CoupledSymbol>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LexicalEvidence {
    pub snippet: String,
    pub matched_text: String,
    pub rank: f64,
    pub field: String,
    pub reason: String,
    pub match_kind: String,
    pub sanitized_query: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FtsSearchResult {
    pub symbol: Symbol,
    pub evidence: LexicalEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoupledHit {
    pub handle: String,
    pub file_handle: String,
    pub rank: usize,
    pub name: String,
    pub kind: String,
    pub language: String,
    pub anchor: FileAnchor,
    pub summary: String,
    #[serde(skip)]
    pub symbol: Symbol,
    pub score: f64,
    pub reason: String,
    pub reason_codes: Vec<String>,
    pub provenance: Vec<GraphProvenance>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SignalScores {
    pub lexical: f64,
    pub symbol: f64,
    pub semantic: f64,
    pub graph: f64,
    pub behavior: f64,
    pub total: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolHit {
    pub handle: String,
    pub file_handle: String,
    pub rank: usize,
    pub name: String,
    pub kind: String,
    pub language: String,
    pub anchor: FileAnchor,
    pub summary: String,
    #[serde(skip)]
    pub symbol: Symbol,
    pub score: f64,
    pub signal_scores: SignalScores,
    pub reason_codes: Vec<String>,
    pub lexical_evidence: Option<LexicalEvidence>,
    pub coupled: Vec<CoupledHit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextToolSuggestion {
    pub tool: String,
    pub reason: String,
    pub args: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Continuation {
    pub cursor: String,
    pub omitted: usize,
    pub next_request_hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryIntent {
    pub intent: String,
    pub confidence: f64,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub contract: String,
    pub version: u32,
    pub index_revision: String,
    pub limit: usize,
    pub truncated: bool,
    pub inspect_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseBudget {
    pub unit: String,
    pub requested: usize,
    pub returned: usize,
    pub omitted: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub contract: String,
    pub version: u32,
    pub index_revision: String,
    pub limit: usize,
    pub truncated: bool,
    pub inspect_required: bool,
    pub budget: ResponseBudget,
    pub continuation: Option<Continuation>,
    pub next_tool_suggestions: Vec<NextToolSuggestion>,
    pub query_intent: QueryIntent,
    pub exact_hits: Vec<SymbolHit>,
    pub beyond_grep: Vec<SymbolHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolListResponse {
    pub contract: String,
    pub version: u32,
    pub index_revision: String,
    pub limit: usize,
    pub truncated: bool,
    pub inspect_required: bool,
    pub budget: ResponseBudget,
    pub query: String,
    pub file_prefix: Option<String>,
    pub kind: Option<String>,
    pub continuation: Option<Continuation>,
    pub next_tool_suggestions: Vec<NextToolSuggestion>,
    pub results: Vec<SymbolHit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolQuery {
    pub symbol: String,
    pub file: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelatedResponse {
    pub contract: String,
    pub version: u32,
    pub index_revision: String,
    pub limit: usize,
    pub truncated: bool,
    pub inspect_required: bool,
    pub budget: ResponseBudget,
    pub query: SymbolQuery,
    pub continuation: Option<Continuation>,
    pub next_tool_suggestions: Vec<NextToolSuggestion>,
    pub results: Vec<CoupledHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImpactResponse {
    pub contract: String,
    pub version: u32,
    pub index_revision: String,
    pub limit: usize,
    pub truncated: bool,
    pub inspect_required: bool,
    pub budget: ResponseBudget,
    pub query: SymbolQuery,
    pub continuation: Option<Continuation>,
    pub next_tool_suggestions: Vec<NextToolSuggestion>,
    pub results: Vec<CoupledHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeighborhoodResponse {
    pub contract: String,
    pub version: u32,
    pub index_revision: String,
    pub limit: usize,
    pub truncated: bool,
    pub inspect_required: bool,
    pub budget: ResponseBudget,
    pub file: String,
    pub line: i64,
    pub anchor: Option<SymbolHit>,
    pub continuation: Option<Continuation>,
    pub next_tool_suggestions: Vec<NextToolSuggestion>,
    pub coupled: Vec<CoupledHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InspectSnippet {
    pub anchor: FileAnchor,
    pub start_line: i64,
    pub end_line: i64,
    pub text: String,
    pub chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectPage {
    pub line_offset: usize,
    pub next_line_offset: Option<usize>,
    pub refused: bool,
    pub refusal_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InspectResponse {
    pub contract: String,
    pub version: u32,
    pub index_revision: String,
    pub limit: usize,
    pub truncated: bool,
    pub inspect_required: bool,
    pub budget: ResponseBudget,
    pub handle: String,
    pub handle_kind: String,
    pub stale: bool,
    pub error: Option<String>,
    pub anchor: Option<FileAnchor>,
    pub snippet: Option<InspectSnippet>,
    pub page: InspectPage,
    pub display_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceCoverageItem {
    pub item: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidencePackResponse {
    pub contract: String,
    pub version: u32,
    pub index_revision: String,
    pub limit: usize,
    pub truncated: bool,
    pub inspect_required: bool,
    pub budget: ResponseBudget,
    pub query: String,
    pub exact_hits: Vec<SymbolHit>,
    pub beyond_grep: Vec<SymbolHit>,
    pub behavior_facts: Vec<EvidenceFactHit>,
    pub role_cards: Vec<FileRoleCard>,
    pub inspected_snippets: Vec<InspectSnippet>,
    pub coverage_checklist: Vec<EvidenceCoverageItem>,
    pub omitted: Vec<String>,
    pub missing_concepts: Vec<String>,
    pub next_tool_suggestions: Vec<NextToolSuggestion>,
    pub display_text: String,
}

#[must_use]
pub fn response_envelope(
    contract: &str,
    index_revision: String,
    limit: usize,
    truncated: bool,
    inspect_required: bool,
) -> ResponseEnvelope {
    ResponseEnvelope {
        contract: contract.to_string(),
        version: CONTRACT_VERSION,
        index_revision,
        limit,
        truncated,
        inspect_required,
    }
}

#[must_use]
pub fn response_budget(
    unit: &str,
    requested: usize,
    returned: usize,
    omitted: usize,
    truncated: bool,
) -> ResponseBudget {
    ResponseBudget {
        unit: unit.to_string(),
        requested,
        returned,
        omitted,
        truncated,
    }
}

#[must_use]
pub fn symbol_handle(index_revision: &str, symbol: &Symbol) -> String {
    if let Some(id) = symbol.id {
        return format!("symbol:{index_revision}:{id}");
    }
    format!(
        "symbol:{index_revision}:unindexed:{}:{}:{}",
        stable_handle_part(&symbol.file),
        symbol.line,
        stable_handle_part(&symbol.name)
    )
}

#[must_use]
pub fn file_handle(index_revision: &str, file: &str) -> String {
    format!("file:{index_revision}:{}", hex_encode(file.as_bytes()))
}

#[must_use]
pub fn behavior_fact_handle(index_revision: &str, fact: &BehaviorFact) -> String {
    if let Some(id) = fact.id {
        return format!("fact:{index_revision}:{id}");
    }
    format!(
        "fact:{index_revision}:unindexed:{}:{}:{}",
        stable_handle_part(&fact.file),
        fact.line,
        stable_handle_part(&fact.value)
    )
}

#[must_use]
pub fn callsite_handle(index_revision: &str, callsite: &Callsite) -> String {
    if let Some(id) = callsite.id {
        return format!("callsite:{index_revision}:{id}");
    }
    format!(
        "callsite:{index_revision}:unindexed:{}:{}:{}",
        stable_handle_part(&callsite.file),
        callsite.line,
        stable_handle_part(&callsite.unresolved_target)
    )
}

#[must_use]
pub fn decode_file_handle_path(encoded: &str) -> Option<String> {
    let bytes = hex_decode(encoded)?;
    String::from_utf8(bytes).ok()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|chunk| {
            let high = hex_value(chunk[0])?;
            let low = hex_value(chunk[1])?;
            Some((high << 4) | low)
        })
        .collect()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn stable_handle_part(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '.' | '/' => character,
            _ => '_',
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreStats {
    pub symbols: i64,
    pub edges: i64,
    pub resolved_edges: i64,
    pub unresolved_edges: i64,
    pub files: i64,
    pub vectors: i64,
    pub behavior_facts: i64,
    pub callsites: i64,
    pub resolved_callsites: i64,
    pub unresolved_callsites: i64,
    pub aliases: i64,
    pub role_cards: i64,
    pub last_indexed: Option<String>,
    pub stale_files: i64,
    pub cochange_pairs: i64,
}

impl StoreStats {
    #[must_use]
    pub fn as_map(&self) -> BTreeMap<String, Option<String>> {
        BTreeMap::from([
            ("symbols".to_string(), Some(self.symbols.to_string())),
            ("edges".to_string(), Some(self.edges.to_string())),
            (
                "resolved_edges".to_string(),
                Some(self.resolved_edges.to_string()),
            ),
            (
                "unresolved_edges".to_string(),
                Some(self.unresolved_edges.to_string()),
            ),
            ("files".to_string(), Some(self.files.to_string())),
            ("vectors".to_string(), Some(self.vectors.to_string())),
            (
                "behavior_facts".to_string(),
                Some(self.behavior_facts.to_string()),
            ),
            ("callsites".to_string(), Some(self.callsites.to_string())),
            (
                "resolved_callsites".to_string(),
                Some(self.resolved_callsites.to_string()),
            ),
            (
                "unresolved_callsites".to_string(),
                Some(self.unresolved_callsites.to_string()),
            ),
            ("aliases".to_string(), Some(self.aliases.to_string())),
            ("role_cards".to_string(), Some(self.role_cards.to_string())),
            ("last_indexed".to_string(), self.last_indexed.clone()),
            (
                "stale_files".to_string(),
                Some(self.stale_files.to_string()),
            ),
            (
                "cochange_pairs".to_string(),
                Some(self.cochange_pairs.to_string()),
            ),
        ])
    }
}
