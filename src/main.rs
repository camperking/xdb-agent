use rig::agent::AgentBuilder;
use rig::client::{CompletionClient, ProviderClient};
use rig::completion::Prompt;
use rig::providers::openai;
use std::env;
use std::io::{self, Write};
use tools::{calculator, CounterState, GetCounter, GetCurrentTime, IncrementCounter, Multiply};

mod tools;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");

    let counter_state = CounterState::new();

    let tools: Vec<Box<dyn rig::tool::ToolDyn>> = vec![
        Box::new(calculator::Add),
        Box::new(Multiply),
        Box::new(GetCurrentTime),
        Box::new(IncrementCounter::new(counter_state.clone())),
        Box::new(GetCounter::new(counter_state)),
    ];

    let client = openai::Client::from_env();

    let agent = AgentBuilder::new(client.completion_model(openai::GPT_4O_MINI))
        .name("Chat Agent")
        .preamble("You are a helpful assistant with access to tools. You can perform calculations, get the current time, and manage a counter. Use the tools provided to answer the user's question.")
        .default_max_turns(3)
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