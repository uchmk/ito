//! The system's own notification, once, on a thread of its own: a toast on
//! Windows (registered under the user's own keys, no installer or shortcut
//! needed), AppleScript on macOS, `notify-send` elsewhere. Where none is
//! there, nothing shows.

/// Who the notification is from: the id the system groups it under and the
/// name it shows.
#[derive(Clone, Copy, Debug)]
pub struct App<'a> {
    /// A stable id such as `uchmk.yagura`.
    pub id: &'a str,
    /// The name beside the notification, such as `yagura`.
    pub name: &'a str,
}

/// Show `title` and `body`.
pub fn show(app: App, title: &str, body: &str) {
    let (id, name, title, body) = (app.id.to_owned(), app.name.to_owned(), title.to_owned(), body.to_owned());
    let _ = std::thread::Builder::new().name("toast".into()).spawn(move || os::show(&id, &name, &title, &body));
}

/// Text put inside the notification's XML (Windows).
#[cfg_attr(not(windows), allow(dead_code))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Text put inside an AppleScript string (macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(windows)]
mod os {
    use super::xml_escape;
    use windows::core::{w, HSTRING};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    /// Register the id with the name the notifications show.
    fn register(id: &str, name: &str) {
        let key = HSTRING::from(format!("Software\\Classes\\AppUserModelId\\{id}"));
        // SAFETY: the key and the value are alive for the calls.
        unsafe {
            let mut k = HKEY::default();
            if RegCreateKeyExW(HKEY_CURRENT_USER, &key, None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut k, None).is_ok() {
                let name: Vec<u8> = format!("{name}\0").encode_utf16().flat_map(u16::to_le_bytes).collect();
                let _ = RegSetValueExW(k, w!("DisplayName"), None, REG_SZ, Some(&name));
                let _ = RegCloseKey(k);
            }
        }
    }

    fn toast(id: &str, title: &str, body: &str) -> windows::core::Result<()> {
        let xml = format!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
            xml_escape(title),
            xml_escape(body)
        );
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(id))?.Show(&toast)
    }

    pub fn show(id: &str, name: &str, title: &str, body: &str) {
        // SAFETY: a thread of its own; already initialized is fine.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        register(id, name);
        if let Err(e) = toast(id, title, body) {
            eprintln!("the notification failed: {e}");
        }
    }
}

#[cfg(not(windows))]
mod os {
    use std::process::{Command, Stdio};

    #[cfg(target_os = "macos")]
    pub fn show(_id: &str, _name: &str, title: &str, body: &str) {
        let script = format!("display notification {} with title {}", super::applescript_string(body), super::applescript_string(title));
        let _ = Command::new("osascript").arg("-e").arg(script).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }

    #[cfg(not(target_os = "macos"))]
    pub fn show(_id: &str, name: &str, title: &str, body: &str) {
        let _ = Command::new("notify-send")
            .arg(format!("--app-name={name}"))
            .args(["--", title, body])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_escaped_for_where_they_go() {
        assert_eq!(xml_escape("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
        assert_eq!(applescript_string("say \"hi\" \\"), "\"say \\\"hi\\\" \\\\\"");
    }
}
