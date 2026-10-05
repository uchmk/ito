//! Which shell the pane starts, what it is called, and how a word is quoted for it.

use std::path::Path;



#[allow(unused_imports)]
use crate::{grid::*, keys::*, log::*, osc::*, terminal::*};

/// The shell the pane starts when `[term] shell` names none.
///
/// On Windows that is `pwsh` when PowerShell 7 is installed, and Windows
/// PowerShell 5.1 when it is not (Q29), which is how Windows Terminal picks
/// too. 5.1 lacks `LocationChangedAction`, the hook the README gives for
/// bringing the directory back, and ships a PSReadLine without prediction, so
/// starting it on a machine that has 7 was starting the worse of two shells.
/// `None` is the platform's own default: `powershell` on Windows, the login
/// shell elsewhere.
pub fn default_shell() -> Option<String> {
    pick_default_shell(cfg!(windows), crate::util::locate("pwsh").is_some())
}

pub(crate) fn pick_default_shell(windows: bool, have_pwsh: bool) -> Option<String> {
    (windows && have_pwsh).then(|| "pwsh".to_owned())
}

/// The program the pane starts when `[term] shell` names none: `pwsh` on
/// Windows where it is installed, else Windows PowerShell; elsewhere the
/// login shell in `$SHELL`, which is what the pty starts. `filer env` asked
/// this question with its own answer, `sh`, while the pane ran bash (#131).
pub fn default_program() -> String {
    default_program_from(default_shell(), cfg!(windows), std::env::var("SHELL").ok())
}

pub(crate) fn default_program_from(picked: Option<String>, windows: bool, env_shell: Option<String>) -> String {
    match (picked, windows) {
        (Some(p), _) => p,
        (None, true) => "powershell".to_owned(),
        (None, false) => env_shell.filter(|s| !s.is_empty()).unwrap_or_else(|| "sh".to_owned()),
    }
}

/// What the pane is about to start, in the words its first toast uses. The
/// difference between PowerShell 5.1 and 7 is where a shell hook quietly stops
/// working, and until this it showed only in the banner or `filer env` (#107).
pub fn shell_label(program: Option<&str>) -> String {
    name_shell(program, cfg!(windows), std::env::var("SHELL").ok())
}

pub(crate) fn name_shell(program: Option<&str>, windows: bool, env_shell: Option<String>) -> String {
    const PS51: &str = "powershell (Windows PowerShell 5.1)";
    match program {
        // Named in the config, the same 5.1 gets the same words: writing
        // `shell = "powershell"` is the ordinary way to get 5.1 on a machine
        // that has 7, and it said only `powershell` -- the one case the
        // version is there for (#136, 29.8). By file name, so a full path to
        // `powershell.exe` reads the same.
        Some(p) if windows && is_powershell_51(p) => PS51.to_owned(),
        Some(p) => crate::util::file_name(Path::new(p)),
        // The platform default, as `spawn` documents it.
        None if windows => PS51.to_owned(),
        None => env_shell.map_or_else(|| "sh".to_owned(), |s| crate::util::file_name(Path::new(&s))),
    }
}

/// `powershell`, `PowerShell.exe` or a full path to it, split on either
/// separator so the rule reads the same whichever platform runs the test.
pub(crate) fn is_powershell_51(program: &str) -> bool {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let stem = name.len().checked_sub(4).filter(|&i| name[i..].eq_ignore_ascii_case(".exe")).map_or(name, |i| &name[..i]);
    stem.eq_ignore_ascii_case("powershell")
}

/// How the shell in the pane wants a word with something awkward in it.
///
/// They do not agree, and the one filer opens by default is the one that
/// agrees least: this used to emit the POSIX form for everything, on a
/// Windows-first program whose pane runs PowerShell unless told otherwise.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Quoting {
    /// `powershell`, `pwsh`. A single quote inside is **doubled**.
    #[default]
    PowerShell,
    /// `bash`, `sh`, `zsh`, … A single quote inside is closed, escaped with a
    /// backslash, and reopened.
    Posix,
    /// `cmd.exe`, where single quotes mean nothing at all -- they would be
    /// handed to the program as part of the name. Double quotes are the
    /// grouping, and a `"` inside a path is not legal on Windows anyway.
    Cmd,
}

impl Quoting {
    /// Work out the convention from what `[term] shell` names.
    ///
    /// An unknown shell on Windows is far likelier to be PowerShell-shaped
    /// than POSIX-shaped, and elsewhere the reverse, so the platform decides
    /// what the fallback is rather than one convention being assumed for all.
    pub fn for_shell(program: Option<&str>) -> Self {
        let Some(program) = program else {
            // What `tty::Options { shell: None }` starts.
            return if cfg!(windows) { Self::PowerShell } else { Self::Posix };
        };
        // Split on both separators by hand rather than through `Path`, which
        // only knows the host's: `C:\\WINDOWS\\System32\\cmd.exe` is one long
        // component on Linux, and this string comes out of a config file that
        // may name either shape.
        let name = program.rsplit(['/', '\\']).next().unwrap_or(program).to_ascii_lowercase();
        let name = [".exe", ".cmd", ".bat"]
            .iter()
            .find_map(|x| name.strip_suffix(x))
            .unwrap_or(&name)
            .to_owned();
        match name.as_str() {
            "powershell" | "pwsh" => Self::PowerShell,
            "cmd" => Self::Cmd,
            "bash" | "sh" | "zsh" | "dash" | "ksh" | "fish" => Self::Posix,
            _ => {
                if cfg!(windows) {
                    Self::PowerShell
                } else {
                    Self::Posix
                }
            }
        }
    }
}

/// Wrap a path for a shell that is about to read it as one word.
///
/// Until v0.47.34 this wrote the POSIX form whatever the shell was, and said
/// in its own doc comment that PowerShell read it the same way. It does not:
/// PowerShell doubles a single quote inside single quotes, and reads the
/// POSIX `'\''` as a closed string followed by a stray backslash and an
/// unterminated one. Section 1 on the Windows machine found the pane sitting
/// at the `>>` continuation prompt after `<A-t>` on a file with a quote in its
/// name -- and getting out of that with `<C-c>` used to quit filer.
pub fn quote(s: &str, how: Quoting) -> String {
    // `'` is deliberately not in the safe set: cmd leaves it alone, but the
    // other two do not, and a name that needs no quotes in one shell still has
    // to come out right in the others.
    //
    // `\` is safe only where it is *nothing but* a path separator. A POSIX shell
    // reads it as an escape, so an unquoted `R:\Temp\x` arrives as `R:Tempx`,
    // and with `[term] shell` set to Git Bash **every ordinary Windows path
    // failed** -- `follow()` typed a `cd` that could not land, so walking the
    // list left the pane behind. Section 1 on the Windows machine found it, and
    // the same run showed the cure already works: 1.21's quoted
    // `cd 'R:\Temp\filer-fixtures\it'\''s here'` arrived as
    // `/r/Temp/filer-fixtures/it's here`, so Git Bash takes `\` intact inside
    // quotes. The test below used to pin the broken form for all three shells.
    let safe: &str = match how {
        Quoting::Posix => "_-./:",
        Quoting::PowerShell | Quoting::Cmd => "_-./:\\",
    };
    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || safe.contains(c)) {
        return s.to_owned();
    }
    match how {
        Quoting::PowerShell => format!("'{}'", s.replace('\'', "''")),
        Quoting::Posix => format!("'{}'", s.replace('\'', r"'\''")),
        // Nothing to escape: a `"` cannot be in a Windows path, and `'` is an
        // ordinary character here.
        Quoting::Cmd => format!("\"{s}\""),
    }
}
