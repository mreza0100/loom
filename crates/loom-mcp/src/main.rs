use anyhow::Result;
use loom_core::LoomConfig;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let target_dir = std::env::current_dir()?;
    let config = LoomConfig::load(target_dir)?;
    let db_path = config.resolve_db_path()?;
    info!(
        db_path = %db_path.display(),
        "loom-mcp Rust foundation shell initialized"
    );
    Ok(())
}
