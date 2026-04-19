use acpr::Acpr;
use sacp::{Client, DynConnectTo};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create multiple agents that can be used in sacp ecosystem
    let agents: Vec<DynConnectTo<Client>> = vec![
        DynConnectTo::new(Acpr::new("auggie")),
        DynConnectTo::new(Acpr::new("cline")),
    ];

    println!(
        "Created {} agents implementing ConnectTo<Client>",
        agents.len()
    );

    // Example: Use first agent directly
    let agent = Acpr::new("auggie");
    println!("Agent name: {}", agent.agent_name);

    // In a real sacp application, you would connect these to clients:
    // Client.builder().connect_to(agent).await?;

    println!("Agents ready for sacp integration");
    Ok(())
}
