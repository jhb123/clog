use std::{fs, io::Write, path::Path};

use anyhow::{anyhow, Context, Error};
use clap::{Parser, Subcommand};
use clog::{
    bump_project_version, detect_project, get_next_version, git::GitHistory, is_repo_ready,
    make_stable_release, semver::SemVer, Config,
};
use git2::Repository;
use inquire::Confirm;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Skip confirmation prompts (automatically answer yes)
    #[arg(short = 'y', long, global = true)]
    yes: bool,
}

#[derive(Default, Subcommand)]
enum Commands {
    #[default]
    Bump,
    Redo,
    Stable,
    InstallAliases,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let current_dir = Path::new("./");
    let config = Config::new(&current_dir);
    let repo = Repository::open(current_dir)
        .with_context(|| format!("Failed to open repo at {:?}", current_dir.canonicalize()))?;

    if !is_repo_ready(&repo) {
        return Err(anyhow!("Repo is not in a clean state. Commit your changes"));
    }

    match cli.command.unwrap_or_default() {
        Commands::Bump => bump_release(&repo, &config, cli.yes),
        Commands::Redo => redo_release(&repo, &config, cli.yes),
        Commands::Stable => major_version_one(&repo, &config, cli.yes),
        Commands::InstallAliases => install_aliases(current_dir),
    }
}

fn bump_release(repo: &Repository, config: &Config, auto_yes: bool) -> anyhow::Result<()> {
    let mut project = detect_project(config)?;
    let current_version = project.get_version().clone();
    let history = GitHistory::new(project.as_ref(), repo);
    let new_version = match get_next_version(history, config) {
        Some(v) => v,
        None => current_version.clone(),
    };

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
            bump_project_version(repo, project.as_mut(), config)?;
        }
    } else {
        println!("No release required")
    }

    Ok(())
}

fn major_version_one(repo: &Repository, config: &Config, auto_yes: bool) -> anyhow::Result<()> {
    let mut project = detect_project(config)?;

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
        make_stable_release(repo, project.as_mut(), config)?;
    }

    Ok(())
}

fn redo_release(repo: &Repository, config: &Config, auto_yes: bool) -> anyhow::Result<()> {
    let mut project = detect_project(config)?;

    let should_redo = if auto_yes {
        println!("Redoing last release");
        true
    } else {
        Confirm::new("Would you like to make the redo the most recent release of this project?")
            .with_help_message(
                "This action will perform a rebase on your project and recalculate the the release."
            )
            .with_default(false)
            .prompt()?
    };

    if should_redo {
        clog::redo_release(repo, project.as_mut(), config)?;
    }

    Ok(())
}

pub fn install_aliases(repo_root: &Path) -> anyhow::Result<()> {
    let git_config = include_str!("./static/.gitconfig.template");
    let prepare_commit_msg = include_str!("./static/.prepare-commit-msg.template");

    let config_path = repo_root.join(".git").join("config");

    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(config_path)?
        .write_all(git_config.as_bytes())?;

    let hooks_dir = repo_root.join(".git").join("hooks");
    fs::create_dir_all(&hooks_dir)?;

    let hook_path = hooks_dir.join("prepare-commit-msg");

    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&hook_path)?
        .write_all(prepare_commit_msg.as_bytes())?;

    Ok(())
}
