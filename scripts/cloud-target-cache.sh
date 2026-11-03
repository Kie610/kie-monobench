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
# 全体の写しのほかに、並列の作業場が共通に使う部分だけの写し(依存だけの組)も 1 つ持つ。組み立てと入口の
# 層より下の外の crate・SMLib・ビルドスクリプトの結果で、固めて約 0.6 GB、戻して約 2.4 GB(2026-10-29)。
# 押し出しの `--dependents-lower` が 375 秒から 148 秒になった(docs/verify.md の 1 節)。
#
#   scripts/cloud-target-cache.sh restore-deps   検査の前に。cargo xtask verify がクラウドで自分で呼ぶ。target/debug が
#                                                あっても、無い物だけを足す(同じ名前の物は中身も同じ)。同じ組は 2 度戻さない
#   scripts/cloud-target-cache.sh save-deps      `cargo xtask warm` の後に。incremental は置かない
#
# 共有フォルダは別のコンテナも同時に使う。読み書きは次の印で譲り合う。
#   readers/<組>.<…>  restore が読んでいる間だけ置く。save はこれが消えるまで前の組を消さない
#   saving/           save が書いている間だけ置く。ほかの save は置かずに終える。持ち主(owner)と、生きている
#                     間 1 分ごとに触る beat、待つ間に預かった前の組の索引(info)を中に持つ
# 印は置いたコンテナが途中で終わると残るので、決めた分(stale_min・lock_stale_min)より古い物は無いものと見る。
# 古い saving/ を見つけたら、restore も save も預かっていた索引を戻してから外す(前は索引を消したまま
# 止まると、次の save が終わるまで誰も戻せず、restore は何も言わずに終わっていた。2026-10-26)。
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
# 組の種類: debug は全体の写し、deps は依存だけの写し。索引・欠片の置き場・書く錠を種類ごとに分ける
case "${1:-}" in
*-deps) kind=deps ;;
*) kind=debug ;;
esac
info=$dir/$kind-linux.txt
readers=$dir/readers
lock=$dir/saving
[ "$kind" = debug ] || lock=$dir/saving-$kind
part_mb=256
margin_gb=10
# restore は 3〜5 分(2026-10-23)。これより古い印は終わったコンテナが残した物
stale_min=${JC_CACHE_STALE_MIN:-30}
# save は生きている間 1 分ごとに saving/beat を触る。これより長く触られていなければ持ち主は終わっている
lock_stale_min=${JC_CACHE_LOCK_STALE_MIN:-10}
# beat を持たない前の版の save は 10 分ほどかかるので、その印は長めに見る
old_lock_stale_min=90
# save が読み終わりを待つ長さ。過ぎたら前の組を消さずに置くのをやめる
wait_s=${JC_CACHE_WAIT_S:-900}
cd "$(dirname "$0")/.."
hack_sum() { sha256sum crates/kmb-workspace-hack/Cargo.toml | cut -c1-16; }
lock_sum() { sha256sum Cargo.lock | cut -c1-16; }
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
    # 依存だけの組は incremental(自前の crate の途中結果)も置かない
    [ "$kind" = debug ] || echo debug/incremental
}
# 古い印を消す
drop_stale() { [ -d "$1" ] && find "$1" -mindepth 1 -maxdepth 1 -mmin +"$stale_min" -exec rm -rf {} + 2>/dev/null || true; }
# 今の時刻から何分前に変えたか
age_min() { echo $(( ($(date +%s) - $(stat -c %Y "$1")) / 60 )); }
# saving/ の持ち主と、最後に生きていた時(分)
lock_desc() {
    b=$lock/beat
    [ -e "$b" ] || b=$lock
    echo "持ち主 $(cat "$lock/owner" 2>/dev/null || echo 不明)、最後に動いたのは $(age_min "$b") 分前"
}
# saving/ が終わったコンテナの残した物なら、預かっていた前の組の索引を戻してから外す。外したら 0 を返す。
# 共有フォルダでは、別のコンテナが作ったフォルダの名前を変えると中がすぐ読めない(2026-10-26 に見た)ので、
# 名前を変えて奪わず、外す直前に持ち主をもう一度読み、別の save が置き直していれば触らない
take_stale_lock() {
    [ -d "$lock" ] || return 1
    if [ -e "$lock/beat" ]; then
        [ -n "$(find "$lock/beat" -mmin +"$lock_stale_min")" ] || return 1
    else
        [ -n "$(find "$lock" -maxdepth 0 -mmin +"$old_lock_stale_min")" ] || return 1
    fi
    who=$(cat "$lock/owner" 2>/dev/null || true)
    desc=$(lock_desc)
    held=$(sed -n 's/^parts: //p' "$lock/info" 2>/dev/null || true)
    [ -n "$who" ] && [ "$(cat "$lock/owner" 2>/dev/null || true)" = "$who" ] || return 1
    if [ -n "$held" ] && [ -d "$dir/$held" ] && [ ! -e "$info" ] && mv "$lock/info" "$info" 2>/dev/null; then
        echo "止まった save の印を外し、預かっていた前の組($held)の索引を戻しました($desc)"
    else
        echo "止まった save の印を外しました($desc)"
    fi
    rm -rf "$lock"
    return 0
}
mark=""
staging=target/.debug-restoring
owner_line=""
beat_pid=""
cleanup() {
    [ -n "$beat_pid" ] && kill "$beat_pid" 2>/dev/null
    [ -n "$mark" ] && rm -f "$mark"
    [ -d "$staging" ] && rm -rf "$staging"
    [ -n "$owner_line" ] && [ -d "$lock" ] && [ "$(cat "$lock/owner" 2>/dev/null)" = "$owner_line" ] && rm -rf "$lock"
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
restore | restore-deps)
    if [ "$kind" = debug ] && [ -d target/debug ]; then
        echo "target/debug があるので戻しません"
        exit 0
    fi
    restored=target/debug/.deps-restored
    rm -rf "$staging"
    # 組の名前を読み、読む印を置いてから、名前が変わっていないかをもう一度見る。save は debug-linux.txt を
    # 外してから印を見るので、もう一度見た時に名前が残っていれば、その組は読み終わるまで消されない
    # 索引が無いのは、save が置き直している途中か、置き直す途中で止まったか、まだ置いていないか
    if [ ! -e "$info" ] && [ -d "$lock" ] && ! take_stale_lock; then
        echo "別のコンテナが置き直している途中なので戻しません($(lock_desc))"
        exit 0
    fi
    parts=$(sed -n 's/^parts: //p' "$info" 2>/dev/null || true)
    if [ -z "$parts" ] || [ ! -d "$dir/$parts" ]; then
        if ls -d "$dir"/$kind-linux-* >/dev/null 2>&1; then
            echo "置いた組の索引(debug-linux.txt)が無いので戻しません(前の save が途中で止まった残り。次の save が置き直します)"
        else
            echo "置いた target がまだありません($dir)"
        fi
        exit 0
    fi
    if [ "$kind" = deps ] && [ "$(cat "$restored" 2>/dev/null)" = "$parts" ]; then
        echo "依存だけの組($parts)はこの target に戻してあります"
        exit 0
    fi
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
    if [ "$kind" = deps ]; then
        saved_lock=$(printf '%s\n' "$snap" | sed -n 's/^lock: //p')
        if [ "$saved_hack" != "$(hack_sum)" ] || [ "$saved_lock" != "$(lock_sum)" ]; then
            echo "依存(Cargo.lock か揃えた依存)が置いた時と違うので、合わない物は作り直します" \
                "(cargo xtask warm の後に $0 save-deps で置き直せます)"
        fi
    elif [ -n "$saved_hack" ] && [ "$saved_hack" != "$(hack_sum)" ]; then
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
    if [ -d target/debug ]; then
        # 依存だけの組を今の target に足す。同じ名前の物は同じ指紋なので、今ある方を残す
        cp -a -l --update=none "$staging/debug/." target/debug/
    else
        mv "$staging/debug" target/debug
    fi
    rm -rf "$staging"
    [ "$kind" = debug ] || echo "$parts" >"$restored"
    echo "target/debug を戻しました($(( $(date +%s) - start )) 秒、$(du -sh target/debug | cut -f1)、残りの空き $(df -Pk . | awk 'NR==2{printf "%.1f", $4/1e6}') GB)"
    printf '%s\n' "$snap" | sed 's/^/  /'
    ;;
save | save-deps)
    [ -d target/debug ] || { echo "target/debug がありません"; exit 1; }
    start=$(date +%s)
    mkdir -p "$dir"
    # 今の版が使わない古い成果物と incremental の前の回を除いてから固める(docs/verify.md の 8 節)。
    # sweep は足りない物をワークスペース全体で作るので、下の層だけを組んだ依存だけの組では流さない
    if [ "$kind" = debug ]; then
        cargo xtask sweep --yes >/dev/null 2>&1 || echo "sweep に失敗(そのまま固めます)"
    fi
    # 共有フォルダはプロジェクトで 10 GiB まで(2026-10-23 は 9 GiB 使っていた)。前の組(4.6 GB)を残したまま
    # 新しい組を書くと上限に当たるので、足りるかを先に確かめ、読み終わるのを待って前の組を消してから書く。
    # 消してから書き終わるまでの間に restore したコンテナは戻さず、一から組み立てるだけ
    old_kb=0
    for old in "$dir"/$kind-linux-* "$dir"/$kind-linux.tar.zst; do
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
    take_stale_lock || true
    if ! mkdir "$lock" 2>/dev/null; then
        echo "別のコンテナが置いている途中なので置きません($(lock_desc))"
        exit 0
    fi
    # コンテナの名前はどれも同じ(vm)なので、持ち主は番号と時刻と乱数で見分ける
    owner_line="$$@$(hostname) $(date -u +%Y-%m-%dT%H:%MZ) $(cat /proc/sys/kernel/random/uuid)"
    echo "$owner_line" >"$lock/owner"
    : >"$lock/beat"
    # このプロセスが消えたか、印が別の持ち主の物になったら触るのをやめる(出力の管を握らない)
    (trap - EXIT; while sleep 60; do
        kill -0 $$ 2>/dev/null && [ "$(cat "$lock/owner" 2>/dev/null)" = "$owner_line" ] || exit 0
        touch "$lock/beat"
    done) </dev/null >/dev/null 2>&1 &
    beat_pid=$!
    # 新しい restore を始めさせないよう、先に debug-linux.txt を saving/ へ預けてから、読んでいる途中の物を
    # 待つ。止まっても、印が古くなれば次の restore か save が預けた索引を戻す
    [ -e "$info" ] && mv "$info" "$lock/info"
    waited=0
    while :; do
        drop_stale "$readers"
        busy=$(ls "$readers" 2>/dev/null | grep -c "^$kind-linux-" || true)
        [ "$busy" -eq 0 ] && break
        if [ "$waited" -ge "$wait_s" ]; then
            [ -e "$lock/info" ] && mv "$lock/info" "$info"
            echo "前の組を読んでいるコンテナが ${wait_s} 秒たっても終わらないので置きません(前の組はそのまま)"
            exit 1
        fi
        [ "$waited" -eq 0 ] && echo "前の組を読んでいるコンテナ($busy)が終わるのを待ちます"
        sleep 10
        waited=$(( waited + 10 ))
    done
    # ここから前の組を消すので、預けた索引も捨てる
    rm -f "$lock/info"
    for old in "$dir"/$kind-linux-* "$dir"/$kind-linux.tar.zst; do
        if [ -e "$old" ]; then rm -rf "$old"; fi
    done
    parts=$kind-linux-$(date -u +%Y%m%d%H%M%S)
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
        echo "lock: $(lock_sum)"
    } >"$info"
    echo "target/debug を置きました($(( $(date +%s) - start )) 秒)"
    sed 's/^/  /' "$info"
    ;;
*)
    echo "使い方: $0 restore|save|restore-deps|save-deps" >&2
    exit 2
    ;;
esac
