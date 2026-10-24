//! The first-run guide: what stops a beginner before the first use of an app
//! (a tool missing or too old, a setting empty), each with the commands to copy.
//!
//! What is wrong is the app's (michi: git, `user.name`); the app finds it off
//! the UI thread and builds a list of [`Problem`]s with its own (already
//! translated) texts. What is shared is the shape, the per-OS pick of a
//! command ([`Os`], [`Fix`]), the state of the panel ([`Guide`]: open, closed
//! by Esc or a click outside, brought back by the palette) and, with the `egui`
//! feature, the panel itself ([`show`]).
//!
//! The panel never blocks: it takes no keys, so work goes on under it. The app
//! runs no fix; the user copies it (a click on a command).

/// The OS a command is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }
}

/// One command to copy and why (both the app's text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub command: String,
    pub why: String,
}

impl Step {
    pub fn new(command: impl Into<String>, why: impl Into<String>) -> Step {
        Step { command: command.into(), why: why.into() }
    }
}

/// The same fix on each OS: `(command, why)` per OS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub windows: Step,
    pub mac: Step,
    pub linux: Step,
}

impl Fix {
    /// The step for `os`.
    pub fn on(&self, os: Os) -> &Step {
        match os {
            Os::Windows => &self.windows,
            Os::Mac => &self.mac,
            Os::Linux => &self.linux,
        }
    }
}

/// One thing to put right: a line saying what, and the steps that fix it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub what: String,
    pub steps: Vec<Step>,
}

impl Problem {
    pub fn new(what: impl Into<String>, steps: Vec<Step>) -> Problem {
        Problem { what: what.into(), steps }
    }

    /// A problem fixed by a different command on each OS, then `more` steps
    /// that are the same everywhere (a download page).
    pub fn per_os(what: impl Into<String>, fix: &Fix, os: Os, more: Vec<Step>) -> Problem {
        let mut steps = vec![fix.on(os).clone()];
        steps.extend(more);
        Problem::new(what, steps)
    }
}

/// How much of the app is shown (michi#115): `Basic` hides the advanced
/// parts, `Full` shows all. The guide itself does not use it; apps keep the
/// choice (`[view] level` in their own settings) and share the word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    Basic,
    #[default]
    Full,
}

impl Level {
    /// `basic` / `full` (any case); `None` for anything else.
    pub fn parse(s: &str) -> Option<Level> {
        match s.trim().to_ascii_lowercase().as_str() {
            "basic" => Some(Level::Basic),
            "full" => Some(Level::Full),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Basic => "basic",
            Level::Full => "full",
        }
    }
}

/// The state of the panel: the problems, and whether it is open.
#[derive(Clone, Debug, Default)]
pub struct Guide {
    problems: Vec<Problem>,
    open: bool,
}

impl Guide {
    /// A new answer of the check: opens when something is wrong, closes (and
    /// forgets) when nothing is.
    pub fn set(&mut self, problems: Vec<Problem>) {
        self.open = !problems.is_empty();
        self.problems = problems;
    }

    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    pub fn is_open(&self) -> bool {
        self.open && !self.problems.is_empty()
    }

    /// The palette's "open the guide": opens it if there is anything to say.
    /// Returns whether it is open now.
    pub fn reopen(&mut self) -> bool {
        self.open = !self.problems.is_empty();
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Esc: closes the panel and says so (`false` when it was not open, so the
    /// key goes on to whatever else wanted it).
    pub fn escape(&mut self) -> bool {
        let was = self.is_open();
        self.open = false;
        was
    }
}

/// The texts around the problems.
#[cfg(feature = "egui")]
#[derive(Clone, Copy, Debug)]
pub struct Words<'a> {
    pub title: &'a str,
    pub lead: &'a str,
    /// Lines under the problems (where to sign in, how to close it).
    pub footer: &'a [&'a str],
}

/// What the user did to the panel in a frame.
#[cfg(feature = "egui")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// A command was clicked: put it on the clipboard.
    pub copy: Option<String>,
}

/// The panel at the top of the window while the guide is open. A click on a
/// command copies it (returned in [`Outcome::copy`]); a click outside closes
/// it. Takes no keys.
#[cfg(feature = "egui")]
pub fn show(ctx: &egui::Context, guide: &mut Guide, words: &Words) -> Outcome {
    use egui::{pos2, Id, RichText, Sense, Stroke};
    let mut out = Outcome::default();
    if !guide.is_open() {
        return out;
    }
    let c = ito_theme::colors();
    let screen = ctx.content_rect();
    let width = 640.0_f32.min(screen.width() - 32.0);
    let area = egui::Area::new(Id::new("ito-guide"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(screen.center().x - width / 2.0 - 8.0, screen.top() + 40.0))
        .show(ctx, |ui| {
            egui::Frame::new().fill(c.panel).stroke(Stroke::new(1.0, c.border)).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(width);
                let small = egui::FontId::proportional(12.0);
                ui.label(RichText::new(words.title).strong());
                ui.label(RichText::new(words.lead).font(small.clone()).color(c.dim));
                for p in guide.problems() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&p.what).color(c.wait));
                    for step in &p.steps {
                        let resp = ui.add(egui::Label::new(RichText::new(&step.command).monospace()).sense(Sense::click()));
                        ui.label(RichText::new(&step.why).font(small.clone()).color(c.dim));
                        if resp.clicked() {
                            out.copy = Some(step.command.clone());
                        }
                    }
                }
                ui.add_space(6.0);
                for line in words.footer {
                    ui.label(RichText::new(*line).font(small.clone()).color(c.dim));
                }
            });
        });
    let outside = out.copy.is_none()
        && ctx.input(|i| i.pointer.any_pressed())
        && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|q| !area.response.rect.contains(q));
    if outside {
        guide.close();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix() -> Fix {
        Fix {
            windows: Step::new("winget install Git.Git", "winget"),
            mac: Step::new("brew install git", "brew"),
            linux: Step::new("sudo apt install git", "apt"),
        }
    }

    #[test]
    fn a_fix_picks_its_os_and_adds_the_common_steps() {
        let p = Problem::per_os("no git", &fix(), Os::Mac, vec![Step::new("https://git-scm.com/downloads", "download")]);
        assert_eq!(p.steps.len(), 2);
        assert_eq!(p.steps[0].command, "brew install git");
        assert_eq!(fix().on(Os::Windows).why, "winget");
        let _ = Os::current();
    }

    #[test]
    fn the_guide_opens_on_problems_and_esc_closes_it_once() {
        let mut g = Guide::default();
        g.set(vec![]);
        assert!(!g.is_open());
        assert!(!g.reopen());
        assert!(!g.escape());
        g.set(vec![Problem::new("x", vec![])]);
        assert!(g.is_open());
        assert!(g.escape());
        assert!(!g.is_open());
        assert!(!g.escape(), "the second Esc goes on to others");
        assert!(g.reopen());
        g.set(vec![]);
        assert!(!g.is_open());
    }

    #[test]
    fn the_level_round_trips() {
        assert_eq!(Level::parse(" Basic "), Some(Level::Basic));
        assert_eq!(Level::parse("full"), Some(Level::Full));
        assert_eq!(Level::parse("x"), None);
        assert_eq!(Level::default().as_str(), "full");
    }

    #[cfg(feature = "egui")]
    #[test]
    fn the_panel_draws_while_open_and_not_when_closed() {
        let ctx = egui::Context::default();
        let mut g = Guide::default();
        g.set(vec![Problem::new("no name", vec![Step::new("git config --global user.name X", "why")])]);
        let words = Words { title: "t", lead: "l", footer: &["f"] };
        ctx.run_ui(egui::RawInput::default(), |_| {
            assert_eq!(show(&ctx, &mut g, &words), Outcome::default());
        })
        .drop_without_applying_deltas();
        assert!(g.is_open());
        g.close();
        ctx.run_ui(egui::RawInput::default(), |_| {
            assert_eq!(show(&ctx, &mut g, &words), Outcome::default());
        })
        .drop_without_applying_deltas();
    }
}
