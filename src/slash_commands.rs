use clap::{Parser, Subcommand};
use tracing::warn;

use crate::agent::Agent;

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

pub async fn handle(agent: &mut Agent, input: &str) -> Option<String> {
    let parsed = match parse(input) {
        Ok(command) => command,
        Err(error) => return Some(error),
    };

    match parsed {
        Some(SlashCommand::New) => {
            agent.reset_conversation();
            Some("Started a new conversation.".to_string())
        }
        Some(SlashCommand::Usage) => Some(agent.usage_summary()),
        Some(SlashCommand::Compact) => Some(match agent.compact_and_reinitialize().await {
            Ok(path) => format!(
                "Conversation compacted. Summary written to {} and agent reinitialized.",
                path.display()
            ),
            Err(error) => {
                warn!(owner = %agent.owner(), %error, "manual compaction failed");
                format!("Compaction failed: {error}")
            }
        }),
        Some(SlashCommand::Shutup) => {
            agent.set_prompting_enabled(false);
            Some("Agent prompting disabled. I will stay quiet until you send /talk.".to_string())
        }
        Some(SlashCommand::Talk) => {
            agent.set_prompting_enabled(true);
            Some("Agent prompting enabled.".to_string())
        }
        None => None,
    }
}

fn parse(input: &str) -> Result<Option<SlashCommand>, String> {
    let mut lines = input.lines();
    let content = match lines.next() {
        Some(first_line) if first_line.starts_with("[sender=") && first_line.ends_with(']') => {
            lines.collect::<Vec<_>>().join("\n")
        }
        Some(_) | None => input.to_string(),
    };

    let command = content.trim();
    if !command.starts_with('/') {
        return Ok(None);
    }

    let body = command.trim_start_matches('/');
    let mut argv: Vec<String> = vec!["/".to_string()];

    let parts =
        shlex::split(body).ok_or_else(|| "Invalid slash command syntax: unclosed quote or escape.".to_string())?;
    argv.extend(parts);

    SlashCli::try_parse_from(argv)
        .map(|parsed| Some(parsed.command))
        .map_err(|error| error.to_string())
}
