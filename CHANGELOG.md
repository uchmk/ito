# 変更履歴

このファイルの形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版のつけ方は
[セマンティックバージョニング](https://semver.org/lang/ja/) に従う。版はルートの `Cargo.toml` の `[workspace.package]` の `version` が正。

## [0.125.0] - 2026-10-29

### 追加
- `ito-common`: `access`（uchmk/ito#66、koyomi の依頼）。`others_can_write(path) -> io::Result<Vec<String>>` が、そのパスと上のフォルダ（ホームの下ならホームまで）を、
  今のユーザーとシステムのアカウントのほかに誰が書き換えられるかを返す。空なら本人だけ。`findings(path)` はどのパスで見つけたかも返す。
  設定のファイルのコマンドを走らせるアプリが、他人が書けるファイルを実行しないために使う。
  - Windows: DACL の許可の項目（書き込み・追記・削除・`WRITE_DAC`・`WRITE_OWNER`。上のフォルダでは子の削除・削除・その 2 つ）を、本人・Administrators・SYSTEM・TrustedInstaller 以外について数える。所有者もその 4 つのどれでもなければ数える。拒否の項目は見ない。名前は `DOMAIN
ame`。
  - Unix: グループ・その他が書ける（スティッキーのフォルダは除く）、または所有者が本人でも root でもない。Linux では自分の主グループによる書き込みは除く（umask 002 の既定）。
  - テスト用に `open_to_everyone(path)`（Windows は Everyone に変更を許す、Unix は `o+w`）。
  使う側がやること: 無し（足しただけ）。Windows の `windows` クレートの機能 `Win32_Security*`・`Win32_System_Threading` と、Unix の `libc` が `ito-common` の依存に増える。

## [0.124.6] - 2026-10-29

### 修正
- `templates/app/SECURITY.md`: tsumugi 専用の方針（tsumugi の advisory のリンク、サーバー・OSC の範囲）をやめ、ito・yagura・michi・minato と同じ、アプリ名の入らない `SECURITY.md` にした（uchmk/ito#64、fude の報告）。kura・tsumugi のような範囲の節が要るアプリは自分で書く。
- `scripts/new-app.ps1`: 終わりに、GitHub のリポジトリを作ったあとに回す 4 つの `gh api`（Dependabot alerts・security updates・private vulnerability reporting・CodeQL の default setup `languages: [actions]`）を表示する。
  使う側がやること: 無し。新しいアプリを作るときに表示を見る。

## [0.124.5] - 2026-10-29

### 修正
- `ito-prefs`: 設定の画面が `ui.max_rect()` ではなく `ui.available_rect_before_wrap()` を使うようにした（uchmk/ito#63、yagura の報告）。上のパネル（タイトルバー）の下にあるルートの ui で呼ぶと、画面がバーの上に描かれて、閉じるボタンなどが消えていた。
- `ito-notices`: 「すべて既読」のボタンと各行の上でポインターを指の形にした（uchmk/ito#62、minato の報告）。
  使う側がやること: 無し。rev を上げるだけ。

## [0.124.4] - 2026-10-29

### 修正
- `templates/app`: `dependabot.yml` が eframe の minor・major の更新も無視するようにした（uchmk/ito#60）。egui だけ無視して eframe を無視していなかったため、eframe の単独の更新が ito の egui と別の egui を引き込んで、ビルドが通らなかった（fude#1）。
  使う側がやること: 既に作ったアプリ（kura・michi・minato・yagura など）の `.github/dependabot.yml` の `egui*` の隣に同じ規則を足す。rev の更新は要らない。

## [0.124.3] - 2026-10-29

### 修正
- `ito-prefs`: 窓が低いと、ページの一覧の最後（About）が下の「config.toml を開く」ボタンに重なっていたのを直した（tsumugi#323）。一覧は縦にスクロールし、ボタンの上で止まる。窓が狭いときは、ページの左右の余白を 48 から 16 まで縮めて、行の幅を保つ。

## [0.124.2] - 2026-10-28

### 修正
- `ito-prefs`: 検索欄の Esc が実アプリで効いていなかったのを直した（uchmk/ito#58、fude の報告）。egui は Esc でフォーカスを外してから描くので、`has_focus` では見分けられなかった。`State::search_held`（前のフレームの終わりに検索欄がキーを持っていたか）で見分け、Esc で問いを消し、フォーカスを検索欄に戻す。
  使う側がやること: `State` を `State { page, query, held }` と書いていれば `search_held: false` を足す（`State::default()` なら何もしない）。

## [0.124.1] - 2026-10-28

fude の続きの 4 つ（uchmk/ito#56〜#59）。

### 追加
- `ito-prefs`: `default_order_grouped(has)`。アプリのページを先に、共通のページを後ろにそろえる（ナビを `Extras::groups` で分けるアプリ向け。見出しが 2 回出ない）。`docs/common-spec.md` に書いた（uchmk/ito#59）。
- `ito-prefs`: `in_results(ui)`。検索が全ページを読んでいる間の `body` で、ページの外のもの（ボタンなど）を出さない判断に使う。

### 修正
- `ito-prefs`: 検索結果で `field` が同じ id を 2 回描いて衝突していたのを直した（見えない 1 回目は大きさだけ取る）（uchmk/ito#56）。
- `ito-prefs`: 検索結果では通知のページの「試しに送る」「既定に戻す」を出さない（uchmk/ito#57）。
- `ito-prefs`: 検索欄にフォーカスがあるときの Esc は、まず問いを消す（フォーカスは残る。`Extras::results` を使うとき）（uchmk/ito#58）。

## [0.124.0] - 2026-10-28

fude の 4 つの crate-request（uchmk/ito#52〜#55）。

### 追加
- `ito-titlebar`: `theme_tag(ui, rect, dark_now, &ThemeWords{dark, light, hover}) -> ThemeTag{clicked, end}`。今の見た目（ダーク/ライト）を出す小さな枠のタグで、押すと切り替えを頼む。フォーカスの輪あり。`right` の矩形の左端に置く（uchmk/ito#52）。
- `ito-prefs`: `flip_theme((theme, dark_theme, light_theme), light_now) -> Vec<CommonChange>`。今がライトなら dark_theme、ダークなら light_theme を `theme` に書く（OS に合わせる設定は明示の設定になる）。タイトルバーのタグとパレットの「テーマを切り替える」が同じ関数を使う（uchmk/ito#52）。
- `ito-palette`: `Entry::group` と `Entry::in_group("…")`。行の左に薄く出るグループ名で、検索はグループ名もタイトルといっしょに見る（グループ名を打つとそのグループが全部出る）（uchmk/ito#53）。
- `ito-prefs`: `show_with(…, Extras, body)`。`show` は `Extras::default()` で呼ぶだけなので動きは同じ。
  - `Extras::groups`・`shared`・`shared_tag`: ナビの見出し（アプリのページ名と「共通（ito）」）と、共通のページの題の横の札（uchmk/ito#55）。
  - `Extras::back`: ページの頭の「← 〜へ戻る  Esc」。押すと Esc と同じ（`Out::close`）（uchmk/ito#55）。
  - `Extras::results`（`Results{head, lead, none}`）: 検索がすべてのページの当たりを 1 枚に出す。ヒットの無いページはナビで薄くなり、ページごとの件数は今までどおり。カードの見出しは「ページ › カード」。Esc は先に検索を消す（uchmk/ito#54）。

### 変更
- `ito-prefs`: `show` の `body` と `section` の `rows` が `FnOnce` から `FnMut` になった。検索の結果は、当たりの無いカードを見えない描画で 1 回試してから省くため。副作用を持たせないこと。
  使う側がやること: `body` や `rows` の中で捨てる（move out する）値があれば、clone するか外に出す。ほとんどのアプリは何もしない。
- `ito-palette`: `Entry` に欄 `group` を足した。
  使う側がやること: `Entry { … }` と欄を並べて作っているところは `group: String::new()` を足す（`Entry::new` は変わらない）。
- 自前の行を描くアプリで、検索の当たりを光らせているところは、光らせるときに `ito_prefs` の行と同じく当たりを知らせる必要がある（`row`・`keyed_row` を使っていれば何もしない）。

## [0.123.0] - 2026-10-28

### 追加
- `scripts/lanes/workflows/release.yml`: 端末版（TUI）の配布物。`scripts/lanes.conf` に `tui_crate=<クレート>` と `tui_bin=<実行ファイル>` を書くと、新しいジョブ `tui` が Linux x64・ARM64 でビルドし、`<tui_bin>-<tag>-linux-{x64,arm64}.tar.gz` を同じリリースに付ける（Windows・macOS は付けない）。書かないアプリではジョブごと飛ばされ、何も変わらない。`lanes.sh` の雛形に `{{tui_crate}}`・`{{tui_bin}}` を足した。`sums` は `tui` のあとに走る。
- `scripts/lanes/release-check.sh`: `tui_bin` があるアプリは配布物を 8 つ（zip 2 + tar.gz 6）と数える。
  使う側がやること: yagura は `lanes.conf` に `tui_crate=yagura-tui`・`tui_bin=yagura-tui` を足し、`rev` を上げて `scripts/lanes.sh sync`。ほかのアプリは何もしない。

## [0.122.2] - 2026-10-27

### 修正
- `templates/app`: 雛形のアプリに `<app> env`（版・ファイル・最後の起動・環境変数。窓なし、10 行以上）を足した（uchmk/ito#50）。`release.yml` の「Check .com」が `<app>.com env` を要るのに雛形に無く、fude の v0.10.1 のリリースが落ちていた。窓を開くとき `runinfo` を保存し、`.com` の前段のテストにも `env` を足した。
- `templates/app` の `ci.yml`: `scripts/lanes.sh check` を `bash scripts/lanes.sh check` にした（uchmk/ito#51）。Windows（`core.filemode=false`）で作ると `lanes.sh` が 100644 でコミットされ、ubuntu で Permission denied（exit 126）になっていた。`new-app.ps1` も `scripts/*.sh` を index で `+x` にする。
  使う側がやること: 前に作ったアプリは `git ls-files -s scripts/lanes.sh` が 100644 なら `git update-index --chmod=+x scripts/lanes.sh` をコミットする。`<app> env` が無いアプリは、minato の `envreport.rs` かこの雛形の `envreport.rs` を写して `env` を足す。

## [0.122.1] - 2026-10-27

### 修正
- `ito-statusbar`: 窓が狭いと左の項目が右（設定のリンク・時計）の下に潜り込んでいた（uchmk/ito#49）。右端を先に置き、左はその手前までの矩形に切り詰める（はみ出す分は描かない）。`item` は入りきらないとき末尾が `…` になる。使う側がやることは無い。

## [0.122.0] - 2026-10-27

### 追加
- 新しいクレート `ito-statusbar`: 全アプリのステータスバー（uchmk/ito#48）。`show(ui, &Spec{clock, lang, opacity}, left, right)` が下のパネルを埋め、側の色と上の線・左にアプリの項目・右端に時計（等幅・一番強い文字の色・ホバーで `Clock::full`・`Clock::next_minute` で再描画）を描く。`right` は時計（と倍率の文字）の左から左向きに並べる。倍率を変えたら `scale_changed(ctx, scale)`（2 秒間 `scale_text` を出す）。項目は `item`・`link`・`divider`・`dot`。`opacity` は入力中に UI を薄くするアプリ（fude）向け。設定の画面を開いている間に隠すのはアプリ。
  使う側がやること: michi・minato・tsumugi は自前の枠と時計を `show` に置き換える（`key_bar` は絶対位置で描いているので、`left` の中で `item`/`link` に直す）。fude は新しく足す。

## [0.121.0] - 2026-10-27

### 変更
- `ito_export::print` が `Result<Printed, String>` を返す（以前は `Result<(), String>`）。Windows で PDF を開くプログラムに `print` の verb が無いとき（Edge・Chrome が既定の素の Windows 11 では、コード 31 `SE_ERR_NOASSOC` で必ず失敗していた）、`open` で開き、`Printed::Opened` を返す（印刷はビューアーから）。刷りに出せたときは `Printed::Sent`。使う側がやること: `print(&p)?;` と捨てているだけなら変更は要らない。`Opened` のときに「印刷はビューアーから」と言い分けたいなら、返りを見る。

## [0.120.1] - 2026-10-27

### 修正
- `ito-notify` の `player`（Windows）: 10 ms × 3 本（約 30 ms）の先読みだと、約 40 ms ごとに約 9 ms の無音が入り、音の約 4 分の 1 が欠けていた（fude の実測）。先読みを 5 本（約 50 ms）にした。聞こえるまでの遅れ（約 31 ms）は Windows 側で決まるので変わらない。使う側がやることは無い。

## [0.120.0] - 2026-10-27

### 追加
- `ito_prefs::NotifyCfg::tell_open(events, app, icon, event, title, body, tag, open)`・`notify_window(&Context)`・`take_clicks() -> Vec<String>`: 押すと窓が前に来るシステムの通知（uchmk/ito#47）。通知（Windows では `open` のボタンも）を押すと、`notify_window` で覚えた窓へ `ViewportCommand::Focus` を送り、`tag` が `take_clicks` から返る（アプリは通知の行き先へ移れる）。同じ `tag` の新しい通知は前のを置き換える。音と webhook は `tell` と同じ。macOS は押されたかを知れないので表示だけ。通知の送り手は最初の呼び出しの `app`・`icon` で決まる（1 プロセスに 1 つ）。`tell` は変わらない。

### 使う側がやること
- 窓を出す最初のフレームで `ito_prefs::notify_window(ctx)` を呼び、`tell` を `tell_open` に替え、毎フレーム `take_clicks()` を見る。
- michi: `notify.rs` の `Notifier`・`WINDOW`・`CLICKED`・`set_window`・`take_clicks`・`notifier()` を消し、`tell_open` に置き換える（`tag` は notice の id の文字列、`take_clicks` は `Vec<String>` なので `parse::<u64>()` する）。

## [0.119.3] - 2026-10-27

### 修正
- `ito_prefs::Keys::standard`: ドロップダウンを開いたまま Esc を押すと、ドロップダウンだけでなく設定の画面まで閉じていた。開いたポップアップはフォーカスを持たないので `busy` では防げず、ページを描く前に Esc を取っていた。ポップアップが開いている間は Esc を取らない（1 回目でドロップダウン、2 回目で画面）。使う側がやることは無い（`rev` を上げるだけ）。

## [0.119.2] - 2026-10-27

### 修正
- `ito_prefs::NotifyCfg`: `[notify]` に `[[notify.webhooks]]` が無いと `missing field webhooks` で読めなかった。`webhooks` に `#[serde(default)]` を付けた（欄が 1 つだけの `[notify]` のテストも足した）。使う側がやることは無い（`rev` を上げるだけ）。

## [0.119.1] - 2026-10-27

### 修正
- `ito_prefs::seg`: 選ばれた部分の文字が `on_accent` でなく `fg` で描かれていた（暗いテーマで塗りの上の文字が読みにくい）。文字を `Color32::PLACEHOLDER` で組むようにし、選び・非選びの色が効く。使う側がやることは無い（`rev` を上げるだけ）。

## [0.119.0] - 2026-10-27

### 追加
- `ito_prefs::NotifyCfg`・`Event`・`HookCfg`・`DONE_FAILED`・`test_notify`: `[notify]` の読み書きを全アプリで 1 つにした（uchmk/ito#46）。kura・michi が持っていた写し（`NotifyCfg`・`check`・`page`・`edits`・`webhooks_of`・`tell`・`test`）の置き換え先。イベントはアプリのもので、`Event { name, sound_key, sound, level }` の並びで渡す（kura・michi の `done`/`failed` は `DONE_FAILED`）。
  - `NotifyCfg` はアプリの設定の構造体に `#[serde(default)] notify: NotifyCfg` で入れ、読んだあとに `check(&events, path, &mut warnings)` を 1 回呼ぶ（書かれていない欄を埋め、知らないイベント・音・形式・http(s) でない宛先を警告して戻し、`sounds` と `places` を作る）。書かれていない `[notify]` は `NotifyCfg::new(&events)`。
  - `tell(&events, app, icon, event, title, body)` はシステムの通知・音・webhook へ。`page(&events)` は設定の画面の `Notify`、`edits(&events, &page)` と `webhooks_of(&events, &page)` は画面の変更の書き戻し。「テストを送る」は `test_notify(&events, &page, what, app, icon, title, body)`。
- `ito-prefs` が `serde` と `toml` に依る（どちらも `ito-common` が既に使っている純 Rust）。

### 使う側がやること
- kura・michi: 自前の `NotifyCfg`・`HookCfg`・`check`・`page`・`webhooks_of`・`edits`・`tell`・`test` を消し、`ito_prefs::NotifyCfg` と `DONE_FAILED` に置き換える。`n.system` などの欄は `Option` になった（`check` のあとは `Some`）。`sound_done` / `sound_failed` は `n.sounds[0]` / `[1]`、`focus_mode` は `n.focus_mode()`。アプリの設定の構造体が `Eq` を導出しているなら外す（`NotifyCfg` は `PartialEq` のみ）。
- 新しいアプリ: 自前で書かず、これを使う。

## [0.118.0] - 2026-10-27

### 追加
- `ito_prefs::button_with(ui, look, words, Tone) -> Response` と `Tone { Plain, Primary, Danger }`: 塗ったボタン（uchmk/ito#44）。`Primary` はテーマの強調色（`run`）、`Danger` はエラー色（`err`）で塗り、文字は `on_accent`。大きさとフォーカスの輪は `button` と同じ。`button` は `Plain` のまま変わらない。

## [0.117.0] - 2026-10-27

### 追加
- `ito_prefs::seg(ui, look, id, now, options) -> Option<T>`: 区切りのボタン（セグメント化されたコントロール、uchmk/ito#43）。外枠が 1 つで、選ばれた部分が塗られる。高さ 26 pt でツールバーの 30 pt の行に収まる。Tab で 1 つの止まり場、フォーカス中は ←→（Home・End）で選びが動き、押すとその部分を選ぶ。フォーカスの輪は他の部品と同じ。`select` と同じ `options: &[(T, &str)]` の形で、変わったときだけ `Some` を返す。

## [0.116.1] - 2026-10-27

### 修正
- `ito-notices::show`: ベルを押して開いたその回に、リストが閉じていた。開いた押しがリストにとって「外の押し」で、`Close` を返していた。ベルの上の押しは外の押しとして数えない（ベルの開閉はアプリの仕事）。使う側がやることは無い（`rev` を上げるだけ）。

## [0.116.0] - 2026-10-27

### 変更
- 雛形 `templates/app/TESTING.md` を `ito-testcheck` が読む形にした（uchmk/ito#45）。見出しは `## 1. Start`、表は `| # | Do | Expect |`、行の番号は `1.1`〜`1.3`。冒頭の説明も「行を外すのは `#[test]` の `///` に `1.1: …` と書いたとき」に直した。
- 雛形の `examples/make-testcheck.rs` の `src_dirs` を `&["src"]` から `&["crates"]` にした（アプリの `src` は `crates/<app>/src` にあるので、前の値ではテストの印が 1 つも見つからなかった）。

### 追加
- `ito-testcheck`: `--check` で、チェック表が 1 行も読めなかった（古い `id | check` の形など）ときは失敗にする。

### 使う側がやること
- 雛形から作ったアプリ（fude など）: `TESTING.md` を上の形に直し、`make-testcheck.rs` の `src_dirs` を `&["crates"]` にする。

## [0.115.0] - 2026-10-26

### 追加
- 新しいクレート `ito-ime`: 自前で描く入力行の入力メソッド（日本語・中国語・韓国語）（uchmk/ito#40）。`ito-pane` の `view.rs` にあった処理を取り出した。
  - `Ime::new(id)`（`.purpose(..)`・`.orientation(Orientation::Horizontal | Vertical)`）と `ime.frame(ui, at, key) -> Frame`: 入力点 `at`（キャレット大の矩形）を窓に伝え（候補窓の位置）、このフレームの `Preedit`・`Commit` を読む。変換中の文字は次の変更の通知までフレームをまたいで覚える。`Frame { commits, preedit, composing }`。
  - 確定した直後の文字は、アプリが自分で描き終えるまで（`key` が変わるか `HOLD` = 0.25 秒）`preedit` に残る。消えて左から戻る見え方を避ける。`key` には入力点の位置を表す数（セルの番号、文字のオフセット）を渡す。
  - `Orientation::Vertical`: 縦書きの列の右隣に候補窓が開くよう、矩形をずらして渡す（`place`）。実機の候補窓の位置は未確認（OS によって開く向きが違うことがある）。
  - `ime.swallow_keys(ctx)`: 変換中は Enter・Backspace・矢印・Tab・Esc などを、そのフレームの入力から外す。キーを読む前に呼ぶ。`ime.composing(ctx)`。
  - `paint(painter, origin, text, font, fg, bg)`: 変換中の文字を下線つきで描く（横書き用。縦書きはアプリが自分で）。
- `ito-keyscript`: 台本の語 `<Preedit:にほん>`（変換中の文字。`<Preedit:>` で終わり）と `<Commit:日本>`（確定）。`Step::Preedit`・`Step::Commit`、`preedit_event`・`commit_event`。文字の逃げ方は `<Paste:>` と同じ（`\n`・`\e`・`\\`・`\xNN`）。`--keys` で日本語の入力を試せる。
- `ito-pane`: IME の候補窓の位置・変換中の文字の描画・確定直後の保持を `ito-ime` に載せ替えた（動きは変えない）。機能 `egui` が `ito-ime` を引く。

### 使う側がやること
- fude: `ito-ime = { git = "https://github.com/uchmk/ito", rev = "…" }` を足し、書く帯のフレームで `ime.swallow_keys(ctx)` の後に `ime.frame(ui, 入力点の矩形, 位置の数)` を呼ぶ。`commits` を挿入し、`preedit` を入力点に描く。台本は `<Preedit:にほん><Commit:日本>`。
- kura・tsumugi・minato: `ito-pane` の `rev` を上げるだけ（`ito-ime` が増える）。

## [0.114.0] - 2026-10-26

### 追加
- `ito_notify::player`: アプリ自前の短い音を、頼まれた瞬間に鳴らす `Player`（uchmk/ito#41）。打鍵ごとの「カチッ」のような、20〜80 ms の音向け。出力を 1 つ開いたままにし、`play` は声（voice）をミキサーに足して即戻る。重ねて鳴らせる（同時に 32 まで、超えたら古い声から）。
  - `Player::new() -> Result<Player, String>`: 既定の出力を開く。Windows は `waveOut`（10 ms × 3 本）、macOS は AudioQueue。**Linux には音の実装を入れない**（ALSA の C ライブラリを引くため）。`Err` が返るので、呼ぶ側は音なしで続ける。出力の無い機械でも `Err`。
  - `load_pcm(&[f32], rate) -> Sample`（モノ、-1.0..=1.0、線形で 44.1 kHz に直す）、`load_wav(&bytes) -> Result<Sample, String>`（PCM 8/16 bit・float32、何チャンネルでもモノに混ぜる）、`play(&Sample, volume)`。
  - 開いている間は無音をミキサーに流し続ける。音を出す設定のときだけ `Player` を作り、切ったら捨てる。`Drop` で出力を閉じる。
  - 通知の音（`sound::play`、システムの音を 1 回）はそのまま。
- Windows の `windows` の機能に `Win32_Media`・`Win32_Media_Audio`・`Win32_System_Threading`（`ito-notify`）。

### 使う側がやること
- fude: `ito_notify::player::Player::new()` を設定が「音あり」のときに作り（`Err` なら音なし）、コードで作ったサンプルを `load_pcm` で 1 回読んで、打鍵ごとに `play(&tick, 0.4)`。

## [0.113.0] - 2026-10-26

### 追加
- 新しいクレート `ito-export`: SVG の文字列で渡されたページを、PNG・複数ページの PDF・印刷にする（uchmk/ito#42）。どれも止まるので、ワーカースレッドから呼ぶ。
  - `Fonts`（`system()`・`empty()`・`with_file(path)`）: SVG が名指す字体。システムの字体は最初に要ったときに 1 回だけ読む。
  - `png_bytes(svg, scale, &fonts)` / `png(.., out)`: resvg で描く。`scale` 2.0 で 192 dpi。
  - `pdf_bytes(&pages, &fonts)` / `pdf(.., out)`: svg2pdf で、ページごとに 1 枚。文字は文字のまま、字体は必要な字だけ埋め込む。SVG の 96 px = 1 inch を pt（1/72）に直す（794 × 1123 px が A4）。
  - `print(&pdf)`: Windows は `ShellExecuteW` の `print`、macOS は `lpr`、Linux は `lp`。紙が出るのではなく、渡し終えたら戻る。
  - 純 Rust（Windows の印刷だけ `windows`）。svg2pdf 0.13 は usvg 0.45 を使い resvg 0.48 とは別なので、字体の表は形式ごとに持つ。

### 使う側がやること
- fude・kura・michi: `ito-export = { git = "https://github.com/uchmk/ito", rev = "…" }` を足し、ページを SVG にして `ito_export::pdf` などへ渡す。他の ito のクレートと同じ `rev` にそろえる。

## [0.112.0] - 2026-10-26

### 追加
- `ito_common::fonts::MINCHO` / `MINCHO_BOLD`: 印刷の組みのような日本語の明朝（セリフ）の字体のパス。`JAPANESE` / `JAPANESE_BOLD`（ゴシック）と同じ形で、`read_first` で読む。縦書きの字形（U+FE10–FE19、U+FE30–FE44）を持つ。Windows は游明朝・MS 明朝、macOS はヒラギノ明朝、Linux は Noto Serif CJK・IPA 明朝（uchmk/ito#38）。
- `ito_common::documents_dir()`: ユーザーの「ドキュメント」フォルダー。Windows は `SHGetKnownFolderPath`（移動・OneDrive・日本語名でも正しい）、macOS は `~/Documents`、Linux は `user-dirs.dirs` の `XDG_DOCUMENTS_DIR`（無ければ `~/Documents`）。環境変数 `UCHMK_DOCUMENTS_DIR` で上書きでき、テストや `--keys` の実行が本物のフォルダーに書かない（uchmk/ito#39）。

### 変更
- `shellhook` の PowerShell のプロファイルの代わりの場所に `documents_dir()` を使う（PowerShell に聞けなかったとき）。
- Windows の `windows` クレートの機能に `Win32_System_Com`・`Win32_UI_Shell` を足した。

### 使う側がやること
- fude: 明朝の清書は `fonts::MINCHO`、ノートの置き場の既定は `documents_dir()` の `fude`。`--keys` の実行では `UCHMK_DOCUMENTS_DIR` を一時のフォルダーに向ける。

## [0.111.0] - 2026-10-26

### 追加
- `ito-syntax`: `#!` の行から言語を決める入口（kura の `shebang_token` を共通に）。`shebang_token(first)`、`Stream::for_shebang(first)`、機能 `themes` の `Themed::for_shebang(first, theme)`。拡張子で言語が決まらないファイル（`knows(path)` が偽）の 1 行目に使う。

### 変更
- `MAX_LINE` を 1500 から 2000 バイトに（kura の 2000 に合わせる。これより長い行は色を付けず、解析もしない）。

### 使う側がやること
- kura は自前の `shebang_token` を外して `Stream::for_shebang` / `Themed::for_shebang` を使う。

## [0.110.0] - 2026-10-26

### 追加
- `ito_syntax::decl_kind(stack: &[Scope])`: スコープの積み全体から宣言の種類を決める（kura の `symbols.rs` の `classify` と同じ規則）。`Stream::line_decls` はこれを使う。
  - 種類は `function` / `class` / `struct` / `enum` / `union` / `trait` / `interface` / `impl` / `namespace` / `mod` / `macro` / `type` / `table`。
  - 呼び出し（`meta.function-call` の中の `entity.name.function`）、型の注釈・戻り値・型引数の中の型名、TypeScript の素の `entity.name.type`（`JSX.Element` など）は宣言にしない。
- 機能 `themes`: `Themed::line_decls(text) -> (Spans, Vec<Decl>)`（色と宣言を 1 回の解析から。`line` はこの前半）。

### 変更
- `Decl.kind` は `String` から `&'static str` に（比較は `d.kind == "function"` のまま使える）。
- `Decl.range` は隣り合う同じ種類をまとめたもので、前後の空白を含むことがある。

### 使う側がやること
- `Decl.kind` を `String` として持つ側は `.to_owned()` する。空白は `trim()` する（kura の `piece.trim()` と同じ）。`table` の見出しは行全体から作る。

## [0.109.0] - 2026-10-26

### 追加
- `ito-syntax`: フェンスや HTML の `language-…` の言語名から引く入口（kura の `find_token` / `fence_syntax` の別名表を共通に）。`fence_lang(info)`、`knows_token(token)`、`Stream::for_token` / `for_fence`、機能 `themes` の `Themed::for_token(token, theme)` / `for_fence(info, theme)`。`text` / `plain` などは言語なし。
- Markdown / HTML の色（見出し・リンク・引用・薄い字）は `themed::scope_color` と `foreground` で足りる（`Theme` 型は出さない）。

## [0.108.0] - 2026-10-26

### 追加
- `ito-syntax` の機能 `themes`（既定オフ）: 名前付きの syntect テーマの色で塗るモジュール `ito_syntax::themed`（uchmk/ito#37。kura の `syntect_theme` のため）。
  - `Themed::new(path, theme_name)` と `line(text) -> Vec<(Range<usize>, Style)>`（`Style { rgb, bold, italic, underline }`。行を隙間なく覆う。`reset`。`MAX_LINE` 超は空）。
  - `theme_names()`、`foreground(theme)`、`scope_color(theme, &["comment", …])`（そのテーマが既定の文字色と違う色にする最初のスコープの色。見出し・リンク・薄い字を取る用）。
  - 知らない名前は `base16-ocean.dark`、それも無ければ最初のテーマ。

### 使う側がやること
- 使うアプリは `ito-syntax = { …, features = ["themes"] }` にし、`syntect` / `two-face` の直接の依存を外す。`Highlighter` の代わりに `Themed`、Markdown / HTML の `Palette` の `pick` は `scope_color` に置き換える。

## [0.107.0] - 2026-10-26

### 追加
- `scripts/lanes/release-check.sh`: リリースの終わりの確認（`bash scripts/release-check.sh vX.Y.Z`、`FILES` に追加）。
  `release.yml` が成功したあとに 1 回回す。確認は 3 つ: 成果物が 6 つ（zip 2 + tar.gz 4）、SHA-256 の表（`release-sums.yml` が書く `<!-- checksums -->` 以降）に 6 つ全部の行がある、
  ノートが長すぎない（40 行超は `WARN`。落とさない）。足りなければ `NG` の行に次の手（`gh workflow run release-sums.yml -f tag=…` など）が出て、終了コードが 1 になる。
  `REPO=owner/name` で別のリポジトリも見られる。

### 使う側がやること
- `rev` を上げ、`scripts/lanes.sh sync` を流す（`scripts/release-check.sh` が入る）。
- リリースの手順の文書（kura は `docs/claude/release.md`）に、「`release.yml` の成功で終わりにしない。`bash scripts/release-check.sh vX.Y.Z` が `RELEASE OK` になるまでが終わり。
  SHA-256 の表を見ていないまま『完了』と報告しない（見られないときは『未確認』と理由を書く）」と書く。

## [0.106.0] - 2026-10-25

### 追加

- `ito-openin`: 拡張子で「システムの開き方か、エディターか」を選ぶ共通の表（uchmk/ito#36）。`[open] system = ["mp4", "pdf"]`（点なし・大小文字は区別しない。無ければ既定の `SYSTEM_EXTENSIONS`、`[]` は 1 つも無し）と `OpenCfg::opens_with_system(path)`。tsumugi の `[open] system` と同じ既定の 40 個。
  - 使う側がやること: tsumugi は自前の `system` を外してこれを読む（キーは同じ）。kura の `[open].rules`（名前・mime の glob から opener を選ぶ）は載せ替えず、rules に当たらなかったファイルの既定として `opens_with_system` を使う案（kura のセッションで判断）。`OpenCfg { .. }` を直接書いているところは `..Default::default()` を足す。

## [0.105.0] - 2026-10-24

### 追加

- `ito-syntax`: 宣言を返す（uchmk/ito#35）。`Stream::line_decls(text) -> (Ink, Vec<Decl>)` は `line` と同じ 1 回の解析で、その行が宣言する名前（`Decl { range, kind }`。`kind` は文法の `entity.name.<kind>` の `<kind>`: function・class・struct・enum・trait・interface・impl・namespace・macro・type、TOML の表は `table`）も返す。HTML のタグ名（`entity.name.tag`）は宣言に数えない。`decl_kind(scope)` も公開。
  - 使う側がやること: 無し（`line` は今までどおり）。アウトラインは kind を選び、字下げで入れ子にする。TOML の表の kind は issue の `section` ではなく、文法そのままの `table`。kura は syntect・two-face を外せる（kura#469）。

## [0.104.2] - 2026-10-24

### 修正

- `ito-pane`: アプリへの返信（端末への問い合わせの答え）や、ペインがキーを得る・失うフォーカスの通知を送るたびに、スクロールで遡っていた表示が一番下へ戻っていた。フォーカスの通知を出すアプリ（Claude Code など）で、範囲選択のクリックが表示を一番下へ戻し、スクロールして選べなかった。人が打ったキー・貼り付けだけが一番下へ戻す。

## [0.104.1] - 2026-10-24

### 修正

- `ito-pane`: 範囲選択のドラッグでウィンドウの外へ出ると、端の少し内側が最後の位置になり、スクロールが効かず見えていない行を選べなかった。ペインの上端・下端から 2 行以内でウィンドウを出たドラッグは、その端の外にいるものとして続けて送る。

## [0.104.0] - 2026-10-24

### 追加

- `ito-watch`（uchmk/ito#21）: フォルダーの変更をその場で受ける。`Watcher::start(on)` が名前付きスレッドを 1 本立て、`watch(roots)` で見る場所を替える（最新の希望だけが効く）。`Event::{Paths, Lost, Watching(bool)}`。ネットワーク共有は見ない（`on_share`）。
- `ito-guard`（#23）: コミット前の見張り。守る名前のブランチ（`*` 付き）、ステージされたファイルの大きさ・鍵・`.env`・中のトークン。Git は知らず、`Entry` の一覧と読む関数を渡す（`scan`）。
- `ito-glossary`（#24）: 専門語の表（`Term`・`Glossary`）。やさしい言い方・ホバーの説明（日英）、`label`、表の検査 `check`、点線のホバー `hover`（機能 `egui`）。`ito_common::Common::easy`（`easy = true`）で全アプリ共通に切り替える。
- `ito-syntax`（#19）: コードの色分けの種類。`highlight(path, lines)`、行をまたぐ状態を持つ `Stream`、`classify`、`knows`。syntect（純 Rust）+ two-face、文法は 1 プロセスで 1 回だけ読む。
- `ito-guide`（#22）: 初回・困りごとのガイド。`Problem`・`Step`・OS ごとの `Fix`・開閉の `Guide`（Esc は `escape`、パレットからは `reopen`）、キーを取らず外のクリックで閉じる egui のパネル `show`（機能 `egui`）、表示の深さ `Level`（basic / full）。
- `ito-a11y`（#27）: 色だけに頼らない状態の印 `State`（記号と読み上げの名前、日英）、スクリーンリーダー向けの `say`・`announce`（機能 `egui`）。
- `ito_theme::Colors::high_contrast()`（#27）: 高コントラストの色（本文 12:1 以上、淡い文字と状態の色は 7:1 以上、罫線 3:1 以上）。`ito_common::Common::high_contrast`（`high_contrast = true`）で全アプリ共通に切り替える。
- `ito_common::Common` に `easy`・`high_contrast`（`easy_or_off()`・`high_contrast_or_off()`）。
- `scripts/lanes/ito-first.txt`: `syntect`・`two-face` は `ito-syntax`、`notify` は `ito-watch` を使う。

### 使う側がやること

- `Common { … }` をリテラルで書いているところは `..Default::default()` を足す（`easy`・`high_contrast` が増えた）。
- michi: 手元の `guard.rs`・`syntax.rs`・`notify` の見張り・ガイドのパネルを `ito-guard`・`ito-syntax`・`ito-watch`・`ito-guide` に替え、`scripts/ito-first.allow` の `notify` の行を外す。専門語の表は `ito-glossary` の `Term` で書く。
- kura: プレビューの色分けを `ito-syntax` に替え、`syntect`・`two-face` の依存を外す。
- `high_contrast_or_off()` が真なら、`ito_theme::set(colors.high_contrast())` を通し、選択・フォーカス・競合などに `ito_a11y::State::mark()` を付ける。

## [0.103.1] - 2026-10-23

### 変更

- `ito-pane`: 画面を縦の罫線（`│` `┃` `║`。Claude Code の横のパネルなど）で左右に分けているとき、罫線の片側でドラッグを始めると、選択（色）もコピーもその側だけになる（今までは行全体で、反対側の文字も入った）。
  罫線が無い画面・Alt+ドラッグ・ダブル／トリプルクリック・キーの選択は今までどおり。`grid::{divider, side_of, limit_selected, selection_in}` を足した。使う側がやることは無い。

## [0.103.1] - 2026-10-23

### 修正

- `ito-prefs`: 設定の検索で光った最初の行へ、ページをスクロールする（検索の語ごとに 1 回。`keyed_row` / `row` が呼ぶ）。長いページの下の方（kura の Shell のカードなど）のヒットが画面の外に残らない（uchmk/ito#34、kura#443）。

### 使う側がやること

- なし（`rev` を上げるだけ。設定のページを `ScrollArea` の中に置いていれば効く）。

## [0.103.0] - 2026-10-23

### 追加

- `ito_common::apps`: 他の uchmk のアプリの exe を探す仕組み。探す順は、`common.toml` の `[apps]`（`kura = "D:/tools/kura.exe"`。全アプリ共通の上書き）→ そのアプリが自分で書いた登録（状態のフォルダーの `apps/<名前>.toml`）→ PATH。
  - `register(app, version)`: 起動時に自分の exe の場所を書く（変わったときだけ書く。`--keys` の実行中と、`target`・`cargo-target` の下のビルドは登録しない）。
  - `find`・`pinned`・`scan`・`scan_in_background`（設定画面用の行）・`write_edits`（`[apps]` の書き換え。`edit_common` に渡す）。
  - `Common::apps`（`[apps]` の表）。名前は `[a-z0-9_-]+` だけ。
- `ito_openin::app`: ほかのアプリを簡単に呼ぶ。`open(app, path)`・`launch(app, args, cwd)`・`available(app)`・`exe(app)`・`not_found(app)`。
- `ito_openin::spawn` は、最初の語がアプリ名なら（`[open]` のひな形の `kura {path}` など）、固定した場所か登録された場所を使う。
- `ito_prefs::apps_card`（`AppsWords`・`AppsForm`・`add_edit`）: 設定の画面の「アプリ」のカード。行ごとに、見つかった場所・見つけ方・版と、場所を固定する欄・外すボタン。登録のないアプリは下の欄から足す。

### 使う側がやること

- 起動時に `ito_common::apps::register("<app>", env!("CARGO_PKG_VERSION"))` を呼ぶ（失敗は無視してよい。UI スレッドを止めたくなければワーカーで）。
- 設定の画面の Open in のページに `apps_card` を置く。行は `apps::scan_in_background(common.apps.clone())` の答え。カードが返す `Edit` は `edit_common(base, |t| apps::write_edits(t, &edits))` で書く。
- 他のアプリを呼ぶ処理は `ito_openin::app::open/launch/available` を使う。独自に PATH を探さない。
- `Common { … }` をリテラルで書いているところは `..Default::default()` を足す（`apps` 欄が増えた）。

## [0.102.0] - 2026-10-23

### 修正

- `ito-titlebar`: 左のスロット（ロゴ・名前・タブ）が中央を越えて伸びても、検索ボックスとベルが最小化・最大化ボタンの上に押し出されない（uchmk/ito#33）。
  グループの右端は `rect.right() - MARGIN` までに丸める。左のスロットが邪魔で入らないときは、ベルを隠し、虫眼鏡だけを右端に寄せる。

### 追加

- `ito_titlebar::left_limit(rect)`: 左のスロットが止まるべき x（虫眼鏡とベルが右に入る場所を残した位置）。`left` のコールバックが受け取った `rect` を渡し、タブの文字を落とす目安にする。
- `Out::squeezed`: 左のスロットが長すぎてベルを隠したとき `true`（このとき `bell_rect` は `None`）。

### 使う側がやること

- タブなどを描く左のスロットは、`ito_titlebar::left_limit(rect)` を超えないように文字を落とす（michi の #145）。`Out` を自分で作っていなければ、ほかに変更は要らない（`Out` は `Default`）。

## [0.101.3] - 2026-10-23

### 修正

- `ito-common` の `shellhook::profile`: `pwsh` の `$PROFILE` を UTF-8 で読む（日本語の `ドキュメント` が文字化けしたフォルダーになり、そこにフックが書かれていた。tsumugi #210）
  - 使う側がやること: `rev` を上げるだけ

## [0.101.2] - 2026-10-23

### 追加

- ワークフローの雛形 `release.yml`: アプリに `scripts/release-notes.md` があれば、その中身をリリースノートの末尾（CHANGELOG へのリンクの前）に足す。アプリ固有の注意書きを雛形の外に置くため。
  - 使う側がやること: `scripts/lanes.sh sync` で `release.yml` が変わる。注意書きを残したいアプリ（yagura の「一部の値は通常のユーザーの権限では読めない」の一文など）は `scripts/release-notes.md` に書く。無いアプリは今までどおり。

## [0.101.1] - 2026-10-23

### 修正

- `ito-prefs`: 設定の環境変数エディタ（シェルのカード）のボタンを、ほかの画面と同じにした。追加は左、取り消しと保存は右に寄せて取り消しが保存の左、
  高さは 30 でそろえ、Tab は取り消し→保存の順に進む。使う側がやることはない（rev を上げるだけ）。

## [0.101.0] - 2026-10-22

### 追加

- `ito-keyscript`: `Runner::new` が `ito_common::scripted_run()` を呼ぶ（`ito-common` に依存する）。
- `ito-common`: `scripted_run()`。`--keys` の台本で起動したアプリが 1 回呼ぶと、`write_atomic`（設定の書き込みの唯一の口）が、`UCHMK_CONFIG_DIR` が無いまま本物の `uchmk` フォルダーへ書くのを断る（`Err`）。`<Theme:…>`・`<Lang:…>` が `common.toml` を書くので、付け忘れた試験が持ち主の設定を書き換えたため（michi 0.106.9 の試験）。
  - `UCHMK_CONFIG_DIR` が付いていれば何も変わらない。台本でないときも変わらない。`<APP>_CONFIG_HOME` で外へ出したアプリのフォルダーは対象外。
- テスト: 守る条件（台本・`UCHMK_CONFIG_DIR`・フォルダーの内外）。

### 使う側がやること

- `ito_keyscript::Runner` を使うアプリは何もしなくてよい（`Runner::new` が `scripted_run()` を呼ぶ）。`Press` で自前のループを回すアプリは、台本を読んだ直後、設定を読み書きする前に `ito_common::scripted_run()` を呼ぶ。
- 台本の試験は必ず `UCHMK_CONFIG_DIR` を捨てるフォルダーに向けて回す（付け忘れると、書き込みが断られて `<Theme:…>` が効かない）。

## [0.100.0] - 2026-10-22

### 追加

- `ito-keyscript`: 台本に 3 つの語を足した。窓の大きさと設定を、台本の中で変えられる。
  - `<Size:480x600>`: 窓の内側の大きさ（点、100〜10000）。狭い窓の確認に使う。`Runner` は `InnerSize` を送り、作り直しを待ってから次へ進む。
  - `<Theme:名前>`・`<Lang:ja>`: テーマと言語を切り替える。やり方はアプリのもの（`common.toml` を書いて見張りに任せる、自分の状態を変える、など）なので、`Runner` は名前を返すだけ（`Do::Theme`・`Do::Lang`）。
  - `Step::{Size, Theme, Lang}`、`Press::{Size, Theme, Lang}`、`Do::{Theme, Lang}`。名前は英数字と `- _ .` だけ。
- テスト: 3 つの語の解析・表記・不正な書き方の拒否。

### 使う側がやること

- `Step`・`Press`・`Do` を `match` で全部並べているところは、新しい枠（`Size`・`Theme`・`Lang`）を足す。
- `Runner` を使うアプリ: `Do::Theme(name)`・`Do::Lang(code)` を受けて、切り替えを行う（`ito_common` で `common.toml` を書くのが一番手軽）。`Size` は `Runner` が自分で行う。
- 自前のフレームのループを持つアプリ: `Press::Size` には `ViewportCommand::InnerSize` を送って少し待つ。`Press::Theme`・`Press::Lang` は上と同じ。

## [0.99.0] - 2026-10-21

### 追加

- `ito-confirm`: 3 つ以上の選択肢を、横のボタンではなくカードで縦に並べる。
  - `Card`（`title` 動詞の見出し・`en` 小さく添える別名・`description` 1 行の説明・`result` 数字で言う結果・`undo`・`key` キーの札・`danger`）と、`Question::cards`。
  - `Undo::{Unsaid, Yes(方法), No(失うもの)}`: 戻せるかどうかの札。`No` のカードは赤い。
  - `Question::recommended`（おすすめのカードを緑に）と `recommended_label`（見出しの横の「おすすめ」）。赤と緑は同じカードに載らない（戻せないものは「おすすめ」にならない）。
  - `Question::danger`: 主ボタンを赤にする（取り返しのつかない 2 択）。
  - `Question::keys`: ボタンの右のキーの札（`Ctrl+Enter`・`Esc`）。
  - 選択肢の番号は、カードが先、ボタンがその後ろ。`focus`・`primary`・`recommended` と、押されて返る番号は同じ数え方。
  - カードがあるときのボタンは、塗りなしで枠線だけ（やめるなど静かなもの）。カードが無いときの見た目は今までどおり。
  - 色の定数 `DANGER_BG`・`DANGER_TEXT`・`OK_TEXT`。
- テスト: カードがどの幅でも全部描かれる。カードを押すと、その番号が返る。

### 修正

- 0.98.0 の CHANGELOG と `CLAUDE.md` の `ito-proc` の行で、`\r` と `\n` が本物の改行に化けていたのを直した。

### 使う側がやること

- `Question { … }` を欄を全部書いて組み立てているところは、`..Default::default()` を足す（欄が増えた: `cards`・`keys`・`danger`・`recommended`・`recommended_label`）。
- カードにするには、`cards` に `Card` を並べ、`buttons` にはやめるだけを入れる。`Card` の文言（`result` の数字・`undo` の方法）は呼ぶ側が作る。
- `recommended` を使うなら `recommended_label` に「おすすめ」を渡す（文言は ito に持たない）。

## [0.98.0] - 2026-10-21

### 追加

- 新しいクレート `ito-proc`（uchmk/ito#31、michi の `net::run` の汎用の部分）。止められる子プロセス。
  - `run(cmd, &stop, &mut |line| keep) -> io::Result<Outcome>`: stdout/stderr をスレッドで読み、stderr は `\r` でも `\n` でも 1 行に切って `on_line` に渡す（git の進み具合は `\r` で上書きされる）。`on_line` が `true` を返した行だけ `Done::lines` に残る。stdout は `Done::stdout` に全部。
  - `stop` が立つと、子と、子が起こしたもの全部（ssh・認証ヘルパー）を殺して `Outcome::Stopped`（失敗とは別）を返す。Unix は子を自分のプロセスグループに入れて `kill -KILL -<pgid>`、Windows は `taskkill /T /F`（窓なし）。C のライブラリも新しい依存も無い。
  - 子が終わったあとにヘルパーがパイプを握っていても、読むスレッドは 1 秒で打ち切る（呼ぶ側を待たせない）。
  - `own_group`・`kill_tree` も公開（自分で spawn するアプリ向け）。呼ぶと UI スレッドを塞ぐので、ワーカーから呼ぶ。
- `ito-confirm` のテスト: 1〜6 個のボタンがどの幅でもボタンとして描かれる。

### 使う側がやること

- michi: `net.rs` の `kill_tree`・`own_group`・`run` を `ito_proc::run` に載せ替える（`Progress::parse` が `Some` の行は `false`、そうでなければ `true` を返す）。
- minato: `gh` の呼び出しに止める手が要るなら使える。

## [0.97.0] - 2026-10-21

### 追加

- `ito_titlebar::Spec::right_min`（右のスロットに確保したい幅。0.0 なら今までどおり）。検索の箱は窓の中央のまま、右のスロットに `right_min` が残るところまで先に縮む（最小は虫眼鏡だけ）。それでも足りなければ右のスロットが自分で畳む。

### 使う側がやること

- `Spec { … }` に `right_min: 0.0,` を足す（雛形の `main.rs` も同じ）。右のスロットに幅が要るアプリ（michi の「設定」とチップなど）は、その幅を渡す。

## [0.96.0] - 2026-10-20

### 追加

- `ito_common::app_dir_in(base, app)`: `app_dir` の base を渡せる形（`<APP>_CONFIG_HOME` があればそれ、なければ `<base>/<app>`）。テストで一時フォルダを base にしたいアプリ向け。`app_dir(app)` はこれを呼ぶだけになった（動きは同じ）。uchmk/ito#32。

## [0.95.2] - 2026-10-20

### 追加

- `templates/app/.git-blame-ignore-revs`（空の雛形。全体を `cargo fmt` しただけのコミットを `git blame` から外す。kura・tsumugi・minato が持っていた）と、雛形の `CLAUDE.md` の一行。

## [0.95.1] - 2026-10-20

### 変更

- `workflows/checklists.yml`: 見る場所を `scripts/testcheck.toml`・`examples/**`・`crates/*/examples/**` にした（ルートに `examples/` を持つ 1 クレートのアプリも、`crates/` のアプリも同じ雛形で足りる）。

## [0.95.0] - 2026-10-20

### 追加

- ワークフローの雛形 `scripts/lanes/workflows/`（`build`・`arm64`・`checklists`・`release`・`release-sums`・`merge-lanes`）。`{{app}}`・`{{crate}}`・`{{bin}}` は `lanes.conf` の値で埋まる。
  アプリの `lanes.conf` に `workflows=<名前…>` を書くと、`scripts/lanes.sh sync` が `.github/workflows/` に書き（先頭に「ito の雛形から書いた」行）、`check` が違いを落とす。書かないアプリは今までどおり自分のワークフロー。
- `scripts/lanes/release-sums.sh`（`FILES` に追加）。`release-sums.yml` が呼ぶ。
- `verify.sh`: 雛形の `{{…}}` が既知の 3 つだけで、YAML として読めるかを見る。

### 変更

- `templates/app`: 0.94.0 で足したワークフローと `release-sums.sh` は雛形に直接置かず、`lanes.conf` の `workflows=` と `files=` で `sync` が書く形にした（新しいアプリの結果は同じ）。

### 使う側がやること

- 既存のアプリは何も要らない（`FILES` を全部取る設定のアプリだけ、`sync` で `release-sums.sh` が増える）。ワークフローを ito に寄せたいアプリは、`lanes.conf` に `workflows=` を足し、`sync` して差分を見て、自分のものとの違いを ito に頼むか受け入れる。

## [0.94.0] - 2026-10-20

### 追加

- `templates/app`: 全部のアプリが持っているのに雛形に無かったものを足した。新しいアプリは最初から持つ。
  - `.github/ISSUE_TEMPLATE/`: `needs-pc.yml`・`finding.yml`・`question.yml`・`retest.yml`（`bug_report.yml` は従来どおり `make-bugform` が作る）。
  - `.github/workflows/`: `build.yml`（6 ターゲットのリンク、Windows は `.com` の確認つき）・`arm64.yml`・`checklists.yml`・`release.yml`・`release-sums.yml`。`-p {{app}}` と `{{app}}-com.exe` で雛形のクレートに合わせてある。
  - `scripts/release-sums.sh`（`release-sums.yml` が呼ぶ）。
- まだ雛形に入れていないもの: `merge-lanes.yml`（`lanes.conf` の `files=` に `merge-lanes.py` などを足すテストのレーンとセット）、`.claude/windows-role.md`（アプリごとの色が濃い）。

### 使う側がやること

- 無し。新しく作るアプリにだけ効く。既存のアプリは自分の版のまま。

## [0.93.2] - 2026-10-20

### 変更

- `templates/app/CLAUDE.md`: 新しいアプリの既定に「状態のファイルは TOML」（`kept::read` / `kept::Writer`、ログは `.log`、例外）を足した。雛形のコードの変更は無し（窓の場所は元から `window.toml`）。

## [0.93.1] - 2026-10-20

### 変更

- `docs/common-spec.md`: 秘密の鍵のファイル（`app_dir` の中。拡張子なし・TOML でなくてよい。`bundle` に入れない）を、設定・状態の形式の規則の例外として書いた。コードの変更は無し。

## [0.93.0] - 2026-10-20

### 変更

- `docs/common-spec.md`: **状態のファイルも TOML** にする規則を足した（これまで状態の形式は「問わない」だった）。
  - `.txt`・`.json`・拡張子なしのバイナリで持たず、一覧は表の中の配列（`recent = [...]`、`[[entries]]`）にする。読み書きは `kept::read` / `kept::Writer`。
  - 古い形は起動時に 1 度だけ TOML に直す（`kept::copy_old` か `move_old`。古いファイルは消さない）。
  - ログは `.log` のまま。キャッシュ・WebView のデータ・画像・実行ファイルの写しなど、状態でないものは対象外。
- コードの変更は無い。使う側がやること: 状態に `.txt`・`.json`・バイナリを持つアプリは、TOML に移す（新しい API は要らない）。

## [0.92.0] - 2026-10-20

### 追加

- `ito-common::kept`: アプリが状態のフォルダー（`state_dir`）に残すファイルの読み書き。tsumugi・yagura・minato・michi が手で書いていた `state.toml` の型を 1 か所に。
  - `read::<T>(path)`: TOML を読む。無い・壊れていれば `None`。
  - `Writer::new(path, on)`: 1 ファイルの書き手。`save(text)` / `save_toml(&v)` は別スレッドで原子的に書き（`create_dir_all` 込み）、速く続けて呼んでも最後の呼び出しが残る（遅れて着いた古い書き込みは捨てる）。`save_now` はスレッドなしで、終了時用。`on == false`（`--keys` の台本など）なら何も書かない。
  - `copy_old(old, new)`: 旧い置き場から 1 回だけ写す（`new` が無く `old` がファイルのとき。`old` は残す。動かすなら `move_old`）。
- `ito-common::window::Saved::load_moved(dir, old)`: `window.toml` を設定のフォルダー（`old`）から状態のフォルダー（`dir`）へ動かしてから読む。`dir` に既にあればそちらが勝つ。

### 使う側がやること

- 任意。窓の場所を状態のフォルダーに移すアプリは `Saved::load(Some(app_dir))` を `Saved::load_moved(Some(state_dir), Some(&app_dir))` に替える。`Tracker` は渡した `dir` に書くので、変更は要らない。
- 手書きの「別スレッドで書く＋番号で最新だけ残す」は `kept::Writer` に置き換えられる。

## [0.91.0] - 2026-10-20

### 追加

- `ito-confirm`: ボタンを箱の右端に寄せた（最後のボタンが一番右。箱より長い行は折り返す）。角は 6pt の丸め。`Question::primary: Option<usize>` で、進める側のボタンを緑の塗り（`PRIMARY_BG`）・太字の白（`PRIMARY_TEXT`）で描く（GitHub のコメント欄と同じ）。`None` なら全部ふつうの色。`Style` は変えていない。
- 使う側がやること: `Question { … }` を `..Default::default()` なしで書いているところは `primary: None` を足す。全アプリで確認のボタンが左寄せから右寄せに変わる。

## [0.90.0] - 2026-10-19

### 追加

- `ito-ipc::door`（uchmk/ito#11）: 窓と `<app> mcp` のつなぎの共通部分。kura・michi・tsumugi が同じ形で書いていた「扉を 1 つのスレッドで開く（先に取った窓が持ち、ほかは 30 秒ごとに取り直す）→ 1 接続 1 リクエスト 1 返事 → リクエストを UI スレッドへチャネルで渡して、メモリの状態から答える」を `Door<Rq>` にした。
  - アプリが持つのは `Rq`（serde の enum）、答え、`ito-mcp` のツール宣言だけ。`Door::new("<app>", "<APP>_ADDRESS", PROTO)` に、UI スレッドの外でやる検査 `check` と返事の待ち時間 `wait`（利用者の押す箱は長く）を足せる。
  - `door.listen(|| ctx.request_repaint())` が `Receiver<Ask<Rq>>` を返す。UI スレッドは `ask.answer(Ok(text))`（`Err` で理由）で答える。`ask.expired` は待つのをやめたときに立つ。GUI は知らない（`wake` を渡す）。
  - `ito_ipc::door::ask(&door, &request)` が `<app> mcp` 側の問い合わせ。窓が無いときは「`<app>` is not running」のツールのエラー。版（`proto`）が違えば、どちらも同じ版を使うよう伝える文を返す。
- 使う側がやること（急がない）: kura・michi・tsumugi の `mcp.rs` の `listen`・`serve_one`・`answer`・`ask` を `Door` に置き換える。`Ask` は `std::sync::mpsc` の `Receiver`（kura は crossbeam から）。新しい扉（minato の PR / CI の問い合わせなど）は最初から `Door` で作る。

## [0.89.1] - 2026-10-19

### 修正

- `scripts/lanes/lane-trust.ps1`: `Set-ClaudeFolderTrust` が Linux の runner で `C:\scratch\Temp\` を相対の名前と見て、作業フォルダーを前に付けていた（`check-ps1.ps1` の「the folder trusted」が落ちた）。ドライブ文字の付いたパスはそのまま使う。
- 使う側がやること: `rev` を上げて `scripts/lanes.sh sync`。

## [0.89.0] - 2026-10-19

### 追加

- `templates/app`: コマンドパレットを繋いだ（`ito-palette`）。バーの中央の箱を押すか `Ctrl+Shift+P` で開き、「不具合を報告」「拡大率を戻す」「終了」の3つが入っている。アプリのコマンドは `Command` と `commands` に足す。`lang/en.toml`・`ja.toml` に `[palette]` を足し、拡大率の変更は `rescale` から `scale_to` に分けた。

## [0.88.0] - 2026-10-19

### 変更

- `scripts/lanes/ito-required.txt`: `ito-titlebar` と `ito-notices` を全アプリの必須にした（持ち主の指示）。`ito-first.py` は、どちらかが `Cargo.toml` に無いアプリを落とす。
- 使う側がやること: `scripts/lanes.sh sync` で表を写したら、`ito-titlebar` と `ito-notices` を載せる（雛形 `templates/app` の `main.rs` が手本）。すぐ載せられないアプリは `scripts/ito-first.allow` に `<crate> <理由>` を書く。

## [0.87.0] - 2026-10-19

### 追加

- `templates/app`: 窓の自前のタイトルバー（`ito-titlebar`）とベル（`ito-notices` の一覧）を入れた。窓は枠なし（Mac は信号機を残す）で、バーに名前・中央の箱・ベル・最小化／最大化／閉じるが付き、窓の縁で大きさを変えられる。ベルは空の一覧を開く（`notices` に `Notice` を push すれば増える）。`lang/en.toml`・`ja.toml` に `[title]`・`[notices]` を足した。箱を押したときのパレットは TODO.md に残した。
- 必須のクレートの表（`ito-required.txt`）には、まだ足していない。足すと既存のアプリの `ito-first.py` が落ちるので、持ち主が決めてから。

## [0.86.0] - 2026-10-19

### 追加

- `ito-notify`: `Toast::open(tag, title, body, label)` と定数 `notifier::OPEN`（uchmk/ito#30）。通知の本体か「開く」ボタン（Windows）を押すと、`Notifier` の `Click` が `tag`（通知の `id`）と `OPEN` で呼ばれる。アプリは `id` から `ito_notices::Notice::go` を引いて、一覧と同じ行き先へ移る。Linux は `notify-send` 0.7.12 以降で本体の押しが戻る。macOS は AppleScript が押しを返せないので、表示だけ。`Notifier` と `launch`・`actions` はこれまでどおり使える。
- 使う側がやること: 行き先のある通知を `Toast::open(notice.id, …)` で `Notifier::show` に渡し、`Click` で `OPEN` を受けたら `Action::Open` と同じ処理をする。

## [0.85.0] - 2026-10-19

### 追加

- `ito-notices`: ベルが開く通知の一覧（uchmk/ito#30）。`Notice<T>`（`id`・`at`・`kind`・`title`・`note`・`go`・`read`）をアプリが古い順に持ち、`show(ctx, bell_rect, &notices, &Words)` が新しい順に描く。未読は明るく、時刻と種類（Ok・Info・Warn・Err・Wait）の色の点が付く。行を押すか Enter で `Action::Open(go, id)`、先頭の「すべて既読」で `ReadAll`、Esc か外を押すと `Close` を返す。`unread` と `any_error` が `ito_titlebar::Spec` のバッジの元になる。色は `ito_theme::colors()`。
- 使う側がやること: 自前の一覧を `show` に置き換え、返った `Action` で既読にして `go` へ移る。文言は `Words`（`title` には未読数を入れた文）で渡す。トーストのクリックで開く対応は、OS ごとに扱いが違うので後の版にした。

## [0.84.0] - 2026-10-19

### 追加

- `ito-titlebar`: 窓の自前のタイトルバー（uchmk/ito#29）。`show` がバーの矩形を描き、空きのドラッグ・ダブルクリックの最大化、最小化/最大化/閉じる（幅 46）、窓の中央のパレットの箱（角丸 6、塗りは `ito_theme` の `panel`、ホバーで `hover()`、枠は `border`）とその右のベル（未読のバッジは `wait`、エラーがあれば `err`）、左右のアプリの枠を受け持つ。文言・キーの表示（`Ctrl+Shift+P` など）・未読数は `Words` と `Spec` で渡す。`apply` が `WindowOp` を窓へ送り、`frameless` が枠なしの viewport を作り、`resize_edges` が枠なし窓の縁での大きさ変更を受け持つ。
- 使う側がやること: 自前のバーを `ito_titlebar::show` に置き換える（tsumugi・michi・minato）。見た目は tsumugi に揃う（バーの高さ 40、箱の角丸 6、ボタンの幅 46）。michi（36）と minato の差は見た目が変わる。

## [0.83.0] - 2026-10-19

### 追加

- templates/app: 画面全体の倍率を Ctrl+マウスホイール、`<C-=>`、`<C-+>`（JIS キーボードは `=` に Shift が要るので `+` も受ける）、`<C-->`、`<C-0>` で変えられるようにした。`scale_step` で次の値を作り、すぐ `set_zoom_factor` で当て、common.toml へは UI ではないスレッドで書く。状態の行に `scale_text` を出す。新しいアプリだけが対象で、使う側がやることはない（既存のアプリは同じ形に揃えるとよい）。

## [0.82.1] - 2026-10-19

### 修正

- ito-common: `write_atomic` の横に置く一時ファイルの名前を `<file>.uchmk-new` の固定から `<file>.uchmk-<pid>-<n>.new` にした。同じファイル（`recent.txt` など）を 2 つのプロセスやスレッドが同時に書いても、一時ファイルを取り合って書き込みが壊れたり rename が失敗したりしない。`bundle` は `.uchmk-` を含む名前を引き続き除く。使う側がやることはない（一時ファイルの名前を決め打ちしていなければ）。

## [0.82.0] - 2026-10-18

### 追加

- ito-confirm: `Question.focus: Option<usize>`。キーボードが乗っているボタンを太い縁で描く（動かすのも Enter で答えるのもアプリ。Tab / Shift+Tab / Enter）。`Question` を構造体リテラルで作っているアプリは `focus: None` を足す（`..Default::default()` なら不要）。

## [0.81.1] - 2026-10-18

### 追加

- scripts/lanes: `lane-trust.ps1`（`Set-ClaudeFolderTrust`）。レーンの一時フォルダー（`<scratch>un-<日時>\projN`）で Claude Code を起動すると「このフォルダーを信頼しますか」で止まり、Enter を押すまで進まなかった。`~/.claude.json` の `projects["<フォルダー>"].hasTrustDialogAccepted` を一時フォルダーの親に 1 回だけ書いて、下の実行用フォルダーを全部信頼済みにする（親が信頼済みなら子も通ることを実機で確かめた）。書いたら `$true`、既にある・設定が無い・書けないときは `$false`（実行は止めない）。ほかの項目は触らない。`check-ps1.ps1` に確かめを足した。

### 使う側がやること

- `rev` を上げて `scripts/lanes.sh sync`（`files=` を書いているアプリは `lane-trust.ps1` を足す）。`auto-wintest.ps1` で `. (Join-Path $PSScriptRoot 'lane-trust.ps1')` を読み、一時フォルダーの親を作った直後に `Set-ClaudeFolderTrust $Scratch | Out-Null` を呼ぶ。ユーザーの作業フォルダーには使わない。

## [0.81.0] - 2026-10-18

### 追加

- ito-pane: `installed_shells()`（`PATH` にあるシェルを (表示名, プログラム) で返す。`ito_prefs::Shell::shells` にそのまま渡せる）と、その候補の表 `KNOWN_SHELLS`（PowerShell 7・Windows PowerShell・Command Prompt・bash・zsh・fish・nushell・sh）。tsumugi の `installed_shells` から移した（uchmk/ito#28）。ディスクを見るので UI スレッドの外で呼ぶ。

### 使う側がやること

- tsumugi: 自前の `KNOWN`・`installed_shells` を消して `ito_pane::installed_shells()` を使う。
- kura: `shell_card` の `shells: &[]` に `ito_pane::installed_shells()` の結果を渡す（設定を開くときに 1 回、ワーカーで）。

## [0.80.0] - 2026-10-18

### 追加

- ito-theme: 字の拡大率 `set_text_zoom(ctx, zoom)`・`text_zoom(ctx)`・`zoomed(ctx, size)`。端末のペインの字の拡大・縮小（font_bigger / font_smaller）に、設定・ヘルプ・パレットなどの全部の画面を追従させる（小さい字と大きい字を混ぜない）。

### 変更

- ito-prefs・ito-browser: 画面の字の大きさと、行・ボタン・トグルなどの高さ・幅を拡大率に掛けるようにした（拡大率 1.0 では今までと同じ）。ito-palette の行の高さも同じ。ito-browser と ito-palette は ito-theme に依る。

### 使う側がやること

- 毎フレーム、画面を描く前に `ito_theme::set_text_zoom(ctx, ペインの字の大きさ / 設定の字の大きさ)` を呼ぶ。
- ito-help・ito-confirm の `FontId` と `row_h` は使う側が渡すので、`ito_theme::zoomed(ctx, 値)` を通す。アプリ自前の画面の `.size(N)` / `FontId` / 行の高さも同じ。

## [0.79.1] - 2026-10-17

### 修正

- ito-pane: 端末のドラッグ選択で、全角文字の左のセルだけで範囲が終わり、文字が半分だけ選ばれて見えるのを直した（`select_at` / `select_kind`。全角文字は、左のセルを左半分、後ろの空きのセルを右半分として扱う）。使う側がやることはない。ito-select の 0.63.1 の直しは別の経路（painter で描く窓）だった。

## [0.79.0] - 2026-10-17

### 追加

- 新しいクレート `ito-fonts`: 等幅フォント JetBrains Mono Regular（SIL OFL 1.1）を同梱する。`JETBRAINS_MONO`（バイト列）・`NAME`・`LICENSE`・`AUTHORS`、機能 `egui` で `register(&mut FontDefinitions, MonoFont)`（uchmk/ito#25）。
- ito-common: `MonoFont`（`Bundled` が既定 / `System`）、common.toml の `mono_font` キー、`Common::mono_font_or_default()`、`mono_font_change`。
- ito-prefs: `mono_font_card`（等幅の字体の行。`CommonChange` を返す）、`mono_font_index`、`MONO_FONT_KEY`、`MoreWords` の `mono_*` の欄。

### 使う側がやること

- `MoreWords` を自分で作っているアプリは `mono_title`・`mono_font`・`mono_font_note`・`mono_bundled`・`mono_system` を足す（`MORE_EN`・`MORE_JA` を使っていれば何もない）。
- FontDefinitions を組むとき、アプリの字体が「自動」なら `ito_fonts::register(&mut fonts, common.mono_font_or_default())` を呼ぶ（`ito-fonts` に機能 `egui` を付ける）。
- 外観のページに `mono_font_card` を置き、返った `CommonChange` を `edit_common` で書く。
- ライセンスの画面に `ito_fonts::LICENSE` を載せる。実行ファイルは約 270 KB 増える。

## [0.78.0] - 2026-10-17

### 追加

- ito-keys: `human_text(&str) -> Cow<str>`（と、macOS かどうかを渡せる `human_text_for`）。翻訳表の固定の文言（`Ctrl+Z to undo`）の `Ctrl+` を、macOS では `Cmd+` と綴る（uchmk/ito#26）。
  - 語の頭の `Ctrl+` だけを直す（`XCtrl+K` はそのまま）。`Ctrl+Tab`・`Ctrl+Shift+Tab` は macOS でも Control なので、そのまま残す。macOS 以外では文字列を借りて返す（コピーしない）。
  - キーマップの割り当て（`<C-k>`）は今まで通り `human_seq` で綴る。これは文の中に書いてある文言のため。

### 使う側がやること

- 文言の表（i18n）の `Ctrl+` を画面に出す所で `ito_keys::human_text(text)` を通す。独自の同じ処理（michi の `keyhelp::spelled`・`native` など）は置き換えられる。

## [0.77.0] - 2026-10-17

### 追加

- ito-theme: 組み込みのテーマを 13 から 20 にした（末尾に足したので、`builtin()[0]` と今までの並びは変わらない）。
  - ダーク: `Campbell`（背景 `#0c0c0c` の深い黒。Windows Terminal の既定の配色。端末の 16 色も本物の値を持つ）、`VS Code Dark+`。
  - ライト: `VS Code Light+`、`GitHub Light`、`One Light`、`Rosé Pine Dawn`、`Tokyo Night Day`（ライトは 4 から 9 になった）。
  - 元の配色で文字が 7:1、薄い文字が 4.5:1 に届かないもの（`Rosé Pine Dawn`・`Tokyo Night Day`）は、文字と薄い文字を一段暗くして `every_theme_reads` を通した。

### 使う側がやること

- 一覧は `builtin()` から作っているアプリは、`rev` を上げるだけで新しいテーマが出る。テーマの数や名前を決め打ちしているテスト・文言（「13 個」など）があれば直す。

## [0.76.0] - 2026-10-17

### 変更（使う側の呼び出しが変わる）

- ito-common: `bundle::collect` / `restore` / `export` / `import` が、最後の引数 `keep: &dyn Fn(&str) -> bool` を取る。アプリのフォルダー（`app/…`）の中で、どのファイルを束に入れ、どのファイルを読み込むかを決める。渡されるのはアプリのフォルダーからのパス（`/` 区切り。`config.toml`・`themes/Mine.toml`）。束にほかのファイルが入っていても、`keep` が断ったものは書かず `Restored::skipped` に返す。全部でよければ `&bundle::all`。`common.toml` と共通のテーマ（`themes/`）には効かない。
- ito-common: `app/…` に `.json` も入る（読み込みの前に JSON として読めるか見る。読めなければ何も書かない）。共通のテーマ（`themes/`）は TOML だけ。

### 使う側がやること

- 0.75.0 の `bundle` を呼んでいるアプリ（minato）: 呼び出しに `&bundle::all` を足す（今までと同じ動き）。アプリのフォルダーに `.json` の設定があるなら、これで束に入る。入れたくないファイルがあれば、`keep` で外す。
- tsumugi: `export.rs` を `bundle` に替える。`keep` には今の許可リスト（`config.toml`・`keymap.toml`・`profiles.toml`・`prompts.toml`・`layouts.toml`・`theme.toml`・`themes/*.toml`）を渡す。古い書き出し（`=== パス ===`、先頭が `# tsumugi settings export`）を読む処理と、`settings.toml` を `config.toml` と読み替えること、`keymap.toml` を退けることは、tsumugi に残す（古い書き出しを `Bundle { app: "tsumugi", files: "app/<名前>" }` に変えて `restore` に渡す）。

## [0.75.0] - 2026-10-17

### 追加

- ito-common: `bundle`（設定を 1 つの TOML にまとめて書き出し、読み込む）。中身は `common.toml`・自分のテーマ（`themes/*.toml`）・アプリの設定のフォルダーの `.toml`（`app/…`）。窓の場所・セッション・履歴のような実行の間に残すもの（state）は入れない。`collect` / `to_text` / `parse` / `restore`、ファイルまで通す `export` / `import`。
  - 読み込みは安全側: 名前に `..`・ドライブ・`\` があるもの、`.toml` でないもの、決まった置き場（`common.toml`・`themes/`・`app/`）の外は飛ばして `Restored::skipped` に理由を返す。TOML として読めないファイルが 1 つでもあれば何も書かない。置き換えるファイルは `<file>.bak-<time>` に残す。同じ中身は書かない（`unchanged`）。別のアプリの束の `app/…` は飛ばす（`common.toml` とテーマは読む）。形式の番号が新しすぎる束は断る。
  - ディスクに触るので、UI のスレッドでは呼ばない。
- ito-prefs: `backup_card`（ファイル名の欄 + 書き出す + 読み込む（2 回押す））と `backup_index`。押されたら `Backup::Export(path)` / `Backup::Import(path)` を返す。ファイル名の欄の初期値は `default_file`（例: ホームの `<app>-settings.toml`）。文言は `MoreWords` に足した（`backup`・`export_*`・`import_*`）。

### 使う側がやること

- Advanced ページの `advanced_card` の下に `backup_card(ui, look, words, &default_file)` を置く。返った `Backup` を UI でないスレッドで `ito_common::bundle::export(&file, &base, &app_dir, "<app>", env!("CARGO_PKG_VERSION"))` / `import(&file, &base, &app_dir, "<app>")` に渡し、結果（`usize` の件数、または `Restored` の `written` / `skipped`）を通知かステータスに出す。`base` は `ito_common::base_dir()`、`app_dir` は `ito_common::app_dir("<app>")`。
- 読み込んだあとは、設定を読み直す（common.toml は見張りが拾う。アプリ自身の設定は、アプリの読み直しを呼ぶか、再起動を促す）。
- 検索用に `backup_index(words)` を Advanced の語に足す。

## [0.74.0] - 2026-10-17

### 追加

- ito-prefs: `shell_card`（既定のシェル・引数・環境変数）。値はアプリの設定ファイルのものなので、common.toml には触らず `ShellChange`（`Program`・`Args`・`SetVar`・`RemoveVar`）を返す。環境変数の編集は `ShellEdit`（アプリが持ち続ける）で、保存すると `env_changes` が消した名前と変えた値だけを返す。名前に `=` や空白があると保存できない。検索用に `shell_index` と `SHELL_KEYS`。文言は `ShellWords` の英日。
- 更新の確認は 0.2x の `update_card`（`UpdateWords`）のまま。新しい共通のカードは足さない。

### 使う側がやること

- tsumugi: 「シェルとフック」ページの最初のカード（`prefs.head.shell`）を `shell_card` に置き換える。`ShellChange` を `[shell]` と `[shell.env]` に `set_key` / `remove_key` で書く。フックと連携のカードはアプリに残す。
- ほかのアプリ: 子のプロセスを起こすなら同じカードを使う。

## [0.73.0] - 2026-10-17

### 追加

- ito-prefs: `fonts_card`（外観のページ向け。字体の一覧・大きさ・行の高さ・合字）。値はアプリの設定ファイルのものなので、common.toml には触らず `FontChange`（`Family`・`Size`・`LineHeight`・`Ligatures`）を返す。大きさは `Fonts::size_range`、行の高さは 0.8–2.0 に丸めて返す。`line_height`・`ligatures` を `None` にするとその行は出ない。`Fonts::using` に「見つかったファイルと書体」の行を渡せる。検索用に `fonts_index` と `FONT_KEYS`（`font.family` など）。文言は `MoreWords` の英日。

### 使う側がやること

- tsumugi: 外観の「文字」のカードを `fonts_card` に置き換える（`[font]` の `family`・`size`・`line_height`・`ligatures` へ `FontChange` を `set_key` で書く。字体の一覧は別のスレッドで集めて `families` に渡す）。窓・タイトルバー・背景など外観のほかのカードはアプリに残す。
- 字体の設定を持つほかのアプリ: 同じカードを使う。

## [0.72.0] - 2026-10-17

### 追加

- ito-common:
  - `[window]`（`remember_place`・`always_on_top`）。`Common::window` と `window_or_default()`。知らないキーと bool でない値は警告して外す。
  - `startup`: サインインでの自動起動（Windows は `HKCU\…\Run` の `uchmk-<app>`、macOS は LaunchAgent、Linux は autostart の `.desktop`）。`enabled(app)`・`set(app, exe, args, on)`。ディスクやレジストリに触るので UI スレッドでは呼ばない。
- ito-prefs（`more`）:
  - ページの既定の順 `Page`・`default_order`（一般 → 外観 → テーマ → アプリ → キー → 通知 → 外部ツール → 詳細 → このアプリについて。一般が先頭、このアプリについてが最後）と、その英日の題と一行 `MoreWords`。
  - `window_card`（`CommonChange` と「覚えた場所を忘れる」を返す）、`startup_card`（OS の状態を渡し、押された後の状態を返す）、`advanced_card`（フォルダーを開く・2 回押して初期化）、`about_page`（版・OS・詳細のコピー・不具合の報告）、`forget_place`、検索用の `*_index`。

### 使う側がやること

- 全アプリ: `[window] remember_place = false` のときは窓の場所を戻さず、保存もしない。`always_on_top` は起動時と変更時に窓へ当てる。「忘れる」が返ったら `forget_place(状態のフォルダー)` を呼ぶ。
- 自動起動を載せるアプリ: `startup::enabled` を別のスレッドで調べて `startup_card` に渡し、返った値を `startup::set` で別のスレッドから当てる。
- 初期化（`Advanced::Reset`）は、アプリが設定を脇へ移して読み直す（コピーを残す）。
- tsumugi: ページを既定の順に並べ替える（外観がテーマの前、キーと通知がアプリのページの後、Advanced は詳細）。

## [0.71.0] - 2026-10-17

### 追加

- 新しいクレート `ito-openin`（uchmk/ito#18・#20、kura・michi）: 別のプログラムで開く。
  - ひな形: 語を空白で区切り、`"` でまとめる。`{path} {line} {left} {right} {base} {merged}` を 1 回の走査で置き換える（値の中の `{...}` は展開しない）。値が無い置き換えはエラー。`{path}` が無ければ最後に付け、`{left}`・`{right}` が無ければ 2 つを付ける。シェルは通さない。
  - 起動: 標準入出力を捨て、Windows は `CREATE_NO_WINDOW`、待たずに別のスレッドで回収。`open`・`open_file`（行が渡せたかを返す）・`open_diff`・`open_merge`。
  - 行: 組み込みの表（nvim・vim・code・sakura `-Y=`・EmEditor・hx など）と、アプリの `[line_args]`（`editor_key` で引く。`{path}` を含む語がちょうど 1 つ）。
  - diff / merge ツール: WinMerge・Beyond Compare・Meld・KDiff3・VS Code・git difftool の表と、PATH と Windows の入れ先（`ProgramFiles` など）からの検出（`detect` は同期、`detect_in_background` は別のスレッド）。
  - `Config::read(text)`: `[open]`（terminal・editor・explorer・diff・merge・diff_tool）、`[line_args]`、`[diff_tools.<名>]`（`diff`・`merge`）を読み、読めない行は警告を返して外す。知らないキー（kura の `[open].rules` など）は触らない。`command(tool, found)` は、書いた行 → 選んだ `diff_tool`（名前、自分で足したもの、`auto`）→ 既定の順。
  - `Edit`（`Command`・`DiffTool`・`UserTool`・`RemoveUserTool`・`LineArg`・`RemoveLineArg`）と `Config::check` / `apply` / `Edit::writes`。
- ito-prefs: `open_card`（コマンド 5 行・diff ツールの選択・見つかったツールの行・自分のツールと行の渡し方の追加／変更／削除）と `write_edits`（`ito_common::set_key` / `remove_key` で書く。コメントは残る）。文言は `OpenWords` でアプリが渡す。

### 使う側がやること

- kura: `exec.rs` の `at_line`・`editor_key`・`LineArgs`・`template_is_valid`・組み込みの表を `ito_openin::lines` / `file_argv` に置き換える（大きな移し替えなので、持ち主に確かめてから）。`[open].rules` は読み取りの対象外なので、そのまま使える。
- michi: `michi_core::openin`・`opencfg` を `ito_openin` に置き換え、#43 の diff / merge は `Config::command(Tool::Diff / Merge, &found)` と `open_diff` / `open_merge` で開く。設定画面は `open_card` を呼び、返った `Edit` を `Config::apply` と `write_edits` に渡す。`detect_in_background` の答えは、画面を開いたときに 1 回受ける。
- WinMerge の 3 方向の行は実機で未確認（真ん中の窓が base）。確かめるまでは表の行を信用しすぎないこと。

## [0.70.0] - 2026-10-17

### 追加

- ito-help（kura）:
  - `Line::Text(String)`: `desc` の色の地の文の行（`Bind` と同じ字下げ）。
  - `Line::WarnLink { text, target }`: `Line::Link` の `warn` の色版（読み込みに失敗したファイルなど、気を付けてほしいものへのリンク）。`hit` はどちらも同じように返す。`text` は `text` をそのまま出す。

### 使う側がやること

- `Line` を `match` で網羅しているなら、`Text`・`WarnLink` の枝を足す。それ以外は無し。

## [0.69.0] - 2026-10-17

### 変更

- `templates/app`（新しいアプリの雛形）に、ito と同じ全部の確認を入れた:
  - `scripts/verify.sh`: `cargo fmt --all --check`、build `--locked`、test、clippy `-D warnings` を手元・Linux・`x86_64-pc-windows-msvc`・`aarch64-apple-darwin`（Mac の外では cargo-zigbuild）で、`cargo audit`、`make-testcheck --check`・`make-bugform --check`、`lanes.sh check`、`ito-first.py`。
  - `.github/workflows/ci.yml` の `clippy` を 3 つのターゲットに。`.github/workflows/audit.yml`（push・PR・毎週月曜）を追加。
- `scripts/new-app.ps1`: `lanes.sh sync` を Git の bash で回す（PATH の先頭の `bash` が WSL の起動役で、ディストリビューションが無いと落ちた）。

### 使う側がやること

- 既存のアプリは、`scripts/verify.sh` と CI に同じ確認（特にターゲットごとの clippy と `cargo audit`）が入っているか見て、無ければ雛形から写す。ito の rev を上げる必要は無い。

## [0.68.0] - 2026-10-17

### 追加

- ito-help（uchmk/ito#17、kura）:
  - `Line::Cmd { keys, desc, cmd }`: `Bind` に、右端へ右寄せで `Style.dim` の色のコマンドを足したもの。コマンドは行の 4 割までで、入りきらなければ `…` で切り、説明がその分だけ狭くなる。
  - `Line::Warn(String)`: `Style.warn` の色の行（`Bind` と同じ字下げ）。
  - `Line::Link { text, target }`: keys の色の行。`hit(body, len, scroll, style, pos) -> Option<usize>` が、`draw` / `draw_with` で描いた一覧の `pos` の下の行の番号を返す（ホバーもクリックもこれで引く。ホバーの背景やカーソルの形はアプリが描く）。
  - `text(lines)`: 行を文字にする（`keys<TAB>desc<TAB>cmd`、`Row` はタブ区切り、`Blank` は空行）。「全部コピー」と描画がずれない。
  - `Style` に `warn` と `dim`。

### 使う側がやること

- `Style { … }` を自分で組み立てているなら `warn` と `dim` を足す。`Line` に `Cmd`・`Warn`・`Link` が増えた（`match` で網羅しているなら枝を足す）。それ以外は無し。

## [0.67.0] - 2026-10-17

### 追加

- ito-common（uchmk/ito#15、kura）: 機能 `dirwatch` の `dirwatch::DirWatcher`。画面に出ているフォルダーだけを `notify` でイベント駆動に見張る（再帰しない）。
  `DirWatcher::new(wake)`、毎フレーム呼んでよい `sync(dirs)`（見張る集合を丸ごと渡す。新しい方が勝つ）、変わったフォルダーが届く `rx: std::sync::mpsc::Receiver<PathBuf>`。
  `ReadDirectoryChangesW` の登録は死んだネットワーク共有で固まりうるので、`notify` のウォッチャーは専用のスレッドに置き、UI スレッドは集合を渡すだけ。既存の `watch`（mtime のポーリング）は設定ファイル向けでそのまま。

### 使う側がやること

- kura は `src/fs/watch.rs` と `notify` の依存・`scripts/ito-first.allow` の行を外し、`ito-common` に機能 `dirwatch` を足して `DirWatcher` を使う。`rx` が crossbeam でなく std の `Receiver` になる（`try_iter` / `try_recv` / `recv_timeout` はそのまま）。

## [0.66.0] - 2026-10-17

### 追加

- ito-match（uchmk/ito#16、kura）: `Subst::new(pattern, replacement, all, ignore_case)` と `Subst::apply(text)`。sed 風の置換を `Matcher` と同じ正規表現（fancy-regex。先読み・後読み・後方参照、置換の `$1`）で行う。`ignore_case` は `(?i)` を前に付ける。

### 使う側がやること

- kura は `fancy-regex` の直接の依存と `scripts/ito-first.allow` の行を外し、`Rule::Subst` を `ito_match::Subst` に替える。それ以外は無し。

## [0.65.0] - 2026-10-17

### 追加

- ito-help（uchmk/ito#13、minato）:
  - `Overflow::{Bar, Counts { above, below }}` と `draw_with` / `draw_plan_with`: スクロールバーの代わりに、見えない行が上にあれば最初の行を `above(n)`、下にあれば最後の行を `below(n)` の文言に置き換える（段ごと）。`draw` / `draw_plan` は今まで通り `Bar`。
    マーカーが隠す行も `n` に数える。1 ページ（`visible`）ずつ動かすと、マーカーの下の行を飛び越すことがある。
  - `Line::Row(Vec<String>)`: 表の行。続く `Row` の組は、各列を内容でいちばん広い幅にそろえ、入りきらなければいちばん広い列から縮めて `…` で切る。1 列目は keys の色、あとは desc の色。
    `deal` / `plan` は `Group` しか配らないので、表は `plan.columns` の段に `Row` を足すか、`draw_with` に `Line` の並びを渡す。

### 使う側がやること

- `Drawn` に `above` / `below` が増えた（`Drawn { … }` を自分で作っているなら足す）。`Line` に `Row` が増えた（`match` で `Line` を網羅しているなら足す）。それ以外は無し。

## [0.64.0] - 2026-10-17

### 追加

- ito-testcheck: セクションの番号に、数字のあとの英小文字を使える（`## 3d. Disks`、id `3d.8`、注の `[section.3d]`、`--stats`。uchmk/ito#14）。
  並びは TESTING.md に書いた順のまま（3、3a、3d、4）。数字だけのアプリ（kura・tsumugi）は変わらない。

### 使う側がやること

- 無し（足すだけ）。

## [0.63.2] - 2026-10-17

### 直した

- merge-lanes.py: yagura の Windows のレーンが `TESTING-CHECKS.md`（生成した表）の印も見る（`TESTING.md` は yagura の切り替えまで残す。uchmk/ito#12）。

### 使う側がやること

- yagura: `lanes.sh sync`。

## [0.63.1] - 2026-10-17

### 直した

- ito-select: ドラッグの両端の文字を、押した（離した）場所が文字のどこでも丸ごと選択に入れる。これまでは両端を「いちばん近い文字の境目」に丸めていたので、
  全角文字の右半分で押す・左半分で離すと、その文字が半分の扱いで外れていた。端末と同じく、ポインターの下の文字が必ず入る。

### 使う側がやること

- 無し。

## [0.63.0] - 2026-10-17

### 追加

- ito-help: 段に分けて、スクロールなしで収める見せ方（uchmk/ito#10、tsumugi）。
  - `Group { title, rows: Vec<(keys, desc)> }`: 見出しと行の組。keymap から作らないアプリ向け（`Group::lines()` で `Line` に）。
  - `deal(&groups, cols)`: 組を順のまま・割らずに、いちばん高い段が最も低くなるよう `cols` 段までに配る（段の中の組の間は空行）。
  - `columns_for(width, min_col, max)`: 幅に入る段の数。
  - `plan(&groups, room, row_h, max_cols, floor) -> Plan { columns, scale, scrolls }`: 1 段から `max_cols` 段まで増やし、それでも入らなければ字を `floor`（0.7 が目安）まで縮め、それでも入らなければ `scrolls`。
    行は `…` で切って折り返さないので、高さは描かずに分かる（フレームをまたいで測り直す `Fit` の状態は要らない）。
  - `draw_plan(painter, body, &plan, scroll, style) -> Drawn`: `plan` の段を横に並べて描く。スクロールの棒は全体で 1 本。
  - 既存の `draw`・`Line`・`push_layer` はそのまま。

### 使う側がやること

- 無し（足すだけ）。tsumugi は `help.rs` の `deal`・`columns_for`・`most_columns` と縮める処理を `plan`・`draw_plan` に置き換え、`groups()` のデータだけ残す。枠・暗くする背景・閉じる処理はアプリのまま。

## [0.62.0] - 2026-10-17

### 追加

- ito-prefs: フィードバックのカード `bug_card(ui, look, words) -> bool`（不具合を報告のボタン 1 つ）と、検索用の `bug_index(words)`。`Words` に `bug_head`・`bug_label`・`bug_note`・`bug_button` を足した（EN・JA）。設定の画面のボタンは `docs/common-spec.md` の約束だった。

### 使う側がやること

- 設定の画面のどこか（Language などのページの末尾か、About のページ）で `if ito_prefs::bug_card(ui, look, words) { …F12 と同じ処理… }` を呼び、`Nav::index` に `bug_index` の語を足す。
- `Words { … }` を自前で書いているアプリは、4 つの欄を足す（`Words::of` を使っていれば何もない）。

## [0.61.1] - 2026-10-17

### 修正

- ito-testcheck `strict`: 表の列の数を `parse` と同じ規則（` | ` で分ける）で数える。コード片の `>|` や `|>` を列の区切りと数えて落ちていた（tsumugi）。行の末尾の印（`[ ]` など）も列に数えない。

### 使う側がやること

- 無し。tsumugi は `TESTING.md` に入れた `\|` を戻してよい。

## [0.61.0] - 2026-10-17

### 追加

- ito-common: `home_dir()`（Windows は `USERPROFILE`、ほかは `HOME`。空は無いものとして扱う）。`dirs` の代わり（uchmk/ito#8）。
- ito-ipc `frame`（線の上の形は変わらない。uchmk/ito#9）:
  - `frame(&msg)` / `frame_max(&msg, max)`: 長さ付きの 1 通を一度だけ作り、同じバイト列を何人にも送る。
  - `read_body(r)` / `read_body_max(r, max)` と `decode(&[u8])`: 先に本体のバイトを読み、あとで読み解く。`max` は読み手が選べる上限（既定は `MAX_BODY` = 64 MB）。
  - `head(body)`: 本体の最初の 2 つの数（enum の番号と、その次の varint = 版など）だけを、どの版が送ったものでも読む。古い・外の相手を早く断るため。

### 使う側がやること

- 無し（足すだけ）。yagura は `dirs::home_dir()` を `ito_common::home_dir()` に、直の `postcard` を `ito_ipc::frame` に置き換えられる。

## [0.60.0] - 2026-10-17

### 追加

- ito-testcheck（tsumugi の試し読みで出た 4 つ）:
  - `TESTING.md` の `## N. 題` でない `##` 見出し（`## Covered by tests` など）の下の表は、チェックの行として読まない（読み手のための表を残せる）。これまでは見出しの前の行として panic していた。
  - 注釈の TOML の先頭に `plain = true`: 訳の無い行を `- [ ] **id** 中身` のまま出し（`〔未訳〕` を付けない）、未訳の数は 0、決まった文言（準備・自動・済みだが無い項目）は英語になる。英語だけのチェック表向け。
  - 注釈の TOML の先頭に `strict = true`: `TESTING.md` の不具合（同じ節の中の id の重複、制御文字、表の見出しと違う列の数、表の行の直後に空行なしで続く行）を `file:line` 付きで出して exit 1（何も書かない）。
  - `keycheck::run_bindings(cfg, &[Binding], root, args)` と、公開の `keycheck::Binding`: `keymap.toml` でなく Rust のデータからキーの表を作るアプリ向け。ticks・`--check`・`--stats` は `run_with` と同じ。

### 使う側がやること

- 無し（どれも足すだけ。`Config`・`Lane` は変わらない）。kura の `TESTING.md` に番号の無い `##` 見出しの下の表があれば、そこの行はチェックに数えなくなる。

## [0.59.1] - 2026-10-17

### 変更

- 雛形（`templates/app/CLAUDE.md`）と ito の `CLAUDE.md`: コミットと push は、`scripts/verify.sh` が `ALL OK` なら頼まれなくてもしてよい（`main` へ直接、force push はしない）。

## [0.59.0] - 2026-10-17

### 追加

- ito-palette: 自分で絞り込み・並べる使い方に要るもの（michi の Picker）。
  - `View::filter`（既定 true）。false だと `show` は `entries` を並べ替えも絞り込みもせず、渡された順に全部出す（打った文字は `View::query` に残る）。
  - `Entry::hits`（title の何文字目が一致したか。`Style::hit` の色で描く）と `Entry::tone`（`Tone::Normal` / `Tone::Accent`。`Style::accent` の色）。
  - `Words::header`（検索欄の下、一覧の上の 1 行）と `Words::footer`（一覧の下の 1 行。キーの一覧など）。空なら出さない。
  - `Words::none` が空なら、合う行が無いときも何も出さない（名前の変更のように入力欄だけを出す使い方）。

### 使う側がやること

- `Style { … }` に `hit` と `accent`、`Words { … }` に `header` と `footer`（空文字でよい）を足す。`Entry::new` を使っていれば `Entry` は変えなくてよい（`Entry { … }` を直に書いているなら `hits: vec![]`、`tone: Tone::Normal` を足す）。

## [0.58.0] - 2026-10-17

### 追加

- ito-layout: 表・一覧のヘッダーの列の端のつまみ（uchmk/ito#7、michi）。
  - `columns`（egui なしで使える）: `resize(widths, index, delta, min, max, Edge)` は 1 つの列の幅を `min..=max` に収めて動かす（`Edge::Right` は右へ、`Edge::Left` は左へ引くと広がる）。
    `trade(widths, index, delta, min)` は隣の列と幅を分け合い、合計を保つ（残りを埋める柔軟な列）。`right_edges(start, widths)` は各列の右端の x。
  - `ui::grips(ui, id, edges, y, grab) -> Option<Grip>`（機能 `egui`）: 各端に幅 `grab` の帯を置き、カーソルを `ResizeHorizontal` にする。
    `Grip { index, delta, released, reset }` は、ドラッグ中は `delta`、離したときは `released`（幅を保存する）、ダブルクリックは `reset`（既定の幅へ戻す）。
  - 使う側がやること: 幅の保存と既定の幅はアプリのもの。`grips` の `delta` を `resize` か `trade` に渡し、`released` で保存する。

## [0.57.0] - 2026-10-17

### 追加

- ito-palette: 選んだ行への Enter 以外のキー。`View::keys`（`(Modifiers, Key)` の一覧。`Modifiers::COMMAND` は Ctrl、macOS では Cmd）に入れたキーが押されると、
  `show` が `Answer::Key(modifiers, key, T)` を返す。Enter より先に見るので `Shift+Enter` も登録できる。合う行が無いときはキーを受けるだけで何も返さない（michi の Picker）。

### 使う側がやること

- `Answer` に枠が 1 つ増えた。`match` で `Pick`・`Close` を網羅している所（yagura の `palette` の処理）は `Answer::Key(..)` の枠を足す（`View::keys` が空なら来ない）。

## [0.56.1] - 2026-10-17

### 修正

- 0.56.0 は git の依存として読めなかった（`templates/app` の `Cargo.toml` の `name = "{{app}}"` を cargo がパッケージとして読み、`invalid character` で解決が落ちる。uchmk/ito#6）。
  雛形の `Cargo.toml` を `Cargo.toml.tmpl` に改め、`scripts/new-app.ps1` が書くときに戻す。**使う側は 0.56.0 の `rev` に上げず、0.56.1 以降にする。**

## [0.56.0] - 2026-10-17

### 追加

- `templates/app` と `scripts/new-app.ps1`: 新しい uchmk のアプリの雛形と生成。ito の必須のクレート（common・i18n・theme・prefs・keys・keymap・keyscript・help・notify・bugreport・testcheck・console・winres・palette）、窓の場所の保存・テーマ・言語・倍率・F12・`--version`/`--help`・`.com`・bug form・testcheck の例を入れる。残り（`--keys`・キーのページ・ヘルプ・通知・パレット）は生成された `TODO.md`。
- `scripts/lanes/ito-first.py`（`ito-first.txt`・`ito-required.txt`）: アプリが ito の受け持つ依存を直接足す／必須の ito のクレートを欠くと落とす。例外は `scripts/ito-first.allow`。
- `.github/ISSUE_TEMPLATE/crate-request.yml`: ito に無い機能を ito に頼む Issue の雛形。

### 修正

- ito-testcheck: TESTING.md の行末の印の欄（`[ ]`・`[x]`・`[~]`・`[-]`）を検査の欄として数えない（uchmk/ito#5。michi）。

### 使う側がやること

- `scripts/lanes/FILES` に 3 つ増えた。`lanes.conf` に `files=` を持たないアプリは `lanes.sh sync` で 3 つが入る。`ito-first.py` が落ちたら、ito のクレートに置き換えるか、`scripts/ito-first.allow` に `<crate> <理由>` を書く。`files=` を持つアプリは、使うなら名前を足す。

## [0.55.0] - 2026-10-17

### 変更

- ito-pane: filer の頃の名前が並んでいた公開の関数を、何をするか分かる名前に改めた（TODO の「ペインの公開の API の名前を整理する」）。古い名前は残さない。

  | 旧 | 新 | 中身 |
  |---|---|---|
  | `encode` | `key_bytes` | 名前の付いたキー（`Special`）をシェルに送るバイトにする（kitty のものは `kitty::encode` のまま） |
  | `snapshot` | `cell_rows` | グリッドの見えている行を `CellView` の行で取り出す |
  | `children` | `child_pids` | プロセスの子の pid |
  | `stem` | `program_stem` | プログラムの名前（拡張子なし・小文字）。比べるために使う |
  | `fit` | `fit_cells` | 矩形に入るセルの数（`Size`） |

  - 使う側がやること: `ito_pane::encode`・`snapshot`・`children`・`stem`・`fit` を上の新しい名前に替える（`pub use ito_pane::*` で受けているなら `terminal::encode` なども）。
    `Terminal` のメソッド（`blocks` など）と `Pane` の trait は変えていない。

## [0.54.0] - 2026-10-17

### 追加

- ito-prefs `update_card`（#3 の候補 7）: 設定画面の「更新を確かめる」のカード。スイッチの下に、直近の確認の結果
  （新しい版がある・最新・確かめられない・確認中）を 1 行で出す。`update_card(ui, look, &UpdateWords, on, Option<&Update>) -> Option<bool>` が
  押されたときの新しい状態を返し、設定ファイルへの書き込みと確認の開始はアプリが行う。文言は `UpdateWords`（`newer` の `{}` に版が入る）。
  - 使う側がやること: yagura・kura・tsumugi の「更新」のカード（スイッチ 1 行だけ）をこれに替え、`ito_common::update::check` の結果を `Option<Update>` で持って渡す。

## [0.53.0] - 2026-10-17

### 追加

- `ito-confirm`（#3 の候補 10）: 「やる前の確認」の重ね表示。暗い背景、題の付いた箱、`キー : 値` の行（何を）、折り返す本文、警告色の注（どうやって）、
  ボタン、答えのキーのヒント。`show(ctx, &Question, &Style) -> Option<usize>` が押されたボタンの番号を返す。キーの読み取り（Enter・Esc・`y`・`n`）はアプリの keymap のまま。
  ボタンは箱からはみ出さず折り返す（切れて見えないのに押せるボタンを作らない）。yagura の `ui/confirm.rs` と kura の `overlay::confirm` が元。
  - 使う側がやること: 自前の箱の描画を `Question` に詰めて `show` に替える。tsumugi の許可の確認は一覧の中の行なので対象外。
- `ito-help`（#3 の候補 5）: キーのヘルプの描画。`push_layer` が `ito_keymap::help_lines` の結果を `Line`（見出し・キーと説明・空行）に並べ、
  `draw(painter, body, &lines, scroll, &Style) -> Drawn` が渡された矩形の中に描く（長い文字は `…` で切り、一覧が長ければ右にスクロールバー）。
  `Drawn { visible, max, scroll }` はページ送りに使う。枠・題・背景はアプリの `panel` のまま。yagura の `ui/help.rs` が元。
  - 使う側がやること: yagura は `Line` と `group` を `push_layer` に替える。kura は設定の出どころの行やクリックで飛ぶ行を持つので、一覧の描画だけ寄せるかは kura の判断。

## [0.52.0] - 2026-10-16

### 追加

- ito-common `text`: 名前の自然順（`natural_cmp`・`alpha_cmp`）、バイト数（`bytes_iec` = yagura の `512 B`/`3.4 KiB`、`bytes_short` = kura の `1.0 K`/`20 M`）、
  待ち時間（`duration_short`）、稼働時間（`uptime`）。kura の `util.rs` と yagura の `util.rs` から移した（#3 の候補 10）。表記は 2 つのアプリで違うので、どちらも残した。
  - 使う側がやること: kura は `natural_cmp`・`alpha_cmp`・`human_size`（= `bytes_short`）・`fmt_duration`（= `duration_short`）を、yagura は `fmt_bytes`（= `bytes_iec`）・`fmt_uptime`（= `uptime`）を置き換える。

## [0.51.1] - 2026-10-16

### 変更

- `runinfo::RunInfo::rendering()` が返す順を `[adapter, backend, device]` にした。`ito_bugreport::Context.rendering` と同じ順なので、そのまま渡せる (#4)。
  - 使う側がやること: 手で組み直していれば `info.rendering()` を直接 `Context.rendering` に渡す。

## [0.51.0] - 2026-10-16

### 追加

- ito-common `report::version_rows(version, os_line)`・`files_rows(app)`・`presence(path)`: `<app> env` の最初の 2 節(Version/Executable/OS/Debug と、
  uchmk フォルダ・アプリのフォルダ・common.toml・themes・状態フォルダ・last-run.toml)を全アプリで同じ行・同じ字面にする（#3 の候補 7）。
  - 使う側がやること: kura・yagura の `envreport.rs` の `version()` と `files()` の共通の行をこれに置き換え（`os_line` は `ito_bugreport::os_line()` を渡す）、
    アプリ固有の行はその後ろに足す。tsumugi の `env` は `report::section` 形式（`Key : value`）に寄せる。

## [0.50.0] - 2026-10-16

### 追加

- ito-common `runinfo`: 直前の起動が使ったもの（GPU・フォント・窓の pt と ppp・起動時刻・バックエンドの設定）の記録。
  kura の `runinfo.rs` の共通の部分を移した（#3 の候補 7）。`RunInfo`・`save`/`load`（`state_dir(app)/last-run.toml`）・`window_line`・`rendering`・`started_ago`・`push_recent`。
  アプリ固有の項目は `extra`（`toml::Table`）に入れる。
  - 使う側がやること: kura は `pane`・`launched`・`pane_shell`・`pane_failed` を `extra` へ移し、`runinfo.rs` をこれに置き換える。
    yagura・tsumugi は最初のフレームで `RunInfo::new(version)` に埋めて `save` し、`env` の GPU の節と報告の Rendering/Scale が `load` を読む。
- ito-bugreport `template(app, &[Field])`・`field_ids`: issue form を全アプリで ito が作る。
  GitHub は form を別のリポジトリから読めないので、アプリは出力を `.github/ISSUE_TEMPLATE/bug_report.yml` に書き、テストで一致を見る。
  アプリ固有の欄は `Field` で足す。`templates/bug_report.yml` は直接写さず、`template` を使う。

## [0.49.0] - 2026-10-16

### 追加

- ito-bugreport（新しいクレート）: 不具合の報告の URL（`url`）と、その部品。kura の `bugreport.rs` のうちアプリに依らない部分を移した
  （#3 の候補 7）。`Spec { app, version, repo, template }`・`version_line`・`os_line`（Windows の名前・ビルド・本物のアーキ・エミュレーションの表示）・
  `Context` と `context`（Last error・Rendering・Scale・アプリの行・Config の順）・`without_home`・`short_name`。依存は Windows の `windows` だけ。
  - form の field の id（`version`・`os`・`context`・`what`・`backtrace`）を共通仕様に固定し、雛形 `templates/bug_report.yml` を置いた。
  - 使う側がやること: kura は `src/bugreport.rs` を `Spec`/`Context` を渡す薄い層に替える（`context` の文面は今までと同じ）。
    yagura・tsumugi は `.github/ISSUE_TEMPLATE/bug_report.yml` を雛形から作り、F12 から `url` を開く。
  - runinfo（直前の起動が使った GPU・フォント・倍率の記録）の共通化は次の版。

## [0.48.0] - 2026-10-16

### 追加

- ito-keymap: `live(layer, noop)`（キーが届く束縛だけ。`noop` に変えた束縛と、先の短いキーに隠れた束縛を除く）と
  `help_lines(layer, noop)`（`live` を、`desc` と `run` が同じものを 1 行にまとめ、キーは ` / ` でつなぐ。`HelpLine { keys, desc, run }`）。
  ヘルプ（F1）の一覧の材料（#3 の候補 5）。描くのはアプリの見た目のままで、ここは一覧の作り方だけ。
  - 使う側がやること: yagura の `config::keymap::live` と `ui/help.rs` の `group`、kura のヘルプの同じ処理を、これに置き換える
    （`noop` には各アプリの何もしない命令の文字、たとえば `"noop"`）。

## [0.47.1] - 2026-10-16

### 修正

- ito-theme: 明るい 2 つのテーマの状態の色を、bg・side・panel のどれの上でも WCAG AA（4.5:1）に届くまで暗くした（色相は変えず明度だけ）。
  - tsumugi Light: wait `#b07c1a`→`#8f6515`、run `#137a7a`→`#137979`、done `#2e7d3a`→`#2d7b39`。
  - Catppuccin Latte: wait `#b46a0a`→`#985908`、run `#04769f`→`#047097`、done `#40a02b`→`#307720`、blue `#1e66f5`→`#0c5af4`（本家の配色から少し外れる）。
  - テスト `the_status_colours_of_the_light_themes_reach_aa` で守る。
  - 使う側がやること: `rev` を上げて `Cargo.lock` を合わせるだけ（API の変更なし）。
  - 未対応（測っただけ）: Gruvbox Light の wait・done、Solarized Light の wait・blue・magenta なども 4.5:1 に届かない。

## [0.47.0] - 2026-10-16

### 追加

- ito-notify: Webhook の送り先を幾つでも持てる（tsumugi #164）。`webhook::Place`（`url`・`format`・`states`）が設定の
  `[[notify.webhooks]]` の 1 つで、`states` に送る出来事の名前を並べる（例: ntfy には `["waiting", "error"]`、Discord には `["done"]`）。
  `webhook::places(old, list)` が前の 1 つだけの形（`[notify] webhook`・`webhook_format`・`webhook_states`）を 1 つ目の送り先にして
  表の前に置き、`webhook::send_all(places, state, …)` がその出来事を受ける送り先すべてに送る。`Place::problems` は設定の警告の文。
- ito-prefs: 通知のページの「Webhook」のカードが送り先の一覧になった。送り先ごとに名前（1 つなら「Webhook」、2 つ以上なら
  「Webhook 1」「Webhook 2」…）と「外す」「ここに試しに送る」、アドレスと形式。最後に「アドレスを足す」。出来事ごとに送り分けるアプリでは、
  「知らせるとき」の表に送り先ごとの行が出る。`webhook_rows` は同じ行を自分のカードに置くアプリ（tsumugi）のため。
  `write_webhooks(text, places, states)` が設定ファイルの `[[notify.webhooks]]` を書き、前の 1 つだけの鍵を取り去る。

### 変更

- ito-common: `set_tables` が表を `name`・`addr` のほかに `url` でも見分ける（`[[notify.webhooks]]` の表が、間の 1 つを外しても
  コメントと一緒に残るように）。

### 使う側がやること

- `ito_prefs::Notify` の `webhook`（出来事ごとのスイッチ）・`webhook_url`・`webhook_format` が `hooks: Vec<Hook>` になった。
  `Hook { url, format, on }` の `on` は出来事ごとのスイッチで、空なら全部の出来事を送り、表に行を出さない。
  設定から作るときは `Hook::of(&place, events, rows)`、送るときは `hook.place(events)`。
- `notify_page` は `bool` でなく `Option<Test>` を返す（`Test::All` は上の「試しに送る」、`Test::Hook(k)` は k 番目の送り先だけ）。
- `Words::notify_webhook_note` は無くなった（新しい言葉は `webhook_empty`・`webhook_add`・`webhook_remove`・`webhook_test`・
  `webhook_none`・`webhook_more`）。
- 設定を読むときは `[[notify.webhooks]]` を読み、前の鍵を `webhook::places` で頭に足す。送るときは `webhook::send_all`、
  書くときは `write_webhooks`（出来事が 1 つのアプリは `states` を書かなくてよい）。

## [0.46.1] - 2026-10-16

### 修正

- ito-pane（Windows）: 窓の幅を変えると、最後のコマンドの出力（`last_output`、tsumugi の Ctrl+Shift+L）が取れない（`None`）か、
  前のプロンプトとコマンドまで含んでいた。プロンプトの行（`prompt_lines`）とブロックも同じ。ConPTY は大きさが変わると自分の画面を
  折り返し直して全部描き直すが、OSC 133 を付けないので、行の印（プロンプト・入力のリンク）が消え、開いたままだった入力の印が描き直しの
  頭の行に付いていた（tsumugi #16）。大きさを変える前に画面の印を覚え、ConPTY が描き直した行のうち同じ字の行に（同じ順で）戻す。
  開いていた印は描き直しの最初の字の前で閉じる。狭くしたときに格子がスクロールバックへ押し上げた行（ConPTY は画面に描き直す）からは
  印を外し、同じプロンプトが二つに数えられないようにする。スクロールバックが満杯のときは押し上げた行の数が分からず、外せない。

## [0.46.0] - 2026-10-16

### 追加

- ito-pane: `Shown::cursor`（`Option<CursorShown>`）。そのフレームにカーソルを描いたセル（`cell`、列・行。右から左の行では画面の上の位置）と、
  `CursorStyle::glide` のときの最後の動き（`slide: Option<Slide>`。`from`（セル、曲がった途中なら小数）・`to`・`between`（途中で描いた
  フレームの数。0 なら跳んだ）・`moving`）。窓の画像では滑りの途中を撮れないので（tsumugi #86、行 17.41）、アプリがこれを `--keys` の状態に出せば、
  滑ったかどうかを文字で確かめられる。カーソルのセルが格子の外（スクロールで戻した先）か、端末が無いときは `None`。

### 修正

- ito-pane: カーソルの滑り（glide）を切ってから入れ直すと、次の動きが、切る前に最後に滑った先から始まっていた。切っている間は覚えていた位置を捨てる。
- ito-pane: 字の大きさや行の高さを変えると、カーソルが同じセルのまま滑っていた。滑りの位置をピクセルではなくセルで覚える。

使う側がやること: `Shown { … }` を自分で作っているところに `cursor: None` か `..Default::default()` を足す。滑りを確かめたいなら、
`show_faces` の戻りの `shown.cursor` をアプリの状態に残して読む。

## [0.45.0] - 2026-10-16

### 修正

- ito-pane（Windows）: ペインを閉じたときの `end_tree` が、シェルの子でないプロセスまで終わらせることがあった。Windows では親が終わっても
  子は親の PID を持ち続け、その PID は別のプロセスに回る。シェルがたまたま、とうに終わった親の PID をもらうと、その「子」（例: explorer.exe と
  そこから起きたもの全部）がシェルの下に見えていた。CI ではテストのサーバーが自分のテストのプロセスを終わらせ、lib のテストが
  `test result` の行も無く exit 1 で終わっていた（tsumugi #175）。プロセスの起動の時刻（`GetProcessTimes`）を見て、親より先に起きたものは
  その親の子と見なさない。`end_tree` は自分のプロセスも終わらせない。`children`・`named_children` も同じ見方をする。

### 変更

- ito-pane: `Proc` に `started: u64`（起動の時刻。Windows は 1601 年からの 100 ns、分からなければ 0。Windows 以外は常に 0）を足した。
  `descendants` は両方の時刻が分かるとき、親より先に起きたものを子に数えない。Windows の `process_table` は 1 回につき 0.4 ms ほど（プロセス 430 個で）遅くなる。

使う側がやること: `ito_pane::Proc { pid, ppid, name, args }` と書いているところに `started: 0` か `..Default::default()` を足す。

## [0.44.1] - 2026-10-16

### 修正

- scripts/lanes/merge-lanes.py: 報告の `None from this run.`・`Nothing from this run.` も「何も無い」と読む。
  kura #412 は Proposals に `None from this run.` しか無いのに、仕分けの issue になっていた。

## [0.44.0] - 2026-10-16

### 追加

- ito-prefs: 設定の画面の行を、設定ファイルのキー（`ui.font_size`・`clock.show`）でも検索で見つける `keyed_row(ui, l, key, label, note, control)`。
  キーは画面に出さない。日本語の画面でも `font_size` で行が光り、Enter でその部品に入るので、`--keys` の台本が言語によらず行に届く（kura #388）。
  `row` は空のキーの `keyed_row`。共通の行（`language_row`・`scale_row`・`clock_card`）は common.toml のキー（`language`・`scale`・
  `clock.show` など）を持ち、`Nav::index` に足すための `GENERAL_KEYS` を置いた。
- docs/common-spec.md: 「設定の画面」の検索に、行をキーでも見つけることを書いた。

使う側がやること: 設定ファイルに値のある自分の行を `keyed_row` にし、そのキーを index に足す。General のページの index に
`GENERAL_KEYS` を足す。

### 修正

- scripts/lanes/merge-lanes.py: 報告の Proposals が `None new.`・`None this run.`・`None filed this run.` だけのとき、仕分けの issue を
  立てていた（kura の #366・#368・#370・#376・#386・#395）。`NOTHING` がこの形（`new`・`filed`、後ろの `this run`）も「何も無い」と読む。
  `None this run passed 2.4.` のように後ろに中身が続くものは、これまでどおり拾う（kura #403）。

## [0.43.0] - 2026-10-16

### 追加

- ito-common: 窓の場所を開くときに読む `window::load(dir)`。kura・yagura・tsumugi が同じ手順をそれぞれ持っていたもの
  （`read` → `reachable_now(PRIMARY_GUESS, 100.0)`、tsumugi は古いファイルの読み直しも）を 1 つにした。今ある画面に出ない場所は返さない。
  `zoom` の無い古い `window.toml`（この形の 0.38 までのもの＝`w`・`h`、kura と yagura。tsumugi v0.113.1 までの独自の形＝`width`・`height`、
  `scale` もあり得る。数は整数でもよい）は、common.toml の `scale`（アプリが読んだ後に掛ける拡大）で取ったものとして読み、この形で書き直す
  （最大化した窓は動かないかぎり書かれないので、後で拡大が変わっても大きさが変わらないように）。
- ito-common: 定数 `window::PRIMARY_GUESS`（1920×1080。画面を聞けない macOS・Linux での主の画面の見当）と `window::GRAB`（100 ポイント）。
- ito-common: アプリの窓の場所を束ねる `window::Saved`。`Saved::load(Option<dir>)`（`None` は場所を持たない窓: `--keys` の台本、
  引き剥がした窓、quake）、`place()`、`viewport(builder)`（機能 `egui`。`opening` の位置・大きさ・最大化を `ViewportBuilder` に付ける）、
  `put(window)`（eframe の creator で）、`tracker()`（`Tracker::new(dir, place)`、場所を持たない窓は `None`）。
- ito-common: `Tracker::follow(ctx)`（機能 `egui`）。`see(Seen::of(ctx))` して、待ちが返れば `request_repaint_after` する。

### 変更

- ito-common: `scale` の無い場所（古いファイル）も `Window::opening` が `zoom` を掛ける（`zoom` が 1 なら前と同じ）。`seen_on` もその画素を
  主の画面の倍率×`zoom` で見る。kura と yagura の古いコードは egui の点（拡大込み）を書き、拡大を抜かずに窓を作っていたので、common.toml の
  `scale` が 1 でないと、開くたびに窓が `scale` 分小さくなっていた（yagura の持ち主のファイルは 130 % で 1280×800 が 984.6×615.4 になっていた）。
  `load` で読めば、前に見えていた大きさで開く。

使う側がやること: 自前の `PRIMARY_GUESS` と `read(...).filter(reachable_now(...))` をやめ、`let saved = Saved::load(<state_dir、場所を持たない窓は None>)`、
`viewport = saved.viewport(viewport)`、creator の先頭で `saved.put(cc)`、`Tracker` は `saved.tracker()`（`Option<Tracker>`）、毎フレーム
`tracker.follow(ctx)`。`on_exit` の `tracker.close(Seen::of(ctx))` はそのまま。古い形のファイルを自分で読んでいたら（tsumugi の `winplace::read`・`old`）消す。
`window::read` は今のまま残る（古いファイルを拡大無しで読む）。

## [0.42.0] - 2026-10-16

### 追加

- ito-console（新しいクレート）: Windows の窓のアプリの前に置くコンソールの前段 `front(app, answers_in_text)`（`<app>.exe` の横の
  `<app>.com`）。kura・yagura・tsumugi・minato・michi がそれぞれ 200 行ほどずつ持っていたものを 1 つにした。文字で答えるコマンドラインは
  `<app>.exe` を同じハンドルで走らせて待ち、ほかは窓が入力を待ち始めたら戻る（その前に終われば、断りの 1 行と終了コードを渡す）。
  どれを文字で答えるかはアプリが決める。kura と yagura の規則（値を取るオプションの値を飛ばして左から読む）は `any_of` にした。
  窓の出力がパイプのときは、窓にパイプでなく一時フォルダのファイル（`<app>-com-<pid>-<n>.out`・`.err`。窓が終われば消える）を渡し、
  窓が上がった時点までの中身をパイプへ写す。これまで kura・yagura・minato・michi は窓にパイプを渡して写していたので、`.com` が
  終わった後の窓の書き込みが誰も読まないパイプに当たり得た。tsumugi は窓にコンソールを渡していたので、パイプで受けた断りが消えていた。
  出力がコンソールかファイルなら、窓はこれまでどおりそれを持つ。
- ito-console: 答えを書く `out`・`err`。ファイルかパイプならそこへ、どちらでもなければ（窓のプログラムを cmd から起こしたとき）
  親のコンソールへ UTF-16 で書く。書けなければ捨てる（`println!` は、PowerShell が読まずに閉じたパイプで panic する）。改行は足さない。
- docs/common-spec.md: 「Windows のコンソールの前段（`<app>.com`）」の節。

使う側がやること: `ito-console` を足し（ほかの ito のクレートと同じ `rev`）、`src/bin/<app>-com.rs` を `answers_in_text` と
`fn main() { ito_console::front("<app>", answers_in_text) }` にする。答えを書く自前の関数（kura・minato・michi の `say`、tsumugi の
`out`）の中身を `ito_console::out`・`err` にし、`println!` で書いている答えもそれにする。使わなくなった `windows` / `windows-sys` の機能を外す。

## [0.41.0] - 2026-10-16

### 追加

- ito-common: 窓を追って `window.toml` に書く `window::Tracker` を足した。kura・yagura・tsumugi がそれぞれ持っていた追跡
  （kura の `remember_place`、yagura の `Place`、tsumugi の `winplace::Tracker`）を 1 つにした。
  動いて `SETTLE`（0.8 秒）止まったら名前 `window-place` のスレッドで書き、`close` はすぐに書く。最大化している間は前の場所を保ち
  （最大化を解くとそこへ戻る）、最小化と全画面の間は追わない。場所は `scale`（`pixels_per_point`）と `zoom`（`zoom_factor`）と一緒に書くので、
  倍率の違う 2 つ目の画面で閉じた窓もそこに開く（kura #401、yagura #58、tsumugi #176）。
- ito-common: 1 フレームの窓の様子 `window::Seen`（外の角・内の大きさ・最大化・最小化・全画面・倍率・拡大）。
  新しい機能 `egui`（egui 0.36）で `Seen::of(ctx)` が egui から読む。機能を入れなければ egui には依存しない。

使う側がやること: ito-common に `features = ["egui"]` を足す。自前の追跡をやめ、`Tracker::new(state_dir, loaded)`（`loaded` は
`window::read` の値で、窓を作った場所）を持ち、毎フレーム `if let Some(left) = tracker.see(Seen::of(ctx)) { ctx.request_repaint_after(left) }`、
eframe の `on_exit` で `tracker.close(Seen::of(ctx))` を呼ぶ。`--keys` の台本で動かしている間は `Tracker` を作らない。

## [0.40.0] - 2026-10-16

### 追加

- ito-keyscript: `Runner` に `left()`（まだ押していない語の数）、`running()`、`finish()`、`watch(ctx)` を足した。
  `watch` は `<APP>_KEYS_DONE` があるときだけスレッドを 1 つ起こし、1 秒ごとに描画を起こす（裏にある窓でも台本が進む）。
  待ちの分を除いて `STALL`（30 秒）何も押せなければ `stalled_report` を 1 回だけ書き、stderr にも出す（tsumugi #39）。
  `<Quit>` は自分で台本を終わらせるので、その後に止まりの報告が上書きすることは無い。
  使う側がやること: 最初の一歩の前に `runner.watch(ctx.clone())` を呼び、`runner.clipboard` にクリップボードの文字を返す関数を入れる
  （既定は空の文字で、`<Paste>`・`<C-v>` は何も貼らない）。`Do::Quit` では `quit_report(runner.left())` を、台本の途中で窓が外から
  閉じられたとき（`running()` が真）は `runner.finish()` のあと `closed_report(runner.left())` を状態の後ろに書く。
  `--keys` の始めには `install_panic_report(done)` を呼び、拒むときは `refused_report(why)` を書く。
- docs/common-spec.md: 「キーの台本」に、`<APP>_KEYS_DONE` の終わりの `keys:` の行（done・quit・closed・stalled・panicked・refused）と、
  `Runner` を使うアプリがやることを書いた。

## [0.39.0] - 2026-10-16

### 変更

- ito-common: `window::Window` に `scale`（場所を取ったときの点から画素への倍率。egui の `pixels_per_point`）と
  `zoom`（そのときのアプリの拡大。egui の `zoom_factor`）を足した。古い `window.toml` は `scale = 0`・`zoom = 1` として読み、
  今までどおり主の画面の倍率で見る。`seen_on`（`reachable_now`）は場所を取った倍率で画素にして、今ある画面に出るかを見る。
- ito-common: 窓を作る値は `Window::opening()`（`Opening { pos, size, maximized }`）で取る。アプリの拡大を抜いた位置と内側の大きさを返す。
  Windows では、eframe の creator で隠れている窓を `Window::put(cc)` に渡すと、場所を取った画素に動かし、最大化ならそこで最大化する。
  ほかの OS では `put` は何もせず、`opening` の値のとおりに作る。
  使う側がやること: `Window { … }` を作るところに `scale: ctx.pixels_per_point()`・`zoom: ctx.zoom_factor()` を足す
  （最大化している間は前の場所を `..was` で保つ）。戻すときは
  `let o = w.opening(); viewport.with_inner_size(o.size).with_position(o.pos).with_maximized(o.maximized)` にし、
  eframe の creator の先頭で `w.put(cc)` を呼ぶ。ito-common は `raw-window-handle` 0.6（Pure Rust）に依存する。

### 修正

- ito-common: 倍率の違う 2 つ目の画面で閉じた窓が、次に開くと主の画面に戻る・ずれる（kura #401）。egui は窓を作るときに
  位置を作った画面の倍率で画素にし、作った後にもう一度、窓が移った画面の倍率で画素にし直すので、どの点の値でも両方は合わない。
  `put` が隠れている間に画素で動かすので、主の画面 150 %・2 つ目 100 % の組でも閉じた場所に開く。最大化も閉じた画面でする。
  上の「使う側がやること」をすると、アプリの拡大（`zoom_factor`）が 1 でないときに次に開くと窓が拡大の分だけ縮む
  （yagura）のも直る。

## [0.38.2] - 2026-10-16

### 変更

- `scripts/lanes/merge-lanes.py`: yagura の実機レーンの PR は、yagura の `Checklists`（`checklists.yml` の `checklists` のジョブ。
  yagura v0.65.1 から、TESTING.md の行の形を確かめる）が緑になるまでマージしない（`required_checks`）。印を 1 つも変えない PR は今までどおり待たない。
  分け前を main に push した後は、kura・tsumugi と同じく `ci.yml` と `checklists.yml` を `workflow_dispatch` で回す（ワークフローの token の push では何も走らない）。
  使う側がやること: yagura は `rev` を上げて `scripts/lanes.sh sync` し、`checklists.yml` を足し、`ci.yml` に `workflow_dispatch` を足す（v0.65.1）。
  kura・tsumugi は次に `rev` を上げたときに `scripts/lanes.sh sync` するだけ（規則は変わらない）。
- `merge-lanes.py` の冒頭の説明から「yagura にはレーンが無い」を消した（yagura のレーンは v0.21.2 から扱っている）。

## [0.38.1] - 2026-10-16

### 修正

- ito-notify: Discord に送る本文の長さを UTF-16 の数で数えて切る。絵文字のような 2 単位の字が多い本文でも 2000 を超えず、
  Discord に断られない（ソースのレビュー）。今までは字の数で数えていた。使う側は、次に `rev` を上げたときに入る。
- ito-notify: `an_http_error_is_a_failure` のテストは、curl が来ないとき（プロキシが間にあるなど）に止まらず 30 秒で落ちる。

## [0.38.0] - 2026-10-16

### 追加

- ito-notify: Webhook の形式に `discord`（Discord の incoming webhook）を足した。太字の題と本文を `content` に入れ
  （Discord が受ける 2000 字で切る）、`allowed_mentions` を空にするので、プログラムが `@everyone` と書いても誰も呼ばない。
  `ito_notify::FORMATS` は `["ntfy", "slack", "discord", "json"]` の 4 つになった。
- ito-prefs: 形式の名前 `format_name("discord")` は「Discord」。アドレスの説明（`webhook_url_note`）に Discord を足した。

### 変更

- ito-notify: Webhook を `curl -f` で送る。相手が 400 以上で断ったとき（アドレスの誤り・回数の制限）も失敗として stderr に出し、
  30 秒後に 1 回だけ送り直す。今までは網が切れたときだけを失敗としていた。
  使う側がやること: `rev` を上げる。`FORMATS` を写した自前の表（minato の `minato_core::prefs::FORMATS`、tsumugi の設定の画面の
  形式の並び）と、「ntfy, slack or json」と書いた文言・設定の例に `discord` を足す。`ito_notify::FORMATS` で確かめているアプリ
  （kura・yagura・tsumugi のサーバ）は、`rev` を上げるだけで `webhook_format = "discord"` を受ける。

## [0.37.9] - 2026-10-16

### 修正

- ito-keyscript: `--keys` の `<C-c>` と `<C-x>` が、キーボードの Ctrl+C・Ctrl+X と違う届き方をしていた（kura #383）。
  egui-winit はこの 2 つを `Event::Copy`・`Event::Cut` に変えてキーの押下を送らないが、スクリプトはキーの押下を送っていたので、
  アプリの Copy の受け口（選んだ文字を先にコピーする所）を通らず、keymap の `<C-c>` がすぐ走った。キーボードと同じく
  `Copy` / `Cut` と、Ctrl を押したままのキーの離し（`ito_keys::from_clipboard` が組み合わせを読む所）を送る。
  macOS ではクリップボードの組み合わせが Cmd なので、今までどおりキーの押下のまま。Shift や Alt が付いたものもキーの押下のまま
  （イベントは修飾キーを持たず、アプリはフレームで押している修飾キーから Shift を読むが、スクリプトは押していない）。
  使う側がやること: `keyscript::Press` を自分で回すアプリ（kura）は何もしなくてよい（`Press::Events` で届く）。
  Copy の受け口が選んだものが無いときに `<C-c>` に戻すことを確かめる（kura・tsumugi・ito-pane はそうなっている）。

## [0.37.8] - 2026-10-16

### 修正

- ito-common: `window::screens()` が、DPI を意識しないスレッド（winit より前のアプリ）で呼ばれたとき、スレッドの DPI の扱いを
  元に戻していなかった（戻り値の `DPI_AWARENESS_CONTEXT_UNAWARE`（-1）を失敗と取り違えていた）。winit がすぐプロセスを
  Per-Monitor V2 にするので、目に見える違いは無い。
- ito-common: `window::screens()` で、`GetMonitorInfoW` が答えない画面を黙って抜かしていたので、その画面の上で閉じた窓を
  既定の位置に開くことがあった。答えない画面があれば、壊れた答えとして空を返す（`reachable_now` は今までの見当で見る）。
  使う側がやることは無い（`rev` を上げるだけ）。

## [0.37.7] - 2026-10-16

### 追加

- ito-common: `window::screens()`（今ある画面の位置・大きさ（ピクセル）・倍率・主の画面か。Windows だけ。ほかの OS は空）と、
  `Window::seen_on(screens, grab)`・`Window::reachable_now(guess, grab)`。`reachable_now` は、今ある画面のどれかにタイトルバーが
  `grab` だけ出るときだけ通す。画面を聞けない OS では今までの `reachable(guess, grab)` と同じ。

### 修正

- 窓の位置を戻すとき、主の画面の 4 倍までの位置を通していたので、外した 2 つ目の画面の上で閉じた窓が、どの画面にも出ない所に開いていた
  （kura #357。2560×1440 の画面 1 つで (5000, 3000) や (2700, 100)）。使う側がやること: `rev` を上げ、
  `read(..).filter(|w| w.reachable(PRIMARY_GUESS, 100.0))` を `reachable_now(PRIMARY_GUESS, 100.0)` に変える（kura・yagura）。

## [0.37.6] - 2026-10-16

### 修正

- ito-pane（Windows）: Ctrl+C を無視するプロセス（タスク スケジューラの連なりや Claude Code の道具など）から起動したアプリでは、
  ペインで `^C` を送っても `ping` や `Start-Sleep` が止まらなかった（kura #354）。Ctrl+C を無視する印は、起動したプロセスに、
  そこから先に起動するものすべてに受け継がれ、ペインのシェルとその子もそれを持っていた。ペインを作るたびに、その印をアプリから外す
  （`SetConsoleCtrlHandler(NULL, FALSE)`）。自分のコンソールで動くアプリは、ほかのプログラムと同じく、そこの Ctrl+C で止まるようになる。
  使う側がやることは `rev` を上げるだけ（tsumugi のサーバのように自分で外していたら、それは消してよい）。

## [0.37.5] - 2026-10-16

### 修正

- `scripts/lanes/merge-lanes.py`: レーンの報告の Proposals・Queue・Votes に「無い」とだけ書いた行（`None.`、`(none)`、
  ``No open `vote` issues.``、`Nothing in the queue needs to change.` など）からも `finding` の issue を立てていた
  （tsumugi #65・#72・#75・#108・#121）。その行は飛ばす。「None of the toasts show.」のように中身のある行はそのまま立てる。
  使う側がやること: `rev` を上げて `scripts/lanes.sh sync`。

## [0.37.4] - 2026-10-16

### 修正

- ito-prefs: 設定の画面で、最後の止まり先（下の「config.toml を開く」などのボタン）から Tab を押すと、1 フレームだけどこにもキーが無い
  状態を挟んでから検索に戻っていた（tsumugi #61、#106）。egui は次に来たものにキーを渡すが、そのボタンが最後なので次のフレームまで
  渡す先が無かった。同じフレームで検索に渡す。Shift+Tab で検索からボタンに戻るのはそのまま。
  使う側がやることは `rev` を上げるだけ（tsumugi のように自分で検索に渡していたら、それは消してよい）。

## [0.37.3] - 2026-10-16

### 修正

- ito-browser（Windows）: ページがキーを持っているときに Alt（または F10）だけを押すと、窓がメニューのモードに入り、
  次のクリックがそのモードを終えるだけで egui に届かず、キーがページに残っていた（tsumugi #109）。WebView2 はページが使わないキーを窓に渡し、
  システムはメニューの無い窓にもメニューのモードを始めるため。ブラウザがあるあいだ、窓をサブクラスにして、Alt や F10 だけのメニュー
  （`SC_KEYMENU` で字の無いもの）を窓に届けない。Alt+Space（窓のメニュー）と Alt+字はそのまま。
  また、窓の中（ページの外）を押すと、egui が押されたことを見る前に、キーを窓に戻す。ブラウザを落とすと（窓が閉じても）サブクラスは外れる。
  使う側がやることは `rev` を上げるだけ。ブラウザがあるあいだは、Alt や F10 だけでは窓のメニューに入らない。

## [0.37.2] - 2026-10-16

### 修正

- ito-notify: macOS の clippy が `notify_send_actions` と `arg_of`（`notify-send` のためのもの。macOS は `osascript` で出す）を
  使われていないと言っていた。Windows と同じく macOS でも使わないものとして扱う。使う側がやることは無い（依存の警告は使う側では出ない）。
- ito-pane: テストの `listening_ports` の `use` が、そのテストを回さない macOS で使われていないと言われていた。そのテストの中で呼ぶ。
- `scripts/verify.sh`: Linux・Windows・macOS の clippy を、どの機械から回しても回す。macOS は objc2 が Objective-C をコンパイルするので、
  Mac でないときは cargo-zigbuild（無ければ PyPI から入れる）を通す。zig のキャッシュは Cargo の出力の隣に置く。
  これまで macOS は CI 任せで、ここの CI には Mac が無いため、上の警告に気づかなかった。

## [0.37.1] - 2026-10-16

### 追加

- ito-pane: キーで作る選択（コピー モード）のための `Pane::select_cells(cell, start)` と `Terminal::select_cells`・`grid::select_cells`。
  どちら向きに伸ばしても、始めたマスと今のマスの両方を含める（vim や Windows Terminal のマーク モードと同じ。alacritty の `include_all`）。
  前はマウスと同じ `select` を使っていたので、始めたマスより前へ伸ばすと、始めたマスが落ちていた（tsumugi #57）。
  始めただけでも、そのマスが 1 つ選ばれる。`Pane` を実装する側は何もしなくても通る（既定は右へのドラッグと同じ `select`）。

## [0.37.0] - 2026-10-15

### 変更

- ito-select: 選択の仕様を tsumugi の端末に合わせた。
  - 選択の色を ito-select で決める: `ito_select::fill(ground, accent, light)`（tsumugi の色。地の色に強調の色を暗いテーマで 25%、明るいテーマで 18% 混ぜる。不透明）。
    ito-theme の `Colors::selection()` はこれを呼ぶようになった（値は前と同じ）ので、ito-theme の色を使うアプリ（tsumugi の端末・`visuals()` の選択）は何もしなくてよい。
  - 選択を文字の後ろに塗る: `Select::note` と `note_inline` が描く `Painter` を受け取り、その時点に場所を取る。`paint` はそこへ塗るので、
    文字の下に塗った帯（コードブロックの地など）の上・文字の下に選択が来る。
  - ドラッグを離したとき・ダブルクリック・トリプルクリックで、選んだ文字を `Driven::copy` で返す（tsumugi の copy on select。アプリがクリップボードに入れる）。
  - ダブルクリックがウェブアドレスの上なら、アドレス全体（前の括弧と後ろの句読点を除く）を選ぶ。アドレスの前の括弧の上では、`url_at` はもうアドレスを返さない。
  - 使う側がやること:
    - `s.note(pos, &galley)` → `s.note(&painter, pos, &galley)`（`note_inline` も）。その galley を描く直前に呼ぶ。
    - `s.drive(&resp, view)` は `Option<String>` ではなく `Driven { copy, open }` を返す。`open` が前の戻り値（Ctrl+クリックしたアドレス）。
      `copy` が `Some` ならクリップボードに入れる（設定で copy on select を切れるアプリはそれに従う）。
    - `s.paint(&painter, fill)` → `s.paint(fill)`。色は半透明にせず `ito_select::fill(…)` か `ito_theme::colors().selection()` を渡す。
  - ito-select の egui は `default-features = false` にした（ほかの ito のクレートと同じ）。
- ito-pane: `Palette::selection` の説明を直した（アプリは ito-select の色を渡す）。

## [0.36.1] - 2026-10-15

### 追加

- ito-pane: トリプルクリックで端末の 1 行（折り返した行ごと）を選ぶ。`Pane::select_line`（既定は `select_word`）と `Terminal::select_line`。
  `view` が `triple_clicked` で呼び、離したときのコピー（`copy_on_select`）にも乗る。`Pane` を実装する側は何もしなくても通る
  （行選択を持つなら `select_line` を実装する）。

## [0.36.0] - 2026-10-15

### 追加

- ito-graph: コミットの樹形図を行ごとに並べるクレートを足した（michi が使う）。Git は知らず、行の id・見せる親・色のための名前を受け取る。
  - `Layout::push` が行を上から 1 つずつ並べる。幹（main の第 1 親の列）はいつもレーン 0、ほかは空いた一番左のレーン。
    まだ来ていない親への辺は下の端まで伸び、`finish` のあとは短い切れ端になる（浅い clone）。
  - `Layout::edges(rows)` が見えている行を通る辺だけを返す（長い辺は別に持つので、10 万行でも全部は見ない）。`width_in` は見えている行の幅。
  - `Colors` がブランチに端末の 16 色の番号を配る。main を `pin`、worktree のブランチを `reserve` で先に違う色にし、
    ほかは名前のハッシュ（同じブランチはいつも同じ色）。隣のレーンと同じなら最初の 1 回だけずらす。赤と灰色は使わない（`ALLOWED`）。
  - `Layout::path` が辺の形（直線と 3 次ベジエ、3 つまで）を返し、機能 `egui` の `ui::paint` が辺と点（コミット・マージ・未コミット・衝突）を描く。

## [0.35.2] - 2026-10-15

### 追加

- ito-select: ダブルクリックで単語、トリプルクリックで行（`note_inline` でつないだ文字列も含む）を選ぶ（`word_at`・`line_at`）。
  Ctrl（Mac は Cmd）+ クリックで、その下の URL を `drive` が返す（`url_at`。`http://`・`https://`・`ftp://`・`www.`。文末の句読点や閉じ括弧は含めない）。
  呼ぶ側は返った URL を開く。

### 変更

- `Select::drive` が `Option<String>`（開く URL）を返すようになった。戻り値を使わない呼び方はそのまま通る。
- `Select::paint` の `fill` は半透明にする（塗りは文字の上に重なる）。文書に明記した。

## [0.35.1] - 2026-10-15

### 追加

- ito-select: `Select::note_inline`。1 行を何度かに分けて描くもの（Markdown の整形表示の span）を、前の文字列と同じ行として覚える。コピーでは行を
  区切らずにつなぎ、離れていれば空白 1 つを挟む。範囲選択の塗りも、同じ行の続きがある文字列の右端は伸ばさない。既存の API は変えていない。

## [0.35.0] - 2026-10-15

### 追加

- ito-select: 新しいクレート。`Painter` で描いた文字（プレビューや詳細欄）をマウスのドラッグで範囲選択してコピーする。egui は `Label` の文字は選べるが、
  `painter.galley` で描いた文字は選べない。描いた順に `Select::note` で文字を覚え、`Select::drive` がドラッグを受けて 2 つの位置（何番目の文字列の何文字目）に直し、
  `Select::paint` が選択を塗り、`Select::copy` が egui の `Copy`（Ctrl+C・Cmd+C）に答えてクリップボードへ渡す。選んだ文字は文字列ごとに 1 行。
  使う側は kura（プレビュー）と minato（詳細欄）。既存のクレートは変えていないので、使う側がすることは無い。

## [0.34.3] - 2026-10-15

### 修正

- ito-palette: コマンドパレットで日本語が入力できなかった。欄にキーを渡す `request_focus` を毎フレーム呼んでいて、変換中の入力（IME）が
  そのたびに打ち切られていた。キーを持っていないときだけ頼む。

## [0.34.2] - 2026-10-15

### 修正

- ito-prefs: キーのページで「キーで探す」を押すと、受け取りの文（日本語では「キーを押して…（Enter で決める · Esc でやめる）」）が
  検索の欄の横に収まらず、ページが右にはみ出して横のスクロールが出ていた。収まらないときは次の行に折り返す。

## [0.34.1] - 2026-10-15

### 修正

- ito-common: `set_tables` で、ファイルの先頭の `[[…]]` の前に新しい表を入れると、2 つの間に空行が入らなかった。
  キーのページで足したキーが、手で書いたものと違う見た目になっていた。新しい表の後ろに空行を入れ、戻すと元の文に戻る。

## [0.34.0] - 2026-10-15

### 追加

- ito-prefs: `KeyTable` に `unbound` と `sup` を足した。
  - `unbound`: 既定でキーの無い操作 `(層, run, desc)` の並び。その層の行の後ろに、キーの無い行として出し、＋でキーを足せる。
  - `sup`: Mac の Cmd を Ctrl（`C`）と分けて `D` と書く。Cmd と Ctrl を別のキーとして扱うアプリ（tsumugi）のため。
  - 使う側がやること: `KeyTable` を作るところに `unbound: &[]` と `sup: false` を足す（今までと同じ動き）。
  - `sup` のとき、Mac では Ctrl を `⌃`、Cmd を `⌘` と分けて見せる。Cmd+C・X・V（egui ではクリップボードの出来事になる）も `D` で受け取り、「特別なキー」の Cmd+C・X・V も `D` にする。みんなのキー（`Cmd+,` など）とのぶつかりも Cmd で見る。
- ito-keys: `Key::human_for`・`human_seq_for`・`from_clipboard_for` を足した。Ctrl と Cmd を分けるアプリのための形。

### 修正

- ito-keys: `from_egui` が Shift+Space（Ctrl+Shift+Space など）の Shift を落としていた。`<C-S-Space>` が `<C-Space>` になっていた。

## [0.33.3] - 2026-10-15

### 変更

- ito-prefs: キーのページで入れかけのキーが多くのキーの頭になるとき（`g` だけなど）、頭が同じキーの文は 3 つまでにし、
  残りは「ほか N 件も同じです」と数だけ出す。前は十を超える文が並んだ。`KeyWords` に `more` を足した。

## [0.33.2] - 2026-10-15

### 追加

- ito-keys: `from_clipboard(events, at, now)`。egui が `Copy`・`Cut`・`Paste` に変えてしまうクリップボードの和音
  （Ctrl+C・X・V、Windows の Ctrl+Insert・Shift+Delete・Shift+Insert）を、もとのキーに戻す。修飾キーは、同じフレームに
  そのキーを離した出来事があればそこから取る（すばやく押して離すと、フレームの終わりの `modifiers` からは消えている）。
  使う側がやること: 和音を `<C-v>` などと決め打ちしているところで、Shift や Alt を付けたキーにも割り当てたいときは、これを通す。

### 修正

- ito-prefs: キーのページで Ctrl+Shift+V を押すと、Shift+Insert として受け取ることがあった（実機）。`from_clipboard` で受け取る。

## [0.33.1] - 2026-10-15

### 修正

- ito-keymap: コメントだけの keymap.toml に `edit` で書くと、足した表がコメントの上に入っていた（toml_edit がコメントを
  文書の末尾として持つため）。コメントを頭に残し、その下に書く。全部を既定に戻したときも、残ったコメントがファイルの頭から始まる。

## [0.33.0] - 2026-10-15

### 追加

- ito-prefs: 設定の「キー」のページ（`keys_page`）。1 行に 1 つの操作、キーをチップで並べ、＋で足し、× で外し、チップを押して
  変え、↺ で既定に戻す。変えた行には「既定: …」、自分で変えたキーに •、届かないキーに ⚠、外した既定のキーは取り消し線（押すと戻る）。
  文字の検索・「⌨ キーで探す」・場面の絞り込み・「変えたものだけ」。キーは押して入れ、続けて押せば並びになり、修飾キーの無い
  Enter で決める（Esc でやめる、Backspace で 1 つ戻す）。修飾キーの無い Enter・Esc・Backspace と、クリップボードの Ctrl+C・X・V
  （貼るものが無いと egui に届かない）は「特別なキー ▾」から。押したままのキーは 1 回、IME の字は受け取らない。
  決める前にぶつかりを文で出す。語は `KeyWords`（`KEYS_EN`・`KEYS_JA`）、検索の言葉は `keys_index`。
- ito-keymap: 設定の画面のための `rows`（操作ごとの行、キーごとの出どころと届くかどうか、外した既定のキー）、`clashes`、
  `reach`、共通のキーの表 `COMMON` と `common`、keymap.toml を書き換える `edit`（`Edit::Add`・`Remove`・`Reset`）。
  書くのは既定と違う分だけで、`prepend_keymap` の頭に `{ on, run, desc }`、外した既定のキーは `{ on, run = "noop" }`。
  既定に戻れば行を消す。コメントは残る。↺ はその操作の既定のキーを取り戻す（ほかの操作がそこに書いた行も消す）。
  `for`・`on_mac` の付いた行は書いたまま残し、リストの `run` はリストのまま書く。
- 共通の仕様の「キー」に「設定の画面から変えるとき」、標準のページにキー。

### 使う側がやること

- キーを変えられるアプリは、設定にページ「キー」（アプリの設定と通知の間）を足し、`KeyTable`（今の keymap と既定だけの keymap の層、
  何もしないコマンド、自分の keymap.toml のパス、場面と説明の訳）と `KeysEdit` で `keys_page` を描く。`KeysEdit::busy()` の間は
  `show` に `Keys::default()` を渡す。返った `Edit` を UI ではないスレッドで `ito_keymap::edit` に当てて書き、keymap を読み直す。

## [0.32.2] - 2026-10-14

### 修正

- ito-common: `set_tables` が新しい表をファイルの最初の表の前に置くとき、ファイルの頭のコメント（空の行までのもの）が新しい表の
  下に回っていた。頭のコメントは頭に残し、新しい表はその下に置く。

## [0.32.1] - 2026-10-14

### 修正

- ito-common: `set_tables` が、名前の無い表（keymap.toml のキー）の前に 1 つ足したり 1 つ消したりしたとき、コメントを隣の表に
  付け替えていた。同じ中身の表は前の表（コメントと場所）をそのまま使う。新しい表は次に残る表のすぐ前に置き、ファイルを読み直した
  並びが渡した並びと同じになる（前は最後の表の場所に置いていた）。

## [0.32.0] - 2026-10-14

### 追加

- ito-keymap: yazi の形の keymap.toml を読むクレート。kura の `config/keymap.rs` を移したもの。層（`[mgr]`・`[main]` など、
  別名も持てる）ごとの `keymap`（置き換え）・`prepend_keymap`・`append_keymap` を組み込みの既定に重ね、押したキーの並びから
  `resolve` で 1 つ（`Exact`）か続き待ち（`Pending`）か無し（`None`）を返す。`on` は `"gg"` とも `"g g"` とも書け、`for`
  （windows・macos・linux・unix）と `on_mac` で機械ごとに分けられる。知らない層・欄、読めないキー、押せない `<S-t>`、
  二重のキー、続きを隠すキー、既定の大半を消す短い `keymap` を警告にし、利用者が既定のキーを置き換えたものは `overrides` に
  分ける。TOML の読み違いには、Windows のパスを単引用符で書く手がかりを足す（`parse_error`）。
- 共通の仕様の「キー」に ito-keymap の段落。対応の表の行を「keymap.toml（yazi の形、`ito-keymap`）」にした。

### 使う側がやること

- keymap.toml を読むアプリは `ito-keymap` に依り、層と既定を `Spec` で書き、`run` の 1 つを自分の動きに読む関数を
  `load` に渡す。kura（0.110.3）と yagura（0.60.0）は移した。`Match::Prefix` は `Match::Pending(_)` になる。

## [0.31.0] - 2026-10-14

### 追加

- ito-notify: アイコン付きのシステムの通知（`toast::show_with_icon`）。アプリの `icon` のモジュールの関数（`toast::Icon`、
  一辺の画素から RGBA）を渡すと、128 画素の PNG を利用者のキャッシュ（Windows は `%LOCALAPPDATA%\uchmk\notify\<id>.png`）に
  起動ごとに 1 回書き、Windows では通知の左と差出人の名前の横に、`notify-send` では通知のアイコンに出す。macOS は今までどおり。
  使う側は `toast::show` を `toast::show_with_icon(app, icon::pixels, …)` に変える。PNG のために `png` に依るようになった。
- 共通の仕様の「通知」に、通知にアイコンを出すことと対応の行。

## [0.30.0] - 2026-10-14

### 追加

- ito-prefs: 設定の画面の「通知」のページ（`notify_page`）。minato の通知のページを共通にしたもの。上に「試しに送る」と
  「既定に戻す」（Webhook のアドレスと形式は残す）、「知らせるとき」（出来事が 1 つならスイッチの行、2 つ以上なら列が出来事の表。
  Webhook の行は出来事ごとに送り分けるときだけ）、「鳴らす音」（出来事ごとの音と ▶、集中モード）、「Webhook」（アドレスと形式）。
  出来事は `End`、設定は `Notify`（アプリの設定ファイルとの読み書きはアプリ）、言葉は `Words` の `notify*`・`sound*`・`focus*`・
  `webhook*`、検索の言葉は `notify_index`。ito-prefs が ito-notify に依るようになった。
- 共通の仕様（`docs/common-spec.md`）に「通知」の節。通知のページを設定の画面の既定のページにする。

### 使う側がやること

- 通知のページを持たないアプリ（kura・yagura）は `notify_page` でページを足し、`[notify]` を読み書きする（鍵は仕様の「通知」）。
- `Words` に欄が増えた。`Words { .. }` を自分で組み立てているなら欄を足す（`EN`・`JA`・`Words::of` を使うなら何もしない）。

## [0.29.1] - 2026-10-14

### 追加

- 紹介画像（GitHub の Social preview）: `docs/social-preview.png` と、元の `docs/banner.html`。左に糸の字、
  右に 4 つのアプリ（tsumugi・kura・yagura・minato）の実画面（`docs/images/`、各アプリの banner-shot の写し）を
  それぞれの色の糸でつないで並べる。リポジトリの Settings > General > Social preview に手で上げる。

### 変更

- README の「The apps」に minato（港）を足した。

## [0.29.0] - 2026-10-14

### 追加

- `ito-pane`: `Screen::cursor_hidden`（プログラムが `\e[?25l` でカーソルを隠した）と `cursor_hidden(term)`。

### 修正

- `ito-pane`: Claude Code がバックグラウンドのエージェントを動かしている間、IME の変換中の文字と候補の窓が、フッターの時計の再描画のあとにカーソルが残る行末に出ていた。カーソルが隠れている間は、最後に見えていた場所に出す。
  - 使う側: `Screen` を作る所（直接作るアプリ）は `cursor_hidden` を足す。tsumugi は mux の `Update` にも運ぶ。

## [0.28.2] - 2026-10-13

### 修正

- `ito-palette`: 長い題が行からはみ出して、右のキーの名前とパレットの外に描かれていた。入らない題は行の中で `…` で切る。
- `ito-browser`: 開いたときにページがキーを取り、`j`・`k` などのキーがアプリに来なかった。開いてもキーは窓のまま（ページをクリックするとページに行く）。
- `ito-browser`: ページが履歴の API で進んだとき（新しいページを読まずに GitHub がたどるリンク）、アドレスの欄が前のアドレスのままだった。ページのアドレスに合わせる。

使う側の変更は無い。

## [0.28.1] - 2026-10-13

### 修正

- `ito-browser`: ページをクリックしたあと、窓のほかの所（アドレスの欄・アプリの画面）をクリックしてもキーがページに行ったままだった。ページの外のクリックでキーを窓に戻す。使う側の変更は無い。

## [0.28.0] - 2026-10-13

### 追加

- `ito-browser`: `Browser::with_data_dir(Option<PathBuf>)`。WebView のクッキーとキャッシュをアプリの状態のフォルダに置き、サインインが次の起動にも残る（渡さないと WebView2 は `.exe` の横に置く。macOS は渡しても OS がアプリごとに持つ）。
- `ito-browser`: アドレスの欄の左に ←（前のページへ）。

### 変更

- `ito-browser`: ページの中でリンクをたどると、アドレスの欄がそのページの URL になる（欄に打っている間は書き換えない）。`↗` と ⟳ もそのページに効く。
- `ito-browser`: 新しい窓で開くリンク（`target="_blank"`・`window.open`）は、別の窓を作らず同じページで開く。
  - 使う側: `Words` に `back`（← のヒント）を足して渡す。状態のフォルダに残したいアプリは `Browser::new(url).with_data_dir(Some(dir))` で作る。

## [0.27.0] - 2026-10-13

### 追加

- `ito-browser`: ペインの横に出すウェブのページ（tsumugi の内蔵ブラウザを移した）。Windows と macOS では OS の WebView（WebView2・WKWebView）を窓の子にし、上に egui のアドレスの欄と ⟳・↗・✕ を描く。Linux には無いので `EMBEDDED` が `false` になり、アプリはシステムのブラウザで開く（同じ API の `Browser` もあり、最初の `show` で `Answer { close: true, external: Some(url) }` を返す）。打った行の読み方（`normalize`: `3000` → `http://localhost:3000`、`file:`・`javascript:` などは断る）と、開くページ（`start_url`）も入れた。
  - 使う側: `Option<Browser>` を持ち、パネルの中で毎フレーム `show(ui, frame, hidden, &Words { reload, external, close, failed })` を呼ぶ（`frame` は `eframe::Frame`、`hidden` は egui の重ね表示が開いているとき。WebView はいつも egui の絵より上に出るため）。パネルを描かないフレームは `hide()`。`Answer::external` はシステムのブラウザで開き、`Answer::close` で `None` にする。ボタンの言葉は `Words` でアプリの言語のものを渡す。

## [0.26.0] - 2026-10-12

### 追加

- `ito_common::fonts::UI_REGULAR`・`UI_BOLD`: 窓の文字（タイトル・ボタン）の比例フォント（IBM Plex Sans JP など）の探すファイル名の並び。tsumugi が持っていたものを移した。

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
