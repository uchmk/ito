# 変更履歴

このファイルの形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版のつけ方は
[セマンティックバージョニング](https://semver.org/lang/ja/) に従う。版はルートの `Cargo.toml` の `[workspace.package]` の `version` が正。

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
