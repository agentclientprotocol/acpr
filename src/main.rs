use acpr::{Acpr, Cli, fetch_registry, list_agents};
use clap::Parser;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Cli {
        agent_name,
        cache_dir,
        registry,
        force,
        list,
        debug,
    } = Cli::parse();

    if debug {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_max_level(tracing::Level::DEBUG)
            .init();
    }

    if list {
        let cache_dir = cache_dir.unwrap_or_else(|| {
            dirs::cache_dir()
                .expect("No cache directory found")
                .join("acpr")
        });
        tokio::fs::create_dir_all(&cache_dir).await?;
        let registry = fetch_registry(&cache_dir, force.as_ref(), registry.as_ref()).await?;
        list_agents(&registry);
        return Ok(());
    }

    let agent_name = agent_name.ok_or("Agent name is required when not using --list")?;

    // Use Acpr as the base API
    let mut acpr = Acpr::new(&agent_name);

    if let Some(cache_dir) = cache_dir {
        acpr = acpr.with_cache_dir(cache_dir);
    }

    if let Some(registry_file) = registry {
        acpr = acpr.with_registry_file(registry_file);
    }

    if let Some(force) = force {
        acpr = acpr.with_force(force);
    }

    acpr.run().await?;
    Ok(())
}
