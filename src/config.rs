use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Persistent configuration stored in `config.toml`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    /// UUIDs of contacts allowed to interact with the agent.
    #[serde(default)]
    pub allowed_uuids: Vec<String>,

    /// Hex-encoded group master keys of groups the agent may respond in.
    #[serde(default)]
    pub allowed_groups: Vec<String>,
}

impl Config {
    /// Return the default config file path (`~/.config/xdb-agent/config.toml`).
    pub fn default_path() -> PathBuf {
        ProjectDirs::from("org", "xdb", "xdb-agent")
            .expect("could not determine XDG directories")
            .config_dir()
            .join("config.toml")
    }

    /// Load config from disk. Returns a default (empty) config if the file doesn't exist.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            info!(path = %path.display(), "config file not found, using defaults");
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        info!(path = %path.display(), "loaded config");
        Ok(config)
    }

    /// Save config to disk (creates parent directories as needed).
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        info!(path = %path.display(), "saved config");
        Ok(())
    }

    /// Add a contact UUID if not already present. Returns true if added.
    pub fn add_uuid(&mut self, uuid: &str) -> bool {
        let s = uuid.to_string();
        if self.allowed_uuids.contains(&s) {
            return false;
        }
        self.allowed_uuids.push(s);
        true
    }

    /// Remove a contact UUID. Returns true if it was present.
    pub fn remove_uuid(&mut self, uuid: &str) -> bool {
        let before = self.allowed_uuids.len();
        self.allowed_uuids.retain(|u| u != uuid);
        self.allowed_uuids.len() < before
    }

    /// Add a group master key (hex) if not already present. Returns true if added.
    pub fn add_group(&mut self, group_hex: &str) -> bool {
        let s = group_hex.to_string();
        if self.allowed_groups.contains(&s) {
            return false;
        }
        self.allowed_groups.push(s);
        true
    }

    /// Remove a group master key (hex). Returns true if it was present.
    pub fn remove_group(&mut self, group_hex: &str) -> bool {
        let before = self.allowed_groups.len();
        self.allowed_groups.retain(|g| g != group_hex);
        self.allowed_groups.len() < before
    }
}
