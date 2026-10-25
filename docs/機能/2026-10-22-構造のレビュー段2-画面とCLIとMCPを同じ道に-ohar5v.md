# 画面と CLI・MCP を同じ道に、画面の状態を文書ごとに分ける

- 日付: 2026-10-22

## 作った物

画面だけが別の道で文書を変え・再計算を回していたのを、CLI・MCP と同じ kmb-session の `Headless` に通した。使う人から見える振る舞いは、今使えない操作を画面でも断るようになったこと(CLI・MCP と同じ)のほかは変えていない。

- 点検を 4 つ足した(2a): 新しい static・画面の Op・Workspace の欄・検査の錠。nextest の件数と段の成否が食い違ったら誤りにする。
- kmb-session に裏の再計算(`start`・`poll`・`wait_recompute`)、選んでいる物を渡して操作を流す `run_with`(`Picks`・`Handled`)、既定の機能名 `new_feature_name` を足した(2a)。CLI・MCP で足した機能の既定の名前は「表示名+番号」になった。
- 画面の状態を文書ごとの `DocState`・窓全体・部品ごとに分けた(2b-1・2b-2)。タブの切り替えは `DocState` を丸ごと入れ替える。
- モジュールの設定(シーンの設定・スケッチの寸法の見せ方)とマシンライブラリをプロセス全体の static から組み立てごとの共有の値(`Registry` の `provide`・`service`)へ移した(2b-3)。同じプロセスの別の作業場は設定を共有しない。
- 画面で取り消しの変更(`Op`)を組む所を無くし、kmb-session の `edit`(部品・アニメーション・構成の表など。いくつかの変更を 1 回の取り消しにする `Changes`)を通す(2b-4)。
- `DocState` は文書・保存先・計算の結果・カーネルを持たず、`Headless` を 1 つ持つ。変更・取り消し・やり直し・開く・保存先・裏の再計算はどれも `Headless` を通す。画面の操作の実行も `Headless::run_with` を通す(2b-4)。
- 画面・CLI・MCP で同じ操作と引数を流すと同じ文書・同じ計算の結果・同じ断り方になることを確かめる試験(kmb-ui の `one_path_tests`)。文書に属する egui の記憶の鍵は `kmb_ui::doc_id` でタブごとに分けた(前は別の文書のタブへ欄の字などが移った)(2b-5)。
- 点検「画面の再計算」(画面とアプリの層で `recompute_watched` などを呼ばない)、`cargo xtask io`(docs/入出力.md を登録から作る)、「変えたら直す合図」を足し、AGENTS.md の構造の節に「同じ道を通る」「画面の状態の分け方」を不変の決まりとして置いた(2b-5)。

## 決めたこと

- 画面・CLI・MCP は kmb-session の `Headless` を通して文書を変え、再計算を回す。画面で `Op` を組まない(AGENTS.md 3 節、点検: 画面の Op・画面の再計算)。
- 画面の状態は文書ごと・窓全体・部品ごとに分け、`Workspace` に欄を足さない(点検: Workspace の欄)。
- 文書に属する egui の記憶の鍵は `kmb_ui::doc_id` で作る。`data_mut` の中で `doc_id` を呼ぶと egui の記憶の錠で止まるので、鍵は外で作る。

## 試し方

`cargo xtask verify --all`。kmb-ui の `one_path_tests`、kmb-session の試験、点検(新しい static・画面の Op・画面の再計算・Workspace の欄)。
