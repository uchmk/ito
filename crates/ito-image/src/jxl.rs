//! JPEG XL: a cheap magic check, and (feature `jxl`) a decoder to straight RGBA8 (jxl-oxide, pure Rust).

#[cfg(feature = "jxl")]
use crate::{Error, Rgba};
#[cfg(feature = "jxl")]
use std::borrow::Cow;

/// The ISOBMFF container signature box: `00 00 00 0C 'J' 'X' 'L' ' ' 0D 0A 87 0A`.
const CONTAINER: [u8; 12] = [0x00, 0x00, 0x00, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A];

/// True when `bytes` start like a JPEG XL file: a bare codestream (`FF 0A`) or the ISOBMFF container
/// (the full 12-byte signature box). Reads only the first bytes; always available, needs no feature.
pub fn is_jxl(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0x0A]) || bytes.starts_with(&CONTAINER)
}

#[cfg(feature = "jxl")]
fn invalid(m: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("JXL: {m}"))
}

/// Decodes a JPEG XL file (bare codestream or container) to straight (not premultiplied) RGBA8 in sRGB.
///
/// - Returns keyframe 0, the first picture shown of an animation. Frames after it are not decoded.
/// - The orientation is applied, so the picture comes out upright. An embedded ICC profile or a wide gamut is converted to
///   sRGB (rendering intent relative; moxcms, pure Rust). Grey pictures become R = G = B. Premultiplied alpha is made straight.
///   A CMYK picture is converted to sRGB through its profile like any other; a conversion the CMS cannot do is an error.
/// - Only the header is read first. A picture with more than `min(max_pixels, dib::MAX_PIXELS)` pixels is
///   [`Error::TooLarge`] before any pixel is decoded. The decoder's own buffers are then capped by an allocation limit derived
///   from that header.
/// - Never panics: a decoder panic becomes [`Error::Invalid`], like bad, empty or cut bytes.
/// - A file cut short still gives keyframe 0 if that frame is complete (an animation cut after frame 0 previews); with no
///   complete keyframe it is [`Error::Invalid`].
/// - A container is not given to the decoder: only its `jxlc` / `jxlp` codestream is. Metadata boxes (Exif, XMP, JUMBF,
///   brotli `brob`, JPEG reconstruction `jbrd`) are skipped and never decoded, so an inflation bomb in them does nothing.
/// - The size check bounds everything that grows with the picture. The decoder's own buffers are also under the allocation
///   cap. The output buffers made here are not counted by the cap: up to `w * h * channels` f32 samples on the premultiplied
///   path (u8 otherwise), plus the RGBA8 result. An embedded ICC profile is bounded by neither (jxl-oxide caps it at
///   256 MiB). An out-of-memory abort cannot be caught.
/// - There is no time limit, so a hostile file can keep the worker busy. The caller may drop the result.
///
/// It is slow (a 3 MP lossless picture takes about 50 ms with threads) and allocates a lot, so call it from a worker thread.
/// It uses rayon's global thread pool (jxl-oxide's default with the `rayon` feature); no pool is built per call.
#[cfg(feature = "jxl")]
pub fn decode_jxl(bytes: &[u8], max_pixels: u64) -> Result<Rgba, Error> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode(bytes, max_pixels))) {
        Ok(r) => r,
        Err(_) => Err(invalid("the decoder panicked")),
    }
}

/// Headroom over the f32 samples (4 bytes per channel and pixel) the decoder keeps.
///
/// Measured by bisecting the smallest cap that still decodes, on 1024x1024 pictures made with ImageMagick/libjxl
/// (lossless RGB, lossy q90, 16-bit, lossless RGBA, 3-frame animation, grey). The peak was 0.4 to 2.2 times
/// `w * h * channels * 4` bytes; the worst were the lossy (VarDCT, 2.1) and the 16-bit (2.2) pictures. 6 leaves 2.7 times
/// room over that worst case; `ALLOC_BASE` adds more for small pictures (the 16x8 test fixtures peak near 3 KB).
#[cfg(feature = "jxl")]
const ALLOC_FACTOR: u64 = 6;
/// Room for what does not scale with the picture (tables, entropy decoders, group buffers of a tiny picture).
#[cfg(feature = "jxl")]
const ALLOC_BASE: u64 = 64 << 20;

/// The allocation cap for a picture of `w` x `h` with `channels` channels (colour and extra).
#[cfg(feature = "jxl")]
fn limit_for(w: u32, h: u32, channels: usize) -> usize {
    let n = (w as u64) * (h as u64) * (channels as u64) * 4 * ALLOC_FACTOR + ALLOC_BASE;
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// Feeds `bytes` in 4 KiB chunks until the header is parsed, so the frame data behind it is not decoded yet.
/// Returns the image and how many bytes of `bytes` it has taken.
#[cfg(feature = "jxl")]
fn parse_header(bytes: &[u8], tracker: Option<jxl_oxide::AllocTracker>) -> Result<(jxl_oxide::JxlImage, usize), Error> {
    use jxl_oxide::{InitializeResult, JxlImage};
    let mut builder = JxlImage::builder();
    if let Some(t) = tracker {
        builder = builder.alloc_tracker(t);
    }
    let mut uninit = builder.build_uninit();
    let mut rest = bytes;
    while !rest.is_empty() {
        let take = rest.len().min(4096);
        let used = uninit.feed_bytes(&rest[..take]).map_err(invalid)?;
        if used == 0 {
            break;
        }
        rest = &rest[used..];
        match uninit.try_init().map_err(invalid)? {
            InitializeResult::Initialized(image) => return Ok((image, bytes.len() - rest.len())),
            InitializeResult::NeedMoreData(u) => uninit = u,
        }
    }
    Err(invalid("the file ends before the header is complete"))
}

/// Interleaved premultiplied f32 samples (colour..., alpha last) to straight u8: divide by alpha at float precision
/// (alpha 0 gives colour 0), then round like jxl-oxide's own u8 output.
#[cfg(feature = "jxl")]
fn straighten(f: &[f32], sc: usize) -> Vec<u8> {
    let q = |v: f32| (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
    let mut out = Vec::with_capacity(f.len());
    for px in f.chunks_exact(sc) {
        let a = px[sc - 1];
        for &c in &px[..sc - 1] {
            out.push(if a > 0.0 { q(c / a) } else { 0 });
        }
        out.push(q(a));
    }
    out
}

/// Extracts the bare codestream, so the container is never handed to the decoder (its box handling, such as inflating `brob`
/// boxes, is outside the allocation cap). A bare codestream is borrowed. In a container the boxes are walked from the start;
/// `jxlc` is the codestream, `jxlp` parts are joined in file order up to the last one, and every other box is skipped
/// without reading its payload. After a `jxlp` part flagged last the walk stops, so later boxes (even a `jxlc`) are not read.
/// A box that runs past the end is cut there (the decoder then sees a cut codestream).
#[cfg(feature = "jxl")]
fn codestream(bytes: &[u8]) -> Result<Cow<'_, [u8]>, Error> {
    if bytes.starts_with(&[0xFF, 0x0A]) {
        return Ok(Cow::Borrowed(bytes));
    }
    let mut pos = 0usize;
    let mut whole: Option<&[u8]> = None;
    let mut parts: Option<Vec<u8>> = None;
    while let Some(rest) = bytes.get(pos..).filter(|r| r.len() >= 8) {
        let size = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]);
        let kind = &rest[4..8];
        let (head, total) = match size {
            0 => (8, rest.len()),
            1 => {
                let Some(big) = rest.get(8..16) else { break };
                let big = u64::from_be_bytes(big.try_into().map_err(|_| invalid("bad box"))?);
                (16, usize::try_from(big).unwrap_or(usize::MAX))
            }
            n => (8, n as usize),
        };
        if total < head {
            return Err(invalid("a box is smaller than its header"));
        }
        let total = total.min(rest.len());
        let payload = &rest[head..total];
        match kind {
            b"jxlc" => {
                if whole.is_some() {
                    return Err(invalid("two jxlc boxes"));
                }
                whole = Some(payload);
            }
            b"jxlp" => {
                let Some((index, part)) = payload.split_first_chunk::<4>() else {
                    return Err(invalid("a jxlp box is too short"));
                };
                parts.get_or_insert_with(Vec::new).extend_from_slice(part);
                if u32::from_be_bytes(*index) >> 31 != 0 {
                    break;
                }
            }
            _ => {}
        }
        pos += total;
    }
    let out = match (whole, parts) {
        (Some(_), Some(_)) => return Err(invalid("both jxlc and jxlp boxes")),
        (Some(w), None) => Cow::Borrowed(w),
        (None, Some(p)) => Cow::Owned(p),
        (None, None) => return Err(invalid("no codestream in the container")),
    };
    if !out.starts_with(&[0xFF, 0x0A]) {
        return Err(invalid("the codestream does not start with FF 0A"));
    }
    Ok(out)
}

#[cfg(feature = "jxl")]
fn decode(bytes: &[u8], max_pixels: u64) -> Result<Rgba, Error> {
    use jxl_oxide::{AllocTracker, EnumColourEncoding, PixelFormat, RenderingIntent};

    if !is_jxl(bytes) {
        return Err(invalid("not a JPEG XL file"));
    }
    let stream = codestream(bytes)?;
    let bytes: &[u8] = &stream;
    // Pass 1: the header only, to learn the size (no limit needed: no frame data is decoded).
    let (w, h, channels) = {
        let (probe, _) = parse_header(bytes, None)?;
        (probe.width(), probe.height(), 3 + probe.image_header().metadata.ec_info.len())
    };
    if w == 0 || h == 0 {
        return Err(invalid("the picture is empty"));
    }
    if (w as u64) * (h as u64) > max_pixels.min(crate::dib::MAX_PIXELS) {
        return Err(Error::TooLarge(w, h));
    }

    // Pass 2: the real decoder, with an allocation cap from the header.
    let (mut image, taken) = parse_header(bytes, Some(AllocTracker::with_limit(limit_for(w, h, channels))))?;
    // Feed until keyframe 0 is complete; later frames of an animation are not decoded.
    let mut rest = &bytes[taken..];
    while image.num_loaded_keyframes() == 0 && !rest.is_empty() {
        let used = image.feed_bytes(&rest[..rest.len().min(64 << 10)]).map_err(invalid)?;
        if used == 0 {
            break;
        }
        rest = &rest[used..];
    }
    if image.num_loaded_keyframes() == 0 {
        return Err(invalid("the file has no complete picture (cut short?)"));
    }

    let grey = image.image_header().metadata.grayscale();
    let intent = RenderingIntent::Relative;
    image.request_color_encoding(if grey { EnumColourEncoding::gray_srgb(intent) } else { EnumColourEncoding::srgb(intent) });
    let format = image.pixel_format();
    let associated = image.image_header().metadata.ec_info.iter().find_map(|ec| ec.alpha_associated()).unwrap_or(false);

    let render = image.render_frame(0).map_err(invalid)?;
    // The stream applies the orientation, so its size is the upright size.
    let mut stream = render.stream();
    let (sw, sh, sc) = (stream.width(), stream.height(), stream.channels() as usize);
    if (sw, sh) != (w, h) {
        return Err(invalid("the frame size differs from the image size"));
    }
    let want = match format {
        PixelFormat::Gray => 1,
        PixelFormat::Graya => 2,
        PixelFormat::Rgb => 3,
        PixelFormat::Rgba => 4,
        // Cannot happen: we always request sRGB (or sRGB grey), never CMYK, so the decoder converts CMYK through its profile.
        PixelFormat::Cmyk | PixelFormat::Cmyka => return Err(Error::Unsupported("JXL: CMYK".into())),
    };
    if sc != want {
        return Err(invalid(format!("unexpected channel count {sc}")));
    }
    let count = (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(sc)).ok_or(Error::TooLarge(w, h))?;
    let samples: Vec<u8> = if associated && (sc == 2 || sc == 4) {
        // Premultiplied alpha: the stream copies it as is, so divide at float precision before quantizing.
        let mut f = vec![0f32; count];
        if stream.write_to_buffer(&mut f) != count {
            return Err(invalid("the decoder gave fewer samples than the picture needs"));
        }
        straighten(&f, sc)
    } else {
        let mut u = vec![0u8; count];
        if stream.write_to_buffer(&mut u) != count {
            return Err(invalid("the decoder gave fewer samples than the picture needs"));
        }
        u
    };
    let mut pixels = Vec::with_capacity(count / sc * 4);
    match sc {
        1 => pixels.extend(samples.iter().flat_map(|&g| [g, g, g, 255])),
        2 => pixels.extend(samples.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]])),
        3 => pixels.extend(samples.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255])),
        _ => pixels = samples,
    }
    Rgba::new(w, h, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_jxl_signatures() {
        assert!(is_jxl(&[0xFF, 0x0A]));
        assert!(is_jxl(&[0xFF, 0x0A, 1, 2, 3]));
        assert!(is_jxl(&CONTAINER));
        let mut long = CONTAINER.to_vec();
        long.extend([9, 9, 9]);
        assert!(is_jxl(&long));
        assert!(!is_jxl(&[]));
        assert!(!is_jxl(&[0xFF]));
        assert!(!is_jxl(&[0xFF, 0x0B]));
        assert!(!is_jxl(&CONTAINER[..11]));
        assert!(!is_jxl(b"\x89PNG\r\n\x1a\n"));
    }
}

#[cfg(all(test, feature = "jxl"))]
mod decode_tests {
    use super::*;
    use crate::decode_png;

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        v
    }

    fn container(boxes: &[Vec<u8>]) -> Vec<u8> {
        let mut v = CONTAINER.to_vec();
        v.extend(bx(b"ftyp", b"jxl \0\0\0\0jxl "));
        for b in boxes {
            v.extend_from_slice(b);
        }
        v
    }

    fn same(a: &Rgba, b: &Rgba) -> bool {
        a.width == b.width && a.height == b.height && a.pixels == b.pixels
    }

    #[test]
    fn container_jxlc_matches_bare() {
        let bare = decode_jxl(LOSSLESS, BIG).unwrap();
        let c = container(&[bx(b"jxlc", LOSSLESS)]);
        assert!(same(&bare, &decode_jxl(&c, BIG).unwrap()));
    }

    #[test]
    fn container_jxlp_parts_match_bare() {
        let bare = decode_jxl(LOSSLESS, BIG).unwrap();
        let (a, rest) = LOSSLESS.split_at(10);
        let (b, c) = rest.split_at(20);
        let part = |i: u32, last: bool, d: &[u8]| {
            let mut p = (i | if last { 1 << 31 } else { 0 }).to_be_bytes().to_vec();
            p.extend_from_slice(d);
            bx(b"jxlp", &p)
        };
        let cont = container(&[part(0, false, a), part(1, false, b), part(2, true, c)]);
        assert!(same(&bare, &decode_jxl(&cont, BIG).unwrap()));
    }

    #[test]
    fn aux_boxes_are_skipped_even_with_garbage_brob() {
        let bare = decode_jxl(LOSSLESS, BIG).unwrap();
        let mut brob = b"Exif".to_vec();
        brob.extend_from_slice(&[0x55; 64]);
        let c = container(&[bx(b"Exif", b"\0\0\0\0II*\0"), bx(b"brob", &brob), bx(b"jxlc", LOSSLESS)]);
        assert!(same(&bare, &decode_jxl(&c, BIG).unwrap()));
    }

    #[test]
    fn jxlc_size_zero_and_largesize_decode() {
        let bare = decode_jxl(LOSSLESS, BIG).unwrap();
        let mut zero = container(&[]);
        zero.extend_from_slice(&0u32.to_be_bytes());
        zero.extend_from_slice(b"jxlc");
        zero.extend_from_slice(LOSSLESS);
        assert!(same(&bare, &decode_jxl(&zero, BIG).unwrap()));
        let mut big = container(&[]);
        big.extend_from_slice(&1u32.to_be_bytes());
        big.extend_from_slice(b"jxlc");
        big.extend_from_slice(&((LOSSLESS.len() + 16) as u64).to_be_bytes());
        big.extend_from_slice(LOSSLESS);
        assert!(same(&bare, &decode_jxl(&big, BIG).unwrap()));
    }

    #[test]
    fn bad_containers_are_invalid() {
        let mut jxlp = 0x8000_0000u32.to_be_bytes().to_vec();
        jxlp.extend_from_slice(LOSSLESS);
        let both = container(&[bx(b"jxlc", LOSSLESS), bx(b"jxlp", &jxlp)]);
        let none = container(&[]);
        let mut tiny = container(&[]);
        tiny.extend_from_slice(&4u32.to_be_bytes());
        tiny.extend_from_slice(b"free");
        let not_stream = container(&[bx(b"jxlc", b"hello")]);
        for bad in [both, none, tiny, not_stream] {
            assert!(matches!(decode_jxl(&bad, BIG), Err(Error::Invalid(_))));
        }
    }

    #[test]
    fn cut_container_never_panics() {
        let c = container(&[bx(b"jxlc", LOSSLESS)]);
        for n in (0..c.len()).step_by(5) {
            assert!(decode_jxl(&c[..n], BIG).is_err(), "cut at {n}");
        }
    }

    const RGB_PNG: &[u8] = include_bytes!("../testdata/rgb.png");
    const RGBA_PNG: &[u8] = include_bytes!("../testdata/rgba.png");
    const GRAY_PNG: &[u8] = include_bytes!("../testdata/gray.png");
    const LOSSLESS: &[u8] = include_bytes!("../testdata/lossless.jxl");
    const LOSSY: &[u8] = include_bytes!("../testdata/lossy.jxl");
    const ALPHA: &[u8] = include_bytes!("../testdata/alpha.jxl");
    const ANIM: &[u8] = include_bytes!("../testdata/anim.jxl");
    const DEEP16: &[u8] = include_bytes!("../testdata/deep16.jxl");
    const GRAY: &[u8] = include_bytes!("../testdata/gray.jxl");
    const ORIENT6: &[u8] = include_bytes!("../testdata/orient6.jxl");
    const ICC: &[u8] = include_bytes!("../testdata/icc.jxl");
    /// Tolerance of `embedded_icc_decodes` against the computed P3 -> sRGB values (measured: 0).
    const ICC_BOUND: u8 = 2;
    const BIG: u64 = 1 << 20;

    fn px(img: &Rgba, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.width + x) * 4) as usize;
        img.pixels[i..i + 4].try_into().unwrap()
    }

    fn mean_abs_err(a: &Rgba, b: &Rgba) -> f64 {
        assert_eq!((a.width, a.height), (b.width, b.height));
        let sum: u64 = a.pixels.iter().zip(&b.pixels).map(|(x, y)| x.abs_diff(*y) as u64).sum();
        sum as f64 / a.pixels.len() as f64
    }

    #[test]
    fn lossless_rgb_is_exact() {
        let src = decode_png(RGB_PNG).unwrap();
        let got = decode_jxl(LOSSLESS, BIG).unwrap();
        assert_eq!((got.width, got.height), (16, 8));
        assert_eq!(got.pixels, src.pixels);
    }

    #[test]
    fn lossy_is_close_and_opaque() {
        let src = decode_png(RGB_PNG).unwrap();
        let got = decode_jxl(LOSSY, BIG).unwrap();
        assert_eq!((got.width, got.height), (src.width, src.height));
        assert!(got.is_opaque());
        let err = mean_abs_err(&got, &src);
        eprintln!("lossy mean abs error {err}");
        assert!(err < 6.0, "mean abs error {err}");
    }

    #[test]
    fn lossless_alpha_is_exact_and_straight() {
        let src = decode_png(RGBA_PNG).unwrap();
        assert!(!src.is_opaque());
        let got = decode_jxl(ALPHA, BIG).unwrap();
        assert_eq!(got.pixels, src.pixels);
    }

    #[test]
    fn animation_gives_the_first_frame() {
        let got = decode_jxl(ANIM, BIG).unwrap();
        assert_eq!((got.width, got.height), (16, 8));
        for p in got.pixels.chunks_exact(4) {
            assert_eq!(p, [255, 0, 0, 255]);
        }
    }

    #[test]
    fn sixteen_bit_is_within_one() {
        let src = decode_png(RGB_PNG).unwrap();
        let got = decode_jxl(DEEP16, BIG).unwrap();
        assert_eq!((got.width, got.height), (src.width, src.height));
        assert!(got.pixels.iter().zip(&src.pixels).all(|(a, b)| a.abs_diff(*b) <= 1));
    }

    #[test]
    fn grayscale_fills_all_three() {
        let src = decode_png(GRAY_PNG).unwrap();
        let got = decode_jxl(GRAY, BIG).unwrap();
        assert_eq!(got.pixels, src.pixels);
        assert!(got.pixels.chunks_exact(4).all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255));
    }

    #[test]
    fn orientation_is_applied() {
        let src = decode_png(RGB_PNG).unwrap();
        let got = decode_jxl(ORIENT6, BIG).unwrap();
        assert_eq!((got.width, got.height), (8, 16));
        // Orientation 6 turns the picture 90 degrees clockwise: out(x, y) = src(y, H - 1 - x).
        for y in 0..got.height {
            for x in 0..got.width {
                assert_eq!(px(&got, x, y), px(&src, y, src.height - 1 - x), "at {x},{y}");
            }
        }
    }

    #[test]
    fn embedded_icc_decodes() {
        let (probe, _) = parse_header(ICC, None).unwrap();
        assert!(probe.original_icc().is_some(), "the fixture must carry an ICC profile");
        let src = decode_png(RGB_PNG).unwrap();
        let got = decode_jxl(ICC, BIG).unwrap();
        assert_eq!((got.width, got.height), (src.width, src.height));
        // The fixture is the source pixels tagged with a Display-P3 profile with an sRGB-shaped curve: expect
        // sRGB-decode, linear P3 -> sRGB matrix, clamp, sRGB-encode.
        const M: [[f64; 3]; 3] = [[1.2249401, -0.2249404, 0.0], [-0.0420569, 1.0420571, 0.0], [-0.0196376, -0.0786361, 1.0982735]];
        let lin = |v: u8| {
            let c = v as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let enc = |c: f64| {
            let c = c.clamp(0.0, 1.0);
            let e = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
            (e * 255.0).round() as u8
        };
        let (mut max_diff, mut max_move) = (0u8, 0u8);
        for (g, s) in got.pixels.chunks(4).zip(src.pixels.chunks(4)) {
            let l = [lin(s[0]), lin(s[1]), lin(s[2])];
            for k in 0..3 {
                let want = enc(M[k][0] * l[0] + M[k][1] * l[1] + M[k][2] * l[2]);
                max_diff = max_diff.max(g[k].abs_diff(want));
                max_move = max_move.max(want.abs_diff(s[k]));
            }
            assert_eq!(g[3], 255);
        }
        eprintln!("icc max diff from expected {max_diff}, expected vs raw source {max_move}");
        assert!(max_diff <= ICC_BOUND, "max diff {max_diff}");
        assert!(max_move >= 20, "the fixture must exercise the conversion (moved {max_move})");
    }

    #[test]
    fn premultiplied_alpha_is_made_straight() {
        // 50% alpha, colour 0.25 premultiplied is 0.5 straight; alpha 0 gives black; opaque stays.
        let got = straighten(&[0.25, 0.1, 0.0, 0.5, 0.3, 0.3, 0.3, 0.0, 1.0, 0.5, 0.0, 1.0], 4);
        assert_eq!(got, [128, 51, 0, 128, 0, 0, 0, 0, 255, 128, 0, 255]);
        assert_eq!(straighten(&[0.5, 1.0, 0.25, 0.5], 2), [128, 255, 128, 128]);
    }

    #[test]
    fn too_large_is_refused_before_decoding() {
        assert!(matches!(decode_jxl(LOSSLESS, 16 * 8 - 1), Err(Error::TooLarge(16, 8))));
        assert!(decode_jxl(LOSSLESS, 16 * 8).is_ok());
    }

    #[test]
    fn bad_input_is_invalid_and_never_panics() {
        // These call the inner `decode`, which has no catch_unwind: a panic in our code or in jxl-oxide fails the test.
        let invalid = |r: Result<Rgba, Error>| matches!(r, Err(Error::Invalid(_)));
        assert!(invalid(decode(&[], BIG)));
        assert!(invalid(decode(&[0xFF, 0x0A], BIG)));
        assert!(invalid(decode(b"not a jxl file at all", BIG)));
        // The public path too.
        assert!(invalid(decode_jxl(&[], BIG)));
        assert!(invalid(decode_jxl(&LOSSLESS[..LOSSLESS.len() - 1], BIG)));
        for n in (0..LOSSLESS.len()).step_by(7).chain([LOSSLESS.len() - 1]) {
            assert!(invalid(decode(&LOSSLESS[..n], BIG)), "cut at {n}");
        }
        for n in (0..ALPHA.len()).step_by(7) {
            assert!(invalid(decode(&ALPHA[..n], BIG)), "alpha cut at {n}");
        }
        // Cut animations and ICC files: any result but a panic (a cut after frame 0 may still preview).
        for n in (0..ANIM.len()).step_by(11) {
            let _ = decode(&ANIM[..n], BIG);
        }
        for n in (0..ICC.len()).step_by(11) {
            let _ = decode(&ICC[..n], BIG);
        }
        for seed in 0u32..40 {
            let mut v = vec![0xFF, 0x0A];
            let mut x = seed.wrapping_mul(2654435761).wrapping_add(1);
            v.extend((0..200).map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 24) as u8
            }));
            let _ = decode(&v, BIG);
            let mut c = CONTAINER.to_vec();
            c.extend(&v[2..]);
            let _ = decode(&c, BIG);
        }
    }

    #[test]
    fn animation_cut_after_frame_zero_still_previews() {
        // Frame 0 of the fixture is complete well before the end, so cutting the last bytes still previews it.
        let got = decode_jxl(&ANIM[..ANIM.len() - 4], BIG).unwrap();
        assert_eq!(px(&got, 0, 0), [255, 0, 0, 255]);
    }
}
