use clap::Parser;
use clog::{semver::SemVer, test_support::*};
use git2::Repository;
use inquire::Confirm;
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Style of repo to generate
    #[arg(value_enum, default_value_t = RepoStyle::Simple)]
    repo: RepoStyle,

    /// Sets parent directory of test repo
    #[clap(short, long, value_name = "FILE", default_value = "./test_repos")]
    path: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    println!(
        "Creating test repo ({:?}) in {}",
        cli.repo,
        cli.path.display()
    );

    match cli.repo {
        RepoStyle::Simple => simple_repo(&cli.path, init_repo),
        RepoStyle::Branches => branches_repo(&cli.path, init_repo),
    }
}

fn init_repo(path: &PathBuf) -> anyhow::Result<Repository> {
    if path.exists() {
        let ans = Confirm::new("Do you want to wipe out the contents of this directory?")
            .with_default(false)
            .with_help_message("This action will allow us to set up a new repo.")
            .prompt()?;

        if ans {
            println!("Creating a brand new repo");
            fs::remove_dir_all(path)?;
            fs::create_dir_all(path)?;
        } else {
            println!("Appending new commits to repo");
        }
    }

    init_python_repo(path, Some(SemVer::parse("0.1.0").unwrap()))
}
