//! What a picture was drawn with: the wgpu backend and the font set. `review::Env::of` has no `egui::Context`, so these are
//! kept process-wide, like the theme in `ito-theme`. That assumes one `egui::Context` per process, which every uchmk app has
//! (eframe makes one, and viewports share it).

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};

use egui::{FontData, FontDefinitions, FontFamily};

use crate::state::sha256_hex;

/// The font set last seen, and its hash.
struct Seen {
    fonts: Vec<(String, Arc<FontData>)>,
    families: BTreeMap<FontFamily, Vec<String>>,
    hash: String,
}

static BACKEND: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));
static FONTS: LazyLock<Mutex<Option<Seen>>> = LazyLock::new(|| Mutex::new(None));

/// The backend the app's renderer got, in wgpu's lowercase words (`Backend::to_str`: `gl`, `dx12`, `vulkan`, `metal`), the same
/// words `WGPU_BACKEND` takes. An app calls this once at startup, where it reads `cc.wgpu_render_state`.
pub fn set_wgpu_backend(name: &str) {
    *BACKEND.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.to_owned());
}

pub(crate) fn wgpu_backend() -> Option<String> {
    BACKEND.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub(crate) fn font_set_sha256() -> Option<String> {
    FONTS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.hash.clone())
}

/// Remember the fonts this context draws with. Cheap when they have not changed.
pub(crate) fn note_fonts(ctx: &egui::Context) {
    // `Context::fonts` panics before the first pass. A `<Shot:>` asked for during the very first pass (only reachable with
    // `<Now>`) therefore keeps `null`: "not known" is true there.
    if ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT) == 0 {
        return;
    }
    let changed = ctx.fonts(|f| {
        let defs = f.definitions();
        let seen = FONTS.lock().unwrap_or_else(|e| e.into_inner());
        match seen.as_ref() {
            Some(s) if same_fonts(s, defs) => None,
            _ => Some(defs.clone()),
        }
    });
    // Hash outside the `ctx.fonts` lock: a font set can be tens of MB.
    if let Some(defs) = changed {
        let hash = hash_font_set(&defs);
        let fonts = defs.font_data.iter().map(|(k, v)| (k.clone(), Arc::clone(v))).collect();
        *FONTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Seen { fonts, families: defs.families, hash });
    }
}

/// The same fonts, by `Arc` identity (comparing the bytes each pass would memcmp megabytes).
fn same_fonts(seen: &Seen, defs: &FontDefinitions) -> bool {
    seen.fonts.len() == defs.font_data.len()
        && seen.fonts.iter().zip(&defs.font_data).all(|((n, a), (m, b))| n == m && Arc::ptr_eq(a, b))
        && seen.families == defs.families
}

/// 64 lowercase hex digits. Every font is hashed, used by a family or not: a false "different" is safe, a false "same" is not.
fn hash_font_set(defs: &FontDefinitions) -> String {
    let mut text = String::from("ito-font-set 1\n");
    for (name, data) in &defs.font_data {
        text.push_str(&format!("font {name:?} {} {} {:?}\n", sha256_hex(&data.font), data.index, data.tweak));
    }
    for (family, names) in &defs.families {
        text.push_str(&format!("family {family:?} {names:?}\n"));
    }
    sha256_hex(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs(bytes: &[u8]) -> FontDefinitions {
        let mut d = FontDefinitions::empty();
        d.font_data.insert("a".into(), Arc::new(FontData::from_owned(bytes.to_vec())));
        d.font_data.insert("b".into(), Arc::new(FontData::from_owned(b"bbb".to_vec())));
        d.families.insert(FontFamily::Proportional, vec!["a".into(), "b".into()]);
        d
    }

    fn is_hex64(s: &str) -> bool {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    #[test]
    fn hash_is_hex_and_follows_content() {
        let base = hash_font_set(&defs(b"aaa"));
        assert!(is_hex64(&base));
        assert_eq!(base, hash_font_set(&defs(b"aaa")));
        assert_ne!(base, hash_font_set(&defs(b"aab")));

        let mut d = defs(b"aaa");
        Arc::make_mut(d.font_data.get_mut("a").unwrap()).index = 1;
        assert_ne!(base, hash_font_set(&d));

        let mut d = defs(b"aaa");
        Arc::make_mut(d.font_data.get_mut("a").unwrap()).tweak.scale = 1.5;
        assert_ne!(base, hash_font_set(&d));

        let mut d = defs(b"aaa");
        let a = d.font_data.remove("a").unwrap();
        d.font_data.insert("c".into(), a);
        d.families.insert(FontFamily::Proportional, vec!["c".into(), "b".into()]);
        assert_ne!(base, hash_font_set(&d));

        let mut d = defs(b"aaa");
        d.families.insert(FontFamily::Proportional, vec!["b".into(), "a".into()]);
        assert_ne!(base, hash_font_set(&d));
    }

    #[test]
    fn change_is_found_by_identity() {
        let d = defs(b"aaa");
        let seen = Seen {
            fonts: d.font_data.iter().map(|(k, v)| (k.clone(), Arc::clone(v))).collect(),
            families: d.families.clone(),
            hash: String::new(),
        };
        assert!(same_fonts(&seen, &d.clone()));
        let mut e = d.clone();
        e.font_data.insert("a".into(), Arc::new(FontData::from_owned(b"aaa".to_vec())));
        assert!(!same_fonts(&seen, &e));
    }

    #[test]
    fn note_fonts_before_a_pass_does_not_panic() {
        note_fonts(&egui::Context::default());
    }

    #[test]
    fn note_fonts_after_a_pass_gives_a_hash() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {}).drop_without_applying_deltas();
        note_fonts(&ctx);
        assert!(is_hex64(&font_set_sha256().expect("a hash")));
    }

    #[test]
    fn set_wgpu_backend_reaches_env() {
        set_wgpu_backend("gl");
        let env = crate::review::Env::of(&crate::review::Meta::default());
        assert_eq!(env.wgpu_backend.as_deref(), Some("gl"));
    }
}
