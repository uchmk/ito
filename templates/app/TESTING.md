# Testing

The checks a person (or an agent with a screen) makes. Each section is a table of `id | check`; a test that names an id
in its code takes the row off the list, and `cargo run --example make-testcheck` writes what is left to `TESTING-CHECKS.md`.

## Start

| id | check |
|---|---|
| start-1 | `{{app}}` opens a window of 1280x800 with the text "{{app}}" |
| start-2 | After moving and resizing it, closing and starting again, the window comes back at the same place and size |
| start-3 | `F12` opens the browser at the repository's new issue form with the version and OS filled in |
