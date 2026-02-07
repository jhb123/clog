# Clog

clog reads your git history to automate producing changelogs
and semtically versioning your project.

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