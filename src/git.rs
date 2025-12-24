use git2::{Commit, Repository, Revwalk, Sort};

use crate::{HistoryItem, Project};

pub struct GitHistory<'repo> {
    project: &'repo dyn Project,
    repo: &'repo Repository,
    revwalk: Revwalk<'repo>,
}

// impl<'repo> Clone for GitHistory<'repo> {
//     fn clone(&self) -> Self {
//         let mut revwalk = self.repo.revwalk().unwrap();
//         revwalk.set_sorting(Sort::TOPOLOGICAL).unwrap();
//         revwalk.push_head().unwrap();
//         Self { project: self.project, repo: self.repo, revwalk }
//     }
// }

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
            .filter_map(|commit| CommitWrapper::new(self.project, self.repo, commit).ok())
            .next()
    }
}

#[derive(Debug, Clone)]
pub struct CommitWrapper {
    message: String,
    version: crate::semver::SemVer,
}

impl CommitWrapper {
    pub fn new(project: &dyn Project, repo: &Repository, commit: Commit) -> anyhow::Result<Self> {
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
