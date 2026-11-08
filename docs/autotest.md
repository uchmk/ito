# uchmk 自動実機テスト基盤の設計

> 2026-11-08 に設計のページ（claude.ai の artifact）から移した。ここが正本。artifact は写し。

**持ち主は ito**。uchmk のアプリ（kura・yagura・koyomi・michi・fude と、これから作るもの）が同じ形で使う。契約・部品・レーンのスクリプトは ito に置き、各アプリはアダプター（状態の中身・Test ID・ハーネス・規則のファイル）だけを足す（§1a）。**最初に載せたのは kura** で、§2a・§3・§11 は kura での縦切り。見た目の行の判定は別のページ [uchmk 視覚テスト設計](visual-test.md) にある。

AI が実機テストを自律で回すための設計。Codex の設計レビュー結果と、持ち主が決めた 5 件の判断を含む。**フェーズ 1（偽の緑を閉じる）は kura v0.151.0 で実装して push した**（再テスト uchmk/kura#507）。Linux のレーンの道具（`xrun.sh`）の穴は ito v0.262.7 で直り、kura v0.151.1 で取り込んだ（CI の smoke の直しが v0.151.2）。**フェーズ 2（観測の契約）は Codex と 6 ラウンドで設計を合意し、kura の側の stage 1（設定の書き込みの通し番号と、書き込みを見る終了の判定）を kura v0.152.0〜v0.152.2 で入れた**（§2a、再テスト uchmk/kura#511）。ito の側の部品は uchmk/ito#203 で 3 段に分けて進め、閉じた（v0.263.0〜v0.267.0）。kura は v0.153.0（run の行・`<ReadHas:>`・examples/term-probe）と v0.154.0（ID の手順 `<ClickId:>`・`<Id:>`・`<ValueId:>`・`<FocusedId:>`、`<Click:文字>`、`<Assert:>`、`<Is…:>`、`<Tree:>`、`<Announced:>`、`<Clipboard:>`）で取り込み、v0.154.1〜v0.154.2 で ito-clipboard と ito v0.271.0 に移った。実行の状態機械と適合テストは uchmk/ito#204（1〜4・6・8 が v0.269.0〜v0.273.0 で入った）。状態 JSON v1 は uchmk/ito#205 の A として v0.275.0 で入った（`state/<name>.json`。アプリが選んだとき）。`<Snap:>`・Shot の付属 JSON・trace.jsonl は B として v0.276.0 で入った。ito#202（`xrun.sh`）は閉じた。**判断 3〜5 は 2026-11-08 に決まった**（持ち主の「推奨で進めてください」）。

Codex との合意の状況: フェーズ 1 は 2 ラウンドで条件つき合意（重大 3 点を仕様に入れた、§3）。フェーズ 2 は 1 ラウンド目（2026-11-08）の指摘（重大 2・高 4・中 2・軽 1）を受けて設計を v2 に書き直した。2 ラウンド目で 6 件が閉じ、残りの指摘（重大 3: 描画が止まったときの観測、設定の書き込みの成功の判定、自己テストの壊し方）をコードで確かめて全部採用し、v3 の修正にした。3 ラウンド目では、v3 の「届いていない出来事は入れ直す」が eframe の入力の持ち越しで二重に届く誤りだと分かり（重大 2・高 2・中 1）、v4 の修正にした。4 ラウンド目（重大は無し）の指摘を v5 の修正にし、5 ラウンド目で 2 件が閉じた。残りの高 1（× や終了の経路 `ended()` が、設定の書き込みの返事を見ずに成功を書く）と中 3 を、コードで確かめて全部採用し v6 の修正にした。6 ラウンド目で R1-M1 と R5-1〜3 が閉じ、重大・高は残っていない。v2〜v6 の仕様を受け入れ基準として固定した。

kura v0.154.4 / main ・ 2026-11-08 更新（初版 2026-11-07、全アプリ向けに書き直したのも 2026-11-08）・ Codex レビュー run_77a21329dfec（フェーズ 1 の 1 ラウンド目）、codex exec（フェーズ 1 の 2 ラウンド目、フェーズ 2 の 1〜6 ラウンド目）

## 要点

- 先に作るのは「アプリから得られる信頼できる観測」と「判定する道具」。エージェントを増やすことではない。
- **最優先は偽の緑の除去**。フェーズ 1 の前の `--keys` は、実行できない手順を黙って飛ばしたまま `keys: done` で終わっていた（v0.151.0 で直した）。
- UI 要素には意味のある Test ID を付け、ID で押して、押したあとの**新しい観測**で確かめる。古い観測や空の木では通さない。
- 状態の取り出しは `<State:name>`（HTTP サーバーは作らない）。JSON 版はフェーズ 2 の後半。
- 終了コード 0 は「走り切った」だけを意味する。期待値の一致は別に判定する（ハーネスがディスクの設定・子プロセスの記録・run の行を読む）。

## 持ち主の判断

### **決定** 1. 合格にする権限

機械で測れる行は、実機セッションの main が、その行の runner の証拠を読み、期待値がすべて一致し、未確認が残らず、後片付けが PASS のときに限って `[x]` を付けてよい。

見た目だけで判断する行は `[~]` のまま、持ち主が `[x]` にする。subagent と runner は `[x]` を付けない。

**2026-11-08 追記:** 見た目の行でも、期待値のすべてを同じ実行で機械が測った行（Tier 1）は「測って確かめた」行として `[x]` にしてよい（視覚テストのページの[変更 1](visual-test.md#rule) を承認）。各アプリの規則のファイルに入れるのは、ito の `docs/visual-test.md`（版 1）が push されてから。AI 判定（Tier 2）は影運用のままで、合格にはしない。

### **決定** 2. 観測の契約

状態 JSON v1（`<State:name>` が `state/<name>.json` も書く。txt は残す。ito v0.275.0 から、アプリが `Runner::state_json(true)` で選んだとき。書けない・来ないときは `keys: failed`）と、意味のある Test ID を採用する。

失敗は閉じる方向に倒す。未対応の手順、成果物の欠落や古さ、書き込み失敗、満たされない待ちは、例外なく成功にしない。

### **決定** 3. 安全の境界

フェーズ 3・4 は今の Windows ユーザーで回し、§9 のガードレール（GUID のルート・持ち主の印・ジャンクションの拒否・ローカルの操作だけ・PID + 開始時刻 + exe で止める・Job Object）を全部掛ける。`-Isolate` はサンドボックスではないので、フェーズ 5 の無人の修正ループの前に、専用の標準ユーザーか Windows Sandbox に移す。

### **決定** 4. 最初に自動化する範囲

§11 の 3 シナリオ（57.2・57.3・46.11 / 46.11a）と、視覚テストのパイロット 4 行（59.5・41.15・49.4・54.18）。

### **決定** 5. 実行の予算と証拠の保存期間

runner が回数と時間で止める（§6 の上限）: 問題ごとに 30 分、修正 2 回（その先は Opus）、Vision は 1 行につき 1 回（修正のあとの再判定を入れて 1 つの問題で 2 回まで）、1 回の実行で 20 行まで、Codex は高リスクの変更だけ。金額の上限は置かない（足すなら持ち主が決める）。

証拠: PASS の回は 14 日、FAIL・INCONCLUSIVE の回は 90 日。PR か `[x]` が引いている回は、その行の次の再テストまで残す。アプリごとに約 5 GB を上限に、古い PASS の回から消す。

決めた日: 2026-11-08（持ち主の「推奨で進めてください」）。判断 3〜5 の中身は Claude の推奨で、持ち主はいつでも変えられる。

## 1. 役割分担と置き場所

| 役 | 担当 |
|---|---|
| Opus | テストの選択、設計、最終判断、コミット・push（main だけ） |
| Sonnet | 製品コードの修正。テストの期待値は変えない。Agent ツールの subagent として起動する |
| Codex | 契約と設計のレビュー。haiku の subagent の中で `codex exec -s read-only` を 1 回回す |
| Haiku | 証拠のサンプル監査（追加の費用対効果は未確認）、`audit`・`verify` |
| vision | 見た目の要件だけ |
| wintest | 実機測定の入口として残す。定型の操作は徐々に Rust の runner に移す |

**置き場所は ito first**。観測の部品（Test ID の登録、観測の世代、ID の手順、run の行、子の確かめ役、設定画面の部品の ID）は ito（`ito-a11y`・`ito-keyscript`・`ito-prefs`）に作る。ito の `Runner` で `--keys` を回すアプリ（いまは yagura・koyomi・michi・fude）は、ito の rev を上げればそのまま使える。kura は自前の実行役（`src/script_run.rs`）なので、同じ意味を写す（§1a）。2 つ以上のアプリで使うものをアプリに先に書いて「あとで ito に移す」ことはしない（`C:\dev\CLAUDE.md`）。

### 状態 API（JSON v1、フェーズ 2 の後半）

スキーマと書き出しの関数は ito、中身は各アプリ。共通の欄は ito の部品から埋め、アプリだけの値は `app_state` の中に置く（ito の検査器はその中を見ない）。視覚の観点に要る欄（`theme`・`window`・`ui[]` の矩形）は [視覚テストのページ](visual-test.md)に詳しい。

```json
{"schema_version":1,"run_id":"r1","snapshot_seq":3,"after_step":7,"frame":120,
 "settled":true,
 "app":{"name":"kura","version":"0.152.2"},
 "screen":"settings","overlay":null,
 "focus":{"test_id":"settings.field.shell-args"},
 "theme":{"requested":"dark","name":"dark","dark":true,"tokens_sha256":"…"},
 "window":{"inner_px":[1280,800],"pixels_per_point":1.25,"monitor_work_area_px":null},
 "ui":[{"test_id":"settings.field.shell-args","role":"TextInput","text":"-NoLogo",
        "rect_px":[…],"clip_px":[…],"visible":true,"elided":false}],
 "toasts":[],
 "capabilities":["state.v1","ui.rects"],
 "app_state":{"settings":{"term":{"args":["-NoLogo"]}},"config_writes_pending":false}}
```

- 同じフレームの値をまとめて撮り、tmp に書いてからリネームで公開する。
- 必須項目の欠落、未知の major、run_id の不一致は harness error。null と未対応を区別する。
- アプリが持っていない値を作らない（前の例にあった `pending: []` は kura に対応するものが無かった。Codex の指摘）。分からない値は `null` にし、`capabilities` からも外す（例: macOS・Linux で作業領域が取れないときの `monitor_work_area_px`）。
- アプリ内の値だけを真実にしない。設定ファイルの再読込、ファイルシステム、OS のプロセスでも確かめる。
- 前半は txt の `<State:>` に行を足して済ませる（kura: `config writes pending`・`term args`・`term env names`・`terminal`）。

<a id="drivers"></a>

### 1a. Runner と自前の実行役（全アプリで同じ意味にする）

画面の名前で探す手順（`<Click:文字>`・`<Assert:文字>` など）は UI の言語に従うので、台本の先頭に `<Lang:en>`（書いた言語）を置く。

いま `--keys` を回す道は 2 つある。yagura・koyomi・michi・fude は ito の `ito_keyscript::Runner`、kura は自前の `src/script_run.rs`（木とクリップボードの手順は窓の前で断る）。同じ台本が、アプリによって違う意味で通ってはいけない。下の穴は [ito#204](https://github.com/uchmk/ito/issues/204) で頼んだ（行は ito v0.263.0、16dbe90 の `crates/ito-keyscript/src/lib.rs`）。

| 穴（Codex の全アプリ向けのレビュー、2026-11-08。コードで確かめた） | 直し方 |
|---|---|
| **`ito_keyscript::pace`（v0.271.0）が実行の状態機械の正本。** 適合テストの表は `pace::conformance::cases()`。`Pace::next` の順: ①終わり／報告なしで空 ②別の viewport ③入力が描かれた（`action_gen`） ④設定の書き込みが答えた ⑤書き込みの失敗 ⑥書き込みの待ち（10 秒） ⑦`DELIVER_WAIT` ⑧`<Wait:N>` の残りを眠る ⑨Shot の待ち（5 秒） ⑩`<Now>` ⑪最初の待ち ⑫準備（続き・`<Now>`・再確認・落ち着き 2 パス） ⑬late ⑭空なら終わり／unsettled ⑮`<Wait:N>` の消化 ⑯取り出し | **v0.272.0 から egui の `Runner` は Pace の薄い宿主**（`decide`/`answer` が Pace の `next`/`taken` を呼び、手順の実行だけを持つ）。適合テストの表は `PaceSubject` と `Runner` の両方で通る（`skips()` は空）。`Taken::Changed`（入力なしで窓が変わる: サイズ・テーマ・言語）と、押した印の規則 `conformance::acts`/`acts_taken`（Look は見つけて押したときだけ動作）が表の約束。`Need`/`skips` は、その主体ができない項目の宣言。kura は適合テストを自前の実装で通してから載せ替える |
| kura の stage 1 のうち、設定の書き込みの待ちと「`<Now>` は直後の 1 手順だけ」は kura の実行役にしか無い（ROOT の確認と `DELIVER_WAIT` は ito v0.263.0 で `Runner` にも入った）。`Runner` には落ち着きの待ちが無く、`<Now>` は `self.next = now` だけ（`lib.rs` 1563 行） | ito に**実行の状態機械**（手順の待ち・`<Now>`・書き込みの待ち・届いたかの確認・終わり方）を置き、`Runner` がそれを使う。kura はそれを呼ぶか写す。どちらにも同じ**適合テスト**（ito に置く台本と期待の表）を通す |
| 書き込みの待ちは、アプリが「書き込みが残っているか」を答えないと作れない | ito に問い合わせの口（アプリが返す `pending_writes()` のようなもの）を置く。ito-prefs の common.toml の書き込みは ito が答える |
| `<Theme:>` は効いているテーマを書き換えるので、頼んだテーマと区別できない（`lib.rs` 1579〜1582 行） | 状態に `theme.requested` と `theme.name` を分けて書く |
| Shot の失敗が文字列を返すだけ（`lib.rs` 1713 行）、付属 JSON の名前が `{name}.json` で State の JSON とぶつかりうる（1724 行） | Shot の失敗は手順の失敗（`keys: failed`）。付属 JSON は `shots/<name>.json`、状態は `state/<name>-before.json`・`-after.json` |
| どちらの道で回したかが証拠に残らない | manifest に実行の種類（Runner か自前か）と ito の rev を書く |

ほかの穴（レーンのスクリプトの後片付け、窓の作業領域の推測値、OS の通知としてのトースト）は [視覚テストのページの穴の表](visual-test.md#gaps)にまとめた。

## 2. Test ID とテストモード

- ID は意味のある名前（`settings.field.shell-args`、`settings.shell.env.row.0.name`）。ラベル、座標、順序、AccessKit の数値 ID は使わない。AccessKit の `author_id` に入れるので、Windows では UI Automation の AutomationId にもなる。
- test_id → egui id・rect・interact_rect・layer・enabled を登録する。重複（同じ名前が 2 つ、同じ部品に 2 つの名前、登録と木の食い違い）はエラー。
- ID 指定のクリックは、**前の frame で取れた新しい観測**の `interact_rect` の中心にポインタ入力を送る（同じ frame の木は無い。Codex の指摘で「そのフレームの rect」を直した）。業務関数を直接呼ばない。見えない・無効・重複・窓の外・他の層に覆われた対象は拒否する。届いたかは、すぐあとの確認（`<FocusedId:…>` など）で見る。
- まず内部状態と ID 登録から始め、UIA のスモークテストを少数足す。
- テストモード（後回し）: 新しいサンドボックス、固定の乱数・時計・ロケール・テーマ・倍率・窓サイズ・フォント、タイムアウトは単調な時計。`make-fixtures.ps1` は runner が作った GUID フォルダーの下だけで使い、manifest とハッシュを付ける。
- 「ネットワークなし」は更新確認を止めるだけでは足りない（外部で開く、webhook、ターミナル）。

## 2a. フェーズ 2 の設計（v6、57.2 → 57.3 の縦切り）

1 ラウンド目の Codex の指摘をコードで確かめて v2 に書き直し、2 ラウンド目の指摘を v3、3 ラウンド目を v4、4 ラウンド目を v5、5 ラウンド目を v6 の修正にした。6 ラウンド目で重大・高が無くなり、この内容で確定した。仕様ファイルは `C:\Users\yuu06\.claude\design\kura-phase2\` の `spec-phase2-v2-design-review.md`、`spec-phase2-v3-amendments.md`〜`spec-phase2-v6-amendments.md`、Codex の結果は同じ場所の `codex-r2.md`〜`codex-r6.md`。

**実装の状況**: kura の stage 1（設定の書き込みに通し番号を付け、手順と `ended()` が書き込みの返事を見て終わる）は kura v0.152.0 で入れ、v0.152.1（設定の画面の Export・Import・Reset も待つ）と v0.152.2（kura 自身の書き込みで「Reloaded」を出さず、監視が書き戻さない）で直した。再テストは uchmk/kura#511。ito の部品（`ito-a11y` の test_id、`ito-keyscript` の観測の世代・ID の手順・run の行・確かめ役、`ito-prefs` の ID）は uchmk/ito#203 で頼んだ。段 ①（観測の世代と `Runner` の待ち）は ito v0.263.0 で入った。ID の手順・run の行・確かめ役は kura が v0.153.0 と v0.154.0 で取り込んだ。ハーネス（scripts/check-term-settings.sh）と壊し方の自己テストは kura で作業中で、まず State に `term args`・`term env names`・`terminal` の行を足している。

### v6 で変えたこと（Codex 5 ラウンド目、重大は無し・高 1・中 3。どれもコードで確かめた）

- **× や終了でも書き込みを見る**（高）: v5 で手順の側は書き込みの返事を待つようにしたが、窓の × や kura の終了は `ended()` を通り、そこは書き込みを見ていなかった（`src/script_run.rs` の `ended()`、`src/main.rs` の × と quit の経路）。最後の手順が `<Theme:dark>` で返事が来る前に × を押すと、`keys: done`（成功）になっていた。v6 では `ended()` も、書き込みの失敗があれば失敗、返事を待っている書き込みが残っていれば「返事の前に閉じた」で失敗にする。閉じる動作そのものは延ばさない。
- **書き込みごとの期限**（中）: v5 は「返事待ちが 0 から 1 以上になった時刻」から 10 秒を数えていたので、続けて書いて、どれも 6 秒で返事が来る正しい実装も落ちた。v6 は書き込みごとに番号と送った時刻を持ち、いちばん古い返事待ちから 10 秒で失敗にする（まとめ書きの返事は、最後の番号までをまとめて消す）。
- **自己テストの分け方**（中）: 正常な run の config.toml の期待値は毎回同じなので、古い成果物は config の中身では見分けられない。古い成果物は run ID と nonce の不一致で落ち、config.toml だけを消す・初期の設定に戻す・消したはずの環境変数を残す、の 3 つは config の確かめで落ちる、と分けた。
- **見張りに実際の進みを知らせる**（中）: 30 秒の見張りは「最後に手順を実行した時刻」から測っていたので、配達の待ち → 書き込みの待ち → 画面の文字を読む待ちが続くと、どれも期限内でも 30 秒を超えた。v6 では手順の実行・配達の確認・書き込みの返事・絵の保存のときだけ進みの時刻を更新する（描画のたびには更新しないので、固まったときはこれまでどおり止まったと分かる）。期限の規則も「落ち着きの待ち 5 秒＋いちばん長い待ち 15 秒 < 30 秒」に直し、単体テストで定数を比べる。

### v5 で変えたこと（Codex 4 ラウンド目、重大は無し。どれもコードで確かめた）

- **書き込みの返事を待つ**: 設定の書き込みの返事が来るまでは、`<Now>`・5 秒の待ち切れ・手順の終わり（成功の done）・`<Quit>`・`ReadHas` のどれでも先へ進まない。10 秒返事が無ければ、次へ進んで「落ち着かなかった」と記録するのではなく、その場で `keys: failed`（`step: <config write>`）。
- **`<Now>` は直後の 1 手順だけ**（今の kura の穴）: 今は `<Now><State:mid>` のあと、読み取りの手順が `<Now>` を解かないので、残りの手順が全部、落ち着きを待たずに進む。どの手順でも 1 つ実行したら解く。CHANGELOG に「変更」として書く。
  - 時間を決めた読み取り（ito v0.267.0）: `<Wait:N>` は `<Now>` を使い切る手順に数えない。`x<Now><State:a><Now><Wait:150><State:b>` と書く（`a` は `x` が描かれたらすぐ、`b` は `a` の約 150 ms 後。間の待ちも落ち着きを待たない）。`<Now><State:a><Wait:150><State:b>` の `b` は今までどおり落ち着きを待つ（穴は閉じたまま）。
  - N は下限で、約束ではない。届いたかの確認・書き込みの待ち・鮮度の規則は `<Now>` でも飛ばさない。実際にかかった時間は `<State:>` の `since last action: 153 ms` の行に書く（`Runner::since_action`・`since_action_line`。基準は直前の操作の最初の入力が入った時刻で、状態を取ったフレームで読む）。
- **終わったあとの config.toml**: `run` は kura が実際に使った config.toml を成果物のフォルダへ決まった名前でコピーし、`check` はそれを必須にする（無ければ失敗。初期の設定から作り直さない）。
- **確かめ役の lock の失敗**: 「もうある」のときだけ次の番号へ進む。書けないなど、ほかのエラーは `probe error` で終了コード 1（番号を探し続けない）。
- **期限の規則**: 実行役の中の期限（届かない 10 秒、書き込み 10 秒、読む 15 秒など）は、どれも見張りの 30 秒より短くし、単体テストで定数どうしを比べる。
- **書けないフォルダの確かめ**: 既にある config.toml ではなく、同じフォルダのまだ無い名前に `touch` して、フォルダが書けないことを確かめる。

### v4 で変えたこと（Codex 3 ラウンド目、どれもコードで確かめた）

- **入れ直さない（v3 の誤りを取り消した）**: eframe 0.36.2 は描画の無い tick（logic-only）のあと、hook が入れた出来事を含む入力を丸ごと残し、次の tick の先頭に戻してから hook を呼ぶ（`epi_integration.rs:337-338`・`:357-362`）。だから入れ直すと 2 回届く。v4 は、ROOT の hook でだけ一度入れ、ROOT の `cumulative_frame_nr`（`run_ui` の終わりでだけ増える）が注入のときより増えたら「届いた」とする。届くまでは次を入れず、手順を進めない。観測は届いた pass より後の pass のものだけを使う。
- **子の窓が入力を持っていく**（main が見つけた）: 持ち越しの入力は窓ごとではなく 1 つで、子の viewport の pass も同じものを取る。注入が届く前に子の窓の hook が来たら `keys: failed` にする。kura に子の窓は無いが、ito の実行役は他のアプリでも動く。
- **届かない期限**: hook は来るのに描画が 10 秒来なければ `keys: failed`（窓が隠れている・最小化）。hook そのものが来なければ今の見張りの `keys: stalled`（30 秒）。
- **kura の実行役**: v3 の「kura は ui の pass の中で動くので要らない」は誤り（kura も入力の hook から入れる）。ただ今の kura は入れ直さず、届く前に次を入れないので、足すのは ROOT の確認と観測の鮮度だけ。
- **書き込みを始められない失敗**: フォルダが無い・書き手を起こせない・送れない、も 1 つの口（`fail_config_write`）で失敗として残す。common.toml は呼ぶたびにスレッドを起こすのをやめ、1 本の書き手で送った順に書き、返事の番号が順番どおりかを確かめる。
- **自己テストの壊し方**: 書けない設定フォルダ (g) は kura の config のフォルダだけを書けなくし、成果物（done・State・確かめ役）は別のフォルダに置く。書けなくなったことを先に確かめ、root などで書けてしまえば自己テストを失敗にする（Linux の CI は root でない利用者）。`UCHMK_CONFIG_DIR` も作業のフォルダに置く。
- **確かめ役の lock**: 書き終えたら lock を消してから `probe ready N`。番号は lock も txt も無いものを選ぶ。
- **`ReadHas` の読み手**: アプリが hook のたびに読み手を渡す。ペインが無いは「待つ」、読めないは「すぐ失敗」。15 秒は最初に試した時刻からの固定の期限。

### v3 で変えたこと（Codex 2 ラウンド目）

- **描画が止まったときの観測**: ~~注入した出来事は、その pass の出力の hook で届いたと確かめるまで「未確認」とし、未確認のまま次の入力の hook が来たら同じ出来事を入れ直して手順を進めない。~~（v4 で取り消した。入れ直すと二重に届く）根の窓（ROOT）で、最小化されていない観測だけを使う。手順の期限は frame や settled より先に見る（tick が来ない間は stalled、来る間は failed）。
- **設定の書き込みの成功の判定**: keymap の書き込みも同じ送り口に通す。common.toml は自分の通し番号と返事の口を持ち、監視の読み直しは数えない。返事を適用したあとでだけ「答えた」を進め、失敗は次の tick で `keys: failed`・`step: <config write>` にする。読む手順は、操作と適用の両方より新しい観測を要る。書き手が panic すれば hook が終了コード 101、黙って止まれば 5 秒後の `keys: unsettled`。
- **ID の照合とクリックの位置**: 名前と AccessKit の id が 1 対 1 で合わなければ、その名前の ID の手順は全部（否定も）失敗。`ClickId` は層の変換、窓の中、層が操作を受けるか、その点の一番上の層かを確かめる。届いたかは保証しないので、`ClickId` のあとには必ず確認の手順を置く（解析で断る）。
- **新しい手順 `<ReadHas:NAME=TEXT>`**: アプリが読める文字（kura ではペインの文字）に TEXT が出るまで最大 15 秒待つ。固定の `<Wait:3000>` の代わりに、確かめ役の `probe ready N` を待つ。
- **確かめ役の書き方**: 番号は lock ファイルの排他作成で取り、tmp に書いて同期してから名前を変え、そのあとで `probe ready N` を出す。失敗なら出さずに終了コード 1。ハーネスは tmp・lock が残っていれば失敗。
- **run の行**: 1 行目を変えず、最後が `keys: done` ならその直前、そうでなければ末尾に、ちょうど 1 行。panic・stalled（ito と kura の見張り）も含めて全部の書き口を通す。
- **ハーネス**: `run` と `check` に分け、受け継いだ環境変数（`KURA_TERM_SHELL` など）を消してから全部を明示する。壊し方 7 つ（別の欄、Save しない、閉じずに隠す、環境変数を受け継ぐ、古い done、確かめ役が書かない、書けない設定フォルダ）に、それぞれ落ちる場所と理由を決めた。

### わかった事実

- kura の `--keys` は ito の `Runner` を使わない自前の実行役（`src/script_run.rs`、v0.151.0 で分けた）。木とクリップボードの手順は v0.154.0 から自分で実行する（v0.151.0〜v0.153 は窓の前で断っていた）。yagura・koyomi・michi・fude は `Runner` を使う（§1a）。
- ito の `Tap` は、AccessKit の更新が無い frame で前の観測をそのまま残す。`<Tree>` は空の木でも通る。`<Assert:>` など空の文字列は何にでも合う。
- 設定の書き込みは別のスレッドで、待っている仕事を 1 回にまとめて書く。`settled` に入らない。
- ターミナルを開くキーは、開いていれば焦点を移すだけ（新しい子にならない）。
- kura のコピーは egui の `CopyText` を通らない（arboard に直接）。v0.268.0 から、自分の道でコピーするアプリは `ito_keyscript::note_copy(text)` を呼ぶ（改行を OS に合わせる前の文字で）と、`<Clipboard:>`・`<ClipboardHas:>` に見える（#206 の 1）。

### ito に作るもの（ito のセッションに頼む）

| 部品 | 中身 |
|---|---|
| `ito-a11y` | `test_id(resp, name)`: 描いたあとの Response に `author_id` を付け、位置・層・有効を登録。AccessKit が切れていれば何もしない |
| `ito-keyscript` の観測 | 観測の世代。更新の無い frame は空・古いとする。操作のあとの世代でしか確認を通さない。`<Tree>` は空なら失敗 |
| `ito-keyscript` の手順 | `<ClickId:>`・`<Id:>`・`<Id:!>`・`<FocusedId:>`・`<EnabledId:>`・`<DisabledId:>`・`<ValueId:name=text>`（完全一致）。空の文字列の手順は解析で断る |
| run の行 | `<APP>_KEYS_RUN_ID`（run_id はレーンの起動役が作って渡す）。done ファイルのすべての終わり方で、最後の状態の行の前に `run: <id>`（`keys: done` が最後の行のまま） |
| 子の確かめ役 | テスト用の「シェル」。nonce・引数・指定した環境変数を `probe-N.txt` に書いて生き続ける。kura は `examples/term-probe.rs` に 1 行だけ |
| `ito-prefs` | 欄に `settings.field.<id>`、Shell カードに `settings.shell.*`。環境変数の行は Edit のあいだ変わらない番号（消しても振り直さない） |
| 実行の状態機械と適合テスト | §1a。`Runner` が使い、kura も同じ表で確かめる（[ito#204](https://github.com/uchmk/ito/issues/204)） |
| レーンのスクリプト | `scripts/lanes/` の `keys.ps1`・`xrun.sh` が run_id を作って渡し、`<evidence>\<run_id>\` を毎回新しく作る。後片付けと manifest は途中で落ちても必ず回す（keys.ps1 の `-Evidence`・xrun.sh の `--evidence`、v0.279.0） |

ito#203 は 3 段で進んでいる（持ち主の判断、2026-11-08）: 段 ① 観測の世代と `Runner` の待ち（v0.263.0、16dbe90。済み）、段 ② `ito-a11y` の test_id・`<ClickId:>` などの ID の手順・`<ReadHas:>`（v0.264.0、2d25645。済み）、段 ③ run_id と `with_run`・確かめ役 `probe::main`・Shell カードの ID（v0.265.0、50612a3。済み）。B4（コピーと読み上げは直前の操作より後だけ）は v0.266.0（99b5e15）、時間を決めた読み取り `<Now><Wait:N>` と `since last action:` の行は v0.267.0。ito#203 は閉じた。実行の状態機械と適合テストは別の Issue [ito#204](https://github.com/uchmk/ito/issues/204)（1〜3 は v0.269.0・v0.271.0・v0.272.0 で入り、残りは 4 `<Theme:>` の要求と適用・6 付属 JSON の置き場所・8 manifest）。状態 JSON v1 のスキーマと書き手・矩形つきの `<Tree>`・`<Snap:>`・検査器・証拠のフォルダは [ito#205](https://github.com/uchmk/ito/issues/205)（見た目のパイロットの前に要る）。

<a id="adopt"></a>

### ほかのアプリが載せるとき

1. **ito の rev**: v0.273.0 以降（ito#203 は閉じた。ito#204 の 1〜4・6・8 が入った rev）にそろえる（Runner のアプリはこれだけで観測の世代・ID の手順・run の行が入る）。ito#204 の残り（5・7）が入ったら、その rev に上げる。
2. **`scripts/lanes.conf`**: `evidence=`（既定 `C:\dev\<app>-evidence`。kura は今の `C:\dev\filer-evidence`）と `visual_specs=`（既定 `tests/visual`）。
3. **状態のアダプター**: `screen`・`overlay` の名前、`app`、`app_state` の中身、書き込みの待ちに答える口。
4. **Test ID**: 自分の画面の部品に `<画面>.<部品>`。ito の部品（設定の画面・パレット・トースト）の ID は ito が付ける。
5. **ハーネスと確かめ役**: 子を起こすアプリ（yagura のターミナル）は ito の確かめ役を使う。壊し方の自己テストも一緒に。
6. **規則のファイル**: 機械で測った行の `[x]` の規則（変更 1）は承認済み（2026-11-08）。各アプリは載せるときに、視覚テストのページの [成果物 5](visual-test.md#rule) の雛形で自分の文を置き換える。Tier 2 はそのアプリで影運用の基準を満たしてから。載せるまでは今の規則のまま。
7. **順**: kura のパイロットが通ったら yagura・koyomi・michi・fude。4 つの順はパイロットのあとで決める。違いは準備だけ（2026-11-08 の `[~]` の行と規則の文: yagura 15 行・`.claude/windows-role.md` 53〜56 行、michi 337 行・`CLAUDE.md` 63 行、fude 5 行、koyomi 0 行。koyomi と fude は、載せるときに規則の文を足す）。tsumugi・minato は 2026-11-08 にメンテナンス対象から外した（代わりに Orca と Linear を使う）ので載せない。これから作るアプリも同じ形で載せる。

### kura でやること（最初に載せるアプリの例。Sonnet の subagent）

- 木を読む手順（`ClickId`・ID の確認・`Tree`）、文字で探す `Assert`・`Is`・`ClickText`、`Clipboard`、`Announced` は kura v0.154.0 で全部受けた。kura が自分で読み上げを出す（AccessKit の live、トーストの announce）のは ito#207 を待つ（kura TODO 57）。
- 設定の書き込みに通し番号を振り、まとめ書きの返事は最大の番号を返す。common.toml は 1 本の書き手にする。始められない失敗も失敗として残す。手順の待ちを「落ち着いた かつ 書き込みが残っていない」にする。
- 注入は ROOT の hook でだけ、ROOT の frame 数で届いたことを確かめる（今の「入れ直さない」はそのまま）。`ReadHas` のためにペインの読み取りを「無い・読めた・失敗」に分ける。
- `<APP>_KEYS_RUN_ID`（kura では `KURA_KEYS_RUN_ID`）を done ファイルの全部の書き口に通す。
- ハーネス `scripts/check-term-settings.sh`（Linux の CI）: 空の設定フォルダ、期待と違う初期値（`args = ["--initial"]`）、受け継いだ環境変数を消す。台本は 57.2 → Cancel → 57.3 の追加 → 削除。終わったら done の run の行、ディスクの config.toml（TOML として読む）、確かめ役の記録がちょうど 2 つで中身が合うかを見る。
- 壊し方の自己テスト（欄に届かない、Save しない、閉じずに開き直す、環境変数を受け継ぐ、古い done、確かめ役が書かない）がどれも失敗すること。
- TESTING.md の 57.2 / 57.3 には「自動: …」を添えるだけで `[x]` は付けない。

### 後回し

状態 JSON v1 の ito 側（v0.275.0 で実装）と検証（ito#205。kura の serde_json の直接依存も、そのとき）、レーンのスクリプト（`xrun.sh`・`keys.ps1`）での run ID の照合と証拠のフォルダ（v0.279.0 の `-Evidence`・`--evidence`。照合は `evidence.py manifest` がする）、テストモード、kura でのクリップボードと読み上げの確認（受けるときは「直前の操作より後」だけを見る）、Windows のハーネス。

## 3. 黙って飛ばす不具合（P0、フェーズ 1 で直した）

フェーズ 1 の前の kura の `--keys` ドライバーは、ito-keyscript が定義する窓読み取りの手順（ClickText・DClickText・Assert・Clipboard・ClipboardHas・Is・Tree・Announced）を実装しておらず、`main.rs:1080-1081` のキャッチオールで黙って飛ばしていた。使った台本は何も検査せずに通った。（このページの行番号はフェーズ 1 の前の `main.rs`。v0.151.0 で実行役は `src/script_run.rs` に移った。）

### 確認できた偽の緑の経路（main.rs）

- 前進の条件は `frame >= last_frame + 2 && (settled() || waited_long)`（954 行）。`waited_long` は 5 秒超（939 行）なので、settled を待っても完了の証明にならない。
- `<Shot>` の待ちも 5 秒で解除される（942-947 行）。
- `let _ = std::fs::write(done, …)`（963 行）が書き込みの失敗を捨てる。
- Read の未知タグは `_ => {}`（1047 行）で黙って飛ばす（Codex の指摘。2026-11-08 にコードで確かめた）。
- State・PaneText・PreviewText の書き込み失敗と、Shot の失敗（プレビューの窓が無い・画面の外・保存の失敗）は `app.error` のトーストだけで、結果は `keys: done` のまま。閉じた・終了の報告（1167・1180 行）も `let _ =`。
- `keys.ps1` は `-Out` を再利用すると古い txt/png が残る → v0.150.4（ito の rev 上げ）で直った。再利用した `-Out` は先に前回の done・txt・png・json を消し、自分で作っていないフォルダーなら止まる（`keys.ps1:116-125`）。

### 直し方

- 解析時に能力を確かめ、最初の操作の前に拒否する。失敗は `keys: failed`・非 0 の終了コード・手順の位置と理由で返し、SKIPPED に変えない。
- State の書き込み失敗と成果物の欠落は、実行を失敗にする。
- 故障注入のテスト: 一致しない assert、未対応の手順、読めない出力、無い PNG が、すべて成功以外で終わること。
- Tree・Clipboard・Announced の本実装を待たない。先に拒否を入れ、次に State の assert と ID クリック。

### Codex 2 ラウンド目で足したこと（2026-11-08、どれもコードで確かめた）

- **失敗の done ファイルの形**: ito の `failed_report` は `keys: failed` の後に `step:`・`why:`・`left: N not pressed` を書く。最後の行は `keys: failed` ではなく `left: …`。窓が開いたあとは前に `state_report` が付く。判定は「`keys: failed` の行を含み、最後の行が `keys: done` でない」+ 終了コード 3。
- **最後の Shot が届く前に窓を閉じると成功になる**: 台本の列は空なので、閉じた報告が `closed_report(0)` = `keys: done` を書く（1164-1168 行）。待っている絵があれば失敗にする。
- **settled でない前進は終了コードも非 0**: 最後の行 `keys: unsettled` に加えて終了コード 3。あとで窓を閉じても成功に戻らない。5 秒の escape は残す。
- 閉じた（× ボタン）経路は `save_on_close`、終了の経路は `on_quit` のまま（入れ替えない）。失敗したら窓を閉じ、`run_native` が戻ったあとに終了コード 3 で終える。
- `left` と unsettled の一覧は台本の手順の数で数える（分けた Drag の各イベントや Size が足す Wait を数えない）。
- 見張り（`watch_script`）の停止報告の書き込み失敗も stderr に出し、失敗にする。
- 窓読み取りの手順（Assert など）はフェーズ 1 では拒否だけで、実装しない（ito の `check` は空の観測で `<Assert:!text>` を通すので、つなぐのは別の作業）。→ フェーズ 2 の観測の世代で扱う（§2a）。
- 範囲: フェーズ 1 は上の失敗を「成功にしない」だけ。状態 JSON v1・Test ID・成果物の鮮度は後のフェーズで、フェーズ 1 だけで判断 2 を満たしたとは言わない。
- **レーンの限界（直った）**: `scripts/xrun.sh`（Linux）は最初の行が `keys: stalled` のときしか `keys: done` を見ず、警告だけで終了コード 0 だった。ito v0.262.7（uchmk/ito#202）で、最後の行が `keys: done` でなければ理由を stderr に出して 0 以外で終わるようになり、kura v0.151.1 で取り込んだ。`keys.ps1`（Windows）は前から最後の行が `keys: done` でなければ結果にしない。

## 4. シナリオ形式

TOML。要件の正は TESTING.md のまま、TOML は手順だけを持つ。ito-testcheck が作るチェックリストは集計の見え方。**フェーズ 2 の前半は TOML を作らず**、`--keys` の台本とシェルのハーネスで 57.2 / 57.3 を通す。TOML の形は後半で決める。

- 行 ID と要件文のハッシュで結ぶ。文が変わったら承認されるまで stale。
- 節が複数ある行は節ごとに assert を置く。一部だけ通っても行全体に `[x]` は付けない。
- `affected_files` は選択のヒントにすぎない。ito や lockfile の変更では広く選ぶ。

```toml
schema_version = 1
id = "settings.shell.args"
rows = ["57.2"]
tags = ["settings", "pty"]
affected_files = ["src/ui/settings.rs", "src/terminal.rs"]
requires = ["state.v1", "test_id"]
timeout_ms = 60000

[preconditions]
fixture = "empty"
reset = true
locale = "en"
theme = "light"

[[steps]]
click_id = "settings.field.shell-args"
keys = "-NoLogo<Enter>"

[[steps]]
assert = { source = "state", path = "settings.term.args", op = "eq", value = ["-NoLogo"] }

[[steps]]
await = { source = "file", path = "config.toml", op = "contains", value = "-NoLogo", timeout_ms = 3000 }
```

## 5. 機械の判定と vision の分担

### 機械で判定する

- ファイルの中身、保存された設定
- フォーカス、選択、処理の完了
- PTY の出力、子プロセスの引数
- 窓の矩形

### vision を使う

- 描画の欠け、文字の重なり、読みやすさ
- 数値で測れるもの（コントラスト、寸法）は数値を優先
- 基準・失敗例を先に決め、PASS / FAIL / INCONCLUSIVE と証拠の場所を返す
- 機械の FAIL は覆せない

- ピクセル差分は、同じ OS・GPU バックエンド・フォント・テーマ・DPI の基準画像だけと比べる。動的な領域はマスクし、閾値は先に固定する。
- `@preview` の切り抜きだけでなく、全体の画像も残す。
- テーマ × DPI は変更に関係する分を毎回、全組み合わせは定期的に。
- 「ちらつかない」は短い画像列かフレーム時間で見る。
- 同じ vision モデルを何度も呼んで未確認を消さない。

## 6. 失敗の分類と修正ループ

- 分類: 製品の不具合 / テストの不具合 / harness error / 環境のゆらぎ / 仕様の曖昧さ。判別できないものは INCONCLUSIVE にして Opus へ。
- 診断のための再実行は 1 回まで。最初の FAIL は保存する。同じ原因の修正は 2 回まで、その先は Opus。
- Sonnet が変えるのは製品コードだけ。assert・fixture・基準画像・セレクターの変更は別の差分にして Codex / Opus が見る。失敗を隠すための閾値の緩和は禁止。
- 修正後は、元の失敗と近隣のテスト、そのあと選んだ回帰を回す。
- 上限（判断 5 で決定）: 問題ごとに 30 分、修正 2 回、Vision は 1 行につき 1 回（修正のあとの再判定を入れて 1 つの問題で 2 回まで）、1 回の実行で 20 行まで。金額ではなく回数と時間で runner が止める。

## 7. 結果の保存

- 保存先は `<evidence>\<run_id>\`（`<evidence>` は各アプリの `lanes.conf` の `evidence=`）。1 回の実行が 1 つの新しいフォルダ。形は全アプリで同じ（[視覚テストのページ](visual-test.md)の証拠の節）。リポジトリの外なので git に入らない。基準画像は別にレビューして入れる。
- 全手順の `trace.jsonl` と `assertions.json` を残す。保存できなければ完了にしない。
- 項目: テスト ID、コミット、exe のハッシュ、環境、時刻、PASS / FAIL / INCONCLUSIVE / SKIPPED、失敗した手順、期待と実測、成果物、操作の履歴、AI の分析、未確認。

## 8. 効率

- まとめて回すのは、同じ exe・fixture・設定で、壊さない操作だけ。設定・PTY・永続化のテストは毎回新しく起動する。
- 待ちは「述語 + 期限 + 最後のスナップショット」。期限切れは失敗。
- 変更に基づく選択と固定のスモーク集合。選ばれなかったテストの一覧も保存する。
- R: のビルドキャッシュのキーは target・toolchain・profile・lockfile・features・RUSTFLAGS・ito rev。実行直前に exe のハッシュを確かめる。
- デスクトップのロックは、fixture の配置から後片付けまで runner が持つ。すべての起動経路に掛ける。ビルド・単体・headless は並列でよい。

## 9. 安全

- `-Isolate` はサンドボックスではない。フェーズ 3・4 は今のユーザーで、下のガードレールを全部掛けて回す。フェーズ 5 の無人の修正ループの前に、専用の標準ユーザーか Windows Sandbox に移す（判断 3）。
- GUID ルート + 持ち主マーカー + 正規化したパスの確認。ジャンクションによる脱出は拒否する。
- 既定ではローカルの fixture 内の操作だけ。webhook・メール・外部で開く・実際の共有・インストール・昇格は、明示の許可がいる。
- プロセスを止めるときは PID + 開始時刻 + exe のパス + 所有で確かめる。名前では止めない。Windows では Job Object に入れ、実行の終わりにまとめて止める。
- クリップボード、画像、ファイル名、ターミナルの出力は信頼できないデータとして扱う。
- 後片付けの失敗や残ったプロセスがあれば、次の実行を止める。

## 10. フェーズ

| フェーズ | 内容 |
|---|---|
| 1 | 偽の緑を閉じる（`--keys` を失敗側に倒す + 故障注入テスト）。**済み** v0.151.0（d0f778b）。TESTING の 25.17a・25.17b を足し、再テストは #507。v0.151.1 で CI の smoke（ubuntu、Xvfb）が `xrun.sh` 経由で成功・書けない State・拒む手順の 3 通りを確かめ、v0.151.2 でその smoke の読み方を直した |
| 2 | 観測の契約。**設計確定** Codex と 6 ラウンドで合意。kura の stage 1 は v0.152.0〜v0.152.2 で入れた（再テスト #511）、ito#203 は閉じ、kura は v0.153.0〜v0.154.4 で取り込んだ。ito#204 は 1〜3 が入り残りは 4・6・8。全アプリで同じ意味にする実行の状態機械と適合テストも ito に（§1a）。前半（§2a）: Test ID・ID の手順・観測の世代・run の行・設定の書き込みの完了・57.2 / 57.3 の縦切り。後半: 状態 JSON v1、レーンの run ID の照合、テストモード |
| 3 | 縦に通す 3 つのテスト（§11。57.2 / 57.3 はフェーズ 2 の前半で先に通す）。**範囲決定** 判断 4。今のユーザーで §9 のガードレールを掛けて回す（判断 3） |
| 4 | 見た目 + 回帰。**範囲決定** パイロット 4 行（59.5・41.15・49.4・54.18）。Tier 1 は変更 1 の承認で使える（ito の `docs/visual-test.md` 版 1 が push されたあと）。Tier 2 は影運用（20 行以上・誤合格 0・カナリア 100 % を、アプリごとに数える） |
| 5 | 上限つきの修正ループ（§6 の上限、判断 5）。始める前に、専用の標準ユーザーか Windows Sandbox に移す（判断 3） |

各フェーズに入る条件、出る条件、削るものの一覧を持つ。フェーズ 2 の前半の出る条件は、ハーネスが CI の Linux で通り、6 通りの壊し方でどれも失敗すること。

## 11. 最初の 3 シナリオ（kura）

| 行 | 内容 | 確かめること |
|---|---|---|
| 57.2 | Shell args（TESTING.md:1789、#429） | 設定ファイルへの書き込み、子プロセスの引数 |
| 57.3 | 環境変数の追加と削除（TESTING.md:1790、#429） | 保存、新しい子の環境、削除後は新しい子に無いこと |
| 46.11 / 46.11a | spot ×6 でコンソール窓（TESTING.md:1522-1523、#505） | 見える窓・所有する子・conhost を数える。全体の conhost 数ではない |

57.2 / 57.3 の UI 入力の半分は、フェーズ 1 までの `--keys` では届かなかった（#443: Shell カードの欄に文字を入力できない）。フェーズ 2 の `<ClickId:>` で欄に焦点を移し、届いたことを `<FocusedId:>` で確かめてから入力する。子の側はテスト用の確かめ役（実のシェルではない）が引数と環境を書き出す。

## 12. 見落とし

| 項目 | 状態 |
|---|---|
| State はアプリの意図であり、画面や OS への効果の保証ではない | **注意** |
| ID による直接操作は、OS の入力経路のテストの代わりにならない | **注意** |
| テスト用の確かめ役で通っても、実のシェル（pwsh・cmd・bash）の起動の確かめにはならない | **注意** |
| `--keys` はセッション復元を切るので、63.x はそのままでは測れない | **測れない** |
| 61.1 のインデックス表示は settled 待ちでは見逃す | **見逃す** |
| 前後 2〜3 点のスナップショットでは「窓が一度も出ない」「1 秒以内」「再インデックスなし」は証明できない | **注意** |
| `keys.ps1` のシステムダイアログ検出は限られたクラスと 500 ms 間隔 | **注意** |
| kura: リンクされた ito の rev と `C:\dev\ito` の一致（2026-11-08: どちらも e0015f90。ほかのアプリは載せるときに確かめる） | **確認済み** |
| kura の `--keys` は ito の `Runner` を使わない自前の実行役。ito の観測の部品をつなぐには kura の側の作業が要る。yagura・koyomi・michi・fude は `Runner`（§1a の穴） | **確認済み** |
| 設定の書き込みは別スレッドのまとめ書きで、`settled` に入らない（フェーズ 2 で通し番号を足す） | **確認済み** |
| kura と ito の `keys.ps1` は SHA256 が同一 | **確認済み** |
| CI に ARM64 の `--keys` スクリーンショット + done の確認と、Windows の不正台本の終了コード確認がある。意味を持つ GUI の assert は無い | **確認済み** |
| CI の GUI カバレッジの全体像 | **未確認** |
| ゲート: TESTING の行が変わったのにハッシュが不一致、選択ゼロ、期待 assert ゼロ、大量 SKIPPED、コミットと合わない dirty な exe を検出する | **要実装** |
| 6 ターゲット対応は 6 回の実機合格ではない。x64 の結果を ARM64 に流用しない。Xvfb は実デスクトップの代わりにならない | **注意** |
| Haiku を足す費用対効果 | **未確認** |
| Codex との合意（フェーズ 1）: 2 ラウンド目（最後）で、フェーズ 1 の仕様と判断 1・2 を見せた。重大 3 点（失敗の done の形、最後の Shot と閉じた経路、unsettled の終了コード）を仕様に入れた。このページそのものは見せていない | **条件つき合意** |
| Codex との合意（フェーズ 2）: 1 ラウンド目で v2 に書き直し、2 ラウンド目で 6 件が閉じた。残りの重大 3（描画が止まったときの観測、設定の書き込みの成功の判定、自己テストの壊し方）と中 5 は、コードで確かめて全部採用し v3 の修正にした。3 ラウンド目で 2 件が閉じ、重大 2（入れ直しの二重配送、書き込みを始められない失敗と common の順番）・高 2・中 1 を v4 の修正にした。4 ラウンド目（重大は無し）を v5、5 ラウンド目の高 1（× や終了の経路が書き込みを見ずに成功を書く）・中 3 を v6 の修正にした（§2a）。6 ラウンド目で R1-M1 と R5-1〜3 が閉じ、重大・高は無くなった | **合意** |
| Codex のサンドボックスの UAC: 持ち主の「1 で進めて」で、権限を上げない（unelevated）設定を試したが、この PC ではコマンドを起動できなかった（`CreateProcessAsUserW failed: 5`）。設定は上げたまま（elevated）にし、文書も `~/.codex/config.toml` も変えていない。持ち主が UAC の画面の詳細を取ってくれた（03:01）。出しているのは npm の CLI ではなく、Codex のデスクトップ アプリの `codex-windows-sandbox-setup.exe`（`interactive-provision`）だった。サンドボックスのログでは、00:10・02:10・03:01 のどれも、こちらの `codex exec` の 1 分以内に、アプリの `codex.exe` が設定を作り直していた。03:01 は版が変わっていないのに、秘密のファイルが空になった直後だったので、更新のあとだけではない。仮説: アプリが入れた部品が CLI の実行のたびに WindowsApps の中のプログラムを起こし、アプリのサンドボックスを通る。候補は 3 つ: `~/.codex/config.toml` の MCP サーバー `node_repl`、同梱のプラグイン `unified-computer-use` の MCP サーバー `cua_repl`（WindowsApps の `node.exe`）、手番の終わりの `notify`（`codex-computer-use.exe`）。6 ラウンド目は、その実行だけ 3 つを止めて（`-c mcp_servers.node_repl.enabled=false --disable plugins -c 'notify=[]'`）回した（`-c mcp_servers.cua_repl.enabled=false` は、config.toml に無い項目を作ってしまい、設定の読み込みで落ちた）。**結果は決め手にならなかった**: サンドボックスのログの最後は 03:01:57 の「sandbox users missing or incompatible with marker version」で、そのあとの 2 回（フラグありの 03:14、フラグなしの 04:07）はどちらも setup を起こさず、UAC も出なかった。フラグが要るとは示せていない。setup が走るのは、サンドボックスの印（marker）と合わないと判断されたとき（00:10 と 02:10 の間で command runner が 0.166 から 0.167 に替わった）と見られる。設定は上げたまま、`~/.codex/config.toml` も変えていない。フラグを既定にするかは持ち主の判断 | **要判断** |
| ワーカーと Codex の起動: 持ち主の指示で、どちらも Agent ツールの subagent として起動する（Sonnet のワーカーは `model: sonnet`、Codex は haiku の包みの中で `codex exec -s read-only`）。kura の `dev-orchestra-kura`（v0.151.3）とグローバルの `dev-orchestra`（claude-config 276edf0）に書いた。`~/.claude/CLAUDE.md` の書き換えは、同じファイルに他のセッションの未コミットの変更があるのでコミットしていない | **決定** |
| `xrun.sh`（Linux のレーン）は最後の行が `keys: done` でない結果も成功として扱っていた。ito v0.262.7（uchmk/ito#202）で直り、kura v0.151.1 で取り込んだ。Linux での確かめは CI の smoke に入れた（この PC には WSL が無い）。ito#202 は閉じた | **済み** |
| 詳細ビュー（`m a`）の不具合 3 件（数万ファイルのフォルダーでスクロールし続けると落ちる、所有者などの欄のちらつき、フォルダーを移ると使用量が消える）を、フェーズ 2 より先に v0.151.4 で直した。落ちるのは AMD の OpenGL のドライバーの中なので、kura は 1 フレームの仕事を減らし、避け方として `[ui] backend = "dx12"` を案内した。再テストは 69.10・69.15〜69.19 | **済み** |

## /clear 後のセッションに渡すプロンプト

> 移したときの文に、2026-11-08 の載せるアプリの変更（kura のあとは yagura・koyomi・michi・fude）だけを足した。中の artifact の URL は写しで、正本はこの `docs/autotest.md` と `docs/visual-test.md`。

```text
uchmk の自動実機テスト基盤（持ち主は ito。最初に載せるのは kura: C:\dev\kura、main、v0.154.4）の続き。フェーズ 2（観測の契約）。返答は日本語、コード・コメント・コミットは英語。

【読むもの】
1. 設計ページ（Artifact）: https://claude.ai/artifact/8VZmtoXgfxC544itHX8SK9（§1a が Runner と自前の実行役、§2a が kura のフェーズ 2）と、視覚テストのページ https://claude.ai/artifact/La6ZMvBmRDuQGP6y7rCWgT（穴の表 #gaps、規則の雛形 #rule）。正のファイルは C:\Users\yuu06\.claude\design\uchmk-autotest\ の autotest-design.html・visual-test.html（R: は再起動で消えるので置かない）
2. フェーズ 2 の仕様（確定。受け入れ基準）: C:\Users\yuu06\.claude\design\kura-phase2\ の spec-phase2-v2-design-review.md と spec-phase2-v3-amendments.md〜spec-phase2-v6-amendments.md、Codex の結果 codex-r2.md〜codex-r6.md
3. CLAUDE.md、C:\dev\CLAUDE.md の「ito first」、kura の docs/claude/lanes.md の「[x] の規則（正）」、.claude/skills/dev-orchestra-kura/SKILL.md

【決定済み】
- 判断1（合格の権限）: 機械で測れる行は、実機セッションの main が runner の証拠（期待値すべて一致・未確認なし・後片付け PASS）を読んだあとだけ [x] を付けてよい。見た目だけの行は [~] のまま持ち主が [x] にする。subagent と runner は付けない。2026-11-08: 視覚テストの変更 1 を承認（同じ実行で機械が期待値を全部測った見た目の行は Tier 1 として [x] 可）。kura の lanes.md・windows-role.md への反映は ito の docs/visual-test.md（版 1）の push のあと。Tier 2（AI 判定）は影運用で、合格にしない。
- 判断2（観測の契約）: 状態 JSON v1 と意味のある Test ID。失敗は閉じる方向に倒す。
- 判断3（安全）: フェーズ 3・4 は今のユーザーで §9 のガードレールつき。フェーズ 5 の前に専用の標準ユーザーか Windows Sandbox。
- 判断4（範囲）: §11 の 3 シナリオ + 見た目のパイロット 4 行（59.5・41.15・49.4・54.18）。
- 判断5（予算と保存）: §6 の上限（問題ごとに 30 分・修正 2 回・Vision は 1 行 1 回・1 回の実行で 20 行）。証拠は PASS 14 日・FAIL/INCONCLUSIVE 90 日・PR か [x] が引く回は次の再テストまで・アプリごとに約 5 GB。
- 見た目のテスト: 印は案 a（[x] のまま証拠に tier）。check spec は tests/visual/<行>.toml で、行を書いた開発のセッションが書く。今の [~] の行を遡って [x] にしない。kura のあとは yagura・koyomi・michi・fude（順はパイロットのあとで決める。tsumugi・minato は 2026-11-08 にメンテナンス対象から外した。代わりに Orca と Linear）。
- 判断 3〜5 と見た目のテストの答えは 2026-11-08 の持ち主の「推奨で進めてください」。
- フェーズ 2 は持ち主の「次を進めて」で着手。Codex は設計に積極的に使ってよい（枠は十分ある）。

【済み】
- フェーズ 1（--keys の fail-closed）: v0.151.0（d0f778b）、再テスト uchmk/kura#507。v0.151.1 で ito v0.262.7（xrun.sh）を取り込み、v0.151.2 で CI の smoke を直した（CI 緑）。
- v0.151.3（80ef476）: ワーカーと Codex を Agent ツールの subagent として起動することを SKILL.md に書いた。
- v0.151.4: 詳細ビュー（m a）の不具合 3 件（落ちる・ちらつき・移ると使用量が消える）。再テストは 69.10・69.15〜69.19。
- フェーズ 2 の設計: Codex と 6 ラウンドで合意（重大・高なし）。ito の部品は uchmk/ito#203 で頼んだ（閉じた。v0.263.0〜v0.267.0）。
- kura の stage 1（設定の書き込みの番号と待ち）: v0.152.0〜v0.152.2、再テスト uchmk/kura#511。
- ito#202（xrun.sh）は閉じた。
- 2 つの設計を全アプリ向けにした（ito と各アプリの分担、§1a の穴、視覚テストの規則の雛形）。Codex の全アプリ向けのレビュー 1 回。
- ito#203 の段 ①（観測の世代と Runner の待ち）: ito v0.263.0（16dbe90）。
- 実行の状態機械と適合テストを別の Issue uchmk/ito#204 にした（③ のあと）。2 つの設計を ito の docs/autotest.md・docs/visual-test.md に移すよう、ito のセッション（ito-d4）に頼んだ。

【次にやること】
1. ito のセッションの push を待つ: ito#204 の残り（5・7）と ito#205（状態 JSON v1 は A として v0.275.0 で入った。Snap・Shot の付属 JSON・trace.jsonl は B として v0.276.0 で入り、残りは C（検査器・証拠・スキーマ）。ito#203 は閉じ、ito#204 の 1〜4・6・8 は入った）、2 つの設計の docs への移し（移したあとは ito の docs/autotest.md・docs/visual-test.md が正で、このページは写し）。ito のリポジトリは直接いじらない（メッセージか Issue で頼む）。
2. ito の docs/visual-test.md（版 1）が push されたら、kura に変更 1 を入れる（視覚テストのページの #rule: docs/claude/lanes.md の 105 行の下に 1 項目と 113〜114 行の置き換え、.claude/windows-role.md 147 行、docs/claude/wintest-measure.md に矩形と画素の測り方、.claude/agents/wintest.md に機械の測定の証拠）→ PATCH の版・CHANGELOG・verify・scripts/push-main.sh。変更 2 の文は影運用の基準を満たしてから。
3. kura の残りの分（ID の手順・run の行・確かめ役は v0.153.0〜v0.154.0 で入った。ハーネス scripts/check-term-settings.sh と壊し方の自己テストは作業中で、まず State に `term args`・`term env names`・`terminal` の行）を Sonnet の subagent に仕様ファイルで渡す → audit → verify → main がコミット・push。
4. ito#204（状態機械と適合テスト）は 1〜3 が v0.269.0・v0.271.0・v0.272.0、4・6・8 が v0.273.0（Shot は `shots/` に書き、`theme`・`lang` は効いていた値で頼んだ値は `*_requested`、`run.json` を Runner が書く）で入り、残りは 5・7。kura は自前の実行（src/script_run.rs）を ito の表に通し、形が決まったら寄せる。状態 JSON v1・矩形つきの Tree・Snap は ito#205 の A・B（v0.275.0・v0.276.0）で入り、検査器・証拠のフォルダ・スキーマは C。見た目のパイロットの前に要る。
5. kura のパイロットが通ったら、yagura・koyomi・michi・fude のアダプター（lanes.conf の evidence= と visual_specs=、状態の中身、Test ID、ハーネス）と規則のファイル（成果物 5 の雛形）を、それぞれのセッションに頼む（順はパイロットのあとで決める）。これから作るアプリも同じ形で載せる。tsumugi・minato には頼まない（メンテナンス対象から外した）。
6. Codex は haiku の包みの subagent で回す。UAC を避けるフラグ（-c mcp_servers.node_repl.enabled=false --disable plugins -c 'notify=[]'）は、効くと示せていないので、持ち主が決めるまで既定にしない。

【進め方】
- 実装は自分で書かず Sonnet の subagent（general-purpose、model: sonnet）に任せる。Codex は haiku の subagent の中で codex exec -s read-only を 1 回。コミットと push は main だけ。
- 起動の前に、生きているワーカーを確かめる。subagent の権限の確認は持ち主に出る。代わりに承諾しない。
- 戻ったら git status と差分を確かめる → audit（差分をラベル付きで渡す。指摘は読んで確かめてから反映）→ verify（終了コード 0 かつ最終行 ALL OK）→ main がコミット・push（版の繰り上げ + CHANGELOG + Cargo.lock はセット、自分のパスだけ git add、scripts/push-main.sh）。ito の rev を上げるコミットは push-main.sh が付け直さない。
- 動きが変わるので TESTING.md の行と retest の issue（立てるかは main が判断）。
- 大きな記録ファイル（TODO-DONE・TESTING-CHECKS・TESTING-LINUX・CHANGELOG・TESTING）は全部を読まない。
- Git Bash の日付は TZ=JST-9 date +%F（この PC では TZ=Asia/Tokyo が UTC の日付を返す）。

【やらないこと】
- 権限の境界（親の Claude の設定、ワーカーの allow/deny/ask）を持ち主の確認なしに変えない。承認されていない操作を自動で承諾しない。権限で止められた操作を、分けたり言い換えたりして通さない。
```
