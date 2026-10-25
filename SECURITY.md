# セキュリティ / Security

## 知らせ方 / Reporting

脆弱性は公開の Issue に書かず、GitHub の非公開の報告(リポジトリの「Security」タブの「Report a vulnerability」)で知らせてください。届いたら内容を確かめて返事をし、直した版を出すまで中身を公開しません。

Please do not report vulnerabilities in public issues. Use GitHub's private vulnerability reporting instead ("Report a vulnerability" on the repository's Security tab). We will confirm the report, reply, and keep the details private until a fix is released.

## 対象 / Scope

対象は main の最新の版です。古い版の直しは出しません。

Only the latest version on main is supported. Fixes are not backported.

次のような物を脆弱性として扱います。

We treat issues like these as vulnerabilities:

- 読んだファイル(文書・STEP・DXF・メッシュ・画像・ポストのプラグイン)で、許した範囲の外へ書く・外のコードを動かす・情報を外へ出す
  A file that is opened (document, STEP, DXF, mesh, image, post-processor plug-in) writes outside the allowed folders, runs outside code, or leaks information
- MCP のサーバ(`jc-mcp`)や画面の「AI との接続」から、許したフォルダの外を読み書きできる
  The MCP server (`jc-mcp`) or the "connect to AI" setting can read or write outside the allowed folders
- WASM のポストのプラグインが、ファイル・ネットワーク・時計に触れられる
  A WASM post-processor plug-in can reach files, the network or the clock

読んだファイルでアプリが落ちるだけの物は、ふつうの不具合として Issue に書いてかまいません。機械を動かす G-code の誤りは、安全に関わるので Issue に書き、題に「G-code」と入れてください。

A crash caused by a malformed file, with no other effect, is an ordinary bug and can be reported as an issue. Wrong G-code that drives a machine is a safety issue: please report it as an issue with "G-code" in the title.
