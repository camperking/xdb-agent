use reqwest::Client;
use rig::tool::{Tool, ToolContext};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct SearchResult {
    pub title: String,
    pub snippet: String,
    pub url: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WebSearchArgs {
    pub query: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct WebSearchError(String);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebSearch;

impl Tool for WebSearch {
    const NAME: &'static str = "web_search";

    type Error = WebSearchError;
    type Args = WebSearchArgs;
    type Output = String;

    fn description(&self) -> String {
        "Sucht im Web (DuckDuckGo HTML) und gibt die wichtigsten Treffer mit Titel, Snippet und URL zurück.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Suchanfrage, z. B. 'Rust async tutorial'"
                }
            },
            "required": ["query"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let query = args.query.trim();
        if query.is_empty() {
            return Err(WebSearchError("query must not be empty".to_string()));
        }

        let results = web_search(query)
            .await
            .map_err(|error| WebSearchError(format!("Web search failed: {error}")))?;

        if results.is_empty() {
            return Ok(format!("No results found for '{query}'."));
        }

        let rendered = results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                format!(
                    "{}. {}\n{}\n{}",
                    index + 1,
                    result.title,
                    result.snippet,
                    result.url
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        Ok(format!("Top web results for '{query}':\n\n{rendered}"))
    }
}

async fn web_search(query: &str) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = "https://html.duckduckgo.com/html/?".to_string();

    let resp = client
        .get(&url)
        .query(&[("q", query)])
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?
        .text()
        .await?;

    let doc = Html::parse_document(&resp);
    let result_selector = Selector::parse(".result")?;
    let title_selector = Selector::parse(".result__a")?;
    let snippet_selector = Selector::parse(".result__snippet")?;
    let url_selector = Selector::parse(".result__url")?;
    let max_results = 5usize;

    let mut results = Vec::new();
    for result in doc.select(&result_selector) {
        let title = result
            .select(&title_selector)
            .next()
            .map(|el| el.text().collect::<String>());
        let snippet = result
            .select(&snippet_selector)
            .next()
            .map(|el| el.text().collect::<String>());
        let url = result
            .select(&url_selector)
            .next()
            .map(|el| format!("https://{}", el.text().collect::<String>()));

        if let (Some(title), Some(snippet), Some(url)) = (title, snippet, url) {
            results.push(SearchResult {
                title,
                snippet,
                url,
            });
        }

        if results.len() >= max_results {
            break;
        }
    }
    Ok(results)
}
