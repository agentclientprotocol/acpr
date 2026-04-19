use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "acpr")]
pub struct Cli {
    pub agent_name: Option<String>,
    #[arg(long)]
    pub force: Option<ForceOption>,
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,
    #[arg(long)]
    pub registry: Option<PathBuf>,
    #[arg(long)]
    pub list: bool,
    #[arg(long)]
    pub debug: bool,
}

#[derive(ValueEnum, Clone)]
pub enum ForceOption {
    All,
    Registry,
    Binary,
}