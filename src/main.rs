mod agent;
mod config;
mod llm;
mod tools;
mod tui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::io::{self, Write};
use std::path::PathBuf;
use serde_json::Value;

#[derive(Parser, Debug)]
#[command(name = "opencode-native", version, about = "Lightweight native OpenCode-style agent")]
struct Cli {
    /// Path to config file
    #[arg(long)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a single prompt
    Run {
        /// Prompt to execute
        prompt: Vec<String>,
    },
    /// Interactive chat (REPL)
    Chat,
    /// Interactive TUI
    Tui,
    /// Show or initialize config
    Config,
    /// List configured/default model info
    Models,
    /// Manage sessions (list/clear)
    Session {
        #[command(subcommand)]
        action: Option<SessionAction>,
    },
    /// Serve HTTP API (compatibility placeholder)
    Serve {
        #[arg(long, default_value = "127.0.0.1:4096")]
        host: String,
        #[arg(long, default_value_t = 4096)]
        port: u16,
    },
    /// Show version and runtime info
    Version,
}

#[derive(Subcommand, Debug)]
enum SessionAction {
    List,
    Path,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg_path = config::resolve_config_path(cli.config.as_deref())?;
    let cfg = config::load_or_default(&cfg_path)?;

    match cli.command {
        Commands::Run { prompt } => {
            let prompt = prompt.join(" ").trim().to_string();
            if prompt.is_empty() {
                anyhow::bail!("prompt is required");
            }
            let answer = agent::run_once(&cfg, &prompt).await?;
            println!("{answer}");
        }
        Commands::Chat => {
            println!("OpenCode Native chat. Type /exit to quit.");
            let mut history = Vec::new();
            loop {
                print!("> ");
                io::stdout().flush().ok();
                let mut line = String::new();
                let n = io::stdin().read_line(&mut line)?;
                if n == 0 {
                    break;
                }
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                if input == "/exit" || input == "/quit" {
                    break;
                }
                let answer = agent::chat_turn(&cfg, &mut history, input, None, None).await?;
                println!("\n{answer}\n");
            }
        }
        Commands::Tui => {
            tui::run(cfg).await?;
        }
        Commands::Config => {
            println!("config_path={}", cfg_path.display());
            println!("{}", toml::to_string_pretty(&cfg).context("serialize config")?);
        }
        Commands::Models => {
            println!("provider={}", cfg.model.provider);
            println!("model={}", cfg.model.model);
            println!("base_url={}", cfg.model.base_url);
            println!("api_key_env={}", cfg.model.api_key_env);
            println!("api_key_configured={}", cfg.model.api_key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false));
        }
        Commands::Session { action } => {
            let p = session_path();
            match action.unwrap_or(SessionAction::List) {
                SessionAction::Path => println!("{}", p.display()),
                SessionAction::List => {
                    let sessions = load_sessions_cli(&p);
                    if sessions.is_empty() { println!("(no sessions)"); }
                    for (i, s) in sessions.iter().enumerate() {
                        println!("{:>2}. {}  [{} msgs]", i+1, s.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled"), s.get("messages").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0));
                    }
                }
            }
        }
        Commands::Serve { host, port } => {
            println!("opencode-native serve is a compatibility placeholder.");
            println!("Native full OpenCode server parity is in progress.");
            println!("Planned listen: http://{}:{}", host, port);
            println!("Use `opencode-native tui` or `opencode-native chat` for the native client.");
        }
        Commands::Version => {
            println!("opencode-native {}", env!("CARGO_PKG_VERSION"));
            println!("os={}", std::env::consts::OS);
            println!("arch={}", std::env::consts::ARCH);
        }
    }

    Ok(())
}

fn session_path() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME").ok().filter(|s| !s.is_empty()).map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.data_dir().to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("opencode-native/sessions.json")
}

fn load_sessions_cli(p: &PathBuf) -> Vec<Value> {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Vec<Value>>(&s).ok()).unwrap_or_default()
}
