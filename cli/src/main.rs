use clap::{Parser, Subcommand};
use cue_common::Result;

mod commands;
mod cache;
mod config;
mod executor;

use commands::{run_execute, build_execute, test_execute, clean_execute, cache_execute, init_execute, CacheCommands, init::InitType};

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
    
    /// Ignore existing cache hits but still write cache results
    #[arg(long, default_value = "false")]
    ignore_existing_cache: bool,
    
    /// Disable cache lookups and writes entirely
    #[arg(long, default_value = "false")]
    no_cache: bool,
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
    
    /// Manage cache
    Cache {
        #[command(subcommand)]
        subcommand: CacheCommands,
    },
    
    /// Initialize workspace or project configuration
    Init {
        /// Type of configuration to initialize
        #[arg(value_enum)]
        config_type: InitType,
        
        /// Project name (required for project init)
        #[arg(long)]
        project_name: Option<String>,
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
    
    if cli.verbose {
        // Debug mode: show full formatting
        tracing_subscriber::fmt()
            .with_max_level(log_level)
            .init();
    } else {
        // Non-debug mode: minimal formatting
        tracing_subscriber::fmt()
            .with_max_level(log_level)
            .without_time()
            .with_target(false)
            .with_level(false)
            .init();
    }
    
    // CLI started successfully
    
    match cli.command {
        Commands::Run { task, args } => {
            run_execute(&task, &args, cli.remote_cache, cli.ignore_existing_cache, cli.no_cache).await?;
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
        Commands::Cache { subcommand } => {
            cache_execute(subcommand).await?;
        }
        Commands::Init { config_type, project_name } => {
            init_execute(config_type, project_name).await?;
        }
    }
    
    Ok(())
}
