use rig::tool::{Tool, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;

#[derive(Debug, Deserialize, Serialize)]
pub struct ReadFileArgs {
    pub path: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct FileReaderError(String);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadFile;

impl Tool for ReadFile {
    const NAME: &'static str = "read_file";

    type Error = FileReaderError;
    type Args = ReadFileArgs;
    type Output = String;

    fn description(&self) -> String {
        "Liest den Inhalt einer Datei aus dem Dateisystem. Gibt den vollständigen Dateiinhalt als Zeichenkette zurück.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Der Pfad zur zu lesenden Datei"
                }
            },
            "required": ["path"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let content = fs::read_to_string(&args.path)
            .map_err(|e| FileReaderError(format!("Failed to read file '{}': {}", args.path, e)))?;
        Ok(content)
    }
}
