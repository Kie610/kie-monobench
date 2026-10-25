//! ビューの移動と倍率、図と画面の座標の変換。

use super::*;
use jc_ui::view3d::Camera;

/// ビューの移動と倍率。全体が収まる表示に対する相対値で持つ。
#[derive(Clone)]
pub(crate) struct ViewState {
    pub(crate) pan: Vec2,
    pub(crate) zoom: f32,
    /// 表示の基準(最初に表示した時の範囲)。編集で範囲が変わっても表示が跳ねないよう固定する
    pub(crate) frame: Option<(Pos2, Vec2)>,
    /// 前のフレームで描いた寸法の値の札(ダブルクリックで値を直す)
    pub(crate) labels: Vec<(jc_sketch::ConstraintId, Rect)>,
    /// 前のフレームで描いた拘束の印(指を置く・クリックで選ぶ)
    pub(crate) marks: Vec<crate::marks::Mark>,
    /// 前のフレームの座標の変換(試験で、スケッチの位置を画面の位置にする)
    pub(crate) last: Option<Transform>,
    /// 3D の視点(右ドラッグで回した時。N1b)。`None` ならスケッチの平面を正面から見た平らな図
    pub(crate) space: Option<Camera>,
    /// 「正面から見る」を押した(3D の視点を正面へ回し終えたら平らな図に戻す。N1c)
    pub(crate) to_front: bool,
    /// 3D の視点のまま、全体が収まるよう合わせる(右クリックの「すべて表示」)
    pub(crate) refit: bool,
    /// 最後に描いた見え方を 3D の視点にした物と、ビューの範囲(スケッチを終えた時に 3D のビューへ
    /// 渡す。10-23 u1)
    pub(crate) seen: Option<(Camera, Rect)>,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            pan: Vec2::ZERO,
            zoom: 1.0,
            frame: None,
            labels: Vec::new(),
            marks: Vec::new(),
            last: None,
            space: None,
            to_front: false,
            refit: false,
            seen: None,
        }
    }
}

impl ViewState {
    /// スケッチの座標が `d` だけずれた時(原点を替えた時)、画面の上で形が跳ねないよう表示の基準も
    /// 同じだけずらす。
    pub(crate) fn follow(&mut self, d: [f64; 2]) {
        if let Some((c, _)) = self.frame.as_mut() {
            c.x += d[0] as f32;
            c.y += d[1] as f32;
        }
    }

    /// 全体が収まる表示に戻す(右クリックの「すべて表示」、ホイールの二度押し)。
    /// 3D の視点で見ている時は、向きを変えずに合わせる。
    pub(crate) fn fit(&mut self) {
        if self.space.is_some() {
            self.refit = true;
            return;
        }
        *self = ViewState {
            labels: std::mem::take(&mut self.labels),
            marks: std::mem::take(&mut self.marks),
            ..ViewState::default()
        };
    }
}

/// 世界座標(mm、y 上向き)の範囲(スケッチの点と、下に描く面の輪郭)
pub(crate) fn bounds(
    sol: &Solution,
    underlay: &Segments,
    refs: &[jc_sketch::RefItem],
) -> (Pos2, Pos2) {
    let mut min = Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    let ref_points: Vec<[f64; 2]> = refs
        .iter()
        .flat_map(|r| match r.shape {
            jc_sketch::RefShape::Point(p) => vec![p],
            jc_sketch::RefShape::Line(a, b) => vec![a, b],
            jc_sketch::RefShape::Circle { center, radius, .. } => vec![
                [center[0] - radius, center[1] - radius],
                [center[0] + radius, center[1] + radius],
            ],
        })
        .collect();
    for p in sol
        .points
        .values()
        .chain(underlay.iter().flatten())
        .chain(ref_points.iter())
    {
        min = min.min(Pos2::new(p[0] as f32, p[1] as f32));
        max = max.max(Pos2::new(p[0] as f32, p[1] as f32));
    }
    if !min.x.is_finite() {
        return (Pos2::new(-50.0, -50.0), Pos2::new(50.0, 50.0));
    }
    (min, max)
}

/// 3D の視点で見ている時の、視点・ビューの範囲・スケッチの平面(N1b)。
#[derive(Clone, Copy)]
pub(crate) struct Space {
    pub(crate) cam: Camera,
    pub(crate) rect: Rect,
    pub(crate) plane: jc_core::Plane,
}

impl Space {
    /// スケッチの平面の点(mm)の、空間の位置。
    pub(crate) fn world(&self, p: [f64; 2]) -> [f32; 3] {
        self.plane.to_world(p).map(|v| v as f32)
    }

    /// 画面の位置を通る視線とスケッチの平面の交わる所(平面の座標)。視線が平面とほぼ平行なら、
    /// 回す中心の平面の上の点を平面へ下ろした所。
    pub(crate) fn hit(&self, p: Pos2) -> [f64; 2] {
        let (o, d) = self.cam.ray(p, self.rect);
        let (o, d) = (o.map(|v| v as f64), d.map(|v| v as f64));
        let n = self.plane.normal();
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let den = dot(d, n);
        let w = if den.abs() > 1e-6 {
            let k = dot([0, 1, 2].map(|i| self.plane.origin[i] - o[i]), n) / den;
            [0, 1, 2].map(|i| o[i] + d[i] * k)
        } else {
            o
        };
        self.plane.to_local(w)
    }
}

#[derive(Clone)]
pub(crate) struct Transform {
    pub(crate) center_world: Pos2,
    pub(crate) center_screen: Pos2,
    /// 1 mm あたりの画素数(3D の視点では回す中心の平面の上で。許す隔たりの目安に使う)
    pub(crate) scale: f32,
    /// 3D の視点で見ている時の写し方(`None` なら平らな図)
    pub(crate) space: Option<Space>,
}

impl Transform {
    /// 平らな図の写し方。
    pub(crate) fn flat(center_world: Pos2, center_screen: Pos2, scale: f32) -> Self {
        Transform {
            center_world,
            center_screen,
            scale,
            space: None,
        }
    }

    /// 3D の視点の写し方。
    pub(crate) fn spatial(cam: Camera, rect: Rect, plane: jc_core::Plane) -> Self {
        let space = Space { cam, rect, plane };
        let c = space.hit(rect.center() + cam.pan);
        Transform {
            center_world: Pos2::new(c[0] as f32, c[1] as f32),
            center_screen: rect.center() + cam.pan,
            scale: cam.zoom,
            space: Some(space),
        }
    }

    pub(crate) fn to_screen(&self, p: [f64; 2]) -> Pos2 {
        if let Some(s) = &self.space {
            return s.cam.project(s.world(p), s.rect).0;
        }
        let d = Vec2::new(
            p[0] as f32 - self.center_world.x,
            p[1] as f32 - self.center_world.y,
        );
        Pos2::new(
            self.center_screen.x + d.x * self.scale,
            self.center_screen.y - d.y * self.scale,
        )
    }
    pub(crate) fn to_world(&self, p: Pos2) -> [f64; 2] {
        if let Some(s) = &self.space {
            return s.hit(p);
        }
        let d = (p - self.center_screen) / self.scale;
        [
            (self.center_world.x + d.x) as f64,
            (self.center_world.y - d.y) as f64,
        ]
    }

    /// スケッチの平面の線分の画面の両端(3D の視点で視点より手前の所は切る。全部なら `None`)。
    pub(crate) fn segment(&self, a: [f64; 2], b: [f64; 2]) -> Option<(Pos2, Pos2)> {
        match &self.space {
            Some(s) => s.cam.segment(s.world(a), s.world(b), s.rect),
            None => Some((self.to_screen(a), self.to_screen(b))),
        }
    }

    /// 中心 `c`・半径 `r`(mm)の円の画面の折れ線(3D の視点では楕円に見える)。
    pub(crate) fn circle_path(&self, c: [f64; 2], r: f64) -> Vec<Pos2> {
        (0..=96)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / 96.0;
                self.to_screen([c[0] + r * a.cos(), c[1] + r * a.sin()])
            })
            .collect()
    }

    /// 中心 `c`・半径 `r`(mm)の円を描く。平らな図ではそのまま円、3D の視点では写した折れ線。
    pub(crate) fn circle(&self, painter: &egui::Painter, c: [f64; 2], r: f64, stroke: Stroke) {
        if self.space.is_none() {
            painter.circle_stroke(self.to_screen(c), r as f32 * self.scale, stroke);
        } else {
            painter.add(egui::Shape::line(self.circle_path(c, r), stroke));
        }
    }
}

/// 平らな図の今の見え方と同じに見える、スケッチの平面を真正面から見る 3D の視点(N1b)。
/// 画面の真ん中の点を回す中心にし、1 mm の画素数も平らな図のままにする。
pub(crate) fn space_camera(t: &Transform, rect: Rect, plane: jc_core::Plane) -> Camera {
    let mut cam = Camera::default();
    // 次に描く時に全体へ合わせ直されないよう、一度合わせてから向きと大きさを決める
    cam.fit_box([0.0; 3], [1.0; 3], rect);
    cam.face(plane.normal().map(|v| v as f32));
    // 平らな図と同じく、スケッチの平面の X が画面の右に来るよう視線のまわりに傾ける
    // (ZX 平面や斜めの面では、傾けないと 3D の視点へ移った時に図が回って見える)
    cam.roll = 0.0;
    let (r, u, _) = cam.basis_pub();
    let x = plane.x_axis.map(|v| v as f32);
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    cam.roll = dot(x, u).atan2(dot(x, r));
    cam.target = plane.to_world(t.to_world(rect.center())).map(|v| v as f32);
    cam.pan = Vec2::ZERO;
    cam.zoom = t.scale;
    cam
}

/// 3D の視点で全体を収める範囲(スケッチの点と、前にできたボディ。世界の座標)。
pub(crate) fn space_bounds(
    sol: &Solution,
    plane: &jc_core::Plane,
    scene: &jc_ui::view3d::Scene,
) -> ([f32; 3], [f32; 3]) {
    let (mut lo, mut hi) = if scene.is_empty() {
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3])
    } else {
        scene.bounds()
    };
    for p in sol.points.values() {
        let w = plane.to_world(*p);
        for i in 0..3 {
            lo[i] = lo[i].min(w[i] as f32);
            hi[i] = hi[i].max(w[i] as f32);
        }
    }
    // 何もなければ、平面の原点のまわりの 100 mm の四角
    if lo[0] > hi[0] {
        for (k, v) in [(-50.0, &mut lo), (50.0, &mut hi)] {
            *v = plane.to_world([k, k]).map(|x| x as f32);
        }
        for i in 0..3 {
            let (a, b) = (lo[i].min(hi[i]), lo[i].max(hi[i]));
            (lo[i], hi[i]) = (a, b);
        }
    }
    (lo, hi)
}
