//! Plain words for a `std::io::Error`, in the window's language.
//!
//! The OS words its own errors in its own language and ends them with
//! "(os error N)". This turns an error into what a person acts on: `problem`
//! names it, `words` says it in short words to go inside an app's own sentence,
//! and `detail` keeps the OS's code and text for a log. The words carry no OS
//! text; that is `detail`'s job.

use std::io::{self, ErrorKind};

/// What went wrong, as a person acts on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Problem {
    /// The file or folder is not there.
    NotFound,
    /// No right to do it.
    NoPermission,
    /// Another program has it open or locked.
    InUse,
    AlreadyExists,
    /// No free space on the disk.
    DiskFull,
    /// Over the account's disk quota.
    QuotaExceeded,
    /// A read-only drive or folder.
    ReadOnly,
    NameTooLong,
    /// A name with characters that are not allowed.
    BadName,
    /// A folder that still has things in it.
    NotEmpty,
    /// A folder where a file was meant.
    IsFolder,
    /// A file where a folder was meant.
    NotFolder,
    /// A drive with no medium or not ready.
    NotReady,
    TooLarge,
    /// A move across drives that cannot be done.
    OtherDrive,
    Network,
    TimedOut,
    /// The contents cannot be read (not UTF-8, say).
    BadData,
    /// The file ends before it should.
    CutShort,
    Other,
}

impl Problem {
    /// Short words, with no final 「。」 or ".", and a lower-case start in English.
    /// `lang` is Japanese when it starts with `ja` (any case), else English.
    /// `Other` is the bare "unexpected error"; `words` adds the OS code to it.
    pub fn words(self, lang: &str) -> &'static str {
        let (ja, en) = match self {
            Problem::NotFound => ("見つかりません", "not found"),
            Problem::NoPermission => ("権限がありません", "no permission"),
            Problem::InUse => ("ほかのプログラムが使っています", "another program is using it"),
            Problem::AlreadyExists => ("すでにあります", "already exists"),
            Problem::DiskFull => ("ディスクの空きがありません", "the disk is full"),
            Problem::QuotaExceeded => ("使える容量の上限を超えました", "over the disk quota"),
            Problem::ReadOnly => ("書き込めない場所です", "the drive is read-only"),
            Problem::NameTooLong => ("名前かパスが長すぎます", "the name or path is too long"),
            Problem::BadName => ("名前に使えない文字があります", "the name has characters that are not allowed"),
            Problem::NotEmpty => ("フォルダーが空ではありません", "the folder is not empty"),
            Problem::IsFolder => ("ファイルではなくフォルダーです", "it is a folder, not a file"),
            Problem::NotFolder => ("フォルダーではありません", "it is not a folder"),
            Problem::NotReady => ("ドライブの準備ができていません", "the drive is not ready"),
            Problem::TooLarge => ("ファイルが大きすぎます", "the file is too large"),
            Problem::OtherDrive => ("別のドライブへは移せません", "cannot move it to another drive"),
            Problem::Network => ("ネットワークにつながりません", "cannot reach the network"),
            Problem::TimedOut => ("時間切れです", "timed out"),
            Problem::BadData => ("中身を読めません", "the contents cannot be read"),
            Problem::CutShort => ("途中で切れています", "it ends too early"),
            Problem::Other => ("予期しないエラーです", "unexpected error"),
        };
        if is_ja(lang) {
            ja
        } else {
            en
        }
    }
}

fn is_ja(lang: &str) -> bool {
    lang.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("ja"))
}

/// The OS whose raw codes `classify` reads. Apple covers macOS, iOS and the BSDs
/// (they share ENAMETOOLONG 63).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Os {
    Windows,
    Linux,
    Apple,
    OtherUnix,
}

impl Os {
    fn running() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(any(target_os = "linux", target_os = "android")) {
            Os::Linux
        } else if cfg!(any(
            target_vendor = "apple",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly"
        )) {
            Os::Apple
        } else {
            Os::OtherUnix
        }
    }
}

/// Raw codes first (they mean different things on each OS), then the kind.
fn classify(kind: ErrorKind, raw: Option<i32>, os: Os) -> Problem {
    if let Some(code) = raw {
        let by_code = match os {
            Os::Windows => match code {
                2 | 3 | 53 | 67 => Some(Problem::NotFound),
                5 | 65 => Some(Problem::NoPermission),
                19 => Some(Problem::ReadOnly),
                21 => Some(Problem::NotReady),
                32 | 33 => Some(Problem::InUse),
                39 | 112 => Some(Problem::DiskFull),
                59 | 64 => Some(Problem::Network),
                80 | 183 => Some(Problem::AlreadyExists),
                123 | 161 => Some(Problem::BadName),
                145 => Some(Problem::NotEmpty),
                206 => Some(Problem::NameTooLong),
                267 => Some(Problem::NotFolder),
                _ => None,
            },
            Os::Linux => (code == 36).then_some(Problem::NameTooLong),
            Os::Apple => (code == 63).then_some(Problem::NameTooLong),
            // Unknown numbering: the kind only.
            Os::OtherUnix => None,
        };
        if let Some(p) = by_code {
            return p;
        }
    }
    match kind {
        ErrorKind::NotFound => Problem::NotFound,
        ErrorKind::PermissionDenied => Problem::NoPermission,
        ErrorKind::ResourceBusy | ErrorKind::ExecutableFileBusy => Problem::InUse,
        ErrorKind::AlreadyExists => Problem::AlreadyExists,
        ErrorKind::StorageFull => Problem::DiskFull,
        ErrorKind::QuotaExceeded => Problem::QuotaExceeded,
        ErrorKind::ReadOnlyFilesystem => Problem::ReadOnly,
        ErrorKind::InvalidFilename => Problem::BadName,
        ErrorKind::DirectoryNotEmpty => Problem::NotEmpty,
        ErrorKind::IsADirectory => Problem::IsFolder,
        ErrorKind::NotADirectory => Problem::NotFolder,
        ErrorKind::FileTooLarge => Problem::TooLarge,
        ErrorKind::CrossesDevices => Problem::OtherDrive,
        ErrorKind::HostUnreachable
        | ErrorKind::NetworkUnreachable
        | ErrorKind::NetworkDown
        | ErrorKind::ConnectionRefused
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::NotConnected
        | ErrorKind::StaleNetworkFileHandle => Problem::Network,
        ErrorKind::TimedOut => Problem::TimedOut,
        ErrorKind::InvalidData => Problem::BadData,
        ErrorKind::UnexpectedEof => Problem::CutShort,
        _ => Problem::Other,
    }
}

/// The `io::Error` inside one made with `io::Error::new(kind, inner)`, when the
/// outer one has no OS code of its own.
fn wrapped(e: &io::Error) -> Option<&io::Error> {
    if e.raw_os_error().is_some() {
        return None;
    }
    e.get_ref().and_then(|i| i.downcast_ref::<io::Error>())
}

/// What went wrong. An error made with `io::Error::new(kind, inner)` where `inner`
/// is itself an `io::Error`, and that has no OS code of its own, is read through
/// to `inner` (one level).
pub fn problem(e: &io::Error) -> Problem {
    let e = wrapped(e).unwrap_or(e);
    classify(e.kind(), e.raw_os_error(), Os::running())
}

/// Short words for the error, to go inside an app's own sentence. `Other` adds the
/// OS code when there is one: 「予期しないエラーです（OS のコード 3）」.
pub fn words(e: &io::Error, lang: &str) -> String {
    let p = problem(e);
    if p != Problem::Other {
        return p.words(lang).to_string();
    }
    match (wrapped(e).unwrap_or(e).raw_os_error(), is_ja(lang)) {
        (Some(n), true) => format!("{}（OS のコード {n}）", p.words(lang)),
        (Some(n), false) => format!("{} (OS code {n})", p.words(lang)),
        (None, _) => p.words(lang).to_string(),
    }
}

/// The OS's code and text for a log: `os error 3: The system cannot find the path
/// specified.` For an error without an OS code: `NotFound: text`. A wrapped error
/// is read through one level, as `problem` does.
pub fn detail(e: &io::Error) -> String {
    let e = wrapped(e).unwrap_or(e);
    let text = e.to_string();
    match e.raw_os_error() {
        Some(n) => {
            let tail = format!(" (os error {n})");
            let text = text.strip_suffix(&tail).unwrap_or(&text);
            format!("os error {n}: {text}")
        }
        None => format!("{:?}: {text}", e.kind()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Os::*;
    use Problem::*;

    const OSES: [Os; 4] = [Windows, Linux, Apple, OtherUnix];

    #[test]
    fn windows_codes_are_read_by_code() {
        let rows = [
            (2, NotFound),
            (3, NotFound),
            (53, NotFound),
            (67, NotFound),
            (5, NoPermission),
            (65, NoPermission),
            (19, ReadOnly),
            (21, NotReady),
            (32, InUse),
            (33, InUse),
            (39, DiskFull),
            (112, DiskFull),
            (59, Network),
            (64, Network),
            (80, AlreadyExists),
            (183, AlreadyExists),
            (123, BadName),
            (161, BadName),
            (145, NotEmpty),
            (206, NameTooLong),
            (267, NotFolder),
        ];
        for (code, want) in rows {
            assert_eq!(classify(ErrorKind::Other, Some(code), Windows), want, "raw {code}");
        }
    }

    #[test]
    fn name_too_long_is_each_oss_own_number() {
        assert_eq!(classify(ErrorKind::InvalidFilename, Some(36), Linux), NameTooLong);
        assert_eq!(classify(ErrorKind::InvalidFilename, Some(63), Apple), NameTooLong);
        assert_eq!(classify(ErrorKind::InvalidFilename, Some(206), Windows), NameTooLong);
        // The traps: another OS's number is not it.
        assert_ne!(classify(ErrorKind::Other, Some(63), Linux), NameTooLong);
        assert_ne!(classify(ErrorKind::Other, Some(36), Apple), NameTooLong);
        assert_ne!(classify(ErrorKind::Other, Some(36), Windows), NameTooLong);
        assert_ne!(classify(ErrorKind::Other, Some(63), OtherUnix), NameTooLong);
        // Left to the kind, it is a bad name.
        assert_eq!(classify(ErrorKind::InvalidFilename, Some(63), Linux), BadName);
    }

    #[test]
    fn a_windows_sharing_number_is_nothing_on_unix() {
        for os in [Linux, Apple, OtherUnix] {
            assert_eq!(classify(ErrorKind::Other, Some(32), os), Other);
            assert_eq!(classify(ErrorKind::Other, Some(33), os), Other);
            assert_eq!(classify(ErrorKind::BrokenPipe, Some(32), os), Other);
        }
    }

    #[test]
    fn every_kind_row_holds_on_every_os() {
        let rows = [
            (ErrorKind::NotFound, NotFound),
            (ErrorKind::PermissionDenied, NoPermission),
            (ErrorKind::ResourceBusy, InUse),
            (ErrorKind::ExecutableFileBusy, InUse),
            (ErrorKind::AlreadyExists, AlreadyExists),
            (ErrorKind::StorageFull, DiskFull),
            (ErrorKind::QuotaExceeded, QuotaExceeded),
            (ErrorKind::ReadOnlyFilesystem, ReadOnly),
            (ErrorKind::InvalidFilename, BadName),
            (ErrorKind::DirectoryNotEmpty, NotEmpty),
            (ErrorKind::IsADirectory, IsFolder),
            (ErrorKind::NotADirectory, NotFolder),
            (ErrorKind::FileTooLarge, TooLarge),
            (ErrorKind::CrossesDevices, OtherDrive),
            (ErrorKind::HostUnreachable, Network),
            (ErrorKind::NetworkUnreachable, Network),
            (ErrorKind::NetworkDown, Network),
            (ErrorKind::ConnectionRefused, Network),
            (ErrorKind::ConnectionReset, Network),
            (ErrorKind::ConnectionAborted, Network),
            (ErrorKind::NotConnected, Network),
            (ErrorKind::StaleNetworkFileHandle, Network),
            (ErrorKind::TimedOut, TimedOut),
            (ErrorKind::InvalidData, BadData),
            (ErrorKind::UnexpectedEof, CutShort),
            (ErrorKind::Other, Other),
        ];
        for os in OSES {
            for (kind, want) in rows {
                assert_eq!(classify(kind, None, os), want, "{kind:?} on {os:?}");
            }
        }
    }

    #[test]
    fn real_errors_made_here() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.txt");
        assert_eq!(problem(&std::fs::File::open(&missing).unwrap_err()), NotFound);

        let there = dir.path().join("there.txt");
        std::fs::write(&there, "x").unwrap();
        let again = std::fs::OpenOptions::new().write(true).create_new(true).open(&there).unwrap_err();
        assert_eq!(problem(&again), AlreadyExists);

        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("f"), "x").unwrap();
        assert_eq!(problem(&std::fs::remove_dir(&sub).unwrap_err()), NotEmpty);
    }

    #[cfg(windows)]
    #[test]
    fn windows_real_errors() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("held.txt");
        let _held = std::fs::OpenOptions::new().write(true).create(true).truncate(true).share_mode(0).open(&path).unwrap();
        let err = std::fs::OpenOptions::new().read(true).open(&path).unwrap_err();
        assert_eq!(problem(&err), InUse);
        assert_eq!(problem(&io::Error::from_raw_os_error(206)), NameTooLong);
    }

    #[test]
    fn a_wrapped_error_is_read_through_one_level() {
        let inner = io::Error::new(ErrorKind::NotFound, "gone");
        let outer = io::Error::other(inner);
        assert_eq!(problem(&outer), NotFound);
        // A plain payload is not an io::Error: the outer kind stands.
        assert_eq!(problem(&io::Error::new(ErrorKind::TimedOut, "slow")), TimedOut);
    }

    #[test]
    fn words_in_two_languages() {
        let nf = io::Error::from(ErrorKind::NotFound);
        assert_eq!(words(&nf, "ja"), "見つかりません");
        assert_eq!(words(&nf, "ja-JP"), "見つかりません");
        assert_eq!(words(&nf, "JA"), "見つかりません");
        assert_eq!(words(&nf, "en"), "not found");
        assert_eq!(words(&nf, ""), "not found");
        assert_eq!(words(&nf, "fr"), "not found");
        let full = io::Error::from(ErrorKind::StorageFull);
        assert_eq!(words(&full, "ja"), "ディスクの空きがありません");
        assert_eq!(words(&full, "en"), "the disk is full");
        assert_eq!(Problem::InUse.words("en"), "another program is using it");
    }

    #[test]
    fn no_word_ends_in_a_full_stop() {
        let all = [
            NotFound,
            NoPermission,
            InUse,
            AlreadyExists,
            DiskFull,
            QuotaExceeded,
            ReadOnly,
            NameTooLong,
            BadName,
            NotEmpty,
            IsFolder,
            NotFolder,
            NotReady,
            TooLarge,
            OtherDrive,
            Network,
            TimedOut,
            BadData,
            CutShort,
            Other,
        ];
        for p in all {
            for lang in ["ja", "en"] {
                let w = p.words(lang);
                assert!(!w.is_empty() && !w.ends_with('。') && !w.ends_with('.'), "{p:?} {lang}: {w}");
            }
        }
    }

    #[test]
    fn other_carries_the_os_code_when_there_is_one() {
        let odd = io::Error::other("x");
        assert_eq!(words(&odd, "ja"), "予期しないエラーです");
        assert_eq!(words(&odd, "en"), "unexpected error");
        // An unlikely number: Other on every OS.
        let coded = io::Error::from_raw_os_error(9999);
        assert_eq!(problem(&coded), Other);
        assert_eq!(words(&coded, "ja"), "予期しないエラーです（OS のコード 9999）");
        assert_eq!(words(&coded, "en"), "unexpected error (OS code 9999)");
    }

    #[test]
    fn detail_keeps_the_os_code_and_text() {
        let d = detail(&io::Error::from_raw_os_error(3));
        assert!(d.starts_with("os error 3: "), "{d}");
        assert!(!d.ends_with("(os error 3)"), "{d}");
        assert_eq!(detail(&io::Error::new(ErrorKind::InvalidData, "x")), "InvalidData: x");
        let wrapped = io::Error::other(io::Error::new(ErrorKind::NotFound, "gone"));
        assert_eq!(detail(&wrapped), "NotFound: gone");
        let wrapped_os = io::Error::other(io::Error::from_raw_os_error(3));
        assert!(detail(&wrapped_os).starts_with("os error 3: "), "{}", detail(&wrapped_os));
    }
}
