use anyhow::anyhow;
use git2::{
    Commit, DiffFormat, DiffOptions, Oid, Repository, Revwalk, Signature, Sort, StatusOptions,
};

use crate::{
    is_last_version_bump_clog, iterate_to_last_version, semver::SemVer, Config, HistoryItem,
    HistoryItemKind, Project,
};

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
    id: Oid,
    kind: HistoryItemKind,
}

impl CommitWrapper {
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
        let tree_entry = tree.get_path(project.get_version_file())?;
        let blob = tree_entry
            .to_object(repo)
            .expect("Null ptr converting tree to blob")
            .peel_to_blob()
            .expect("All commits expected to have blob");
        let text = std::str::from_utf8(blob.content())?.to_string();
        let version = project.parse_version_file(&text)?;
        let id = commit.id();
        let kind = Self::parse_commit_kind(&commit);
        Ok(Self {
            message: message.to_string(),
            version,
            id,
            kind,
        })
    }

    fn parse_commit_kind(commit: &Commit) -> HistoryItemKind {
        let message = commit.message().unwrap_or("");
        match message.contains(CLOG_TRAILER) {
            true => HistoryItemKind::ClogBump,
            false => HistoryItemKind::Normal,
        }
    }
}

impl HistoryItem for CommitWrapper {
    fn message(&self) -> String {
        self.message.clone()
    }

    fn version(&self) -> crate::semver::SemVer {
        self.version.clone()
    }

    fn kind(&self) -> HistoryItemKind {
        self.kind
    }

    fn commit_id(&self) -> Option<Oid> {
        Some(self.id)
    }
}

/// Create a bump commit on the current branch
pub fn create_clog_commit(
    repo: &Repository,
    project: &mut dyn Project,
    config: &Config,
    next_version: SemVer,
) -> anyhow::Result<()> {
    let message = make_clog_commit_message(&project.get_version(), &next_version);
    project.set_version(next_version.clone());
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
    let obj = repo.revparse_single("HEAD")?;
    let tag = format!("v{next_version}");
    repo.tag(&tag, &obj, &sig,&format!("Release {next_version}"), true)?;

    Ok(())
}

pub fn remove_last_release_commit(repo: &Repository, project: &dyn Project) -> anyhow::Result<()> {
    let history = GitHistory::new(project, repo);
    if !is_last_version_bump_clog(history) {
        return Err(anyhow!(
            "The last release was not performed by clog, cannot redo"
        ));
    }

    let history = GitHistory::new(project, repo);
    let mut commits: Vec<CommitWrapper> = iterate_to_last_version(history).collect();
    commits.reverse();

    let base = repo
        .find_commit(commits.first().map(|x| x.id).unwrap())?
        .parent(0)?;
    let head = repo.head()?;
    let branch_ref = head
        .name()
        .ok_or_else(|| anyhow::anyhow!("Detached HEAD not supported"))?;
    let branch_ref = branch_ref.to_string();
    repo.set_head_detached(base.id())?;
    repo.checkout_head(None)?;

    let mut iter = commits.iter();
    iter.next();
    for c in iter {
        let commit = repo.find_commit(c.id)?;
        let tree = commit.tree()?;

        let parent = repo.head()?.peel_to_commit()?;
        let parents = [&parent];

        repo.commit(
            Some("HEAD"),
            &commit.author(),
            &commit.committer(),
            commit.message().unwrap_or(""),
            &tree,
            &parents,
        )?;
    }

    let new_head = repo.head()?.target().unwrap();
    let mut reference = repo.find_reference(&branch_ref)?;
    reference.set_target(new_head, "drop release commit")?;
    repo.set_head(&branch_ref)?;

    Ok(())
}

pub fn is_repo_ready(repo: &Repository) -> bool {
    repo_has_commits(repo) && repo_is_clean(repo)
}

pub fn repo_has_commits(repo: &Repository) -> bool {
    repo.head().ok().and_then(|h| h.target()).is_some()
}

pub fn generate_diff_for_window(
    repo: &Repository,
    commits: &[CommitWrapper],
) -> anyhow::Result<String> {
    match (commits.first(), commits.last()) {
        (Some(newest), Some(oldest)) => diff_oids(repo, newest.id, oldest.id),
        _ => Ok(String::new()),
    }
}

pub(crate) fn diff_oids(repo: &Repository, newest: Oid, oldest: Oid) -> anyhow::Result<String> {
    let oldest_commit = repo.find_commit(oldest)?;
    let base_tree = match oldest_commit.parent(0) {
        std::result::Result::Ok(parent) => parent.tree()?,
        Err(_) => repo.find_tree(repo.treebuilder(None)?.write()?)?,
    };
    let head_tree = repo.find_commit(newest)?.tree()?;

    let mut opts = DiffOptions::new();
    opts.reverse(false)
        .force_text(true)
        .ignore_whitespace_eol(false)
        .ignore_whitespace_change(false)
        .ignore_whitespace(false)
        .include_ignored(false)
        .include_untracked(false)
        .patience(true)
        .minimal(true);

    let diff = repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut opts))?;

    let mut diff_str = String::new();
    diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
        let content = std::str::from_utf8(line.content()).unwrap_or("");
        diff_str.push_str(content);
        true
    })?;
    Ok(diff_str)
}

fn repo_is_clean(repo: &Repository) -> bool {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);
    repo.statuses(Some(&mut opts)).is_ok_and(|s| s.is_empty())
}

fn make_clog_commit_message(from: &SemVer, to: &SemVer) -> String {
    format!("chore: bump version {} -> {}\n\n{}", from, to, CLOG_TRAILER)
}

#[cfg(test)]
mod test {
    use std::fs;

    use crate::{
        detect_project,
        git::{create_clog_commit, make_clog_commit_message, CommitWrapper, CLOG_TRAILER},
        semver::SemVer,
        test_support::{empty_commit, init_python_repo_0_1_0},
        Config, HistoryItemKind,
    };
    use assert_fs::TempDir;
    use fs_extra::{copy_items, dir};
    use git2::Repository;
    use rstest::*;

    #[fixture]
    #[once]
    fn cached_pre_stable_repo_dir() -> TempDir {
        let tmp_dir = TempDir::new().unwrap();
        init_python_repo_0_1_0(&tmp_dir).unwrap();
        tmp_dir
    }

    #[fixture]
    fn pre_stable_repo_dir(cached_pre_stable_repo_dir: &TempDir) -> TempDir {
        let tmp_dir = TempDir::new().unwrap();
        let options = dir::CopyOptions::new();
        let items: Vec<_> = std::fs::read_dir(cached_pre_stable_repo_dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        copy_items(&items, &tmp_dir, &options).unwrap();
        tmp_dir
    }

    #[rstest]
    fn test_clog_commit_kind(pre_stable_repo_dir: TempDir) {
        let repo = Repository::open(&pre_stable_repo_dir).unwrap();
        let config = Config::new(&pre_stable_repo_dir);
        let project = detect_project(&config).unwrap();
        empty_commit(&repo, "feat: test commit\nthis is a test\ntrailer text").unwrap();
        let commit = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .and_then(|oid| repo.find_commit(oid).ok())
            .unwrap();
        let wrapper = CommitWrapper::parse_commit(project.as_ref(), &repo, commit).unwrap();
        assert_eq!(wrapper.kind, HistoryItemKind::Normal);

        empty_commit(
            &repo,
            &make_clog_commit_message(&SemVer::version_0_1_0(), &SemVer::new(0, 1, 1, None, None)),
        )
        .unwrap();
        let commit = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .and_then(|oid| repo.find_commit(oid).ok())
            .unwrap();

        let wrapper = CommitWrapper::parse_commit(project.as_ref(), &repo, commit).unwrap();
        assert_eq!(wrapper.kind, HistoryItemKind::ClogBump);
    }

    #[rstest]
    fn test_clog_commit_tag(pre_stable_repo_dir: TempDir) {
        let repo = Repository::open(&pre_stable_repo_dir).unwrap();
        let config = Config::new(&pre_stable_repo_dir);
        let mut project = detect_project(&config).unwrap();

        // make an empty changelog - we aren't verifying this in the test
        let changelog = pre_stable_repo_dir.join(project.get_changelog());
        fs::File::create(&changelog).unwrap();

        empty_commit(&repo, "feat: test commit\nthis is a test\ntrailer text").unwrap();
        let version = SemVer::parse("1.0.0").unwrap();
        create_clog_commit(&repo, project.as_mut(), &config, version.clone()).unwrap();

        let head_oid = repo.head().unwrap().target().unwrap();

        let tag_oid = repo
            .revparse_single(&format!("refs/tags/v{version}^{{commit}}"))
            .unwrap_or_else(|_| panic!("expected tag v{version} to exist"))
            .id();

        assert_eq!(tag_oid, head_oid, "tag {version} does not point to HEAD");
    }

    #[rstest]
    fn test_clog_commit_message(pre_stable_repo_dir: TempDir) {
        let repo = Repository::open(&pre_stable_repo_dir).unwrap();
        let config = Config::new(&pre_stable_repo_dir);
        let mut project = detect_project(&config).unwrap();

        // make an empty changelog - we aren't verifying this in the test
        let changelog = pre_stable_repo_dir.join(project.get_changelog());
        fs::File::create(&changelog).unwrap();

        empty_commit(&repo, "feat: test commit\nthis is a test\ntrailer text").unwrap();
        let version = SemVer::parse("1.0.0").unwrap();
        create_clog_commit(&repo, project.as_mut(), &config, version.clone()).unwrap();

        let head_oid = repo.head().unwrap().target().unwrap();
        let head_commit = repo.find_commit(head_oid).unwrap();

        let message = head_commit.message().unwrap();
        assert_eq!(
            message,
            format!("chore: bump version 0.1.0 -> {version}\n\n{CLOG_TRAILER}")
        );
    }
}
