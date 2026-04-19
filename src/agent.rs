use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use crate::cli::ForceOption;
use crate::registry::{Agent, BinaryDist};
use tracing::{debug, info};

pub async fn run_agent(agent: &Agent, cache_dir: &PathBuf, force: Option<&ForceOption>) -> Result<(), Box<dyn std::error::Error>> {
    debug!("Running agent: {}", agent.id);
    
    if let Some(npx) = &agent.distribution.npx {
        info!("Executing npx package: {}", npx.package);
        let mut cmd = Command::new("npx");
        cmd.arg("-y");
        let package_arg = if npx.package.contains('@') && npx.package.matches('@').count() > 1 {
            npx.package.clone()
        } else {
            format!("{}@latest", npx.package)
        };
        cmd.arg(package_arg);
        cmd.args(&npx.args);
        cmd.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
        cmd.status().await?;
    } else if let Some(uvx) = &agent.distribution.uvx {
        info!("Executing uvx package: {}", uvx.package);
        let mut cmd = Command::new("uvx");
        cmd.arg(format!("{}@latest", uvx.package));
        cmd.args(&uvx.args);
        cmd.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
        cmd.status().await?;
    } else if !agent.distribution.binary.is_empty() {
        let platform = get_platform();
        debug!("Platform detected: {}", platform);
        if let Some(binary_dist) = agent.distribution.binary.get(&platform) {
            let binary_path = download_binary(agent, binary_dist, cache_dir, force).await?;
            info!("Executing binary: {:?}", binary_path);
            let mut cmd = Command::new(&binary_path);
            cmd.args(&binary_dist.args);
            cmd.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
            cmd.status().await?;
        } else {
            return Err(format!("No binary available for platform: {}", platform).into());
        }
    } else {
        return Err("No supported distribution method found".into());
    }
    Ok(())
}

pub fn get_platform() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => "unknown",
    }.to_string()
}

pub async fn download_binary(agent: &Agent, binary_dist: &BinaryDist, cache_dir: &PathBuf, force: Option<&ForceOption>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let agent_cache_dir = cache_dir.join(&agent.id);
    tokio::fs::create_dir_all(&agent_cache_dir).await?;
    
    let binary_name = binary_dist.cmd.trim_start_matches("./");
    let binary_path = agent_cache_dir.join(binary_name);
    
    let should_download = match force {
        Some(ForceOption::All | ForceOption::Binary) => {
            debug!("Force download requested for binary");
            true
        },
        _ => {
            let exists = binary_path.exists();
            debug!("Binary exists at {:?}: {}", binary_path, exists);
            !exists
        },
    };
    
    if should_download {
        info!("Downloading binary from: {}", binary_dist.archive);
        let response = reqwest::get(&binary_dist.archive).await?;
        let archive_data = response.bytes().await?;
        debug!("Downloaded {} bytes", archive_data.len());
        
        if binary_dist.archive.ends_with(".zip") {
            debug!("Extracting zip archive");
            extract_zip(&archive_data, &agent_cache_dir).await?;
        } else if binary_dist.archive.ends_with(".tar.gz") || binary_dist.archive.ends_with(".tgz") {
            debug!("Extracting tar.gz archive");
            extract_tar_gz(&archive_data, &agent_cache_dir).await?;
        } else {
            debug!("Writing raw binary");
            tokio::fs::write(&binary_path, &archive_data).await?;
        }
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = tokio::fs::metadata(&binary_path).await?.permissions();
            perms.set_mode(0o755);
            tokio::fs::set_permissions(&binary_path, perms).await?;
            debug!("Set executable permissions on binary");
        }
        
        info!("Binary ready at: {:?}", binary_path);
    } else {
        debug!("Using cached binary: {:?}", binary_path);
    }
    
    Ok(binary_path)
}

async fn extract_zip(data: &[u8], dest: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let data = data.to_vec();
    let dest = dest.clone();
    
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let cursor = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
        
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
            let outpath = dest.join(file.name());
            
            if file.is_dir() {
                std::fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
            } else {
                if let Some(parent) = outpath.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let mut outfile = std::fs::File::create(&outpath).map_err(|e| e.to_string())?;
                std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }).await.map_err(|e| e.to_string())??;
    
    Ok(())
}

async fn extract_tar_gz(data: &[u8], dest: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let data = data.to_vec();
    let dest = dest.clone();
    
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let decoder = flate2::read::GzDecoder::new(&data[..]);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(&dest).map_err(|e| e.to_string())?;
        Ok(())
    }).await.map_err(|e| e.to_string())??;
    
    Ok(())
}