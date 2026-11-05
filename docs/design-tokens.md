# デザインのトークン: 色・余白・角・文字

画面の見た目を決める値の、**ito にある現状**と**足りないもの**の表。足りないものは Issue で頼んでいる。
どの部品を使うかは [design-guide.md](design-guide.md)、状態の扱いは [interaction.md](interaction.md)。

## 色（`ito-theme`）

色は `ito_theme::colors()` から引く。16 進を直接書かない（ダーク・ライト・高コントラスト・20 テーマで崩れる）。

基本の 12 色は `bg`・`side`・`panel`・`border`・`fg`・`dim`・`wait`・`run`・`err`・`done`・`blue`・`magenta`。
ここから混ぜて作る役割:

| 役割 | 意味 |
|---|---|
| `raised` | メニュー・ポップアップ（panel より一段上） |
| `border_strong` | 目立つ枠（枠つきボタン、ダイアログの縁） |
| `chosen` | 選ばれたものの背景（入っているフィルター、選んだ行） |
| `hover` | ポインターが乗っているものの背景 |
| `selection` | 選択範囲・表示中のタブの背景（`ito-select` の色。全アプリ共通） |
| `strong` | 一番大事な文字（名前、見出し） |
| `faint` | 一番弱い文字（キーの札、件数） |
| `wait_bg` / `wait_text` | 人を待つカード |
| `on_accent` | 塗った強調色の上の文字 |

### 設計書の分け方との対応

weft の設計書は意味の色を Surface・Text・Border・Accent・State・Editor・Diff に分ける。ito との対応:

| 分け方 | ito の現状 | 足りない |
|---|---|---|
| Surface | `bg`・`side`・`panel`・`raised` | — |
| Text | `fg`・`dim`・`strong`・`faint` | — |
| Border | `border`・`border_strong` | — |
| Accent | `accent()`（= `run`） | — |
| State | `err`・`success()`（= `done`）・`warning()`（= `wait`） | — |
| Editor | `selection`・`gutter`・`gutter_active`・`current_line`・`caret`・`caret_idle`・`invisible`・`invisible_fill`・`ime_underline` | — |
| Diff・検索 | `diff_added`・`diff_modified`・`diff_removed`・`diff_bg(mark)`・`match_bg`・`match_current_bg`・`fold` | — |

役割はテーマのファイルのキーではなく、基本の 12 色から出す。アプリに 16 進や `mix(dim, bg, 0.4)` で書かない。

### 色の直書きが残る所

ito-theme の外で `Color32::from_rgb` などを使う箇所は 56（2026-11 の調べ）。多くは `ito-pane`（端末の色）で、これは別扱いでよい。
`ito-prefs` の `tag.rs`（色見本）と、アクセントの上の文字色の直書きは、`on_accent` などへの置き換えを調べる。

## 余白・角・高さ・文字の大きさ（`ito_theme::scale`）

数値でなく**役割・段の名前**で引く（[#185](https://github.com/uchmk/ito/issues/185)、0.246.0）。倍率（`set_text_zoom`）は文字だけに掛かる（`Text::at(ctx)`）。角・余白・高さは egui の `set_zoom_factor` が運ぶので掛けない。

| 文字の役割 | pt | 用途 |
|---|---|---|
| `editor.text` | 14 | エディターの本文 |
| `editor.gutter` | 12.5 | 行番号（本文より一段下） |
| `chrome.body` | 13 | 一覧・欄・ラベル |
| `chrome.button` | 12 | ボタンの文字 |
| `chrome.small` | 11 | 件数・注釈 |
| `dialog.title` | 16 | ダイアログの見出し |
| `dialog.key` | 12 | キーの札（等幅） |
| `dialog.hint` | 11.5 | 見出しの下の説明 |
| `page.title` | 18 | 設定のページなどの見出し |

| 段 | 値 |
|---|---|
| `radius::BOX` / `CONTROL` / `MARK` | 8 / 6 / 4（箱の角は `BOX` の 1 つ） |
| `space::XS` `SM` `MD` `LG` `XL` | 4 / 6 / 10 / 16 / 24 |
| `height::CONTROL` / `ROW` | 28 / 24 |

窓の枠の高さは各クレートの定数のまま: titlebar 40（`ito_titlebar::HEIGHT`）、statusbar 26、タブの帯 30、ダイアログの幅 `ito_confirm::MAX_WIDTH` 600。

既存の直書き（`ito-prefs` ほか 44 か所の文字、角丸 4/6/7/8/10）は**まだ置き換えていない**。部品を直すときに順にこの表へ寄せる。新しく書く画面は最初から使う。

## フォント（`ito-fonts`）

- 同梱は JetBrains Mono Regular（SIL OFL）1 本。`common.toml` の `mono_font`（`bundled` が既定、`system`）で選ぶ。
- 日本語と絵文字は OS のフォントを別スレッドで探す（`find`）。等幅では全角を 2 マスにする倍率 `two_column_scale` を使う。
- UI 用の比例フォントは同梱していない。日本語の字形は OS で違う。
- 行間の共通値は無い。

## 状態の色

色だけで伝えない（`ito-a11y::State` の記号を付ける）。高コントラストは `Colors::high_contrast`。
