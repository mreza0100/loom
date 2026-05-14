mod server;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use loom_core::models::{
    CoupledHit, EvidencePackResponse, ImpactResponse, InspectResponse, NeighborhoodResponse,
    NextToolSuggestion, RelatedResponse, SearchResponse, SymbolHit, SymbolListResponse,
};
use rmcp::ServiceExt;
use serde::Serialize;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "loom-mcp", about = "Loom MCP server and CLI")]
struct Cli {
    #[arg(long, env = "LOOM_TARGET_DIR", default_value = ".", global = true)]
    target: PathBuf,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json, global = true)]
    format: OutputFormat,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Json,
    Text,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve,
    Reindex,
    Status,
    Search {
        query: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
        #[arg(long)]
        kind: Option<String>,
    },
    Symbols {
        query: String,
        #[arg(long)]
        file_prefix: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value_t = 24)]
        limit: usize,
    },
    Related {
        symbol: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        kind: Option<String>,
    },
    Impact {
        symbol: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        kind: Option<String>,
    },
    Neighborhood {
        file: String,
        line: i64,
    },
    Inspect {
        handle: String,
        #[arg(long, default_value_t = 24)]
        line_budget: usize,
        #[arg(long, default_value_t = 4_000)]
        char_budget: usize,
        #[arg(long, default_value_t = 0)]
        line_offset: usize,
    },
    EvidencePack {
        query: String,
        #[arg(long, default_value_t = 1_200)]
        budget_tokens: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Some(Command::Status) => {
            let state = server::LoomServerState::new(cli.target);
            let status = state.status()?;
            print_output(cli.format, &status, |_| {
                let mut output = format!(
                    "health={} warnings={} symbols={} edges={} resolved_edges={} unresolved_edges={} files={} stale_files={} callsites={} unresolved_callsites={} schema={} vector={} embedder={} degraded={} watcher_active={} auto_watch={}",
                    status.health.status,
                    status.health.warnings.len(),
                    status.stats.symbols,
                    status.stats.edges,
                    status.stats.resolved_edges,
                    status.stats.unresolved_edges,
                    status.stats.files,
                    status.stats.stale_files,
                    status.stats.callsites,
                    status.stats.unresolved_callsites,
                    status.schema_version,
                    status.vector_backend,
                    status.embedder_backend.as_deref().unwrap_or("unknown"),
                    status.embedder_degraded,
                    status.watcher_active,
                    status.auto_watch
                );
                if !status.health.warnings.is_empty() {
                    output.push_str("\nhealth_warnings:");
                    for warning in &status.health.warnings {
                        output.push_str(&format!("\n- {warning}"));
                    }
                }
                output
            })?;
        }
        Some(Command::Reindex) => {
            let state = server::LoomServerState::new(cli.target);
            let result = state.reindex()?;
            print_output(cli.format, &result, |_| {
                format!(
                    "indexed={} skipped={} deleted={} symbols={} edges={} facts={} callsites={} aliases={} role_cards={} embeddings={} resolved={} errors={} total_files={} total_symbols={} total_edges={} total_vectors={} stale_files={}",
                    result.indexed,
                    result.skipped,
                    result.deleted,
                    result.symbols,
                    result.edges,
                    result.behavior_facts,
                    result.callsites,
                    result.aliases,
                    result.role_cards,
                    result.embeddings,
                    result.resolved,
                    result.errors,
                    result.total_files,
                    result.total_symbols,
                    result.total_edges,
                    result.total_vectors,
                    result.stale_files
                )
            })?;
        }
        Some(Command::Search { query, limit, kind }) => {
            let state = server::LoomServerState::new(cli.target);
            let response = state.search(&query, limit, kind.as_deref())?;
            print_output(cli.format, &response, render_search)?;
        }
        Some(Command::Symbols {
            query,
            file_prefix,
            kind,
            limit,
        }) => {
            let state = server::LoomServerState::new(cli.target);
            let response = state.symbols(&query, file_prefix.as_deref(), kind.as_deref(), limit)?;
            print_output(cli.format, &response, render_symbols)?;
        }
        Some(Command::Related { symbol, file, kind }) => {
            let state = server::LoomServerState::new(cli.target);
            let response = state.related(&symbol, file.as_deref(), kind.as_deref())?;
            print_output(cli.format, &response, render_related)?;
        }
        Some(Command::Impact { symbol, file, kind }) => {
            let state = server::LoomServerState::new(cli.target);
            let response = state.impact(&symbol, file.as_deref(), kind.as_deref())?;
            print_output(cli.format, &response, render_impact)?;
        }
        Some(Command::Neighborhood { file, line }) => {
            let state = server::LoomServerState::new(cli.target);
            let response = state.neighborhood(&file, line)?;
            print_output(cli.format, &response, render_neighborhood)?;
        }
        Some(Command::Inspect {
            handle,
            line_budget,
            char_budget,
            line_offset,
        }) => {
            let state = server::LoomServerState::new(cli.target);
            let response = state.inspect(&handle, line_budget, char_budget, line_offset)?;
            print_output(cli.format, &response, render_inspect)?;
        }
        Some(Command::EvidencePack {
            query,
            budget_tokens,
        }) => {
            let state = server::LoomServerState::new(cli.target);
            let response = state.evidence_pack(&query, budget_tokens)?;
            print_output(cli.format, &response, render_evidence_pack)?;
        }
        Some(Command::Serve) | None => {
            let server = server::LoomMcpServer::new(cli.target)?;
            server
                .serve((tokio::io::stdin(), tokio::io::stdout()))
                .await?
                .waiting()
                .await?;
        }
    }
    Ok(())
}

fn print_output<T: Serialize>(
    format: OutputFormat,
    value: &T,
    render_text: impl FnOnce(&T) -> String,
) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(value)?),
        OutputFormat::Text => println!("{}", render_text(value)),
    }
    Ok(())
}

fn render_search(response: &SearchResponse) -> String {
    let result_count = response.exact_hits.len() + response.beyond_grep.len();
    let mut output = format!("search results={result_count}");
    if response.truncated {
        output.push_str(&format!(" more={}", response.budget.omitted));
    }
    if response.beyond_grep.is_empty() {
        append_symbol_section(&mut output, "results", &response.exact_hits);
    } else {
        append_symbol_section(&mut output, "exact_hits", &response.exact_hits);
        append_symbol_section(&mut output, "beyond_grep", &response.beyond_grep);
    }
    output
}

fn render_symbols(response: &SymbolListResponse) -> String {
    let mut output = format!("symbols results={}", response.results.len());
    if response.truncated {
        output.push_str(&format!(" more={}", response.budget.omitted));
    }
    append_symbol_section(&mut output, "results", &response.results);
    output
}

fn render_related(response: &RelatedResponse) -> String {
    let mut output = render_coupled_response(
        "related",
        &response.index_revision,
        response.truncated,
        &response.results,
    );
    append_next_suggestions(&mut output, &response.next_tool_suggestions);
    output
}

fn render_impact(response: &ImpactResponse) -> String {
    let mut output = render_coupled_response(
        "impact",
        &response.index_revision,
        response.truncated,
        &response.results,
    );
    append_next_suggestions(&mut output, &response.next_tool_suggestions);
    output
}

fn render_neighborhood(response: &NeighborhoodResponse) -> String {
    let mut output = format!(
        "neighborhood rev={} anchor={}:{} truncated={}",
        response.index_revision, response.file, response.line, response.truncated
    );
    if let Some(anchor) = &response.anchor {
        output.push_str(&format!(
            "\nanchor {} {} {}:{} handle={}",
            anchor.kind, anchor.name, anchor.anchor.file, anchor.anchor.line, anchor.handle
        ));
    }
    append_coupled_section(&mut output, "coupled", &response.coupled);
    append_next_suggestions(&mut output, &response.next_tool_suggestions);
    output
}

fn render_inspect(response: &InspectResponse) -> String {
    if let Some(error) = &response.error {
        return format!(
            "inspect rev={} handle_kind={} stale={} error={}",
            response.index_revision, response.handle_kind, response.stale, error
        );
    }
    let mut output = format!(
        "inspect rev={} handle_kind={} stale={} truncated={}",
        response.index_revision, response.handle_kind, response.stale, response.truncated
    );
    if let Some(snippet) = &response.snippet {
        output.push_str(&format!(
            "\n{}:{}-{}\n{}",
            snippet.anchor.file, snippet.start_line, snippet.end_line, snippet.text
        ));
    }
    if let Some(next) = response.page.next_line_offset {
        output.push_str(&format!(
            "\nnext: inspect {} --line-offset {}",
            response.handle, next
        ));
    }
    output
}

fn render_evidence_pack(response: &EvidencePackResponse) -> String {
    let mut output = format!(
        "{}\nrev={} truncated={} omitted={}",
        response.display_text,
        response.index_revision,
        response.truncated,
        response.omitted.len()
    );
    append_symbol_section(&mut output, "exact_hits", &response.exact_hits);
    append_symbol_section(&mut output, "beyond_grep", &response.beyond_grep);
    if !response.behavior_facts.is_empty() {
        output.push_str("\nfacts:");
        for hit in &response.behavior_facts {
            output.push_str(&format!(
                "\n- {}={} {}:{} handle={}",
                hit.fact.fact_type, hit.fact.value, hit.anchor.file, hit.anchor.line, hit.handle
            ));
        }
    }
    if !response.inspected_snippets.is_empty() {
        output.push_str("\nsnippets:");
        for snippet in &response.inspected_snippets {
            output.push_str(&format!(
                "\n- {}:{}-{} chars={}",
                snippet.anchor.file, snippet.start_line, snippet.end_line, snippet.chars
            ));
        }
    }
    if !response.missing_concepts.is_empty() {
        output.push_str("\nmissing:");
        for concept in &response.missing_concepts {
            output.push_str(&format!("\n- {concept}"));
        }
    }
    append_next_suggestions(&mut output, &response.next_tool_suggestions);
    output
}

fn render_coupled_response(
    label: &str,
    index_revision: &str,
    truncated: bool,
    results: &[CoupledHit],
) -> String {
    let mut output = format!(
        "{label} rev={index_revision} results={} truncated={truncated}",
        results.len()
    );
    append_coupled_section(&mut output, "results", results);
    output
}

fn append_symbol_section(output: &mut String, label: &str, hits: &[SymbolHit]) {
    output.push_str(&format!("\n{label}:"));
    if hits.is_empty() {
        output.push_str("\n- none");
        return;
    }
    for hit in hits {
        output.push_str(&format!(
            "\n- {} {} {}:{}",
            hit.kind, hit.name, hit.anchor.file, hit.anchor.line
        ));
        if let Some(evidence) = &hit.lexical_evidence {
            if let Some(location) = evidence_location(&evidence.reason) {
                output.push_str(&format!("\n  match {location}: {}", evidence.snippet));
            }
        }
    }
}

fn append_coupled_section(output: &mut String, label: &str, hits: &[CoupledHit]) {
    output.push_str(&format!("\n{label}:"));
    if hits.is_empty() {
        output.push_str("\n- none");
        return;
    }
    for hit in hits {
        output.push_str(&format!(
            "\n- {} {} {}:{} reason={}",
            hit.kind, hit.name, hit.anchor.file, hit.anchor.line, hit.reason
        ));
        if !hit.provenance.is_empty() {
            output.push_str(&format!(
                "\n   provenance={}",
                hit.provenance
                    .iter()
                    .take(3)
                    .map(|entry| format!(
                        "{}:{} depth={} confidence={:.2} source={}",
                        entry.relationship,
                        entry.direction,
                        entry.depth,
                        entry.confidence,
                        entry.source
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }
}

fn append_next_suggestions(output: &mut String, suggestions: &[NextToolSuggestion]) {
    if suggestions.is_empty() {
        return;
    }
    output.push_str("\nnext:");
    for suggestion in suggestions.iter().take(3) {
        output.push_str(&format!("\n- {}: {}", suggestion.tool, suggestion.reason));
    }
}

fn evidence_location(reason: &str) -> Option<&str> {
    reason.rsplit_once(" at ").map(|(_, location)| location)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_search_cli_with_global_target_and_format() {
        let cli = Cli::parse_from([
            "loom-mcp",
            "--target",
            "/tmp/project",
            "--format",
            "text",
            "search",
            "findProjectSpec",
            "--limit",
            "3",
            "--kind",
            "function",
        ]);
        assert_eq!(cli.target, PathBuf::from("/tmp/project"));
        assert_eq!(cli.format, OutputFormat::Text);
        let Some(Command::Search { query, limit, kind }) = cli.command else {
            panic!("expected search command");
        };
        assert_eq!(query, "findProjectSpec");
        assert_eq!(limit, 3);
        assert_eq!(kind.as_deref(), Some("function"));
    }

    #[test]
    fn parses_symbols_cli_with_filters() {
        let cli = Cli::parse_from([
            "loom-mcp",
            "symbols",
            "execute",
            "--file-prefix",
            "sources/commands",
            "--kind",
            "method",
            "--limit",
            "12",
        ]);
        let Some(Command::Symbols {
            query,
            file_prefix,
            kind,
            limit,
        }) = cli.command
        else {
            panic!("expected symbols command");
        };
        assert_eq!(query, "execute");
        assert_eq!(file_prefix.as_deref(), Some("sources/commands"));
        assert_eq!(kind.as_deref(), Some("method"));
        assert_eq!(limit, 12);
    }

    #[test]
    fn render_search_text_is_human_facing_not_score_dump() {
        let symbol = loom_core::models::Symbol {
            id: Some(1),
            name: "CacheCommand.execute".to_string(),
            kind: "method".to_string(),
            file: "sources/commands/Cache.ts".to_string(),
            line: 20,
            end_line: 22,
            language: "typescript".to_string(),
            context: "async execute() {}".to_string(),
        };
        let hit = SymbolHit {
            handle: "symbol:idx:1".to_string(),
            file_handle: "file:idx:cache".to_string(),
            rank: 1,
            name: "CacheCommand.execute".to_string(),
            kind: "method".to_string(),
            language: "typescript".to_string(),
            anchor: loom_core::models::FileAnchor {
                file: "sources/commands/Cache.ts".to_string(),
                line: 20,
                end_line: 22,
            },
            summary: "async execute() {".to_string(),
            symbol,
            score: 0.9,
            signal_scores: loom_core::models::SignalScores {
                lexical: 1.0,
                total: 0.9,
                ..Default::default()
            },
            reason_codes: vec!["exact:file_line".to_string()],
            lexical_evidence: Some(loom_core::models::LexicalEvidence {
                snippet: "async execute() {".to_string(),
                matched_text: "execute(".to_string(),
                rank: 0.0,
                field: "file_line".to_string(),
                reason: "exact file-line scan at sources/commands/Cache.ts:20".to_string(),
                match_kind: "exact_phrase".to_string(),
                sanitized_query: "execute(".to_string(),
            }),
            coupled: Vec::new(),
        };
        let response = SearchResponse {
            contract: "loom.search.response".to_string(),
            version: 1,
            index_revision: "idx".to_string(),
            limit: 10,
            truncated: false,
            inspect_required: true,
            budget: loom_core::models::ResponseBudget {
                unit: "results".to_string(),
                requested: 10,
                returned: 1,
                omitted: 0,
                truncated: false,
            },
            continuation: None,
            next_tool_suggestions: Vec::new(),
            query_intent: loom_core::models::QueryIntent {
                intent: "exact".to_string(),
                confidence: 1.0,
                reasons: Vec::new(),
            },
            exact_hits: vec![hit],
            beyond_grep: Vec::new(),
        };

        let rendered = render_search(&response);
        assert!(rendered.contains("- method CacheCommand.execute sources/commands/Cache.ts:20"));
        assert!(rendered.contains("match sources/commands/Cache.ts:20: async execute() {"));
        assert!(!rendered.contains("\n1."));
        assert!(!rendered.contains("score="));
        assert!(!rendered.contains("signals="));
        assert!(!rendered.contains("total="));
        assert!(!rendered.contains("handle="));
    }

    #[test]
    fn parses_default_server_mode() {
        let cli = Cli::parse_from(["loom-mcp"]);
        assert_eq!(cli.target, PathBuf::from("."));
        assert_eq!(cli.format, OutputFormat::Json);
        assert!(cli.command.is_none());
    }
}
