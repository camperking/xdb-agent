use rig::agent::AgentBuilder;
use rig::client::CompletionClient;
use rig::completion::{Message, Prompt, PromptError, Usage};
use rig::providers::openai;
use tracing::info;

use crate::tools::{
    calculator, CounterState, GetCounter, GetCurrentTime, IncrementCounter, Multiply, ReadFile,
};

pub type InnerAgent = rig::agent::Agent<openai::CompletionModel>;

pub struct Agent {
    inner: InnerAgent,
    owner: String,
    history: Vec<Message>,
    cumulative_usage: Usage,
}

impl Agent {
    pub fn new(inner: InnerAgent, owner: impl Into<String>) -> Self {
        Self {
            inner,
            owner: owner.into(),
            history: Vec::new(),
            cumulative_usage: Usage::default(),
        }
    }

    pub fn set_owner(&mut self, owner: impl Into<String>) {
        self.owner = owner.into();
    }

    pub async fn chat(&mut self, input: &str) -> Result<String, PromptError> {
        let response = self
            .inner
            .prompt(input)
            .with_history(&mut self.history)
            .extended_details()
            .await?;

        let output = response.output;
        let (call_usage, estimated) = if has_provider_usage(response.total_usage) {
            (response.total_usage, false)
        } else {
            let input_tokens = estimate_tokens(input);
            let output_tokens = estimate_tokens(&output);
            let estimated_usage = Usage {
                input_tokens,
                output_tokens,
                total_tokens: input_tokens + output_tokens,
                cached_input_tokens: 0,
            };
            (estimated_usage, true)
        };

        self.cumulative_usage += call_usage;

        info!(
            owner = %self.owner,
            input_tokens = call_usage.input_tokens,
            output_tokens = call_usage.output_tokens,
            total_tokens = call_usage.total_tokens,
            cumulative_total_tokens = self.cumulative_usage.total_tokens,
            estimated,
            "token usage"
        );

        Ok(output)
    }

}

fn has_provider_usage(usage: Usage) -> bool {
    usage.input_tokens > 0 || usage.output_tokens > 0 || usage.total_tokens > 0
}

fn estimate_tokens(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    (text.chars().count() as u64).div_ceil(4)
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

pub fn initialize() -> Agent {
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

    Agent::new(inner, "interactive")
}