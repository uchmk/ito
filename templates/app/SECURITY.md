# Security

## Reporting a vulnerability

Please report it privately, through GitHub's
[private vulnerability reporting](https://github.com/uchmk/tsumugi/security/advisories/new),
rather than by opening an issue. An issue is public from the moment it is
filed, which tells everyone about the problem before there is anything to
update to.

Useful in a report, roughly in order of how much they help:

- what an attacker gets, and what they have to be able to do first;
- the output, file or setting that triggers it, or a small one that does;
- the version (the release's name, or `version` in `Cargo.toml`), and which
  Windows, macOS or Linux.

There is no bounty and no guaranteed response time: this is one person's
project. Expect a reply within about a week.

## What is in scope

tsumugi runs a server per user that holds terminal sessions, and draws
whatever the programs in them print. Both are deliberate, so the interesting
question is where that goes further than intended:

- another user on the same machine reaching the server: attaching to a
  session, reading its screen, typing into it (the pipe on Windows and the
  socket elsewhere are meant to be the user's own);
- output from a program -- escape sequences, OSC 7 / 8 / 9 / 52 / 133, a
  title -- that runs something, writes a file, reads the clipboard without
  being asked, or crashes the window or the server rather than being
  ignored;
- a folder name, branch name, tag or path that escapes quoting and becomes
  part of a command (the tab menu's `[open]` and `[[menu.session]]`, the
  worktree commands, `claude --resume`);
- `tsumugi notify` or a hook's JSON changing a session it was not run in;
- importing a settings export writing outside the settings folder;
- the settings screen's changes to files that are not tsumugi's
  (`~/.claude/settings.json`, a shell's profile) touching more than tsumugi's
  own lines.

Not in scope, because it is the program working:

- a command in your `config.toml` (`[open]`, `[[menu.session]]`,
  `[shell]`) running what it says it runs;
- the shell in a pane doing what you type into it, or a program you started
  there doing what it does;
- tsumugi having the same access to your files that you do;
- the update check asking `api.github.com` for the latest release (turn it
  off in Settings → General).

## Supported versions

The latest release. Fixes go into the next version rather than being
backported -- see [releases](https://github.com/uchmk/tsumugi/releases).

## Dependencies

`cargo audit` runs on every push and weekly against the RustSec advisory
database, so an advisory against a dependency turns a check red rather than
waiting to be noticed. Dependabot opens one grouped pull request a month to
move the dependencies and the workflows' actions forward.
