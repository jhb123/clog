mod changelog;
pub mod git;
mod python;
mod rust;
pub mod semver;

use std::{
    path::{Path, PathBuf},
    vec,
};

use git2::Repository;
use once_cell::sync::Lazy;
use regex::Regex;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use crate::git::is_repo_ready;

use crate::{
    git::{create_clog_commit, remove_last_release_commit, CommitWrapper, GitHistory},
    python::PyProject,
    rust::CargoProject,
    semver::{SemVer, SemVerBump},
};

/// Create a commit which updates the changelog and bumps the version
pub fn bump_project_version(
    repo: &Repository,
    project: &mut dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    let history: Vec<CommitWrapper> = GitHistory::new(project, repo).collect();

    changelog::prepare_changelog(history.clone().into_iter(), project, config).unwrap();

    let next_version = match get_next_version(history.into_iter(), config) {
        Some(v) => v,
        None => return Ok(()),
    };

    create_clog_commit(repo, project, config, next_version)
}

/// Create the initial release commit on the current branch
pub fn make_stable_release(
    repo: &Repository,
    project: &mut dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    let history: Vec<CommitWrapper> = GitHistory::new(project, repo).collect();
    changelog::prepare_changelog(history.clone().into_iter(), project, config).unwrap();
    project.set_initial_release()?;
    project.update_project_file()?;
    create_clog_commit(repo, project, config, SemVer::version_1_0_0())
}

pub fn redo_release(
    repo: &Repository,
    project: &mut dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    remove_last_release_commit(repo, project)?;
    bump_project_version(repo, project, config)
}

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
    fn set_version(&mut self, bump: SemVer);
    fn update_project_file(&self) -> anyhow::Result<()>;
    fn get_version_file(&self) -> &Path; // needs to be dyn compatible
    fn set_initial_release(&mut self) -> anyhow::Result<()>;
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

pub trait HistoryItem {
    fn message(&self) -> String;
    fn version(&self) -> SemVer;
    fn kind(&self) -> HistoryItemKind;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryItemKind {
    Normal,
    ClogBump,
}

fn iterate_to_last_version<I, H>(history: I) -> impl Iterator<Item = H>
where
    I: Iterator<Item = H>,
    H: HistoryItem,
{
    history.scan(None, |head_version, commit| {
        let v = commit.version();

        match head_version {
            None => {
                *head_version = Some(v);
                Some(commit)
            }
            Some(hv) if *hv == v => Some(commit),
            _ => None,
        }
    })
}

fn is_last_version_bump_clog<I, H>(history: I) -> bool 
where
    I: Iterator<Item = H>,
    H: HistoryItem,
{
    iterate_to_last_version(history)
        .last()
        .is_some_and(|c| c.kind() == HistoryItemKind::ClogBump)
}

pub fn get_next_version<I, H>(history: I, config: &Config) -> Option<SemVer>
where
    I: Iterator<Item = H>,
    H: HistoryItem,
{
    let commits: Vec<_> = iterate_to_last_version(history).collect();

    let version = commits.first()?.version();
    let bump = commits
        .iter()
        .map(|c| parse_commit_message(c, config))
        .max()?;
    if bump == SemVerBump::None {
        return None;
    }
    Some(version.bump(bump))
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

fn parse_commit_message<H: HistoryItem>(commit: &H, config: &Config) -> SemVerBump {
    let message = commit.message();
    if config.patterns.major.iter().any(|r| r.is_match(&message)) {
        SemVerBump::Major
    } else if config.patterns.minor.iter().any(|r| r.is_match(&message)) {
        SemVerBump::Minor
    } else if config.patterns.patch.iter().any(|r| r.is_match(&message)) {
        SemVerBump::Patch
    } else {
        SemVerBump::None
    }
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
        bump_project_version(repo, project.as_mut(), &config).unwrap();
    }

    fn test_initial_stable_helper(dir: &TempDir, repo: &Repository) {
        let config = Config::new(dir);
        let mut project = detect_project(&config).unwrap();
        make_stable_release(repo, project.as_mut(), &config).unwrap();
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

    #[rstest]
    #[case::clog_bump_is_most_recent(
        vec![
            TestCommitWrapper::new("", SemVer::new_simple(0, 2, 0), HistoryItemKind::Normal),
            TestCommitWrapper::new("", SemVer::new_simple(0, 2, 0), HistoryItemKind::ClogBump),
            TestCommitWrapper::new("",  SemVer::new_simple(0, 1, 0), HistoryItemKind::Normal),
        ],
        true
    )]
    #[case::clog_bump_is_last(
        vec![
            TestCommitWrapper::new("", SemVer::new_simple(0, 2, 0), HistoryItemKind::ClogBump),
            TestCommitWrapper::new("",  SemVer::new_simple(0, 1, 0), HistoryItemKind::Normal),
        ],
        true
    )]
    #[case::normal_is_last(
        vec![
            TestCommitWrapper::new("", SemVer::new_simple(0, 2, 0), HistoryItemKind::Normal),
            TestCommitWrapper::new("",  SemVer::new_simple(0, 1, 0), HistoryItemKind::Normal),
        ],
        false
    )]
    #[case::normal_is_most_recent(
        vec![
            TestCommitWrapper::new("", SemVer::new_simple(0, 2, 0), HistoryItemKind::Normal),
            TestCommitWrapper::new("", SemVer::new_simple(0, 2, 0), HistoryItemKind::Normal),
            TestCommitWrapper::new("",  SemVer::new_simple(0, 1, 0), HistoryItemKind::Normal),
        ],
        false
    )]
    #[case::normal_is_most_recent_with_clog(
        vec![
            TestCommitWrapper::new("", SemVer::new_simple(0, 3, 0), HistoryItemKind::Normal),
            TestCommitWrapper::new("", SemVer::new_simple(0, 2, 0), HistoryItemKind::ClogBump),
            TestCommitWrapper::new("", SemVer::new_simple(0, 2, 0), HistoryItemKind::Normal),
            TestCommitWrapper::new("",  SemVer::new_simple(0, 1, 0), HistoryItemKind::Normal),
        ],
        false
    )]
    #[case::empty_history(vec![], false)]
    #[case::single_entry_is_clog(
        vec![TestCommitWrapper::new("", SemVer::new_simple(0, 1, 0), HistoryItemKind::ClogBump)],
        true
    )]
    #[case::single_entry_is_normal(
        vec![TestCommitWrapper::new("", SemVer::new_simple(0, 1, 0), HistoryItemKind::Normal)],
        false
    )]
    fn test_is_last_version_bump_clog(#[case] history: Vec<TestCommitWrapper>, #[case] expected: bool) {
        let result = is_last_version_bump_clog(history.into_iter());
        assert_eq!(result, expected);
    }
}
