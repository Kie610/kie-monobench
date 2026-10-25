#!/bin/bash
# 実際のアプリを立てたまま、外からマウスとキーで 1 手ずつ操作する(人の手順での点検向け)。
# drive.sh は手順を最後まで流すだけだが、こちらは 1 手ごとに写真を見て次を決められる。
# 台の番号ごとに別の画面(DISPLAY :80+番号)と別の HOME を使うので、何台でも同時に動かせる。
#
# 使い方: scripts/ui-shot/session.sh <台の番号> <操作> [引数...]
#   start [アプリの引数...]  … 画面とアプリを立てる(窓は SESS_SIZE に広げる)
#   shot 名前               … 画面全体を <SESS_OUT>/<番号>/名前.png に撮り、そのパスを出す
#   move x y / click x y / dclick x y / rclick x y / mclick x y
#   drag x1 y1 x2 y2        … 左ボタンを押したまま動かす(rdrag は右、mdrag はホイール押し)
#   wheel x y 回数          … 正なら奥へ(拡大)、負なら手前へ
#   key キー名 [...]        … xdotool のキー名(例 Return Escape ctrl+z shift+Tab)
#   type 文字               … 文字を打つ
#   hold 修飾キー click x y … 修飾キーを押したままクリック(例 hold ctrl click 100 200)
#   focus                   … キーの焦点をアプリへ戻す(ダイアログを閉じた後に要る。窓の管理がないため)
#   log [行数]              … アプリのログの終わり
#   stop                    … アプリと画面を止める
# 環境変数: SESS_OUT(既定 ./.local/ui-session)、SESS_SIZE(既定 1600x1000)、SESS_SCALE(既定 1)、
#   SESS_THEME(light / dark)、PAUSE(1 手の後に待つ秒、既定 1.2)、WAIT(起動を待つ秒、既定 15)、
#   APP(既定 target/debug/kie-monobench)
# 要るもの: drive.sh と同じ(Xvfb、xdotool、ImageMagick、mesa-vulkan-drivers、libxkbcommon-x11-0)
set -u
N=$1; CMD=$2; shift 2
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
APP=${APP:-$ROOT/target/debug/kie-monobench}
OUT=${SESS_OUT:-$ROOT/.local/ui-session}/$N
mkdir -p "$OUT"
export DISPLAY=:$((80 + N))
export HOME=$OUT/home
# OS のファイルダイアログ(rfd → zenity)は UTF-8 の言語設定でないと日本語のタイトルを拒んで出ない
export LANG=${LANG:-C.UTF-8}
case $LANG in *UTF-8*|*utf8*) ;; *) export LANG=C.UTF-8 ;; esac
P=${PAUSE:-1.2}
win() { xdotool search --name kieMonoBench 2>/dev/null | head -1; }
case $CMD in
start)
  mkdir -p "$HOME/.local/share/kie-monobench"
  echo "{\"brightness\":\"${SESS_THEME:-light}\"}" > "$HOME/.local/share/kie-monobench/layout.json"
  SIZE=${SESS_SIZE:-1600x1000}
  Xvfb "$DISPLAY" -screen 0 "${SIZE}x24" >/dev/null 2>&1 & echo $! > "$OUT/xvfb.pid"
  sleep 1
  WGPU_BACKEND=vulkan VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
    WINIT_X11_SCALE_FACTOR=${SESS_SCALE:-1} JC_LOG=${JC_LOG:-info} \
    "$APP" "$@" > "$OUT/app.log" 2>&1 & echo $! > "$OUT/app.pid"
  sleep "${WAIT:-15}"
  W=$(win)
  [ -z "$W" ] && { echo "窓が見つからない(ログ: $OUT/app.log)"; exit 1; }
  xdotool windowmove "$W" 0 0 windowsize "$W" "${SIZE%x*}" "${SIZE#*x}"; sleep 2
  # 窓の管理がない Xvfb では、キーがアプリに届くよう焦点を移す
  for w in $(xdotool search --name kieMonoBench); do xdotool windowfocus "$w" 2>/dev/null; done
  echo "台 $N を起動した(DISPLAY $DISPLAY、写真 $OUT)" ;;
shot) import -window root "$OUT/$1.png"; echo "$OUT/$1.png" ;;
move) xdotool mousemove "$1" "$2"; sleep "$P" ;;
click) xdotool mousemove "$1" "$2"; sleep 0.3; xdotool click 1; sleep "$P" ;;
dclick) xdotool mousemove "$1" "$2"; sleep 0.3; xdotool click --repeat 2 --delay 80 1; sleep "$P" ;;
rclick) xdotool mousemove "$1" "$2"; sleep 0.3; xdotool click 3; sleep "$P" ;;
mclick) xdotool mousemove "$1" "$2"; sleep 0.3; xdotool click 2; sleep "$P" ;;
drag|rdrag|mdrag)
  B=1; [ "$CMD" = rdrag ] && B=3; [ "$CMD" = mdrag ] && B=2
  # 速すぎるとスケッチの点を掴めないので、押して待ち、20 歩に分けて動かす
  xdotool mousemove "$1" "$2"; sleep 0.3; xdotool mousedown $B; sleep 0.2
  for i in $(seq 1 20); do
    xdotool mousemove $(( $1 + ($3 - $1) * i / 20 )) $(( $2 + ($4 - $2) * i / 20 )); sleep 0.05
  done
  sleep 0.2; xdotool mouseup $B; sleep "$P" ;;
wheel)
  xdotool mousemove "$1" "$2"; sleep 0.3
  C=$3; B=4; [ "$C" -lt 0 ] && { B=5; C=$(( -C )); }
  xdotool click --repeat "$C" --delay 60 $B; sleep "$P" ;;
key) for k in "$@"; do xdotool key "$k"; sleep 0.3; done; sleep "$P" ;;
type) xdotool type --delay 40 -- "$1"; sleep "$P" ;;   # -- で「-5」などをオプションと読ませない
focus) for w in $(xdotool search --name kieMonoBench); do xdotool windowfocus "$w" 2>/dev/null; done ;;
hold)
  M=$1; shift; xdotool keydown "$M"; sleep 0.2
  "$0" "$N" "$@"; xdotool keyup "$M"; sleep 0.3 ;;
log) tail -n "${1:-40}" "$OUT/app.log" ;;
stop) kill "$(cat "$OUT/app.pid")" 2>/dev/null; sleep 0.5; kill "$(cat "$OUT/xvfb.pid")" 2>/dev/null; echo "台 $N を止めた" ;;
*) echo "知らない操作: $CMD"; exit 2 ;;
esac
