# 変更履歴

このファイルの形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版のつけ方は
[セマンティックバージョニング](https://semver.org/lang/ja/) に従う。版はルートの `Cargo.toml` の `[workspace.package]` の `version` が正。

## [0.289.0] - 2026-11-10

### 追加

- Vision の答えの形 `docs/schemas/vision.v1.json` と例 2 つ（`docs/schemas/examples/vision.json`・`vision-inconclusive.json`）。`scripts/check-schemas.py` が検査する。形を決めただけで、印・門番・verdict は変えない（`evidence.py` の `vision_points` は `not implemented` のまま。Tier 2 は未承認）。

### 使う側がやること

- いま直すところは無い。Vision の答えをファイルに残すようになったら（kura の影運用）、保存の前に `model`・`prompt_sha256`・`inputs[].sha256` を埋める（`usage` は分からなければ null）。

## [0.288.0] - 2026-11-10

### 追加

- `ito-secret`: Linux の `get` が、OS の保管庫より先に systemd の `LoadCredential=` のファイル `$CREDENTIALS_DIRECTORY/<app>_<name>` を読む。読むだけで、`set`・`delete` は触らない。末尾の改行は 1 つだけ除く。`app` に `_` があるとき、`app`・`name` に `/`・`\`・NUL があるとき、フォルダーが絶対パスでないときは見ない。ファイルがあるのに読めないときは `secret-tool` へ進まず、同じ `kind()` の `Err` を返す（ファイル名は出すが中身は出さない）。
- `ito-http`: `Client::max_reply`（受ける本文の上限。既定は今までと同じ 4 MiB）。`Content-Length` が上限を超えていれば `the body is too large (over N bytes)` と言う。
- `ito-http`: `Client::deadline: Option<Duration>`（既定は `None` で今までと同じ）。名前解決・接続・TLS の握手・送信・受信を合わせた 1 回の呼び出し全体の期限で、切れたら新しい `Error::Deadline` を返す。少しずつ送り続けるサーバーでも期限を越えない。`Error::Deadline` で返るときソケットは閉じる。止まった名前解決のスレッドは OS の時間切れまで残る（ソケットは持たず、結果は捨てる）。サーバーがその要求を実行したかどうかは分からない。
- `ito-service`: Linux の `state` が `systemctl show` で `<name>.service` の状態を返す。`pid` は `MainPID`。systemd で起動していない機械とユニットが無いときは `Ok(None)`、調べられないときと知らない状態は `Err(Failed)`、名前に `*`・`?`・`[` があれば `BadName`。`reloading`・`refreshing` は `Running`、`maintenance` は `Stopped`。ほかの関数と macOS は今までどおり `Unsupported`。

### 使う側がやること

- `ito_http::client::Error` を網羅して `match` しているところに `Error::Deadline` の腕を足す。
- `deadline` と `max_reply` は要るところでだけ設定する（どちらも `Client` の公開の欄）。

## [0.287.1] - 2026-11-09

### 修正

- `scripts/lanes/keys.ps1`: `-Dir` がバックスラッシュで書いたドライブのルート（`C:\`）のとき、閉じる引用符がエスケープされて引数がつながり、`--keys` がアプリに届かないまま `-Timeout` まで待っていた。Windows のコマンドラインの規則どおりに、引用符の前と末尾のバックスラッシュを倍にして渡す。`-Keys` の `\"` と末尾の `\` も同じ規則で渡すようにした。

### 使う側がやること

- `rev` を上げて `scripts/lanes.sh sync` を流す。

## [0.287.0] - 2026-11-09

### 追加

- `ito-keyscript`: Shot の付属 JSON と manifest の `env.font_set_sha256`・`env.wgpu_backend` を埋める。`font_set_sha256` は窓に登録したフォントの全部（名前・中身・`index`・`tweak`・ファミリーの順序）のハッシュで、アプリのコードは要らない（フォントが変わったときだけ計算し直す）。`wgpu_backend` はアプリが起動時に新しい `set_wgpu_backend(name)` を呼ぶと入り（`gl`・`dx12`・`vulkan`・`metal`）、呼ばなければ今までどおり `null`。`Runner` は絵を頼んだ時点の `Env` を残すので、保存までの間にフォントが変わっても、絵と違う値は付かない。最初のパスの途中で頼んだ `<Now><Shot:>` だけは `null` のまま。
- `templates/app`: 雛形の `main.rs` が `set_wgpu_backend` を呼ぶ。

### 変更

- `lanes.conf` の `scale_env=` の定義をはっきりさせた（#213）。値は OS の倍率を含めた絶対の `pixels_per_point` で、アプリはこれを `native_pixels_per_point` で割ってズームにする。0.286.0 のここの「kura は `KURA_SCALE`」は誤りだった。`KURA_SCALE` は OS の倍率に掛かるズームなので、書くと 100% 以外の画面で倍率が合わず `ERROR` になる（誤って PASS にはならない）。`docs/visual-test.md` と雛形の `lanes.conf` も同じ定義にした。

### 使う側がやること

- `rev` を上げるだけで `font_set_sha256` は入る。ただし自前の実行が `observe` を呼ばないアプリでは `null` のまま。
- `cc.wgpu_render_state` を読むところで、`ito_keyscript::set_wgpu_backend(rs.adapter.get_info().backend.to_str())` を 1 行足す。足さなくても壊れない（`null` のまま）。
- 自前の実行を持つアプリ（kura）は、`review::Env::of` を絵を頼んだ時点で呼ぶようにする。保存のときに呼ぶと、頼んだあとにフォントが変わった回で、絵と違う値が付く。シグネチャは変えていないので、直さなくてもビルドは通る。
- `scale_env=` に掛け算のズームの変数を書いているアプリは外す。絶対の `pixels_per_point` を受ける変数を足すまで、書かないままでよい（倍率が違う回は `ERROR` になる）。

## [0.286.0] - 2026-11-09

### 追加

- `ito-pane`: プロンプトの印のバーにテストの ID `pane.mark.N` を付けた（#210）。`N` はその瞬間の画面の上からの位置（0 から）で、スクロールすると同じ印の番号が変わる。描かれない印は番号を取らず、全画面のプログラムの間（代替画面）は 1 つも登録しない。バーはクリックもドラッグも取らないので、バーの上から始めるドラッグは今までどおり選択になる。
- `ito-pane`: `Palette` に `mark_ok`・`mark_failed` を足した（#210）。印のバーの色をテーマのトークンから渡せる。`ito-theme` の `Colors::palette` は `success` と `err` を渡す。
- `scripts/lanes/make-canary.py`: カナリアを作る道具（#212）。実際の回の folder を `<run>/canary/<種類>/<run id>/` に写して測る値を壊し、`visual-check.py` が FAIL と言うことを確かめる（PASS も ERROR も不合格）。種類は spec の横の `<行>.canary.toml` のデータで、`drop`・`shift`・`set_text`・`copy_from`・`set`・`paint` を書く。製品のコードと元の回には触らない。種類を作る前に元の回を使い捨ての写しで検査し、PASS でなければ何も作らずに終了コード 2 で止まる（もともと FAIL の回では、どのカナリアも良く見えてしまうため）。手順は `docs/visual-test.md` の「カナリア」。
- `lanes.conf` の `scale_env=`（#213）。アプリの倍率の環境変数の名前を書くと、`keys.ps1 -Evidence`・`xrun.sh --evidence` が spec の `window.pixels_per_point` をその変数でアプリに渡す。manifest の新しい欄 `scale_override`（`{ env, value, source }` か `null`）に残るので、上書きした回を実機の倍率の回と取り違えない。`--spec` 同士で倍率が違う・変数名が `keys_env` などと重なるときは `evidence.py new` が folder を作る前に断る。古い manifest も `evidence.py check` で読める。

### 変更

- `ito-pane`: 印のバーを不透明で塗る（#210）。これまでは不透明度を 0.8 倍にしていて、画素の色をトークンと比べられなかった。バーは少し濃く見える。幅は 2pt のまま。

### 使う側がやること

- `Palette { … }` を全部の欄で書いているなら `mark_ok`・`mark_failed` を足す（`..Default::default()` なら要らない）。kura は `palette(theme)` で `success` と `err` を渡す。
- `rev` を上げて `scripts/lanes.sh sync` を流す（`FILES` に `make-canary.py` が増え、`evidence.py`・`keys.ps1`・`xrun.sh` の写しが変わる）。
- 倍率の環境変数があるアプリは `lanes.conf` に `scale_env=<変数名>` を足す（kura は `KURA_SCALE`）。無いアプリはそのままでよい。

## [0.285.0] - 2026-11-09

### 追加

- `ito-titlebar`: 窓のタイトルバーの部品にテストの ID を付けた（#209）。`titlebar.search`（パレットの箱）・`titlebar.theme`（テーマの札）・`titlebar.bell`（ベル）・`titlebar.caption.minimize`・`titlebar.caption.maximize`・`titlebar.caption.close`（窓のボタン）。最大化のボタンは、最大化している間も同じ名前。ドラッグの場所には名前を付けない。
- `ito-prefs`: `select` にテストの ID を付けた（#211）。閉じた箱が `settings.select.{id}`、開いた一覧の項目が `settings.select.{id}.{n}`（`options` の順に 0 から）。`is_test_id` が受け付けない `id` には名前を付けない。`settings.shell.program` のように自分で名前を付けるシェルのボタンは、その名前のまま。

### 使う側がやること

- `rev` を上げるだけ。コードの変更は要らない。`--keys` の台本では `<ClickId:titlebar.search>`・`<Id:settings.select.{id}>` のように書ける。一覧の項目の名前は、開いている間だけある。

## [0.284.1] - 2026-11-09

### 修正

- `scripts/lanes/evidence.py`: 自己検査が、自分で作ったシンボリックリンクを `os.rmdir` で消そうとして、Linux で落ちていた（`[Errno 20] Not a directory`）。`os.unlink` で消す。手元の Windows の `verify.sh` では通るので、CI でだけ赤かった。
- ワークフロー: ito の Build と雛形（`scripts/lanes/workflows/` の `build.yml`・`release.yml`）の macOS のランナーを `macos-14` から `macos-15` にした。GitHub が `macos-14-arm64-us-default` を受け付けなくなり、macOS の 2 つのジョブが取り消されていた。

### 使う側がやること

- `rev` を上げて `scripts/lanes.sh sync` を流す（`evidence.py` の写しと、`workflows=` に並べた雛形が変わる）。

## [0.284.0] - 2026-11-08

### 変更

- `ito-mdview`: 先頭の front matter（YAML の `---`・TOML の `+++`）を、コードの帯と色分けではなく、帯なしの薄い字（`Palette::dim`）で元の行のまま描く（#195）。行の種類は新しい `LineKind::Properties`。本文より目立たず、クリックした位置は元の文章の正確なバイトに戻る。長い行は窓の幅で切り（句読点や閉じ括弧を段の頭に置かない）、検索では 1 行のまま（切れ目に空白を入れない）。

### 修正

- `ito-mdview`: CRLF の文章で、コードブロックの行の `bytes` が 1 行ごとに 1 バイトずつ前へずれていた。また、どの種類の行でも `bytes` の終わりに行末の `\r` が入っていた。
- `ito-mdview`: 窓の幅で切ったコードの行で、どの段も `bytes` が元の行の先頭から始まり、前の段が空の範囲になっていた。段ごとに見せているバイトを持つようにした。

### 使う側がやること

- `LineKind` を網羅して `match` しているアプリは `Properties` を足す。`render` と `show` だけを使うアプリ（weft）は何もしなくてよい。

## [0.283.0] - 2026-11-08

### 追加

- `ito-proc`: `git_program()` を追加（#208）。Windows で git の実体（`git --exec-path` の下の `git.exe`）を返す。`Git\cmd\git.exe`（ラッパー）を通すと、起動のフラグが何でも `conhost.exe` が 1 つできる。実体を `DETACHED_PROCESS` で起動すれば、できない。`--exec-path` を動かすのはプロセスで最初の 1 回だけで、止まるのでワーカーから呼ぶ。Windows 以外と、実体が見つからないときは `"git"`。

### 使う側がやること

- git を起動するアプリは、`Command::new("git")` を `Command::new(ito_proc::git_program())` にできる。conhost を 0 にするには `DETACHED_PROCESS` で起動する（`CREATE_NO_WINDOW` のままだと、1 回ごとに 1 つできる）。ラッパーを飛ばしても、フック・alias・Git LFS・資格情報マネージャーは同じように動く（Git for Windows 2.52.0 で実測）。読み取りの呼び出しに限らない。違うのは、子の `PATH` で `%HOME%\bin` が `mingw64\bin`・`usr\bin` より前に来ることだけ。

## [0.282.0] - 2026-11-08

### 追加

- `ito-keyscript`: `state::sha256_hex`・`evidence::write_durable` を公開。`review::Env::of(&Meta)` を追加（`Runner` を使わず `pace::Pace` で動かすアプリが、`<Snap:>` の付属 JSON を同じ形で書けるように。`Runner` もこれを使う）。
- 適合表 `pace::conformance` に、`<Quit>` のあとも窓が閉じるまでフレームが来るときの 2 行を足した（`frames_after_a_quit_press_nothing`・`the_step_after_a_quit_is_left_while_the_window_closes`）。`Taken::Quit` を答えたホストは、そのあと `Pace::next` を呼ばない。`Pace` 自身は止まらないので、止めるのはホストの役目（`Runner` と kura は前から止めていて、`PaceSubject` も同じにした）。

### 修正

- `scripts/lanes/evidence.py new` が、別の OS の形のパスの `evidence=` で、思わぬ場所にフォルダーを作っていた（Linux では `C:\…` や `\` を含むパスがリポジトリの下の変な名前のフォルダーになり、Windows では `/…` がドライブの直下にできる）。コード 2 で断り、何も作らない。`--evidence`（`xrun.sh --evidence-root`、`keys.ps1 -EvidenceRoot`）で渡す。

### 使う側がやること

- `scripts/lanes.sh sync` で `evidence.py` の写しを更新する。
- `pace::conformance` を回しているアプリは、`rev` を上げると新しい 2 行も回る。`Pace` で動かすアプリは、`Taken::Quit` を答えたあと `next` を呼ばない。
- `review::Capture` は `#[non_exhaustive]` なので、`Default` から欄を埋める。

## [0.281.0] - 2026-11-08

### 追加

- `scripts/lanes/merge-lanes.py`: 見た目の行の `[x]` の門番（ito#205 の続き）。PR のベースのブランチに `tests/visual/<row>.toml` がある行に新しく `[x]` を付けるとき、次のどれかなら PR をマージしない。
  - 検査の仕様が読めない・正しくない。`tier` が 1 でない（Tier 2 はまだ無く、Tier 3 は持ち主のもの）。
  - PR の本文（`evidence_in_report` のアプリは報告も）に、`<row> tier1 [x] <verdict のパス>` の行が無い。行は `41.15 tier1 [x] C:\dev\filer-evidence\20261107-101203-41.15\verdict-41.15.json` の形で、行の ID は完全一致（範囲は通らない）、パスの最後は `verdict-<row>.json`。
  - `[~]`・`[-]`、仕様の無い行は今までどおり。`tests/visual` が無いアプリでは断る行が出ない（新しい `[x]` のある PR ごとに、ベースの fetch と `git ls-tree` を 1 回ずつ走らせるだけ。それが失敗すれば、その PR はエラーでマージしない）。設定は要らない。
  - 限界: 証拠はレーンの機械にあるので、GitHub Actions からは verdict の中身もハッシュも読めない。見るのは宣言の行だけで、本当に `auto-pass-candidate` だったかは確かめない。仕様を持たない見た目の行も見分けられない（これはレーンの規則のまま）。

### 変更

- `merge-lanes.py`: 確かめている間に PR のベースが変わったら、頭が動いたときと同じくマージせず次回に回す。
- `merge-lanes.py`: `git` の出力が UTF-8 でないとき、実行全体が止まっていた。その PR だけのエラーにして、次の PR に進む。

### 使う側がやること

- `rev` を上げて `scripts/lanes.sh sync` を流す。`tests/visual/` が無いアプリは、ほかに何もしなくてよい。

## [0.280.0] - 2026-11-08

### 追加

- `ito-keyscript`: `Pace::check_since()`。いま再試行している `<Assert:>` などの確かめが、最初に `Taken::Again` で戻ったフレームの時刻を読む。再試行していなければ `None`。`<ReadHas:>` の再試行は別の時計なので、ここには出ない。

### 修正

- `ito-keyscript`: 最初の `run.json` が書けないとき、`Runner::start_run` が失敗の行を書かないまま `trace.jsonl` を閉じていた。`<run.json>` の失敗の行が残るようになった。
- `ito-keyscript`: 確かめの再試行の途中で窓が閉じると、実行が終わったあとも `Pace::due()` が次のフレームを頼み続けていた。終わるときに再試行の時計を消す。`Runner` は実行中だけ `due` を読むので、`Runner` を使うアプリには影響がない。

## [0.279.0] - 2026-11-08

### 追加

- `scripts/lanes/keys.ps1` に `-Evidence`、`scripts/lanes/xrun.sh` に `--evidence LABEL`（見た目の行の証拠、ito#205 の続き）。`evidence.py new` と `manifest` を手で順に呼ばなくてよくなった。
  実行の folder を作り、run ID を環境変数（`keys_env` の `_DONE` を `_RUN_ID` に替えた名前）で渡し、`isolate=` の変数ごとに空のフォルダーを渡し、どの終わり方（終わった・失敗・時間切れ・窓が出ない・Ctrl+C・SIGTERM）でも manifest を書く。
  アプリが残したものは manifest の note に名前を書いて止め、cleanup を FAIL にする（Windows はジョブ オブジェクト、Linux はプロセス グループ）。`-Spec`・`-Scenario`・`-EvidenceRoot`（`--spec`・`--scenario`・`--evidence-root`）も渡せる。folder の道は最後の行に出す。
  - 終了コードを 2 つ足した。5 は証拠を作れなかった（実行の前に断った・`new` か `manifest` が失敗した）、6 は実行は成功したが manifest に問題がある。実行の前に断ったときは folder を残さない。
  - `-Evidence` は `-Out`・`-Launches` と組めない（5）。`--evidence` はアプリの引数に `--keys` が要る。
  - 限界: `xrun.sh --evidence` はまだ本物の Linux で回していない（Git Bash の上のスタブで確かめただけで、`setsid`・`ps`・グループへの kill は未確認）。`keys.ps1` のジョブの一覧が取れないときの道と、`evidence.py` の呼び出しが失敗するときの道も試していない。自分でセッションやプロセス グループを作ったプログラムは、`xrun.sh` の後片付けの検査の外。

### 変更

- `keys.ps1`: 設定した環境変数（`keys_env`・`isolate=` のもの）を、終わるときに元の値に戻す（無かったものは消す）。同じセッションの 2 回目の実行が、1 回目のフォルダーを受け継がない。
- `xrun.sh`: `--evidence` でなくても、アプリを `setsid` で起動し、終わるときにアプリ・グループに残ったもの・Xvfb の順に止める。`setsid`（util-linux）と `ps`（procps）が要る。再利用する OUTDIR から `*.jsonl` も消す。

### 使う側がやること

- `rev` を上げたら `scripts/lanes.sh sync` を流す。
- `-Evidence` を使うアプリは、`scripts/lanes.conf` に `isolate=` と `evidence=` を足す（`evidence=` の代わりに `-EvidenceRoot`・`--evidence-root` でもよい）。いま `isolate=` があるのは kura だけで、ほかのアプリでは 5 で断る。

## [0.278.0] - 2026-11-08

### 追加

- ito-testcheck の `testcheck` に `--open`（ito#200）。まだ済んでいない手で押す行（`[ ]` と、見ただけの `[~]`）だけを、その節の見出し・注・準備と一緒に stdout に出す。
  全体の表（kura では 400 KB ほど）を読まずに、残りだけを丸ごと読める。ファイルは書かず、印は今までどおり全体の表に付ける。`--lane NAME` と組める。
  全部が自動テスト済みの節と「自動テスト済みなので下には出していない」の行は出さない。済みだが TESTING.md に無い項目は全体の表と同じに出す。残りが無ければ「残りはありません。」（`plain` なら `Nothing open.`）。
  行の書き方は全体の表と同じ関数で作るので、2 つがずれない。`--check`・`--stats` とは組めない（終了コード 2）。行が 1 つも読めない TESTING.md は `--check` と同じく終了コード 1 で断る。
  使う側は何も変えなくてよい（`cargo run --example make-testcheck -- --open`、ほかのレーンは `-- --lane linux --open`）。

## [0.277.0] - 2026-11-08

### 追加

- 見た目の行の検査と証拠の道具（ito#205 の C）。`scripts/lanes/` に入り、`lanes.sh sync` で各アプリの `scripts/` に写る（標準ライブラリだけの Python 3.11 以上）。
  - `visual-check.py`: check spec（`tests/visual/<行>.toml`）と実行の folder を読み、`machine/assertions.json`（assertions.v1）を書く。終了コードは PASS 0・FAIL 1・ERROR 2。
  - `evidence.py`: `new`（実行の folder と、spec を固定する `pre/intent.json`）・`manifest`（どの終わり方でも）・`check`・`verdict`（`verdict-<行>.json`。`result` は候補で、`mark` と `decided_by` は main モデルが書く）。
- `docs/schemas/`: state・shot・trace・run・intent・manifest・assertions・verdict の `*.v1.json` と、`examples/`。`scripts/check-schemas.py` が schema の書き方、例、両スクリプトの出す実物を検査し、verify.sh と CI の `lanes` が回す。
- ito-keyscript のテスト: Rust の書き手（state・shot の付属 JSON・trace・run）の出力が `docs/schemas/examples/` と同じ形（キー・順序・型）であることを確かめる。
- `pace::conformance` の 4 つを `pub` にした（kura の頼み）。アプリが自前の `Subject` で `check(&mut subject, &cases())` を回し、合わない手順の答えを自分で組める:
  `StatusSpec::status(base)`（台帳の `Status` を作る）、`Answer::taken(&Entry)`（`Stage` は `<Snap:>` の段で決める）、`Answer::default_for(&Entry)`（手順の種類の既定の答え）、`dialect()`（台本を読む `Dialect`。`<ReadHas:Pane=…>` の `Pane`）。

### 変更

- `docs/visual-test.md`（版は 1 のまま。判定は変えず、規則を正確にした）: check spec の読み方（`spec = 1`・`size_pt`・`expect` の閉じた一覧・単位・色のサンプリングと mask・半透明のトークン・ERROR と FAIL）、
  実際のファイル名と run id（`.` を含まない）、実行の順、条件 1〜10 と verdict の `conditions` の対応。

### 使う側がやること

- `rev` を上げて `scripts/lanes.sh sync` を流す（`evidence.py` と `visual-check.py` が `scripts/` に入る）。
- 見た目の行は `tests/visual/<行>.toml` を書き、実行の前にコミットする（`docs/visual-test.md` の d6 の形）。
- Rust の API は足しただけ（上の `pace::conformance` の 4 つ）。使う側に直すところは無い。

## [0.276.0] - 2026-11-08

### 追加

- `<Snap:名前>`（ito#205 の B）: 状態（`state/<名前>-before.json`）→ 絵（`shots/<名前>.png`）→ 状態（`state/<名前>-after.json`）を 1 つの手順にする。
  - 前後の state は同じ `capture_id` と `snapshot_seq` を持つ（`phase` は `before` / `after`）。付属の `shots/<名前>.json` は after の state が書けたあと、手順が終わる前に書く。前後の path・sha256・`state_unchanged`・`png_sha256` を持つ。
  - state JSON を使わない（`Runner::state_json(true)` を呼ばない）実行では、`<Snap:>` は理由つきで失敗する。
  - 同じ名前の `<Snap:a>` が 2 つ、`<Snap:a>` と `<Shot:a>`、`<Snap:a>` と `<State:a-before|a-after>` は同じファイルに書くので、解析の時点で断る（`@接尾辞` つきの Shot は別名）。
- `trace.jsonl`: Runner が 1 手順 1 行（step・label・outcome・settled・frame・time・why）で書く。書けなければ `<trace.jsonl>` の手順で失敗にする。`keys.ps1` は `*.jsonl` も片付ける。
- `<State:>` の `settled` を Pace が入れる（これまでは `null`）。
- 適合テストに `snap_*` の 11 例を足した。

### 変更

- `ito-a11y`: `REPEAT_GAP` の説明を直した（同じトーストを続けて読むときの最短の間隔。`Told` がトーストごとに持つので、別のトーストは 2 つとも続けて読む）。動きは変わらない。

### 使う側がやること

- `Step::Snap`・`Press::Snap` が増えた。kura の `unsupported()` に `Press::Snap { .. }` の腕を足し、`Taken::Failed` を返す。
- `state::Ack::Written` に `snapshot_seq` が増えた。`Ack::Written { name, .. }` で受ける。
- `state::Header` に `phase`・`capture_id` が増えた。`..Default::default()` で足す。
- `Runner::write_state` は、待っている `<State:>`（または `<Snap:>` の前後）が無いときに `Err` を返す。これまでは `after_step` を空にして書いていた。実行は止めない。
- 自前の実行で Snap に対応するなら、after の `state_saved` は付属 JSON が durable になってから呼ぶ。
- 任意: `Pace::take_settled`、`Pace::take_trace` と `evidence::TraceFile`、`review::Capture` と `Meta::to_json_with`（`Meta` の欄は増やしていない）。
- 適合テスト: `Answer::Stage` と `Need::Snap` は追加。`Answer::taken` は内部用。`pub fn acts` は `Stage` を手順で解く。

## [0.275.0] - 2026-11-08

### 追加

- `ito-keyscript::state`: 状態の JSON v1（ito#205 の A）。`<State:名前>` で `<dir>/state/<名前>.json` を書く。
  - アプリは `App { screen, overlay, toasts, app_state }`（知っていることだけ）を渡す。ito は同じパスの a11y の木（test ID・役割・名前・値・状態・`rect_px`）と、窓・ズーム・テーマ・言語を足して 1 つにする。
  - `rect_px` は物理ピクセル（原点は viewport の左上、丸めない）。`visible`・`clip_px`・`elided` は `test_id_in` で登録した行だけ。
  - アプリが渡さなかった欄は `null` で、`capabilities` にその欄が無い。「無い」と「知らない」を分ける。
  - `stable` は木・アプリ・画面の部分のハッシュ（sha256、キーを並べ替えた JSON）。座標は丸めずに入るので、ハッシュが同じでも、間に動いたものが無いとは言えない。
  - 書くのは専用のスレッド（`.part` に書いて flush してから名前替え）。書き終わるまで Pace が `<State:>` の次を止める。書けない、または 5 秒（`Config::state_deadline`）以内に来ないときは `keys: failed`。
- `Runner::state_json(true)` と `Runner::write_state(ctx, 名前, App)`。自前の実行は `state::submit(ctx, Header, App)` と `take_ack`。egui が捨てたパスは次のパスに引き継ぐ。
- `ito-a11y::test_id_in(ui, resp, 名前, galley)`: クリップ矩形と文字の省略を一緒に登録する。`TestIdEntry` に `clip`・`elided` が増えた。
- `ito-theme::Colors::tokens()`（42 の名前つきの色）と `rgba_hex`。`css_vars` はこれを使う。
- `ito-keyscript::pace`: `Taken::State`、`Config::state_deadline`、`Pace::state_saved`・`state_failed`。適合テストに `Answer::State`・`StateEvent`・`Need::StateJson` と 5 つの例を足した。`Tick` に欄 `state`（組み立ては `.state(StateEvent)`）。`Tick { .. }` を字で書いている側は `state: None` を足す。

### 変更

- `IdNode` に `label`・`clip`・`elided` が増えた。

### 使う側がやること

- Runner を使うアプリ（yagura・koyomi・michi・fude）は任意。使うなら `.state_json(true)` を付け、`Do::State(名前)` が出たら同じフレームの中で `runner.write_state(ctx, &名前, App { … })` を呼ぶ（`.txt` は今までどおり）。知っている欄だけ埋める。切れる・隠れる文字の欄は `test_id_in` で登録すると `clip_px`・`visible`・`elided` が付く。
- kura（Pace の上の自前の実行）:
  - `Taken`・`Answer`・`Need` を網羅して `match` しているなら、`State` の腕を足す。
  - 適合テストの `Subject::skips` に、state JSON を返すまでは `Need::StateJson` を入れる。
  - `Config` を構造体リテラルで作っているなら、`state_deadline` を足す（`..Config::new(…)` なら変更なし）。
  - `TestIdEntry` を外で組み立てているなら、`clip`・`elided` を足す。`IdNode` は `non_exhaustive` なので変更なし。

## [0.274.0] - 2026-11-08

### 修正

- `ito-a11y::announce`: 知らせる文を live region（`Live::Polite`）のノードにした。今まではただのラベルで、スクリーンリーダーは読まなかった（accesskit_windows 0.32 の adapter は、`live` が `Off` でないノードが名前つきで現れたときと名前が変わったときだけ UIA の LiveRegionChanged を出す）（ito#207）。

### 追加

- `ito-a11y::announce_with(ui, 文, Urgency)`: 読み上げの急ぎを選べる。`Urgency::Polite`（既定。読んでいるものを終えてから）と `Urgency::Assertive`（割り込む。見落とせないエラー向け）。
- `ito-a11y::toast_line(level, text, lang)`: トーストの読み上げの文。重さがあれば言葉で前に付ける（`注意: …`・`error: …`）。
- `ito-a11y::Told` と `announce_toast(ui, &mut told, raises, level, text, lang, now)`: トーストを出し直し 1 回につき 1 回だけ読む。
  - 同じトーストの繰り返しは 1 秒に 1 回まで。間引いた分はあとで読まない。
  - 文か重さが変われば別のトーストとして、すぐ読む。
  - `State::Error` は割り込みで、ほかは割り込まずに読む。
  - 時刻は `now` で渡す。

### 使う側がやること

- トーストを積むところ（kura の `App::raise`、yagura の `App::toast`）で、トーストの横に `Told` を持つ。
- トーストを描くフレームで、`announce_toast` に出し直しの回数（新しいトーストは 1）、`×n` を付けない文、重さ、言語、`Instant::now()` を渡す。
- `announce` の呼び方は変わらない。
- `announce_with`・`Urgency`・`announce_toast` は機能 `egui` の後ろ。`Told`・`toast_line` は機能なしでも使える。

## [0.273.0] - 2026-11-08

### 追加

- `ito-keyscript::evidence`: `Runner` が実行の記録 `run.json`（スキーマ・app・ito の版と rev・run_id・OS・steps・開始と終了・status・失敗した手順と理由・残り）を、最初のフレームで `running` として、終わりに 1 度だけ書く（`.part` に書いて flush してから名前替え）。書けなければ手順 `<run.json>` で失敗にする（終わりの記録が書けなければ、`keys: done` で終わるはずの実行（残りの無い quit・close も）をその失敗にし、ファイルは `running` のまま残る）。`Runner::app(名前, 版)` で app を渡す（ito#204 の 8）。
- `ito-keyscript` の `build.rs`: ito の rev を `ITO_BUILD_REV`、無ければ cargo の git checkout の道から入れる（どちらも無ければ unknown）。
- `ito-theme::set_theme` / `name`、`ito-i18n::set_in_force` / `in_force`: いま効いているテーマと言語の名前を覚える。

### 変更

- Shot の付属 JSON の `theme`・`lang` は、撮ると頼んだ時点で効いていた値になった。`<Theme:>`・`<Lang:>` が頼んだ値は新しい欄 `theme_requested`・`lang_requested` に分けた（ito#204 の 4）。
- Shot の PNG と JSON は `<dir>/shots/` に書く（ito#204 の 6）。`keys.ps1`・`xrun.sh` は `shots/`・`state/`・`run.json.part` も片付ける（リンク（中にあるものも）や非フォルダーなら止まる）。
- `ito-keyscript` が `ito-theme`・`ito-i18n` に依る。

### 使う側がやること

- `Runner::new(..)` に `.app("名前", env!("CARGO_PKG_VERSION"))` を足す（足さなくても動くが、`run.json` の app が空になる）。
- テーマを適用するところで `ito_theme::set(colors)` の代わりに `ito_theme::set_theme(&theme)` を、言語を決めるところで `ito_i18n::set_in_force(&lang)` を呼ぶ（呼ばなければ Shot の `theme`・`lang` は空）。
- Shot を読む側は `<dir>/shots/<名前>.png`・`.json` を見る。

## [0.272.1] - 2026-11-08

### 変更

- `docs/autotest.md`・`docs/visual-test.md`: 古くなった進み具合を直した（ito#203 は閉じた・ito#204 は 1〜3 が入った・状態 JSON v1 と `<Snap:>` と矩形つきの Tree は ito#205・kura は v0.154.4 で ID の手順と確かめを受けた・visual-test の版 1 の変更 1 は承認済み）。kura のセッションの依頼。

## [0.272.0] - 2026-11-08

### 変更

- `ito_keyscript::Runner` を `pace::Pace` の薄い宿主にした（#204 の 3）。待ち・落ち着き・再確認・設定の書き込み・画像の待ちの数え方は Pace に一本化。適合テストの表を `Runner` でも通す。
  - 追加: `Do::Unsettled(String)`（落ち着かないまま終わった。中身は done ファイルの tail）・`Runner::end(Ending) -> Option<Ended>`・`Runner::report() -> Option<&str>`。
  - 非推奨: `Runner::finish()`（`end(Ending::Closed)` を使う）。
  - `left()` は終わったあと、終わりの報告に入れた `left` を返す。`running()` は `end` が返ったあと（終わりの報告を渡したあと）か `Quit` を渡したあとは false。
- 動きが変わる 9 点（どれもテストで固定）:
  1. `<Size:>` のあと、約 400 ms + 60 ms 置く。
  2. `<Wait:N>` は前の手順の入力から数える。先頭の `<Wait>` も最初の 500 ms を守る。
  3. `<Now>` は Pace が受け、続く `<Wait>` をまたぐ。
  4. ポインターの手順は、最初のフレームを取った回に入れ、印は 1 つ。最後のフレームの次は 2 パスと GAP 待つ。
  5. 確認（Assert など）と Poll は毎パス再確認。Look は通常の準備で再確認。期限（TEXT_WAIT・READ_WAIT）は変わらない。
  6. `left` は失敗した手順も数える（最後の確認が失敗すると `left: 1`）。
  7. Shot: 保存の失敗は実行の失敗、画像が来なければ期限後に失敗、画像が出ている間に閉じると失敗。
  8. 設定の書き込み: 答えが来るまで止まり、失敗か CONFIG_WAIT（10 秒）で失敗。
  9. `watch`: `Progress.at` は再確認（Again）では動かず、手順が進んだとき（押した・描いた・待ちが明けた・画像を保存した・設定の答えが来た）に動く。`Progress.due` は残りの `<Wait>`。
- 公開を足した: `pace::Taken::Changed { more }`（入力でない変化。Size・Theme・Lang）・`pace::Ended.left`・`Pace::in_flight()`。
  `pace::conformance` の `acts`・`acts_taken`・`Need`・`Case::needs`・`Subject::skips`・`skipped`・`Answer::Changed`・`PaceSubject`（`Runner` の適合テストが使う）。

### アプリがやること

1. `Do::Unsettled(tail)` を扱う: 状態の報告のあとに tail を書き、`say` し、`FAILED_EXIT` で終わる。
2. 窓が閉じたとき、`script.finish()` と `closed_report` の代わりに `script.end(Ending::Closed)`: `e.tail` を書き、`e.say` を出し、`e.failed` なら `FAILED_EXIT`。
3. `Do::Quit` のあと、`quit_report(script.left())` の代わりに `script.end(Ending::Quit)` の tail を書く。
4. `Ended` を作る・分解するコードは `left` を足す。`Taken` の網羅的な `match` は `Changed` を足す。自前の Subject は、Look が「見つけて押した」ときだけ動作にする（`conformance::acts`）。
5. `finish` は非推奨。
6. `left` は失敗した手順も数える。

## [0.271.0] - 2026-11-08

### 追加

- `ito_keyscript::pace`（#204 の 2）: `--keys` の実行の状態機械 `Pace`。窓なし（egui の文脈には触らない。手順は `Press` のまま中を見ずに渡す）。kura の `ScriptRun` の動きをそのまま写した。
  - `Pace::new(Config, &[Step], now)`・`next(&Frame) -> Next::{Hold, Take, End}`・`act`・`taken(&Frame, Taken)`・`shot_saved`/`shot_failed`・`end(Ending, now, writes)`・`left`・`progress` など。
    `next` が見る順（16 項目）は `pace` のモジュールの説明と docs/autotest.md の表にある。
  - 補助: `fresh`（木が最後の操作より新しいか）・`read_has_why`・`state_lines`・`unsettled_lines`、`kind(&Press)`。
  - `pace::conformance`: 適合テストの表 `cases()`（台本・時計・フレームごとの様子・期待する記録）と、`Subject` を実装して回す `check`。`PaceSubject` が `Pace` 自身。
- `Progress` を公開（`left`・`at`・`due`・`finished`）。

### 変更

- `<Now>` の説明を 1 つの意味（kura と同じ: 直後の 1 手順を、落ち着きを待たずに送る）に統一した。
- 画面の名前で探す手順（`<Click:文字>`・`<DClick:文字>`・`<Assert:>`・`<Enabled:>` など）と `<Tree:>` の出力は UI の言語に従うので、台本の先頭に `<Lang:en>` を置く、と `Step` の説明に 1 か所だけ書いた。
- `scripts/lanes/ito-first.txt` に `arboard` を足した（`ito-clipboard` を使う。kura は v0.154.1 で載せ替え済み）。
- `GAP`・`FRAME` を `pub(crate)` に。

### 使う側がやること

- 今回は `Runner` はまだ `Pace` に載っていない（次の push）。kura は `pace::conformance::check` を自前の実装で通してから載せ替える。
- `rev` を上げて `scripts/lanes.sh sync` を流すと、`arboard` に直接依存するアプリは `ito-first.py` で落ちる。`ito-clipboard` の `clipboard!()` に載せ替えるか、`scripts/ito-first.allow` に理由を書く。

## [0.270.0] - 2026-11-08

### 追加

- `ito-clipboard`（#206 の 2）: 文字をクリップボードに写す共通の書き手。依存は任意の arboard（機能 `os`、既定）だけ。
  - `clipboard!()` が `Clipboard` を選ぶ。そのスレッドで `Fake::enter()` したものがあればそれ、なければ（呼んだクレート自身の `cfg(test)` では）そのスレッドの偽物、それ以外は OS。テストが機械のクリップボードを書き換えない。
  - `Clipboard::set_text(text)`: OS は長生きの 1 つの handle で書く。Windows は `\n` を CRLF にする（`line_endings(text, windows)`。既存の CRLF は 1 つのまま、ほかの OS は CRLF を LF に）。成功したときだけ `last_set` を覚え、`note_copy` に元の文字を渡す。`get_text()` は短命の handle で読む（書き込みを待たせない。X11 では止まるのでワーカーから。空・文字なしは `Ok(None)`）。`last_set()`・`is_fake()`。`Clipboard` は `Clone + Send + Sync` で、ワーカーに渡すと選んだ偽物がついていく。
  - `Error::{Occupied, NotSupported(String), Conversion, Other(String)}`（`#[non_exhaustive]`）。
  - `Fake`: メモリ上のクリップボード（clone は同じものを指す）。`enter`（`Entered` の guard。入れ子可）・`this_thread`・`put`（他のプログラムがコピーした体で `get_text` に出す）・`contents`・`last_set`・`writes`・`fail_next_write`・`noting`（成功した書き込みを `note_copy` にも渡す。記録はプロセスで 1 つなので、順番を取ること）。
  - `release()`: 終わるときに 1 回。X11 では最後の handle を落としたときにクリップボードマネージャーへ渡る。
  - コピーの記録（`note_copy`・`record::{start, next_stamp, snapshot, note_when, reset}`）を `ito-keyscript` から移した。動きは同じ。

### 変更

- `ito-keyscript`: `note_copy` は `ito_clipboard::note_copy` の再エクスポート（呼び方・意味は同じ）。`observe` は `ito_clipboard::record::start()` を呼ぶ。
- 文書: `docs/visual-test.md`・`docs/autotest.md` の「kura は木とクリップボードの手順を窓の前で断る」を、kura v0.154.0 から自分で実行する、に直した。

### 使う側がやること

- kura: `exec::set_clipboard` → `ito_clipboard::clipboard!().set_text(text)`、`get_clipboard` → `clipboard!().get_text()`（ワーカーで。`None` は空）、`last_set_clipboard` → `clipboard!().last_set()`、`fake_clipboard` → `Fake::this_thread().put(Some(..))`、`note_copies_here` → `Fake::new().noting()` を `enter` して順番を取る。arboard の直接の依存を外す。終わるときに `ito_clipboard::release()`。
- ほかのアプリ（michi・fude・yagura・koyomi）: `ctx.copy_text` はそのまま `<Clipboard:>` に見える。Windows で CRLF にしたいなら `clipboard!().set_text`。

## [0.269.0] - 2026-11-08

### 追加

- `ito-common`: 設定の書き込みの台帳 `writes`（#204 の 1）。`--keys` の実行が、アプリが設定のファイルを書いて当て終わるまで次の手順・読み取り・終わりに進まないための数。
  - `ledger()` はプロセスで 1 つの台帳。`watch()` を呼ぶまで何も記録しない（ふつうに動くアプリは何も持たない）。ON にするのは `--keys` の実行で、それは #204 の後の push で入る。`Ledger::new()` は最初から記録する自分の台帳（テスト用）。
  - 書き始めに `sent(file) -> Ticket`、UI で結果を当てたあとに `Ticket::applied()`、書けなかった・当てられなかったら `failed(why)`。答えずに落とした券は失敗（`"{file}: write #{seq} was dropped without an answer"`）。
  - `status() -> Status { pending, oldest, answers, failed }`。`failed` は最初の失敗が残る。記録していないときの券は何もしない（`Ticket::inert()` も同じ）。
- `ito-common`: common.toml の共通の書き手 `change_common(base, changes, back)` と `Written::{Saved, Failed, NoFolder}`。アプリで 1 本の書き手のスレッド（`ito-common-writer`）が、頼まれた順に `CommonChange` を全部当てて書き、読み直して `back(Written, Ticket)` を呼ぶ。`back` は UI へ送って起こすだけにし、UI が当てたあとに券に答える。`changes` が空なら何もしない。`base` が `None` なら `NoFolder` を、書き手が動かせないときは `Failed` を、呼んだスレッドで返す。`back` の中の panic で書き手は止まらない。`edit_common` は今までどおり。
- 雛形（`templates/app`）: 拡大率の保存を `change_common` にした（拡大率は先に UI で当てるので、`back` ですぐ答える）。変えるたびにスレッドを立てていたので、続けて変えたとき書く順が入れ替わることがあった。

### 使う側がやること

- yagura・koyomi・michi・fude: `prefs.rs` の `common_change`（変更ごとにスレッドを立てて `edit_common` と `Common::read` をし、`Reload::Common` / `Reload::Failed` を送るもの）を `change_common` に置き換える。`Reload` に `Written` と `Ticket` を載せ、`pump` で当てたあとに答える（`Saved(Ok)` は `applied()`、`NoFolder` も含めてほかは `failed(..)`）。拡大率も雛形と同じく `change_common` で書く（続けて変えても、1 本の書き手が頼まれた順に書く）。`change_common` を通らない書き込み（アプリ自身のファイル（keymap など）や、自分で `edit_common` を呼ぶところ）は、`ito_common::writes::ledger().sent("<file>")` の券を書き始めに取り、同じように答える。
- kura: 今は自分の `config_writes.rs` のまま。Runner と kura の実行の状態機械をそろえる push（#204）で載せ替える。
- 今はまだ誰も `watch()` を呼ばないので、置き換えなくても動きは変わらない。

## [0.268.0] - 2026-11-08

### 追加

- `ito-keyscript`: `note_copy(text)`（#206 の 1）。egui の `CopyText` を通らずに自分の道でコピーするアプリ（arboard に直接など）が呼ぶと、そのコピーが `<Clipboard:>`・`<ClipboardHas:>` に見える。どのスレッドからでも、`egui::Context` なしで呼べる。記録するのは `observe` のあと（`--keys` の実行か、`observe` を呼ぶアプリ自前のループ）だけで、それ以外は何もしない。記録はプロセスで 1 つ。`mark().copies` と直前の操作のあとの判定は、egui のコピーと `note_copy` を合わせて数え、新しい方を最後のコピーにする。
  - kura がやること: `exec::set_clipboard` で、CRLF に直す前の LF の文字で `note_copy` を呼ぶ。`<Clipboard:>`・`<ClipboardHas:>` を断るのをやめる。

## [0.267.0] - 2026-11-08

### 追加

- `ito-keyscript`: 読み取りを実際にいつ取ったかを記録する（#203、時間を決めた読み取り）。`Runner::since_action()`（直前の操作の最初の入力が入ってからの時間。最初の操作の前は `None`）と `since_action_line(since)`（`<State:>` の行 `since last action: 153 ms`、操作の前は `since last action: none`。ミリ秒は切り捨て、最後に改行）。ポインターの手順は最初のフレームで 1 回だけ時刻を取る。`<Wait>`・`<State>`・`<Now>` では変わらない。
  - アプリがやること: 使うなら、`Do::State` を受けたフレームで `since_action()` を読み、`since_action_line` の行を `<State:>` のファイルに足す（ファイルを書くときではなく、状態を取るときに読む）。自動では書かない。

### 変更

- `ito-keyscript`: `<Now>` のあとに `<Wait:N>` を置けるようにした（時間を決めた読み取り）。`<Now>` は、間の `<Wait:N>` を越えて、次の待ちでない手順に掛かる。
  - 書き方: `x<Now><State:a><Now><Wait:150><State:b>`（`a` は `x` が描かれたらすぐ、`b` は `a` の約 150 ms 後。間の待ちも落ち着きを待たない）。`<Now>` は 1 つの手順で使い切るので、`<Now><State:a><Wait:150><State:b>` の `b` は今までどおり落ち着きを待つ。
  - 待ちが続くときは 1 つずつ順に数え、それぞれ 1 フレームほど延びることがある。N は下限で、約束ではない。届いたかの確認・アプリの設定の書き込みの待ち・木の読み取りの鮮度の規則は、`<Now>` でも飛ばさない。
  - 通るようになったのは、`<Now><Wait:N>` のあとに待ちでない手順が続く形だけ（`<Now><Wait:N>x` など。今までは断っていた）。今までどおり断る形: 最後の `<Now>`、`<Now><Now>`、`<Now><Quit>`、`<Now><Wait:N>` で終わる、`<Now><Wait:N><Quit>`、`<Now><Wait:N><Now>…`。違ったときの文言を直した（`` …, or before `<Wait:N>` and then one (`<Now><Wait:150><State:b>`) ``）。
  - `Runner` の時間の進め方は変えていない（落ち着きの待ちが無く、手順の間に `GAP`（60 ms）を置く）。だから `Runner` の時間を決めた読み取りは、N より `GAP` ほど遅い。進め方をそろえるのは ito#204。
  - kura がやること: 自分のループはもうこの意味で動く（`<Wait>` は `<Now>` を解かない）。27.6 の行を `<Now><State:a><Now><Wait:150><State:b>` に直す。`state_report` に `since last action:` の行を足す。Drag は続きのフレームでも時刻を取り直さず、操作の最初の入力の時刻を使う。

## [0.266.0] - 2026-11-08

### 追加

- `ito-keyscript`: 直前の操作より後だけを見る確かめ（#203 B4）。`Mark { copies, announced }`（操作が入ったときのコピーの数と読み上げの通し番号）、`mark(ctx)`、`check_clipboard_after(ctx, after, expect)`・`check_clipboard_has_after(ctx, after, part)`・`check_announced_after(after, text)`。今までの `check_clipboard`・`check_clipboard_has`・`check_announced` は形も意味（実行の始まりから）も文言もそのまま。
- `ito-a11y`: 記録した読み上げに通し番号を付ける。`announced_count()`（最後の番号。1 から数え、戻さない。まだ無ければ 0）と `announced_after(count)`（その番号より後のもの、古い順）。残すのは今までどおり最後の 200 件。`announced()` はそのまま。

### 変更

- `ito-keyscript`: `Runner` の `<Clipboard:>`・`<ClipboardHas:>`・`<Announced:>` は、直前の操作より後のコピーと読み上げだけを見る（#203 B4）。前の手順のコピーや読み上げで合格にしない。
  - 操作は入力を入れる手順: キー・文字・`<Paste>`・`<Paste:文字>`・`<Size:>`・`<Theme:>`・`<Lang:>`・ポインターの手順（`<Click:>`・`<DClick:>`・`<Hover:>`・`<Wheel:>`・`<Drag:>`・`<Click:文字>`・`<RClick:文字>`・`<DClick:文字>`・`<ClickId:>`）。`<Wait>`・`<Now>`・`<State>`・`<Shot>`・読む手順は操作ではないので、`y<Wait:500><Clipboard:x>` は今までどおり通る。
  - 区切りは、操作の最初の入力が入るとき（その入力を描く描画より前）。ポインターの手順は最初のフレームで 1 回だけなので、押したフレームのコピーは離したあとも数える。最初の操作の前は、実行の始まりからのコピーと読み上げを数える（違ったときの文言も今までどおり）。
  - 違ったときの文言: `` nothing copied after the last action, not `x` (the last copy, before it, was `y`) ``、`` `x` was not announced after the last action; announced after it: … ``（後に何も無ければ `(before it: …)` も）。
  - アプリがやること: 台本のコピー・読み上げの確かめは、それを起こす操作の直後に置く（間に別の操作を挟まない）。kura は自分のループで、まだこの手順を断る（使うときは `mark` と `_after` を呼ぶ）。

## [0.265.1] - 2026-11-08

### 変更

- 文書: tsumugi・minato をメンテナンス中のアプリとして書いていたところを直した（2026-11-08 にメンテナンス対象から外した）。メンテナンスしているのは kura・yagura・koyomi・michi・fude（と ito）。
  - `README.md` の使う側の一覧から tsumugi・minato を外し、koyomi を足した。2 つが前に使っていたことは注に残した。
  - 手本のアプリを koyomi（設定・ボタン・パレット・通知・キー）と michi（ヘルプ・`--keys`・字体）にした（`docs/design-guide.md`・`templates/app/TODO.md`）。空の一覧（`empty_state`）の手本は、メンテナンスしているアプリにはまだ無い。
  - `docs/common-spec.md`・`docs/design-principles.md` のアプリの並びを直した。2026-10-11 の時点の表と文中の古い記述は記録として残し、そう注に書いた。テーマの名前 `tsumugi Dark`・`tsumugi Light` と変更履歴はそのまま。
  - `CLAUDE.md` のクレートの表の「主に使う側」から tsumugi・minato を外した。抜けたところは、各アプリの `Cargo.toml` で今使っているもので埋めた。kura・yagura・koyomi・michi・fude の全部が使うものは「全アプリ」と書いた。`ito-browser` は今使うアプリが無い。冒頭の使う側の一覧も直した。
  - 切り出しの由来、動きに関わる `tsumugi` の文字列、needs-pc の巡回の対象の記述は残した。

## [0.265.0] - 2026-11-08

### 追加

- `ito-keyscript`: done ファイルの report に run ID を付ける（#203 part 3）。古い実行の done ファイルと自分の実行のものを、テストの道具が見分けるため。
  - `run_id(prefix)`（`<PREFIX>_KEYS_RUN_ID` か、なければ `<unix ms>-<pid>`。1 プロセスで最初に決まったものを保つ。値が `A-Za-z0-9_-` の 1〜64 文字でなければ `Err`）、`current_run_id()`、`with_run(id, report)`、`with_current_run(report)`。
  - 置き場所の規則: report にちょうど 1 行の `run: <id>` を入れる。1 行目は元のまま（`keys: stalled` を 1 行目で読む `xrun.sh` はそのまま動く）。成功の report は最後が `keys: done` のまま（`run:` はその直前）。
  - ito の watchdog（stalled）と `install_panic_report` は自分で `with_current_run` で包む（`run_id` を呼ばないアプリでは `run: (none)`）。`*_report` の関数そのものは `run:` の行を足さない。
  - アプリがやること: 窓を開く前に `run_id("KURA")` のように 1 回呼ぶ（`Err` は `refused_report` で断る。その report も `with_current_run` で包む）。done ファイルに書く全部の report（done・failed・quit・closed・refused・アプリ自身の stalled など）を `with_run` / `with_current_run` で包む。
- `ito-keyscript::probe`: 子として起動される確かめ役（Settings → Shell の program・引数・環境変数が子に届いたかを、テストが確かめる）。`ITO_PROBE_DIR`・`ITO_PROBE_NONCE`（どちらも必須。無いか空、または nonce に制御文字があれば出力なしで exit 2）・`ITO_PROBE_ENV`（記録する変数名）を読み、`probe-<n>.txt` を書いてから `probe ready <n>` を出す。書けなければ stderr に `probe error: …` で exit 1。stdin が閉じるか 120 秒で exit 0。kura は `examples/term-probe.rs`（`fn main() { ito_keyscript::probe::main() }`）を足す。yagura も同じ形で使える。
- `ito-prefs`: Shell カードにテスト ID（AccessKit が有効なときだけ付く）: `settings.shell.program`、`settings.shell.env.edit`・`.add`・`.cancel`・`.save`、環境変数の行 `settings.shell.env.row.<key>.name`・`.value`・`.remove`。引数の欄は `settings.field.shell-args`。

### 変更

- `ito-keyscript`: `refused_report(why)` は `why` の改行を ` / ` にして 1 行にする（`failed_report` と同じ。改行で `run:` などの行が混ざらない）。`run_id` の `Err` の文は値をエスケープして見せる。
- `ito-prefs`: Shell カードの「追加」は、押したフレームの次の描画を頼む（足した行がすぐ見える）。
- `ito-prefs`: `field` が欄に `settings.field.<id>` のテスト ID を付ける（`ito_a11y::is_test_id` に通る id だけ。通らない id には付けず、落ちない）。
- `ito-prefs`: Shell カードの環境変数の行は番号（key）を持ち、行を消しても他の行の番号を振り直さない（テスト ID が同じ行を指し続ける）。公開 API は変わらない（`ShellEdit` は `Default` のまま）。

## [0.264.1] - 2026-11-08

### 追加

- `docs/autotest.md`（自動実機テスト基盤の設計）と `docs/visual-test.md`（視覚テストの設計、version 1）。claude.ai の設計のページから移した。ここが正本で、ページは写し。アプリの規則のファイルは「ito docs/visual-test.md, version 1」のように版で名指しする。
  - 載せるアプリは kura → yagura・koyomi・michi・fude（4 つの順は kura のパイロットのあとで決める）。2026-11-08 にメンテナンス対象から外した tsumugi・minato は載せない。

## [0.264.0] - 2026-11-08

### 追加

- `ito-a11y::test_id` と `take_test_ids`（#203 の ②）: 部品に試験用の名前（`is_test_id` の形）を付ける。AccessKit の `author_id` と、描いた場所の記録を残す。AccessKit が切れていれば何もしない。
- `ito-keyscript` の ID の手順 `<ClickId:名前>`・`<Id:名前>`・`<Id:!名前>`・`<FocusedId:>`・`<EnabledId:>`・`<DisabledId:>`・`<ValueId:名前=文字>`、`Observation::ids`・`IdNode`・`IdCheck::check`。
  - 同じ名前が 2 つ、1 つの部品に 2 つの名前などは、どの ID の手順も失敗にする（`<Id:!>` も）。
  - `<Id:名前>` は、木で隠れている（hidden）部品では失敗する（`<Id:!名前>` も通らない）。
  - `<ClickId:>` は、押した結果を確かめる手順（`<FocusedId:>`・`<ValueId:>`・`<Id:>`・`<Id:!>`）が続かなければ解析で断る。
- `<ReadHas:名前=文字>` と `Reader`・`Runner::input_with`・`READ_WAIT`（15 秒）。`TEXT_WAIT`・`DELIVER_WAIT` も公開した。

### 変更

- 探す文字が空の手順（`<Assert:>`・`<Clipboard:>`・`<ClipboardHas:>`・`<Announced:>`・`<Enabled:>` などと文字のクリック）は、解析で断る。
- ID の手順と `<ReadHas:>` は `*N` で繰り返せない（解析で断る）。
- 使う側（kura）がやること: `Press` と `Step` に `ClickId`・`IdCheck`・`ReadHas` が増えたので、`match` に腕を足す。ID の手順は `fresh` の観測で `IdCheck::check`／`Observation::id(..).click_at` を使える。
  `<ReadHas:>` を使うアプリは `Runner::input` の代わりに `input_with` で `Reader` を渡す（渡さなければ `this app does not read …` で失敗）。部品には描いたあとで `ito_a11y::test_id(&resp, "名前")` を付ける。

## [0.263.0] - 2026-11-08

### 変更

- `ito-keyscript`（#203 の ①）: 窓の観測に世代を付け、`Runner` の読み取りの手順を新しい観測だけで判定するようにした。
  - `observe` が見るのは主の窓（`ViewportId::ROOT`）の pass だけ。AccessKit の木が来た pass ごとに観測を作り、世代を 1 つ上げる（新しい `generation(ctx)`。単調に増え、`observe` の前は 0）。
    木が来なかった主の窓の pass のあとは観測を空にする（`items` も空を返す。これまでは古い一覧を返し続けた）。子の窓の pass は観測を変えない（コピーした文字は今までどおり記録する）。
  - 新しい `observation(ctx) -> Option<Observation>`: 主の窓の最後の観測（世代・一覧・木・pass の番号・viewport・窓の矩形・最小化・フォーカス）。最後の pass で木が来ていなければ `None`。`Observation` は `#[non_exhaustive]`。
  - `Runner` は、入れた入力（キー・貼り付け・ポインターの 1 フレームずつ・`<Size:>`・`<Theme:>`・`<Lang:>`）を主の窓の pass が描いたのを確かめてから次へ進む。
    10 秒（`DELIVER_WAIT`）のうちに描かれなければ `keys: failed`（`the input was not drawn within 10 s …`）。
    キー・貼り付け・ポインターのあと、主の窓が描かれないうちに子の窓の hook が来たら（eframe は残った入力を次の窓に渡す）`keys: failed`（入力を入れない `<Size:>`・`<Theme:>`・`<Lang:>` では落ちない）。
    主の窓以外の hook には何も入れない（これまではどの窓の hook にも入れていた）。描かれるのを待つあいだ、`due()` は `FRAME`（20 ms）より短くならない。
  - 読み取りの手順（`<Assert:>`・`<Assert:!…>`・`<Enabled:>` などの状態・`<Tree:>`・`<Click:文字>`・`<DClick:文字>`）は、最後の入力を描いた pass より後の観測でだけ判定する。
    観測が古い・無い・窓が最小化されているときは、毎フレームやり直し、`TEXT_WAIT`（3 秒）を過ぎたら `keys: failed`。否定の手順（`<Assert:!x>`）は、`x` の無い新しい観測で通る（消えるのを `TEXT_WAIT` まで待つ）。通ったあとで出てくるものは見ない。
    `<Tree:>` は、新しい観測が無いか木が空なら失敗する。
  - `<Clipboard:>`・`<ClipboardHas:>`・`<Announced:>` は今のまま。
  - 使う側: API は変わらない（yagura はそのまま）。読み取りの手順は 1 フレーム遅れて通る。`items`・`locate`・`check`・`check_is`・`tree_text` を自分で呼ぶアプリは、木が来なかった pass のあとで一覧と木が空になることに注意。

## [0.262.7] - 2026-11-08

### 修正

- `scripts/lanes/xrun.sh`（#202）: `--keys` のとき、`keys.done` の最後の行が `keys: done` でなければ（`keys: failed`・`keys: unsettled`・`keys: refused`・`keys: stalled`、またはファイルが無い）、
  `shot.png`・`title.txt`・`clip.txt` を撮ったあとで、`keys:`・`step:`・`why:`・`pressed:`・`left:`・`unsettled:` の行を stderr に出して非 0 で終わる。
  アプリが自分で先に終わっていたときはその終了コード（0 なら 1）。これまでは `keys: stalled` のときに警告を出すだけで、いつも 0 で終わっていた。
  使う側（kura・yagura・tsumugi）は `scripts/lanes.sh sync` で写す。`xrun.sh` を呼ぶ手順が非 0 を「結果なし」として扱えるか確かめる。
- `ito-watch` のテスト `a_file_made_below_a_watched_folder_is_told`: Linux の inotify は、できたばかりのサブフォルダーの監視を後から足すので、その前の書き込みを取りこぼす。
  知らせが来るまでファイルを書き直すようにした（CI の ubuntu で落ちていた。ライブラリの動きは変えない）。

## [0.262.6] - 2026-11-08

### 修正

- `ito-tray`: フェイクのトレイのテストが、`.done` ファイルが作られた直後の空の状態を読んで落ちることがあった。中身が書かれるまで待つ。

## [0.262.5] - 2026-11-08

### 修正

- `ito-tabs`: `labels` のテストの Windows 形式のパス（`C:\dev\…`）を Windows だけで回す（Linux では `\` が区切りでなく落ちていた）。
- CI の clippy: Windows 向けを Linux の `cargo-zigbuild` ではなく `windows-latest` の別 job（`clippy-windows`）で回す（zig の msvc には libc のヘッダーが無く、`ring` のビルドで落ちた）。

## [0.262.4] - 2026-11-08

### 修正

- `ito-common`: `in_temp` が、存在しないパスを一時フォルダーの下と判定できないことがあったのを直した（一時フォルダーが短い名前（`RUNNER~1`）などで、親が存在しないとき。存在する一番近い親を解決してから残りを戻す）。
- `ito-common`: `apps` のテストが Linux で `\` 区切りの Windows のパスを使って落ちていたのを直した（テストだけ）。
- CI の clippy: Windows 向けを `cargo-zigbuild` で回す（素の `cargo clippy` だと `ring` が `lib.exe` を要して落ちる。`verify.sh` と同じ）。

## [0.262.3] - 2026-11-07

### 修正

- `scripts/lanes/keys.ps1`・`xrun.sh`: 使い回した `-Out` / `OUTDIR` に前回の成果物（`<State:>` の `*.txt`・`<Shot:>` の `*.png`・`*.json`・`xrun.sh` は `*.log`、`keys.ps1` は `isolate\`）が残り、今回の run が書けなかったものが今回の結果として読まれていたのを直した。走らせるたびに目印 `.keys-run` を置き、前回の成果物を消してから始める（消せなければ止まる）。スクリプトが作ったのでないフォルダー（`.keys-run` も `keys.done` も無く中身がある）は拒む（`keys.ps1` は exit 4、`xrun.sh` は exit 2）。説明に、成果物にならない最終行 `keys: failed`・`keys: unsettled` を足した。使う側は `scripts/lanes.sh sync` で写す。`xrun.sh` は最終行が `keys: done` でなくても exit 0 のまま（警告だけ）。
- `ito-common`: `access` のテストが、一時フォルダーの ACL（サンドボックス用アカウントや `Authenticated Users` の継承）に依存して落ちていたのを直した。Windows ではテスト用のフォルダーの継承を切って本人だけにし、対象ファイル自身の指摘だけを見る。コードは変えていない。

## [0.262.2] - 2026-11-06

### 変更

- `ito-confirm`: 直書きの余白・角丸のうち、スケールに同じ値があるもの（6・10・角 6）を `ito_theme::scale` の `space::SM`・`space::MD`・`radius::CONTROL` に置き換えた（#185）。見た目は変わらない。値が合うトークンが無いもの（8・14・2・5・ito-markdown の 13.5・タイトルバーのバッジ 10）は直書きのまま。

## [0.262.1] - 2026-11-06

### 修正

- `ito-prefs`: 「このアプリについて」で、ラベルの列の幅が確保されず、ラベルと値がくっついていたのを直した（ラベルの列を固定幅にした）。

## [0.262.0] - 2026-11-06

### 変更

- `ito-prefs`: 「このアプリについて」の行（Version・Build・System・License）の値を、右詰めをやめてラベルの列の右隣に左揃えで置いた。コピーの行は従来どおり右。使う側がやることはない。
- `ito-prefs`: License は表示のときだけ段落を再整形する（空行で区切られた段落内の改行を空白にして折り返しに任せる。`----` の罫線・`1)` のような番号付きの項・URL だけの行の改行は残す）。コピーされる文面は元のまま。

## [0.261.0] - 2026-11-06

### 修正

- `ito-prefs`: 「このアプリについて」の複数行の値（System・License）が 1 行ずつ右詰めになっていたのを、左揃えにした（#199）。

## [0.260.0] - 2026-11-05

### 修正
- アプリの雛形 `templates/app/scripts/verify.sh` の clippy の分岐を ito 自身の `scripts/verify.sh` と同じにした。以前は `*-linux-gnu:*`・`*-windows-msvc:*` が「どのホストでも素の clippy」になり、C の依存を足すと Windows などのホストから Linux ターゲットで落ちた。今はホストと同じターゲットだけ素の clippy、ほかは cargo-zigbuild を通す。
- 使う側がやること: 既存のアプリ（tsumugi など）の `scripts/verify.sh` は `lanes.sh sync` で配られないので、`case "$t:$host"` の 1 行目を `*-apple-darwin:*-apple-darwin | *-linux-gnu:*-linux-gnu | *-windows-msvc:*-windows-msvc)` に手で直す。

## [0.259.0] - 2026-11-05

### 変更
- 文字の大きさの直書きを `ito_theme::scale::Text` の役割に寄せた（#185）。`ito-confirm`（14 → `ChromeBody`、12 → `ChromeButton`、11 → `ChromeSmall`、diff の等幅 12 → `DialogKey`）、
  `ito-titlebar`（13 → `ChromeBody`、12 → `ChromeButton`、11 → `ChromeSmall`）、`ito-statusbar`（12 → `ChromeButton`／時計の等幅は `DialogKey`、11.5 → `DialogHint`）、`ito-guide`（12 → `ChromeButton`）。
  値が変わるのは `ito-confirm` の 14 → 13 だけ（ボタンの文字とカードの題が 1pt 小さくなる）。ほかは同じ値で見た目は変わらない。拡大（`scale_text`）では従来どおり動かない（`size()` を使う）。
  使う側がやること: なし。`ito-confirm` は `ito-theme` に直接依存するようになった。
  残し: `ito-markdown` の 13.5、バッジの 10（役割に当たる段が無い）。

## [0.258.0] - 2026-11-05

### 追加
- #198: `ito-palette` に `show_at(ctx, caret, &mut InlineView, entries, style)`。エディターのキャレットに付く補完の小さな一覧（`[[…]]` のリンクやパスの補完向け）。
  キャレットの矩形の下に開き、下に足りなければ上へ返し、窓の内に収める。8 行まで・選択に窓が付いてくる。`entries` は順位つきで `hits` 付き（ここでは絞らない）。
  取るキーは Up/Down・Ctrl+P/N・Enter/Tab（選ぶ）・Esc（`Answer::Close`）だけで、打鍵はエディターに残る。窓の外を押すと `Close`。候補が空なら何も描かず、キーも取らない。
  使う側がやること: 開く・閉じる（IME の変換中は閉じる）はアプリが決め、候補が変わったら `view.selected = 0` にする。色は `Style` をパレットと同じに渡す。
- 一覧の行の描画を `rows` に分け、パレットと共有した（見た目は変わらない）。

## [0.257.0] - 2026-11-05

### 追加
- #197: 新しいクレート `ito-paste`（貼り付けを賢く）。`clipboard_html()`（クリップボードの HTML。`Result<Option<String>, Error>`）、`html_to_markdown(html)`（見出し・段落・太字/斜体/取り消し線/コード・リンク・画像・入れ子のリスト・引用・コードブロック・表 → GFM・タスクの箱）、`tsv_to_table(text)`（表計算のコピー → GFM の表）。
  script/style は捨て、`javascript:` のリンクはリンクを外す。壊れた HTML・深い入れ子でも落ちない。
  使う側がやること: 貼り付けのときに `clipboard_html()` を（ワーカースレッドで）読み、あれば `html_to_markdown`、無ければプレーンテキストを `tsv_to_table` に通して `Some` なら表として貼る。

## [0.256.0] - 2026-11-05

### 変更
- #185 の続き（余白）: 段の真ん中で決められなかった値を決めた。`add_space(8)` は `SM`（6）へ、`add_space(20)` は `XL`（24）へ（`ito-prefs`）。2 pt 動く。密度を上げても読み分けられる側（Density with Clarity）に寄せた。
  `ito-confirm` は `ito-theme` に依存しないので触っていない。使う側は rev を上げたら、設定の画面を前後で撮って比べる。

## [0.255.0] - 2026-11-05

### 変更
- #185 の続き（余白）: 段にいちばん近い値の `add_space` を `ito_theme::scale::space` に寄せた（9・12 → `MD`、18 → `LG`、22 → `XL`。`ito-prefs`・`ito-find`）。1〜3 pt 動く。
  段の真ん中で決められない 8 と 20、細い隙間の 2、文字の大きさは、画面で比べてから決めるので触っていない。

## [0.254.0] - 2026-11-05

### 変更
- 角丸を `scale::radius` の段に寄せた（#185）。箱（`ito-notices` の一覧、`ito-prefs` のカード・キーの窓・絵文字・日付・返信の窓）の 10・12 → `BOX`（8）、`ito-prefs` の 7 → `CONTROL`（6）、行の強調・コードの角など 3 → `MARK`（4）（`ito-outline`・`ito-markdown`・`ito-minimap`）。丸みが 1〜4 pt 変わる。余白と文字の大きさは未着手。

## [0.253.0] - 2026-11-05

### 変更
- 内部: 角丸 4・6・8 と余白 4・6・10・16 の直書きを `ito_theme::scale` の `radius::{MARK, CONTROL, BOX}`・`space::{XS, SM, MD, LG}` に置き換えた（`ito-find`・`ito-guide`・`ito-markdown`・`ito-palette`・`ito-prefs`・`ito-statusbar`）。値は同じで見た目は変わらない。段に乗らない値（3・5・10 の角丸、2・5・8・12 の余白、文字の大きさ）は残してある（#185）。

### 修正
- `ito-notify`: テスト `toasts_are_written_and_numbered` が、通し番号を共有する別のテストと並ぶと落ちることがあったのを直した（テストだけ）。

## [0.252.0] - 2026-11-05

### 追加
- `ito-lineindex`（新しいクレート）: 大きなテキストファイルを、丸ごと読まずに行番号で読む。`LineIndex::open(path)` が別スレッドで 64 KiB ずつ読んで行頭の位置を覚え（1 行 8 バイト）、`lines(range)` が seek で必要な行だけを読む（mmap は使わない）。1 行は 8 KiB で切り（`Line::cut`）、全文は `full_line`。`grow()` は追記を拾い、切り詰め・書き直しは末尾 32 バイトの比較で `Grow::Rebuild`。`guess` が BOM・NUL の割合・UTF-8・EUC-JP・Shift_JIS で文字コードを推す（UTF-16 は 2 バイト単位）。`start_of`・`line_at`・`count`・`progress`・`is_done`・`cancel`。編集・色分け・内容の検索は含まない（#196）。

### 使う側がやること
- kura: `src/preview/logindex.rs` をこのクレートに置き換える（内容の検索は kura のまま）。weft: 8 MiB 以上のファイルに使う。

## [0.251.0] - 2026-11-05

### 追加
- `ito-mdedit`: `front_matter(text) -> Option<Range<usize>>`。先頭（BOM は読み飛ばす）の `---` から、閉じの `---` か `...` の行の改行までの範囲。閉じが無ければ `None`（先頭の `---` だけなら水平線のまま）（#195）。

### 修正
- `ito-mdedit`: 箇条書き・表の解析（`shift_lines`・`renumber` など）と `inline` の解析が、front matter を 1 つのブロックとして読むようにした。中の `1. a` や `**x**` が箇条書き・強調として扱われない（`ito-outline`・`ito-mdview` は前から対応済み）。

### 使う側がやること
- なし。front matter の色分けは、`front_matter` の範囲を `ito_syntax::Stream::for_token("yaml")` に渡す（#195 の残り: `ito-mdview` の「静かな属性ブロック」の見た目は未着手）。

## [0.250.0] - 2026-11-05

### 追加
- `ito-find`: `cmd::REPLACE_ONE`（`"replace.one"`）。`FindBar::command` が `Output::replace_one` を立てる。キーに割り当て、パレットに載せられる（#191）。
- `ito-image`: `Rgba::fit_width(max)`。縦横比を保ち、`max` 画素幅まで面積平均で縮める（透明な画素で色が暗くならないよう、アルファで重みを付ける）。すでに狭ければそのまま（#194）。
- `ito-outline`（機能 `egui`）: `column::split(rect, items, &look) -> Option<Split { text, column, cols }>`。右の目次の列が本文に重ならないよう、ペインを本文の矩形と列の矩形に分ける。`items` が空、またはペインが 40 セル未満なら `None`（本文を全幅で描く）。本文は `text` の上の子 `Ui` で `ito_mdview::show` すれば、返る `cols` が狭い幅になる。列は `show(.., split.column, .., overlay = false, ..)` で描く（#192）。

### 使う側がやること
- 列の `overlay = true` で本文を隠していたアプリは、`split` に替える。

## [0.249.0] - 2026-11-05

### 変更
- `ito-minimap`: 短いファイルは、ストリップ全体に引き伸ばさず、1 行 `LINE_H`（3pt）の自然な高さで上に寄せて描く（VS Code と同じ）。長いファイルは今までどおり全体に収める。窓なしの `map_height(lines, height)`。マップの下の空きは、クリック・ホバーに反応しない。全部の `Style` に効く。

### 使う側がやること
- なし（rev を上げるだけ）。

## [0.248.0] - 2026-11-05

### 追加
- `ito-minimap`（#193）: VS Code 風の描き方。`Look::style`（`Style::{Bands, Runs, Glyphs}`）で選ぶ。
  - `Runs`: 単語ごとに、その色の塊を並べる（語の間は空く）。`Glyphs`: 1 文字 1 セルで、インクの量（文字は濃く、記号は薄く）が濃さになる。
  - 窓なしの `runs(pieces, max_cols)` → `Run { start, len, color }` と `ink(char)`。
  - 1 行が 1 帯以上になるとき（行数が帯の数以内）だけ詳しく描く。それより長いファイルは `Bands` に戻る。見える帯の分だけ `line(i)` を呼ぶ。

### 使う側がやること
- `Look { row_h, font }` に `style` を足す（今までの見た目は `style: Style::Bands`、新しい見た目は `Style::Runs`）。`Runs`・`Glyphs` では `show` の `line(i)` が毎フレーム呼ばれる（行数ぶん。軽く返すこと）。

## [0.247.0] - 2026-11-05

### 追加
- `ito-keyscript::review`（#187）: Visual Review の道具。窓なしで、PNG と文字列だけで動く。
  - `Runner::shot` が `<名前>.png` の隣に `<名前>.json` を書く（`name`・`app`・`version`・`theme`・`lang`・`size`・`zoom`・`state`・`component`・`rect`・`time`・`pixels_per_point`）。アプリは `Runner::info`（`review::Meta`）に `app`・`version`・`zoom`・`state`・`component` を入れる。`theme`・`lang` は `<Theme:>`・`<Lang:>` を通ると自動で入る。
  - `review::compare(&old, &new, threshold)` → `Diff { fraction, bounds, image }`（変わった割合・変わった所を囲む矩形・赤で示した差分の画像）。`Image::{read, decode, encode, write}`。
  - `review::crop(&Image, [x, y, w, h])` と、点の矩形を画素にする `rect_pixels(rect, pixels_per_point)`。どの矩形かは今までどおりアプリ（`Dialect::shot_suffixes`）が決める。
  - `review::matrix(themes, states, enter, leave)`: テーマ × 状態を順に撮る台本の文字列を作る。
- `docs/design-principles.md`（Quiet Precision の原則・優先順位・役割の分担・完了の条件）と `docs/visual-review.md`（撮り方・比べ方・レビュー指示の雛形）。

### 使う側がやること
- 何もしなくても `<Shot:>` が JSON を書く。中身を正しくするには `runner.info.app`・`version` などを入れる。

## [0.246.0] - 2026-11-05

### 追加
- `ito-theme::scale`（#185）: 共通のサイズのスケール。文字は役割の名前で引く `Text`（`EditorText` 14・`EditorGutter` 12.5・`ChromeBody` 13・`ChromeButton` 12・`ChromeSmall` 11・`DialogTitle` 16・`DialogKey` 12・`DialogHint` 11.5・`PageTitle` 18）。`size()` は倍率前、`at(ctx)` は `set_text_zoom` の倍率つき。
  角は `radius::{BOX 8, CONTROL 6, MARK 4}`（箱は 1 つ）、余白は `space::{XS 4, SM 6, MD 10, LG 16, XL 24}`、高さは `height::{CONTROL 28, ROW 24}`。値は `docs/design-tokens.md` の表。
- `ito-mdview`（#188）: 機能 `egui` で、整形した `Doc` を egui に描く `show(ui, &Doc, &mut State, &Options) -> Output`。等幅のマス目に、見えている行だけを描く（見出し・コード帯・罫線・引用・インラインコード・太字・取り消し線・下線・`Placeholder`）。色は `ito_theme::colors()`、文字の大きさは `scale::Text::EditorText`。
  - `State::scroll_to_byte(byte)` で元の文章のバイトの行を先頭に出す。`Output::top_byte` が今の先頭のバイト。`Output::scrolled` は人のスクロールだけ true（`scroll_to_byte` の動きは false なので、エディターと互いに追いかけても戻らない）。
  - クリックは `Output::clicked_byte`（新しい `Doc::byte_at(line, col)`。印を落とした行では近い値で、正確ではない。呼ぶ側が文字の境目に寄せる）。
  - `Options::matcher` の一致を `match_bg`、`caret_byte` の行に `current_line`、その行の最初の一致に `match_current_bg`。
  - `Output::cols` が幅に入る桁数。`doc.body_cols` と違えば、アプリが（ワーカーで）その桁で `render` し直す。描くときに解析はしない。

### 使う側がやること
- `ito-mdview` を `features = ["egui"]` で読む。`render` は `output.cols` で。
- 新しく書く画面は直書きの数値でなく `scale` を使う。既存の直書きは部品を直すときに順に置き換える（ito 側もまだ置き換えていない）。

## [0.245.0] - 2026-11-05

### 追加
- `ito-find`（#190）: エディターの検索・置換バー。`FindBar`（Find / Replace の欄・正規表現 / 大文字小文字 / 単語 / 選択範囲の切り替え・`3 / 12`・`No results`・エラーの 1 行・`searching…`）と `show(ui, look, &Words) -> Output { query_changed, next, prev, replace_one, replace_all, closed }`。
  バーは文章を編集しない。置換は `FindBar::subst()`（`ito-match` の `Subst`）の `edits` でアプリが 1 回の undo にする。
  検索はアプリがワーカースレッドで走らせ、`version()` を付けて `set_result` で返す（古い結果は捨てる）。`ranges(text, selection)` で強調と次・前の一覧が取れる。
  Enter / Shift+Enter で次・前、Esc で閉じる（変換中は奪わない）。キーの名前は `cmd`（`find.open`・`find.next`・`find.prev`・`replace.open`・`replace.all`）。

### 使う側がやること
- 文言は `Words` で渡す。キーマップに `cmd` の名前を割り当てる。

## [0.244.0] - 2026-11-05

### 追加
- `ito-match`（#189）: 置換の編集の一覧。
  - `Subst::edits(text)` / `edits_in(text, range)`: `(バイト範囲, 置換後の文字)` を昇順・重ならずに返す（`$1`・`${name}` は展開済み。空の一致も含み、止まらなくなることはない。`edits_in` は範囲に収まる一致だけ・lookaround は範囲の外も見る）。
  - `Subst::from_query(query, rep, Query)`: `Matcher::new` と同じスマートケースと `(?m)`。`Query { regex, whole_word, ignore_case, all }` で文字どおり検索・単語単位・大文字小文字を選ぶ。
  - `nth_after(ranges, byte)` / `nth_before(ranges, byte)`: 一致の一覧をキャレットから次・前へ（端で回る）。

### 使う側がやること
- `rev` を上げるだけ。

## [0.243.0] - 2026-11-05

### 追加
- `ito-theme`（#186）: 名前のついた役割を `Colors` に足した。テーマのファイルのキーは増やさず、今の 12 色から出す（20 テーマと高コントラストで読める比をテストで守る）。`css_vars` にも出る。
  - 意味: `accent`（`run`）・`success`（`done`）・`warning`（`wait`）。
  - エディター: `gutter`・`gutter_active`・`current_line`・`caret`・`caret_idle`・`invisible`・`invisible_fill`・`ime_underline`。選択は今までの `selection()`。
  - 差分・検索: `diff_added`・`diff_modified`・`diff_removed`、背景の `diff_bg(mark)`、`match_bg`・`match_current_bg`（背景だけ）、`fold`。差分は色だけにせず形（`+`・棒）を添える。
- docs/design-tokens.md・docs/interaction.md。

### 使う側がやること
- `rev` を上げるだけ。アプリで `mix(dim, bg, 0.4)` などで代用していた所は、上の役割に置き換えられる。

## [0.242.0] - 2026-11-03

### 修正
- `ito-sftp`（#184）: 一発の `exec()` が、TERM とチャンネルの close を積んだ直後にランタイムを捨てるため、止める・時間切れのときにサーバーへ届かないことがあった（4 回に 1 回ほど）。
  今は切断を送って接続が閉じるまで（最大 2 秒）待ってから戻る。`Session::exec` は元から確実。
- 回帰テスト `ito-testserver/tests/ssh.rs` の `the_one_shot_exec_delivers_term_before_it_returns`（12 回繰り返し）。

### 使う側がやること
- `rev` を上げるだけ。止める・時間切れの `exec()` が最大 2 秒ほど長く待つことがある（相手が切断に応えないとき）。

## [0.241.0] - 2026-11-03

### 追加
- `ito-testserver`（#146 の第 3 段、dev-dependency だけ）: `ssh::SshServer`（russh）。
  - `start()`・`start_with(Config { host_key, login, allowed_keys, reject_all, silent })`、`KeyPair::generate()`（`private_openssh`・`public_openssh`・`fingerprint()`）、サーバーの `fingerprint()`・`addr()`・`port()`・`root()`・`local(remote)`。
  - 認証: パスワードと公開鍵、全部断る `reject_all`、つながるが黙る `silent`。
  - exec は台本: `on_command(部分文字列, Output::new().stdout().stderr().exit().signal("KILL").delay().chunk_delay().hang())`（バイト列なので UTF-8 も CP932 も。`hang` は signal かチャンネルの close まで待つ）。台本の無い `sha256sum -- '<path>'` は SFTP のフォルダーから答える（`Verify::Exec` が試せる）。ほかは 127。
  - `set_accept_env`、`deny_write(path)`・`allow_write()`、`set_damage(writes, reads)`（1 バイト壊して `Mismatch` を作る）。
  - 記録: `logins()`（パスワードは持たない）・`commands()`・`envs()`・`signals()`・`channels_closed()`・`log()`、`wait_until(timeout, f)`。
  - SFTP は一時フォルダーの上で put／get／ls／mkdir／rmdir／rename／remove。
  - 実物の `ito-sftp` で試した（指紋の固定と違い・認証・exec の出力と終了コードとシグナル・env・時間切れと停止・`.part` から名前替え・`Options::verify` の Exec／Readback・書けない場所）。
  - 足りないもの: `ito-testserver ssh` の CLI、本物のシェル。`ito-sftp` に mkdir／rename のクライアントが無い。
### 使う側がやること
- 無し（既存の API は変わらない）。koyomi は `ito-testserver` の `ssh` を dev-dependency で使える。ito のクレートは全部同じ `rev` にそろえる。

## [0.240.0] - 2026-11-03

### 修正
- `ito-mdedit`（#182）: `shift_lines` の Tab のあとに Shift+Tab を押すと元のバイトに戻る（`Spaces(2/4/8)`・`Tab`、LF／CRLF）。
  - Tab で 4 桁以上になり字下げコードに見える囲みを、Shift+Tab が囲みとして見つけ直し、開く行・本文・閉じる行を 1 つの塊として動かす。
  - 囲みの動く量は今までどおり（単位、または直前に動いた項目の量）。
  - 親の項目が上に無い項目（リストの最初の項目）は、字下げが 4 桁以上になる Tab で動かない（`None`）。`indent_item` も同じ。字下げコードにならない。
  - Tab は自分の結果に Shift+Tab を当てて、元に戻らないときは `None` を返す（Spaces で空白の中にタブがある行は除く）。
- `ito-mdedit`（#183）: `next_cell`・`format_table` などの「キャレットの表」は、囲み（``` と ~~~）と字下げのコードの中の表のような文章を表として整形しない（`None`）。

### 使う側がやること
- weft: 最初の項目への 2 回目の Tab が `None` になる（Tab を押しても動かない）。ito のクレートは全部同じ `rev` にそろえる。

## [0.239.0] - 2026-11-03

### 変更（壊れる）
- `ito-keys`（`egui`、#180）:
  - `Input` に `Ime(egui::ImeEvent)` を追加（`#[non_exhaustive]` ではないので `match` が壊れる）。`read` は IME の Preedit・Commit・Enabled・Disabled を、キーと文字と同じイベントの順に返す。
  - `from_egui` は数字キーの Shift を残す: Alt+Shift+5 は `<A-S-5>` で、`<A-5>` とは別。Ctrl+Shift+数字は `<C-N>` に当たらなくなる。
### 追加
- `ito-keys::read_for(events, now, took, mac)`（`read` は `cfg!(target_os = "macos")` で呼ぶ）。macOS では Option の文字（å・€・JIS の `\`）を、`took` が真を返したキー以外は捨てない。
- `read` は同じフレームの Alt+テンキーの数字（Alt を押したまま数字、Alt を離して Text）をキーにせず `took` にも渡さず、文字（`¥` など）を残す。フレームをまたぐ入力は見分けない。
### 修正
- `ito-outline`／`ito-mdedit`（#181）: 見出しの `section` の始まりを `#` でなく行頭にした。`> ## Q`・`- ## L`・先頭に空白の ` ## C` でも、行のどこにキャレットがあってもその見出しに効く（`delete_heading`・`insert_toc`・`next`・`promote_section`・`move_section` など）。
- `delete_heading` は `LineOnly`・`Section`・`LiftChildren` で引用／リストの印と空白も行ごと消す（`ToParagraph` は行が残るので残す）。
- `insert_toc` は挿入する行の改行を使う（改行が混ざった文章に別の改行を入れない）。

### 使う側がやること
- weft: `Input::Ime(e)` を `match` に足し、IME の確定をその順で入れる（IME を別に適用している処理を外す）。Alt+Shift+数字や Ctrl+Shift+数字のキー割り当ては `<A-S-N>`・`<C-S-N>` に書き直す。ito のクレートは全部同じ `rev` にそろえる。

## [0.238.0] - 2026-11-03

### 追加
- `ito-testserver`（#146 の第 2 段、dev-dependency だけ）:
  - `imap::ImapServer`: 偽の IMAP サーバー（`Mode::{Plain, ImplicitTls}`・LOGIN／AUTHENTICATE PLAIN・EXAMINE／SELECT・NOOP・FETCH／UID FETCH（`BODY.PEEK` も）。`add_message`・`add_mail`・`remove_message`・`set_uid_validity`・`renumber`、`drop_connections`、`commands`・`log`（パスワードは出さない））。実物の `ito-mail` で、新着・`From::After` での再開・UIDVALIDITY の変化・切断からの復帰・パスワード違い・フォルダー無しを試した。
  - `winrm::WinrmServer`: 偽の WinRM（`Mode::{Http, Https}`・Basic 認証・Create／Command／Receive／Signal／Delete）。`on_command(部分文字列, Output::new().stdout().stderr().exit().delay().hang())` で答えを決め、`requests`・`actions`・`commands`・`signalled`・`shells_open` で見る。実物の `ito-winrm` で、出力と終了コード・環境変数・パスワード違い・指紋違い・時間切れと停止を試した。
  - 足りないもの: WinRM の NTLM（Basic だけ）、IMAP の STARTTLS・IDLE・書き込み系。SSH／SFTP は第 3 段。
- `ito-mail`: `Stream` を公開し、`Inbox::open_with(connect, &Account, password, From)` を追加（`Inbox::open` は公開の CA に対する暗黙の TLS だけで、自己署名の偽サーバーに届かないため）。

### 使う側がやること
- 無し（既存の API は変わらない）。koyomi は `ito-testserver` の `imap`・`winrm` を dev-dependency で使える。ito のクレートは全部同じ `rev` にそろえる。

## [0.237.0] - 2026-11-03

### 修正
- `ito-markdown`（`egui`）: `Writing::apply(tool)` で押した道具の帯のボタンにフォーカスが移ったままだった。`show()` が保留の印を当てたら、`grab` の有無によらず書く欄へフォーカスを戻す（選び範囲は今のまま）。キーで当てた印は、もとから欄にフォーカスがあるので変わらない。

### 使う側がやること
- fumi: 押したフレームに印を残して `show()` の後で `pen.focus(ctx)` を呼ぶ回避（mail.rs の `pen_bar`・`pen_body`）を外せる。ito のクレートは全部同じ `rev` にそろえる。

## [0.236.0] - 2026-11-03

### 追加
- `ito-mdedit`: `shift_lines(text, sel, deeper, unit) -> Option<SelChange>` と `Unit::{Spaces(u8), Tab}`（#179）。Tab／Shift+Tab で、選択が触る全部の行を 1 つの `Change` で動かす。
  - 解析は 1 回（項目ごとに読み直さない）。1,000 項目・1 MB で 50 ms 未満（release）。選択を写し、向きを保つ。
  - 箇条書き・番号の項目は、入れ子の行ごと `indent_item`／`outdent_item` と同じ量。それ以外の空でない行は `unit`。空行は触らない。表の行は、動く項目の中にあるときだけ。
  - 動いた項目の直後の項目でない行は、その項目と同じ量で動く（Tab → Shift+Tab で元に戻る）。選択が行頭で終わるとき、その行は動かさない。一部だけ選んだコードのフェンスは、開きから閉じまで丸ごと動かす。CRLF は保つ。タブは幅 4 で読む（動かした行は空白になる。`Unit::Tab` を除く）。何も動かなければ `None`。

### 修正
- `ito-mdedit`: 10 番以降の番号付き項目で `indent_item`／`outdent_item` が入れ子と番号を壊していた（#178）。字下げは直前の行（新しい親）の内容の桁へ、戻すときは親の桁へ動かし、出た側と入った側の番号を振り直す。字下げ → 戻すで、番号も含めて元の文章に戻る。

### 使う側がやること
- weft: 複数行の Tab／Shift+Tab は自前の約 150 行を `shift_lines` に載せ替える。ito のクレートは全部同じ `rev` にそろえる。
- 注意: 4 桁以上に字下げした段落や項目は、インデント付きコードとして読まれることがある。

## [0.235.0] - 2026-11-03

### 追加
- `ito-testserver`（新、フェーズ 1。#146）: テスト用の偽のサーバー。cargo test の中で立てる（CLI は後）。127.0.0.1 の空きポートだけで待ち、受けたものを記録し、Drop で止まる。使う側は dev-dependency にだけ入れる。
  - `smtp::SmtpServer`: `Plain`・`StartTls`・`ImplicitTls`。AUTH PLAIN／LOGIN（合わなければ 535、未認証の MAIL は 530）。自己署名の証明書の指紋（`fingerprint`・`fingerprint_text`・`certificate_der`）。受けたメールは `.eml` と `messages()`／`wait_for_messages`。`reject_recipient`（550）・`cut_during_data`・`log()`（認証の中身は載せない）。
  - `http::HttpServer`: `route(method, path, Reply)`（状態コード・ヘッダー・本文・`delay`）、`requests()`／`wait_for_requests`。合わない道は 404。chunked と `Expect: 100-continue` に対応。TLS・keep-alive は無し。
  - `tls`: `Cert::generate`・`Fingerprint`・`pinned_client_config(fp)`（その指紋だけを信じる rustls の設定）。
- まだ無い: IMAP・WinRM（フェーズ 2）、SSH／SFTP（フェーズ 3）、CLI。

### 使う側がやること
- koyomi: SMTP と HTTP のテストに使う。`ito-notify::mail::send_message` は curl が証明書を検証するので、自己署名の偽サーバーには向かない（組み済みのメールは `ito-mail::compose` で作れる）。ito のクレートは全部同じ `rev` にそろえる。

## [0.234.0] - 2026-11-03

### 追加
- `ito-prefs`: 機能 `tabs` の設定の行 `tabs_rows(ui, look, &mut ito_tabs::Settings) -> bool`（#177）。変えたら `true`。
  - 3 行（タブの帯を出す・外から開いたファイル・起動時）。どれも分節のボタンで、`--keys` の `<Click:文字>` で押せる。ja / en。
  - 言語を渡すなら `tabs_rows_in(ui, look, TabsWords::of(lang), settings)`。`tabs_rows` は OS の言語で選ぶ。`TABS_KEYS` と `tabs_index` は設定の検索用。
  - 既定では `ito-tabs` を引かない。保存はアプリ（値はアプリごと、common.toml には入れない）。

### 使う側がやること
- weft: `ito-prefs` に `features = ["tabs"]` を足し、設定の画面で `tabs_rows_in` を呼ぶ。ito のクレートは全部同じ `rev` にそろえる。

## [0.233.0] - 2026-11-03

### 追加
- `ito-tabs`（新）: 文書のタブ（#175。サクラエディタ式）。窓なしのモデルと、機能 `egui` のバー。
  - `Tabs<Id>`: `open`（あれば前へ・なければ前面のタブの隣に）・`open_evicting`（上限で落とした id を返す）・`apply(Op)`（`Next`・`Prev`（回る）・`Close`・`CloseAt`・`Goto`・`Move`。閉じた id を返す）。上限の既定は 12。
  - `labels(paths)`: 同じ名前のファイルだけ親のフォルダーを足す（`docs/README.md`）。
  - `state_path(app)`・`to_toml`・`from_toml`・`load`・`save(&kept::Writer)`: `tabs.toml` への保存と復元（重複・上限超え・前面の範囲を直す）。
  - `ShowBar`（常に・2 つ以上で・出さない）・`OpenFromOutside`（新しいタブ・新しい窓）・`Reopen`（前回のタブ・渡されたファイルだけ）・`Settings`。
  - `cmd::{NEXT, PREV, CLOSE, GOTO}` と `command(name)`: `tab.next`・`tab.prev`・`tab.close`・`tab.goto_1`〜`9`。キーは固定しない。
  - `bar(ui, &[TabView { label, tooltip, unsaved }], front) -> Option<Op>`: 前面のタブは上に強調の線、未保存は `●`（ホバーで×）、×と中クリックで閉じる、ドラッグで並べ替え、狭くなると 96pt で止まり、ホイールか `‹ ›` で送る。バーは何も閉じない（`ito-confirm` で聞くのはアプリ）。タブの名前は ito-a11y に出る（`<Click:ラベル>`）。
- `ito-ipc::single`: 動いている窓へ引数を渡す単一インスタンス（#175）。`acquire(app, env, proto, &msg, wake)` が `Outcome::Owner`（扉を持つ。`try_iter`・`recv_timeout`）か `Outcome::Forwarded`（持ち主に渡した。終わってよい）を返す。`forward` は持ち主がいなければ `NotFound`。同時起動でも持ち主は 1 つ。`OpenRequest::parse(args, cwd)` が `+LINE:COL FILE` を読む。
- `ito-keys`: `read(events, now, took)` と `Input::{Key, Text}`（機能 `egui`、#176）。1 フレームのイベントをキーと文字の順に返す。Alt+文字の文字（Windows は `Key` のあとに `Text` も来る）は、キーマップが取ったキーと、Ctrl の無い Alt の和音で落とす。AltGr と Alt+テンキーは残す。Cut/Copy/Paste は `from_clipboard` で和音に戻す。egui の `quit_shortcuts`（Command+Q、Shift を見ないので Ctrl+Shift+Q）に注意と書いた。

### 使う側がやること
- weft: 文書のタブは `ito-tabs`、起動時の引き渡しは `ito_ipc::single::acquire`、Alt+文字は `ito_keys::read` を使う。ito のクレートは全部同じ `rev` にそろえる。
- ito-pane・ito-prefs の自前の `swallow_text`／`skip_text` を `read` に載せ替えるのは、あとで（動きは今のまま）。

## [0.232.0] - 2026-11-03

### 追加
- `ito-mdedit`: 選択範囲への書式の命令 `format`（#174）。すべて窓なしの純関数で、文章と選択のバイト範囲を渡すと `SelChange`（`change`・変更後の `selection`・`unflanked`）を返す。
  - `inline(text, sel, Inline::{Bold, Italic, Code, Strike, Link})`: 太字・斜体・コード・取り消し線・リンク。すでに付いていれば外す。コードの中は見ない。表は 1 セルずつ。複数行は行ごと。選択が空ならカーソルの単語、単語がなければ空の対（`**|**`）を入れる。CJK・CRLF でも安全。
  - `line_type(text, sel, Line::{Heading(0..=6), Quote, Fence, Bullet, Number, Task})`: 選択した行の種類。同じ種類をもう一度で外し、別の種類なら前の印を置き換える。番号は連番（最初の番号と `.`／`)` を保つ）。
  - `set_tasks`・`check_tasks`・`uncheck_tasks`・`toggle_tasks`: 選択内のタスクのチェック。

### 修正
- `ito-prefs`: `NotifyCfg::tell_open` の通知をクリックしたとき、最小化された窓（トレイに隠れた窓も）が戻らなかったのを直した。クリックの処理が `ViewportCommand::Focus` しか送らず、Windows ではそれだけでは最小化が解けなかった。`Visible(true)`・`Minimized(false)`・`Focus` の順に送る（#172）。
- `ito-outline`: `headings` が、見出しごとに文章の先頭から行番号を数え、さらに節の終わりを後ろへ探していたため、文章の大きさの 2 乗の時間がかかっていた（8 MB で 4.6 秒、見出しが密な文章で 71 秒。#173）。前の見出しからの改行だけを数え足し、節の終わりはスタックで閉じるようにして、大きさに比例する時間にした（2 MB の密な見出しで 31 秒 → 45 ms）。返す `Heading` は今までと同じ（旧い方法と突き合わせるテストを足した）。

使う側がやること: `rev` を上げるだけ（#172・#173 は API が変わらない）。#174 は、エディターのコマンド（ツールバー・キー）から `format` の関数を呼び、`SelChange::apply` か `change` を `ito-textbuf` の 1 手として適用し、`selection` をカーソルにする。`unflanked` が true のとき（`** a **` のように CommonMark では強調にならない）は、必要ならアプリが警告を出す。ito のクレートは全部同じ `rev` にそろえる。

## [0.231.0] - 2026-11-03

### 修正
- ito-mdedit: 表の整形と次のセル（`format_table`・`next_cell`）が、各行の先頭（字下げと引用の `>`）と改行（CRLF）を保つようにした。リストの項目の中の表が項目から外れず、CRLF の文章に LF が混ざらない。最後のセルのあとに足す行にも同じ先頭を付ける。混ざった文章では表の最初の行の改行に合わせる（#170）。
- ito-mdedit: リストの命令（`indent_item`・`outdent_item`・`toggle_task`・`move_item`・`renumber`）がコードブロック（フェンスと字下げ）の中の `- x` を項目と見なさず `None` を返すようにした。タブで字下げした項目も項目として扱う（字下げし直した行はスペースで書く）。`indent_item`・`outdent_item` は触れた番号つきのリストを数え直し、新しい入れ子のリストは 1 から始める（`1. a
2. b` の `2. b` を字下げすると `   1. b` になる）（#171）。

使う側がやること: 関数の形は同じなので `rev` を上げるだけでよい。`indent_item`・`outdent_item` が返す `Change` の範囲は、数え直した行の分だけ広がることがあるので、範囲を決め打ちしている所があれば `Change` の `range`・`text`・`cursor` をそのまま使う。`renumber` が返す `Change` の範囲は狭くなった（適用後の文章は同じ）。コードブロックの中では `None` が返るので、weft の Tab は空白の挿入に落ちる。ito のクレートは全部同じ `rev` にそろえる。

## [0.230.0] - 2026-11-03

### 追加
- `ito-fonts`: 日本語の面を egui の絵文字・アイコンの字体より前に置く、共通のフォントの組み立て（#167）。`find()`（システムの日本語・絵文字の面を読む。遅いのでワーカースレッドから）、`Faces`、`install(&mut FontDefinitions, &Faces, MonoFont)`（UI スレッドから）、`load_in_background(ctx, mono)`、並びだけを決める純関数 `order`。順番は、ラテンの面（アプリの面・同梱の等幅・egui の既定）→ 日本語の面 → 絵文字 → egui の `NotoEmoji`・`emoji-icon-font`。全角の括弧・数字・英字（`（Ａ１）` など）がアイコンの字体で描かれない。等幅（`mono_font = bundled`、Nerd Font なし）では日本語の面を 1.2 倍にして、全角 1 文字がちょうど 2 桁になる。`system` や Nerd Font（HackGen のような 1:2 の字体）では拡大しない。
- `ito-mdedit`: 目次・構造選択・構造を壊さない見出しの削除・構造ナビゲーション（#163）。
  - `insert_toc(text, caret)`: `<!-- toc -->` … `<!-- /toc -->` の目次を挿入する。すでにあれば今の見出しから作り直す（変化がなければ `None`）。アンカーは GitHub 流（重複は `-1`, `-2`）。
  - `expand_selection` / `shrink_selection`: 単語（日本語は句読点までの連なり）→ 文 → 段落・リスト項目・表のセル／行 → 表・リスト全体 → 節 → 親の節 → 全文。
  - `delete_heading(text, caret, HeadingDelete::{LineOnly, Section, ToParagraph, LiftChildren})`: どれも 1 つの `Change`。
  - `next(text, caret, Target::{Heading, List, Table, Code, OpenTask, Parent, Child}, Dir)`: 移動先のバイト。
  - 依存に `unicode-segmentation` を追加。
- 新しいクレート `ito-mdlint`（窓なし。依存は `ito-mdedit` だけ。#164）。Markdown のリントで、構造（見出しの飛び・空の見出し・重複・リストの字下げ／記号・表の列数・行末の空白・閉じていないフェンス・リンク先なし・未定義の参照）と日本語（全角スペース・全角英数・和欧文の空白・句読点・です／である調）を調べる。`lint(text, &Rules, exists)` が返す `Finding` に範囲・種類・重大度と、あれば直し（`Change`）が入る。`fix_all`/`merge` で複数の直しを 1 つの `Change`（undo 1 回）にできる。コード・front matter・リンク先・URL は見ない。CRLF 可。
- 使う側がやること:
  - `rev` を上げる（同じアプリの ito のクレートは全部同じ `rev`）。既存の API は変えていない。
  - フォント: 自前の `fonts.rs`（yagura・minato・michi・fumi・fude・weft・koyomi）を消し、`ito_fonts::load_in_background(ctx, mono)` を呼ぶ（または `find()` をスレッドで回して UI スレッドで `install`）。`install` は自分のフォントを足したあとに最後に呼ぶ。tsumugi・kura は日本語・絵文字・Nerd Font を足して先頭に置く部分だけ `Faces`（`nerd` は自分で埋める）と `install` に置き換える。
  - weft: `insert_toc`・`delete_heading`・`expand_selection`/`shrink_selection`・`next` をつなぐ。
  - `ito-mdlint = { git = "https://github.com/uchmk/ito", rev = "…" }` を足す。`Rules::default()` から始め、`Rules.spacing` の既定は `Any`（和欧文の空白は見ない）。`exists` には開いているファイルのフォルダーからの存在確認を渡す。

## [0.229.0] - 2026-11-03

### 追加
- `ito-common::text::grouped(n: u64)`: 3 桁ごとにカンマを入れた数の文字（`12345` → `12,345`、`0` → `0`）。言語によらず同じ。負の数は `format!("-{}", grouped(n.unsigned_abs()))`。(#168)
- `ito-textbuf::pos::count_chars(text, range)`: 範囲の中の「人が見る文字」（書記素クラスタ。キャレットの動く単位）の数。改行（`
`・`
`・単独の ``）は数えない。範囲の端は char の境界に引き戻し、テキストの外は切り詰める。(#169)
- 使う側がやること: 任意。minato の `thousands` と kura の `grouped` を `ito_common::text::grouped` に、weft の `count.rs` のループを `pos::count_chars` に置き換えられる。破壊的変更はなく、`rev` を上げるだけで使える。

## [0.228.0] - 2026-11-03

### 追加
- 新しいクレート `ito-image`（#162）。窓なし（egui に依らない）。
  - `clipboard_image() -> Result<Option<Rgba>, Error>`: OS のクリップボードの画像を素の RGBA8 で読む。画像が無ければ `Ok(None)`。Windows は `PNG`（アルファを保つ）→ `CF_DIBV5` → `CF_DIB` の順に Win32 で直に読む（Win+Shift+S のスクリーンショット）。macOS・Linux は `arboard`。
  - `dib::decode`: 16/24/32 ビット・パレット（1/4/8）・`BI_RGB`・`BI_BITFIELDS`・`BI_ALPHABITFIELDS`・上下どちらの向きの DIB を RGBA8 に。アルファの無い 32 ビットは不透明にする。RLE・JPEG・PNG 圧縮は `Unsupported`。
  - `Rgba { width, height, pixels }`（`content_hash`: FNV-1a 64・`is_opaque`・`unpremultiply`）。
  - `encode_webp(&Rgba, quality: f32) -> Result<Vec<u8>, Error>`: 透明を保つ非可逆 WebP（`quality` は 0〜100）。純 Rust（不透明は `oxideav-webp`、それが失敗か panic すれば・透明があれば `webp-rust`）。辺は 16383 まで。
  - `fake`: `--keys` の試験用の偽のクリップボード。`fake::set`・`set_png_file`・`clear`、または環境変数 `ITO_CLIPBOARD_IMAGE` に PNG のパス（読めなければ `Error`）。`<Paste>` は文字だけで egui に画像の貼り付けの事象が無いため、`--keys` の語は足さず、アプリの `Dialect` から `fake::set` を呼ぶ。
- 使う側がやること: weft は `ito-image = { git = "https://github.com/uchmk/ito", rev = "…" }` を足し、自前のクリップボードの画像の読みと WebP への変換をこれに置き換える。画像を読む・符号化するのは止まるので、ワーカースレッドから呼ぶ。

## [0.227.0] - 2026-11-03

### 追加
- `ito-ime`: IME がオンかオフかと入力モードを読む `Ime::status(ctx) -> Option<Status>`（`Status { open, mode }`、`Mode` は `Hiragana`・`FullKatakana`・`HalfKatakana`・`FullAlnum`・`HalfAlnum`・`Other`）を足した（#166）。Windows はキーボードの焦点がある窓の `ImmGetOpenStatus` と `ImmGetConversionStatus` を読む。ほかの OS・窓なし・IME なしは `None`。
- `Ime::fake_status(ctx, Some(Some(status)))` で `status` の答えを差し替えられる（状態欄のテスト用。`None` で元に戻る）。`mode_of(conversion)` は変換状態のビットから `Mode` を出す純関数。
- 使う側がやること: 状態欄に `Ime::status` の結果を出す。モードの言葉はアプリの `lang/*.toml` に置く。`--keys` から差し替える語（`<Ime:…>`）はまだ無い。

## [0.226.2] - 2026-11-03

### 追加
- `ito-openin`: 組み込みの行指定の表に `weft`（`+行:列`、列は文字数。micro と同じ形）を足した（#165）。`BUILTIN_EDITORS` は 21 個になった。
- 使う側がやること: 無し（kura は `rev` を上げるだけで、weft を行つきで開ける）。

## [0.226.1] - 2026-11-03

### 変更
- 依存の更新（Dependabot の cargo グループ、#153）を取り込んだ: serde_json・wry・notify・libc・resvg 0.49・encoding_rs・fancy-regex 0.20・unicode-width・harfrust・skrifa・tokio・base64 0.23。
- `pdf-writer` だけは 0.12 のまま（`ito-export` が使う svg2pdf 0.13 が 0.12 の型を使うため、0.15 だと型が食い違う）。svg2pdf が 0.15 に載ったら上げる。
- 使う側がやること: 無し（`Cargo.lock` の更新だけ）。

## [0.226.0] - 2026-11-03

### 追加
- `ito-mdedit`: `move_item(text, byte, Dir) -> Option<Change>`（#160）。キャレットのあるリストの項目 1 つを、入れ子の項目と続きの行ごと、隣の項目と入れ替える。`move_block` はリスト全体を動かす。
  - 最初／最後の項目、1 つだけのリスト、リストの外、種類（`-` と `1.`）の違う隣は `None`（アプリは `move_block` に戻せる）。キャレットは項目の中の位置のまま付いていく。
  - 番号つきのリストは番号をその場に残して中身を入れ替える（`renumber` は呼ばない）。タスク（`- [x]`）・全角・CRLF・タブの字下げに対応。
- `ito-textbuf`（#159）:
  - 行の索引: `Buffer::line_count`・`line_of`・`line_range`・`line_col`・`byte_at`（行頭の並びを持ち、編集・undo・redo で更新。`pos` の関数と同じ答えで、探すのは O(log n)）。
  - 保存の印が undo/redo に付いてくる: `Doc::mark_saved`・`Doc::is_dirty`。保存した文章に undo で戻れば clean。保存した点が履歴から消えれば dirty。`mark_saved` は打鍵のまとまりを区切る。
  - 続けた削除を 1 手に: `Doc::deleted(range, now_ms)`（`typed` と同じ時計・`pause_ms`・`seal`。Backspace と Delete は混ぜない。選択の削除は単独の 1 手）。
- 使う側がやること: 無し（追加だけ）。weft は自前の dirty の判定・行の索引・削除のまとめを、これに置き換えられる。

## [0.225.1] - 2026-11-03

### 修正
- `ito-outline`: `at_byte` が、文章の終わり（`byte == text.len()`）のキャレットを見つけられなかったのを直した（#161）。終わりまで続く節のうち一番深い見出しを返す。範囲の外は今まで通り `None`。使う側がやることは無い（「1 つ手前のバイトで聞く」回避策は外してよい）。

## [0.225.0] - 2026-11-03

### 追加
- `ito-openin`: 桁を渡せるようにした（#158）。
  - `Col { chars, bytes }`（1 から。`Col::in_line(行の文字, 文字の桁)` でバイトの桁を計算、`Col::new`）と `Pos { line, col }`（`Pos::line`・`Pos::new`）。エディターごとに数え方が違う（Vim の `cursor()` と Kakoune はバイト、VS Code・nano などは文字）ので、アプリは両方を渡し、ito が選ぶ。
  - `file_argv_at(template, path, Option<Pos>, custom)`・`open_file_at`・`Config::open_file_at(path, Option<Pos>)` は `(line_passed, col_passed)`。`file_argv`・`open_file`・`Config::open_file` は今のまま（行だけ）。
  - テンプレート（`[open]` の editor と `[line_args]`）に `{col}`（文字の桁）と `{bcol}`（バイトの桁）。桁を渡していないときは 1。
  - 組み込みの表: code 系 `-g f:L:C`、hx・zed・subl `f:L:C`、nvim・vim・gvim `+call cursor(L,C)`（バイト）、nano `+L,C`、emacs・emacsclient・micro `+L:C`、kak `+L:C`（バイト）、hidemaru `/jL,C`、sakura `-Y=L -X=C`、emeditor `/l L /cl C`、notepad++ `-nL -cC`。`vi` は昔の vi だと `:call` を知らないので行だけ。
  - `How::args(line, col)`・`How::token(line, col, path)`・`How::takes_col()`。`How` に `Cursor`・`Joined`・`Pair` を足した。
- `Built` に `col_passed`（桁を頼んだのにエディターが行しか取らないときだけ false）。`Vars` に `col`・`col_bytes`。
- 使う側がやること: `Built { … }` や `Vars { … }` を欄をすべて書いて作っているなら `col_passed`・`col`・`col_bytes` を足す（`..Default::default()` なら不要）。`How::Flag("+")` を nvim などの結果として比べているなら、`How::Cursor` などに変わった。実機で確かめたのは、これまで確かめていた sakura の `-Y=` だけ。ほかは各エディターの文書による。

## [0.224.1] - 2026-11-03

### 変更
- 文書: `docs/common-spec.md` の OAuth クライアントの節に、リフレッシュ トークンはそれを得たクライアントでしか使えないので、どちらでサインインしたかを覚えて同じクライアントで再開する、を足した（fumi からの指摘）。secret は User アカウントなのでリポジトリごとに登録する、と直した。
- 使う側がやることは無い。

## [0.224.0] - 2026-11-02

### 追加
- `ito-power`: `on_session_with(f: Fn(SessionEvent, &SessionInfo))`（#157）。`SessionInfo { id, user, remote }` で、どのセッションの出来事かを渡す。`user` は `DOMAIN\name`（ローカルのアカウントは `PC\name`）。サインアウトでは、システムが名前を忘れていることがあるので、そのセッションの前の出来事で覚えた名前を返す。`remote` はリモート デスクトップのセッションか。`on_session` は今のまま（`on_session_with` の上に載る）。ユーザー プロセスでも `ito-service` のサービスでも同じ。
- `ito-power::same_user(a, b)`: `PC\taro`・`.\taro`・`taro` を大文字小文字を区別せず同じ人として比べる（ドメインが違えば別）。
- 使う側がやること: なし（足しただけ）。

## [0.223.0] - 2026-11-02

### 追加
- `ito-oauth`: ビルドのときに入れる組み込みのクライアント（fumi の依頼）。`built_in_client!("UCHMK_GOOGLE")` が `UCHMK_GOOGLE_CLIENT_ID`・`UCHMK_GOOGLE_CLIENT_SECRET` を `option_env!` で読んで `Option<Client>` を返す（無い・空なら `None`）。`Client::from_build`・`Client::choose(own, built_in)`（取り込んだ自分のもの > 組み込み > なし）。
- ワークフローの雛形 `build.yml`・`release.yml`: release のビルドの手順の `env` に Actions secret `UCHMK_GOOGLE_CLIENT_ID`・`UCHMK_GOOGLE_CLIENT_SECRET` を渡す。fork の PR では空になり、組み込みなしでビルドが通る。
- `docs/common-spec.md` に「OAuth クライアント」の節（選ぶ順番・secret の置き場所）。
- 使う側がやること: アプリの crate で `built_in_client!` を呼び、`lanes.sh sync` でワークフローを写す。secret の登録は持ち主が GitHub の設定でする。

## [0.222.0] - 2026-11-02

### 追加
- 新しいクレート `ito-textbuf`: エディターが自分で持つ文書（`egui::TextEdit` を使わない）の土台（#156）。窓なし。
  - `Buffer`: 読んだままの文章（1 本の `String` なので `text()` は `ito-mdedit` にそのまま渡せる）、編集ごとに上がる `version()`、BOM と改行（`Eol`: `None`/`Lf`/`Crlf`/`Mixed`）を覚えて `saved()` で書き戻す。
  - `pos`: バイト・`char`・書記素・行と列の変換（CRLF・全角・絵文字）。
  - `Selection`: キャレットと anchor。
  - `Doc`: バッファ + 選択 + 履歴。`Transaction`（`Edit` の並び + 前後の選択 + `Effect`）が undo の 1 手。`Transaction::from_change` で `ito_mdedit::Change` をそのまま 1 手にできる。`typed(text, now_ms)` は間が `pause_ms` 以内で続きの場所なら 1 手にまとめる（改行・キャレットの移動・`seal()` で切れる）。`commit` は IME の確定を単独の 1 手にする。
  - `Effect`（`undo`/`redo`）: 画像の貼り付けで書いたファイルなど、文字以外の副作用。undo は逆順・redo は順に、文字に触る前に呼ぶ。失敗したら文書は元のまま。
  - 使う側がやること: `ito-textbuf = { git = …, rev = … }` を足す。マルチカーソル・矩形選択は無い。ロープではなく 1 本の `String`（8 MiB の途中への挿入は memmove 1 回）。

## [0.221.2] - 2026-11-02

### 修正
- `ito-tray`（Windows）: 右クリックのメニューが OS のダークモードに従わず、明るいまま出ていた（#148）。メニューを出す前に 1 回、uxtheme の `SetPreferredAppMode(AllowDark)` と `FlushMenuThemes` を序数で読んで呼ぶ（無い版の Windows では何もしない）。API の変更はない。

### 文書
- `CLAUDE.md`: デザインで迷ったら Google Stitch（`mcp__stitch__*`）で案を出して比べる、の一行を足した。

## [0.221.1] - 2026-11-02

### 修正
- `templates/app`: 空の一覧の `ito_prefs::empty_state` に `&` を付けた `String` を渡していて、Rust 1.99 の clippy（`needless_borrow`）で新しいアプリの `verify.sh` が落ちた（#154）。`&` を外した。
- `scripts/lanes/ito-first.py`: `cargo metadata` の出力を UTF-8 で読む。Windows（cp932）で `UnicodeDecodeError` になり、`new-app.ps1` が途中で止まって `chmod +x` の段が飛んでいた（#155）。
  使う側がやること: `scripts/lanes.sh sync` で写しを更新する。

## [0.221.0] - 2026-11-02

### 追加
- `ito-mdedit`: Markdown の構造の命令（Markdown エディター weft 向け、#3）。どれも文章とキャレットのバイトを渡すと `Change`（置き換える範囲・新しい文字・キャレットの行き先）を返す純関数で、
  窓もバッファも知らない（効かないときは `None`）。ブロックと節: `move_block`（見出しなら節ごと。見出しと front matter は越えない）・`move_section`（同じ段の兄弟と入れ替え）・
  `promote_section`/`demote_section`（下の見出しごと `#` を増減。下線の見出しと範囲外は `None`）。リスト: `indent_item`/`outdent_item`（入れ子ごと）・`toggle_task`（普通の項目はタスクにする）・`renumber`。
  表: `format_table`（全角は 2 桁で桁をそろえ、`:--:` の寄せを書き直す。`\|` とコードの中の `|` では割らない）・`next_cell`（`Step::Next`/`Prev`。Tab で隣のセルへ、最後のセルの先では行を足す）。
  `ito-outline` の見出しの節の範囲の上に作った。

### 変更
- `ito-syntax`: `Stream` が `Clone` になった（#4）。大きいファイルでは N 行ごとに複製を取っておき、編集した行の手前の複製から読み直せる（その行より後の複製は捨てる）。

## [0.220.0] - 2026-11-02

### 追加
- `ito-mdview`: Markdown の整形レイアウトを kura のプレビューから切り出した（#151、`Doc` を作る部分）。`render(text, cols, &Palette)` が段落の折り返し（日本語の禁則つき）・
  リストの印・引用の線・Alert・揃えた表・色分けしたコード（`ito-syntax`）を、セルの格子に並べた `Doc` にする。窓にも egui にも依らない。
  各行 `DocLine` が元の文章のバイトの範囲 `bytes` と行 `src` を持ち、`Doc::line_at_byte`（元のバイト → 描いた行）・`byte_at_line`（描いた行 → 元のバイト）・
  `src_for_line`・`line_for_src`・見出しの一覧 `headings`・検索の一致 `marks` で、エディターのスクロールの連動とクリックでのキャレットの移動ができる。
  数式・図（mermaid など）は `LineKind::Placeholder` の 1 行で場所を取る。色は `Palette`（機能 `theme` で `from_theme()` が `ito_theme::colors()` から作る）。
  egui の描画は別（この版にはない）。使う側がやること: kura は `preview/markdown.rs` を `ito_mdview::render` に替える。

## [0.219.0] - 2026-11-02

### 追加
- `ito-minimap`: kura のプレビューのミニマップを切り出した（#149）。`Row`・`row`/`rows`（`(text, color)` の並びから。kura の `Span` に依らない）、
  縮尺 `at_line`/`line_at_y`、`hover_card`/`hover_window`（`Area` で窓なしにテスト）。機能 `egui`: `show(ui, rect, body, rows, offset, on_screen, &Look, line)` が
  帯・枠・クリックとドラッグ・ホバーのカードを描き、`Shown::scroll_to`（行の真ん中に来る上の行）を返す。kura のテストを移した。
  使う側がやること: `MapRow` を `ito_minimap::Row` に、`minimap(lines)` を `rows(...)` に替え、`Look { row_h, font }` を渡す。

## [0.218.0] - 2026-11-02

### 追加

- `ito-outline` の機能 `egui`(#150 の描画の部分): 右の目次の列。`column::items(&headings, |h| h.line)` で行を作り、`column::show(ui, rect, &items, cols, top, cursor, overlay, &Look)` で描く。読んでいる見出し(`top` 以前で最後のもの)の強調、キーで動かすときのカーソル(`cursor`)、クリックで飛ぶ(押した行の番号を返す)、`overlay`。色は `ito_theme::colors()`。
- 幅の計算は egui なしでも使える: `cells`(全角は 2)・`column_cols`・`cols_for`・`current`・`first_row`。

### 使う側がやること

- 目次を出すアプリ(kura・Markdown エディター)は、`Look { cell, row_h, font, bold, title }` を渡す。`title`(「目次」)は呼ぶ側の言語で。kura は #150 の乗せ替えのとき、自前の `TocEntry`・`outline_cols` と描画を消す。

## [0.217.0] - 2026-11-02

### 追加

- 新しいクレート `ito-outline`(#150 の論理の部分): Markdown の見出しの一覧。`headings(text, max_level)` は見出しごとに `level`・`title`(記号なしの文字、空白は 1 つに)・`range`(見出し自身のバイト範囲)・`line`(0 始まり)・`section`(次の同じかそれより上の見出しまでの範囲)を返す。`at_byte` は位置を持つ一番深い節を引く。`options()` は kura と同じ読み方(表・脚注・取り消し線・タスク・GFM・YAML/TOML の前付け)。純 Rust(`pulldown-cmark`)。
- egui の描画(右の列・クリックで飛ぶ)は機能 `egui` として次の版で足す。

### 使う側がやること

- なし(新しいクレートを足しただけ)。

## [0.216.0] - 2026-11-02

### 変更

- 新しい文書 `docs/design-guide.md`: 「こういうときはこのクレート」の早見表(ボタン・アイコン・空の一覧・確認・パレット・Markdown の欄など)。`CLAUDE.md` と `templates/app/CLAUDE.md` からリンクした。
- `templates/app`: 新しい `--keys` の手順(`<Enabled:>`・`<Tree:>`・`<Announced:>` など)と、テスト用の口(`ito-notify/test-sink`、トレイは `ito-tray/test-hook`)を TODO に書いた。`Cargo.toml` に機能 `test-hooks`(`ito-notify/test-sink`)を足した。

### 使う側がやること

- 既存のアプリは何もしなくてよい。テストで通知を記録したいアプリは、同じ `test-hooks` の機能を足す。

## [0.215.0] - 2026-11-02

### 追加

画面を見ずに、スクリーンリーダーに渡る情報（状態・木・アナウンス）を `--keys` で確かめる(koyomi の依頼 #145)。

- `ito-keyscript`: 新しい手順。egui の AccessKit の木を読む。`<Assert:>` と同じく数秒待ってから落ちる。
  - `<Enabled:文字>`・`<Disabled:文字>`・`<Focused:文字>`・`<Checked:文字>`・`<Unchecked:文字>`: `<Click:文字>` と同じ見つけ方で 1 つに決め、その状態を確かめる。違えば「何であるか」（`unchecked`・`disabled`・`not a check box or switch` など）を添えて落ちる。
  - `<Tree:名前>`: 完了ファイルの横の `名前.txt` に木を書く。名前のあるノードを 1 行ずつ、親の下に字下げして `button "Go"`・`checkbox "Dark" checked`・`text value="hi" focused`・`button "Stop" disabled`。最後に `announced: …` の行。
  - `<Announced:文字>`: `ito_a11y::announce` された文言に含まれる。
  - 公開: `Step::{Is, Tree, Announced}`・`Press::{Is, Tree, Announced}`・`Prop`・`check_is`・`check_announced`・`tree_text`。`Item` に `focused`・`toggled` の欄。
- `ito-a11y`: `record_announcements(bool)`・`announced()`。`announce` の文言を覚える（既定は覚えない。`Runner` が `<Tree:>`・`<Announced:>` のある台本のときだけ入れる）。

### 使う側がやること

- `Step`・`Press` を `match` で全部書いているアプリは、新しい 3 つの腕を足す（`_ =>` で受けているなら不要）。`Item { … }` を自分で組んでいるなら `focused`・`toggled` を足す。
- 自前のフレームのループのアプリは、`Press::Is`・`Press::Tree`・`Press::Announced` に `check_is(ctx, prop, text)`・`tree_text(ctx)` を書き出す・`check_announced(text)` を当てる（`Runner` を使うアプリは何もしない）。
- アナウンスを確かめるアプリは、`ito-a11y` の `egui` の機能を入れて `announce` を使う。

## [0.214.0] - 2026-11-02

### 追加

トレイと通知を、画面なしで試すための口(koyomi の依頼 #144)。どちらも機能のスイッチを入れたビルドだけが環境変数を読む。出荷するビルドは読まない。

- `ito-tray`(機能 `test-hook`): `ITO_TRAY_STATE` か `ITO_TRAY_KEYS` があるとき、`Tray::new`・`new_with` はアイコンを作らず、偽のトレイを返す。
  - `ITO_TRAY_STATE=<file>`: 変わるたびに TOML を書き直す(`state`・`tip`・`[[menu]]` の `id`・`label`・`enabled`・`checked`、区切りは `separator = true`)。`state` は `State` の名前(`Waiting` など)、無ければ空。
  - `ITO_TRAY_KEYS=<script>`: `<TrayClick>`・`<TrayMenu:id>`・`<Wait:ms>` を空白で区切る。本物と同じく `Tray::events()` に `Event::Click`・`Event::Menu(id)` で届き、`wake` も呼ぶ。終わりに `<script>.done`(`ok`、読めない・書き間違いなら `error: …`)を書く。
  - `ito_tray::hook::{parse, render}` も公開。
- `ito-notify`(機能 `test-sink`): `ITO_NOTIFY_SINK=<file>` があるとき、通知を出さず音も鳴らさず、ファイルに足していく。
  - `toast::show`・`Notifier::show` は `[[toast]]`(`n`(1 から)・`app`・`title`・`body`・`tag`・`launch`・`actions = ["ラベル=arg"]`)、`sound::play` は `[[sound]]`(`name`)。
  - クリックは `<file>.click` に 1 行ずつ書く: `n`(通知そのもの)か `n:arg`(ボタン)。`Notifier::new` に渡した `click(tag, arg)` が、本物のクリックと同じに呼ばれる(`n` は `launch`)。読んだら空にする。0.2 秒ごとに見る。

### 使う側がやること

- テスト用のビルドにだけ、`ito-tray = { …, features = ["test-hook"] }`・`ito-notify = { …, features = ["test-sink"] }` を足す(`[features]` でアプリの機能 1 つにまとめるとよい)。通常のビルドは何も変わらない。
- `Notifier::new` を通さず `toast::show` だけ使う通知は、記録されるがクリックは呼ばれない(呼ぶ先が無い)。

## [0.213.0] - 2026-11-02

### 追加

- `ito-common::now`(機能 `clock`): 「いま」を 1 か所に寄せる(koyomi の依頼 #143)。`now()`(UTC)・`now_local()`・`fake_notice()`。
  - 通常のビルドは `Utc::now()` / `Local::now()` と同じ。
  - 機能 `fake-clock` のビルドだけ、環境変数 `UCHMK_FAKE_CLOCK=<file>` の TOML(`now = "2026-12-31T23:59:50+09:00"`)を読む。その時刻から実時間で進み、ファイルは 1 秒に 1 回まで読み直して、`now` が変わったらその時刻に飛ぶ(進める・戻す)。OS の時計を動かさずに「1 時間戻す」「日をまたぐ」を回せる。
  - `fake-clock` を入れないビルドは変数を読まない(出荷版の時刻はずらせない)。`Instant` には触らない。`fake_notice()` は偽の時計のときだけ「fake clock: …」の 1 行を返す(ログの先頭用)。
  - 使う側がやること: `Utc::now()` / `Local::now()` を `ito_common::now::now()` / `now_local()` に寄せ、試験用のビルドだけ `ito-common` の機能 `fake-clock` を付ける。`UCHMK_FAKE_TZ` は後回し。

## [0.212.0] - 2026-11-02

### 追加

- `ito-prefs`: 印 `Mark::Database`(重ねた円柱。ほかの印と同じ線の太さ・テーマに従う)。`Mark::ALL` は 79 個になった(koyomi の依頼 #142)。
  - 使う側がやること: `Mark` を `match` で網羅しているなら `Database` を足す。

## [0.211.0] - 2026-11-02

### 追加

- サインイン・サインアウト・ロック・ロックの解除を知る（koyomi の依頼 #141）。
  - `ito-power`: `SessionEvent { SignIn, SignOut, Lock, Unlock }` と `on_session(f) -> Result<SessionWatch, WhyNot>`（戻り値を drop すると止まる。`f` は別スレッドから呼ばれる）。
    - 本人のセッションで動くプロセス: 隠したメッセージ専用の窓で `WTSRegisterSessionNotification`。そのセッションの出来事。
    - `ito-service` のサービス（session 0）: SCM の `SERVICE_CONTROL_SESSIONCHANGE`（`SERVICE_ACCEPT_SESSIONCHANGE`）を `ito-service` が渡す。session 0 以外のすべてのセッションの出来事。どちらでも同じ呼び方。
    - macOS・Linux は `Err(WhyNot::Unsupported)`。
  - `ito-power::user_idle_for()`: コンソールのセッションの人の空いている時間を `WTSQuerySessionInformation`（`WTSSessionInfoEx`）から読む。サービスからでも読める。
    **この PC では `LastInputTime` が 0 で `None` だった**（Windows が持つのはリモート デスクトップなど一部のセッションだけ）。`None` は「分からない」。分かるのが要るなら、本人のセッションのプロセスが `idle_for` を知らせる形（別の依頼）になる。
- `ito-service`: `SERVICE_ACCEPT_SESSIONCHANGE` を受け付け、`run` が `ito-power` をサービスの形にする（`#[doc(hidden)]` の `set_service_mode`・`service_session_change`）。`ito-service` は `ito-power` に依る。

### 変更（使う側がやること）

- 何もない（足しただけ）。`ito-service` を使うアプリは、同じ `rev` の `ito-power` も読み込まれる（同じ `rev` にそろえる）。

## [0.210.0] - 2026-11-02

### 追加

- 送る・取るの途中を確かめてから名前を替える（koyomi の依頼 #147）。違えば `.part` を消し、本来の名前には何も置かない（上書きのときは元のファイルが残る）。
  - `ito-sftp`: `Options::verify`（`Verify::None`（既定）・`Exec`・`Readback`）。
    - `Exec`: 相手に `sha256sum` を走らせる。Windows のサーバーは PowerShell の `Get-FileHash` に切り替える。
    - `Readback`: SFTP で読み戻して計算する。コマンドを走らせられない相手（`ForceCommand internal-sftp`）でも使える。読み戻しの分は進みに数えない。
    - 送るとき、`.part` を書き終えてから比べ、合ってから名前替え。取るとき、`Exec` は先に相手のハッシュを取り（食い違いならダウンロードもしない）、`Readback` は受け取った後に読み戻す。
    - `Error::Mismatch { name, ours, theirs }`（中身が違う）と `Error::VerifyUnavailable(理由)`（`Exec` で相手のハッシュを取れない。`Readback` を勧める目印）。
  - `ito-cloud`: S3 のチェックサム（`x-amz-checksum-sha256`）。`Options::checksum`（既定 true）。
    - 送る: 1 リクエストなら本文の SHA-256 を付け、マルチパートは Create に `x-amz-checksum-algorithm: SHA256`、各パートと Complete にパートごとのチェックサムを付ける。ストアが `BadDigest` などで断れば `Error::Mismatch`（マルチパートは Abort）。
    - 取る: `x-amz-checksum-mode: ENABLED` を付け、相手が返した全体の SHA-256 を受けながら計算した値と比べる。違えば `Error::Mismatch` で `.part` を消す。マルチパートで作られた物（`-N` つき）やチェックサムの無い物は比べられず、`verified: false`。
  - `ito-cloud::fake::FakeS3`: `corrupt_object`・`damage_in_transit`・`checksum(key)`（試験用）。
- `ito-sftp` の試験用の SSH サーバーが `sha256sum` に答える・断る・書き込みや読み出しで壊す。

### 変更（使う側がやること）

- `ito-sftp`: `Options { overwrite }` に `verify` が増えた。`Options { overwrite: true, ..Default::default() }` に直す。`Moved` に `sha256: Option<String>` が増えた（`Moved { … }` を自分で作っていれば足す）。`Error` に `Mismatch`・`VerifyUnavailable` が増えた（`match` を網羅しているなら足す）。
- `ito-cloud`: `Options` に `checksum: bool` が増えた（`..Default::default()` を使っていれば何もしない）。`put`・`get` は `u64` でなく `Moved { bytes, sha256, verified }` を返す（`.bytes` を使う）。`Error` に `Mismatch(String)` が増えた。
  `verified` が `false` のときは、相手が検査しなかった（取る側でチェックサムの無い物）か、`checksum: false` にした。

## [0.209.0] - 2026-11-02

### 追加

- 新しいクレート `ito-cloud`（#140）。S3 互換のオブジェクト ストレージ（Amazon S3・MinIO・Cloudflare R2 など）にファイルを送る・取る・一覧・消す、ブロッキングの API（ワーカースレッドから呼ぶ）。
  - `put(&Target, local, key, &Options, &cancel, progress)`: `Options::part_size`（既定 16 MiB、下限 5 MiB、1 万パートに収まるよう自動で上げる）以下は 1 リクエスト、それより大きいと
    マルチパート。パートは 5xx・429・408・通信の失敗で最大 3 回やり直す。失敗・停止・時間切れでは中途半端なアップロードを Abort する。`key` が空か `/` で終わるとファイル名を足す。
  - `get(&Target, key, local, …)`: `<名前>.part` に書いて、全部届いてから名前替え。失敗すれば `.part` も消す。
  - `list(&Target, pattern, &Options, &cancel)`: `*` `?` のパターン（`*` は `/` にも一致）。最初のワイルドカードまでを prefix にして、全ページを辿る。
  - `delete(&Target, key, …)`: 無いキーを消してもエラーにしない。
  - SigV4（本文の SHA-256 も署名）。`Target::S3 { bucket, region, endpoint, path_style, key_id, secret, session_token }`。`Debug` は秘密を隠す。秘密は呼ぶ側が `ito-secret` から読んで渡す。
  - `Error::{NotFound, Auth, Status, Network, Tls, Cancelled, Timeout, Io}` と `retryable()`。全体の時間切れ（`Options::timeout`）と 1 リクエストの時間切れ（`request_timeout`）、止める旗。
  - 機能 `test-util`: `fake::FakeS3`（127.0.0.1 の小さな S3。署名と本文のハッシュを確かめ、失敗の注入・遅延・ページ分けができる）。
  - HTTP は `ureq`（rustls + ring）。Linux・macOS 向けのビルドは C が要る（`ito-sftp` と同じ）。
  - 使う側がやること: 依存に `ito-cloud` を足す。Amazon 以外は `endpoint` を `https://host[:port]` で渡し、MinIO は `path_style: true`。

### 変更

- `ito-match`: `glob_match`・`has_wildcard`（`*` `?` のファイル名の規則）を `ito-sftp` から移した。
- `ito-sftp`: 上の 2 つは `ito_match` の再エクスポート（`ito_sftp::glob_match` はそのまま使える）。

## [0.208.0] - 2026-11-02

### 追加

- 新しいクレート `ito-sql`（#139）。SQL Server・PostgreSQL・SQLite で SQL のスクリプトを走らせる、ブロッキングの API（ワーカースレッドから呼ぶ）。
  - `run(&Target, script, out: Option<&Path>, &cancel, timeout) -> Result<Outcome, Error>`。バッチごとに実行し、変えた行数（`rows_affected`）と
    最後の結果セットを CSV（`ito-table`: BOM・CRLF・`=` `+` `@` の保護）で返す。止める旗と時間切れで、接続を捨ててサーバに取り消させる。
  - `Target::{SqlServer, Postgres, Sqlite}`、`SqlAuth::{Windows, Login}`（Windows 認証は Windows のみ）、`Tls::{Disable, Require, Verify}`。`Debug` はパスワードを隠す。
  - `split_batches(script, Dialect)`: SQL Server は `GO` だけの行、PostgreSQL・SQLite は引用符・コメント・`$tag$`・`CREATE TRIGGER … END` の外の `;`。空のバッチは捨てる。
  - 機能 `sqlserver`（tiberius）・`postgres`（tokio-postgres + rustls）・`sqlite`（rusqlite、bundled）。非同期は `run` の中の専用ランタイムに閉じる。
  - SQL Server と PostgreSQL の実機での確認は koyomi に頼む（テストは SQLite と分割まで）。
  - 使う側がやること: 依存に `ito-sql` を足す。SQL Server で `out` を渡すと、`SELECT`／`EXEC` を含むバッチは流して読むので、その分の行数は `rows_affected` に入らない。

## [0.207.0] - 2026-11-02

### 追加

- `ito-oauth`: `Error::rate_limited() -> Option<Duration>`。429、または Google の JSON の本文（`error.errors[]` / `error.details[]` の `reason`）が
  `rateLimitExceeded`・`userRateLimitExceeded`・`RATE_LIMIT_EXCEEDED` の 403（Gmail は利用者ごとの枠を 403 で返す）のとき、待つ長さを返す（`Retry-After` が無ければ 60 秒）。
  `retryable()` もその 403 で true になる。文字の検索ではなく JSON を読む。エラーの本文の保持を 2000 から 8000 文字に増やした（JSON が途中で切れないように）。

## [0.206.0] - 2026-11-02

### 追加

- `ito-notify::mail::send_message(&Mail, to, message, timeout)`: 組み済みのメッセージ（`ito_mail::compose` の出力、添付つき）を、`Mail` のサーバーとログインで `to` に送る（uchmk/ito#138）。
  呼ぶスレッドで curl が終わるまで待つ（ワーカースレッドから）。やり直しは呼ぶ側。差出人は `mail.from`、宛先は `mail.to` ではなく引数。
  失敗の文は curl の言葉か終了コードで、パスワードは出ない。宛先なし・アドレスでない宛先・空のメッセージは curl を呼ぶ前に断る。
  添付の個数・大きさの上限は呼ぶ側が決める（`send_composed` は作らない）。

## [0.205.0] - 2026-11-02

### 修正

- `ito-common`: 倍率の違うモニターの間で Win+←/→ が、隣のモニターに半分かかったところで止まるのを直した（uchmk/ito#136）。
  winit は `WM_DPICHANGED` で Windows の提案の矩形ではなく自分で決めたサイズに置き直すので、スナップが途中で止まっていた。
  `window::follow_dpi(&window)` が、winit の処理のあとで提案の矩形を当てる（Chrome と同じ）。`Window::put`・`Saved::put` が呼ぶので、
  すでに `put` を呼んでいるアプリは `rev` を上げるだけでよい。呼んでいないアプリは eframe の creator で `follow_dpi(cc)` を呼ぶ。Windows 以外では何もしない。

## [0.204.0] - 2026-11-01

### 追加

- `ito-markdown`: 書く欄（`Writing`）で、リストの行の Enter が次の項目を作る。`- `・`* `・`- [ ] `（チェックは外した状態で続く）・`N. `（次は `N+1. `）。
  中身が空の項目で Enter を押すと印が消えてリストが終わる。字下げは引き継ぐ。印の途中にカーソルがあるとき・リストでない行・Shift/Ctrl+Enter は今までどおり。
  純粋な関数 `continue_list(text, cursor) -> Option<(String, usize)>`（文字の位置）も公開。使う側がやることは無い。

### 変更

- `ito-prefs`: `Mark::Numbers` は線の横に数字の 1 2 3 を描き、`Mark::Tasks` は 3 つの箱（先頭にチェック）にした。ツールバーで一目で見分けがつく。

## [0.203.0] - 2026-11-01

### 変更

- `ito-http` の tailnet の口を、tailscaled に届かなくても `Server::start` が失敗しないようにした（koyomi から）。本体の口（loopback・LAN の TLS）は先に開き、
  tailnet の口は開けるまで裏で間をおいて（2 秒から倍々で最大 1 分）試し続ける。最初の 1 回は `start` の中で試すので、Tailscale が動いていれば今までどおり
  `start` の直後に `Server::tailnet_port()` が `Some` になる。開くまでは `None`。
- 追加: `Server::tailnet_error() -> Option<String>`（開けない理由。開けたら `None`）。
- 設定の誤り（ログインもタグも無い・空の名前・tailnet でないアドレス）は今までどおり `start` が `Err`。
  使う側がやること: `tailnet_port()` が `None` のままでも起動を成功として扱い、`tailnet_error()` を画面に出す。

## [0.202.0] - 2026-11-01

### 追加

- `ito-mail::Attachment::cid`（fumi からの依頼）。`Some("logo@x")` を付けて `html` の `<img src="cid:logo@x">` から参照すると、`compose` はそのファイルを
  添付ではなく `multipart/related` の中の `Content-ID`・`Content-Disposition: inline` の部品にする（`multipart/alternative` の text/html の側に入れる）。
  `html` が無い・id が空のときは今まで通りの添付。`ito-markdown` は変更なし（`cid:` は通り、`to_html` の `image` 関数で解ける）。

### 変更（使う側がやること）

- `Attachment { … }` に `cid: None` を足す（インラインにしないもの）。

## [0.201.0] - 2026-11-01

### 追加

- `ito-http` の tailnet の口（uchmk/ito#135）。`Config::tailnet = Some(Tailnet::new(port).login("alice@example.com", "owner").tag("tag:koyomi-ha", "standby"))`。
  - tailnet のアドレス（`100.64.0.0/10`・`fd7a:115c:a1e0::/48`）だけに bind した平文 HTTP の口を、loopback／LAN の口に並べて足す。`addrs` を空にすると tailscaled の status から自分のアドレスを使う。tailnet の外のアドレスは `start` が断る。
  - token の無い要求は、接続元を tailscaled の LocalAPI の whois で確かめ、タグ付きの端末はタグ、ほかはログイン名で `Tailnet` の一覧に引いて `Request::who` を決める。一覧に無い・知らない相手・tailscaled に届かない（3 秒）は 401。token は今までどおり併用できる。token が空でも `tailnet` があれば `start` できる。
  - whois で通る要求はクッキーと同じ扱い。変える要求（`GET`・`HEAD`・`OPTIONS` 以外）は `X-Ito-Csrf: 1` が無ければ 403、`Host` はこの PC の tailnet のアドレス・MagicDNS の名前（と `Tailnet::hosts`）に限る（無ければ 403）。bearer の要求には課さない。
  - `LocalApi`（Windows は名前付きパイプ、Unix は tailscaled のソケット。Windows の通常ユーザーで動くことを確かめた）、差し替えられる `Whois` の trait、`is_tailnet_ip`、`Server::tailnet_port()`。
- `LogLine` に `who` と `via: Via`（`Token`・`Session`・`Tailscale`・`Public`・`Denied`）。

### 使う側がやること

- `LogLine { … }` を自分で組み立てているなら `who`・`via` を足す（ログを読むだけなら要らない）。`Config { … }` を欄ごと書いているなら `tailnet: None` を足す（`Config::new` なら要らない）。
- koyomi: `[[http.user]]` にログイン名／タグを書けるようにし、`Config::tailnet` に渡す。

## [0.200.0] - 2026-11-01

### 追加

- `ito-prefs::Mark::Desktop`（78 個に）。画面と台の印（Windows の PC。ラックの `Server` に対して）。koyomi の winrm の部品の棚用。

### 使う側がやること

- `Mark` を網羅して `match` しているなら、`Desktop` の腕を足す。

## [0.199.0] - 2026-11-01

### 追加

- `ito-winrm` に NTLMv2 のサインイン。`Auth { Basic, Ntlm }` と `Target::with_auth(Auth)`（既定は `Basic` で今までどおり）。
  `Authorization: Negotiate <message 1>` を空の本文で送り、401 の `WWW-Authenticate` の message 2 から message 3 を作って本物の要求を送る（サーバーの書き方に合わせ `Negotiate`／`NTLM`）。
  つながっている間は 2 つ目以降の要求にヘッダーを付けない（つなぎ直したらやり直す）。署名・封印・MIC はなし（HTTPS 前提）。純 Rust（`md4`・`md-5`・`hmac`）。
  MS-NLMP のテスト ベクター（NTOWFv2・NTProofStr）と、モックの握手で確かめた。Kerberos と `Auth::Negotiate` は未実装（ドメインの環境ができてから。uchmk/ito#134）。

### 使う側がやること

- NTLM で入るなら `Target::https(..).with_auth(Auth::Ntlm)`。ユーザーはドメインなら `DOMAIN
ame`、ローカル アカウントなら `name`（`name@realm` は ドメイン無しの名前のまま）。HTTPS で使う。
- `Basic` のままなら何も変わらない。`Target` の欄が 1 つ増えたので、`Target { .. }` を直接組み立てているなら `auth: Auth::Basic` を足す。

## [0.198.0] - 2026-11-01

### 追加

- `ito-markdown::Writing` に `grab: bool`（既定 true で今までどおり）と `focus(&Context)`。`grab = false` のあいだは、箱にフォーカスがある（またはプレビュー中）ときだけ
  Ctrl+Enter・Ctrl+P/B/I/K/U・Esc を消費し、フォーカスを取り返さない。他の欄と同居するアプリ向け。
- `ito-prefs::Mark` に `Heading`・`Quote`・`Code`・`Bullets`・`Numbers`・`Tasks`（77 個に）。

### 使う側がやること

- fumi・minato: `rev` を上げる。`Mark` を全部 `match` している所があれば 6 つ足す。同居する欄があれば `writing.grab = false` にし、箱を使い始めるとき `focus` を呼ぶ。

## [0.197.0] - 2026-11-01

### 追加

- `ito-markdown` に機能 `egui`（#133 の 2 つ目。`egui` と `ito-theme` を足すだけ。機能なしは今までどおり依存なし）。
  - `layout(&Painter, text, width, size) -> Md`・`Md::paint(&Painter, x, y)`・`Md::height`: プレビューの描画。見出し（大きい 2 つは下線）・箇条書き・番号・タスク・引用・コード・表・罫線・`<summary>`・段落、印は太字・斜体・下線・打ち消し・コード・リンク。色は `ito_theme::colors()`（強調は `blue`、地は `side`）。
  - `Writing`（`new(id)`・`text`・`preview`・`apply(Tool)`・`show(ui, field_rect, empty, size) -> Out`）: 渡された矩形に書く欄（複数行・バーなし・キャレットを追う）かプレビュー（ホイールで上から送る）を描く。Ctrl+B / I / K / U が選択範囲に印を付ける（ツールバーのボタンは `apply` で同じ道を通る）、Ctrl+P がプレビューの切り替え、Ctrl+Enter が `Out::Send { close: false }`、Ctrl+Shift+Enter が `Send { close: true }`、Esc はプレビューを閉じ、閉じていれば `Out::Leave`。ショートカットは欄に届く前に取る。
  - 枠・見出し・ツールバー・ボタン・ピッカー・キー表は載せない（アプリの側）。Alt+文字のキー（minato のピッカー）もアプリの側で、`show` の前に取る。

### 使う側がやること

- minato: `md_ui.rs` を `ito_markdown::{layout, Md}` に置き換える（検索語の色付け `layout_lit` と選択のコピー `select::note` はまだ無い。要るなら `crate-request`）。`compose_ui.rs` の欄の部分を `Writing::show` に替える。`Compose.text`・`preview`・`tool` は `Writing` が持つ。
- fumi: `Writing` を欄に使い、メール本文は `to_html` で作る。

## [0.196.0] - 2026-11-01

### 追加

- 新しいクレート `ito-markdown`（#133 の 1 つ目。依存なし・UI なし）: 書く欄の Markdown。minato の `composer.rs` から切り出した。`wrap`・`link`・`Tool`（`Tool::ALL` は 13 個。どれを出すかはアプリが選ぶ）・`tool(text, a, b, Tool)`（選択範囲に印を付ける。下線は `<u>…</u>`、打ち消しは `~~`）、読む側の `blocks`（段落の改行は空白に畳む）・`blocks_hard`（段落の中の改行を残す）・`Block`・`spans`・`Span { bold, italic, strike, underline, code, image, link }`・`html_inline`。
- 書き出し: `to_html(text, &|src| …)`（メール本文向け。見出し・箇条書き・タスク `☐`/`☑`・引用・コード・表・罫線・段落、段落の中の改行は `<br>`、本文は全部エスケープ、`javascript:`・`vbscript:`・`data:` のリンクは `#`、画像の `src` は呼ぶ側の関数で `cid:` などに替える）と `to_plain(text)`（印を落とし、`- `・`1.`・`> ` は残し、リンクは `text (url)`）。
- まだ無いもの: egui の書く欄とプレビューの描画（#133 の 2 つ目。minato の `md_ui.rs`・`compose_ui.rs` から切り出す）。

### 使う側がやること

- fumi: `ito-markdown` を `rev` で読み、本文の送信に `to_html`（`image` は添付の `cid:`）と `to_plain` を使う。ツールバーの下線・打ち消しは `Tool::Underline`・`Tool::Strike`。minato は `composer.rs` の同じ関数を置き換えられる（egui の部品が入るまで待ってもよい）。

## [0.195.0] - 2026-11-01

### 追加

- 新しいクレート `ito-winrm`: Windows の PC で WinRM（WS-Management の cmd シェル）越しにコマンドを走らせる。`exec(&Target, command, &ExecOptions, &stop, &mut |Stream, &[u8]| …) -> Result<Exit, Error>` は `ito_sftp::exec` と同じ形（`Stream`・`ExecOptions { env, timeout }`・`Exit { status }`）。出力は届いた順に渡り、`stop` を立てる／時間切れになると、リモートのコマンドに終了を頼み（Signal）、シェルを消してから `Stopped`／`Timeout` を返す。止まるのでワーカースレッドから呼ぶ。
- HTTPS（5986、`Target::https`）は証明書の SHA-256 を固定できる（違えば `Error::Fingerprint(見えた指紋)`）。初回は `peek_fingerprint(host, port, timeout, &stop)` で指紋を見て、利用者に確かめてもらってから固定する。`fingerprint::{sha256, format, parse}`。HTTP（5985、`Target::http`）も使えるが、Basic のパスワードが平文で流れるので信頼できる網だけ。純 Rust（rustls + ring）なので Linux・macOS からも使える。
- まだ無いもの: NTLM／Kerberos（今は Basic だけ。PC 側で `AllowUnencrypted` と Basic を許す設定が要る）。要るアプリは #132 に書く。

### 使う側がやること

- koyomi: `ito-winrm = { git = "https://github.com/uchmk/ito", rev = "…" }` を足す。

## [0.194.1] - 2026-11-01

### 修正

- `ito-prefs`: `secret_field` が、マウスを押すとアプリごと固まるのを直した（v0.165.0 から）。`ui.input` の中で `ui.data` を呼んでいて、egui のロックを入れ子に取っていた。目のボタンの矩形を先に読んでから `input` で調べる。ポインタを押すテストを足した。使う側がやることは `rev` を上げるだけ。

## [0.194.0] - 2026-11-01

### 追加

- `ito-prefs`: 設定画面の左下に、アプリ固有の設定ファイルのボタンを `common.toml` の上へ積めるようにした。`Extras::files: &[&str]`（例 `["koyomi.toml"]`）に並べると、同じ見た目のボタン（「開く: koyomi.toml」）が上から順に並び、一番下に共通のファイルが来る。左の一覧の高さはボタンの数だけ低くなる。Tab で回る順は、アプリ固有のファイル、common.toml の順。

### 変更（壊れる）

- `Out::open_file` を `bool` から `Option<usize>` にした。押されたボタンの番号で、共通のファイル（`Nav::file`）は `Some(0)`、`Extras::files[i]` は `Some(1 + i)`。押されていなければ `None`。

### 使う側がやること

- `Extras { … }` を欄ごとに書いているなら `files: &[]` を足す（`..Default::default()` なら不要）。
- `if out.open_file { open(common) }` を `match out.open_file { Some(0) => …, Some(i) => open(files[i - 1]), None => {} }` に替える。
- koyomi: `Extras { files: &["koyomi.toml"], .. }` で、`common.toml` の上に `koyomi.toml` を置ける。

## [0.193.0] - 2026-11-01

### 追加

- `ito-titlebar`: `brand(ui, rect, name, logo: impl FnOnce(u32) -> Vec<u8>) -> f32`。左の枠にアプリのロゴと名前を描き、終わりの x を返す（koyomi が自前で書いていたものと同じ見た目: 32 px の絵を 16 px で、名前は 13 px の `fg`）。`logo(px)` はアプリの `icon::pixels`（RGBA、`px` 四方）。テクスチャは 1 度だけ作って egui の文脈に置くので、アプリは持たなくてよい。

### 使う側がやること

- `ito_titlebar::show(ui, &spec, |ui, rect| ito_titlebar::brand(ui, rect, &name, icon::pixels), …)`。koyomi は自前の `logo` の欄と `LOGO_PX` を消して置き換えられる（fumi は名前だけを描いていた所を替える）。

## [0.192.1] - 2026-11-01

### 修正

- `templates/app`: 窓の起動のテーマを `common.theme_choice(None)` で決める。今までは `{{app}} Dark` / `{{app}} Light` という無い名前を既定にしていて、common.toml にテーマが無い PC で `ito_theme::pick` が失敗した（uchmk/ito#130）。

### 使う側がやること

- 雛形から作った既存のアプリで `"<app> Dark"` / `"<app> Light"` を既定にしている所（fumi の `crates/fumi/src/settings.rs:511` など）は `theme_choice(None)` に替える。

## [0.192.0] - 2026-11-01

### 追加

- `ito-prefs`: `startup_card_with(ui, l, w, now, problem: Option<&str>)`。スイッチが置いた所に残らなかった理由（`Some`）を、スイッチの下にエラーの色で出す（読み上げもする）。`startup_card` は今までどおり（`problem` なし）。
- `ito-common`: `startup::apply(app, args, on, refuse) -> (bool, Option<String>)`。ワーカースレッドで呼ぶ。`current_exe` → `refuse`（入れるときだけ。`|_| Ok(())` か `startup::refuse_temp`、アプリ独自でもよい）→ `set` → `enabled` の順に回し、OS の今の状態と、失敗・断りの理由を返す（uchmk/ito#131）。

### 使う側がやること

- 破壊的な変更は無い。`startup_card` を `startup_card_with(…, self.startup_problem.as_deref())` に替え、`startup::apply` の結果の `.1` を `startup_problem` に入れる（次のクリックで消す）。
- koyomi: Temp の断りを `refuse` に渡し、`KOYOMI_ALLOW_TEMP=1` の抜け道もそこに入れる。kura・fude・minato・yagura の `let _ =` や独自のエラー行は置き換えられる。

## [0.191.0] - 2026-11-01

### 追加

- `ito-common`: `in_temp(path) -> bool`。`path` が一時フォルダー（`std::env::temp_dir()`・`%TEMP%`・`%TMP%`。Unix は `/tmp`・`/var/tmp` も）の中かを返す。両側を `canonicalize` してから比べるので、短い名前（`YUU06~1`）やリンクでも合い、Windows は大文字小文字を区別しない。まだ無いファイルは、置き場所のフォルダーで決める。
- `ito-common::startup::refuse_temp(exe) -> Result<(), String>`: `in_temp` を見て、中なら文言つきの `Err`。`set` と `ito_service::install` は自分では断らない（呼ぶ側が、パスワードを聞く前などに呼ぶ）。

一時フォルダーの exe は、掃除で消され、zip の中から開くと `%TEMP%` で動き、そこから作った常駐は Defender に `Behavior:Win32/Persistence.A!ml` として消される（koyomi の実測）ので、断る。

### 使う側がやること

- 断りたいアプリだけ、登録の前に `ito_common::in_temp(&exe)`（または `startup::refuse_temp`）を呼ぶ。呼ばなければこれまでどおり。koyomi の持ち主の判断で、koyomi・tsumugi の server はまだ呼ばない。

## [0.190.0] - 2026-11-01

### 直した

- `ito-sftp`: `exec` の停止の旗を、始めから見る（#129）。今までは出力を読む輪に入るまで旗を見ず、TCP の接続・鍵交換・ログイン・チャンネルを開く・env/exec の返事のどこで止めても、1 つの段で最大 15 秒ほど待っていた。
  - どの段も 100 ms ごとに旗を見て、立っていれば `Error::Stopped` をすぐ返す（`step` を使う）。
  - チャンネルが開いたあと `exec` を送る前に旗が立っていれば、`exec` を送らずに閉じる（相手では何も始まらない）。`exec` を送ったあとなら、今までどおり `TERM` を送って閉じる。
  - 公開の API は変わらない。

### 使う側がやること

- koyomi: `rev` を上げるだけ。`koyomi ssh run` の停止が、接続の途中でも猶予のうちに返る。

## [0.189.0] - 2026-11-01

### 追加

- `ito-theme`: テーマの色を CSS にする（#128）。アプリが `ito-http` で配るページを、窓と同じテーマで見せる。
  - `Colors::css_vars() -> String`: カスタム プロパティを 1 行ずつ（セレクターなし）。基本の `--bg --side --panel --border --fg --dim --wait --run --err --done --blue --magenta`、混ぜて作る `--raised --border-strong --chosen --hover --strong --faint --wait-bg --wait-text --on-accent --selection`、`color-scheme`。色は `#rrggbb`、透明があれば `rgba(...)`。
  - `stylesheet(themes, choice, dark, light, high_contrast) -> Result<String, String>`: `pick` と同じ選び方で 1 枚のシートにする。固定のテーマは `:root { … }` 1 つ。`system` は暗いほうを `:root` に、明るいほうを `@media (prefers-color-scheme: light) { :root { … } }` に入れる（決めるのはエンジンの OS でなくブラウザー）。`high_contrast` は `Colors::high_contrast()` の色。

### 使う側がやること

- koyomi: common.toml とテーマのフォルダーを `prefs.rs` と同じに読み、`GET /theme.css` で `stylesheet(…)` を返す。`app.css` は `var(--…)` だけにする。

## [0.188.0] - 2026-11-01

### 追加

- `ito-proc`: 窓のプログラム（コンソールを持たない）の子にも「止まって」が届く（#127）。
  - `run_lines_soft` は、`grace` が 0 でないとき Windows で名前つきのイベント（`Local\ito-proc-stop-<pid>-<n>`）を作り、環境変数 `ITO_PROC_STOP_EVENT` で子に名前を渡す。止めるときは `CTRL_BREAK` に加えてこのイベントも立てる（どちらかが届けば `Asked`）。
  - `stop_requested()` は、この環境変数があればイベントを別スレッドで待ち、立ったら旗を真にする。コンソールがあってもなくても、窓のエンジン → 窓の子（PowerShell を通さない形）でも効く。環境変数は子孫に引き継がれるので、間に `powershell -Command` を挟んでも中のプログラムに届く。
  - 届かない形: イベントを待つのは `stop_requested()` を呼んだ子だけ（呼ばない子は今までどおり `grace` のあとで殺される）。別アカウントの子（`run_lines_as_soft`）は今までどおり頼めない。Unix は変わらない（SIGTERM）。

### 使う側がやること

- 無し（`rev` を上げるだけ）。koyomi: `koyomi ssh run` は起動の早い段階で `stop_requested()` を呼ぶこと（呼んだときにイベントの待ち受けが始まる）。PowerShell を通さず直に動かしてよい。

## [0.187.0] - 2026-11-01

### 追加

- `ito-proc`: 止め方を 2 段にする（先に頼み、猶予のあとで殺す）。
  - 親の側: `run_lines_soft(cmd, stop, grace, enc, on_line)`（`run_lines_as_soft` もある）。`stop` が立つと、子の木に「止まって」を送り、`grace` のあとまだ動いていれば今までどおり木ごと殺す。`grace` が 0 なら今までと同じ（`run_lines_with`・`run_lines_as` はこれを呼ぶ）。
    - Unix は子のプロセスグループに `SIGTERM`。Windows は `CREATE_NEW_PROCESS_GROUP` で起こして `CTRL_BREAK_EVENT` を送る（こちらにコンソールがあれば子はそれを共有し、無ければ子の隠しコンソールにつなぎ直して送る）。
    - `powershell -Command` を挟んでも、中で動いているプログラムは同じグループなので合図を受ける（PowerShell 自身は CTRL_BREAK では終わらないが、中のプログラムが終われば終わる）。合図を受けても終わらないプログラム（`ping` など）は猶予のあとで殺す。
    - 別アカウントの子（`run_lines_as_soft`）は、そのコンソールに届かないので頼めない。すぐ殺し、`ending` は `Killed`。
  - 結果: `Lines` に `ending: Ending` を足した。`Exited`（自分で終わった）・`Asked`（頼まれて猶予のうちに終わった。`status` は子自身のもの）・`Killed`（殺した。`status` は `None` で `stopped()` は真）。
  - 子の側: `stop_requested() -> &'static AtomicBool`。Windows は CTRL_C・CTRL_BREAK・コンソールを閉じる・ログオフ・シャットダウン、ほかは `SIGINT`・`SIGTERM` で真になる。初めて呼んだときに受け口を入れる（それまでの「その場で死ぬ」は無くなるので、旗を見て自分で返ること）。`ito_sftp::exec` の停止の旗にそのまま渡せる。

### 使う側がやること

- `Lines { … }` を自分で作っている所は無いはず（欄が 1 つ増えた）。`match` で全部の欄を分解している所があれば `ending` を足す。
- koyomi: `koyomi ssh run` の側で `ito_proc::stop_requested()` を `exec` の停止の旗に渡し、親の側を `run_lines_soft(…, Duration::from_secs(10), …)` にする。「止めた（相手も止めた）」は `Ending::Asked`、「殺した」は `Ending::Killed`。

## [0.186.0] - 2026-11-01

### 追加

- `ito-prefs`: `Mark::Server`（積んだ 3 つの箱に点が 1 つずつ。ネットの向こうの機械・サーバー）。`Mark::ALL` は 71 個になった。

### 使う側がやること

- koyomi: ssh の印を `Mark::Link` から `Mark::Server` に替える。

## [0.185.0] - 2026-11-01

### 追加

- `ito-common`: ネットから来たファイルの扱い（`web`、`ito_common::` にも出ている）。
  - `mark_from_web(path, from: Option<&str>) -> io::Result<()>`: ブラウザーと同じ「ネットから来た」印を付ける。Windows は `Zone.Identifier` の ADS に `ZoneId=3`（`from` があれば `HostUrl=`。制御文字は落とすので行は足せない）、macOS は `com.apple.quarantine`、ほかは何もしない。FAT などの印を持てない所は `Ok(())`。ファイルが無ければ `NotFound`（空のファイルは作らない）。
  - `runs_code(name) -> bool`: 最後の拡張子（大文字小文字を区別しない。末尾の点と空白は無視）が、プログラム・スクリプト・ショートカット・ディスクイメージ・インストーラーなど動く種類か（exe bat cmd ps1 js vbs lnk url msi iso dmg sh jar など。マクロ付きの Office も）。

### 使う側がやること

- fumi: 添付を `ito_openin::open_default` で開く前に `runs_code(name)` を見て、`true` なら直接は開かず「保存してから自分で開いてください」と出す。保存したファイルにも、開くために temp に書いたファイルにも `mark_from_web(&path, Some(url))` を付ける。

## [0.184.0] - 2026-11-01

### 追加

- `ito-common`: `safe_file_name(name) -> String`（`filename::safe_file_name`）。メールの添付・ダウンロード・アーカイブの中の名前のように、他人が決めた名前を、フォルダーの下に書ける 1 つの名前の部品にする。`dir.join(結果)` は `dir` の外に出ない。
  - `/ \ : * ? " < > |` と制御文字は `_`。`.`・`..`・空・点と空白だけの名前は `_`。末尾の点と空白（Windows で消える）は取る。
  - `CON`・`PRN`・`AUX`・`NUL`・`COM1`-`COM9`・`LPT1`-`LPT9`（拡張子付きも）は頭に `_`。
  - 拡張子は残し、255 バイトを超えるときは本体を UTF-8 の字の途中で切らずに切る。すでに良い名前はそのまま返る。

### 使う側がやること

- fumi: 添付を temp に書く前に `ito_common::safe_file_name(&attachment.name)` を通す。kura の展開など、他人の名前でファイルを作るところも同じ。

## [0.183.1] - 2026-11-01

### 修正

- `ito-layout` の `Panels::show`: 拡大率（`set_zoom_factor`）が約 1.11 を超えると、大きさのない最初のフレーム（egui は 10000x10000 を使う）を見分けられず、そこから出した既定の幅が居座った（拡大率 1.2 では 10000/1.2 ≈ 8333 になり、9000 の判定を外れていた）。拡大率を掛けた点で比べるようにした。

### 使う側がやること

- なし（`rev` を上げるだけ）。fumi の `max_size(room - 300)` の回避は外してよい。

## [0.183.0] - 2026-11-01

### 追加

- 新しいクレート `ito-zip`: zip にファイルを足す・中を見る・1 つ読み出す。純 Rust（`zip` の deflate）で、Windows・macOS・Linux で同じ。名前は UTF-8 なのでエクスプローラーでも開ける。
  - `add(zip, &[Source::new(path, "中の名前")], OnExisting)`・`add_dir(zip, dir, prefix, on)`（下のフォルダーを `prefix/相対パス` で全部）・`add_bytes(zip, &[(name, bytes)], on)`。zip が無ければ作り、あれば足す。
  - 書くのは `<zip>.part`。flush して `sync_all` してから名前替えで本物に替えるので、途中で落ちても元の zip は無傷で、`.part` も残らない。元の中身は再圧縮せずにそのまま写す。
  - `OnExisting::Replace`（同じ名前は置き換える）か `Refuse`（`Error::Exists`。zip は触らない）。戻りの `Added { added, replaced, bytes }`。
  - `list(zip) -> Vec<Entry { name, size, packed, modified, is_dir }>`、`read(zip, name)`、`read_to(zip, name, &mut impl Write)`。
  - 中の名前は相対の `/` 区切りだけ。`..`・先頭の `/`・ドライブ・`\\` は `Error::BadName`。`Error::{Io, Corrupt, NotFound, Exists, BadName}`。時刻は UTC で保存。止まるのでワーカースレッドから。

### 使う側がやること

- koyomi: `kind = "zip"` の PowerShell の `Compress-Archive` を `ito_zip::add_dir` / `add` に替える。`ito-zip` を足して全部の ito のクレートと同じ `rev` にそろえる。

## [0.182.0] - 2026-11-01

### 追加

- `ito-sftp`: SSH でコマンドを 1 つ走らせる `exec(&Target, command, &ExecOptions, &stop, &mut |Stream, &[u8]|)`（SFTP を始めないので、SFTP の無いサーバーでも使える）と、つないだ `Session` で走らせる `Session::exec`。つなぎ方は今までと同じ（`Target`・指紋を固定したホスト鍵・パスワードか鍵）。
  - 標準出力・標準エラー（`Stream::Stdout` / `Stderr`）を届いた順にコールバックへ流す。
  - 戻りは `Exit { status, signal, env_refused }`。`success()` は終了コード 0。シグナルで終わったときは `signal`（`TERM` など）。
  - `ExecOptions { env, timeout }`: `env` は `env` リクエストで頼む（サーバーは `AcceptEnv` の分だけ受けるので、断られた名前は `env_refused` に入り、失敗にはしない）。`timeout` は全体の時間（`None` は終わるまで待つ）。
  - 停止の旗か時間切れで `Error::Stopped` / `Error::Timeout`。どちらも先に相手のプロセスへ `TERM` を送ってからチャンネルを閉じる。
  - 終了コードを受け取る前に切れたら `Error::Lost`。接続・認証・ホスト鍵の失敗は今までの `Error` のまま。
- 手元に SSH サーバーが無いので、テストは終了コードの判定と、つながらないときの `Error::Connect` まで。実機の確認は koyomi の側で。

### 使う側がやること

- 無し（足しただけ）。koyomi の依頼 #123。

## [0.181.0] - 2026-11-01

### 追加

- `ito-keyscript`: ダブル クリック `<DClick:0.5,0.4>`（`C-` / `A-` / `S-` つき）と `<DClick:文字>`（ウィジェットの中心を 1 回だけ探す）。`Step::DClick`・`Step::DClickText`、`Press::DClick`・`Press::DClickText`。押して離す組を 2 つ、1 組を 1 フレームに入れるので、フレームが遅くても egui の `double_clicked()` の窓に入る。`<Click:>` を 2 つ続けても二度押しになるのは運だった（koyomi の依頼、#122）。

### 使う側がやること

- 自前のループで `Press` を `match` しているアプリは、`Press::DClick`・`Press::DClickText` の腕を足す（`pointer_frames` / `pointer_events` に渡せば `DClick` は動く。`DClickText` は `Click` の文字の探し方で位置を決めてから 3 フレームの二度押しを流す）。`Runner` を使うアプリは何もしなくてよい。

## [0.180.0] - 2026-11-01

### 追加

- `ito-mail`: 送るメールを RFC 5322 の生のバイトに組む `compose(&Outgoing, SystemTime) -> Vec<u8>`（`Outgoing`・`Attachment`）。CRLF・`MIME-Version`・`Date`（UTC）・From のドメインと乱数の `Message-ID`・`In-Reply-To`/`References`。件名と表示名は RFC 2047（`=?UTF-8?B?…?=`、1 語 75 文字以内で折る）、`"` や `,` のある名前は引用符で包む。本文は UTF-8 の base64（76 文字ごと）、HTML があれば `multipart/alternative`、添付があれば `multipart/mixed`（ファイル名は RFC 2231 の `filename*` と RFC 2047 の `name=` の両方、`mime` が `None` なら拡張子から）。境界は乱数。ヘッダーに CR/LF は入らない。読み返す検査つき（fumi の依頼）。

### 使う側がやること

- 送るなら `ito_mail::compose` の結果を SMTP か Gmail `messages/send` の `raw`（base64url）へ。`Outgoing` は `..Default::default()` で埋められる。

## [0.179.1] - 2026-11-01

### 修正

- `ito-prefs`: `CodeBox` が、渡された幅より 0.3〜0.5 pt 広く描いていた（エディターの幅が描画の格子に切り上がる）。自動の大きさの親（`Panel::right` など）が毎フレーム広がった。エディターを 1 pt 内側に取る。幅を変えた検査 `the_parts_stay_inside_their_room` を足した（koyomi の報告）。

## [0.179.0] - 2026-11-01

### 追加

- `ito-confirm`: `Question::marks: &[Option<Mark>]`。ボタンごとの印（`ito_prefs::Mark`）を、言葉の前に文字の色で描く（`IconButton` と同じ。足りない・`None` は印なし）。`primary`・`danger` の塗りはそのまま（koyomi の #121）。`ito-confirm` は `ito-prefs` に依るようになった。

### 使う側がやること

- `Question { … }` を `..Default::default()` なしで書いているなら、`marks: &[]` を足す。

## [0.178.1] - 2026-11-01

### 追加

- `ito-prefs`: `emoji_picker_open(ctx, id) -> bool`。ピッカーが開いているか。アプリの Esc の処理より先に聞けば、ピッカーの Esc でカードごと閉じずに済む（fumi の依頼）。

### 修正

- `ito-prefs`: `emoji_picker` が、呼ばれなかったフレームをまたぐと閉じる。開いたままカードが消えても、次に出したときに開いた状態で出ない。

## [0.178.0] - 2026-11-01

### 追加

- `ito-prefs`: 印を 6 つ（#120、koyomi の依頼）: `Mark::Stop`（四角。止める・強制終了）、`Skip`（三角と棒。飛ばす）、`Print`（プリンタ）、`Rerun`（三角を囲む回る矢印。`Refresh` は読み直し）、`Compare`（並んだ 2 枚）、`Import`（箱へ入る矢印。`Download` の逆）。`Mark::ALL` は 70。
- `ito-prefs`: `InputField::rows(n)`。2 以上なら同じ枠・輪のまま複数行の文章の欄（行番号なし。コードは `CodeBox`）。

### 使う側がやること

- `Mark` を網羅して match しているアプリは 6 つの腕を足す。

## [0.177.0] - 2026-11-01

### 追加

- `ito-prefs`: 編集フォームの部品（koyomi の依頼）。
  - `form_label(ui, l, label, need, note, &FormWords)`: 12px の太字・dim のラベル。`Need::Required` は ` *`（`c.err`）、`Optional` は `(任意)`、`Plain` は無し。`note` は後ろに小さく。読み上げは「ラベル, 必須／任意」。言葉は `FormWords::{en, ja, of(lang)}`。
  - `InputField::new(id).name(..).mark(Mark::Folder).unit("秒").hint(..).mono().width(w).error(bool).show(ui, &mut String)`: 枠つきの 1 行の欄（`c.bg`・1px `c.border`・角 6、フォーカスで `c.run` の輪、`error` で `c.err`）。
  - `CodeBox::new(id).name(..).title("run").aside("1行に1つ").rows(3).error(bool).show(ui, &mut String)`: 行番号つきの等幅の複数行の欄。折り返しても番号は 1 行に 1 つで、スクロールにも付いてくる。
  - `fieldset(ui, l, title, aside, body)`: カードでない節。見出し（Heading）と右端の大文字の aside、1px の線、本文。
  - `Mark::Lock`（南京錠。`Mark::ALL` は 64）。
- 欄は `Id::new(("ito-input"/"ito-codebox", id))` の安定した id を持ち、`.name(..)` が読み上げ・`--keys` の `<Click:文字>` の名前になる（既定は id）。

### 使う側がやること

- `Mark` を網羅して match しているアプリは `Mark::Lock` の腕を足す。

## [0.176.0] - 2026-11-01

### 追加

- `ito-prefs`: 絵文字ピッカー `emoji_picker(ui, look, id, &anchor, &EmojiWords) -> Option<&'static str>`。アプリが描くボタン（`mark_button(.., Mark::Emoji, ..)` など）の `Response` を渡すと、押すたびに開閉し、ボタンの上に 5 つの分類（顔・手・ハート・もの・記号）の 8 列の表を出す。選ぶ・Esc・外を押す、で閉じる。分類の名前は `EmojiWords`（`en()`・`ja()`・`for_lang`）。一覧は `EMOJI_CATEGORIES`（fumi の依頼）。

### 使う側がやること

- 返信の窓の足の「テンプレートと保存時刻の間」に `mark_button` を置き、その `Response` を `emoji_picker` に渡す。返った絵文字を本文のカーソル位置へ入れる。

## [0.175.0] - 2026-11-01

### 追加

- `ito-prefs`: `FloatCard::avoid_top(h)`。上に空ける高さ（タイトルバー）。広げたカード（`expanded`）もこの下に収まる（fumi の依頼）。

### 変更

- `ito-prefs`: `FloatCard::avoid_bottom(h)` と `avoid_top(h)` は、文字の倍率に合わせて伸びる（zoom 1 の点で渡す）。
- `ito-prefs`: `chips`／`chips_with` の入力欄の id を、チップの数が変わっても変えない（以前は Enter・`,`・`;` でチップが増えるたびに id が変わり、欄がフォーカスを失った）。
  Enter の判定は、OS の窓にフォーカスがなくても（`--keys`）効く（`lost_focus` も見る）。

#### 使う側がやること

- `avoid_bottom` に倍率を掛けた値を渡していたら、zoom 1 の点（`ito_statusbar::HEIGHT` のまま）に戻す。
- チップ欄に自前で `request_focus` を返していた回避は外してよい。

## [0.174.0] - 2026-11-01

### 追加

- `ito-prefs`: `seg_named(ui, look, id, name, now, options) -> (Option<T>, Response)`。区切りのボタンの組に読み上げの名前を付け（「名前: 選んだ項目」）、組の `Response` を返す。`seg` は名前なしの同じ動きで、今まで通り（koyomi の依頼）。`seg` も選んだ項目を読み上げるようになった。

#### 使う側がやること

- 既存の `seg` はそのまま動く。組に名前を付けたいところだけ `seg_named` に替える。

## [0.173.1] - 2026-11-01

### 変更

- `templates/app`: 雛形の本体の画面を `ito_prefs::empty_state` の空のゴーストにした（以前は見出しと文字だけ）。雛形の `CLAUDE.md` に「空の一覧は `empty_state`」の規約（出す条件は呼ぶ側・追加できる空だけボタン・追加できない空は `None`）を足した。

#### 使う側がやること

- 既存のアプリは何もしなくてよい。雛形の規約を自分の `CLAUDE.md` に写してもよい。

## [0.173.0] - 2026-11-01

### 追加

- `ito-prefs`: `empty_state(ui, look, mark, title, hint, action)`。一覧が空のときの薄い絵＋文言（何が空か・何をすればよいか）と、追加できるときだけ出す 1 つのボタン。押されたかは返り値の `Response`。

#### 使う側がやること

- 空の一覧の自前の「ありません」を `empty_state` に置き換える（`if items.is_empty()` の中で呼ぶ）。追加できない空（絞り込みで 0 件・読み取り専用）は `action` を `None` にする。

## [0.172.0] - 2026-11-01

### 追加

- `ito-prefs`: `Mark` に `CalendarOff`（予定の面に小さな ×）・`CalendarCheck`（同じ面に小さな ✓）（計 63。koyomi の #119）。
- `ito-prefs`: 日付欄の読み上げの名前。`DateField::name("開始日")`、`RangeField::names("開始日", "終了日")`。付けなければこれまで通りヒント（`YYYY-MM-DD`）を読む（koyomi の依頼）。

### 使う側がやること

- `match` で `Mark` を網羅している所があれば、2 個足す。

## [0.171.0] - 2026-11-01

### 追加

- `ito-prefs`: 日付の欄（koyomi の #118）。`YYYY-MM-DD` の文字欄（打つ・貼るはそのまま）の右にカレンダーの印があり、押すか Alt+↓ で下に月のグリッドが開く。テーマ・読み上げ（`ito-a11y`）・`--keys` の `<Click:2026-11-05>` に従う。
  - `DateField::new(id, DateWords::ja()).empty_ok().range(min, max).monday_first(bool).today(d).width(w).day_look(&|d| Some(DayLook { ink, tip })).show(ui, look, &mut String) -> DateResponse { response, changed, valid, open }`。`.multi()` でコンマ区切りの一覧（日を押すと入り切り、グリッドは開いたまま。書き出しは `2026-11-05, 2026-11-20`）。
  - `RangeField::new(id, words).show(ui, look, &mut from, &mut to)`: 開始と終了の 2 欄に 1 つのグリッド。1 回目の押しが開始、2 回目が終了（前後は入れ替わる）。終了を探している間は間の日が薄く光る。
  - `month_picker(ui, look, id, &words, &anchor_response, &mut (year, month)) -> bool`: 任意の `Response` の下に年（◀ ▶）と 12 の月を出す。月が選ばれたら true。
  - グリッドのキー: ←→ で 1 日、↑↓ で 1 週、PageUp/PageDown で 1 か月、Home で今日、Enter で選ぶ、Esc で閉じる。見出しを押すと年・月の選びに替わり、「今日」で戻る。今日は輪、選んだ日は塗り、`min..=max` の外は押せない。
  - 言葉は `DateWords`（`en()`・`ja()`・`for_lang(code)`。月・曜日（日曜始まり）・見出し `{y}`/`{m}`/`{month}` ほか）で渡す。文字は直書きしない。
  - `parse_date`（`2026-11-05`・`2026/1/5`）・`format_date`・`shift_months`・`month_grid` も出す。日付の型は chrono の `NaiveDate`（`ito_prefs::NaiveDate` で再エクスポート）。`ito-prefs` に chrono（`clock`）の依存を足した。

### 使う側がやること

- 手書きの `YYYY-MM-DD` 欄を `DateField`／`RangeField` に置き換える。`DateWords` は `ito_i18n` の言語に合わせて `DateWords::for_lang(&lang)` で作る。

## [0.170.0] - 2026-11-01

### 追加

- `ito-prefs`: `Mark` に `Save`・`Undo`・`Redo`・`Arrange`・`ArrowDown`・`ArrowRight`・`Settings`・`Terminal`・`Flow`・`Branch`・`Merge`・`File`・`Person`・`Folder`・`Globe`（計 61。koyomi の #117）。
- `ito-prefs`: `IconButton::drag()`。クリックに加えてドラッグも受ける（部品の棚から引き出す用）。

### 使う側がやること

- `match` で `Mark` を網羅している所があれば、新しい 15 個を足す。

## [0.169.0] - 2026-11-01

### 追加

- `ito-prefs`: 作成ウィンドウの部品（fumi の依頼）。
  - `Mark` に `Minimize`・`Expand`・`Restore`・`Emoji`・`Underline`・`Strike`・`Format`（計 46）。
  - `SplitButton::new(mark, words, menu_said).tone(..).key(..).mark_last().show(ui)`: 本体とメニュー（下向きの山）の二つ割りボタン。`SplitResponse { main, menu }`。
  - `chips(ui, look, &mut Vec<String>, &mut String, hint)`（言葉を変えるなら `chips_with`）: 宛先のチップ入力。Enter・`,`・`;` で確定、空の欄の Backspace で最後を外す、折り返す。`ChipsResponse { changed, removed, input }`。候補は `input.rect` の下にアプリが出す。
  - `FloatCard::new(id, title).badge(..).corner(..).size(w, h).avoid_bottom(h).minimized(..).expanded(..).show(ctx, look, |ui| ..)`: 隅に浮くカード（見出しに ─ ⤢ ✕）。最小化・全面の状態はアプリが持ち、`FloatResponse { minimize, expand, close, inner, rect }` で反転する。

### 使う側がやること

- 仮の部品を置き換え、`rev` を上げる。

## [0.168.2] - 2026-11-01

### 修正

- `ito-layout`: `Panels::show` は、サイズを覚えていないパネルの egui の記憶を、窓の大きさがまだ無い最初の 1 フレームだけ捨てる（0.168.0 では毎フレーム捨てていたので、`default` より高い・広い中身が伸びず、下のものに重なった）。fumi の報告。

### 使う側がやること

- なし。`rev` を上げるだけ。

## [0.168.0] - 2026-11-01

### 追加

- `ito-prefs`: `attachment` に引数 `download_words`（ダウンロードの印のホバーと読み上げの名前。"Download"・"保存"）。uchmk/ito#115。
- `ito-prefs`: `avatar` の頭文字。最初の語が CJK（漢字・かな・ハングル）なら 1 文字（"佐藤 花子" は "佐"）。英字の名前は今まで通り（"AL"）。

### 修正

- `ito-layout`: `Panels::show` は、まだ大きさが保存されていないパネルの egui の `PanelState` を毎フレーム忘れ、`default` を読み直す（uchmk/ito#114）。
  最初のフレームは窓の大きさが無く egui が 10000×10000 として扱うため、そこで出した default（`available_width() * 0.55` など）が居座っていた。
  端をドラッグして大きさが決まってからは今まで通り。

### 使う側がやること

- `attachment(ui, look, name, size_text)` の呼び出しに 5 つ目の引数 `download_words` を足す。

## [0.168.1] - 2026-11-01

### 修正

- `ito-service`: `read_password` が、窓のアプリ（`windows_subsystem = "windows"`）の `.exe` でも使える（uchmk/ito#116、koyomi の依頼）。
  標準入力がパイプ・ファイルならこれまで通りそのまま読む。そうでなければ親のコンソールにつなぎ（`ito-console` と同じ。つながっていても無害）、`CONIN$` / `CONOUT$` を開いて、
  文言を `WriteConsoleW` で出し、エコーを切って `ReadConsoleW` で 1 行読む。
- `ito-service`: コンソールもパイプも無いときは `Error::NoConsole`（`Failed(0)` ではなく）を返す。

### 使う側がやること

- `ito_service::Error` を `match` している所は `NoConsole` の腕を足す（「パスワードをパイプで渡すか、端末から実行してください」と案内する）。

## [0.167.1] - 2026-11-01

### 追加

- `ito-prefs`: `Tone::Warn`（uchmk/ito#111、koyomi の依頼）。テーマの警告の色（`wait`）で塗り、文字は地の明るさで暗い色か白を選ぶ。`button_with` と `IconButton` の両方で使える。

### 使う側がやること

- `Tone` を `match` している所は `Warn` の腕を足す（koyomi は「スケジュールを戻す」を `Tone::Primary` から `Tone::Warn` に）。

## [0.167.0] - 2026-11-01

### 追加

- `ito-prefs`: メール アプリ向けの印と部品（uchmk/ito#113、#108、#109 をまとめて。fumi の依頼）。
  - `Mark` に 28 を足した（`Mark::ALL` は 39）: Inbox・Star・StarFilled・Flag・Clock・Important・Send・Calendar・Pencil・Mail・Archive・Warning・Trash・Label・Reply・ReplyAll・Forward・Paperclip・Link・More・Sort・Check・ChevronDown・ChevronUp・Info・Bell・Bold・Italic。どれも 14 の箱・1.5 の線。
  - `IconButton::new(Mark, words)`: `.said(..)`・`.tone(Tone)`（Primary は `run` の塗り、Danger は `err` の縁と薄い地の輪郭、Plain）・`.key("R")`（右の小さなキーの札。文字はアプリが keymap から渡す）・`.wide()`（列の幅いっぱい。札は右端）・`.mark_last()`（文字のあとに印。送信ボタン）・`.show(ui)`。`icon_button` は `IconButton` の薄い包み。
  - `NavRow` に `mark: Option<Mark>`（固定幅の印の列。印の有る行も無い行も名前の x がそろう）と `dot: Option<Color32>`（ラベルの色の点）。今の行の件数は `run` のピルになる。
  - `avatar(ui, look, name, seed, size)`: 頭文字の円。色は `seed`（アドレス）の安定したハッシュ（`tag_hue`）。
  - `notice(ui, look, Mark, text, action) -> Option<Response>`: お知らせの帯。右に 1 つのボタン。
  - `attachment(ui, look, name, size_text) -> AttachmentResponse { open, download }`: 添付のカード。拡張子の札・名前（長ければ「…」）・サイズ・Download の印のボタン。カード全体も押せる。
  - `pill(ui, look, words, Option<Mark>) -> Response`: 完全に丸い静かなボタン。末尾の印は ChevronDown など。

### 使う側がやること

- `NavRow { icon, text, count, strong, current }` と全部の欄を書いている所は、`..Default::default()`（か `Row::new(text)`）を足す（欄が 2 つ増えた）。

## [0.166.0] - 2026-11-01

### 追加

- `ito-prefs`: 印の付いた枠のボタン `icon_button` と、印だけを描く `mark`（uchmk/ito#112、koyomi の依頼）。
  - `Mark`: Search・Download・Upload・Play・Pause・ChevronLeft・ChevronRight・Plus・Close・Refresh・Copy（`Mark::ALL` で全部）。字体の記号でなく 1.5 の線で描くので、どのテーマ・高コントラストにも合う。
  - `icon_button(ui, Mark, words, said) -> Response`: 地は `raised`・縁は `border`、ポインタの下かキーの上では `hover` と `border_strong`。印は `dim`（熱いときは `fg`）、文字は `ui` の Body の大きさ、フォーカスの輪は `run`。読み上げは `ito_a11y::say(Role::Button, said)`。`said` が文字と違うときだけホバーに出る。
  - `mark_button(ui, Mark, said, side) -> Response`: 印だけの正方形のボタン（畳む印・閉じる印）。`side` は字の拡大の前の点数。
  - `mark(painter, rect, Mark, colour)`: 印だけ。`rect` の短い辺を 14 として拡げ縮みする。
- `ito-prefs` が `ito-a11y`（機能 `egui`）に依る。

### 使う側がやること

- koyomi: `icon_button`・`enum Icon`・`fold_button` を `ito_prefs::icon_button`・`mark_button(…, Mark::ChevronRight / ChevronLeft, …, 22.0)` に置き換える。

## [0.165.0] - 2026-10-31

### 追加

- `ito-prefs`: `secret_field`（uchmk/ito#107）。秘密（トークン・パスワード・OAuth のクライアント シークレット）を `ito-secret` に渡すための、伏せ字の 1 行の入力欄。
  - `secret_field(ui, look, &mut drafts, id, hint, &SecretWords { show, hide }, width) -> Option<String>`。Enter か欄を離れたときに打った秘密を 1 度だけ返す（Esc と空の欄は `None`）。
  - 欄は常に空から始まり、保存済みの秘密は見せない。`hint` は「保存済み」「未保存」をアプリが渡す。取り出したあと `Drafts` には何も残らない。
  - 右の「表示 / 隠す」ボタンで伏せ字を切り替える（ボタンを押しても欄を離れた扱いにならない）。`secret_taken` は離れたときの判定だけの関数。

### 使う側がやること

- 秘密を打つ欄の `ito_prefs::field` を `secret_field` に替える。返った文字列は `ito_secret::set` に渡し、設定のファイルには書かない。

## [0.164.0] - 2026-10-31

### 追加

- `ito-oauth`: `Client::from_google_json(&str)`。Google がダウンロードさせる `client_secret_*.json`（`{"installed": {…}}` か `{"web": {…}}`）から `client_id` と `client_secret` を読んで `Client` にする。
  どちらのキーも無い・JSON でない・`client_id` が空のときは `Error::Bad`。`client_secret` が空・無いときは `None`。`Debug` は今までどおり秘密を隠す。

### 使う側がやること

- 設定画面の「JSON から読む…」で `ito-dialog` で選んだファイルの中身を `Client::from_google_json` に渡し、id は設定に、秘密は `ito-secret` に置く。

## [0.163.0] - 2026-10-31

### 追加

- `ito-dialog`（uchmk/ito#77）。OS 標準の「開く」「保存」ダイアログ（Windows は共通アイテム ダイアログ、macOS は NSOpenPanel、Linux はデスクトップ ポータル）。`rfd` の薄い包みで、Linux でも gtk を要らない。
  - `Dialog::new().title(..).directory(..).file_name(..).filter(Filter::new("TOML", &["toml"]))` に対して `open_file()`（`Option<PathBuf>`）・`open_files()`（取り消しは空の `Vec`）・`open_folder()`・`save_file()`（提案名と開始フォルダー）。
  - どれも答えが出るまで止まる。`ito_dialog::spawn(move || d.open_file())` が別スレッドで呼び、`Receiver` を返す（egui の毎フレームで `try_recv`）。
  - 親の窓は渡さない（最初の版）。必要になったら足す。

### 使う側がやること

- `ito-dialog = { git = "https://github.com/uchmk/ito", rev = "…" }` を足し、独自の `rfd` の依存を外す。

## [0.162.0] - 2026-10-31

### 追加

- `ito-oauth`（uchmk/ito#106）。デスクトップアプリの OAuth 2.0 サインインと、それが使う HTTPS クライアント。純 Rust（rustls + ring、webpki の根）、ブロッキング。
  - `sign_in(&GOOGLE, &Client { id, secret }, &[scope…], |url| …, &stop, &Options::default())`: RFC 8252 のインストール型。同意の URL を `open` に渡し、`127.0.0.1` の乱数ポートで code を受け、PKCE（S256）と `state` を確かめて交換する。
    favicon や state の違う要求では待ちをやめない。「許可しない」は `Error::Denied`、停止の旗は `Cancelled`、`Options::timeout`（既定 5 分）は `Timeout`。
  - `Session`: `start(app, account, provider, client, tokens)` でリフレッシュ トークンを `ito-secret` に置く（ファイルには置かない）。`resume` で前回のサインインを戻し、`token()` は期限の 1 分前から更新する。
    `invalid_grant` は `Error::Refused`（秘密も消す）。`sign_out()` は秘密を消してから revoke する。
  - `https::request(method, url, bearer, body, timeout)`: 2xx だけ `Ok`。ほかは `Error::Status { code, retry_after, body }`（`retryable()` は 429・5xx・通信の失敗・タイムアウト）。chunked と Content-Length、本文は 32 MiB まで。`http://` はループバックだけ（試験用）。
  - `Tokens`・`Session` の `Debug` はトークンを出さない。

### 使う側がやること

- fumi: 自前の仮実装（mock）をこれに替える。`Client` の id は Google Cloud の「デスクトップ アプリ」のクライアント。
- koyomi（任意）: `ito-mail` の XOAUTH2 に `Session::token()` を渡せる。

## [0.161.0] - 2026-10-31

### 追加

- `ito-table`（uchmk/ito#82）。表を CSV と Excel のブック（`.xlsx`）で渡す。
  - `Table::new(&["日付", …])`・`row([Cell…])`・`sheet(名前)`。セルは `Cell::Text`・`Number`・`Stamp`（`Stamp::date`・`Stamp::new`）で、`&str`・`String`・数・`Stamp` から `.into()` で作る。
    短い行は空で埋め、長い行は切る。
  - `csv(path)`・`csv_string()`: UTF-8 の BOM・CRLF・RFC 4180。`=` `+` `@`・タブ・CR で始まる文字（と、続きのある `-`）は Excel が式として実行しないよう `'` を前に付ける（`.xlsx` の文字は常に文字列なのでそのまま）。
  - `xlsx(path)`・`xlsx_bytes()`: 1 枚のシート、見出しは太字で固定、列幅は中身に合わせる（全角は 2 桁）、日時は本物の日付（並べ替え・絞り込みができる）。純 Rust（`rust_xlsxwriter`）。ありえない日付は panic せず `Err`。
  - ファイルは隣に書いてから名前替え。止まるのでワーカースレッドから呼ぶ。

### 使う側がやること

- koyomi は `crates/koyomi/src/csv.rs` の自前の CSV をやめて `ito_table::Table` に移す（`--csv <file>` はそのまま、`.xlsx` を足せる）。

## [0.160.0] - 2026-10-31

### 追加

- `ito-agent`（uchmk/ito#96）。AI エージェントにアプリを教える。
  - `setup(&App, &Home, &Options)`・`status`・`remove(&App, &Home, dry_run)`。どれも冪等で、`dry_run` は同じ `Report`（`lines` と `changed`）を返して何も書かない。
  - Claude Code: スキル `~/.claude/skills/<app>/SKILL.md`、MCP は `~/.claude.json` の `mcpServers.<app>`、SessionStart の hook は `~/.claude/settings.json`。
    Codex: `~/.codex/config.toml` の `[mcp_servers.<app>]`（コメントと並びは残る）。他のキー・サーバ・hook は順序ごと残す。
  - アプリの状態のフォルダーの manifest（`App::manifest`）にハッシュを記録する。`State` は `Missing`・`Installed`・`Stale`（自分が入れた後、触られていない）・`Edited`（ユーザーが変えた。`force` でだけ置き換える）。
    `remove` は manifest にあって今も同じものだけ消し、編集されたものは残して言う。`Options::hook = false` は hook を入れず、入れた hook は外す。
  - 壊れた JSON / TOML は書き潰さずエラー。書き込みは同じフォルダーに書いてから名前替え。見つからない（`~/.claude` などが無い）エージェントは、`Options::agents` で名指しされない限り飛ばす。
  - テストは `Home::under(root)`、または環境変数 `ITO_AGENT_HOME` と `Home::detect()`。

### 使う側がやること

- `App { name, skill, mcp: Some(Launch::new("<app>", &["mcp"])), hook: Some("<app> hook session-start".into()), manifest }` を作って、`<app> skill setup|status|remove` から呼ぶ。`Report::lines` は `ito-console::out` で出す。

## [0.159.0] - 2026-10-31

### 追加

- `ito-layout` の `panels` の機能（uchmk/ito#105）: 幅を変えられる egui のパネルの大きさを覚える `Panels`。
  - `Panels::load(state_dir)` で `panels.toml`（`[size]` にパネルの名前ごとの大きさ）を読む。`None`（`--keys`）は何も読まず書かない。
  - `Panels::show(ui, Side, 名前, 既定, tune, add)` は覚えた大きさから始め、中身をパネルいっぱいに描く。egui は描いた大きさを覚えるので、中身が少ないパネルが縮んだままになるのを防ぐ。
  - 覚えるのは縁をドラッグした大きさだけで、離したときに `kept::Writer` で書く。窓を小さくしたり最小化したりして押し縮められた大きさは覚えない。

## [0.158.0] - 2026-10-31

### 追加

- `ito-http`（uchmk/ito#104 の 3・4）。
  - 名前つきのトークン: `Config::token_named(name, token)`。どれで来たかは `Request::who`（`None` は `Config::new` の既定のトークン）。
    既定のトークンを空にできるのは、名前つきが 1 つ以上あるときだけ。名前や値が空なら `start` が断る。`Debug` はトークンの名前だけを出す。
  - 公開パス: `Config::public_path("/")`。そのパスへの GET だけ、トークンなしで通す（ログイン画面の HTML など）。
  - 生の本文: `Response::bytes(status, content_type, body)` と `with_header(name, value)`。見出しの値の改行は取り除き、`X-Content-Type-Options: nosniff` を付ける。
  - ブラウザ向けのセッション（`Config::sessions = true`、`session_ttl` 既定 8 時間、上限 256）。`POST /session` に Bearer で入ると
    `ito_session` の Cookie（HttpOnly・SameSite=Strict・TLS なら Secure）を返し、以後は Cookie がトークンの代わりになる。
    Cookie での GET/HEAD/OPTIONS 以外は `X-Ito-Csrf: 1` が要る（無ければ 403）。`GET /session` は誰か、`DELETE /session` は出る。
    Bearer のときは今までどおり何も要らない。

### 使う側がやること

- `Request` と `Response` を `{ … }` で直に作っているなら、`who`・`raw`・`headers` を足す（`Response::ok` などの関数を使っていれば何もしない）。
- 依存に `getrandom` が増えた（セッションの ID 用）。

## [0.157.0] - 2026-10-31

### 追加

- `ito-proc::run_elevated(exe, args) -> io::Result<i32>`: UAC の確認（`ShellExecuteExW`、`runas`）を出して昇格した exe を起こし、終わるまで待って終了コードを返す（uchmk/ito#103）。
  相手には自前のコンソールの窓が付くので、パスワードをそこで読める。確認を断ると `ErrorKind::PermissionDenied`。Windows 以外は `Unsupported`。止まるので worker から。
- `ito-proc::is_elevated() -> bool`: 今のプロセスが昇格済みか（Windows の `TokenElevation`。ほかは `false`）。

### 使う側がやること

- koyomi: `ito_service` が `NeedsAdmin` を返したら、`!is_elevated()` を確かめて `run_elevated(current_exe, args)` で同じコマンドを昇格して回し、その終了コードをそのまま返す。

## [0.156.0] - 2026-10-31

### 追加

- `ito-a11y::State` に予定の仕事の 6 つの状態を足した（uchmk/ito#75）。色だけに頼らず、記号と読み上げの名前を持つ。

  | 状態 | 記号 | en | ja |
  |---|---|---|---|
  | `Waiting` | ○ | waiting | 実行待ち |
  | `Held` | ‖ | held | 保留 |
  | `Skipped` | » | skipped | スキップ |
  | `Missed` | ↷ | missed | 時刻を過ぎた |
  | `NotRun` | · | not run | 未実行 |
  | `Blocked` | ⊘ | blocked | 未確認で止めた |

  「異常ありで実行中」は `State::spoken(text, &[Running, Error], lang)` で言える。`ito-tray` のバッジも 6 つ分の色と白い記号を持つ。
- `ito-keyscript`: 台本の `<ClipboardHas:文字>`（`Step::ClipboardHas`・`Press::ClipboardHas`・`check_clipboard_has`）。最後にコピーした文字が、この文字を**含む**。
  状態やパスを含む長い行に使う（`<Clipboard:>` は今までどおり完全一致）。tsumugi-d6 の依頼（#98）。

### 変更

- `ito-keyscript`: `<Quit>` の説明に、窓を開く台本は `<Quit>` で終えること（無いと最後の手順のあとも窓が残る）を書いた。

### 使う側がやること

- `ito_a11y::State` を `match` で全部書いているなら、6 つの腕を足す（`_ =>` で受けているなら何もしない）。色を付けるアプリは、新しい状態の色も決める（`ito-tray::badge_color` が一例）。
- 自前の台本のループで `ito_keyscript::Press` を `match` しているなら、`Press::ClipboardHas(text)` の腕を足して `check_clipboard_has(ctx, &text)` を呼ぶ（`Runner` は何もしなくてよい）。

## [0.155.0] - 2026-10-31

### 追加

- `ito-http` を LAN に開けられるようにした（機能 `tls`、既定は切）（uchmk/ito#104 の 1 と 2）。
  - `Config.bind: Bind`。`Bind::Loopback`（既定、今まで通り）か `Bind::Lan { addr, tls: TlsCfg }`。LAN は **TLS だけ**で、平文の HTTP は PC の外に出ない（型で決まる）。
  - 証明書は自己署名で、`TlsCfg::new(cert_dir)` のフォルダーに初回に作る（`ito-http-cert.der`・`ito-http-key.der`。鍵は先に書く・Unix は 0600）。2 回目からは同じものを読む。
  - `Server::fingerprint()` と `tls::{fingerprint(cert_dir), format, parse, sha256}`。指紋は SHA-256 で、`AB:CD:…`。コロン・空白・`-`・大小どれでも読める。
  - 指紋を固定する HTTPS のクライアント `client::Client::new(url, 指紋, token)`・`get`・`post_json`（`Reply { status, json }`）。違う証明書なら `Error::Fingerprint(見えた指紋)`。
    初回の信頼用に `client::peek_fingerprint(url, timeout)`（見せられた指紋を返すだけ。本人に確かめさせてから固定する）。
  - TLS は rustls（ring）。Linux・macOS 向けのビルドには C のコンパイラーが要る（ito-sftp・ito-mail と同じ）。

### 使う側がやること

- `Config` に欄 `bind` が増えた。`Config { … }` を直に書いているなら `bind: Bind::Loopback` を足す（`Config::new` を使っていれば何もしない）。
- LAN に開けるなら `ito-http = { …, features = ["tls"] }`。鍵は `cert_dir` の中の秘密なので、アプリ自身の状態のフォルダーに置く。指紋は設定の画面で本人に見せる。
- まだ無いもの: 名前つきのトークン（`Request::who`）と、HTML・ブラウザーのセッション（uchmk/ito#104 の 3 と 4）。

## [0.154.0] - 2026-10-31

### 追加

- 新しいクレート `ito-mail`: メールボックスを IMAP で見て、新しい着信を返す（uchmk/ito#101）。読むだけで、メールを変えない。
  - `Inbox::open(&Account, password, From)`: つないで、ログインして、フォルダーを `EXAMINE`（読み取り専用）で開く。サーバー・ログイン・フォルダーが間違っていればここで `Error::Connect` / `Tls` / `Login` / `NoFolder`。
  - `Inbox::poll()`: 前回より後に来たメール（`Mail { uid, from, name, subject, date }`）を古い順に返す。ヘッダーは `BODY.PEEK` で取るので既読にならない。切れていれば次の呼び出しでつなぎ直す（場所は失わない）。1 回に最大 200 UID。
  - `Inbox::position()` → `Position { uid_validity, uid }`: アプリが状態に保存し、再起動後に `From::After(position)` で渡すと、二重にも取りこぼしもしない。`UIDVALIDITY` が変わっていたら終わりから始め直す。`From::End` は開いた後の着信だけ。
  - 件名・名前は RFC 2047（`B`・`Q`、ISO-2022-JP などの文字コード、隣り合う語の間の空白は捨てる）を復号する。`Date` は `chrono::DateTime<FixedOffset>`。
  - TLS は最初から（993）。rustls（ring）+ webpki-roots で OpenSSL なし。つなぎ・読み書きに 20 秒のタイムアウト。`ring` は ito-sftp と同じく Linux・macOS 向けのビルドで C のコンパイラーが要る。

### 使う側がやること

- パスワードは `ito-secret` から読んで `Inbox::open` に渡す（ファイルに置かない）。`poll` は止まるのでワーカースレッドから呼ぶ。
- フォルダー名は ASCII のまま送る（修正 UTF-7 にはしない）。

## [0.153.0] - 2026-10-31

### 追加

- `ito-common`（機能 `clock`）: 過去の時刻を時計の書式（`date_format`・`hour24`・`weekday`）で書く（uchmk/ito#99）。
  - `Clock::short(&at, &now, lang)`: 一覧の行向け。今日なら時刻だけ（`11:48`）、今年なら年なしの日付（`10/29`。`DD/MM/YYYY` は `29/10`、`YYYY-MM-DD` は `10-29`）、それより前は日付全部（`2025/10/29`）。「今日」「今年」は `now` のもの。
  - `Clock::stamp(&at, lang)`: 日付と時刻を全部（`2026/10/29 (木) 09:15`）。`date` が切れていても日付は出す（`date` は状態バーの時計だけの設定）。曜日は `weekday` に従う。
  - 時刻は状態バーの時計と同じく `%H:%M`（24 時間でなければ `%-I:%M %p`）で、ゼロ埋めする。

## [0.152.1] - 2026-10-31

### 変更

- `ito-sftp`: russh の機能 `rsa` を外した。`rsa` クレートには直っていない助言 RUSTSEC-2023-0071（Marvin、タイミング攻撃）があり、`cargo audit` が落ちていたため。`Cargo.lock` から `rsa` が消える。純 Rust のまま。

### 使う側がやること

- RSA のホスト鍵・RSA の鍵ファイルは使えなくなった（ed25519・ecdsa は使える）。RSA しか持たないサーバーが要るなら ito に知らせる。

## [0.152.0] - 2026-10-31

### 追加

- `ito-keyscript`: 窓の中の文字で押す・確かめる台本の語（uchmk/ito#98）。egui の AccessKit の木（ラベルと値）と、アプリが最後にコピーした文字を読む。
  - `<Click:文字>` / `<RClick:文字>`: その文字の物の真ん中を押す。完全一致を先に、なければ 1 つだけ含むもの。無い・2 つ以上なら、約 3 秒（`TEXT_WAIT`）見直してから失敗にして、窓にあるものを挙げる。`<Click:0.5,0.4>` の座標の形はそのまま使える。
  - `<Assert:文字>` / `<Assert:!文字>`: その文字が窓にある／ない。`!` で始まる文字そのものは `\x21`。
  - `<Clipboard:文字>`: アプリが最後にコピーした文字がこれと同じ。
  - 文字の書き方は `<Paste:>` と同じ（`\n \t \e \\ \xNN`。`>` は `\x3e`）。
  - 公開: `observe(ctx)`（木とコピーを読み始める。`Runner` は台本が使うときに自分で呼ぶ）、`items`・`locate`・`check`・`check_clipboard`、`Item`、`failed_report`、`FAILED_EXIT`（3）。`Press` に `ClickText`・`Assert`・`Clipboard`。

### 使う側がやること

- `Runner` を使うアプリ（yagura など）: `Do` に `Do::Failed { step, why }` が増えた。`match` に足し、`failed_report(&step, &why, runner.left())` を done ファイルに書いて `FAILED_EXIT` で終わる。
- `Press` を自前のループで流すアプリ（kura）: 起動で `observe(ctx)` を 1 回呼び、`Press::ClickText`・`Assert`・`Clipboard` を `locate`・`check`・`check_clipboard` で処理する（見つからないときは数フレーム見直してから失敗にする）。

## [0.151.0] - 2026-10-31

### 追加

- 新しいクレート `ito-service`: 本人のアカウントで動く Windows のサービス（uchmk/ito#102）。資格情報ストア・状態のフォルダー・`ito-ipc` の扉を、サインイン時に動かしたときと同じに保つ。Windows 以外は `Error::Unsupported`。
  - `run(name, main)`: SCM が起こしたプロセスなら Running を報告して `main(Stop)` を呼び、返り値を終了コードにして Stopped を報告する。`Stop` は STOP・SHUTDOWN・PRESHUTDOWN で立つ（`is_set`・`wait(timeout)`）。コンソールから起こすと `Error::NotService`。
  - `install(Install { name, display, description, exe, args, account, password, start })`（管理者が要る。無ければ `NeedsAdmin`）: アカウントに `SeServiceLogonRight` を付け、そのアカウントが管理者なしで start・stop・query できる DACL を設定する。`Start::AutoDelayed` は遅延自動開始。途中で失敗したらサービスを消す。パスワードは引数だけで、保存しない。アカウント無し・パスワード無し（PIN だけ）は `BadAccount`、パスワード違いは最初の start で `LogonFailed`。
  - `uninstall(name)`・`start(name)`（動作中は成功）・`stop(name, wait)`（止まっていれば成功、間に合わなければ `Timeout`）・`state(name) -> Option<Status { state, pid }>`（`None` は未インストール）。
  - `read_password(prompt)`: コンソールではエコーなし、パイプならそのまま読む。
  - 引数のクォート `command_line` と DACL の `access_sddl` は公開（純粋な関数でテスト済み）。

### 使う側がやること

- koyomi: `ito-service = { git = "https://github.com/uchmk/ito", rev = "…" }` を足し、`koyomi service run` で `ito_service::run("koyomi", |stop| engine(stop))`、`service install` で `read_password` → `install`。サービスの本人は `start`・`stop`・`state` を管理者なしで呼べる。

## [0.150.0] - 2026-10-31

### 追加

- 新しいクレート `ito-http`: アプリが自分の PC で開く小さな HTTP の待ち受け（受け取る webhook、スクリプトから叩く手元の REST）（uchmk/ito#100）。標準ライブラリだけの HTTP/1.1（依存は serde_json と ito-secret）。
  - `Server::start(Config::new(port, token), handler, log)`。`stop()` と drop で止まる。`port()` は 0 を渡したときの実際のポート。
  - 待ち受けは 127.0.0.1 だけ（`ipv6` で `::1` も）。LAN には出せない設定しか無い。
  - 全部のリクエストに `Authorization: Bearer <token>` を要る。定時間で比べ、無い・違うは本文を読む前に 401、ハンドラーは呼ばれない。トークンはログにも `Debug` にも出ない。`token_from_secret(app, name)` で `ito-secret` から読む。
  - 上限: 本文 `max_body`（既定 64 KiB、超えると本文を読まずに 413）、同時の接続 `max_connections`（既定 8、超えると 503）、読みの待ち `timeout`（既定 10 秒、408）。chunked は 501。ハンドラーの panic は 500 で、待ち受けは続く。
  - `handler(Request { method, path（%デコード済み）, query, headers（小文字）, body }) -> Response { status, json }`。`Request::json::<T>()`、`Response::ok/error`。
  - ログは 1 リクエストにつき `LogLine { method, path, status, millis }`（クエリは秘密が入りうるので出さない）。1 接続 1 リクエスト（`Connection: close`）、TLS なし。

### 使う側がやること

- koyomi: `ito-http = { git = "https://github.com/uchmk/ito", rev = "…" }` を足し、`POST /runs`・`GET /runs/<id>` を `Request` に直して ito-ipc の扉へ渡す。ハンドラーは接続のスレッドで呼ばれるので、長い仕事は受け付けて 202 を返す。

## [0.149.0] - 2026-10-31

### 追加

- 新しいクレート `ito-sftp`: SFTP で送る・取る、ふつうの同期 API（uchmk/ito#95）。中で russh + russh-sftp と tokio（Session ごとに 1 本のランタイム）を使うが、公開の API に async は出ない。
  - `Session::connect(&Target)`・`list(dir)`・`remove(path)`・`put(local, remote, Options, &stop, progress)`・`get(remote, local, ...)`。`*`・`?` のワイルドカード、送り先がフォルダー（末尾 `/` か既存）ならその中へ。
  - ホスト鍵は指紋（`SHA256:…`）を固定して照合し、違えば `HostKeyMismatch`。初回に指紋を見る `fingerprint(host, port, timeout)`。パスワードか鍵ファイル（パスフレーズ可）。`Debug` は秘密を出さない。
  - 書き込みは `<名前>.part` に書いてから名前を替える。上書きは `Options { overwrite }` で明示（既定は `Exists`）。失敗・停止で `.part` は消す。接続と I/O のタイムアウト、停止の `AtomicBool`（`Stopped`）、進みの `progress(name, done, total)`。
  - russh は `ring`（既定の aws-lc-rs ではなく）で入れる。Windows の MSVC はそのままビルドできるが、Linux・macOS 向けは C のコンパイラーが要る（Mac の外から macOS 向けは cargo-zigbuild。verify.sh が objc2 でやっているのと同じ）。
  - テストは 127.0.0.1 に russh のサーバーを立てて本物の SSH で通す。

### 使う側がやること

- koyomi: `ito-sftp = { git = "https://github.com/uchmk/ito", rev = "…" }` を足し、自前の SFTP の呼び出しを載せ替える。

## [0.148.0] - 2026-10-31

### 追加

- `ito-common::text`: 設定に書いた時間の長さを読む `parse_duration("30m" | "1h30m" | "90s") -> Result<u64, String>`（秒。単位は h・m・s を順に 1 回ずつ、大文字小文字と前後の空白は無視。数字だけ・空・順の違い・桁あふれは、書き方 `30m・1h30m・90s のように書く` を添えたエラー。0 は読めるので、0 を断る側が自分で見る）（uchmk/ito#76）。
- `ito-common::text::duration_text(secs)`: `parse_duration` が同じ秒に読み戻す一番短い文字（`1h30m`、`0s`）。
- `ito-common::text::duration_words(Duration, lang)`: 言語に合わせた時間の文字（ja: `12秒`・`41分`・`1時間5分`、ほかは英語: `12 s`・`41 min`・`1 h 5 min`）。大きい方の 2 つの単位まで、2 つ目が 0 なら出さない、日が最大（uchmk/ito#88）。`lang` は `common.toml` の言語の符号（`ja-JP` も可）。

### 使う側がやること

- koyomi: `koyomi-core/src/duration.rs` を `parse_duration`・`duration_text` に、`show.rs` の時間を `duration_words` に載せ替える。

## [0.147.0] - 2026-10-31

### 追加

- `ito-proc`: 別の Windows アカウントで子プロセスを動かす `run_lines_as(cmd, &Logon { user, domain, password }, stop, enc, on_line)`（uchmk/ito#97）。
  - `CreateProcessWithLogonW`（`LOGON_WITH_PROFILE`）。標準入出力は今までどおりこちらのパイプ、止めると子の木ごと終わる。子は一時停止で作り、専用のジョブ オブジェクトに入れてから動かす（別アカウントのプロセスは `taskkill` できないので、ジョブを終わらせる。こちらが死んでもジョブが閉じて終わる）。
  - 間違ったパスワード・ログオンの権限が無い、などはシステムの `io::Error` のまま返る。パスワードの幅広文字の領域は呼び出しの後で 0 で潰す。`Logon` の `Debug` はパスワードを `***` にする。
  - Windows 以外は `Err(Unsupported)`。昇格はしない（管理者のアカウントも UAC で絞られたトークン）。
  - `cmd` から使うのはプログラム・引数・作業フォルダー。環境変数は、アカウント自身のブロック（`LogonUserW` → `CreateEnvironmentBlock`）に `cmd.env(..)`・`env_remove(..)` を重ねたもの（名前は大文字小文字を区別しない）。`env` が無ければ `LogonUserW` はせず、ログオンのままの環境。プログラムはフルパスで渡す（探すのはこちらのプロセスから）。
  - 別のアカウントが要るテストは `#[ignore]`（`net user ito-proc-test <pw> /add` と環境変数 `ITO_PROC_TEST_USER` / `ITO_PROC_TEST_PASSWORD`）。

### 使う側がやること

- koyomi: `run_lines_with` の代わりに `run_lines_as` を呼ぶ。パスワードは `ito-secret` から読んだものをそのまま渡す。

## [0.146.0] - 2026-10-31

### 追加

- `ito-mcp`（uchmk/ito#96 の MCP の部分）:
  - `Tool::destructive()`（閉じる・消す。`annotations.destructiveHint`）と `Tool::needs_confirm()`（アプリの窓で人が確認してから動く。`_meta["ito/needsConfirm"]`）。どちらも `writes()` を含む。
  - `Opened`: エージェントが開いたものの集合（`insert`・`contains`・`remove`・`check`・`ids`）。閉じるツールが「自分が開いたものだけ」を閉じるのに使う。clone は同じ集合を見る。
  - `Phase`（`Running`・`Waiting`・`Done`・`Error`、`as_str`・`parse`・`is_finished`）と、上限つきの待ち `wait(timeout, every, check)`（`MAX_WAIT` = 50 秒。1 件ずつ答えるので、長い待ちは呼び直す）、秒の引数 `arg_secs`。

### 使う側がやること

- `Tool { … }` を構造体リテラルで作っていれば、`destructive: false, needs_confirm: false` の 2 欄を足す（`Tool::new` を使っていれば何もしない）。
- 閉じるツールに `.destructive()`、確認の箱を出すツールに `.needs_confirm()` を付ける。

## [0.145.0] - 2026-10-31

### 追加

- 新しいクレート `ito-tail`: ログ ファイルの新しい行を追う（uchmk/ito#94）。
  - `Follower::new(path, From::End | Start | Offset(n))` / `poll() -> io::Result<Vec<String>>` / `offset()`。毎回開いて閉じる（Windows は共有 読み・書き・削除で開くので、書く側の邪魔をしない）。
  - 改行で終わった行だけ返し、書きかけの行は次の `poll` へ。`offset()` に書きかけは入らないので、保存した位置からの再開で取りこぼしも重複もない。
  - 切り詰め（長さが位置より短い）と入れ替え（同じ名前の別のファイル）は先頭から読み直す。ファイルが無い間は空。UTF-8 でなければ Shift_JIS、BOM は外す。
- `ito-eventlog` に読む側: `Subscription::new(channel, &Filter { ids, source, level }, From::End | Start | After(record_id))` / `poll() -> Vec<Record>` / `position()`。
  - Windows の `EvtQuery`（`Application`・`System`・`Security` など）。`Record` は record_id・time（ISO 8601 UTC）・id・source・level・message（プロバイダーの文面。無ければ値をつないだもの）。
  - `position()`（最後に返した record_id）を保存し、`From::After` で返すと続きから読める。Windows 以外は `WhyNot::Unsupported`。

### 変更

- `WhyNot` に `Denied`（読む権限が無い。Security は管理者か Event Log Readers）と `NoChannel`（その名前のログが無い）を足した。

### 使う側がやること

- `WhyNot` を `match` で全部並べているなら、`Denied` と `NoChannel` を足す（`_` があれば不要）。

## [0.144.0] - 2026-10-31

### 追加

- 新しいクレート `ito-store`: アプリが大量に持つデータ（スレッドや項目のキャッシュ）を置く手元の SQLite（uchmk/ito#93、fumi の依頼。SQLite の `bundled` を許す方針）。
  - `Store::open(app, name, migrations)`（`state_dir(app)` の `<name>.sqlite3`）/ `open_at(path, migrations)`: WAL・busy timeout・`foreign_keys`。`migrations[i]` は版 i → i+1 の SQL で、`PRAGMA user_version` で未適用のものだけを順に（各 1 トランザクション）流す。出荷した移行は変えず足す。自分の版より新しいデータベースは開かない。
  - `write(|tx| ..) -> Pending<T>`: **書き込みのスレッドは 1 本**。トランザクションで包み、`Ok` なら commit、失敗なら全部 rollback。UI は `Pending::try_get` で後から見る（`wait` はワーカー向け）。`read(|conn| ..)` は別の読み取り接続（WAL なので書き込みを待たない）。
  - `meta_get` / `meta_set`: 同じデータベースの key-value。同期の位置（Gmail の `historyId` など）を、行と同じ `write` の中で書けば一緒に commit される。
  - `fts_sql(content, rowid, columns)`: 外部コンテンツの FTS5 表 `<content>_fts`（`tokenize='trigram'`、日本語が引ける）と、挿入・更新・削除に追従するトリガー、既存の行の索引づけの SQL（移行に入れる）。
  - `parse_query(input) -> Query` / `search_ids`: 打った文字の各語を引用して FTS5 の演算子（`AND`・`NEAR`・`*`・`-`・`:`・`"`）を文字として扱う。trigram は 3 文字未満の語に当たらないので、それは `Query::short` に分けて返す（アプリが `LIKE` で絞る）。
  - `describe()`: 場所・大きさ・スキーマの版の 1 行。`<app> env` と不具合の報告のファイルの欄に。
  - C のコンパイラーが要る（SQLite を同梱でビルドする）。verify.sh の 3 ターゲットで確かめた。
  使う側（fumi）がやること: 自前の rusqlite と接続の管理を `ito-store` に置き換える。

## [0.143.0] - 2026-10-31

### 追加

- `ito-prefs`: 一覧の選べる行 `nav_row(ui, look, NavRow)`（uchmk/ito#92、fumi の依頼。フォルダー・ブランチ・場所・カレンダーなど）。`NavRow { icon, text, count, strong, current }`: 先頭の記号（空なら無し）、文字、右端の薄い件数（`strong` ＝未読なら文字が強くなり、件数がアクセント色）、今いる行（アクセントの塗り）、ホバー、`button` と同じフォーカスの枠。列の幅いっぱいで高さはどの行も同じ。長い文字は件数の手前で「…」で切る。キーは持たない（アプリの移動に任せ、`current` を描いてクリックを返すだけ）。
  使う側がやること: `selectable_label` を `nav_row` に置き換える。

## [0.142.0] - 2026-10-31

### 追加

- `ito-prefs`: タグの色の 16 色の固定パレットと割り当て、色選び（uchmk/ito#91、tsumugi の依頼）。
  - `TAG_PALETTE`（`"#rrggbb"` が 16。先頭 8 は tsumugi の今の 8 色なので、保存済みの `[tags.colors]` はそのまま有効）、`tag_slot(hex)`（パレットの何番目か）、`parse_hex`。
  - `settle_tag_colors(tags, own) -> Vec<(tag, hex)>`: `own`（タグ → hex）にまだ色のないタグだけに色を割り当てる。名前の FNV-1a（32 ビット）の番号の色が他で使われていなければそれ、使われていれば最初の空き、空きがなければ最も共有の少ない色。`{` を含む名前は飛ばす。結果をアプリが 1 回だけ設定（`[tags.colors]`）に書けば、再起動しても色が変わらない。
  - `tag_picker(ui, look, current) -> Option<&'static str>`: 設定画面向けの色見本の列（今の色に輪）。押された色を返す。
  - 色は `tag_colors` でテーマに合わせて直すので、16 色とも 20 テーマの明暗で文字のコントラスト 4.5 以上。
  - 使う側（tsumugi）がやること: 自前の `TAG_PALETTE`・`tag_slot`・`settle_tag_colors` と色選びを消して、これに置き換える。色の表示は `Tag { color: parse_hex(..), .. }` を `tag` / `paint_tag` に渡す。

## [0.141.0] - 2026-10-31

### 追加

- `ito-prefs`: 色つきのタグ（チップ）の部品（uchmk/ito#90、fumi の依頼）。`Tag { text, color, on }` を `tag(ui, look, Tag)`（ウィジェット。`button` と同じくキーとフォーカスの枠を持つ。クリックは呼ぶ側）、`paint_tag(painter, ctx, &colors, at, Tag, max_width)`（widget を使わず描く一覧の行向け。長い文字は「…」で切る）、`tag_width`（幅を測る）で描く。
  色は外から（Gmail の色など。何色でもよい）か、`None` ならテキストから安定して選ぶ（`tag_hue`）。どちらもテーマに合わせて直す（`tag_colors`。egui なしで試せる）ので、20 の組み込みテーマの明暗すべてで文字のコントラストが 4.5 以上になる。`on` は選んだ状態で、色で塗りつぶす。
  使う側がやること: 特になし（足しただけ）。

## [0.140.0] - 2026-10-31

### 追加
- 新しいクレート `ito-eventlog`: 監視ツール（SCOM・Zabbix・イベント ビューアー）が見られるシステムのログに 1 行書く。`Log::open(source)` と `log.write(Level, id, text)`（`Level` は `Info`・`Warning`・`Error`）。
  Windows はアプリケーション ログに `ReportEventW` で書く（ソースの登録は要らない。登録しないと、イベント ビューアーは「説明が見つかりません」の注記つきで本文を出す。登録は管理者がレジストリに `EventMessageFile` を置く。このクレートはしない）。
  macOS・Linux は `logger` で syslog へ（ID は本文に入る）。書けないときは `WhyNot`（uchmk/ito#86）。

## [0.139.0] - 2026-10-31

### 追加
- `ito-power`: 実行条件の調べ方 `on_ac()`（電源につながっているか。電池のない PC は `true`）・`network_up()`（ローカルかインターネットにつながっているか。ループバックは数えない）・`idle_for()`（キーボード・マウスを触っていない時間）。分からないときは `None`。
  Windows は `GetSystemPowerStatus`・Network List Manager・`GetLastInputInfo`、macOS は `pmset`・`route`・`ioreg`、Linux は `/sys/class`（無操作時間は `xprintidle` があれば X11 のみ）（uchmk/ito#87）。

## [0.138.0] - 2026-10-31

### 追加
- `ito_openin::open_default(path)`（と `default_argv`）: OS 既定のアプリでファイル・フォルダーを開く（`explorer` / `open` / `xdg-open`。窓なし。無いパスは `Err`）（uchmk/ito#85）。
- `ito-proc`: 起動した子を、このプロセスのジョブ オブジェクト（`KILL_ON_JOB_CLOSE`）に入れる。アプリが落ちても、子とその子孫は OS が殺す（Windows のみ。Unix は従来どおり `kill_tree` のときだけ）。`spawn_detached` の子は入れない（uchmk/ito#83）。
- `ito_common::single::acquire(app, scope) -> Result<Guard, Held{pid}>`: 状態のフォルダーのロックで、アプリ（の一部）を 1 つだけ動かす。落ちても殺されても OS が外す。状態のフォルダーが使えないときは、ロックなしで `Ok`（uchmk/ito#84）。

## [0.137.2] - 2026-10-30

### 変更
- 雛形（`templates/app/CLAUDE.md`）に「ボタンは ito-prefs のものを使う」を追記した（`button`・`button_with` と `Tone`・`seg`・`ito-confirm`。`egui::Button` を自前で作らない）。

## [0.137.1] - 2026-10-30

### 修正
- `ito_prefs::new_mail` を再エクスポートした（0.137.0 で書き忘れ。koyomi の指摘）。

## [0.137.0] - 2026-10-30

### 追加
- 設定画面の Notifications に、メールの通知先 `[[notify.mails]]` の欄（uchmk/ito#81、koyomi の依頼）。`ito-prefs`。
  - `mail_rows(ui, look, words, &mut Vec<Mail>, secret_cmd, &mut drafts) -> Option<usize>`: 通知先ごとに「外す」「ここに試しに送る」、サーバー・ポート・差出人・宛先（コンマ区切り）を編集できる。パスワードは値を出さず、`secret:<名前>` の参照と、それを入れるコマンド（`{secret_cmd} <名前>`）だけ。ファイルに直に書かれたものは `••••` と警告を出す。「メールの送り先を足す」で `new_mail()` を足す。押された「試しに送る」の番号を返す。
  - `write_mails(text, &[Mail], states)`: `[[notify.mails]]` を書き戻す（コメントは残る。587 の `port`・空の `user` は書かない。パスワードは `secret:` の参照だけ書き、平文は書き戻さない）。
  - `mail_label`・`new_mail`、`Words` に `mail`・`mail_host`…`mail_more`（EN・JA）、`notify_index` に検索の語を追加。
  - 使う側がやること: `Words` を `EN`／`JA` でなく自前の struct 式で作っているアプリは、`mail*` の欄を足す。`Mail` は `NotifyCfg::mail_places` から取り、押された試し送りは `ito_prefs::test_mail`（ワーカースレッド）へ。保存は `write_mails`。`Notify`／`Test` は変えていない。

## [0.136.0] - 2026-10-30

### 追加
- メール（SMTP）の通知先 `[[notify.mails]]`（uchmk/ito#80、koyomi の依頼）。`ito_notify::mail` と `ito_prefs::MailCfg`。
  - 送るのはシステムの `curl`（新しい依存なし）。webhook と同じく別スレッドで、失敗は 30 秒後に 1 回やり直し、それでも駄目なら stderr と `mail::on_failure` に出す。呼ぶ側を待たせない。
  - 常に暗号化する。465 は最初から TLS（`smtps://`）、それ以外は STARTTLS を必須（`--ssl-reqd`）。
  - パスワードは `secret:<名前>` の参照だけ。ファイルに直に書くと警告して送らない。`NotifyCfg::reveal(app)` が資格情報ストアから読み戻す（`secret` 欄）。`curl` には標準入力の設定で渡すので、プロセス一覧に出ない。`Debug` にも出ない。
  - 件名 = 題、本文 = 本文（UTF-8 を base64、日本語の件名・差出人名は RFC 2047 で符号化、題の改行はヘッダーの注入にならない）。宛先は `koyomi <jobs@example.com>` の形も読む。
  - `Mail::problems`（host なし・port 0・from/to の形・パスワードの書き方・知らない出来事）、`NotifyCfg::check` が `[[notify.mails]] N:` を付けて警告し、同じ `name` も警告する。
  - `NotifyCfg::tell`／`tell_open` は webhook のあとにメールも送る。`only`／`unnamed` は名前でメールの通知先も選ぶ。
  - 「テストを送る」は `ito_prefs::test_mail(app, &Mail, level, title, body)`（パスワードを読むのでワーカースレッドから）。
  - 使う側がやること: `NotifyCfg` の `mails`／`mail_places` が増えただけで、既存の `[notify]` は変わらない。パスワードは `ito_secret::set(app, "mail-ops", …)` で入れる。設定の画面への表示は 0.137.0。

## [0.135.0] - 2026-10-30

### 追加
- 通知先（`[[notify.webhooks]]`）に名前（`name`）を付けられる（uchmk/ito#79、koyomi の依頼）。アプリが 1 つの通知を一部の通知先だけに送れる。
  - `HookCfg`・`ito_notify::webhook::Place`・設定画面の `Hook` に `name: String`（書かなければ空 = 名前なし）。設定画面の書き戻し（`write_webhooks`）は、名前があれば `name = "…"` を書く。
  - `NotifyCfg::check` は、同じ名前の通知先が 2 つあれば警告する。
  - `NotifyCfg::only(&["accounting"])` は、その名前の通知先だけに送る写しを返す。`NotifyCfg::unnamed()` は名前のない通知先だけの写し。どちらも `check` のあとに呼ぶ。名前のない通知先をどう扱うかはアプリが決める。
  - `webhook::secret_name(i, &place)`: 資格情報ストアの名前。名前があれば `webhook-accounting`、なければ今までどおり `webhook-1`（位置）。`conceal` と、隠すのをやめるときの削除がこれを使う。

### 変更
- 使う側がやること: `Place { … }`・`Hook { … }` を欄ごとに書いているところに `name` を足す（`Place` は `..Place::default()` でもよい）。名前を書かないアプリの動きは変わらない。

## [0.134.1] - 2026-10-30

### 修正
- `ito-notify` の `toast::show` が、Windows で 2 回目の呼び出しでプロセスごと落ちるのを直した（uchmk/ito#78、koyomi が見つけた）。
  トーストごとのスレッドが MTA の最後の使い手として抜けると COM が MTA を壊し、windows-rs が持ち回る activation factory が消えたアパートメントを指していた。
  プロセスで 1 回 `CoIncrementMTAUsage()` を呼んで MTA を生かしておく。使う側がやることはない。

## [0.134.0] - 2026-10-30

### 追加
- 新しいクレート `ito-tray`（uchmk/ito#73、koyomi の依頼）: 窓が無くても動き続けるアプリのタスクトレイのアイコン。
  `Tray::new(icon, tip)`（`Icon::new(w, h, rgba)`）、`set_state(Some(State))`（アイコンの右下に色の丸と白い記号のバッジ。ツールチップの頭にも `State::mark`。色だけに頼らない）、
  `set_tip`、`set_menu(Vec<Item>)`（`Item::entry(id, label)`・`Separator`・無効・チェック）、`events()`（`Event::Click`・`Event::Menu(id)`。`try_recv` で読む）。
  `Tray::new_with(icon, tip, wake)` は、イベントを積むたびに `wake` を呼ぶ（egui なら `ctx.request_repaint()`）。落とすとアイコンが消える。
  Windows は専用スレッドと隠れた窓で `Shell_NotifyIconW`（エクスプローラーが落ちて戻ったときは付け直す）。macOS・Linux は `Tray::new` が `Unsupported` を返す（メニューバー・StatusNotifierItem は後で）。
  `with_badge(&Icon, State)` と `badge_color` も公開。

## [0.133.0] - 2026-10-30

### 追加
- 新しいクレート `ito-power`（uchmk/ito#72、koyomi の依頼）: 電源と眠り。
  `Awake::hold(reason)`（ガード。落とすと PC がまた眠れる。`is_active` で効いたか見る）、
  `wake_at(when) -> Result<WakeHandle, WhyNot>`（落とすと取り消し。`WhyNot` は `TimersDisabled`・`OnBattery`・`Unsupported`・`InThePast`・`Failed(os エラー)` で、文にもなる）、
  `sleep_after() -> Option<Duration>`（今の電源の「スリープまで」。眠らない設定や不明は `None`）。
  Windows は電源要求（`powercfg /requests` に理由が出る）・再開つきの待機タイマー・有効な電源プラン（「スリープ」「起動タイマーの許可」）。
  macOS は `caffeinate -i`、Linux は `systemd-inhibit` の子プロセスで眠りを止める（落とすと殺す）。この 2 つで起こすのは未対応（`Unsupported`、`sleep_after` は `None`）。

## [0.132.0] - 2026-10-30

### 追加
- `ito-proc`: 子の出力の文字コードを選べる（uchmk/ito#74、koyomi の依頼）。日本語の Windows の `.bat` や古い exe は CP932 で書くので、UTF-8 だけだと文字化けする。
  `Encoding`（`Utf8` が既定、`ShiftJis`、`Console`（Windows は `GetOEMCP` のコードページ。ほかは UTF-8）、`Auto`（1 行ごとに、UTF-8 として正しければそのまま、でなければ `Console`））。
  `run_with` / `run_lines_with` は `Encoding` を受ける。`Done::stdout_as(enc)`。生のバイトは今までどおり `Done::stdout`。
  `run` / `run_lines` / `Done::stdout_text` は変わらない（UTF-8、壊れたバイトは置き換え）。使う側は必要な所で `*_with` に替えるだけ。
- `ito-proc` が `encoding_rs`（純 Rust）に依る。

## [0.131.1] - 2026-10-30

### 修正
- `ito-confirm`: 確認の箱が毎フレーム縦に伸びて窓から出る不具合（uchmk/ito#71、koyomi の報告）。ボタンの行が、前のフレームの箱の残りの高さを全部取っていた。
  行を自分の高さだけにし、箱が窓に入らないときは、行・本文・差分・注・カードを スクロールにして、タイトルとボタンとヒントは見える所に残す。
  背の高い箱のあとに短い箱を出すと、短くなる。使う側がやることはない。

## [0.131.0] - 2026-10-29

### 変更
- 新しいアプリの雛形（`templates/app`）に `ito-statusbar` を入れた。`Cargo.toml` の依存、下のパネル（`status_bar`: 左にアプリの項目、右端に時計）、
  倍率を変えたときの `scale_changed`。`new-app` で作ったのにステータスバーが無い、が毎回起きていたため。
- `scripts/lanes/ito-required.txt` に `ito-statusbar` を足した。`ito-first.py` が、ステータスバーの無いアプリを落とす。
  使う側がやること: rev を上げて `scripts/lanes.sh sync` を流すと落ちる。バーを付ける（`ito_statusbar::show`）か、
  要らないアプリは `scripts/ito-first.allow` に `ito-statusbar <理由>` を書く。

## [0.130.0] - 2026-10-29

### 追加
- `ito-pane`: 検索の一致を選択と別の色で描く（uchmk/ito#70、tsumugi の依頼）。`CellView::found`（今の一致の中。そこでは `selected` を外す）、
  `Palette::found`（一致の背景。文字は黒か白のうち読める方、太字。kura の find_keyword と同じ流れ）、`ito-theme` の `Colors::palette` は `wait` を渡す。
  `Terminal::found() -> Option<(Point, Point)>` は今の一致の始まりと終わり（選択がまだその一致のときだけ。ドラッグ・クリック・`end_search` のあとは `None`）で、
  「3 / 12」のように何番目かを数えるアプリが、自分の持つ一致の一覧と突き合わせるのに使う。数え上げは `ito-pane` に入れない。
  使う側がやること（破壊的）: `CellView { … }` を直に組んでいれば `found: false`、`Palette { … }` を直に組んでいれば `found` を足す（`..Palette::default()` なら不要）。

## [0.129.0] - 2026-10-29

### 追加
- `ito-proc`: `spawn_detached(cmd)`（uchmk/ito#69、koyomi の依頼）。常駐させる子（エンジン・デーモン）の起こし方。stdio は null、窓なし、自分のプロセスグループ
  （Windows は detached）で、待たない。Windows の `Command` は `Stdio::null()` でも呼び側の継承できるハンドルを全部子に渡し、呼び側の stdout/stderr のパイプを
  子が握り続けて待つ側（テスト・`$(app list)`・`.com`）が EOF を見られなくなる。そこで起こす間だけ呼び側の標準ハンドルを継承しない印にして、あとで戻す。
  使う側がやること: koyomi は `client.rs` の `keep_own_handles` と起こし方の `creation_flags` を外して `ito_proc::spawn_detached` に替える。

## [0.128.0] - 2026-10-29

### 追加
- `ito-prefs`: 通知の画面の Webhook に「アドレスを OS の資格情報ストアに置く」スイッチ（uchmk/ito#67）。設定は `[notify] hide_webhooks`（既定は false）。
  `NotifyCfg::webhooks_to_write(app, events, page)` が、書くべき Webhook を返す（スイッチが入っていれば `conceal` 済みで、切ったときはストアの `webhook-N` を消して
  アドレスをそのままに戻す）。変えていなければ `None`。ストアを触るのでワーカースレッドから呼ぶ。
  使う側がやること（破壊的）: `Notify { … }` を直に組んでいれば欄 `hide: bool` を足す。自前の `Words` を組んでいれば `webhook_hide`・`webhook_hide_note` を足す
  （`..EN` / `..JA` なら不要）。`ito-prefs` は `ito-secret` に依る。

## [0.127.0] - 2026-10-29

### 追加
- `ito-proc`: `run_lines(cmd, stop, on_line)`（uchmk/ito#65、koyomi の依頼）。`run` と同じ止め方（`stop` で子の木ごと殺す）で、stdout と stderr を
  1 行ずつ `on_line(Stream, &str) -> bool` に渡す（stderr は CR・LF で区切る）。`true` を返した行だけ `Lines { status, kept }` に残る。
  ログに書く・画面に流すだけなら何も残さずに済み、時間切れで殺された子の言ったことも `kept` に残る。`run`・`Outcome`・`Done` は変えない。
- `ito-secret`（新しいクレート。uchmk/ito#67、koyomi の依頼）。秘密（Webhook のアドレスは token を含む、ジョブのパスワードなど）をファイルでなく OS の
  資格情報ストアに置く。`get`・`set`・`delete(app, name)`（ストア上の名前は `uchmk/<app>/<name>`）。Windows は資格情報マネージャー、macOS はキーチェーン、
  Linux は `secret-tool`（値は stdin で渡し、コマンドラインに載せない）。ストアが無い所は `ErrorKind::Unsupported` の `Err` で、ファイルには退避しない。
  `mask(text, secrets)` は秘密を `***` に置き換える（長い順）。ストアは待つことがあるのでワーカースレッドから呼ぶ。
- `ito-notify::webhook`: `secret:<名前>` の参照。`conceal(app, places)` がアドレスをストアに移して `secret:webhook-N` に置き換え（ファイルに書く側）、
  `reveal(app, places)` が読み戻す（読めないものは空のアドレス＝送らず、警告の行を返す）。`reference(url)`。`Place::problems` は参照を警告しない。
- `ito-prefs`: `NotifyCfg::reveal(app)`（`check` のあとに呼ぶ）。`[notify] webhook` と `[[notify.webhooks]]` の `url` に参照を書いても警告しない。
  使う側がやること: 無し（足しただけ）。参照を使うなら、起動時に `check` のあとワーカースレッドで `reveal` を呼び、書くときは `conceal` の結果を `write_webhooks` に渡す。
  設定の画面の「Webhook を隠す」スイッチは文言の欄を増やすので別の版で入れる。

## [0.126.0] - 2026-10-29

### 追加
- `ito-diff`（新しいクレート。uchmk/ito#68、koyomi の依頼）。2 つの文字列の行の差分。純 Rust（`similar`）。Git は知らない。
  `diff_lines(old, new) -> Vec<Line>`（`Line { kind: Same | Added | Removed, old_no, new_no, text }`。行末は含まない）、
  `fold(&lines, context) -> Vec<Item>`（変更の前後 `context` 行だけ残し、続く変わらない行は `Item::Folded(n)` にする。1 行だけの塊は畳まない）、
  `diff(old, new, context)`（上の 2 つを続けたもの）、`counts(&lines) -> (追加, 削除)`、`Kind::mark()`（`+`・`-`・空白。色だけに頼らない印）。
- `ito-confirm`: `Question::diff: &[ito_diff::Item]`。本文の下に差分を等幅で描く（行番号の溝・`+`/`-` の印・緑/赤・畳んだ所は「… N lines」・高さ 240 を超えたらスクロール）。
  使う側がやること: `Question { … }` を全部の欄を書いて作っているところは `..Default::default()` を足す（欄が 1 つ増えた）。`ito-confirm` が `ito-diff` に依る。

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
