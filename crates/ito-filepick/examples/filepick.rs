//! A headless demo of the exchange between `Picker` and an app: a fake engine on a worker thread answers the dialog's wants
//! with delays, a long Japanese name, 100 000 files, a failing folder, a truncated listing and a slow folder.
//!
//! No window is opened (the workspace has no `eframe`): frames run on a bare `egui::Context` with scripted keys, and the log
//! says what the dialog asked for and how it ended. Run: `cargo run -p ito-filepick --example filepick --features egui`.
//!
//! With feature `local` and `--local`, the engine lists this machine's real disk with `list_local` instead (it starts at
//! home, with `local_places()`), and the run only opens the dialog and prints what it shows.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use egui::{Context, Event, RawInput};
use ito_filepick::{Done, Entry, Filter, Listing, Look, Mode, Picker, Place, PlaceKind, Style, Ticket, Want, Words};

type Reply = (Ticket, Result<Listing, String>);

/// The fake engine: one thread, one request at a time, answers after a delay that depends on the folder.
fn engine(wants: mpsc::Receiver<(Ticket, String)>, replies: mpsc::Sender<Reply>) {
    for (ticket, path) in wants {
        let (delay, answer) = match path.as_str() {
            "/slow" => (1500, Ok(Listing::new(vec![Entry::file("finally.txt")]))),
            "/broken" => (100, Err("permission denied (os error 13)".to_owned())),
            "/big" => (200, Ok(Listing::new((0..100_000).map(|i| Entry::file(&format!("file{i:06}.log")).size(i)).collect()))),
            "/capped" => (100, Ok(Listing::new((0..2000).map(|i| Entry::file(&format!("item{i:04}"))).collect()).truncated(true))),
            _ => (
                60,
                Ok(Listing::new(vec![
                    Entry::dir("big"),
                    Entry::dir("broken"),
                    Entry::dir("capped"),
                    Entry::dir("slow"),
                    Entry::file("とても長い名前のファイルでウィンドウの幅を超えたときに省略記号で切れることを確かめるためのもの.txt")
                        .size(1234),
                    Entry::file("notes.md").size(5_000_000).modified(1_700_000_000),
                    Entry::file(".hidden").hidden(true),
                ])),
            ),
        };
        thread::sleep(Duration::from_millis(delay));
        if replies.send((ticket, answer)).is_err() {
            return;
        }
    }
}

/// The real engine: lists this machine's disk, one request at a time.
#[cfg(feature = "local")]
fn local_engine(wants: mpsc::Receiver<(Ticket, String)>, replies: mpsc::Sender<Reply>) {
    for (ticket, path) in wants {
        let options = if std::env::args().any(|a| a == "--strict") {
            ito_filepick::ListOptions::strict(5000)
        } else {
            ito_filepick::ListOptions::new(5000)
        };
        let answer = ito_filepick::list_local_with(&path, &options).map_err(|e| e.words("en"));
        if replies.send((ticket, answer)).is_err() {
            return;
        }
    }
}

fn main() {
    let (want_tx, want_rx) = mpsc::channel();
    let (reply_tx, reply_rx) = mpsc::channel();
    #[cfg(feature = "local")]
    let local = std::env::args().any(|a| a == "--local");
    #[cfg(not(feature = "local"))]
    let local = {
        if std::env::args().any(|a| a == "--local") {
            eprintln!("--local needs feature `local`; running the scripted fake engine");
        }
        false
    };
    #[cfg(feature = "local")]
    if local {
        thread::spawn(move || local_engine(want_rx, reply_tx));
    } else {
        thread::spawn(move || engine(want_rx, reply_tx));
    }
    #[cfg(not(feature = "local"))]
    thread::spawn(move || engine(want_rx, reply_tx));

    let mut picker = Picker::new("demo", Mode::OpenFile, Style::Posix)
        .start("/")
        .places(vec![Place::new("Root", "/", PlaceKind::Root), Place::new("Big", "/big", PlaceKind::Folder)])
        .filters(vec![Filter::new("Text", &["txt", "md"])]);
    #[cfg(feature = "local")]
    if local {
        let strict = std::env::args().any(|a| a == "--strict");
        let places =
            if strict { ito_filepick::local_places_with(&ito_filepick::ListOptions::strict(5000)) } else { ito_filepick::local_places() };
        let home = places.iter().find(|p| p.kind == PlaceKind::Home).or(places.first()).map(|p| p.path.clone());
        let style = ito_filepick::local_style();
        picker = Picker::new("demo", Mode::OpenFile, style).start(&home.unwrap_or_else(|| style.default_root().to_owned())).places(places);
    }
    let (words, clock) = (Words::en(), ito_common::Clock::default());
    let look = Look { words: &words, clock: &clock, lang: "en" };
    let ctx = Context::default();

    // (frame, what to do): navigate with the model API and type with real key events.
    type Step = Box<dyn Fn(&mut Picker) -> Vec<Event>>;
    let script: Vec<(usize, Step)> = vec![
        (
            20,
            Box::new(|p| {
                p.go("/big");
                vec![]
            }),
        ),
        (40, Box::new(|_| vec![Event::Text("file0999".to_owned())])),
        (
            60,
            Box::new(|p| {
                p.go("/broken");
                vec![]
            }),
        ),
        (
            80,
            Box::new(|p| {
                p.retry();
                vec![]
            }),
        ),
        (
            100,
            Box::new(|p| {
                p.go("/capped");
                vec![]
            }),
        ),
        (
            120,
            Box::new(|p| {
                p.go("/slow");
                vec![]
            }),
        ),
        (
            160,
            Box::new(|p| {
                p.go("/");
                vec![]
            }),
        ),
        (
            200,
            Box::new(|p| {
                p.set_name("notes.md");
                vec![]
            }),
        ),
        (
            230,
            Box::new(|p| {
                // The slow listing of "/" has landed by now; Open is the same call the button makes.
                p.accept();
                vec![]
            }),
        ),
    ];

    let frames = if local { 60 } else { 400 };
    for frame in 0..frames {
        while let Ok((ticket, answer)) = reply_rx.try_recv() {
            println!("frame {frame}: engine answered {ticket} ({})", if answer.is_ok() { "listing" } else { "error" });
            picker.set_listing(ticket, answer);
        }
        let events = script.iter().filter(|(at, _)| !local && *at == frame).flat_map(|(_, f)| f(&mut picker)).collect();
        let input = RawInput { time: Some(frame as f64 * 0.016), events, ..Default::default() };
        let mut shown = Default::default();
        let mut out = ctx.run_ui(input, |ui| shown = picker.show(ui.ctx(), &look));
        out.textures_delta.clear();
        let ito_filepick::Shown { want, done } = shown;
        if let Some(Want::List { ticket, path }) = want {
            println!("frame {frame}: dialog wants {path} as {ticket}");
            want_tx.send((ticket, path)).expect("the engine is gone");
        }
        match done {
            Some(Done::Picked(paths)) => return println!("frame {frame}: picked {paths:?}"),
            Some(Done::Cancelled) => return println!("frame {frame}: cancelled"),
            None => {}
        }
        thread::sleep(Duration::from_millis(16));
    }
    if local {
        return println!("local: at {:?}, {} entries, status {:?}", picker.current(), picker.entry_count(), picker.status());
    }
    println!(
        "the script ended without a result: name {:?} error {:?} can_accept {} target {}",
        picker.name(),
        picker.error(),
        picker.can_accept(),
        picker.target()
    );
}
