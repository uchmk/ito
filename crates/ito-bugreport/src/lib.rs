//! A bug report with the machine's own answers already filled in.
//!
//! The report form asks for a version, an architecture and an OS build before
//! it asks what went wrong. Those are what the program knows about itself and
//! the reporter has to go and look up -- and looking them up is where a report
//! gets abandoned. GitHub's issue forms take query parameters keyed by the
//! field `id`, so the program answers its own questions and leaves the human
//! the part it cannot know: what happened, and which keys.
//!
//! Every app gives the same lines, so a report reads alike whichever app it
//! came from. The form's field ids are fixed in `docs/common-spec.md`
//! (`version`, `os`, `context`; the others, such as `keys`, are the app's).
//! What is the app's own: its name, its repository, and the extra rows.

use std::path::{Path, PathBuf};

/// Where an app's form lives and what it is called.
#[derive(Clone, Copy, Debug)]
pub struct Spec<'a> {
    /// `kura`: the first word of the version line.
    pub app: &'a str,
    /// The app's version (`env!("CARGO_PKG_VERSION")` in the app).
    pub version: &'a str,
    /// The tracker, `https://github.com/uchmk/kura`. A fork points this at its
    /// own: the program cannot know it was forked, and filing reports against
    /// somebody else's repository would be worse than not opening a browser.
    pub repo: &'a str,
    /// The template's filename, `bug_report.yml`.
    pub template: &'a str,
}

/// The URL that opens a pre-filled report: the `version` and `os` fields, and
/// any `extra` fields as `(id, text)`, ids from the template. A field whose
/// text is empty is left out. GitHub ignores a parameter it does not
/// recognise, so a field renamed in the form comes up empty and nothing says so:
/// the app's tests check its form for the ids it fills.
pub fn url(spec: &Spec, extra: &[(&str, String)]) -> String {
    let mut u = format!(
        "{}/issues/new?template={}&version={}&os={}",
        spec.repo,
        spec.template,
        encode(&version_line(spec.app, spec.version)),
        encode(&os_line()),
    );
    for (id, text) in extra.iter().filter(|(_, t)| !t.is_empty()) {
        u.push_str(&format!("&{id}={}", encode(text)));
    }
    u
}

/// What `--version` prints, and for the same reason: with more than one binary
/// in circulation, which one is running is the first thing to settle.
pub fn version_line(app: &str, version: &str) -> String {
    format!("{app} {version} ({})", std::env::consts::ARCH)
}

/// What the program knew when the report was asked for, beyond the version and
/// the OS.
#[derive(Clone, Debug, Default)]
pub struct Context<'a> {
    /// The last error it raised.
    pub last_error: Option<&'a str>,
    /// The home folder, written as `~` wherever it appears: a user name must
    /// not reach a public tracker.
    pub home: Option<&'a Path>,
    /// How it was drawing: the adapter, the backend and the kind of device
    /// (a software one is a finding by itself).
    pub rendering: Option<[&'a str; 3]>,
    /// Pixels per point; shown as a percentage.
    pub scale: Option<f32>,
    /// The config files it read, named by [`short_name`], never by path.
    pub config: &'a [PathBuf],
    /// Folder names that start a config file's short name (`yazi`, `kura`).
    pub anchors: &'a [&'a str],
    /// The app's own rows, `(label, text)`, after the shared ones.
    pub rows: &'a [(&'a str, String)],
}

/// The `context` field: `Label: text` lines, in a fixed order -- Last error,
/// Rendering, Scale, the app's rows, Config -- so reports from every app are
/// read the same way.
pub fn context(c: &Context) -> String {
    let mut lines = Vec::new();
    if let Some(e) = c.last_error {
        lines.push(format!("Last error: {}", without_home(e, c.home)));
    }
    if let Some([adapter, backend, device]) = c.rendering.filter(|r| !r[0].is_empty()) {
        lines.push(format!("Rendering: {adapter} ({backend}, {device})"));
    }
    if let Some(s) = c.scale.filter(|s| *s > 0.0) {
        lines.push(format!("Scale: {:.0}%", s * 100.0));
    }
    for (label, text) in c.rows.iter().filter(|(_, t)| !t.is_empty()) {
        lines.push(format!("{label}: {}", without_home(text, c.home)));
    }
    let names: Vec<String> = c.config.iter().map(|p| short_name(p, c.anchors)).collect();
    lines.push(format!("Config: {}", if names.is_empty() { "defaults only".into() } else { names.join(", ") }));
    lines.join(
        "
",
    )
}

/// `text` with the home folder written as `~`. An error often names a full
/// path (`No such file or folder: … — showing C:\Users\<name>\…`), and the last
/// one went into the report as it was, user name and all, which Q64 says a
/// report never carries (found by the QA agent's tests for section 26).
/// Windows paths are matched without regard to case, and in either slash.
pub fn without_home(text: &str, home: Option<&std::path::Path>) -> String {
    let Some(home) = home.map(|h| h.to_string_lossy().trim_end_matches(['/', '\\']).to_owned()) else {
        return text.to_owned();
    };
    // A home of `/` or `C:` would turn every path into `~`; it says nothing.
    if home.len() < 4 {
        return text.to_owned();
    }
    let mut out = text.to_owned();
    let forms = [home.clone(), home.replace('\\', "/")];
    for form in forms.iter() {
        loop {
            let at = match cfg!(windows) {
                true => out.to_ascii_lowercase().find(&form.to_ascii_lowercase()),
                false => out.find(form.as_str()),
            };
            let Some(at) = at else { break };
            out.replace_range(at..at + form.len(), "~");
        }
    }
    out
}

/// `…\yazi\config\keymap.toml` as `yazi\config\keymap.toml` -- the folders
/// that say whose file it is, without the home directory above them. `anchors`
/// are the folder names that start it; with none found, the last two parts.
pub fn short_name(p: &Path, anchors: &[&str]) -> String {
    let parts: Vec<String> = p.components().rev().take(3).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let tail: Vec<&String> = parts.iter().rev().skip_while(|c| !anchors.contains(&c.as_str())).collect();
    let keep = if tail.is_empty() { parts.iter().rev().skip(1).collect() } else { tail };
    keep.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(std::path::MAIN_SEPARATOR_STR)
}

/// The OS line, in the shape the form's own PowerShell snippet produces, so a
/// report that was filled in by hand and one that came from here read alike.
///
/// `ProcessArchitecture` is this program's own architecture, which is the
/// question worth answering on Windows on ARM: an x64 build running under
/// emulation is the case that quietly misleads everyone, and here the program
/// is the emulated process, so it can say so directly.
/// Everything the PowerShell snippet in the report form prints, so that a
/// report filled in from here needs no correcting afterwards. It used to print
/// less, and the first person to use it went and pasted the fuller version over
/// the top -- which the next `<F12>` then overwrote, since the value lives in
/// the URL.
///
/// Both architectures, because one of them is the whole point. On Windows on
/// ARM an x64 build runs under emulation and every ordinary check agrees it is
/// on x64; the machine underneath is what `GetNativeSystemInfo` answers, and it
/// is unaffected by the emulation. When the two lines disagree, the
/// disagreement is the finding.
#[cfg(windows)]
pub fn os_line() -> String {
    format!("OS: {}\nOS arch: {}\nProcess arch: {}", windows_name(), native_arch(), process_arch(native_arch(), std::env::consts::ARCH),)
}

/// The process's own architecture, saying so outright when it is not the
/// machine's. Two lines that disagree were the finding, but only to a reader
/// who compared them; #84 read past it twice.
#[cfg(any(windows, test))]
fn process_arch(native: &str, own: &str) -> String {
    if native == own {
        own.to_owned()
    } else {
        format!("{own} (emulated on {native})")
    }
}

/// `Windows 11 Pro 25H2 (build 26200.9457)`, assembled from the two places
/// Windows keeps the pieces.
#[cfg(windows)]
fn windows_name() -> String {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

    let mut info = OSVERSIONINFOW { dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
    // `RtlGetVersion` rather than `GetVersionEx`, which lies about anything
    // past Windows 8 unless the binary carries a compatibility manifest.
    if unsafe { RtlGetVersion(&mut info) }.is_err() {
        return "Windows (version unavailable)".to_owned();
    }

    // Not `ProductName` from the registry: on Windows 11 it still reads
    // "Windows 10 Pro", which is the single most misleading string on the
    // machine. The build number is what actually separates the two.
    let mut name = if info.dwBuildNumber >= 22000 { "Windows 11" } else { "Windows 10" }.to_owned();

    // The registry spells two editions the way nobody else does: "Professional"
    // is "Pro" on the box and the About page, and "Core" is Home -- the ARM64
    // laptop's report read `Windows 11 Core`, which names no edition anyone
    // has bought.
    if let Some(ed) = reg_string("EditionID") {
        name.push(' ');
        name.push_str(edition_name(&ed));
    }
    // 25H2 and friends. Absent on builds old enough to use `ReleaseId`, and a
    // missing feature update is not worth an apology in the middle of a line.
    if let Some(display) = reg_string("DisplayVersion") {
        name.push(' ');
        name.push_str(&display);
    }
    match reg_dword("UBR") {
        Some(ubr) => format!("{name} (build {}.{ubr})", info.dwBuildNumber),
        None => format!("{name} (build {})", info.dwBuildNumber),
    }
}

/// `EditionID` as the edition is sold. Only the two the registry spells
/// differently are mapped; anything else (`Enterprise`, `Education`) already
/// reads right.
#[cfg(any(windows, test))]
fn edition_name(id: &str) -> &str {
    match id {
        "Professional" => "Pro",
        "Core" => "Home",
        other => other,
    }
}

/// The architecture of the machine, not of this process -- the whole reason
/// the line exists is to show the two disagreeing under emulation.
///
/// `IsWow64Process2`'s native machine first. `GetNativeSystemInfo` was used
/// alone until v0.51.2 on the strength of its name, and ARM64's x64 emulation
/// answers it with AMD64 for compatibility: the ARM64 laptop's x64 build said
/// `OS arch x86_64`, so a report from an emulated build claimed an x64
/// machine (TESTING.md 26.6 / 25.5). Only the *native* half is read: for an
/// x64 process on ARM64 the process half comes back `UNKNOWN`, since that is
/// emulation and not WOW64. `IsWow64Process2` is linked at load time and needs
/// Windows 10 1709; everything older is long out of support.
#[cfg(windows)]
fn native_arch() -> &'static str {
    use windows::Win32::System::SystemInformation::{
        GetNativeSystemInfo, IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64, IMAGE_FILE_MACHINE_I386,
        PROCESSOR_ARCHITECTURE_AMD64, PROCESSOR_ARCHITECTURE_ARM64, PROCESSOR_ARCHITECTURE_INTEL, SYSTEM_INFO,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};

    let (mut process, mut native) = (IMAGE_FILE_MACHINE::default(), IMAGE_FILE_MACHINE::default());
    if unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, Some(&mut native)) }.is_ok() {
        if native == IMAGE_FILE_MACHINE_ARM64 {
            return "aarch64";
        } else if native == IMAGE_FILE_MACHINE_AMD64 {
            return "x86_64";
        } else if native == IMAGE_FILE_MACHINE_I386 {
            return "x86";
        }
    }

    let mut si = SYSTEM_INFO::default();
    unsafe { GetNativeSystemInfo(&mut si) };
    // Spelled the way Rust spells `std::env::consts::ARCH`, so the two lines
    // can be compared without translating between them.
    let arch = unsafe { si.Anonymous.Anonymous.wProcessorArchitecture };
    if arch == PROCESSOR_ARCHITECTURE_AMD64 {
        "x86_64"
    } else if arch == PROCESSOR_ARCHITECTURE_ARM64 {
        "aarch64"
    } else if arch == PROCESSOR_ARCHITECTURE_INTEL {
        "x86"
    } else {
        "unknown"
    }
}

/// One string from `CurrentVersion`. `None` for anything that goes wrong: a
/// bug report missing the edition is still a bug report.
#[cfg(windows)]
fn reg_string(name: &str) -> Option<String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut buf = [0u16; 128];
    let mut bytes = std::mem::size_of_val(&buf) as u32;
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    }
    .is_ok()
    .then_some(())?;
    // The count comes back in bytes and includes the terminator.
    let chars = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars.min(buf.len())]))
}

/// The update build revision -- the `.9457` that `winver` shows and the version
/// struct does not carry.
#[cfg(windows)]
fn reg_dword(name: &str) -> Option<u32> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};

    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut value = 0u32;
    let mut bytes = std::mem::size_of::<u32>() as u32;
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut bytes),
        )
    }
    .is_ok()
    .then_some(())?;
    Some(value)
}

#[cfg(not(windows))]
pub fn os_line() -> String {
    // No emulation story to tell here, so the two arch lines would say the same
    // thing twice.
    format!("OS: {}\nProcess arch: {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Percent-encode for a query string.
///
/// Written out rather than pulled in: it is a dozen lines against a new
/// dependency, and the rule is small enough to state. Everything outside the
/// unreserved set of RFC 3986 is escaped, which is stricter than a browser
/// needs and never wrong.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: Spec = Spec { app: "demo", version: "1.2.3", repo: "https://github.com/uchmk/demo", template: "bug_report.yml" };

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s.replace('/', std::path::MAIN_SEPARATOR_STR))
    }

    /// An x64 build on an ARM64 machine says it is emulated in the line itself.
    #[test]
    fn an_emulated_process_says_so() {
        assert_eq!(process_arch("aarch64", "x86_64"), "x86_64 (emulated on aarch64)");
        assert_eq!(process_arch("aarch64", "aarch64"), "aarch64");
    }

    /// Home and Pro as they are sold, not as the registry spells them.
    #[test]
    fn editions_read_as_they_are_sold() {
        assert_eq!(edition_name("Core"), "Home");
        assert_eq!(edition_name("Professional"), "Pro");
        assert_eq!(edition_name("Enterprise"), "Enterprise");
    }

    #[test]
    fn encodes_what_a_query_string_cannot_carry() {
        assert_eq!(encode("demo 0.1.0 (x86_64)"), "demo%200.1.0%20%28x86_64%29");
        assert_eq!(encode("a&b=c#d"), "a%26b%3Dc%23d");
        assert_eq!(encode("a\nb"), "a%0Ab");
        assert_eq!(encode("-_.~"), "-_.~");
        // Per byte, not per character: a path in a report may well be Japanese.
        assert_eq!(encode("あ"), "%E3%81%82");
    }

    #[test]
    fn the_url_carries_the_template_the_shared_fields_and_the_extras() {
        let u = url(&SPEC, &[("keys", "j j".into()), ("context", String::new())]);
        assert!(u.starts_with("https://github.com/uchmk/demo/issues/new?template=bug_report.yml&version=demo%201.2.3%20"), "{u}");
        assert!(u.contains("&os=OS%3A%20"), "{u}");
        assert!(u.ends_with("&keys=j%20j"), "{u}");
        assert!(!u.contains("context="), "an empty field is left out: {u}");
        assert!(!u.contains(' '), "nothing unescaped can split the parameters: {u}");
    }

    #[test]
    fn a_config_file_is_named_without_its_path() {
        let sep = std::path::MAIN_SEPARATOR;
        let a = ["yazi", "demo"];
        assert_eq!(short_name(&p("/home/someone/.config/yazi/keymap.toml"), &a), format!("yazi{sep}keymap.toml"));
        assert_eq!(short_name(&p("/Users/someone/AppData/Roaming/yazi/config/yazi.toml"), &a), format!("yazi{sep}config{sep}yazi.toml"));
        assert_eq!(short_name(&p("/somewhere/else/conf/config.toml"), &a), format!("conf{sep}config.toml"));
    }

    #[test]
    fn the_home_folder_never_reaches_the_report() {
        let home = p("/home/someone");
        let said = without_home("No such file or folder: /home/someone/x/y — showing /home/someone/x", Some(&home));
        assert_eq!(said, "No such file or folder: ~/x/y — showing ~/x");
        assert_eq!(without_home("/srv/data: denied", Some(&home)), "/srv/data: denied", "other paths stay");
        assert_eq!(without_home("C:/x", Some(&p("/"))), "C:/x", "a root home says nothing");
        assert_eq!(without_home("anything", None), "anything");
    }

    #[test]
    fn the_context_is_the_same_lines_in_the_same_order() {
        let cfg = [p("/home/someone/.config/demo/config.toml")];
        let rows = [("Shell", "pwsh".to_owned()), ("Empty", String::new())];
        let home = p("/home/someone");
        let c = context(&Context {
            last_error: Some("Copy: /home/someone/a.txt: denied"),
            home: Some(&home),
            rendering: Some(["GPU 1", "Vulkan", "DiscreteGpu"]),
            scale: Some(1.5),
            config: &cfg,
            anchors: &["demo"],
            rows: &rows,
        });
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            c,
            format!("Last error: Copy: ~/a.txt: denied\nRendering: GPU 1 (Vulkan, DiscreteGpu)\nScale: 150%\nShell: pwsh\nConfig: demo{sep}config.toml")
        );
        assert!(!c.contains("someone"));
        assert_eq!(context(&Context::default()), "Config: defaults only");
    }

    #[test]
    fn the_version_line_names_the_architecture() {
        let v = version_line("demo", "1.2.3");
        assert!(v.starts_with("demo 1.2.3 (") && v.contains(std::env::consts::ARCH), "{v}");
    }
}
