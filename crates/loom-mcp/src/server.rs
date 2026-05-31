use loom_core::{
    embedder::{DefaultEmbedder, Embedder},
    graph::SymbolGraph,
    indexer::IndexPipeline,
    models::{
        EvidencePackResponse, ImpactResponse, InspectResponse, NeighborhoodResponse,
        RelatedResponse, SearchResponse, StoreStats, SymbolHit, SymbolListResponse,
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use tracing::{error, warn};

const MAX_QUERY_BYTES: usize = 4_096;
const MAX_SYMBOL_BYTES: usize = 512;
const MAX_FILE_BYTES: usize = 2_048;
const MAX_HANDLE_BYTES: usize = 4_096;
const MAX_INSPECT_LINES: usize = 120;
const MAX_INSPECT_CHARS: usize = 16_000;
const MAX_INSPECT_LINE_OFFSET: usize = 1_000_000;
const MAX_BATCH_QUERIES: usize = 32;
const MAX_EVIDENCE_BUDGET_TOKENS: usize = 16_000;
const MAX_SEARCH_BUDGET_TOKENS: usize = 8_000;
const MCP_SEARCH_LIMIT: usize = 100;
const MCP_SYMBOL_LIMIT: usize = 48;

#[derive(Debug)]
struct SymbolOnlyEmbedder {
    dimensions: usize,
}

impl Embedder for SymbolOnlyEmbedder {
    fn embed(&self, _texts: &[String]) -> loom_core::Result<Vec<Vec<f32>>> {
        Err(loom_core::LoomError::InvalidInput(
            "symbol-only embedder cannot run semantic search".to_string(),
        ))
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    #[schemars(
        description = "Code search query for exact repo file:line answers. Call loom.search before shell grep/find/cat."
    )]
    pub query: String,
    #[schemars(
        description = "Optional exact kind filter: function, class, method, variable, interface, struct."
    )]
    pub kind: Option<String>,
    #[schemars(
        description = "Search mode: auto, callers, callees, impact, definitions, implementations. Use definitions for type/method locations, impact for blast radius."
    )]
    pub mode: Option<String>,
    #[schemars(description = "Search response token budget, default 2000 and capped at 8000.")]
    #[serde(default = "default_search_budget_tokens")]
    pub budget_tokens: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BatchSearchItem {
    #[schemars(
        description = "One concrete code query, such as type Storage interface or DB.Close."
    )]
    pub query: String,
    #[schemars(
        description = "Optional exact kind filter: function, class, method, variable, interface, struct."
    )]
    pub kind: Option<String>,
    #[schemars(
        description = "Search mode for this query: auto, callers, callees, impact, definitions, implementations."
    )]
    pub mode: Option<String>,
    #[schemars(description = "Per-query token budget, default 900 and capped at 8000.")]
    #[serde(default = "default_batch_search_budget_tokens")]
    pub budget_tokens: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum BatchSearchEntry {
    Text(String),
    Item(BatchSearchItem),
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BatchSearchRequest {
    #[schemars(
        description = "Batch of concrete code queries. Strings are accepted; objects can set query/kind/mode/budget_tokens."
    )]
    pub queries: Vec<BatchSearchEntry>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolListRequest {
    #[schemars(
        description = "Exact symbol/name suffix to enumerate after search, especially duplicate same-name methods."
    )]
    pub query: String,
    #[schemars(description = "Optional repo-relative file prefix, such as sources/commands.")]
    pub file_prefix: Option<String>,
    #[schemars(
        description = "Optional exact kind filter: function, class, method, variable, interface, struct."
    )]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolRequest {
    #[schemars(description = "Symbol name returned by search, symbols, or neighborhood.")]
    pub symbol: String,
    #[schemars(description = "Optional repo-relative path to disambiguate duplicate names.")]
    pub file: Option<String>,
    #[schemars(
        description = "Optional exact kind filter: function, class, method, variable, interface, struct."
    )]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NeighborhoodRequest {
    #[schemars(description = "Repo-relative indexed file path.")]
    pub file: String,
    #[schemars(description = "One-based line number; anchors to covering or nearest symbol.")]
    pub line: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectRequest {
    #[schemars(description = "Handle from a Loom result. Inspect only selected handles.")]
    pub handle: String,
    #[schemars(description = "Source line budget, capped; use line_offset for pagination.")]
    #[serde(default = "default_inspect_lines")]
    pub line_budget: usize,
    #[schemars(description = "Source character budget, capped.")]
    #[serde(default = "default_inspect_chars")]
    pub char_budget: usize,
    #[schemars(description = "Zero-based pagination offset.")]
    #[serde(default)]
    pub line_offset: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvidencePackRequest {
    #[schemars(
        description = "full user question for one-shot evidence_pack. Include all sub-questions; do not split into search/inspect follow-ups."
    )]
    pub query: String,
    #[schemars(description = "Evidence token budget, default 8000 and capped at 16000.")]
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
    index_ready: AtomicBool,
    background_index_started: AtomicBool,
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
        core.full_index_and_refresh()
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
        let core = self.core()?;
        let engine = SearchEngine::new(
            Arc::clone(&core.db),
            Arc::new(SymbolOnlyEmbedder {
                dimensions: core.config.embedding_dimensions,
            }),
            None,
            core.config.clone(),
        );
        engine.symbols(query, file_prefix, kind, limit)
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
        )
        .with_index_ready(core.index_ready.load(Ordering::Acquire)))
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
        let stats = db.get_stats()?;
        let core = Arc::new(CoreState {
            config,
            db,
            graph: Mutex::new(graph),
            embedder: Mutex::new(None),
            index_ready: AtomicBool::new(stats.stale_files == 0 && stats.files > 0),
            background_index_started: AtomicBool::new(false),
            reindex_lock: Mutex::new(()),
            watcher: Mutex::new(None),
        });
        if core.config.auto_watch {
            core.start_watcher_once()?;
        }
        *guard = Some(Arc::clone(&core));
        Ok(core)
    }

    fn start_background_indexing(&self) -> loom_core::Result<()> {
        let core = self.core()?;
        core.start_background_indexing();
        Ok(())
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
    fn start_background_indexing(self: &Arc<Self>) {
        if self.index_ready.load(Ordering::Acquire)
            || self.background_index_started.swap(true, Ordering::AcqRel)
        {
            return;
        }
        let core = Arc::clone(self);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || core.run_background_index());
        } else {
            std::thread::spawn(move || core.run_background_index());
        }
    }

    fn run_background_index(self: Arc<Self>) {
        match self.full_index_and_refresh() {
            Ok(_) => self.index_ready.store(true, Ordering::Release),
            Err(source) => {
                self.background_index_started
                    .store(false, Ordering::Release);
                warn!(error = %source, "background index failed");
            }
        }
    }

    fn full_index_and_refresh(&self) -> loom_core::Result<IndexResult> {
        let _guard = self.lock_reindex();
        let embedder = self.embedder()?;
        let pipeline = IndexPipeline::new(self.config.clone(), Arc::clone(&self.db), embedder);
        let result = pipeline.full_index()?;
        self.refresh_graph()?;
        self.index_ready.store(true, Ordering::Release);
        Ok(result)
    }

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
        validate_optional("kind", self.kind.as_deref(), MAX_SYMBOL_BYTES)?;
        validate_optional("mode", self.mode.as_deref(), MAX_SYMBOL_BYTES)?;
        validate_range(
            "budget_tokens",
            self.budget_tokens,
            1,
            MAX_SEARCH_BUDGET_TOKENS,
        )
    }
}

impl BatchSearchItem {
    fn validate(&self) -> Result<(), ErrorData> {
        validate_nonempty("query", &self.query, MAX_QUERY_BYTES)?;
        validate_optional("kind", self.kind.as_deref(), MAX_SYMBOL_BYTES)?;
        validate_optional("mode", self.mode.as_deref(), MAX_SYMBOL_BYTES)?;
        validate_range(
            "budget_tokens",
            self.budget_tokens,
            1,
            MAX_SEARCH_BUDGET_TOKENS,
        )
    }
}

impl BatchSearchEntry {
    fn validate(&self) -> Result<(), ErrorData> {
        match self {
            BatchSearchEntry::Text(query) => validate_nonempty("query", query, MAX_QUERY_BYTES),
            BatchSearchEntry::Item(item) => item.validate(),
        }
    }

    fn query(&self) -> String {
        match self {
            BatchSearchEntry::Text(query) => query.clone(),
            BatchSearchEntry::Item(item) => item.query.clone(),
        }
    }

    fn kind(&self) -> Option<&str> {
        match self {
            BatchSearchEntry::Text(query) => infer_batch_kind(query),
            BatchSearchEntry::Item(item) => item.kind.as_deref(),
        }
    }

    fn mode(&self) -> Option<&str> {
        match self {
            BatchSearchEntry::Text(query) => infer_batch_mode(query),
            BatchSearchEntry::Item(item) => item.mode.as_deref(),
        }
    }

    fn budget_tokens(&self) -> usize {
        match self {
            BatchSearchEntry::Text(_) => default_batch_search_budget_tokens(),
            BatchSearchEntry::Item(item) => item.budget_tokens,
        }
    }
}

impl BatchSearchRequest {
    fn validate(&self) -> Result<(), ErrorData> {
        validate_range("queries", self.queries.len(), 1, MAX_BATCH_QUERIES)?;
        for query in &self.queries {
            query.validate()?;
        }
        Ok(())
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

    pub fn start_lazy_indexing(&self) -> loom_core::Result<()> {
        self.state.start_background_indexing()
    }
}

#[tool_router]
impl LoomMcpServer {
    #[tool(
        description = "Single focused query for targeted follow-up only. Prefer evidence_pack for initial exploration.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn search(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let engine = self.state.search_engine().map_err(to_mcp_error)?;
        let results = engine
            .search_mode_with_budget(
                &request.query,
                MCP_SEARCH_LIMIT,
                request.kind.as_deref(),
                request.mode.as_deref(),
                request.budget_tokens,
            )
            .map_err(to_mcp_error)?;
        Ok(Content::text(render_mcp_search(&results, true)))
    }

    #[tool(
        description = "Use only after loom.evidence_pack when you already know several narrow subqueries. Do not use this as the first tool for full task prompts. Results are verified citable file:line evidence from the local index. Do not run shell rg/cat/nl/sed to verify returned hits.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn batch_search(
        &self,
        Parameters(request): Parameters<BatchSearchRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let engine = self.state.search_engine().map_err(to_mcp_error)?;
        let mut rendered = format!(
            "loom.batch_search queries={}\nNO_SHELL_VERIFICATION: returned hits are verified local file:line evidence. Do not rg/cat/nl/sed to re-check them; use loom.inspect only for body text.",
            request.queries.len()
        );
        for (index, entry) in request.queries.iter().enumerate() {
            let query = entry.query();
            rendered.push_str(&format!(
                "\n\n## q{} query={} mode={} kind={}",
                index + 1,
                query,
                entry.mode().unwrap_or("auto"),
                entry.kind().unwrap_or("any")
            ));
            rendered.push('\n');
            let response = engine
                .search_mode_with_budget(
                    &query,
                    MCP_SEARCH_LIMIT,
                    entry.kind(),
                    entry.mode(),
                    entry.budget_tokens(),
                )
                .map_err(to_mcp_error)?;
            rendered.push_str(&render_mcp_search(&response, false));
        }
        rendered.push_str(
            "\n\nBATCH_COMPLETE: answer from the relevant file:line hits above. Do not run shell follow-up. If a body is needed, call loom.inspect on the handle instead of rg/cat/nl/sed.",
        );
        Ok(Content::text(rendered))
    }

    #[tool(
        description = "Use after loom.search to enumerate exact citable symbols when names are duplicated. Read-only/local-only. Prefer this over rg for same-name methods, interfaces, structs, and file-prefix narrowing. Do not inspect every result.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub fn symbols(
        &self,
        Parameters(request): Parameters<SymbolListRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let results = self
            .state
            .symbols(
                &request.query,
                request.file_prefix.as_deref(),
                request.kind.as_deref(),
                MCP_SYMBOL_LIMIT,
            )
            .map_err(to_mcp_error)?;
        Ok(Content::text(render_mcp_symbols(&results)))
    }

    #[tool(
        description = "Read-only/local-only graph expansion after loom.search. Use instead of shell follow-up when you need neighboring callers, callees, implemented interfaces, or related definitions. Pass file to disambiguate duplicate names.",
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
        description = "Read-only/local-only blast-radius check. Use for signature-change impact, callers, implementations, and Commit/Close-style method fanout. Prefer search(mode=\"impact\") first; use this to expand one exact symbol.",
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
        description = "Read-only/local-only file:line neighborhood. Use after a Loom hit to identify the containing function, method, struct, interface, callers, and adjacent symbols without cat/sed.",
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
        description = "Read source lines for one handle. Rarely needed - evidence_pack already includes snippets.",
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
        Ok(Content::text(render_mcp_inspect(&results)))
    }

    #[tool(
        description = "One-shot answer engine for code questions. Returns complete source snippets with file:lines. Call ONCE with your full question - do not follow up with search/inspect/symbols/shell.",
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
        Ok(Content::text(render_mcp_evidence_pack(&results)))
    }

    #[tool(
        description = "Non-read-only index mutation. Updates only local .loom after source changes or stale status.",
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
        description = "Read-only/local-only index health check. Call when unsure whether Loom is ready; if healthy/stale_files=0, trust loom.search before shell grep/find/cat.",
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

fn render_mcp_search(response: &SearchResponse, include_continuation: bool) -> String {
    let total = response.exact_hits.len() + response.beyond_grep.len();
    let mut output = format!(
        "loom.search rev={} results={} truncated={} omitted={}",
        response.index_revision, total, response.truncated, response.budget.omitted
    );
    if let Some(status) = &response.index_status {
        output.push_str(&format!("\nindex_status={status}"));
    }
    output.push_str(
        "\nCITE_DIRECTLY: each hit is local file:line evidence. Do not grep/cat/sed to re-verify. Use loom.inspect only when the answer needs body text.",
    );
    append_mcp_hits(&mut output, "exact_hits", &response.exact_hits);
    append_mcp_hits(&mut output, "beyond_grep", &response.beyond_grep);
    if include_continuation {
        if let Some(continuation) = &response.continuation {
            output.push_str(&format!(
                "\nmore omitted={} hint={}",
                continuation.omitted, continuation.next_request_hint
            ));
        }
    }
    output
}

fn render_mcp_symbols(response: &SymbolListResponse) -> String {
    let mut output = format!(
        "loom.symbols rev={} query={} results={} truncated={}",
        response.index_revision,
        response.query,
        response.results.len(),
        response.truncated
    );
    if let Some(prefix) = &response.file_prefix {
        output.push_str(&format!(" file_prefix={prefix}"));
    }
    if let Some(kind) = &response.kind {
        output.push_str(&format!(" kind={kind}"));
    }
    output.push_str(
        "\nCITE_DIRECTLY: each result is local file:line evidence. Do not inspect every result.",
    );
    append_mcp_hits(&mut output, "results", &response.results);
    output
}

fn append_mcp_hits(output: &mut String, label: &str, hits: &[SymbolHit]) {
    output.push_str(&format!("\n{label}:"));
    if hits.is_empty() {
        output.push_str("\n- none");
        return;
    }
    for hit in hits {
        output.push_str(&format!(
            "\n- {} {} {}:{}-{} handle={} summary={}",
            hit.kind,
            hit.name,
            hit.anchor.file,
            hit.anchor.line,
            hit.anchor.end_line,
            hit.handle,
            compact_one_line(&hit.summary)
        ));
        if let Some(role) = &hit.graph_role {
            output.push_str(&format!(" role={role}"));
        }
    }
}

fn render_mcp_inspect(response: &InspectResponse) -> String {
    if let Some(error) = &response.error {
        return format!(
            "loom.inspect rev={} handle={} kind={} stale={} error={}",
            response.index_revision, response.handle, response.handle_kind, response.stale, error
        );
    }
    let mut output = format!(
        "loom.inspect rev={} handle={} kind={} stale={} truncated={}",
        response.index_revision,
        response.handle,
        response.handle_kind,
        response.stale,
        response.truncated
    );
    output.push_str("\nNO_SHELL: snippet below is local disk text; cite these lines directly.");
    if let Some(snippet) = &response.snippet {
        output.push_str(&format!(
            "\n{}:{}-{}\n{}",
            snippet.anchor.file, snippet.start_line, snippet.end_line, snippet.text
        ));
    }
    if let Some(next) = response.page.next_line_offset {
        output.push_str(&format!("\nnext: loom.inspect line_offset={next}"));
    }
    output
}

fn render_mcp_evidence_pack(response: &EvidencePackResponse) -> String {
    let mut output = format!(
        "loom.evidence_pack budget={} sub_questions={}",
        response.budget.requested,
        response.sub_questions.len()
    );
    for question in &response.sub_questions {
        output.push_str(&format!(
            "\n\n## {}: {}",
            question.label,
            compact_one_line(&question.query)
        ));
        if !question.symbols.is_empty() {
            output.push_str(&format!("\nsymbols: {}", question.symbols.join(", ")));
        }
    }
    if !response.exact_hits.is_empty() {
        output.push_str("\n\nexact_hits:");
        for hit in &response.exact_hits {
            output.push_str(&format!(
                "\n{}:{}-{} | {} | {}",
                hit.anchor.file, hit.anchor.line, hit.anchor.end_line, hit.kind, hit.name
            ));
        }
    }
    if !response.beyond_grep.is_empty() {
        output.push_str("\n\nbeyond_grep:");
        for hit in &response.beyond_grep {
            output.push_str(&format!(
                "\n{}:{}-{} | {} | {}",
                hit.anchor.file, hit.anchor.line, hit.anchor.end_line, hit.kind, hit.name
            ));
        }
    }
    if !response.behavior_facts.is_empty() {
        output.push_str("\n\nfacts:");
        for hit in &response.behavior_facts {
            output.push_str(&format!(
                "\n{}:{} | {} | {}",
                hit.anchor.file, hit.anchor.line, hit.kind, hit.name
            ));
        }
    }
    if !response.inspected_snippets.is_empty() {
        output.push_str("\n\nsnippets:");
        for snippet in &response.inspected_snippets {
            append_numbered_snippet(&mut output, snippet);
        }
    }
    if !response.missing_concepts.is_empty() {
        output.push_str("\n\nmissing:");
        for concept in &response.missing_concepts {
            output.push_str(&format!("\n- {concept}"));
        }
    }
    if !response.omitted.is_empty() {
        let visible_notes = response
            .omitted
            .iter()
            .filter(|note| {
                !note.starts_with("not_found_symbol")
                    && *note != "search results were truncated before evidence packing"
            })
            .collect::<Vec<_>>();
        if !visible_notes.is_empty() {
            output.push_str("\n\nnotes:");
            for note in visible_notes {
                output.push_str(&format!("\n- {}", compact_one_line(note)));
            }
        }
    }
    output.push_str(&format!(
        "\n\nCOMPLETE: {} questions, {} evidence items. Use the snippets above as sufficient source evidence; do not run follow-up tools, including evidence_pack/shell/rg/cat/sed/nl.",
        response.sub_questions.len(),
        response.exact_hits.len()
            + response.beyond_grep.len()
            + response.behavior_facts.len()
            + response.inspected_snippets.len()
    ));
    output
}

fn append_numbered_snippet(output: &mut String, snippet: &loom_core::models::InspectSnippet) {
    output.push_str(&format!(
        "\n{}:{}-{}",
        snippet.anchor.file, snippet.start_line, snippet.end_line
    ));
    for (offset, line) in snippet.text.lines().enumerate() {
        let line_number = snippet.start_line + i64::try_from(offset).unwrap_or(0);
        output.push_str(&format!("\n  {line_number}: {line}"));
    }
}

fn compact_one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn infer_batch_mode(query: &str) -> Option<&'static str> {
    let lower = query.to_ascii_lowercase();
    if lower.contains("definition") || lower.contains("defined") {
        Some("definitions")
    } else if lower.contains("caller") || lower.contains("call site") || lower.contains("callsite")
    {
        Some("callers")
    } else if lower.contains("impact") || lower.contains("blast") {
        Some("impact")
    } else {
        None
    }
}

fn infer_batch_kind(query: &str) -> Option<&'static str> {
    let lower = query.to_ascii_lowercase();
    if lower.contains(" interface") {
        Some("interface")
    } else if lower.contains(" method") || query.contains('.') {
        Some("method")
    } else if lower.contains(" struct") || lower.contains(" type") {
        Some("class")
    } else if lower.contains(" function") || lower.starts_with("func ") {
        Some("function")
    } else {
        None
    }
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
    8_000
}

const fn default_search_budget_tokens() -> usize {
    2_000
}

const fn default_batch_search_budget_tokens() -> usize {
    900
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
            "batch_search",
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
            &["Single focused", "targeted follow-up", "evidence_pack"],
        );
        assert_description_contains(
            &tools,
            "batch_search",
            &[
                "after loom.evidence_pack",
                "Do not use this as the first tool",
            ],
        );
        assert_description_contains(&tools, "symbols", &["after loom.search", "same-name"]);
        assert_description_contains(
            &tools,
            "related",
            &["after loom.search", "instead of shell follow-up"],
        );
        assert_description_contains(
            &tools,
            "impact",
            &["search(mode=\"impact\")", "signature-change impact"],
        );
        assert_description_contains(&tools, "neighborhood", &["file:line neighborhood"]);
        assert_description_contains(&tools, "inspect", &["Read source lines", "evidence_pack"]);
        assert_description_contains(
            &tools,
            "evidence_pack",
            &[
                "One-shot answer engine",
                "complete source snippets",
                "Call ONCE",
            ],
        );
        assert_description_contains(&tools, "reindex", &["Non-read-only index mutation"]);
        assert_description_contains(&tools, "status", &["Read-only", "trust loom.search"]);

        assert_read_only(&tools, "search", true);
        assert_read_only(&tools, "batch_search", true);
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

        assert_property_description_contains(&tools, "search", "query", "before shell");
        assert_property_description_contains(&tools, "search", "kind", "function");
        assert_property_description_contains(&tools, "search", "mode", "blast radius");
        assert_property_absent(&tools, "search", "limit");
        assert_property_description_contains(&tools, "symbols", "file_prefix", "file prefix");
        assert_property_description_contains(&tools, "symbols", "query", "same-name");
        assert_property_absent(&tools, "symbols", "limit");
        assert_property_description_contains(&tools, "related", "file", "disambiguate");
        assert_property_description_contains(&tools, "neighborhood", "line", "One-based");
        assert_property_description_contains(&tools, "inspect", "handle", "selected handles");
        assert_property_description_contains(&tools, "inspect", "line_budget", "pagination");
        assert_property_description_contains(&tools, "inspect", "line_offset", "pagination");
        assert_property_description_contains(
            &tools,
            "evidence_pack",
            "query",
            "full user question",
        );
        assert_property_description_contains(&tools, "evidence_pack", "budget_tokens", "16000");
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
