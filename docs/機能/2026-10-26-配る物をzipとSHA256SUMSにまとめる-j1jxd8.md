# 配る物の zip と SHA256SUMS

- 日付: 2026-10-26

## 作った物

タグで配る物を組み立てる CI(`.github/workflows/release.yml`)が、OS ごとの配る物を `kie-monobench-<タグ>-<OS>.zip` にまとめ、両方の OS が済んだ後の回で全部の zip の `SHA256SUMS` を作って成果物に置く(docs/verify.md の 15 節)。受け取った人は `sha256sum -c SHA256SUMS` で zip が壊れていないかを確かめられる。

## 決めたこと

- zip の中は名前と同じフォルダ 1 つにする(展開しても散らばらない)。
- `SHA256SUMS` は OS ごとに分けず 1 つにする。両方の zip がそろってから別の回で作る。
- まとめるのは CI の段で行い、xtask は組み立てと数えだけを持つ(zip を作る依存を xtask に足さない)。
