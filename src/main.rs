use clap::Parser;
use acpr::{Cli, fetch_registry, run_agent, list_agents};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    if cli.debug {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_max_level(tracing::Level::DEBUG)
            .init();
    }
    
    let cache_dir = cli.cache_dir.unwrap_or_else(|| {
        dirs::cache_dir()
            .expect("No cache directory found")
            .join("acpr")
    });
    
    tokio::fs::create_dir_all(&cache_dir).await?;
    
    let registry = fetch_registry(&cache_dir, cli.force.as_ref(), cli.registry.as_ref()).await?;
    
    if cli.list {
        list_agents(&registry);
        return Ok(());
    }
    
    let agent_name = cli.agent_name.ok_or("Agent name is required when not using --list")?;
    let agent = registry.agents.iter()
        .find(|a| a.id == agent_name)
        .ok_or("Agent not found")?;
    
    run_agent(agent, &cache_dir, cli.force.as_ref()).await?;
    Ok(())
}