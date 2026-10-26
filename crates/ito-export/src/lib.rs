//! Pages given as SVG strings, out as PNG images, as one PDF, or to the printer.
//!
//! The app lays every page out itself (absolutely placed glyphs, lines,
//! rectangles) and hands each over as an SVG. Everything here blocks (reading
//! the system's fonts takes a moment, a PDF of a notebook longer): call it
//! from a worker thread, never the UI's.
//!
//! The PDF keeps text as text, with the fonts subset and embedded. SVG sizes
//! are CSS pixels (96 to the inch), so an 794 × 1123 page is A4.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// The fonts the SVGs name. Cheap to make; the system's fonts are read the
/// first time a page needs them, once per format, and kept.
pub struct Fonts {
    system: bool,
    files: Vec<PathBuf>,
    png: OnceLock<Arc<resvg::usvg::fontdb::Database>>,
    pdf: OnceLock<Arc<svg2pdf::usvg::fontdb::Database>>,
}

impl Fonts {
    /// The system's fonts.
    pub fn system() -> Fonts {
        Fonts { system: true, files: Vec::new(), png: OnceLock::new(), pdf: OnceLock::new() }
    }

    /// No font but the ones added with [`Fonts::with_file`] (for tests, and
    /// for an app that ships its own face).
    pub fn empty() -> Fonts {
        Fonts { system: false, ..Fonts::system() }
    }

    /// Also this font file (`.ttf`, `.otf`, `.ttc`); one that does not read is
    /// skipped when the fonts load.
    pub fn with_file(mut self, path: impl Into<PathBuf>) -> Fonts {
        self.files.push(path.into());
        self
    }
}

// resvg and svg2pdf each bring their own usvg, so their font databases are
// different types of the same shape: one macro fills either.
macro_rules! database {
    ($fonts:ident . $cell:ident, $db:ty) => {
        $fonts
            .$cell
            .get_or_init(|| {
                let mut db = <$db>::new();
                if $fonts.system {
                    db.load_system_fonts();
                }
                for f in &$fonts.files {
                    let _ = db.load_font_file(f);
                }
                Arc::new(db)
            })
            .clone()
    };
}

/// Page units to points: SVG pixels are 1/96 inch, PDF points 1/72.
const PT_PER_PX: f32 = 0.75;

/// One SVG page as a PNG image at `scale` times its size (2.0 for 192 dpi).
pub fn png_bytes(svg: &str, scale: f32, fonts: &Fonts) -> Result<Vec<u8>, String> {
    use resvg::{tiny_skia, usvg};
    if !(scale.is_finite() && scale > 0.0) {
        return Err(format!("bad scale {scale}"));
    }
    let opt = usvg::Options { fontdb: database!(fonts.png, usvg::fontdb::Database), ..Default::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("bad SVG: {e}"))?;
    let size = tree.size();
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| format!("a page of {w} x {h} pixels cannot be drawn"))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

/// [`png_bytes`] written to `out`.
pub fn png(svg: &str, scale: f32, fonts: &Fonts, out: &Path) -> Result<(), String> {
    let bytes = png_bytes(svg, scale, fonts)?;
    std::fs::write(out, bytes).map_err(|e| format!("{}: {e}", out.display()))
}

/// One PDF with a page per SVG, in order. Text stays text.
pub fn pdf_bytes(pages: &[String], fonts: &Fonts) -> Result<Vec<u8>, String> {
    use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref};
    use svg2pdf::usvg;
    if pages.is_empty() {
        return Err("no pages".into());
    }
    let opt = usvg::Options { fontdb: database!(fonts.pdf, usvg::fontdb::Database), ..Default::default() };
    let mut next = Ref::new(1);
    let (catalog, tree_id) = (next.bump(), next.bump());
    let mut pdf = Pdf::new();
    let mut page_ids = Vec::with_capacity(pages.len());
    for (i, svg) in pages.iter().enumerate() {
        let n = i + 1;
        let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("page {n}: bad SVG: {e}"))?;
        let (chunk, root) = svg2pdf::to_chunk(&tree, svg2pdf::ConversionOptions::default()).map_err(|e| format!("page {n}: {e}"))?;
        // Each page's objects are numbered from 1: move them behind the earlier pages'.
        let mut map = std::collections::HashMap::new();
        let chunk = chunk.renumber(|old| *map.entry(old).or_insert_with(|| next.bump()));
        let root = map[&root];
        pdf.extend(&chunk);
        let (id, content_id) = (next.bump(), next.bump());
        let (w, h) = (tree.size().width() * PT_PER_PX, tree.size().height() * PT_PER_PX);
        let mut page = pdf.page(id);
        page.parent(tree_id).media_box(Rect::new(0.0, 0.0, w, h)).contents(content_id);
        page.resources().x_objects().pair(Name(b"P"), root);
        page.finish();
        // The page's picture fills the unit square.
        let mut content = Content::new();
        content.transform([w, 0.0, 0.0, h, 0.0, 0.0]).x_object(Name(b"P"));
        pdf.stream(content_id, &content.finish());
        page_ids.push(id);
    }
    pdf.catalog(catalog).pages(tree_id);
    pdf.pages(tree_id).kids(page_ids.iter().copied()).count(page_ids.len() as i32);
    Ok(pdf.finish())
}

/// [`pdf_bytes`] written to `out`.
pub fn pdf(pages: &[String], fonts: &Fonts, out: &Path) -> Result<(), String> {
    let bytes = pdf_bytes(pages, fonts)?;
    std::fs::write(out, bytes).map_err(|e| format!("{}: {e}", out.display()))
}

/// Hand the PDF to the OS to print, and return once it is handed over (not
/// when the paper is out). Windows asks the shell for the `print` verb of the
/// program that opens PDFs; macOS and Linux send it to the default printer
/// with `lpr` and `lp`.
pub fn print(pdf: &Path) -> Result<(), String> {
    if !pdf.is_file() {
        return Err(format!("{}: no such file", pdf.display()));
    }
    #[cfg(windows)]
    {
        use windows::core::{w, HSTRING};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
        // SAFETY: both strings outlive the call, and no other pointer is passed.
        let r = unsafe { ShellExecuteW(None, w!("print"), &HSTRING::from(pdf.as_os_str()), None, None, SW_HIDE) };
        // At most 32 is an error code, not a handle.
        if r.0 as usize > 32 {
            Ok(())
        } else {
            Err(format!("the shell would not print {} (code {})", pdf.display(), r.0 as usize))
        }
    }
    #[cfg(not(windows))]
    {
        let tool = if cfg!(target_os = "macos") { "lpr" } else { "lp" };
        let out = std::process::Command::new(tool).arg(pdf).output().map_err(|e| format!("{tool}: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!("{tool}: {}", String::from_utf8_lossy(&out.stderr).trim()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resvg::tiny_skia::Pixmap;

    const PAGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect width="200" height="100" fill="#ffffff"/><rect x="10" y="10" width="50" height="30" fill="#ff0000"/></svg>"##;

    fn pixel(pm: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let p = pm.pixel(x, y).unwrap();
        [p.red(), p.green(), p.blue(), p.alpha()]
    }

    #[test]
    fn a_page_is_drawn_at_the_scale() {
        let fonts = Fonts::empty();
        let pm = Pixmap::decode_png(&png_bytes(PAGE, 2.0, &fonts).unwrap()).unwrap();
        assert_eq!((pm.width(), pm.height()), (400, 200));
        assert_eq!(pixel(&pm, 40, 40), [255, 0, 0, 255]);
        assert_eq!(pixel(&pm, 300, 150), [255, 255, 255, 255]);
        assert!(png_bytes(PAGE, 0.0, &fonts).is_err());
        assert!(png_bytes("not svg", 1.0, &fonts).unwrap_err().contains("bad SVG"));
    }

    #[test]
    fn a_pdf_has_a_page_per_svg_at_the_right_size() {
        let fonts = Fonts::empty();
        let two = vec![PAGE.to_string(), PAGE.replacen("width=\"200\"", "width=\"300\"", 1)];
        let bytes = pdf_bytes(&two, &fonts).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 2"), "{text}");
        // 200 x 100 px is 150 x 75 pt; the second page is 300 px wide: 225 pt.
        assert!(text.contains("/MediaBox [0 0 150 75]"), "{text}");
        assert!(text.contains("/MediaBox [0 0 225 75]"), "{text}");
        assert!(pdf_bytes(&[], &fonts).is_err());
        assert!(pdf_bytes(&["x".to_string()], &fonts).unwrap_err().starts_with("page 1"));
    }

    #[test]
    fn files_are_written_and_print_wants_a_file() {
        let dir = std::env::temp_dir().join(format!("ito-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (p, f) = (dir.join("a.png"), dir.join("a.pdf"));
        png(PAGE, 1.0, &Fonts::empty(), &p).unwrap();
        pdf(&[PAGE.to_string()], &Fonts::empty(), &f).unwrap();
        assert!(std::fs::metadata(&p).unwrap().len() > 0 && std::fs::metadata(&f).unwrap().len() > 0);
        assert!(print(&dir.join("missing.pdf")).unwrap_err().contains("no such file"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
