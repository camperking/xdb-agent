use directories::ProjectDirs;
use rig::agent::AgentBuilder;
use rig::client::EmbeddingsClient;
use rig::client::CompletionClient;
use rig::completion::{Message, Prompt, PromptError, Usage};
use rig::embeddings::{Embed, EmbedError, EmbeddingsBuilder, TextEmbedder};
use rig::providers::openai;
use rig::vector_store::in_memory_store::InMemoryVectorStore;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::tools::{
    calculator, CounterState, GetCounter, GetCurrentTime, IncrementCounter, Multiply, ReadFile,
};

pub type InnerAgent = rig::agent::Agent<openai::CompletionModel>;

const MAX_PROJECT_FILES: usize = 500;
const MAX_FILE_BYTES: u64 = 200_000;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct ProjectFileDocument {
    path: String,
    content: String,
}

impl Embed for ProjectFileDocument {
    fn embed(&self, embedder: &mut TextEmbedder) -> Result<(), EmbedError> {
        embedder.embed(format!("Path: {}\n\n{}", self.path, self.content));
        Ok(())
    }
}

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
        if let Some(response) = self.handle_slash_command(input) {
            return Ok(response);
        }

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

        Ok(output.to_string())
    }

    pub fn handle_slash_command(&mut self, input: &str) -> Option<String> {
        let command = input.trim();
        match command {
            "/new" => {
                self.reset_conversation();
                Some("Started a new conversation.".to_string())
            }
            "/usage" => Some(self.usage_summary()),
            _ => None,
        }
    }

    pub fn reset_conversation(&mut self) {
        self.history.clear();
        self.cumulative_usage = Usage::default();
    }

    pub fn usage_summary(&self) -> String {
        format!(
            "Token usage (cumulative): input={}, output={}, total={}, cached_input={}",
            self.cumulative_usage.input_tokens,
            self.cumulative_usage.output_tokens,
            self.cumulative_usage.total_tokens,
            self.cumulative_usage.cached_input_tokens,
        )
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

fn collect_project_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }

    files
}

fn load_project_documents(root: &Path) -> Vec<ProjectFileDocument> {
    let mut docs = Vec::new();

    for path in collect_project_files(root) {
        if docs.len() >= MAX_PROJECT_FILES {
            break;
        }

        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        if metadata.len() > MAX_FILE_BYTES {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        if content.trim().is_empty() {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());

        docs.push(ProjectFileDocument {
            path: relative,
            content,
        });
    }

    docs
}

pub async fn initialize() -> Agent {
    let client: openai::CompletionsClient = openai::CompletionsClient::builder()
        .api_key("")
        .base_url("http://localhost:8080")
        .build()
        .expect("Failed to build OpenAI client");

    let embedding_client: openai::Client = openai::Client::builder()
        .api_key("")
        .base_url("http://localhost:11434")
        .build()
        .expect("Failed to build OpenAI embedding client");

    let mut builder = AgentBuilder::new(client.completion_model("Qwen3.5-35B-A3B"))
        .name("Chat Agent")
        .preamble(
            "You are a helpful assistant with access to tools. \
             You can perform calculations, get the current time, and manage a counter. \
             Use the tools provided to answer the user's question.",
        )
        .default_max_turns(100)
        .tools(build_tools());

    if let Some(document_root) = ProjectDirs::from("org", "xdb", "xdb-agent")
        .map(|dirs| dirs.data_dir().to_path_buf())
    {
        let document_root = document_root.join("memory");
        let documents = load_project_documents(&document_root);
        if documents.is_empty() {
            warn!(path = %document_root.display(), "no project files found for dynamic context");
        } else {
            let embedding_model = embedding_client.embedding_model("nomic-embed-text-v2-moe");
            match EmbeddingsBuilder::new(embedding_model.clone()).documents(documents) {
                Ok(embeddings_builder) => match embeddings_builder.build().await {
                    Ok(embeddings) => {
                        let vector_store = InMemoryVectorStore::from_documents_with_id_f(
                            embeddings,
                            |doc| doc.path.clone(),
                        );
                        let index = vector_store.index(embedding_model);
                        builder = builder.dynamic_context(4, index);
                        info!(path = %document_root.display(), "dynamic project context enabled");
                    }
                    Err(error) => {
                        warn!(%error, "failed to build project embeddings; continuing without dynamic context");
                    }
                },
                Err(error) => {
                    warn!(%error, "failed to prepare project documents for embeddings; continuing without dynamic context");
                }
            }
        }
    } else {
        warn!("could not determine XDG project data directory; continuing without dynamic context");
    }

    let inner = builder.build();

    Agent::new(inner, "interactive")
}