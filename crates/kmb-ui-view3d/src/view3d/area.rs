//! 囲んで選ぶ範囲(四角の窓と、なぞった多角形)。囲みの選びの判定は、どちらの形でもこの型で決める。

use egui::{Pos2, Rect};

/// 左ドラッグで囲む形(選択のメニューで替える)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Enclose {
    /// 押した所と離した所を角にする四角の窓
    #[default]
    Window,
    /// なぞった道を閉じた自由な形(投げ縄)
    Lasso,
}

/// 囲んだ範囲(画面の位置)。
#[derive(Clone, Debug, PartialEq)]
pub enum Area {
    /// 四角の窓
    Rect(Rect),
    /// なぞった多角形。最後の点から最初の点へ閉じる。3 点以上([`Area::lasso`] で作る)
    Polygon(Vec<Pos2>),
}

impl From<Rect> for Area {
    fn from(r: Rect) -> Self {
        Area::Rect(r)
    }
}

/// なぞった点の間をこれより短くしない(画素。細かすぎる点は判定を遅くするだけ)。
pub const LASSO_STEP: f32 = 3.0;

impl Area {
    /// なぞった点から多角形を作る。点が 3 つ未満か、囲んだ外形が小さすぎる(押して少し動いただけ)時は `None`。
    pub fn lasso(points: &[Pos2]) -> Option<Area> {
        if points.len() < 3 {
            return None;
        }
        let b = Rect::from_points(points);
        (b.width().max(b.height()) >= 2.0 * LASSO_STEP).then(|| Area::Polygon(points.to_vec()))
    }

    /// 範囲を囲む四角。
    pub fn bounds(&self) -> Rect {
        match self {
            Area::Rect(r) => *r,
            Area::Polygon(p) => Rect::from_points(p),
        }
    }

    /// 縁の角(四角は 4 つ、多角形はなぞった点)。
    fn corners(&self) -> Vec<Pos2> {
        match self {
            Area::Rect(r) => vec![
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
            ],
            Area::Polygon(p) => p.clone(),
        }
    }

    /// 点 `p` が範囲に入るか(縁の上も入る)。多角形は偶奇の決まりで内外を決める
    /// (なぞった道が自分と交わった所は、2 重に囲んだ所が外になる)。
    pub fn contains(&self, p: Pos2) -> bool {
        match self {
            Area::Rect(r) => r.contains(p),
            Area::Polygon(poly) => {
                if !Rect::from_points(poly).contains(p) {
                    return false;
                }
                let mut inside = false;
                for i in 0..poly.len() {
                    let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                    if on_segment(a, b, p) {
                        return true;
                    }
                    if (a.y > p.y) != (b.y > p.y) {
                        let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                        if p.x < x {
                            inside = !inside;
                        }
                    }
                }
                inside
            }
        }
    }

    /// 線分 `a`〜`b` が範囲に重なるか(端が入る、または縁と交わる)。
    pub fn segment_touches(&self, a: Pos2, b: Pos2) -> bool {
        match self {
            Area::Rect(r) => segment_touches_rect(*r, a, b),
            Area::Polygon(poly) => {
                if !self.bounds().intersects(Rect::from_two_pos(a, b)) {
                    return false;
                }
                self.contains(a)
                    || self.contains(b)
                    || (0..poly.len())
                        .any(|i| segments_meet(a, b, poly[i], poly[(i + 1) % poly.len()]))
            }
        }
    }

    /// 三角形 `t` が範囲に重なるか(三角形の点が入る、範囲の角が三角形に入る、辺どうしが交わる)。
    pub fn triangle_touches(&self, t: [Pos2; 3]) -> bool {
        if !self.bounds().intersects(Rect::from_points(&t)) {
            return false;
        }
        if t.iter().any(|p| self.contains(*p)) {
            return true;
        }
        let side =
            |a: Pos2, b: Pos2, p: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        let in_triangle = |p: Pos2| {
            let s = [
                side(t[0], t[1], p),
                side(t[1], t[2], p),
                side(t[2], t[0], p),
            ];
            s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
        };
        if self.corners().into_iter().any(in_triangle) {
            return true;
        }
        (0..3).any(|i| self.segment_touches(t[i], t[(i + 1) % 3]))
    }

    /// 点を全部含むか(中だけ選ぶ: 左から右へ囲んだ時)。
    pub fn contains_all(&self, points: &[Pos2]) -> bool {
        points.iter().all(|p| self.contains(*p))
    }

    /// 折れ線が範囲にかかるか(交差して選ぶ: 右から左へ囲んだ時)。点 1 つの線も見る。
    pub fn polyline_touches(&self, line: &[Pos2]) -> bool {
        match line {
            [] => false,
            [p] => self.contains(*p),
            _ => line.windows(2).any(|w| self.segment_touches(w[0], w[1])),
        }
    }
}

/// 四角 `area` と線分 `a`〜`b` が重なるか。
fn segment_touches_rect(area: Rect, a: Pos2, b: Pos2) -> bool {
    // 範囲で線分を切る(Liang-Barsky)。残りがあれば重なる
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-d.x, a.x - area.min.x),
        (d.x, area.max.x - a.x),
        (-d.y, a.y - area.min.y),
        (d.y, area.max.y - a.y),
    ] {
        if p.abs() < 1e-9 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    t0 <= t1
}

fn cross(o: Pos2, a: Pos2, b: Pos2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// 点 `p` が線分 `a`〜`b` の上にあるか。
fn on_segment(a: Pos2, b: Pos2, p: Pos2) -> bool {
    let len = (b - a).length().max(1e-6);
    (cross(a, b, p) / len).abs() <= 1e-3
        && p.x >= a.x.min(b.x) - 1e-3
        && p.x <= a.x.max(b.x) + 1e-3
        && p.y >= a.y.min(b.y) - 1e-3
        && p.y <= a.y.max(b.y) + 1e-3
}

/// 線分 `a`〜`b` と `c`〜`d` が交わるか(端が触れるだけも交わる)。
fn segments_meet(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> bool {
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    on_segment(c, d, a) || on_segment(c, d, b) || on_segment(a, b, c) || on_segment(a, b, d)
}

/// なぞった道の向きで選び方を決める: 押した所から最初に横へ大きく動いた向きが左なら交差して選ぶ
/// (四角の窓の「右から左へ囲むとかかった物」と同じ覚え方)。横へ動いていなければ中だけ選ぶ。
pub fn lasso_crossing(points: &[Pos2]) -> bool {
    let Some(start) = points.first() else {
        return false;
    };
    points
        .iter()
        .map(|p| p.x - start.x)
        .find(|dx| dx.abs() >= 2.0 * LASSO_STEP)
        .is_some_and(|dx| dx < 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    /// L 字の多角形(右上が欠けている)。
    fn ell() -> Area {
        Area::lasso(&[
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 20.0),
            pos2(30.0, 20.0),
            pos2(30.0, 30.0),
            pos2(0.0, 30.0),
        ])
        .unwrap()
    }

    #[test]
    fn polygon_inside_and_outside() {
        let a = ell();
        assert!(a.contains(pos2(5.0, 5.0)));
        assert!(a.contains(pos2(25.0, 25.0)));
        // 外形の中でも欠けた所は外
        assert!(!a.contains(pos2(20.0, 10.0)));
        assert!(!a.contains(pos2(40.0, 25.0)));
        // 縁の上は入る
        assert!(a.contains(pos2(10.0, 10.0)));
        assert!(a.contains(pos2(0.0, 0.0)));
    }

    #[test]
    fn self_crossing_lasso_uses_even_odd() {
        // 8 の字: 2 つの輪はどちらも中
        let a = Area::lasso(&[
            pos2(0.0, 0.0),
            pos2(20.0, 20.0),
            pos2(20.0, 0.0),
            pos2(0.0, 20.0),
        ])
        .unwrap();
        assert!(a.contains(pos2(2.0, 10.0)));
        assert!(a.contains(pos2(18.0, 10.0)));
        assert!(!a.contains(pos2(10.0, 2.0)));
    }

    #[test]
    fn too_small_lasso_is_none() {
        assert!(Area::lasso(&[pos2(0.0, 0.0), pos2(5.0, 5.0)]).is_none());
        assert!(Area::lasso(&[pos2(0.0, 0.0), pos2(1.0, 1.0), pos2(2.0, 0.0)]).is_none());
    }

    #[test]
    fn polygon_segment_and_triangle() {
        let a = ell();
        // 欠けた所だけを通る線はかからない
        assert!(!a.segment_touches(pos2(15.0, 5.0), pos2(28.0, 15.0)));
        // 腕をまたぐ線はかかる(両端は外)
        assert!(a.segment_touches(pos2(-5.0, 10.0), pos2(15.0, 10.0)));
        // 欠けた所に収まる三角形はかからない、腕に触れる三角形はかかる
        assert!(!a.triangle_touches([pos2(12.0, 2.0), pos2(28.0, 2.0), pos2(28.0, 18.0)]));
        assert!(a.triangle_touches([pos2(8.0, 2.0), pos2(28.0, 2.0), pos2(28.0, 18.0)]));
        // 範囲を丸ごと含む大きな三角形もかかる
        assert!(a.triangle_touches([
            pos2(-100.0, -100.0),
            pos2(200.0, -100.0),
            pos2(-100.0, 200.0)
        ]));
    }

    #[test]
    fn window_and_crossing_on_polygon() {
        let a = ell();
        // 腕の中に収まる線: 中だけでも交差でも入る
        let inner = [pos2(2.0, 2.0), pos2(8.0, 18.0)];
        assert!(a.contains_all(&inner) && a.polyline_touches(&inner));
        // 欠けた所へはみ出す線: 交差だけで入る
        let over = [pos2(5.0, 5.0), pos2(20.0, 5.0)];
        assert!(!a.contains_all(&over) && a.polyline_touches(&over));
        // 外だけの線: どちらでも入らない
        let out = [pos2(15.0, 5.0), pos2(25.0, 15.0)];
        assert!(!a.contains_all(&out) && !a.polyline_touches(&out));
        // 点 1 つ
        assert!(a.polyline_touches(&[pos2(5.0, 5.0)]));
        assert!(!a.polyline_touches(&[pos2(20.0, 5.0)]));
    }

    #[test]
    fn rect_area_keeps_old_answers() {
        let r = Area::from(Rect::from_min_max(pos2(10.0, 10.0), pos2(20.0, 20.0)));
        assert!(r.segment_touches(pos2(0.0, 15.0), pos2(30.0, 15.0)));
        assert!(!r.segment_touches(pos2(0.0, 0.0), pos2(30.0, 5.0)));
        assert!(r.triangle_touches([pos2(0.0, 0.0), pos2(40.0, 0.0), pos2(0.0, 40.0)]));
        assert!(!r.triangle_touches([pos2(30.0, 30.0), pos2(40.0, 30.0), pos2(30.0, 40.0)]));
    }

    #[test]
    fn lasso_direction_picks_crossing() {
        let right = [
            pos2(0.0, 0.0),
            pos2(1.0, 4.0),
            pos2(10.0, 5.0),
            pos2(-30.0, 9.0),
        ];
        let left = [pos2(0.0, 0.0), pos2(-8.0, 1.0), pos2(30.0, 9.0)];
        assert!(!lasso_crossing(&right));
        assert!(lasso_crossing(&left));
        assert!(!lasso_crossing(&[pos2(0.0, 0.0), pos2(0.0, 50.0)]));
    }
}
