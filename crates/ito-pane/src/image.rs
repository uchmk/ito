//! Pictures in the pane: sixel (DCS `q`), kitty's graphics protocol (APC
//! `G`) and iTerm2's inline images (OSC 1337 `File=`).
//!
//! Like the prompts' marks ([`PromptLinks`](crate::osc)), the place a
//! picture goes is known only to the parser, which runs with the grid locked
//! on the reader thread. So the sequence is taken out of what is read, the
//! picture decoded there and then, and in its place goes a space carrying an
//! OSC 8 link to `tsumugi:img?<key>` -- the picture's top left cell, which
//! alacritty scrolls, rewraps and erases like any other -- followed by the
//! cursor moves the protocol asks for past the picture. The view finds the
//! cells by their links ([`placements`](crate::placements)) and draws each
//! picture over the cells from there, as kitty does (above the text). A
//! picture whose cell is overwritten or cleared is gone with it.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A picture, decoded: `width` x `height` pixels, RGBA, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Where a picture is on the view: its top left cell (the line may be
/// above the view's top, 0 being the top) and how many cells it covers.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Placement {
    pub key: u64,
    pub line: i32,
    pub col: usize,
    pub cols: usize,
    pub rows: usize,
}

/// The link the top left cell of a picture carries, before its key.
pub const IMAGE_LINK: &str = "tsumugi:img?";

/// Keys are unique in the process, so pictures from different panes (and,
/// through the server, different sessions) never meet in a cache.
static NEXT: AtomicU64 = AtomicU64::new(1);

/// A picture shown, and the cells it covers.
#[derive(Clone)]
pub(crate) struct Shown {
    pub(crate) picture: Arc<Picture>,
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    /// The kitty image it is a placement of, to delete it by (0 for one
    /// sent without an id); none for sixel and iTerm2.
    kitty: Option<u32>,
}

/// The pictures of a pane, shared by the reader (which adds them) and the
/// pane (which hands them to the view).
#[derive(Default)]
pub(crate) struct Store {
    shown: HashMap<u64, Shown>,
    /// Oldest first, to let go of when there are too many bytes.
    order: VecDeque<u64>,
    bytes: usize,
    /// The cell's size in pixels and the grid's columns, as last resized.
    pub(crate) cell: (u16, u16),
    pub(crate) cols: usize,
}

/// The most picture a pane keeps, in bytes of RGBA; the oldest go first.
const STORE_MAX: usize = 256 << 20;
/// The most of a sequence read before it is thrown away, encoded.
const BODY_MAX: usize = 64 << 20;
/// No picture is kept larger than this on either side (its RGBA then fits
/// in one of the server's messages, 64 MiB).
const SIDE_MAX: u32 = 4000;

pub(crate) type Pictures = Arc<Mutex<Store>>;

impl Store {
    pub(crate) fn get(&self, key: u64) -> Option<Shown> {
        self.shown.get(&key).cloned()
    }

    /// The most rows any picture covers: how far above the view to look.
    pub(crate) fn tallest(&self) -> usize {
        self.shown.values().map(|s| s.rows).max().unwrap_or(0)
    }

    fn add(&mut self, shown: Shown) -> u64 {
        let key = NEXT.fetch_add(1, Ordering::Relaxed);
        self.bytes += shown.picture.rgba.len();
        self.shown.insert(key, shown);
        self.order.push_back(key);
        while self.bytes > STORE_MAX && self.order.len() > 1 {
            let old = self.order.pop_front().expect("more than one");
            if let Some(s) = self.shown.remove(&old) {
                self.bytes -= s.picture.rgba.len();
            }
        }
        key
    }

    fn remove_where(&mut self, f: impl Fn(&Shown) -> bool) {
        let gone: Vec<u64> = self.shown.iter().filter(|(_, s)| f(s)).map(|(&k, _)| k).collect();
        for k in gone {
            if let Some(s) = self.shown.remove(&k) {
                self.bytes -= s.picture.rgba.len();
            }
            self.order.retain(|&o| o != k);
        }
    }
}

/// Which sequence is being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Sixel,
    Kitty,
    ITerm,
}

#[derive(Default)]
enum State {
    #[default]
    Ground,
    /// ESC was the last byte.
    Esc,
    /// `ESC P` and the parameters so far, waiting for the `q` of a sixel.
    Dcs(Vec<u8>),
    /// `ESC _`, waiting for the `G` of kitty's.
    Apc,
    /// `ESC ]` and what follows, until it is or is not `1337;File=`.
    Osc(Vec<u8>),
    /// Inside a picture's sequence: its kind, what it says so far (or
    /// `None` past [`BODY_MAX`]), and whether the last byte was ESC.
    Body(Kind, Option<Vec<u8>>, bool),
}

/// The OSC 1337 commands taken here; the others (`SetUserVar`,
/// `CurrentDir`, ...) go on to the parser.
const ITERM: [&[u8]; 4] = [b"1337;File=", b"1337;MultipartFile=", b"1337;FilePart=", b"1337;FileEnd"];

/// Takes the pictures out of what a program writes; see the module.
pub(crate) struct Catcher {
    state: State,
    store: Pictures,
    /// Kitty's images by id, sent and kept to be placed (`a=t`, `a=p`).
    kitty: HashMap<u32, Arc<Picture>>,
    kitty_order: VecDeque<u32>,
    /// A kitty transmission in chunks (`m=1`): its first chunk's keys and
    /// the payload so far.
    chunks: Option<(Keys, Vec<u8>)>,
    /// An iTerm2 file in parts (`MultipartFile`): its arguments and data.
    parts: Option<(Vec<u8>, Vec<u8>)>,
    /// What to write back to the program (kitty's answers).
    pub(crate) replies: Vec<Vec<u8>>,
}

/// The most kitty images kept to be placed later.
const KITTY_MAX: usize = 64;

impl Catcher {
    pub(crate) fn new(store: Pictures) -> Self {
        Self { state: State::Ground, store, kitty: HashMap::new(), kitty_order: VecDeque::new(), chunks: None, parts: None, replies: Vec::new() }
    }

    /// `chunk` with the pictures taken out and their cells put in, or
    /// `None` when there was nothing to take.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Option<Vec<u8>> {
        // The common case: no ESC and nothing under way.
        if matches!(self.state, State::Ground) && !chunk.contains(&0x1b) {
            return None;
        }
        let mut out = Vec::with_capacity(chunk.len());
        // Bytes held from the last read are given back (or taken) now, so
        // this chunk alone is not what is read.
        let mut changed = !matches!(self.state, State::Ground);
        let mut i = 0;
        while i < chunk.len() {
            let b = chunk[i];
            i += 1;
            match std::mem::take(&mut self.state) {
                State::Ground => {
                    if b == 0x1b {
                        self.state = State::Esc;
                    } else {
                        out.push(b);
                    }
                }
                State::Esc => match b {
                    b'P' => self.state = State::Dcs(Vec::new()),
                    b'_' => self.state = State::Apc,
                    b']' => self.state = State::Osc(Vec::new()),
                    0x1b => {
                        out.push(0x1b);
                        self.state = State::Esc;
                    }
                    _ => out.extend_from_slice(&[0x1b, b]),
                },
                State::Dcs(mut params) => {
                    if b == b'q' {
                        changed = true;
                        self.state = State::Body(Kind::Sixel, Some(params), false);
                    } else if (b.is_ascii_digit() || b == b';') && params.len() < 32 {
                        params.push(b);
                        self.state = State::Dcs(params);
                    } else {
                        out.extend_from_slice(b"\x1bP");
                        out.extend_from_slice(&params);
                        // The byte is looked at again, from the ground.
                        i -= 1;
                    }
                }
                State::Apc => {
                    if b == b'G' {
                        changed = true;
                        self.state = State::Body(Kind::Kitty, Some(Vec::new()), false);
                    } else {
                        out.extend_from_slice(b"\x1b_");
                        i -= 1;
                    }
                }
                State::Osc(mut said) => {
                    said.push(b);
                    if ITERM.iter().any(|p| said.len() >= p.len() && said.starts_with(p)) {
                        changed = true;
                        self.state = State::Body(Kind::ITerm, Some(said), false);
                    } else if ITERM.iter().any(|p| p.starts_with(&said)) {
                        self.state = State::Osc(said);
                    } else {
                        out.extend_from_slice(b"\x1b]");
                        out.extend_from_slice(&said);
                    }
                }
                State::Body(kind, mut body, esc) => {
                    let end = match b {
                        0x07 if kind == Kind::ITerm => true,
                        b'\\' if esc => true,
                        // Cancelled: thrown away.
                        0x18 | 0x1a => {
                            body = None;
                            true
                        }
                        _ => false,
                    };
                    if end {
                        if let Some(body) = body {
                            let cells = self.finish(kind, body);
                            out.extend_from_slice(&cells);
                        }
                    } else if b == 0x1b {
                        self.state = State::Body(kind, body, true);
                    } else {
                        if esc {
                            // An ESC not ending it with `\` is part of it
                            // (none of the three uses one), and kept.
                            push(&mut body, 0x1b);
                        }
                        push(&mut body, b);
                        self.state = State::Body(kind, body, false);
                    }
                }
            }
        }
        // An ESC or the start of a sequence at the very end is held until
        // the next read says what it is; that only changes the chunk when
        // something was taken, or when bytes are held back.
        let held = !matches!(self.state, State::Ground | State::Body(..));
        (changed || held).then_some(out)
    }

    /// A picture's sequence has ended: the cells to put in its place.
    fn finish(&mut self, kind: Kind, body: Vec<u8>) -> Vec<u8> {
        match kind {
            // The body starts with the parameters, digits and `;`, which
            // the decoder passes over.
            Kind::Sixel => {
                match sixel(&body) {
                    Some(p) => self.show(p, None, None, None, After::Below),
                    None => Vec::new(),
                }
            }
            Kind::Kitty => self.kitty(&body),
            Kind::ITerm => self.iterm(&body),
        }
    }

    /// Keep `picture` and give the cells for it at the cursor: `cols` and
    /// `rows` are what the program asked for, if anything.
    fn show(&mut self, picture: Picture, cols: Option<usize>, rows: Option<usize>, kitty: Option<u32>, after: After) -> Vec<u8> {
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let (cw, ch) = (store.cell.0.max(1) as f64, store.cell.1.max(1) as f64);
        let (w, h) = (picture.width.max(1) as f64, picture.height.max(1) as f64);
        let (cols, rows) = match (cols, rows) {
            (Some(c), Some(r)) => (c, r),
            (Some(c), None) => (c, ((c as f64 * cw) * h / w / ch).ceil() as usize),
            (None, Some(r)) => ((((r as f64 * ch) * w / h) / cw).ceil() as usize, r),
            (None, None) => {
                // As many cells as its pixels need, but no wider than the
                // pane: a wider one is made smaller, keeping its shape.
                let most = store.cols.max(1) as f64 * cw;
                let scale = if w > most { most / w } else { 1.0 };
                (((w * scale) / cw).ceil() as usize, ((h * scale) / ch).ceil() as usize)
            }
        };
        let (cols, rows) = (cols.clamp(1, 1000), rows.clamp(1, 1000));
        // Twice the pixels the cells have, for a screen at 2x; no more.
        let picture = smaller(picture, (cols as f64 * cw * 2.0) as u32, (rows as f64 * ch * 2.0) as u32);
        let key = store.add(Shown { picture: Arc::new(picture), cols, rows, kitty });
        drop(store);
        cells(key, cols, rows, after)
    }

    fn kitty(&mut self, body: &[u8]) -> Vec<u8> {
        let (control, payload) = match body.iter().position(|&b| b == b';') {
            Some(at) => (&body[..at], &body[at + 1..]),
            None => (body, &b""[..]),
        };
        let keys = Keys::parse(control);
        // A chunk of a longer transmission: kept until the last.
        let (keys, payload) = match self.chunks.take() {
            Some((first, mut data)) => {
                data.extend_from_slice(payload);
                if keys.more {
                    if data.len() <= BODY_MAX {
                        self.chunks = Some((first, data));
                    }
                    return Vec::new();
                }
                (first, data)
            }
            None if keys.more => {
                self.chunks = Some((keys, payload.to_vec()));
                return Vec::new();
            }
            None => (keys, payload.to_vec()),
        };
        match keys.action {
            b'd' => {
                let id = keys.id;
                match keys.delete {
                    b'i' | b'I' => {
                        self.store.lock().unwrap_or_else(|e| e.into_inner()).remove_where(|s| s.kitty.is_some() && s.kitty == id);
                        if keys.delete == b'I' {
                            if let Some(id) = id {
                                self.kitty.remove(&id);
                            }
                        }
                    }
                    // All of them, and anything else asked (by cell, by
                    // z-index ...), which is rarer than clearing all.
                    _ => self.store.lock().unwrap_or_else(|e| e.into_inner()).remove_where(|s| s.kitty.is_some()),
                }
                Vec::new()
            }
            b'p' => {
                let found = keys.id.and_then(|id| self.kitty.get(&id).cloned());
                match found {
                    Some(p) => {
                        self.reply(&keys, Ok(()));
                        self.show((*p).clone(), keys.cols, keys.rows, Some(keys.id.unwrap_or(0)), keys.after())
                    }
                    None => {
                        self.reply(&keys, Err("ENOENT:no such image"));
                        Vec::new()
                    }
                }
            }
            b't' | b'T' | b'q' => {
                let picture = kitty_picture(&keys, &payload);
                let Ok(picture) = picture else {
                    self.reply(&keys, picture.map(|_| ()));
                    return Vec::new();
                };
                self.reply(&keys, Ok(()));
                if keys.action == b'q' {
                    return Vec::new();
                }
                let picture = Arc::new(picture);
                if let Some(id) = keys.id {
                    if self.kitty.insert(id, picture.clone()).is_none() {
                        self.kitty_order.push_back(id);
                    }
                    while self.kitty_order.len() > KITTY_MAX {
                        let old = self.kitty_order.pop_front().expect("over the most");
                        self.kitty.remove(&old);
                    }
                }
                if keys.action == b'T' {
                    self.show((*picture).clone(), keys.cols, keys.rows, Some(keys.id.unwrap_or(0)), keys.after())
                } else {
                    Vec::new()
                }
            }
            _ => {
                self.reply(&keys, Err("EINVAL:unknown action"));
                Vec::new()
            }
        }
    }

    /// Kitty's answer, unless the program asked for quiet (`q=1` keeps
    /// errors, `q=2` neither) or gave no id to answer to.
    fn reply(&mut self, keys: &Keys, result: Result<(), &str>) {
        let Some(id) = keys.id else { return };
        let quiet = match result {
            Ok(()) => keys.quiet >= 1,
            Err(_) => keys.quiet >= 2,
        };
        if quiet {
            return;
        }
        let mut r = format!("\x1b_Gi={id}");
        if let Some(p) = keys.placement {
            r.push_str(&format!(",p={p}"));
        }
        r.push(';');
        r.push_str(result.err().unwrap_or("OK"));
        r.push_str("\x1b\\");
        self.replies.push(r.into_bytes());
    }

    fn iterm(&mut self, body: &[u8]) -> Vec<u8> {
        let body = &body[b"1337;".len()..];
        let (args, data) = if let Some(rest) = body.strip_prefix(b"File=") {
            match rest.iter().position(|&b| b == b':') {
                Some(at) => (rest[..at].to_vec(), rest[at + 1..].to_vec()),
                None => return Vec::new(),
            }
        } else if let Some(rest) = body.strip_prefix(b"MultipartFile=") {
            self.parts = Some((rest.to_vec(), Vec::new()));
            return Vec::new();
        } else if let Some(rest) = body.strip_prefix(b"FilePart=") {
            if let Some((_, data)) = self.parts.as_mut() {
                if data.len() + rest.len() <= BODY_MAX {
                    data.extend_from_slice(rest);
                } else {
                    self.parts = None;
                }
            }
            return Vec::new();
        } else {
            // FileEnd.
            match self.parts.take() {
                Some(p) => p,
                None => return Vec::new(),
            }
        };
        let args = String::from_utf8_lossy(&args).into_owned();
        let arg = |name: &str| args.split(';').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| v.to_owned()));
        // Without `inline=1` it is a file to save, which is not done here.
        if arg("inline").as_deref() != Some("1") {
            return Vec::new();
        }
        let Some(bytes) = base64(&data) else { return Vec::new() };
        let Some(picture) = decode(&bytes) else { return Vec::new() };
        let (cw, ch, pane) = {
            let s = self.store.lock().unwrap_or_else(|e| e.into_inner());
            (s.cell.0.max(1) as f64, s.cell.1.max(1) as f64, s.cols.max(1) as f64)
        };
        // `N` cells, `Npx`, `N%` of the pane, or `auto`.
        let size = |v: Option<String>, cell: f64, whole: f64| -> Option<usize> {
            let v = v?;
            if let Some(px) = v.strip_suffix("px") {
                Some((px.parse::<f64>().ok()? / cell).ceil() as usize)
            } else if let Some(pc) = v.strip_suffix('%') {
                Some((pc.parse::<f64>().ok()? / 100.0 * whole).ceil() as usize)
            } else {
                v.parse::<usize>().ok()
            }
        };
        let rows_whole = 50.0;
        let cols = size(arg("width"), cw, pane);
        let rows = size(arg("height"), ch, rows_whole);
        let keep = arg("preserveAspectRatio").as_deref() != Some("0");
        let (cols, rows) = match (cols, rows) {
            // Both given and the shape kept: the picture fits inside them.
            (Some(c), Some(r)) if keep => {
                let (w, h) = (picture.width as f64, picture.height as f64);
                let scale = (c as f64 * cw / w).min(r as f64 * ch / h);
                (Some(((w * scale) / cw).ceil() as usize), Some(((h * scale) / ch).ceil() as usize))
            }
            other => other,
        };
        self.show(picture, cols, rows, None, After::Beside)
    }
}

fn push(body: &mut Option<Vec<u8>>, b: u8) {
    if let Some(v) = body {
        if v.len() < BODY_MAX {
            v.push(b);
        } else {
            *body = None;
        }
    }
}

/// Where the cursor goes after a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum After {
    /// To the line below it, in the column it started in (sixel).
    Below,
    /// To the cell after its last on its bottom line (kitty, iTerm2).
    Beside,
    /// Back where it was (kitty's `C=1`).
    Stay,
}

/// The bytes put in a picture's place: its top left cell, carrying the
/// link, then the cursor moved past it.
fn cells(key: u64, cols: usize, rows: usize, after: After) -> Vec<u8> {
    let mut out = format!("\x1b]8;id=tsumugi-img-{key};{IMAGE_LINK}{key}\x1b\\ \x1b]8;;\x1b\\").into_bytes();
    match after {
        After::Stay => out.push(0x08),
        After::Below => {
            out.push(0x08);
            out.extend(std::iter::repeat_n(b'\n', rows));
        }
        After::Beside => {
            out.extend(std::iter::repeat_n(b'\n', rows - 1));
            if cols > 1 {
                out.extend_from_slice(format!("\x1b[{}C", cols - 1).as_bytes());
            }
        }
    }
    out
}

/// A kitty command's keys (`a=T,f=100,i=7,...`).
#[derive(Clone, Debug, Default)]
struct Keys {
    action: u8,
    format: u32,
    medium: u8,
    compressed: bool,
    width: u32,
    height: u32,
    id: Option<u32>,
    placement: Option<u32>,
    cols: Option<usize>,
    rows: Option<usize>,
    stay: bool,
    more: bool,
    quiet: u8,
    delete: u8,
}

impl Keys {
    fn parse(control: &[u8]) -> Self {
        let mut k = Keys { action: b't', format: 32, medium: b'd', delete: b'a', ..Default::default() };
        for kv in control.split(|&b| b == b',') {
            let Some(at) = kv.iter().position(|&b| b == b'=') else { continue };
            let (key, value) = (&kv[..at], &kv[at + 1..]);
            let num = || std::str::from_utf8(value).ok().and_then(|v| v.parse::<u32>().ok());
            let ch = value.first().copied().unwrap_or(0);
            match key {
                b"a" => k.action = ch,
                b"f" => k.format = num().unwrap_or(32),
                b"t" => k.medium = ch,
                b"o" => k.compressed = ch == b'z',
                b"s" => k.width = num().unwrap_or(0),
                b"v" => k.height = num().unwrap_or(0),
                b"i" => k.id = num().filter(|&i| i > 0),
                b"p" => k.placement = num().filter(|&p| p > 0),
                b"c" => k.cols = num().filter(|&c| c > 0).map(|c| c as usize),
                b"r" => k.rows = num().filter(|&r| r > 0).map(|r| r as usize),
                b"C" => k.stay = ch == b'1',
                b"m" => k.more = ch == b'1',
                b"q" => k.quiet = num().unwrap_or(0) as u8,
                b"d" => k.delete = ch,
                _ => {}
            }
        }
        k
    }

    fn after(&self) -> After {
        if self.stay {
            After::Stay
        } else {
            After::Beside
        }
    }
}

/// The picture a kitty transmission carries.
fn kitty_picture(keys: &Keys, payload: &[u8]) -> Result<Picture, &'static str> {
    let data = base64(payload).ok_or("EINVAL:bad base64")?;
    let data = match keys.medium {
        b'd' => data,
        // A file, or a temporary file to delete after (only one named as
        // the protocol says, so nothing else is ever deleted).
        b'f' | b't' => {
            let path = String::from_utf8(data).map_err(|_| "EINVAL:bad path")?;
            let read = std::fs::metadata(&path).ok().filter(|m| m.is_file() && m.len() <= BODY_MAX as u64).and_then(|_| std::fs::read(&path).ok());
            if keys.medium == b't' && path.contains("tty-graphics-protocol") {
                let _ = std::fs::remove_file(&path);
            }
            read.ok_or("EBADF:cannot read the file")?
        }
        _ => return Err("EINVAL:shared memory is not supported"),
    };
    let data = if keys.compressed { miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&data, BODY_MAX * 4).map_err(|_| "EINVAL:bad zlib data")? } else { data };
    match keys.format {
        100 => decode(&data).ok_or("EBADPNG:cannot read the PNG"),
        24 | 32 => {
            let (w, h) = (keys.width, keys.height);
            let per = if keys.format == 24 { 3 } else { 4 };
            if w == 0 || h == 0 || w > SIDE_MAX * 4 || h > SIDE_MAX * 4 || data.len() < (w * h) as usize * per {
                return Err("EINVAL:bad size");
            }
            let rgba = if per == 4 { data[..(w * h) as usize * 4].to_vec() } else { data.as_chunks::<3>().0.iter().take((w * h) as usize).flat_map(|&[r, g, b]| [r, g, b, 255]).collect() };
            Ok(Picture { width: w, height: h, rgba })
        }
        _ => Err("EINVAL:unknown format"),
    }
}

/// A PNG, JPEG or GIF (its first frame), as RGBA.
fn decode(bytes: &[u8]) -> Option<Picture> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(SIDE_MAX * 4);
    limits.max_image_height = Some(SIDE_MAX * 4);
    reader.limits(limits);
    let img = reader.decode().ok()?.into_rgba8();
    Some(Picture { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

/// `picture`, no larger than `w` x `h` (nor [`SIDE_MAX`]), shape kept.
fn smaller(picture: Picture, w: u32, h: u32) -> Picture {
    let (w, h) = (w.clamp(1, SIDE_MAX), h.clamp(1, SIDE_MAX));
    if picture.width <= w && picture.height <= h {
        return picture;
    }
    let scale = (w as f64 / picture.width as f64).min(h as f64 / picture.height as f64);
    let (nw, nh) = (((picture.width as f64 * scale) as u32).max(1), ((picture.height as f64 * scale) as u32).max(1));
    let Some(img) = image::RgbaImage::from_raw(picture.width, picture.height, picture.rgba) else {
        return Picture { width: 1, height: 1, rgba: vec![0; 4] };
    };
    let img = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
    Picture { width: nw, height: nh, rgba: img.into_raw() }
}

/// Base64 as both protocols send it: padding or none, and line breaks
/// ignored. `None` when it is not base64.
pub(crate) fn base64(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in text {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The colors a sixel starts with: the VT340's, in percent.
const VT340: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (20, 20, 80),
    (80, 13, 13),
    (20, 80, 20),
    (80, 20, 80),
    (20, 80, 80),
    (80, 80, 20),
    (53, 53, 53),
    (26, 26, 26),
    (33, 33, 60),
    (60, 26, 26),
    (33, 60, 33),
    (60, 33, 60),
    (33, 60, 60),
    (60, 60, 33),
    (80, 80, 80),
];

fn percent(p: u32) -> u8 {
    (p.min(100) * 255 / 100) as u8
}

/// DEC's HLS (blue at 0 degrees, red at 120) as RGB.
fn hls(h: u32, l: u32, s: u32) -> [u8; 3] {
    let h = ((h + 240) % 360) as f64;
    let (l, s) = (l.min(100) as f64 / 100.0, s.min(100) as f64 / 100.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// A sixel image's data (what follows the `q`), as a picture; pixels no
/// sixel sets stay clear. `None` when it sets none.
pub(crate) fn sixel(data: &[u8]) -> Option<Picture> {
    // Twice over: once for the size, once to paint.
    let mut size = (0u32, 0u32);
    let mut canvas: Vec<u8> = Vec::new();
    for paint in [false, true] {
        if paint {
            if size.0 == 0 || size.1 == 0 {
                return None;
            }
            canvas = vec![0; (size.0 * size.1 * 4) as usize];
        }
        let mut palette: Vec<[u8; 3]> = (0..256).map(|i| VT340.get(i).map_or([0, 0, 0], |&(r, g, b)| [percent(r as u32), percent(g as u32), percent(b as u32)])).collect();
        let mut color = palette[0];
        let (mut x, mut y) = (0u32, 0u32);
        let mut i = 0;
        let number = |i: &mut usize| -> u32 {
            let mut n = 0u32;
            while let Some(d) = data.get(*i).filter(|d| d.is_ascii_digit()) {
                n = n.saturating_mul(10).saturating_add((d - b'0') as u32);
                *i += 1;
            }
            n
        };
        let numbers = |i: &mut usize| -> Vec<u32> {
            let mut v = vec![number(i)];
            while data.get(*i) == Some(&b';') {
                *i += 1;
                v.push(number(i));
            }
            v
        };
        while i < data.len() {
            let c = data[i];
            i += 1;
            let mut repeat = 1;
            let c = match c {
                b'"' => {
                    let v = numbers(&mut i);
                    if !paint {
                        if let (Some(&w), Some(&h)) = (v.get(2), v.get(3)) {
                            size = (size.0.max(w.min(SIDE_MAX)), size.1.max(h.min(SIDE_MAX)));
                        }
                    }
                    continue;
                }
                b'#' => {
                    let v = numbers(&mut i);
                    let n = v[0] as usize % 256;
                    if v.len() >= 5 {
                        palette[n] = match v[1] {
                            1 => hls(v[2], v[3], v[4]),
                            _ => [percent(v[2]), percent(v[3]), percent(v[4])],
                        };
                    }
                    color = palette[n];
                    continue;
                }
                b'$' => {
                    x = 0;
                    continue;
                }
                b'-' => {
                    x = 0;
                    y += 6;
                    continue;
                }
                b'!' => {
                    repeat = number(&mut i).max(1);
                    match data.get(i) {
                        Some(&c) => {
                            i += 1;
                            c
                        }
                        None => break,
                    }
                }
                c => c,
            };
            if !(0x3f..=0x7e).contains(&c) {
                continue;
            }
            let bits = c - 0x3f;
            if paint {
                for dx in 0..repeat {
                    let px = x + dx;
                    if px >= size.0 {
                        break;
                    }
                    for bit in 0..6 {
                        let py = y + bit;
                        if bits & (1 << bit) != 0 && py < size.1 {
                            let at = ((py * size.0 + px) * 4) as usize;
                            canvas[at..at + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
                        }
                    }
                }
            } else if bits != 0 {
                let top = (0..6).rev().find(|b| bits & (1 << b) != 0).unwrap_or(0);
                size = (size.0.max((x + repeat).min(SIDE_MAX)), size.1.max((y + top + 1).min(SIDE_MAX)));
            }
            x = x.saturating_add(repeat);
        }
    }
    Some(Picture { width: size.0, height: size.1, rgba: canvas })
}

/// Write kitty's answers to the program.
pub(crate) fn answer(w: &mut impl Write, replies: &mut Vec<Vec<u8>>) {
    for r in replies.drain(..) {
        let _ = w.write_all(&r);
    }
    let _ = w.flush();
}
