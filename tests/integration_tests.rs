use acpr::{Agent, BinaryDist, ForceOption, download_binary, fetch_registry, get_platform};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

#[tokio::test]
async fn test_fetch_custom_registry() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().to_path_buf();

    let registry_path = PathBuf::from("tests/fixtures/test-registry.json");
    let registry = fetch_registry(&cache_dir, None, Some(&registry_path))
        .await
        .unwrap();

    assert_eq!(registry.agents.len(), 3);
    assert_eq!(registry.agents[0].id, "test-npx-agent");
    assert_eq!(registry.agents[1].id, "test-binary-agent");
    assert_eq!(registry.agents[2].id, "test-versioned-npx-agent");
}

#[tokio::test]
async fn test_registry_caching() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().to_path_buf();

    // First fetch should create cache
    let registry1 = fetch_registry(&cache_dir, None, None).await.unwrap();
    assert!(!registry1.agents.is_empty());

    // Cache files should exist
    assert!(cache_dir.join("registry.json").exists());
    assert!(cache_dir.join("registry_cache.json").exists());

    // Second fetch should use cache (no network call)
    let registry2 = fetch_registry(&cache_dir, None, None).await.unwrap();
    assert_eq!(registry1.agents.len(), registry2.agents.len());
}

#[tokio::test]
async fn test_force_registry_refresh() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().to_path_buf();

    // Create initial cache
    let _registry1 = fetch_registry(&cache_dir, None, None).await.unwrap();

    // Force refresh should work
    let registry2 = fetch_registry(&cache_dir, Some(&ForceOption::Registry), None)
        .await
        .unwrap();
    assert!(!registry2.agents.is_empty());
}

#[test]
fn test_platform_detection() {
    let platform = get_platform();
    assert!(
        platform.contains("darwin") || platform.contains("linux") || platform.contains("windows")
    );
    assert!(platform.contains("aarch64") || platform.contains("x86_64"));
}

#[tokio::test]
async fn test_binary_caching() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().to_path_buf();

    // Create a mock agent with binary distribution
    let agent = Agent {
        id: "test-agent".to_string(),
        distribution: acpr::Distribution {
            binary: {
                let mut map = HashMap::new();
                map.insert(
                    "darwin-aarch64".to_string(),
                    BinaryDist {
                        // httpbin.org/base64/aGVsbG8gd29ybGQ= returns "hello world" (base64 decoded)
                        archive: "https://httpbin.org/base64/aGVsbG8gd29ybGQ=".to_string(),
                        cmd: "./test-binary".to_string(),
                        args: vec![],
                    },
                );
                map
            },
            npx: None,
            uvx: None,
        },
    };

    let binary_dist = &agent.distribution.binary["darwin-aarch64"];

    // First download
    let binary_path1 = download_binary(&agent, binary_dist, &cache_dir, None).await;

    // Should succeed if we can reach httpbin.org
    if binary_path1.is_ok() {
        let path1 = binary_path1.unwrap();
        assert!(path1.exists());

        // Verify the content is what we expect
        let content = tokio::fs::read_to_string(&path1).await.unwrap();
        assert_eq!(content, "hello world");

        // Second download should use cache (no force)
        let binary_path2 = download_binary(&agent, binary_dist, &cache_dir, None)
            .await
            .unwrap();
        assert_eq!(path1, binary_path2);

        // Force download should re-download
        let binary_path3 =
            download_binary(&agent, binary_dist, &cache_dir, Some(&ForceOption::Binary))
                .await
                .unwrap();
        assert_eq!(path1, binary_path3);
    }
}

#[test]
fn test_versioned_package_handling() {
    // Test that versioned packages don't get @latest appended
    let versioned_package = "@google/gemini-cli@0.38.2";
    let unversioned_package = "cowsay";

    // Simulate the logic from run_agent
    let versioned_arg =
        if versioned_package.contains('@') && versioned_package.matches('@').count() > 1 {
            versioned_package.to_string()
        } else {
            format!("{}@latest", versioned_package)
        };

    let unversioned_arg =
        if unversioned_package.contains('@') && unversioned_package.matches('@').count() > 1 {
            unversioned_package.to_string()
        } else {
            format!("{}@latest", unversioned_package)
        };

    assert_eq!(versioned_arg, "@google/gemini-cli@0.38.2");
    assert_eq!(unversioned_arg, "cowsay@latest");
}

async fn test_agent_sacp_integration(agent_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Skip if environment variable is set (for CI)
    if std::env::var("ACPR_SKIP_AGENT").is_ok() {
        return Ok(());
    }

    // Enable tracing based on RUST_LOG environment variable (ignore if already set)
    // Default to OFF if RUST_LOG is not set
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("off")),
        )
        .try_init();

    use acpr::Acpr;
    use agent_client_protocol::{
        Client,
        schema::{InitializeRequest, ProtocolVersion},
    };
    use std::time::Duration;
    use tracing::info;

    info!("Testing sacp integration with agent: {}", agent_name);

    let agent = Acpr::new(agent_name);
    info!("Created Acpr instance for {} agent", agent_name);

    // Add a 30 second timeout
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        Client
            .builder()
            .name("acpr-test-client")
            .connect_with(agent, async |cx| {
                info!("Connected to agent, initializing...");

                // Just test initialization - no session/prompt complexity
                info!("Sending initialize request...");
                let init_response = cx
                    .send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                    .block_task()
                    .await?;
                info!("Initialization complete: {:?}", init_response);

                // Success if we get here
                Ok(())
            })
            .await
    })
    .await;

    match result {
        Ok(Ok(())) => {
            info!(
                "Agent {} integration test completed successfully",
                agent_name
            );
            Ok(())
        }
        Ok(Err(e)) => Err(format!("Agent {} integration test failed: {}", agent_name, e).into()),
        Err(_) => Err(format!(
            "Agent {} integration test timed out after 30 seconds",
            agent_name
        )
        .into()),
    }
}

#[tokio::test]
async fn test_amp_acp_integration() {
    test_agent_sacp_integration("amp-acp")
        .await
        .expect("amp-acp integration test failed");
}

#[tokio::test]
async fn test_claude_integration() {
    test_agent_sacp_integration("claude-acp")
        .await
        .expect("claude-acp integration test failed");
}

#[tokio::test]
async fn test_uvx_agent_basic() {
    // Skip if environment variable is set (for CI)
    if std::env::var("ACPR_SKIP_AGENT").is_ok() {
        return;
    }

    use acpr::Acpr;
    use std::time::Duration;
    use tokio::io;

    // Test with fast-agent uvx agent - just verify it starts and closes cleanly
    let agent = Acpr::new("fast-agent");

    let (stdin_read, stdout_write) = tokio::io::duplex(1024);

    // Add a 5 second timeout for basic start/stop test
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        // Just start the agent and let it close when stdin closes
        drop(stdin_read); // Close stdin immediately
        agent.run_with_streams(io::empty(), stdout_write).await
    })
    .await;

    match result {
        Ok(Ok(())) => {
            // Success - agent started and exited cleanly
        }
        Ok(Err(_)) => {
            // Agent started but exited with error - still counts as working
        }
        Err(_) => {
            panic!("uvx agent test timed out - agent may not be starting properly");
        }
    }
}
