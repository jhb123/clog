use std::fs;

use assert_cmd::{cargo::cargo_bin_cmd, pkg_name};
use assert_fs::fixture::TempDir;
use clog::{Config, detect_project, semver::SemVer, test_support::{init_python_repo, simple_repo}};
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
    let config = Config::new(&simple_repo_dir);
    let project = detect_project(&config).unwrap();
    let v1 = project.get_version();
    assert_eq!(v1, SemVer::parse("0.1.0").unwrap());
    cargo_bin_cmd!(pkg_name!())
        .arg("--yes")
        .current_dir(project.get_dir())
        .assert()
        .success()
        .stderr("");
    let mut pyproject = project.get_dir().to_path_buf();
    pyproject.push(project.get_version_file());
    let v2 = project.parse_version_file(&fs::read_to_string(pyproject).unwrap()).unwrap();
    assert_eq!(v2, SemVer::parse("0.2.0").unwrap());
}
