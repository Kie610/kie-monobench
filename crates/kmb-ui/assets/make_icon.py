"""仮のアイコンを作る(製品名が決まるまでの物。名前の字は使わない)。

濃い紺の角丸の地に、水色の立方体(スケッチの色)と、上の面を輪郭に沿って削る橙の経路を描く。
大きさごとに 4 倍で描いてから縮めるので、16 px でも線がつぶれにくい。

    python3 crates/kmb-ui/assets/make_icon.py

icon.png(256 px、窓のアイコン)と icon.ico(16〜256 px、exe のアイコンと上の帯の左上の印)を書き直す。Pillow が要る。
"""

from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
SS = 4  # 描く時の倍率

BG = (31, 42, 58, 255)
TOP = (150, 214, 245, 255)
LEFT = (74, 160, 214, 255)
RIGHT = (44, 116, 172, 255)
EDGE = (232, 246, 255, 255)
PATH = (255, 150, 40, 255)


def draw(size: int) -> Image.Image:
    n = size * SS
    im = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    u = n / 256  # 256 の目盛りで座標を書く

    def p(x: float, y: float) -> tuple[float, float]:
        return (x * u, y * u)

    d.rounded_rectangle([p(8, 8), p(248, 248)], radius=48 * u, fill=BG)
    # 立方体(等角)
    top, bot = 40, 220
    l, r = 48, 208
    ft = [p(128, top), p(r, 86), p(128, 132), p(l, 86)]  # 上の面
    fl = [p(l, 86), p(128, 132), p(128, bot), p(l, 174)]  # 左の面
    fr = [p(128, 132), p(r, 86), p(r, 174), p(128, bot)]  # 右の面
    d.polygon(fl, fill=LEFT)
    d.polygon(fr, fill=RIGHT)
    d.polygon(ft, fill=TOP)
    w = max(1, round((3 if size >= 48 else 6 if size >= 24 else 9) * u))
    for face in (ft, fl, fr):
        d.line(face + [face[0]], fill=EDGE, width=w, joint="curve")
    # 上の面を削る経路(輪郭に沿って内へ回るポケット。大きい絵は 2 周、小さい絵は 1 周)
    def on_top(a: float, b: float) -> tuple[float, float]:
        # a: 奥→右の向き、b: 奥→左の向き(0〜1)
        x = 128 + (r - 128) * a - (128 - l) * b
        y = top + (86 - top) * a + (86 - top) * b
        return p(x, y)

    pw = max(1, round((7 if size >= 48 else 11 if size >= 24 else 14) * u))
    for k in ([0.2, 0.4] if size >= 32 else [0.25]):
        loop = [on_top(k, k), on_top(1 - k, k), on_top(1 - k, 1 - k), on_top(k, 1 - k)]
        d.line(loop + [loop[0]], fill=PATH, width=pw, joint="curve")
    return im.resize((size, size), Image.LANCZOS)


def main() -> None:
    images = {s: draw(s) for s in SIZES}
    images[256].save(HERE / "icon.png")
    images[256].save(
        HERE / "icon.ico",
        sizes=[(s, s) for s in SIZES],
        append_images=[images[s] for s in SIZES if s != 256],
    )
    print("icon.png と icon.ico を書きました")


if __name__ == "__main__":
    main()
