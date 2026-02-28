use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize, Serialize)]
pub struct AddArgs {
    pub a: i32,
    pub b: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MultiplyArgs {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CalculatorError(String);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Add;

impl Tool for Add {
    const NAME: &'static str = "add";

    type Error = CalculatorError;
    type Args = AddArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "add".to_string(),
            description: "Add two numbers together. Returns the sum of a and b.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "a": {
                        "type": "integer",
                        "description": "The first number"
                    },
                    "b": {
                        "type": "integer",
                        "description": "The second number"
                    }
                },
                "required": ["a", "b"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let result = args.a + args.b;
        Ok(format!("{} + {} = {}", args.a, args.b, result))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Multiply;

impl Tool for Multiply {
    const NAME: &'static str = "multiply";

    type Error = CalculatorError;
    type Args = MultiplyArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "multiply".to_string(),
            description: "Multiply two numbers together. Returns the product of x and y.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "x": {
                        "type": "integer",
                        "description": "The first number"
                    },
                    "y": {
                        "type": "integer",
                        "description": "The second number"
                    }
                },
                "required": ["x", "y"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let result = args.x * args.y;
        Ok(format!("{} * {} = {}", args.x, args.y, result))
    }
}