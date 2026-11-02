# デザインの早見表: こういうときは、この ito のクレート

画面を作るとき、**先にここを見る。** 見た目を自分で描く前に、ito に同じ役目のものがないかを確かめる。
自前で描くと、大きさ・角・フォーカスの輪・テーマの色が画面ごとにずれる。ito のものを使えば、全アプリで同じ見た目になり、テーマ・高コントラスト・倍率・スクリーンリーダーにも付いてくる。

ない場合は、アプリに先に書かず、uchmk/ito に `crate-request` の Issue を立てる。

## 土台（どのアプリも最初に）

| やりたいこと | 使うもの |
|---|---|
| 色・テーマ（20 個、ライト/ダーク、高コントラスト） | `ito-theme`。色は `ito_theme::colors()` から引く。**色を 16 進で直接書かない** |
| 字体（等幅、日本語） | `ito-fonts`（`common.toml` の `mono_font`） |
| 窓の枠（ドラッグ、最小化/最大化/閉じる、中央のパレットの箱、ベル） | `ito-titlebar` |
| 窓の下の帯（左にアプリの項目、右端に時計、倍率） | `ito-statusbar` |
| 幅を変えられるパネル（ドラッグした幅を覚える） | `ito-layout` の `Panels::show` |
| 窓の位置・大きさを覚える | `ito-common::window`（`Saved`・`Tracker`） |
| 設定の画面（言語・時計・テーマ・キー・通知） | `ito-prefs`（`show_with`、`theme_page`、`keys_page`、`notify_page`） |

## ボタン・印・アイコン（`ito-prefs`）

| やりたいこと | 使うもの |
|---|---|
| ふつうのボタン | `button(ui, look, "文言")` |
| 主な操作・取り返しのつかない操作 | `button_with(…, Tone::Primary / Tone::Danger)` |
| **アイコン＋文字のボタン** | `icon_button(ui, Mark, "文言", "読み上げ")`（`IconButton` で Tone・キーの札・`wide`） |
| アイコンだけのボタン（ツールバー） | `mark_button(ui, Mark, "読み上げ", 大きさ)` |
| 印そのもの（79 個。`Mark::…`） | `mark(painter, rect, Mark, 色)`。**アイコンが足りなければ描かずに ito に頼む** |
| 区切りのボタン（タブ風） | `seg` |
| 小さな札・タグ | `pill`（右に印も付けられる） |
| 空の一覧・0 件 | `empty_state`（薄い絵＋文言＋ボタン）。「ありません」だけ書かない |
| 注意・お知らせの帯 | `notice` |
| 人の丸い印（頭文字） | `avatar` |
| 添付ファイルの行 | `attachment` |
| 左の一覧の行（印・件数のピル） | `nav_row` |
| 絵文字を選ぶ | `emoji_picker` |
| 日付を選ぶ（月のグリッド、期間） | `DateField`・`RangeField`・`month_picker` |
| 編集フォーム（必須/任意の見出し、入力欄、行番号つきの欄、囲み） | `form_label`・`InputField`・`CodeBox`・`fieldset` |
| 宛先のチップ入力、二つ割りボタン、隅に浮くカード | `chips`・`SplitButton`・`FloatCard` |

## 確認・案内・知らせる

| やりたいこと | 使うもの |
|---|---|
| 「本当にやる？」の確認（差分つき、おすすめは緑・戻せないものは赤） | `ito-confirm` |
| 初回の案内・困りごとのガイド | `ito-guide` |
| 専門語に点線のホバーの説明 | `ito-glossary` |
| ベルの通知の一覧 | `ito-notices` |
| システムの通知・音・webhook | `ito-notify` |
| コマンドパレット（`Ctrl+Shift+P`）。**ツールバーも同じ元から出す** | `ito-palette` |
| `?`/F1 のキーのヘルプ | `ito-help` |
| 色だけに頼らない状態の印、読み上げ | `ito-a11y`（`State`、`announce`） |

## 文字を扱う画面

| やりたいこと | 使うもの |
|---|---|
| **Markdown を書く欄**（Ctrl+B/I/K/U、選択に印を付ける） | `ito-markdown` の `Writing`（機能 `egui`） |
| **Markdown の整形プレビュー**（見出し・リスト・表・コード） | `ito-markdown` の `layout`・`Md` |
| Markdown を HTML・プレーンに書き出す | `ito-markdown::to_html`・`to_plain` |
| **Markdown の目次**(見出し一覧・節の範囲・右の列) | `ito-outline`(`headings`・`section`、機能 `egui` の `column::show`) |
| **ミニマップ**(右端の細い帯・見えている範囲の枠・ホバーで周りの数行) | `ito-minimap`(`rows`・`show`。色は `ito-syntax` の出力から) |
| コードの色分け | `ito-syntax` |
| 日本語の変換中の下線・候補窓の位置 | `ito-ime` |
| 描いた文字をマウスで範囲選択してコピー | `ito-select` |
| 検索の一致（正規表現・曖昧） | `ito-match` |
| 行の差分（ガターの変更表示、保存前の比較） | `ito-diff` |
| 外のプログラムで開く（`$EDITOR`、diff ツール） | `ito-openin` |
| PNG・PDF・印刷に書き出す | `ito-export` |
| フォルダーの変更を受ける | `ito-watch` |

## 見た目を決めるときの約束

- 色は `ito_theme::colors()`。ダークでもライトでも高コントラストでも崩れないように、**意味のある名前の色**（強調色・エラー色・弱い文字）で選ぶ。
- 一つの行に主なボタンは一つ。Esc／Enter が押すものを主にする。
- 文言は `lang/*.toml` のキー（日英）。ボタンの文字を直に書かない。
- 状態は色だけで伝えない（`ito-a11y::State` の記号を付ける）。
- 見た目が合っているかは `--keys` の台本で確かめる（`<Theme:名前>`・`<Size:WxH>`・`<Shot>`）。窓を人が開かなくても、テーマごとのスクリーンショットが撮れる。

## 迷ったら

1. この表。2. ito の `CLAUDE.md` の表（クレートの全部）。3. 手本のアプリ: 設定・ボタン・空の一覧は minato、ヘルプ・`--keys` は michi、パレットは minato の `main.rs`。
4. 無ければ `crate-request`（`gh issue create -R uchmk/ito --label crate-request`）。
