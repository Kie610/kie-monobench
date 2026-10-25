# 製品名 kieMonoBench への改名

- 日付: 2026-10-25
- 決定: 製品名は kieMonoBench、文書の拡張子は kmb、公開の前に決めること

## 作った物

- 画面のアプリの実行ファイルを `kie-monobench`(Windows は `kie-monobench.exe`)にし、窓の題・安全モードの題・誤りの窓・ファイルを開く窓の種類の名前を「kieMonoBench」にした(apps/jc-app)。`cargo xtask release`・`cargo xtask sweep`・取り込みのスクリプト・画面を撮るスクリプトも新しい名前を見る。
- データフォルダを `%APPDATA%\kie-monobench`(Linux は `~/.local/share/kie-monobench`)にした。新しいフォルダが無く前の `jisaku-cadcam` があれば、最初に開いた時に中身(設定・並び・窓の記憶など。ログは除く)を隣へ写してから名前を替える(`jc_session::carry_over`)。前のフォルダは消さない。窓の大きさ・位置の記憶(eframe の鍵)も `kie-monobench` にした。
- 文書の拡張子を `.kmb` にした(`jc_core::DOCUMENT_EXTENSION`。中身は今までどおり JSON)。`.json` の文書も開く窓・リンク・プロジェクトの一覧で今までどおり扱い、名前を付けて保存すると `.kmb` になる。
- 書き出す STEP・STL・3MF・glTF・G-code・SVG・加工指示書に書く作ったソフトの名前を「kieMonoBench」にした。STEP を読む側は、前の名前(`jisaku-cadcam`)で書いた STEP も JC の物と見分け続ける(立てるかを聞かない、部品の名前に使わない)。
- 加工の既定とテンプレートのファイルの形式の名前を `kie-monobench/…` にし、前の名前の既定のファイルも読む。
- MCP のサーバの名前(serverInfo)を `kie-monobench` にした。使う人の MCP の設定の鍵は各自のまま動く。
- 画面の字体を渡す環境変数を `JC_FONT` にした(前の `JISAKU_CADCAM_FONT` も見る)。
- LICENSE-MIT の著作権者の表記を「kieMonoBench contributors」にした。

## 決めたこと

- 変えない物: CLI の `jc`・`jc-mcp`、crate の頭 `jc-`、履歴の置き場 `.jc/history`、ポストのプラグインの口の名前 `jisaku-cadcam:post`(作り済みのプラグインが読めなくなるので、口の版を上げる時に一緒に変える)、試験に使う前に書いた STEP のデータ。
- 前の名前で残した物を読む道(データフォルダの写し・`.json`・STEP の見分け・加工の既定の形式・字体の環境変数)は消さない。

## 試し方

- 前の版で使っていた PC で新しい exe を起動し、設定の単位・窓の大きさと位置・最近使ったファイルが前のままか見る。`%APPDATA%\jisaku-cadcam` が残っていることも見る。
- 前の `.json` の文書を開き、名前を付けて保存すると `.kmb` が選ばれることを見る。
