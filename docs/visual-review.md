# Visual Review（撮る・渡す・撮り直して比べる）

画面を変えたら、`--keys` の台本で撮り、AI（または人）に見せて直し、同じ条件で撮り直して比べる。
考え方は [design-principles.md](design-principles.md)。道具は `ito-keyscript`（`ito_keyscript::review` は窓なしで使える）。

## 1. 撮る

台本の `<Shot:名前>` が `shots/名前.png` を、`<Theme:>`・`<Lang:>`・`<Size:WxH>` を使って決まった条件で撮る。
`Runner` は `shots/名前.json` も隣に書く（フォルダーは完了ファイルの隣。`state/` の `<State:>` と混ざらない）:

| キー | 中身 |
|---|---|
| `name` `app` `version` | 撮った名前、アプリ、版 |
| `theme` `lang` | 撮ると頼んだ時点で効いていたテーマ（`ito_theme::name`）と言語（`ito_i18n::in_force`）。分からなければ空 |
| `theme_requested` `lang_requested` | 直前の `<Theme:>` `<Lang:>` が頼んだ値（`dark`・`system` など。頼んでいなければ空）。効いている名前（`tsumugi Dark` など）とは比べず、食い違っても失敗にしない |
| `size` | 窓の大きさ（点） |
| `zoom` | 文字の倍率 |
| `state` `component` | 画面の状態（`empty`・`error` など）、写っている部分（`window`、切り出しなら `preview` など） |
| `rect` | 写っている範囲 `[x, y, w, h]`（点） |
| `pixels_per_point` | PNG の画素と `rect` を結ぶ倍率 |
| `time` | UNIX 秒 |

アプリは `Runner::new(..).app(名前, 版)` と、`Runner::info`（`review::Meta`）の `zoom` を作るときに入れ、効いているテーマと言語は `ito_theme::set_theme(&theme)`・`ito_i18n::set_in_force(&code)` で知らせ、状態が変わる所で `state`（切り出しなら `component`）を書き換える。

**全テーマ × 状態**は台本を手で書かず、`review::matrix(themes, states, enter, leave)` で作る。
アプリが渡すのはテーマと状態の一覧と、状態に入る・出るキーだけ。名前は `状態-テーマ`。

```rust
let script = ito_keyscript::review::matrix(&["dark", "light"], &["empty", "error", "long-ja"],
    |s| enter_keys(s), |s| leave_keys(s));
```

要素単位の切り出しは、どの矩形かをアプリが決め（`Dialect::shot_suffixes` の `@preview` など）、切り出しは共通の
`review::crop(&Image, review::rect_pixels(rect, pixels_per_point))` を使う。

## 2. 撮り直して比べる

前回の PNG と今回の PNG を `review::compare(&old, &new, threshold)` に渡す（`Image::read`・`Image::write`）。

- `fraction`: 変わった画素の割合（0.0〜1.0）。`threshold` は各色の差の許容（0 は完全一致）。
- `bounds`: 変わった所を囲む矩形 `[x, y, w, h]`（画素）。変わらなければ `None`。
- `image`: 変わった画素を赤で示した差分の画像。

大きさが違えば、広い方の面積で数え、足りない部分は変わったとみなす。

## 3. レビューの指示（AI に渡す雛形）

```
添付: <名前>.png（<名前>.json の条件で撮影）。
原則: docs/design-principles.md（Quiet Precision）。トークン: docs/design-tokens.md。
見てほしいこと:
1. 主役が 1 つか。優先順位（内容 > 現在位置 > 構造 > 操作 > 状態表示 > 装飾）が崩れていないか。
2. 余白・角丸・文字の大きさがトークンの段に乗っているか（直書きの疑い）。
3. 状態を色だけで伝えていないか。
4. 長い日本語・長い名前・狭い窓で切れたり重なったりしていないか。
5. ライトとダークで読めるか（コントラスト）。
指摘は「どの部品・何が・どうあるべきか」で書く。好みでなく原則とトークンを根拠にする。
```

直したら同じ台本で撮り直し、`compare` の `bounds` が直した部品の中に収まっているか（ほかが動いていないか）を見る。
