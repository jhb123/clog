mod changelog;
mod python;
mod rust;
pub mod semver;

use std::{
    path::{Path, PathBuf},
    vec,
};

use git2::{Commit, Oid, Repository, Signature, Sort, StatusOptions};
use once_cell::sync::Lazy;
use regex::Regex;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use crate::{
    python::PyProject,
    rust::CargoProject,
    semver::{SemVer, SemVerBump},
};

static CLOG_TRAILER: &str = "Bumped-by: clog";

static DEFAULT_PATTERNS: Lazy<Patterns> = Lazy::new(|| Patterns {
    major: vec![Regex::new(r"^.*!:").unwrap()],
    minor: vec![Regex::new(r"^feat:").unwrap()],
    patch: vec![Regex::new(r"^fix:").unwrap()],
});

pub trait Project {
    fn get_version(&self) -> SemVer;
    fn from_dir(path: &Path) -> anyhow::Result<Self>
    where
        Self: Sized;
    fn get_dir(&self) -> &Path;
    fn bump(&mut self, bump: SemVerBump);
    fn write(&self) -> anyhow::Result<()>;
    fn get_version_file(&self) -> &Path; // needs to be dyn compatible
    fn set_initial_release(&mut self) -> anyhow::Result<()>;
    fn get_latest_release(&self, repo: &Repository) -> anyhow::Result<Option<Oid>> {
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(Sort::TOPOLOGICAL)?;

        for oid_result in revwalk {
            let oid = oid_result?; // fail if object db is corrupt
                                   // revwalk only has commits because we didn't pass anything
                                   // apart from commits to the revwalk
            let commit = repo.find_commit(oid)?;

            // handle first commit...
            let tree = commit.tree()?;

            let parent = match commit.parent(0) {
                Ok(c) => c,
                Err(_) => {
                    if tree.get_path(self.get_version_file()).is_ok() {
                        return Ok(Some(oid)); // version file first appears here
                    }
                    return Ok(None);
                }
            };

            let parent_tree = parent.tree()?;

            let entry = tree.get_path(self.get_version_file());
            let parent_entry = parent_tree.get_path(self.get_version_file());

            let changed = match (entry, parent_entry) {
                (Ok(e), Ok(pe)) => {
                    // check the files contents are acually different and it
                    // not a change in file permission
                    let oid_e = e.to_object(repo)?.peel_to_blob()?.id();
                    let oid_pe = pe.to_object(repo)?.peel_to_blob()?.id();
                    oid_e != oid_pe // changed
                }
                (Ok(_), Err(_)) => true,   // removed
                (Err(_), Ok(_)) => true,   // added
                (Err(_), Err(_)) => false, //no change
            };

            if changed {
                let blob = parent_tree
                    .get_path(self.get_version_file())?
                    .to_object(repo)?
                    .peel_to_blob()?;
                let raw = std::str::from_utf8(blob.content())?;
                let changed_version = self.parse_version_file(raw)?;
                let current_version = self.get_version();
                if changed_version < current_version {
                    return anyhow::Ok(Some(oid));
                }
            }
        }
        Err(git2::Error::from_str("No commit found that changes project's version").into())
    }
    fn parse_version_file(&self, unparsed_str: &str) -> anyhow::Result<SemVer>;
    fn get_extra_files(&self, config: &Config) -> anyhow::Result<Vec<PathBuf>>;
    fn get_changelog(&self) -> &Path {
        Path::new("Changelog.md")
    }
}

pub struct Config {
    patterns: Patterns,
    path: PathBuf,
    name: String,
    email: String,
}

impl Config {
    pub fn new<P: AsRef<std::path::Path>>(path: &P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
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

#[derive(Debug, Clone)]
pub struct Patterns {
    major: Vec<Regex>,
    minor: Vec<Regex>,
    patch: Vec<Regex>,
}

impl Default for Patterns {
    fn default() -> Self {
        DEFAULT_PATTERNS.clone()
    }
}

pub fn repo_has_commits(repo: &Repository) -> bool {
    repo.head().ok().and_then(|h| h.target()).is_some()
}

fn parse_commit_message(commit: &Commit, config: &Config) -> SemVerBump {
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

fn get_changelog_message(commit: &Commit, config: &Config) -> Option<String> {
    let mut patterns = config
        .patterns
        .major
        .iter()
        .chain(&config.patterns.minor)
        .chain(&config.patterns.patch);

    let message = commit.message()?.split("\n").next()?;

    if patterns.any(|r| r.is_match(message)) {
        Some(message.to_string())
    } else {
        None
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

pub fn repo_is_clean(repo: &Repository) -> anyhow::Result<bool> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    Ok(statuses.is_empty())
}

fn get_latest_release(repo: &Repository, project: &dyn Project) -> anyhow::Result<Option<Oid>> {
    match project.get_latest_release(repo)? {
        Some(oid) => Ok(Some(oid)),
        None => get_prev_clog_bump(repo),
    }
}

fn calculate_bump(
    repo: &Repository,
    project: &dyn Project,
    config: &Config,
) -> anyhow::Result<SemVerBump> {
    let since_oid = get_latest_release(repo, project)?;
    let upto_oid = get_head(repo).unwrap();
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TOPOLOGICAL)?;
    revwalk.push(upto_oid)?;

    if let Some(since) = since_oid {
        revwalk.hide(since)?;
    }

    let mut bump = SemVerBump::None;
    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        let bump_kind = parse_commit_message(&commit, config);
        bump = std::cmp::max(bump, bump_kind);
    }
    Ok(bump)
}

fn get_prev_clog_bump(repo: &Repository) -> anyhow::Result<Option<Oid>> {
    let upto_oid = get_head(repo).unwrap();
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
    project: &mut dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    changelog::prepare_changelog(repo, project, config).unwrap();

    let bump = calculate_bump(repo, project, config).unwrap();
    if bump == SemVerBump::None {
        return Ok(());
    }
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

/// Create the initial release commit on the current branch
pub fn make_initial_stable_commit(
    repo: &Repository,
    project: &mut dyn Project,
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
        index.add_path(rel_path)?;
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

fn get_head(repo: &Repository) -> Option<Oid> {
    repo.head().ok().and_then(|h| h.target())
}

pub fn get_next_version(
    repo: &Repository,
    project: &dyn Project,
    config: &Config,
) -> anyhow::Result<SemVer> {
    let bump = calculate_bump(repo, project, config)?;
    Ok(project.get_version().bump(bump))
}

fn is_version_bump(
    commit: &Commit,
    repo: &Repository,
    project: &dyn Project,
) -> anyhow::Result<Option<SemVer>> {
    if commit.parent_count() == 0 {
        return Ok(None);
    }

    let parent = commit.parent(0)?;

    let prev = version_at_commit(&parent, repo, project)?;
    let curr = version_at_commit(commit, repo, project)?;

    if prev != curr {
        Ok(Some(curr))
    } else {
        Ok(None)
    }
}

fn version_at_commit(
    commit: &Commit,
    repo: &Repository,
    project: &dyn Project,
) -> anyhow::Result<SemVer> {
    let tree = commit.tree()?;
    let tree_entry = tree.get_path(project.get_version_file())?;
    let blob = tree_entry.to_object(repo)?.peel_to_blob()?;
    let text = std::str::from_utf8(blob.content())?.to_string();
    project.parse_version_file(&text)
}

fn find_first_version_of_project(
    repo: &Repository,
    project: &mut dyn Project,
) -> anyhow::Result<Option<SemVer>> {
    let upto_oid = get_head(repo).unwrap();
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TOPOLOGICAL)?;
    revwalk.push(upto_oid)?;
    let mut version = None;

    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        if let Ok(v) = version_at_commit(&commit, repo, project) {
            version = Some(v);
        }
    }
    Ok(version)
}

#[cfg(test)]
mod test {
    use assert_fs::TempDir;
    use fs_extra::{copy_items, dir};
    use git2::Repository;
    use rstest::{fixture, rstest};

    use crate::test_support::*;
    use crate::*;

    #[fixture]
    #[once]
    fn cached_pre_stable_repo_dir() -> TempDir {
        let tmp_dir = TempDir::new().unwrap();
        init_python_repo_0_1_0(&tmp_dir).unwrap();
        tmp_dir
    }

    #[fixture]
    #[once]
    fn cached_stable_repo_dir() -> TempDir {
        let tmp_dir = TempDir::new().unwrap();
        init_python_repo_1_0_0(&tmp_dir).unwrap();
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

    #[fixture]
    fn stable_repo_dir(cached_stable_repo_dir: &TempDir) -> TempDir {
        let tmp_dir = TempDir::new().unwrap();
        let options = dir::CopyOptions::new();
        let items: Vec<_> = std::fs::read_dir(cached_stable_repo_dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        copy_items(&items, &tmp_dir, &options).unwrap();
        tmp_dir
    }

    #[fixture]
    fn pre_stable_simple_repo_dir() -> TempDir {
        let tmp_dir = TempDir::new().unwrap();
        simple_repo(&tmp_dir, init_python_repo_0_1_0).unwrap();
        tmp_dir
    }

    #[fixture]
    fn pre_stable_branches_repo_dir() -> TempDir {
        let tmp_dir = TempDir::new().unwrap();
        branches_repo(&tmp_dir, init_python_repo_0_1_0).unwrap();
        tmp_dir
    }

    #[fixture]
    fn stable_simple_repo_dir() -> TempDir {
        let tmp_dir = TempDir::new().unwrap();
        simple_repo(&tmp_dir, init_python_repo_1_0_0).unwrap();
        tmp_dir
    }

    #[fixture]
    fn stable_branches_repo_dir() -> TempDir {
        let tmp_dir = TempDir::new().unwrap();
        branches_repo(&tmp_dir, init_python_repo_1_0_0).unwrap();
        tmp_dir
    }

    fn test_bump_helper(dir: &TempDir, repo: &Repository) {
        let config = Config::new(dir);
        let mut project = detect_project(&config).unwrap();
        make_bump_commit(repo, project.as_mut(), &config).unwrap();
    }

    fn test_initial_stable_helper(dir: &TempDir, repo: &Repository) {
        let config = Config::new(dir);
        let mut project = detect_project(&config).unwrap();
        make_initial_stable_commit(repo, project.as_mut(), &config).unwrap();
    }

    #[rstest]
    fn test_make_bump_commit_prestable(
        pre_stable_repo_dir: TempDir,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg1: CommitCase,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg2: CommitCase,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg3: CommitCase,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg4: CommitCase,
    ) {
        let repo = Repository::open(&pre_stable_repo_dir).unwrap();
        empty_commit(&repo, msg1.msg).unwrap();
        empty_commit(&repo, msg2.msg).unwrap();
        empty_commit(&repo, msg3.msg).unwrap();
        empty_commit(&repo, msg4.msg).unwrap();

        let v1 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();
        assert_eq!(v1, SemVer::version_0_1_0());
        test_bump_helper(&pre_stable_repo_dir, &repo);
        let v2 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();
        let expected_bump = [msg1.bump, msg2.bump, msg3.bump, msg4.bump]
            .into_iter()
            .max()
            .unwrap();
        assert_eq!(v2, v1.bump(expected_bump));
        if expected_bump != SemVerBump::None {
            assert_clog_commit_version(&pre_stable_repo_dir, v1.bump(expected_bump))
        }
    }

    #[rstest]
    fn test_make_bump_commit_stable(
        stable_repo_dir: TempDir,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg1: CommitCase,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg2: CommitCase,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg3: CommitCase,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg4: CommitCase,
    ) {
        let repo = Repository::open(&stable_repo_dir).unwrap();
        empty_commit(&repo, msg1.msg).unwrap();
        empty_commit(&repo, msg2.msg).unwrap();
        empty_commit(&repo, msg3.msg).unwrap();
        empty_commit(&repo, msg4.msg).unwrap();

        let v1 = get_python_pyroject_version(&stable_repo_dir).unwrap();
        assert_eq!(v1, SemVer::version_1_0_0());
        test_bump_helper(&stable_repo_dir, &repo);
        let v2 = get_python_pyroject_version(&stable_repo_dir).unwrap();
        let expected_bump = [msg1.bump, msg2.bump, msg3.bump, msg4.bump]
            .into_iter()
            .max()
            .unwrap();
        assert_eq!(v2, v1.bump(expected_bump));
        if expected_bump != SemVerBump::None {
            assert_clog_commit_version(&stable_repo_dir, v1.bump(expected_bump))
        }
    }
    #[rstest]
    #[case(pre_stable_simple_repo_dir())]
    #[case(pre_stable_branches_repo_dir())]
    fn test_version_1_version_file(#[case] repo_dir: TempDir) {
        let repo = Repository::open(&repo_dir).unwrap();
        let v1 = get_python_pyroject_version(&repo_dir).unwrap();
        assert_eq!(v1, SemVer::new(0, 1, 0, None, None));
        test_initial_stable_helper(&repo_dir, &repo);
        let v2 = get_python_pyroject_version(&repo_dir).unwrap();
        assert_eq!(v2, SemVer::new(1, 0, 0, None, None));
        assert_clog_commit_version(&repo_dir, SemVer::parse("1.0.0").unwrap())
    }

    #[rstest]
    fn test_two_semver_bumps_prestable(
        pre_stable_repo_dir: TempDir,
        #[values(PATCH, MINOR, MAJOR)] msg1: CommitCase,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg2: CommitCase,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg3: CommitCase,
    ) {
        let repo = Repository::open(&pre_stable_repo_dir).unwrap();
        empty_commit(&repo, msg1.msg).unwrap();

        let v1 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();
        assert_eq!(v1, SemVer::new(0, 1, 0, None, None));
        test_bump_helper(&pre_stable_repo_dir, &repo);

        let v2 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();
        assert_eq!(v2, v1.bump(msg1.bump));
        assert_repo_is_clean(&repo);
        if msg1.bump != SemVerBump::None {
            assert_clog_commit_version(&pre_stable_repo_dir, v1.bump(msg1.bump))
        }

        empty_commit(&repo, msg2.msg).unwrap();
        empty_commit(&repo, msg3.msg).unwrap();
        test_bump_helper(&pre_stable_repo_dir, &repo);

        let v3 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();

        let expected_bump = [msg2.bump, msg3.bump].into_iter().max().unwrap();
        assert_eq!(v3, v2.bump(expected_bump));
        if expected_bump != SemVerBump::None {
            assert_clog_commit_version(&pre_stable_repo_dir, v2.bump(expected_bump));
            assert_repo_is_clean(&repo);
        }
    }

    #[rstest]
    fn test_two_semver_bumps_stable(
        stable_repo_dir: TempDir,
        #[values(PATCH, MINOR, MAJOR)] msg1: CommitCase,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg2: CommitCase,
        #[values(PATCH, MINOR, MAJOR, NONE)] msg3: CommitCase,
    ) {
        let repo = Repository::open(&stable_repo_dir).unwrap();
        empty_commit(&repo, msg1.msg).unwrap();

        let v1 = get_python_pyroject_version(&stable_repo_dir).unwrap();
        assert_eq!(v1, SemVer::new(1, 0, 0, None, None));
        test_bump_helper(&stable_repo_dir, &repo);

        let v2 = get_python_pyroject_version(&stable_repo_dir).unwrap();
        assert_eq!(v2, v1.bump(msg1.bump));
        assert_repo_is_clean(&repo);
        if msg1.bump != SemVerBump::None {
            assert_clog_commit_version(&stable_repo_dir, v1.bump(msg1.bump))
        }

        empty_commit(&repo, msg2.msg).unwrap();
        empty_commit(&repo, msg3.msg).unwrap();
        test_bump_helper(&stable_repo_dir, &repo);

        let v3 = get_python_pyroject_version(&stable_repo_dir).unwrap();

        let expected_bump = [msg2.bump, msg3.bump].into_iter().max().unwrap();
        assert_eq!(v3, v2.bump(expected_bump));
        if expected_bump != SemVerBump::None {
            assert_clog_commit_version(&stable_repo_dir, v2.bump(expected_bump));
            assert_repo_is_clean(&repo);
        }
    }
}
