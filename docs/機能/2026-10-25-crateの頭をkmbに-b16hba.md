# crate の頭を kmb- に

- 日付: 2026-10-25
- 決定: crate の頭を kmb- に

## 作った物

- crate とフォルダの頭を `jc-` から `kmb-` に変えた(crates/・apps/ の全部、`kmb-workspace-hack`、プラグインの見本 `kmb-post-example` も)。コードの `jc_<crate>` も `kmb_<crate>` に、層の名乗りの表を `[package.metadata.kmb]` にした。
- ログの絞り込み(`kmb_` で始まる target)、xtask の点検(他の層の名前・資料の名前)、試験の一時フォルダの名前、insta の写し、試験用の字体のファイル名も合わせた。

## 決めたこと

- 変えない物: ログとデータの置き場とファイル名(`kie-monobench`・`kmb-cli`・`kmb-mcp`)、履歴の置き場 `.jc/history`、ポストのプラグインの口の名前、SMLib の橋渡しの C の関数名(`jc_smlib.cpp` などの `jc_…`。作り直しの要る外の C の名前で、使う人に見えない)、画面の記憶の鍵(`jc_dock` など。変えると保存した並びが消える)、環境変数 `JC_…`、窓の枠の印の字 "jc"、docs/history の凍結した記録。

## 試し方

- `cargo xtask verify --all` が通ること。`git grep -P '(?<![\w])jc-'` が資料の経緯の文のほかに出ないこと。
