use std::path::Path;

use anyhow::{anyhow, Context, Error};
use clap::Parser;
use clog::{
    detect_project, get_next_version, make_bump_commit, make_initial_commit, repo_has_commits,
    repo_is_clean, semver::SemVer, Config, Project,
};
use git2::Repository;
use inquire::Confirm;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    
    /// Declare stable and bump to v1.0.0
    #[clap(short, long, default_value = "false")]
    stable: bool,
    
    /// Skip confirmation prompts (automatically answer yes)
    #[arg(short = 'y', long)]
    yes: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let current_dir = Path::new("./");
    let config = Config::new(&current_dir);
    let project = detect_project(&config)?;

    let repo = Repository::open(current_dir)
        .with_context(|| format!("Failed to open repo at {:?}", current_dir.canonicalize()))?;

    if !repo_has_commits(&repo) {
        return Err(anyhow!("Repo has no commits"));
    }

    if !repo_is_clean(&repo)? {
        return Err(anyhow!("Repo is not in a clean state. Commit your changes"));
    }

    if cli.stable {
        major_version_one(project, &repo, &config, cli.yes)
    } else {
        bump_release(project, &repo, &config, cli.yes)
    }
}

fn bump_release(
    mut project: Box<dyn Project>,
    repo: &Repository,
    config: &Config,
    auto_yes: bool,
) -> anyhow::Result<()> {
    let current_version = project.get_version().clone();
    let new_version = get_next_version(repo, &project, config).unwrap();
    
    if new_version > current_version {
        let should_bump = if auto_yes {
            println!(
                "Bumping version from {} to {}",
                current_version, new_version
            );
            true
        } else {
            Confirm::new(&format!(
                "would you like to bump this project's version from {} to {}?",
                current_version, new_version
            ))
            .with_help_message(
                "This action will modify your project's configuration file and create a release commit",
            )
            .with_default(false)
            .prompt()?
        };
        
        if should_bump {
            make_bump_commit(repo, &mut project, config)?;
        }
    } else {
        println!("No release required")
    }

    Ok(())
}

fn major_version_one(
    mut project: Box<dyn Project>,
    repo: &Repository,
    config: &Config,
    auto_yes: bool,
) -> anyhow::Result<()> {
    if SemVer::version_1_0_0() <= project.get_version() {
        return Err(Error::msg(format!(
            "This repo already has released {}",
            SemVer::version_1_0_0()
        )));
    }
    
    println!(
        "New version: {} -> {}",
        project.get_version().clone(),
        SemVer::version_1_0_0(),
    );

    let should_release = if auto_yes {
        println!("Creating release v1.0.0");
        true
    } else {
        Confirm::new("Would you like to make the first release of this project?")
            .with_help_message(
                "By releasing version 1.0.0, you are declaring that this API is stable. \
                This action will modify your project's configuration file and create a release commit"
            )
            .with_default(false)
            .prompt()?
    };
    
    if should_release {
        make_initial_commit(repo, &mut project, config)?;
    }

    Ok(())
}