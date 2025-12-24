use std::{fs, io::Write};

use crate::{
    get_changelog_message, get_next_version, git::CommitWrapper, iterate_to_last_version,
    semver::SemVer, ChangeLogEntry, Config, HistoryItem, Project,
};

pub fn prepare_changelog<T: Iterator<Item = CommitWrapper> + Clone>(
    history: T,
    project: &dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    let path = project.get_dir().join(project.get_changelog());
    if !path.exists() {
        generate_entire_changelog(history, project, config)
    } else {
        append_changelog(history, project, config)
    }
}

fn append_changelog<T: Iterator<Item = CommitWrapper> + Clone>(
    history: T,
    project: &dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    let path = project.get_dir().join(project.get_changelog());

    let next_version = match get_next_version(history.clone(), config) {
        Some(v) => v,
        None => return Ok(()),
    };

    let mut changelog_entries = vec![ChangeLogEntry::BumpVersion(next_version)];

    changelog_entries
        .extend(iterate_to_last_version(history).map(|c| ChangeLogEntry::Entry(c.message())));

    let original = fs::read_to_string(&path)?;
    let changelog = render::prepend_render_changelog(&changelog_entries, &original, config);

    fs::write(&path, changelog)?;
    Ok(())
}

fn generate_entire_changelog<T: Iterator<Item = CommitWrapper> + Clone>(
    history: T,
    project: &dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    // let history = GitHistory::new(project, repo).into_iter();
    let mut next_version = match get_next_version(history.clone(), config) {
        Some(v) => v,
        None => return Ok(()),
    };
    let mut changelog_entries = vec![];

    for commit in history.clone() {
        if commit.version() != next_version {
            changelog_entries.push(ChangeLogEntry::BumpVersion(next_version));
            next_version = commit.version();
        }
        if let Some(s) = get_changelog_message(&commit, config) {
            changelog_entries.push(ChangeLogEntry::Entry(s));
        }
    }

    if let Some(version) = find_first_version_of_project(history.clone())? {
        changelog_entries.push(ChangeLogEntry::InitialVersion(version));
    }

    let changelog = render::render_changelog(&changelog_entries, config);

    let path = project.get_dir().join(project.get_changelog());

    let mut file = fs::File::create(path)?;

    file.write_all(changelog.as_bytes())
        .expect("failed to make changelog");
    Ok(())
}

fn find_first_version_of_project<T: Iterator<Item = CommitWrapper>>(
    history: T,
) -> anyhow::Result<Option<SemVer>> {
    let version = history.map(|c| c.version()).min();
    Ok(version)
}

mod render {
    use crate::{ChangeLogEntry, Config};

    pub fn render_changelog(changelog_entries: &[ChangeLogEntry], _config: &Config) -> String {
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

    pub fn prepend_render_changelog(
        changelog_entries: &[ChangeLogEntry],
        original_changelog: &str,
        config: &Config,
    ) -> String {
        let new_changelog = render_changelog(changelog_entries, config);
        format!("{new_changelog}{original_changelog}")
    }
}
