# 届いた証明つきのファイル転送（koyomi の send / fetch を広げる案）

> 2026-11-13: ito-sftp, ito-cloud, ito-winrm and the other crates this proposal names moved to koyomi (`koyomi-*`); ito no longer has them.

HULFT のような「確実に届いたことを残す」ファイル転送を、新しいアプリでなく koyomi の send / fetch の部品に「届いたかの確認」を足して作る案。
2026-11-01 の ito セッションでの会話と実機の確認から始め、同じ日に koyomi の今の部品と ito-sftp の中身に合わせて書き直した。まだ作っていない。

## 目的

- サーバー間（PC・Proxmox のコンテナ・取引先）のファイル転送を、**届いた証明つき**で、定義・履歴・再送まで一か所で見られるようにする。
- 製品の HULFT を置き換える、ではなく、小さな拠点・自前のサーバー間で、同じ安心を安く得る。

## 置き場: 別のアプリにせず koyomi に足す

koyomi（軽い JP1）は、この案の「転送ジョブ・実行の記録・一覧・再送・スケジュール・再試行・通知」をもう持っている:

- `kind = "send"` / `"fetch"`（TODO 78、H7、2026-10-31）。SFTP は ito-sftp で koyomi が自分でつなぐ。相手のホスト鍵は `host_key = "SHA256:…"` で固定し、パスワードは `secret:<名前>`。`retry` が効く。
- `kind = "ssh"`（ito-sftp の `exec`）と `kind = "winrm"` で、相手の PC のコマンドを流せる。
- ジョブネット・履歴・再実行・通知・取り込みと書き出しは koyomi のものがそのまま使える。

別のアプリにすると、スケジューラと履歴の画面を二重に持つことになる。足りないのは「**届いたことを確かめて、その証拠を残す**」所だけなので、それを ito-sftp と koyomi の send / fetch に足す。
受け側に koyomi があれば、ファイルの契機（`[[when]] file`）が HULFT の集信後のジョブにあたる。

## 実機で確かめたこと（2026-11-01）

| 項目 | 結果 |
|---|---|
| 経路 | Tailscale。`tailscale ping couchdb` が `via 192.168.0.132`（直接、4 ms）で通った |
| 転送 | Windows から Proxmox のコンテナ `couchdb` へ `scp t.txt root@couchdb:/tmp/` が通った |
| 整合性 | 送り側 `Get-FileHash` と受け側 `sha256sum` が一致した |
| 認証 | 初めは `yuu06@couchdb` が無く、root で通した（本番は転送専用のユーザーと鍵にする） |

## 役割の分担（HULFT が持つもの → 置き換え）

| HULFT が持つもの | 置き換え |
|---|---|
| 暗号化・相手の認証 | SSH（koyomi の `host_key` の固定と鍵・パスワード） |
| 到達性 | ふつうのネットワーク。自前のサーバー間なら Tailscale も使える（前提にはしない。取引先に Tailscale は求められない） |
| 転送そのもの | ito-sftp（koyomi の send / fetch）。`.part` に書き、済んだら名前を替える |
| 再開 | 当分は持たない（下の「大きなファイル」） |
| 整合性・届いた証明 | 受け側で取ったハッシュの突き合わせ（下の「届いたことの確認」） |
| 文字コード | 未定（下の「文字コード」。`iconv` の 1 行では済まない） |
| スケジュール・再送・通知 | koyomi（ジョブネット・`retry`・通知） |
| 履歴・定義・一覧・再送ボタン | koyomi の画面 |

## 届いたことの確認（2 段に分ける）

### 段 1「届いた」: 名前を替える前に確かめる（先に作る）

send の流れ:

1. 送り側でハッシュ（SHA-256）を取りながら `<name>.part` に送る（ito-sftp は今も `.part` に書いてから名前を替える）。
2. 受け側で `.part` のハッシュを取る（取り方は下の「受け側でハッシュを取る方法」）。
3. 一致したら名前を替える。合わなければ `.part` を消して ✕（`retry` が効く）。
4. koyomi の履歴に、ファイルの名前・サイズ・ハッシュ・突き合わせの結果を残す。

名前を替えたあとに確かめると、受け側の取り込みが確かめる前のファイルを拾ってしまう。替える前に確かめれば、**最終の名前で見えたファイルは中身が正しい**と言える。
fetch はこの逆: 相手のファイルのハッシュを先に取り、こちらの `.part` に受けて自分で取ったハッシュと合えば名前を替える。

送り側が自分で突き合わせるので、`.ack` のファイルも、`file.sha256` を一緒に送ることも要らない。

### 段 2「処理した」: 受け側のアプリが返す ack（後で）

`.ack` に意味があるのは、受け側のアプリが「取り込んだ」と返すとき（業務の段の確認）。要るようになってから、形式（JSON の欄）と置き方を決める。

- `ssh-keygen -Y sign` の署名も後回し。通り道はホスト鍵の固定で守れている。署名は、ack を監査の証拠として後で他に渡すときに効く。
- `.ack` は慣習の名前で、規格ではない。

## 受け側でハッシュを取る方法

案のぶつかり所: 転送専用のユーザー（`/incoming` にだけ書ける）は、ふつう OpenSSH の `ForceCommand internal-sftp` と chroot で作る。そうすると `exec` が使えず、受け側で `sha256sum` を走らせられない。OpenSSH の SFTP には、ハッシュを取る拡張（`check-file` など）も無い。

| 方法 | 良い所 | 限り |
|---|---|---|
| sshd + `exec`（`sha256sum`、Windows は `Get-FileHash`） | 受け側に何も入れない | シェルを開けるユーザーが要る。転送専用の制限と両立しない。Windows の OpenSSH は既定のシェルが `cmd` なので、PowerShell を呼ぶ形にする |
| 受け側を SFTPGo にする | SFTP だけのユーザーのまま `sha256sum` が使える（SFTPGo が自前で持つ SSH のコマンド。`md5sum`・`sha1sum`・`sha256sum` は初めから有効。ソースの `internal/sftpd/sftpd.go` の `defaultSSHCommands` で確かめた） | 受け側に SFTPGo を入れる。取引先には頼めない |
| 送ったものを読み戻してハッシュを取る | どの SFTP サーバーでも使える | 通信量が倍。受け側のディスクから読んだ証明にはなるが、相手の手元で計算したものではない |
| 受け側に小さなエージェントを置く | 段 2 の ack にもつながる | 入れて保守する物が増える |

方針: 自前のサーバーは sshd + `exec` か SFTPGo、取引先は読み戻し（相手がシェルを許すなら `exec`）。定義で `verify = "sha256"`（`exec`）と `verify = "readback"` のように選べるようにする。

## 大きなファイル

- ito-sftp は、失敗したり止めたりすると `.part` を消す（残さない作り）。途中からの再開は持たない。
- rsync（`rsync -avP --partial`）は Windows にふつうは入っていない。Windows を優先する koyomi では前提にしない。
- 再開が要るようになったら、ito-sftp に「`.part` を残し、その長さから続きを送る」オプションを頼む。最後のハッシュの突き合わせが全体を保証するので、再開しても安全に確かめられる。中小企業の CSV のやり取りでは当分要らない。
- 分割（`split`・tus・S3 のマルチパート）は、数百 GB 以上で部品ごとの再試行が要るときだけ。
- 受け側の空き容量（`/tmp` が小さいコンテナ）に注意。

## 文字コード（将来の部品の候補）

取引先との転送では、Shift_JIS と UTF-8 の変換が HULFT を選ぶ理由の 1 つになっている。`iconv` の 1 行で済むように見えるが、次を決めないと、黙って中身が変わったり、途中で止まったりする。

- **CP932 か Shift_JIS か**: 「Shift_JIS で」と言われるファイルは、たいてい Windows の CP932（①・㈱・髙 などを含む）。`iconv -f SHIFT_JIS` はこれらで止まる。`CP932` と書く。
- **写せない文字**: UTF-8 から CP932 へは 𠮷・絵文字などを写せない。止めるか、`〓` や `?` に替えるか（替えると黙ってデータが変わる）。
- **表による行き先の違い**: 〜（U+301C）と ～（U+FF5E）、−（U+2212）と －（U+FF0D）などは、変換の表（JIS 寄りか Microsoft 寄りか）で行き先が変わる。往復すると別の文字になったり、写せずに止まったりする。
- **外字**: 取引先が自分で足した文字（CP932 の F040〜F9FC）は表に無い。相手との取り決めが要る。
- **固定長のレコード**: 変換でバイト数が変わる（半角カナは CP932 で 1 バイト、UTF-8 で 3 バイト。漢字は 2 → 3）。桁で区切るレコードはずれるので、レコードの定義を知った変換が要る（HULFT が得意な所）。
- **BOM と改行**: BOM の有無、CRLF と LF。
- **推測しない**: 元の文字コードは定義に書く（短いファイルでは自動の判定が外れる）。
- **ハッシュとの順番**: 送る途中で変換すると、送り側と受け側のハッシュが合わない。送る前に変換するか、確かめたあとに受け側で変換し、どちらのバイト（元のものか、変換したものか）の証明を残すかを決める。

### HULFT の扱い（HULFT8 のマニュアルで確かめた。2026-11-01）

- **対応の無い文字は既定の文字にする**: 外字テーブルで引き、無ければ出力のコードごとの既定の文字にする。Shift-JIS は `0x81A0`、EUC-JP は `0xA2A2`、UTF-8 は `E2 96 A1` で、どれも □。IBM 漢字は `0x44E9`、NEC は `0x7F7F`（どの文字かは見ていない）。
- **止めることもできる**: `gaijierr`（外字未登録時処理）で「既定の文字にする」か「エラー」かを選ぶ。ただし、マニュアルの書き方は「外字の範囲で外字テーブルに無いコード」。UTF-8 の 𠮷 や絵文字も同じ扱いかは、読んだページからは分からなかった。
- **文字ごとの対応は外字テーブル**: `HULPATH/gtf` の `gt○○○.txt` を書き換え、`utlgtfextd -i` で展開し、HULFT を再起動する。例: Shift-JIS の ㈱（`0x878A`）を JEF の ㈱（`0x78D5`）へ。取引先の外字はここで扱う。
- **〜 などは受け側の OS で決まる**: 他のコードから UTF-8 への変換で、受け側が Windows・IBM i・z/OS なら Microsoft の表（〜 は U+FF5E、− は U+FF0D、‖ は U+2225）、UNIX・Linux なら Unicode の表（U+301C・U+2212・U+2016）。HULFT-HUB を通すときに送り側で変換すると期待どおりにならず、受け側の変換を勧めている。
- **送り側・受け側のどちらで変換するかを選べる**（簡易転送では `S` / `R`）。
- **固定長は項目の定義で変換する**: フォーマット転送で項目ごとに型（X・N・M・P・S など）と長さを決め、項目ごとに変換する。変換で短くなった分は半角スペースで埋める。長くなって項目に入らないときの扱いは見つけられなかった。

出典（HULFT の公式ヘルプ）:
[外字テーブル組み込み](https://www.hulft.com/help/ja-jp/HULFT-V8/UX-ADM/Content/HULFT_ADM_UNIX/Env/inclusion_external_chara.htm)・
[コード変換の設定](https://www.hulft.com/help/ja-jp/HULFT-V8/WIN-ADM/Content/HULFT_ADM_WIN/Env/field_CodeConv.htm)・
[UTF-8機種依存文字の変換](https://www.hulft.com/help/ja-jp/HULFT-V10/COM-CNV/Content/HULFT_CNV/code_table/UTF8_Dependent.htm)・
[UTF-8のコード変換に関する留意事項](https://www.hulft.com/help/ja-jp/HULFT-V8/COM-CNV/Content/HULFT_CNV/caution/Notes_UTF8.htm)・
[データが減少する場合](https://www.hulft.com/help/ja-jp/HULFT-V8/COM-CNV/Content/HULFT_CNV/specific/Ex_FMT_space2byte.htm)・
[フォーマット転送の変換規則](https://www.hulft.com/help/ja-jp/HULFT-V8/COM-CNV/Content/HULFT_CNV/specific/CnvRule_Frmt_MultFrmtTrns.htm)・
[UTF-8の場合の機能制限](https://www.hulft.com/help/ja-jp/HULFT-V8/COM-CNV/Content/HULFT_CNV/caution/Notes_Version_utf8.htm)・
[外字テーブル展開コマンド（DataMagic）](https://www.hulft.com/help/ja-jp/DM-V3/COM-REF/Content/DM3_JA_REF/Chap01/utledgtfextdExpandExtChar.htm)

### 作るならこうする（案）

- **既定は止める（✕）**。黙って □ にすると、届いたあとに業務の側で見つかる。□ などに替えるのは、定義に書いたときだけ（`unmapped = "□"` のように）。
- **替えたら件数と行を履歴に残す**（HULFT には無い）。
- **文字ごとの対応は定義の TOML に書く**: `"㈱" = "(株)"`、`"髙" = "高"` のように。展開や再起動は要らない。
- **〜 などの表は定義で選ぶ**（`"microsoft"` か `"unicode"`）。受け側の OS で暗黙に決めない。koyomi は Windows を優先するので、既定は Microsoft の表（CP932）。
- **固定長は初めは扱わない**。項目の定義を持つ大きな機能になる。初めはテキストの変換だけ。

ito には文字コードの変換のクレートが無い（`encoding_rs` を ito-mail・ito-pane・ito-proc・ito-tail が中で使っているだけ）。部品にするなら ito に頼む。

## 既存の OSS（競合）

完全な競合は見つかっていない。重なるもの（要 README の確認。検索の要約しか見ていない）:

- [FileDrop](https://github.com/talder/FileDrop): SFTP のジョブ・スケジュール・履歴の Web 画面。ack と整合性の扱いは未確認。
- [GoMFT](https://github.com/StarFleetCPTN/GoMFT): rclone を包んだ Web 画面。アーカイブ扱いに見える。
- [JobScheduler](https://sourceforge.net/projects/jobscheduler/): ジョブ管理に転送を含む。転送の保証が主役ではない。
- [SFTPGo](https://sftpgo.com/on-premises): 受け側のサーバーとして有力（上の「受け側でハッシュを取る方法」）。送り側のジョブ管理は別。
- [Vento](https://zenn.dev/kyonaka/articles/108200585ba1db): Rust 製の転送 CLI。運用の機能は無い。
- Rundeck・NiFi・Stork は未調査。
- スケジューラ側の比べ（JP1/FTP・Control-M MFT・Airflow の SFTP provider など）は koyomi の `COMPETITORS.md` の H7。

空いている所: **日本の中小企業向けのスケジューラの中で、ハッシュの突き合わせまで済ませる転送**。

## HULFT が残す価値（コマンドでは置き換わらない所）

- ベンダーのサポートと責任、監査の証跡として認められやすいこと。
- メインフレーム・固定長レコード・EBCDIC など、古い基幹との接続。
- 大量の定義・世代管理・長年の運用の蓄積。

基幹や取引先との転送で「製品を使っている」ことが要件なら、置き換えには別の説明が要る。
監査の証跡に使うなら、koyomi の履歴（既定 90 日）より長く、転送の記録だけを残す設定が要る。

## やること

### ito（`crate-request`）

- ito-sftp の `put` / `get` に、名前を替える前の確認を入れる。送りながらハッシュを取り、受け側（`exec` か SFTPGo の `sha256sum`、または読み戻し）と突き合わせ、合ったときだけ名前を替える。結果の `Moved` にハッシュを返す。
- （後で）再開のオプション。（要るなら）文字コードの変換のクレート。

### koyomi

- send / fetch に `verify`（`"sha256"`・`"readback"`、無ければ今までどおり）を足す。合わないときは ✕ で `retry` が効く。
- 履歴にサイズ・ハッシュ・突き合わせの結果を残し、画面と CSV に出す。
- 転送の記録を残す期間を決める。

## 決めること

1. 受け側でハッシュを取る方法の既定（`exec` / SFTPGo / 読み戻し）と、定義での書き方。
2. 突き合わせが合わないときの扱い（`.part` を消して ✕ で決まりか、調べるために残すか）。
3. 転送の記録を残す期間（監査に使うか）。
4. 段 2 の ack の形式（JSON の欄・署名の要否）。後で。
5. 文字コードの変換を部品にするか、いつにするか。作るなら、上の「作るならこうする」（既定は止める・対応は TOML・表は定義で選ぶ）でよいか。

## 次の一歩

1. 鍵を入れる（パスワードなしの `ssh root@couchdb`）。
2. 転送専用のユーザーを作り（`/incoming` にだけ書ける）、`exec` が断られることと、読み戻しで突き合わせられることを実機で確かめる。
3. 上の「決めること」の 1 と 2 を決め、ito に `crate-request` を出す。
4. koyomi の TODO に `verify` を足す。
