//! Read a big text file by line number without loading it.
//!
//! [`LineIndex::open`] starts a thread that reads the file once from the front, 64 KiB at a time, and records where
//! every line starts (about 8 bytes a line). [`LineIndex::lines`] then reads any run of lines by seeking to it. No
//! memory map: a file on a network drive that is cut short, or one another program holds open, fails a read with an
//! error here and not the whole process.
//!
//! Not here: editing, syntax colouring, searching the content.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// How much of the file one read takes when the index is built.
const CHUNK_BYTES: usize = 64 * 1024;
/// A line is cut to this many bytes when read.
pub const MAX_LINE_BYTES: usize = 8 * 1024;
/// How much of a line is read from the disk: room for multi-byte text to still fill [`MAX_LINE_BYTES`] once decoded.
const READ_LINE_BYTES: u64 = 4 * MAX_LINE_BYTES as u64;
/// Lines asked for in one go are read with one read when they take no more than this.
const BULK_READ_BYTES: u64 = 4 * 1024 * 1024;
/// How many bytes before the end of what is indexed are remembered, to tell a file that was cut and written again
/// from one that only grew.
const TAIL_BYTES: u64 = 32;
/// How much of the head of a file is looked at to tell its encoding.
const SNIFF_BYTES: usize = 64 * 1024;

/// How a file's bytes are text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Enc {
    Utf8,
    Utf16Le,
    Utf16Be,
    /// A byte-oriented set that keeps the newline as a byte of its own (Shift_JIS, EUC-JP).
    Legacy(&'static encoding_rs::Encoding),
}

/// The encoding of a file from its first bytes, and how many bytes of byte-order mark to skip. A mark says it; UTF-16
/// without one shows as NULs on every second byte; text that is not valid UTF-8 is tried as EUC-JP, then Shift_JIS
/// (EUC-JP first, because Shift_JIS reads most EUC-JP as half-width kana). Anything else is UTF-8.
pub fn guess(head: &[u8]) -> (Enc, u64) {
    match head {
        [0xff, 0xfe, ..] => return (Enc::Utf16Le, 2),
        [0xfe, 0xff, ..] => return (Enc::Utf16Be, 2),
        [0xef, 0xbb, 0xbf, ..] => return (Enc::Utf8, 3),
        _ => {}
    }
    let probe = &head[..head.len().min(4096)];
    let pairs = probe.len() / 2;
    if pairs >= 16 {
        let (mut even, mut odd) = (0, 0);
        for c in probe.as_chunks::<2>().0 {
            even += (c[0] == 0) as usize;
            odd += (c[1] == 0) as usize;
        }
        if odd * 3 > pairs && even * 20 < pairs {
            return (Enc::Utf16Le, 0);
        }
        if even * 3 > pairs && odd * 20 < pairs {
            return (Enc::Utf16Be, 0);
        }
    }
    // A head cut in the middle of a character is not an error.
    let whole = if head.len() >= SNIFF_BYTES { &head[..head.len() - 3] } else { head };
    match std::str::from_utf8(whole) {
        Ok(_) => return (Enc::Utf8, 0),
        Err(e) if e.error_len().is_none() => return (Enc::Utf8, 0),
        _ => {}
    }
    // Cut in the middle of a two- or three-byte character: up to two bytes of the end may be left off.
    let cuts = if head.len() >= SNIFF_BYTES { 0..=2 } else { 0..=0 };
    for enc in [encoding_rs::EUC_JP, encoding_rs::SHIFT_JIS] {
        for cut in cuts.clone() {
            if enc.decode_without_bom_handling_and_without_replacement(&head[..head.len() - cut]).is_some() {
                return (Enc::Legacy(enc), 0);
            }
        }
    }
    (Enc::Utf8, 0)
}

/// What [`LineIndex::grow`] found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grow {
    /// Nothing new (or the index is still being built).
    Same,
    /// The file grew; the new part is being indexed on a thread.
    Grew,
    /// The file is shorter or its end changed: it was cut or rewritten. Open a new index.
    Rebuild,
}

/// One line as read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Line {
    /// The text, without the line end, cut to [`MAX_LINE_BYTES`].
    pub text: String,
    /// The line is longer than what `text` holds.
    pub cut: bool,
}

#[derive(Default)]
struct Inner {
    /// Where each line starts. While the index is building the last entry may be the end of what has been read so far.
    starts: Vec<u64>,
    /// How far into the file the build has read.
    end: u64,
    /// The file ended in a newline, so the start that followed it was dropped.
    nl_end: bool,
    /// The last bytes before `end` when the build last finished.
    tail: Vec<u8>,
}

/// A line index of one file. Share it as `Arc<LineIndex>`.
pub struct LineIndex {
    path: PathBuf,
    enc: Enc,
    /// The file's length when the build last began.
    len: AtomicU64,
    inner: Mutex<Inner>,
    done: AtomicBool,
    cancel: AtomicBool,
}

impl LineIndex {
    /// Begin indexing `path` on a thread of its own and return at once.
    pub fn open(path: &Path) -> std::io::Result<Arc<Self>> {
        let mut f = File::open(path)?;
        let len = f.metadata()?.len();
        let mut head = Vec::with_capacity(SNIFF_BYTES);
        (&mut f).take(SNIFF_BYTES as u64).read_to_end(&mut head)?;
        let (enc, bom) = guess(&head);
        let idx = Arc::new(Self {
            path: path.to_owned(),
            enc,
            len: AtomicU64::new(len),
            inner: Mutex::new(Inner { starts: vec![bom], end: bom, nl_end: false, tail: Vec::new() }),
            done: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        });
        let me = idx.clone();
        std::thread::Builder::new().name("line-index".into()).spawn(move || me.build(f, bom))?;
        Ok(idx)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn encoding(&self) -> Enc {
        self.enc
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn build(&self, mut f: File, from: u64) {
        let mut buf = vec![0u8; CHUNK_BYTES];
        let mut pos = from;
        if f.seek(SeekFrom::Start(from)).is_err() {
            self.finish(pos);
            return;
        }
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return;
            }
            let mut got = 0;
            while got < buf.len() {
                match f.read(&mut buf[got..]) {
                    Ok(0) => break,
                    Ok(n) => got += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let full = got == buf.len();
            // A UTF-16 file cut in the middle of a unit (a writer caught half way): leave the odd byte for the next
            // read, or every pair after it would be read one byte off.
            if self.wide() {
                got &= !1;
            }
            if got == 0 {
                break;
            }
            let mut found = Vec::new();
            match self.enc {
                Enc::Utf8 | Enc::Legacy(_) => {
                    for (i, b) in buf[..got].iter().enumerate() {
                        if *b == b'\n' {
                            found.push(pos + i as u64 + 1);
                        }
                    }
                }
                Enc::Utf16Le | Enc::Utf16Be => {
                    let (nl, zero) = if self.enc == Enc::Utf16Le { (0usize, 1usize) } else { (1, 0) };
                    for (i, pair) in buf[..got].as_chunks::<2>().0.iter().enumerate() {
                        if pair[nl] == b'\n' && pair[zero] == 0 {
                            found.push(pos + i as u64 * 2 + 2);
                        }
                    }
                }
            }
            pos += got as u64;
            let mut g = self.lock();
            g.starts.extend(found);
            g.end = pos;
            drop(g);
            if !full {
                break;
            }
        }
        self.finish(pos);
    }

    fn finish(&self, pos: u64) {
        let tail = self.read_tail(pos);
        let mut g = self.lock();
        g.end = pos;
        g.tail = tail;
        // A file that ends in a newline has no line after it.
        g.nl_end = g.starts.last().is_some_and(|&s| s >= pos);
        if g.nl_end {
            g.starts.pop();
        }
        drop(g);
        self.done.store(true, Ordering::Release);
    }

    fn read_tail(&self, end: u64) -> Vec<u8> {
        self.read_bytes(end.saturating_sub(TAIL_BYTES), end).unwrap_or_default()
    }

    /// UTF-16: the text is in units of two bytes.
    fn wide(&self) -> bool {
        matches!(self.enc, Enc::Utf16Le | Enc::Utf16Be)
    }

    /// The build has read the whole file (as long as it was when it began).
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// Stop the build for good: the file is no longer on show.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// The lines known so far: all of them once [`Self::is_done`].
    pub fn count(&self) -> usize {
        self.lock().starts.len()
    }

    /// How much of the file the build has read, in 0..=1.
    pub fn progress(&self) -> f32 {
        let len = self.len();
        if self.is_done() || len == 0 {
            return 1.0;
        }
        (self.lock().end as f32 / len as f32).min(1.0)
    }

    /// The file's length when the build last began.
    pub fn len(&self) -> u64 {
        self.len.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The byte offset where line `n` starts, if it is known yet.
    pub fn start_of(&self, n: usize) -> Option<u64> {
        self.lock().starts.get(n).copied()
    }

    /// The line that holds byte `offset` (the last one when it is past the end), among those known so far.
    pub fn line_at(&self, offset: u64) -> usize {
        let g = self.lock();
        g.starts.partition_point(|&s| s <= offset).saturating_sub(1)
    }

    /// Index what was appended since the build finished, on a thread of its own. A shorter file, or one whose last
    /// bytes before the old end differ, was cut or replaced: [`Grow::Rebuild`], and the caller opens a new index.
    pub fn grow(self: &Arc<Self>) -> Grow {
        if !self.is_done() {
            return Grow::Same;
        }
        let Ok(mut f) = File::open(&self.path) else { return Grow::Same };
        let Ok(mut len) = f.metadata().map(|m| m.len()) else { return Grow::Same };
        if self.wide() {
            len &= !1;
        }
        let mut g = self.lock();
        if len < g.end {
            return Grow::Rebuild;
        }
        if !g.tail.is_empty() && self.read_tail(g.end) != g.tail {
            return Grow::Rebuild;
        }
        if len == g.end {
            return Grow::Same;
        }
        let from = g.end;
        if g.nl_end {
            g.starts.push(from);
            g.nl_end = false;
        }
        drop(g);
        self.len.store(len, Ordering::Relaxed);
        self.done.store(false, Ordering::Release);
        let _ = f.seek(SeekFrom::Start(from));
        let me = self.clone();
        if std::thread::Builder::new().name("line-index".into()).spawn(move || me.build(f, from)).is_err() {
            self.done.store(true, Ordering::Release);
            return Grow::Same;
        }
        Grow::Grew
    }

    /// Lines `range` as text, without their line ends and cut to [`MAX_LINE_BYTES`]. Fewer when the file ends first
    /// or a read fails.
    pub fn lines(&self, range: Range<usize>) -> Vec<Line> {
        let Some((from, to, starts)) = self.span(range.start, range.end) else { return Vec::new() };
        // One read for the lot, unless a long line makes it big: then each line is read up to what can be shown.
        if to - from <= BULK_READ_BYTES {
            let Ok(bytes) = self.read_bytes(from, to) else { return Vec::new() };
            return starts
                .windows(2)
                .map(|w| {
                    let a = (w[0] - from) as usize;
                    let b = ((w[1] - from) as usize).min(bytes.len());
                    self.line(&bytes[a.min(b)..b], false)
                })
                .collect();
        }
        let Ok(mut f) = File::open(&self.path) else { return Vec::new() };
        let mut lines = Vec::with_capacity(starts.len() - 1);
        for w in starts.windows(2) {
            let whole = w[1] - w[0];
            let mut n = whole.min(READ_LINE_BYTES);
            if self.wide() {
                n &= !1;
            }
            let mut v = Vec::with_capacity(n as usize);
            if f.seek(SeekFrom::Start(w[0])).is_err() || (&mut f).take(n).read_to_end(&mut v).is_err() {
                break;
            }
            lines.push(self.line(&v, whole > n));
        }
        lines
    }

    /// The whole text of line `n`, however long (the 8 KiB cut is for the screen; this is for copying). `None` when
    /// the line is not known or is longer than `limit` bytes.
    pub fn full_line(&self, n: usize, limit: u64) -> Option<String> {
        let (from, to, _) = self.span(n, n + 1)?;
        if to - from > limit {
            return None;
        }
        let bytes = self.read_bytes(from, to).ok()?;
        Some(self.decode(&bytes).trim_end_matches(['\n', '\r']).to_owned())
    }

    /// The byte range of lines `first..end`, and the start of every one of them plus the end of the last.
    fn span(&self, first: usize, end: usize) -> Option<(u64, u64, Vec<u64>)> {
        let g = self.lock();
        if first >= g.starts.len() || first >= end {
            return None;
        }
        let end = end.min(g.starts.len());
        let mut starts = g.starts[first..end].to_vec();
        starts.push(g.starts.get(end).copied().unwrap_or(g.end));
        Some((starts[0], *starts.last().unwrap(), starts))
    }

    fn read_bytes(&self, from: u64, to: u64) -> std::io::Result<Vec<u8>> {
        let mut f = File::open(&self.path)?;
        f.seek(SeekFrom::Start(from))?;
        let mut v = Vec::with_capacity((to - from) as usize);
        f.take(to - from).read_to_end(&mut v)?;
        Ok(v)
    }

    fn line(&self, b: &[u8], read_cut: bool) -> Line {
        let (text, cut) = clip(&self.decode(b));
        Line { text, cut: cut || read_cut }
    }

    fn decode(&self, b: &[u8]) -> String {
        match self.enc {
            Enc::Utf8 => String::from_utf8_lossy(b).into_owned(),
            Enc::Legacy(e) => e.decode_without_bom_handling(b).0.into_owned(),
            enc => {
                let units: Vec<u16> = b
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| if enc == Enc::Utf16Be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) })
                    .collect();
                String::from_utf16_lossy(&units)
            }
        }
    }
}

impl Drop for LineIndex {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn clip(s: &str) -> (String, bool) {
    let s = s.trim_end_matches(['\n', '\r']);
    if s.len() <= MAX_LINE_BYTES {
        return (s.to_owned(), false);
    }
    let mut n = MAX_LINE_BYTES;
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    (s[..n].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn file(name: &str, bytes: &[u8]) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ito-lineindex-{}-{name}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn wait(i: &LineIndex) {
        for _ in 0..2000 {
            if i.is_done() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("index did not finish");
    }

    fn texts(i: &LineIndex, r: Range<usize>) -> Vec<String> {
        i.lines(r).into_iter().map(|l| l.text).collect()
    }

    #[test]
    fn reads_lines_by_number() {
        let p = file("basic", b"one\ntwo\r\nthree\n");
        let i = LineIndex::open(&p).unwrap();
        wait(&i);
        assert_eq!(i.count(), 3);
        assert_eq!(texts(&i, 0..3), ["one", "two", "three"]);
        assert_eq!(texts(&i, 1..2), ["two"]);
        assert_eq!(texts(&i, 2..99), ["three"]);
        assert!(i.lines(3..4).is_empty());
        assert_eq!(i.start_of(1), Some(4));
        assert_eq!(i.line_at(5), 1);
        assert_eq!(i.progress(), 1.0);
    }

    #[test]
    fn a_last_line_without_a_newline_counts() {
        let p = file("nonl", b"a\nb");
        let i = LineIndex::open(&p).unwrap();
        wait(&i);
        assert_eq!(texts(&i, 0..9), ["a", "b"]);
    }

    #[test]
    fn a_long_line_is_cut_and_says_so() {
        let mut b = "あ".repeat(10_000).into_bytes();
        b.extend(b"\nshort\n");
        let p = file("long", &b);
        let i = LineIndex::open(&p).unwrap();
        wait(&i);
        let l = i.lines(0..2);
        assert!(l[0].cut && l[0].text.len() <= MAX_LINE_BYTES && l[0].text.chars().all(|c| c == 'あ'));
        assert_eq!(l[1], Line { text: "short".into(), cut: false });
        assert_eq!(i.full_line(0, 1 << 20).unwrap().chars().count(), 10_000);
        assert!(i.full_line(0, 100).is_none());
    }

    #[test]
    fn a_big_file_is_indexed_across_chunks() {
        let body: String = (0..30_000).map(|n| format!("line {n}\n")).collect();
        let p = file("big", body.as_bytes());
        let i = LineIndex::open(&p).unwrap();
        wait(&i);
        assert_eq!(i.count(), 30_000);
        assert_eq!(texts(&i, 12_345..12_346), ["line 12345"]);
        assert_eq!(texts(&i, 29_999..30_000), ["line 29999"]);
    }

    #[test]
    fn guesses_the_encoding() {
        assert_eq!(guess(b"\xef\xbb\xbfhi"), (Enc::Utf8, 3));
        assert_eq!(guess(b"\xff\xfeh\0"), (Enc::Utf16Le, 2));
        assert_eq!(guess(b"\xfe\xff\0h"), (Enc::Utf16Be, 2));
        assert_eq!(guess("日本語のテキスト".as_bytes()), (Enc::Utf8, 0));
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("これはシフトJISの文章です。");
        assert_eq!(guess(&sjis), (Enc::Legacy(encoding_rs::SHIFT_JIS), 0));
        let wide: Vec<u8> = "hello world, this is wide text\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(guess(&wide), (Enc::Utf16Le, 0));
    }

    #[test]
    fn reads_utf16_and_shift_jis() {
        let wide: Vec<u8> = [0xfeffu16].into_iter().chain("あい\nう\n".encode_utf16()).flat_map(|u| u.to_le_bytes()).collect();
        let p = file("u16", &wide);
        let i = LineIndex::open(&p).unwrap();
        wait(&i);
        assert_eq!(i.encoding(), Enc::Utf16Le);
        assert_eq!(texts(&i, 0..2), ["あい", "う"]);

        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("一行目の文章です\n二行目の文章です\n");
        let p = file("sjis", &sjis);
        let i = LineIndex::open(&p).unwrap();
        wait(&i);
        assert_eq!(texts(&i, 0..2), ["一行目の文章です", "二行目の文章です"]);
    }

    #[test]
    fn grow_tells_appended_from_rewritten() {
        let p = file("grow", b"a\nb\n");
        let i = LineIndex::open(&p).unwrap();
        wait(&i);
        assert_eq!(i.grow(), Grow::Same);
        std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(b"c\nd").unwrap();
        assert_eq!(i.grow(), Grow::Grew);
        wait(&i);
        assert_eq!(texts(&i, 0..9), ["a", "b", "c", "d"]);
        // Rewritten to the same length with other bytes.
        std::fs::write(&p, b"x\ny\nz\nw").unwrap();
        assert_eq!(i.grow(), Grow::Rebuild);
        // Cut.
        std::fs::write(&p, b"a").unwrap();
        assert_eq!(i.grow(), Grow::Rebuild);
    }

    #[test]
    fn an_empty_or_missing_file() {
        let p = file("empty", b"");
        let i = LineIndex::open(&p).unwrap();
        wait(&i);
        assert!(i.is_empty());
        assert!(i.lines(1..2).is_empty());
        assert!(LineIndex::open(&std::env::temp_dir().join("ito-lineindex-no-such-file")).is_err());
    }
}
