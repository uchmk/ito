# 変更履歴

このファイルの形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版のつけ方は
[セマンティックバージョニング](https://semver.org/lang/ja/) に従う。版はルートの `Cargo.toml` の `[workspace.package]` の `version` が正。

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
