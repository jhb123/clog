use assert_cmd::{cargo::cargo_bin_cmd, pkg_name};
use assert_fs::fixture::TempDir;
use clog::{semver::SemVer, test_support::{get_python_pyroject_version, init_python_repo, simple_repo}};
use rstest::{fixture, rstest};

#[fixture]
fn simple_repo_dir() -> TempDir {
    let tmp_dir = TempDir::new().unwrap();
    simple_repo(&tmp_dir, init_python_repo).unwrap();
    tmp_dir
}


#[test]
fn test_help() {
    cargo_bin_cmd!(pkg_name!())
        .arg("--help")
        .assert()
        .success()
        .stderr("");
}


#[rstest]
fn test_bump_commit_version_file(simple_repo_dir: TempDir) {
    let v1 = get_python_pyroject_version(&simple_repo_dir).unwrap();
    assert_eq!(v1, SemVer::parse("0.1.0").unwrap());
    cargo_bin_cmd!(pkg_name!())
        .arg("--yes")
        .current_dir(&simple_repo_dir)
        .assert()
        .success()
        .stderr("");
    let v2 = get_python_pyroject_version(&simple_repo_dir).unwrap();
    assert_eq!(v2, SemVer::parse("0.2.0").unwrap());
}
