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
| Accent | `run` を代用 | 専用の役割（[#186](https://github.com/uchmk/ito/issues/186)） |
| State | `err`・`done`・`wait` | Success・Warning の専用色（#186） |
| Editor | `selection` だけ | 現在行・行番号・キャレット・印・IME の下線（#186） |
| Diff・検索 | なし（`ito-diff` の使う側が決めている） | 追加・変更・削除、一致、折りたたみ（#186） |

足りない役割を、アプリに 16 進や `mix(dim, bg, 0.4)` で書かない。#186 が入るまでは、近い役割から混ぜて、コメントに `ito #186` と書く。

### 色の直書きが残る所

ito-theme の外で `Color32::from_rgb` などを使う箇所は 56（2026-11 の調べ）。多くは `ito-pane`（端末の色）で、これは別扱いでよい。
`ito-prefs` の `tag.rs`（色見本）と、アクセントの上の文字色の直書きは、`on_accent` などへの置き換えを調べる。

## 余白・角・高さ・文字の大きさ

**共通のスケールはまだ無い**（[#185](https://github.com/uchmk/ito/issues/185)）。現状:

| 項目 | 現状 |
|---|---|
| 角丸 | 4・6・7・8・10 が混在（`ito-prefs/lib.rs` だけで 9 か所） |
| 余白 | `vec2(10.0, 8.0)` などの直書き |
| 窓の枠の高さ | 定数: titlebar 40（`ito_titlebar::HEIGHT`）、statusbar 26、タブの帯 30 |
| ダイアログの幅 | `ito_confirm::MAX_WIDTH` 600 |
| 文字の大きさ | 部品ごとの直書き（`ito-prefs`・`ito-confirm`・`ito-titlebar`・`ito-statusbar`・`ito-help` ほか 44 か所） |

方針（#185 で決める）:

- 数値でなく**役割の名前**で引く（`editor.text`・`chrome.body`・`dialog.title`・`page.title` のように）。
- 箱（ダイアログ・カード）の角は 1 つの値にそろえる。
- 倍率（`ito-statusbar::scale_text`）と高コントラストで崩れないこと。

## フォント（`ito-fonts`）

- 同梱は JetBrains Mono Regular（SIL OFL）1 本。`common.toml` の `mono_font`（`bundled` が既定、`system`）で選ぶ。
- 日本語と絵文字は OS のフォントを別スレッドで探す（`find`）。等幅では全角を 2 マスにする倍率 `two_column_scale` を使う。
- UI 用の比例フォントは同梱していない。日本語の字形は OS で違う。
- 行間の共通値は無い。

## 状態の色

色だけで伝えない（`ito-a11y::State` の記号を付ける）。高コントラストは `Colors::high_contrast`。
