# ito — Claude 向けメモ

uchmk のアプリが使う共有のクレートを集めたリポジトリ。糸（ito）のように、アプリをつなぐ。Rust、Windows 優先。
使う側は tsumugi（紬、ターミナル）、kura（蔵、ファイラー。旧 filer）、yagura（櫓、システムモニター。旧 mimamori）。fuda（札、TODO）と fude（筆、エディター）も使う予定。

| クレート | 中身 | 主に使う側 |
|---|---|---|
| `ito-pane` | ターミナルのペイン（PTY・ConPTY、キーの変換、OSC、egui の表示） | tsumugi、kura |
| `ito-layout` | 矩形をペインに分ける木と、egui の仕切り | tsumugi、kura、yagura |
| `ito-i18n` | 画面の文言をキーで引く。言語の設定（`common.toml`）を全アプリで共有する | tsumugi、yagura |
| `ito-keys` | yazi のキーの書き方（`<C-a>`・`<A-S-Up>`。`C` は macOS で Cmd） | yagura |
| `ito-match` | 検索の一致の規則（正規表現か、fzf 風の曖昧一致） | tsumugi、kura |
| `ito-ipc` | 動いているアプリへの手元の扉（Unix ソケット・名前付きパイプ）と、長さ付きの serde のメッセージ | tsumugi、kura |
| `ito-mcp` | stdio の小さな MCP サーバ（非同期ランタイムなし） | tsumugi、kura |
| `ito-common` | 設定のフォルダー、`common.toml`（言語・テーマ・時計）の読み書きと見張り、TOML の 1 キーの書き換え、時計の文字（機能 `clock`） | tsumugi、yagura |
| `ito-theme` | 組み込みの 13 のテーマ、テーマのファイルの読み方、色の混ぜ方 | tsumugi、yagura |
| `ito-prefs` | 設定の画面の枠と部品、Language・CLOCK・Theme の共通のページ | tsumugi、yagura |
| `ito-testcheck` | テストのレーンが押すチェック表の生成（`TESTING.md` → レーンごとの表、`keymap.toml` → キーの表）。アプリの `examples/` が `Config` を渡して `run` を呼ぶ。依存は serde と toml だけ | kura（dev-dependencies） |

**`scripts/lanes/`** は、アプリのテストのレーンが使うスクリプトの正（`FILES` に並べたもの: merge-lanes.py、claim.sh、push-main.sh、xrun.sh、keys.ps1 など）。
アプリの `scripts/lanes.sh sync` が、固定した `rev` のここから自分の `scripts/` へバイト単位で同じものを写し、`lanes.sh check` は写しが違えば落ちる。
アプリごとの値（名前・クレート・実行ファイル・`--keys` の環境変数・状態のフォルダー・TODO の印）はアプリの `scripts/lanes.conf` から読むので、
ここにアプリの名前を書き込まない。ここではアプリに対して走らせられないので、verify.sh と CI は構文と `merge-lanes.py --self-test`、pwsh の
`check-ps1.ps1` だけを見る。直したら push し、使う側の `rev` を上げて `scripts/lanes.sh sync` を流す。

**uchmk のアプリの共通仕様**（common.toml・設定の画面・テーマ・時計）は [docs/common-spec.md](docs/common-spec.md)。全部のアプリがこれに従う。
仕様を変えたらこの文書も直す。

2026-10-11 に tsumugi（34e5cd2、v0.80.0）から履歴ごと切り出した（`git filter-repo`）。`ito-common`・`ito-theme`・`ito-prefs` と
`ito-i18n` の直しは、その後の tsumugi v0.81.0（a26aefd）から写した（0.2.0）。それより前のペインの履歴は kura にある。

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

## 持ち主の PC への依頼（needs-pc）

クラウドのセッションは、持ち主の Windows の PC でしかできないことを、ラベル `needs-pc` の Issue で PC のセッションに頼む
（雛形 `.github/ISSUE_TEMPLATE/needs-pc.yml`）。

- PC に頼むもの: 実機の確認（窓・キー・ConPTY・トースト・見た目）、Windows でだけ出る失敗の再現、リリース（`release.yml` を回す）、
  PC の環境（`%APPDATA%\uchmk` の設定・字体・タスク スケジューラ・wintest のレーン）。実装・テスト・文書・マージはクラウドのまま。
- 書くこと: やること、終わりの条件、版かコミット、頼んだセッションの題。会話の記録を読まなくても進められるように。
- PC のセッションは 30 分ごとくらいに開いている `needs-pc` を拾い、Issue に結果を書いて閉じ、頼んだセッションにもメッセージで知らせる。
  クラウドのセッションからは PC のセッションにメッセージを返せないので、PC への連絡は Issue だけで行う。
- 開いている Issue と PR は、持ち主の GitHub Project「uchmk」（https://github.com/users/uchmk/projects/2）に 4 つのリポジトリの分が集まる。
  kura は Project の自動の追加で、ito・yagura・tsumugi は PC の needs-pc の巡回（毎時 :07 と :37）が足す（ラベル `lane-status` は除く）。
  クラウドのセッションは個人の Project に触れないので、Issue を立てるだけでよい（30 分以内に入る）。
- PC のセッションはクラウドが作業しているブランチや main のコードを直接いじらない。直しが要れば担当のセッションに頼む。

## 設計の約束事

- **マルチ OS**: Windows（ConPTY、Win32、UNC）、macOS（Cmd キー）、Linux（X11 / Wayland）の違いは `#[cfg(...)]` のモジュールに閉じ込め、
  使う側から見えない API にする。
- **マルチアーキテクチャ**: x86_64 と ARM64 の 6 ターゲットへのクロスコンパイルを軽く保つ。Pure Rust のクレートを優先し、C のライブラリを抱えるものは避ける。
- **描画とロジックを分ける**: egui に依る部分は機能（`egui`）の後ろに置き、無しでもビルドできるようにする。
- **ディスクやプロセスに触る処理は UI スレッドで実行しない**（ワーカースレッドとチャネル）。
- **egui の版は全部のアプリと一緒に動かす**（新しいマイナー版は API が変わる）。dependabot はここでは上げない。
- アプリに固有のもの（keymap の層、コマンド、設定の画面）はアプリに残し、ここには持ってこない。
