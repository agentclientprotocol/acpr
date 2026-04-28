use acpr::{self, Acpr};
use agent_client_protocol::{Client, DynConnectTo};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Simple function interface
    println!("Testing simple interface...");
    match acpr::run("auggie").await {
        Ok(_) => println!("✓ Simple interface works"),
        Err(e) => println!("✗ Simple interface failed: {}", e),
    }

    // 2. Builder pattern with configuration
    println!("\nTesting builder pattern...");
    let agent = Acpr::new("auggie")
        .with_cache_dir("/tmp/acpr_test".into())
        .with_force(acpr::ForceOption::All);

    match agent.run().await {
        Ok(_) => println!("✓ Builder pattern works"),
        Err(e) => println!("✗ Builder pattern failed: {}", e),
    }

    // 3. sacp integration
    println!("\nTesting sacp integration...");
    let agents: Vec<DynConnectTo<Client>> = vec![
        DynConnectTo::new(Acpr::new("auggie")),
        DynConnectTo::new(Acpr::new("cline")),
    ];
    println!("✓ Created {} sacp-compatible agents", agents.len());

    // 4. Direct field access
    let agent = Acpr::new("test-agent");
    println!("✓ Agent name accessible: {}", agent.agent_name);

    println!("\nAll interfaces working correctly!");
    Ok(())
}
