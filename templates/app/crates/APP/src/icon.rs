//! The app's icon, drawn at any size so no image file is carried. A plain
//! rounded tile for now: draw the real one here (see minato's `icon.rs`).

/// The icon as RGBA, `size` x `size`.
pub fn pixels(size: u32) -> Vec<u8> {
    let n = size as usize;
    let (tile, rim) = ([0x16, 0x18, 0x1d], [0x6f, 0xd0, 0xd0]);
    let mut out = vec![0u8; n * n * 4];
    let r = size as f32 * 0.2;
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (cx, cy) = (fx.clamp(r, size as f32 - r), fy.clamp(r, size as f32 - r));
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            if d > r {
                continue;
            }
            let c = if d > r - size as f32 * 0.06 { rim } else { tile };
            out[(y * n + x) * 4..][..4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_square_rgba_with_clear_corners() {
        let p = pixels(32);
        assert_eq!(p.len(), 32 * 32 * 4);
        assert_eq!(p[3], 0);
        assert_eq!(p[(16 * 32 + 16) * 4 + 3], 255);
    }
}
