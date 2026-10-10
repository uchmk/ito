# ito — Claude 向けメモ

uchmk のアプリが使う共有のクレートを集めたリポジトリ。糸（ito）のように、アプリをつなぐ。Rust、Windows 優先。
使う側は tsumugi（紬、ターミナル）、kura（蔵、ファイラー。旧 filer）、yagura（櫓、システムモニター。旧 mimamori）。fuda（札、TODO）と fude（筆、エディター）も使う予定。

| クレート | 中身 | 主に使う側 |
|---|---|---|
| `ito-pane` | ターミナルのペイン（PTY・ConPTY、キーの変換、OSC、egui の表示） | tsumugi、kura |
| `ito-layout` | 矩形をペインに分ける木と、egui の仕切り | tsumugi、kura、yagura |
| `ito-i18n` | 画面の文言をキーで引く。言語の設定（`common.toml`）を全アプリで共有する | tsumugi、yagura |
| `ito-keys` | yazi のキーの書き方（`<C-a>`・`<A-S-Up>`・`<P-e>`） | yagura |
| `ito-match` | 検索の一致の規則（正規表現か、fzf 風の曖昧一致） | tsumugi、kura |
| `ito-ipc` | 動いているアプリへの手元の扉（Unix ソケット・名前付きパイプ）と、長さ付きの serde のメッセージ | tsumugi、kura |
| `ito-mcp` | stdio の小さな MCP サーバ（非同期ランタイムなし） | tsumugi、kura |

2026-10-11 に tsumugi（34e5cd2、v0.80.0）から履歴ごと切り出した（`git filter-repo`）。それより前のペインの履歴は kura にある。

- Cargo の workspace。クレートは `crates/` の下に置く。版はルートの `Cargo.toml` の `[workspace.package]` で 1 つ。
- **push の前に `scripts/verify.sh` を 1 回回す。**最後の行が `ALL OK: …` なら push してよい（`cargo fmt --check`、build `--locked`、test、clippy を Linux と `x86_64-pc-windows-msvc` の両方で `-D warnings`）。

## 作業ルール

- 返答は日本語。コード・コメント・コミットメッセージは英語。
- 頼まれるまでコミットしない。
- 改行は LF。スクリプトで書き換えるときは改行を変えない（Python なら `newline=''`）。
- やることは [TODO.md](TODO.md) に書く（`【人】` は持ち主の作業）。
- **整形は `cargo fmt` で揃える**（tsumugi・kura と違うところ）。設定は yagura と同じ `rustfmt.toml`（`max_width = 140`、
  `use_small_heuristics = "Max"`）で、手で整形していた形（`Self { a, b, c }` を 1 行）に近い。CI と verify.sh が `cargo fmt --check` で見る。
  切り出しの後に 1 回だけ全体を整形したコミットは `.git-blame-ignore-revs` に載せてある（`git config blame.ignoreRevsFile .git-blame-ignore-revs`）。
- clippy は `--all-targets -- -D warnings` で警告ゼロを保つ。検証は CI と同じ stable で回す。

## 版と変更ログ

`Cargo.toml` の `version` が正、版の繰り上げと CHANGELOG.md（日本語、Keep a Changelog、日付は JST）はセットでコミットする。
`main` への直接の push を許す。マージは merge コミットで行う（rebase / squash を使わない）。
crates.io には出さない。使う側は git の依存を `rev` で固定して読むので、タグは付けなくてよい。

## 使う側との関係

- 使う側は `ito-pane = { git = "https://github.com/uchmk/ito", rev = "…" }` で読む。**1 つのアプリの中では、全部のクレートを同じ `rev` にそろえる。**
- **直しはここで入れ、push してから、使う側の `rev` を上げる**（`cargo build` で使う側の `Cargo.lock` も合わせる）。
  公開の API を変えたときは、CHANGELOG に使う側がやることを書く（例: 構造体に欄を足したら、使う側の `Foo { … }` に何を足すか）。
- 使う側とここを手元で一緒に直すときは、使う側で `rev` を書き換えずに、ローカルの ito を差し込む:

  ```sh
  cargo build --config 'patch."https://github.com/uchmk/ito".ito-pane.path="../ito/crates/ito-pane"'
  ```

  クレートごとに 1 つ `--config` を足す。確かめ終えたら ito を push し、使う側の `rev` を上げてから使う側を push する。
- 文書・テストの中の `tsumugi` は、名前の変更から外したもの。どれも動きに関わる:
  - 動きに関わる文字列: OSC の URI（`tsumugi:prompt`・`tsumugi:output`・`tsumugi:img?`）、id（`tsumugi-mark`・`tsumugi-img-`）、一時フォルダの名前（`tsumugi-cast-` など）。
  - XTVERSION の応答（`crates/ito-pane/src/osc.rs`）。アプリの名前と版を渡す形にする件は TODO にある。

## 設計の約束事

- **マルチ OS**: Windows（ConPTY、Win32、UNC）、macOS（Cmd キー）、Linux（X11 / Wayland）の違いは `#[cfg(...)]` のモジュールに閉じ込め、
  使う側から見えない API にする。
- **マルチアーキテクチャ**: x86_64 と ARM64 の 6 ターゲットへのクロスコンパイルを軽く保つ。Pure Rust のクレートを優先し、C のライブラリを抱えるものは避ける。
- **描画とロジックを分ける**: egui に依る部分は機能（`egui`）の後ろに置き、無しでもビルドできるようにする。
- **ディスクやプロセスに触る処理は UI スレッドで実行しない**（ワーカースレッドとチャネル）。
- **egui の版は全部のアプリと一緒に動かす**（新しいマイナー版は API が変わる）。dependabot はここでは上げない。
- アプリに固有のもの（keymap の層、コマンド、設定の画面）はアプリに残し、ここには持ってこない。
