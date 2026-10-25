//! スケッチの図に重ねるもの: 閉じた形の塗りと、寸法の引出し線(補助の線・寸法線・矢印・値)。
//! 試作 v10 のスケッチ画面(外形を薄く塗る、寸法は形から離した線の上に値の札)に合わせる。

use crate::view::Transform;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use jc_sketch::{ConstraintKind as K, Entity, EntityId, Sketch, Solution};

/// 閉じた形の点列(スケッチの平面の座標)。外周の後に穴が続く。
pub(crate) type Loops = Vec<Vec<[f64; 2]>>;

/// スケッチの閉じた領域(押し出しなどに使える単位)。
pub(crate) struct Area {
    /// 領域の名前(押し出しが選んだ領域を覚えるのに使う。寸法を変えても変わらない)
    pub key: String,
    pub loops: Loops,
}

/// スケッチの閉じた形(押し出しなどに使える領域)を、領域ごとの点列にする。
/// 重なった形は交点で分けた一番小さな領域ごと(押し出しで 1 つずつ選べる単位)。
pub(crate) fn regions(sketch: &Sketch, sol: &Solution) -> Vec<Area> {
    let faces = jc_sketch::faces_of(sketch, sol, jc_core::Plane::XY);
    faces
        .faces
        .iter()
        .map(|f| Area {
            key: f.key.clone(),
            loops: std::iter::once(&f.region.outer)
                .chain(&f.region.holes)
                .map(|l| l.polyline())
                .collect(),
        })
        .collect()
}

/// 点 `p` を含む領域のうち、いちばん小さいもの(入れ子の時は内側)の名前。
pub(crate) fn region_at(areas: &[Area], p: [f64; 2]) -> Option<&str> {
    areas
        .iter()
        .filter(|a| contains(&a.loops, p))
        .min_by(|a, b| area_of(&a.loops).total_cmp(&area_of(&b.loops)))
        .map(|a| a.key.as_str())
}

/// 領域の面積(外周から穴を引く)。
pub(crate) fn area_of(loops: &Loops) -> f64 {
    loops
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let a = jc_core::kernel::polygon_area(l).abs();
            if i == 0 { a } else { -a }
        })
        .sum()
}

/// 点が領域の中にあるか(偶奇の規則。穴の中は外)。
pub(crate) fn contains(loops: &Loops, p: [f64; 2]) -> bool {
    let mut inside = false;
    for l in loops {
        let n = l.len();
        for i in 0..n {
            let (a, b) = (l[i], l[(i + 1) % n]);
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
            {
                inside = !inside;
            }
        }
    }
    inside
}

/// 領域を塗る。画面の 1 行ずつ、辺との交点の間を偶奇の規則で埋める(穴は塗らない)。
pub(crate) fn fill(
    painter: &egui::Painter,
    rect: Rect,
    t: &Transform,
    loops: &Loops,
    color: Color32,
) {
    let screen: Vec<Vec<Pos2>> = loops
        .iter()
        .map(|l| l.iter().map(|p| t.to_screen(*p)).collect())
        .collect();
    let (mut top, mut bottom) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in screen.iter().flatten() {
        top = top.min(p.y);
        bottom = bottom.max(p.y);
    }
    let (top, bottom) = (
        top.max(rect.top()).floor(),
        bottom.min(rect.bottom()).ceil(),
    );
    let mut mesh = egui::Mesh::default();
    let mut xs: Vec<f32> = Vec::new();
    let mut y = top;
    while y < bottom {
        let yc = y + 0.5;
        xs.clear();
        for l in &screen {
            let n = l.len();
            for i in 0..n {
                let (a, b) = (l[i], l[(i + 1) % n]);
                if (a.y <= yc) != (b.y <= yc) {
                    xs.push(a.x + (yc - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
        }
        xs.sort_by(f32::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            let (x0, x1) = (pair[0].max(rect.left()), pair[1].min(rect.right()));
            if x1 > x0 {
                mesh.add_colored_rect(
                    Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 1.0)),
                    color,
                );
            }
        }
        y += 1.0;
    }
    painter.add(mesh);
}

/// 寸法の線と値の色。値は地の箱なしで、寸法線の上に線と平行に書く。
pub(crate) struct DimLook<'a> {
    pub line: Color32,
    pub text: Color32,
    /// 参照寸法(線を点線にする)
    pub dashed: bool,
    /// 形の真ん中(画面の座標)。寸法線はそこから離れる側に置く
    pub away: Pos2,
    /// 寸法を置く位置(画面の座標)。あれば寸法線をこの点の側に出す(`away` より優先)
    pub toward: Option<Pos2>,
    /// 既に置いた値の札(重なれば寸法線を外へずらす)
    pub taken: &'a std::cell::RefCell<Vec<Rect>>,
}

/// 形から寸法線までの距離(画面の px)
const OFFSET: f32 = 24.0;
/// 補助の線を寸法線の先へ伸ばす長さ
const OVERSHOOT: f32 = 5.0;
/// 補助の線を形から離す隙間
const GAP: f32 = 3.0;
/// 値の字を寸法線(引出し線の肩)から浮かせる隙間
const LIFT: f32 = 2.0;
/// 値の字の大きさ
const TEXT: f32 = 13.0;

fn pt(sol: &Solution, id: &EntityId) -> Option<[f64; 2]> {
    sol.points.get(id).copied()
}

/// 線分の両端の解いた位置。線分でなければ `None`。
pub(crate) fn line_ends(
    sketch: &Sketch,
    sol: &Solution,
    id: EntityId,
) -> Option<([f64; 2], [f64; 2])> {
    match sketch.entity(id)? {
        Entity::Line { a, b, .. } => Some((pt(sol, a)?, pt(sol, b)?)),
        _ => None,
    }
}

/// 円・円弧の中心、半径、値の札を出す向き(円弧は真ん中、円は右上)。
fn round(sketch: &Sketch, sol: &Solution, id: EntityId) -> Option<([f64; 2], f64, f64)> {
    match sketch.entity(id)? {
        Entity::Circle { center, .. } => Some((
            pt(sol, center)?,
            *sol.radii.get(&id)?,
            std::f64::consts::FRAC_PI_4,
        )),
        Entity::Arc {
            center, start, end, ..
        } => {
            let (c, s, e) = (pt(sol, center)?, pt(sol, start)?, pt(sol, end)?);
            let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
            let span = ((e[1] - c[1]).atan2(e[0] - c[0]) - a0).rem_euclid(std::f64::consts::TAU);
            Some((c, (s[0] - c[0]).hypot(s[1] - c[1]), a0 + span / 2.0))
        }
        _ => None,
    }
}

struct Pen<'a> {
    painter: &'a egui::Painter,
    look: &'a DimLook<'a>,
    /// 最後に描いた値の札(ダブルクリックで値を直す所)
    last: std::cell::Cell<Option<Rect>>,
    /// 寸法線を通す所(画面)。置く位置は字の真ん中なので、線の上に書く寸法では字を浮かせた分
    /// だけ戻した所([`Pen::aim`])。札を掴んでも跳ねないように
    toward: std::cell::Cell<Option<Pos2>>,
}

impl Pen<'_> {
    fn seg(&self, a: Pos2, b: Pos2, width: f32) {
        let stroke = Stroke::new(width, self.look.line);
        if self.look.dashed {
            self.painter
                .add(egui::Shape::dashed_line(&[a, b], stroke, 4.0, 3.0));
        } else {
            self.painter.line_segment([a, b], stroke);
        }
    }

    /// 先が `tip` の矢印(`dir` は矢の進む向き)。
    fn arrow(&self, tip: Pos2, dir: Vec2) {
        let d = dir.normalized();
        let n = Vec2::new(-d.y, d.x);
        let back = tip - d * 7.0;
        self.painter.add(egui::Shape::convex_polygon(
            vec![tip, back + n * 2.6, back - n * 2.6],
            self.look.line,
            Stroke::NONE,
        ));
    }

    /// 両端に矢印のある寸法線。短すぎれば矢印を外向きに出す。
    fn dim_line(&self, a: Pos2, b: Pos2) {
        let d = b - a;
        if d.length() < 1.0 {
            return;
        }
        self.seg(a, b, 1.0);
        if d.length() > 20.0 {
            self.arrow(a, -d);
            self.arrow(b, d);
        } else {
            let u = d.normalized() * 12.0;
            self.seg(a - u, a, 1.0);
            self.seg(b, b + u, 1.0);
            self.arrow(a, d);
            self.arrow(b, -d);
        }
    }

    /// 置く位置(字の真ん中)から、`angle` の向きに書く字を浮かせる分だけ戻して、寸法線を通す所にする。
    fn aim(&self, angle: f32, text: &str) {
        let h = self.text_size(text).y;
        self.toward
            .set(self.look.toward.map(|c| c - up(angle) * (h / 2.0 + LIFT)));
    }

    /// 寸法線 a-b の上の値の札の真ん中。置く位置(`toward`)を寸法線に写した所で、なければ
    /// 真ん中。札が寸法線の外にあれば、近い端から札まで寸法線を延ばす。
    fn along(&self, a: Pos2, b: Pos2) -> Pos2 {
        let m = a + (b - a) / 2.0;
        let (Some(c), d) = (self.toward.get(), b - a) else {
            return m;
        };
        let len = d.length();
        if len < 1e-3 {
            return m;
        }
        let u = d / len;
        let k = (c - a).dot(u);
        let at = a + u * k;
        if k < 0.0 {
            self.seg(at, a, 1.0);
        } else if k > len {
            self.seg(b, at, 1.0);
        }
        at
    }

    /// 値の字の大きさ(回す前)。
    fn text_size(&self, text: &str) -> Vec2 {
        let font = egui::FontId::proportional(TEXT);
        self.painter
            .layout_no_wrap(text.to_string(), font, self.look.text)
            .size()
    }

    /// 値の札と寸法線が、既に置いた札や寸法線と重ならない、形から寸法線までの距離。
    /// `at` は距離から(札の真ん中、寸法線の両端)を返す。`want` は置いた位置までの距離
    /// (人が置いた・引いて動かした寸法はそこに出す。重なっても動かさない。網羅版 F5)。
    fn offset(
        &self,
        text: &str,
        angle: f32,
        want: Option<f32>,
        at: impl Fn(f32) -> (Pos2, Pos2, Pos2),
    ) -> f32 {
        if let Some(w) = want {
            return w.max(GAP + OVERSHOOT + 4.0);
        }
        let raw = self.text_size(text);
        let size = turned_size(raw, angle);
        let lift = up(angle) * (raw.y / 2.0 + LIFT);
        let taken = self.look.taken.borrow();
        (0..6)
            .map(|k| OFFSET + 22.0 * k as f32)
            .find(|off| {
                let (c, a, b) = at(*off);
                let label = Rect::from_center_size(c + lift, size).expand(2.0);
                let line = Rect::from_two_pos(a, b).expand(1.0);
                !taken
                    .iter()
                    .any(|t| t.intersects(label) || t.intersects(line))
            })
            .unwrap_or(OFFSET + 22.0 * 6.0)
    }

    /// 寸法線を、ほかの寸法が避ける所として覚える。
    fn take_line(&self, a: Pos2, b: Pos2) {
        self.look
            .taken
            .borrow_mut()
            .push(Rect::from_two_pos(a, b).expand(1.0));
    }

    /// 値を、真ん中が `at` になるよう横書きで置く(寸法線の上でない所: 平行な線の角度など)。
    fn label(&self, at: Pos2, text: String) {
        self.text(at, text, 0.0);
    }

    /// 値を寸法線(`at` はその上の点)の上へ、線と平行に置く(`angle` は画面で時計回りの rad)。
    /// JIS と同じく字は寸法線の上側(縦の線は左側)で、地の箱は付けない。
    fn label_turned(&self, at: Pos2, text: String, angle: f32) {
        let h = self.text_size(&text).y;
        self.text(at + up(angle) * (h / 2.0 + LIFT), text, angle);
    }

    /// 値の字を、真ん中が `center` になるよう `angle` だけ回して書く。当たり判定(値を直す
    /// ダブルクリック・札を引く)と重なりよけは、回した字を少し広げて囲む箱で。
    fn text(&self, center: Pos2, text: String, angle: f32) {
        let font = egui::FontId::proportional(TEXT);
        let g = self.painter.layout_no_wrap(text, font, self.look.text);
        let (s, c) = angle.sin_cos();
        let turn = |v: Vec2| Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c);
        let r = Rect::from_center_size(center, turned_size(g.size(), angle)).expand(3.0);
        self.look.taken.borrow_mut().push(r);
        self.last.set(Some(r));
        let pos = center + turn(-g.size() / 2.0);
        self.painter
            .add(egui::epaint::TextShape::new(pos, g, self.look.text).with_angle(angle));
    }

    /// 円・円弧の寸法の引出し線: 円周の `edge` から肩まで引き、肩から横へ字の幅だけ線を延ばして、
    /// その上に値を書く。`text_at` は字の真ん中。
    fn leader(&self, edge: Pos2, text_at: Pos2, right: bool, text: String) {
        let size = self.text_size(&text);
        let y = text_at.y + size.y / 2.0 + LIFT;
        let half = size.x / 2.0 + 2.0;
        let (elbow, end) = if right {
            (
                Pos2::new(text_at.x - half, y),
                Pos2::new(text_at.x + half, y),
            )
        } else {
            (
                Pos2::new(text_at.x + half, y),
                Pos2::new(text_at.x - half, y),
            )
        };
        self.seg(edge, elbow, 1.0);
        self.seg(elbow, end, 1.0);
        self.text(text_at, text, 0.0);
    }

    /// 2 点を結ぶ向きに測る寸法(長さ・距離)。形の外側(`away` から離れる側)へずらして描く。
    fn aligned(&self, p: Pos2, q: Pos2, away: Pos2, text: String) {
        let d = q - p;
        if d.length() < 1e-3 {
            self.label(p + Vec2::new(0.0, -OFFSET), text);
            return;
        }
        let u = d.normalized();
        let mut n = Vec2::new(-u.y, u.x);
        let mid = p + d / 2.0;
        if (mid - away).dot(n) < 0.0 {
            n = -n;
        }
        let angle = readable(d);
        self.aim(angle, &text);
        let want = self.toward.get().map(|c| (c - mid).dot(n).abs());
        let off = self.offset(&text, angle, want, |o| (mid + n * o, p + n * o, q + n * o));
        self.seg(p + n * GAP, p + n * (off + OVERSHOOT), 1.0);
        self.seg(q + n * GAP, q + n * (off + OVERSHOOT), 1.0);
        self.dim_line(p + n * off, q + n * off);
        self.take_line(p + n * off, q + n * off);
        let at = self.along(p + n * off, q + n * off);
        self.label_turned(at, text, angle);
    }
}

/// 向き `d`(画面)の線に沿わせる札の角度(画面で時計回りの rad)。字が逆さにならないよう、
/// 下か右から読める向きにする(縦の線は下から上へ読む。JIS の寸法の向きと同じ)。
fn readable(d: Vec2) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
    let mut a = d.y.atan2(d.x);
    if a >= FRAC_PI_2 - 1e-3 {
        a -= PI;
    } else if a < -FRAC_PI_2 - 1e-3 {
        a += PI;
    }
    a
}

/// `angle` だけ回した字の上の向き(画面。横書きなら上、縦の線に沿う字なら左)。
fn up(angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(s, -c)
}

/// `angle` だけ回した大きさ `size` の札を囲む箱の大きさ。
fn turned_size(size: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(
        size.x * c.abs() + size.y * s.abs(),
        size.x * s.abs() + size.y * c.abs(),
    )
}

/// 寸法を引出し線つきで描き、値の札の場所を返す。描けない寸法(参照先がない)は何もしない。
pub(crate) fn dimension(
    painter: &egui::Painter,
    t: &Transform,
    sketch: &Sketch,
    sol: &Solution,
    kind: &K,
    text: String,
    look: &DimLook,
) -> Option<Rect> {
    // 寸法線を出す側: 置く位置があればその側(要素の真ん中について写した点から離れる)
    let side = |mid: Pos2| look.toward.map_or(look.away, |c| mid + (mid - c));
    let pen = Pen {
        painter,
        look,
        last: std::cell::Cell::new(None),
        toward: std::cell::Cell::new(look.toward),
    };
    let s = |p: [f64; 2]| t.to_screen(p);
    match kind {
        K::Length { line, .. } => {
            let (p, q) = line_ends(sketch, sol, *line)?;
            let (p, q) = (s(p), s(q));
            pen.aligned(p, q, side(p + (q - p) / 2.0), text);
        }
        K::Distance { a, b, .. } => {
            let (p, q) = (s(pt(sol, a)?), s(pt(sol, b)?));
            pen.aligned(p, q, side(p + (q - p) / 2.0), text);
        }
        K::HorizontalDistance { a, b, .. } => {
            let (p, q) = (s(pt(sol, a)?), s(pt(sol, b)?));
            // 形の上側にあれば上へ、下側なら下へ寸法線を出す
            let up = (p.y + q.y) / 2.0 <= side(p + (q - p) / 2.0).y;
            let base = |off: f32| {
                if up {
                    p.y.min(q.y) - off
                } else {
                    p.y.max(q.y) + off
                }
            };
            pen.aim(0.0, &text);
            let want = pen.toward.get().map(|c| (base(0.0) - c.y).abs());
            let off = pen.offset(&text, 0.0, want, |o| {
                let y = base(o);
                (
                    Pos2::new((p.x + q.x) / 2.0, y),
                    Pos2::new(p.x, y),
                    Pos2::new(q.x, y),
                )
            });
            let y = base(off);
            pen.take_line(Pos2::new(p.x, y), Pos2::new(q.x, y));
            let sign = if up { -1.0 } else { 1.0 };
            for v in [p, q] {
                pen.seg(
                    Pos2::new(v.x, v.y + sign * GAP),
                    Pos2::new(v.x, y + sign * OVERSHOOT),
                    1.0,
                );
            }
            pen.dim_line(Pos2::new(p.x, y), Pos2::new(q.x, y));
            let at = pen.along(Pos2::new(p.x, y), Pos2::new(q.x, y));
            pen.label_turned(at, text, 0.0);
        }
        K::VerticalDistance { a, b, .. } => {
            let (p, q) = (s(pt(sol, a)?), s(pt(sol, b)?));
            let right = (p.x + q.x) / 2.0 >= side(p + (q - p) / 2.0).x;
            let base = |off: f32| {
                if right {
                    p.x.max(q.x) + off
                } else {
                    p.x.min(q.x) - off
                }
            };
            let vertical = readable(Vec2::new(0.0, 1.0));
            pen.aim(vertical, &text);
            let want = pen.toward.get().map(|c| (base(0.0) - c.x).abs());
            let off = pen.offset(&text, vertical, want, |o| {
                let x = base(o);
                (
                    Pos2::new(x, (p.y + q.y) / 2.0),
                    Pos2::new(x, p.y),
                    Pos2::new(x, q.y),
                )
            });
            let x = base(off);
            pen.take_line(Pos2::new(x, p.y), Pos2::new(x, q.y));
            let sign = if right { 1.0 } else { -1.0 };
            for v in [p, q] {
                pen.seg(
                    Pos2::new(v.x + sign * GAP, v.y),
                    Pos2::new(x + sign * OVERSHOOT, v.y),
                    1.0,
                );
            }
            pen.dim_line(Pos2::new(x, p.y), Pos2::new(x, q.y));
            let at = pen.along(Pos2::new(x, p.y), Pos2::new(x, q.y));
            pen.label_turned(at, text, vertical);
        }
        // 点と線(平行な 2 本の線の間も)の距離。寸法線は線に直角で、置く位置があれば線に沿って
        // その横へずらし、点と線からそこまで補助の線を引く(Kie のレビュー K3)
        K::PointLineDistance { point, line, .. } => {
            let x = pt(sol, point)?;
            let (a, b) = line_ends(sketch, sol, *line)?;
            let (sx, sf) = (s(x), s(foot_on(x, a, b)));
            let (sa, sb) = (s(a), s(b));
            let len = (sb - sa).length();
            let lu = if len > 1e-3 { (sb - sa) / len } else { Vec2::X };
            pen.aim(readable(sf - sx), &text);
            let shift = pen
                .toward
                .get()
                .map_or(0.0, |c| (c - (sx + (sf - sx) / 2.0)).dot(lu));
            let (p, q) = (sx + lu * shift, sf + lu * shift);
            // 点から寸法線の端まで(線に沿う向き)
            if shift.abs() > GAP {
                let sg = shift.signum();
                pen.seg(sx + lu * (sg * GAP), p + lu * (sg * OVERSHOOT), 1.0);
            }
            // 線の外に寸法線が来たら、線の近い端から延ばす
            let k = (q - sa).dot(lu);
            if k < 0.0 {
                pen.seg(sa - lu * GAP, q - lu * OVERSHOOT, 1.0);
            } else if k > len {
                pen.seg(sb + lu * GAP, q + lu * OVERSHOOT, 1.0);
            }
            pen.dim_line(p, q);
            let at = pen.along(p, q);
            pen.label_turned(at, text, readable(q - p));
        }
        // 半径・直径。値は円の外へ引出し線と肩で出す。置く位置があれば字をそこへ
        // 出し、円の中へ置けば引出し線なしで書く(札を円から離す距離もマウスで変えられる。K9)
        K::Radius { circle, .. } | K::Diameter { circle, .. } => {
            let (c, r, ang) = round(sketch, sol, *circle)?;
            let center = s(c);
            let size = pen.text_size(&text);
            let half = size.x / 2.0 + 2.0;
            let lift = size.y / 2.0 + LIFT;
            // 字の真ん中と、肩の線の円に近い端(引出し線は円の中心から肩へ向かう)
            let placed = look.toward.filter(|at| (*at - center).length() > 1.0);
            let (text_at, right, ang) = match placed {
                Some(at) => {
                    let right = at.x >= center.x;
                    let elbow = Pos2::new(at.x + if right { -half } else { half }, at.y + lift);
                    let w = t.to_world(elbow);
                    (Some(at), right, (w[1] - c[1]).atan2(w[0] - c[0]))
                }
                None => (None, ang.cos() >= 0.0, ang),
            };
            let dir = [ang.cos(), ang.sin()];
            let edge = s([c[0] + r * dir[0], c[1] + r * dir[1]]);
            let out = (edge - center).normalized();
            arc_extension(&pen, t, sketch, sol, *circle, ang);
            if matches!(kind, K::Diameter { .. }) {
                arc_extension(&pen, t, sketch, sol, *circle, ang + std::f64::consts::PI);
                let other = s([c[0] - r * dir[0], c[1] - r * dir[1]]);
                pen.dim_line(other, edge);
            } else {
                pen.seg(center, edge, 1.0);
                pen.arrow(edge, out);
                painter.circle_filled(center, 2.0, look.line);
            }
            let text_at = text_at.unwrap_or_else(|| {
                let elbow = edge + out * 18.0;
                Pos2::new(elbow.x + if right { half } else { -half }, elbow.y - lift)
            });
            let elbow_y = text_at.y + lift;
            let elbow = Pos2::new(text_at.x + if right { -half } else { half }, elbow_y);
            if (elbow - center).length() > (edge - center).length() {
                pen.leader(edge, text_at, right, text);
            } else {
                pen.label(text_at, text);
            }
        }
        K::Angle { a, b, .. } => {
            let (l1, l2) = (line_ends(sketch, sol, *a)?, line_ends(sketch, sol, *b)?);
            angle_dim(&pen, t, l1, l2, text);
        }
        // 中心線からの直径: 点と、中心線について映した点の間に寸法線を引く
        K::LinearDiameter { point, line, .. } => {
            let x = pt(sol, point)?;
            let (a, b) = line_ends(sketch, sol, *line)?;
            let foot = foot_on(x, a, b);
            let mirror = [2.0 * foot[0] - x[0], 2.0 * foot[1] - x[1]];
            let (p, q) = (s(x), s(mirror));
            pen.seg(s(foot), q, 1.0);
            pen.dim_line(p, q);
            pen.aim(readable(q - p), &text);
            let at = if look.toward.is_some() {
                pen.along(p, q)
            } else {
                p + (q - p) * 0.75
            };
            pen.label_turned(at, text, readable(q - p));
        }
        // 円周からの距離: 円周の近い(遠い)点から、相手の近い(遠い)点まで
        K::CircleDistance {
            circle, other, far, ..
        } => {
            let (c, r, _) = round(sketch, sol, *circle)?;
            let target = match sketch.entity(*other)? {
                jc_sketch::Entity::Point { .. } => (pt(sol, other)?, 0.0),
                jc_sketch::Entity::Line { .. } => {
                    let (a, b) = line_ends(sketch, sol, *other)?;
                    (foot_on(c, a, b), 0.0)
                }
                _ => {
                    let (c2, r2, _) = round(sketch, sol, *other)?;
                    (c2, r2)
                }
            };
            let (q0, r2) = target;
            let d = [q0[0] - c[0], q0[1] - c[1]];
            let len = d[0].hypot(d[1]).max(1e-12);
            let u = [d[0] / len, d[1] / len];
            let sg = if *far { -1.0 } else { 1.0 };
            let p = [c[0] + sg * r * u[0], c[1] + sg * r * u[1]];
            let q = [q0[0] - sg * r2 * u[0], q0[1] - sg * r2 * u[1]];
            let (p, q) = (s(p), s(q));
            pen.dim_line(p, q);
            pen.aim(readable(q - p), &text);
            let at = pen.along(p, q);
            pen.label_turned(at, text, readable(q - p));
        }
        _ => return None,
    }
    pen.last.get()
}

/// 画面へ写した形で決めた寸法(3D スケッチの寸法を、2D と同じ見た目で描く。まとめてレビュー R7)。
pub(crate) enum ScreenDim {
    /// 2 点の向きに測る寸法(長さ・距離・軸の向きの差・平面との距離)
    Aligned(Pos2, Pos2),
    /// 円の半径か直径。`rim` は画面へ写した円周(閉じた輪の点。始めと終わりは同じ点でよい)
    Round {
        center: Pos2,
        rim: Vec<Pos2>,
        diameter: bool,
    },
    /// 角度の円弧(画面の折れ線。両端に矢印)。`at` は札を出す円弧の点の番号、`out` はそこから
    /// 札を出す外向き、`ext` は線分が円弧まで届かない所を延ばす補助の線
    Arc {
        pts: Vec<Pos2>,
        at: usize,
        out: Vec2,
        ext: Vec<(Pos2, Pos2)>,
    },
    /// 値だけ(真ん中の位置。置いた位置があればそこ)
    Text(Pos2),
}

/// 画面へ写した形の寸法を、2D の寸法と同じ線・矢印・札で描き、値の札の場所を返す。
pub(crate) fn screen_dimension(
    painter: &egui::Painter,
    look: &DimLook,
    dim: &ScreenDim,
    text: String,
) -> Option<Rect> {
    let side = |mid: Pos2| look.toward.map_or(look.away, |c| mid + (mid - c));
    let pen = Pen {
        painter,
        look,
        last: std::cell::Cell::new(None),
        toward: std::cell::Cell::new(look.toward),
    };
    match dim {
        ScreenDim::Aligned(p, q) => pen.aligned(*p, *q, side(*p + (*q - *p) / 2.0), text),
        ScreenDim::Round {
            center,
            rim,
            diameter,
        } => round_screen(&pen, *center, rim, *diameter, text),
        ScreenDim::Arc { pts, at, out, ext } => {
            if pts.len() < 3 {
                return None;
            }
            for (a, b) in ext {
                pen.seg(*a, *b, 1.0);
            }
            let stroke = Stroke::new(1.0, look.line);
            if look.dashed {
                painter.add(egui::Shape::dashed_line(pts, stroke, 4.0, 3.0));
            } else {
                painter.add(egui::Shape::line(pts.clone(), stroke));
            }
            let n = pts.len();
            pen.arrow(pts[0], pts[0] - pts[1]);
            pen.arrow(pts[n - 1], pts[n - 1] - pts[n - 2]);
            // 値は円弧の外側に、円弧の接線と平行に書く(2D と同じ。円弧が字より短ければ横書き)
            let i = (*at).clamp(1, n - 2);
            let size = pen.text_size(&text);
            let length: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
            let angle = if length > size.x + 8.0 {
                readable(pts[i + 1] - pts[i - 1])
            } else {
                0.0
            };
            let (sn, cs) = angle.sin_cos();
            let reach = (out.x * -sn + out.y * cs).abs() * size.y / 2.0
                + (out.x * cs + out.y * sn).abs() * size.x / 2.0
                + LIFT;
            pen.text(pts[i] + *out * reach, text, angle);
        }
        ScreenDim::Text(at) => pen.label(look.toward.unwrap_or(*at), text),
    }
    pen.last.get()
}

/// 画面へ写した円(3D では楕円に見える)の半径・直径の寸法。2D と同じく、円周から引出し線と肩で
/// 外へ出し、置く位置があれば字をそこへ、円の中へ置けば引出し線なしで書く。
fn round_screen(pen: &Pen, center: Pos2, rim: &[Pos2], diameter: bool, text: String) {
    let n = if rim.len() > 1 && rim[0] == rim[rim.len() - 1] {
        rim.len() - 1
    } else {
        rim.len()
    };
    if n < 3 {
        return;
    }
    let size = pen.text_size(&text);
    let half = size.x / 2.0 + 2.0;
    let lift = size.y / 2.0 + LIFT;
    let placed = pen.look.toward.filter(|at| (*at - center).length() > 1.0);
    // 引出し線を出す向き: 置いた字の肩の向きか、右上
    let want = match placed {
        Some(at) => {
            let right = at.x >= center.x;
            Pos2::new(at.x + if right { -half } else { half }, at.y + lift) - center
        }
        None => Vec2::new(1.0, -1.0),
    };
    let i = (0..n)
        .max_by(|a, b| {
            let d = |k: usize| (rim[k] - center).normalized().dot(want.normalized());
            d(*a).total_cmp(&d(*b))
        })
        .unwrap_or(0);
    let edge = rim[i];
    let out = (edge - center).normalized();
    if diameter {
        pen.dim_line(rim[(i + n / 2) % n], edge);
    } else {
        pen.seg(center, edge, 1.0);
        pen.arrow(edge, out);
        pen.painter.circle_filled(center, 2.0, pen.look.line);
    }
    let right = placed.map_or(out.x >= 0.0, |at| at.x >= center.x);
    let text_at = placed.unwrap_or_else(|| {
        let elbow = edge + out * 18.0;
        Pos2::new(elbow.x + if right { half } else { -half }, elbow.y - lift)
    });
    let elbow = Pos2::new(
        text_at.x + if right { -half } else { half },
        text_at.y + lift,
    );
    if (elbow - center).length() > (edge - center).length() {
        pen.leader(edge, text_at, right, text);
    } else {
        pen.label(text_at, text);
    }
}

/// 角度の寸法の円弧を描く扇(始まりの向きと開き、rad)。角度の寸法の値は、1 本目の線の向きから
/// 2 本目の線の向きまで左回りに測って π で折り返した角 `v`(jc-sketch の measure と同じ)なので、
/// 値と同じ開きの扇は、1 本目の向き `ta` から始まるものと、その向かい(`ta + π`)の 2 つ。
/// 置く位置 `want` があればその向きに近い方、なければ線分の実際にある側(内角)を選ぶ
/// (Kie のレビュー K4: 既定は内角、反対側へ引けば線を延ばして出す)。
pub(crate) fn angle_sector(
    x: [f64; 2],
    l1: ([f64; 2], [f64; 2]),
    l2: ([f64; 2], [f64; 2]),
    want: Option<[f64; 2]>,
) -> (f64, f64) {
    use std::f64::consts::PI;
    let dir = |(p, q): ([f64; 2], [f64; 2])| (q[1] - p[1]).atan2(q[0] - p[0]);
    let ta = dir(l1);
    let v = (dir(l2) - ta).rem_euclid(PI);
    let starts = [ta, ta + PI];
    let gap = |a: f64, b: f64| ((a - b + PI).rem_euclid(2.0 * PI) - PI).abs();
    let start = match want {
        Some(w) => {
            let phi = (w[1] - x[1]).atan2(w[0] - x[0]);
            if gap(phi, starts[0] + v / 2.0) <= gap(phi, starts[1] + v / 2.0) {
                starts[0]
            } else {
                starts[1]
            }
        }
        None => {
            // 線分がその向きへどれだけあるか(交点から見て)。両方の線が実際にある扇を選ぶ
            let reach = |(p, q): ([f64; 2], [f64; 2]), a: f64| {
                let d = [a.cos(), a.sin()];
                [p, q]
                    .iter()
                    .map(|e| (e[0] - x[0]) * d[0] + (e[1] - x[1]) * d[1])
                    .fold(f64::NEG_INFINITY, f64::max)
            };
            let score = |s: f64| {
                let (r1, r2) = (reach(l1, s), reach(l2, s + v));
                (
                    (r1 > 1e-9) as u8 + (r2 > 1e-9) as u8,
                    r1.max(0.0) + r2.max(0.0),
                )
            };
            let (s0, s1) = (score(starts[0]), score(starts[1]));
            if s1.0 > s0.0 || (s1.0 == s0.0 && s1.1 > s0.1 + 1e-9) {
                starts[1]
            } else {
                starts[0]
            }
        }
    };
    (start, v)
}

/// 角度の寸法: 交点を中心に、値と同じ開きの扇へ円弧と矢印を描き、札を置く。札の位置まで
/// 円弧の半径を広げ、線分が円弧まで届かなければ補助の線で延ばす。
/// 円弧の半径・直径の矢印が円弧の端より外(向き `ang`)にある時、近い方の端から矢印の先まで
/// 円弧を点線で延ばす(どの円弧の寸法かが分かる)。
/// 円と、矢印が円弧の上にある時は何も描かない。
fn arc_extension(
    pen: &Pen,
    t: &Transform,
    sketch: &Sketch,
    sol: &Solution,
    id: EntityId,
    ang: f64,
) {
    use std::f64::consts::TAU;
    let Some(Entity::Arc {
        center, start, end, ..
    }) = sketch.entity(id)
    else {
        return;
    };
    let (Some(c), Some(a), Some(e)) = (pt(sol, center), pt(sol, start), pt(sol, end)) else {
        return;
    };
    let r = (a[0] - c[0]).hypot(a[1] - c[1]);
    let px = 1.0 / t.scale as f64;
    if r < px {
        return;
    }
    let a0 = (a[1] - c[1]).atan2(a[0] - c[0]);
    let span = ((e[1] - c[1]).atan2(e[0] - c[0]) - a0).rem_euclid(TAU);
    let Some((from, to)) = extension_range(
        a0,
        span,
        ang,
        GAP as f64 * px / r,
        OVERSHOOT as f64 * px / r,
    ) else {
        return;
    };
    let n = (((to - from) * r / px / 4.0).ceil() as usize).clamp(2, 96);
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let k = from + (to - from) * i as f64 / n as f64;
            t.to_screen([c[0] + r * k.cos(), c[1] + r * k.sin()])
        })
        .collect();
    pen.painter.add(egui::Shape::dashed_line(
        &pts,
        Stroke::new(1.0, pen.look.line),
        4.0,
        3.0,
    ));
}

/// 始めの角 `a0` から左回りに `span` の円弧で、向き `ang` の矢印まで延ばす角の範囲(小さい方から)。
/// 矢印が円弧の上なら `None`。近い方の端から `gap` 離して始め、矢印の先を `over` 越える(ラジアン)。
fn extension_range(a0: f64, span: f64, ang: f64, gap: f64, over: f64) -> Option<(f64, f64)> {
    use std::f64::consts::TAU;
    let rel = (ang - a0).rem_euclid(TAU);
    if rel <= span {
        return None;
    }
    let (from, to) = if rel - span <= TAU - rel {
        (a0 + span + gap, a0 + rel + over)
    } else {
        (a0 + rel - TAU - over, a0 - gap)
    };
    (to > from).then_some((from, to))
}

fn angle_dim(
    pen: &Pen,
    t: &Transform,
    l1: ([f64; 2], [f64; 2]),
    l2: ([f64; 2], [f64; 2]),
    text: String,
) {
    let s = |p: [f64; 2]| t.to_screen(p);
    let look = pen.look;
    let Some(x) = jc_sketch::line_crossing(l1.0, l1.1, l2.0, l2.1) else {
        // 平行: 角が決まらない。2 本の真ん中の間(置く位置があればそこ)に札だけ
        let (m1, m2) = (
            [(l1.0[0] + l1.1[0]) / 2.0, (l1.0[1] + l1.1[1]) / 2.0],
            [(l2.0[0] + l2.1[0]) / 2.0, (l2.0[1] + l2.1[1]) / 2.0],
        );
        let at = look
            .toward
            .unwrap_or(s([(m1[0] + m2[0]) / 2.0, (m1[1] + m2[1]) / 2.0]));
        pen.label(at, text);
        return;
    };
    let px = 1.0 / t.scale as f64;
    let want = look.toward.map(|c| t.to_world(c));
    let (start, v) = angle_sector(x, l1, l2, want);
    let ray = |a: f64, k: f64| [x[0] + k * a.cos(), x[1] + k * a.sin()];
    // 値を書く向き(置く位置の向きを扇の中に収める)
    let lab = match want {
        Some(w) => {
            let phi = (w[1] - x[1]).atan2(w[0] - x[0]);
            let rel = (phi - start).rem_euclid(std::f64::consts::TAU);
            if rel <= v {
                start + rel
            } else if rel - v < std::f64::consts::TAU - rel {
                start + v
            } else {
                start
            }
        }
        None => start + v / 2.0,
    };
    // 値は円弧の外側に、円弧の接線と平行に書く。円弧が字より短ければ、接線に
    // 沿わせると線に重なるので横書き。返すのは字の角度と、円弧から字の真ん中までの距離(px)
    let size = pen.text_size(&text);
    let out = (s(ray(lab, 1.0)) - s(x)).normalized();
    let fit = |rad: f64| {
        let tangent = s(ray(lab + 0.01, rad)) - s(ray(lab - 0.01, rad));
        let angle = if (rad / px * v) as f32 > size.x + 8.0 {
            readable(tangent)
        } else {
            0.0
        };
        // 回した字の外向きの幅の半分(字の横は接線、縦はその直角)
        let (sn, cs) = angle.sin_cos();
        let across = (out.x * -sn + out.y * cs).abs() * size.y / 2.0
            + (out.x * cs + out.y * sn).abs() * size.x / 2.0;
        (angle, across + LIFT)
    };
    // 置く位置は字の真ん中なので、円弧はそこから字の分だけ内側(札を掴んでも跳ねない)
    let rad = want
        .map_or(40.0 * px, |w| {
            let d = (w[0] - x[0]).hypot(w[1] - x[1]);
            let first = d - fit(d).1 as f64 * px;
            d - fit(first).1 as f64 * px
        })
        .max(14.0 * px);
    // 線分が円弧まで届かない側は、補助の線で延ばす(反対側へ引いた時も)
    for ((p, q), a) in [(l1, start), (l2, start + v)] {
        let d = [a.cos(), a.sin()];
        let along = |e: [f64; 2]| (e[0] - x[0]) * d[0] + (e[1] - x[1]) * d[1];
        let (k0, k1) = (along(p).min(along(q)), along(p).max(along(q)));
        if rad > k1 + GAP as f64 * px {
            pen.seg(
                s(ray(a, k1.max(0.0) + GAP as f64 * px)),
                s(ray(a, rad + OVERSHOOT as f64 * px)),
                1.0,
            );
        } else if rad < k0 - GAP as f64 * px {
            pen.seg(
                s(ray(a, (rad - OVERSHOOT as f64 * px).max(0.0))),
                s(ray(a, k0 - GAP as f64 * px)),
                1.0,
            );
        }
    }
    let pts: Vec<Pos2> = (0..=24)
        .map(|i| s(ray(start + v * i as f64 / 24.0, rad)))
        .collect();
    let (first, last) = (pts[0], pts[pts.len() - 1]);
    let (f2, l2p) = (pts[1], pts[pts.len() - 2]);
    if look.dashed {
        pen.painter.add(egui::Shape::dashed_line(
            &pts,
            Stroke::new(1.0, look.line),
            4.0,
            3.0,
        ));
    } else {
        pen.painter
            .add(egui::Shape::line(pts, Stroke::new(1.0, look.line)));
    }
    pen.arrow(first, first - f2);
    pen.arrow(last, last - l2p);
    let (angle, reach) = fit(rad);
    pen.text(s(ray(lab, rad)) + out * reach, text, angle);
}

/// 点 `x` から直線 a-b へ下ろした足。
fn foot_on(x: [f64; 2], a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let d = [b[0] - a[0], b[1] - a[1]];
    let dd = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
    let k = ((x[0] - a[0]) * d[0] + (x[1] - a[1]) * d[1]) / dd;
    [a[0] + d[0] * k, a[1] + d[1] * k]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    /// t4: 円弧の端より外に置いた半径の矢印まで、近い方の端から円弧を延ばす範囲。
    #[test]
    fn a_radius_beyond_the_arc_ends_extends_the_arc_from_the_nearer_end() {
        let d = f64::to_radians;
        let (gap, over) = (d(1.0), d(2.0));
        // 0°〜90° の円弧
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert_eq!(
            extension_range(0.0, d(90.0), d(45.0), gap, over),
            None,
            "円弧の上"
        );
        let (f, t) = extension_range(0.0, d(90.0), d(135.0), gap, over).unwrap();
        assert!(
            near(f, d(91.0)) && near(t, d(137.0)),
            "終わりの端から {f} {t}"
        );
        let (f, t) = extension_range(0.0, d(90.0), d(-30.0), gap, over).unwrap();
        assert!(
            near(f, d(-32.0)) && near(t, d(-1.0)),
            "始めの端から {f} {t}"
        );
        // 240° は始めの端の方が近い(始めから 120°、終わりから 150°)
        let (f, t) = extension_range(0.0, d(90.0), d(240.0), gap, over).unwrap();
        assert!(near(f, d(-122.0)) && near(t, d(-1.0)), "近い方の端 {f} {t}");
        // 0 をまたぐ円弧(300°〜30°)
        assert_eq!(extension_range(d(300.0), d(90.0), d(10.0), gap, over), None);
        let (f, t) = extension_range(d(300.0), d(90.0), d(60.0), gap, over).unwrap();
        assert!(near(f, d(391.0)) && near(t, d(422.0)), "{f} {t}");
    }

    /// 寸法の数字は線と平行で、逆さにならない(縦は下から上へ読む)。線の向きには左右されない。
    #[test]
    fn dimension_text_is_turned_along_the_line_and_never_upside_down() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(readable(egui::vec2(1.0, 0.0)), 0.0));
        assert!(close(readable(egui::vec2(-1.0, 0.0)), 0.0));
        // 画面の y は下向き: 縦の線はどちら向きでも -90°(下から上へ読む)
        assert!(close(readable(egui::vec2(0.0, 1.0)), -FRAC_PI_2));
        assert!(close(readable(egui::vec2(0.0, -1.0)), -FRAC_PI_2));
        let a = readable(egui::vec2(1.0, -1.0));
        assert!(close(a, readable(egui::vec2(-1.0, 1.0))));
        assert!(a > -FRAC_PI_2 && a < FRAC_PI_2);
        // 回した札を囲む箱
        let s = turned_size(egui::vec2(40.0, 10.0), -FRAC_PI_2);
        assert!((s.x - 10.0).abs() < 1e-3 && (s.y - 40.0).abs() < 1e-3);
    }

    /// 角度の扇: 既定は 2 本の線分が実際にある側(内角)。線の向き(端の順)には左右されず、
    /// 置く位置を向かいへ出すと向かいの扇になる(開きは同じ)。
    #[test]
    fn the_angle_sector_is_the_inner_one_unless_placed_opposite() {
        use std::f64::consts::{FRAC_PI_4, PI};
        let x = [0.0, 0.0];
        let close = |a: f64, b: f64| {
            ((a - b).rem_euclid(2.0 * PI)).min((b - a).rem_euclid(2.0 * PI)) < 1e-9
        };
        // 横の線と 45° の線、どちらも交点から外へ
        let (s, v) = angle_sector(
            x,
            ([0.0, 0.0], [10.0, 0.0]),
            ([0.0, 0.0], [10.0, 10.0]),
            None,
        );
        assert!(close(s, 0.0) && (v - FRAC_PI_4).abs() < 1e-9, "{s} {v}");
        // 2 本目の端の順が逆でも、同じ内角の扇
        let (s, v) = angle_sector(
            x,
            ([0.0, 0.0], [10.0, 0.0]),
            ([10.0, 10.0], [0.0, 0.0]),
            None,
        );
        assert!(close(s, 0.0) && (v - FRAC_PI_4).abs() < 1e-9, "{s} {v}");
        // 値が補角になる向き(1 本目から左回りに 135°): 内角の 45° でなく 135° の扇で、片方の線は延ばす側
        let (s, v) = angle_sector(
            x,
            ([0.0, 0.0], [10.0, 0.0]),
            ([0.0, 0.0], [10.0, -10.0]),
            None,
        );
        assert!((v - 3.0 * FRAC_PI_4).abs() < 1e-9, "{v}");
        assert!(close(s, 0.0) || close(s, PI), "{s}");
        // 置く位置を左下へ: 向かいの扇
        let (s, _) = angle_sector(
            x,
            ([0.0, 0.0], [10.0, 0.0]),
            ([0.0, 0.0], [10.0, 10.0]),
            Some([-5.0, -2.0]),
        );
        assert!(close(s, PI), "{s}");
    }
}
