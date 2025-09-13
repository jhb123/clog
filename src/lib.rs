mod python;
mod rust;
pub mod semver;

use std::{
    path::{Path, PathBuf},
    vec,
};

use anyhow::anyhow;
use git2::{Commit, Oid, Repository, Signature, Sort};
use regex::Regex;

use crate::{
    python::PyProject,
    rust::CargoProject,
    semver::{SemVer, SemVerBump},
};

static CLOG_TRAILER: &str = "Bumped-by: clog";

pub trait Project {
    fn get_version(&self) -> SemVer;
    fn from_dir(path: &Path) -> anyhow::Result<Self>
    where
        Self: Sized;
    fn bump(&mut self, bump: SemVerBump);
    fn write(&self) -> anyhow::Result<()>;
    fn get_version_file(&self) -> PathBuf;
    fn set_initial_release(&mut self) -> anyhow::Result<()>;
}

pub struct Config {
    patterns: Patterns,
    path: PathBuf,
    name: String,
    email: String,
}

impl Config {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            patterns: Patterns::default(),
            ..Default::default()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let path = PathBuf::from("./");
        Self {
            path,
            patterns: Patterns::default(),
            name: "clog-bot".to_string(),
            email: "clog-bot@local".to_string(),
        }
    }
}

pub struct Patterns {
    major: Vec<Regex>,
    minor: Vec<Regex>,
    patch: Vec<Regex>,
}

impl Default for Patterns {
    fn default() -> Self {
        Self {
            major: vec![Regex::new(r"^.*!:.*$").unwrap()],
            minor: vec![Regex::new(r"^feat:.*$").unwrap()],
            patch: vec![Regex::new(r"^fix:.*$").unwrap()],
        }
    }
}

pub fn repo_has_commits(repo: &Repository) -> bool {
    repo.head().ok().and_then(|h| h.target()).is_some()
}

pub fn parse_commit_message(commit: &Commit, config: &Config) -> SemVerBump {
    if let Some(message) = commit.message() {
        let message = message.trim();
        if config.patterns.major.iter().any(|r| r.is_match(message)) {
            SemVerBump::Major
        } else if config.patterns.minor.iter().any(|r| r.is_match(message)) {
            SemVerBump::Minor
        } else if config.patterns.patch.iter().any(|r| r.is_match(message)) {
            SemVerBump::Patch
        } else {
            SemVerBump::None
        }
    } else {
        SemVerBump::None
    }
}

fn is_clog_bump(commit: &Commit) -> bool {
    if let Some(msg) = commit.message() {
        msg.lines()
            .any(|line| line.trim_start().starts_with(CLOG_TRAILER))
    } else {
        false
    }
}

// pub fn get_initial_release(repo: &Repository, upto_oid: Oid) -> anyhow::Result<Option<Oid>> {
//     let mut revwalk = repo.revwalk()?;
//     revwalk.set_sorting(Sort::TOPOLOGICAL)?;
//     revwalk.push(upto_oid)?;
//     for oid in revwalk.flatten() {
//         let commit = repo.find_commit(oid)?;
//         if is_clog_bump(&commit) {
//             return Ok(Some(commit.id()));
//         }
//     }
//     Ok(None)
// }

pub fn get_prev_clog_bump(repo: &Repository, upto_oid: Oid) -> anyhow::Result<Option<Oid>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TOPOLOGICAL)?;
    revwalk.push(upto_oid)?;

    for oid in revwalk.flatten() {
        let commit = repo.find_commit(oid)?;
        if is_clog_bump(&commit) {
            return Ok(Some(commit.id()));
        }
    }
    Ok(None)
}

pub fn detect_project(config: &Config) -> anyhow::Result<Box<dyn Project>> {
    if config.path.join("Cargo.toml").exists() {
        Ok(Box::new(CargoProject::from_dir(&config.path)?))
    } else if config.path.join("pyproject.toml").exists() {
        Ok(Box::new(PyProject::from_dir(&config.path)?))
    } else {
        Err(anyhow::anyhow!("No supported project file found"))
    }
}

/// Create a bump commit on the current branch
pub fn make_bump_commit(
    repo: &Repository,
    project: &mut Box<dyn Project>,
    bump: SemVerBump,
    config: &Config,
) -> anyhow::Result<Oid> {
    let message = format!(
        "chore: bump version {} -> {}\n\n{}",
        project.get_version(),
        project.get_version().bump(bump),
        CLOG_TRAILER
    );
    project.bump(bump);
    project.write()?;
    // get your user?
    let sig = match repo.signature() {
        Ok(s) => s,
        Err(_) => Signature::now(&config.name, &config.email)?,
    };

    let tree_id = {
        let mut index = repo.index()?;
        let rel_path = project.get_version_file();
        index.add_path(&rel_path)?;
        index.write()?;
        index.write_tree()?
    };

    let tree = repo.find_tree(tree_id)?;

    let parent_commit = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok());

    let commit_id = if let Some(parent) = parent_commit {
        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&parent])?
    } else {
        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[])?
    };

    Ok(commit_id)
}

/// Create the inital release commit on the current branch
pub fn make_initial_commit(
    repo: &Repository,
    project: &mut Box<dyn Project>,
    config: &Config,
) -> anyhow::Result<Oid> {
    let message = format!(
        "chore: bump version {} -> {}\n\n{}",
        project.get_version(),
        SemVer::version_1_0_0(),
        CLOG_TRAILER
    );

    project.set_initial_release()?;
    project.write()?;

    // get your user?
    let sig = match repo.signature() {
        Ok(s) => s,
        Err(_) => Signature::now(&config.name, &config.email)?,
    };

    let tree_id = {
        let mut index = repo.index()?;
        let rel_path = project.get_version_file();
        index.add_path(&rel_path)?;
        index.write()?;
        index.write_tree()?
    };

    let tree = repo.find_tree(tree_id)?;

    let parent_commit = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok());

    let commit_id = if let Some(parent) = parent_commit {
        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&parent])?
    } else {
        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[])?
    };

    Ok(commit_id)
}
