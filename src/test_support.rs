use anyhow::Ok;
use clap::ValueEnum;
use git2::{build::CheckoutBuilder, Commit, Oid, Repository, Signature};
use inquire::Confirm;
use names::{Generator, Name};
use std::{
    fs,
    path::Path,
    process::exit,
};

use crate::{Project, python::PyProject, repo_has_commits, semver::SemVer};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Default, Debug)]
pub enum RepoStyle {
    /// Repo with linear history
    #[default]
    Simple,
    /// Repo with merge commit history
    Branches,
}

pub fn simple_repo<P: AsRef<std::path::Path>, F: Fn(&P) -> anyhow::Result<Repository>>(path: &P, init_repo: F) -> anyhow::Result<()> {
    let repo = init_repo(path)?;

    empty_commit(
        &repo,
        "fix: correct logic in parser\nextra detail\nfooter notes",
    )?;
    empty_commit(&repo, "docs: add README")?;
    empty_commit(&repo, "feat!: breaking change")?;
    empty_commit(&repo, "chore: cleanup build script")?;

    Ok(())
}

pub fn branches_repo<P: AsRef<std::path::Path>, F: Fn(&P) -> anyhow::Result<Repository>>(path: &P, init_repo: F) -> anyhow::Result<()> {
    let repo = init_repo(path)?;
    let mut generator = Generator::with_naming(Name::Numbered);
    let branch_name = generator.next().unwrap();

    let feature_commit = make_branch(&repo, &branch_name, |repo| {
        empty_commit(repo, "feat: add feature A")?;
        empty_commit(repo, "fix!: bug in A")?;
        empty_commit(repo, "chore: formatting for feature A")
    })?;

    let branch_name = generator.next().unwrap();
    let feature_commit_2 = make_branch(&repo, &branch_name, |repo| {
        empty_commit(repo, "feat: add feature B")?;
        empty_commit(repo, "fix: bug in B")?;
        empty_commit(repo, "chore: formatting for feature B")
    })?;

    // switch back to main
    repo.set_head("refs/heads/main")?;
    // updates head ref and updates the working dir + index etc.
    repo.checkout_head(Some(CheckoutBuilder::default().force()))?;

    // Commit on main to make history interesting
    empty_commit(&repo, "chore: update CI config")?;

    let master_commit = repo.head()?.peel_to_commit()?;

    merge_commits(
        &repo,
        "merge: feature branch A",
        &[&master_commit, &feature_commit],
    )?;

    let master_commit = repo.head()?.peel_to_commit()?;

    merge_commits(
        &repo,
        "merge: feature branch B",
        &[&master_commit, &feature_commit_2],
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



pub fn init_python_repo<P: AsRef<std::path::Path>>(path: &P, version: Option<SemVer>) -> anyhow::Result<Repository> {
    let repo = Repository::open(path)
        .or_else(|_| {
            Repository::init(path)
        })
        .map_err(|_| anyhow::anyhow!("Failed to open repo"))?;
    if !repo_has_commits(&repo) {
        pyproject_init_commit(&repo, version)?;
    }
    Ok(repo)
}

/// Given a directory containing a python project, parse the
/// version from the pyproject.toml
pub fn get_python_pyroject_version<P: AsRef<std::path::Path>>(dir: &P) -> anyhow::Result<SemVer>{
    let p = PyProject::from_dir(dir.as_ref())?;
    Ok(p.get_version())
}

/// Create an empty commit with a message on the current branch
pub fn empty_commit(repo: &Repository, message: &str) -> anyhow::Result<Oid> {
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

/// Creates a new branch from `HEAD`, checks it out, and runs a user-provided closure
/// to perform commits or changes on that branch.
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

fn make_pyproject(path: &Path, version: Option<SemVer>) {
    let mut data = include_str!("pyproject.toml.example").to_string();
    if let Some(v) = version {
        data = data.replace("0.1.0", &v.to_string());
    }
    fs::write(path, format!("{data}")).unwrap();
}

/// Create a commit with a message on the current branch
fn pyproject_init_commit(repo: &Repository, version: Option<SemVer>) -> anyhow::Result<()> {
    let sig = Signature::now("Test User", "test@example.com")?;

    let mut pyproject_path = repo
        .path()
        .parent()
        .expect("git repo has no parent")
        .to_path_buf();
    pyproject_path.push("pyproject.toml");
    make_pyproject(&pyproject_path, version);

    let tree_id = {
        let mut index = repo.index()?;
        index.add_path(Path::new("pyproject.toml"))?;
        index.write()?;
        index.write_tree()?
    };

    let tree = repo.find_tree(tree_id)?;

    let parent_commit = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok());

    if parent_commit.is_some() {
        eprintln!("Cannot initialise a repo that has already been created");
        exit(1)
    }
    repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;
    Ok(())
}
