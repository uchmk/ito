# uchmk のアプリの共通仕様

tsumugi・yagura（旧 mimamori）・kura（旧 filer）、これから作る uchmk のアプリが揃えるもの（2026-10-10、持ち主の依頼で決めた）。
**common.toml を変えれば、uchmk のアプリが一括で変わる**ようにする。コードは ito の共有のクレートにあり、
どのアプリも `Cargo.toml` の `rev` で固定して読む（直すときは ito で直して push し、使う側の `rev` を上げる）。
2026-10-11 に tsumugi の `docs/common-spec.md` からここに移した。同じ日に倍率・標準のキー・置き場所・環境変数・CLI を揃えた（ito 0.3.0）。
**新しいアプリはこの文書を既定にする。**

| クレート | 持つもの |
| --- | --- |
| `ito-common` | 設定と状態のフォルダー（`base_dir`・`app_dir`・`state_dir`・`env_dir`・`move_old`）、字体の並び（`fonts`）、common.toml の読み書き（`Common`・`CommonChange`・`edit_common`）、倍率（`tidy_scale`・`scale_step`・`parse_scale`・`scale_text`・`scale_change`）、TOML の 1 キーの書き換え（`set_key`・`remove_key`・`set_tables`、toml_edit でコメントを残す）、`write_atomic`、ファイルの見張り（`watch`）。機能 `clock` で時計の文字（`Clock::text`・`full`・`next_minute`、chrono） |
| `ito-theme` | テーマ（`Colors`・`Theme`・組み込みの 13 個・`from_table`・`pick`・`mix`・`contrast`）。egui の `ecolor` だけに依る。機能 `pane` でターミナルの `Palette` |
| `ito-prefs` | 設定の画面の枠と部品、共通のページ（`language_row`・`scale_row`・`clock_card`・`theme_page`）。文字は `Words`（`EN`・`JA`、`Words::of(lang)`） |
| `ito-i18n` | 言語の表（`tr`）。`base_dir` などは `ito-common` の再エクスポート |
| `ito-keys` | yazi のキーの書き方（`<C-a>`・`<A-S-Up>`。`C` は Windows と Linux で Ctrl、macOS で Cmd）、egui のキーの変換（`from_egui`）、画面に出す名前（`Key::human`） |

## 置き場所

`<config>` は Windows が `%APPDATA%`、macOS が `~/Library/Application Support`、Linux が `$XDG_CONFIG_HOME`（無ければ `~/.config`）。
`UCHMK_CONFIG_DIR` があれば `<config>/uchmk` の代わりにそこを使う（`ito_common::base_dir`）。

| ファイル | 中身 |
| --- | --- |
| `<config>/uchmk/common.toml` | 全部のアプリが読む共通の設定（下） |
| `<config>/uchmk/<app>/config.toml` | アプリごとの設定（`ito_common::app_dir(app)`）。全部のアプリで同じ形 |
| `<config>/uchmk/<app>/keymap.toml` | キーを変えられるアプリのキーの上書き（yagura の形が既定。下の「キー」） |
| `<config>/uchmk/themes/*.toml` | 自分で作ったテーマ。どのアプリの一覧にも出る |

- 状態（セッション・履歴・窓の位置と大きさなど）は設定と分け、`ito_common::state_dir(app)` に置く:
  `<APP>_STATE_HOME`、無ければ Windows は `%LOCALAPPDATA%\uchmk\<app>`（ローミングしない）、
  macOS は `~/Library/Application Support/uchmk/<app>/state`、Linux は `$XDG_STATE_HOME`（無ければ `~/.local/state`）`/uchmk/<app>`。
- **古い場所からは自動で移す。**起動時に `ito_common::move_old(old, new)` を呼ぶ（古いものがあり、新しいものが無いときだけ名前を変えて移す。
  新しいものは決して上書きしない）。移したら 1 行の通知を出す。
  - tsumugi: `<config>/tsumugi/settings.toml` → `<config>/uchmk/tsumugi/config.toml`（0.83）
  - kura: `<config>/uchmk/kura/kura.toml` → `config.toml`（0.96）

## 環境変数

| 変数 | 意味 |
| --- | --- |
| `UCHMK_CONFIG_DIR` | `<config>/uchmk` の代わりのフォルダ（common.toml・themes・各アプリのフォルダ） |
| `<APP>_CONFIG_HOME` | そのアプリの設定のフォルダ（`app_dir` が読む）。例 `KURA_CONFIG_HOME`・`TSUMUGI_CONFIG_HOME`・`YAGURA_CONFIG_HOME` |
| `<APP>_STATE_HOME` | そのアプリの状態のフォルダ（`state_dir` が読む） |
| `<APP>_KEYS_DONE` | `--keys` の終わりに状態を書くファイル（下の「コマンドライン」） |

- 空の値は無いのと同じ（`ito_common::env_dir`）。
- 古い名前は当面読む: tsumugi の `TSUMUGI_SETTINGS`（ファイル）と `TSUMUGI_STATE`。
- アプリ固有の 1 回だけの上書き（kura の `KURA_SCALE` など）は残してよいが、設定ファイルには書かない。

## common.toml

```toml
language = "auto"          # auto / en / ja
theme = "dark"             # テーマの名前、または dark / light / system（OS に合わせる）
dark_theme = "tsumugi Dark"   # system で OS が暗いとき
light_theme = "tsumugi Light" # system で OS が明るいとき
scale = 1.0                # 画面の倍率。0.2 から 5.0、0.1 刻み

[clock]
show = true
hour24 = true
date = true
date_format = "YYYY/MM/DD"  # YYYY/MM/DD, YYYY-MM-DD, MM/DD/YYYY, DD/MM/YYYY
weekday = true
```

- 無いキーは既定の値。知らないキーと知らない `date_format` は警告に出す（読み込みは止めない）。値の型が違えば読めない理由を出す。
- **テーマ・時計・倍率は common.toml だけに置く。**アプリごとの上書きは持たない。
  - 移行: tsumugi の config.toml に古い `theme`・`dark_theme`・`light_theme`・`[clock]` があり、common.toml に無ければ古い値を使う
    （`Common::theme_choice(Some(..))`・`Common::clock_or(Some(..))`）。設定の画面で書いた値は common.toml に入る。
- **言語だけは**アプリの config.toml に `language` があればそちらが勝つ（`ito_i18n::resolve`）。
- アプリは common.toml とテーマのフォルダーの時刻を **2 秒ごとに** UI ではないスレッドで見て、変わったら読み直し、すぐ当てる。

## 倍率

- 画面全体の倍率は common.toml の `scale`（`Common::scale_or_one()`）。egui では `ctx.set_zoom_factor(s)` で当てる。
  起動時と、common.toml の見張りが変わりを知らせたときに当てる。
- egui の組み込みの倍率のキーは切る（`ctx.options_mut(|o| o.zoom_with_keyboard = false)`）。キーは keymap から ito の関数で動かす:
  `scale_step(now, ScaleTo::In | Out | Reset)` で次の値を作り、`edit_common(scale_change(next))` を UI ではないスレッドで書く。
  書いた common.toml を見張りが拾うので、同じ机のほかのアプリも 2 秒以内に同じ倍率になる。
- 変えたらステータスバーに `scale_text(s, lang)`（「Scale 120%」「倍率 120%（最大）」）を出す。
- 設定の画面の General に `scale_row`（50–200% を 10% 刻み、それに 250・300・400%）を置く。
- **ターミナル（tsumugi）は例外**: `<C-=>` `<C-->` `<C-0>` はターミナルの字の大きさに使う。common.toml の `scale` は読んで当てるだけ
  （設定の画面の `scale_row` からは書く）。

## キー

`C` は Windows と Linux で Ctrl、macOS で Cmd（macOS では Ctrl を押しても同じ）。キーを変えられるアプリは yazi の書き方の keymap.toml（`ito-keys`）で持つ。
組み込みの既定を `defaults/keymap.toml` に置き、`<app>/keymap.toml` に書いた分だけ上書きする（yagura の形）。

### keymap.toml の形

**yazi の形を標準にする**（kura は yazi の keymap.toml をそのまま読むので、ほかのアプリもこれに合わせる）:

```toml
# C = Ctrl on Windows and Linux, Cmd on macOS (where Ctrl works too).
# <C-,> is Ctrl+, (Cmd+, on a Mac).
[mgr]
prepend_keymap = [
  { on = "<C-,>", run = "settings", desc = "Open the settings" },
  { on = ["g", "h"], run = "cd ~", desc = "Go home" },
]
```

- 層（`[mgr]`・`[main]` など、アプリが決める）ごとに `keymap`（全部を置き換える）・`prepend_keymap`（前に足す）・`append_keymap`（後ろに足す）。
  1 つは `{ on, run, desc }`。`on` は 1 つのキーか、続けて押すキーの配列。macOS だけ違うキーは `on_mac`。
- `[[mgr.keymap]]` の表の配列で書いても同じ（TOML として同じもの）。
- **`C` の書き方**: Ctrl も Cmd も `C` と書く（どの OS の人にも読める。ito 0.6.0 から）。
  - `ito_keys::from_egui` は macOS の Cmd と Ctrl をどちらも `C` にする。macOS で Ctrl と Cmd を別のキーに分けない。
    `<D-…>`（Cmd だけ）は yazi の書き方として読むが、合うキーは来ない。
  - 組み込みの keymap.toml と、アプリが作る雛形の頭に上の 2 行のコメントを置く。
  - 画面（ヘルプ・パレット・設定・ステータスバー）には OS の名前（`Ctrl+,`・`⌘,`）で出す（`Key::human`）。
  - ito 0.5 の `<C-…>`・`<primary-…>` も `C` と同じに読む。
- tsumugi の settings.toml の `[keys]`（`"Ctrl+Shift+T"` の形）は古い形。keymap.toml に移す（tsumugi の TODO）。

| 役割 | キー | 補足 |
| --- | --- | --- |
| 設定の画面 | `<C-,>` | 開いている間にもう一度押すと閉じる |
| ヘルプ | `<F1>` | アプリ固有の別のキー（kura の `~`、yagura の `?`）は残してよい |
| 倍率 | 大きく `<C-=>`・`<C-+>`・`<C-;>`、小さく `<C-->`、戻す `<C-0>` | 上の「倍率」。tsumugi は字の大きさ。下の「JIS の配列」 |
| コマンドパレット | `<C-S-p>` | パレットを持つアプリ |
| 終了 | `<C-q>` | ターミナルの tsumugi は除く（シェルに渡す） |

- アプリ固有のキーは自由。ただし上の表のキーをほかの役割に使わない。
- **JIS の配列**: `=` は Shift+`-`、`+` は Shift+`;` にあり、US の Ctrl+= の位置では押せない。そこで大きくするキーを 3 つ並べる:
  - `<C-+>`: JIS の Ctrl+Shift+;、US の Ctrl+Shift+=、テンキーの +。`ito_keys::from_egui` は記号の Shift を字に含めて渡すので、
    `<C-S-+>` と書かなくてよい（書くと合わない）。
  - `<C-;>`: JIS の Ctrl+;（Shift なし。+ の刻まれたキーで、Windows の VK_OEM_PLUS。ブラウザもこれで大きくする）。
  - `<C-=>`: US の Ctrl+=（JIS では Ctrl+Shift+- でも届く）。
  - 小さくする `<C-->` と戻す `<C-0>` はどちらの配列でも同じ位置。

## 設定の読み直し

- アプリは自分の config.toml（と keymap.toml）も見張り、変わったら自動で読み直して当てる（common.toml と同じ 2 秒）。
- 読めなければ前の値のまま、理由を警告に出す。手で読み直すキー（kura の `<C-F5>`）は残してよい。

## コマンドライン

| 引数 | 動き |
| --- | --- |
| `--version`・`-V` | `<app> X.Y.Z` を 1 行出して終わる |
| `--help`・`-h` | 使い方を出して終わる |
| `--keys "<script>"` | 実機のテスト用。起動したあと、アプリの中でキーを順に押す（kura・yagura の `keyscript.rs` の書き方）。終わったら `<APP>_KEYS_DONE` のファイルに状態を書く |

使うアプリだけが持つ下位のコマンド（必須ではない。持つならこの名前にする）:

| コマンド | 動き |
| --- | --- |
| `<app> mcp` | MCP のサーバとして標準入出力で動く（AI の CLI から操作させる） |
| `<app> shell-hook pwsh\|bash\|zsh` | シェルに足す 1 行を出す（今のフォルダをアプリに知らせるなど） |

## 字体

- 字体の並びは `ito_common::fonts` に 1 つだけ置き（kura の順）、全部のアプリが同じ順に読む:
  - `NERD_FONTS`（HackGen35 Console NF から JetBrains Mono NF まで 6 つ）を `font_dirs`（ユーザーの字体のフォルダ → システム）で探す（`nerd_font`・`find_file`）。
  - 日本語は `JAPANESE`（Windows はメイリオ → 游ゴシック → MS ゴシック）、太字は `JAPANESE_BOLD`。`read_first` で最初に読めたものを使う。
- 読むのは数十 MB なので UI ではないスレッドで。egui に渡す（等幅・プロポーショナルのどこに足すか）は各アプリ。

## 窓

- 最初の窓の大きさは **1280×800**（論理ピクセル）。
- 窓の位置と大きさは、閉じるときにアプリの状態のフォルダ（`state_dir`）に書き、次に開くときに戻す。
  戻す位置が今の画面の外なら既定に戻す。

## 設定の画面

- 開くキーは **Ctrl+,**（macOS は Cmd+,。keymap では `<C-,>`）。開いている間にもう一度押すと閉じる。窓いっぱいに出し、サイドバーやステータスバーは隠す。
- 左に幅 240 の帯: 上から検索、ページの一覧、一番下に「Open <file>」のボタン（そのアプリの設定ファイルを OS の既定のアプリで開く）。
- 右はページ: 題（24pt）とひとこと（13.5pt）の下に、見出しの付いたカードを並べる。カードの中の行は高さ 48、左にラベルと注、右に操作の部品
  （`switch` / `select` / `field` / `button` / `keycap`）。
- キー:
  - Esc は 2 段（部品から離れる → 閉じる）。
  - Ctrl+Tab・Ctrl+PageDown で次のページ、Ctrl+Shift+Tab・Ctrl+PageUp で前のページ。
  - Ctrl+F（macOS は Cmd+F）で検索へ。Tab で部品を移る（帯とページの一覧には止まらない）。Space でスイッチを切り替える。
  - 画面の中のキーは後ろ（ペインや一覧）に届けない。
- 検索は `Nav::index`（ページ番号と、そのページの行の言葉）で絞る。行を足したら index にも足す（tsumugi のテストが食い違いを落とす）。
- 共通の行は共通のクレートが描き、`CommonChange` を返す。アプリは `ito_common::edit_common` で UI ではないスレッドから書く。
  - General: **Language**（`language_row`）・**Scale**（`scale_row`）と **CLOCK** のカード（`clock_card`）。
  - Theme: 上にモード（Follow OS / Light / Dark）、左にテーマの一覧、右に見本（`theme_page` の閉包でアプリが描く）。
- アプリ自身の設定は、そのアプリの設定ファイルに `set_key` で書く（コメントと並びを残す）。書いたら読み直して当てる。

## 時計

- ステータスバーの右端に、等幅の字体と一番強い文字の色で出す。ポインタを載せると日付を全部出す（`Clock::full`）。
- 分が変わるときだけ再描画する（`request_repaint_after(Clock::next_minute(now))`）。
- 言語が日本語なら曜日を「(月)」のように出す（`Clock::text(now, lang)`）。

## 新しいアプリを作るとき

1. `ito-common`（`clock`）・`ito-theme`・`ito-prefs`・`ito-i18n`・`ito-keys` を同じ `rev` で足す。
2. 設定は `ito_common::app_dir("<app>")/config.toml`、状態は `ito_common::state_dir("<app>")` に置く。
3. 起動時と見張りのスレッドで `Common::read` を読み、テーマ・時計・言語・倍率を当てる。自分の config.toml と keymap.toml も見張る。
4. egui の `zoom_with_keyboard` を切り、`defaults/keymap.toml`（yazi の形、頭に `C` の説明）に標準のキー（上の表）を入れる。
5. `ito_prefs::show` で設定の画面を作る（General に `language_row`・`scale_row`・`clock_card`、Theme に `theme_page`）。
6. ステータスバーの右端に時計を出す。
7. 字体は `ito_common::fonts` の並びで読む。窓は 1280×800 で開き、位置と大きさを状態のフォルダに覚える。
8. `--version`・`--help`・`--keys "<script>"` を受ける。
9. README とその CLAUDE.md から、この文書を指す。
