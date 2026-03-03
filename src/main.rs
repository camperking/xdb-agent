use std::path::PathBuf;

use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use std::io::{self, Write};

mod agent;
mod config;
mod signal;
mod tools;

#[derive(Parser)]
#[clap(name = "chat-agent", about = "LLM chat agent with Signal messenger integration")]
struct Cli {
    /// Path to the presage SQLite database (default: XDG data dir)
    #[clap(long, global = true)]
    db_path: Option<String>,

    /// Path to the config file (default: ~/.config/xdb-agent/config.toml)
    #[clap(long, global = true)]
    config: Option<PathBuf>,

    #[clap(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Pair this device as a secondary Signal device (scan QR code)
    LinkDevice {
        /// Name for this device shown in Signal
        #[clap(long, short = 'n', default_value = "xdb-agent")]
        device_name: String,
    },
    /// Run as a Signal chat daemon — receive messages and reply via LLM
    Daemon,
    /// Interactive terminal chat (default if no subcommand given)
    Chat,
    /// Manage the allowlist configuration
    #[clap(subcommand)]
    Config(ConfigCommand),
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Show current configuration
    Show,
    /// Add a contact UUID to the allowlist
    AddUuid {
        /// The contact's UUID
        uuid: String,
    },
    /// Remove a contact UUID from the allowlist
    RemoveUuid {
        /// The contact's UUID
        uuid: String,
    },
    /// Add a group master key (hex) to the allowlist
    AddGroup {
        /// Group master key as hex string (64 hex characters = 32 bytes)
        group_key: String,
    },
    /// Remove a group master key (hex) from the allowlist
    RemoveGroup {
        /// Group master key as hex string
        group_key: String,
    },
}

fn default_db_path() -> String {
    ProjectDirs::from("org", "xdb", "xdb-agent")
        .expect("could not determine XDG directories")
        .data_dir()
        .join("signal.db3")
        .display()
        .to_string()
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::metadata::LevelFilter::INFO.into())
        .from_env_lossy()
        .add_directive("libsignal=error".parse().unwrap());
    tracing_subscriber::fmt::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
}

async fn run_interactive() -> Result<(), Box<dyn std::error::Error>> {
    let mut agent = agent::initialize().await;

    println!("Chat agent initialized. Type 'exit' to quit.");
    println!("Slash commands: /new (new conversation), /usage (show token usage)");
    println!("Available tools: add, multiply, get_time, increment_counter, get_counter, read_file\n");

    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let input = input.trim();
        if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
            break;
        }

        if input.is_empty() {
            continue;
        }

        match agent.chat(input).await {
            Ok(response) => {
                println!("{}", response);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    println!("Goodbye!");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let cli = Cli::parse();
    let db_path = cli.db_path.unwrap_or_else(default_db_path);
    let config_path = cli.config.unwrap_or_else(config::Config::default_path);

    match cli.command {
        Some(Command::LinkDevice { device_name }) => {
            signal::link_device(&db_path, &device_name).await?;
        }
        Some(Command::Daemon) => {
            let cfg = config::Config::load(&config_path)?;
            let local = tokio::task::LocalSet::new();
            local
                .run_until(signal::run_daemon(&db_path, &cfg, agent::initialize))
                .await?;
        }
        Some(Command::Config(config_cmd)) => {
            let mut cfg = config::Config::load(&config_path)?;
            match config_cmd {
                ConfigCommand::Show => {
                    println!("Config file: {}", config_path.display());
                    println!();
                    if cfg.allowed_uuids.is_empty() {
                        println!("Allowed contacts: (none)");
                    } else {
                        println!("Allowed contacts:");
                        for uuid in &cfg.allowed_uuids {
                            println!("  - {uuid}");
                        }
                    }
                    println!();
                    if cfg.allowed_groups.is_empty() {
                        println!("Allowed groups: (none)");
                    } else {
                        println!("Allowed groups:");
                        for group in &cfg.allowed_groups {
                            println!("  - {group}");
                        }
                    }
                }
                ConfigCommand::AddUuid { uuid } => {
                    if cfg.add_uuid(&uuid) {
                        cfg.save(&config_path)?;
                        println!("Added UUID: {uuid}");
                    } else {
                        println!("UUID already in allowlist: {uuid}");
                    }
                }
                ConfigCommand::RemoveUuid { uuid } => {
                    if cfg.remove_uuid(&uuid) {
                        cfg.save(&config_path)?;
                        println!("Removed UUID: {uuid}");
                    } else {
                        println!("UUID not found in allowlist: {uuid}");
                    }
                }
                ConfigCommand::AddGroup { group_key } => {
                    // Validate hex format
                    match hex::decode(&group_key) {
                        Ok(bytes) if bytes.len() == 32 => {}
                        Ok(bytes) => {
                            eprintln!("Error: group key must decode to 32 bytes, got {} bytes", bytes.len());
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Error: invalid hex string: {e}");
                            std::process::exit(1);
                        }
                    }
                    if cfg.add_group(&group_key) {
                        cfg.save(&config_path)?;
                        println!("Added group: {group_key}");
                    } else {
                        println!("Group already in allowlist: {group_key}");
                    }
                }
                ConfigCommand::RemoveGroup { group_key } => {
                    if cfg.remove_group(&group_key) {
                        cfg.save(&config_path)?;
                        println!("Removed group: {group_key}");
                    } else {
                        println!("Group not found in allowlist: {group_key}");
                    }
                }
            }
        }
        Some(Command::Chat) | None => {
            run_interactive().await?;
        }
    }

    Ok(())
}