# TODO

ito のやること。`【人】` は持ち主の作業。

- [x] tsumugi・yagura にも `scripts/lanes.conf` を置き、`scripts/lanes.sh sync` で `scripts/lanes/` の写しに揃える（v0.8.0 で `files=` と
      `check-ps1-app.ps1` を足した。tsumugi のチェック表は自分の example のまま: 表の形が kura と違う）。
      tsumugi v0.94.0（全部）・yagura v0.42.0（`files=` で 4 本）・kura v0.103.2 で済んだ。
- [x] 【人】Social preview を上げる（Settings → General → Social preview、`ito.png`）。2026-10-15 に持ち主が上げた。
- [x] XTVERSION の応答をアプリの名前と版にする。`crates/ito-pane/src/osc.rs` の `xtversion()` は `tsumugi {CARGO_PKG_VERSION}` を返すが、
      ito に移ったので版が ito の版（0.1.0）になった。アプリが名前と版を渡す形にする（`Terminal` を作るときの引数か、プロセスで 1 回の設定）。
      kura が使うときに kura と名乗れるようにもなる。v0.10.0 の `set_identity` で済んだ（tsumugi・kura が main で呼ぶ）。
- [x] ペインの公開の API の名前を整理する（`encode`、`snapshot`、`children` など filer の頃の名前が並んでいる。tsumugi の `docs/pane-extraction.md`）。
- [x] macOS の型検査を CI に足すかを決める（`sys/unix.rs` の `hang_up_children`）。費用は tsumugi・kura と同じ考えで。 2026-10-16 に足した（clippy と Build が cargo-zigbuild で aarch64-apple-darwin を見る）。
- [ ] 見た目の行の証拠（ito#205 の C のあと）。
  - [x] `keys.ps1` / `xrun.sh` に `-Evidence`: folder を作り、run id を環境変数に入れ、どの終わり方でも manifest を書く。v0.279.0 で済んだ。
  - [ ] `xrun.sh --evidence` を本物の Linux で回す（v0.279.0 は Git Bash の上のスタブで確かめただけ。`setsid`・`ps`・グループへの kill）。アプリが `isolate=` と `evidence=` を足してからの Linux のレーンで。
  - [x] `merge-lanes.py`: 見た目の行の `[x]` を、候補の verdict が無いまま付けたら断る。v0.281.0 で済んだ。
  - [x] Vision（Tier 2）の実行と突き合わせのスクリプト。v0.290.0 で済んだ（`evidence.py vision-prep`・`vision-record`・verdict の `ai-pass-candidate`。持ち主の決定 2026-11-10 で影運用の基準は外した）。アプリの規則のファイルへの反映は各アプリのセッションで。
  - [x] `env.wgpu_backend` / `env.font_set_sha256` を ito が埋める。v0.287.0 で済んだ（`wgpu_backend` はアプリが `set_wgpu_backend` を呼ぶ）。
  - [ ] 基準画像との比較（承認した基準画像と、証拠の鎖の中での置き場所）。
- [ ] ito-keyscript の小さな直し（kura v0.158.0 が `pace::Pace` に載せ替えたときの指摘）。
  - [x] `Runner::start_run`: 最初の `run.json` が書けないとき、失敗の行を `trace.jsonl` に書かないまま trace を捨てている。捨てる前に `flush_trace` する。v0.280.0 で済んだ。
  - [x] `Pace` に `check_since` を読む口を足す（kura のテストが回り道をしていた）。v0.280.0 で済んだ。
  - [ ] テストで `Step` を手で作るときの小さな builder（要るアプリが増えたら）。
- [ ] 【保留】kura でリモートの PC のファイルを見る（2026-11-10 に持ち主が保留にした。実装も実機の確認もしていない）。
  - 再開するときの案: まず読むだけの範囲で、ito の土台（`~/.ssh/config` のホストの一覧の読み手。kura と koyomi で使う）と kura の閲覧まで進める。
  - 同じように使えるもの: フォルダーをたどる・カーソル・選択・`y`/`p` で手元へコピー、テキスト・Markdown・CSV のプレビュー（先頭だけ取る）。`<Enter>` は手元にコピーしていつものアプリで開く（編集はリモートに戻らない）。
  - 動きが変わるもの: 画像・PDF・Office のプレビューは全体を取るので待つ。動画など大きいものは名前とサイズだけ。巨大なログの行ごとの表示は、最初は先頭だけ。
  - 読むだけの段階で使えないもの: リモートの削除・名前の変更・移動、リモートへの貼り付け（kura のアーカイブの中と同じように理由を出して断る）、ターミナルのペイン、Git の状態、ディスク使用量、中身の検索。
  - 決めること: 初めてのホストの確認とパスワード・パスフレーズの入力画面、踏み台（`ProxyJump`）の扱い、Linux のファイル名の `\`・`:` を kura の Windows のパスの形で壊さない表し方、スリープからの復帰などで切れたときのつなぎ直し、リモートの変更は通知されないので再読み込みか定期の確認。
  - 別案: 書き込みも要るなら SSHFS-Win でリモートを `\…` のパスに見せる（Windows 専用・別のソフトが要る・kura で動くかは未確認）。
