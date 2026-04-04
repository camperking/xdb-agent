use reqwest::Client;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

const MAX_BODY_BYTES: usize = 512_000;

#[derive(Debug, Deserialize, Serialize)]
pub struct WebFetchArgs {
    pub url: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct WebFetchError(String);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebFetch;

impl Tool for WebFetch {
    const NAME: &'static str = "web_fetch";

    type Error = WebFetchError;
    type Args = WebFetchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "web_fetch".to_string(),
            description: "Ruft eine Webseite ab und gibt deren Inhalt als Markdown zurück. Nützlich, um den Text einer bestimmten URL zu lesen.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Die URL der abzurufenden Webseite, z. B. 'https://example.com'"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let url = args.url.trim();
        if url.is_empty() {
            return Err(WebFetchError("url must not be empty".to_string()));
        }

        let html = fetch_page(url)
            .await
            .map_err(|e| WebFetchError(format!("Failed to fetch '{url}': {e}")))?;

        let markdown = htmd::convert(&html)
            .map_err(|e| WebFetchError(format!("Failed to convert HTML to markdown: {e}")))?;

        let markdown = markdown.trim().to_string();
        if markdown.is_empty() {
            return Ok(format!("Page at '{url}' returned no readable content."));
        }

        Ok(format!("Content of {url}:\n\n{markdown}"))
    }
}

async fn fetch_page(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = Client::new();
    let resp = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?
        .error_for_status()?;

    let bytes = resp.bytes().await?;
    let body = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_BODY_BYTES)]);
    Ok(body.into_owned())
}
