use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use rig::agent::AgentBuilder;
use rig::client::EmbeddingsClient;
use rig::client::CompletionClient;
use rig::client::Nothing;
use rig::completion::{Message, Prompt, PromptError, Usage};
use rig::embeddings::{Embed, EmbedError, EmbeddingsBuilder, TextEmbedder};
use rig::providers::ollama;
use rig::providers::openai;
use rig::vector_store::in_memory_store::InMemoryVectorStore;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::vec;
use tracing::{info, warn};

use crate::tools::{
    calculator, CounterState, GetCounter, GetCurrentTime, GetWeather, IncrementCounter,
    Multiply, ReadFile,
};

pub type InnerAgent = rig::agent::Agent<openai::CompletionModel>;

#[derive(Debug, Parser)]
#[command(name = "/", about = "Slash commands for managing the agent conversation and usage", disable_version_flag = true, disable_help_flag = true)]
struct SlashCli {
    #[clap(subcommand)]
    command: SlashCommand,
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum SlashCommand {
    /// - Start a new conversation, clearing history and usage
    New,
    /// - Show cumulative token usage for the current conversation
    Usage,
    /// - Manually compact the conversation and save a summary to the memory
    Compact,
    /// - Disable prompting the LLM for this conversation
    Shutup,
    /// - Re-enable prompting the LLM for this conversation
    Talk,
}

const XDEVBOT_PREAMBLE: &str = include_str!("templates/xDevBot.md");
const MAX_PROJECT_FILES: usize = 500;
const MAX_FILE_BYTES: u64 = 200_000;
const CONTEXT_USAGE_THRESHOLD_TOKENS: u64 = 120_000;

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
    prompting_enabled: bool,
}

impl Agent {
    pub fn new(inner: InnerAgent, owner: impl Into<String>) -> Self {
        Self {
            inner,
            owner: owner.into(),
            history: Vec::new(),
            cumulative_usage: Usage::default(),
            prompting_enabled: true,
        }
    }

    pub fn set_owner(&mut self, owner: impl Into<String>) {
        self.owner = owner.into();
    }

    pub async fn chat(&mut self, input: &str) -> Result<String, PromptError> {
        if let Some(response) = self.handle_slash_command(input).await {
            return Ok(response);
        }

        if !self.prompting_enabled {
            return Ok("HEARTBEAT_OK".to_string());
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

        if let Err(error) = self.compact_and_reinitialize_if_needed().await {
            warn!(owner = %self.owner, %error, "failed to compact context at threshold");
        }

        Ok(output.to_string())
    }

    pub async fn handle_slash_command(&mut self, input: &str) -> Option<String> {
        let parsed = match parse_slash_command(input) {
            Ok(command) => command,
            Err(error) => return Some(error),
        };

        match parsed {
            Some(SlashCommand::New) => {
                self.reset_conversation();
                Some("Started a new conversation.".to_string())
            }
            Some(SlashCommand::Usage) => Some(self.usage_summary()),
            Some(SlashCommand::Compact) => Some(match self.compact_and_reinitialize().await {
                Ok(path) => format!(
                    "Conversation compacted. Summary written to {} and agent reinitialized.",
                    path.display()
                ),
                Err(error) => {
                    warn!(owner = %self.owner, %error, "manual compaction failed");
                    format!("Compaction failed: {error}")
                }
            }),
            Some(SlashCommand::Shutup) => {
                self.prompting_enabled = false;
                Some("Agent prompting disabled. I will stay quiet until you send /talk.".to_string())
            }
            Some(SlashCommand::Talk) => {
                self.prompting_enabled = true;
                Some("Agent prompting enabled.".to_string())
            }
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

    async fn compact_and_reinitialize_if_needed(&mut self) -> anyhow::Result<()> {
        if self.cumulative_usage.total_tokens < CONTEXT_USAGE_THRESHOLD_TOKENS {
            return Ok(());
        }

        let summary_path = self.compact_and_reinitialize().await?;

        info!(
            owner = %self.owner,
            path = %summary_path.display(),
            threshold = CONTEXT_USAGE_THRESHOLD_TOKENS,
            "context threshold reached; wrote memory summary and reinitialized agent"
        );

        Ok(())
    }

    async fn compact_and_reinitialize(&mut self) -> anyhow::Result<PathBuf> {

        let summary_prompt = "Fasse die bisherige Unterhaltung als kompaktes Markdown für das Langzeitgedächtnis zusammen. Enthalte:\n- Wichtige Nutzerpräferenzen und Einschränkungen\n- Entschiedene Lösungen und Begründungen\n- Wichtige Fakten und Kennungen\n- Offene Fragen und nächste Schritte\nNutze prägnante Stichpunkte und Überschriften.";

        let response = self
            .inner
            .prompt(summary_prompt)
            .with_history(&mut self.history)
            .await?;

        let summary_md = response.to_string();
        let summary_path = write_memory_summary_markdown(&self.owner, &summary_md)?;

        self.inner = build_inner().await;
        self.history.clear();
        self.cumulative_usage = Usage::default();

        Ok(summary_path)
    }

}

fn write_memory_summary_markdown(owner: &str, summary: &str) -> anyhow::Result<PathBuf> {
    let memory_dir = ProjectDirs::from("org", "xdb", "xdb-agent")
        .ok_or_else(|| anyhow::anyhow!("could not determine XDG project data directory"))?
        .data_dir()
        .join("memory");

    std::fs::create_dir_all(&memory_dir)?;

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let owner_slug = sanitize_filename_segment(owner);
    let file_name = format!("history-summary-{owner_slug}-{timestamp}.md");
    let path = memory_dir.join(file_name);

    std::fs::write(&path, summary)?;
    Ok(path)
}

fn sanitize_filename_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "agent".to_string()
    } else {
        out
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
        Box::new(GetWeather),
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

async fn build_inner() -> InnerAgent {
    let client: openai::CompletionsClient = openai::CompletionsClient::builder()
        .api_key("")
        .base_url("http://localhost:8080")
        .build()
        .expect("Failed to build OpenAI client");

    let embedding_client: ollama::Client = ollama::Client::builder()
        .api_key(Nothing)
        .base_url("http://localhost:11434")
        .build()
        .expect("Failed to build OpenAI embedding client");

    let mut builder = AgentBuilder::new(client.completion_model("Qwen3.5-35B-A3B"))
        .name("Chat Agent")
        .preamble(XDEVBOT_PREAMBLE)
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

    builder.build()
}

pub async fn initialize() -> Agent {
    let inner = build_inner().await;

    Agent::new(inner, "interactive")
}

fn parse_slash_command(input: &str) -> Result<Option<SlashCommand>, String> {
    let mut lines = input.lines();
    let content = match lines.next() {
        Some(first_line)
            if first_line.starts_with("[sender=") && first_line.ends_with(']') =>
        {
            lines.collect::<Vec<_>>().join("\n")
        }
        Some(_) | None => input.to_string(),
    };

    let command = content.trim();
    if !command.starts_with('/') {
        return Ok(None);
    }

    let body = command.trim_start_matches('/');
    let mut argv: Vec<String> = vec!["/".to_string()]; // dummy program name for clap

    let parts = shlex::split(body)
        .ok_or_else(|| "Invalid slash command syntax: unclosed quote or escape.".to_string())?;
    argv.extend(parts);

    SlashCli::try_parse_from(argv)
        .map(|parsed| Some(parsed.command))
        .map_err(|error| error.to_string())
}