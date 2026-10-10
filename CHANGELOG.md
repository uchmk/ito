# 変更履歴

このファイルの形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版のつけ方は
[セマンティックバージョニング](https://semver.org/lang/ja/) に従う。版はルートの `Cargo.toml` の `[workspace.package]` の `version` が正。

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
