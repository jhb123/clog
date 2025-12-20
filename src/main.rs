use std::path::PathBuf;

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
    /// Sets parent directory of test repo
    #[clap(short, long, value_name = "FILE", default_value = "./")]
    path: PathBuf,
    #[clap(short, long, value_name = "initial-release", default_value = "false")]
    initial: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config = Config::new(&cli.path);
    let project = detect_project(&config)?;

    let repo = Repository::open(&cli.path)
        .with_context(|| format!("Failed to open repo at {:?}", cli.path))?;

    if !repo_has_commits(&repo) {
        return Err(anyhow!("Repo has no commits"));
    }

    if !repo_is_clean(&repo)? {
        return Err(anyhow!("Repo is not in a clean state. Commit your changes"));
    }

    if cli.initial {
        major_version_one(project, &repo, &config)
    } else {
        bump_release(project, &repo, &config)
    }
}

fn bump_release(
    mut project: Box<dyn Project>,
    repo: &Repository,
    config: &Config,
) -> anyhow::Result<()> {
    let current_version = project.get_version().clone();
    let new_version = get_next_version(repo, &project, config).unwrap();
    if new_version > current_version {
        let ans = Confirm::new(&format!(
            "would you like to bump this project's version from {} {}",
            current_version, new_version
        ))
        .with_help_message(
            "This action will modify your project's configuration file and create a release commit",
        )
        .with_default(false)
        .prompt()?;
        if ans {
            make_bump_commit(repo, &mut project, config).unwrap();
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

    let ans = Confirm::new("Would you like to make the first release this project?")
        .with_help_message("By releasing version 1.0.0, you are declaring that this API is stable. This action will modify your project's configuration file and create a release commit")
        .with_default(false)
        .prompt()?;
    if ans {
        make_initial_commit(repo, &mut project, config)?;
    }

    Ok(())
}

//
