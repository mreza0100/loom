use loom_core::{
    embedder::DefaultEmbedder,
    graph::SymbolGraph,
    indexer::IndexPipeline,
    models::{
        EvidencePackResponse, ImpactResponse, InspectResponse, NeighborhoodResponse,
        RelatedResponse, SearchResponse, StoreStats, SymbolListResponse,
    },
    store::LoomDb,
    watcher::{FnChangeHandler, LoomWatcher},
    IndexResult, LoomConfig, SearchEngine,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Content, ErrorData, Implementation, InitializeResult, ServerCapabilities},
    tool, tool_handler, tool_router, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use tracing::{error, warn};

const MAX_QUERY_BYTES: usize = 4_096;
const MAX_SYMBOL_BYTES: usize = 512;
const MAX_FILE_BYTES: usize = 2_048;
const MAX_HANDLE_BYTES: usize = 4_096;
const MAX_INSPECT_LINES: usize = 120;
const MAX_INSPECT_CHARS: usize = 16_000;
const MAX_INSPECT_LINE_OFFSET: usize = 1_000_000;
const MAX_EVIDENCE_BUDGET_TOKENS: usize = 8_000;
const MCP_SEARCH_LIMIT: usize = 100;
const MCP_SYMBOL_LIMIT: usize = 256;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    #[schemars(
        description = "Natural language or code terms. Use Loom search before grep when you do not know the exact file or symbol; use symbols instead for exact same-name enumeration."
    )]
    pub query: String,
    #[schemars(
        description = "Optional exact symbol kind filter, such as function, class, method, variable, interface, or struct."
    )]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolListRequest {
    #[schemars(
        description = "Exact symbol or method suffix to enumerate, such as execute or Engine.resolveDescriptor. Use this before grep for same-name symbols."
    )]
    pub query: String,
    #[schemars(
        description = "Optional repo-relative file prefix, such as sources/commands, to bound enumeration."
    )]
    pub file_prefix: Option<String>,
    #[schemars(
        description = "Optional exact symbol kind filter, such as function, class, method, variable, interface, or struct."
    )]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolRequest {
    #[schemars(
        description = "Symbol name to inspect. Prefer a name returned by Loom search or neighborhood."
    )]
    pub symbol: String,
    #[schemars(
        description = "Optional repo-relative indexed file path used to disambiguate duplicate symbol names."
    )]
    pub file: Option<String>,
    #[schemars(
        description = "Optional exact symbol kind filter, such as function, class, method, variable, interface, or struct."
    )]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NeighborhoodRequest {
    #[schemars(
        description = "Repo-relative indexed file path. Use this when you already have a file and line."
    )]
    pub file: String,
    #[schemars(
        description = "One-based line number. Loom anchors to the symbol covering this line, or the nearest symbol."
    )]
    pub line: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectRequest {
    #[schemars(
        description = "Handle from search, related, impact, neighborhood, or evidence_pack. Use inspect only for selected handles."
    )]
    pub handle: String,
    #[schemars(
        description = "Preferred source line budget is 1 to 32. Larger accepted values are capped by Loom; use pagination via line_offset instead of broad file reads."
    )]
    #[serde(default = "default_inspect_lines")]
    pub line_budget: usize,
    #[schemars(
        description = "Preferred source character budget is 1 to 2000. Larger accepted values are capped by Loom; smaller budgets keep MCP answers compact."
    )]
    #[serde(default = "default_inspect_chars")]
    pub char_budget: usize,
    #[schemars(
        description = "Zero-based line offset within the symbol or file handle for pagination."
    )]
    #[serde(default)]
    pub line_offset: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvidencePackRequest {
    #[schemars(
        description = "Question or code concept to prove. Run before a final answer when citations are needed."
    )]
    pub query: String,
    #[schemars(
        description = "Preferred token budget for the evidence bundle is 1 to 3000. Larger accepted values are capped by Loom."
    )]
    #[serde(default = "default_evidence_budget_tokens")]
    pub budget_tokens: usize,
}

#[derive(Debug, Serialize)]
pub struct IndexHealth {
    pub status: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub stats: StoreStats,
    pub health: IndexHealth,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub vector_backend: String,
    pub embedder_backend: Option<String>,
    pub embedder_degraded: bool,
    pub embedder_model: Option<String>,
    pub embedder_dimensions: Option<usize>,
    pub schema_version: i64,
    pub watcher_active: bool,
    pub auto_watch: bool,
}

#[derive(Clone)]
pub struct LoomServerState {
    target_dir: PathBuf,
    core: Arc<Mutex<Option<Arc<CoreState>>>>,
}

struct CoreState {
    config: LoomConfig,
    db: Arc<LoomDb>,
    graph: Mutex<Arc<SymbolGraph>>,
    embedder: Mutex<Option<Arc<DefaultEmbedder>>>,
    reindex_lock: Mutex<()>,
    watcher: Mutex<Option<LoomWatcher>>,
}

impl LoomServerState {
    pub fn new(target_dir: PathBuf) -> Self {
        Self {
            target_dir,
            core: Arc::new(Mutex::new(None)),
        }
    }

    pub fn status(&self) -> loom_core::Result<StatusResponse> {
        let core = self.core()?;
        let graph = core.lock_graph();
        let embedder_status = core.embedder_status_or_config();
        let watcher_active = core.watcher_active();
        let stats = core.db.get_stats()?;
        let health = index_health(
            &stats,
            &embedder_status,
            core.config.auto_watch,
            watcher_active,
        );
        Ok(StatusResponse {
            stats,
            health,
            graph_nodes: graph.node_count(),
            graph_edges: graph.edge_count(),
            vector_backend: core.db.vector_backend_name().to_string(),
            embedder_backend: Some(embedder_status.backend.to_string()),
            embedder_degraded: embedder_status.degraded,
            embedder_model: embedder_status.model,
            embedder_dimensions: Some(embedder_status.dimensions),
            schema_version: core.db.schema_version()?,
            watcher_active,
            auto_watch: core.config.auto_watch,
        })
    }

    pub fn reindex(&self) -> loom_core::Result<IndexResult> {
        let core = self.core()?;
        let _guard = core.lock_reindex();
        let embedder = core.embedder()?;
        let pipeline = IndexPipeline::new(core.config.clone(), Arc::clone(&core.db), embedder);
        let result = pipeline.full_index()?;
        core.refresh_graph()?;
        Ok(result)
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        kind: Option<&str>,
    ) -> loom_core::Result<SearchResponse> {
        self.search_engine()?.search(query, limit, kind)
    }

    pub fn symbols(
        &self,
        query: &str,
        file_prefix: Option<&str>,
        kind: Option<&str>,
        limit: usize,
    ) -> loom_core::Result<SymbolListResponse> {
        self.search_engine()?
            .symbols(query, file_prefix, kind, limit)
    }

    pub fn related(
        &self,
        symbol: &str,
        file: Option<&str>,
        kind: Option<&str>,
    ) -> loom_core::Result<RelatedResponse> {
        self.search_engine()?.related(symbol, file, kind)
    }

    pub fn impact(
        &self,
        symbol: &str,
        file: Option<&str>,
        kind: Option<&str>,
    ) -> loom_core::Result<ImpactResponse> {
        self.search_engine()?.impact(symbol, file, kind)
    }

    pub fn neighborhood(&self, file: &str, line: i64) -> loom_core::Result<NeighborhoodResponse> {
        self.search_engine()?.neighborhood(file, line)
    }

    pub fn inspect(
        &self,
        handle: &str,
        line_budget: usize,
        char_budget: usize,
        line_offset: usize,
    ) -> loom_core::Result<InspectResponse> {
        self.search_engine()?
            .inspect(handle, line_budget, char_budget, line_offset)
    }

    pub fn evidence_pack(
        &self,
        query: &str,
        budget_tokens: usize,
    ) -> loom_core::Result<EvidencePackResponse> {
        self.search_engine()?.evidence_pack(query, budget_tokens)
    }

    fn search_engine(&self) -> loom_core::Result<SearchEngine<DefaultEmbedder>> {
        let core = self.core()?;
        let graph = core.lock_graph();
        Ok(SearchEngine::new(
            Arc::clone(&core.db),
            core.embedder()?,
            Some(graph),
            core.config.clone(),
        ))
    }

    fn core(&self) -> loom_core::Result<Arc<CoreState>> {
        let mut guard = match self.core.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(core) = guard.as_ref() {
            return Ok(Arc::clone(core));
        }
        let config = LoomConfig::load(self.target_dir.clone())?;
        let db = Arc::new(LoomDb::open(config.clone())?);
        let graph = Arc::new(SymbolGraph::build_from_db(&db)?);
        let core = Arc::new(CoreState {
            config,
            db,
            graph: Mutex::new(graph),
            embedder: Mutex::new(None),
            reindex_lock: Mutex::new(()),
            watcher: Mutex::new(None),
        });
        if core.config.auto_watch {
            core.start_watcher_once()?;
        }
        *guard = Some(Arc::clone(&core));
        Ok(core)
    }
}

fn index_health(
    stats: &StoreStats,
    embedder_status: &loom_core::embedder::EmbedderStatus,
    auto_watch: bool,
    watcher_active: bool,
) -> IndexHealth {
    let mut warnings = Vec::new();
    if stats.stale_files > 0 {
        warnings.push(format!(
            "{} stale or unindexed files; run reindex before trusting impact/search exhaustiveness",
            stats.stale_files
        ));
    }
    if auto_watch && !watcher_active {
        warnings.push("auto_watch is enabled but the watcher is not active".to_string());
    }
    if embedder_status.degraded {
        warnings.push("embedder is running in degraded fallback mode".to_string());
    }
    if stats.edges > 0 && stats.unresolved_edges > stats.resolved_edges.saturating_mul(3) {
        warnings.push(format!(
            "high unresolved edge ratio: {} unresolved vs {} resolved",
            stats.unresolved_edges, stats.resolved_edges
        ));
    }
    if stats.callsites > 0
        && stats.unresolved_callsites > stats.resolved_callsites.saturating_mul(3)
    {
        warnings.push(format!(
            "high unresolved callsite ratio: {} unresolved vs {} resolved",
            stats.unresolved_callsites, stats.resolved_callsites
        ));
    }
    let status = if stats.stale_files > 0 || embedder_status.degraded {
        "stale_or_degraded"
    } else if warnings.is_empty() {
        "healthy"
    } else {
        "attention"
    };
    IndexHealth {
        status: status.to_string(),
        warnings,
    }
}

impl CoreState {
    fn start_watcher_once(self: &Arc<Self>) -> loom_core::Result<()> {
        let mut guard = self.lock_watcher();
        if guard.is_some() {
            return Ok(());
        }
        let weak: Weak<Self> = Arc::downgrade(self);
        let handler = Arc::new(FnChangeHandler::new(move |paths| {
            let Some(core) = weak.upgrade() else {
                return Ok(());
            };
            core.handle_changed_paths(paths).map(|_| ())
        }));
        let watcher = LoomWatcher::start(self.config.clone(), handler)?;
        *guard = Some(watcher);
        Ok(())
    }

    fn handle_changed_paths(&self, paths: Vec<PathBuf>) -> loom_core::Result<IndexResult> {
        let _guard = self.lock_reindex();
        let embedder = self.embedder()?;
        let pipeline = IndexPipeline::new(self.config.clone(), Arc::clone(&self.db), embedder);
        let result = pipeline.incremental_index(paths)?;
        self.refresh_graph()?;
        Ok(result)
    }

    fn embedder(&self) -> loom_core::Result<Arc<DefaultEmbedder>> {
        let mut guard = self.lock_embedder();
        if let Some(embedder) = guard.as_ref() {
            return Ok(Arc::clone(embedder));
        }
        let embedder = Arc::new(DefaultEmbedder::from_config(&self.config)?);
        *guard = Some(Arc::clone(&embedder));
        Ok(embedder)
    }

    fn embedder_status(&self) -> Option<loom_core::embedder::EmbedderStatus> {
        let guard = self.lock_embedder();
        guard.as_ref().map(|embedder| embedder.status())
    }

    fn embedder_status_or_config(&self) -> loom_core::embedder::EmbedderStatus {
        self.embedder_status()
            .unwrap_or_else(|| loom_core::embedder::EmbedderStatus {
                backend: self.config.embedding_backend.as_str(),
                degraded: false,
                dimensions: self.config.embedding_dimensions,
                model: match self.config.embedding_backend {
                    loom_core::EmbeddingBackendConfig::Candle => {
                        Some(self.config.embedding_model.clone())
                    }
                    loom_core::EmbeddingBackendConfig::Hashing => None,
                },
            })
    }

    fn refresh_graph(&self) -> loom_core::Result<()> {
        let graph = Arc::new(SymbolGraph::build_from_db(&self.db)?);
        let mut guard = self.lock_graph_mut();
        *guard = graph;
        Ok(())
    }

    fn lock_graph(&self) -> Arc<SymbolGraph> {
        match self.graph.lock() {
            Ok(guard) => Arc::clone(&guard),
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }

    fn lock_graph_mut(&self) -> std::sync::MutexGuard<'_, Arc<SymbolGraph>> {
        match self.graph.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn lock_embedder(&self) -> std::sync::MutexGuard<'_, Option<Arc<DefaultEmbedder>>> {
        match self.embedder.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn lock_reindex(&self) -> std::sync::MutexGuard<'_, ()> {
        match self.reindex_lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn lock_watcher(&self) -> std::sync::MutexGuard<'_, Option<LoomWatcher>> {
        match self.watcher.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("watcher lock poisoned; recovering inner state");
                poisoned.into_inner()
            }
        }
    }

    fn watcher_active(&self) -> bool {
        self.lock_watcher().is_some()
    }
}

impl SearchRequest {
    fn validate(&self) -> Result<(), ErrorData> {
        validate_nonempty("query", &self.query, MAX_QUERY_BYTES)?;
        validate_optional("kind", self.kind.as_deref(), MAX_SYMBOL_BYTES)
    }
}

impl SymbolListRequest {
    fn validate(&self) -> Result<(), ErrorData> {
        validate_nonempty("query", &self.query, MAX_SYMBOL_BYTES)?;
        validate_optional("file_prefix", self.file_prefix.as_deref(), MAX_FILE_BYTES)?;
        validate_optional("kind", self.kind.as_deref(), MAX_SYMBOL_BYTES)
    }
}

impl SymbolRequest {
    fn validate(&self) -> Result<(), ErrorData> {
        validate_nonempty("symbol", &self.symbol, MAX_SYMBOL_BYTES)?;
        validate_optional("file", self.file.as_deref(), MAX_FILE_BYTES)?;
        validate_optional("kind", self.kind.as_deref(), MAX_SYMBOL_BYTES)
    }
}

impl NeighborhoodRequest {
    fn validate(&self) -> Result<(), ErrorData> {
        validate_nonempty("file", &self.file, MAX_FILE_BYTES)
    }
}

impl InspectRequest {
    fn validate(&self) -> Result<(), ErrorData> {
        validate_nonempty("handle", &self.handle, MAX_HANDLE_BYTES)?;
        validate_range("line_budget", self.line_budget, 1, MAX_INSPECT_LINES)?;
        validate_range("char_budget", self.char_budget, 1, MAX_INSPECT_CHARS)?;
        validate_range("line_offset", self.line_offset, 0, MAX_INSPECT_LINE_OFFSET)
    }
}

impl EvidencePackRequest {
    fn validate(&self) -> Result<(), ErrorData> {
        validate_nonempty("query", &self.query, MAX_QUERY_BYTES)?;
        validate_range(
            "budget_tokens",
            self.budget_tokens,
            1,
            MAX_EVIDENCE_BUDGET_TOKENS,
        )
    }
}

#[derive(Clone)]
pub struct LoomMcpServer {
    state: LoomServerState,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl LoomMcpServer {
    pub fn new(target_dir: PathBuf) -> loom_core::Result<Self> {
        Ok(Self {
            state: LoomServerState::new(target_dir),
            tool_router: Self::tool_router(),
        })
    }
}

#[tool_router]
impl LoomMcpServer {
    #[tool(
        description = "Read-only first step for conceptual code discovery. The model does not choose a result limit; Loom returns the complete internally bounded exact_hits and beyond_grep set with handles, anchors, summaries, reasons, and budget metadata. Use symbols for exact enumeration, evidence_pack for broad proof, and inspect only chosen handles next.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn search(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let engine = self.state.search_engine().map_err(to_mcp_error)?;
        let results = engine
            .search(&request.query, MCP_SEARCH_LIMIT, request.kind.as_deref())
            .map_err(to_mcp_error)?;
        json_content(results)
    }

    #[tool(
        description = "Read-only exact symbol enumerator. The model does not choose a result limit; Loom returns every internally bounded matching symbol. Use before grep for same-name methods or known symbols, for example query=execute file_prefix=sources/commands kind=method; returns compact handles for selective inspect.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn symbols(
        &self,
        Parameters(request): Parameters<SymbolListRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let engine = self.state.search_engine().map_err(to_mcp_error)?;
        let results = engine
            .symbols(
                &request.query,
                request.file_prefix.as_deref(),
                request.kind.as_deref(),
                MCP_SYMBOL_LIMIT,
            )
            .map_err(to_mcp_error)?;
        json_content(results)
    }

    #[tool(
        description = "Read-only capped expansion after search or symbols. Returns compact coupled handles and anchors from Loom; pass file to disambiguate duplicate names, then inspect selected handles.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn related(
        &self,
        Parameters(request): Parameters<SymbolRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let engine = self.state.search_engine().map_err(to_mcp_error)?;
        let results = engine
            .related(
                &request.symbol,
                request.file.as_deref(),
                request.kind.as_deref(),
            )
            .map_err(to_mcp_error)?;
        json_content(results)
    }

    #[tool(
        description = "Read-only blast-radius check before editing. Returns compact static callers/dependents from Loom; this is not runtime tracing and snippets require inspect.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn impact(
        &self,
        Parameters(request): Parameters<SymbolRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let engine = self.state.search_engine().map_err(to_mcp_error)?;
        let results = engine
            .impact(
                &request.symbol,
                request.file.as_deref(),
                request.kind.as_deref(),
            )
            .map_err(to_mcp_error)?;
        json_content(results)
    }

    #[tool(
        description = "Read-only anchor lookup when you have a file and line. Returns compact nearby/coupled handles so you can avoid broad grep and inspect only selected code.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn neighborhood(
        &self,
        Parameters(request): Parameters<NeighborhoodRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let engine = self.state.search_engine().map_err(to_mcp_error)?;
        let results = engine
            .neighborhood(&request.file, request.line)
            .map_err(to_mcp_error)?;
        json_content(results)
    }

    #[tool(
        description = "Read-only source inspection. Resolves one Loom handle into a bounded snippet with file/line citations, stale-handle guidance, and pagination metadata.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn inspect(
        &self,
        Parameters(request): Parameters<InspectRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let engine = self.state.search_engine().map_err(to_mcp_error)?;
        let results = engine
            .inspect(
                &request.handle,
                request.line_budget,
                request.char_budget,
                request.line_offset,
            )
            .map_err(to_mcp_error)?;
        json_content(results)
    }

    #[tool(
        description = "Read-only proof bundle before a final answer. Orchestrates search, beyond-grep evidence, graph neighbors, and inspected snippets within budget; shell is last resort.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn evidence_pack(
        &self,
        Parameters(request): Parameters<EvidencePackRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let engine = self.state.search_engine().map_err(to_mcp_error)?;
        let results = engine
            .evidence_pack(&request.query, request.budget_tokens)
            .map_err(to_mcp_error)?;
        json_content(results)
    }

    #[tool(
        description = "Non-read-only index mutation. Updates only the local .loom index after source changes or stale status; run before search when freshness is uncertain.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    pub fn reindex(&self) -> Result<Content, ErrorData> {
        json_content(self.state.reindex().map_err(to_mcp_error)?)
    }

    #[tool(
        description = "Read-only index health check. Use before relying on Loom; reports freshness, counts, backends, schema, and watcher state.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn status(&self) -> Result<Content, ErrorData> {
        json_content(self.state.status().map_err(to_mcp_error)?)
    }
}

#[tool_handler]
impl ServerHandler for LoomMcpServer {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("loom-mcp", env!("CARGO_PKG_VERSION")))
    }
}

fn json_content(value: impl Serialize) -> Result<Content, ErrorData> {
    Content::json(value)
}

fn to_mcp_error(source: loom_core::LoomError) -> ErrorData {
    error!(error = %source, "loom MCP tool failed");
    match source {
        loom_core::LoomError::InvalidInput(message)
        | loom_core::LoomError::InvalidConfig(message) => ErrorData::invalid_params(message, None),
        other => ErrorData::internal_error(
            format!(
                "Loom tool execution failed: {other}. Retry status, reindex if stale, then search again."
            ),
            None,
        ),
    }
}

const fn default_inspect_lines() -> usize {
    24
}

const fn default_inspect_chars() -> usize {
    2_000
}

const fn default_evidence_budget_tokens() -> usize {
    1_200
}

fn validate_range(name: &str, value: usize, min: usize, max: usize) -> Result<(), ErrorData> {
    if !(min..=max).contains(&value) {
        return Err(ErrorData::invalid_params(
            format!("{name} must be between {min} and {max}"),
            None,
        ));
    }
    Ok(())
}

fn validate_nonempty(name: &str, value: &str, max_bytes: usize) -> Result<(), ErrorData> {
    if value.trim().is_empty() {
        return Err(ErrorData::invalid_params(
            format!("{name} must not be empty"),
            None,
        ));
    }
    validate_length(name, value, max_bytes)
}

fn validate_optional(name: &str, value: Option<&str>, max_bytes: usize) -> Result<(), ErrorData> {
    if let Some(value) = value {
        validate_length(name, value, max_bytes)?;
    }
    Ok(())
}

fn validate_length(name: &str, value: &str, max_bytes: usize) -> Result<(), ErrorData> {
    if value.len() > max_bytes {
        return Err(ErrorData::invalid_params(
            format!("{name} must be at most {max_bytes} bytes"),
            None,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Tool;
    use serde_json::Value;
    use std::collections::BTreeMap;

    #[test]
    fn status_opens_db_without_loading_embedder() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".loom")).unwrap();
        std::fs::write(
            temp.path().join(".loom/config.toml"),
            r#"
vector_backend = "blob"
embedding_backend = "hashing"
auto_watch = false
"#,
        )
        .unwrap();
        let state = LoomServerState::new(temp.path().to_path_buf());
        let status = state.status().unwrap();
        assert_eq!(status.stats.symbols, 0);
        assert_eq!(status.graph_nodes, 0);
        assert_eq!(status.vector_backend, "blob");
        assert_eq!(status.embedder_backend, Some("hashing".to_string()));
        assert!(!status.embedder_degraded);
        assert_eq!(status.embedder_model, None);
        assert_eq!(status.embedder_dimensions, Some(768));
        assert_eq!(status.health.status, "healthy");
        assert!(status.health.warnings.is_empty());
        assert!(!status.watcher_active);
        assert!(!status.auto_watch);
    }

    #[test]
    fn mcp_server_registers_expected_tools() {
        let temp = tempfile::tempdir().unwrap();
        let server = LoomMcpServer::new(temp.path().to_path_buf()).unwrap();
        let names = server
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        for expected in [
            "evidence_pack",
            "impact",
            "inspect",
            "neighborhood",
            "reindex",
            "related",
            "search",
            "status",
            "symbols",
        ] {
            assert!(names.contains(expected));
        }
    }

    #[test]
    fn mcp_tools_prompt_models_to_stay_inside_loom() {
        let temp = tempfile::tempdir().unwrap();
        let server = LoomMcpServer::new(temp.path().to_path_buf()).unwrap();
        let tools = tools_by_name(&server);

        assert_description_contains(
            &tools,
            "search",
            &["first step", "exact_hits", "inspect only chosen handles"],
        );
        assert_description_contains(&tools, "symbols", &["exact symbol", "same-name"]);
        assert_description_contains(&tools, "related", &["after search", "pass file"]);
        assert_description_contains(&tools, "impact", &["before editing", "not runtime tracing"]);
        assert_description_contains(
            &tools,
            "neighborhood",
            &["file and line", "avoid broad grep"],
        );
        assert_description_contains(&tools, "inspect", &["bounded snippet", "stale-handle"]);
        assert_description_contains(
            &tools,
            "evidence_pack",
            &["proof bundle", "shell is last resort"],
        );
        assert_description_contains(&tools, "reindex", &["Non-read-only index mutation"]);
        assert_description_contains(&tools, "status", &["Read-only index health check"]);

        assert_read_only(&tools, "search", true);
        assert_read_only(&tools, "symbols", true);
        assert_read_only(&tools, "related", true);
        assert_read_only(&tools, "impact", true);
        assert_read_only(&tools, "neighborhood", true);
        assert_read_only(&tools, "inspect", true);
        assert_read_only(&tools, "evidence_pack", true);
        assert_read_only(&tools, "status", true);
        assert_read_only(&tools, "reindex", false);
        assert_eq!(
            tools["reindex"]
                .annotations
                .as_ref()
                .unwrap()
                .destructive_hint,
            Some(false)
        );

        assert_property_description_contains(&tools, "search", "query", "before grep");
        assert_property_description_contains(&tools, "search", "kind", "function");
        assert_property_absent(&tools, "search", "limit");
        assert_property_description_contains(&tools, "symbols", "file_prefix", "file prefix");
        assert_property_description_contains(&tools, "symbols", "query", "same-name");
        assert_property_absent(&tools, "symbols", "limit");
        assert_property_description_contains(&tools, "related", "file", "disambiguate");
        assert_property_description_contains(&tools, "neighborhood", "line", "One-based");
        assert_property_description_contains(&tools, "inspect", "handle", "selected handles");
        assert_property_description_contains(&tools, "inspect", "line_budget", "pagination");
        assert_property_description_contains(&tools, "inspect", "line_offset", "pagination");
        assert_property_description_contains(&tools, "evidence_pack", "budget_tokens", "3000");
    }

    #[test]
    fn changed_paths_helper_indexes_incrementally_and_refreshes_graph() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".loom")).unwrap();
        std::fs::write(
            temp.path().join(".loom/config.toml"),
            r#"
vector_backend = "blob"
embedding_backend = "hashing"
embedding_dimensions = 16
auto_watch = false
"#,
        )
        .unwrap();
        let src_dir = temp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let source = src_dir.join("app.ts");
        std::fs::write(
            &source,
            "function alpha() {\n  return beta();\n}\n\nfunction beta() {\n  return 1;\n}\n",
        )
        .unwrap();

        let state = LoomServerState::new(temp.path().to_path_buf());
        let core = state.core().unwrap();
        let result = core.handle_changed_paths(vec![source]).unwrap();
        let status = state.status().unwrap();

        assert_eq!(result.indexed, 1);
        assert_eq!(status.stats.symbols, 2);
        assert!(status.graph_nodes > 0);
        assert_eq!(status.embedder_backend, Some("hashing".to_string()));
        assert_eq!(status.embedder_model, None);
        assert_eq!(status.embedder_dimensions, Some(16));
    }

    #[test]
    fn changed_paths_helper_handles_rename_without_stale_index_rows() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".loom")).unwrap();
        std::fs::write(
            temp.path().join(".loom/config.toml"),
            r#"
vector_backend = "blob"
embedding_backend = "hashing"
embedding_dimensions = 16
auto_watch = false
"#,
        )
        .unwrap();
        let src_dir = temp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let old_path = src_dir.join("old.ts");
        let new_path = src_dir.join("new.ts");
        std::fs::write(&old_path, "function renamedSymbol() {\n  return 1;\n}\n").unwrap();

        let state = LoomServerState::new(temp.path().to_path_buf());
        let core = state.core().unwrap();
        core.handle_changed_paths(vec![old_path.clone()]).unwrap();
        std::fs::rename(&old_path, &new_path).unwrap();

        let result = core
            .handle_changed_paths(vec![old_path.clone(), new_path.clone()])
            .unwrap();
        let status = state.status().unwrap();

        assert_eq!(result.deleted, 1);
        assert_eq!(result.indexed, 1);
        assert_eq!(status.stats.files, 1);
        assert_eq!(status.stats.symbols, 1);
        assert!(core.db.get_file_hash("src/old.ts").unwrap().is_none());
        assert!(core.db.get_file_hash("src/new.ts").unwrap().is_some());
        let symbols = core
            .db
            .get_symbol_by_name("renamedSymbol", None)
            .unwrap()
            .into_iter()
            .map(|symbol| symbol.file)
            .collect::<Vec<_>>();
        assert_eq!(symbols, vec!["src/new.ts"]);
    }

    fn tools_by_name(server: &LoomMcpServer) -> BTreeMap<String, Tool> {
        server
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| (tool.name.to_string(), tool))
            .collect()
    }

    fn assert_description_contains(tools: &BTreeMap<String, Tool>, name: &str, needles: &[&str]) {
        let description = tools[name].description.as_deref().unwrap();
        for needle in needles {
            assert!(
                description.contains(needle),
                "{name} description should contain {needle:?}: {description}"
            );
        }
    }

    fn assert_read_only(tools: &BTreeMap<String, Tool>, name: &str, expected: bool) {
        assert_eq!(
            tools[name].annotations.as_ref().unwrap().read_only_hint,
            Some(expected),
            "{name} read_only_hint"
        );
    }

    fn assert_property_description_contains(
        tools: &BTreeMap<String, Tool>,
        tool_name: &str,
        property_name: &str,
        needle: &str,
    ) {
        let properties = tools[tool_name]
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .unwrap();
        let description = properties[property_name]
            .get("description")
            .and_then(Value::as_str)
            .unwrap();
        assert!(
            description.contains(needle),
            "{tool_name}.{property_name} description should contain {needle:?}: {description}"
        );
    }

    fn assert_property_absent(
        tools: &BTreeMap<String, Tool>,
        tool_name: &str,
        property_name: &str,
    ) {
        let properties = tools[tool_name]
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .unwrap();
        assert!(
            !properties.contains_key(property_name),
            "{tool_name} should not expose model-chosen {property_name}"
        );
    }
}
