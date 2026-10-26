# PR ごとに Linux と Windows で検査する

- 日付: 2026-10-26

## 作った物

`.github/workflows/verify.yml`。PR のたびと main で毎晩 1 回、GitHub の CI が `cargo xtask verify`(SMLib)を Linux と Windows で流す。PR は変えた crate とその下流、検査の道具や Rust の版を変えた PR と毎晩の main は全体。落ちた時は `target/verify/` のログを取れる。

## 決めたこと

- CI は xtask の検査をそのまま呼ぶ。範囲の選び方と点検は xtask が持ち、CI の設定に検査の中身を書かない(docs/verify.md の 14 節)。
- truck の回は PR と毎晩の main の両方で流す(`--truck`)。main の回だけで流すと、truck だけで落ちる試験が取り込んだ後に初めて見つかる。
- main へのプッシュでは流さず、毎晩 1 回にする。取り込みの前に PC が全体を通しており、プッシュごとに流していた間は続けて来る取り込みが前の回を止め、main の回がほとんど「取り消し」で終わっていた。
