//! The hidden session window, in a process of its own (the service-mode test in the crate sets a global).

#[test]
#[cfg(windows)]
fn a_window_watch_starts_and_stops_cleanly_more_than_once() {
    for _ in 0..3 {
        let a = ito_power::on_session(|_| {}).expect("a message-only window can be made");
        let b = ito_power::on_session(|_| {}).expect("a second one too");
        drop(a);
        drop(b);
    }
}

#[test]
#[cfg(not(windows))]
fn there_is_nothing_to_watch_off_windows() {
    assert_eq!(ito_power::on_session(|_| {}).unwrap_err(), ito_power::WhyNot::Unsupported);
    assert_eq!(ito_power::user_idle_for(), None);
}
