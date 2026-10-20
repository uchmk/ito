# {{app}} — Claude 向けメモ

{{description}} uchmk のアプリで、共通の部品は [ito](https://github.com/uchmk/ito) にある。Rust、Windows 優先。

## ito first（最優先の約束）

**ito にあるものは ito を使い、ないものは自分で書かずに ito に頼む。**

- 新しい機能を足す前に、ito の `CLAUDE.md` の表（`C:\dev\ito\CLAUDE.md`）で、同じ役目のクレートがないか見る。
- ないときは、uchmk/ito に Issue を立てる（雛形 `crate-request`）か、ito のセッションに頼む。
  このアプリに先に書いて、あとで ito に移すことはしない（移す手間が残り、二重になる）。
- `scripts/ito-first.py` が機械で見る。ito が受け持つ依存（`toml_edit`・`dirs`・`notify` …）を直接足すと落ちる。
  持つべき ito のクレート（keyscript・help・notify・bugreport …）が抜けていても落ちる。
  理由があって外すときだけ `scripts/ito-first.allow` に `<crate> <理由>` を書く（理由のない行は効かない）。
- 全部の ito のクレートは 1 つの `rev` にそろえる。上げたら `scripts/lanes.sh sync`。
  `.github/workflows/` の build・arm64・checklists・release・release-sums は ito の雛形から `sync` が書く（`lanes.conf` の `workflows=`）。直すなら ito で。

## 状態のファイルは TOML

アプリが機械的に書いて読み戻すもの（窓の場所・履歴・最近開いたもの・セッション）は `ito_common::state_dir("{{app}}")` に **TOML** で置く
（`window.toml`、`state.toml`、`recent.toml` …）。`.txt`・`.json`・拡張子なしのバイナリで持たない。

- 一覧は表の中の配列（`recent = ["…"]`、`[[entries]]`）。読むのは `ito_common::kept::read`、書くのは `kept::Writer`（別スレッド・最新が勝つ・`--keys` では書かない）。UI スレッドで書かない。
- ログは `.log`。キャッシュ・画像・WebView のデータなど、読み戻さないものは対象外。秘密の鍵は `app_dir` の中で、TOML でなくてよい。
- 詳しくは ito の `docs/common-spec.md` の「設定の形式は TOML」。

## 作業ルール

- 返答は日本語。コード・コメント・コミットメッセージは英語。
- 改行は LF。
- コミットと push は、`scripts/verify.sh` が `ALL OK` のときに、頼まれなくてもしてよい（`main` へ直接。force push・履歴の書き換えはしない）。自分の変更だけを `git add` する。
- 整形は `cargo fmt`（ito と同じ `rustfmt.toml`）。clippy は `-D warnings`。
- ビルドは R:（RAM ディスク）。新しいクローンでは `pwsh -NoProfile -File %LOCALAPPDATA%\uchmk-build\cargo-ramdisk.ps1` を先に回す。
- やることは [TODO.md](TODO.md)。版の繰り上げと CHANGELOG.md（日本語、日付は JST）はセットでコミットする。
- 共通の仕様（common.toml・設定の画面・テーマ・時計）は ito の `docs/common-spec.md`。
