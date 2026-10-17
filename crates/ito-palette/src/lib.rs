//! The command palette (`Ctrl+Shift+P` in common-spec): one list of
//! things the app can do, narrowed by what is typed, drawn as a box at the
//! top of the window. It only finds; the app does what was picked.
//!
//! The app owns the list ([`Entry`], with its own type for what picking one
//! does), the colours ([`Style`]) and the words ([`Words`]); this crate owns
//! the matching ([`score`], [`search`]), the keys while it is open and the
//! drawing ([`show`]).

/// What an app's palette is made of, apart from the entries.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub panel: egui::Color32,
    pub border: egui::Color32,
    pub text: egui::Color32,
    pub dim: egui::Color32,
    pub selected_bg: egui::Color32,
    pub selected_text: egui::Color32,
    /// The letters of a title that matched ([`Entry::hits`]).
    pub hit: egui::Color32,
    /// A title with [`Tone::Accent`].
    pub accent: egui::Color32,
}

/// The palette's own words, in the app's language.
#[derive(Clone, Copy, Debug)]
pub struct Words<'a> {
    /// The search box's hint.
    pub hint: &'a str,
    /// Shown when nothing matches.
    pub none: &'a str,
    /// A line above the list, under the search box (empty for none).
    pub header: &'a str,
    /// A line under the list, such as the keys (empty for none).
    pub footer: &'a str,
}

/// How a line's title is inked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Normal,
    /// In [`Style::accent`]: a line that makes something ("Create …").
    Accent,
}

/// One line of the list: what it says, the smaller words after it, the key
/// that does the same (empty for none), and what the app does with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry<T> {
    pub title: String,
    pub detail: String,
    pub key: String,
    pub pick: T,
    /// Where in `title` (by char) the query matched, drawn in [`Style::hit`].
    /// The app fills it when it does the matching ([`View::filter`] off).
    pub hits: Vec<usize>,
    pub tone: Tone,
}

impl<T> Entry<T> {
    pub fn new(title: impl Into<String>, key: impl Into<String>, pick: T) -> Self {
        Self { title: title.into(), detail: String::new(), key: key.into(), pick, hits: Vec::new(), tone: Tone::Normal }
    }
}

/// The search box while it is open.
#[derive(Debug)]
pub struct View {
    pub query: String,
    /// The line picked with the arrows.
    pub selected: usize,
    /// Keys besides Enter that act on the selected line: a press answers
    /// [`Answer::Key`] with the line. `Modifiers::COMMAND` is Ctrl, or Cmd on
    /// macOS. Checked before Enter, so `Shift+Enter` can be one of them.
    pub keys: Vec<(egui::Modifiers, egui::Key)>,
    /// On (the default): [`show`] narrows and orders `entries` by the query
    /// ([`search`]). Off: the entries are shown as given, all of them, in
    /// their order, for an app that matches by its own rules and lays out its
    /// own lines (the query is still typed and kept in [`View::query`]).
    pub filter: bool,
    /// Opened this frame: the click that opened it is not a click outside.
    opening: bool,
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}

impl View {
    pub fn new() -> Self {
        Self { query: String::new(), selected: 0, keys: Vec::new(), filter: true, opening: true }
    }
}

/// What the palette was asked to do.
#[derive(Debug, PartialEq)]
pub enum Answer<T> {
    Pick(T),
    /// One of [`View::keys`] was pressed on the selected line. With no line
    /// to act on (nothing matches) the press is taken and nothing is answered.
    Key(egui::Modifiers, egui::Key, T),
    Close,
}

/// How well `query` matches `text`: its letters in order, case aside, or
/// `None`. Higher is better: letters next to each other, at the start of a
/// word, and an early start count.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let mut score = 0;
    let mut at = 0;
    let mut last: Option<usize> = None;
    for c in q {
        let found = t.get(at..)?.iter().position(|x| *x == c)? + at;
        score += 1;
        if last == Some(found.wrapping_sub(1)) {
            score += 5;
        }
        if found == 0 || !t[found - 1].is_alphanumeric() {
            score += 3;
        }
        if last.is_none() {
            score -= found.min(20) as i32 / 4;
        }
        last = Some(found);
        at = found + 1;
    }
    Some(score)
}

/// The entries that match, best first (by place in `entries`); ties keep
/// their order. Each is matched on its title and detail together.
pub fn search<T>(query: &str, entries: &[Entry<T>]) -> Vec<usize> {
    let mut found: Vec<(i32, usize)> =
        entries.iter().enumerate().filter_map(|(k, e)| score(query, &format!("{} {}", e.title, e.detail)).map(|s| (s, k))).collect();
    found.sort_by_key(|(s, k)| (std::cmp::Reverse(*s), *k));
    found.into_iter().map(|(_, k)| k).collect()
}

/// A title as one line, the chars at `hits` in `hit` and the rest in `ink`.
fn title_job(title: &str, hits: &[usize], font: egui::FontId, ink: egui::Color32, hit: egui::Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut run = String::new();
    let mut run_hit = false;
    let flush = |job: &mut egui::text::LayoutJob, run: &mut String, was: bool| {
        if !run.is_empty() {
            let color = if was { hit } else { ink };
            job.append(run, 0.0, egui::TextFormat::simple(font.clone(), color));
            run.clear();
        }
    };
    for (k, c) in title.chars().enumerate() {
        let h = hits.contains(&k);
        if h != run_hit {
            flush(&mut job, &mut run, run_hit);
            run_hit = h;
        }
        run.push(c);
    }
    flush(&mut job, &mut run, run_hit);
    job
}

const ROWS: usize = 10;
const WIDTH: f32 = 560.0;

/// Draw the box and take the keys while it is open: typing narrows,
/// Up/Down and Ctrl+P/N move, Enter picks, Esc or a click outside closes.
/// `entries` is what the app offers now; the answer is `None` while the
/// palette stays open.
pub fn show<T: Clone>(ctx: &egui::Context, view: &mut View, entries: &[Entry<T>], style: &Style, words: &Words) -> Option<Answer<T>> {
    let opening = std::mem::take(&mut view.opening);
    let hits = if view.filter { search(&view.query, entries) } else { (0..entries.len()).collect() };
    let last = hits.len().saturating_sub(1);
    view.selected = view.selected.min(last);

    let (extra, up, down, enter, esc) = ctx.input_mut(|i| {
        // First: `Shift+Enter` must not be taken for Enter.
        let extra = view.keys.iter().find(|&&(m, k)| i.consume_key(m, k)).copied();
        let mut ctrl = |k| i.consume_key(egui::Modifiers::CTRL, k);
        let up = ctrl(egui::Key::P) | ctrl(egui::Key::K);
        let down = ctrl(egui::Key::N) | ctrl(egui::Key::J);
        let mut plain = |k| i.consume_key(egui::Modifiers::NONE, k);
        (extra, up | plain(egui::Key::ArrowUp), down | plain(egui::Key::ArrowDown), plain(egui::Key::Enter), plain(egui::Key::Escape))
    });
    if esc {
        return Some(Answer::Close);
    }
    if let Some((m, k)) = extra {
        if let Some(&e) = hits.get(view.selected) {
            return Some(Answer::Key(m, k, entries[e].pick.clone()));
        }
    }
    if up {
        view.selected = view.selected.saturating_sub(1);
    }
    if down {
        view.selected = (view.selected + 1).min(last);
    }
    if enter {
        return Some(match hits.get(view.selected) {
            Some(&k) => Answer::Pick(entries[k].pick.clone()),
            None => Answer::Close,
        });
    }

    let mut answer = None;
    let screen = ctx.content_rect();
    let width = WIDTH.min(screen.width() - 32.0);
    let area = egui::Area::new(egui::Id::new("ito-palette"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(screen.center().x - width / 2.0 - 8.0, screen.top() + 40.0))
        .show(ctx, |ui| {
            egui::Frame::new().fill(style.panel).stroke(egui::Stroke::new(1.0, style.border)).corner_radius(6.0).inner_margin(8.0).show(
                ui,
                |ui| {
                    ui.set_width(width);
                    let before = view.query.clone();
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut view.query)
                            .id(egui::Id::new("ito-palette-query"))
                            .hint_text(words.hint)
                            .desired_width(f32::INFINITY)
                            .lock_focus(true),
                    );
                    // Only when it has lost it: asking again every frame ends an IME
                    // composition, so Japanese could not be typed here.
                    if !edit.has_focus() {
                        edit.request_focus();
                    }
                    if view.query != before {
                        view.selected = 0;
                    }
                    ui.add_space(4.0);
                    if !words.header.is_empty() {
                        ui.colored_label(style.dim, words.header);
                    }
                    if hits.is_empty() && !words.none.is_empty() {
                        ui.colored_label(style.dim, words.none);
                    }
                    // A window of ROWS lines that follows the selection.
                    let first = view.selected.saturating_sub(ROWS - 1).min(hits.len().saturating_sub(ROWS));
                    for (row, &k) in hits.iter().enumerate().skip(first).take(ROWS) {
                        let e = &entries[k];
                        let on = row == view.selected;
                        let (rect, resp) = ui
                            .allocate_exact_size(egui::vec2(ui.available_width(), ito_theme::zoomed(ui.ctx(), 22.0)), egui::Sense::click());
                        if on {
                            ui.painter().rect_filled(rect, 3.0, style.selected_bg);
                        }
                        let ink = match (on, e.tone) {
                            (true, _) => style.selected_text,
                            (false, Tone::Accent) => style.accent,
                            (false, Tone::Normal) => style.text,
                        };
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        let mid = rect.center().y;
                        // Nothing leaves the row: a long title ends in "…" so the
                        // detail and the key still show.
                        let p = ui.painter_at(rect);
                        let key = (!e.key.is_empty()).then(|| p.layout_no_wrap(e.key.clone(), font.clone(), style.dim));
                        let detail = (!e.detail.is_empty()).then(|| p.layout_no_wrap(e.detail.clone(), font.clone(), style.dim));
                        let key_w = key.as_ref().map_or(0.0, |g| g.size().x + 12.0);
                        let detail_w = detail.as_ref().map_or(0.0, |g| g.size().x + 8.0);
                        let room = (rect.width() - 12.0 - key_w - detail_w).max(rect.width() / 2.0);
                        let mut job = title_job(&e.title, &e.hits, font.clone(), ink, style.hit);
                        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
                        let title = p.layout_job(job);
                        let right = rect.left() + 6.0 + title.size().x;
                        p.galley(egui::pos2(rect.left() + 6.0, mid - title.size().y / 2.0), title, ink);
                        if let Some(g) = detail {
                            p.galley(egui::pos2(right + 8.0, mid - g.size().y / 2.0), g, style.dim);
                        }
                        if let Some(g) = key {
                            p.galley(egui::pos2(rect.right() - 6.0 - g.size().x, mid - g.size().y / 2.0), g, style.dim);
                        }
                        if resp.clicked() {
                            answer = Some(Answer::Pick(e.pick.clone()));
                        }
                        if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            view.selected = row;
                        }
                    }
                    if !words.footer.is_empty() {
                        ui.add_space(4.0);
                        ui.colored_label(style.dim, words.footer);
                    }
                },
            );
        });
    if answer.is_none() && !opening && ctx.input(|i| i.pointer.any_pressed()) {
        let outside = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| !area.response.rect.contains(p));
        if outside {
            answer = Some(Answer::Close);
        }
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, detail: &str) -> Entry<u8> {
        Entry { title: title.into(), detail: detail.into(), key: String::new(), pick: 0, hits: Vec::new(), tone: Tone::Normal }
    }

    #[test]
    fn letters_in_order_match() {
        assert!(score("spr", "Split right").is_some());
        assert!(score("SR", "split right").is_some(), "case aside");
        assert_eq!(score("rs", "split"), None, "out of order");
        assert_eq!(score("", "anything"), Some(0));
        assert_eq!(score("abc", "ab"), None, "ran out of text");
    }

    #[test]
    fn a_registered_key_answers_before_enter() {
        let ctx = egui::Context::default();
        let mut view = View::new();
        view.keys = vec![(egui::Modifiers::SHIFT, egui::Key::Enter), (egui::Modifiers::COMMAND, egui::Key::R)];
        let list = [entry("main", ""), entry("dev", "")];
        let style = Style {
            panel: egui::Color32::BLACK,
            border: egui::Color32::GRAY,
            text: egui::Color32::WHITE,
            dim: egui::Color32::GRAY,
            selected_bg: egui::Color32::BLUE,
            selected_text: egui::Color32::WHITE,
            hit: egui::Color32::YELLOW,
            accent: egui::Color32::GREEN,
        };
        let words = Words { hint: "", none: "", header: "", footer: "" };
        let press = |mods: egui::Modifiers, key: egui::Key| egui::RawInput {
            events: vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods }],
            ..Default::default()
        };
        let ask = |view: &mut View, raw| {
            let mut out = None;
            let mut full = ctx.run_ui(raw, |ui| out = show(ui.ctx(), view, &list, &style, &words));
            full.textures_delta.clear();
            out
        };
        assert_eq!(
            ask(&mut view, press(egui::Modifiers::SHIFT, egui::Key::Enter)),
            Some(Answer::Key(egui::Modifiers::SHIFT, egui::Key::Enter, 0))
        );
        assert_eq!(ask(&mut view, press(egui::Modifiers::NONE, egui::Key::Enter)), Some(Answer::Pick(0)));
        view.query = "zzz".into();
        assert_eq!(ask(&mut view, press(egui::Modifiers::COMMAND, egui::Key::R)), None, "no line, nothing answered");
    }

    #[test]
    fn the_better_match_comes_first() {
        let list = [entry("Split down", ""), entry("Settings", ""), entry("host", "web-1 · main")];
        let titles = |q: &str| search(q, &list).into_iter().map(|k| list[k].title.clone()).collect::<Vec<_>>();
        assert_eq!(titles("set"), ["Settings"]);
        // Together and at a word's start beats spread out.
        assert_eq!(titles("sd")[0], "Split down");
        // The detail counts too.
        assert_eq!(titles("main"), ["host"]);
        assert_eq!(titles("").len(), 3, "nothing typed, everything");
    }

    /// With the filter off the app's list is shown whole and in its order,
    /// whatever is typed, and Enter picks the selected line of it.
    #[test]
    fn without_the_filter_the_list_is_the_apps() {
        let ctx = egui::Context::default();
        let mut view = View::new();
        view.filter = false;
        view.query = "zzz".into();
        view.selected = 1;
        let mut create = entry("Create zzz", "");
        create.pick = 7;
        create.tone = Tone::Accent;
        create.hits = vec![0, 1];
        let list = [entry("main", ""), create];
        let style = Style {
            panel: egui::Color32::BLACK,
            border: egui::Color32::GRAY,
            text: egui::Color32::WHITE,
            dim: egui::Color32::GRAY,
            selected_bg: egui::Color32::BLUE,
            selected_text: egui::Color32::WHITE,
            hit: egui::Color32::YELLOW,
            accent: egui::Color32::GREEN,
        };
        let words = Words { hint: "", none: "", header: "head", footer: "foot" };
        let raw = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut out = None;
        let mut full = ctx.run_ui(raw, |ui| out = show(ui.ctx(), &mut view, &list, &style, &words));
        full.textures_delta.clear();
        assert_eq!(out, Some(Answer::Pick(7)));
    }

    #[test]
    fn a_title_is_split_at_its_hits() {
        let font = egui::FontId::default();
        let (ink, hit) = (egui::Color32::WHITE, egui::Color32::YELLOW);
        let job = title_job("abcd", &[1, 2], font, ink, hit);
        assert_eq!(job.text, "abcd");
        let colors: Vec<_> = job.sections.iter().map(|s| s.format.color).collect();
        assert_eq!(colors, [ink, hit, ink]);
    }
}
