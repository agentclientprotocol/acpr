use acpr::Acpr;
use sacp::{Client, ConnectTo};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to auggie agent via sacp...");

    // Create an acpr agent that implements ConnectTo<Client>
    let agent = Acpr::new("auggie");

    // Connect as a client to the agent
    match Client
        .builder()
        .connect_with(agent, |_cx| async {
            println!("Connected to agent successfully!");
            // In a real application, you would send prompts and receive responses here
            Ok(())
        })
        .await
    {
        Ok(_) => println!("Client connection completed"),
        Err(e) => println!("Client connection failed: {}", e),
    }

    Ok(())
}
