pub mod cli;
pub mod registry;

pub use cli::*;
pub use registry::*;

use agent_client_protocol::{
    AcpAgent, Agent as AcpAgentRole, Client, ConnectTo, Stdio,
    schema::{EnvVariable, McpServer, McpServerStdio},
};
use agent_client_protocol_conductor::{AgentOnly, ConductorImpl};
use std::ffi::OsString;
use std::path::PathBuf;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{debug, info};

/// A resolved command ready to be spawned, before any wrapping is applied.
#[derive(Debug, Clone)]
pub struct ResolvedCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub envs: Vec<(OsString, OsString)>,
}

/// Trait for transforming a resolved command before it is spawned.
///
/// Implement this to wrap agent processes in a sandbox or modify their
/// execution environment. A blanket impl is provided for all
/// `Fn(ResolvedCommand) -> ResolvedCommand`.
pub trait CommandWrapper {
    fn wrap(&self, cmd: ResolvedCommand) -> ResolvedCommand;
}

/// The default (no-op) command wrapper.
pub struct NullWrapper;

impl CommandWrapper for NullWrapper {
    fn wrap(&self, cmd: ResolvedCommand) -> ResolvedCommand {
        cmd
    }
}

impl<F: Fn(ResolvedCommand) -> ResolvedCommand> CommandWrapper for F {
    fn wrap(&self, cmd: ResolvedCommand) -> ResolvedCommand {
        self(cmd)
    }
}

/// Simple function to run an agent by name
pub async fn run(agent_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    Acpr::new(agent_name).run().await
}

/// Main library interface for acpr
pub struct Acpr<W: CommandWrapper = NullWrapper> {
    pub agent_name: String,
    cache_dir: Option<PathBuf>,
    registry_file: Option<PathBuf>,
    force: Option<ForceOption>,
    command_wrapper: W,
}

impl Acpr {
    /// Create a new Acpr instance for the specified agent
    pub fn new(agent_name: &str) -> Self {
        Self {
            agent_name: agent_name.to_string(),
            cache_dir: None,
            registry_file: None,
            force: None,
            command_wrapper: NullWrapper,
        }
    }
}

impl<W: CommandWrapper> Acpr<W> {
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

    /// Set a command wrapper that transforms the resolved command before it is spawned.
    ///
    /// The wrapper receives the fully-resolved command (after registry lookup, binary download,
    /// and args applied) and returns a modified command. This is useful for wrapping the agent
    /// process in a sandbox (e.g., bubblewrap/bwrap).
    pub fn with_command_wrapper<F: CommandWrapper>(self, wrapper: F) -> Acpr<F> {
        Acpr {
            agent_name: self.agent_name,
            cache_dir: self.cache_dir,
            registry_file: self.registry_file,
            force: self.force,
            command_wrapper: wrapper,
        }
    }

    /// Resolve the registry and build the command for this agent,
    /// applying the command wrapper.
    async fn resolve_agent(&self) -> Result<AcpAgent, Box<dyn std::error::Error>> {
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

        debug!("Resolving agent: {}", agent.id);

        let resolved = self.resolve_command(agent, &cache_dir).await?;
        let resolved = self.command_wrapper.wrap(resolved);

        let args: Vec<String> = resolved
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let envs: Vec<EnvVariable> = resolved
            .envs
            .iter()
            .map(|(k, v)| {
                EnvVariable::new(
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                )
            })
            .collect();

        let command = resolved.program.to_string_lossy().into_owned();
        let mcp_server = McpServerStdio::new(&self.agent_name, &command)
            .args(args)
            .env(envs);
        let acp_agent = AcpAgent::new(McpServer::Stdio(mcp_server));

        Ok(acp_agent)
    }

    /// Run the agent with default stdio, using the Conductor for protocol handling.
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let acp_agent = self.resolve_agent().await?;
        let conductor = ConductorImpl::new_agent(&self.agent_name, AgentOnly(acp_agent));
        conductor
            .run(Stdio::new())
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Run the agent with custom stdio streams, using the Conductor for protocol handling.
    pub async fn run_with_streams<R, Wr>(
        &self,
        stdin: R,
        stdout: Wr,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        R: AsyncRead + Unpin + Send + 'static,
        Wr: AsyncWrite + Unpin + Send + 'static,
    {
        let acp_agent = self.resolve_agent().await?;
        let conductor = ConductorImpl::new_agent(&self.agent_name, AgentOnly(acp_agent));

        let byte_streams = agent_client_protocol::ByteStreams::new(
            tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(stdout),
            tokio_util::compat::TokioAsyncReadCompatExt::compat(stdin),
        );
        conductor
            .run(byte_streams)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn resolve_command(
        &self,
        agent: &Agent,
        cache_dir: &PathBuf,
    ) -> Result<ResolvedCommand, Box<dyn std::error::Error>> {
        if let Some(npx) = &agent.distribution.npx {
            info!("Executing npx package: {}", npx.package);
            let package_arg = if npx.package.contains('@') && npx.package.matches('@').count() > 1 {
                npx.package.clone()
            } else {
                format!("{}@latest", npx.package)
            };
            let mut args: Vec<OsString> = vec!["-y".into(), package_arg.into()];
            args.extend(npx.args.iter().map(OsString::from));
            Ok(ResolvedCommand {
                program: "npx".into(),
                args,
                envs: vec![],
            })
        } else if let Some(uvx) = &agent.distribution.uvx {
            info!("Executing uvx package: {}", uvx.package);
            let mut args: Vec<OsString> = vec![uvx.package.clone().into()];
            args.extend(uvx.args.iter().map(OsString::from));
            Ok(ResolvedCommand {
                program: "uvx".into(),
                args,
                envs: vec![],
            })
        } else if !agent.distribution.binary.is_empty() {
            let platform = get_platform();
            debug!("Platform detected: {}", platform);
            if let Some(binary_dist) = agent.distribution.binary.get(&platform) {
                let binary_path =
                    download_binary(agent, binary_dist, cache_dir, self.force.as_ref()).await?;
                info!("Executing binary: {:?}", binary_path);
                let args: Vec<OsString> = binary_dist.args.iter().map(OsString::from).collect();
                Ok(ResolvedCommand {
                    program: binary_path.into_os_string(),
                    args,
                    envs: vec![],
                })
            } else {
                Err(format!("No binary available for platform: {}", platform).into())
            }
        } else {
            Err("No supported distribution method found".into())
        }
    }
}

/// Implement ConnectTo<Client> so Acpr can act as an ACP agent via Conductor
impl<W: CommandWrapper + Send + Sync + 'static> ConnectTo<Client> for Acpr<W> {
    async fn connect_to(
        self,
        client: impl ConnectTo<AcpAgentRole>,
    ) -> Result<(), agent_client_protocol::Error> {
        let acp_agent = self
            .resolve_agent()
            .await
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;

        let conductor = ConductorImpl::new_agent(&self.agent_name, AgentOnly(acp_agent));
        conductor.run(client).await
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
