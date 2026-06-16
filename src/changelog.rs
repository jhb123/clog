use std::{fs, io::{BufRead, BufReader, Write}, process::{Command, Stdio}};

use anyhow::Ok;
use git2::{Oid, Repository};

use crate::{
    get_next_version,
    git::CommitWrapper,
    iterate_to_last_version,
    semver::{SemVer, SemVerBump},
    Config, HistoryItem, Project,
};

#[derive(Debug, PartialEq, Eq)]
enum ChangeLogEntry {
    BumpVersion(SemVer),
    InitialVersion(SemVer),
    Entry(String),
}

pub fn prepare_changelog<T: Iterator<Item = CommitWrapper> + Clone>(
    history: T,
    repo: Option<&Repository>,
    project: &dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    let path = project.get_dir().join(project.get_changelog());
    if !path.exists() {
        generate_entire_changelog(history, repo, project, config)
    } else {
        append_changelog(history, repo, project, config)
    }
}

fn append_changelog<T: Iterator<Item = CommitWrapper> + Clone>(
    history: T,
    repo: Option<&Repository>,
    project: &dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    let path = project.get_dir().join(project.get_changelog());

    let changelog_entries = get_newest_changelog_items(history, repo, config)?;
    if changelog_entries.is_empty() {
        return Ok(());
    }
    let original = fs::read_to_string(&path)?;
    let changelog = render::prepend_render_changelog(&changelog_entries, &original, config);

    fs::write(&path, changelog)?;
    Ok(())
}

fn get_newest_changelog_items<T: Iterator<Item = impl HistoryItem> + Clone>(
    history: T,
    repo: Option<&Repository>,
    config: &Config,
) -> anyhow::Result<Vec<ChangeLogEntry>> {
    let next_version = match get_next_version(history.clone(), config) {
        Some(v) => v,
        None => return Ok(vec![]),
    };
    let window: Vec<_> = iterate_to_last_version(history).collect();
    let messages: Vec<String> = window.iter().map(|c| c.message()).collect();
    let newest_oid = window.first().and_then(|c| c.commit_id());
    let oldest_oid = window.last().and_then(|c| c.commit_id());
    let diff = compute_diff(repo, newest_oid, oldest_oid)?;

    let entries = get_entries_for_window(&messages, &diff, config)?;
    let mut changelog_entries = vec![ChangeLogEntry::BumpVersion(next_version)];
    changelog_entries.extend(entries.into_iter().map(ChangeLogEntry::Entry));
    Ok(changelog_entries)
}

fn generate_entire_changelog<T: Iterator<Item = CommitWrapper> + Clone>(
    history: T,
    repo: Option<&Repository>,
    project: &dyn Project,
    config: &Config,
) -> anyhow::Result<()> {
    let changelog_entries = get_all_changelog_entries(history, repo, config)?;
    let changelog = render::render_changelog(&changelog_entries, config);
    let path = project.get_dir().join(project.get_changelog());
    let mut file = fs::File::create(path)?;
    file.write_all(changelog.as_bytes())
        .expect("failed to make changelog");
    Ok(())
}

fn get_all_changelog_entries<T: Iterator<Item = impl HistoryItem> + Clone>(
    history: T,
    repo: Option<&Repository>,
    config: &Config,
) -> anyhow::Result<Vec<ChangeLogEntry>> {
    let mut bump_to = match get_next_version(history.clone(), config) {
        Some(v) => v,
        None => return Ok(vec![]),
    };

    let mut changelog_entries = vec![];
    let mut window_messages: Vec<String> = vec![];
    let mut window_newest_oid: Option<Oid> = None;
    let mut window_oldest_oid: Option<Oid> = None;
    let mut window_version: Option<SemVer> = None;

    for commit in history.clone() {
        let cv = commit.version();
        match &window_version {
            None => {
                window_version = Some(cv);
            }
            Some(v) if *v != cv => {
                let diff = compute_diff(repo, window_newest_oid, window_oldest_oid)?;
                changelog_entries.push(ChangeLogEntry::BumpVersion(bump_to));
                let entries = get_entries_for_window(&window_messages, &diff, config)?;
                changelog_entries.extend(entries.into_iter().map(ChangeLogEntry::Entry));
                bump_to = v.clone();
                window_messages.clear();
                window_newest_oid = None;
                window_version = Some(cv);
            }
            Some(_) => {}
        }
        if window_newest_oid.is_none() {
            window_newest_oid = commit.commit_id();
        }
        window_oldest_oid = commit.commit_id();
        window_messages.push(commit.message());
    }

    if !window_messages.is_empty() {
        let diff = compute_diff(repo, window_newest_oid, window_oldest_oid)?;
        changelog_entries.push(ChangeLogEntry::BumpVersion(bump_to));
        let entries = get_entries_for_window(&window_messages, &diff, config)?;
        changelog_entries.extend(entries.into_iter().map(ChangeLogEntry::Entry));
    }

    if let Some(version) = find_first_version_of_project(history) {
        changelog_entries.push(ChangeLogEntry::InitialVersion(version));
    }

    Ok(changelog_entries)
}

fn compute_diff(
    repo: Option<&Repository>,
    newest: Option<Oid>,
    oldest: Option<Oid>,
) -> anyhow::Result<String> {
    match (repo, newest, oldest) {
        (Some(r), Some(n), Some(o)) => crate::git::diff_oids(r, n, o),
        _ => Ok(String::new()),
    }
}

fn find_first_version_of_project<T, H>(history: T) -> Option<SemVer>
where
    T: Iterator<Item = H>,
    H: HistoryItem,
{
    history.map(|c| c.version()).min()
}

fn get_entries_for_window(
    messages: &[String],
    diff: &str,
    config: &Config,
) -> anyhow::Result<Vec<String>> {
    if let Some(command) = &config.summarizer_command {
        run_summarizer(command, messages, diff)
    } else {
        Ok(messages
            .iter()
            .filter_map(|m| conventional_entry(m, config))
            .collect())
    }
}

fn run_summarizer(command: &str, messages: &[String], diff: &str) -> anyhow::Result<Vec<String>> {
    let prompt = format!(
        "Generate a concise changelog entry list for the following changes.\n\
         Output one entry per line, no bullet points or numbering.\n\
         Only include user-facing changes worth noting in a changelog.\n\
         \n\
         ## Commits\n\
         {}\n\
         \n\
         ## Diff\n\
         {}",
        messages.join("\n"),
        diff,
    );

    eprintln!("Running summarizer: {}", command);

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to launch summarizer '{}': {}", command, e))?;

    // Ignore broken pipe — if the command exits early due to an error, we'll
    // catch the real cause via the exit status below.
    let _ = child.stdin.as_mut().unwrap().write_all(prompt.as_bytes());
    drop(child.stdin.take());

    let mut entries = Vec::new();
    let reader = BufReader::new(child.stdout.take().unwrap());
    for line in reader.lines() {
        let line = line?;
        eprintln!("{}", line);
        if !line.trim().is_empty() {
            entries.push(line);
        }
    }

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!(
            "Summarizer '{}' exited with status {}. \
             Check your summarizer_command in clog.toml.",
            command,
            status
        );
    }

    Ok(entries)
}

fn conventional_entry(message: &str, config: &Config) -> Option<String> {
    let bump = crate::get_bump_from_trailer(message);
    if bump != SemVerBump::None {
        return message.split('\n').next().map(String::from);
    }

    let mut patterns = config
        .patterns
        .major
        .iter()
        .chain(&config.patterns.minor)
        .chain(&config.patterns.patch);

    let first_line = message.split('\n').next()?;
    if patterns.any(|r| r.is_match(first_line)) {
        Some(first_line.to_string())
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
                    changelog.push_str(&format!("# Version {}\n- Initial Commit", sem_ver));
                }
                ChangeLogEntry::Entry(msg) => {
                    changelog.push_str(&format!("- {}", msg));
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
        changelog::{get_all_changelog_entries, ChangeLogEntry},
        semver::SemVer,
        test_support::TestCommitWrapper,
        Config,
    };

    use rstest::rstest;

    #[rstest]
    #[case::single_version(
        vec![
            TestCommitWrapper::new_normal("feat: test 1", SemVer::new(0, 1, 0, None, None)),
            TestCommitWrapper::new_normal("fix: test 2", SemVer::new(0, 1, 0, None, None)),
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
            TestCommitWrapper::new_normal("feat: test 6", SemVer::new(0, 2, 0, None, None)),
            TestCommitWrapper::new_normal("feat: test 5", SemVer::new(0, 2, 0, None, None)),
            TestCommitWrapper::new_normal("feat: test 4", SemVer::new(0, 2, 0, None, None)),
            TestCommitWrapper::new_normal("feat: test 3", SemVer::new(0, 1, 0, None, None)),
            TestCommitWrapper::new_normal("feat: test 2", SemVer::new(0, 1, 0, None, None)),
            TestCommitWrapper::new_normal("feat: test 1", SemVer::new(0, 1, 0, None, None)),
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
            TestCommitWrapper::new_normal("chore: update docs", SemVer::new(1, 0, 0, None, None)),
            TestCommitWrapper::new_normal("chore: refactor", SemVer::new(1, 0, 0, None, None)),
        ],
        vec![]
    )]
    #[case::major_version_bump(
        vec![
            TestCommitWrapper::new_normal("feat!: breaking change", SemVer::new(1, 5, 0, None, None)),
            TestCommitWrapper::new_normal("feat: old feature", SemVer::new(1, 5, 0, None, None)),
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
    #[case::trailer_style(
        vec![
            TestCommitWrapper::new_normal(&format!("trailer feature\n{}: {}",crate::CLOG_BUMP_TRAILER, "patch"), SemVer::new(1, 5, 0, None, None)),
            TestCommitWrapper::new_normal("feat: old feature", SemVer::new(1, 5, 0, None, None)),
        ],
        vec![
            ChangeLogEntry::BumpVersion(SemVer::new(1, 6, 0, None, None)),
            ChangeLogEntry::Entry("trailer feature".to_string()),
            ChangeLogEntry::Entry("feat: old feature".to_string()),
            ChangeLogEntry::InitialVersion(SemVer::new(1, 5, 0, None, None)),
        ]
    )]
    fn test_history_to_changelog(
        #[case] history: Vec<TestCommitWrapper>,
        #[case] expected: Vec<ChangeLogEntry>,
    ) {
        let config = Config::default();
        let changelog = get_all_changelog_entries(history.into_iter(), None, &config).unwrap();
        assert_eq!(expected, changelog);
    }
}
