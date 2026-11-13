//! Listing a folder of this machine for a [`Picker`](crate::Picker) on another machine (feature `local`).
//!
//! The engine side of the exchange: the window forwards a [`Want::List`](crate::Want) path, the engine calls
//! [`list_local`] on a worker thread and sends the [`Listing`] back (feature `serde` gives JSON), and the window calls
//! `set_listing`. [`local_style`] and [`local_places`] tell the window how this machine writes paths and what to offer
//! in the places column.
//!
//! [`list_local_with`], [`check_local`] and [`local_places_with`] take [`ListOptions`] so an engine that serves a window on
//! another computer can keep that window from reaching yet another computer through the engine. The order of the checks
//! is: the lexical gate; (Windows) the absolute form, device names and bad characters; (Windows, drive letters) the drive
//! check; (Windows, UNC roots) the network check; the walk for links; then `read_dir`.
//!
//! # What the checks promise
//!
//! They keep a person in the window from reaching another computer **by name**: a UNC path, a drive letter mapped to a
//! share, or a link (symbolic link, junction, mounted folder, `subst` drive) that could lead there. They do not:
//! - stop a file system driver from fetching on its own (cloud files such as OneDrive placeholders, a network file system
//!   mounted on Unix, automounts);
//! - hold against someone who changes the folders while the checks run: each check looks at the path at that moment, and
//!   someone who can write in those folders can swap a folder for a link between two checks or between the checks and the
//!   read (on Windows only a symbolic link can lead to another computer, and making one needs the right to create symbolic
//!   links: administrators, or Developer Mode; junctions and mount points only lead to local volumes);
//! - say what the last item is (a device file on Unix, for example): the app checks the type when it opens it.
//!
//! They are a policy for the window, not a sandbox.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[cfg(any(windows, test))]
use crate::path::starts_ci;
#[cfg(windows)]
use crate::path::{has_bad_char, is_device_name};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::path::Gate;
use crate::{Entry, Kind, Listing, PathError, Place, PlaceKind, Style, Words};

/// How many items past `max` are read before a listing gives up (names that cannot be sent and failing reads count).
const READ_SLACK: usize = 1024;

/// The path style of the machine this program runs on (`Windows` on Windows, `Posix` elsewhere): what an engine tells
/// the window so its `Picker` reads paths the way this machine writes them.
pub fn local_style() -> Style {
    if cfg!(windows) {
        Style::Windows
    } else {
        Style::Posix
    }
}

/// Why [`list_local`], [`list_local_with`] or [`check_local`] could not list a folder.
#[derive(Debug)]
#[non_exhaustive]
pub enum ListError {
    /// Relative, or (Windows) `\dir`: both depend on the engine's current folder or drive, which the person never sees.
    NotAbsolute,
    /// A form that is refused: `C:foo` (`DriveRelative`), `\\?\...` `\\.\...` `\??\...` (`Verbatim`), `\\server`
    /// (`NoShare`), a NUL (`Nul`).
    Path(PathError),
    /// The OS could not read the folder (not found, no permission, not a folder, drive not ready, ...).
    Io(io::Error),
    /// (Windows) A name in the path is a device of Windows (`NUL`, `CON`, `COM1`, `LPT1`, ...), always refused.
    Device { name: String },
    /// (Windows) A name in the path has a character Windows does not allow in a name (`:` as in `file:stream`, `*`, `?`,
    /// `"`, `<`, `>`, `|`, or a control character), always refused.
    Name { name: String },
    /// `network` is off and the path is on another computer, or on a drive this PC cannot tell is local.
    Network,
    /// `links_in_path` is off and `at` (the folder itself, one above it, or a `subst` drive root such as `X:\`) is a link.
    /// `at` is in the form [`check_local`] returns.
    Link { at: String },
}

impl ListError {
    /// A sentence for the person in `lang` (Japanese when it starts with `ja`, in any case; anything else English).
    /// `Path` uses the crate's [`Words::path_error`]; `Io` uses `ito_common::ioerror::words` (no OS text);
    /// `NotAbsolute` names this machine's form (starting at a drive such as `C:\`, or at `/`).
    pub fn words(&self, lang: &str) -> String {
        let ja = lang.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("ja"));
        let (words, code) = if ja { (Words::ja(), "ja") } else { (Words::en(), "en") };
        match self {
            ListError::NotAbsolute => match (cfg!(windows), ja) {
                (true, true) => "ドライブから始まるパス（例: C:\\）で指定してください。".to_owned(),
                (true, false) => "Use a full path starting at a drive, such as C:\\.".to_owned(),
                (false, true) => "/ から始まるパスで指定してください。".to_owned(),
                (false, false) => "Use a full path starting at /.".to_owned(),
            },
            ListError::Path(p) => words.path_error(p),
            ListError::Io(e) => ito_common::ioerror::words(e, code),
            ListError::Device { name } if ja => format!("「{name}」は Windows の装置の名前で、ファイルやフォルダーの名前には使えません。"),
            ListError::Device { name } => format!("\"{name}\" is the name of a device of Windows, not of a file or folder."),
            ListError::Name { name } if ja => format!("「{name}」には、Windows の名前に使えない文字が入っています。"),
            ListError::Name { name } => format!("\"{name}\" has a character that Windows does not allow in a name."),
            ListError::Network if ja => {
                "このフォルダーは別のコンピューターにあるか、そうでないと確かめられません。ここからは開きません。".to_owned()
            }
            ListError::Network => {
                "This folder is on another computer, or this PC cannot tell that it is not. It is not opened from here.".to_owned()
            }
            ListError::Link { at } if ja => format!("{at} はリンクです。リンクの先のフォルダーは、ここからは開きません。"),
            ListError::Link { at } => format!("{at} is a link. Folders through links are not opened from here."),
        }
    }
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.words("en"))
    }
}

impl std::error::Error for ListError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ListError::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// How [`list_local_with`], [`check_local`] and [`local_places_with`] treat links, other computers and size.
///
/// `new` turns all three switches on, which is what [`list_local`] uses (the Windows device-name and bad-character refusals
/// are on with any options); [`strict`](ListOptions::strict) turns all three switches off, which is
/// what an engine serving a window on another computer should use. Links are the business of `follow_links` and
/// `links_in_path`: only with all three off is the listing kept from reaching another computer through a link. See the
/// module doc for what the checks do and do not promise.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ListOptions {
    /// Entries returned at most (same as `list_local`'s `max`). Not used by `check_local`.
    pub max: usize,
    /// `true` (today): a link entry's kind, size and modified time are the target's, so the target is opened (it may be on
    /// another computer). `false`: link entries are not followed (`link: true`, `size` and `modified` `None`; `kind` from
    /// the link's own attributes on Windows, `Unknown` elsewhere). Not used by `check_local`.
    pub follow_links: bool,
    /// `true` (today): the path may go through links. `false`: a path that is a link or lies under one, or starts at a
    /// `subst` drive, is refused with [`ListError::Link`].
    pub links_in_path: bool,
    /// `true` (today): UNC paths and network drive letters are read. `false`: a path that names another computer by itself
    /// (a UNC root, a drive letter mapped to a share, or a drive whose kind cannot be told) is refused with
    /// [`ListError::Network`] before anything is opened. Links are the other two options' business: only `strict` (all
    /// three off) keeps the listing from reaching another computer through a link. On Unix nothing is refused by `network`
    /// (a mounted network file system cannot be told from the path).
    pub network: bool,
}

impl ListOptions {
    /// All three on: follow links, allow links in the path, allow network paths (what [`list_local`] uses).
    pub fn new(max: usize) -> Self {
        ListOptions { max, follow_links: true, links_in_path: true, network: true }
    }
    /// All three off: what an engine serving a window on another computer should use.
    pub fn strict(max: usize) -> Self {
        ListOptions { max, follow_links: false, links_in_path: false, network: false }
    }
    /// Sets `follow_links`.
    pub fn follow_links(mut self, on: bool) -> Self {
        self.follow_links = on;
        self
    }
    /// Sets `links_in_path`.
    pub fn links_in_path(mut self, on: bool) -> Self {
        self.links_in_path = on;
        self
    }
    /// Sets `network`.
    pub fn network(mut self, on: bool) -> Self {
        self.network = on;
        self
    }
}

/// Lists one folder of this machine for a `Picker` on another machine, with all switches on
/// (`list_local_with(path, &ListOptions::new(max))`).
///
/// It **blocks** (disk, maybe a network share): call it from a worker thread.
///
/// - `path` must be absolute in this machine's style ([`local_style`]). The gate is lexical only and runs before the
///   disk is touched: a NUL, a relative path, `\dir` or `C:foo`, the verbatim and device forms and a UNC path with no
///   share are refused. `C:` alone is read as `C:\`.
/// - On Windows two refusals are always on, because they only refuse paths that are not plain folders: a name that is a
///   device of Windows (`C:\x\CON`, `NUL.txt`, `COM1`: [`ListError::Device`]) and a name with a character Windows does not
///   allow (`a:b`, `*`: [`ListError::Name`]). The folder that is read is the absolute form (`/` turned into `\`, `..` and
///   trailing dots and spaces dropped as Win32 does).
/// - **No containment and no authorization.** It follows links and lists any absolute path the process can read. The app
///   decides who may ask (koyomi, for example, only for an Admin). Use [`list_local_with`] to refuse links and other
///   computers.
/// - At most `max` entries are returned. `truncated` means the folder has more entries or the listing stopped before the
///   end: reading stops as soon as one more valid entry exists, or after `max + 1024` items of any kind (so a huge folder,
///   or a stream of failing reads, is not read to the end). `left_out` counts what was seen and left out: names that are
///   not valid Unicode, and reads that failed. They do not use up `max`.
/// - Each entry is the entry's own: `link` is a symlink or a junction, and then `kind`, `size` and `modified` are those of
///   the target (`Unknown` and `None` when it cannot be read). `size` is for files only. `modified` is Unix seconds.
///   `hidden` is the Windows hidden attribute (a dot name is the dialog's business).
pub fn list_local(path: &str, max: usize) -> Result<Listing, ListError> {
    list_local_with(path, &ListOptions::new(max))
}

/// [`list_local`] with [`ListOptions`]. It **blocks**: call it from a worker thread.
///
/// The checks run in this order, then the folder is read: (1) the lexical gate; (2) Windows: the absolute form
/// (`std::path::absolute`), device names ([`ListError::Device`]) and bad characters ([`ListError::Name`]), always;
/// (3) Windows, a drive letter: the drive's DOS device is read (`QueryDosDeviceW`, no file system) and a `subst` drive is
/// refused with `Link` when `links_in_path` is off, a network or unknown drive with `Network` when `network` is off (a
/// drive whose DOS device cannot be read: `Network` with `network` off, else an `Io` error with `links_in_path` off);
/// (4) Windows, a UNC root, with `network` off: `Network`; (5) with `links_in_path` off: each folder from the top down is
/// looked at with `symlink_metadata` and a link is refused with `Link { at }` (an error above the last item is an `Io`
/// error at once); (6) `read_dir`.
///
/// With `follow_links` off a link entry is not opened: `link: true`, `size` and `modified` `None`, and `kind` from the
/// link's own attributes on Windows (`Dir` for a directory link, else `File`) or `Unknown` on Unix. Other entries are
/// unchanged. See the module doc for what the checks promise.
pub fn list_local_with(path: &str, options: &ListOptions) -> Result<Listing, ListError> {
    let path = check(path, options)?;
    let dir = fs::read_dir(&path).map_err(ListError::Io)?;
    let follow = options.follow_links;
    Ok(collect(dir.map(|item| item.map(|de| entry_of(de, follow)).map_err(|_| ())), options.max))
}

/// The path checks of [`list_local_with`] without reading the folder, for an app that is about to open or write a picked
/// path. It uses `links_in_path` and `network` (not `max` or `follow_links`). The last item may be missing (a file about to
/// be made). With `links_in_path` off every folder above it must exist and pass the walk; with it on nothing on the disk is
/// looked at (only the names, and on Windows the drive). Success is true at the moment of the call only.
///
/// Returns the path to use: on Windows the `std::path::absolute` form (`/` turned into `\`, `..` and trailing dots and
/// spaces dropped); use that string, not the one passed in. It **blocks**: call it from a worker thread.
pub fn check_local(path: &str, options: &ListOptions) -> Result<String, ListError> {
    check(path, options)
}

fn check(path: &str, options: &ListOptions) -> Result<String, ListError> {
    let gated = gate(path)?;
    let path = text_checks(gated, options)?;
    if !options.links_in_path {
        walk_links(&path)?;
    }
    Ok(path)
}

fn gate(path: &str) -> Result<String, ListError> {
    local_style().gate_local(path).map_err(|g| match g {
        Gate::NotAbsolute => ListError::NotAbsolute,
        Gate::Path(p) => ListError::Path(p),
    })
}

/// Steps 2 to 4: nothing here opens a file. On Unix the path stays as the gate returned it.
#[cfg(not(windows))]
fn text_checks(gated: String, _: &ListOptions) -> Result<String, ListError> {
    Ok(gated)
}

#[cfg(windows)]
fn text_checks(gated: String, options: &ListOptions) -> Result<String, ListError> {
    let last = || gated.rsplit(['\\', '/']).find(|p| !p.is_empty()).unwrap_or(&gated).to_owned();
    let abs = std::path::absolute(&gated).map_err(ListError::Io)?;
    let abs = abs.into_os_string().into_string().map_err(|_| ListError::Io(io::Error::new(io::ErrorKind::InvalidData, "not Unicode")))?;
    if abs.starts_with("\\\\.\\") || abs.starts_with("\\\\?\\") {
        return Err(ListError::Device { name: last() });
    }
    let abs = gate(&abs)?;
    for part in Path::new(&abs).components() {
        if let Component::Normal(n) = part {
            let n = n.to_string_lossy();
            if is_device_name(&n) {
                return Err(ListError::Device { name: n.into_owned() });
            }
            if has_bad_char(&n) {
                return Err(ListError::Name { name: n.into_owned() });
            }
        }
    }
    let bytes = abs.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        if !options.network || !options.links_in_path {
            drive_check(char::from(bytes[0]).to_ascii_uppercase(), options)?;
        }
    } else if !options.network && abs.starts_with("\\\\") {
        return Err(ListError::Network);
    }
    Ok(abs)
}

/// Step 3 for one drive letter.
#[cfg(windows)]
fn drive_check(letter: char, options: &ListOptions) -> Result<(), ListError> {
    match query_dos_device(letter) {
        DosQuery::NoDrive => Err(ListError::Io(io::Error::new(io::ErrorKind::NotFound, "no such drive"))),
        // The kind cannot be told: allowed only when nothing is refused by kind.
        DosQuery::Failed if options.network && options.links_in_path => Ok(()),
        DosQuery::Failed if options.network => Err(ListError::Io(io::Error::other("the drive's kind cannot be read"))),
        DosQuery::Failed => Err(ListError::Network),
        DosQuery::Target(t) => {
            let (subst, class) = classify_drive(&t, &target_of);
            drive_decision(letter, subst, class, options)
        }
    }
}

#[cfg(windows)]
fn target_of(letter: char) -> Option<String> {
    match query_dos_device(letter) {
        DosQuery::Target(t) => Some(t),
        _ => None,
    }
}

#[cfg(windows)]
enum DosQuery {
    Target(String),
    NoDrive,
    Failed,
}

/// The first string `QueryDosDeviceW("X:")` gives: the object manager's name for the drive. No file system is touched.
#[cfg(windows)]
fn query_dos_device(letter: char) -> DosQuery {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::QueryDosDeviceW;
    let name: Vec<u16> = format!("{letter}:").encode_utf16().chain(Some(0)).collect();
    // Room for a `subst` target of the longest path Windows has (32 767 units) and its `\??\` prefix.
    let mut buf = vec![0u16; 32_832];
    // SAFETY: `name` is NUL-terminated and outlives the call; `buf` is a writable slice the call fills.
    let n = unsafe { QueryDosDeviceW(PCWSTR(name.as_ptr()), Some(buf.as_mut_slice())) };
    if n == 0 {
        return if io::Error::last_os_error().raw_os_error() == Some(2) { DosQuery::NoDrive } else { DosQuery::Failed };
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    DosQuery::Target(String::from_utf16_lossy(&buf[..len]))
}

/// What a drive letter is, by its DOS device target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(any(windows, test))]
enum DriveClass {
    Local,
    Network,
    Unknown,
}

/// The part after `\??\` or `\DosDevices\` when the target is a `subst` drive or a `DefineDosDevice` alias.
#[cfg(any(windows, test))]
fn subst_rest(target: &str) -> Option<&str> {
    ["\\??\\", "\\DosDevices\\"].iter().find(|p| starts_ci(target, p)).map(|p| &target[p.len()..])
}

/// `(is a subst alias, class)` of a drive whose DOS device target is `target`. `lookup` gives another drive's target (for
/// `subst X: Y:\dir`); a failed lookup, a loop or more than 8 hops is `Unknown`. Case-insensitive.
#[cfg(any(windows, test))]
fn classify_drive(target: &str, lookup: &dyn Fn(char) -> Option<String>) -> (bool, DriveClass) {
    (subst_rest(target).is_some(), class_of(target, lookup, 0))
}

#[cfg(any(windows, test))]
fn class_of(target: &str, lookup: &dyn Fn(char) -> Option<String>, hops: u32) -> DriveClass {
    if let Some(rest) = subst_rest(target) {
        if starts_ci(rest, "UNC\\") {
            return DriveClass::Network;
        }
        if starts_ci(rest, "Volume{") {
            return DriveClass::Local;
        }
        let b = rest.as_bytes();
        if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() && hops < 8 {
            return match lookup(char::from(b[0]).to_ascii_uppercase()) {
                Some(t) => class_of(&t, lookup, hops + 1),
                None => DriveClass::Unknown,
            };
        }
        return DriveClass::Unknown;
    }
    const NETWORK: [&str; 6] = [
        "\\Device\\Mup",
        "\\Device\\LanmanRedirector",
        "\\Device\\WebDavRedirector",
        "\\Device\\RdpDr",
        "\\Device\\NfsRdr",
        "\\Device\\CscDevice",
    ];
    const LOCAL: [&str; 6] =
        ["\\Device\\HarddiskVolume", "\\Device\\Harddisk", "\\Device\\CdRom", "\\Device\\Floppy", "\\Device\\ImDisk", "\\Device\\Volume"];
    if NETWORK.iter().any(|p| starts_ci(target, p)) || target.to_ascii_lowercase().contains("redirector") {
        DriveClass::Network
    } else if LOCAL.iter().any(|p| starts_ci(target, p)) {
        DriveClass::Local
    } else {
        DriveClass::Unknown
    }
}

/// Whether the options let a drive of this kind be read.
#[cfg(any(windows, test))]
fn drive_decision(letter: char, subst: bool, class: DriveClass, options: &ListOptions) -> Result<(), ListError> {
    if !options.links_in_path && subst {
        return Err(ListError::Link { at: format!("{letter}:\\") });
    }
    if !options.network && class != DriveClass::Local {
        return Err(ListError::Network);
    }
    Ok(())
}

/// Step 5: `symlink_metadata` on each prefix, top down. The root is not looked at. A link is refused as `Link { at }`; an
/// error above the last item fails at once; on the last item `NotFound` passes (`list_local_with` then gets its error from
/// `read_dir`; `check_local` takes a file about to be made).
fn walk_links(path: &str) -> Result<(), ListError> {
    let parts: Vec<Component<'_>> = Path::new(path).components().collect();
    let mut at = PathBuf::new();
    for (i, part) in parts.iter().enumerate() {
        at.push(part.as_os_str());
        if matches!(part, Component::Prefix(_) | Component::RootDir | Component::CurDir) {
            continue;
        }
        match fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => return Err(ListError::Link { at: at.to_string_lossy().into_owned() }),
            Ok(_) => {}
            Err(e) if i + 1 == parts.len() && e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(ListError::Io(e)),
        }
    }
    Ok(())
}

/// The cap and the read budget, over what the iterator yields: `Err` is a failed read, `Ok(None)` a name that cannot be
/// sent, `Ok(Some)` an entry. The iterator is pulled lazily, so reading stops when this does.
fn collect(items: impl Iterator<Item = Result<Option<Entry>, ()>>, max: usize) -> Listing {
    let budget = max.saturating_add(READ_SLACK);
    let mut listing = Listing::default();
    for (read, item) in items.enumerate() {
        if read >= budget {
            listing.truncated = true;
            break;
        }
        match item {
            Ok(Some(entry)) => {
                if listing.entries.len() >= max {
                    listing.truncated = true;
                    break;
                }
                listing.entries.push(entry);
            }
            Ok(None) | Err(()) => listing.left_out = listing.left_out.saturating_add(1),
        }
    }
    listing
}

fn entry_of(de: fs::DirEntry, follow_links: bool) -> Option<Entry> {
    let name = de.file_name().into_string().ok()?;
    let ft = de.file_type().ok();
    let own = de.metadata().ok();
    let link = ft.or_else(|| own.as_ref().map(|m| m.file_type())).is_some_and(|t| t.is_symlink());
    let hidden = own.as_ref().is_some_and(is_hidden);
    if link && !follow_links {
        // Never open the target: the entry's own attributes only.
        return Some(Entry { name, kind: link_kind(own.as_ref()), link, hidden, size: None, modified: None });
    }
    // What the kind, size and time are read from: the entry itself, or what a link points at.
    let source = if link { fs::metadata(de.path()).ok() } else { own };
    let kind_of = |t: fs::FileType| {
        if t.is_dir() {
            Kind::Dir
        } else if t.is_file() {
            Kind::File
        } else {
            Kind::Other
        }
    };
    let (kind, size, modified) = match source {
        Some(m) => {
            let kind = kind_of(m.file_type());
            (kind, (kind == Kind::File).then_some(m.len()), m.modified().ok().and_then(unix_secs))
        }
        // No metadata: a non-link keeps what its file type says; a link we could not follow is Unknown.
        None => (if link { None } else { ft.map(kind_of) }.unwrap_or(Kind::Unknown), None, None),
    };
    Some(Entry { name, kind, link, hidden, size, modified })
}

/// A link's kind from its own attributes (Windows: a directory link has the directory bit), `Unknown` elsewhere.
#[cfg(windows)]
fn link_kind(own: Option<&fs::Metadata>) -> Kind {
    use std::os::windows::fs::MetadataExt;
    match own {
        Some(m) if m.file_attributes() & 0x10 != 0 => Kind::Dir,
        Some(_) => Kind::File,
        None => Kind::Unknown,
    }
}

#[cfg(not(windows))]
fn link_kind(_: Option<&fs::Metadata>) -> Kind {
    Kind::Unknown
}

#[cfg(windows)]
fn is_hidden(m: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x2 != 0
}

#[cfg(not(windows))]
fn is_hidden(_: &fs::Metadata) -> bool {
    false
}

/// Unix seconds: whole seconds rounded down (toward minus infinity before 1970); `None` when it does not fit.
fn unix_secs(t: SystemTime) -> Option<i64> {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).ok(),
        Err(e) => {
            let d = e.duration();
            let secs = i64::try_from(d.as_secs()).ok()?.checked_add(i64::from(d.subsec_nanos() > 0))?;
            secs.checked_neg()
        }
    }
}

/// The places column for this machine, best effort. Windows: each assigned drive letter (`C:\`, label `C:`; the drives
/// are not touched, so an empty card reader or a lost network drive cannot stall it), then home. Elsewhere: `/`, then
/// home. Home's label is the last part of its path, its kind `Home`; it is left out when it is not known or not usable.
/// Labels are language-free; the app may rename them.
pub fn local_places() -> Vec<Place> {
    let mut places = system_places();
    places.extend(home_place(ito_common::home_dir()));
    places
}

/// [`local_places`] for an engine with these options. Windows: drive letters that the drive check of
/// [`list_local_with`] would refuse are left out (`network` off: network and unknown drives; `links_in_path` off: `subst`
/// drives, and drives whose kind cannot be read), and so is home when it is on such a drive or (with `network` off) a UNC
/// path. Only the DOS device names are read; no drive is opened. Home is checked by its name only, not walked: with
/// `links_in_path` off a home under a link (a symlinked `/home`, say) is still offered, and listing it gives `Link`.
/// Elsewhere, and with the default options, it equals `local_places()`.
pub fn local_places_with(options: &ListOptions) -> Vec<Place> {
    let mut places = system_places_with(options);
    let all = options.network && options.links_in_path;
    places.extend(home_place(ito_common::home_dir()).filter(|p| all || text_checks(p.path.clone(), options).is_ok()));
    places
}

#[cfg(windows)]
fn system_places_with(options: &ListOptions) -> Vec<Place> {
    if options.network && options.links_in_path {
        return system_places();
    }
    // SAFETY: GetLogicalDrives takes no arguments and only reads the drive bitmask.
    let mask = unsafe { windows::Win32::Storage::FileSystem::GetLogicalDrives() };
    drive_places_with(mask, options, |letter| match query_dos_device(letter) {
        DosQuery::Target(t) => classify_drive(&t, &target_of),
        // Gone or not readable: left out whenever the options refuse anything (as `drive_check` would).
        _ => (true, DriveClass::Unknown),
    })
}

#[cfg(not(windows))]
fn system_places_with(_: &ListOptions) -> Vec<Place> {
    system_places()
}

/// The drives of a `GetLogicalDrives` mask that `classify` (`(is subst, class)` per letter) and the options let through.
#[cfg(any(windows, test))]
fn drive_places_with(mask: u32, options: &ListOptions, classify: impl Fn(char) -> (bool, DriveClass)) -> Vec<Place> {
    drive_places(mask)
        .into_iter()
        .filter(|p| {
            let letter = p.path.chars().next().unwrap_or('?');
            let (subst, class) = classify(letter);
            drive_decision(letter, subst, class, options).is_ok()
        })
        .collect()
}

#[cfg(windows)]
fn system_places() -> Vec<Place> {
    // SAFETY: GetLogicalDrives takes no arguments and only reads the drive bitmask.
    let mask = unsafe { windows::Win32::Storage::FileSystem::GetLogicalDrives() };
    drive_places(mask)
}

#[cfg(not(windows))]
fn system_places() -> Vec<Place> {
    vec![Place::new("/", "/", PlaceKind::Root)]
}

/// One place per bit of a `GetLogicalDrives` mask (bit 0 is `A:`), in letter order.
#[cfg(any(windows, test))]
fn drive_places(mask: u32) -> Vec<Place> {
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| {
            let letter = char::from(b'A' + i);
            Place::new(&format!("{letter}:"), &format!("{letter}:\\"), PlaceKind::Drive)
        })
        .collect()
}

/// The home place for a home folder, if it is a usable one: absolute, valid Unicode, and passing the gate.
pub(crate) fn home_place(home: Option<PathBuf>) -> Option<Place> {
    let text = home?.into_os_string().into_string().ok()?;
    let style = local_style();
    let path = style.gate_local(&text).ok()?;
    let label = style.last_part(&path).unwrap_or_else(|| path.clone());
    Some(Place::new(&label, &path, PlaceKind::Home))
}

#[cfg(test)]
mod seam_tests {
    use super::*;

    fn e(n: &str) -> Result<Option<Entry>, ()> {
        Ok(Some(Entry::file(n)))
    }

    #[test]
    fn cap_looks_one_ahead() {
        let five = || (0..5).map(|i| e(&format!("f{i}")));
        let l = collect(five(), 3);
        assert_eq!((l.entries.len(), l.truncated), (3, true));
        let l = collect(five(), 5);
        assert_eq!((l.entries.len(), l.truncated), (5, false));
        let l = collect(five(), 6);
        assert_eq!((l.entries.len(), l.truncated), (5, false));
    }

    #[test]
    fn max_zero() {
        let l = collect((0..2).map(|i| e(&format!("f{i}"))), 0);
        assert_eq!((l.entries.len(), l.truncated), (0, true));
        let l = collect(std::iter::empty(), 0);
        assert_eq!((l.entries.len(), l.truncated), (0, false));
    }

    #[test]
    fn unsendable_names_do_not_use_up_max() {
        let items = vec![e("a"), Ok(None), e("b"), Err(()), Ok(None), e("c")];
        let l = collect(items.into_iter(), 3);
        assert_eq!((l.entries.len(), l.truncated, l.left_out), (3, false, 3));
        let items = vec![Ok(None), e("a"), Err(()), e("b"), e("c")];
        let l = collect(items.into_iter(), 2);
        assert_eq!((l.entries.len(), l.truncated, l.left_out), (2, true, 2));
    }

    #[test]
    fn budget_stops_a_stream_of_errors_and_of_bad_names() {
        let pulled = std::cell::Cell::new(0usize);
        let errs = std::iter::repeat_with(|| {
            pulled.set(pulled.get() + 1);
            Err(())
        });
        let l = collect(errs, 3);
        assert!(l.truncated && l.entries.is_empty());
        assert_eq!(l.left_out as usize, 3 + READ_SLACK);
        assert_eq!(pulled.get(), 3 + READ_SLACK + 1);
        let l = collect(std::iter::repeat(Ok(None)), 0);
        assert!(l.truncated);
        assert_eq!(l.left_out as usize, READ_SLACK);
    }

    #[test]
    fn exactly_the_budget_is_not_truncated() {
        let l = collect((0..READ_SLACK).map(|_| Err(())), 0);
        assert!(!l.truncated);
        assert_eq!(l.left_out as usize, READ_SLACK);
    }

    #[test]
    fn unix_secs_boundaries() {
        use std::time::Duration;
        assert_eq!(unix_secs(UNIX_EPOCH), Some(0));
        assert_eq!(unix_secs(UNIX_EPOCH + Duration::from_millis(999)), Some(0));
        assert_eq!(unix_secs(UNIX_EPOCH + Duration::from_millis(1500)), Some(1));
        assert_eq!(unix_secs(UNIX_EPOCH - Duration::from_millis(500)), Some(-1));
        assert_eq!(unix_secs(UNIX_EPOCH - Duration::from_secs(1)), Some(-1));
        assert_eq!(unix_secs(UNIX_EPOCH - Duration::from_millis(1001)), Some(-2));
    }

    #[test]
    fn drive_masks() {
        assert!(drive_places(0).is_empty());
        assert_eq!(drive_places(1 << 2), vec![Place::new("C:", "C:\\", PlaceKind::Drive)]);
        let acz = drive_places(1 | 1 << 2 | 1 << 25);
        let paths: Vec<_> = acz.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(paths, ["A:\\", "C:\\", "Z:\\"]);
        assert_eq!(acz[2].label, "Z:");
    }

    #[test]
    fn home_place_cases() {
        assert_eq!(home_place(None), None);
        assert_eq!(home_place(Some(PathBuf::new())), None);
        assert_eq!(home_place(Some(PathBuf::from("relative/home"))), None);
        #[cfg(windows)]
        {
            assert_eq!(
                home_place(Some(PathBuf::from("C:\\Users\\koyomi"))),
                Some(Place::new("koyomi", "C:\\Users\\koyomi", PlaceKind::Home))
            );
            assert_eq!(home_place(Some(PathBuf::from("C:\\"))), Some(Place::new("C:\\", "C:\\", PlaceKind::Home)));
            assert_eq!(home_place(Some(PathBuf::from("\\\\?\\C:\\Users\\x"))), None);
            assert_eq!(home_place(Some(PathBuf::from("C:x"))), None);
            use std::os::windows::ffi::OsStringExt;
            assert_eq!(home_place(Some(PathBuf::from(std::ffi::OsString::from_wide(&[0x43, 0x3A, 0x5C, 0xD800])))), None);
        }
        #[cfg(unix)]
        {
            assert_eq!(home_place(Some(PathBuf::from("/home/koyomi"))), Some(Place::new("koyomi", "/home/koyomi", PlaceKind::Home)));
            assert_eq!(home_place(Some(PathBuf::from("/"))), Some(Place::new("/", "/", PlaceKind::Home)));
            use std::os::unix::ffi::OsStringExt;
            assert_eq!(home_place(Some(PathBuf::from(std::ffi::OsString::from_vec(b"/home/\xff".to_vec())))), None);
        }
    }
}

#[cfg(test)]
mod rules_tests {
    use super::*;

    fn no_lookup(_: char) -> Option<String> {
        None
    }

    #[test]
    fn options_values_and_builders() {
        let n = ListOptions::new(7);
        assert_eq!((n.max, n.follow_links, n.links_in_path, n.network), (7, true, true, true));
        let s = ListOptions::strict(9);
        assert_eq!((s.max, s.follow_links, s.links_in_path, s.network), (9, false, false, false));
        assert_eq!(ListOptions::new(1).follow_links(false).links_in_path(false).network(false), ListOptions::strict(1));
        assert!(ListOptions::strict(1).network(true).network);
    }

    #[test]
    fn words_of_the_new_errors() {
        let d = ListError::Device { name: "NUL".into() };
        assert!(d.words("en").contains("\"NUL\"") && d.words("en").contains("device"));
        assert!(d.words("JA").contains("「NUL」"));
        let n = ListError::Name { name: "a:b".into() };
        assert!(n.words("en").contains("\"a:b\"") && n.words("ja-JP").contains("「a:b」"));
        assert!(ListError::Network.words("en").contains("another computer"));
        assert!(ListError::Network.words("ja").contains("別のコンピューター"));
        let l = ListError::Link { at: "X:\\".into() };
        assert!(l.words("en").starts_with("X:\\ is a link") && l.words("ja").starts_with("X:\\ はリンク"));
        assert_eq!(l.to_string(), l.words("en"));
    }

    #[test]
    fn classify_by_device_target() {
        let c = |t: &str| classify_drive(t, &no_lookup);
        assert_eq!(c("\\Device\\HarddiskVolume3"), (false, DriveClass::Local));
        assert_eq!(c("\\Device\\ImDisk0"), (false, DriveClass::Local));
        assert_eq!(c("\\Device\\Mup\\;Z:000\\srv\\s"), (false, DriveClass::Network));
        assert_eq!(c("\\Device\\LanmanRedirector\\;Z:0\\srv\\s"), (false, DriveClass::Network));
        assert_eq!(c("\\Device\\WebDavRedirector\\x"), (false, DriveClass::Network));
        assert_eq!(c("\\device\\mup\\x"), (false, DriveClass::Network));
        assert_eq!(c("\\Device\\Foo"), (false, DriveClass::Unknown));
        assert_eq!(c("\\??\\UNC\\srv\\s"), (true, DriveClass::Network));
        assert_eq!(c("\\??\\Volume{1234}\\x"), (true, DriveClass::Local));
        assert_eq!(c("\\DosDevices\\Volume{1234}\\x"), (true, DriveClass::Local));
        assert_eq!(c("\\??\\Foo"), (true, DriveClass::Unknown));
        // A subst of another drive follows that drive; a failed lookup is Unknown.
        let table = |l: char| match l {
            'C' => Some("\\Device\\HarddiskVolume3".to_owned()),
            'N' => Some("\\Device\\Mup\\x".to_owned()),
            'X' => Some("\\??\\Y:\\a".to_owned()),
            'Y' => Some("\\??\\X:\\b".to_owned()),
            _ => None,
        };
        assert_eq!(classify_drive("\\??\\C:\\base", &table), (true, DriveClass::Local));
        assert_eq!(classify_drive("\\??\\c:\\base", &table), (true, DriveClass::Local));
        assert_eq!(classify_drive("\\??\\N:\\base", &table), (true, DriveClass::Network));
        assert_eq!(classify_drive("\\??\\Q:\\base", &table), (true, DriveClass::Unknown));
        assert_eq!(classify_drive("\\??\\X:\\a", &table), (true, DriveClass::Unknown));
    }

    #[test]
    fn drive_decisions() {
        let strict = ListOptions::strict(1);
        let link = |l: &ListError| matches!(l, ListError::Link { at } if at == "X:\\");
        assert!(link(&drive_decision('X', true, DriveClass::Local, &strict).unwrap_err()));
        assert!(link(&drive_decision('X', true, DriveClass::Network, &strict).unwrap_err()));
        assert!(matches!(drive_decision('X', false, DriveClass::Unknown, &strict), Err(ListError::Network)));
        assert!(matches!(drive_decision('X', false, DriveClass::Network, &strict), Err(ListError::Network)));
        assert!(drive_decision('C', false, DriveClass::Local, &strict).is_ok());
        let new = ListOptions::new(1);
        assert!(drive_decision('X', true, DriveClass::Unknown, &new).is_ok());
        let no_net = ListOptions::new(1).network(false);
        assert!(drive_decision('X', true, DriveClass::Local, &no_net).is_ok());
        let no_links = ListOptions::new(1).links_in_path(false);
        assert!(drive_decision('X', false, DriveClass::Network, &no_links).is_ok());
    }

    #[test]
    fn drive_filter_through_the_mask() {
        // C local, N network, S subst of a local volume, U unknown.
        let classify = |l: char| match l {
            'C' => (false, DriveClass::Local),
            'N' => (false, DriveClass::Network),
            'S' => (true, DriveClass::Local),
            _ => (false, DriveClass::Unknown),
        };
        let m = ['C', 'N', 'S', 'U'].iter().fold(0u32, |m, l| m | 1 << (*l as u8 - b'A'));
        let letters = |o: &ListOptions| drive_places_with(m, o, classify).iter().map(|p| p.label.clone()).collect::<Vec<_>>();
        assert_eq!(letters(&ListOptions::new(1)), ["C:", "N:", "S:", "U:"]);
        assert_eq!(letters(&ListOptions::new(1).network(false)), ["C:", "S:"]);
        assert_eq!(letters(&ListOptions::new(1).links_in_path(false)), ["C:", "N:", "U:"]);
        assert_eq!(letters(&ListOptions::strict(1)), ["C:"]);
    }
}
