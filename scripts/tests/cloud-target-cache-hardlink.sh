#!/bin/sh
# cloud-target-cache.sh restore の試験: 揃えた依存が置いた時と違う時、incremental へのハードリンクを
# 含む組でも取り出せ、incremental だけが消えることを確かめる(前は --exclude で除き、リンク先が無く全部を捨てていた)。
#   sh scripts/tests/cloud-target-cache-hardlink.sh
set -eu
src=$(cd "$(dirname "$0")/.." && pwd)/cloud-target-cache.sh
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# 試す場所に小さな作業場を作る(本物の target に触らない)
repo=$work/repo
mkdir -p "$repo/scripts" "$repo/crates/kmb-workspace-hack"
cp "$src" "$repo/scripts/"
echo old >"$repo/crates/kmb-workspace-hack/Cargo.toml"
old_hack=$(sha256sum "$repo/crates/kmb-workspace-hack/Cargo.toml" | cut -c1-16)
echo new >"$repo/crates/kmb-workspace-hack/Cargo.toml"
# incremental が先に入り、deps の rmeta がそれへのハードリンクになる組
t=$work/t
mkdir -p "$t/debug/incremental/a" "$t/debug/deps"
echo rmeta >"$t/debug/incremental/a/x.rmeta"
ln "$t/debug/incremental/a/x.rmeta" "$t/debug/deps/libdep-0123456789abcdef.rmeta"
echo rlib >"$t/debug/deps/libdep-0123456789abcdef.rlib"
# 直す前の失敗の再現: リンク先を除くと取り出しが失敗する
mkdir -p "$work/x"
tar -C "$t" -cf "$work/t.tar" debug/incremental debug/deps
if tar -C "$work/x" -xf "$work/t.tar" --exclude=debug/incremental 2>/dev/null; then
    echo "失敗: --exclude で取り出せてしまい、前の失敗が再現しない" >&2
    exit 1
fi
cache=$work/root/.cache/target
mkdir -p "$cache/p"
zstd -q -c "$work/t.tar" >"$cache/p/part-000"
printf 'parts: p\nrustc: %s\ncount: 1\nbytes_kb: 1\nhack: %s\n' "$(cd "$repo" && rustc -V)" "$old_hack" >"$cache/debug-linux.txt"
JC_CACHE_ROOT=$work/root sh "$repo/scripts/cloud-target-cache.sh" restore >"$work/log" 2>&1 || { cat "$work/log"; exit 1; }
grep -q "を戻しました" "$work/log" || { cat "$work/log"; echo "失敗: 戻さなかった" >&2; exit 1; }
d=$repo/target/debug
[ "$(cat "$d/deps/libdep-0123456789abcdef.rmeta")" = rmeta ] || { echo "失敗: rmeta が戻っていない" >&2; exit 1; }
[ -f "$d/deps/libdep-0123456789abcdef.rlib" ] || { echo "失敗: rlib が戻っていない" >&2; exit 1; }
[ ! -e "$d/incremental" ] || { echo "失敗: incremental が残っている" >&2; exit 1; }
echo "通過: ハードリンクを含む組を戻し、incremental を消した"
