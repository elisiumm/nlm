mod adapters;
mod cli;
mod commands;
mod config;
mod notebooklm;
mod pptx;

use anyhow::Result;
use clap::Parser;

// #[tokio::main] is a procedural macro that wraps main() in a tokio async runtime.
// Without it, you cannot use .await anywhere in main.
// It expands roughly to: fn main() { tokio::runtime::Runtime::new().unwrap().block_on(async { ... }) }
#[tokio::main]
async fn main() -> Result<()> {
    // Load .env from the current working directory (the user's project root).
    // The _ binding silences the warning when no .env file exists — that's fine.
    let _ = dotenvy::dotenv();

    let cli = cli::Cli::parse();
    commands::dispatch(cli).await
}
