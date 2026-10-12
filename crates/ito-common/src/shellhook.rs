//! The lines a shell's profile gets so it says which folder it is in
//! (OSC 7) and marks where each prompt starts (OSC 133;A), how the command
//! before it ended (`D;<code>`), and where its output starts (`C`) or where
//! the prompt ends (`B`). tsumugi and kura print them with `shell-hook`;
//! only the app's name and what it uses the folder for differ.
//!
//! Each hook starts with an empty line, so appending it to a file that does
//! not end in a newline does not glue it onto that file's last line.

/// The shells there is a hook for, as `shell-hook` takes them.
pub const SHELLS: &str = "pwsh, bash, zsh";

fn lead(app: &str, purpose: &str, tail: &str) -> String {
    format!("\n# {app}: say where the shell is (OSC 7), {purpose},\n# and mark each prompt, {tail} (OSC 133)\n")
}

/// PowerShell 7. The folder is said on each `cd` rather than from `prompt`,
/// calling whatever handler was there before (`mise activate pwsh` puts one
/// there). The prompt mark wraps the `prompt` there is, as Windows Terminal's
/// own lines do, so Starship's keeps working when it is set up first; and
/// only once. pwsh has no hook before a command runs, so the end of the
/// prompt (`B`) is said instead of the start of the output.
pub fn pwsh(app: &str, purpose: &str) -> String {
    lead(app, purpose, "where it ends and how the command before it ended") + &PWSH.replace("{app}", app)
}

const PWSH: &str = r#"$prev = $ExecutionContext.SessionState.InvokeCommand.LocationChangedAction
$ExecutionContext.SessionState.InvokeCommand.LocationChangedAction = {
    param($sender, $e)
    if ($prev) { $prev.Invoke($sender, $e) }
    $p = $e.NewPath.ProviderPath -replace '\\', '/' -replace '^(?!/)', '/'
    [Console]::Write("$([char]27)]7;file://$p$([char]27)\")
}.GetNewClosure()
if (-not $global:__{app}_marks_input) {
    $global:__{app}_marks_input = $global:__{app}_marked = $true
    # An older hook's wrapper is replaced, around the prompt it kept.
    if ("$function:prompt" -notmatch '__{app}_prompt') { $global:__{app}_prompt = $function:prompt }
    function global:prompt {
        $c = if ($?) { 0 } elseif ($LASTEXITCODE) { $LASTEXITCODE } else { 1 }
        "$([char]27)]133;D;$c$([char]27)\$([char]27)]133;A$([char]27)\" + (& $global:__{app}_prompt) + "$([char]27)]133;B$([char]27)\"
    }
}
"#;

/// bash has no hook on `cd`, so this says it at every prompt; the app only
/// acts on a folder that differs from the one it has.
pub fn bash(app: &str, purpose: &str) -> String {
    lead(app, purpose, "each command's output and how it ended") + &BASH.replace("{app}", app)
}

const BASH: &str = r#"__{app}_osc7() { printf '\e]133;D;%s\e\\\e]7;file://%s%s\e\\\e]133;A\e\\' "$?" "$HOSTNAME" "$PWD"; }
PROMPT_COMMAND="__{app}_osc7${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
PS0="${PS0}\e]133;C\a"
"#;

/// zsh has one: `chpwd`. It does not fire for the directory the shell starts
/// in, hence the call at the end. The prompt mark goes out from `precmd`.
pub fn zsh(app: &str, purpose: &str) -> String {
    lead(app, purpose, "each command's output and how it ended") + &ZSH.replace("{app}", app)
}

const ZSH: &str = r#"__{app}_osc7() { printf '\e]7;file://%s%s\e\\' "$HOST" "$PWD" }
__{app}_mark() { printf '\e]133;D;%s\e\\\e]133;A\e\\' "$?" }
__{app}_run() { printf '\e]133;C\e\\' }
autoload -Uz add-zsh-hook
add-zsh-hook chpwd __{app}_osc7
add-zsh-hook precmd __{app}_mark
add-zsh-hook preexec __{app}_run
__{app}_osc7
"#;

/// The hook for `shell`, by the name its program goes by. With none named,
/// PowerShell's: the one shell that sends nothing unless told to.
pub fn text(app: &str, purpose: &str, shell: Option<&str>) -> Result<String, String> {
    match shell.map(str::to_ascii_lowercase).as_deref() {
        None | Some("pwsh" | "pwsh.exe") => Ok(pwsh(app, purpose)),
        Some("bash" | "bash.exe") => Ok(bash(app, purpose)),
        Some("zsh") => Ok(zsh(app, purpose)),
        Some("powershell" | "powershell.exe") => {
            Err("Windows PowerShell 5.1 has no LocationChangedAction; the hook needs PowerShell 7 (pwsh)".into())
        }
        Some(other) => Err(format!("no hook for {other:?} (one of: {SHELLS})")),
    }
}

fn begin(app: &str) -> String {
    format!("# >>> {app} shell integration >>>")
}

fn end(app: &str) -> String {
    format!("# <<< {app} shell integration <<<")
}

/// The profile with the hook between the app's marks: added at the end, or
/// put in place of the one there.
pub fn with_hook(app: &str, profile: &str, hook: &str) -> String {
    let mut out = without_hook(app, profile);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&begin(app));
    out.push_str(hook);
    out.push_str(&end(app));
    out.push('\n');
    out
}

/// The profile with the app's marked hook taken out.
pub fn without_hook(app: &str, profile: &str) -> String {
    let (begin, end) = (begin(app), end(app));
    let (Some(a), Some(b)) = (profile.find(&begin), profile.find(&end)) else { return profile.to_string() };
    if b < a {
        return profile.to_string();
    }
    let rest = &profile[b + end.len()..];
    let mut out = profile[..a].to_string();
    out.push_str(rest.strip_prefix('\n').unwrap_or(rest));
    out
}

/// The app's marked hook is in the profile.
pub fn has_hook(app: &str, profile: &str) -> bool {
    profile.contains(&begin(app))
}

/// The profile file a shell reads at start, by the shell's program name.
/// PowerShell is asked for its `$PROFILE` (it moves with OneDrive), so this
/// is a thread's work.
pub fn profile(shell: &str, home: &std::path::Path) -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};
    let name = Path::new(shell).file_stem()?.to_string_lossy().to_ascii_lowercase();
    match name.as_str() {
        "bash" => Some(home.join(".bashrc")),
        "zsh" => Some(std::env::var_os("ZDOTDIR").map(PathBuf::from).unwrap_or_else(|| home.to_path_buf()).join(".zshrc")),
        "pwsh" => {
            let mut cmd = std::process::Command::new(shell);
            cmd.args(["-NoLogo", "-NoProfile", "-Command", "$PROFILE.CurrentUserCurrentHost"]).stdin(std::process::Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let asked = cmd.output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|p| !p.is_empty());
            asked.map(PathBuf::from).or_else(|| {
                Some(if cfg!(windows) {
                    home.join("Documents/PowerShell/Microsoft.PowerShell_profile.ps1")
                } else {
                    home.join(".config/powershell/Microsoft.PowerShell_profile.ps1")
                })
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shell_gets_its_own_and_the_rest_are_refused() {
        let t = |s| text("app", "for x", s);
        assert_eq!(t(None), Ok(pwsh("app", "for x")));
        assert_eq!(t(Some("PWSH.EXE")), Ok(pwsh("app", "for x")));
        assert_eq!(t(Some("bash")), Ok(bash("app", "for x")));
        assert_eq!(t(Some("zsh")), Ok(zsh("app", "for x")));
        assert!(t(Some("powershell")).unwrap_err().contains("PowerShell 7"));
        assert!(t(Some("fish")).unwrap_err().contains(SHELLS));
        for hook in [pwsh("app", "for x"), bash("app", "for x"), zsh("app", "for x")] {
            assert!(hook.starts_with("\n# app: say where the shell is (OSC 7), for x,\n#") && hook.ends_with('\n'), "{hook:?}");
            assert!(hook.contains("]7;file://") && hook.contains("]133;A") && hook.contains("133;D;"), "{hook:?}");
            assert!(!hook.contains("{app}"), "{hook:?}");
        }
        assert!(pwsh("app", "x").contains(r#"(& $global:__app_prompt) + "$([char]27)]133;B"#));
    }

    #[test]
    fn the_marked_hook_goes_in_once_and_comes_out_clean() {
        let mine = "alias ll='ls -l'";
        let hook = bash("app", "x");
        let once = with_hook("app", mine, &hook);
        assert!(has_hook("app", &once) && once.starts_with("alias ll='ls -l'\n# >>> app shell"), "{once}");
        assert!(!has_hook("other", &once));
        assert_eq!(with_hook("app", &once, &hook), once, "twice is once");
        assert_eq!(without_hook("app", &once), "alias ll='ls -l'\n");
        assert_eq!(without_hook("app", mine), mine);
        assert_eq!(without_hook("other", &once), once);
    }
}
