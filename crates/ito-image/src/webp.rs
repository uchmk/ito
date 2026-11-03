//! Lossy WebP (VP8) encoding that keeps transparency, in pure Rust.
//!
//! The `image` crate only writes lossless WebP, which is several times bigger for
//! a screenshot. Two pure-Rust lossy encoders are combined:
//!
//! - fully opaque pictures go to `oxideav-webp` (plain VP8, the smallest output),
//!   and fall back to `webp-rust` if it panics or fails;
//! - pictures with transparency go to `webp-rust` (VP8 plus an `ALPH` chunk).

use crate::{Error, Rgba};

/// The most a WebP side can be.
const MAX_SIDE: u32 = 16383;

/// Encodes `image` as a lossy WebP. `quality` is 0 to 100 (clamped; NaN means 75).
/// Transparency is kept. Slow for big pictures: call it from a worker thread.
pub fn encode_webp(image: &Rgba, quality: f32) -> Result<Vec<u8>, Error> {
    if image.width > MAX_SIDE || image.height > MAX_SIDE {
        return Err(Error::TooLarge(image.width, image.height));
    }
    let quality = if quality.is_nan() { 75.0 } else { quality.clamp(0.0, 100.0) };
    if image.is_opaque() {
        // A panic inside the encoder (seen on some alpha-like patterns) falls through to the other encoder.
        if let Ok(Ok(bytes)) = std::panic::catch_unwind(|| opaque(image, quality)) {
            return Ok(bytes);
        }
    }
    with_alpha(image, quality)
}

#[allow(clippy::field_reassign_with_default)] // `#[non_exhaustive]`: no struct expression
fn opaque(image: &Rgba, quality: f32) -> Result<Vec<u8>, Error> {
    let mut o = oxideav_webp::EncodeOptions::default();
    o.quality = Some(quality);
    oxideav_webp::encode_rgba8(image.width, image.height, &image.pixels, &o).map_err(|e| Error::Encode(e.to_string()))
}

#[allow(clippy::field_reassign_with_default)] // `#[non_exhaustive]`: no struct expression
fn with_alpha(image: &Rgba, quality: f32) -> Result<Vec<u8>, Error> {
    let buf = webp_rust::ImageBuffer { width: image.width as usize, height: image.height as usize, rgba: image.pixels.clone() };
    let mut cfg = webp_rust::LossyEncodingConfig::default();
    cfg.quality = quality;
    webp_rust::encode_lossy_with_config(&buf, &cfg, None).map_err(|e| Error::Encode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> image::RgbaImage {
        image::load_from_memory_with_format(bytes, image::ImageFormat::WebP).unwrap().to_rgba8()
    }

    /// A smooth gradient with a flat block, a little like a UI screenshot.
    fn picture(w: u32, h: u32, alpha: impl Fn(u32, u32) -> u8) -> Rgba {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let flat = (20..44).contains(&x) && (20..44).contains(&y);
                let (r, g, b) = if flat { (240, 240, 240) } else { ((x * 255 / w) as u8, (y * 255 / h) as u8, 128) };
                px.extend([r, g, b, alpha(x, y)]);
            }
        }
        Rgba::new(w, h, px).unwrap()
    }

    fn mean_abs_diff(a: &[u8], b: &[u8], channels: usize) -> f64 {
        let mut sum = 0u64;
        let mut n = 0u64;
        for (p, q) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
            for c in 0..channels {
                sum += p[c].abs_diff(q[c]) as u64;
                n += 1;
            }
        }
        sum as f64 / n as f64
    }

    #[test]
    fn opaque_round_trip() {
        let img = picture(64, 48, |_, _| 255);
        let bytes = encode_webp(&img, 90.0).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        let dec = decode(&bytes);
        assert_eq!((dec.width(), dec.height()), (64, 48));
        assert!(mean_abs_diff(&img.pixels, dec.as_raw(), 3) < 6.0);
        assert!(dec.pixels().all(|p| p.0[3] == 255));
    }

    #[test]
    fn transparency_is_kept() {
        // Left half clear, right half opaque, a soft ramp in between.
        let img = picture(64, 48, |x, _| {
            if x < 24 {
                0
            } else if x < 40 {
                ((x - 24) * 16) as u8
            } else {
                255
            }
        });
        let bytes = encode_webp(&img, 80.0).unwrap();
        let dec = decode(&bytes);
        assert_eq!((dec.width(), dec.height()), (64, 48));
        let alpha_err = dec.pixels().zip(img.pixels.chunks_exact(4)).map(|(p, q)| p.0[3].abs_diff(q[3]) as u32).max().unwrap();
        assert!(alpha_err <= 8, "alpha off by {alpha_err}");
        assert_eq!(dec.get_pixel(2, 2).0[3], 0);
        assert_eq!(dec.get_pixel(60, 2).0[3], 255);
    }

    #[test]
    fn quality_trades_size() {
        let img = picture(96, 96, |_, _| 255);
        let low = encode_webp(&img, 10.0).unwrap().len();
        let high = encode_webp(&img, 95.0).unwrap().len();
        assert!(low < high, "{low} !< {high}");
    }

    #[test]
    fn odd_sizes_and_bad_quality() {
        let img = picture(33, 17, |_, _| 255);
        for q in [-5.0, 0.0, 100.0, 1000.0, f32::NAN] {
            let dec = decode(&encode_webp(&img, q).unwrap());
            assert_eq!((dec.width(), dec.height()), (33, 17));
        }
        let one = Rgba::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        assert_eq!(decode(&encode_webp(&one, 90.0).unwrap()).dimensions(), (1, 1));
    }

    #[test]
    fn too_large_is_refused() {
        let wide = Rgba { width: 16384, height: 1, pixels: vec![0; 16384 * 4] };
        assert!(matches!(encode_webp(&wide, 80.0), Err(Error::TooLarge(..))));
    }
}
