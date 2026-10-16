# uchmk のアプリの共通仕様

tsumugi・yagura（旧 mimamori）・kura（旧 filer）、これから作る uchmk のアプリが揃えるもの（2026-10-10、持ち主の依頼で決めた）。
**common.toml を変えれば、uchmk のアプリが一括で変わる**ようにする。コードは ito の共有のクレートにあり、
どのアプリも `Cargo.toml` の `rev` で固定して読む（直すときは ito で直して push し、使う側の `rev` を上げる）。
2026-10-11 に tsumugi の `docs/common-spec.md` からここに移した。同じ日に倍率・標準のキー・置き場所・環境変数・CLI・字体・窓を揃えた
（ito 0.3.0〜0.6.0、kura 0.99・yagura 0.39・tsumugi 0.87）。
**新しいアプリはこの文書を既定にする。**どこまで揃ったかは末尾の「各アプリの対応」。

- 仕様を変えるときは、この文書と ito の CHANGELOG を同じコミットで直す。追いつく作業は各アプリの TODO.md に書き、
  「各アプリの対応」の表を直す。
- 「〜する」は全部のアプリが守ること、「〜してよい」はアプリが選べること。

| クレート | 持つもの |
| --- | --- |
| `ito-common` | 設定と状態のフォルダー（`base_dir`・`app_dir`・`state_dir`・`env_dir`・`move_old`）、字体の並び（`fonts`）、common.toml の読み書き（`Common`・`CommonChange`・`edit_common`）、倍率（`tidy_scale`・`scale_step`・`parse_scale`・`scale_text`・`scale_change`）、TOML の 1 キーの書き換え（`set_key`・`remove_key`・`set_tables`、toml_edit でコメントを残す）、`write_atomic`、ファイルの見張り（`watch`）、OS の明暗（`OsLight`）、窓の場所（`window`。`window.toml` の読み書き（古い形も読む `load`）・戻し方・`Tracker`・アプリの手順を束ねる `Saved`）。機能 `clock` で時計の文字（`Clock::text`・`full`・`next_minute`、chrono）、機能 `egui` で egui から窓の様子を読む（`window::Seen::of`・`Tracker::follow`・`Saved::viewport`） |
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
  - tsumugi: `<config>/tsumugi/settings.toml` → `<config>/uchmk/tsumugi/config.toml`、状態は `<local data>/tsumugi/` → `state_dir`（0.87）
  - kura: `<config>/kura/kura.toml` → `<config>/uchmk/kura/config.toml`、状態は `state_dir` へ（0.99。`KURA_CONFIG_HOME` のフォルダは動かさず、ファイルの名前だけ変える）

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

- 無いキーは既定の値。知らないキーと知らない `date_format` は警告に出す（読み込みは止めない）。値の型が違うキーも警告に出し、そのキーだけ無いものとして読む（ほかのキーは使う。TOML として読めないときだけ全体が読めない）。
- `system` の OS の明暗は `ito_common::OsLight` で読む（`OsLight::watch(2 秒, 描き直し)` の `light_or(ctx.system_theme() == Light)`）。
  Windows では egui の `system_theme`（winit）が OS の切り替えに 1 回遅れるので、それだけに頼らない。
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
読むのは `ito-keymap`（ito 0.32.0 から。kura の `config/keymap.rs` を移したもの）: 層の名前と既定を `Spec` で渡し、
`run` の 1 つを読む関数をアプリが持つ。重ねる順・`for`・`on_mac`・押せないキーや二重のキーの警告・既定を置き換えた印（`overrides`）は
どのアプリでも同じになる。

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
  1 つは `{ on, run, desc }`。`on` は 1 つのキーか、続けて押すキーの配列（`"gg"`・`"g g"` も同じ）。macOS だけ違うキーは `on_mac`、
  1 つの OS だけのものは yazi の `for`（`windows`・`macos`・`linux`・`unix`）。
- `[[mgr.keymap]]` の表の配列で書いても同じ（TOML として同じもの）。
- **`C` の書き方**: Ctrl も Cmd も `C` と書く（どの OS の人にも読める。ito 0.6.0 から）。
  - `ito_keys::from_egui` は macOS の Cmd と Ctrl をどちらも `C` にする。macOS で Ctrl と Cmd を別のキーに分けない。
    `<D-…>`（Cmd だけ）は yazi の書き方として読むが、合うキーは来ない。
  - 組み込みの keymap.toml と、アプリが作る雛形の頭に上の 2 行のコメントを置く。
  - 画面（ヘルプ・パレット・設定・ステータスバー）には OS の名前（`Ctrl+,`・`⌘,`）で出す（`Key::human`）。
  - ito 0.5 の `<C-…>`・`<primary-…>` も `C` と同じに読む。
- tsumugi はターミナルなので、macOS でも Ctrl のキーはペインのプログラムに渡す。そのため tsumugi の keymap.toml（層 `[window]`）だけは
  `C` がどの OS でも Ctrl、`D` が Mac の Cmd（ほかの OS では押されない）。窓のキーの既定は Mac では Cmd（設定の「macOS で Cmd を使う」）。
- tsumugi の config.toml の `[keys]`（`"Ctrl+Shift+T"` の形）は古い形。今も読み、設定 → キーで次にキーを変えたときに keymap.toml に移す（0.108.0 から）。

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

### 設定の画面から変えるとき

設定の「キー」のページ（`ito_prefs::keys_page`、ito 0.33.0 から）が、keymap.toml を手で書かなくてもキーを変えられるようにする。

- 1 行に 1 つの操作（層の `run`）。キーをチップで横に並べ、＋で足し、× で外し、チップを押して変える。変えた行には ↺（既定に戻す）と、
  その下に薄い字の「既定: …」。自分の keymap.toml で変えたキーには •、ほかのキーが先に動いて届かないキーには ⚠ を付け、
  外した既定のキーは取り消し線で残す（押すと戻る）。
- 文字の検索（訳した説明・`run`・キー・場面の名前）、「⌨ キーで探す」（押したキーで始まるキーを持つ行だけ）、場面の絞り込み、
  「変えたものだけ」。
- キーは押して入れる。続けて押したキーは 1 つの並び（`g h`）になる。**修飾キーの無い** Enter で決め、Esc でやめ、Backspace で
  1 つ戻す。修飾キーの付いたもの（`<C-S-Enter>` など）はふつうのキー。修飾キーの無い Enter・Esc・Backspace・Tab は
  「特別なキー ▾」から入れる。決めるまで何も書かない。決める前に、ぶつかり（同じキー・頭が同じキー・上の表の共通のキー、
  アプリが足すもの）を文で出す。頭が同じキーは 3 つまで名前を出し、残りは数だけ出す（`g` は十を超えるキーの頭）。
- 入れている間（`KeysEdit::busy`）は、`show` に `Keys::default()` を渡す（Esc・Ctrl+Tab をページに渡す）。
  ほかのページへ移ったとき、設定の画面を閉じたとき・開き直したときは、入れかけのキー（`KeysEdit::capture`・`listening`）を捨てる
  （残すと、次に開いたときもまだ受け取っていて、最初の Esc がページに取られる。`KeysEdit` を画面と一緒に作り直すのでもよい）。
- Ctrl+C・X・V は egui が `Copy`・`Cut`・`Paste` に変えるので、ページは `ito_keys::from_clipboard` でもとのキーに戻して受け取る。
  ページで `<C-S-v>` などに割り当てたキーが一覧でも効くよう、アプリのキーの入口も同じものを通す（kura 0.111 から）。
- ページは `Edit` を返す。アプリは UI ではないスレッドで `ito_keymap::edit` を自分の keymap.toml に当てて `write_atomic` で書き、
  読み直す。**書くのは変えた分だけ**（`prepend_keymap` の頭）:
  - 足す: `{ on, run, desc }`。既定のキーを戻すだけなら、それを隠していた行を消す。
  - 外す: 既定のキーは `{ on, run = "noop" }`（アプリの何もしないコマンド）。自分で足したキーは、その行を消す。
  - 変える: 外してから足す。↺: その操作の行と、その既定のキーの `noop` を消す。
  - 結果が既定と同じになれば行は残らない。何も変えなければファイルを作らない。コメントは残る。`for` の付いた行には触らない。
  - 層が `keymap`（全部を置き換える）を持つファイルは書かない（エラーを返す）。ほかのファイル（kura が読む yazi の keymap.toml など）の
    キーは出どころを出し、そのファイルで変えてもらう。

## 設定の読み直し

- アプリは自分の config.toml（と keymap.toml）も見張り、変わったら自動で読み直して当てる（common.toml と同じ 2 秒）。
- 読めなければ前の値のまま、理由を警告に出す。手で読み直すキー（kura の `<C-F5>`）は残してよい。

## コマンドライン

| 引数 | 動き |
| --- | --- |
| `--version`・`-V` | `<app> X.Y.Z` を 1 行出して終わる |
| `--help`・`-h` | 使い方を出して終わる |
| `--keys "<script>"` | 実機のテスト用。起動したあと、アプリの中でキーを順に押す（下の「キーの台本」）。終わったら `<APP>_KEYS_DONE` のファイルに状態を書く |

### Windows のコンソールの前段（`<app>.com`）

`<app>.exe` は窓のプログラムなので、PowerShell はそれを待たず、出力も受け取らない（`$v = & kura --version` が空になり、`kura env` は
次のプロンプトの後に出た。kura #160・#176・#183）。そこで Windows のリリースの zip には、横にコンソールのプログラム `<app>.com` を入れる。
`PATHEXT` で `.COM` が `.EXE` より先なので、`<app>` と打てばこちらが動く（Visual Studio の `devenv.com` と同じ形）。

- 中身は `ito-console` の `front`。`src/bin/<app>-com.rs` は `fn main() { ito_console::front("<app>", answers_in_text) }` と、
  どのコマンドラインが文字で答えるかの関数（`answers_in_text`）だけにする。値を取るオプションの値を飛ばして左から読む規則は `ito_console::any_of`。
- 文字で答えるもの（`--version`・`--help`・`env` など）は、`<app>.exe` を同じ引数・同じハンドルで走らせて待ち、終了コードを渡す。
- ほかは窓を開き、窓が入力を待ち始めたら戻る。その前に `<app>.exe` が終われば（`--keys` の台本を断ったなど）、断りの 1 行と終了コードを渡す。
- 窓には `.com` のほかのハンドルを渡さない（パイプの読み手が窓の終わりまで待たされた。kura #192）。出力がパイプなら、窓は一時フォルダの
  自分のファイルに書き、`.com` は窓が上がった時点までの中身をパイプへ写す。ファイルは窓が終われば消える。
- `<app>.exe` は答えを `ito_console::out`・`err` で書く（ファイルかパイプならそこへ、どちらでもなければ親のコンソールへ。書けなければ捨てる）。
  `println!` は、PowerShell が読まずに閉じたパイプ（`$v = & <app>.exe --version`）で panic する。
- ビルドの成果物は `<app>-com.exe`。リリースの zip と CI では `<app>.com` に写す。CI は `--version` をパイプで受けることと、
  `<app>.exe --version` が閉じたパイプでも 0 で終わることを確かめる。

### キーの台本（`--keys`）

人の手を使わずに、窓を開いたままキーとクリックを送り、画面と状態を残す。実機のテストのセッション（Agent）がこれで確かめる。
キーは egui の入力の前（`raw_input_hook`）に egui のイベントとして入れるので、本物のキーと同じ道を通る。

台本は語を区切りなしに続けたもの（`<C-t><Wait:500>echo<Space>hi<Enter>`）。`<…>` の外の 1 文字は、その文字を打つキー 1 つ。
生の空白は書けない（打ちたいなら `<Space>`、書くと空白の位置を示して起動を止める。kura #110・yagura・tsumugi 0.95 が同じ）。
**全部のアプリが受ける語**:

| 語 | 動き |
| --- | --- |
| `j`・`<C-,>`・`<A-S-Up>`・`<Enter>` | キー 1 つ（yazi の書き方。上の「キー」の `C` もそのまま） |
| `<…>*N` | 直前のキーを N 回 |
| `<Wait:ms>` | ms ミリ秒待つ（描画が追いつくのを待つ） |
| `<Shot:name>` | 窓の画像を `name.png` に書く |
| `<State:name>` | その時の状態を `name.txt` に書く（形は下） |
| `<Click:x,y>` | 窓の中の点をクリックする。`x`・`y` は窓の幅と高さに対する 0〜1 の割合 |
| `<Drag:x,y-x,y>` | 1 つ目の点から 2 つ目の点へ引きずる（割合は同じ） |
| `<Now>` | 待たずに次へ（直後のキーか `<State:>`・`<Shot:>` と同じ回に入れる。`d<Now>w` のように） |
| `<RClick:x,y>` | 右クリック |
| `<Click:C-x,y>` | 修飾キーを押したままクリック（`C`・`S`・`A`。`<RClick:…>` も同じ） |
| `<Hover:x,y>` | ポインタをその点へ動かすだけ |
| `<Wheel:n@x,y>` | その点でホイールを n 目盛り回す（負は逆向き） |
| `<Paste>`・`<Paste:text>` | 貼り付け（`<Paste>` はクリップボード、`text` は `\n \t \e \\ \xNN` が使える）。`<C-v>` も貼り付けになる |
| `<Quit>` | アプリを閉じる |

- `name` は英数字と `-`・`_` だけ。ファイルは `<APP>_KEYS_DONE` のファイルと同じフォルダに書く。
- 台本が終わったとき（`<Quit>` の前）にも `<APP>_KEYS_DONE` に状態を書く。テストはこのファイルができるのを待つ。
- 状態の後ろの `keys:` の行が終わり方を言う。テストはこれを読み、来ないまま待ち続けない:

  | 行 | いつ（作るのは `ito-keyscript`） |
  | --- | --- |
  | `keys: done` | 最後の語まで押した。`<Quit>` で終わったときは `quit: yes` の後（`quit_report(0)`） |
  | `keys: quit`・`left: N not pressed` | `<Quit>` の後ろに N 語が残っていた（`quit_report(N)`） |
  | `keys: closed`・`left: N not pressed` | 台本の途中で窓が外から閉じられた（×・Alt+F4。`closed_report(N)`）。閉じるかを問う窓で止めたときは書かない |
  | `keys: stalled`・`pressed:`・`left:` | 待ちの分を除いて `STALL`（30 秒）何も押せていない（`Runner::watch` のスレッドが 1 回だけ書く。その後に進めば上の行で上書きする） |
  | `keys: panicked`・`thread:`・`at:`・`why:` | アプリがパニックした（`install_panic_report(done)`） |
  | `keys: refused`・`why:` | 窓を開く前に台本を拒んだ（読めない、打てないキー、引数の数。`refused_report`）。終了コードは 2 で、同じ文を stderr にも出す |

  `Runner` を使うアプリは、最初の一歩の前に `watch(ctx)` を呼び（1 秒ごとに描画を起こすので、裏の窓でも台本が進む）、`clipboard` に
  クリップボードの文字を返す関数を入れる。`<Quit>` では `quit_report(runner.left())` を、外から閉じられたときは `finish()` のあと
  `closed_report(runner.left())` を状態の後ろに書く（`running()` が偽なら書かない）。
- 上の語はすべて ito の `ito-keyscript` が解析して再生する（kura・yagura・tsumugi が同じものを使う）。`<Space>` はキーとテキストの両方で届く。
- アプリが足してよい語（kura の `<PaneText:name>`・`<PreviewText:name>`、`<Shot:name@preview>` の `@preview` など）は `Dialect`
  （`parse_with(script, &Dialect { reads, shot_suffixes })`）で渡す。`reads` の語は `<Word:name>` の形で、`Do::Read { tag, name }` として
  アプリに返る。足すときは名前の形（`<Word:引数>`）を守り、そのアプリの README に書く。
- 自前のフレームのループで回すアプリ（kura は窓の落ち着くのを待つ）は、`press` で語を `Press` に直して自分で再生する。

**状態のファイル**は UTF-8 の `key: value` の行（1 行に 1 つ、順は自由）:

```text
focus: list           # キーが届いている所（アプリが決める名前）。全部のアプリが書く
overlay: none         # 開いている重ね物（settings・help・palette など、無ければ none）
scale: 100%           # 書くなら倍率を % で
```

- `focus:` は全部のアプリが書く。`overlay:`・`scale:` は持つアプリが書く（新しいアプリは書く）。
- そのほかの行（yagura の `screen:`・`sort:`、kura の `sel:` など）はアプリが足してよい。テストは知らない行を読み飛ばす。
- 値の中の改行は `\n` と書く。

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
- 窓の位置と大きさは、動いたときにアプリの状態のフォルダ（`state_dir`）の `window.toml` に書き（`ito_common::window`）、次に開くときに戻す。
  戻す位置が今の画面の外なら既定に戻す。外かどうかは今ある画面で見る（`Window::reachable_now`。タイトルバーが 100 ポイント（`GRAB`）以上
  どれかの画面に出ること。画面を聞けない OS では主の画面の大きさの見当（`PRIMARY_GUESS`＝1920×1080）で見る）。
- 場所は、取ったときの倍率（`scale`＝egui の `pixels_per_point`）とアプリの拡大（`zoom`）と一緒に書く。戻すときは拡大を抜いた値で窓を作り
  （`Window::opening`）、Windows では窓を表示する前に閉じた画素へ動かす（`Window::put`。倍率の違う画面で閉じても、その画面に開く）。
  最大化して閉じた窓は、閉じた画面で最大化して開く。
- 読むのは `window::load`（アプリごとに作らない）。`zoom` の無い古い `window.toml`（ito 0.38 までの形と tsumugi v0.113.1 までの形）は、
  common.toml の `scale` で取ったものとして読み、今の形で書き直す（拡大していても前に見えていた大きさで開く）。
- いつ書くかは `window::Tracker` が決める（アプリごとに作らない）。窓が動いて 0.8 秒（`SETTLE`）止まったら UI ではないスレッドで書き、
  閉じるときはすぐに書く（`Tracker::close`）。最大化している間は前の場所を保ち、最小化と全画面の間は追わない。
- アプリがやることは `window::Saved` の 5 つだけ: 起動で `Saved::load(state_dir)`（`--keys` の台本・引き剥がした窓・quake のように場所を持たない窓は
  `None`）、`saved.viewport(viewport)` で窓を作り、eframe の creator の先頭で `saved.put(cc)`、`saved.tracker()` を持って毎フレーム
  `tracker.follow(ctx)`（ito-common の機能 `egui`）、`on_exit` で `tracker.close(Seen::of(ctx))`。

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
  - 設定ファイルに値のある行は `keyed_row` で描き、そのキー（アプリの設定なら `ui.font_size`、common.toml なら `clock.show`。表の名前とキーを `.` でつなぐ）でも見つかるようにする（ito 0.44.0 から）。キーは画面に出さず、index にも足す。
    どの言語の画面でも同じ文字で行に届くので、`--keys` の台本が検索から行へ入れる（kura #388）。共通の行のキーは `GENERAL_KEYS`。
    通知のページ（`notify_page`）の行はまだキーを持たない（言葉でだけ見つかる）。
- 共通の行は共通のクレートが描き、`CommonChange` を返す。アプリは `ito_common::edit_common` で UI ではないスレッドから書く。
  - General: **Language**（`language_row`）・**Scale**（`scale_row`）と **CLOCK** のカード（`clock_card`）。
  - Theme: 上にモード（Follow OS / Light / Dark）、左にテーマの一覧、右に見本（`theme_page` の閉包でアプリが描く）。
- アプリ自身の設定は、そのアプリの設定ファイルに `set_key` で書く（コメントと並びを残す）。書いたら読み直して当てる。
- **ページは General・Theme・アプリの設定・キー（Keys）・通知（Notifications）・Advanced を既定にする**（下の「通知」）。
  **新しいアプリは、キーのページ（上の「設定の画面から変えるとき」）を設定の画面の機能として必ず作る。**keymap.toml を
  `ito-keymap` で読み、何もしないコマンド（`noop` か `none`）を 1 つ持ち、層ごとの今のキーと既定のキーを `KeyTable` で渡す。

## 通知

アプリの出来事（kura なら長い操作の終わり、yagura ならアラート、minato なら見張った実行の終わり）を窓の外に知らせる。
知らせ方は `ito-notify`、設定の画面の「通知」のページは `ito_prefs::notify_page` が描く。

- 知らせ方は 3 つ: **システムの通知**（`ito_notify::toast::show_with_icon`。アプリの id は `uchmk.<app>`、窓と同じアイコンを
  `icon` のモジュールの関数で渡す。Windows では通知の左と差出人の名前の横に出る）、**音**（`ito_notify::sound::play`。
  `ito_notify::SOUNDS` の chime・low・alert・default から出来事ごとに 1 つ）、**Webhook**（`ito_notify::webhook::send`。
  ntfy・Slack・Discord・JSON、`ito_notify::FORMATS`）。送り先は幾つでも持てて、それぞれ送る出来事を選ぶ
  （`ito_notify::webhook::Place`、`send_all` が出来事の名前で選んで送る）。アドレスが空か http(s) で始まらなければ送らない。
  `curl -f` で送るので、相手が 400 以上で断ったとき（アドレスの誤り・回数の制限）も失敗として、30 秒後に 1 回だけ送り直す。
  Discord は太字の題と本文を `content` に入れ（2000 字で切る）、`allowed_mentions` を空にして `@everyone` などで誰も呼ばない。
- Windows の集中モード・プレゼンテーション・全画面のゲームの間は音を鳴らさない（`focus_mode`、既定は入り。`sound::quiet_time`）。
- ページ（`notify_page`。出来事は `End` の並びでアプリが渡す）:
  - 上に「試しに送る」（3 つとも。Webhook はアドレスのあるものすべて、`Test::All`）と「既定に戻す」（Webhook の送り先は残す）。
  - 「知らせるとき」: 出来事が 1 つなら行ごとのスイッチ、2 つ以上なら列が出来事・行が知らせ方の表。
    Webhook の行は送り先ごとに 1 つ（1 つなら「Webhook」、2 つ以上なら「Webhook 1」「Webhook 2」…）で、
    出来事ごとに送り分けるアプリだけ（`Hook::on` が空なら出さない）。
  - 「鳴らす音」: 出来事ごとの音（▶ で鳴らしてみる）と集中モードのスイッチ。
  - 「Webhook」: 送り先ごとに名前と「外す」「ここに試しに送る」（`Test::Hook(k)`）、アドレス（http(s) でなければ赤い注意）と形式。
    最後に「アドレスを足す」。自分のカードに置くアプリ（tsumugi）は `webhook_rows` を使う。
  - 検索の言葉は `notify_index`。
- 設定はアプリの設定ファイルの `[notify]` に置く（minato は `minato-settings.json`）。鍵は tsumugi に揃える:
  `system`・`sound`（出来事の名前の並び。出来事が 1 つのアプリは真偽値にしてよい）、`sound_<出来事>`、`focus_mode`。
  Webhook の送り先は `[[notify.webhooks]]` の表を並べ、それぞれ `url`・`format`・`states`（出来事の名前の並び。省けばアプリの既定）。
  前の 1 つだけの形（`[notify]` の `webhook`（アドレス）・`webhook_format`・`webhook_states`）も読み、1 つ目の送り先にする
  （`webhook::places`）。設定の画面が書くときは表に移す（`ito_prefs::write_webhooks`。表はアドレスで見分け、コメントを残す）。
  知らない値は警告して既定に戻す（送り先は `Place::problems`）。

## 時計

- ステータスバーの右端に、等幅の字体と一番強い文字の色で出す。ポインタを載せると日付を全部出す（`Clock::full`）。
- 分が変わるときだけ再描画する（`request_repaint_after(Clock::next_minute(now))`）。
- 言語が日本語なら曜日を「(月)」のように出す（`Clock::text(now, lang)`）。

## 直前の起動の記録（`ito_common::runinfo`）

窓のある起動が、状態フォルダの `last-run.toml` に「使ったもの」（GPU のアダプター・バックエンド・フォント・窓の pt と ppp・起動時刻・バックエンドを選んだ設定）を書き、
`<app> env` と不具合の報告が読む（`env` は窓を開かない）。アプリ固有の項目は `extra`（TOML の表）。`window_line`・`rendering`・`started_ago` が報告の行の元になる。

## 不具合の報告（`ito-bugreport`）

どのアプリも、F12（と設定の画面のボタン）で、版・OS・アプリが知っていることを埋めた GitHub の issue form を開く。報告は全アプリで同じ形になる。

- `ito_bugreport::url(&Spec { app, version, repo, template }, &[("keys", …)])` が URL を作る。アプリが持つのは名前・版・リポジトリ・テンプレートの名前だけ。
- **form の field の `id` は固定**: `version`・`os`・`context`（共通。ここが埋める）、`what`・`backtrace`（共通。人が書く）。アプリ固有の欄（`keys`・`files`・`config` など）は足してよい。
  form は **全アプリで ito が作る**: `ito_bugreport::template(app, &[Field{…}])` が共通の form を返し、アプリ固有の欄は `extra` で足す。
  GitHub は form をそのリポジトリの `.github/ISSUE_TEMPLATE/bug_report.yml` からしか読まない（別のリポジトリを指せない）ので、アプリは出力をそのファイルに書き、
  テストで `template()` の出力とファイルが一致することを見る（`scripts/lanes` の写しと同じ作法）。
  GitHub は知らない id を黙って捨てるので、アプリのテストで form の id と `url` が埋める id を突き合わせる。
- `os` の行は `OS:`・`OS arch:`・`Process arch:`（Windows。エミュレーションで食い違えばそれ自体が手がかり）。ほかは `OS:`・`Process arch:`。
- `context` は `Context` が組む `Label: text` の行で、順は **Last error・Rendering・Scale・アプリの行・Config** 。ホームのフォルダは `~` に、
  設定ファイルはパスでなく名前（`short_name`）で書く。利用者の名前を公開の tracker に出さない。

## 新しいアプリを作るとき

1. `ito-common`（`clock`）・`ito-theme`・`ito-prefs`・`ito-i18n`・`ito-keys`・`ito-keymap` を同じ `rev` で足す。
2. 設定は `ito_common::app_dir("<app>")/config.toml`、状態は `ito_common::state_dir("<app>")` に置く。
3. 起動時と見張りのスレッドで `Common::read` を読み、テーマ・時計・言語・倍率を当てる。自分の config.toml と keymap.toml も見張る。
4. egui の `zoom_with_keyboard` を切り、`defaults/keymap.toml`（yazi の形、頭に `C` の説明）に標準のキー（上の表）を入れる。
   読むのは `ito-keymap`。何もしないコマンド（`noop`）を持たせる（キーのページで既定のキーを外すのに使う）。
5. `ito_prefs::show` で設定の画面を作る（General に `language_row`・`scale_row`・`clock_card`、Theme に `theme_page`、
   キーに `keys_page`、通知に `notify_page`）。**キーのページは必ず入れる**（変えた分だけを UI ではないスレッドで
   keymap.toml の `prepend_keymap` に書き、読み直す。上の「設定の画面から変えるとき」）。TESTING.md にキーのページの行を足す。
6. ステータスバーの右端に時計を出す。
7. 字体は `ito_common::fonts` の並びで読む。窓は 1280×800 で開き、位置と大きさを状態のフォルダに覚える。
8. `--version`・`--help`・`--keys "<script>"` を受ける。答えは `ito_console::out`・`err` で書き、Windows には `<app>.com` を付ける
   （上の「Windows のコンソールの前段」）。
9. `ito-bugreport` で F12 の不具合の報告を付け、`.github/ISSUE_TEMPLATE/bug_report.yml` を雛形から作る（上の「不具合の報告」）。
   `scripts/verify.sh`・`release.yml`・`release-sums.yml` を kura か yagura から写す（下の「版とリリース」）。
10. README とその CLAUDE.md から、この文書を指す。下の「各アプリの対応」に行を足す。

## 版とリリース

- 版はルートの `Cargo.toml`（workspace なら `[workspace.package]`）の `version` が正。版の繰り上げと CHANGELOG.md は同じコミットにする。
- CHANGELOG.md は日本語、[Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) の形、日付は JST。
- 版のコミットの題は `vX.Y.Z: <英語のひとこと>`。本文の最初の段落は英語で、それだけで意味が通るように書く（リリースのノートになる）。
- push の前に `scripts/verify.sh` を回し、最後の行が `ALL OK: …` になってから push する（test と clippy `-D warnings` を
  Linux と `x86_64-pc-windows-msvc` の両方で）。マージは merge コミット（rebase・squash を使わない）。
- リリースは `.github/workflows/release.yml` を `workflow_dispatch` で回し、`tag` に `vX.Y.Z` を渡す（タグが無ければ作る。
  `Cargo.toml` の版と違えば落ちる）。前のタグのリリースができてから次を投げる。
  - 成果物は 6 つ: x86_64 と aarch64 の Windows・macOS・Linux。Windows は `.zip`（ConPTY を使うアプリは同梱する）、ほかは `.tar.gz`。
  - ノートは前のタグからの `vX.Y.Z:` のコミットの最初の段落から作る。最後に `release-sums.yml` が SHA-256 の表を足す。
- ito 自身はバイナリを出さないので、版と CHANGELOG とタグ（`vX.Y.Z`）だけ。使う側は `rev` で固定する。

## 各アプリの対応

2026-10-11 の時点（kura 0.100・yagura 0.39・tsumugi 0.87）。○ は揃った、△ は一部、× はまだ（各アプリの TODO にある）。

| 項目 | kura | yagura | tsumugi |
| --- | --- | --- | --- |
| 置き場所 `uchmk/<app>/config.toml`・古い場所から移す | ○ | ○ | ○ |
| `<APP>_CONFIG_HOME`・`<APP>_STATE_HOME` | ○ | ○ | ○（古い名前も読む） |
| common.toml の言語・テーマ・時計 | ○ | ○ | ○ |
| 倍率（common.toml の `scale`・`scale_row`） | ○ | ○ | △（読んで当てるだけ。キーは字の大きさ） |
| 自分の設定の自動の読み直し | ○ | ○ | ○ |
| keymap.toml（yazi の形、`ito-keymap`） | ○（0.110.3） | ○（0.60.0） | ○（0.108.0。`C` は Ctrl、`D` が Cmd。minato も 0.43.0 から） |
| `<C-,>`・`<F1>`・`<C-q>` | ○ | ○ | △（`<C-q>` はシェルに渡す） |
| コマンドパレット `<C-S-p>` | ○ | × | × |
| 設定の画面（`ito_prefs`） | ○ | ○ | ○ |
| キーのページ（`keys_page`） | ○（0.111） | ○（0.61） | ○（0.109。minato も 0.44.0 から） |
| 通知のページ（`notify_page`・`[notify]`） | ○（0.110） | ○（0.59） | △（独自のページ。行が多い。minato も独自） |
| 通知にアイコン（`show_with_icon`） | ○（0.110） | ○（0.59.1） | ○（独自。minato は 0.42.1） |
| 字体の並び（`ito_common::fonts`） | ○ | ○ | ○ |
| 窓 1280×800 | ○（config で変えられる） | ○ | ○ |
| 窓の位置と大きさを覚える（`window::Saved`。古い形も読む） | ○ | ○ | ○ |
| `--version`・`--help` | ○ | ○ | ○ |
| Windows の `<app>.com`（`ito_console`） | ○ | ○ | ○（minato・michi も） |
| `--keys` | ○ | ○ | ○（0.95） |
| `mcp`・`shell-hook` | ○ | — | ○ |
