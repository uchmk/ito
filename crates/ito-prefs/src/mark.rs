//! Drawn marks (a magnifier, an arrow onto a tray, a play triangle ...) and the quiet outlined buttons built on them.
//!
//! The marks are strokes, not font glyphs, so they follow every theme and high contrast; the colours are the theme's
//! (`raised` / `border` / `hover` / `border_strong` / `fg` / `dim`, the focus ring `run`).

use crate::{focus_ring, take_keys, z, Tone};
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use ito_a11y::Role as Said;
use ito_theme::mix;
use std::f32::consts::PI;

/// A mark. Each is drawn in a 14 x 14 box, scaled to the rect it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A magnifying glass.
    Search,
    /// An arrow down onto a tray.
    Download,
    /// An arrow up from a tray.
    Upload,
    /// A triangle pointing on: start again.
    Play,
    /// Two bars: stop for now.
    Pause,
    /// A chevron pointing left (open a pane / back).
    ChevronLeft,
    /// A chevron pointing right (fold a pane / on).
    ChevronRight,
    Plus,
    /// A horizontal bar: less.
    Minus,
    /// A cross.
    Close,
    /// A circling arrow.
    Refresh,
    /// Two sheets, one over the other.
    Copy,
    /// A tray (a mailbox's inbox).
    Inbox,
    /// A five-point star, outlined.
    Star,
    /// A five-point star, filled.
    StarFilled,
    /// A flag on a pole.
    Flag,
    /// A clock face (snoozed).
    Clock,
    /// Gmail's importance marker: a tag with a notch.
    Important,
    /// A paper plane (sent).
    Send,
    /// A calendar page (scheduled).
    Calendar,
    /// A pencil (drafts).
    Pencil,
    /// An envelope (all mail).
    Mail,
    /// A box with a lid (archive).
    Archive,
    /// A triangle with "!" (spam, a warning).
    Warning,
    /// A bin.
    Trash,
    /// A tag with a hole (a label).
    Label,
    /// An arrow turning back.
    Reply,
    /// Two arrows turning back.
    ReplyAll,
    /// An arrow turning on.
    Forward,
    /// A paper clip (an attachment).
    Paperclip,
    /// Two chain links.
    Link,
    /// Three dots.
    More,
    /// Three bars, shorter and shorter.
    Sort,
    /// A tick.
    Check,
    ChevronDown,
    ChevronUp,
    /// A circle with an "i".
    Info,
    /// A bell.
    Bell,
    /// A bold "B".
    Bold,
    /// A slanted "I".
    Italic,
    /// A bar low in the box: shrink a window to its title.
    Minimize,
    /// Two corners pointing out: fill the window.
    Expand,
    /// Two corners pointing in: back from filling the window.
    Restore,
    /// A smiling face.
    Emoji,
    /// A "U" over a line.
    Underline,
    /// An "S" struck through.
    Strike,
    /// An "A" over a short line: text formatting.
    Format,
    /// A floppy disk.
    Save,
    /// A curved arrow turning back (counter-clockwise).
    Undo,
    /// A curved arrow turning on (clockwise).
    Redo,
    /// Boxes on a grid: tidy a layout.
    Arrange,
    /// A plain arrow down.
    ArrowDown,
    /// A plain arrow right.
    ArrowRight,
    /// A gear.
    Settings,
    /// A box with a prompt and a cursor.
    Terminal,
    /// Three nodes joined by lines.
    Flow,
    /// One line forking into two.
    Branch,
    /// Two lines joining into one.
    Merge,
    /// A page with a folded corner.
    File,
    /// Head and shoulders.
    Person,
    /// A folder.
    Folder,
    /// A globe.
    Globe,
    CalendarOff,
    CalendarCheck,
    /// A padlock.
    Lock,
    /// A square: stop for good (a kill), not a pause.
    Stop,
    /// A triangle with a bar after it: skip this one.
    Skip,
    /// A printer.
    Print,
    /// A circling arrow round a play triangle: run again (not a reload).
    Rerun,
    /// Two sheets side by side.
    Compare,
    /// An arrow from outside into an open box: bring in (the opposite of a download).
    Import,
    /// Three stacked boxes with a light each: a server (a machine you reach over the network).
    Server,
    /// A large letter H over two short lines: a heading.
    Heading,
    /// A heavy bar down the left of two lines of text: a quotation.
    Quote,
    /// Angle brackets round a slash: code.
    Code,
    /// Three lines, each led by a dot: a bulleted list.
    Bullets,
    /// Three lines, each led by a short stroke (a number): a numbered list.
    Numbers,
    /// Three lines, the first led by a checked box: a task list.
    Tasks,
    /// A screen on a stand: a PC (a Windows machine you reach, as against the rack of `Server`).
    Desktop,
    /// A stack of discs: a database.
    Database,
}

impl Mark {
    /// Every mark, for a gallery or a test.
    pub const ALL: [Mark; 80] = [
        Mark::Search,
        Mark::Download,
        Mark::Upload,
        Mark::Play,
        Mark::Pause,
        Mark::ChevronLeft,
        Mark::ChevronRight,
        Mark::Plus,
        Mark::Minus,
        Mark::Close,
        Mark::Refresh,
        Mark::Copy,
        Mark::Inbox,
        Mark::Star,
        Mark::StarFilled,
        Mark::Flag,
        Mark::Clock,
        Mark::Important,
        Mark::Send,
        Mark::Calendar,
        Mark::Pencil,
        Mark::Mail,
        Mark::Archive,
        Mark::Warning,
        Mark::Trash,
        Mark::Label,
        Mark::Reply,
        Mark::ReplyAll,
        Mark::Forward,
        Mark::Paperclip,
        Mark::Link,
        Mark::More,
        Mark::Sort,
        Mark::Check,
        Mark::ChevronDown,
        Mark::ChevronUp,
        Mark::Info,
        Mark::Bell,
        Mark::Bold,
        Mark::Italic,
        Mark::Minimize,
        Mark::Expand,
        Mark::Restore,
        Mark::Emoji,
        Mark::Underline,
        Mark::Strike,
        Mark::Format,
        Mark::Save,
        Mark::Undo,
        Mark::Redo,
        Mark::Arrange,
        Mark::ArrowDown,
        Mark::ArrowRight,
        Mark::Settings,
        Mark::Terminal,
        Mark::Flow,
        Mark::Branch,
        Mark::Merge,
        Mark::File,
        Mark::Person,
        Mark::Folder,
        Mark::Globe,
        Mark::CalendarOff,
        Mark::CalendarCheck,
        Mark::Lock,
        Mark::Stop,
        Mark::Skip,
        Mark::Print,
        Mark::Rerun,
        Mark::Compare,
        Mark::Import,
        Mark::Server,
        Mark::Heading,
        Mark::Quote,
        Mark::Code,
        Mark::Bullets,
        Mark::Numbers,
        Mark::Tasks,
        Mark::Desktop,
        Mark::Database,
    ];
}

/// The box a mark is designed in.
const BOX: f32 = 14.0;

/// Draw `mark` centred in `rect` (scaled by the smaller side over 14) with 1.5 strokes in `colour`.
pub fn mark(p: &Painter, rect: Rect, mark: Mark, colour: Color32) {
    let k = rect.width().min(rect.height()) / BOX;
    let m = rect.center();
    let at = |x: f32, y: f32| m + Vec2::new(x, y) * k;
    let line = Stroke::new(1.5 * k, colour);
    let path = |pts: Vec<Pos2>| Shape::line(pts, line);
    let closed = |pts: Vec<Pos2>| Shape::closed_line(pts, line);
    let pts = |v: &[(f32, f32)]| -> Vec<Pos2> { v.iter().map(|&(x, y)| at(x, y)).collect() };
    let arc = |cx: f32, cy: f32, r: f32, from: f32, to: f32| -> Vec<Pos2> {
        (0..=16).map(|i| from + (to - from) * i as f32 / 16.0).map(|a| at(cx + r * a.cos(), cy + r * a.sin())).collect()
    };
    let seg = |a: (f32, f32), b: (f32, f32)| p.line_segment([at(a.0, a.1), at(b.0, b.1)], line);
    let dot = |x: f32, y: f32, r: f32| p.circle_filled(at(x, y), r * k, colour);
    match mark {
        Mark::Search => {
            let o = at(-1.5, -1.5);
            p.circle_stroke(o, 4.5 * k, line);
            p.line_segment([o + Vec2::splat(3.3 * k), o + Vec2::splat(7.0 * k)], line);
        }
        Mark::Download | Mark::Upload => {
            let d = if mark == Mark::Download { 1.0 } else { -1.0 };
            // The tray is under the arrow for a download, over it for an upload.
            let (tip, tail) = (at(0.0, 2.5 * d), at(0.0, -6.0 * d));
            p.line_segment([tail, tip], line);
            p.add(path(vec![at(-3.5, -d), tip, at(3.5, -d)]));
            p.add(path(vec![at(-6.0, 3.0 * d), at(-6.0, 6.0 * d), at(6.0, 6.0 * d), at(6.0, 3.0 * d)]));
        }
        Mark::Play => {
            p.add(Shape::convex_polygon(vec![at(-3.5, -5.5), at(5.0, 0.0), at(-3.5, 5.5)], Color32::TRANSPARENT, line));
        }
        Mark::Pause => {
            for x in [-2.5, 2.5] {
                p.line_segment([at(x, -5.0), at(x, 5.0)], Stroke::new(2.0 * k, colour));
            }
        }
        Mark::ChevronLeft | Mark::ChevronRight => {
            let d = if mark == Mark::ChevronRight { 1.0 } else { -1.0 };
            p.add(path(vec![at(-2.5 * d, -4.5), at(2.5 * d, 0.0), at(-2.5 * d, 4.5)]));
        }
        Mark::Plus => {
            p.line_segment([at(-5.0, 0.0), at(5.0, 0.0)], line);
            p.line_segment([at(0.0, -5.0), at(0.0, 5.0)], line);
        }
        Mark::Minus => {
            p.line_segment([at(-5.0, 0.0), at(5.0, 0.0)], line);
        }
        Mark::Close => {
            p.line_segment([at(-4.5, -4.5), at(4.5, 4.5)], line);
            p.line_segment([at(4.5, -4.5), at(-4.5, 4.5)], line);
        }
        Mark::Refresh => {
            // Most of a circle, open at the upper right, with a head on the clockwise end.
            let (r, from, to) = (5.0, -0.4 * PI, 1.2 * PI);
            let arc: Vec<Pos2> = (0..=18).map(|i| from + (to - from) * i as f32 / 18.0).map(|a| at(r * a.cos(), r * a.sin())).collect();
            p.add(path(arc));
            let (tip, t, n) = (at(r * to.cos(), r * to.sin()), Vec2::new(-to.sin(), to.cos()), Vec2::new(to.cos(), to.sin()));
            p.add(path(vec![tip - t * 3.0 * k + n * 2.5 * k, tip, tip - t * 3.0 * k - n * 2.5 * k]));
        }
        Mark::Copy => {
            // The sheet behind shows only its top and left; the one in front is whole.
            p.add(path(vec![at(-2.0, 2.0), at(-6.0, 2.0), at(-6.0, -6.0), at(2.0, -6.0), at(2.0, -2.0)]));
            p.rect_stroke(Rect::from_min_max(at(-2.0, -2.0), at(6.0, 6.0)), 1.5 * k, line, StrokeKind::Middle);
        }
        Mark::Inbox => {
            p.add(closed(pts(&[(-6.0, 0.5), (-4.0, -5.0), (4.0, -5.0), (6.0, 0.5), (6.0, 5.5), (-6.0, 5.5)])));
            p.add(path(pts(&[(-6.0, 0.5), (-2.5, 0.5), (-1.5, 2.5), (1.5, 2.5), (2.5, 0.5), (6.0, 0.5)])));
        }
        Mark::Star | Mark::StarFilled => {
            let star: Vec<Pos2> = (0..10)
                .map(|i| {
                    let (r, a) = (if i % 2 == 0 { 6.8 } else { 2.9 }, -PI / 2.0 + i as f32 * PI / 5.0);
                    at(r * a.cos(), r * a.sin() + 0.5)
                })
                .collect();
            if mark == Mark::StarFilled {
                // A star is seen whole from its middle: a fan of triangles fills it.
                for i in 0..10 {
                    p.add(Shape::convex_polygon(vec![at(0.0, 0.5), star[i], star[(i + 1) % 10]], colour, Stroke::NONE));
                }
            }
            p.add(closed(star));
        }
        Mark::Flag => {
            seg((-4.5, -6.5), (-4.5, 6.5));
            p.add(path(pts(&[(-4.5, -5.5), (5.5, -5.5), (3.0, -2.5), (5.5, 0.5), (-4.5, 0.5)])));
        }
        Mark::Clock => {
            p.circle_stroke(m, 6.0 * k, line);
            p.add(path(pts(&[(0.0, -3.0), (0.0, 0.0), (3.0, 1.5)])));
        }
        Mark::Important => {
            p.add(closed(pts(&[(-6.5, -4.5), (2.0, -4.5), (6.5, 0.0), (2.0, 4.5), (-6.5, 4.5), (-3.5, 0.0)])));
        }
        Mark::Send => {
            p.add(closed(pts(&[(-6.0, -5.5), (6.5, 0.0), (-6.0, 5.5), (-4.0, 0.0)])));
            seg((-4.0, 0.0), (1.5, 0.0));
        }
        Mark::Calendar => {
            p.rect_stroke(Rect::from_min_max(at(-6.0, -4.5), at(6.0, 6.0)), 1.5 * k, line, StrokeKind::Middle);
            seg((-6.0, -1.0), (6.0, -1.0));
            seg((-3.0, -6.5), (-3.0, -3.0));
            seg((3.0, -6.5), (3.0, -3.0));
        }
        Mark::Pencil => {
            p.add(closed(pts(&[(-6.0, 6.0), (-5.0, 2.0), (2.0, -5.0), (5.0, -2.0), (-2.0, 5.0)])));
            seg((-1.0, -2.0), (2.0, 1.0));
        }
        Mark::Mail => {
            p.rect_stroke(Rect::from_min_max(at(-6.5, -4.5), at(6.5, 4.5)), 1.5 * k, line, StrokeKind::Middle);
            p.add(path(pts(&[(-6.0, -4.0), (0.0, 1.0), (6.0, -4.0)])));
        }
        Mark::Archive => {
            p.rect_stroke(Rect::from_min_max(at(-6.5, -5.5), at(6.5, -2.0)), 1.0 * k, line, StrokeKind::Middle);
            p.add(path(pts(&[(-5.5, -2.0), (-5.5, 5.5), (5.5, 5.5), (5.5, -2.0)])));
            seg((-2.0, 1.0), (2.0, 1.0));
        }
        Mark::Warning => {
            p.add(closed(pts(&[(0.0, -6.0), (6.5, 5.5), (-6.5, 5.5)])));
            seg((0.0, -1.5), (0.0, 1.5));
            dot(0.0, 3.5, 0.9);
        }
        Mark::Trash => {
            seg((-6.0, -3.5), (6.0, -3.5));
            p.add(path(pts(&[(-2.0, -3.5), (-2.0, -5.5), (2.0, -5.5), (2.0, -3.5)])));
            p.add(path(pts(&[(-4.5, -3.5), (-3.8, 6.0), (3.8, 6.0), (4.5, -3.5)])));
            seg((-1.5, -0.5), (-1.5, 3.5));
            seg((1.5, -0.5), (1.5, 3.5));
        }
        Mark::Label => {
            p.add(closed(pts(&[(-6.5, -4.0), (1.5, -4.0), (6.5, 0.0), (1.5, 4.0), (-6.5, 4.0)])));
            dot(-3.5, 0.0, 1.1);
        }
        Mark::Reply | Mark::ReplyAll | Mark::Forward => {
            // The arrow turns back to the left; Forward is the same turned about.
            let s = if mark == Mark::Forward { -1.0 } else { 1.0 };
            let head = |dx: f32| path(pts(&[(s * (-1.0 + dx), -4.5), (s * (-5.0 + dx), -0.5), (s * (-1.0 + dx), 3.5)]));
            let dx = if mark == Mark::ReplyAll { 1.5 } else { 0.0 };
            p.add(head(dx));
            if mark == Mark::ReplyAll {
                p.add(head(-2.0));
            }
            p.add(path(pts(&[(s * (-4.5 + dx), -0.5), (s * (1.0 + dx), -0.5), (s * (4.5 + dx), 1.5), (s * (5.0 + dx), 5.5)])));
        }
        Mark::Paperclip => {
            let mut v = pts(&[(0.0, 2.5), (0.0, -3.0)]);
            v.extend(arc(1.5, -3.0, 1.5, PI, 2.0 * PI));
            v.extend(pts(&[(3.0, -3.0), (3.0, 3.5)]));
            v.extend(arc(0.0, 3.5, 3.0, 0.0, PI));
            v.extend(pts(&[(-3.0, 3.5), (-3.0, -3.8)]));
            v.extend(arc(0.0, -3.8, 3.0, PI, 1.5 * PI));
            p.add(path(v));
        }
        Mark::Link => {
            // Two open links turned up to the right, with a bar across.
            let r = |x: f32, y: f32| at((x + y) * std::f32::consts::FRAC_1_SQRT_2, (y - x) * std::f32::consts::FRAC_1_SQRT_2);
            for sg in [1.0_f32, -1.0] {
                let mut v = vec![r(-sg, -2.5)];
                v.extend((0..=12).map(|i| 1.5 * PI - PI * i as f32 / 12.0).map(|a| r(sg * (-3.5 + 2.5 * a.cos()), 2.5 * a.sin())));
                v.push(r(-sg, 2.5));
                p.add(path(v));
            }
            p.line_segment([r(-1.5, 0.0), r(1.5, 0.0)], line);
        }
        Mark::More => {
            for x in [-4.5, 0.0, 4.5] {
                dot(x, 0.0, 1.3);
            }
        }
        Mark::Sort => {
            seg((-6.0, -4.0), (6.0, -4.0));
            seg((-6.0, 0.0), (3.0, 0.0));
            seg((-6.0, 4.0), (0.0, 4.0));
        }
        Mark::Check => {
            p.add(path(pts(&[(-5.0, 0.5), (-1.5, 4.0), (5.5, -4.0)])));
        }
        Mark::ChevronDown | Mark::ChevronUp => {
            let d = if mark == Mark::ChevronDown { 1.0 } else { -1.0 };
            p.add(path(pts(&[(-4.5, -2.5 * d), (0.0, 2.5 * d), (4.5, -2.5 * d)])));
        }
        Mark::Info => {
            p.circle_stroke(m, 6.0 * k, line);
            seg((0.0, -0.5), (0.0, 3.5));
            dot(0.0, -3.0, 0.9);
        }
        Mark::Bell => {
            let mut v = arc(0.0, -1.0, 4.5, PI, 2.0 * PI);
            v.extend(pts(&[(4.5, 3.0), (6.0, 4.5), (-6.0, 4.5), (-4.5, 3.0)]));
            v.push(at(-4.5, -1.0));
            p.add(path(v));
            p.add(path(arc(0.0, 5.5, 1.5, 0.0, PI)));
            seg((0.0, -6.0), (0.0, -5.5));
        }
        Mark::Bold => {
            let heavy = Stroke::new(2.0 * k, colour);
            p.line_segment([at(-3.0, -5.5), at(-3.0, 5.5)], heavy);
            p.add(Shape::line(pts(&[(-3.0, -5.0), (1.5, -5.0), (3.5, -3.5), (3.5, -2.2), (1.5, -0.5), (-3.0, -0.5)]), heavy));
            p.add(Shape::line(pts(&[(-3.0, -0.5), (2.0, -0.5), (4.5, 1.5), (4.5, 3.2), (2.0, 5.0), (-3.0, 5.0)]), heavy));
        }
        Mark::Italic => {
            seg((-1.0, -5.0), (4.0, -5.0));
            seg((-4.0, 5.0), (1.0, 5.0));
            seg((2.5, -5.0), (-1.5, 5.0));
        }
        Mark::Minimize => {
            seg((-5.0, 3.5), (5.0, 3.5));
        }
        Mark::Expand => {
            p.add(path(pts(&[(0.5, -5.5), (5.5, -5.5), (5.5, -0.5)])));
            seg((5.5, -5.5), (1.0, -1.0));
            p.add(path(pts(&[(-0.5, 5.5), (-5.5, 5.5), (-5.5, 0.5)])));
            seg((-5.5, 5.5), (-1.0, 1.0));
        }
        Mark::Restore => {
            p.add(path(pts(&[(-5.5, -1.0), (-1.0, -1.0), (-1.0, -5.5)])));
            seg((-1.0, -1.0), (-5.5, -5.5));
            p.add(path(pts(&[(5.5, 1.0), (1.0, 1.0), (1.0, 5.5)])));
            seg((1.0, 1.0), (5.5, 5.5));
        }
        Mark::Emoji => {
            p.circle_stroke(m, 6.0 * k, line);
            dot(-2.2, -1.8, 0.9);
            dot(2.2, -1.8, 0.9);
            p.add(path(arc(0.0, 0.0, 3.4, 0.15 * PI, 0.85 * PI)));
        }
        Mark::Underline => {
            let mut v = pts(&[(-3.5, -6.0), (-3.5, 0.5)]);
            v.extend(arc(0.0, 0.5, 3.5, PI, 0.0));
            v.push(at(3.5, -6.0));
            p.add(path(v));
            seg((-5.5, 6.0), (5.5, 6.0));
        }
        Mark::Strike => {
            p.add(path(pts(&[
                (3.5, -3.5),
                (2.0, -5.0),
                (-2.0, -5.0),
                (-3.5, -3.0),
                (-1.5, -1.0),
                (1.5, 1.0),
                (3.5, 3.0),
                (2.0, 5.0),
                (-2.0, 5.0),
                (-3.5, 3.5),
            ])));
            seg((-6.0, 0.0), (6.0, 0.0));
        }
        Mark::Format => {
            p.add(path(pts(&[(-4.5, 5.0), (0.0, -5.5), (4.5, 5.0)])));
            seg((-2.5, 1.5), (2.5, 1.5));
        }
        Mark::Save => {
            p.add(closed(pts(&[(-6.0, -6.0), (3.5, -6.0), (6.0, -3.5), (6.0, 6.0), (-6.0, 6.0)])));
            p.add(path(pts(&[(-3.0, -6.0), (-3.0, -2.5), (2.5, -2.5), (2.5, -6.0)])));
            p.add(path(pts(&[(-3.5, 6.0), (-3.5, 1.5), (3.5, 1.5), (3.5, 6.0)])));
        }
        Mark::Undo | Mark::Redo => {
            let s = if mark == Mark::Redo { -1.0 } else { 1.0 };
            let mut v = pts(&[(s * -5.5, -3.0), (s * 1.0, -3.0)]);
            v.extend((0..=16).map(|i| -PI / 2.0 + PI * i as f32 / 16.0).map(|a| at(s * (1.0 + 4.5 * a.cos()), 1.5 + 4.5 * a.sin())));
            v.push(at(s * -2.0, 6.0));
            p.add(path(v));
            p.add(path(pts(&[(s * -3.0, -5.5), (s * -5.5, -3.0), (s * -3.0, -0.5)])));
        }
        Mark::Arrange => {
            for (x, y) in [(-6.0, -6.0), (1.0, -6.0), (-6.0, 1.0), (1.0, 1.0)] {
                p.rect_stroke(Rect::from_min_max(at(x, y), at(x + 5.0, y + 5.0)), 1.0 * k, line, StrokeKind::Middle);
            }
        }
        Mark::ArrowDown => {
            seg((0.0, -6.0), (0.0, 6.0));
            p.add(path(pts(&[(-4.0, 2.0), (0.0, 6.0), (4.0, 2.0)])));
        }
        Mark::ArrowRight => {
            seg((-6.0, 0.0), (6.0, 0.0));
            p.add(path(pts(&[(2.0, -4.0), (6.0, 0.0), (2.0, 4.0)])));
        }
        Mark::Settings => {
            let mut v = Vec::new();
            for i in 0..8 {
                let a = i as f32 * PI / 4.0;
                for (r, d) in [(4.8, -0.38), (6.6, -0.2), (6.6, 0.2), (4.8, 0.38)] {
                    v.push(at(r * (a + d).cos(), r * (a + d).sin()));
                }
            }
            p.add(closed(v));
            p.circle_stroke(m, 2.2 * k, line);
        }
        Mark::Terminal => {
            p.rect_stroke(Rect::from_min_max(at(-6.5, -5.0), at(6.5, 5.0)), 1.5 * k, line, StrokeKind::Middle);
            p.add(path(pts(&[(-3.5, -2.0), (-1.0, 0.0), (-3.5, 2.0)])));
            seg((1.0, 2.2), (3.5, 2.2));
        }
        Mark::Flow => {
            for (x, y) in [(0.0, -4.5), (-4.5, 4.5), (4.5, 4.5)] {
                p.circle_stroke(at(x, y), 1.7 * k, line);
            }
            seg((-0.8, -3.0), (-3.7, 2.8));
            seg((0.8, -3.0), (3.7, 2.8));
        }
        Mark::Branch => {
            seg((0.0, -6.0), (0.0, 0.0));
            p.add(path(pts(&[(-4.0, 4.0), (0.0, 0.0), (4.0, 4.0)])));
            dot(-4.0, 4.5, 1.3);
            dot(4.0, 4.5, 1.3);
        }
        Mark::Merge => {
            seg((0.0, 0.0), (0.0, 6.0));
            p.add(path(pts(&[(-4.0, -4.0), (0.0, 0.0), (4.0, -4.0)])));
            dot(-4.0, -4.5, 1.3);
            dot(4.0, -4.5, 1.3);
        }
        Mark::File => {
            p.add(closed(pts(&[(-4.5, -6.5), (1.5, -6.5), (5.0, -3.0), (5.0, 6.5), (-4.5, 6.5)])));
            p.add(path(pts(&[(1.5, -6.5), (1.5, -3.0), (5.0, -3.0)])));
        }
        Mark::Person => {
            p.circle_stroke(at(0.0, -2.5), 3.0 * k, line);
            let mut v = arc(0.0, 6.5, 5.5, PI, 2.0 * PI);
            v.push(at(-5.5, 6.5));
            p.add(path(v));
        }
        Mark::Folder => {
            p.add(closed(pts(&[(-6.5, -4.5), (-2.0, -4.5), (-0.5, -2.5), (6.5, -2.5), (6.5, 5.5), (-6.5, 5.5)])));
        }
        Mark::CalendarOff | Mark::CalendarCheck => {
            p.rect_stroke(Rect::from_min_max(at(-6.0, -4.5), at(6.0, 6.0)), 1.5 * k, line, StrokeKind::Middle);
            seg((-6.0, -1.0), (6.0, -1.0));
            seg((-3.0, -6.5), (-3.0, -3.0));
            seg((3.0, -6.5), (3.0, -3.0));
            if mark == Mark::CalendarOff {
                seg((-2.0, 1.0), (2.0, 5.0));
                seg((2.0, 1.0), (-2.0, 5.0));
            } else {
                p.add(path(vec![at(-2.5, 3.0), at(-0.8, 4.8), at(2.6, 1.0)]));
            }
        }
        Mark::Stop => {
            p.rect_stroke(Rect::from_min_max(at(-4.5, -4.5), at(4.5, 4.5)), 1.5 * k, line, StrokeKind::Middle);
        }
        Mark::Skip => {
            p.add(Shape::convex_polygon(vec![at(-5.0, -5.0), at(2.0, 0.0), at(-5.0, 5.0)], Color32::TRANSPARENT, line));
            seg((5.5, -5.0), (5.5, 5.0));
        }
        Mark::Print => {
            p.add(closed(pts(&[(-3.5, -2.0), (-3.5, -6.5), (3.5, -6.5), (3.5, -2.0)])));
            p.add(closed(pts(&[(-3.5, 1.0), (-6.5, 1.0), (-6.5, -2.0), (6.5, -2.0), (6.5, 1.0), (3.5, 1.0)])));
            p.add(closed(pts(&[(-3.5, 0.0), (3.5, 0.0), (3.5, 6.5), (-3.5, 6.5)])));
        }
        Mark::Rerun => {
            p.add(path(arc(0.0, 0.0, 5.5, 0.15 * PI, 1.7 * PI)));
            p.add(path(pts(&[(1.2, -4.3), (4.0, -4.0), (3.6, -6.4)])));
            p.add(Shape::convex_polygon(vec![at(-1.5, -2.5), at(2.5, 0.0), at(-1.5, 2.5)], Color32::TRANSPARENT, line));
        }
        Mark::Compare => {
            p.rect_stroke(Rect::from_min_max(at(-6.5, -5.5), at(-0.8, 5.5)), 1.5 * k, line, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(at(0.8, -5.5), at(6.5, 5.5)), 1.5 * k, line, StrokeKind::Middle);
        }
        Mark::Import => {
            p.add(path(pts(&[(-1.0, -6.0), (6.0, -6.0), (6.0, 6.0), (-1.0, 6.0)])));
            seg((-6.5, 0.0), (2.0, 0.0));
            p.add(path(pts(&[(-1.0, -3.0), (2.0, 0.0), (-1.0, 3.0)])));
        }
        Mark::Server => {
            for y in [-6.0, -1.5, 3.0] {
                p.rect_stroke(Rect::from_min_max(at(-6.5, y), at(6.5, y + 3.5)), 1.5 * k, line, StrokeKind::Middle);
            }
            for y in [-4.25, 0.25, 4.75] {
                dot(-3.8, y, 0.8);
            }
        }
        Mark::Heading => {
            seg((-4.5, -5.5), (-4.5, 5.5));
            seg((4.5, -5.5), (4.5, 5.5));
            seg((-4.5, 0.0), (4.5, 0.0));
        }
        Mark::Quote => {
            seg((-5.5, -5.0), (-5.5, 5.0));
            seg((-2.0, -3.0), (6.0, -3.0));
            seg((-2.0, 0.5), (6.0, 0.5));
            seg((-2.0, 4.0), (3.5, 4.0));
        }
        Mark::Code => {
            p.add(path(pts(&[(-2.5, -4.5), (-6.5, 0.0), (-2.5, 4.5)])));
            p.add(path(pts(&[(2.5, -4.5), (6.5, 0.0), (2.5, 4.5)])));
            seg((1.0, -5.5), (-1.0, 5.5));
        }
        Mark::Bullets => {
            for y in [-4.5, 0.0, 4.5] {
                dot(-5.2, y, 1.1);
                seg((-1.5, y), (6.5, y));
            }
        }
        Mark::Numbers => {
            // The digits 1 2 3 by the lines, so the list reads as numbered at a glance.
            for y in [-4.5, 0.0, 4.5] {
                seg((-1.0, y), (6.5, y));
            }
            let (y1, y2, y3) = (-4.5, 0.0, 4.5);
            p.add(path(pts(&[(-5.6, y1 - 1.0), (-4.5, y1 - 1.8), (-4.5, y1 + 1.8)])));
            p.add(path(pts(&[(-5.9, y2 - 1.2), (-4.9, y2 - 1.9), (-3.7, y2 - 1.2), (-3.9, y2 + 0.0), (-5.9, y2 + 1.9), (-3.5, y2 + 1.9)])));
            p.add(path(pts(&[(-5.9, y3 - 1.9), (-3.7, y3 - 1.9), (-4.7, y3 - 0.3), (-3.6, y3 + 0.7), (-4.6, y3 + 1.9), (-5.9, y3 + 1.4)])));
        }
        Mark::Tasks => {
            // Three boxes by the lines; the first is ticked.
            for (i, y) in [-4.5, 0.0, 4.5].into_iter().enumerate() {
                p.rect_stroke(Rect::from_min_max(at(-6.3, y - 2.0), at(-2.5, y + 2.0)), 0.8 * k, line, StrokeKind::Middle);
                seg((-1.0, y), (6.5, y));
                if i == 0 {
                    p.add(path(pts(&[(-5.5, y), (-4.7, y + 1.0), (-3.4, y - 1.2)])));
                }
            }
        }
        Mark::Desktop => {
            p.rect_stroke(Rect::from_min_max(at(-6.5, -6.0), at(6.5, 2.5)), 1.5 * k, line, StrokeKind::Middle);
            seg((0.0, 2.5), (0.0, 5.5));
            seg((-3.5, 6.0), (3.5, 6.0));
        }
        Mark::Database => {
            // A can: an ellipse on top, the sides, and two curved bands under the lid.
            let ellipse = |cy: f32, from: f32, to: f32| -> Vec<Pos2> {
                (0..=16).map(|i| from + (to - from) * i as f32 / 16.0).map(|a| at(5.5 * a.cos(), cy + 2.2 * a.sin())).collect()
            };
            p.add(Shape::closed_line(ellipse(-4.0, 0.0, 2.0 * PI), line));
            seg((-5.5, -4.0), (-5.5, 4.0));
            seg((5.5, -4.0), (5.5, 4.0));
            p.add(path(ellipse(4.0, 0.0, PI)));
            p.add(path(ellipse(0.0, 0.0, PI)));
        }
        Mark::Lock => {
            p.rect_stroke(Rect::from_min_max(at(-5.0, -0.5), at(5.0, 6.5)), 1.5 * k, line, StrokeKind::Middle);
            let mut v = vec![at(-3.0, -0.5), at(-3.0, -3.0)];
            v.extend(arc(0.0, -3.0, 3.0, PI, 2.0 * PI));
            v.push(at(3.0, -0.5));
            p.add(path(v));
            p.circle_filled(at(0.0, 3.0), 1.0 * k, colour);
        }
        Mark::Globe => {
            p.circle_stroke(m, 6.0 * k, line);
            p.add(path((0..=24).map(|i| 2.0 * PI * i as f32 / 24.0).map(|a| at(2.8 * a.cos(), 6.0 * a.sin())).collect()));
            seg((-6.0, 0.0), (6.0, 0.0));
        }
    }
}

/// The fill, edge, mark and words of an [`IconButton`] of `tone`, hot (under the pointer or with the keys) or not.
pub(crate) fn tone_look(c: &ito_theme::Colors, tone: Tone, hot: bool) -> (Color32, Color32, Color32, Color32) {
    match tone {
        Tone::Plain if hot => (c.hover(), c.border_strong(), c.fg, c.fg),
        Tone::Plain => (c.raised(), c.border, c.dim, c.fg),
        Tone::Primary => {
            let f = if hot { mix(c.run, c.fg, 0.15) } else { c.run };
            (f, f, c.on_accent(), c.on_accent())
        }
        Tone::Warn => {
            let f = if hot { mix(c.wait, c.fg, 0.15) } else { c.wait };
            let ink = crate::ink_on(f);
            (f, f, ink, ink)
        }
        // An outline tinted with the error colour, not a solid fill.
        Tone::Danger if hot => (mix(c.raised(), c.err, 0.2), c.err, c.err, c.err),
        Tone::Danger => (mix(c.raised(), c.err, 0.07), mix(c.border, c.err, 0.6), c.err, c.err),
    }
}

/// What `IconButton::show` lays out, worked out once so `size` and `show` cannot drift.
struct Measured {
    galley: std::sync::Arc<egui::Galley>,
    chip: Option<std::sync::Arc<egui::Galley>>,
    chip_w: f32,
    /// Mark, words and chip with their gaps, without the padding.
    content: f32,
    size: Vec2,
    glyph: f32,
    pad: f32,
    gap: f32,
}

/// A quiet outlined button with a [`Mark`] and words, and optionally a [`Tone`], the key that does the same (a small chip at
/// the right), the full width of the column, or the mark after the words (a Send button).
#[derive(Clone, Copy, Debug)]
pub struct IconButton<'a> {
    mark: Mark,
    words: &'a str,
    said: &'a str,
    tone: Tone,
    key: Option<&'a str>,
    wide: bool,
    mark_last: bool,
    drag: bool,
    min_size: Vec2,
    text_size: Option<f32>,
}

impl<'a> IconButton<'a> {
    pub fn new(mark: Mark, words: &'a str) -> Self {
        IconButton {
            mark,
            words,
            said: words,
            tone: Tone::Plain,
            key: None,
            wide: false,
            mark_last: false,
            drag: false,
            min_size: Vec2::ZERO,
            text_size: None,
        }
    }

    /// The spoken name, and the tip when it differs from the words.
    pub fn said(mut self, said: &'a str) -> Self {
        self.said = said;
        self
    }

    /// `Tone::Primary` for the one main action, `Tone::Danger` for the one that throws something away.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// The shortcut, as the app's keymap spells it (so a rebound key shows the new one).
    pub fn key(mut self, key: &'a str) -> Self {
        self.key = Some(key);
        self
    }

    /// As wide as the column; the key chip goes to the right edge.
    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }

    /// The mark after the words rather than before.
    pub fn mark_last(mut self) -> Self {
        self.mark_last = true;
        self
    }

    /// Also sense drags (a part dragged out of a shelf, or clicked to add).
    pub fn drag(mut self) -> Self {
        self.drag = true;
        self
    }

    /// The size of the words in points, in the proportional family; by default the body style of the `Ui`, which does not
    /// follow `ito_theme`'s text zoom (pass `ito_theme::zoomed(ctx, 12.5)` for words that do). `size` and `show` both use it.
    pub fn text_size(mut self, points: f32) -> Self {
        self.text_size = Some(points);
        self
    }

    /// At least this big. When the button is wider than its mark, words and key chip, they are centred as one group (with
    /// [`wide`](Self::wide) the content stays at the left and the chip at the right edge, as without a minimum). The height is
    /// the larger of the usual one and `size.y`.
    pub fn min_size(mut self, size: Vec2) -> Self {
        self.min_size = size;
        self
    }

    /// The size [`show`](Self::show) takes before [`wide`](Self::wide) stretches it, [`min_size`](Self::min_size) included, so
    /// a layout can set a slot first. Measure with the same `Ui` style and zoom as the `Ui` that shows it.
    pub fn size(&self, ui: &Ui) -> Vec2 {
        self.measure(ui).size
    }

    fn measure(&self, ui: &Ui) -> Measured {
        let font = self.text_size.map_or_else(|| egui::TextStyle::Body.resolve(ui.style()), egui::FontId::proportional);
        let galley = ui.painter().layout_no_wrap(self.words.to_owned(), font, Color32::PLACEHOLDER);
        let chip =
            self.key.map(|k| ui.painter().layout_no_wrap(k.to_owned(), egui::FontId::proportional(z(ui, 10.5)), Color32::PLACEHOLDER));
        let (glyph, pad, gap) = (z(ui, 14.0), z(ui, 10.0), z(ui, 6.0));
        let chip_w = chip.as_ref().map_or(0.0, |g| (g.size().x + z(ui, 10.0)).max(z(ui, 18.0)));
        let mut content = glyph + gap + galley.size().x;
        if chip.is_some() {
            content += gap + chip_w;
        }
        let height = ui.spacing().interact_size.y.max(z(ui, 26.0)).max(self.min_size.y);
        let size = Vec2::new((pad + content + pad).max(self.min_size.x), height);
        Measured { galley, chip, chip_w, content, size, glyph, pad, gap }
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let c = ito_theme::colors();
        let Measured { galley, chip, chip_w, content, mut size, glyph, pad, gap } = self.measure(ui);
        if self.wide {
            let room = ui.available_width();
            if room.is_finite() {
                size.x = size.x.max(room);
            }
        }
        let (rect, r) = ui.allocate_exact_size(size, if self.drag { Sense::click_and_drag() } else { Sense::click() });
        let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
        let r = if self.said != self.words { r.on_hover_text(self.said) } else { r };
        ito_a11y::say(&r, Said::Button, self.said);
        take_keys(ui, &r);
        if ui.is_rect_visible(rect) {
            let hot = r.hovered() || r.has_focus();
            let (fill, edge, mark_ink, text_ink) = tone_look(&c, self.tone, hot);
            let p = ui.painter();
            p.rect(rect, 6.0, fill, Stroke::new(1.0, edge), StrokeKind::Inside);
            let cy = rect.center().y;
            let glyph_at = |x: f32| Rect::from_center_size(Pos2::new(x + glyph / 2.0, cy), Vec2::splat(glyph));
            let mut x = if self.wide { rect.left() + pad } else { rect.left() + (rect.width() - content) / 2.0 };
            let text_w = galley.size().x;
            let text_pos = |x: f32| Pos2::new(x, cy - galley.size().y / 2.0);
            if self.mark_last {
                p.galley(text_pos(x), galley.clone(), text_ink);
                mark(p, glyph_at(x + text_w + gap), self.mark, mark_ink);
                x += text_w + gap + glyph;
            } else {
                mark(p, glyph_at(x), self.mark, mark_ink);
                p.galley(text_pos(x + glyph + gap), galley.clone(), text_ink);
                x += glyph + gap + text_w;
            }
            if let Some(g) = chip {
                let cx = if self.wide { rect.right() - pad - chip_w } else { x + gap };
                let cr = Rect::from_min_size(Pos2::new(cx, cy - z(ui, 8.0)), Vec2::new(chip_w, z(ui, 16.0)));
                p.rect_stroke(cr, 4.0, Stroke::new(1.0, mix(fill, text_ink, 0.35)), StrokeKind::Inside);
                p.galley(cr.center() - g.size() / 2.0, g, mix(fill, text_ink, 0.8));
            }
        }
        focus_ring(ui, &r);
        r
    }
}

/// A quiet outlined button with `mark` before `words`: the `raised` fill and a thin `border`, `hover` and `border_strong` under
/// the pointer or with the keys, the mark in `dim` (`fg` when hot), the words in the body size of `ui`. `said` is its spoken
/// name, and its tip when it differs from the words. For a tone, a key or the full width, build an [`IconButton`].
pub fn icon_button(ui: &mut Ui, m: Mark, words: &str, said: &str) -> Response {
    IconButton::new(m, words).said(said).show(ui)
}

/// Paint the square button face of a [`mark_button`] in `rect`, for a `r` the caller made.
pub(crate) fn paint_square(ui: &Ui, rect: Rect, r: &Response, m: Mark) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let c = ito_theme::colors();
    let (fill, edge, ink, _) = tone_look(&c, Tone::Plain, r.hovered() || r.has_focus());
    let p = ui.painter();
    p.rect(rect, 6.0, fill, Stroke::new(1.0, edge), StrokeKind::Inside);
    mark(p, rect.shrink(rect.width() * 0.25), m, ink);
}

/// The same button with the mark alone, a square `side` points on a side (before the letters' zoom): a fold mark, a close
/// cross. `said` is its tip and what a reader says.
pub fn mark_button(ui: &mut Ui, m: Mark, said: &str, side: f32) -> Response {
    let side = z(ui, side);
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let r = r.on_hover_text(said).on_hover_cursor(egui::CursorIcon::PointingHand);
    ito_a11y::say(&r, Said::Button, said);
    take_keys(ui, &r);
    paint_square(ui, rect, &r, m);
    focus_ring(ui, &r);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: impl FnMut(&mut Ui)) -> egui::FullOutput {
        let mut f = f;
        let mut out = egui::Context::default().run_ui(egui::RawInput::default(), |ui| f(ui));
        out.textures_delta.clear();
        out
    }

    #[test]
    fn every_mark_paints_something() {
        for m in Mark::ALL {
            let out = run(|ui| {
                mark(ui.painter(), Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(14.0)), m, Color32::WHITE);
            });
            assert!(!out.shapes.is_empty(), "{m:?} drew nothing");
        }
    }

    #[test]
    fn the_marks_are_all_listed_once() {
        for (i, a) in Mark::ALL.iter().enumerate() {
            assert!(!Mark::ALL[..i].contains(a), "{a:?} twice");
        }
    }

    #[test]
    fn the_buttons_draw_and_the_words_widen_the_button() {
        let (mut short, mut long, mut square) = (0.0, 0.0, 0.0);
        run(|ui| {
            short = icon_button(ui, Mark::Search, "Go", "Go").rect.width();
            long = icon_button(ui, Mark::Download, "Export everything", "Export").rect.width();
            square = mark_button(ui, Mark::ChevronRight, "Fold", 22.0).rect.width();
        });
        assert!(long > short, "{long} {short}");
        assert!((square - 22.0).abs() < 0.5, "{square}");
    }

    #[test]
    fn a_key_widens_a_tone_does_not_and_wide_fills_the_column() {
        let (mut plain, mut keyed, mut danger, mut wide, mut room) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let mut heights = [0.0; 3];
        run(|ui| {
            room = ui.available_width();
            let a = IconButton::new(Mark::Reply, "Reply").show(ui);
            let b = IconButton::new(Mark::Reply, "Reply").tone(Tone::Primary).key("R").show(ui);
            let d = IconButton::new(Mark::Trash, "Reply").tone(Tone::Danger).show(ui);
            let w = IconButton::new(Mark::Plus, "New mail").key("C").wide().show(ui);
            let s = IconButton::new(Mark::Send, "Send").mark_last().show(ui);
            (plain, keyed, danger, wide) = (a.rect.width(), b.rect.width(), d.rect.width(), w.rect.width());
            heights = [a.rect.height(), b.rect.height(), s.rect.height()];
        });
        assert!(keyed > plain, "{keyed} {plain}");
        assert!((danger - plain).abs() < 0.5, "{danger} {plain}");
        assert!((wide - room).abs() < 1.0, "{wide} {room}");
        assert!(heights.iter().all(|h| (h - heights[0]).abs() < 0.01), "{heights:?}");
    }

    #[test]
    fn size_grows_with_the_words_and_min_size_wins_when_larger() {
        let (mut short, mut long, mut keyed, mut floor, mut shown) = (Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO);
        let mut plain = Vec2::ZERO;
        let mut inside = true;
        run(|ui| {
            short = IconButton::new(Mark::Save, "Go").size(ui);
            long = IconButton::new(Mark::Save, "Save everything").size(ui);
            keyed = IconButton::new(Mark::Save, "Go").key("S").size(ui);
            let b = IconButton::new(Mark::Save, "Go").min_size(Vec2::new(200.0, 40.0));
            floor = b.size(ui);
            let r = b.show(ui);
            shown = r.rect.size();
            // The bigger button still starts inside the Ui.
            let left = ui.min_rect().left();
            inside = r.rect.left() >= left;
            let p = IconButton::new(Mark::Save, "Go");
            plain = p.size(ui);
            let pr = p.show(ui);
            assert!((pr.rect.size() - plain).abs().max_elem() < 0.01);
        });
        assert!(long.x > short.x && keyed.x > short.x, "{short:?} {long:?} {keyed:?}");
        assert_eq!(floor, Vec2::new(200.0, 40.0));
        assert!((shown - floor).abs().max_elem() < 0.01, "{shown:?} {floor:?}");
        assert!(inside && plain.y < 40.0);
        // A minimum smaller than the content changes nothing.
        let mut same = false;
        run(|ui| {
            same = IconButton::new(Mark::Save, "Go").min_size(Vec2::new(10.0, 10.0)).size(ui) == IconButton::new(Mark::Save, "Go").size(ui)
        });
        assert!(same);
    }

    #[test]
    fn a_text_size_sets_the_size_and_size_matches_show() {
        fn is_copy<T: Copy>() {}
        is_copy::<IconButton>();
        let (mut plain, mut big, mut shown) = (Vec2::ZERO, Vec2::ZERO, Vec2::ZERO);
        run(|ui| {
            plain = IconButton::new(Mark::Save, "Save").size(ui);
            let b = IconButton::new(Mark::Save, "Save").text_size(30.0);
            big = b.size(ui);
            shown = b.show(ui).rect.size();
        });
        assert!(big.x > plain.x, "{plain:?} {big:?}");
        assert!((shown - big).abs().max_elem() < 0.01, "{shown:?} {big:?}");
    }
}
