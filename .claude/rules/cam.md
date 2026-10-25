---
paths:
  - "crates/kmb-cam-*/**"
  - "crates/kmb-strategy-*/**"
  - "crates/kmb-toolpath*/**"
  - "crates/kmb-post*/**"
  - "crates/kmb-sim/**"
  - "crates/kmb-gcode-read/**"
  - "crates/kmb-plugin-host/**"
  - "plugins/**"
---
# 加工を触る時の決まり

正本: policy.md「決めたこと」の対象の機械・加工画面・加工の高さと空振り、「機械とポスト」「Carvera Air への出力仕様」、plan.md「経路の中間表現とポスト」。

- 加工の核(セットアップ・工具・戦略・経路)に機械の名前を書かない(層の点検が落とす)。機械の違いは `MachineLibrary` の値とポストに閉じ込める。セットアップは機械の値の写しを文書に持つ。知らない機械 ID は誤りにする。
- 戦略は `plan(&StrategyInput)` の純関数。evaluate の中で経路を作らない。戦略の crate は kmb-post とカーネルに依存しない。加工の操作は戦略の名前を知らず、名乗りのデータで扱う。
- ポストは中間表現を文字にするだけ。範囲・回転数・工具番号は出力の前に `check` で確かめる。Carvera Air は mm(G21)で出し、工具交換は `T<n> M6`。ほかのポストのコードは参照しない。
- 1 つのポケットを工具の順番で削れること、入る位置を 3D で置けることが売り。工具の自動選びは作らない(docs/避ける形.md)。
- 削った所の空振りは控えめな格子で判断し、疑わしい所は残す。早送りが材料に当たらないことを優先する。
- 経路と G-code はゴールデン(`apps/kmb-mcp/tests/golden`)で確かめる。実機の試し削りはしない。
