pub mod run;
pub mod build;
pub mod test;
pub mod clean;
pub mod cache;

pub use run::execute as run_execute;
pub use build::execute as build_execute;
pub use test::execute as test_execute;
pub use clean::execute as clean_execute;
pub use cache::execute as cache_execute;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum CacheCommands {
    /// Show cache statistics
    Stats,
    
    /// Clean up cache
    Clean {
        /// Maximum cache size (e.g., "10GB")
        #[arg(long)]
        size_limit: Option<String>,
        
        /// Maximum age (e.g., "30d")
        #[arg(long)]
        max_age: Option<String>,
    },
}
