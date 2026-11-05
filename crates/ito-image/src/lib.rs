//! Images for apps: a picture from the OS clipboard, and lossy WebP.
//!
//! - [`clipboard_image`] reads what a screenshot tool (Win+Shift+S) or "Copy image"
//!   put on the clipboard, as straight (not premultiplied) RGBA8. `None` when the
//!   clipboard holds no picture. Windows reads `"PNG"`, then `CF_DIBV5`, then
//!   `CF_DIB` directly; macOS and Linux go through `arboard`.
//! - [`encode_webp`] writes lossy WebP (quality 0 to 100) and keeps transparency.
//!   Pure Rust, so the six build targets stay light. It is slow for a big picture
//!   (a 4K screenshot takes about a second): call it from a worker thread.
//! - [`Rgba::content_hash`] is a key for "the same picture, the same file".
//! - [`fake`] stands in for the clipboard in `--keys` tests.
//!
//! The DIB reader ([`dib::decode`]) and the PNG reader ([`decode_png`]) are plain
//! functions on bytes, so they can be tested (and used) on any OS.
// Pixel loops over `chunks_exact(4)` read better than `as_chunks`, and the MSRV-wide form is the same.
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod dib;
pub mod fake;
mod webp;

use std::fmt;

pub use webp::encode_webp;

/// A picture as straight (not premultiplied) RGBA8, rows from the top, 4 bytes a pixel.
#[derive(Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes: red, green, blue, alpha.
    pub pixels: Vec<u8>,
}

impl fmt::Debug for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rgba({}x{})", self.width, self.height)
    }
}

impl Rgba {
    /// Checks that `pixels` is exactly `width * height * 4` bytes and not empty.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Rgba, Error> {
        if width == 0 || height == 0 {
            return Err(Error::Invalid("the picture is empty".into()));
        }
        let want = (width as u64) * (height as u64) * 4;
        if pixels.len() as u64 != want {
            return Err(Error::Invalid(format!("{width}x{height} needs {want} bytes, got {}", pixels.len())));
        }
        Ok(Rgba { width, height, pixels })
    }

    /// True when no pixel is see-through.
    pub fn is_opaque(&self) -> bool {
        self.pixels.chunks_exact(4).all(|p| p[3] == 255)
    }

    /// A 64-bit key of the size and the pixels (FNV-1a), for "the same picture, the
    /// same file". Not a cryptographic hash. Pixels that are fully transparent
    /// count by their colour too, so hash what you save.
    pub fn content_hash(&self) -> u64 {
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |bytes: &[u8]| {
            for &b in bytes {
                h = (h ^ b as u64).wrapping_mul(PRIME);
            }
        };
        feed(&self.width.to_le_bytes());
        feed(&self.height.to_le_bytes());
        feed(&self.pixels);
        h
    }

    /// A copy at most `max` pixels wide, the aspect kept (area average, alpha-weighted so see-through edges do not
    /// darken). Unchanged when already narrow enough; `max` 0 counts as 1. The height is at least 1.
    pub fn fit_width(&self, max: u32) -> Rgba {
        let max = max.max(1);
        if self.width <= max {
            return self.clone();
        }
        let (sw, sh) = (self.width as usize, self.height as usize);
        let dw = max as usize;
        let dh = ((sh as u64 * dw as u64 + sw as u64 / 2) / sw as u64).max(1) as usize;
        // Source span of destination cell `i` of `n` over `len`, as (first, last_exclusive, weight_of(k)).
        let span =
            |i: usize, n: usize, len: usize| -> (f64, f64) { (i as f64 * len as f64 / n as f64, (i + 1) as f64 * len as f64 / n as f64) };
        let overlap = |k: usize, a: f64, b: f64| -> f64 { ((k as f64 + 1.0).min(b) - (k as f64).max(a)).max(0.0) };
        let mut pixels = Vec::with_capacity(dw * dh * 4);
        for y in 0..dh {
            let (y0, y1) = span(y, dh, sh);
            for x in 0..dw {
                let (x0, x1) = span(x, dw, sw);
                let (mut r, mut g, mut b, mut a, mut wsum) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
                for ky in (y0.floor() as usize)..(y1.ceil() as usize).min(sh) {
                    let wy = overlap(ky, y0, y1);
                    for kx in (x0.floor() as usize)..(x1.ceil() as usize).min(sw) {
                        let w = wy * overlap(kx, x0, x1);
                        let p = &self.pixels[(ky * sw + kx) * 4..][..4];
                        let wa = w * p[3] as f64;
                        r += wa * p[0] as f64;
                        g += wa * p[1] as f64;
                        b += wa * p[2] as f64;
                        a += wa;
                        wsum += w;
                    }
                }
                if a > 0.0 {
                    pixels.extend([(r / a).round() as u8, (g / a).round() as u8, (b / a).round() as u8]);
                    pixels.push((a / wsum).round().min(255.0) as u8);
                } else {
                    pixels.extend([0, 0, 0, 0]);
                }
            }
        }
        Rgba { width: max, height: dh as u32, pixels }
    }

    /// Turns premultiplied pixels (colour already scaled by alpha) into straight ones.
    pub fn unpremultiply(&mut self) {
        for p in self.pixels.chunks_exact_mut(4) {
            let a = p[3] as u32;
            if a != 0 && a != 255 {
                for c in &mut p[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
    }
}

/// Why a picture could not be read or written.
#[derive(Debug)]
pub enum Error {
    /// The clipboard could not be opened or read (another program holds it, the OS refused).
    Clipboard(String),
    /// The bytes are not a picture of a kind we read, or are cut short.
    Invalid(String),
    /// A kind of picture we do not read (for example a run-length compressed bitmap).
    Unsupported(String),
    /// Too big to handle: more than 2^28 pixels to read, or more than 16383 on a side to write as WebP.
    TooLarge(u32, u32),
    /// The WebP encoder failed.
    Encode(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Clipboard(m) => write!(f, "clipboard: {m}"),
            Error::Invalid(m) => write!(f, "invalid picture: {m}"),
            Error::Unsupported(m) => write!(f, "unsupported picture: {m}"),
            Error::TooLarge(w, h) => write!(f, "the picture is too large ({w}x{h})"),
            Error::Encode(m) => write!(f, "WebP encoder: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// The picture on the clipboard, or `None` when there is none.
///
/// Asks [`fake`] first (so a `--keys` run can put a picture there), then the OS.
/// Touches the system, so call it from a worker thread when the clipboard may be slow.
pub fn clipboard_image() -> Result<Option<Rgba>, Error> {
    if let Some(r) = fake::get() {
        return r;
    }
    os::read()
}

/// Decodes a PNG file to straight RGBA8 (any bit depth or colour type).
pub fn decode_png(bytes: &[u8]) -> Result<Rgba, Error> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| Error::Invalid(format!("PNG: {e}")))?;
    let (w, h) = reader.info().size();
    if (w as u64) * (h as u64) > dib::MAX_PIXELS {
        return Err(Error::TooLarge(w, h));
    }
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or(Error::TooLarge(w, h))?];
    let frame = reader.next_frame(&mut buf).map_err(|e| Error::Invalid(format!("PNG: {e}")))?;
    buf.truncate(frame.buffer_size());
    let pixels = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Indexed => return Err(Error::Invalid("PNG: palette was not expanded".into())),
    };
    Rgba::new(frame.width, frame.height, pixels)
}

#[cfg(windows)]
mod os {
    use super::{decode_png, dib, Error, Rgba};
    use windows::core::w;
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    const CF_DIB: u32 = 8;
    const CF_DIBV5: u32 = 17;

    /// Closes the clipboard when it goes out of scope.
    struct Open;
    impl Drop for Open {
        fn drop(&mut self) {
            // SAFETY: only built after OpenClipboard succeeded on this thread.
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    fn open() -> Result<Open, Error> {
        // Another program may hold the clipboard for a moment (a clipboard manager).
        let mut last = None;
        for _ in 0..10 {
            // SAFETY: plain Win32 call; no window owns the clipboard for a read.
            match unsafe { OpenClipboard(None) } {
                Ok(()) => return Ok(Open),
                Err(e) => last = Some(e),
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Err(Error::Clipboard(format!("cannot open it: {}", last.map(|e| e.to_string()).unwrap_or_default())))
    }

    /// Copies the bytes of one clipboard format; `None` when it is not there.
    fn bytes(format: u32) -> Option<Vec<u8>> {
        // SAFETY: the clipboard is open (the caller holds `Open`). The handle belongs to
        // the clipboard: it is locked, copied and unlocked here, never freed.
        unsafe {
            IsClipboardFormatAvailable(format).ok()?;
            let h: HANDLE = GetClipboardData(format).ok()?;
            let g = HGLOBAL(h.0);
            let len = GlobalSize(g);
            let p = GlobalLock(g) as *const u8;
            if p.is_null() || len == 0 {
                return None;
            }
            let v = std::slice::from_raw_parts(p, len).to_vec();
            let _ = GlobalUnlock(g);
            Some(v)
        }
    }

    pub fn read() -> Result<Option<Rgba>, Error> {
        let _open = open()?;
        // SAFETY: registering a format name has no preconditions.
        let png = unsafe { RegisterClipboardFormatW(w!("PNG")) };
        let mut first_err = None;
        // PNG keeps the alpha exactly; DIBV5 next; DIB last (no alpha, usually).
        for format in [png, CF_DIBV5, CF_DIB] {
            if format == 0 {
                continue;
            }
            let Some(data) = bytes(format) else { continue };
            let r = if format == png { decode_png(&data) } else { dib::decode(&data) };
            match r {
                Ok(img) => return Ok(Some(img)),
                Err(e) => first_err = first_err.or(Some(e)),
            }
        }
        // Something we could not read is an error; nothing there is `None`.
        match first_err {
            Some(e) => Err(e),
            None => Ok(None),
        }
    }
}

#[cfg(not(windows))]
mod os {
    use super::{Error, Rgba};

    pub fn read() -> Result<Option<Rgba>, Error> {
        let mut cb = arboard::Clipboard::new().map_err(|e| Error::Clipboard(e.to_string()))?;
        match cb.get_image() {
            Ok(img) => Rgba::new(img.width as u32, img.height as u32, img.bytes.into_owned()).map(Some),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(e) => Err(Error::Clipboard(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_width_averages_and_keeps_aspect() {
        let img = Rgba::new(4, 2, [[255, 0, 0, 255], [255, 0, 0, 255], [0, 0, 255, 255], [0, 0, 255, 255]].repeat(2).concat()).unwrap();
        assert_eq!(img.fit_width(8), img, "already narrow enough");
        let half = img.fit_width(2);
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(half.pixels, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        let one = img.fit_width(1);
        assert_eq!((one.width, one.height), (1, 1));
        assert_eq!(one.pixels[3], 255);
        assert!((one.pixels[0] as i32 - 128).abs() <= 1);
    }

    #[test]
    fn fit_width_does_not_darken_see_through_pixels() {
        let img = Rgba::new(2, 1, vec![200, 100, 50, 255, 0, 0, 0, 0]).unwrap();
        let one = img.fit_width(1);
        assert_eq!(one.pixels, vec![200, 100, 50, 128]);
        let wide = Rgba::new(1000, 1, vec![9; 4000]).unwrap().fit_width(10);
        assert_eq!((wide.width, wide.height), (10, 1), "height is at least 1");
    }

    #[test]
    fn new_checks_length() {
        assert!(Rgba::new(2, 1, vec![0; 8]).is_ok());
        assert!(Rgba::new(2, 1, vec![0; 7]).is_err());
        assert!(Rgba::new(0, 1, vec![]).is_err());
    }

    #[test]
    fn hash_follows_the_picture() {
        let a = Rgba::new(2, 1, vec![1, 2, 3, 255, 4, 5, 6, 255]).unwrap();
        let mut b = a.clone();
        assert_eq!(a.content_hash(), b.content_hash());
        b.pixels[5] = 9;
        assert_ne!(a.content_hash(), b.content_hash());
        // Same bytes, other shape.
        let c = Rgba::new(1, 2, a.pixels.clone()).unwrap();
        assert_ne!(a.content_hash(), c.content_hash());
    }

    #[test]
    fn unpremultiply_restores_colour() {
        let mut p = Rgba::new(3, 1, vec![50, 25, 0, 128, 7, 7, 7, 0, 9, 9, 9, 255]).unwrap();
        p.unpremultiply();
        assert_eq!(&p.pixels[..4], &[100, 50, 0, 128]);
        assert_eq!(&p.pixels[4..8], &[7, 7, 7, 0]);
        assert_eq!(&p.pixels[8..], &[9, 9, 9, 255]);
    }

    fn png_of(color: png::ColorType, data: &[u8], w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(color);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(data).unwrap();
        out
    }

    #[test]
    fn png_kinds_become_rgba() {
        let rgba = decode_png(&png_of(png::ColorType::Rgba, &[1, 2, 3, 4], 1, 1)).unwrap();
        assert_eq!(rgba.pixels, [1, 2, 3, 4]);
        let rgb = decode_png(&png_of(png::ColorType::Rgb, &[1, 2, 3], 1, 1)).unwrap();
        assert_eq!(rgb.pixels, [1, 2, 3, 255]);
        let ga = decode_png(&png_of(png::ColorType::GrayscaleAlpha, &[9, 100], 1, 1)).unwrap();
        assert_eq!(ga.pixels, [9, 9, 9, 100]);
        assert!(decode_png(b"not a png").is_err());
    }

    /// Reads the real clipboard; run by hand (`cargo test -p ito-image -- --ignored`).
    #[test]
    #[ignore = "touches the real clipboard"]
    fn real_clipboard_does_not_fail() {
        let r = os::read();
        println!("{r:?}");
        assert!(r.is_ok());
    }
}
