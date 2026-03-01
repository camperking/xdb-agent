use rig::agent::AgentBuilder;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::providers::openai;
use std::io::{self, Write};
use tools::{calculator, CounterState, GetCounter, GetCurrentTime, IncrementCounter, Multiply};

mod tools;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let counter_state = CounterState::new();

    let tools: Vec<Box<dyn rig::tool::ToolDyn>> = vec![
        Box::new(calculator::Add),
        Box::new(Multiply),
        Box::new(GetCurrentTime),
        Box::new(IncrementCounter::new(counter_state.clone())),
        Box::new(GetCounter::new(counter_state)),
    ];

    let client: openai::CompletionsClient = openai::CompletionsClient::builder()
        .api_key("")
        .base_url("http://localhost:8080")
        .build()
        .expect("Failed to build OpenAI client");

    let agent = AgentBuilder::new(client.completion_model("Qwen3.5-35B-A3B"))
        .name("Chat Agent")
        .preamble("You are a helpful assistant with access to tools. You can perform calculations, get the current time, and manage a counter. Use the tools provided to answer the user's question.")
        .default_max_turns(100)
        .tools(tools)
        .build();

    println!("Chat agent initialized. Type 'exit' to quit.");
    println!("Available tools: add, multiply, get_time, increment_counter, get_counter\n");

    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let input = input.trim();
        if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
            break;
        }

        if input.is_empty() {
            continue;
        }

        match agent.prompt(input).await {
            Ok(response) => {
                println!("{}", response);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    println!("Goodbye!");
    Ok(())
}