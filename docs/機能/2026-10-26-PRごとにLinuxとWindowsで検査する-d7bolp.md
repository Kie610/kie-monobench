# PR ごとに Linux と Windows で検査する

- 日付: 2026-10-26

## 作った物

`.github/workflows/verify.yml`。PR と main へのプッシュのたびに、GitHub の CI が `cargo xtask verify`(SMLib)を Linux と Windows で流す。PR は変えた crate とその下流、検査の道具や Rust の版を変えた PR と main は全体。落ちた時は `target/verify/` のログを取れる。

## 決めたこと

- CI は xtask の検査をそのまま呼ぶ。範囲の選び方と点検は xtask が持ち、CI の設定に検査の中身を書かない(docs/verify.md の 14 節)。
