//! A sound of the system's, once,
//! on a thread of its own. `[notify] sound_waiting` and `sound_error` name
//! one of four ([`crate::SOUNDS`]): `chime`, `low`, `alert`, `default`.
//! Windows plays its message sounds; macOS a sound of the system's by
//! `afplay`; elsewhere the freedesktop sound theme's, by `canberra-gtk-play`
//! or `paplay`, when either is there.

/// Play the sound named `name`.
pub fn play(name: &str) {
    let name = name.to_owned();
    let _ = std::thread::Builder::new().name("sound".into()).spawn(move || os::play(&name));
}

/// Windows' focus mode (do not disturb), a presentation or a full-screen
/// game is on: nothing should sound or flash. Best effort: what the shell
/// says of the user's state.
pub fn quiet_time() -> bool {
    os::quiet_time()
}

#[cfg(windows)]
mod os {
    pub fn play(name: &str) {
        use windows::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONASTERISK, MB_ICONEXCLAMATION, MB_ICONHAND, MB_OK};
        let kind = match name {
            "chime" => MB_ICONASTERISK,
            "low" => MB_ICONHAND,
            "alert" => MB_ICONEXCLAMATION,
            _ => MB_OK,
        };
        // SAFETY: no pointers; the sound is the system's.
        let _ = unsafe { MessageBeep(kind) };
    }

    pub fn quiet_time() -> bool {
        use windows::Win32::UI::Shell::{
            SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN,
        };
        // SAFETY: no arguments; it answers a value.
        match unsafe { SHQueryUserNotificationState() } {
            Ok(s) => [QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME].contains(&s),
            Err(_) => false,
        }
    }
}

#[cfg(not(windows))]
mod os {
    use std::process::{Command, Stdio};

    fn run(program: &str, args: &[&str]) -> bool {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[cfg(target_os = "macos")]
    pub fn play(name: &str) {
        let file = match name {
            "chime" => "Glass",
            "low" => "Basso",
            "alert" => "Sosumi",
            _ => "Tink",
        };
        run("afplay", &[&format!("/System/Library/Sounds/{file}.aiff")]);
    }

    #[cfg(not(target_os = "macos"))]
    pub fn play(name: &str) {
        let (event, file) = match name {
            "chime" => ("message-new-instant", "message.oga"),
            "low" => ("dialog-error", "dialog-error.oga"),
            "alert" => ("dialog-warning", "dialog-warning.oga"),
            _ => ("bell", "bell.oga"),
        };
        if !run("canberra-gtk-play", &["-i", event]) {
            run("paplay", &[&format!("/usr/share/sounds/freedesktop/stereo/{file}")]);
        }
    }

    pub fn quiet_time() -> bool {
        false
    }
}
