mod agent;
mod config;
mod llm;
mod tools;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::io::{self, Write};
use std::path::PathBuf;

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
    /// Interactive chat
    Chat,
    /// Print resolved config path and defaults
    Config,
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
                let answer = agent::chat_turn(&cfg, &mut history, input).await?;
                println!("\n{answer}\n");
            }
        }
        Commands::Config => {
            println!("config_path={}", cfg_path.display());
            println!("{}", toml::to_string_pretty(&cfg).context("serialize config")?);
        }
    }

    Ok(())
}
