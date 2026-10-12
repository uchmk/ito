//! The fonts every uchmk app looks for, in one order (kura's, which the
//! others follow since ito 0.3.0): the user's Nerd Fonts, and the system
//! faces that draw Japanese for what egui's own faces lack. Only paths and
//! names: reading them and handing them to egui is each app's.

use std::path::PathBuf;

/// Nerd Fonts (their icons are what a prompt such as starship draws), looked
/// for in [`font_dirs`] in this order.
pub const NERD_FONTS: [&str; 6] = [
    "HackGen35ConsoleNF-Regular.ttf",
    "HackGenConsoleNF-Regular.ttf",
    "HackGen35Console-Regular.ttf",
    "FiraCodeNerdFont-Regular.ttf",
    "CaskaydiaCoveNerdFont-Regular.ttf",
    "JetBrainsMonoNerdFont-Regular.ttf",
];

/// The window's own words (titles, buttons) in a proportional face, as the
/// design has them (IBM Plex Sans JP): the first of these found by
/// [`find_file`], else egui's own.
pub const UI_REGULAR: &[&str] = &[
    "IBMPlexSansJP-Regular.ttf",
    "IBMPlexSansJP-Regular.otf",
    "IBMPlexSans-Regular.ttf",
    "IBMPlexSans-Regular.otf",
    "segoeui.ttf",
    "NotoSans-Regular.ttf",
    "DejaVuSans.ttf",
];

/// Their bold for titles (egui has no bold of its own: `strong` is only a colour).
pub const UI_BOLD: &[&str] = &[
    "IBMPlexSansJP-SemiBold.ttf",
    "IBMPlexSansJP-SemiBold.otf",
    "IBMPlexSansJP-Bold.ttf",
    "IBMPlexSans-SemiBold.ttf",
    "IBMPlexSans-SemiBold.otf",
    "seguisb.ttf",
    "segoeuib.ttf",
    "NotoSans-SemiBold.ttf",
    "DejaVuSans-Bold.ttf",
];

/// The system faces that draw Japanese, in order; the first that reads is
/// used. Plain paths rather than fontconfig, which would be a C dependency
/// for one lookup.
#[cfg(windows)]
pub const JAPANESE: &[&str] =
    &[r"C:\Windows\Fonts\meiryo.ttc", r"C:\Windows\Fonts\YuGothM.ttc", r"C:\Windows\Fonts\YuGothR.ttc", r"C:\Windows\Fonts\msgothic.ttc"];
#[cfg(target_os = "macos")]
pub const JAPANESE: &[&str] = &[
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
];
/// Debian and Ubuntu, Arch, Fedora; then IPA and Droid, which older or
/// smaller installs carry instead of Noto.
#[cfg(not(any(windows, target_os = "macos")))]
pub const JAPANESE: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
];

/// Their bold, where the system has one. Elsewhere bold is overstruck.
#[cfg(windows)]
pub const JAPANESE_BOLD: &[&str] = &[r"C:\Windows\Fonts\meiryob.ttc", r"C:\Windows\Fonts\YuGothB.ttc"];
#[cfg(target_os = "macos")]
pub const JAPANESE_BOLD: &[&str] = &["/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc"];
#[cfg(not(any(windows, target_os = "macos")))]
pub const JAPANESE_BOLD: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Bold.ttc",
];

/// Where a user's own fonts are, then the system's: a Nerd Font is usually
/// installed for the user alone.
pub fn font_dirs() -> Vec<PathBuf> {
    let var = |k: &str| crate::env_dir(k);
    let mut out = Vec::new();
    if cfg!(windows) {
        out.extend(var("LOCALAPPDATA").map(|d| d.join("Microsoft").join("Windows").join("Fonts")));
        out.push(PathBuf::from(r"C:\Windows\Fonts"));
    } else if cfg!(target_os = "macos") {
        out.extend(var("HOME").map(|h| h.join("Library").join("Fonts")));
        out.push("/Library/Fonts".into());
    } else {
        out.extend(var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("share"))).map(|d| d.join("fonts")));
        out.extend(var("HOME").map(|h| h.join(".fonts")));
        out.push("/usr/share/fonts/truetype".into());
        out.push("/usr/local/share/fonts".into());
    }
    out
}

/// The first of `names` in [`font_dirs`] or one folder below them
/// (`truetype/ibm-plex/…`).
pub fn find_file(names: &[&str]) -> Option<PathBuf> {
    let dirs = font_dirs();
    let mut all = dirs.clone();
    for d in &dirs {
        if let Ok(entries) = std::fs::read_dir(d) {
            all.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    names.iter().find_map(|n| all.iter().map(|d| d.join(n)).find(|p| p.is_file()))
}

/// The user's Nerd Font, the first of [`NERD_FONTS`] found.
pub fn nerd_font() -> Option<PathBuf> {
    font_dirs().iter().flat_map(|d| NERD_FONTS.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// The first of `paths` that reads, with its bytes (tens of megabytes: not
/// on the UI thread).
pub fn read_first(paths: &[&str]) -> Option<(PathBuf, Vec<u8>)> {
    paths.iter().find_map(|p| std::fs::read(p).ok().map(|b| (PathBuf::from(p), b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lists_are_whole_paths_and_file_names() {
        assert!(JAPANESE.iter().chain(JAPANESE_BOLD).all(|p| std::path::Path::new(p).is_absolute()));
        assert!(NERD_FONTS.iter().chain(UI_REGULAR).chain(UI_BOLD).all(|n| !n.contains(['/', '\\'])));
        assert!(read_first(&["/no/such/font.ttc"]).is_none());
    }
}
