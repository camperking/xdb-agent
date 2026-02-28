use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Deserialize, Serialize)]
pub struct IncrementCounterArgs;

#[derive(Debug, Deserialize, Serialize)]
pub struct GetCounterArgs;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CounterError(String);

#[derive(Clone, Debug)]
pub struct CounterState {
    pub counter: Arc<Mutex<i32>>,
}

impl CounterState {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(Mutex::new(0)),
        }
    }
}

impl Default for CounterState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct IncrementCounter {
    pub state: CounterState,
}

impl IncrementCounter {
    pub fn new(state: CounterState) -> Self {
        Self { state }
    }
}

impl Tool for IncrementCounter {
    const NAME: &'static str = "increment_counter";

    type Error = CounterError;
    type Args = IncrementCounterArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "increment_counter".to_string(),
            description: "Increment a counter by 1 and return the new value".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut counter = self.state.counter.lock().await;
        *counter += 1;
        Ok(format!("Counter incremented to {}", counter))
    }
}

#[derive(Clone, Debug)]
pub struct GetCounter {
    pub state: CounterState,
}

impl GetCounter {
    pub fn new(state: CounterState) -> Self {
        Self { state }
    }
}

impl Tool for GetCounter {
    const NAME: &'static str = "get_counter";

    type Error = CounterError;
    type Args = GetCounterArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "get_counter".to_string(),
            description: "Get the current value of the counter without modifying it".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let counter = self.state.counter.lock().await;
        Ok(format!("Current counter value: {}", counter))
    }
}