//! The Windows resource file (`.res`) that gives an `.exe` its icon and its
//! version information, written byte by byte, and the `build.rs` glue that
//! hands it to the linker.
//!
//! Windows' `rc.exe`, which crates such as `winresource` run, comes with the
//! Windows SDK and does not exist on a Linux or macOS host. The `.res` format
//! is small, and both `link.exe` and `lld-link` take a `.res` among their
//! inputs, so a `build.rs` can write one and pass it on: nothing to install,
//! and a cross build from another OS gets the icon too.
//!
//! In a `build.rs`: `ito_winres::emit("app", &[16, 32, 256], |side| rgba(side))`.
//! (or `ito_winres::emit_ico("app", include_bytes!("assets/icon.ico"))` for an
//! `.ico` file made already). It reads the name's version, description and target from Cargo's own
//! variables and does nothing for a target that is not MSVC.

/// For a `build.rs`: write `<name>.res` into `OUT_DIR` and link it, with the
/// icon drawn at each of `sides` by `rgba(side)` (side x side RGBA pixels) and
/// the package's version and description. Only for the MSVC targets, whose
/// linkers take a `.res` as it is; the GNU one would want an object first.
pub fn emit(name: &str, sides: &[u32], rgba: impl Fn(u32) -> Vec<u8>) {
    println!("cargo:rerun-if-changed=build.rs");
    if !msvc() {
        return;
    }
    let icons: Vec<(u32, Vec<u8>)> = sides.iter().map(|&side| (side, png(side, &rgba(side)))).collect();
    link(name, &icons);
}

/// [`emit`] for an icon that is already a file: the bytes of an `.ico` whose
/// images are all PNG (what most tools write for 32-bit images). Panics on
/// anything else, since the `.ico` is a file of the app's own.
pub fn emit_ico(name: &str, ico: &[u8]) {
    println!("cargo:rerun-if-changed=build.rs");
    if !msvc() {
        return;
    }
    link(name, &ico_images(ico).unwrap_or_else(|e| panic!("{name}'s icon: {e}")));
}

fn msvc() -> bool {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    var("CARGO_CFG_TARGET_OS") == "windows" && var("CARGO_CFG_TARGET_ENV") == "msvc"
}

fn link(name: &str, icons: &[(u32, Vec<u8>)]) {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let num = |k: &str| var(k).parse().unwrap_or(0);
    let text = var("CARGO_PKG_VERSION");
    let comments = var("CARGO_PKG_DESCRIPTION");
    let info = Info {
        version: [num("CARGO_PKG_VERSION_MAJOR"), num("CARGO_PKG_VERSION_MINOR"), num("CARGO_PKG_VERSION_PATCH")],
        text: &text,
        name,
        comments: &comments,
    };
    let path = std::path::PathBuf::from(var("OUT_DIR")).join(format!("{name}.res"));
    std::fs::write(&path, res(icons, &info)).unwrap_or_else(|e| panic!("writing {name}.res: {e}"));
    // Every linked target, the tests too.
    println!("cargo:rustc-link-arg={}", path.display());
}

/// The images of an `.ico` as (side, PNG): its directory is a 6-byte header
/// and 16 bytes per image, a side of 0 meaning 256.
fn ico_images(ico: &[u8]) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let at = |i: usize, n: usize| ico.get(i..i + n).ok_or_else(|| "cut short".to_owned());
    let head = at(0, 6)?;
    if head[..4] != [0, 0, 1, 0] {
        return Err("not an .ico".into());
    }
    let count = u16::from_le_bytes([head[4], head[5]]) as usize;
    let mut out = Vec::new();
    for i in 0..count {
        let d = at(6 + 16 * i, 16)?;
        let side = if d[0] == 0 { 256 } else { d[0] as u32 };
        let size = u32::from_le_bytes([d[8], d[9], d[10], d[11]]) as usize;
        let off = u32::from_le_bytes([d[12], d[13], d[14], d[15]]) as usize;
        let data = at(off, size)?;
        if !data.starts_with(b"\x89PNG") {
            return Err(format!("the {side}-pixel image is not a PNG"));
        }
        out.push((side, data.to_vec()));
    }
    if out.is_empty() {
        return Err("no images".into());
    }
    Ok(out)
}

/// `rgba` (side x side) as PNG.
fn png(side: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = ::png::Encoder::new(&mut out, side, side);
    enc.set_color(::png::ColorType::Rgba);
    enc.set_depth(::png::BitDepth::Eight);
    let mut w = enc.write_header().expect("PNG header");
    w.write_image_data(rgba).expect("PNG data");
    w.finish().expect("PNG end");
    out
}

/// The resource types used: `RT_ICON`, `RT_GROUP_ICON` and `RT_VERSION`.
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
/// US English and Unicode, as the version block's `040904B0` says.
const LANG: u16 = 0x0409;
const CODEPAGE: u16 = 1200;

/// What the version information says.
pub struct Info<'a> {
    /// Major, minor, patch.
    pub version: [u16; 3],
    /// The version as text, `0.29.0`.
    pub text: &'a str,
    /// The program's name, `app`: Task Manager lists the process by it.
    pub name: &'a str,
    /// A line on what it is.
    pub comments: &'a str,
}

/// The whole `.res`: the icon as one image per size (`side` in pixels and
/// the image as PNG), and the version block.
pub fn res(icons: &[(u32, Vec<u8>)], info: &Info) -> Vec<u8> {
    let mut out = Vec::new();
    // A `.res` opens with an empty entry, which marks the format.
    entry(&mut out, 0, 0, 0, 0, &[]);
    for (i, (_, png)) in icons.iter().enumerate() {
        entry(&mut out, RT_ICON, i as u16 + 1, 0x1010, LANG, png);
    }
    entry(&mut out, RT_GROUP_ICON, 1, 0x1030, LANG, &group(icons));
    entry(&mut out, RT_VERSION, 1, 0x0030, LANG, &version(info));
    out
}

/// One entry: a 32-byte header naming the type and the ID by number, the
/// data, and padding to four bytes.
fn entry(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, lang: u16, data: &[u8]) {
    u32le(out, data.len() as u32);
    u32le(out, 32);
    for w in [0xFFFF, kind, 0xFFFF, id] {
        u16le(out, w);
    }
    u32le(out, 0); // data version
    u16le(out, flags);
    u16le(out, lang);
    u32le(out, 0); // version
    u32le(out, 0); // characteristics
    out.extend_from_slice(data);
    pad(out);
}

/// The icon group (`GRPICONDIR`): one 14-byte line per image, pointing at
/// its `RT_ICON` by ID. A side of 256 is written as 0.
fn group(icons: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut g = Vec::new();
    for w in [0, 1, icons.len() as u16] {
        u16le(&mut g, w);
    }
    for (i, (side, png)) in icons.iter().enumerate() {
        let s = if *side >= 256 { 0 } else { *side as u8 };
        g.extend_from_slice(&[s, s, 0, 0]);
        u16le(&mut g, 1); // planes
        u16le(&mut g, 32); // bits per pixel
        u32le(&mut g, png.len() as u32);
        u16le(&mut g, i as u16 + 1);
    }
    g
}

/// `VS_VERSIONINFO`: the numbers Explorer compares (`VS_FIXEDFILEINFO`) and
/// the strings it shows under Details.
fn version(info: &Info) -> Vec<u8> {
    let [major, minor, patch] = info.version.map(u32::from);
    let mut fixed = Vec::new();
    for d in [
        0xFEEF_04BD,         // signature
        0x0001_0000,         // structure version
        major << 16 | minor, // file version
        patch << 16,         //
        major << 16 | minor, // product version
        patch << 16,         //
        0x3F,                // flags that are meaningful
        0,                   // flags: a release, not a debug build
        0x0004_0004,         // VOS_NT_WINDOWS32
        1,                   // VFT_APP
        0,                   // subtype
        0,                   // date
        0,                   //
    ] {
        u32le(&mut fixed, d);
    }
    let exe = format!("{}.exe", info.name);
    let strings: Vec<Vec<u8>> = [
        ("Comments", info.comments),
        ("FileDescription", info.name),
        ("FileVersion", info.text),
        ("InternalName", info.name),
        ("OriginalFilename", &exe),
        ("ProductName", info.name),
        ("ProductVersion", info.text),
    ]
    .iter()
    .map(|(k, v)| {
        let mut w = Vec::new();
        wide(&mut w, v);
        block(k, 1, &w, (w.len() / 2) as u16, &[])
    })
    .collect();
    let table = block(&format!("{LANG:04X}{CODEPAGE:04X}"), 1, &[], 0, &strings);
    let string_info = block("StringFileInfo", 1, &[], 0, &[table]);
    let mut lang = Vec::new();
    u16le(&mut lang, LANG);
    u16le(&mut lang, CODEPAGE);
    let var = block("Translation", 0, &lang, 4, &[]);
    let var_info = block("VarFileInfo", 1, &[], 0, &[var]);
    block("VS_VERSION_INFO", 0, &fixed, fixed.len() as u16, &[string_info, var_info])
}

/// One block of the version information: its length, the value's length (in
/// bytes for binary, in characters for text), the type (0 binary, 1 text),
/// the key, the value and the children, each starting on four bytes. The
/// length leaves out the padding after the last child.
fn block(key: &str, kind: u16, value: &[u8], value_len: u16, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0, 0];
    u16le(&mut b, value_len);
    u16le(&mut b, kind);
    wide(&mut b, key);
    pad(&mut b);
    b.extend_from_slice(value);
    for c in children {
        pad(&mut b);
        b.extend_from_slice(c);
    }
    let len = b.len() as u16;
    b[..2].copy_from_slice(&len.to_le_bytes());
    b
}

/// `s` as UTF-16 with its NUL.
fn wide(out: &mut Vec<u8>, s: &str) {
    for w in s.encode_utf16().chain([0]) {
        u16le(out, w);
    }
}

fn pad(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn u16le(out: &mut Vec<u8>, w: u16) {
    out.extend_from_slice(&w.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, d: u32) {
    out.extend_from_slice(&d.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }

    fn u32at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    /// The entries of a `.res`: (type, ID, language, data).
    fn entries(b: &[u8]) -> Vec<(u16, u16, u16, &[u8])> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let size = u32at(b, i) as usize;
            assert_eq!(u32at(b, i + 4), 32);
            assert_eq!((u16at(b, i + 8), u16at(b, i + 12)), (0xFFFF, 0xFFFF));
            let data = &b[i + 32..i + 32 + size];
            out.push((u16at(b, i + 10), u16at(b, i + 14), u16at(b, i + 22), data));
            i += 32 + size.next_multiple_of(4);
        }
        assert_eq!(i, b.len());
        out
    }

    /// A version block: (key, value, children), checking the lengths.
    fn walk(b: &[u8]) -> (String, &[u8], Vec<&[u8]>) {
        let len = u16at(b, 0) as usize;
        let (vlen, kind) = (u16at(b, 2) as usize, u16at(b, 4));
        let mut key = Vec::new();
        let mut i = 6;
        while u16at(b, i) != 0 {
            key.push(u16at(b, i));
            i += 2;
        }
        i = (i + 2).next_multiple_of(4);
        let vbytes = if kind == 1 { vlen * 2 } else { vlen };
        let value = &b[i..i + vbytes];
        i += vbytes;
        let mut children = Vec::new();
        while i.next_multiple_of(4) < len {
            i = i.next_multiple_of(4);
            let clen = u16at(b, i) as usize;
            children.push(&b[i..i + clen]);
            i += clen;
        }
        assert_eq!(i, len);
        (String::from_utf16(&key).unwrap(), value, children)
    }

    fn text(v: &[u8]) -> String {
        let w: Vec<u16> = v.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(w.last(), Some(&0));
        String::from_utf16(&w[..w.len() - 1]).unwrap()
    }

    fn sample() -> Vec<u8> {
        let icons = vec![(16, vec![1u8; 10]), (256, vec![2u8; 7])];
        let info = Info { version: [0, 29, 3], text: "0.29.3", name: "app", comments: "A monitor" };
        res(&icons, &info)
    }

    #[test]
    fn the_entries_are_the_marker_two_icons_their_group_and_the_version() {
        let res = sample();
        let e = entries(&res);
        let kinds: Vec<(u16, u16)> = e.iter().map(|&(k, id, _, _)| (k, id)).collect();
        assert_eq!(kinds, [(0, 0), (RT_ICON, 1), (RT_ICON, 2), (RT_GROUP_ICON, 1), (RT_VERSION, 1)]);
        assert!(e[0].3.is_empty());
        assert_eq!(e[2].3, [2u8; 7]);
        assert!(e[1..].iter().all(|&(_, _, lang, _)| lang == LANG));

        let g = e[3].3;
        assert_eq!([u16at(g, 0), u16at(g, 2), u16at(g, 4)], [0, 1, 2]);
        // The second line: 256 written as 0, 7 bytes, ID 2.
        let line = &g[6 + 14..6 + 28];
        assert_eq!(&line[..4], [0, 0, 0, 0]);
        assert_eq!((u16at(line, 6), u32at(line, 8), u16at(line, 12)), (32, 7, 2));
        assert_eq!(g[6], 16);
    }

    #[test]
    fn the_version_block_has_the_numbers_and_the_strings() {
        let res = sample();
        let e = entries(&res);
        let (key, fixed, children) = walk(e[4].3);
        assert_eq!(key, "VS_VERSION_INFO");
        assert_eq!(fixed.len(), 52);
        assert_eq!(u32at(fixed, 0), 0xFEEF_04BD);
        assert_eq!((u32at(fixed, 8), u32at(fixed, 12)), (29, 3 << 16));

        let (key, _, tables) = walk(children[0]);
        assert_eq!(key, "StringFileInfo");
        let (key, _, strings) = walk(tables[0]);
        assert_eq!(key, "040904B0");
        let pairs: Vec<(String, String)> = strings
            .iter()
            .map(|s| {
                let (k, v, _) = walk(s);
                (k, text(v))
            })
            .collect();
        let get = |k: &str| pairs.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("FileVersion"), Some("0.29.3"));
        assert_eq!(get("FileDescription"), Some("app"));
        assert_eq!(get("OriginalFilename"), Some("app.exe"));
        assert_eq!(get("Comments"), Some("A monitor"));

        let (key, _, vars) = walk(children[1]);
        assert_eq!(key, "VarFileInfo");
        let (key, lang, _) = walk(vars[0]);
        assert_eq!(key, "Translation");
        assert_eq!((u16at(lang, 0), u16at(lang, 2)), (LANG, CODEPAGE));
    }

    #[test]
    fn an_ico_gives_its_png_images_by_side() {
        let png = b"\x89PNG-data".to_vec();
        let mut ico = vec![0, 0, 1, 0, 2, 0];
        for (side, off) in [(16u8, 38u32), (0, 38)] {
            ico.extend_from_slice(&[side, side, 0, 0, 1, 0, 32, 0]);
            ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
            ico.extend_from_slice(&off.to_le_bytes());
        }
        ico.extend_from_slice(&png);
        let images = ico_images(&ico).unwrap();
        assert_eq!(images, [(16, png.clone()), (256, png)]);
        assert!(ico_images(&ico[..30]).is_err());
        assert!(ico_images(b"BM....").is_err());
        let mut bmp = ico.clone();
        bmp[38] = b'B';
        assert!(ico_images(&bmp).unwrap_err().contains("not a PNG"));
    }
}
