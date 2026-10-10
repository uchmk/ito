# ito

The shared crates of uchmk's apps, the thread (糸, *ito*) that runs through
them: the terminal pane, splitting a window into panes, the words on screen,
key notation, search matching, a local door into a running app, a small
MCP server, and the settings, themes and clock every app shares. Rust, Windows first, with macOS and Linux on x86_64 and ARM64.

| Crate | What it is |
|---|---|
| `ito-pane` | A terminal pane: a shell on a PTY (ConPTY on Windows), its key encoding, and an egui view |
| `ito-layout` | Splitting a rectangle into panes: a tree of splits, and its dividers in egui |
| `ito-i18n` | The words on screen by key, and the language setting the apps share (`common.toml`) |
| `ito-keys` | yazi's key notation (`<C-a>`, `<A-S-Up>` …; `C` is Cmd on macOS): parsing, printing, and egui's key events |
| `ito-match` | How a search query matches text: a regular expression with smart case, or fzf-style fuzzy letters |
| `ito-ipc` | A per-user local door into a running app (Unix socket / named pipe) and framed serde messages |
| `ito-mcp` | A small MCP server over stdio: JSON-RPC, `tools/list` and `tools/call`, without an async runtime |
| `ito-common` | What every app shares: the config folder, `common.toml` (language, theme, clock), editing a TOML file in place and watching it |
| `ito-theme` | The themes: the thirteen built in, a theme file's colours, and the one in force |
| `ito-prefs` | The settings screen: the nav, the cards of rows and their controls, and the pages every app shares |

What the apps keep the same (`common.toml`, the settings screen, the themes,
the clock) is in [docs/common-spec.md](docs/common-spec.md) (in Japanese).

## The apps

- [tsumugi](https://github.com/uchmk/tsumugi) (紬): a terminal for running many AI CLI sessions side by side
- [kura](https://github.com/uchmk/kura) (蔵): a keyboard-driven file manager in the spirit of yazi
- [yagura](https://github.com/uchmk/yagura) (櫓): a keyboard-driven system monitor
- [fuda](https://github.com/uchmk/fuda) (札, to come): a keyboard-driven to-do list
- [fude](https://github.com/uchmk/fude) (筆, to come): a keyboard-driven text editor

## Using a crate

The crates are not on crates.io. Read them from git, pinned to one commit,
and keep every `ito-*` crate of an app at the same `rev`:

```toml
[dependencies]
ito-pane = { git = "https://github.com/uchmk/ito", rev = "…", features = ["egui"] }
ito-layout = { git = "https://github.com/uchmk/ito", rev = "…" }
```

To work on an app and these crates together, point the app at a local
checkout without touching its `rev`:

```sh
cargo build --config 'patch."https://github.com/uchmk/ito".ito-pane.path="../ito/crates/ito-pane"'
```

## History

The crates were cut out of tsumugi on 2026-10-11, with their history. The
terminal pane's history before it left filer (now kura) lives there.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.
