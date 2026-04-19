pub mod cli;
pub mod registry;

pub use cli::*;
pub use registry::*;

use sacp::{Agent as SacpAgent, ByteStreams, Client, ConnectTo};
use std::path::PathBuf;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::Command;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, info};

/// Simple function to run an agent by name
pub async fn run(agent_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    Acpr::new(agent_name).run().await
}

/// Main library interface for acpr
pub struct Acpr {
    pub agent_name: String,
    cache_dir: Option<PathBuf>,
    registry_file: Option<PathBuf>,
    force: Option<ForceOption>,
}

impl Acpr {
    /// Create a new Acpr instance for the specified agent
    pub fn new(agent_name: &str) -> Self {
        Self {
            agent_name: agent_name.to_string(),
            cache_dir: None,
            registry_file: None,
            force: None,
        }
    }

    /// Set a custom cache directory
    pub fn with_cache_dir(mut self, cache_dir: PathBuf) -> Self {
        self.cache_dir = Some(cache_dir);
        self
    }

    /// Set a custom registry file
    pub fn with_registry_file(mut self, registry_file: PathBuf) -> Self {
        self.registry_file = Some(registry_file);
        self
    }

    /// Set force option
    pub fn with_force(mut self, force: ForceOption) -> Self {
        self.force = Some(force);
        self
    }

    /// Run the agent with default stdio
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.run_with_stdio(tokio::io::stdin(), tokio::io::stdout())
            .await
    }

    /// Run the agent with custom stdio streams
    pub async fn run_with_stdio<R, W>(
        &self,
        stdin: R,
        stdout: W,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let cache_dir = self.cache_dir.clone().unwrap_or_else(|| {
            dirs::cache_dir()
                .expect("No cache directory found")
                .join("acpr")
        });

        tokio::fs::create_dir_all(&cache_dir).await?;
        let registry =
            fetch_registry(&cache_dir, self.force.as_ref(), self.registry_file.as_ref()).await?;
        let agent = registry
            .agents
            .iter()
            .find(|a| a.id == self.agent_name)
            .ok_or("Agent not found")?;

        debug!("Running agent: {}", agent.id);

        let mut cmd = self.build_command(agent, &cache_dir).await?;
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        let mut child = cmd.spawn()?;
        let child_stdin = child.stdin.take().unwrap();
        let child_stdout = child.stdout.take().unwrap();

        let stdin_task = tokio::spawn(async move {
            let mut stdin = stdin;
            let mut child_stdin = child_stdin;
            tokio::io::copy(&mut stdin, &mut child_stdin).await
        });

        let stdout_task = tokio::spawn(async move {
            let mut child_stdout = child_stdout;
            let mut stdout = stdout;
            tokio::io::copy(&mut child_stdout, &mut stdout).await
        });

        let (status_result, stdin_result, stdout_result) =
            tokio::join!(child.wait(), stdin_task, stdout_task);
        let status = status_result?;
        stdin_result.map_err(|e| format!("stdin task failed: {}", e))??;
        stdout_result.map_err(|e| format!("stdout task failed: {}", e))??;

        if !status.success() {
            return Err(format!("Process exited with status: {}", status).into());
        }
        Ok(())
    }

    async fn build_command(
        &self,
        agent: &Agent,
        cache_dir: &PathBuf,
    ) -> Result<Command, Box<dyn std::error::Error>> {
        if let Some(npx) = &agent.distribution.npx {
            info!("Executing npx package: {}", npx.package);
            let mut cmd = Command::new("npx");
            cmd.arg("-y");
            let package_arg = if npx.package.contains('@') && npx.package.matches('@').count() > 1 {
                npx.package.clone()
            } else {
                format!("{}@latest", npx.package)
            };
            cmd.arg(package_arg).args(&npx.args);
            Ok(cmd)
        } else if let Some(uvx) = &agent.distribution.uvx {
            info!("Executing uvx package: {}", uvx.package);
            let mut cmd = Command::new("uvx");
            cmd.arg(format!("{}@latest", uvx.package)).args(&uvx.args);
            Ok(cmd)
        } else if !agent.distribution.binary.is_empty() {
            let platform = get_platform();
            debug!("Platform detected: {}", platform);
            if let Some(binary_dist) = agent.distribution.binary.get(&platform) {
                let binary_path =
                    download_binary(agent, binary_dist, cache_dir, self.force.as_ref()).await?;
                info!("Executing binary: {:?}", binary_path);
                let mut cmd = Command::new(&binary_path);
                cmd.args(&binary_dist.args);
                Ok(cmd)
            } else {
                Err(format!("No binary available for platform: {}", platform).into())
            }
        } else {
            Err("No supported distribution method found".into())
        }
    }
}

/// Implement ConnectTo<Client> so Acpr can act as an ACP agent
impl ConnectTo<Client> for Acpr {
    async fn connect_to(self, client: impl ConnectTo<SacpAgent>) -> Result<(), sacp::Error> {
        let (client_stdin, agent_stdout) = tokio::io::duplex(8192);
        let (agent_stdin, client_stdout) = tokio::io::duplex(8192);

        let agent_task = tokio::spawn(async move {
            self.run_with_stdio(agent_stdin, agent_stdout)
                .await
                .map_err(|e| sacp::Error::internal_error().data(e.to_string()))
        });

        let byte_streams = ByteStreams::new(client_stdin.compat_write(), client_stdout.compat());
        let client_task = ConnectTo::<Client>::connect_to(byte_streams, client);

        tokio::try_join!(
            async {
                agent_task
                    .await
                    .map_err(|e| sacp::Error::internal_error().data(e.to_string()))?
            },
            client_task
        )?;

        Ok(())
    }
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
    }
    .to_string()
}

pub async fn download_binary(
    agent: &Agent,
    binary_dist: &BinaryDist,
    cache_dir: &PathBuf,
    force: Option<&ForceOption>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let agent_cache_dir = cache_dir.join(&agent.id);
    tokio::fs::create_dir_all(&agent_cache_dir).await?;
    let binary_name = binary_dist.cmd.trim_start_matches("./");
    let binary_path = agent_cache_dir.join(binary_name);

    let should_download = match force {
        Some(ForceOption::All | ForceOption::Binary) => {
            debug!("Force download requested for binary");
            true
        }
        _ => {
            let exists = binary_path.exists();
            debug!("Binary exists at {:?}: {}", binary_path, exists);
            !exists
        }
    };

    if should_download {
        info!("Downloading binary from: {}", binary_dist.archive);
        let response = reqwest::get(&binary_dist.archive).await?;
        let archive_data = response.bytes().await?;
        debug!("Downloaded {} bytes", archive_data.len());

        if binary_dist.archive.ends_with(".zip") {
            debug!("Extracting zip archive");
            extract_zip(&archive_data, &agent_cache_dir).await?;
        } else if binary_dist.archive.ends_with(".tar.gz") || binary_dist.archive.ends_with(".tgz")
        {
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
    })
    .await
    .map_err(|e| e.to_string())??;
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
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(())
}
