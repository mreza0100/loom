use loom_core::{
    embedder::CandleEmbedder, graph::SymbolGraph, indexer::IndexPipeline, models::StoreStats,
    store::LoomDb, IndexResult, LoomConfig, SearchEngine,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Content, ErrorData, Implementation, InitializeResult, ServerCapabilities},
    tool, tool_handler, tool_router, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::error;

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
    embedder: Mutex<Option<Arc<CandleEmbedder>>>,
    reindex_lock: Mutex<()>,
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
        Ok(StatusResponse {
            stats: core.db.get_stats()?,
            graph_nodes: graph.node_count(),
            graph_edges: graph.edge_count(),
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

    fn search_engine(&self) -> loom_core::Result<SearchEngine<CandleEmbedder>> {
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
        });
        *guard = Some(Arc::clone(&core));
        Ok(core)
    }
}

impl CoreState {
    fn embedder(&self) -> loom_core::Result<Arc<CandleEmbedder>> {
        let mut guard = self.lock_embedder();
        if let Some(embedder) = guard.as_ref() {
            return Ok(Arc::clone(embedder));
        }
        let embedder = Arc::new(CandleEmbedder::from_config(&self.config)?);
        *guard = Some(Arc::clone(&embedder));
        Ok(embedder)
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

    fn lock_embedder(&self) -> std::sync::MutexGuard<'_, Option<Arc<CandleEmbedder>>> {
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
        let state = LoomServerState::new(temp.path().to_path_buf());
        let status = state.status().unwrap();
        assert_eq!(status.stats.symbols, 0);
        assert_eq!(status.graph_nodes, 0);
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
}
