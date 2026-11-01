//! Small parts a mail-like app repeats: an avatar, a notice band, an attachment card and a pill button.

use egui::{Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};
use ito_a11y::Role as Said;
use ito_theme::mix;

use crate::mark::{paint_square, tone_look};
use crate::{button_with, focus_ring, mark, tag_hue, take_keys, z, Look, Mark, Tone};

/// Han, kana (with the prolonged-sound mark) or Hangul: a script where one character already stands for a word.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF
        | 0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F | 0xFF66..=0xFF9F | 0x20000..=0x2FA1F)
}

/// The letters that stand for `name`: one character if its first word is CJK ("佐藤 花子" is "佐"), else the first letters of
/// its first two words, or its first two characters.
fn monogram(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| c.is_whitespace() || c == '@' || c == '.').filter(|w| !w.is_empty()).collect();
    let up = |s: &str| s.chars().next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    match words.as_slice() {
        [] => "?".to_owned(),
        [first, ..] if first.chars().next().is_some_and(is_cjk) => up(first),
        [one] => one.chars().take(2).flat_map(char::to_uppercase).collect(),
        [a, b, ..] => up(a) + &up(b),
    }
}

/// A circle with the letters of `name`, coloured from a stable hash of `seed` (the address), `size` points across before the
/// letters' zoom (about 28, or 36 in a reader). Not clickable; the response is for placing.
pub fn avatar(ui: &mut Ui, l: Look, name: &str, seed: &str, size: f32) -> Response {
    let c = l.c;
    let size = z(ui, size);
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ito_a11y::say(&r, Said::Label, name);
    if ui.is_rect_visible(rect) {
        let hue = tag_hue(&c, seed);
        let p = ui.painter();
        p.circle_filled(rect.center(), size / 2.0, mix(c.side, hue, 0.35));
        p.circle_stroke(rect.center(), size / 2.0 - 0.5, Stroke::new(1.0, mix(c.side, hue, 0.6)));
        p.text(rect.center(), Align2::CENTER_CENTER, monogram(name), FontId::proportional(size * 0.4), c.strong());
    }
    r
}

/// A band across the column: `mark`, `text` and, if given, one action button at the right. Returns the action's response.
pub fn notice(ui: &mut Ui, l: Look, m: Mark, text: &str, action: Option<&str>) -> Option<Response> {
    let c = l.c;
    let mut out = None;
    egui::Frame::new()
        .fill(mix(c.side, c.run, 0.12))
        .stroke(Stroke::new(1.0, mix(c.border, c.run, 0.4)))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let glyph = z(ui, 14.0);
                let (rect, r) = ui.allocate_exact_size(Vec2::splat(glyph), Sense::hover());
                mark(ui.painter(), rect, m, c.run);
                ito_a11y::say(&r, Said::Label, "");
                if let Some(a) = action {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        out = Some(button_with(ui, l, a, Tone::Plain));
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(text).color(c.fg)).wrap());
                        });
                    });
                } else {
                    ui.add(egui::Label::new(egui::RichText::new(text).color(c.fg)).wrap());
                }
            });
        });
    out
}

/// What a click on an [`attachment`] card did.
#[derive(Debug)]
pub struct AttachmentResponse {
    /// The card (open it).
    pub open: Response,
    /// The download mark at the card's right end.
    pub download: Response,
}

/// The upper-case extension of `name` for a badge, at most four letters; "FILE" if it has none.
fn badge_text(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && ext.len() <= 4 => ext.to_uppercase(),
        _ => "FILE".to_owned(),
    }
}

/// A card for one attached file: a badge of its type from the extension, the name (cut with "…"), `size_text`, and a download
/// mark. The whole card is clickable (`open`); the mark is its own button (`download`), and `download_words` is its tooltip
/// and accessible name ("Download", "保存").
pub fn attachment(ui: &mut Ui, l: Look, name: &str, size_text: &str, download_words: &str) -> AttachmentResponse {
    let c = l.c;
    let size = Vec2::new(ui.available_width().min(z(ui, 260.0)), z(ui, 48.0));
    let (rect, open) = ui.allocate_exact_size(size, Sense::click());
    let open = open.on_hover_cursor(egui::CursorIcon::PointingHand);
    ito_a11y::say(&open, Said::Button, name);
    take_keys(ui, &open);
    let pad = z(ui, 8.0);
    let side = z(ui, 28.0);
    let square = Rect::from_min_size(Pos2::new(rect.right() - pad - side, rect.center().y - side / 2.0), Vec2::splat(side));
    // Registered after the card, so it is on top of it.
    let download = ui.interact(square, ui.id().with(("attachment-download", name)), Sense::click());
    let download = download.on_hover_text(download_words).on_hover_cursor(egui::CursorIcon::PointingHand);
    ito_a11y::say(&download, Said::Button, download_words);
    if ui.is_rect_visible(rect) {
        let hot = open.hovered() || open.has_focus();
        let (fill, edge, _, _) = tone_look(&c, Tone::Plain, hot);
        let p = ui.painter();
        p.rect(rect, 8.0, fill, Stroke::new(1.0, edge), StrokeKind::Inside);
        let badge = Rect::from_min_size(Pos2::new(rect.left() + pad, rect.center().y - side / 2.0), Vec2::splat(side));
        let hue = tag_hue(&c, &badge_text(name));
        p.rect(badge, 5.0, mix(c.side, hue, 0.3), Stroke::new(1.0, mix(c.side, hue, 0.6)), StrokeKind::Inside);
        p.text(badge.center(), Align2::CENTER_CENTER, badge_text(name), FontId::proportional(z(ui, 8.5)), c.strong());
        let x = badge.right() + z(ui, 8.0);
        let room = (square.left() - z(ui, 8.0) - x).max(0.0);
        let font = FontId::proportional(z(ui, 12.5));
        let width_of = |s: &str| p.layout_no_wrap(s.to_owned(), font.clone(), c.fg).size().x;
        let shown = crate::tag::fit(name, room, width_of);
        p.text(Pos2::new(x, rect.center().y - z(ui, 7.0)), Align2::LEFT_CENTER, shown, font, c.fg);
        p.text(Pos2::new(x, rect.center().y + z(ui, 8.0)), Align2::LEFT_CENTER, size_text, FontId::proportional(z(ui, 11.0)), c.dim);
        paint_square(ui, square, &download, Mark::Download);
    }
    focus_ring(ui, &open);
    AttachmentResponse { open, download }
}

/// A quiet, fully rounded button: `words` and, if given, a trailing mark (a chevron for a menu).
pub fn pill(ui: &mut Ui, l: Look, words: &str, trail: Option<Mark>) -> Response {
    let c = l.c;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(words.to_owned(), font, Color32::PLACEHOLDER);
    let (pad, gap, glyph) = (z(ui, 12.0), z(ui, 4.0), z(ui, 12.0));
    let mut w = pad * 2.0 + galley.size().x;
    if trail.is_some() {
        w += gap + glyph - pad * 0.3;
    }
    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, z(ui, 26.0)), Sense::click());
    let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
    ito_a11y::say(&r, Said::Button, words);
    take_keys(ui, &r);
    if ui.is_rect_visible(rect) {
        let (fill, edge, mark_ink, text_ink) = tone_look(&c, Tone::Plain, r.hovered() || r.has_focus());
        let p = ui.painter();
        p.rect(rect, rect.height() / 2.0, fill, Stroke::new(1.0, edge), StrokeKind::Inside);
        p.galley(Pos2::new(rect.left() + pad, rect.center().y - galley.size().y / 2.0), galley.clone(), text_ink);
        if let Some(m) = trail {
            let at = Pos2::new(rect.left() + pad + galley.size().x + gap + glyph / 2.0, rect.center().y);
            mark(p, Rect::from_center_size(at, Vec2::splat(glyph)), m, mark_ink);
        }
    }
    focus_ring(ui, &r);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monograms() {
        assert_eq!(monogram("田中 太郎"), "田");
        assert_eq!(monogram("佐藤 花子"), "佐");
        assert_eq!(monogram("ada 佐藤"), "A佐");
        assert_eq!(monogram("ada lovelace"), "AL");
        assert_eq!(monogram("zed"), "ZE");
        assert_eq!(monogram(""), "?");
        assert_eq!(badge_text("report.pdf"), "PDF");
        assert_eq!(badge_text("Makefile"), "FILE");
        assert_eq!(badge_text(".bashrc"), "FILE");
    }

    #[test]
    fn the_parts_draw() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let l = Look { c: ito_theme::colors(), q: "" };
            let a = avatar(ui, l, "田中", "tanaka@example.com", 28.0);
            assert!((a.rect.width() - 28.0).abs() < 0.5);
            assert!(notice(ui, l, Mark::Info, "Offline", Some("Retry")).is_some());
            assert!(notice(ui, l, Mark::Info, "Sent", None).is_none());
            let at = attachment(ui, l, "a-very-long-file-name-that-has-to-be-cut-somewhere.pdf", "1.2 MB", "保存");
            assert!(at.download.rect.width() > 0.0 && at.open.rect.contains(at.download.rect.center()));
            let short = pill(ui, l, "All", None).rect.width();
            let long = pill(ui, l, "All", Some(Mark::ChevronDown)).rect.width();
            assert!(long > short);
        });
        out.textures_delta.clear();
    }
}
