use loom_core::{
    embedder::DefaultEmbedder,
    graph::SymbolGraph,
    indexer::IndexPipeline,
    models::StoreStats,
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

const MAX_LIMIT: usize = 100;
const MAX_QUERY_BYTES: usize = 4_096;
const MAX_SYMBOL_BYTES: usize = 512;
const MAX_FILE_BYTES: usize = 2_048;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolRequest {
    pub symbol: String,
    pub file: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NeighborhoodRequest {
    pub file: String,
    pub line: i64,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub stats: StoreStats,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub vector_backend: String,
    pub embedder_backend: Option<String>,
    pub embedder_degraded: bool,
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
        let embedder_status = core.embedder_status();
        Ok(StatusResponse {
            stats: core.db.get_stats()?,
            graph_nodes: graph.node_count(),
            graph_edges: graph.edge_count(),
            vector_backend: core.db.vector_backend_name().to_string(),
            embedder_backend: embedder_status
                .as_ref()
                .map(|status| status.backend.to_string()),
            embedder_degraded: embedder_status.is_some_and(|status| status.degraded),
            schema_version: core.db.schema_version()?,
            watcher_active: core.watcher_active(),
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
        validate_limit(self.limit)?;
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
    #[tool(description = "Hybrid FTS/vector code search with coupled symbol expansion.")]
    pub fn search(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<Content, ErrorData> {
        request.validate()?;
        let engine = self.state.search_engine().map_err(to_mcp_error)?;
        let results = engine
            .search(&request.query, request.limit, request.kind.as_deref())
            .map_err(to_mcp_error)?;
        json_content(results)
    }

    #[tool(description = "Return symbols coupled to a named symbol.")]
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

    #[tool(description = "Return likely blast radius for a named symbol.")]
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

    #[tool(description = "Return the coupling neighborhood for a file and line.")]
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

    #[tool(description = "Trigger a full reindex of the target project.")]
    pub fn reindex(&self) -> Result<Content, ErrorData> {
        json_content(self.state.reindex().map_err(to_mcp_error)?)
    }

    #[tool(description = "Return index health, graph size, and store stats.")]
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
    ErrorData::internal_error("Loom tool execution failed", None)
}

const fn default_limit() -> usize {
    10
}

fn validate_limit(limit: usize) -> Result<(), ErrorData> {
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ErrorData::invalid_params(
            format!("limit must be between 1 and {MAX_LIMIT}"),
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
        assert_eq!(status.embedder_backend, None);
        assert!(!status.embedder_degraded);
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
            "impact",
            "neighborhood",
            "reindex",
            "related",
            "search",
            "status",
        ] {
            assert!(names.contains(expected));
        }
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
        let source = src_dir.join("app.py");
        std::fs::write(
            &source,
            "def alpha():\n    return beta()\n\ndef beta():\n    return 1\n",
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
        let old_path = src_dir.join("old.py");
        let new_path = src_dir.join("new.py");
        std::fs::write(&old_path, "def renamed_symbol():\n    return 1\n").unwrap();

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
        assert!(core.db.get_file_hash("src/old.py").unwrap().is_none());
        assert!(core.db.get_file_hash("src/new.py").unwrap().is_some());
        let symbols = core
            .db
            .get_symbol_by_name("renamed_symbol", None)
            .unwrap()
            .into_iter()
            .map(|symbol| symbol.file)
            .collect::<Vec<_>>();
        assert_eq!(symbols, vec!["src/new.py"]);
    }
}
