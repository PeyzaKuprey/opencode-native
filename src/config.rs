use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model: ModelConfig,
    pub agent: AgentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub base_url: String,
    pub api_key_env: String,
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub system_prompt: String,
    pub max_steps: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: ModelConfig {
                provider: "openai-compatible".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                model: "gpt-4.1-mini".to_string(),
                api_key: None,
            },
            agent: AgentConfig {
                system_prompt: "You are OpenCode Native, a lightweight coding agent running directly on the user's device. Be concise, safe, and helpful. Use tools when needed.".to_string(),
                max_steps: 20,
            },
        }
    }
}

impl Config {
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() { fs::create_dir_all(parent).with_context(|| format!("create config dir {}", parent.display()))?; }
        let text = toml::to_string_pretty(self).context("serialize config")?;
        fs::write(path, text).with_context(|| format!("write config {}", path.display()))?;
        Ok(())
    }
}

pub fn default_config_path() -> PathBuf {
    if let Ok(envp) = std::env::var("OPENCODE_NATIVE_CONFIG") {
        if !envp.trim().is_empty() { return PathBuf::from(envp); }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() { return PathBuf::from(xdg).join("opencode-native/config.toml"); }
    }
    BaseDirs::new()
        .map(|b| b.config_dir().join("opencode-native/config.toml"))
        .unwrap_or_else(|| PathBuf::from(".opencode-native/config.toml"))
}

pub fn resolve_config_path(cli: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = cli { return Ok(p.to_path_buf()); }
    Ok(default_config_path())
}

pub fn load_or_default(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let data = fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
    let cfg: Config = toml::from_str(&data).with_context(|| format!("parse config {}", path.display()))?;
    Ok(cfg)
}

pub fn resolve_api_key(cfg: &Config) -> Result<String> {
    if let Some(k) = &cfg.model.api_key {
        if !k.trim().is_empty() {
            return Ok(k.clone());
        }
    }
    let key = std::env::var(&cfg.model.api_key_env)
        .with_context(|| format!("missing API key env {}", cfg.model.api_key_env))?;
    if key.trim().is_empty() {
        anyhow::bail!("API key env {} is empty", cfg.model.api_key_env);
    }
    Ok(key)
}
