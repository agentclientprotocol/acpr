use acpr::Acpr;
use sacp::{ByteStreams, Client, ConnectTo};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Demonstrating ByteStreams integration...");

    // Example 1: Using Acpr directly (it handles ByteStreams internally)
    let agent = Acpr::new("auggie");
    println!("Created agent: {}", agent.agent_name);

    // Example 2: Manual ByteStreams creation (for custom stdio handling)
    let (stdin_read, stdin_write) = tokio::io::duplex(1024);
    let (stdout_read, stdout_write) = tokio::io::duplex(1024);

    let byte_streams = ByteStreams::new(stdout_write.compat_write(), stdin_read.compat());

    println!("Created ByteStreams from custom stdio");

    // In a real application, you would connect these:
    // ConnectTo::<Client>::connect_to(byte_streams, client).await?;

    println!("ByteStreams ready for sacp communication");
    Ok(())
}
