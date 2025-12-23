use std::{fs, io::Write};

use git2::{Repository, Sort};

use crate::{
    find_first_version_of_project, get_changelog_message, get_head, get_latest_release,
    get_next_version, is_version_bump, semver::SemVer, Config, Project,
};

enum ChangeLogEntry {
    BumpVersion(SemVer),
    InitialVersion(SemVer),
    Entry(String),
}

pub fn prepare_changelog(
    repo: &Repository,
    project: &mut dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    let mut path = repo.commondir().parent().unwrap().to_path_buf();
    path.push(project.get_changelog());

    if !path.exists() {
        generate_entire_changelog(repo, project, config)
    } else {
        append_changelog(repo, project, config)
    }
}

fn append_changelog(
    repo: &Repository,
    project: &mut dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    let mut path = repo.commondir().parent().unwrap().to_path_buf();
    path.push(project.get_changelog());
    let mut changelog_entries = vec![ChangeLogEntry::BumpVersion(get_next_version(
        repo, project, config,
    )?)];

    let since_oid = get_latest_release(repo, project)?;
    let upto_oid = get_head(repo).unwrap();
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TOPOLOGICAL)?;
    revwalk.push(upto_oid)?;

    if let Some(since) = since_oid {
        revwalk.hide(since)?;
    }

    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        if let Some(s) = get_changelog_message(&commit, config) {
            changelog_entries.push(ChangeLogEntry::Entry(s));
        }
    }

    let original = fs::read_to_string(&path)?;
    let changelog = prepend_render_changelog(&changelog_entries, &original, config);

    fs::write(&path, changelog)?;
    Ok(())
}

fn generate_entire_changelog(
    repo: &Repository,
    project: &mut dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    let mut changelog_entries = vec![ChangeLogEntry::BumpVersion(get_next_version(
        repo, project, config,
    )?)];

    let upto_oid = get_head(repo).unwrap();
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TOPOLOGICAL)?;
    revwalk.push(upto_oid)?;

    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        if let Some(version) = is_version_bump(&commit, repo, project)? {
            changelog_entries.push(ChangeLogEntry::BumpVersion(version));
        }

        if let Some(s) = get_changelog_message(&commit, config) {
            changelog_entries.push(ChangeLogEntry::Entry(s));
        }
    }

    if let Some(version) = find_first_version_of_project(repo, project)? {
        changelog_entries.push(ChangeLogEntry::InitialVersion(version));
    }

    let changelog = render_changelog(&changelog_entries, config);

    let mut path = repo
        .commondir()
        .parent()
        .expect(".git should be in a dir")
        .to_path_buf();
    path.push(project.get_changelog());

    let mut file = fs::File::create(path)?;

    file.write_all(changelog.as_bytes())
        .expect("failed to make changelog");
    Ok(())
}

fn render_changelog(changelog_entries: &[ChangeLogEntry], _config: &Config) -> String {
    let mut changelog = String::new();
    for entry in changelog_entries {
        match entry {
            ChangeLogEntry::BumpVersion(sem_ver) => {
                changelog.push_str(&format!("# Version {}", sem_ver));
            }
            ChangeLogEntry::InitialVersion(sem_ver) => {
                changelog.push_str(&format!("# Version {}\nInitial Commit", sem_ver));
            }
            ChangeLogEntry::Entry(msg) => {
                changelog.push_str(msg);
            }
        }
        changelog.push('\n');
    }
    changelog
}

fn prepend_render_changelog(
    changelog_entries: &[ChangeLogEntry],
    original_changelog: &str,
    config: &Config,
) -> String {
    let new_changelog = render_changelog(changelog_entries, config);
    format!("{new_changelog}{original_changelog}")
}
