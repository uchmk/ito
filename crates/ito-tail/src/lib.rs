//! Follow the new lines of a log file, the way `tail -F` does, for an app that reacts to a line
//! (`ERROR` appearing in a log) without leaving a handle on the file.
//!
//! [`Follower::poll`] opens the file, reads what was added since the last call and closes it again, so
//! nothing stays locked: on Windows it is opened with `FILE_SHARE_READ | WRITE | DELETE`, and the writer
//! can rotate or delete the file at any time. Only lines that end in a newline are returned; a line the
//! writer is still in the middle of waits for the next call.
//!
//! A file that is smaller than where the last call stopped was truncated, and one that is a different
//! file under the same name (rotated: renamed away and started again) is a new one; both are read from
//! the top. (A file truncated and grown past the old position between two calls cannot be told apart
//! from an append, so poll often enough.) A line is UTF-8 when it is valid UTF-8, else Shift_JIS, so a
//! Japanese business application's log reads whichever it writes.
//!
//! The app keeps [`Follower::offset`] with its state and passes it back as [`From::Offset`] after a
//! restart, so old lines are not delivered twice and lines written while the app was down are not lost.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A line that has no newline after this many bytes is returned as it is, so a file without newlines
/// does not fill the memory.
const LONGEST_LINE: usize = 1 << 20;
/// At most this many bytes are read per [`Follower::poll`]; the rest comes on the next call.
const PER_POLL: u64 = 8 << 20;

/// Where to begin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum From {
    /// The end of the file as it is now: only lines written from here on. (A file that does not exist yet
    /// is read from its first line once it appears.)
    End,
    /// The first line.
    Start,
    /// A position the app saved from [`Follower::offset`]. A file shorter than that starts over.
    Offset(u64),
}

/// Which file is under the name: told apart so a rotated log is read from the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Id(u64, u64);

#[derive(Debug)]
pub struct Follower {
    path: PathBuf,
    /// The first byte not yet turned into a line (`pending` begins here).
    offset: u64,
    /// The bytes after `offset` that had no newline yet.
    pending: Vec<u8>,
    id: Option<Id>,
}

impl Follower {
    /// Follow `path`. It need not exist yet.
    pub fn new(path: impl Into<PathBuf>, from: From) -> Follower {
        let path = path.into();
        let (mut offset, mut id) = (0, None);
        match from {
            From::Start => {}
            From::Offset(n) => offset = n,
            From::End => {
                if let Ok(f) = open(&path) {
                    if let Ok(m) = f.metadata() {
                        offset = m.len();
                        id = identify(&m);
                    }
                }
            }
        }
        Follower { path, offset, pending: Vec::new(), id }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where the next unread line begins (a half-written line is not counted, so a restart from this
    /// reads it whole). Save it and give it back as [`From::Offset`].
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// The lines added since the last call, without their line ends. A file that is not there yet is no
    /// lines, not an error. Do not call this from the UI thread of a slow (network) drive.
    pub fn poll(&mut self) -> io::Result<Vec<String>> {
        let mut f = match open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Gone (between a rename and the new file): the next one is read from its top.
                self.restart();
                return Ok(Vec::new());
            }
            Err(e) => return Err(e),
        };
        let meta = f.metadata()?;
        let id = identify(&meta);
        let len = meta.len();
        if (self.id.is_some() && id.is_some() && self.id != id) || len < self.offset + self.pending.len() as u64 {
            self.restart();
        }
        self.id = id;
        let from = self.offset + self.pending.len() as u64;
        if len <= from {
            return Ok(Vec::new());
        }
        f.seek(SeekFrom::Start(from))?;
        let mut buf = Vec::new();
        f.take((len - from).min(PER_POLL)).read_to_end(&mut buf)?;
        self.pending.extend_from_slice(&buf);

        let mut lines = Vec::new();
        let mut start = 0;
        while let Some(i) = self.pending[start..].iter().position(|&b| b == b'\n') {
            let end = start + i;
            lines.push(decode(&self.pending[start..end], self.offset == 0 && start == 0));
            start = end + 1;
        }
        if self.pending.len() - start > LONGEST_LINE {
            lines.push(decode(&self.pending[start..], false));
            start = self.pending.len();
        }
        self.offset += start as u64;
        self.pending.drain(..start);
        Ok(lines)
    }

    fn restart(&mut self) {
        self.offset = 0;
        self.pending.clear();
        self.id = None;
    }
}

fn open(path: &Path) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE: never in the writer's way.
        o.share_mode(7);
    }
    o.open(path)
}

#[cfg(unix)]
fn identify(m: &std::fs::Metadata) -> Option<Id> {
    use std::os::unix::fs::MetadataExt;
    Some(Id(m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn identify(m: &std::fs::Metadata) -> Option<Id> {
    let t = m.created().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(Id(t.as_secs(), t.subsec_nanos() as u64))
}

fn decode(line: &[u8], first: bool) -> String {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let text = match std::str::from_utf8(line) {
        Ok(s) => s.to_owned(),
        Err(_) => encoding_rs::SHIFT_JIS.decode_without_bom_handling(line).0.into_owned(),
    };
    if first {
        text.strip_prefix('\u{feff}').map(str::to_owned).unwrap_or(text)
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Dir {
            static N: AtomicU32 = AtomicU32::new(0);
            let d = std::env::temp_dir().join(format!("ito-tail-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Dir(d)
        }

        fn file(&self) -> PathBuf {
            self.0.join("app.log")
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn append(p: &Path, bytes: &[u8]) {
        OpenOptions::new().create(true).append(true).open(p).unwrap().write_all(bytes).unwrap();
    }

    fn lines(f: &mut Follower) -> Vec<String> {
        f.poll().unwrap()
    }

    #[test]
    fn starting_from_the_end_skips_what_is_there() {
        let d = Dir::new();
        append(&d.file(), b"old 1\nold 2\n");
        let mut f = Follower::new(d.file(), From::End);
        assert!(lines(&mut f).is_empty());
        append(&d.file(), b"new 1\r\nnew 2\n");
        assert_eq!(lines(&mut f), ["new 1", "new 2"]);
        assert!(lines(&mut f).is_empty());
    }

    #[test]
    fn a_half_written_line_waits_for_its_newline() {
        let d = Dir::new();
        let mut f = Follower::new(d.file(), From::Start);
        append(&d.file(), b"one\ntw");
        assert_eq!(lines(&mut f), ["one"]);
        assert_eq!(f.offset(), 4);
        append(&d.file(), b"o\n");
        assert_eq!(lines(&mut f), ["two"]);
        assert_eq!(f.offset(), 8);
    }

    #[test]
    fn a_saved_offset_resumes_without_repeating_or_losing() {
        let d = Dir::new();
        append(&d.file(), b"a\nb\n");
        let mut f = Follower::new(d.file(), From::Start);
        assert_eq!(lines(&mut f), ["a", "b"]);
        let saved = f.offset();
        append(&d.file(), b"c\n");
        let mut g = Follower::new(d.file(), From::Offset(saved));
        assert_eq!(lines(&mut g), ["c"]);
    }

    #[test]
    fn a_truncated_file_is_read_from_the_top() {
        let d = Dir::new();
        append(&d.file(), b"a long first line\nsecond\n");
        let mut f = Follower::new(d.file(), From::Start);
        assert_eq!(lines(&mut f).len(), 2);
        std::fs::write(d.file(), b"x\n").unwrap();
        assert_eq!(lines(&mut f), ["x"]);
        // An offset past the end of a shorter file starts over too.
        let mut g = Follower::new(d.file(), From::Offset(9999));
        assert_eq!(lines(&mut g), ["x"]);
    }

    #[test]
    fn a_rotated_file_is_read_from_the_top_and_a_missing_one_is_quiet() {
        let d = Dir::new();
        append(&d.file(), b"first generation, longer than the next\n");
        let mut f = Follower::new(d.file(), From::Start);
        assert_eq!(lines(&mut f).len(), 1);
        std::fs::rename(d.file(), d.0.join("app.log.1")).unwrap();
        assert!(lines(&mut f).is_empty());
        append(&d.file(), b"new\n");
        assert_eq!(lines(&mut f), ["new"]);
    }

    #[test]
    fn utf8_and_shift_jis_both_read() {
        let d = Dir::new();
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("エラー: 失敗しました");
        let mut bytes = "\u{feff}日本語 ok\n".as_bytes().to_vec();
        bytes.extend_from_slice(&sjis);
        bytes.push(b'\n');
        append(&d.file(), &bytes);
        let mut f = Follower::new(d.file(), From::Start);
        assert_eq!(lines(&mut f), ["日本語 ok", "エラー: 失敗しました"]);
    }

    #[test]
    fn a_file_that_is_not_there_yet_is_no_lines_and_then_all_of_them() {
        let d = Dir::new();
        let mut f = Follower::new(d.file(), From::End);
        assert!(lines(&mut f).is_empty());
        append(&d.file(), b"hello\n");
        assert_eq!(lines(&mut f), ["hello"]);
    }

    #[test]
    fn the_writer_can_delete_the_file_while_we_follow() {
        let d = Dir::new();
        append(&d.file(), b"a\n");
        let mut f = Follower::new(d.file(), From::Start);
        assert_eq!(lines(&mut f), ["a"]);
        std::fs::remove_file(d.file()).unwrap();
        assert!(lines(&mut f).is_empty());
    }
}
