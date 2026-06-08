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
```