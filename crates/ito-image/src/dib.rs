//! Device-independent bitmaps (the clipboard's `CF_DIB` and `CF_DIBV5`, and the
//! inside of a `.bmp` file) to straight RGBA8.
//!
//! Handles `BITMAPINFOHEADER` and the V2 to V5 headers; 1, 2 (no), 4, 8 bits with a
//! palette, 16, 24 and 32 bits; `BI_RGB`, `BI_BITFIELDS` and `BI_ALPHABITFIELDS`;
//! bottom-up and top-down. Run-length, JPEG and PNG compression, and the old
//! 12-byte `BITMAPCOREHEADER`, are [`Error::Unsupported`].
//!
//! Alpha: a header that has an alpha mask (V3 and later, or `BI_ALPHABITFIELDS`)
//! gives the alpha; a 32-bit `BI_RGB` bitmap with a V4/V5 header uses its fourth
//! byte only if some pixel has it set (a screenshot with zeros there is opaque).
//! Everything else is opaque. The colour is taken as straight, which is how
//! `CF_DIBV5` is written by the screenshot tools; use [`Rgba::unpremultiply`] for a
//! source known to be premultiplied.

use crate::{Error, Rgba};

/// The most pixels we read (2^28, a 16384 x 16384 picture), so a bad header cannot ask for gigabytes.
pub const MAX_PIXELS: u64 = 1 << 28;

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// One colour channel of a pixel: where its bits are and how to widen them to 8.
#[derive(Clone, Copy)]
struct Channel {
    mask: u32,
    shift: u32,
    max: u32,
}

impl Channel {
    fn new(mask: u32) -> Channel {
        if mask == 0 {
            return Channel { mask: 0, shift: 0, max: 0 };
        }
        let shift = mask.trailing_zeros();
        Channel { mask, shift, max: mask >> shift }
    }
    fn get(self, px: u32) -> u8 {
        if self.max == 0 {
            return 0;
        }
        let v = (px & self.mask) >> self.shift;
        if self.max == 255 {
            v as u8
        } else {
            ((v * 255 + self.max / 2) / self.max) as u8
        }
    }
}

/// Decodes a packed DIB: the header, then the masks and the palette, then the pixels.
pub fn decode(data: &[u8]) -> Result<Rgba, Error> {
    let bad = |m: &str| Error::Invalid(format!("DIB: {m}"));
    let header = u32_at(data, 0).ok_or_else(|| bad("too short"))? as usize;
    if header < 40 {
        return Err(Error::Unsupported("DIB with a BITMAPCOREHEADER".into()));
    }
    if header > data.len() {
        return Err(bad("the header is cut short"));
    }
    let width = u32_at(data, 4).ok_or_else(|| bad("too short"))? as i32;
    let raw_height = u32_at(data, 8).ok_or_else(|| bad("too short"))? as i32;
    let bpp = u16_at(data, 14).ok_or_else(|| bad("too short"))? as u32;
    let compression = u32_at(data, 16).ok_or_else(|| bad("too short"))?;
    let clr_used = u32_at(data, 32).ok_or_else(|| bad("too short"))? as usize;
    if width <= 0 || raw_height == 0 || raw_height == i32::MIN {
        return Err(bad("empty or negative width"));
    }
    let (w, h) = (width as u32, raw_height.unsigned_abs());
    let top_down = raw_height < 0;
    if (w as u64) * (h as u64) > MAX_PIXELS {
        return Err(Error::TooLarge(w, h));
    }
    if !matches!(compression, BI_RGB | BI_BITFIELDS | BI_ALPHABITFIELDS) {
        return Err(Error::Unsupported(format!("DIB compression {compression}")));
    }
    if !matches!(bpp, 1 | 4 | 8 | 16 | 24 | 32) {
        return Err(Error::Unsupported(format!("DIB with {bpp} bits a pixel")));
    }

    // Masks: inside a V2+ header, or right after a 40-byte one.
    let mut at = header;
    let has_masks = compression == BI_BITFIELDS || compression == BI_ALPHABITFIELDS;
    let mut masks: Option<[u32; 4]> = None;
    if header >= 52 {
        let r = u32_at(data, 40).ok_or_else(|| bad("too short"))?;
        let g = u32_at(data, 44).ok_or_else(|| bad("too short"))?;
        let b = u32_at(data, 48).ok_or_else(|| bad("too short"))?;
        let a = if header >= 56 { u32_at(data, 52).ok_or_else(|| bad("too short"))? } else { 0 };
        if has_masks {
            masks = Some([r, g, b, a]);
        }
    } else if has_masks {
        let n = if compression == BI_ALPHABITFIELDS { 4 } else { 3 };
        let mut m = [0u32; 4];
        for (i, slot) in m.iter_mut().enumerate().take(n) {
            *slot = u32_at(data, at + i * 4).ok_or_else(|| bad("the masks are cut short"))?;
        }
        masks = Some(m);
        at += n * 4;
    }
    let palette_len = if bpp <= 8 {
        if clr_used > 0 {
            clr_used.min(1 << bpp)
        } else {
            1 << bpp
        }
    } else {
        clr_used.min(256)
    };
    let palette_at = at;
    at += palette_len * 4;

    let stride = ((w as usize * bpp as usize).div_ceil(32)) * 4;
    let need = stride.checked_mul(h as usize).ok_or(Error::TooLarge(w, h))?;
    let pixels_in = data.get(at..).filter(|p| p.len() >= need).ok_or_else(|| bad("the pixels are cut short"))?;

    let v4_plus = header >= 108;
    let (chans, alpha_chan, alpha_if_set) = match (bpp, masks) {
        (16 | 32, Some([r, g, b, a])) => ([Channel::new(r), Channel::new(g), Channel::new(b)], Channel::new(a), false),
        (16, None) => ([Channel::new(0x7C00), Channel::new(0x03E0), Channel::new(0x001F)], Channel::new(0), false),
        (32, None) => ([Channel::new(0xFF_0000), Channel::new(0xFF00), Channel::new(0xFF)], Channel::new(0xFF00_0000), v4_plus),
        _ => ([Channel::new(0); 3], Channel::new(0), false),
    };

    let mut out = vec![0u8; w as usize * h as usize * 4];
    for y in 0..h as usize {
        let src_row = if top_down { y } else { h as usize - 1 - y };
        let row = &pixels_in[src_row * stride..src_row * stride + stride];
        let dst = &mut out[y * w as usize * 4..(y + 1) * w as usize * 4];
        for (x, px) in dst.chunks_exact_mut(4).enumerate() {
            let rgba = match bpp {
                24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255],
                32 => {
                    let v = u32::from_le_bytes([row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]]);
                    [chans[0].get(v), chans[1].get(v), chans[2].get(v), alpha_chan.get(v)]
                }
                16 => {
                    let v = u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]) as u32;
                    [chans[0].get(v), chans[1].get(v), chans[2].get(v), alpha_chan.get(v)]
                }
                _ => {
                    let per = 8 / bpp as usize;
                    let byte = row[x / per];
                    let shift = 8 - bpp as usize * (x % per + 1);
                    let idx = (byte >> shift) as usize & ((1 << bpp) - 1);
                    if idx >= palette_len {
                        [0, 0, 0, 255]
                    } else {
                        let e = palette_at + idx * 4;
                        [data[e + 2], data[e + 1], data[e], 255]
                    }
                }
            };
            px.copy_from_slice(&rgba);
        }
    }

    // Does this bitmap really carry alpha?
    let carries_alpha = match (bpp, masks) {
        (16 | 32, Some([_, _, _, a])) => a != 0,
        (32, None) => alpha_if_set && out.chunks_exact(4).any(|p| p[3] != 0),
        _ => false,
    };
    if !carries_alpha {
        for p in out.chunks_exact_mut(4) {
            p[3] = 255;
        }
    }
    Rgba::new(w, h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header of `size` bytes (40, 56, 108 or 124) for the fixtures.
    fn header(size: u32, w: i32, h: i32, bpp: u16, compression: u32, clr_used: u32, masks: [u32; 4]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(size.to_le_bytes());
        v.extend(w.to_le_bytes());
        v.extend(h.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(bpp.to_le_bytes());
        v.extend(compression.to_le_bytes());
        v.extend([0u8; 4]); // size image
        v.extend([0u8; 8]); // ppm
        v.extend(clr_used.to_le_bytes());
        v.extend([0u8; 4]); // important
        if size >= 52 {
            for m in &masks[..3] {
                v.extend(m.to_le_bytes());
            }
        }
        if size >= 56 {
            v.extend(masks[3].to_le_bytes());
        }
        v.resize(size as usize, 0);
        v
    }

    #[test]
    fn rgb24_bottom_up_with_padding() {
        // 2x2, bottom-up: first stored row is the bottom one. Row = 6 bytes + 2 padding.
        let mut d = header(40, 2, 2, 24, BI_RGB, 0, [0; 4]);
        // bottom row: red, green (BGR order)
        d.extend([0, 0, 255, 0, 255, 0, 9, 9]);
        // top row: blue, white
        d.extend([255, 0, 0, 255, 255, 255, 9, 9]);
        let img = decode(&d).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.pixels, [0, 0, 255, 255, 255, 255, 255, 255, 255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn rgb24_top_down() {
        let mut d = header(40, 1, -2, 24, BI_RGB, 0, [0; 4]);
        d.extend([0, 0, 255, 0]); // top: red (+1 pad)
        d.extend([0, 255, 0, 0]); // bottom: green
        let img = decode(&d).unwrap();
        assert_eq!(img.pixels, [255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn v5_bitfields_keeps_straight_alpha() {
        // What a screenshot tool writes: 32-bit BGRA, BI_BITFIELDS, top-down, V5 header.
        let mut d = header(124, 2, -1, 32, BI_BITFIELDS, 0, [0xFF0000, 0xFF00, 0xFF, 0xFF00_0000]);
        d.extend([10, 20, 30, 128]); // b g r a  -> r=30 g=20 b=10 a=128
        d.extend([1, 2, 3, 0]);
        let img = decode(&d).unwrap();
        assert_eq!(img.pixels, [30, 20, 10, 128, 3, 2, 1, 0]);
    }

    #[test]
    fn v5_rgb32_zero_fourth_byte_is_opaque_but_set_byte_is_alpha() {
        let mut d = header(124, 1, 1, 32, BI_RGB, 0, [0; 4]);
        d.extend([10, 20, 30, 0]);
        assert_eq!(decode(&d).unwrap().pixels, [30, 20, 10, 255]);
        let mut d = header(124, 1, 1, 32, BI_RGB, 0, [0; 4]);
        d.extend([10, 20, 30, 77]);
        assert_eq!(decode(&d).unwrap().pixels, [30, 20, 10, 77]);
        // An old 40-byte header never has alpha, whatever is in the fourth byte.
        let mut d = header(40, 1, 1, 32, BI_RGB, 0, [0; 4]);
        d.extend([10, 20, 30, 77]);
        assert_eq!(decode(&d).unwrap().pixels, [30, 20, 10, 255]);
    }

    #[test]
    fn bitfields_masks_after_a_40_byte_header() {
        // CF_DIB from a 32-bit surface: masks follow the header, no alpha mask, so opaque.
        let mut d = header(40, 1, 1, 32, BI_BITFIELDS, 0, [0; 4]);
        for m in [0xFF0000u32, 0xFF00, 0xFF] {
            d.extend(m.to_le_bytes());
        }
        d.extend([10, 20, 30, 0]);
        assert_eq!(decode(&d).unwrap().pixels, [30, 20, 10, 255]);
        // BI_ALPHABITFIELDS adds a fourth mask.
        let mut d = header(40, 1, 1, 32, BI_ALPHABITFIELDS, 0, [0; 4]);
        for m in [0xFF0000u32, 0xFF00, 0xFF, 0xFF00_0000] {
            d.extend(m.to_le_bytes());
        }
        d.extend([10, 20, 30, 40]);
        assert_eq!(decode(&d).unwrap().pixels, [30, 20, 10, 40]);
    }

    #[test]
    fn rgb565_widens_to_8_bits() {
        let mut d = header(40, 2, 1, 16, BI_BITFIELDS, 0, [0; 4]);
        for m in [0xF800u32, 0x07E0, 0x001F] {
            d.extend(m.to_le_bytes());
        }
        d.extend(0xF800u16.to_le_bytes()); // red
        d.extend(0x07FFu16.to_le_bytes()); // green + blue
        let img = decode(&d).unwrap();
        assert_eq!(img.pixels, [255, 0, 0, 255, 0, 255, 255, 255]);
    }

    #[test]
    fn palette_8_and_4_bits() {
        let mut d = header(40, 3, 1, 8, BI_RGB, 2, [0; 4]);
        d.extend([0, 0, 255, 0]); // 0: red (BGRX)
        d.extend([255, 0, 0, 0]); // 1: blue
        d.extend([1, 0, 1, 9]); // pixels 1 0 1 + pad
        assert_eq!(decode(&d).unwrap().pixels, [0, 0, 255, 255, 255, 0, 0, 255, 0, 0, 255, 255]);

        let mut d = header(40, 3, 1, 4, BI_RGB, 2, [0; 4]);
        d.extend([0, 0, 255, 0]);
        d.extend([255, 0, 0, 0]);
        d.extend([0x10, 0x10, 0, 0]); // pixels 1 0 1 (high nibble first)
        assert_eq!(decode(&d).unwrap().pixels, [0, 0, 255, 255, 255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        assert!(matches!(decode(&[]), Err(Error::Invalid(_))));
        assert!(matches!(decode(&[12, 0, 0, 0, 1, 0, 1, 0, 1, 0, 24, 0]), Err(Error::Unsupported(_))));
        let rle = header(40, 1, 1, 8, 1, 0, [0; 4]);
        assert!(matches!(decode(&rle), Err(Error::Unsupported(_))));
        // Pixels cut short.
        let mut d = header(40, 4, 4, 24, BI_RGB, 0, [0; 4]);
        d.extend([0; 10]);
        assert!(matches!(decode(&d), Err(Error::Invalid(_))));
        // A header that claims a huge picture is refused before any allocation.
        let big = header(40, 100_000, 100_000, 24, BI_RGB, 0, [0; 4]);
        assert!(matches!(decode(&big), Err(Error::TooLarge(..))));
    }
}
