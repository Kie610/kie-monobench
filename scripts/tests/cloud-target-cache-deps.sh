#!/bin/sh
# cloud-target-cache.sh の試験: 依存だけの組(save-deps・restore-deps)を確かめる。
# - save-deps は incremental を置かず、全体の組(debug-linux)に触らない
# - restore-deps は今ある target/debug に無い物だけを足し(同じ名前の物は今ある方を残す)、同じ組を 2 度戻さない
#   sh scripts/tests/cloud-target-cache-deps.sh
set -eu
src=$(cd "$(dirname "$0")/.." && pwd)/cloud-target-cache.sh
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() { cat "$work/log" >&2; echo "失敗: $1" >&2; exit 1; }
# 試す場所に小さな作業場を作る(本物の target に触らない)
repo=$work/repo
mkdir -p "$repo/scripts" "$repo/crates/kmb-workspace-hack"
cp "$src" "$repo/scripts/"
echo hack >"$repo/crates/kmb-workspace-hack/Cargo.toml"
echo lock >"$repo/Cargo.lock"
cache=$work/root/.cache/target
run() { (cd "$repo" && JC_CACHE_ROOT=$work/root sh scripts/cloud-target-cache.sh "$@") >"$work/log" 2>&1; }
# 全体の組が置いてある所に、依存だけの組を置く
mkdir -p "$cache/debug-linux-20261028000000"
echo whole >"$cache/debug-linux-20261028000000/part-000"
echo "parts: debug-linux-20261028000000" >"$cache/debug-linux.txt"
mkdir -p "$repo/target/debug/deps" "$repo/target/debug/incremental/own-1"
echo dep >"$repo/target/debug/deps/libdep-0123456789abcdef.rlib"
echo inc >"$repo/target/debug/incremental/own-1/x"

# 1. save-deps
run save-deps || fail "save-deps が 0 以外で終わった"
parts=$(sed -n 's/^parts: //p' "$cache/deps-linux.txt")
case "$parts" in deps-linux-*) ;; *) fail "依存だけの組の名前が違う($parts)" ;; esac
grep -q "^lock: " "$cache/deps-linux.txt" || fail "Cargo.lock の指紋を書いていない"
names=$(cat "$cache/$parts"/part-* | zstd -dc | tar -tf -)
echo "$names" | grep -q "libdep-0123456789abcdef.rlib" || fail "依存を置いていない"
echo "$names" | grep -q incremental && fail "incremental を置いた"
[ -e "$cache/debug-linux-20261028000000/part-000" ] && [ -e "$cache/debug-linux.txt" ] || fail "全体の組を消した"

# 2. restore-deps: 今ある target/debug に足す。同じ名前の物は今ある方を残す
rm -rf "$repo/target"
mkdir -p "$repo/target/debug/deps"
echo mine >"$repo/target/debug/deps/libdep-0123456789abcdef.rlib"
echo own >"$repo/target/debug/deps/libown-fedcba9876543210.rlib"
mkdir -p "$work/t/debug/deps"
echo other >"$work/t/debug/deps/libother-1111111111111111.rlib"
echo dep >"$work/t/debug/deps/libdep-0123456789abcdef.rlib"
# 置いた組に、今の target に無い物を足しておく(欠片を作り直す)
tar -C "$work/t" -cf - debug | zstd -q -c >"$cache/$parts/part-000"
rm -f "$cache/$parts"/part-0[0-9][1-9]
sed -i 's/^count: .*/count: 1/; s/^bytes_kb: .*/bytes_kb: 1/' "$cache/deps-linux.txt"
run restore-deps || fail "restore-deps が 0 以外で終わった"
grep -q "を戻しました" "$work/log" || fail "戻さなかった"
[ "$(cat "$repo/target/debug/deps/libdep-0123456789abcdef.rlib")" = mine ] || fail "今ある物を上書きした"
[ -f "$repo/target/debug/deps/libother-1111111111111111.rlib" ] || fail "無い物を足さなかった"
[ -f "$repo/target/debug/deps/libown-fedcba9876543210.rlib" ] || fail "今ある物を消した"
[ "$(cat "$repo/target/debug/.deps-restored")" = "$parts" ] || fail "戻した組を覚えていない"

# 3. 同じ組は 2 度戻さない
run restore-deps || fail "2 回目の restore-deps が 0 以外で終わった"
grep -q "戻してあります" "$work/log" || fail "2 回目も戻した"
echo "通過: 依存だけの組は incremental を除いて置き、全体の組に触らず、今の target に無い物だけを 1 度だけ足した"
