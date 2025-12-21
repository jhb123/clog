use assert_cmd::{cargo::cargo_bin_cmd, pkg_name};
use assert_fs::fixture::TempDir;
use clog::{semver::{SemVer, SemVerBump}, test_support::{branches_repo, empty_commit, get_python_pyroject_version, init_python_repo, simple_repo}};
use git2::Repository;
use rstest::*;

fn run_clog(dir: &TempDir){
    cargo_bin_cmd!(pkg_name!())
        .arg("--yes")
        .current_dir(&dir)
        .assert()
        .success()
        .stderr("");
}

#[fixture]
fn repo_dir() -> TempDir {
    let tmp_dir = TempDir::new().unwrap();
    init_python_repo(&tmp_dir).unwrap();
    tmp_dir
}


#[fixture]
fn simple_repo_dir() -> TempDir {
    let tmp_dir = TempDir::new().unwrap();
    simple_repo(&tmp_dir, init_python_repo).unwrap();
    tmp_dir
}

#[fixture]
fn branches_repo_dir() -> TempDir {
    let tmp_dir = TempDir::new().unwrap();
    branches_repo(&tmp_dir, init_python_repo).unwrap();
    tmp_dir
}


#[rstest]
#[case(simple_repo_dir())]
#[case(branches_repo_dir())]
fn test_bump_commit_version_file(#[case] repo_dir: TempDir) {
    let v1 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v1, SemVer::parse("0.1.0").unwrap());
    run_clog(&repo_dir);
    let v2 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v2, SemVer::parse("0.2.0").unwrap());
}

#[rstest]
fn test_no_bump(repo_dir: TempDir) {
    let v1 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v1, SemVer::parse("0.1.0").unwrap());
    run_clog(&repo_dir);
    let v2 = get_python_pyroject_version(&repo_dir).unwrap();
   assert_eq!(v1,v2);
}

#[derive(Debug, Clone, Copy)]
struct CommitCase {
    bump: SemVerBump,
    msg: &'static str,
}

impl CommitCase {
    const fn new(bump: SemVerBump, msg: &'static str) -> Self {
        Self { bump, msg }
    }
}

impl std::fmt::Display for CommitCase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.bump)
    }
}

const PATCH: CommitCase = CommitCase::new(SemVerBump::Patch, "fix: 1");
const MINOR: CommitCase = CommitCase::new(SemVerBump::Minor, "feat: 1");
const MAJOR: CommitCase = CommitCase::new(SemVerBump::Major, "feat!: 1");
const NONE: CommitCase = CommitCase::new(SemVerBump::None, "random");

#[rstest]
fn test_semver_bump_clean(
    repo_dir: TempDir,
    #[values(PATCH, MINOR, MAJOR, NONE)] msg1: CommitCase,
    #[values(PATCH, MINOR, MAJOR, NONE)] msg2: CommitCase,
    #[values(PATCH, MINOR, MAJOR, NONE)] msg3: CommitCase,
) {
    let repo = Repository::open(&repo_dir).unwrap();
    empty_commit(&repo, msg1.msg).unwrap();
    empty_commit(&repo, msg2.msg).unwrap();
    empty_commit(&repo, msg3.msg).unwrap();

    let v1 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v1, SemVer::parse("0.1.0").unwrap());    
    run_clog(&repo_dir);
    let v2 = get_python_pyroject_version(&repo_dir).unwrap();
    let expected_bump = [msg1.bump, msg2.bump, msg3.bump]
        .into_iter()
        .max()
        .unwrap();
    assert_eq!(v2, v1.bump(expected_bump));
    assert_repo_is_clean(&repo);
}

#[rstest]
fn test_two_semver_bumps_clean(
    repo_dir: TempDir,
    #[values(PATCH, MINOR, MAJOR)] msg1: CommitCase,
    #[values(PATCH, MINOR, MAJOR, NONE)] msg2: CommitCase,
    #[values(PATCH, MINOR, MAJOR, NONE)] msg3: CommitCase,
) {
    let repo = Repository::open(&repo_dir).unwrap();
    empty_commit(&repo, msg1.msg).unwrap();

    let v1 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v1, SemVer::parse("0.1.0").unwrap());
    run_clog(&repo_dir);

    let v2 = get_python_pyroject_version(&repo_dir).unwrap();    
    assert_eq!(v2, v1.bump(msg1.bump));
    assert_repo_is_clean(&repo);

    empty_commit(&repo, msg2.msg).unwrap();
    empty_commit(&repo, msg3.msg).unwrap();
    run_clog(&repo_dir);

    let v3 = get_python_pyroject_version(&repo_dir).unwrap();

    let expected_bump = [msg2.bump, msg3.bump]
        .into_iter()
        .max()
        .unwrap();
    assert_eq!(v3, v2.bump(expected_bump));
    assert_repo_is_clean(&repo);
}

fn assert_repo_is_clean(repo: &Repository) {
    let statuses = repo.statuses(None).unwrap();
    assert_eq!(statuses.len(), 0);
}