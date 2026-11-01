//! A file name from outside (an e-mail attachment, a download, an archive entry) made safe to write
//! under a folder you choose.

/// Names Windows keeps for devices, with or without an extension (`CON.txt` is `CON`).
const DEVICES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];

/// The longest a name may be on the usual file systems (NTFS, ext4, APFS), in bytes of UTF-8.
const MAX_BYTES: usize = 255;

/// Longest extension kept apart when cutting a long name; a longer "extension" is just part of the name.
const MAX_EXT_BYTES: usize = 32;

fn is_device(stem: &str) -> bool {
    let up = stem.trim_end_matches([' ', '.']).to_ascii_uppercase();
    DEVICES.contains(&up.as_str())
        || ((up.starts_with("COM") || up.starts_with("LPT"))
            && up.len() == 4
            && up.as_bytes()[3].is_ascii_digit()
            && up.as_bytes()[3] != b'0')
}

/// `name` as one safe file name: always a single part, so `dir.join(result)` stays inside `dir`.
///
/// - `/ \ : * ? " < > |` and control characters become `_`.
/// - `.`, `..`, an empty name, and a name of only dots and spaces become `_`; trailing dots and spaces
///   (Windows drops them) are removed.
/// - Device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`, also with an extension) get a `_` in front.
/// - The extension is kept; a name over 255 bytes is cut in its stem, never inside a UTF-8 character.
///
/// Names that are already fine come back unchanged.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let trimmed = cleaned.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        return "_".to_string();
    }
    let (stem, ext) = match trimmed.rfind('.') {
        Some(i) if i > 0 && trimmed.len() - i <= MAX_EXT_BYTES => trimmed.split_at(i),
        _ => (trimmed, ""),
    };
    let device = is_device(stem.split('.').next().unwrap_or(stem));
    let room = MAX_BYTES - ext.len() - usize::from(device);
    let mut end = stem.len().min(room);
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    // Only a cut can leave a trailing dot or space; a stem that was whole keeps its own.
    let stem = if end < stem.len() { stem[..end].trim_end_matches([' ', '.']) } else { stem };
    let stem = if stem.is_empty() { "_" } else { stem };
    format!("{}{stem}{ext}", if device { "_" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::safe_file_name as safe;

    #[test]
    fn good_names_are_kept() {
        for n in ["report.pdf", "日本語 の ファイル.docx", ".bashrc", "a.tar.gz", "no_ext", "CONSOLE.txt", "COM10.txt", "COM0"] {
            assert_eq!(safe(n), n);
        }
    }

    #[test]
    fn separators_and_windows_characters_become_underscores() {
        assert_eq!(safe("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(safe("a\\b:c*d?e\"f<g>h|i.txt"), "a_b_c_d_e_f_g_h_i.txt");
        assert_eq!(safe("tab\there\u{7}.txt"), "tab_here_.txt");
        assert_eq!(safe("C:\\x.txt"), "C__x.txt");
    }

    #[test]
    fn dots_spaces_and_empty_names_are_fixed() {
        for n in ["", ".", "..", "...", " ", " . ", "/", "\\"] {
            let s = safe(n);
            assert!(!s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\']), "{n:?} -> {s:?}");
        }
        assert_eq!(safe(""), "_");
        assert_eq!(safe(".."), "_");
        assert_eq!(safe("file.txt. . "), "file.txt");
        assert_eq!(safe("name "), "name");
    }

    #[test]
    fn device_names_get_a_prefix_with_or_without_an_extension() {
        assert_eq!(safe("CON"), "_CON");
        assert_eq!(safe("nul.txt"), "_nul.txt");
        assert_eq!(safe("Aux.tar.gz"), "_Aux.tar.gz");
        assert_eq!(safe("COM1"), "_COM1");
        assert_eq!(safe("lpt9.log"), "_lpt9.log");
        assert_eq!(safe("CON .txt"), "_CON .txt");
        assert_eq!(safe("PRN."), "_PRN");
    }

    #[test]
    fn a_long_name_is_cut_in_the_stem_and_keeps_the_extension() {
        let long = format!("{}.pdf", "あ".repeat(200));
        let s = safe(&long);
        assert!(s.len() <= 255 && s.ends_with(".pdf"), "{}", s.len());
        assert!(s.starts_with('あ'));
        let ascii = format!("{}.txt", "a".repeat(400));
        assert_eq!(safe(&ascii).len(), 255);
        let device = format!("con{}", ".x".repeat(200));
        assert!(safe(&device).len() <= 255);
        let all = "x".repeat(1000);
        assert_eq!(safe(&all).len(), 255);
        let odd_ext = format!("a.{}", "e".repeat(300));
        assert!(safe(&odd_ext).len() <= 255);
    }

    #[test]
    fn the_result_never_leaves_the_folder() {
        let dir = std::path::Path::new("base");
        for n in ["..", "../x", "/abs", "C:\\abs", "a/../../b", "\\\\server\\share", "..\\..", "x/", "CON", "aux.", "\u{0}"] {
            let s = safe(n);
            let p = dir.join(&s);
            assert_eq!(p.parent(), Some(dir), "{n:?} -> {s:?}");
            assert!(p.components().count() == 2, "{n:?} -> {s:?}");
        }
    }
}
