# TODO

雛形（ito の `templates/app`）から作った。雛形で動いているのは、窓（場所を覚える）、common.toml のテーマ・言語・倍率、
`--version` / `--help`、F12 の不具合報告、`{{app}}.com`、アイコンと版の情報、TESTING の表の生成まで。
下は**依存は入っているが、まだ配線していないもの**。先に作った別のアプリが手本になる。自前で書かずに ito のクレートを使う
（足りない所は ito に頼む。`scripts/ito-first.py` が見ている）。

- [ ] `--keys` の台本: `ito-keyscript`（`Dialect`・`Runner`・`Press`）。手本は michi の `main.rs`。`lanes.conf` の `keys_env` と合わせる。
- [ ] `keymap.toml` と設定の「キー」ページ: `ito-keys`・`ito-keymap`・`ito-prefs` の `keys_page`。既定のキーは `defaults/keymap.toml`
  （`<C-,>` 設定・`<F1>` ヘルプ・`<C-q>` 終了）。手本は minato の `keymap.rs`・`prefs.rs`。
- [ ] `?` と F1 のヘルプ: `ito-help`（`ito-keymap` の一覧を描く）。手本は michi の `app.rs`・`ui.rs`。
- [ ] 通知と設定の「通知」ページ: `ito-notify`（システムの通知・音・webhook）と `ito-prefs` の `notify_page`。手本は minato の `app.rs`・`settings.rs`。
- [ ] 設定の画面: `ito_prefs::show`（Language・CLOCK・Theme の共通のページ + キー）。手本は minato の `settings.rs`。
- [ ] 窓の場所を戻す: 雛形は `window::Saved` で戻し `Tracker::follow` で書く。終わるときの `Tracker::close` は未配線。
- [ ] 通知を入れる: 雛形のベルは `notices` が空のまま。`ito_notices::Notice` を push し（`go` に飛び先の型）、`ito-notify` の `Toast::open` で押すと開くトーストにする。
- [ ] 窓の中央の箱（`Out.search`）をコマンドパレットに繋ぐ（下の項目）。
- [ ] コマンドパレット `<C-S-p>`: `ito-palette`（要らなければ `Cargo.toml` から外し、`ito-first.allow` に理由を書く）。手本は minato の `main.rs`。
- [ ] 字体（日本語が出る字体を `egui` に足す）。手本は minato の `fonts.rs`。
- [ ] `TESTING.md` に確認の行を足し、`cargo run --example make-testcheck` で `TESTING-CHECKS.md` を作る。
- [ ] `icon.rs` に本当のアイコンを描く。
- [ ] リリースの workflow は kura か yagura のものを写す。
