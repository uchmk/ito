# 見た目の行を、証拠で合格にする

> **version 1**（2026-11-08）。2026-11-08 に設計のページ（claude.ai の artifact）から移した。ここが正本。artifact は写し。
> アプリの規則ファイルは「ito docs/visual-test.md, version 1」のように版で名指しする。判定の規則（Tier・自動合格の条件・判定不能の条件）を変えるときは版を上げる。

uchmk · ito · 視覚テスト · 版 1 ・ 変更 1（Tier 1）は承認済み（2026-11-08）、変更 2（Tier 2）は未承認 · 2026-11-08

AI Vision を「合格を決める目」にしない。事前に決めた観点を確かめる検査員として使い、機械の測定と同じ実行の証拠がそろったときだけ合格の候補にする。 **uchmk の全部のアプリ（kura・yagura・koyomi・michi・fude と、これから作るもの）で使う形**にする。tsumugi・minato は 2026-11-08 にメンテナンス対象から外した（代わりに Orca と Linear を使う）ので、この設計の対象にしない。契約・部品・規則の文は ito が持ち、アプリは自分の状態と Test ID と観点の表を出すだけ。 土台は [自動実機テスト基盤の設計（観測の契約）](autotest.md)で、その上に載る。

> **この文書は調査と設計だけ**。どのアプリの規則のファイル（kura `docs/claude/lanes.md`、yagura `.claude/windows-role.md`、michi `CLAUDE.md`）も、`TESTING*.md` も、コードも変えていない。 規則の変更は [成果物 5 の案](#rule)。**2026-11-08 に持ち主が変更 1（機械が測った見た目の行）を承認した**（[持ち主の決定](#ask)）。効くのは、ito の `docs/visual-test.md`（版 1）が push され、そのアプリの規則のファイルに載せてから。**載せるまで、見た目だけの行（kura のいまの `[~]` の 15 行を含む）は、どのアプリでも `[x]` にしない**。変更 2（AI 判定）は影運用の基準を満たすまで効かない。

<a id="sum"></a>

### 要点

1. **全アプリ共通**。state JSON v1・`<Snap:>`・check spec・証拠の形・判定の規則は ito に 1 つだけ置き、各アプリは「アダプター」（状態の出し方・Test ID・`tests/visual/`・`lanes.conf` の 2 行・規則のファイルの参照）を足す。最初に載せるのは kura。
2. **kura のいまの `[~]` の 15 行（11-07 の 17 行から、56.6・58.4 が v0.152.1 で `[ ]` に戻った）の多くは「見た目」の行ではなく、測る手段が無かった行**。状態（state JSON）と UI 要素の矩形があれば、14 行ほどは期待値を機械で測れる見込み。最初に要るのは Vision ではなく観測の契約（フェーズ 2）。
3. 3 つの段に分ける: **Tier 1 自動合格** 機械の測定だけ／**Tier 2 AI 判定合格** 機械の前提＋Vision の場所つきの答え＋突き合わせ／**Tier 3 人間承認** 好み・美しさ・基準画像そのもの。
4. **Vision は合格を止められるが、ひとりでは通せない**。Vision の「問題なし」もモデル同士の一致も、合格の根拠に数えない。通るには機械の前提がすべて要る。
5. 規則の変更は 2 段: 変更 1（機械の測定で見た目の行を `[x]`）と、変更 2（AI 判定合格。Vision を走らせても `[~]` にとどめる影運用で、誤合格 0 を確かめてから）。
6. パイロット（kura）は道具ごとに 1 行: 59.5（状態）→ 41.15（矩形）→ 49.4（画素）→ 54.18（テーマ。Tier 2 の影運用の題材）。

1. [全アプリ共通の形（ito とアプリの分担）](#common)・[共通にするときの穴](#gaps)
2. [3 つの段と A/B/C の分類](#tiers)
3. [流れと判定の表](#flow)
4. [成果物 1 — Vision で自動化できる範囲](#d1)
5. [成果物 2 — ito keys と state JSON v1](#d2)
6. [成果物 3 — 画像比較・UI 要素・Vision の分担](#d3)
7. [成果物 4 — 自動合格の条件と判定不能](#d4)
8. [成果物 6 — 主観を減らす受け入れ基準](#d6)
9. [成果物 7 — 証拠の保存形式](#d7)
10. [成果物 8 — 導入の順とパイロット](#d8)
11. [成果物 9 — 人の承認が要る例外](#d9)
12. [成果物 10 — 時間・費用・誤判定](#d10)
13. [成果物 5 — 規則の変更案（変更 1 は承認・未適用。kura の例とほかのアプリの雛形）](#rule)
14. [持ち主の決定（2026-11-08）](#ask)

<a id="base"></a>

## 前提: いまの規則と道具（2026-11-08）

- **規則は kura・yagura・michi で同じ考え。**`[x]` は「実機で測って確かめた」で、付けてよいのは実機のセッションの main モデルと持ち主だけ。見た目の行は `[~]`（Agent が撮った画像で判断した。済みに数えない。持ち主が同じ画像を見て `[x]` にする）。 書いてある場所はアプリごとに違う: kura `docs/claude/lanes.md` 100 行〜（「`[x]` の規則（正）」）、yagura `.claude/windows-role.md` 44・53 行（“Never tick a look.”）、michi `CLAUDE.md` 63 行（「見た目の行は `[~]`（画面の画像で判断した）まで」）。koyomi と fude には、`[~]`・`[x]` の規則の文がまだ無い。
- **いまの規則にある食い違い（kura で見つけた。ほかのアプリの文も同じ形）**。kura `lanes.md` 114 行は「見た目の行には絶対に付けない」、`windows-role.md` 157 行は「位置や色はピクセルで測れるので測る」。 どこまでが「測った」かの線が無い。変更 1 はこの線を引くだけの変更にする。
- **ito keys**。ito の `ito-keyscript` には `<Tree:name>`（AccessKit の木。役割・名前・値・focused・checked と `announced:`）、`<Assert:>`、`<Is:>`、`<ClickText:>` がある。 yagura・koyomi・michi・fude は ito の `Runner` で実行するのでそのまま使える。**kura は自前の実行（`src/script_run.rs`）で、v0.154.0 からこれらとクリップボードの手順を自分で実行する**（v0.151.0〜v0.153 は窓を開く前に拒み、それより前は黙って飛ばしていた）。`tree_text` は矩形を出さない。
- **`<State:name>` と `<Shot:name>` は kura と Runner のアプリ（yagura・koyomi・michi・fude）にある**が、中身はアプリごとの自由なテキスト。kura はそこに `settled: yes|no` を足した（v0.151.0）。`<Shot:>` は PNG だけで、いつの状態の画像かを示すものが無い。
- **runner**。ito のレーンのスクリプト `keys.ps1 -Isolate -Out`（Windows）と `xrun.sh`（Linux）を各アプリが写して使う（ito `scripts/lanes/FILES`、値は各アプリの `scripts/lanes.conf`）。 使い回した `-Out` に前の回の成果物が残る問題は ito v0.262.3 で直った（kura は v0.150.4 で写した）。`xrun.sh` が `keys: done` で終わらない run を成功にしていた問題は ito v0.262.7 で直った（ito#202、閉じた）。

<a id="common"></a>

## 全アプリ共通の形（ito とアプリの分担）

uchmk の「ito first」に合わせ、**2 つ以上のアプリで要るものは全部 ito に置く**。アプリは「アダプター」だけを持つ。視覚テストの契約は ito が持ち、どのアプリで測っても同じ形の証拠になり、同じ規則の文で判定する。

| もの | ito が持つ | 各アプリが出す（アダプター） |
|---|---|---|
| 契約の文書 | 新しい `docs/visual-test.md`（いまの `docs/visual-review.md` の続き。sidecar・`review::matrix`・`crop`・`compare` はそちらのものを使い、書き直さない） | — |
| state JSON v1 | スキーマと書き出しの関数。共通の欄（`theme`・`window`・`ui[]`・`toasts`・`capabilities`）を ito の部品から埋める補助 | `screen`・`overlay` の名前と、アプリの名前と版（`app`）、アプリだけの値を `app_state` の中に（kura なら cwd・カーソル・spot） |
| Test ID と矩形 | `ito-a11y` の `test_id(resp, name)`（AccessKit の author_id＋矩形・層・enabled の登録）。ito の部品（設定の画面・パレット・トースト）の ID は ito が付ける（`settings.field.<id>` など） | 自分の画面の部品の ID（名前は `<画面>.<部品>`） |
| 手順 | `ito-keyscript`: `<Snap:>`、ID の手順（`<ClickId:>`・`<Id:>` など）、観測の世代、run の行 | Runner のアプリは何もしない。**kura は自前の実行に同じものを写す** |
| check spec と検査器 | TOML のスキーマ、機械の検査のスクリプト、カナリアの作り方（ito `scripts/lanes/` に置き、各アプリに写す） | `tests/visual/<行>.toml` と fixtures、承認済みの基準画像 |
| 証拠と判定 | 証拠のフォルダーの形、`manifest.json`・`verdict`・`vision.v1` のスキーマ | 置き場所だけ（`lanes.conf` の `evidence=`） |
| Vision の subagent | 定義・問いの雛形・盲検の手順（全アプリで同じ） | 観点の文は check spec に書く |
| 規則の文 | 段の定義・自動合格の条件・Vision の扱いを `docs/visual-test.md` に 1 つ | 自分の規則のファイルから参照し、アプリの差（レーンの名前・パス）だけ書く |

### アプリごとの今

| アプリ | `--keys` の実行 | 規則のファイル | `[~]` の行 | 載せる順 |
|---|---|---|---|---|
| **kura** | 自前（`src/script_run.rs`）。ito-keyscript は解析だけ | `docs/claude/lanes.md` | 15 | 1 番目。パイロット 4 行 |
| yagura | ito の `Runner`（`src/main.rs`） | `.claude/windows-role.md` | 15 | kura のあと（4 つの順はパイロットのあとで決める） |
| koyomi | ito の `Runner`（`crates/koyomi/src/main.rs`） | まだ無い（載せるときに `CLAUDE.md` に足す） | 0 | kura のあと |
| michi | ito の `Runner`（`crates/michi/src/main.rs`） | `CLAUDE.md` 63 行 | 337 | kura のあと |
| fude | ito の `Runner`（`crates/fude/src/main.rs`） | まだ無い（載せるときに `CLAUDE.md` に足す） | 5 | kura のあと |

kura のあとの 4 つは、準備のほかに違いが無い。順は kura のパイロットのあとで決める。tsumugi・minato は 2026-11-08 にメンテナンス対象から外した（代わりに Orca と Linear を使う）。この表と載せる順から外した。これから作るアプリも、同じ形で足す。

`[~]` の数は 2026-11-08 の各アプリの `TESTING-CHECKS.md` の `- [~]` で始まる行。見立ての表は kura の分だけ作った。

> **実行の分かれ目に注意**。kura の段 ①（v0.152.0〜v0.152.2: 設定の書き込みの番号と待ち、注入は ROOT だけ、配達の待ち、`<Now>` は次の 1 手だけ）はいま **kura の自前の実行にだけ**ある。 ROOT の確かめと配達の待ちは ito v0.263.0（[ito#203](https://github.com/uchmk/ito/issues/203) の段 ①）で `Runner` にも入った。残り（設定の書き込みの待ち、`<Now>` の意味、終わりの状態）は [ito#204](https://github.com/uchmk/ito/issues/204) で共通の状態機械と適合テストにする。そろうまでは、同じスクリプトがアプリによって違う意味になり、証拠が比べられない。 ito に入れたものは、Runner のアプリ（いまは yagura・koyomi・michi・fude）にそのまま届く。kura は自前の実行に写すか、いずれ Runner に移る（移るかは別に決める）。

### 載せる順

1. ito: `docs/visual-test.md`（契約と規則の文）と、state JSON v1・check spec・証拠のスキーマ。[ito#203](https://github.com/uchmk/ito/issues/203)（閉じた）、[ito#204](https://github.com/uchmk/ito/issues/204)（実行の状態機械と適合テスト）、[ito#205](https://github.com/uchmk/ito/issues/205)（状態 JSON v1・`<Snap:>`・矩形つきの Tree。パイロットの前）。
2. kura: 状態の書き出しを state JSON v1 にし、パイロットの 4 行（[成果物 8](#d8)）。ここで形を固める。
3. ほかのアプリ: アダプター（`app_state` の中身・Test ID・`tests/visual/`・`lanes.conf` の 2 行）を足すだけで使える。規則のファイルへの参照は、変更 1 が承認済み（2026-11-08）なので、ito の `docs/visual-test.md`（版 1）の push のあと、各アプリに 1 項目ずつ。

<a id="gaps"></a>

### 共通にするときの穴と直し方（Codex の設計レビュー、2026-11-08）

共通の形を Codex に読み取り専用でレビューさせた。引用された行は全部コードで読んで確かめた（下の「確かめた行」）。直し方は上の表に足す決まりとして扱う。

| 重さ | 穴 | 直し方 |
|---|---|---|
| **重大** | **Runner と kura で実行の保証が違う**。ito の `Runner` は決まった間隔で進み、`<Now>` は飛ばす待ちが無い（`ito-keyscript/src/lib.rs` 1562 行「This runner has no settling to skip」。ROOT の確かめと配達の待ちは v0.263.0 で入った。残りは [ito#204](https://github.com/uchmk/ito/issues/204)）。kura は注入を ROOT だけにし、設定の書き込みの失敗と待ち、配達の待ち、1 手だけの `<Now>` を持つ（v0.152.0〜v0.152.2）。 | 実行の状態機械と**適合テスト**を ito に 1 つ置き、Runner も kura もそれを通す。アダプターの API に `settled`、保存の pending・番号・ack・error、ROOT の描画の世代を足す。`<Now>` が飛ばすのは 1 手の落ち着き待ちだけで、保存と配達の待ちは飛ばさない。ID の手順・観測の世代・`<ReadHas:>`・`<Snap:>`・どの終わり方でも書く run の行は、両方の実行に入れる。 |
| **重大** | **JSON の名前がぶつかり、証拠が混ざる**（**v0.273.0 で解消**: Runner は `shots/<name>.png`・`shots/<name>.json` に書き（`shots` がファイルなら失敗）、`keys.ps1`・`xrun.sh` は `shots/`・`state/`・`run.json.part` も空にする（リンクや非フォルダーなら止まる）。以下は当時の指摘。）。Runner の `<Shot:name>` はもう `name.json` を付属の JSON として書く（`lib.rs` 1724 行）。`<State:name>` も `name.json` にすると上書きになる。レーンの片付けは一番上の段だけ（`keys.ps1` 123 行、`xrun.sh` 57 行の `-maxdepth 1`）で、`state/` などの下は残る。 | 名前を固定する: `state/<name>-before.json`・`state/<name>-after.json`・`shots/<name>.png`・`shots/<name>.json`。**run ごとに新しいフォルダー**（使い回さない）。`<Snap:>` の前後でどこまで状態が変わってよいか、`frame` が何を数えるか、どのバイトのハッシュかを契約に書く。撮影や書き込みの失敗は PASS にしない（いまは `lib.rs` 1713 行が文字列を返すだけ）。 |
| **高** | **ito の補助だけでは state JSON v1 が埋まらない**。Runner の `<Theme:>` は頼んだ時点でテーマの名前を書き換える（`lib.rs` 1579〜1582 行）ので、頼んだテーマと効いたテーマが分からない。macOS・Linux の窓の補助は画面の大きさを 1920×1080 と推測する（`ito-common/src/window.rs` の `PRIMARY_GUESS`）。`ito-notify` のトーストは OS の通知で、アプリの中のトースト（kura `app.rs` 240 行）とは別もの。 | 補助は「取る・変える・確かめる」だけ。`screen`・`overlay`・`settled` の意味・**効いている**テーマ・画面に出ているトースト・アプリの値はアプリが出す。`visible`・`elided` は描く側で登録する。**分からない値は推測せず `null` にし、`capabilities` から外す**（推測した画面の大きさを `monitor_work_area_px` に入れない）。 |
| **高** | **`lanes.conf` の 2 行だけでは証拠が保証されない。**`keys.ps1` は `-Out` が無ければ一時フォルダーに書き（113 行）、アプリに渡すのは done のファイルの場所だけ（127 行）。 | `evidence=` は証拠の**親フォルダー**（OS ごとに解決する。既定 `C:\dev\<app>-evidence`、kura はいまの `C:\dev\filer-evidence` のまま）、`visual_specs=` はリポジトリからの相対パス。run_id は起動側（`keys.ps1`・`xrun.sh`）が作って `<APP>_KEYS_RUN_ID` で渡し、終わったら done・manifest・証拠の run_id がそろうか確かめる。run の行は最後の状態の行の直前に書き、`keys: done` を最後の行のままにする。 |
| **高** | **共通の規則の文を「参照するだけ」だと、今の規則と矛盾する**。kura `lanes.md` 105 行と yagura `windows-role.md` 56 行は「`[~]` を `[x]` にするのは持ち主だけ」、michi `CLAUDE.md` 63 行は「見た目の行は `[~]`（画面の画像で判断した）まで」と言い切っている。 | 載せるアプリは、いまの文を**置き換え**、共通の規則の版・どちらが優先か・対象の行・使う段・誰が付けるかを書く。影運用のあいだは昇格しない。**まだ載せていないアプリは今の規則のまま**（[成果物 5](#rule) の雛形）。 |

分担の線（契約・検査器・証拠の書き出し・判定の処理・設定の画面など共通の部品の ID は ito、アプリの ID・シナリオ・期待値はアプリ）は Codex も同じ見方。 確かめた行: ito `ito-keyscript/src/lib.rs` 1562・1579〜1582・1713・1724（ito v0.263.0、16dbe90 で読み直した）、`ito-common/src/window.rs` `PRIMARY_GUESS`、`ito-notify/src/toast.rs`、`scripts/lanes/keys.ps1` 113・123・127、`xrun.sh` 57、kura `app.rs` 240・`lanes.md` 105、yagura `windows-role.md` 56、michi `CLAUDE.md` 63（2026-11-08 に足した）。kura の `script_run.rs` の行番号は読んでいない（中身は段 ① の実装で知っている）。

<a id="tiers"></a>

## 3 つの段と A/B/C の分類

### **Tier 1** 自動合格

- **根拠**: 数値・状態・画像の比較。**機械の測定だけ**。
- **Vision**: 要らない。走らせたときは止める側にだけ使う。
- **付ける人**: 実機のセッションの main モデル（証拠を読んでから）。
- **規則**: いまの規則の延長。変更 1 で線を引く。

### **Tier 2** AI 判定合格

- **根拠**: 事前に書いた客観的な観点＋機械の前提＋Vision の**場所つき**の答え＋機械との突き合わせ＋カナリア（既知の不良）の検出。
- **付ける人**: 実機のセッションの main モデル。
- **規則**: 変更 2。影運用で誤合格 0 を確かめるまでは `[~]`。

### **Tier 3** 人間承認

- **根拠**: 好み・美しさ・情報の階層・基準画像そのもの。
- **AI**: 観点ごとの報告と、画像のどこを見たかだけ。
- **付ける人**: 持ち主（いまの `[~]` → `[x]` のまま）。要るときだけ。

| 分類 | 例 | 主な道具 | 段 |
|---|---|---|---|
| **A 客観** | 文字の欠け、重なり、画面外のダイアログ、テーマの適用、崩れたレイアウト、仕様との不一致 | state の値、UI 要素の矩形、画素 | **1** Vision は見落とし探しで、止める側だけ |
| **B 条件つき** | 基準画像との一致、余白と整列、デザインシステムへの準拠、状態ごとの画面 | 基準画像との差、矩形の数値、トークンの色 | 条件がそろえば **1**。欠ければ観点が書けるとき **2**、書けなければ **3** |
| **C 主観** | 静かさ（Quiet Precision）、階層、バランス、好み | なし（AI の報告） | **3** 持ち主が要るときだけ承認 |

B が Tier 1 になる条件: 基準画像と同じ OS・アーキテクチャ・GPU のバックエンド・フォント・テーマ・倍率・窓の大きさ／動く部分（時刻・カーソル・スピナー）を覆う／閾値を実行の前に check spec に書く／基準画像を持ち主が承認している／色は ito のトークン表と比べる。

<a id="flow"></a>

## 流れと判定の表

1. **操作**。runner（`wintest` か実機のレーン）が `keys.ps1 -Isolate` でシナリオを押す。 **止める条件:** `keys.done` の最後が `keys: done` でない、終了コードが 0 でない、State に `settled: no` がある（フェーズ 1 の fail closed）。
2. **撮る**。行の観点の時点で State → Shot → State。前後の state のハッシュ（frame の番号を除く）が同じときだけ「その状態の画像」とする。案: `<Snap:name>` 1 つでこれをする。 **止める条件:** 前後の state が違う、run_id か snapshot_seq が合わない、PNG が無い。
3. **UI 要素**。state の `ui[]`（test_id・役割・文字・矩形・クリップ・`elided`・visible）。`<Tree:>` にも矩形を足す。 **止める条件:** 観点の対象の test_id が無い。
4. **機械の検査**。状態の値、矩形の関係（はみ出し・重なり・画面外・整列・余白）、画素（トークンの色・基準画像との差・帯の走査）。観点ごとに `PASS` / `FAIL` / `ERROR`。 **止める条件:** `FAIL` は所見。Vision の答えで覆さない。`ERROR` は判定不能。
5. **Vision（盲検）**。観点の問いと画像（全体、矩形から切った原寸の切り抜き、カナリア）だけを渡し、機械の結果は見せない。観点ごとに `PASS` / `FAIL` / `INCONCLUSIVE`、見た場所（bbox）、見たもの。観点の外で気づいたものは `unexpected`。 **止める条件:** 観点に答えが無い、場所が無い、カナリアを見逃した。
6. **突き合わせ（スクリプト）**。Vision の bbox が対象の矩形に入るか、読んだ文字が state と合うか、カナリアを `FAIL` にしたか。 **止める条件:** 合わない所が 1 つでもある。
7. **決める**。実機のセッションの main モデルが manifest と条件の表を自分で読み、段の条件を満たせば印を付ける。満たさなければ `[~]` と報告、または所見の issue。runner と subagent は印を付けない。

| 機械 | Vision | 結果 |
|---|---|---|
| PASS | PASS（全観点に場所つき） | Tier 1 の行は**自動合格の候補**（Vision は無くてよい）。Tier 2 の行は、突き合わせとカナリアも通れば **AI 判定合格の候補**。 |
| PASS | FAIL | **判定不能**。main が画像を見て、所見か持ち主へ。機械の検査に穴がある疑いとして記録する。 |
| PASS | INCONCLUSIVE | Tier 1 の行は機械の結果のまま（Vision は参考）。Tier 2 の行は**判定不能** → 持ち主。 |
| FAIL | どれでも | **不合格**（所見の issue）。Vision の PASS で覆さない。 |
| ERROR・証拠の欠け | どれでも | **判定不能**。撮り直す。 |

<a id="d1"></a>

## 成果物 1 — AI Vision で自動化できる範囲

| 任せられる（観点つきで） | 条件つき | 任せない |
|---|---|---|
| <br>・決めた物の有無（確認ダイアログが窓の中にある、豆腐 □ が出ていない、画像が出て文字の帯ではない）<br>・画面の文字を読む（state の文字との突き合わせ用）<br>・仕様の文と画面の意味の比較（ボタンの文言が操作を表す）<br>・test_id の無い部品の異常を見つける（未登録のはみ出し、描画の崩れ）<br>・所見の説明の下書き | <br>・色の違い（大きな差だけ。値は画素で測る）<br>・整列・余白（大きなずれだけ。数 px は矩形で）<br>・文字の欠け（明らかなものだけ。`elided` で機械が見る）<br>・状態ごとの画面の見比べ（抜き取り） | <br>・px 単位の位置・大きさ、色の値、コントラスト比<br>・時間（「約 1.5 秒後」）とフレームの一致<br>・画面に無いもの（ファイル・レジストリ・クリップボード）<br>・好み・美しさの最終判断<br>・基準画像の承認 |

Vision の役は 3 つに絞る: (1) Tier 2 の行で、機械が測れない残りの観点に場所つきで答える。(2) 機械が登録していない異常を見つける（止める側だけ）。(3) 所見と報告の文を下書きする。

<a id="d2"></a>

## 成果物 2 — ito keys と state JSON v1 の使い方

### 足すもの（案。どれもフェーズ 2 以降。自動テストの設計の判断 3〜5 は 2026-11-08 に決まった）

| もの | どこ | 中身 |
|---|---|---|
| state JSON v1 | スキーマと書き出しは ito、中身は各アプリ（kura が最初） | いまのテキストの State に並べて JSON を書く。`settled`・`theme`・`window`・`ui[]`・`toasts[]`・`capabilities`、アプリだけの値は `app_state` の中。分からない値は `null`（[穴の表](#gaps)）。 |
| Test ID | `ito-a11y` の `test_id`。名前は各アプリ（ito の部品の分は ito） | egui の id と別の、変わらない名前（kura なら `spot.scroll.thumb`、ito の設定の画面なら `settings.field.<id>`）。観点はこれで対象を指す。 |
| `<Snap:name>`、矩形つきの `<Tree:>` | ito-keyscript（ito#205） | State → Shot → State を 1 つの手順に。木の行に矩形とクリップを足す。Runner の 4 つのアプリ（yagura・koyomi・michi・fude）はそのまま使え、kura は自前の実行に写す。 |
| Shot の付属 JSON | ito（いまの `review::Meta` を広げる） | PNG ごとに run_id・snapshot_seq・前後の state のハッシュ・環境。名前は `shots/<name>.json`（State と分ける。v0.273.0 で実装。`theme`・`lang` は撮ると頼んだ時点で効いていたもの、`theme_requested`・`lang_requested` は `<Theme:>`・`<Lang:>` が頼んだもの。run_id・snapshot_seq・ハッシュは ito#205）。 |
| kura の `<Tree:>`・`<Assert:>`・`<Is:>` | kura だけ | **済み（kura v0.154.0）。** ito の `tree_text` などを呼んで自前の実行で読む。Runner のアプリはもう使える。 |

**座標の約束:** 矩形はスクリーンショットの画素（`rect_px`）で出し、`pixels_per_point` を並べて書く。画像と state の座標をそろえないと、切り抜きも突き合わせもできない。

### state JSON v1（視覚の観点に要る部分の例）

```json
{
  "schema_version": 1,
  "run_id": "20261107-101203-41.15",
  "snapshot_seq": 2, "after_step": 9, "frame": 412,
  "settled": true,
  "app": { "name": "kura", "version": "0.152.2" },
  "screen": "main", "overlay": "spot",
  "theme": { "requested": "Nord", "name": "Nord", "dark": true, "tokens_sha256": "…" },
  "window": { "inner_px": [0, 0, 1350, 630], "pixels_per_point": 1.5,
              "monitor_work_area_px": [0, 0, 2560, 1400] },
  "ui": [
    { "test_id": "spot.panel", "role": "group",
      "rect_px": [768, 60, 1332, 606], "clip_px": [768, 60, 1332, 606], "visible": true },
    { "test_id": "spot.scroll.thumb", "role": "scrollbar",
      "rect_px": [1323, 60, 1329, 177], "visible": true },
    { "test_id": "spot.row.0", "role": "text", "text": "Title",
      "rect_px": [780, 66, 1010, 88], "elided": false }
  ],
  "toasts": [],
  "capabilities": ["state.v1", "ui.rects", "snap", "monitor.work_area"],
  "app_state": { "cwd": "C:\\fixtures\\docs", "cursor": "report.docx" }
}
```

`app` はアプリの名前と版、`app_state` はそのアプリだけの値（ここは kura。ほかのアプリはそれぞれの値を入れ、ito の検査器は中を見ない）。 `theme.requested` は頼んだテーマ、`name` は効いているテーマ。macOS・Linux で作業領域が取れないときは `monitor_work_area_px: null` にし、`capabilities` から `monitor.work_area` を外す。

`elided` は egui の `Galley` が文字を省略したかどうか。これがあれば「文字の欠け」は画像を見ずに分かる。省略されずに枠からはみ出した文字は、文字の矩形 ⊄ クリップで分かる。

### Shot の付属 JSON

```json
{
  "run_id": "20261107-101203-41.15", "snapshot_seq": 2,
  "png": "shots/02-top.png", "png_sha256": "…", "size_px": [1350, 630],
  "state_before_sha256": "…", "state_after_sha256": "…",
  "requested_frame": 412, "captured_frame": 413,
  "env": { "os": "Windows 11 26300", "arch": "x86_64", "wgpu_backend": "Dx12",
           "font_set_sha256": "…", "theme": "Nord", "pixels_per_point": 1.5 }
}
```

### シナリオの例（41.15）

```text
# いまの手順で書けるもの（Snap が無い間）
<Size:900x420> …（対象の .docx へ）… <Tab>
<State:top><Shot:top><State:top-after>
<A-j><A-j>…
<State:bottom><Shot:bottom><State:bottom-after><Quit>

# 案: Snap があれば
… <Tab><Snap:top> <A-j>… <Snap:bottom><Quit>
```

runner はどのアプリも ito から写した同じスクリプト: Windows は `scripts\keys.ps1 -Dir <fixtures> -Keys '…' -Out <新しい run の folder> -Isolate`、Linux は `scripts/xrun.sh`。 結果として使えるのは、終了 0・最後の行が `keys: done`・全 State が落ち着いている回だけ（kura は v0.151.0 から `keys: failed` と `keys: unsettled` を結果にしない。Runner のアプリにも同じ扱いを入れる）。

<a id="d3"></a>

## 成果物 3 — 画像比較・UI 要素・Vision の分担

| 観点 | state・UI 要素 | 画素・基準画像 | Vision | 分類 |
|---|---|---|---|---|
| 文字の欠け | `elided`、文字の矩形 ⊄ クリップ | — | 未登録の文字の切れを探す | A |
| 重なり | 兄弟の矩形の交差（許す重なりは check spec に書く） | — | 重なって読めない所を探す | A |
| 画面外のダイアログ | 矩形 ⊄ 窓の内側・作業領域 | — | — | A |
| テーマの適用 | `theme` の名前と明暗 | 決めた矩形の画素 = トークンの色（ΔE ≤ 閾値） | 取り残された部品（検索欄・スクロールバー）を探す | A |
| 崩れたレイアウト | 不変条件（大きさ > 0、親の中、行の高さ） | 基準画像との差 | 何が崩れたかの説明 | A・B |
| 仕様との不一致 | 状態の値・文字 | — | 仕様の文と画面の意味（観点つき） | A |
| 基準画像との一致 | — | 覆いつきの差、事前の閾値 | 差が出た所の説明（判定はしない） | B |
| 余白・整列 | 矩形の差 = トークンの余白 × 倍率 ± 1 px、左端の一致 | — | — | B |
| デザインシステムへの準拠 | — | 画素の色の集合 ⊂ トークン（アンチエイリアスを除く）。コードの色の直書きの検査（ito-first と同じ発想） | 「目立ちすぎ」などの報告だけ | B |
| 状態ごとの画面 | 行列のすべてに A の検査 | 基準画像は代表だけ | 抜き取りで見る | B |
| 静かさ・階層・好み | — | — | 観点ごとの報告 | C |

**状態の行列**（uchmk の設計原則の「実画面で検証」に合わせる）: ライト／ダーク × 100 %／150 % × 狭い窓／広い窓 × 長い日本語の名前・空のフォルダ・エラー。1 画面 24 枚になるので、機械の検査（A）は全部に、Vision は抜き取りにだけ使う。

**Vision に渡す画像:** 全体を 1 枚と、観点の対象の矩形から余白をつけて切った**原寸**の切り抜き。縮んだ全体の画像だけでは 1〜2 px の欠けを見落とす。

<a id="d4"></a>

## 成果物 4 — 自動合格の条件と判定不能の条件

### 合格の候補になる条件（すべて。1 つでも欠ければ判定不能）

| # | 条件 | 確かめ方 |
|---|---|---|
| 1 | 対象と期待値が明確 | 行の check spec が実行の前にコミットされている（コミットの時刻 < 実行の開始）。観点ごとに test_id、期待値、許容差、失敗の見え方。 |
| 2 | スクリーンショットがある | PNG があり、sha256 が manifest にあり、大きさ = 窓 × 倍率。 |
| 3 | state JSON がある | `schema_version` が 1、`settled: true`、観点に要る `capabilities` がある。 |
| 4 | 画像と state が同じ実行 | run_id と snapshot_seq が同じ。前後の state のハッシュが同じ。 |
| 5 | 機械が問題を見つけていない | `assertions.json` がすべて PASS。ERROR が無い。 |
| 6 | Vision が指定の観点を確認した | Tier 2: 全観点に答えと bbox がある。Tier 1: 要らない（走らせたなら FAIL が無いこと）。 |
| 7 | 重大な不一致が無い | Vision の bbox が対象の矩形に入る。読んだ文字が state と合う。`unexpected` が空。 |
| 8 | 未確認が残っていない | INCONCLUSIVE・CANNOT-MEASURE・BLOCKED が無い。check spec の観点がすべて測られている。 |
| 9 | 後片付けが PASS | プロセスが終わった、`-Isolate` の folder を消した、機械の設定の前後の差が無い、残った窓が無い。 |
| 10 | 根拠が保存されている | `verdict.json` と manifest があり、全ファイルの sha256 が合う。PR に 1 行。 |
| + | 環境が基準と同じ（画素と基準画像の観点） | manifest の env が基準画像の env と同じ。 |
| + | カナリアを見つけた（Tier 2） | 同じ回に混ぜた既知の不良の画像を、Vision が FAIL にした。 |
| + | 系のダイアログが前に出ていない | `blocked.txt` が無い。 |

### 判定不能（印を付けず、`[~]` の報告か持ち主へ）

- 上の条件のどれかが欠けた。runner が `keys: failed`・`keys: unsettled`・`keys: stalled`・時間切れで終わった。
- 機械が PASS、Vision が FAIL（どちらかが間違っている）。
- Vision が INCONCLUSIVE、または観点の一部にしか答えていない。カナリアを見逃した。
- モデルの間で答えが割れた（独立レビューをした回）。
- 環境が基準画像と違う。動く部分を覆っていない。基準画像がまだ無い。
- 観点に test_id が無い、期待値が数か列挙になっていない、閾値が実行のあとで決まった。

> **合格にしないもの:** Vision の「問題なし」だけ。モデル同士の一致だけ（一致は「割れていない」を示すだけで、証拠の不足を埋めない）。main モデル自身が画像を見た印象だけ（それも目で見ただけに入る）。

<a id="d6"></a>

## 成果物 6 — 主観を減らす受け入れ基準の書き方

TESTING.md の行は人が読む要件のまま残し、測る形は行ごとの check spec に書く（置き場所の案: `tests/visual/<行>.toml`）。check spec は行を書いた開発のセッションが書き、実機のセッションは同じ実行の中で書き換えない。段の格上げと閾値を緩めることは持ち主の承認。

1. **対象を名前で。**「右上に」ではなく、どの test_id の、どの辺から何 px。
2. **期待値を数か列挙で。**「細い」→ 幅 ≤ 6 px。「薄い下地」→ トークン `muted` の色。文字は文字そのもの。
3. **状態を固定する**。テーマ、倍率、窓の大きさ、fixtures。
4. **いつ撮るか**。どのキーのあと、settled で。
5. **失敗の見え方を先に書く**（いまの `[~]` の規則と同じ）。
6. **残る主観は分けて書き、Tier 3 と明記する**（例: 2.2 の「元のファイルの形に見えること」）。

```toml
# tests/visual/41.15.toml（案）
row = "41.15"
tier = 1
fixtures = "spot-long"
window = { size = [900, 420], pixels_per_point = 1.5 }

[[point]]
id = "thumb-top"
snap = "top"
target = "spot.scroll.thumb"
relative_to = "spot.panel"
expect = { right_gap_px = { max = 2 }, width_px = { max = 9 }, top_gap_px = { max = 1 } }
fail_looks_like = "つまみが無い、左に寄る、パネルの外に出る"

[[point]]
id = "thumb-bottom"
snap = "bottom"
target = "spot.scroll.thumb"
relative_to = "spot.panel"
expect = { bottom_gap_px = { max = 1 } }

[[point]]
id = "no-thumb-when-fits"
snap = "short"
target = "spot.scroll.thumb"
expect = { present = false }

# tests/visual/49.4.toml（抜粋）
[[point]]
id = "ok-bar"
snap = "after-dir"
target = "pane.mark[0]"          # dir のプロンプトの印。行の y は state から
expect = { color_token = "ok", delta_e = { max = 3 } }
```

### kura の `[~]` の行の見立て（最初に載せるアプリの例）

見立てで、どの行もまだ測っていない。要旨は TESTING-CHECKS.md の行の前半から。「要る観測」は kura のコードを行ごとには確かめていない（state にもうある値もありうる）。承認まで、どの行も `[~]` のまま。2026-11-07 に 17 行を見立て、そのあと 56.6・58.4 は v0.152.1（`--keys` が書き出し・読み込み・Reset を待つ）で動きが変わり `[ ]` に戻した（retest #511）。いまの `[~]` は 15 行。

| 行 | 期待値（要旨） | 機械で測る | 要る観測（案） | Vision | 人に残る | 段 |
|---|---|---|---|---|---|---|
| 1.46 | `less` の間、ペインの右上に `<C-t> list` が薄い下地・淡い字 | 文字、位置、色（トークン） | 札の test_id と矩形 | — | — | **1** |
| 2.2 | ミニマップの帯が 40 行ごと、字下げは右から、ほかの縞が無い、元の形に見える | 帯の位置と始まり、縞（画素の行の走査） | 帯の矩形の一覧 | 「元の形に見える」の報告 | 形の印象 | **1**＋**3** |
| 18.7 | ドラッグ中、離す前に枠と「copy」（Shift で「move」） | 枠の色、札の文字 | ドラッグの途中で State を取る手段、札の test_id | — | — | **1** |
| 20.16 | ファイルの種類ごとのキーの案内 | 文字 | 案内の test_id（か Tree） | — | — | **1** |
| 23.4b | 約 1.5 秒後に `Listing …`、終われば消え、失敗は赤いトースト | 文字と時刻、トーストの種類 | 時刻つきの State、届かないホストの fixture | — | — | **1** |
| 41.15 | spot の右端に細いつまみ、下へ動く、収まれば無い | 矩形 | つまみの test_id | — | — | **1** |
| 49.4 | 成功は緑、失敗は赤の帯。全画面のプログラムの中には無い | 画素の色（行の y は state から） | プロンプトの印の行とセルの大きさ | — | — | **1** |
| 49.8 | OSC 1337 の画像がペインに出る | 切り抜きと元画像の似かた（数値） | 画像のセルの矩形 | 「画像が出て、文字の帯ではない」 | 閾値が決まるまで | **1**〜**2** |
| 49.12 | 幅を変えたあとも、コピー・プロンプトの位置・緑の帯が正しい | トースト、クリップボード、ペインの文字、帯の色 | 49.4 と同じ | — | — | **1** |
| 54.16 | 150 % ですぐ大きくなる、common.toml、隣のアプリ（yagura）も 3 秒で | 倍率、ファイル | 他のアプリの state | — | 他のアプリが state を出すまで | **1** |
| 54.18 | テーマが同じフレームで変わる、egui の部品も、common.toml のコメントが残る | frame の番号、画素（地・検索欄・スクロールバー）、ファイル | 部品の矩形 | 取り残された部品を探す | 行の残りに主観があればその分 | **1**（**2** の題材） |
| 54.19 | OS のライト／ダークに 1 秒で従う | ファイル、札の文字、画素 | — | — | **Windows の設定を変える操作** | **1**（実行に承認） |
| 56.4 | サインイン時の起動: レジストリの Run と タスク マネージャー | レジストリ、スイッチの状態 | — | — | タスク マネージャーの表示（OS の画面） | **1**＋**3** |
| 56.6<br>いま [ ] | 書き出し・読み込みでテーマが戻り、トーストがそう言う | ファイル、テーマ、トーストの文字 | トーストの文字 | — | — | **1** |
| 57.3 | 環境変数がターミナルに出て、設定に残る | ペインの文字（`<Read:PaneText>`）、ファイル | — | — | — | **1** |
| 58.4<br>いま [ ] | Reset 2 回で `.bak`、既定が効き、トーストが数を言う | ファイル、state、トースト | トーストの文字 | — | — | **1** |
| 59.5 | 箱のクリックと Ctrl+Shift+P でパレットが開く | state の overlay | 箱の test_id（クリック先を名前で） | — | — | **1** |

<a id="d7"></a>

## 成果物 7 — 証拠の保存形式

置き場所は `<evidence>\<run_id>\`。`<evidence>` は各アプリの `scripts/lanes.conf` の `evidence=`（既定 `C:\dev\<app>-evidence`。kura は今の `[~]` の規則と同じ `C:\dev\filer-evidence`）。 1 回の実行が 1 つの**新しい** folder（使い回さない。[穴の表](#gaps)）。形は全アプリで同じで、ito の検査器が読む。下は kura の例。

```text
C:\dev\filer-evidence\20261107-101203-41.15\
  manifest.json         アプリの名前・版・コミット、実行の種類（Runner か自前か）、ito の rev、OS・アーキテクチャ・GPU のバックエンド・倍率・テーマ・フォント、
                        窓の大きさ、シナリオと check spec の sha256、開始と終了、keys.done の最後の行、終了コード、
                        後片付けの結果、全ファイルの sha256
  keys.done
  run.json              Runner が書く実行の記録（v0.273.0。app・ito の rev・status・steps・failed の手順。manifest.json の元になる）
  trace.jsonl           1 手順 1 行: step, label, settled, frame, time
  state\02-top-before.json   state JSON v1（Snap の前。テキストの State も並べて置く）
  state\02-top-after.json    Snap の後
  shots\02-top.png      全体
  shots\02-top.json     Shot の付属 JSON
  crops\41.15-thumb-top.png
  canary\…              Tier 2 だけ。名前は伏せ、対応表は manifest の外（scratch）に
  machine\assertions.json
  vision\claude-<model>-41.15.json   生の応答も残す
  verdict-41.15.json
```

### vision の出力（案）

```json
{
  "schema": "vision.v1", "row": "54.18",
  "model": "<model id>", "prompt_sha256": "…",
  "inputs": [{ "id": "A", "sha256": "…" },
             { "id": "B", "sha256": "…" }],
  "points": [
    { "id": "egui-light", "image": "A",
      "question": "検索欄とスクロールバーも明るい地か",
      "verdict": "PASS",
      "bbox_px": [12, 8, 420, 36],
      "saw": "検索欄の地は周りと同じ明るさ" }
  ],
  "unexpected": [],
  "overall": "PASS",
  "usage": { "input_tokens": 0, "output_tokens": 0 }
}
```

### verdict（案）

```json
{
  "row": "41.15", "tier": 1,
  "result": "auto-pass-candidate",
  "conditions": {
    "expected_defined": true, "screenshot": true,
    "state_json": true, "same_run": true,
    "machine_issues": 0, "vision_points": "n/a",
    "major_mismatch": false, "unconfirmed": 0,
    "cleanup": "PASS", "rationale_saved": true
  },
  "evidence": ["machine/assertions.json#thumb-top",
               "machine/assertions.json#thumb-bottom"],
  "mark": "[x]",
  "decided_by": "real-machine session, main model"
}
```

**書く人:** `assertions.json` は検査のスクリプト、`vision\*.json` は Vision の subagent、`verdict` の `result` までは突き合わせのスクリプト。`mark` と `decided_by` は main モデルだけが書く。 **PR の 1 行（kura の例）:** `41.15 tier1 [x] C:\dev\filer-evidence\20261107-101203-41.15\verdict-41.15.json`。**残す期間の案:** いまの印の根拠になっている回は残し、それ以外は 30 日で消す。

<a id="d8"></a>

## 成果物 8 — 導入の順とパイロット

1. **前提: フェーズ 1（fail closed）** — 済み。kura v0.151.0（`keys: failed`・`settled`）、ito v0.262.3（`-Out` の掃除）と v0.262.7（`xrun.sh` は `keys: done` で終わらない回を失敗に。ito#202 は閉じた）。kura の段 ①（書き込みの番号と待ち）は v0.152.0〜v0.152.2 で、再テストは kura#511。
2. **観測の契約（フェーズ 2）** — ito: state JSON v1 のスキーマ、`<Snap:>`、矩形つきの Tree、ID の手順、Runner の観測の世代（[ito#203](https://github.com/uchmk/ito/issues/203) は閉じた（v0.263.0〜v0.267.0）。状態 JSON v1 のスキーマ・`<Snap:>`・矩形つきの Tree は [ito#205](https://github.com/uchmk/ito/issues/205)。実行の状態機械と適合テストは [ito#204](https://github.com/uchmk/ito/issues/204)（1〜4・6・8 は v0.269.0・v0.271.0・v0.272.0・v0.273.0））。kura は v0.153.0〜v0.154.0 で ID の手順・run の行を受けた。kura: state JSON v1 の中身、Test ID、自前の実行への写し。判断 3〜5 は 2026-11-08 に決まった。
3. **Tier 1 の文字と状態の行（kura）** — 59.5・57.3・20.16（56.6・58.4 は v0.152.1 で `[ ]` に戻ったので、kura#511 の再テストでテキストとして測る）。規則の文言は変えずに測れるが、いまの `[~]` は「見た目の行」と判断された記録なので、`[x]` にするかは持ち主が決める。それまでは結果を報告に書くだけ。
4. **Tier 1 の矩形と画素の行（kura。変更 1 は承認済み。ito の docs 版 1 と kura の規則のファイルに載せたあと）** — 41.15・49.4・1.46・18.7・54.16、54.18 の色。
5. **Tier 2 の影運用** — Vision を走らせ、結果は `[~]` と AI の報告にとどめて、持ち主の判断と比べる。基準: 20 行以上・誤合格 0・カナリアの検出 100 %。満たしたら変更 2 を持ち主に出す。
6. **基準画像の回帰** — 主な画面（設定の各ページ、パレット、spot、ヘルプ、確認）を ライト／ダーク × 100 %／150 % で。基準画像は持ち主が承認する。
7. **独立レビュー** — Codex・ChatGPT は高リスクの画面、意見の割れ、大きなデザインの変更だけ。
8. **ほかのアプリ** — kura のパイロットが通ったら、yagura・koyomi・michi・fude にアダプターを足し（順はパイロットのあとで決める）、道具ごとに 1 行ずつパイロットを回す。これから作るアプリも同じ形で足す。Runner のアプリは ito v0.272.0 以降が前提（ito#204 の残りが入ったら、その rev）。

### パイロット（kura。道具ごとに 1 行）

| 行 | 確かめる道具 | わざと壊す（カナリア） |
|---|---|---|
| 59.5 | state だけ。runner → 証拠 → verdict の流れ全体 | overlay を書き換えた state で FAIL になる（検査のユニットテスト） |
| 41.15 | UI 要素の矩形 | つまみの矩形を消した・ずらした state で FAIL |
| 49.4 | 画素の色とトークン | 帯の色を入れ替えた画像で FAIL |
| 54.18 | テーマの画素（Tier 1）と、Vision の盲検・突き合わせ・カナリア（Tier 2 の影運用） | 検索欄だけ暗い地に塗った画像を Vision が FAIL にする |

**パイロットの合格:** 各行 3 回続けて同じ結果、カナリアの回はすべて FAIL、証拠が欠けない、持ち主が結果に同意する。カナリアは製品のコードを変えずに、同じ回の画像と state をスクリプトで書き換えて作る。

<a id="d9"></a>

## 成果物 9 — 人の承認が要る例外

1. **基準画像を作る・更新する**（何が正しいかを決める行為）。
2. **Tier 3 の観点**（好み・美しさ・Quiet Precision・情報の階層）。
3. **判定不能**（INCONCLUSIVE、機械と Vision の食い違い、モデルの間の割れ）。
4. **高リスクの画面**: 消す・上書き・Reset の確認、機械の設定を変える画面（サインイン時の起動）、権限やセキュリティの表示、初めて起動したときの画面。
5. **大きなデザインの変更**（トークン、テーマ、レイアウト）。基準画像を作り直す。
6. **機械の全体に効く操作が要る行**（54.19 の OS のテーマ、56.4 のレジストリ）: 測り方より先に、実行そのものに承認。
7. **OS が描くもの**（通知、系のダイアログ、IME の候補、タイトルバー、タスク マネージャー）: アプリの state に無い。
8. **段の格上げ**（3→2、2→1）と、check spec の閾値を緩めること。
9. **初めて出る画面**（基準画像も観点の実績も無い）。
10. Linux レーンの画素の判定は使わない（いまの規則のまま。CPU の描画は実機の代わりにならない）。

### 独立レビュー（Claude の Vision ＋ Codex・ChatGPT）

- 使うのは、高リスクの画面、Vision と機械か Vision 同士で割れたとき、大きなデザインの変更のときだけ（Codex の枠は足りているが、判定の数を増やしても証拠は増えないので、場面を絞る）。
- 同じ観点の問いと同じ画像だけを渡す。機械の結果と、ほかのモデルの答えは見せない。同じ `vision.v1` の形で返させる。
- **一致しても、客観的な証拠が足りなければ合格にしない**。一致は「割れていない」を示すだけ。割れたら持ち主へ。

<a id="d10"></a>

## 成果物 10 — 時間・費用・誤判定のリスク

| 段 | 1 行の実行（目安） | トークン（目安） | 持ち主の手間 |
|---|---|---|---|
| **1** | 15〜40 秒（起動・押す・落ち着く・終わる）＋検査 1 秒未満 | main が assertions と verdict を読む分 | 無し（承認した規則の範囲） |
| **2** | ＋Vision 20〜60 秒 | 画像 1〜3 枚と観点の文。`usage` を記録して、パイロットで見積もり直す | 影運用の間は全部、そのあとは抜き取り |
| **3** | 同じ | AI の報告の分 | 1 行数分 |
| 独立レビュー | ＋1 回 | Codex の残りを使う | 割れたときの判断 |

数は目安で、まだ測っていない。料金は実行するときのモデルの料金表で計算する。

**作る手間の目安:** ito（スキーマ・`<Snap:>`・矩形つきの Tree・実行の状態機械と適合テスト・共通の検査器）ito のセッション 2〜4 回／観測の契約のアダプター 1 つのアプリにつき Sonnet のワーカー 1〜3 回（kura は自前の実行への写しがあるので多め）／検査のスクリプト 1〜2 回／Vision の subagent（定義・問い・スキーマ・カナリア）1 回／パイロット 実機のレーン 1〜2 回と持ち主の確認。

| リスク | 起き方 | 害 | 対策 |
|---|---|---|---|
| **誤合格（機械）** | 検査が要素を見ていない（test_id の漏れ）、閾値が緩い、基準画像が悪い | 壊れた見た目が `[x]` になる | 観点ごとに test_id を必須に、閾値は実行の前に固定、基準画像は持ち主が承認、検査そのものをカナリアのユニットテストで試す |
| **誤合格（Vision）** | 幻覚、小さな欠けや色の小さな差の見落とし、迎合 | 同じ | Vision だけでは通さない、場所を示させて矩形と突き合わせる、盲検、カナリア、Tier 2 は影運用で誤合格 0 を確かめてから |
| **取り違え** | 別の回・別のフレームの画像、前の回の成果物 | 正しくない証拠 | run_id・snapshot_seq・前後のハッシュ、run ごとに新しい folder（`-Out` の掃除は ito v0.262.3）、manifest の sha256 |
| **環境の差** | 倍率・フォント・GPU・テーマが基準と違う | 誤った FAIL か PASS | env を manifest に書き、違えば判定不能 |
| **誤った FAIL** | アンチエイリアス、動く部分 | 持ち主の手間 | 覆い、許容差、3 回の再現 |
| **画面の文字による誘導** | 画面に映った文字（ファイル名など）が Vision への指示のように読まれる | 誤判定 | 固定の fixtures、画像の中の文字はデータとして扱うと問いに書く、Vision だけでは通さない |
| **一致の過信** | 2 つのモデルが同じ誤り | 誤合格 | 一致を証拠に数えない（規則に書く） |
| **アプリの間のずれ** | Runner と kura の自前の実行で、同じ手順の意味が違う | 同じ check spec がアプリによって通ったり落ちたりする | 実行の状態機械と適合テストを ito に置き、両方の実行に通す。manifest に実行の種類と ito の rev を書く |
| **規則の空洞化** | 見た目の行を Tier 1 だと言い張る | `[x]` の意味が薄まる | 段は check spec に実行の前に書く、格上げは持ち主の承認、PR に段を 1 行ずつ書く |

**変更 1 は承認（2026-11-08）・未適用** **変更 2 は影運用の基準待ち**

<a id="rule"></a>

## 成果物 5 — 規則の変更案（kura の lanes.md を例に、ほかのアプリの雛形つき）

形は 2 層。**共通の規則の文**（段の定義・自動合格の条件・証拠の形・影運用の基準）は ito の `docs/visual-test.md` に版つきで置き、**各アプリの規則のファイル**は「どの版を、どの行に、どの段まで使うか」と「誰が付けるか」だけを書く。 共通の文を引くだけでは、いまの「見た目の行には付けない」と食い違う（[穴の表](#gaps)）。だから載せるアプリは自分の文を**置き換える**。**この文書の作成では、どのファイルも変えていない。**

変更 1 は持ち主が承認した（2026-11-08）。各アプリの規則のファイルに入れるのは、ito の `docs/visual-test.md`（版 1）が push されてから（参照先が無いまま規則を書き換えない）。変更 2 は、そのアプリで影運用の基準を満たしてから持ち主に出す。

下は最初に載せる kura の案（正の `docs/claude/lanes.md` の「`[x]` の規則（正）」に 1 項目足し、1 文を置き換える。kura の細かい測り方は `wintest-measure.md` に足す）。ほかのアプリの雛形はその下。

### 変更 1 — 機械が測った見た目の行（Tier 1）

105 行の項目の下に足す:

> **案:** 見た目の行でも、期待値のすべてを**機械が測った**行は「測って確かめた」に入る。機械の測定は、同じ実行の state JSON の値・UI 要素の矩形・画素の値・持ち主が承認した基準画像との差で、閾値は実行の前に check spec に書いたもの。条件は ito の `docs/visual-test.md`（版 1）の「自動合格の条件」。**AI Vision の答えとモデル同士の一致は、測定に数えない。**

113〜114 行を置き換える:

> **現行:** …期待値が**読めるテキストかファイルの状態**である／根拠を示せる）。**見た目の行には絶対に付けない。**

> **案:** …期待値が**読めるテキストかファイルの状態か、機械が測った値**である／根拠を示せる）。**目で見て判断しただけの行には絶対に付けない**（AI Vision の判断もここに入る。下の AI 判定合格を除く）。

### 変更 2 — AI 判定合格（Tier 2。変更 1 のあと、影運用の基準を満たしてから）

新しい項目:

> **案:** **AI 判定合格**。check spec に `tier = 2` と観点が書かれた行は、次のすべてがそろったときだけ、実機のセッションの main モデルが `[x]` を付けてよい: 機械の前提（同じ実行・settled・後片付け・機械の観点がすべて PASS）、Vision が全観点に場所つきで PASS、場所と読んだ文字が state と合う、同じ回のカナリアを FAIL にした。`verdict.json` に `tier = 2` を書き、PR に 1 行書く。Vision の答えだけ、モデル同士の一致だけでは付けない。判定不能・意見の割れ・高リスクの画面は `[~]` で持ち主へ。影運用（Vision を走らせても `[~]` にとどめ、持ち主の判断と比べる）で 20 行以上・誤合格 0・カナリアの検出 100 % を満たすまで、この項目は効かない。

### 変えないもの

- subagent（`wintest`・Vision の subagent を含む）と runner は印を付けない。
- `[~]` の規則（見る前に失敗の見え方を書く、画像のパスを PR に書く、済みに数えない、持ち主が `[x]` にする）は Tier 3 にそのまま使う。
- Linux レーンでは画素の判定を使わない。

### 合わせて直すファイル（変更 1 を載せるとき）

- `.claude/windows-role.md` 147 行: “Never tick an appearance row.” → “Never tick an appearance row on a look, yours or a model's. A row whose every expected value a machine measured in the same run (ito docs/visual-test.md, version 1) is a measured row. If you are unsure which kind a row is, it is an appearance row.”
- `docs/claude/wintest-measure.md`: 矩形と画素の測り方を足す。
- `.claude/agents/wintest.md`: 機械の検査の証拠を返す（印は付けない、は変えない）。Vision の subagent の定義を新しく足す。
- 印の形: 案 a に決めた（`[x]` のまま証拠に `tier` を書く。生成器を変えない）。案 b（別の印。ito の `ito-testcheck` の変更が要る）は使わない。

### ほかのアプリの雛形（載せるときに使う。いまは何も変えない）

いまの規則の文はアプリごとに違う。載せるまでは、どのアプリも今の規則のまま（見た目の行は `[~]` まで、`[x]` は持ち主）。

| アプリ | いまの文（読んだ行） | 載せるときに置き換える先 |
|---|---|---|
| kura | `docs/claude/lanes.md` 105・113〜114 行、`.claude/windows-role.md` 147 行 | 上の変更 1・2 |
| yagura | `.claude/windows-role.md` 53〜56 行（同じ形。画像は `Save-Shot`） | 下の英語の雛形 |
| michi | `CLAUDE.md` 63 行（日本語。「見た目の行は `[~]`（画面の画像で判断した）まで」） | 下の日本語の雛形（63 行を置き換える） |
| koyomi・fude | まだ無い | 下の日本語の雛形を `CLAUDE.md` に足す |

**英語の雛形**（`.claude/windows-role.md` の “Never tick a look.” の段落を置き換える）:

> **案:** Visual rows follow ito `docs/visual-test.md`, version 1, for the rows listed in `tests/visual/` only; on any conflict, that document wins over this file. A row whose every expected value a machine measured in the same run (Tier 1) is a measured row and you may tick it with the run's `verdict.json` named in the pull request. Tier 2 (AI-judged) is **off** for this app until the owner turns it on here. Never tick a row on a look, yours or a model's; a row with no check spec, or one you are unsure about, is a look: mark it `[~]` as before, and the owner turns `[~]` into `[x]`.

**日本語の雛形**（michi の `CLAUDE.md` 63 行の置き換えと、koyomi・fude・これから作るアプリで見た目の行の規則を日本語の `CLAUDE.md` に書くときに使う）:

> **案:** 見た目の行は ito の `docs/visual-test.md`（版 1）に従い、対象は `tests/visual/` にある行だけ。食い違えば ito の文が勝つ。同じ実行で期待値のすべてを機械が測った行（Tier 1）は、実機のセッションが `verdict.json` を PR に書いて `[x]` にしてよい。AI 判定合格（Tier 2）は、持ち主がここで ON にするまで使わない。check spec の無い行と迷う行は、目で見た行として `[~]` まで（`[x]` は持ち主）。

どのアプリでも変えないもの: subagent と runner は印を付けない。`[~]` の規則は Tier 3 にそのまま使う。Tier 2 を ON にするのは、そのアプリで影運用の基準（20 行・誤合格 0・カナリア 100 %）を満たしたあと（kura で満たしても、ほかのアプリでは数え直す）。

<a id="ask"></a>

## 持ち主の決定（2026-11-08）

持ち主の「推奨で進めてください」で、下の 9 つはすべて推奨の案に決まった。9 は同じ日に持ち主が変えた（tsumugi・minato を外し、koyomi・michi・fude を足した）。持ち主はいつでも変えられる。

1. **変更 1: 承認**。機械が同じ実行で期待値を全部測った見た目の行（Tier 1）は `[x]` にしてよい。各アプリの規則のファイルに入れるのは、ito の `docs/visual-test.md`（版 1）の push のあと。
2. **変更 2: 影運用の基準つきで進める**。20 行以上・誤合格 0・カナリアの検出 100 % を、アプリごとに数える。規則の文は、そのアプリで基準を満たしてから持ち主に出す。それまで Vision の答えは `[~]` と報告にとどめる。
3. **印の形: 案 a。**`[x]` のまま、証拠（`verdict.json`）に `tier` を書く。生成器は変えない。
4. **check spec: `tests/visual/<行>.toml`**。書くのは、その行を書いた開発のセッション。
5. **[ito#203](https://github.com/uchmk/ito/issues/203) は閉じた**（3 段、v0.263.0〜v0.267.0）。① 観測の世代と `Runner` の待ち、② test_id・ID の手順・`<ReadHas:>`、③ run_id・`with_run`・確かめ役・Shell カードの ID。実行の状態機械と適合テストは別の Issue [ito#204](https://github.com/uchmk/ito/issues/204)（1〜4・6・8 は v0.269.0・v0.271.0・v0.272.0・v0.273.0）。state JSON v1 のスキーマ・`<Snap:>`・矩形つきの Tree・検査器・証拠のフォルダは [ito#205](https://github.com/uchmk/ito/issues/205)（見た目のパイロットの前）。
6. **2 つの設計は ito の `docs/visual-test.md`・`docs/autotest.md` に移す**。ito のセッションに頼んだ。移したあとは ito の文書が正で、このページは写し。
7. **今の `[~]` の行を遡って `[x]` にしない**。59.5 などはパイロットで Tier 1 として測り直す。
8. **パイロットは 4 行**（59.5・41.15・49.4・54.18）。
9. **kura のあとは yagura・koyomi・michi・fude**。4 つの順は kura のパイロットのあとで決める。tsumugi・minato は 2026-11-08 にメンテナンス対象から外した（代わりに Orca と Linear を使う）。これから作るアプリも同じ形で足す。

自動テストの設計の判断 3〜5（安全の境界・最初の範囲・予算と証拠の保存期間）も同じ日に決まった（[自動テストの設計](autotest.md)の「持ち主の判断」）。この設計はその上に載る。

根拠に読んだもの: kura `docs/claude/lanes.md` 100〜131 行、`.claude/windows-role.md` 145〜174 行、`TESTING-CHECKS.md` の `[~]` の行、 kura `src/main.rs` の `raw_input_hook`・`take_shot`・`state_report`、`src/app.rs` のトースト、 yagura `.claude/windows-role.md` の規則の段落、michi `CLAUDE.md` 63 行（2026-11-08 に足した）（tsumugi・minato の規則と Shot も読んだが、2026-11-08 にメンテナンス対象から外した）、 ito `ito-keyscript`（`Runner`・`tree_text`・`failed_report`・Shot の付属 JSON）、`ito-window`、`ito-notify`、`scripts/lanes/keys.ps1`・`xrun.sh`、 前の設計（8VZmtoXgfxC544itHX8SK9）、Codex の設計レビュー（6 回と、全アプリ向けの 1 回）。
