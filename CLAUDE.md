# ito — Claude 向けメモ

uchmk のアプリが使う共有のクレートを集めたリポジトリ。糸（ito）のように、アプリをつなぐ。Rust、Windows 優先。
使う側は tsumugi（紬、ターミナル）、kura（蔵、ファイラー。旧 filer）、yagura（櫓、システムモニター。旧 mimamori）。michi（道、Git のクライアント）も使う。fuda（札、TODO）と fude（筆、エディター）も使う予定。

| クレート | 中身 | 主に使う側 |
|---|---|---|
| `ito-pane` | ターミナルのペイン（PTY・ConPTY、キーの変換、OSC、egui の表示） | tsumugi、kura |
| `ito-layout` | 矩形をペインに分ける木と、egui の仕切り | tsumugi、kura、yagura |
| `ito-i18n` | 画面の文言をキーで引く。言語の設定（`common.toml`）を全アプリで共有する | tsumugi、yagura |
| `ito-keys` | yazi のキーの書き方（`<C-a>`・`<A-S-Up>`。`C` は macOS で Cmd） | yagura |
| `ito-match` | 検索の一致の規則（正規表現か、fzf 風の曖昧一致） | tsumugi、kura |
| `ito-ipc` | 動いているアプリへの手元の扉（Unix ソケット・名前付きパイプ）と、長さ付きの serde のメッセージ | tsumugi、kura |
| `ito-mcp` | stdio の小さな MCP サーバ（非同期ランタイムなし）。窓との扉は `ito-ipc::door` | tsumugi、kura、michi |
| `ito-common` | 設定のフォルダー、`common.toml`（言語・テーマ・時計）の読み書きと見張り、TOML の 1 キーの書き換え、時計の文字（機能 `clock`）、`env` の文面（`report`）、`shell-hook` の行（`shellhook`）、状態のフォルダーのファイルの読み書き（`kept`: `read`・最新を残す別スレッドの `Writer`・`copy_old`）、他のアプリの exe の探し方（`apps`: `[apps]`・自己登録・PATH）、他のアカウントが書けるか（`access`: `others_can_write`）、窓の場所の保存と戻し方（`window`、古い形も読む `load`、追跡の `Tracker`、アプリの手順を束ねる `Saved`。egui から読むのは機能 `egui`） | tsumugi、yagura、kura |
| `ito-fonts` | 同梱の等幅フォント（JetBrains Mono、SIL OFL）と egui への登録（機能 `egui`）。`common.toml` の `mono_font`（`bundled` が既定、`system`）で選ぶ | kura、tsumugi、michi |
| `ito-theme` | 組み込みの 20 のテーマ、テーマのファイルの読み方、色の混ぜ方 | tsumugi、yagura |
| `ito-prefs` | 設定の画面の枠と部品、Language・CLOCK・Theme の共通のページ | tsumugi、yagura |
| `ito-openin` | 別のプログラムで開く（コマンドラインのひな形 `{path} {line} {left} {right} {base} {merged}`、窓を出さない起動、他のアプリを呼ぶ `app::open/launch`、エディターの行の渡し方の表と `[line_args]`、diff / merge ツールの表と見つけ方、`[open]`・`[line_args]`・`[diff_tools.<名>]` の読み書きと設定画面の `Edit`）。設定画面のカードは `ito-prefs` の `open_card` | kura、michi |
| `ito-notify` | 人に知らせる 3 つの手段（システムの通知・音・webhook（ntfy / Slack / Discord / JSON））。アプリ自前の短い音を頼まれた瞬間に鳴らす `player`（Windows・macOS のみ、Linux は `Err`） | yagura、fude（tsumugi も載せ替える） |
| `ito-export` | SVG の文字列のページを PNG・複数ページの PDF（文字は文字のまま）・印刷にする（resvg・svg2pdf、純 Rust。止まるのでワーカースレッドから） | fude（kura・michi も） |
| `ito-ime` | 自前で描く入力行の入力メソッド（候補窓の位置・縦書きは列の右隣・変換中の文字を覚えて下線で描く・確定・変換中のキーを奪わない）。`ito-keyscript` の `<Preedit:>`・`<Commit:>` と組む | fude、ito-pane |
| `ito-keyscript` | `--keys` の台本（`<Wait>`・`<Now>`・`<Shot>`・`<State>`・`<Click>`・`<Wheel>`・`<Paste>`・`<Key:>`・`<Size:WxH>`・`<Theme:名前>`・`<Lang:ja>`…）の解析（アプリ固有の語は `Dialect`）、egui の入力への流し込み（`Runner`、または自前のループ向けの `Press`）、終わり方の報告・panic の報告・PNG の保存 | yagura・kura（tsumugi も載せ替える） |
| `ito-gpu` | wgpu のバックエンドの選び方（Windows は GL を先に）と `options`。`ito-pane::gpu` はこれの再エクスポート | yagura、kura |
| `ito-proc` | 止められる子プロセス（stdout/stderr をスレッドで読む、stderr は `\r`・`\n` で 1 行ずつ渡す、止めると子のプロセスの木ごと殺して `Stopped`。1 行ずつ渡す `run_lines`。Unix はプロセスグループ、Windows は `taskkill /T`） | michi（minato も） |
| `ito-power` | 電源と眠り（`Awake::hold` で動かしている間 PC を眠らせない・`wake_at` で予定の時刻に起こす／起こせない理由 `WhyNot`・`sleep_after` で何分で眠る設定か。Windows は電源要求・再開タイマー・電源プラン。macOS は `caffeinate`、Linux は `systemd-inhibit`（起こすのは未対応）） | koyomi（yagura・kura も） |
| `ito-eventlog` | システムのログに 1 行書く（`Log::open(source)`・`write(Level, id, text)`。Windows はイベント ログ（アプリケーション）、ほかは syslog の `logger`） | koyomi |
| `ito-tray` | タスクトレイの常駐アイコン（`Tray::new(icon, tip)`・`set_state` で色と記号のバッジ（`ito-a11y::State`）・`set_tip`・`set_menu`・押した・選んだは `events()` のチャンネル、`new_with` で起こす関数も渡せる。Windows のみ、ほかは `Unsupported`） | koyomi（tsumugi の server も） |
| `ito-secret` | 秘密（Webhook のアドレス・パスワード）を OS の資格情報ストア（Windows 資格情報マネージャー・macOS キーチェーン・Linux `secret-tool`）に置く `get`/`set`/`delete`、ログ用の `mask`。ファイルには退避しない。`ito-notify::webhook` の `secret:` 参照が使う | koyomi（yagura・tsumugi も） |
| `ito-palette` | コマンドパレット（一覧・文字の順の一致・egui の重ね表示） | yagura（tsumugi も載せ替える） |
| `ito-select` | 描画で並べた文字（ガラスのレイアウト）をマウスの範囲選択でコピーする（プレビュー・詳細欄のように、部品でなく painter で描く窓向け） | kura、minato |
| `ito-browser` | ペインの横のウェブのページ（Windows・macOS は OS の WebView を窓の子に、Linux はシステムのブラウザへ）、egui のアドレスの欄、打った行の読み方 | tsumugi・minato |
| `ito-testcheck` | テストのレーンが押すチェック表の生成（`TESTING.md` → レーンごとの表、`keymap.toml` → キーの表）。アプリの `examples/` が `Config` を渡して `run` を呼ぶ。依存は serde と toml だけ | kura（dev-dependencies） |
| `ito-winres` | `.exe` のアイコンと版の情報の `.res`（rc.exe なしで書く）と、`build.rs` から呼ぶ `emit(name, sides, rgba)`。MSVC のターゲットだけ、クロスビルドでも付く |
| `ito-watch` | フォルダーの変更をその場で受ける（`notify` を別スレッドで。ネットワークの共有は見ない・見られないときは `Watching(false)` で呼ぶ側が定期に見に行く・最新の願いだけ通す） | michi |
| `ito-guard` | コミットの前の見張り（守る名前のブランチ `*`、ステージされたファイルの大きさ・鍵・`.env`・中のトークン）。Git は知らない（`Entry` の一覧と読む関数を渡す） | michi |
| `ito-glossary` | 専門語の表（やさしい言い方・ホバーの説明・日英）、`easy` の表示、表の検査 `check`、点線のホバー（機能 `egui`）。語はアプリの表 | michi、tsumugi |
| `ito-syntax` | コードの色分けの種類（path と行 → トークンの種類のバイト範囲。syntect の純 Rust + two-face、文法は 1 プロセスで 1 回。`Stream` が行をまたぐ状態を持つ） | michi、kura |
| `ito-guide` | 初回・困りごとのガイド（Problem / Step、OS ごとの `Fix`、開閉の `Guide`、キーを取らない egui のパネル `show`（機能 `egui`）、`Level`） | michi |
| `ito-a11y` | 色だけに頼らない状態の印 `State`（記号・読み上げの名前）と、スクリーンリーダー向けの `say`・`announce`（機能 `egui`）。高コントラストの色は `ito_theme::Colors::high_contrast` | 全アプリ |
| `ito-graph` | コミットの樹形図を行ごとに並べる（レーン・辺・ブランチの色・曲線）と、egui の描画（機能 `egui`）。Git は知らない | michi |
| `ito-bugreport` | 不具合の報告（アプリの issue form に、版・OS・アプリが知っていることを埋めた URL。F12 から開く。全アプリで同じ行・同じ順） | kura（yagura・tsumugi は載せる予定） |
| `ito-diff` | 2 つの文字列の行の差分（`diff_lines`・畳む `fold`・`counts`。純 Rust の `similar`。Git は知らない）。`ito-confirm` の `Question::diff` が描く | yagura、kura、koyomi |
| `ito-confirm` | やる前の確認の重ね表示（行・本文・差分・注・ボタン。押された番号を返す。3 つ以上はカード: 動詞の見出し・結果の数字・戻せるか・キーの札、おすすめは緑・戻せないものは赤） | yagura、kura |
| `ito-help` | キーのヘルプ（`ito-keymap` の一覧を見出しとキーと説明の行にして、渡された矩形に描く） | yagura、kura |
| `ito-console` | Windows の窓のアプリの前に置くコンソールの前段（`<app>.exe` の横の `<app>.com`、`front`。文字で答えるコマンドは待って渡し、ほかは窓が開いたら戻る）と、答えを書く `out`・`err`（誰も読まないパイプでも panic せずに捨てる） | kura、yagura、tsumugi、minato、michi |
| `ito-titlebar` | 窓の自前のタイトルバー（ドラッグ・ダブルクリックで最大化・最小化/最大化/閉じる、中央のパレットの箱とその右のベル（未読のバッジ）、左右にアプリの枠、枠なし窓の `resize_edges`）。色は `ito_theme::colors()`。文言とキーの表示は呼ぶ側から渡す | tsumugi、michi、minato |
| `ito-statusbar` | 窓の下のステータスバー（`show`: 側の色と上の線・左にアプリの項目・右端に時計（等幅・一番強い文字の色・ホバーで `Clock::full`・分が変わるときだけ再描画）・倍率を変えた直後の `scale_text`（`scale_changed`）。項目の `item`・`link`・`divider`・`dot`。`opacity` で入力中に薄くできる）。色は `ito_theme::colors()`。設定の画面を開いている間はアプリが隠す | fude、michi、minato、tsumugi |
| `ito-notices` | ベルが開く通知の一覧（新しい順・未読は明るく・時刻と種類の色の点・行き先 `go` を押すと `Action::Open`・↑↓/Enter/Esc・すべて既読）と、バッジ用の `unread`/`any_error`。文言は `Words` で渡す。色は `ito_theme::colors()`。トーストの押しは `ito-notify` の `Toast::open` | michi、tsumugi |

**新しいアプリは `pwsh -NoProfile -File scripts/new-app.ps1 -Name <app> -Description "…"`** で `templates/app` から作る（ito の必須のクレート・窓・テーマ・F12・`.com`・bug form・testcheck 入り。`-Rev` は push 済みの ito のコミット）。`scripts/lanes/ito-first.py`（`ito-first.txt` の禁止、`ito-required.txt` の必須）が、アプリが ito を使わず独自に書くのを落とす。例外はアプリの `scripts/ito-first.allow`（`<crate> <理由>`）。ここ（提供側）では `--self-test` だけ回す。ito に無いものは `crate-request` の Issue で頼む。

**`scripts/lanes/`** は、アプリのテストのレーンが使うスクリプトの正（`FILES` に並べたもの: merge-lanes.py、claim.sh、push-main.sh、xrun.sh、keys.ps1 など）。
アプリの `scripts/lanes.sh sync` が、固定した `rev` のここから自分の `scripts/` へバイト単位で同じものを写し、`lanes.sh check` は写しが違えば落ちる。
アプリごとの値（名前・クレート・実行ファイル・`--keys` の環境変数・状態のフォルダー・TODO の印）はアプリの `scripts/lanes.conf` から読むので、
ここにアプリの名前を書き込まない。
`scripts/lanes/workflows/*.yml` はワークフローの雛形（`{{app}}`・`{{crate}}`・`{{bin}}`）で、アプリの `lanes.conf` の `workflows=` に並べたものを `lanes.sh sync` が `.github/workflows/` に書き、`check` が違いを落とす（並べないアプリは自分のワークフローのまま）。ここではアプリに対して走らせられないので、verify.sh と CI は構文と `merge-lanes.py --self-test`、pwsh の
`check-ps1.ps1` だけを見る。直したら push し、使う側の `rev` を上げて `scripts/lanes.sh sync` を流す。

**uchmk のアプリの共通仕様**（common.toml・設定の画面・テーマ・時計）は [docs/common-spec.md](docs/common-spec.md)。全部のアプリがこれに従う。
仕様を変えたらこの文書も直す。

2026-10-11 に tsumugi（34e5cd2、v0.80.0）から履歴ごと切り出した（`git filter-repo`）。`ito-common`・`ito-theme`・`ito-prefs` と
`ito-i18n` の直しは、その後の tsumugi v0.81.0（a26aefd）から写した（0.2.0）。それより前のペインの履歴は kura にある。

- Cargo の workspace。クレートは `crates/` の下に置く。版はルートの `Cargo.toml` の `[workspace.package]` で 1 つ。
- **push の前に `scripts/verify.sh` を 1 回回す。**最後の行が `ALL OK: …` なら push してよい（`cargo fmt --check`、build `--locked`、test、clippy を Linux・`x86_64-pc-windows-msvc`・`aarch64-apple-darwin` の 3 つで `-D warnings`。どの機械からでも 3 つとも回る。Mac の外の macOS は objc2 が C を要るので cargo-zigbuild で（無ければ PyPI から入れる））。

## 作業ルール

- 返答は日本語。コード・コメント・コミットメッセージは英語。
- コミットと push は、`scripts/verify.sh` が `ALL OK` のときに、頼まれなくてもしてよい（`main` へ直接。force push・履歴の書き換えはしない）。版と CHANGELOG はセット。push したら、使う側のセッションに rev を知らせる。
- 改行は LF。スクリプトで書き換えるときは改行を変えない（Python なら `newline=''`）。
- やることは [TODO.md](TODO.md) に書く（`【人】` は持ち主の作業）。
- **整形は `cargo fmt` で揃える**（tsumugi・kura と違うところ）。設定は yagura と同じ `rustfmt.toml`（`max_width = 140`、
  `use_small_heuristics = "Max"`）で、手で整形していた形（`Self { a, b, c }` を 1 行）に近い。CI と verify.sh が `cargo fmt --check` で見る。
  切り出しの後に 1 回だけ全体を整形したコミットは `.git-blame-ignore-revs` に載せてある（`git config blame.ignoreRevsFile .git-blame-ignore-revs`）。
- clippy は `--all-targets -- -D warnings` で警告ゼロを保つ。検証は CI と同じ stable で回す。
  CI の `clippy` も Linux の機械から Linux・`x86_64-pc-windows-msvc`・`aarch64-apple-darwin` の 3 つを見る（macOS は 2026-10-16 から、verify.sh と同じく cargo-zigbuild で）。
  macOS で C が要るのは objc2-exception-helper（wry が objc2 の `exception` を入れ、`setURLSchemeHandler` の呼び出しを包む）だけ。
  Rust の代わりは無い（stable の Rust は Objective-C の例外を捕まえられない）ので、Mac の外では zig を C のコンパイラーにする。
- `Build`（`.github/workflows/build.yml`、2026-10-16 から）は main への push で、全部のクレートとそのテスト・例を 6 つのターゲット
  （Windows x64・ARM64、macOS ARM64・x64、Linux x64・ARM64）でビルドしてリンクする。clippy はリンクしないので、macOS と ARM64 のリンクはここでしか見ない。
  何も上げず、テストも回さない（テストは ci.yml、macOS のテストは回さない。kura v0.75.25 の持ち主の判断）。
- `Checklists`（`.github/workflows/checklists.yml`、2026-10-16 から）は ito-testcheck を変えたときに、kura の main の 3 つの確認
  （`make-keycheck`・`make-testcheck`・`--lane linux`、どれも `--check`）を、この ito-testcheck を `[patch]` で差し込んで回す。
  赤なら、kura のチェックリストの書き方が変わる。間違いなら直す。意図した変更なら、kura が `rev` を上げて作り直すまで赤のまま（手で回し直す）。

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
