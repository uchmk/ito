//! "Check for updates": ask GitHub for an app's latest release and say whether
//! it is newer than the running build. Through the system's `curl` (in
//! Windows since 10, and on macOS and Linux), so no HTTP client is built in;
//! a machine without it, or offline, answers [`Update::Unknown`] and the app
//! says nothing.

use std::sync::mpsc::Receiver;

/// What the check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// A release newer than ours: its version, without the `v`.
    Newer(String),
    /// Ours is the latest (or newer: a build from main).
    Current,
    /// No answer: offline, no `curl`, rate limited, or not a version.
    Unknown,
}

/// Ask on a thread of its own about `repo` (`uchmk/kura`) against `ours`
/// (`env!("CARGO_PKG_VERSION")`); `wake` runs after the answer is sent, to
/// repaint the window.
pub fn check(repo: &str, ours: &str, wake: impl Fn() + Send + 'static) -> Receiver<Update> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (repo, ours) = (repo.to_owned(), ours.to_owned());
    let _ = std::thread::Builder::new().name("update-check".into()).spawn(move || {
        let answer = fetch(&repo).map_or(Update::Unknown, |body| judge(&body, &ours));
        let _ = tx.send(answer);
        wake();
    });
    rx
}

fn fetch(repo: &str) -> Option<String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let agent = repo.rsplit('/').next().unwrap_or("uchmk");
    let mut cmd = std::process::Command::new("curl");
    cmd.args(["-sfL", "--max-time", "10", "-H", "Accept: application/vnd.github+json", "-A", agent, &url]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up from a window app.
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.stdin(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// What the releases/latest answer `body` says against `ours`.
pub fn judge(body: &str, ours: &str) -> Update {
    let Some(theirs) = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("tag_name").and_then(|t| t.as_str()).map(|t| t.trim_start_matches('v').to_owned()))
    else {
        return Update::Unknown;
    };
    match (parts(&theirs), parts(ours)) {
        (Some(t), Some(o)) if t > o => Update::Newer(theirs),
        (Some(_), Some(_)) => Update::Current,
        _ => Update::Unknown,
    }
}

fn parts(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split(['.', '-', '+']).map(|p| p.parse::<u64>());
    Some((it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_newer_release_is_newer() {
        assert_eq!(judge(r#"{"tag_name":"v0.42.0","name":"x"}"#, "0.41.3"), Update::Newer("0.42.0".into()));
        assert_eq!(judge(r#"{"tag_name":"v0.41.3"}"#, "0.41.3"), Update::Current);
        assert_eq!(judge(r#"{"tag_name":"v0.9.10"}"#, "0.10.0"), Update::Current);
        assert_eq!(judge(r#"{"message":"Not Found"}"#, "0.1.0"), Update::Unknown);
        assert_eq!(judge("not json", "0.1.0"), Update::Unknown);
        assert_eq!(judge(r#"{"tag_name":"nightly"}"#, "0.1.0"), Update::Unknown);
    }
}
