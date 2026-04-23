use anyhow::Result;
use rig::client::{CompletionClient, ProviderClient};
use rig::completion::Prompt;
use rig::providers::deepseek;

const PREAMBLE: &str = "You are a helpful assistant. Be concise and clear in your responses.";

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    // Create Deepseek client from environment variable DEEPSEEK_API_KEY
    let client = deepseek::Client::from_env();

    // Build a simple agent with a system prompt
    let agent = client
        .agent(deepseek::DEEPSEEK_CHAT)
        .preamble(PREAMBLE)
        .build();

    // Prompt the agent and print the response
    let response = agent.prompt("Hello! What can you help me with?").await?;
    println!("{response}");

    Ok(())
}
