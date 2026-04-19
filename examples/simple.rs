use acpr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Simple usage - run an agent directly
    println!("Running agent with simple interface...");
    match acpr::run("auggie").await {
        Ok(_) => println!("Agent completed successfully"),
        Err(e) => println!("Agent failed: {}", e),
    }

    // Builder pattern with configuration
    println!("\nRunning agent with builder pattern...");
    let agent = acpr::Acpr::new("auggie").with_cache_dir("/tmp/acpr_example".into());

    match agent.run().await {
        Ok(_) => println!("Configured agent completed successfully"),
        Err(e) => println!("Configured agent failed: {}", e),
    }

    Ok(())
}
