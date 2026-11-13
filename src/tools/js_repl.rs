use boa_engine::{Context, Source};
use rig::tool::{Tool, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::json;

const MAX_OUTPUT_CHARS: usize = 4_000;

#[derive(Debug, Deserialize, Serialize)]
pub struct JsReplArgs {
    pub code: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct JsReplError(String);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsRepl;

impl Tool for JsRepl {
    const NAME: &'static str = "js_repl";

    type Error = JsReplError;
    type Args = JsReplArgs;
    type Output = String;

    fn description(&self) -> String {
        "Führt JavaScript-Code aus und gibt das Ergebnis zurück. Nützlich für Berechnungen, Datenverarbeitung und Logik.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "Der auszuführende JavaScript-Code"
                }
            },
            "required": ["code"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let code = args.code.trim();
        if code.is_empty() {
            return Err(JsReplError("code must not be empty".to_string()));
        }

        let mut context = Context::default();

        match context.eval(Source::from_bytes(code)) {
            Ok(result) => {
                let output = result
                    .to_string(&mut context)
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_else(|_| format!("{}", result.display()));

                if output.len() > MAX_OUTPUT_CHARS {
                    Ok(format!("{}… (truncated)", &output[..MAX_OUTPUT_CHARS]))
                } else {
                    Ok(output)
                }
            }
            Err(e) => {
                let err_msg = e
                    .as_opaque()
                    .and_then(|v| {
                        v.to_string(&mut context)
                            .ok()
                            .map(|s| s.to_std_string_escaped())
                    })
                    .unwrap_or_else(|| format!("{e}"));
                Ok(format!("Error: {err_msg}"))
            }
        }
    }
}
