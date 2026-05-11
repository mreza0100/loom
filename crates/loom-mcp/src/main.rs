mod server;

use anyhow::Result;
use clap::{Parser, Subcommand};
use loom_core::{store::LoomDb, LoomConfig};
use rmcp::ServiceExt;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "loom-mcp", about = "Loom MCP server and CLI")]
struct Cli {
    #[arg(long, env = "LOOM_TARGET_DIR", default_value = ".", global = true)]
    target: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Reindex,
    Status,
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
            let config = LoomConfig::load(cli.target)?;
            let db = LoomDb::open(config)?;
            println!("{}", serde_json::to_string_pretty(&db.get_stats()?)?);
        }
        Some(Command::Reindex) => {
            let state = server::LoomServerState::new(cli.target);
            let result = state.reindex()?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        None => {
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
