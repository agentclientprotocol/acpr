use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use crate::cli::ForceOption;
use anstyle::{Color, Style};
use tracing::{debug, info};

#[derive(Deserialize)]
pub struct Registry {
    pub agents: Vec<Agent>,
}

#[derive(Deserialize)]
pub struct Agent {
    pub id: String,
    pub distribution: Distribution,
}

#[derive(Deserialize)]
pub struct Distribution {
    #[serde(default)]
    pub binary: HashMap<String, BinaryDist>,
    #[serde(default)]
    pub npx: Option<NpxDist>,
    #[serde(default)]
    pub uvx: Option<UvxDist>,
}

#[derive(Deserialize)]
pub struct BinaryDist {
    pub archive: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Deserialize)]
pub struct NpxDist {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Deserialize)]
pub struct UvxDist {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct CacheInfo {
    pub timestamp: u64,
    pub version: String,
}

pub async fn fetch_registry(cache_dir: &PathBuf, force: Option<&ForceOption>, registry_file: Option<&PathBuf>) -> Result<Registry, Box<dyn std::error::Error>> {
    if let Some(file_path) = registry_file {
        debug!("Using custom registry file: {:?}", file_path);
        let registry_content = tokio::fs::read_to_string(file_path).await?;
        return Ok(serde_json::from_str(&registry_content)?);
    }
    
    let registry_file = cache_dir.join("registry.json");
    let cache_info_file = cache_dir.join("registry_cache.json");
    
    let should_fetch = match force {
        Some(ForceOption::All | ForceOption::Registry) => {
            debug!("Force refresh requested for registry");
            true
        },
        _ => {
            if let Ok(info_content) = tokio::fs::read_to_string(&cache_info_file).await {
                if let Ok(cache_info) = serde_json::from_str::<CacheInfo>(&info_content) {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs();
                    let age_hours = (now - cache_info.timestamp) / 3600;
                    debug!("Registry cache age: {} hours", age_hours);
                    now - cache_info.timestamp > 3 * 3600 // 3 hours
                } else { 
                    debug!("Invalid cache info file, will fetch");
                    true 
                }
            } else { 
                debug!("No cache info file found, will fetch");
                true 
            }
        }
    };
    
    if should_fetch {
        info!("Fetching registry from ACP...");
        let response = reqwest::get("https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json").await?;
        let registry_content = response.text().await?;
        debug!("Writing registry to cache: {:?}", registry_file);
        tokio::fs::write(&registry_file, &registry_content).await?;
        
        let cache_info = CacheInfo {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            version: "1.0.0".to_string(),
        };
        tokio::fs::write(&cache_info_file, serde_json::to_string(&cache_info)?).await?;
        debug!("Registry cache updated");
    } else {
        debug!("Using cached registry: {:?}", registry_file);
    }
    
    let registry_content = tokio::fs::read_to_string(&registry_file).await?;
    let registry: Registry = serde_json::from_str(&registry_content)?;
    debug!("Loaded {} agents from registry", registry.agents.len());
    Ok(registry)
}

pub fn list_agents(registry: &Registry) {
    let header_style = Style::new().fg_color(Some(Color::Ansi(anstyle::AnsiColor::Cyan))).bold();
    let name_style = Style::new().fg_color(Some(Color::Ansi(anstyle::AnsiColor::Green)));
    let desc_style = Style::new().fg_color(Some(Color::Ansi(anstyle::AnsiColor::White)));
    
    println!("{header_style}Available ACP Agents:{header_style:#}");
    println!();
    
    for agent in &registry.agents {
        let dist_types = get_distribution_types(&agent.distribution);
        println!("{name_style}{}{name_style:#} {desc_style}({}){desc_style:#}", 
                 agent.id, 
                 dist_types.join(", "));
    }
}

fn get_distribution_types(dist: &Distribution) -> Vec<&'static str> {
    let mut types = Vec::new();
    if !dist.binary.is_empty() { types.push("binary"); }
    if dist.npx.is_some() { types.push("npx"); }
    if dist.uvx.is_some() { types.push("uvx"); }
    types
}