use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use ito_common::ioerror;

/// What was being done when an operation failed.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Doing {
    List,
    Read,
    Delete,
    Copy,
    MakeFolder,
    Rename,
    Hash,
    SetTime,
    SetAttributes,
    Sync,
    Cleanup,
}

impl Doing {
    fn en(self) -> &'static str {
        match self {
            Doing::List => "list",
            Doing::Read => "read",
            Doing::Delete => "delete",
            Doing::Copy => "copy",
            Doing::MakeFolder => "make the folder",
            Doing::Rename => "rename",
            Doing::Hash => "read back",
            Doing::SetTime => "set the time of",
            Doing::SetAttributes => "set the attributes of",
            Doing::Sync => "flush",
            Doing::Cleanup => "clean up",
        }
    }

    fn ja(self) -> &'static str {
        match self {
            Doing::List => "の中身を調べられません",
            Doing::Read => "を読めません",
            Doing::Delete => "を消せません",
            Doing::Copy => "にコピーできません",
            Doing::MakeFolder => "のフォルダーを作れません",
            Doing::Rename => "の名前を替えられません",
            Doing::Hash => "を読み戻せません",
            Doing::SetTime => "の日時を設定できません",
            Doing::SetAttributes => "の属性を設定できません",
            Doing::Sync => "を書き出し切れません",
            Doing::Cleanup => "の後片付けができません",
        }
    }
}

/// Why a path was refused before anything there was touched.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsafe {
    /// An empty path.
    Empty,
    /// A file system root or a share root, where a folder was to be emptied.
    Root,
    /// A symlink or junction where a real folder or file was needed.
    Link,
    /// The two roots of a copy are the same folder or one is inside the other.
    Overlap,
    /// The source and the destination are the same file.
    SameFile,
}

impl Unsafe {
    fn en(self) -> &'static str {
        match self {
            Unsafe::Empty => "the path is empty",
            Unsafe::Root => "it is the top of a drive or share",
            Unsafe::Link => "it is a link",
            Unsafe::Overlap => "the source and destination folders overlap",
            Unsafe::SameFile => "source and destination are the same file",
        }
    }

    fn ja(self) -> &'static str {
        match self {
            Unsafe::Empty => "パスが空です",
            Unsafe::Root => "ドライブや共有の一番上です",
            Unsafe::Link => "リンクです",
            Unsafe::Overlap => "コピー元とコピー先のフォルダーが重なっています",
            Unsafe::SameFile => "コピー元とコピー先が同じファイルです",
        }
    }
}

/// Why an operation could not go on.
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// The OS refused something at `path`.
    Io { path: PathBuf, doing: Doing, source: io::Error },
    /// `verify`: the hash of the source and of the staged copy differ (both SHA-256, hex).
    Mismatch { path: PathBuf, from: String, to: String },
    /// `verify`: the source changed (size or write time) while it was being copied.
    SourceChanged { path: PathBuf },
    /// Refused before touching anything at `path`.
    Unsafe { path: PathBuf, why: Unsafe },
}

pub(crate) fn io_err(path: &Path, doing: Doing, source: io::Error) -> Error {
    Error::Io { path: path.to_path_buf(), doing, source }
}

pub(crate) fn unsafe_err(path: &Path, why: Unsafe) -> Error {
    Error::Unsafe { path: path.to_path_buf(), why }
}

impl Error {
    /// The path the error is about.
    pub fn path(&self) -> &Path {
        match self {
            Error::Io { path, .. } | Error::Mismatch { path, .. } | Error::SourceChanged { path } | Error::Unsafe { path, .. } => path,
        }
    }

    /// A sentence for the person, in `lang` (`ja`; anything else is English). OS errors use
    /// `ito_common::ioerror::words`, with no OS text.
    pub fn words(&self, lang: &str) -> String {
        let ja = lang.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("ja"));
        let p = self.path().display();
        match (self, ja) {
            (Error::Io { doing, source, .. }, true) => format!("{p} {}（{}）", doing.ja(), ioerror::words(source, lang)),
            (Error::Io { doing, source, .. }, false) => format!("cannot {} {p} ({})", doing.en(), ioerror::words(source, lang)),
            (Error::Mismatch { .. }, true) => format!("{p} のコピーがコピー元と一致しません（SHA-256 が違います）"),
            (Error::Mismatch { .. }, false) => format!("the copy of {p} does not match the source (SHA-256 differs)"),
            (Error::SourceChanged { .. }, true) => format!("コピー中に {p} が変わりました"),
            (Error::SourceChanged { .. }, false) => format!("{p} changed while it was being copied"),
            (Error::Unsafe { why, .. }, true) => format!("{p} は扱えません（{}）", why.ja()),
            (Error::Unsafe { why, .. }, false) => format!("refusing {p} ({})", why.en()),
        }
    }
}

impl fmt::Display for Error {
    /// English, with the OS code and text for a log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = self.path().display();
        match self {
            Error::Io { doing, source, .. } => write!(f, "cannot {} {p}: {}", doing.en(), ioerror::detail(source)),
            Error::Mismatch { from, to, .. } => {
                write!(f, "the copy of {p} does not match the source: source SHA-256 {from}, copy SHA-256 {to}")
            }
            Error::SourceChanged { .. } => write!(f, "{p} changed while it was being copied"),
            Error::Unsafe { why, .. } => write!(f, "refusing {p}: {}", why.en()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
