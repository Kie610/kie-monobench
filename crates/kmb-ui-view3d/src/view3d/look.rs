//! 色と目安: 道具の立体の重ね塗り、画面の色、格子と原点の平面、経路とスケッチの線の色。

use super::*;

/// 形に重ねて塗る道具の立体(押し出しの見本など。結合は材料の色、
/// 切り取りは赤、交差は黄)。
#[derive(Clone, Debug, PartialEq)]
pub struct Ghost {
    /// 三角形(外から見て左回り)
    pub tris: Vec<[[f32; 3]; 3]>,
    /// 立体の稜線
    pub edges: Vec<[[f32; 3]; 2]>,
    /// 面の色。透明度が 255 未満なら透かして塗る
    pub color: Color32,
    pub edge_color: Color32,
    pub layer: GhostLayer,
}

/// 道具の立体をどう重ねるか。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GhostLayer {
    /// ボディと同じく奥行きを見て不透明に塗る(結合・新しいボディ: できる形そのもの)
    Solid,
    /// ボディの上に透かして重ねる(切り取り・交差: ボディの中の削る所・残る所も透けて見える。
    /// 交差の黄色は結果の上にもかかる)
    Over,
}

impl Ghost {
    /// 見た目が変わったかを比べる値。
    pub fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for t in &self.tris {
            t.as_flattened()
                .iter()
                .for_each(|v| v.to_bits().hash(&mut h));
        }
        for e in &self.edges {
            e.as_flattened()
                .iter()
                .for_each(|v| v.to_bits().hash(&mut h));
        }
        (
            self.color.to_array(),
            self.edge_color.to_array(),
            self.layer,
        )
            .hash(&mut h);
        h.finish()
    }
}

/// 本体の奥では薄い点線で描く線と面(スケッチの線・作業平面。Q6 v1)。手前に見える所は今の線のまま
/// 描き、本体に隠れた所は隠れた辺と同じ刻みと濃さの点線にする。面は手前に見える所だけ塗る。
/// 表示の「全部透かす」を入れた時は使わず、今まで通り形の上に重ねる。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Behind {
    /// 線分の両端・色・太さ(point)
    pub lines: Vec<([[f32; 3]; 2], Color32, f32)>,
    /// 半透明に塗る三角形と色(透明度で混ぜる)
    pub tris: Vec<([[f32; 3]; 3], Color32)>,
}

/// 3D のビューのスケッチの線の太さ(point)と、作業平面の縁の太さ。
pub const SKETCH_WIDTH: f32 = 1.6;
pub const PLANE_EDGE_WIDTH: f32 = 1.0;
/// 作業平面の塗りの濃さ(縁の色に掛ける)。
pub const PLANE_FILL: f32 = 0.16;

impl Behind {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.tris.is_empty()
    }

    /// 重ねる物のうち、スケッチの線(2 点以上の作図の線)と作業平面を取り出す。
    pub fn from_overlay(overlay: &kmb_core::Overlay, colors: &Colors) -> Behind {
        let f32s = |p: [f64; 3]| p.map(|v| v as f32);
        let mut b = Behind::default();
        for (pts, _) in overlay.lines.iter().filter(|(_, k)| behind_line(k.ink)) {
            for w in pts.windows(2) {
                b.lines
                    .push(([f32s(w[0]), f32s(w[1])], colors.sketch, SKETCH_WIDTH));
            }
        }
        for pl in overlay.planes.iter().filter(|p| behind_plane(p.ink)) {
            let c = pl.corners.map(f32s);
            let fill = colors.machine.gamma_multiply(PLANE_FILL);
            b.tris.push(([c[0], c[1], c[2]], fill));
            b.tris.push(([c[0], c[2], c[3]], fill));
            for k in 0..4 {
                b.lines
                    .push(([c[k], c[(k + 1) % 4]], colors.machine, PLANE_EDGE_WIDTH));
            }
        }
        b
    }

    /// 見た目が変わったかを比べる値。
    pub fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (e, c, w) in &self.lines {
            e.as_flattened()
                .iter()
                .for_each(|v| v.to_bits().hash(&mut h));
            (c.to_array(), w.to_bits()).hash(&mut h);
        }
        for (t, c) in &self.tris {
            t.as_flattened()
                .iter()
                .for_each(|v| v.to_bits().hash(&mut h));
            c.to_array().hash(&mut h);
        }
        h.finish()
    }
}

/// 本体の奥で点線にする線の役割(スケッチの作図の線)。
pub(crate) fn behind_line(ink: kmb_core::LineInk) -> bool {
    ink == kmb_interchange::ink::line::CONSTRUCTION
}

/// 本体の奥で点線にする面の役割(作業平面)。
pub(crate) fn behind_plane(ink: kmb_core::PlaneInk) -> bool {
    ink == kmb_interchange::ink::plane::DATUM
}

/// 3D のビューの色(画面の色の一揃いから作る)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    /// 背景(上の端の色。下へ `background_low` まで変える)
    pub background: Color32,
    pub background_low: Color32,
    pub body: Color32,
    pub picked: Color32,
    pub hover: Color32,
    pub edge: Color32,
    /// 既定の色のボディの稜線([`line_color`]。ボディごとの色はボディの色から作る)
    pub line: Color32,
    /// 暗い画面か(稜線の色の寄せ方)
    pub dark: bool,
    /// 経路(削る所)・早送り・案内の線・ストック・作業範囲
    pub cut: Color32,
    pub rapid: Color32,
    pub guide: Color32,
    pub stock: Color32,
    pub envelope: Color32,
    pub machine: Color32,
    /// 加工の途中の形(材料の色)
    pub shape: Color32,
    /// スケッチの線(水色。ボディの白・灰色と一目で見分ける)
    pub sketch: Color32,
    /// 経路の入り(らせん・斜め・真下)、リード、つなぎ
    pub entry: Color32,
    pub lead: Color32,
    pub link: Color32,
    /// 意味の名前を付けた色(軸・道具の立体・シミュレーションの印・加工の再生の色の役)
    pub inks: Inks,
}

/// 画面の色の一揃いのうち、3D のビューの色の元にする色(画面のテーマが作って渡す)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub canvas: Color32,
    pub ink: Color32,
    pub ink2: Color32,
    pub ink3: Color32,
    pub err: Color32,
    pub warn: Color32,
    /// 設計の作業空間の色(経路の削る所)
    pub design: Color32,
    /// 加工の作業空間の色(加工の途中の形)
    pub cam: Color32,
    pub shade3: Color32,
    /// 今の作業空間の差し色(選んだ面・マウスの下の面)
    pub accent: Color32,
    /// 意味の名前を付けた色(テーマが明暗ごとに持つ)
    pub inks: Inks,
}

impl Colors {
    pub fn from_look(l: impl Into<Palette>) -> Self {
        let p = l.into();
        // ボディは色味のない白・灰色(スケッチの水色、選んだ色と見分ける)
        let body = p.inks.body;
        let edge = crate::mix(p.ink2, p.ink, 0.3);
        Colors {
            background: p.canvas,
            // 明るい画面は下へ少し暗くする(ボディの白・灰色が背景に溶けない)
            background_low: if p.dark {
                p.canvas
            } else {
                crate::mix(p.canvas, p.ink2, 0.3)
            },
            body,
            picked: crate::mix(body, p.accent, 0.55),
            hover: crate::mix(body, p.accent, 0.22),
            edge,
            line: line_color(body, p.dark),
            dark: p.dark,
            cut: p.design,
            rapid: p.err.gamma_multiply(0.7),
            guide: p.ink3,
            stock: p.warn,
            envelope: p.ink3.gamma_multiply(0.6),
            machine: crate::mix(p.ink3, p.ink2, 0.4),
            shape: crate::mix(p.cam, p.shade3, 0.45),
            sketch: p.inks.sketch_line,
            entry: p.inks.path_entry,
            lead: p.inks.path_lead,
            link: crate::mix(p.ink2, p.ink3, 0.5),
            inks: p.inks,
        }
    }
}

/// 形の下に描く目安(格子・原点の軸)。設定の「表示」で替える。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Guides {
    /// XY 平面(z = 0)の格子。間隔は拡大に合わせて 1・2・5 の刻みで変える
    pub grid: bool,
    /// 原点を通る X・Y・Z の軸
    pub axes: bool,
}

impl Guides {
    pub fn any(self) -> bool {
        self.grid || self.axes
    }
}

/// 格子の細かい線の間隔(mm)。画面で `min_px` 画素以上あくよう、1・2・5 × 10^k から選ぶ。
pub fn grid_step(px_per_mm: f32, min_px: f32) -> f32 {
    let raw = (min_px / px_per_mm.max(1e-6)).max(1e-3);
    let p = 10f32.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|k| k * p)
        .find(|v| *v >= raw * 0.999)
        .unwrap_or(10.0 * p)
}

/// ビューの左下に出す格子の間隔の札(字は [`crate::prop`] の 10.5)。
pub fn grid_label(px_per_mm: f32) -> String {
    let step = grid_step(px_per_mm, 14.0);
    let fmt = |v: f32| {
        if v >= 1.0 {
            format!("{v:.0}")
        } else {
            format!("{v}")
        }
    };
    format!("格子 {} mm(太線 {} mm)", fmt(step), fmt(grid_major(step)))
}

/// 太い線の間隔(細かい線の 5 本か 10 本ごと。2 の刻みなら 5 本で 10 の刻みにそろえる)。
pub fn grid_major(step: f32) -> f32 {
    let lead = step / 10f32.powf(step.log10().floor());
    if (lead - 2.0).abs() < 0.01 {
        step * 5.0
    } else {
        step * 10.0
    }
}

/// 3D スケッチの作図面の四角の大きさの元(mm)。画面の短い辺の 2 割ほどを、きりのよい値に丸める。
pub fn guide_size(cam: &Camera, rect: Rect) -> f32 {
    let raw = 0.2 * rect.width().min(rect.height()) / cam.zoom.max(1e-6);
    let p = 10f32.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|k| k * p)
        .rfind(|v| *v <= raw * 1.001)
        .unwrap_or(p)
}

/// 原点の平面の一辺と原点の軸の、画面の上の長さ(論理画素)。原点の平面は、拡大・縮小
/// しても窓の大きさを変えても画面の上の大きさが変わらない。
pub const ORIGIN_PX: f32 = 110.0;

/// 原点の平面の一辺と原点の軸の長さ(mm)。原点の奥行きで画面の上の [`ORIGIN_PX`] になる長さ。
pub fn origin_size(cam: &Camera, rect: Rect) -> f32 {
    let (r, _, _) = cam.basis();
    let o = cam.project([0.0; 3], rect).0;
    let p = cam.project(r, rect).0;
    ORIGIN_PX / o.distance(p).max(1e-6)
}

/// 原点の平面(名前と四隅)。軸の正の向きへ一辺 `size`(mm)の正方形を、原点と
/// 軸から [`kmb_core::Plane::ORIGIN_GAP`] だけ浮かせて置く(選ぶ範囲も同じ。kmb-sketch の平面の候補)。
pub fn base_planes(size: f32) -> [(&'static str, [[f32; 3]; 4]); 3] {
    let a = kmb_core::Plane::ORIGIN_GAP as f32 * size;
    let b = a + size;
    [
        (
            "XY 平面",
            [[a, a, 0.0], [b, a, 0.0], [b, b, 0.0], [a, b, 0.0]],
        ),
        (
            "YZ 平面",
            [[0.0, a, a], [0.0, b, a], [0.0, b, b], [0.0, a, b]],
        ),
        (
            "ZX 平面",
            [[a, 0.0, a], [a, 0.0, b], [b, 0.0, b], [b, 0.0, a]],
        ),
    ]
}

/// 原点の平面を、灰色 `gray` の半透明の四角と名前で描く(選べない目安)。
/// 真横から見て潰れた面は描かない。名前は平面に貼った字で、平面の外の角に寄せる(N6)。
pub fn origin_planes(painter: &egui::Painter, cam: &Camera, rect: Rect, gray: Color32) {
    origin_planes_marked(painter, cam, rect, gray, None);
}

/// [`origin_planes`] と同じに描き、`marked`(平面の番号 XY・YZ・ZX の 0・1・2 と色)の平面だけを
/// その色で塗る(機能の欄で選んでいる原点の平面)。
pub fn origin_planes_marked(
    painter: &egui::Painter,
    cam: &Camera,
    rect: Rect,
    gray: Color32,
    marked: Option<(usize, Color32)>,
) {
    let size = origin_size(cam, rect);
    for (k, ((name, q), (ux, uy))) in base_planes(size)
        .into_iter()
        .zip(PLANE_TEXT_AXES)
        .enumerate()
    {
        let (fill, edge, ink) = match marked {
            Some((m, c)) if m == k => (c.gamma_multiply(0.38), egui::Stroke::new(2.0, c), c),
            _ => (
                gray.gamma_multiply(0.22),
                egui::Stroke::new(1.0, gray),
                gray,
            ),
        };
        let pts: Vec<Pos2> = q.iter().map(|p| cam.project(*p, rect).0).collect();
        let area =
            (pts[1] - pts[0]).x * (pts[3] - pts[0]).y - (pts[1] - pts[0]).y * (pts[3] - pts[0]).x;
        if area.abs() < 400.0 {
            continue;
        }
        painter.add(egui::Shape::convex_polygon(pts.clone(), fill, edge));
        // 外の角(原点から遠い角)の内側に、右上をそろえて置く
        let far = q[2];
        let pad = PLANE_TEXT_PAD * size;
        let corner = [0, 1, 2].map(|i| far[i] - (ux[i] + uy[i]) * pad);
        plane_text(
            painter,
            cam,
            rect,
            name,
            corner,
            (ux, uy),
            PLANE_TEXT_HEIGHT * size,
            ink,
        );
    }
}

/// 原点の平面の名前の字の向き(右へ進む向き、字の上の向き)。`base_planes` の順(XY・YZ・ZX)。
/// その平面を正面から見る視点(上・右・前のボタン)で、普通に読める向き。
pub const PLANE_TEXT_AXES: [([f32; 3], [f32; 3]); 3] = [
    ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
];
/// 原点の平面の名前の字の高さ(平面の一辺に対する割合。拡大しても平面と一緒に大きさが変わる)。
pub const PLANE_TEXT_HEIGHT: f32 = 0.2;
/// 名前を平面の角から離す量(平面の一辺に対する割合)。
pub(super) const PLANE_TEXT_PAD: f32 = 0.06;

/// 字 `text` を、空間の平面に貼った字として描く。`top_right` は字の右上の角、`axes` は字の右へ進む
/// 向きと上の向き(どちらも長さ 1)、`height` は字の高さ(mm)。egui の字の網を作り、点を 1 つずつ
/// 空間へ置いて画面へ写す(字は小さいので、透視投影でも点ごとに写せば足りる)。
#[allow(clippy::too_many_arguments)]
pub fn plane_text(
    painter: &egui::Painter,
    cam: &Camera,
    rect: Rect,
    text: &str,
    top_right: [f32; 3],
    axes: ([f32; 3], [f32; 3]),
    height: f32,
    color: Color32,
) {
    let galley = painter.layout_no_wrap(text.to_string(), crate::prop(18.0), color);
    let (w, h) = (galley.size().x, galley.size().y.max(1e-3));
    let k = height / h;
    let (ux, uy) = axes;
    let ctx = painter.ctx();
    let shape = egui::epaint::ClippedShape {
        clip_rect: Rect::EVERYTHING,
        shape: egui::Shape::galley(Pos2::ZERO, galley, color),
    };
    for prim in ctx.tessellate(vec![shape], ctx.pixels_per_point()) {
        let egui::epaint::Primitive::Mesh(mut mesh) = prim.primitive else {
            continue;
        };
        for v in &mut mesh.vertices {
            // 字の網の位置(左上が 0、下へ増える)を、右上の角からの平面の上の位置へ
            let (x, y) = ((v.pos.x - w) * k, -v.pos.y * k);
            let world = [0, 1, 2].map(|i| top_right[i] + ux[i] * x + uy[i] * y);
            v.pos = cam.project(world, rect).0;
        }
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// 道具の立体を塗る色(交差は黄)。線と札の色(`Look::tool_op`)は
/// 字として読みやすい濃さだが、面は透かして重ねるので明るく鮮やかな色にする。
/// 色と透かし具合は見本の画面の画素から求めた:
/// 暗い地(42,42,42)の上で切り取りは (145,44,42) 前後、交差は (127,128,26) 前後なので、
/// 赤はおよそ (255,45,45) を 48 %、黄はおよそ (255,255,0) を 40 % 重ねた色になる。
pub const GHOST_CUT: Color32 = Color32::from_rgb(255, 45, 45);
pub const GHOST_INTERSECT: Color32 = Color32::from_rgb(255, 255, 0);
/// 1 枚の面を透かす割合(0〜255)。道具の立体は表と裏の 2 枚が重なって見え、面には弱い陰も付くので、
/// 暗い地の上で狙った色(切り取り (148,46,44) 前後、交差 (127,128,26) 前後)に
/// なるよう測って決めた(2 枚重ねて切り取り約 56 %、交差約 44 %)。1 枚で 48 % にすると 7 割ほど濃くなった。
pub const GHOST_CUT_ALPHA: u8 = 86;
pub const GHOST_INTERSECT_ALPHA: u8 = 64;
/// 結合の道具の立体の青(切り取りの赤と同じ透かし具合で重ねる)と、その稜線・線・札の青。
/// 新しいボディはボディと同じ灰色で描くので、結合だけを青にして見分ける(Kie 2026-10-25)。
pub const GHOST_JOIN: Color32 = Color32::from_rgb(60, 140, 255);
pub const GHOST_JOIN_ALPHA: u8 = GHOST_CUT_ALPHA;
pub const GHOST_JOIN_EDGE: Color32 = Color32::from_rgb(25, 95, 215);
/// 切り取りの稜線・線・札の赤。失敗の知らせの色(`Palette::err`)と分け、道具の赤にそろえる。
pub const GHOST_CUT_EDGE: Color32 = Color32::from_rgb(215, 35, 35);
/// 交差の稜線。黄の色相のまま暗くして、明るい地でも線が見えるようにする(黄土色にしない)。
pub const GHOST_INTERSECT_EDGE: Color32 = Color32::from_rgb(185, 170, 0);
/// 押し出す輪郭を塗る青。見本の画面で輪郭は赤 (148,46,44) の上で
/// (52,71,147)、黄 (127,128,26) の上で (43,96,136) 前後に見え、両方に合うのは (0,85,205) を 65 % 重ねた色。
pub const PROFILE_BLUE: Color32 = Color32::from_rgb(0, 85, 205);
pub const PROFILE_ALPHA: u8 = 166;

/// 透かす道具の立体の陰の濃さ。透ける立体は面の向きでほとんど明るさを変えない
/// (交差の上の面 (127,128,26) と横の面 (140,141,39))ので、ボディより陰を弱くして色を濁らせない。
pub(super) fn ghost_shade(l: f32) -> f32 {
    if l >= 0.0 { 0.9 + 0.1 * l } else { 0.88 }
}

/// スケッチの線の色(水色)。
/// 経路の入り(赤紫)とリード(黄土)の色。切削の緑、早送りの赤、スケッチの水色と見分ける。
pub const fn path_ink(dark: bool) -> (Color32, Color32) {
    if dark {
        (
            Color32::from_rgb(236, 120, 214),
            Color32::from_rgb(236, 186, 64),
        )
    } else {
        (
            Color32::from_rgb(190, 40, 160),
            Color32::from_rgb(196, 128, 0),
        )
    }
}

pub const fn sketch_ink(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(110, 196, 240)
    } else {
        Color32::from_rgb(24, 150, 214)
    }
}

/// ボディごとの色。1 つ目はそのまま、2 つ目からは白と灰の明るさを変えて、重なったボディを
/// 見分けられるようにする(色味は付けない。スケッチの水色や選んだ色と紛れないように)。
/// 稜線の色 `edge` がボディより明るい時(暗い画面)は、明るくする幅を小さくして暗い側に寄せる
/// (明るくしすぎると明るい稜線と同じ明るさになり、辺と分割したボディの境が見えなかった。M8)。
pub fn body_tint(body: Color32, edge: Color32, index: usize) -> Color32 {
    const SHADES: [(f32, bool); 4] = [(0.55, true), (0.2, false), (0.3, true), (0.32, false)];
    const SHADES_DARK: [(f32, bool); 4] =
        [(0.45, false), (0.12, true), (0.25, false), (0.6, false)];
    if index == 0 {
        return body;
    }
    let luma =
        |c: Color32| 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b());
    let shades = if luma(edge) > luma(body) {
        &SHADES_DARK
    } else {
        &SHADES
    };
    let (k, lighter) = shades[(index - 1) % shades.len()];
    let to = if lighter {
        Color32::WHITE
    } else {
        Color32::from_gray(60)
    };
    crate::mix(body, to, k)
}

/// 色を `a`(0〜255)の濃さで透かす。
pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub(super) fn shade(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| ((v as f32) * k).clamp(0.0, 255.0) as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}
