use anyhow::Ok;
use clap::{Parser, ValueEnum};
use clog::repo_has_commits;
use git2::{build::CheckoutBuilder, Commit, Oid, Repository, Signature};
use inquire::Confirm;
use names::{Generator, Name};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Default, Debug)]
enum RepoStyle {
    /// Repo with linear history
    #[default]
    Simple,
    /// Repo with merge commit history
    Branches,
}

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
        RepoStyle::Simple => simple_repo(&cli.path),
        RepoStyle::Branches => branches_repo(&cli.path),
    }
}

fn simple_repo(path: &Path) -> anyhow::Result<()> {
    let mut path = path.to_path_buf();
    path.push("simple");
    let repo = init_repo(&path)?;

    check_repo_has_commits(&repo)?;
    if repo_has_commits(&repo) {
        empty_commit(&repo, "test: fix tests")?;
    } else {
        empty_commit(&repo, "feat: initial commit")?;
    }

    empty_commit(&repo, "feat: initial commit")?;
    empty_commit(&repo, "fix: correct logic in parser")?;
    empty_commit(&repo, "docs: add README")?;
    empty_commit(&repo, "chore: cleanup build script")?;

    Ok(())
}

fn branches_repo(path: &Path) -> anyhow::Result<()> {
    let mut path = path.to_path_buf();
    path.push("branches");
    let repo = init_repo(&path)?;
    check_repo_has_commits(&repo)?;
    if repo_has_commits(&repo) {
        empty_commit(&repo, "test: fix tests")?;
    } else {
        empty_commit(&repo, "feat: initial commit")?;
    }

    let mut generator = Generator::with_naming(Name::Numbered);
    let branch_name = generator.next().unwrap();

    let feature_commit = make_branch(&repo, &branch_name, |repo| {
        empty_commit(repo, "feat: add feature A")?;
        empty_commit(repo, "fix: bug in A")?;
        empty_commit(repo, "chore: formatting for feature A")
    })?;

    // Commit on main to make history interesting
    empty_commit(&repo, "chore: update CI config")?;

    let master_commit = repo.head()?.peel_to_commit()?;

    merge_commits(
        &repo,
        "merge: feature branch",
        &[&master_commit, &feature_commit],
    )?;

    Ok(())
}

fn check_repo_has_commits(repo: &Repository) -> Result<(), anyhow::Error> {
    if repo_has_commits(repo) {
        let ans =
            Confirm::new("This repo is not empty. Do you want to add new commits to this repo?")
                .with_default(false)
                .prompt()?;
        if !ans {
            return Ok(());
        }
    };
    Ok(())
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
            println!("Nothing happened");
        }
    }

    let repo = Repository::open(path)
        .or_else(|_| {
            println!("Initializing new repo in {}", path.display());
            Repository::init(path)
        })
        .map_err(|_| anyhow::anyhow!("Failed to open repo"))?;

    Ok(repo)
}

/// Create an empty commit with a message on the current branch
fn empty_commit(repo: &Repository, message: &str) -> anyhow::Result<Oid> {
    let sig = Signature::now("Test User", "test@example.com")?;

    // get git into the "stage changes" stage

    // tree id is a hash of the tree object in the git object db
    let tree_id = {
        // the inedx is basically the staging area. It is a file
        // that stores what will go into the next commit gitbook, p97
        let mut index = repo.index()?;

        // add current state of index to git object db as a tree
        index.write_tree()?
    };

    /* The git tree fits into the rest of git like:

    blob -> raw file content
    tree -> directory listing (maps filenames to blobs and subtrees).
    Commit → metadata + pointer to a tree + parent commits.

    It records the names of files and directories.
    For each entry, it stores:
    - File mode (executable bit, symlink, etc.)
    - Name (file1.txt, src/, etc.)
    - SHA (the object ID of the blob or subtree it points to).
    */

    let tree = repo.find_tree(tree_id)?;

    let parent_commit = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok());

    let commit_id = if let Some(parent) = parent_commit {
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?
    } else {
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])?
    };

    Ok(commit_id)
}

/// Creates a new branch from `main`, checks it out, and runs a user-provided closure
/// to perform commits or changes on that branch. After the closure finishes, the
/// branch is left in place but `HEAD` is restored back to `main`.
fn make_branch<'a, F>(repo: &'a Repository, name: &'a str, f: F) -> anyhow::Result<Commit<'a>>
where
    F: FnOnce(&Repository) -> anyhow::Result<Oid>,
{
    // head can point to a tag, a branch refs/heads/main, or a detatched state
    // peel gives the commit to of whatever head points to

    let head = repo.head()?.peel_to_commit()?;
    let branch = repo.branch(name, &head, false)?;
    let branch_ref = branch.into_reference();

    repo.set_head(branch_ref.name().unwrap())?;
    repo.checkout_head(Some(CheckoutBuilder::default().force()))?;
    f(repo)?;
    let branch_commit = repo.head()?.peel_to_commit()?;

    // switch back to main
    repo.set_head("refs/heads/main")?;
    // updates head ref and updates the working dir + index etc.
    repo.checkout_head(Some(CheckoutBuilder::default().force()))?;

    Ok(branch_commit)
}

/// Creates a merge commit with the given message and parents.
/// Uses the tree from the first parent.
fn merge_commits(
    repo: &Repository,
    message: &str,
    parents: &[&git2::Commit],
) -> anyhow::Result<()> {
    let sig = Signature::now("Test User", "test@example.com")?;
    // a commit needs a tree i.e. a snapshot of the repo at that time
    // - what files exist
    // - what are the contents of the file
    // - how the directories look.
    // a commit is basically = this tree + these parents.
    // this means you can checkout diffs faster, rather working
    // back from the initial commit to work out what the
    let tree = parents[0].tree()?;
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, parents)?;
    Ok(())
}
