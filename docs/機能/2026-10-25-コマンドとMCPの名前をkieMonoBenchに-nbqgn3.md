# コマンドと MCP の名前を kieMonoBench に

- 日付: 2026-10-25
- 決定: コマンドと MCP の名前を kieMonoBench に

## 作った物

- CLI の実行ファイルを `kmb`(Windows は `kmb.exe`)、MCP サーバの実行ファイルを `kmb-mcp` にした(apps/kmb-cli・apps/kmb-mcp の `[[bin]]`)。`--help`・使い方の例・誤りの文の命令の名前も `kmb` にした。
- ログと落ちた時の記録の名前を `kmb-cli`・`kmb-mcp` にした(`kmb` にすると、ログを数える頭 `kmb-` が MCP のログまで拾うため)。
- README・README.en・SECURITY・使い方・plan.md・verify.md・policy.md の例と名前を直し、README に前の名前から移る手順(MCP の設定の `command` の書き換え)を書いた。
- `cargo xtask sweep` が見る release の実行ファイルの名前を `kmb`・`kmb-mcp` にした(前は CLI を `kmb-cli` で探していて見つけられなかった)。骨格の資料の点検は `kmb <命令>` の形を CLI の命令と突き合わせる。

## 決めたこと

- 使う人が打つ名前は CLI が `kmb`(文書の拡張子 `.kmb` と同じ)、MCP が `kmb-mcp`。crate の名前(`kmb-cli`・`kmb-mcp`)と頭 `jc-`、フォルダ apps/kmb-cli・apps/kmb-mcp は変えない。
- 前の名前の実行ファイルは作らない(別名の実行ファイルを残すと組み立てが倍になり、どちらが新しいか迷う)。移り方は README に書く。

## 試し方

- `cargo run -p kmb-cli -- --help` の頭が `kmb` になっていることを見る。
- MCP の設定の `command` を `target/release/kmb-mcp` にして、AI から文書を開けることを見る。
