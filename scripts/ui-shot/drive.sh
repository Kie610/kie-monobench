#!/bin/bash
# 画面の写真を撮る(Linux のクラウドの作業場向け。画面のない所で、仮想の画面と CPU の描画で動かす)。
# 決まった場面は apps/jc-app/tests/screens.rs の方が手軽(使い分けは同じフォルダの README.md)。
#
# 使い方: scripts/ui-shot/drive.sh <手順のファイル> [アプリの引数...]
#   例: SHOT_OUT=out scripts/ui-shot/drive.sh scripts/ui-shot/cam.steps --sample
# 手順のファイルは bash で、次が使える:
#   shot 名前      … 画面全体を <SHOT_OUT>/名前.png に撮る
#   click x y / rclick x y / move x y / key キー名 / type 文字 / wait 秒
# 環境変数:
#   SHOT_OUT   写真の置き場(既定 .)
#   SHOT_HOME  アプリのデータフォルダの親(既定 /tmp/jc-shot-home)。
#              明るさは $SHOT_HOME/.local/share/kie-monobench/layout.json の "brightness"
#   SHOT_THEME light / dark(既定 light。layout.json を書き直す)
#   SHOT_SIZE  画面の大きさ(実際の画素、既定 1280x760)。窓をこの大きさに広げる(最大化と同じ)
#   SHOT_SCALE 表示倍率(既定 1。Windows の 200% なら 2。例: SHOT_SIZE=3840x2068 SHOT_SCALE=2)
#   WAIT       起動を待つ秒(既定 12)
#   APP        実行ファイル(既定 target/debug/kie-monobench。cargo build -p jc-app で作る)
# 要るもの: Xvfb、xdotool、ImageMagick、Mesa の lavapipe(Vulkan の CPU 実装)、libxkbcommon-x11
#   apt-get install -y xvfb xdotool imagemagick mesa-vulkan-drivers libxkbcommon-x11-0 fonts-ipafont-gothic
set -u
STEPS=$1; shift
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
APP=${APP:-$ROOT/target/debug/kie-monobench}
export HOME=${SHOT_HOME:-/tmp/jc-shot-home}
mkdir -p "$HOME/.local/share/kie-monobench"
echo "{\"brightness\":\"${SHOT_THEME:-light}\"}" > "$HOME/.local/share/kie-monobench/layout.json"
export WGPU_BACKEND=vulkan VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
OUTDIR=${SHOT_OUT:-.}; mkdir -p "$OUTDIR"
DISP=:${SHOT_DISPLAY:-78}
SIZE=${SHOT_SIZE:-1280x760}
Xvfb $DISP -screen 0 "${SIZE}x24" >/dev/null 2>&1 & XP=$!
sleep 1
export DISPLAY=$DISP
export WINIT_X11_SCALE_FACTOR=${SHOT_SCALE:-1}
"$APP" "$@" > "$OUTDIR/app.log" 2>&1 & AP=$!
sleep "${WAIT:-12}"
if [ -n "${SHOT_SIZE:-}" ]; then
  WIN=$(xdotool search --name kieMonoBench | head -1)
  xdotool windowmove "$WIN" 0 0 windowsize "$WIN" "${SIZE%x*}" "${SIZE#*x}"
  sleep 3
fi
# 窓の管理がない Xvfb では、文字やキーがアプリに届くよう入力の焦点を移しておく
for w in $(xdotool search --name kieMonoBench 2>/dev/null); do xdotool windowfocus "$w" 2>/dev/null; done
shot() { import -window root "$OUTDIR/$1.png"; }
click() { xdotool mousemove "$1" "$2"; sleep 0.3; xdotool click 1; sleep "${PAUSE:-1.5}"; }
rclick() { xdotool mousemove "$1" "$2"; sleep 0.3; xdotool click 3; sleep "${PAUSE:-1.5}"; }
move() { xdotool mousemove "$1" "$2"; sleep "${PAUSE:-1.0}"; }
key() { xdotool key "$1"; sleep "${PAUSE:-1.5}"; }
type() { xdotool type "$1"; sleep "${PAUSE:-1.0}"; }
wait() { sleep "$1"; }
# shellcheck disable=SC1090
source "$STEPS"
kill $AP; sleep 0.5; kill $XP
