use cue_common::Result;
use tracing::info;
use crate::config::{create_default_workspace_config, create_default_project_config};
use clap::ValueEnum;

#[derive(ValueEnum, Clone)]
pub enum InitType {
    /// Initialize workspace configuration
    Workspace,
    /// Initialize project configuration
    Project,
}

pub async fn execute(config_type: InitType, project_name: Option<String>) -> Result<()> {
    match config_type {
        InitType::Workspace => {
            init_workspace().await?;
        }
        InitType::Project => {
            let project_name = project_name
                .ok_or_else(|| cue_common::CueError::Config(
                    "Project name is required for project initialization. Use --project-name <name>".to_string()
                ))?;
            init_project(&project_name).await?;
        }
    }
    Ok(())
}

async fn init_workspace() -> Result<()> {
    let workspace_root = std::env::current_dir()?;
    info!("Initializing workspace configuration in: {}", workspace_root.display());
    
    create_default_workspace_config(&workspace_root).await?;
    
    println!("✅ Workspace configuration created successfully!");
    println!("📁 Created: cue.workspace.toml");
    println!();
    println!("Next steps:");
    println!("1. Edit cue.workspace.toml to customize your workspace settings");
    println!("2. Create projects in the projects/ directory");
    println!("3. Run 'cue init project --project-name <name>' in each project directory");
    
    Ok(())
}

async fn init_project(project_name: &str) -> Result<()> {
    let project_path = std::env::current_dir()?;
    info!("Initializing project configuration for '{}' in: {}", project_name, project_path.display());
    
    create_default_project_config(&project_path, project_name).await?;
    
    println!("✅ Project configuration created successfully!");
    println!("📁 Created: cue.toml");
    println!("🏷️  Project name: {}", project_name);
    println!();
    println!("Next steps:");
    println!("1. Edit cue.toml to customize your project tasks");
    println!("2. Run 'cue run <task-name>' to execute tasks");
    println!("3. Available default tasks: build, test");
    
    Ok(())
}
