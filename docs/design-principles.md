# デザインの原則（Quiet Precision）

uchmk のアプリ（kura・yagura・koyomi・michi・fude など）が共有する見た目と操作の考え方。値は [design-tokens.md](design-tokens.md)、
状態と操作の規則は [interaction.md](interaction.md)、部品の選び方は [design-guide.md](design-guide.md)、撮って確かめる手順は [visual-review.md](visual-review.md)。
文言の出どころは weft の `docs/design.md`「ビジュアルデザインとインタラクション」。

## コンセプト

**Quiet Precision（静かで、精密で、操作に迷わない）。** UI は内容を支えるもので、内容より目立たせない。
他のアプリの見た目をまねない。取り入れるのは、情報設計・視覚的な階層・操作の一貫性・反応の速さ。

## 原則

- **Content First**: 主役は内容。UI は内容の邪魔をしない。
- **Visual Restraint**: 色・枠・アイコン・装飾は必要な所だけ。
- **Consistent Geometry**: 余白・サイズ・角丸は共通の規則（ito のトークン `ito_theme::scale`）から取る。個別の部品に直書きしない。
- **Typography Matters**: 日本語の読みやすさ・行間・字形を実機で見る。
- **Progressive Disclosure**: 今要らないものは出さず、必要になったときに出す。
- **Immediate Feedback**: 操作にはすぐ反応を返す。
- **Consistency over Decoration**: 飾りより、同じものが同じに見えること。
- **Density with Clarity**: 密度を上げても、読み分けられること。
- **Keyboard and Pointer Parity**: キーでもポインターでも同じことができる。

## 視覚的な優先順位

編集中の内容 > 現在位置と選択範囲 > 構造と見出し > 編集とナビゲーション > 保存状態・文字コード > 補助的な装飾

- 1 つの画面に主役を複数作らない。
- 色だけで状態を伝えない（記号・文字・形も付ける。`ito-a11y`）。

## 役割の分担

- **ito が持つ**: テーマ、トークン、フォント、アイコン、ボタン、入力欄、パレット、ダイアログ、通知、余白、角丸、Hover / Focus / Pressed / Disabled の見た目。
- **アプリが持つ**: アプリ自身の内容だけ（weft ならソースの編集レイアウト、Markdown の構造ビュー、プレビュー）。

ito にあるものをアプリに書かない。足りなければ `crate-request` の Issue で頼む。

## 完了の条件

画面を変えたら、起動して撮る。変更の前後を、次の条件で比べる。動くだけでは完了にしない。

- ライトとダーク
- 長い日本語、長い名前
- 狭い窓と広い窓
- 空の状態とエラーの状態
- パレットやポップアップが開いているとき

撮り方と比べ方は [visual-review.md](visual-review.md)。
