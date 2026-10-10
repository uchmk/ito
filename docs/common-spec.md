# uchmk のアプリの共通仕様

tsumugi・mimamori・filer、これから作る uchmk のアプリが揃えるもの（2026-10-10、持ち主の依頼で決めた）。
**common.toml を変えれば、uchmk のアプリが一括で変わる**ようにする。コードは ito の共有のクレートにあり、
どのアプリも `Cargo.toml` の `rev` で固定して読む（直すときは ito で直して push し、使う側の `rev` を上げる）。
2026-10-11 に tsumugi の `docs/common-spec.md` からここに移した。

| クレート | 持つもの |
| --- | --- |
| `ito-common` | 設定のフォルダー、common.toml の読み書き（`Common`・`CommonChange`・`edit_common`）、TOML の 1 キーの書き換え（`set_key`・`remove_key`・`set_tables`、toml_edit でコメントを残す）、`write_atomic`、ファイルの見張り（`watch`）。機能 `clock` で時計の文字（`Clock::text`・`full`・`next_minute`、chrono） |
| `ito-theme` | テーマ（`Colors`・`Theme`・組み込みの 13 個・`from_table`・`pick`・`mix`・`contrast`）。egui の `ecolor` だけに依る。機能 `pane` でターミナルの `Palette` |
| `ito-prefs` | 設定の画面の枠と部品、共通のページ（`language_row`・`clock_card`・`theme_page`）。文字は `Words`（`EN`・`JA`、`Words::of(lang)`） |
| `ito-i18n` | 言語の表（`tr`）。`base_dir` などは `ito-common` の再エクスポート |
| `ito-keys` | yazi のキーの書き方（`<C-a>`・`<P-,>`。`P` は macOS で Cmd、ほかで Ctrl） |

## 置き場所

`<config>` は Windows が `%APPDATA%`、macOS が `~/Library/Application Support`、Linux が `$XDG_CONFIG_HOME`（無ければ `~/.config`）。
`UCHMK_CONFIG_DIR` があれば `<config>/uchmk` の代わりにそこを使う（`ito_common::base_dir`）。

| ファイル | 中身 |
| --- | --- |
| `<config>/uchmk/common.toml` | 全部のアプリが読む共通の設定（下） |
| `<config>/uchmk/<app>/config.toml` | アプリごとの設定（mimamori・filer）。tsumugi だけは前からの `<config>/tsumugi/settings.toml` |
| `<config>/uchmk/themes/*.toml` | 自分で作ったテーマ。どのアプリの一覧にも出る |

## common.toml

```toml
language = "auto"          # auto / en / ja
theme = "dark"             # テーマの名前、または dark / light / system（OS に合わせる）
dark_theme = "tsumugi Dark"   # system で OS が暗いとき
light_theme = "tsumugi Light" # system で OS が明るいとき

[clock]
show = true
hour24 = true
date = true
date_format = "YYYY/MM/DD"  # YYYY/MM/DD, YYYY-MM-DD, MM/DD/YYYY, DD/MM/YYYY
weekday = true
```

- 無いキーは既定の値。知らないキーと知らない `date_format` は警告に出す（読み込みは止めない）。値の型が違えば読めない理由を出す。
- **テーマと時計は common.toml だけに置く。**アプリごとの上書きは持たない。
  - 移行: tsumugi の settings.toml に古い `theme`・`dark_theme`・`light_theme`・`[clock]` があり、common.toml に無ければ古い値を使う
    （`Common::theme_choice(Some(..))`・`Common::clock_or(Some(..))`）。設定の画面で書いた値は common.toml に入る。
- **言語だけは**アプリの config.toml に `language` があればそちらが勝つ（`ito_i18n::resolve`）。
- アプリは common.toml とテーマのフォルダーの時刻を **2 秒ごとに** UI ではないスレッドで見て、変わったら読み直し、すぐ当てる。

## 設定の画面

- 開くキーは **Ctrl+,**（macOS は Cmd+,。keymap では `<P-,>`）。開いている間にもう一度押すと閉じる。窓いっぱいに出し、サイドバーやステータスバーは隠す。
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
  - General: **Language**（`language_row`）と **CLOCK** のカード（`clock_card`）。
  - Theme: 上にモード（Follow OS / Light / Dark）、左にテーマの一覧、右に見本（`theme_page` の閉包でアプリが描く）。
- アプリ自身の設定は、そのアプリの設定ファイルに `set_key` で書く（コメントと並びを残す）。書いたら読み直して当てる。

## 時計

- ステータスバーの右端に、等幅の字体と一番強い文字の色で出す。ポインタを載せると日付を全部出す（`Clock::full`）。
- 分が変わるときだけ再描画する（`request_repaint_after(Clock::next_minute(now))`）。
- 言語が日本語なら曜日を「(月)」のように出す（`Clock::text(now, lang)`）。

## 新しいアプリを作るとき

1. `ito-common`（`clock`）・`ito-theme`・`ito-prefs`・`ito-i18n`・`ito-keys` を同じ `rev` で足す。
2. 起動時と見張りのスレッドで `Common::read` を読み、テーマ・時計・言語を当てる。
3. keymap に `<P-,>` = `settings` を足し、`ito_prefs::show` で画面を作る（ページの中身は閉包）。
4. ステータスバーの右端に時計を出す。
5. README とその CLAUDE.md から、この文書を指す。
