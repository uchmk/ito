# 変更履歴

このファイルの形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版のつけ方は
[セマンティックバージョニング](https://semver.org/lang/ja/) に従う。版はルートの `Cargo.toml` の `[workspace.package]` の `version` が正。

## [0.25.0] - 2026-10-12

### 追加

- `ito_common::shellhook`: `shell-hook`（tsumugi・kura）が出す pwsh・bash・zsh の行（OSC 7 と OSC 133 の印）と、プロファイルに印をつけて入れる・外す（`with_hook`・`without_hook`・`has_hook`）・シェルのプロファイルの場所（`profile`）。アプリの名前と、フォルダを何に使うかの一言だけ引数で渡す。

## [0.24.0] - 2026-10-12

### 追加

- `ito_common::report`: `env` コマンド（`kura env`・`yagura env`）の文面の組み方（`section`・`finish`・`row`・`variables`）。キーをそろえ、長いキーと複数行の値を逃がす書式は kura と yagura で同じだったので、ここに移した。

## [0.23.0] - 2026-10-12

### 追加

- `ito-winres::emit_ico(name, ico)`: 絵を描く関数の代わりに、できあがった `.ico`（中身がすべて PNG のもの）を渡せる。kura の `assets/icon.ico` のように、すでに絵のファイルがあるアプリ向け。

## [0.22.0] - 2026-10-12

### 追加

- `ito-winres`: `.exe` のアイコンと版の情報の `.res` を Rust で書き、リンカーに渡すクレート（yagura の `winres.rs` と `build.rs` を移した）。`build.rs` から `ito_winres::emit("app", &[16, 32, 256], |side| rgba(side))` と呼ぶだけで、パッケージの版と説明を読み、MSVC のターゲットなら `OUT_DIR` に `<name>.res` を書いて `cargo:rustc-link-arg` で渡す。rc.exe は要らず、Linux や macOS からの Windows 向けビルドにも付く。

## [0.21.4] - 2026-10-12

### 変更

- `ito-pane` の合字（`liga.rs`）を harfrust 0.14・skrifa 0.48・vello_cpu 0.3 に書き換えた（Dependabot の #2 で見送った分）。harfrust 0.14 は `ShaperFont`・`Buffer`・`shape()` の形に変わったので、シェイプの呼び出しだけを直した。合字の出し方（`Sub`）と描き方は変わらない。

## [0.21.3] - 2026-10-12

### 変更

- 依存の更新（Dependabot #2）のうち `toml` 1.1.8・`toml_edit` 0.25.17・`fancy-regex` 0.19.2・`miniz_oxide` 0.9.1 を入れた。`toml_edit` 0.25 で `Table::set_position` が `Option<isize>` を取るようになったので、`ito-common` の 2 か所を `Some(place)` に直した。
- `harfrust` 0.14・`skrifa` 0.48・`vello_cpu` 0.3 は入れていない（0.12・0.44・0.1 のまま）。harfrust 0.14 は API が作り直され（`FontRef`・`ShaperData`・`UnicodeBuffer` が無くなり、`ShaperFont` と `shape` に）、`ito-pane` の `liga.rs` の書き換えが要るため、別の作業にした。

## [0.21.2] - 2026-10-12

### 追加

- `scripts/lanes/merge-lanes.py` が yagura の実機レーン（`test/win-*`・`test/arm-*`）を扱う。yagura の表は `TESTING.md` の 1 枚で、行は `- [ ] 3e.12 …`（太字なし）なので、`row_id` が先頭の `3e.12` を行 ID として読む。印は `[x]`・`[~]`。yagura の `required_checks` は空（レーンの PR は Markdown だけで、`ci.yml` が走らない）。ラベルは全部（`retest`・`lane:win` を含む）。

## [0.21.1] - 2026-10-12

### 変更

- `docs/common-spec.md` の「キーの台本」を v0.21.0 に合わせた。`<Now>`・`<RClick>`・修飾キー付きの `<Click>`・`<Hover>`・`<Wheel>`・`<Paste>` を全部のアプリの語に移し、アプリ固有の語は `Dialect` で足すと書いた。コードは変わらない。

## [0.21.0] - 2026-10-12

### 変更

- `ito-keyscript`: kura の `keyscript.rs`（yagura のものの上位集合）を取り込み、kura も使えるようにした。台本の語彙が増えた: `<Now>`・`<Paste>`・`<Paste:text>`（`\n \t \e \\ \xNN`）・`<Key:C-Tab>`（egui のキーそのもの）・`<Click:C-0.5,0.4>`・`<RClick:…>`・`<Hover:…>`・`<Wheel:-3@x,y>`。`<Space>` はキーとテキストの両方で届く。`Step` は kura の形（`Click { right, mods, at }`・`Drag { from, to }`）になり、yagura の `Click(At)`・`Drag(At, At)` は使えない。
- 台本を自前のフレームのループで回すアプリ（kura は窓の落ち着くのを待つ）のために、`Press`・`press`・`label`・`pointer_events`・`paste_text_event`・`STALL`・`stalled_report`・`quit_report`・`closed_report`・`refused_report`・`panic_report`・`install_panic_report`・`save_png` を公開した。`Runner` は `Press` を全部扱い（`<Now>` は待たないだけ）、`<Paste>` のクリップボードは `Runner::clipboard`（`fn() -> String`）で渡す。
- アプリ固有の語は `Dialect`（`parse_with(script, &Dialect { reads, shot_suffixes })`）で足す: `reads` は kura の `<PaneText:name>`・`<PreviewText:name>`（`Step::Read { tag, name }`・`Do::Read`）、`shot_suffixes` は `<Shot:name@preview>` の `@preview`。`parse(script)` は共通の語だけ。

## [0.20.0] - 2026-10-12

### 追加

- `ito-keyscript`（ito#3 の 1）: `--keys` の台本。yagura と tsumugi が同じ中身で持っていた `keyscript.rs` を 1 つにした。`parse`（`2<A-S-Right>*2<Wait:300><Shot:cpu><State:a><Click:x,y><Drag:x1,y1-x2,y2><Quit>`）、`events`（キー → egui のイベント）、`Runner { new, input, due, shot }`、`Step`・`Do`。`<State:名前>` が何を書くか・`<APP>_KEYS_DONE` の名前はアプリが決める（`Runner::new(steps, done)` と `Do::State`）。使う側がやること: `crate::keyscript` を消して `ito_keyscript` に替える。
- `ito-gpu`（ito#3 の 2）: wgpu のバックエンドの選び方。`ito-pane::gpu` にあった `pick_backends`・`auto_backends`・`has_adapter` を、ペインを使わないアプリ（yagura）も使えるよう独立させ、yagura の `gpu_backend::options` をそのまま `options(backend) -> (WgpuConfiguration, Option<String>)` として足した（警告の文は `backend = "…": …` で、`config.toml: ` のような前置きはアプリが付ける）。`ito-pane` の `wgpu` 機能は `ito-gpu` を再エクスポートするだけになり、`ito_pane::gpu::…` の名前は変わらない。
- `ito-palette`（ito#3 の 10 の一部、yagura#15 の 1）: コマンドパレット（`Ctrl+Shift+P`）。`Entry<T>`（題・詳細・同じことをするキー・選んだときの `T`）の一覧を、打った文字が順に含まれるもの（`score`・`search`）に絞り、窓の上に出す `show(ctx, &mut View, &entries, &Style, &Words) -> Option<Answer<T>>`。↑↓・Ctrl+P/N/K/J で動き、Enter で選び、Esc か外のクリックで閉じる。色と文言はアプリが渡す。tsumugi の `palette.rs` の `score`・`search` を一般にしたもの（tsumugi の載せ替えは別）。

## [0.19.2] - 2026-10-12

### 修正

- `ito-prefs::select`: リストをキーボードだけで使えるようにした。開いている間もキーはボタンが持ち、↑↓・Home・End で現在の値から光が動き、Enter / Space で選んで閉じ、Esc は選ばずに閉じてフォーカスを残し、Tab は閉じて次のコントロールへ進む。これまでは Down が先頭の項目へフォーカスを飛ばし（位置が揺れる）、Enter ではポップアップが開いたまま、Tab で開いたまま次へ出ていた（yagura のコンボボックスの不具合）。

## [0.19.1] - 2026-10-12

### 追加

- `ito-prefs::field_id(id)`: `field` の egui の id。アプリが Enter で渡された値を断るとき、`request_focus` でキャレットを戻すのに使う（kura の再テストの (c)）。

## [0.19.0] - 2026-10-12

### 追加

- `ito-common::update`: 「更新を確かめる」の部品（ito#3 の 7）。`check(repo, ours, wake)` が専用のスレッドでシステムの `curl` から GitHub の `releases/latest` を聞き、`Update::Newer(版)` / `Current` / `Unknown`（オフライン・`curl` なし・レート制限）を `Receiver` で返す。比べ方は `judge(body, ours)`（純粋な関数、`tag_name` の `v` を外して 3 つの数で比べる）。Windows では `curl` の窓を出さない。`serde_json` を足した。

## [0.18.0] - 2026-10-12

### 追加

- `ito-common::window`: 窓の位置と大きさを覚える部品（ito#3 の 8）。`Window { x, y, w, h, maximized }`（外枠の位置と内側の大きさ、論理ポイント）、`read(dir)` / `write(dir, &w)` は状態のフォルダー（`state_dir`）の `window.toml` を読み書きする。数が窓として見えないもの（NaN・極端な大きさ・最小化の駐車位置 -32000）は書かず、読まない（`Window::sane`）。`Window::reachable(monitor, grab)` は主モニターの外へ出すぎた窓を断る。egui にも winit にも依存しない。使う側は、起動時に `read` で戻し、動いたら `write` する。

## [0.17.7] - 2026-10-12

### 追加

- `ito-keys`: `display_seq`（と、OS を指定する `display_seq_for`）。macOS（`C` が Cmd）では `<C-q>` を `⌘Q`、`<C-A-x>` を `⌥⌘X` と書き、素の名前つきキーは `<Enter>` のまま。ほかの OS では keymap の書き方（`render_seq`）のまま。kura の `config/keys.rs` にあったものを移した（kura#345 の提案 5）。`human_seq`（`Ctrl+Q` と書く）とは別の綴り。

## [0.17.6] - 2026-10-12

### 修正

- `ito-prefs`: `select`（Language・Scale などのコンボ）にもフォーカスの輪が出る。ボタンと入力欄にはあった。
- `ito-prefs`: `field` が Enter を 2 フレーム続けて返し、有効な値でも 2 回書き込まれていた（使う側の「Saved」のトーストが 2 回出た）。送った値は、欄がもう一度キーを持つまで返さない。

## [0.17.5] - 2026-10-11

### 修正

- `ito-pane`: 日本語の文の中のリンクを、文ごと拾っていた。全角の括弧・かぎ括弧・読点（`（）「」『』【】、。，` など）を単語の区切りにし、URL は最初の ASCII 以外の文字で終わる。`（R:\dev.exe）でできます` は括弧の中のパスだけがリンクになる。

## [0.17.4] - 2026-10-11

### 追加

- `ito-pane::link_across`: 端末が折り返した行（行末の `WRAPLINE`）をまたぐリンクを 1 つの単語として拾い、行ごとのセルの範囲を返す。ホバーの下線・Ctrl / Cmd+クリック・ダブルクリックの選択がこれを使うので、折り返された URL も 1 つのリンクとして扱える。アプリが自分で入れた改行は折り返しではないので、つなげない。`link_at`（1 行だけ）はそのまま。

## [0.17.3] - 2026-10-11

### 修正

- `ito-pane`: ペインがキーを得た・失った知らせ（`ESC [ I` / `ESC [ O`）を、入力の時刻（`Terminal::last_input`）に数えない。数えていたので、focus 報告を求めるプログラム（Claude など）の窓に焦点が戻っただけで、それより前の「待ち」の通知が無効になり、tsumugi のトーストの Allow / Deny が状態を Running と見て何もしなかった。

## [0.17.2] - 2026-10-11

### 追加

- `ito-notify::notifier`: Linux の `notify-send` で、通知のボタンも返せる。`--action=名前=ラベル` を `Action` ごとに渡し（`launch` と同じ `arg` と、システムの dismiss は除く）、`--wait` が出力した名前をコールバックの `arg` に戻す。通知自体のクリックは `launch`。

## [0.17.1] - 2026-10-11

### 変更

- `ito-pane`: https:// などのリンクの上でダブルクリックすると、単語の規則で途中で切れず、リンク全体（`https://host/a?b=c`）を選ぶ。リンクでない所は今までどおり単語。Ctrl / Cmd+クリックで開くのも今までどおり。

## [0.17.0] - 2026-10-11

### 追加

- `ito-notify::notifier`: 答えられる通知。`Notifier` が専用のスレッドを持ち、出した通知をそこで生かすので、アプリが動いている間のクリックはコールバックに届く。`Toast`（題・本文・`tag`（同じ tag は前のを置き換える）・`urgent`（自然には消えない）・`launch`（通知自体のクリック）・ボタン `Action`）。Windows はボタン付きのトースト、`notify-send`（libnotify 0.7.12 以降）は通知自体のクリックを返し、macOS は返せない。ロゴは最初の通知の前に 1 度だけ `fn() -> Option<PathBuf>` で頼む。tsumugi の `alert.rs` の通知の部分を切り出した。

### 変更

- `webhook::send` と `webhook::args` が `Level`（`Waiting`・`Error`・`Done`・`Alert`）を取る。ntfy の `Tags` と `Priority`、JSON の `state` が決まる。送れなかったら 30 秒後に 1 度だけやり直す。tsumugi の `webhook.rs` に揃えた。

## [0.16.0] - 2026-10-11

### 追加

- `ito-notify`: 人に知らせる 3 つの手段。`toast::show`（システムの通知。Windows はトースト、macOS は AppleScript、ほかは `notify-send`）、`sound::play`（`chime`・`low`・`alert`・`default`）と `sound::quiet_time`（Windows の集中モードなど）、`webhook::send`（ntfy・Slack・JSON。システムの `curl` で、スレッドで送る）。tsumugi の `sound.rs` と `webhook.rs` をそのまま切り出した。設定の言葉は `SOUNDS`・`FORMATS`、アドレスの検査は `is_http`。使う側（yagura が最初）が `[notify]` に同じキーを持つ。tsumugi を `ito-notify` に載せ替えるのはこの後。

## [0.15.3] - 2026-10-11

### 変更

- `ito-prefs`: テーマの画面で「OS に合わせる」のとき、一覧を今の OS の明暗に合うテーマだけにする（OS が暗ければダークだけ）。もう一方の明暗のテーマは、OS がそちらに替わったときに選ぶ。

## [0.15.2] - 2026-10-11

### 修正

- `ito-pane`: IME で確定した文字が、確定の瞬間に消えて、プログラムのエコーで左から出直すように見えていた。確定した文字を、カーソルが動くか 0.25 秒たつまで、同じ場所に描いたままにする。

## [0.15.1] - 2026-10-11

### 修正

- `ito-pane`: IME の候補の窓がペインの左上に出ていた。egui-winit が窓の位置に使う `rect` に、ペイン全体でなくカーソルのセルを渡す。

## [0.15.0] - 2026-10-11

### 追加

- `ito-pane`: 入力メソッド（日本語・中国語・韓国語の IME）で打てるようにした。フォーカスのあるペインがカーソルの位置を IME に伝え
  （アプリの入力欄にフォーカスがあるときは伝えない）、変換中の文字をカーソルの位置に下線付きで描き、確定した文字列を
  `input::feed` がシェルへ送る。使う側がやることは無い（`rev` を上げるだけ）。

## [0.14.0] - 2026-10-11

### 追加

- `ito-pane`: `Terminal::log_note(text)`。PTY のログ（`KURA_PTY_LOG` など）に、PTY を渡ったバイトでない 1 行（`note`）を書く。
  ペインが受けて何も送らなかったキーを、ログから見えるようにするため（kura #332）。ログが無ければ何もしない。
- `scripts/lanes/keys.ps1`: `-Isolate`。`lanes.conf` の `isolate=`（空白区切りの環境変数）をそれぞれ、走らせたフォルダの下の空のフォルダに向け、
  機械の設定を読まず書かない。
- `scripts/lanes/keys.ps1`: 前面のシステムのダイアログ（ファイアウォールや Windows セキュリティの確認、`Shell_SystemDialog` など）を見る。
  アプリが動いている間に出たら `blocked.txt` に書き、結果にしない（終了コード 3）。`-Foreground` だけで、今出ているかを 1 行で言う
  （マウスの行の前に。合成のマウスの入力はダイアログの間アプリに届かない。kura #327）。

### 変更

- `ito-pane`: win32 の入力モードで、Ctrl+[ と Ctrl+Space をそれぞれのキーのレコード（VK_OEM_4 と VK_SPACE）で送る。
  前は ESC と NUL の素のバイトで、ConPTY が 2 つを 1 つの Alt+Ctrl+Space と読んでいた（kura #341）。`[` の仮想キーは US の配列を前提にする。
  PTY のログはこのキーを `Ctrl+[` と名前で書く。
- `ito-prefs`: 検索の欄で Enter を押すと、ページを移るだけでなく、検索が照らした最初の行の部品（欄・スイッチ・選択・ボタン）にフォーカスを渡す。
  検索の欄と文字の欄にも、ほかの部品と同じフォーカスの輪を描く（kura #340）。
- `ito-prefs`: 文字の欄がフォーカスを得たら中身を全部選ぶ。打てば置き換わる（前は末尾に足されて `14abc` になった。kura #324）。
- `ito-prefs`: テーマのページの PREVIEW の見出しに、色の一致でなく選んでいるテーマの名前を出し、色を `fg` にする。
  theme.toml が窓を塗り直していると、どの組み込みのテーマとも色が合わず、名前が空だった（kura #321）。
  Follow OS の一覧で今のテーマを照らすのも、色でなく明暗（`Colors::light`）で比べる。

### 使う側がやること

- `rev` を上げる（公開の API は足しただけ）。
- `scripts/lanes.sh sync` で keys.ps1 を写す。`-Isolate` を使うなら `scripts/lanes.conf` に `isolate=` を足す
  （例: kura は `isolate=KURA_CONFIG_HOME YAZI_CONFIG_HOME KURA_STATE_HOME UCHMK_CONFIG_DIR`）。

## [0.13.0] - 2026-10-11

### 追加

- `ito-pane`: 右から左の文字（ヘブライ語・アラビア語など）を読む順に描く。行ごとに Unicode の双方向アルゴリズム
  （暗黙のレベル、段落は左から右。VTE の既定と同じ）で並べ替え、右から左の並びの中の括弧は向きを変える。
  動くのは描画だけで、グリッド・カーソルの位置・選択・コピー・トリガーは書かれた順のまま。ポインターのセル
  （選択・リンク・マウスの報告）とカーソル・リンクの下線は描いた位置に合わせる。並べ替えた行では合字を作らない。
  右から左の文字の無い行は前と同じ。依存に `unicode-bidi`（Pure Rust、機能 `egui` のとき）を足した。

### 使う側がやること

- `rev` を上げる（API は変わらない）。

## [0.12.0] - 2026-10-11

### 追加

- `ito-pane`: `CursorStyle::glide`。真なら、ペインのカーソルが動いたとき前の位置から約 90 ms かけて滑り（着く前に遅くなる）、
  途中でまた動けばその場から向きを変える。ペインを動かしただけでは滑らない（位置をペインの角から持つ）。既定は偽（前と同じ）。
- `ito-pane`: スクリーンリーダー（AccessKit）に、ペインを読み取り専用の端末（`Role::Terminal`）として見せ、値に画面の行
  （行末の空白と、下の空行を除く）を渡す。AccessKit が動いているときだけ組み立てる。

### 使う側がやること

- `rev` を上げる。`CursorStyle { shape, blink }` を書いて組み立てていれば `glide: false` を足す（`from_word` は偽にする）。

## [0.11.0] - 2026-10-11

### 追加

- `ito-pane`: `Terminal::bells`。`drain` が読んだ分でプログラムが鳴らしたベル（BEL）の数。アプリが見せたら 0 に戻す（視覚ベルのため）。
  使う側がやること: `rev` を上げる（欄が増えただけで、`Terminal` を組み立てる側は無い）。

## [0.10.0] - 2026-10-11

### 追加

- `ito-pane`: `set_identity(name, version)`。XTVERSION（`\e[>q`）に答える名前と版を、アプリがプロセスで 1 回だけ決める（最初の呼び出しが勝つ）。
  前は `tsumugi` と ito の版（0.x）を返していたので、tsumugi は版が違い、kura のペインは名前も違っていた。
  呼ばなければ `ito-pane <ito の版>` と答える（前の `tsumugi` から変わる）。
  使う側がやること: `rev` を上げ、`main` の頭の `ito_pane::restrict_dll_search()` の隣で
  `ito_pane::set_identity("<アプリの名前>", env!("CARGO_PKG_VERSION"))` を呼ぶ。

## [0.9.2] - 2026-10-11

### 修正

- `ito-common`: `set_tables` が `[[name]]` の表を全部消して最後に書き直していたので、表の中と上のコメントが消え、間にあった別の表（yagura の `[agent]`）が
  上に寄って見えた（yagura の #11、ホストの追加と削除）。今は表を `name`、無ければ `addr`、無ければ残った順で前の表に合わせ、その場で値だけを書き換える。
  同じ値は触らず、変えた値の後ろのコメントは残す。消す表の上の、空行で離れたコメント（ファイルの見出し）は次の表へ渡す。新しい表は最後の表の後ろに入る。
- `ito-common`: `Common::parse` が最初の型違いで `Err` を返していたので、2 つ目からは警告に出なかった（`scale = "x"` と `theme = 3` で `theme` が出ない。yagura の #11）。
  型違いはどれも警告にし、そのキーだけ `None` にする（`[clock]` は型違いのキーだけを捨てる）。`Err` は TOML として読めないときだけ。
  使う側がやること: `rev` を上げる。`Err` を「型違い」として扱っていたところがあれば、警告の一覧を見る形にする。

## [0.9.1] - 2026-10-11

### 修正

- `scripts/lanes/merge-lanes.py`: 印を 1 つも変えず報告だけを足したレーンの PR を、チェック表の CI（`required_checks`）を待ったままにしていた。
  そのジョブはチェック表が変わったときにしか走らないので、待ちが終わらず、レーンも次を始めなかった（tsumugi の #14、2026-10-11）。
  印の無い PR は、そのジョブを待たずに、走った分が緑ならマージする。使う側がやること: `rev` を上げて `scripts/lanes.sh sync`。

## [0.9.0] - 2026-10-11

### 追加

- `ito-common`: `OsLight`。OS の明暗（Windows はレジストリの `AppsUseLightTheme`）を自分のスレッドで見張り、変わったら `wake` を呼ぶ。
  winit は WM_SETTINGCHANGE で uxtheme の `ShouldAppsUseDarkMode` を、その写しを新しくする前に読むので、egui の `system_theme` が
  OS の切り替えに 1 回遅れていた（tsumugi・yagura の `theme = "system"`）。Windows のほかでは何も読まず `None`。
  使う側がやること: `OsLight::watch(Duration::from_secs(2), move || ctx.request_repaint())` を持ち、`system` の明暗を
  `os_light.light_or(ctx.system_theme() == Some(egui::Theme::Light))` で決める。

## [0.8.2] - 2026-10-11

### 修正

- `ito-pane`: 知らせるトリガー（`set_triggers`）が、プロンプトとそこで打っているコマンドの行にも当たっていた。PSReadLine はキーごとに行を描き直すので、
  `echo Build succeeded` を打つだけで当たり、同じトリガーの 10 秒の間隔を使い切っていた（tsumugi の実機、2.62〜2.65）。
  シェルの OSC 133 の印で、`A`（プロンプトの始め）から `C`（出力の始め）まで、`C` の無い pwsh では `B`（プロンプトの終わり）の後の最初の行までを読まない。
  印を出さないシェルは今までどおり全部の行を読む。改行の無い出力の最後の行は、次のプロンプトの始めで終わる。使う側がやることは無い。

## [0.8.1] - 2026-10-11

### 変更

- TODO: tsumugi（v0.94.0）・yagura（v0.42.0）・kura（v0.103.2）が `scripts/lanes/` の写しを `scripts/lanes.sh sync` で揃えた。

## [0.8.0] - 2026-10-11

### 追加

- `scripts/lanes/lanes.sh`: 写しを作る・確かめるスクリプトそのものも共通にした（`FILES` に入り、自分も写す）。
  ito の場所は `cargo metadata` で、`uchmk/ito` の git の依存のどれからでも探す（kura の `ito-pane` 決め打ちをやめた）。
  ito のクレートが 2 つ以上の `rev` にまたがっていれば落ちる。
  `scripts/lanes.conf` の `files=` で、アプリが取るものだけを選べる（`lanes.sh` は常に入る）。
- `check-ps1.ps1`: `lane-marks.ps1`・`lane-status.ps1` の確かめは、そのファイルがあるときだけ回す。
  アプリの `scripts/check-ps1-app.ps1` があれば読み込んで回す（`Expect` と `$bad` を共有する）。
- `merge-lanes.py`: yagura の規則（レーンは無く、ラベルと票だけ）。規則の `labels` は `True`（全部）か名前の並び。

### 変更

- `merge-lanes.py`: tsumugi の分け前を kura と同じ issue の流れにした（再テストは `retest` の issue、報告の残りは `finding` の issue、
  票は `question` の issue へのコメント。ラベルと票の数えも回す）。tsumugi は v0.94.0 で合わせる。

## [0.7.0] - 2026-10-11

### 追加

- `ito-testcheck`: テストのレーンが押すチェック表を作るクレート。kura の `examples/make-testcheck.rs`・`make-keycheck.rs` の中身を移した。
  - `testcheck::run(&Config)`: `TESTING.md` の表を、`cargo test` が見ている行を外し、訳を添えて、レーンごとの 1 行 1 チェックの表にする。
  - `keycheck::run(&Config)`: yazi 風の `keymap.toml` のバインドを 1 行 1 チェックにする。
  - どちらも付いているチェックを引き継ぎ、`--check`（書かずに、古ければ 1 で終わる）と `--stats` を受ける。出力は kura の旧版とバイト単位で同じ。
  - 使う側は `[dev-dependencies]` に同じ `rev` で足し、example で `Config`（パス・レーン・前書き）を作って `run` を呼ぶ。
- `scripts/lanes/`: アプリのテストのレーンのスクリプト（merge-lanes.py、lane-boot.ps1、lane-status.ps1、lane-marks.ps1、check-ps1.ps1、labels.sh、
  claim.sh、todo-open.sh、push-main.sh、xrun.sh、keys.ps1）の正。kura の写しから、アプリの名前を `scripts/lanes.conf` から読む形にした。
  使う側は `scripts/lanes.conf` を置き、`scripts/lanes.sh sync` で `FILES` の分を写す。
  - `keys.ps1` の `-Kura` は `-Bin` になった（`-Kura` は別名で残る）。
  - verify.sh と CI が、構文（`bash -n`・pwsh）と `merge-lanes.py --self-test` を見る。

## [0.6.1] - 2026-10-11

### 変更

- `docs/common-spec.md` を書き足した: `--keys` の台本の語と状態のファイルの形、版とリリースの決まり、各アプリの対応の表。
  新しいアプリはこの文書を既定にする。コードの変更は無い。

## [0.6.0] - 2026-10-11

### 変更

- `ito-keys`: Ctrl と Cmd をどちらも `C` と書く。`from_egui` は macOS の Cmd と Ctrl をどちらも `C` にする（`<C-q>` は macOS で Cmd+Q、
  Ctrl+Q でも同じ）。`<D-…>` は読むが、合うキーは来ない。`Key::human` は macOS で `C` を `⌘` と出す。
- `ito-keys`: 0.5.0 の `<P-…>`・`<primary-…>` は `<C-…>` と同じに読む。使う側は keymap の `<P-` を `<C-` に直す。
- `docs/common-spec.md` のキーを `<C-…>` の書き方にした。

## [0.5.0] - 2026-10-11

### 変更

- `ito-pane`: `Terminal::record` が、始めた時点の画面を `.cast` の最初の出力（0 秒）として書く。画面の各行を、セルの色と見た目（SGR）ごと
  書き直し、カーソルを元の場所に置く（隠れていれば隠す）。これまでは始めた後の出力だけで、プロンプトで待っている間に始めて何も出さずに止めると
  ヘッダーだけの `.cast` になり、再生しても始めの画面が無かった。使う側の変更は要らない。

## [0.4.1] - 2026-10-11

### 修正

- `ito-common`: 相対の `XDG_CONFIG_HOME`・`XDG_STATE_HOME` を無視する（XDG の仕様のとおり）。`base_dir`・`app_dir`・`state_dir` が作業フォルダからの道を返していた。

## [0.4.0] - 2026-10-11

### 追加

- `ito-pane`: プロンプトの印に 2 つの OSC 8 のリンクを足した。
  - `INPUT_LINK`（`tsumugi:input`）: OSC 133;B（プロンプトの終わり、コマンドを打つ所）から。
  - `PROMPT_REST_LINK`（`tsumugi:prompt-rest`）: B を送るシェルの、2 行目からのプロンプト（`REST_ROWS` = 4 行まで）。

### 修正

- `ito-pane`: 2 行以上のプロンプト（Starship の `~ on main` の下に `>`、pwsh の折り返した長いパス）を、1 つのプロンプトとして数える。
  これまでは行ごとに 1 つと数え、`last_output`（最後の出力のコピー）が空になり、`prompt_lines`（プロンプトへの移動）が 2 行目にも止まった。
  折り返した続きの行の A は前の行の続きとして、B を送るシェルの 2 行目からは `prompt-rest` として読む。
  使う側は、pwsh のフックのプロンプトの後ろに `\e]133;B\e\\` を足すと効く（tsumugi v0.84.0）。
- `ito-pane`: Windows で大きな OSC 1337 の画像（数十 KB）が 10〜15 秒かかっていた。画像の途中の読みは 0 バイトを返し、Windows の PTY は空のパイプでも
  0 を返すので、一度空になった後は読み直さずに alacritty の読みの輪に 0 を返し、次の通知まで止まっていた。0 を見たかを読むごとに戻す。

## [0.3.0] - 2026-10-11

### 追加

- `ito-common`:
  - 倍率: `SCALE_MIN`（0.2）・`SCALE_MAX`（5.0）・`SCALE_STEP`（0.1）・`ScaleTo`・`tidy_scale`・`scale_step`・`parse_scale`（`1.2` と `120%`）・
    `scale_text`（「倍率 120%」）・`scale_change`（common.toml の `scale` を書く `CommonChange`）。
  - common.toml の `scale`（`Common::scale`・`scale_or_one`）。範囲の外は警告を出して丸める。
  - フォルダ: `app_dir(app)`（`<APP>_CONFIG_HOME`、無ければ `<uchmk>/<app>`）・`state_dir(app)`（`<APP>_STATE_HOME`、無ければ OS の状態の場所の
    `uchmk/<app>`）・`env_dir`（空の値は無いのと同じ）・`move_old(old, new)`（古い設定を、新しいものを上書きせずに移す）。
  - `fonts`: 全アプリで同じ字体の並び（kura の順）。`NERD_FONTS`・`JAPANESE`・`JAPANESE_BOLD`・`font_dirs`・`find_file`・`nerd_font`・`read_first`。
- `ito-prefs`: `scale_row`（General の倍率の行）と `Words.scale`。
- `ito-keys`: `Key::human`・`human_seq`。画面に出すキーを OS の名前で書く（`Ctrl+Shift+P`、macOS は `⇧⌘P`）。keymap の `P` を画面に出さないため。
- 共通仕様に、倍率・標準のキー（設定 `<P-,>`・ヘルプ F1・倍率・パレット・終了、JIS の配列）・keymap.toml の形（yazi の形）と `P` の説明・
  置き場所と古い場所からの移し方・環境変数・設定の自動の読み直し・CLI（`--version`・`--help`・`--keys "<script>"`、任意の `mcp`・`shell-hook`）・
  字体・窓（最初は 1280×800、位置と大きさを状態に覚える）を書いた。新しいアプリはこれを既定にする。

### 変更

- 文書とクレートの説明の名前を新しい名前に直した（filer → kura、mimamori → yagura。kura v0.95.0・yagura v0.33.0 で改名）。コードは変わらない。

## [0.2.0] - 2026-10-11

### 追加

- tsumugi v0.81.0（a26aefd）で足された共有のクレート 3 つを、名前を変えて写した。中身と公開の API は a26aefd のときのまま。
  - `ito-common`: 設定のフォルダー、`common.toml`（言語・テーマ・時計）の読み書きと見張り、コメントを残して 1 キーを書き換える道具、時計の文字（機能 `clock`）。
  - `ito-theme`: 組み込みの 13 のテーマと、テーマのファイルの読み方・色の混ぜ方。
  - `ito-prefs`: 設定の画面の枠と部品、Language・CLOCK・Theme の共通のページ。
- uchmk のアプリの共通仕様（`docs/common-spec.md`）を tsumugi から移した。

### 変更

- `ito-i18n` の `base_dir`・`common_path`・`os_language` は `ito-common` の再エクスポートになった（tsumugi v0.81.0 と同じ）。
  使う側の呼び出しは変わらない。

## [0.1.0] - 2026-10-11

### 追加

- tsumugi（34e5cd2、v0.80.0）から 7 つの共有クレートを履歴ごと移し、名前を `tsumugi-*` から `ito-*` に変えた。
  `ito-pane`・`ito-layout`・`ito-i18n`・`ito-keys`・`ito-match`・`ito-ipc`・`ito-mcp`。中身と公開の API は v0.80.0 のときのまま。
  使う側は `tsumugi_X::` を `ito_X::` に書き換え、git の依存の URL を `https://github.com/uchmk/ito` にする。
- `scripts/verify.sh`（`cargo fmt --check`・build・test・clippy を Linux と Windows の型で）と CI（Windows と Linux の test、clippy、fmt）、
  毎週の `cargo audit`、dependabot（egui は全アプリで一緒に上げるので外す）。
- `rustfmt.toml`（yagura と同じ `max_width = 140`・`use_small_heuristics = "Max"`）。切り出しの後に 1 回だけ全体を `cargo fmt` で整形し、
  そのコミットを `.git-blame-ignore-revs` に載せた。
