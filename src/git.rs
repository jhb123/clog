use git2::{Commit, Repository, Revwalk, Signature, Sort, StatusOptions};

use crate::{semver::SemVer, Config, HistoryItem, Project};

static CLOG_TRAILER: &str = "Bumped-by: clog";

pub struct GitHistory<'repo> {
    project: &'repo dyn Project,
    repo: &'repo Repository,
    revwalk: Revwalk<'repo>,
}

impl<'repo> GitHistory<'repo> {
    pub fn new(project: &'repo dyn Project, repo: &'repo Repository) -> Self {
        let mut revwalk = repo.revwalk().unwrap();
        revwalk.set_sorting(Sort::TOPOLOGICAL).unwrap();
        revwalk.push_head().unwrap();
        Self {
            project,
            repo,
            revwalk,
        }
    }
}

impl<'repo> Iterator for GitHistory<'repo> {
    type Item = CommitWrapper;

    fn next(&mut self) -> Option<Self::Item> {
        self.revwalk
            .by_ref()
            .filter_map(|oid| oid.ok())
            .filter_map(|oid| self.repo.find_commit(oid).ok())
            .filter_map(|commit| CommitWrapper::parse_commit(self.project, self.repo, commit).ok())
            .next()
    }
}

#[derive(Debug, Clone)]
pub struct CommitWrapper {
    message: String,
    version: crate::semver::SemVer,
}

impl CommitWrapper {
    pub fn new(message: &str, version: SemVer) -> Self {
        Self {
            message: message.to_string(),
            version,
        }
    }

    pub fn parse_commit(
        project: &dyn Project,
        repo: &Repository,
        commit: Commit,
    ) -> anyhow::Result<Self> {
        let message = commit
            .message()
            .expect("Should not parse commits with no message to a history item")
            .to_string();

        let tree = commit.tree().unwrap();
        let tree_entry = tree.get_path(project.get_version_file()).unwrap();
        let blob = tree_entry
            .to_object(repo)
            .expect("Null ptr converting tree to blob")
            .peel_to_blob()
            .expect("All commits expected to have blob");
        let text = std::str::from_utf8(blob.content())?.to_string();
        let version = project.parse_version_file(&text)?;

        Ok(Self { message, version })
    }
}

impl HistoryItem for CommitWrapper {
    fn message(&self) -> String {
        self.message.clone()
    }

    fn version(&self) -> crate::semver::SemVer {
        self.version.clone()
    }
}

/// Create a bump commit on the current branch
pub fn create_clog_commit(
    repo: &Repository,
    project: &mut dyn Project,
    config: &Config,
    next_version: SemVer,
) -> anyhow::Result<()> {
    let message = format!(
        "chore: bump version {} -> {}\n\n{}",
        project.get_version(),
        next_version,
        CLOG_TRAILER
    );
    project.set_version(next_version);
    project.update_project_file()?;
    // get your user?
    let sig = match repo.signature() {
        Ok(s) => s,
        Err(_) => Signature::now(&config.name, &config.email)?,
    };
    let tree_id = {
        let mut index = repo.index().unwrap();
        let version_file = project.get_version_file();
        index.add_path(version_file)?;

        let change_log = project.get_changelog();
        index.add_path(change_log)?;

        for file in project.get_extra_files(config)? {
            index.add_path(&file)?
        }
        index.write()?;
        index.write_tree()?
    };

    let tree = repo.find_tree(tree_id)?;

    let parent_commit = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok());

    if let Some(parent) = parent_commit {
        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&parent])?;
    } else {
        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[])?;
    };

    Ok(())
}

pub fn is_repo_ready(repo: &Repository) -> bool {
    repo_has_commits(repo) && repo_is_clean(repo)
}

pub fn repo_has_commits(repo: &Repository) -> bool {
    repo.head().ok().and_then(|h| h.target()).is_some()
}

fn repo_is_clean(repo: &Repository) -> bool {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);
    repo.statuses(Some(&mut opts)).is_ok_and(|s| s.is_empty())
}
