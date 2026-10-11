# TODO

ito のやること。`【人】` は持ち主の作業。

- [ ] tsumugi・yagura にも `scripts/lanes.conf`・`scripts/lanes.sh` と `ito-testcheck` の example を入れ、`scripts/lanes/` の写しに揃える
      （kura の v0.103.0 が手本。tsumugi の `wintest-queue.ps1` は `lane-marks.ps1` と同じ関数）。
- [ ] 【人】Social preview を上げる（Settings → General → Social preview、`ito.png`）。
- [ ] XTVERSION の応答をアプリの名前と版にする。`crates/ito-pane/src/osc.rs` の `xtversion()` は `tsumugi {CARGO_PKG_VERSION}` を返すが、
      ito に移ったので版が ito の版（0.1.0）になった。アプリが名前と版を渡す形にする（`Terminal` を作るときの引数か、プロセスで 1 回の設定）。
      kura が使うときに kura と名乗れるようにもなる。
- [ ] ペインの公開の API の名前を整理する（`encode`、`snapshot`、`children` など filer の頃の名前が並んでいる。tsumugi の `docs/pane-extraction.md`）。
- [ ] macOS の型検査を CI に足すかを決める（`sys/unix.rs` の `hang_up_children`）。費用は tsumugi・kura と同じ考えで。
