#!/bin/sh
# cloud-target-cache.sh の試験: save が前の組の索引を外したまま止まっても、restore が黙って終わらず、
# 印が古くなれば預けた索引を戻して使えることを確かめる(前は sed の失敗で set -e が何も言わずに 2 で抜け、
# 次の save が終わるまで誰も戻せなかった)。
#   sh scripts/tests/cloud-target-cache-stale-save.sh
set -eu
src=$(cd "$(dirname "$0")/.." && pwd)/cloud-target-cache.sh
work=$(mktemp -d)
save_pid=""
trap '[ -n "$save_pid" ] && kill -9 -"$save_pid" 2>/dev/null; rm -rf "$work"' EXIT
fail() { cat "$work/log" >&2; echo "失敗: $1" >&2; exit 1; }
# 試す場所に小さな作業場を作る(本物の target に触らない)
repo=$work/repo
mkdir -p "$repo/scripts" "$repo/crates/kmb-workspace-hack"
cp "$src" "$repo/scripts/"
echo hack >"$repo/crates/kmb-workspace-hack/Cargo.toml"
hack=$(sha256sum "$repo/crates/kmb-workspace-hack/Cargo.toml" | cut -c1-16)
cache=$work/root/.cache/target
lock=$cache/saving
run() { (cd "$repo" && JC_CACHE_ROOT=$work/root sh scripts/cloud-target-cache.sh "$@") >"$work/log" 2>&1; }
# 前の組を置く(欠片 1 つ)
set_name=debug-linux-20261026000000
mkdir -p "$work/t/debug/deps" "$cache/$set_name"
echo rlib >"$work/t/debug/deps/libdep-0123456789abcdef.rlib"
tar -C "$work/t" -cf - debug | zstd -q -c >"$cache/$set_name/part-000"
printf 'parts: %s\nrustc: %s\ncount: 1\nbytes_kb: 1\nhack: %s\n' "$set_name" "$(cd "$repo" && rustc -V)" "$hack" >"$cache/debug-linux.txt"

# 1. 生きている save が索引を預けている間は、理由を言って 0 で終え、印に触らない
mkdir "$lock"
echo "1@vm 生きている" >"$lock/owner"
: >"$lock/beat"
mv "$cache/debug-linux.txt" "$lock/info"
run restore || fail "生きている save の途中で 0 以外で終わった"
grep -q "置き直している途中なので戻しません" "$work/log" || fail "理由を言わなかった"
[ -e "$lock/info" ] && [ ! -e "$repo/target/debug" ] || fail "生きている save の印を外したか、戻した"
mv "$lock/info" "$cache/debug-linux.txt"
rm -rf "$lock"

# 2. 本物の save を読んでいる印の前で待たせ、後片付けなしで消す(コンテナが終わったのと同じ)。
#    印が古くなった後の restore は、預けた索引を戻して前の組を戻す
mkdir -p "$repo/target/debug" "$cache/readers"
: >"$cache/readers/$set_name.vm.1"
(cd "$repo" && JC_CACHE_ROOT=$work/root exec setsid sh scripts/cloud-target-cache.sh save) >"$work/save.log" 2>&1 &
save_pid=$!
i=0
until grep -q "終わるのを待ちます" "$work/save.log" 2>/dev/null; do
    i=$((i + 1))
    [ "$i" -lt 300 ] || { cp "$work/save.log" "$work/log"; fail "save が待ちに入らなかった"; }
    sleep 0.1
done
kill -9 -"$save_pid"
wait "$save_pid" 2>/dev/null || true
save_pid=""
rm -rf "$repo/target" "$cache/readers/$set_name.vm.1"
[ -e "$cache/debug-linux.txt" ] && fail "save が索引を外していない(試験の前提が崩れた)"
[ -d "$lock" ] || fail "save の印が残っていない(試験の前提が崩れた)"
# 止まってから 11 分たったことにする
touch -d '11 minutes ago' "$lock/beat" 2>/dev/null || touch -d '11 minutes ago' "$lock"
run restore || fail "止まった save の後の restore が 0 以外で終わった"
grep -q "を戻しました" "$work/log" || fail "止まった save の後に戻さなかった"
[ -f "$repo/target/debug/deps/libdep-0123456789abcdef.rlib" ] || fail "rlib が戻っていない"
[ -e "$cache/debug-linux.txt" ] && [ ! -e "$lock" ] || fail "索引を戻さなかったか、印を外さなかった"

# 3. beat を持たない前の版の印が古く、索引も無い時は、印を外し、索引が無いと言って 0 で終える
rm -rf "$repo/target"
rm "$cache/debug-linux.txt"
mkdir "$lock"
echo "11933@vm" >"$lock/owner"
touch -d '91 minutes ago' "$lock"
run restore || fail "前の版の古い印で 0 以外で終わった"
grep -q "止まった save の印を外しました" "$work/log" && grep -q "索引(debug-linux.txt)が無い" "$work/log" || fail "理由を言わなかった"
[ ! -e "$lock" ] && [ ! -e "$repo/target/debug" ] || fail "印を外さなかったか、戻した"
echo "通過: 生きている save では理由を言って待たず、止まった save の後は預けた索引で戻し、前の版の古い印は外した"
