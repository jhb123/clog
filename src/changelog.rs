use std::{fs, io::Write};

use anyhow::Ok;

use crate::{
    get_next_version, git::CommitWrapper, iterate_to_last_version, semver::SemVer, Config,
    HistoryItem, Project,
};

#[derive(Debug, PartialEq, Eq)]
enum ChangeLogEntry {
    BumpVersion(SemVer),
    InitialVersion(SemVer),
    Entry(String),
}

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

    let changelog_entries = get_newest_changelog_items(history, config);
    if changelog_entries.is_empty() {
        return Ok(());
    }
    let original = fs::read_to_string(&path)?;
    let changelog = render::prepend_render_changelog(&changelog_entries, &original, config);

    fs::write(&path, changelog)?;
    Ok(())
}

fn get_newest_changelog_items<T: Iterator<Item = CommitWrapper> + Clone>(
    history: T,
    config: &Config,
) -> Vec<ChangeLogEntry> {
    let next_version = match get_next_version(history.clone(), config) {
        Some(v) => v,
        None => return vec![],
    };
    let mut changelog_entries = vec![ChangeLogEntry::BumpVersion(next_version)];
    changelog_entries
        .extend(iterate_to_last_version(history).map(|c| ChangeLogEntry::Entry(c.message())));
    changelog_entries
}

fn generate_entire_changelog<T: Iterator<Item = CommitWrapper> + Clone>(
    history: T,
    project: &dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    // let history = GitHistory::new(project, repo).into_iter();
    let changelog_entries = get_all_changelog_entries(history, config);

    let changelog = render::render_changelog(&changelog_entries, config);

    let path = project.get_dir().join(project.get_changelog());

    let mut file = fs::File::create(path)?;

    file.write_all(changelog.as_bytes())
        .expect("failed to make changelog");
    Ok(())
}

fn get_all_changelog_entries<T: Iterator<Item = CommitWrapper> + Clone>(
    history: T,
    config: &Config,
) -> Vec<ChangeLogEntry> {
    let mut next_version = match get_next_version(history.clone(), config) {
        Some(v) => v,
        None => return vec![],
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
    if let Some(version) = find_first_version_of_project(history.clone()) {
        changelog_entries.push(ChangeLogEntry::InitialVersion(version));
    }
    changelog_entries
}

fn find_first_version_of_project<T: Iterator<Item = CommitWrapper>>(history: T) -> Option<SemVer> {
    history.map(|c| c.version()).min()
}

fn get_changelog_message(commit: &CommitWrapper, config: &Config) -> Option<String> {
    let mut patterns = config
        .patterns
        .major
        .iter()
        .chain(&config.patterns.minor)
        .chain(&config.patterns.patch);

    let message = commit.message();
    let message = message.split("\n").next()?;

    if patterns.any(|r| r.is_match(message)) {
        Some(message.to_string())
    } else {
        None
    }
}

mod render {
    use crate::{changelog::ChangeLogEntry, Config};

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

#[cfg(test)]
mod test {

    use crate::{
        changelog::get_all_changelog_entries, changelog::ChangeLogEntry, semver::SemVer,
        CommitWrapper, Config,
    };

    use rstest::rstest;

    #[rstest]
    #[case::single_version(
        vec![
            CommitWrapper::new("feat: test 1", SemVer::new(0, 1, 0, None, None)),
            CommitWrapper::new("fix: test 2", SemVer::new(0, 1, 0, None, None)),
        ],
        vec![
            ChangeLogEntry::BumpVersion(SemVer::new(0, 2, 0, None, None)),
            ChangeLogEntry::Entry("feat: test 1".to_string()),
            ChangeLogEntry::Entry("fix: test 2".to_string()),
            ChangeLogEntry::InitialVersion(SemVer::new(0, 1, 0, None, None)),
        ]
    )]
    #[case::multiple_version_bumps(
        vec![
            CommitWrapper::new("feat: test 6", SemVer::new(0, 2, 0, None, None)),
            CommitWrapper::new("feat: test 5", SemVer::new(0, 2, 0, None, None)),
            CommitWrapper::new("feat: test 4", SemVer::new(0, 2, 0, None, None)),
            CommitWrapper::new("feat: test 3", SemVer::new(0, 1, 0, None, None)),
            CommitWrapper::new("feat: test 2", SemVer::new(0, 1, 0, None, None)),
            CommitWrapper::new("feat: test 1", SemVer::new(0, 1, 0, None, None)),
        ],
        vec![
            ChangeLogEntry::BumpVersion(SemVer::new(0, 3, 0, None, None)),
            ChangeLogEntry::Entry("feat: test 6".to_string()),
            ChangeLogEntry::Entry("feat: test 5".to_string()),
            ChangeLogEntry::Entry("feat: test 4".to_string()),
            ChangeLogEntry::BumpVersion(SemVer::new(0, 2, 0, None, None)),
            ChangeLogEntry::Entry("feat: test 3".to_string()),
            ChangeLogEntry::Entry("feat: test 2".to_string()),
            ChangeLogEntry::Entry("feat: test 1".to_string()),
            ChangeLogEntry::InitialVersion(SemVer::new(0, 1, 0, None, None)),
        ]
    )]
    #[case::no_bump_needed(
        vec![
            CommitWrapper::new("chore: update docs", SemVer::new(1, 0, 0, None, None)),
            CommitWrapper::new("chore: refactor", SemVer::new(1, 0, 0, None, None)),
        ],
        vec![]
    )]
    #[case::major_version_bump(
        vec![
            CommitWrapper::new("feat!: breaking change", SemVer::new(1, 5, 0, None, None)),
            CommitWrapper::new("feat: old feature", SemVer::new(1, 5, 0, None, None)),
        ],
        vec![
            ChangeLogEntry::BumpVersion(SemVer::new(2, 0, 0, None, None)),
            ChangeLogEntry::Entry("feat!: breaking change".to_string()),
            ChangeLogEntry::Entry("feat: old feature".to_string()),
            ChangeLogEntry::InitialVersion(SemVer::new(1, 5, 0, None, None)),
        ]
    )]
    #[case::empty_history(
        vec![],
        vec![]
    )]
    fn test_history_to_changelog(
        #[case] history: Vec<CommitWrapper>,
        #[case] expected: Vec<ChangeLogEntry>,
    ) {
        let config = Config::default();
        let changelog = get_all_changelog_entries(history.into_iter(), &config);
        assert_eq!(expected, changelog);
    }
}
