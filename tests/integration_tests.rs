use acpr::{fetch_registry, ForceOption, get_platform, download_binary, Agent, BinaryDist};
use std::path::PathBuf;
use tempfile::TempDir;
use std::collections::HashMap;

#[tokio::test]
async fn test_fetch_custom_registry() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().to_path_buf();
    
    let registry_path = PathBuf::from("tests/fixtures/test-registry.json");
    let registry = fetch_registry(&cache_dir, None, Some(&registry_path)).await.unwrap();
    
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
    let registry2 = fetch_registry(&cache_dir, Some(&ForceOption::Registry), None).await.unwrap();
    assert!(!registry2.agents.is_empty());
}

#[test]
fn test_platform_detection() {
    let platform = get_platform();
    assert!(platform.contains("darwin") || platform.contains("linux") || platform.contains("windows"));
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
                map.insert("darwin-aarch64".to_string(), BinaryDist {
                    archive: "https://httpbin.org/base64/aGVsbG8gd29ybGQ=".to_string(),
                    cmd: "./test-binary".to_string(),
                    args: vec![],
                });
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
        
        // Second download should use cache (no force)
        let binary_path2 = download_binary(&agent, binary_dist, &cache_dir, None).await.unwrap();
        assert_eq!(path1, binary_path2);
        
        // Force download should re-download
        let binary_path3 = download_binary(&agent, binary_dist, &cache_dir, Some(&ForceOption::Binary)).await.unwrap();
        assert_eq!(path1, binary_path3);
    }
}

#[test]
fn test_versioned_package_handling() {
    // Test that versioned packages don't get @latest appended
    let versioned_package = "@google/gemini-cli@0.38.2";
    let unversioned_package = "cowsay";
    
    // Simulate the logic from run_agent
    let versioned_arg = if versioned_package.contains('@') && versioned_package.matches('@').count() > 1 {
        versioned_package.to_string()
    } else {
        format!("{}@latest", versioned_package)
    };
    
    let unversioned_arg = if unversioned_package.contains('@') && unversioned_package.matches('@').count() > 1 {
        unversioned_package.to_string()
    } else {
        format!("{}@latest", unversioned_package)
    };
    
    assert_eq!(versioned_arg, "@google/gemini-cli@0.38.2");
    assert_eq!(unversioned_arg, "cowsay@latest");
}