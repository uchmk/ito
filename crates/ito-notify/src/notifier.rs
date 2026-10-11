//! Notifications a person can answer: buttons on them, one that replaces the
//! last of its tag, and a way back to the app when one is clicked. A
//! [`Notifier`] owns a thread; the notifications it shows live there, so a
//! click on one while the app runs still reaches the callback.
//!
//! On Windows these are toasts with buttons. Where the desktop's
//! `notify-send` (libnotify 0.7.12 or later) can wait for a click, a click on
//! the notification itself comes back; macOS's AppleScript cannot say it was
//! clicked, and an old `notify-send` only shows it.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;

use crate::toast::App;

/// A button on a notification.
#[derive(Clone, Debug)]
pub struct Action {
    /// What the button says.
    pub label: String,
    /// What the click is called with.
    pub arg: String,
    /// The system's own dismiss: the notification goes away and nothing is
    /// called back.
    pub dismiss: bool,
}

/// One notification.
#[derive(Clone, Debug, Default)]
pub struct Toast {
    pub title: String,
    pub body: String,
    /// A newer one with the same tag replaces the last in the Action Center
    /// rather than piling up. It is also what the callback is told.
    pub tag: String,
    /// Stays on screen until it is dealt with, rather than going away by
    /// itself.
    pub urgent: bool,
    /// What a click on the notification itself calls back with.
    pub launch: String,
    pub actions: Vec<Action>,
}

/// Called with the tag of the notification that was clicked and the
/// `arg` of the button (or the `launch` of the notification itself), from
/// whichever thread the system tells it on.
pub type Click = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Shows notifications on a thread of its own.
pub struct Notifier {
    tx: mpsc::Sender<Toast>,
}

impl Notifier {
    /// `logo` is asked for once, before the first notification, for the path
    /// of a picture to show beside the name (Windows).
    pub fn new(app: App<'static>, logo: Option<fn() -> Option<PathBuf>>, click: Click) -> Self {
        let (tx, rx) = mpsc::channel::<Toast>();
        let _ = std::thread::Builder::new().name("notifier".into()).spawn(move || os::run(app, logo, click, rx));
        Self { tx }
    }

    pub fn show(&self, toast: Toast) {
        let _ = self.tx.send(toast);
    }
}

#[cfg(windows)]
mod os {
    use super::{Click, Toast};
    use crate::toast::{os::register, xml_escape, App};
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use windows::core::IInspectable;
    use windows::core::{Interface, HSTRING};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    use windows::UI::Notifications::{ToastActivatedEventArgs, ToastNotification, ToastNotificationManager};

    pub fn run(app: App<'static>, logo: Option<fn() -> Option<PathBuf>>, click: Click, rx: mpsc::Receiver<Toast>) {
        // SAFETY: a thread of its own; already initialized is fine.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // Kept, so a click on one still reaches its handler.
        let mut shown = VecDeque::new();
        let mut logo_path = None;
        let mut registered = false;
        for toast in rx {
            // The name and the logo, written at the first notification only.
            if !registered {
                logo_path = logo.and_then(|f| f());
                register(app.id, app.name, logo_path.as_deref());
                registered = true;
            }
            match make(app.id, &toast, logo_path.as_deref(), click.clone()) {
                Ok(t) => {
                    shown.push_back(t);
                    if shown.len() > 50 {
                        shown.pop_front();
                    }
                }
                Err(e) => eprintln!("the notification failed: {e}"),
            }
        }
    }

    fn make(id: &str, t: &Toast, logo: Option<&std::path::Path>, click: Click) -> windows::core::Result<ToastNotification> {
        let image = logo.map_or_else(String::new, |p| {
            format!("<image placement=\"appLogoOverride\" src=\"file:///{}\"/>", xml_escape(&p.display().to_string().replace('\\', "/")))
        });
        let actions: String = t
            .actions
            .iter()
            .map(|a| {
                let system = if a.dismiss { " activationType=\"system\"" } else { "" };
                format!("<action content=\"{}\" arguments=\"{}\"{system}/>", xml_escape(&a.label), xml_escape(&a.arg))
            })
            .collect();
        let scenario = if t.urgent { " scenario=\"reminder\"" } else { "" };
        let xml = format!(
            "<toast launch=\"{}\"{scenario}><visual><binding template=\"ToastGeneric\">{image}<text>{}</text><text>{}</text></binding></visual>\
             <actions>{actions}</actions></toast>",
            xml_escape(&t.launch),
            xml_escape(&t.title),
            xml_escape(&t.body)
        );
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        if !t.tag.is_empty() {
            toast.SetTag(&HSTRING::from(t.tag.as_str()))?;
            toast.SetGroup(&HSTRING::from("tags"))?;
        }
        let (tag, launch) = (t.tag.clone(), t.launch.clone());
        toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, args| {
            let said = args.as_ref().and_then(|a| a.cast::<ToastActivatedEventArgs>().ok()).and_then(|a| a.Arguments().ok());
            match said.as_ref().map(|a| a.to_string()) {
                None => click(&tag, &launch),
                Some(a) if a.is_empty() => click(&tag, &launch),
                Some(a) => click(&tag, &a),
            }
            Ok(())
        }))?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(id))?.Show(&toast)?;
        Ok(toast)
    }
}

#[cfg(not(windows))]
mod os {
    use super::{Click, Toast};
    use crate::toast::App;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    pub fn run(app: App<'static>, _logo: Option<fn() -> Option<PathBuf>>, click: Click, rx: mpsc::Receiver<Toast>) {
        for toast in rx {
            let click = click.clone();
            // Each on a thread of its own: one that can say it was clicked
            // waits until it is closed.
            let _ = std::thread::Builder::new().name("notify".into()).spawn(move || {
                if show(app, &toast) {
                    click(&toast.tag, &toast.launch);
                }
            });
        }
    }

    /// Show it; whether it was clicked.
    #[cfg(target_os = "macos")]
    fn show(_app: App, t: &Toast) -> bool {
        let script = format!(
            "display notification {} with title {}",
            crate::toast::applescript_string(&t.body),
            crate::toast::applescript_string(&t.title)
        );
        let _ = Command::new("osascript").arg("-e").arg(script).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        false
    }

    /// Show it; whether it was clicked. An older `notify-send` does not know
    /// the options for waiting, and shows it plainly.
    #[cfg(not(target_os = "macos"))]
    fn show(app: App, t: &Toast) -> bool {
        use std::sync::atomic::{AtomicBool, Ordering};
        static PLAIN: AtomicBool = AtomicBool::new(false);
        let run = |actions: bool| {
            let mut cmd = Command::new("notify-send");
            cmd.arg(format!("--app-name={}", app.name));
            if actions {
                cmd.args(["--action=default=Open", "--wait"]);
            }
            cmd.args(["--", &t.title, &t.body]).stdin(Stdio::null()).stderr(Stdio::null());
            cmd.output()
        };
        if !PLAIN.load(Ordering::Relaxed) {
            match run(true) {
                Ok(out) if out.status.success() => return String::from_utf8_lossy(&out.stdout).trim() == "default",
                // Not there at all: nothing more to try.
                Err(_) => return false,
                Ok(_) => PLAIN.store(true, Ordering::Relaxed),
            }
        }
        let _ = run(false);
        false
    }
}
