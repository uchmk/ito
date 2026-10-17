//! Starting an app when the user logs in. The state is the OS's own, not a
//! setting file: a value under `HKCU\…\Run` on Windows, a LaunchAgent on
//! macOS, an autostart `.desktop` file on Linux. Every call touches the disk
//! or the registry, so run it off the UI thread.

use std::path::{Path, PathBuf};

/// The command line that starts `app`: the program, quoted, then `args`.
pub fn command_line(exe: &Path, args: &[&str]) -> String {
    let mut out = format!("\"{}\"", exe.display());
    for a in args {
        out.push(' ');
        if a.is_empty() || a.contains([' ', '"']) {
            out.push_str(&format!("\"{}\"", a.replace('"', "\\\"")));
        } else {
            out.push_str(a);
        }
    }
    out
}

/// The name the entry has in the OS's list: `uchmk-<app>`.
fn entry_name(app: &str) -> String {
    format!("uchmk-{app}")
}

/// A LaunchAgent plist that runs `exe args` at login.
pub fn plist(app: &str, exe: &Path, args: &[&str]) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut argv = format!("    <string>{}</string>\n", esc(&exe.display().to_string()));
    for a in args {
        argv.push_str(&format!("    <string>{}</string>\n", esc(a)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>com.uchmk.{app}</string>\n  <key>ProgramArguments</key>\n  <array>\n{argv}  </array>\n  <key>RunAtLoad</key>\n  <true/>\n</dict>\n</plist>\n"
    )
}

/// An XDG autostart entry that runs `exe args` at login.
pub fn desktop(app: &str, exe: &Path, args: &[&str]) -> String {
    format!("[Desktop Entry]\nType=Application\nName={app}\nExec={}\nX-GNOME-Autostart-enabled=true\n", command_line(exe, args))
}

/// Where the macOS / Linux entry is, under the home folder `home` (and the
/// XDG config folder `xdg` on Linux). `None` on Windows, which has no file.
pub fn entry_path(app: &str, home: &Path, xdg: Option<&Path>) -> Option<PathBuf> {
    if cfg!(windows) {
        None
    } else if cfg!(target_os = "macos") {
        Some(home.join("Library").join("LaunchAgents").join(format!("com.uchmk.{app}.plist")))
    } else {
        let config = xdg.map_or_else(|| home.join(".config"), Path::to_path_buf);
        Some(config.join("autostart").join(format!("{}.desktop", entry_name(app))))
    }
}

#[cfg(not(windows))]
fn path_of(app: &str) -> Result<PathBuf, String> {
    let home = crate::home_dir().ok_or("the home folder is not known")?;
    let xdg = crate::env_dir("XDG_CONFIG_HOME").filter(|d| d.is_absolute());
    entry_path(app, &home, xdg.as_deref()).ok_or_else(|| "no entry file on this OS".to_owned())
}

/// Whether `app` starts at login now.
#[cfg(not(windows))]
pub fn enabled(app: &str) -> bool {
    path_of(app).is_ok_and(|p| p.is_file())
}

/// Start `app` (`exe` with `args`) at login, or stop doing so.
#[cfg(not(windows))]
pub fn set(app: &str, exe: &Path, args: &[&str], on: bool) -> Result<(), String> {
    let path = path_of(app)?;
    if !on {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        };
    }
    let text = if cfg!(target_os = "macos") { plist(app, exe, args) } else { desktop(app, exe, args) };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(windows)]
const RUN: &windows::core::PCWSTR = &windows::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Whether `app` starts at login now.
#[cfg(windows)]
pub fn enabled(app: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

    let name = wide(&entry_name(app));
    let mut size = 0u32;
    // SAFETY: `name` is NUL-terminated and outlives the call; `size` is the out pointer.
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, *RUN, PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&raw mut size)) };
    status.is_ok()
}

/// Start `app` (`exe` with `args`) at login, or stop doing so.
#[cfg(windows)]
pub fn set(app: &str, exe: &Path, args: &[&str], on: bool) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

    let name = wide(&entry_name(app));
    if !on {
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, *RUN, PCWSTR(name.as_ptr())) };
        return if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!("the registry said {}", status.0))
        };
    }
    let value = wide(&command_line(exe, args));
    // SAFETY: the strings are NUL-terminated; the byte count is of `value`, which outlives the call.
    let status = unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, *RUN, PCWSTR(name.as_ptr()), REG_SZ.0, Some(value.as_ptr().cast()), (value.len() * 2) as u32)
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("the registry said {}", status.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_quotes_the_program_and_the_odd_arguments() {
        let exe = Path::new("C:\\Program Files\\app\\app.exe");
        assert_eq!(command_line(exe, &["--minimized", "a b"]), "\"C:\\Program Files\\app\\app.exe\" --minimized \"a b\"");
    }

    #[test]
    fn the_plist_and_the_desktop_entry_run_the_program() {
        let exe = Path::new("/usr/bin/app");
        let p = plist("app", exe, &["--x", "a&b"]);
        assert!(p.contains("<string>com.uchmk.app</string>") && p.contains("<string>a&amp;b</string>") && p.contains("RunAtLoad"));
        assert!(desktop("app", exe, &["--x"]).contains("Exec=\"/usr/bin/app\" --x"));
    }

    #[test]
    fn the_entry_lives_where_the_os_looks() {
        let home = Path::new("/home/u");
        let p = entry_path("app", home, None);
        if cfg!(windows) {
            assert!(p.is_none());
        } else if cfg!(target_os = "macos") {
            assert_eq!(p.unwrap(), home.join("Library/LaunchAgents/com.uchmk.app.plist"));
        } else {
            assert_eq!(p.unwrap(), home.join(".config/autostart/uchmk-app.desktop"));
            assert_eq!(entry_path("app", home, Some(Path::new("/x"))).unwrap(), Path::new("/x/autostart/uchmk-app.desktop"));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn turning_it_off_when_it_is_not_there_is_fine() {
        // No file is touched when the entry is absent; only the answer is checked.
        assert!(!enabled("ito-test-no-such-app"));
        assert!(set("ito-test-no-such-app", Path::new("/bin/true"), &[], false).is_ok());
    }
}
