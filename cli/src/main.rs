use clap::{Parser, Subcommand};
use cue_common::Result;
use tracing::info;

mod commands;
mod cache;
mod config;
mod executor;

use commands::{run_execute, build_execute, test_execute, clean_execute};

#[derive(Parser)]
#[command(name = "cue")]
#[command(about = "A monorepo orchestration and caching tool")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    #[arg(long, default_value = "false")]
    verbose: bool,
    
    #[arg(long)]
    remote_cache: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a specific task
    Run {
        /// Name of the task to run
        task: String,
        
        /// Additional arguments to pass to the task
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    
    /// Build the project
    Build {
        /// Build target
        #[arg(default_value = "all")]
        target: String,
    },
    
    /// Run tests
    Test {
        /// Test pattern to run
        #[arg(default_value = "all")]
        pattern: String,
    },
    
    /// Clean build artifacts and cache
    Clean {
        /// Clean cache as well
        #[arg(long, default_value = "false")]
        cache: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    // Initialize logging
    let log_level = if cli.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    
    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .init();
    
    info!("Starting cue CLI");
    
    match cli.command {
        Commands::Run { task, args } => {
            run_execute(&task, &args, cli.remote_cache).await?;
        }
        Commands::Build { target } => {
            build_execute(&target, cli.remote_cache).await?;
        }
        Commands::Test { pattern } => {
            test_execute(&pattern, cli.remote_cache).await?;
        }
        Commands::Clean { cache } => {
            clean_execute(cache).await?;
        }
    }
    
    Ok(())
}
