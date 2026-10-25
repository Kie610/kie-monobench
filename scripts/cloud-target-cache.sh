#!/bin/sh
# クラウドの作業場で、組み立て済みの target/debug をコンテナをまたいで使い回す(docs/verify.md の 1 節)。
# コンテナは毎回 target が空から始まり、一からの組み立てに 10 分、clippy にさらに 7 分かかっていた。
# 戻した target は cargo が中身を確かめて使うので、版が違っても結果は変わらない(取り出し直後は
# ソースの時刻が新しいので、自前の crate だけを作り直す。incremental が効いて 30 秒ほど)。
# 自前の crate の成果物(テストと実行ファイル、lib)は毎回作り直すので、置かず戻さない(固める前で
# 約 9 GB。戻すと、指紋の変わった時に新しい組と並んで空きを使い切っていた。2026-10-23、docs/verify.md の 8 節)。
#
#   scripts/cloud-target-cache.sh restore   検査の前に(target/debug が無く、戻しても空きが 10 GB 残る時だけ戻す)
#   scripts/cloud-target-cache.sh save      検査が通った後に(1 つだけ持つ。共有フォルダの空きを確かめ、前のものを消してから書く)
#
# 共有フォルダは別のコンテナも同時に使う。読み書きは次の印で譲り合う。
#   readers/<組>.<…>  restore が読んでいる間だけ置く。save はこれが消えるまで前の組を消さない
#   saving/           save が書いている間だけ置く。ほかの save は置かずに終える
# 印は置いたコンテナが途中で終わると残るので、決めた分(stale_min・lock_stale_min)より古い物は無いものと見る。
# restore は別の場所へ取り出し、終わってから target/debug へ移す(途中で切れても半分だけ戻さない)。
#
# 共有フォルダは 1 ファイルがおよそ 0.5 GB を超えると上がらず、コンテナが終わると消える
# (2026-10-22 に測った。400 MB は 20 秒で上がり、700 MB と 1 GB は 7〜25 分たっても上がらなかった)。
# なので 256 MB ずつに分けて置く。上がるまでの間はメモリ(/dev/shm)に載っている。
#
# /mnt/project-files が無い所(利用者の PC など)では何もしない。JC_CACHE_ROOT で置き場所を変えられる(試す時)。
set -eu
root=${JC_CACHE_ROOT:-/mnt/project-files}
dir=$root/.cache/target
info=$dir/debug-linux.txt
readers=$dir/readers
lock=$dir/saving
part_mb=256
margin_gb=10
# restore は 3〜5 分(2026-10-23)。これより古い印は終わったコンテナが残した物
stale_min=${JC_CACHE_STALE_MIN:-30}
# save は 10 分ほど。書いている途中の組を別の save が消さないよう、長めに見る
lock_stale_min=${JC_CACHE_LOCK_STALE_MIN:-90}
# save が読み終わりを待つ長さ。過ぎたら前の組を消さずに置くのをやめる
wait_s=${JC_CACHE_WAIT_S:-900}
cd "$(dirname "$0")/.."
hack_sum() { sha256sum crates/kmb-workspace-hack/Cargo.toml | cut -c1-16; }
# 置かず戻さない物の型(tar の --exclude)。自前の crate のテストと実行ファイル(deps/<名前>-<16 桁>)、
# lib(deps/lib<名前>-<16 桁>.rlib・.rmeta)、target/debug の直下の実行ファイル(deps の物と同じ実体)。
# 置いた後に足した crate の物は残るが、次の save で除かれる。型を作れなければ何も除かない
ws_excludes() {
    cargo metadata --no-deps --format-version 1 --offline 2>/dev/null | python3 -I -c '
import json, sys
h = "?" * 16
for p in json.load(sys.stdin)["packages"]:
    for t in p["targets"]:
        if "custom-build" in t["kind"]:
            continue
        n = t["name"].replace("-", "_")
        print(f"debug/deps/{n}-{h}")
        for ext in ("rlib", "rmeta", "so"):
            print(f"debug/deps/lib{n}-{h}.{ext}")
        if "bin" in t["kind"]:
            print("debug/" + t["name"])
'
}
# 古い印を消す
drop_stale() { [ -d "$1" ] && find "$1" -mindepth 1 -maxdepth 1 -mmin +"$stale_min" -exec rm -rf {} + 2>/dev/null || true; }
mark=""
staging=target/.debug-restoring
cleanup() {
    [ -n "$mark" ] && rm -f "$mark"
    [ -d "$staging" ] && rm -rf "$staging"
    [ -d "$lock" ] && [ "$(cat "$lock/owner" 2>/dev/null)" = "$$@$(hostname)" ] && rm -rf "$lock"
    return 0
}
trap cleanup EXIT
trap 'exit 130' INT TERM

[ -d "$root" ] || { echo "共有フォルダが無いので使いません"; exit 0; }
if ! command -v zstd >/dev/null 2>&1; then
    echo "zstd を入れます"
    (apt-get install -y -qq zstd >/dev/null 2>&1 || { apt-get update -qq >/dev/null 2>&1 && apt-get install -y -qq zstd >/dev/null 2>&1; }) ||
        { echo "zstd を入れられないので使いません"; exit 0; }
fi
rustc_v=$(rustc -V)

case "${1:-}" in
restore)
    if [ -d target/debug ]; then
        echo "target/debug があるので戻しません"
        exit 0
    fi
    rm -rf "$staging"
    # 組の名前を読み、読む印を置いてから、名前が変わっていないかをもう一度見る。save は debug-linux.txt を
    # 外してから印を見るので、もう一度見た時に名前が残っていれば、その組は読み終わるまで消されない
    parts=$(sed -n 's/^parts: //p' "$info" 2>/dev/null)
    [ -n "$parts" ] && [ -d "$dir/$parts" ] || { echo "置いた target がまだありません($dir)"; exit 0; }
    mkdir -p "$readers"
    mark=$readers/$parts.$(hostname).$$
    : >"$mark"
    # 読み終わる前に save が debug-linux.txt を外すことがあるので、ここで写しを取る
    snap=$(cat "$info" 2>/dev/null || true)
    [ "$(printf '%s\n' "$snap" | sed -n 's/^parts: //p')" = "$parts" ] || { echo "別のコンテナが置き直している途中なので戻しません"; exit 0; }
    if ! printf '%s\n' "$snap" | grep -qxF "rustc: $rustc_v"; then
        echo "置いた target は別の Rust の版で作ったので使いません($rustc_v)"
        exit 0
    fi
    want=$(printf '%s\n' "$snap" | sed -n 's/^count: //p')
    got=$(ls "$dir/$parts" | wc -l)
    [ "$want" = "$got" ] || { echo "置いた target の欠片が足りないので使いません($got/$want)"; exit 0; }
    # 戻した後も、検査が自前の crate を作り直す分の空きを残す(2026-10-23 は 18 GB を戻して空き 12.8 GB、
    # 揃えた依存の変わった main で作り直す途中に尽きた。docs/verify.md の 8 節)
    need_kb=$(printf '%s\n' "$snap" | sed -n 's/^bytes_kb: //p')
    # 大きさを書く前の save が置いた物は、固めた大きさの 4 倍と見る(2026-10-23 は 4.6 GB → 18 GB)
    [ -n "$need_kb" ] || need_kb=$(( $(du -sk "$dir/$parts" | cut -f1) * 4 ))
    avail_kb=$(df -Pk . | awk 'NR==2{print $4}')
    if [ -n "$need_kb" ] && [ "$avail_kb" -lt $(( need_kb + margin_gb * 1000000 )) ]; then
        echo "空きが足りないので戻しません(空き $(( avail_kb / 1000000 )) GB、戻すと $(( need_kb / 1000000 )) GB + 残す ${margin_gb} GB)"
        exit 0
    fi
    # 揃えた依存(kmb-workspace-hack)が置いた時と違えば、自前の crate は全部別の指紋で作り直すので
    # incremental は使われない。取り出した後で消して場所を空ける。tar の --exclude で除くと、deps の
    # rmeta が incremental の物へのハードリンクになっていて、リンク先が無く取り出し全体が失敗する
    drop_incr=""
    saved_hack=$(printf '%s\n' "$snap" | sed -n 's/^hack: //p')
    if [ -n "$saved_hack" ] && [ "$saved_hack" != "$(hack_sum)" ]; then
        echo "揃えた依存が置いた時と違うので、incremental は取り出した後で消します"
        drop_incr=1
    fi
    start=$(date +%s)
    mkdir -p "$staging"
    ex=$(mktemp)
    ws_excludes >"$ex" || true
    # sh には pipefail が無いので、前の段の失敗は印のファイルで受け取る
    failed=$(mktemp)
    if ! { cat "$dir/$parts"/part-* || echo cat >>"$failed"; } | { zstd -dc -T0 || echo zstd >>"$failed"; } |
        tar -C "$staging" -xf - -X "$ex" || [ -s "$failed" ]; then
        rm -f "$ex" "$failed"
        echo "取り出しに失敗したので戻しません(一から組み立てます)" >&2
        exit 1
    fi
    rm -f "$ex" "$failed"
    [ -z "$drop_incr" ] || rm -rf "$staging/debug/incremental"
    mv "$staging/debug" target/debug
    rm -rf "$staging"
    echo "target/debug を戻しました($(( $(date +%s) - start )) 秒、$(du -sh target/debug | cut -f1)、残りの空き $(df -Pk . | awk 'NR==2{printf "%.1f", $4/1e6}') GB)"
    printf '%s\n' "$snap" | sed 's/^/  /'
    ;;
save)
    [ -d target/debug ] || { echo "target/debug がありません"; exit 1; }
    start=$(date +%s)
    mkdir -p "$dir"
    # 今の版が使わない古い成果物と incremental の前の回を除いてから固める(docs/verify.md の 8 節)
    cargo xtask sweep --yes >/dev/null 2>&1 || echo "sweep に失敗(そのまま固めます)"
    # 共有フォルダはプロジェクトで 10 GiB まで(2026-10-23 は 9 GiB 使っていた)。前の組(4.6 GB)を残したまま
    # 新しい組を書くと上限に当たるので、足りるかを先に確かめ、読み終わるのを待って前の組を消してから書く。
    # 消してから書き終わるまでの間に restore したコンテナは戻さず、一から組み立てるだけ
    old_kb=0
    for old in "$dir"/debug-linux-* "$dir"/debug-linux.tar.zst; do
        [ -e "$old" ] && old_kb=$(( old_kb + $(du -sk "$old" | cut -f1) ))
    done
    # 新しい組の大きさの見込み: 前の組の 1.1 倍(前の組が無ければ、固める前の 3 割)
    if [ "$old_kb" -gt 0 ]; then need_kb=$(( old_kb * 11 / 10 )); else need_kb=$(( $(du -sk target/debug | cut -f1) * 3 / 10 )); fi
    used_kb=$(du -sk "$root/" | cut -f1)
    quota_kb=$(( ${JC_SHARED_QUOTA_GIB:-10} * 1048576 ))
    if [ $(( used_kb - old_kb + need_kb )) -gt "$quota_kb" ]; then
        echo "共有フォルダの空きが足りないので置きません(使用 $(( used_kb / 1048576 )) GiB、前の組 $(( old_kb / 1048576 )) GiB、新しい組の見込み $(( need_kb / 1048576 )) GiB、上限 $(( quota_kb / 1048576 )) GiB)"
        exit 1
    fi
    # 書くのは 1 つのコンテナだけ(mkdir は在るか確かめるのと作るのを 1 度にする)
    if [ -d "$lock" ] && [ -n "$(find "$lock" -maxdepth 0 -mmin +"$lock_stale_min")" ]; then rm -rf "$lock"; fi
    if ! mkdir "$lock" 2>/dev/null; then
        echo "別のコンテナが置いている途中なので置きません($(cat "$lock/owner" 2>/dev/null || echo 不明))"
        exit 0
    fi
    echo "$$@$(hostname)" >"$lock/owner"
    # 新しい restore を始めさせないよう、先に debug-linux.txt を外してから、読んでいる途中の物を待つ
    kept_info=$(mktemp)
    cp "$info" "$kept_info" 2>/dev/null || true
    rm -f "$info"
    waited=0
    while :; do
        drop_stale "$readers"
        busy=$(ls "$readers" 2>/dev/null | wc -l)
        [ "$busy" -eq 0 ] && break
        if [ "$waited" -ge "$wait_s" ]; then
            [ -s "$kept_info" ] && cp "$kept_info" "$info"
            rm -f "$kept_info"
            echo "前の組を読んでいるコンテナが ${wait_s} 秒たっても終わらないので置きません(前の組はそのまま)"
            exit 1
        fi
        [ "$waited" -eq 0 ] && echo "前の組を読んでいるコンテナ($busy)が終わるのを待ちます"
        sleep 10
        waited=$(( waited + 10 ))
    done
    rm -f "$kept_info"
    for old in "$dir"/debug-linux-* "$dir"/debug-linux.tar.zst; do
        if [ -e "$old" ]; then rm -rf "$old"; fi
    done
    parts=debug-linux-$(date -u +%Y%m%d%H%M%S)
    mkdir -p "$dir/$parts"
    ex=$(mktemp)
    ws_excludes >"$ex" || true
    if ! tar -C target -cf - -X "$ex" debug | zstd -T0 -3 -q | split -b ${part_mb}M -d -a 3 - "$dir/$parts/part-"; then
        rm -rf "${dir:?}/$parts"
        echo "書き込みに失敗したので、書きかけの組を消しました(置いた target はありません)"
        exit 1
    fi
    # 戻した後の大きさ(restore が空きと比べる)
    kept_kb=$(cd target && du -sk -X "$ex" debug | cut -f1)
    rm -f "$ex"
    # debug-linux.txt は書き終わってから作る(restore はこれと欠片の数で、書き終わった組だけを使う)
    {
        echo "rustc: $rustc_v"
        echo "commit: $(git rev-parse --short HEAD) $(git rev-parse --abbrev-ref HEAD)"
        echo "saved: $(TZ=Asia/Tokyo date '+%Y-%m-%d %H:%M JST')"
        echo "parts: $parts"
        echo "count: $(ls "$dir/$parts" | wc -l)"
        echo "size: $(du -sh "$dir/$parts" | cut -f1)(固める前 $(du -sh target/debug | cut -f1)、${part_mb} MB ずつ)"
        echo "bytes_kb: $kept_kb"
        echo "hack: $(hack_sum)"
    } >"$info"
    echo "target/debug を置きました($(( $(date +%s) - start )) 秒)"
    sed 's/^/  /' "$info"
    ;;
*)
    echo "使い方: $0 restore|save" >&2
    exit 2
    ;;
esac
