# TODO

ito のやること。`【人】` は持ち主の作業。

- [x] tsumugi・yagura にも `scripts/lanes.conf` を置き、`scripts/lanes.sh sync` で `scripts/lanes/` の写しに揃える（v0.8.0 で `files=` と
      `check-ps1-app.ps1` を足した。tsumugi のチェック表は自分の example のまま: 表の形が kura と違う）。
      tsumugi v0.94.0（全部）・yagura v0.42.0（`files=` で 4 本）・kura v0.103.2 で済んだ。
- [ ] 【人】Social preview を上げる（Settings → General → Social preview、`ito.png`）。
- [x] XTVERSION の応答をアプリの名前と版にする。`crates/ito-pane/src/osc.rs` の `xtversion()` は `tsumugi {CARGO_PKG_VERSION}` を返すが、
      ito に移ったので版が ito の版（0.1.0）になった。アプリが名前と版を渡す形にする（`Terminal` を作るときの引数か、プロセスで 1 回の設定）。
      kura が使うときに kura と名乗れるようにもなる。v0.10.0 の `set_identity` で済んだ（tsumugi・kura が main で呼ぶ）。
- [ ] ペインの公開の API の名前を整理する（`encode`、`snapshot`、`children` など filer の頃の名前が並んでいる。tsumugi の `docs/pane-extraction.md`）。
- [ ] macOS の型検査を CI に足すかを決める（`sys/unix.rs` の `hang_up_children`）。費用は tsumugi・kura と同じ考えで。
