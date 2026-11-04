# 操作と状態の共通ルール

部品の選び方は [design-guide.md](design-guide.md)、値は [design-tokens.md](design-tokens.md)。

## 約束

- 一つの行に主なボタンは一つ。Esc／Enter が押すものを主にする。
- 状態は色だけで伝えない（`ito-a11y::State` の記号を付ける。スクリーンリーダーには `say`・`announce`）。
- 文言は `lang/*.toml` のキー（日英）。ボタンの文字を直に書かない。
- コマンドは 1 つの元から出す。パレット・ツールバー・キーが同じ名前を指す（`ito-palette`・`ito-keymap`）。
- ディスクやプロセスに触る処理は UI スレッドで走らせない。
- 取り返しのつかない操作は先に `ito-confirm`。おすすめは緑、戻せないものは赤。

## 状態

部品が見せる状態は次の 11。ito の現状（2026-11 の調べ。部品ごとに全部そろっているかは未確認）:

| 状態 | ito の現状 |
|---|---|
| Default | — |
| Hover | `Colors::hover` |
| Focus | `ito_prefs::focus_ring(ui, &response)` |
| Pressed | 共通の定義なし |
| Selected | `Colors::chosen`・`Colors::selection` |
| Disabled | egui の標準。`--keys` の `<Disabled:文字>` で読める |
| Loading | **なし**（スピナー・進捗の部品が無い） |
| Success / Warning / Error | `done`・`wait`・`err` ＋ `ito-a11y::State`。専用の役割は [#186](https://github.com/uchmk/ito/issues/186) |
| Empty | `empty_state`（薄い絵＋文言＋ボタン） |

新しい部品を ito に入れるときは、この 11 のうちどれに対応するかを docs に書く。
足りない部品（メニュー・ポップアップ・ツールチップ・検索欄・Loading）は、画面で要ると分かってから `crate-request` で頼む。

## キーボード

- Tab の順は画面の読み順。フォーカスの輪は消さない。
- Esc は閉じる・取り消す。Enter は主な操作。
- 数字キーを含むキーの書き方は `ito-keys`（`<A-S-5>` ≠ `<A-5>`）。
- 日本語の変換中のキーは奪わない（`ito-ime`）。

## 確かめ方

`--keys` の台本で撮る（`<Theme:名前>`・`<Size:WxH>`・`<Lang:ja>`・`<Hover:…>`・`<Shot:名前>`）。
メタデータと前後の比較は [#187](https://github.com/uchmk/ito/issues/187)。
