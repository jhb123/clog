# Version 0.7.0
- Add `clog preview` command to view the diff for unreleased changes in a pager
- Add LLM summarizer support via `summarizer_command` in `clog.toml` to generate changelog entries from commit messages and diffs
- Show LLM summarizer output in real time while it runs
- Add support for Poetry-style `pyproject.toml` files (`[tool.poetry]` version field)
- Fix diff generation for repositories where the oldest commit in a window has no parent
- Fix crash when the version file is absent in early git history
- Pre-built binaries for macOS and Linux are now available on the GitHub releases page
- Binary is now statically linked and self-contained — no system libgit2 required
# Version 0.6.0
- Add trailer commits to release notes
- feat: make alias installation
# Version 0.5.0
- fix: iteratre to last version should always check for version bump
- feat: "redo release" sub command
# Version 0.4.1
- fix: appended changelog generation
- fix: formatting lists in markdown changelog
# Version 0.4.0
- feat: add changelog prototype
# Version 0.3.0
- feat: detect Cargo.lock for rust projects
- feat: detect the most recent version bump in project
# Version 0.2.0
- fix: cargo project uses project instead of package
- feat: check repo is clean state before making release commit
- fix: multi-line conventional commits regex
- feat: semver parsing and release commits for python and rust
- feat: add mkrepo test tool
# Version 0.1.0
- Initial Commit
