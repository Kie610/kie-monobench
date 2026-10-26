#!/usr/bin/env bash
# PR の先頭のコミット($1)が main から分かれた後に、main で core の API の版(CORE_API_VERSION)が
# 変わったかを確かめる。変わっていれば 1 を返す。その PR は古い core の API で検査されているので、
# main に載せ直して検査し直してから取り込む(docs/verify.md の 16 節)。
# GitHub の CI の中で動かす(gh と GH_TOKEN・GITHUB_REPOSITORY を使う)。今の main と比べるので、
# 検査の回が始まった後に入った変更も見える。
set -euo pipefail
head="$1"
lib=crates/kmb-core/src/lib.rs
# compare の A...B は、分かれ目から B までの差。ここでは分かれ目から今の main までを見る
json=$(gh api "repos/$GITHUB_REPOSITORY/compare/$head...main")
count=$(jq '.files | length' <<<"$json")
patch=$(jq -r --arg f "$lib" '.files[] | select(.filename == $f) | (.patch // "本文なし")' <<<"$json")
# compare が返すファイルは 300 まで。それを超えると core の変更を見落とすので、載せ直しを求める
if [ "$count" -ge 300 ] || grep -qE '^[-+].*CORE_API_VERSION|^本文なし$' <<<"$patch"; then
  echo "main で core の API の版が変わった(または差が大きすぎて確かめられない)。main に載せ直して検査し直す"
  exit 1
fi
echo "main の core の API の版は、この PR が分かれた時から変わっていない"
