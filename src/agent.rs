use rig::agent::AgentBuilder;
use rig::client::CompletionClient;
use rig::completion::{Chat, Message, PromptError};
use rig::providers::openai;

use crate::tools::{
    calculator, CounterState, GetCounter, GetCurrentTime, IncrementCounter, Multiply, ReadFile,
};

pub struct Agent<A: Chat> {
    inner: A,
    history: Vec<Message>,
}

impl<A: Chat> Agent<A> {
    pub fn new(inner: A) -> Self {
        Self {
            inner,
            history: Vec::new(),
        }
    }

    pub async fn chat(&mut self, input: &str) -> Result<String, PromptError> {
        let response = self.inner.chat(input, self.history.clone()).await?;
        self.history.push(Message::user(input));
        self.history.push(Message::assistant(response.clone()));
        Ok(response)
    }
}

fn build_tools() -> Vec<Box<dyn rig::tool::ToolDyn>> {
    let counter_state = CounterState::new();
    vec![
        Box::new(calculator::Add),
        Box::new(Multiply),
        Box::new(GetCurrentTime),
        Box::new(IncrementCounter::new(counter_state.clone())),
        Box::new(GetCounter::new(counter_state.clone())),
        Box::new(ReadFile),
    ]
}

pub fn initialize() -> Agent<impl Chat> {
    let client: openai::CompletionsClient = openai::CompletionsClient::builder()
        .api_key("")
        .base_url("http://localhost:8080")
        .build()
        .expect("Failed to build OpenAI client");

    let inner = AgentBuilder::new(client.completion_model("Qwen3.5-35B-A3B"))
        .name("Chat Agent")
        .preamble(
            "You are a helpful assistant with access to tools. \
             You can perform calculations, get the current time, and manage a counter. \
             Use the tools provided to answer the user's question.",
        )
        .default_max_turns(100)
        .tools(build_tools())
        .build();

    Agent::new(inner)
}