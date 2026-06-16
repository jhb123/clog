# Clog

clog reads your git history to automate producing changelogs
and semtically versioning your project.

# Installation

Download the latest release for your platform from the [releases page](../../releases/latest):

```bash
# macOS
curl -L https://github.com/jhb123/clog/releases/latest/download/clog-macos.tar.gz | tar -xz -C ~/.local/bin/

# Linux
curl -L https://github.com/jhb123/clog/releases/latest/download/clog-linux.tar.gz | tar -xz -C ~/.local/bin/
```

Alternatively, build from source with Cargo:

```bash
cargo install --git https://github.com/jhb123/clog
```

# Usage

If you follow conventional commits, then clog will parse
them accordingly. If you prefer to use git trailers, then
you can include a trailer like `Clog-Semver-Bump: minor`
for clog to read.

```
# To create a release commit, run
$ clog

# skip the CLI interactions
$ clog --yes

# To create major version 1,
$ clog stable

# If you want to append something to a release
$ clog redo

# If you to install a git alias for the trailer workflow
$ clog install-aliases
$ git bump <patch/minor/major>

# Preview the diff for the current unreleased changes
$ clog preview
```

# Configuration

Place a `clog.toml` file in your project root to configure clog's behaviour.

## LLM changelog summarizer

If your commit messages are not conventional commits, clog can use an LLM to
generate changelog entries from the git diff and commit messages for each
release.

Set `summarizer_command` to any shell command that reads a prompt from stdin
and writes the changelog entries to stdout, one per line:

```toml
# clog.toml

summarizer_command = "llm -m gpt-4o"

# Other examples:
# summarizer_command = "ollama run mistral"
# summarizer_command = "claude --no-tools -p"
```

The prompt clog sends contains the commit messages and the full diff for that
release. The command is run via `sh -c`, so pipelines work too:

```toml
summarizer_command = "llm -m gpt-4o | head -20"
```

> **Warning: agentic CLI tools**
> Some LLM CLI tools (such as Claude Code's `claude` command) run as agents
> with access to file editing and shell tools. Piping a prompt describing code
> changes to such a tool can cause it to modify your project rather than just
> generate text. Always use a flag that disables tool use — for example
> `claude --no-tools -p` — or prefer a purely generative CLI like
> [`llm`](https://llm.datasette.io) which has no tool access by default.

If no `summarizer_command` is set, clog falls back to conventional commit
parsing (`feat:`, `fix:`, breaking changes via `!`, and `Clog-Semver-Bump`
trailers).