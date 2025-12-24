use std::{fs, path::Path};

use assert_cmd::{cargo::cargo_bin_cmd, pkg_name};
use assert_fs::fixture::TempDir;
use clog::{
    semver::{SemVer, SemVerBump},
    test_support::*,
};
use fs_extra::{copy_items, dir};
use git2::Repository;
use rstest::*;

fn run_clog(dir: &TempDir) {
    cargo_bin_cmd!(pkg_name!())
        .arg("--yes")
        .current_dir(&dir)
        .assert()
        .success()
        .stderr("");
}

fn run_clog_stable_release(dir: &TempDir) {
    cargo_bin_cmd!(pkg_name!())
        .arg("--yes")
        .arg("--stable")
        .current_dir(&dir)
        .assert()
        .success()
        .stderr("");
}

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

#[rstest]
#[case(pre_stable_simple_repo_dir())]
#[case(pre_stable_branches_repo_dir())]
fn test_bump_commit_version_file(#[case] repo_dir: TempDir) {
    let v1 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v1, SemVer::new(0, 1, 0, None, None));
    run_clog(&repo_dir);
    let v2 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v2, SemVer::new(0, 2, 0, None, None));
    assert_clog_commit_version(&repo_dir, SemVer::parse("0.2.0").unwrap());
}

#[rstest]
#[case(pre_stable_simple_repo_dir())]
#[case(pre_stable_branches_repo_dir())]
fn test_version_1_version_file(#[case] repo_dir: TempDir) {
    let v1 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v1, SemVer::new(0, 1, 0, None, None));
    run_clog_stable_release(&repo_dir);
    let v2 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v2, SemVer::new(1, 0, 0, None, None));
    assert_clog_commit_version(&repo_dir, SemVer::parse("1.0.0").unwrap());
}

#[rstest]
fn test_no_bump(pre_stable_repo_dir: TempDir) {
    let v1 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();
    assert_eq!(v1, SemVer::new(0, 1, 0, None, None));
    run_clog(&pre_stable_repo_dir);
    let v2 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();
    assert_eq!(v1, v2);
}

#[rstest]
#[case(stable_simple_repo_dir())]
#[case(stable_branches_repo_dir())]
fn test_no_bump_stable(#[case] repo_dir: TempDir) {
    let v1 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v1, SemVer::new(1, 0, 0, None, None));
    cargo_bin_cmd!(pkg_name!())
        .arg("--yes")
        .arg("--stable")
        .current_dir(&repo_dir)
        .assert()
        .failure();

    let v2 = get_python_pyroject_version(&repo_dir).unwrap();
    assert_eq!(v1, v2);
}

#[rstest]
fn test_semver_bump_prestable(
    pre_stable_repo_dir: TempDir,
    #[values(PATCH, MINOR, MAJOR, NONE)] msg1: CommitCase,
) {
    let repo = Repository::open(&pre_stable_repo_dir).unwrap();
    empty_commit(&repo, msg1.msg).unwrap();

    let v1 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();
    assert_eq!(v1, SemVer::new(0, 1, 0, None, None));
    run_clog(&pre_stable_repo_dir);
    let v2 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();
    assert_eq!(v2, v1.bump(msg1.bump));
    assert_repo_is_clean(&repo);
}

#[rstest]
fn test_two_semver_bumps_prestable(
    pre_stable_repo_dir: TempDir,
    #[values(PATCH, MINOR, MAJOR)] msg1: CommitCase,
    #[values(PATCH, MINOR, MAJOR, NONE)] msg2: CommitCase,
) {
    let repo = Repository::open(&pre_stable_repo_dir).unwrap();
    empty_commit(&repo, msg1.msg).unwrap();

    let v1 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();
    assert_eq!(v1, SemVer::new(0, 1, 0, None, None));
    run_clog(&pre_stable_repo_dir);

    let v2 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();
    assert_eq!(v2, v1.bump(msg1.bump));
    assert_repo_is_clean(&repo);
    if msg1.bump != SemVerBump::None {
        assert_clog_commit_version(&pre_stable_repo_dir, v1.bump(msg1.bump))
    }

    empty_commit(&repo, msg2.msg).unwrap();
    run_clog(&pre_stable_repo_dir);

    let v3 = get_python_pyroject_version(&pre_stable_repo_dir).unwrap();

    assert_eq!(v3, v2.bump(msg2.bump));
    assert_repo_is_clean(&repo);
    if msg2.bump != SemVerBump::None {
        assert_clog_commit_version(&pre_stable_repo_dir, v2.bump(msg2.bump))
    }
}

#[rstest]
fn test_semver_bump_stable(
    stable_repo_dir: TempDir,
    #[values(PATCH, MINOR, MAJOR, NONE)] msg1: CommitCase,
) {
    let repo = Repository::open(&stable_repo_dir).unwrap();
    empty_commit(&repo, msg1.msg).unwrap();

    let v1 = get_python_pyroject_version(&stable_repo_dir).unwrap();
    assert_eq!(v1, SemVer::new(1, 0, 0, None, None));
    run_clog(&stable_repo_dir);
    let v2 = get_python_pyroject_version(&stable_repo_dir).unwrap();
    assert_eq!(v2, v1.bump(msg1.bump));
    assert_repo_is_clean(&repo);
    if msg1.bump != SemVerBump::None {
        assert_clog_commit_version(&stable_repo_dir, v1.bump(msg1.bump))
    }
}

#[rstest]
fn changelog(pre_stable_branches_repo_dir: TempDir) {
    let v1 = get_python_pyroject_version(&pre_stable_branches_repo_dir).unwrap();
    run_clog(&pre_stable_branches_repo_dir);
    let v2 = get_python_pyroject_version(&pre_stable_branches_repo_dir).unwrap();
    let changelog = fs::read_to_string(pre_stable_branches_repo_dir.join("Changelog.md")).unwrap();
    assert_eq!(changelog,"# Version 0.2.0\nfix: bug in B\nfeat: add feature B\nfix!: bug in A\nfeat: add feature A\n# Version 0.1.0\nInitial Commit\n")
}
