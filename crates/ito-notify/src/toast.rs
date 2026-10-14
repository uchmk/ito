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

/// The app's icon drawn at a size: `side` x `side` RGBA pixels, as the
/// apps' own `icon` modules draw the window's.
pub type Icon = fn(u32) -> Vec<u8>;

/// The side the icon is written at for a notification.
const ICON_SIDE: u32 = 128;

/// Show `title` and `body`.
pub fn show(app: App, title: &str, body: &str) {
    send(app, None, title, body);
}

/// Show `title` and `body` with the app's icon: on the notification and
/// beside the name on Windows, as the notification's icon where
/// `notify-send` shows one. macOS's AppleScript shows the script's own.
pub fn show_with_icon(app: App, icon: Icon, title: &str, body: &str) {
    send(app, Some(icon), title, body);
}

fn send(app: App, icon: Option<Icon>, title: &str, body: &str) {
    let (id, name, title, body) = (app.id.to_owned(), app.name.to_owned(), title.to_owned(), body.to_owned());
    let _ = std::thread::Builder::new().name("toast".into()).spawn(move || {
        let logo = icon.and_then(|f| icon_file(&id, f));
        os::show(&id, &name, logo.as_deref(), &title, &body);
    });
}

/// The icon as a PNG in the user's cache (`uchmk/notify/<id>.png`), for the
/// system to read: its path, or None if it could not be written. Written
/// once a run, so a newer icon replaces an old one at the next start.
pub fn icon_file(id: &str, icon: Icon) -> Option<std::path::PathBuf> {
    use std::sync::{Mutex, OnceLock};
    static WRITTEN: OnceLock<Mutex<Vec<(String, std::path::PathBuf)>>> = OnceLock::new();
    let mut written = WRITTEN.get_or_init(Mutex::default).lock().ok()?;
    if let Some((_, p)) = written.iter().find(|(i, _)| i == id) {
        return Some(p.clone());
    }
    let dir = cache_dir()?.join("uchmk").join("notify");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{}.png", file_name(id)));
    std::fs::write(&path, png(ICON_SIDE, &icon(ICON_SIDE))?).ok()?;
    written.push((id.to_owned(), path.clone()));
    Some(path)
}

/// Where a per-user cache goes: the local app data on Windows, `~/Library/Caches`
/// on macOS, `$XDG_CACHE_HOME` or `~/.cache` elsewhere.
fn cache_dir() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Caches"))
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache")))
    }
}

/// An id as a file name: anything but letters, digits, `.`, `-` and `_` is `_`.
fn file_name(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect()
}

/// `rgba` (side x side) as PNG; None if the pixels do not fill it.
fn png(side: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    if rgba.len() != (side * side * 4) as usize {
        return None;
    }
    let mut out = Vec::new();
    let mut enc = ::png::Encoder::new(&mut out, side, side);
    enc.set_color(::png::ColorType::Rgba);
    enc.set_depth(::png::BitDepth::Eight);
    let mut w = enc.write_header().ok()?;
    w.write_image_data(rgba).ok()?;
    w.finish().ok()?;
    Some(out)
}

/// The notification's picture in place of the app's logo (Windows).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn logo_xml(logo: Option<&std::path::Path>) -> String {
    logo.map_or_else(String::new, |p| {
        format!("<image placement=\"appLogoOverride\" src=\"file:///{}\"/>", xml_escape(&p.display().to_string().replace('\\', "/")))
    })
}

/// Text put inside the notification's XML (Windows).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Text put inside an AppleScript string (macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(windows)]
pub(crate) mod os {
    use super::xml_escape;
    use windows::core::{w, HSTRING};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    /// Register the id with the name the notifications show, and the small
    /// mark (`logo`, a picture file) beside it.
    pub fn register(id: &str, name: &str, logo: Option<&std::path::Path>) {
        let key = HSTRING::from(format!("Software\\Classes\\AppUserModelId\\{id}"));
        // SAFETY: the key and the value are alive for the calls.
        unsafe {
            let mut k = HKEY::default();
            if RegCreateKeyExW(HKEY_CURRENT_USER, &key, None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut k, None).is_ok() {
                let name: Vec<u8> = format!("{name}\0").encode_utf16().flat_map(u16::to_le_bytes).collect();
                let _ = RegSetValueExW(k, w!("DisplayName"), None, REG_SZ, Some(&name));
                if let Some(path) = logo {
                    let uri: Vec<u8> = format!("{}\0", path.display()).encode_utf16().flat_map(u16::to_le_bytes).collect();
                    let _ = RegSetValueExW(k, w!("IconUri"), None, REG_SZ, Some(&uri));
                }
                let _ = RegCloseKey(k);
            }
        }
    }

    fn toast(id: &str, logo: Option<&std::path::Path>, title: &str, body: &str) -> windows::core::Result<()> {
        let xml = format!(
            "<toast><visual><binding template=\"ToastGeneric\">{}<text>{}</text><text>{}</text></binding></visual></toast>",
            super::logo_xml(logo),
            xml_escape(title),
            xml_escape(body)
        );
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(id))?.Show(&toast)
    }

    pub fn show(id: &str, name: &str, logo: Option<&std::path::Path>, title: &str, body: &str) {
        // SAFETY: a thread of its own; already initialized is fine.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        register(id, name, logo);
        if let Err(e) = toast(id, logo, title, body) {
            eprintln!("the notification failed: {e}");
        }
    }
}

#[cfg(not(windows))]
mod os {
    use std::process::{Command, Stdio};

    #[cfg(target_os = "macos")]
    pub fn show(_id: &str, _name: &str, _logo: Option<&std::path::Path>, title: &str, body: &str) {
        let script = format!("display notification {} with title {}", super::applescript_string(body), super::applescript_string(title));
        let _ = Command::new("osascript").arg("-e").arg(script).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }

    #[cfg(not(target_os = "macos"))]
    pub fn show(_id: &str, name: &str, logo: Option<&std::path::Path>, title: &str, body: &str) {
        let _ = Command::new("notify-send")
            .arg(format!("--app-name={name}"))
            .args(logo.map(|p| format!("--icon={}", p.display())))
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

    #[test]
    fn the_icon_is_a_png_of_its_side() {
        let p = png(2, &[255; 16]).unwrap();
        assert_eq!(&p[..8], &[0x89, b'P', b'N', b'G', 13, 10, 0x1a, 10]);
        assert_eq!(&p[16..24], &[0, 0, 0, 2, 0, 0, 0, 2], "width and height in the header");
        assert!(png(2, &[255; 12]).is_none(), "too few pixels");
        assert_eq!(file_name("uchmk.kura"), "uchmk.kura");
        assert_eq!(file_name("a/b c"), "a_b_c");
        assert_eq!(logo_xml(None), "");
        assert_eq!(
            logo_xml(Some(std::path::Path::new("C:\\x\\a&b.png"))),
            "<image placement=\"appLogoOverride\" src=\"file:///C:/x/a&amp;b.png\"/>"
        );
    }
}
