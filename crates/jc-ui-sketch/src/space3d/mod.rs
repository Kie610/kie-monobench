//! 3D スケッチの編集器。中央に自分の 3D のビューを持ち、
//! 作図面(XY・YZ・ZX のどれかに平行で、最後に置いた点を通る平面。Tab で替える)の上に線分・
//! 矩形・円・点を描く。高さの違う図や直交する図を 1 つのスケッチに描ける。形は寸法と拘束
//! (軸に沿う・平行・直角・3 次元の角度・面と面の角度)で決める。閉じた線分の輪で平らなものは
//! 面になり、角度や距離の相手にできる。

mod menu;
mod ops;
mod view;

#[cfg(test)]
mod tests;

pub(crate) use ops::{DrawPlane, Sel};

use jc_core::{FeatureEntry, FeatureId};
use jc_sketch::{ConstraintId, EntityId, SolveStatus};
use jc_sketch_3d::body::BodyItem;
use jc_sketch_3d::{
    AxisPlane, DerivedDef, Entity3d, Kind3d, P3, PlaneDef, PlaneRef, SKETCH3D, Sketch3d,
    Solution3d, basis, dimension_values3d, geom, plane_at,
};
use jc_ui::{EditorHost, EditorTool, FeatureEditor, ToolGroup};
use std::collections::HashMap;

/// 取り込みの道具で取り込む物。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IncludePick {
    /// 押した角・辺・円
    Parts,
    /// 押した面の縁の辺と円
    Face,
    /// 押したボディの角・辺・円すべて
    Body,
}

/// 投影の道具で写す物(2D の投影の「写す物」と同じ 3 択)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectPick {
    /// 押した角・辺・円の中心、面なら面の縁
    Parts,
    /// 押したボディを作図面へ写した外形
    Silhouette,
    /// 押したボディの辺と角すべて
    Edges,
}

/// 「正面から見る」の説明。
const LOOK_AT_HINT: &str =
    "今の作図面を真正面から見る(押すたびに作図面の表から。向きは滑らかに替わる)";

/// 3D スケッチの道具。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tool3 {
    #[default]
    Select,
    Line,
    Rectangle,
    Circle,
    Point,
    /// ボディの角・辺・円を取り込む
    Include,
    /// ボディの形を今の作図面へ垂直に写す(まとめてレビュー R4)
    Project,
    /// ボディを今の作図面で切った線を写す(まとめてレビュー R4)
    Intersect,
}

/// 描く道具(道具、名前、アイコン、キーの行の名前(`jc_ui::keys`)、説明)。
const DRAW3: [(Tool3, &str, &str, &str, &str); 4] = [
    (
        Tool3::Line,
        "線分",
        "line",
        "sketch_line",
        "作図面の上を順にクリックして線分をつなぐ。点の上でクリックするとその点につなぐ。軸に近い向きは軸に沿わせる(拘束が付く)",
    ),
    (
        Tool3::Rectangle,
        "長方形",
        "rect",
        "sketch_rect",
        "作図面の上で対角の 2 点をクリック。辺は作図面の軸に沿う",
    ),
    (
        Tool3::Circle,
        "円",
        "circle",
        "sketch_circle",
        "中心と、円周の点をクリック。円は作図面に平行",
    ),
    (
        Tool3::Point,
        "点",
        "point",
        "sketch_point",
        "クリックした所に点を置く",
    ),
];

impl Tool3 {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Tool3::Select => "選ぶ",
            Tool3::Include => "3D 取り込み",
            Tool3::Project => "投影",
            Tool3::Intersect => "交差",
            t => DRAW3.iter().find(|d| d.0 == t).map_or("道具", |d| d.1),
        }
    }
}

/// 描き始めの点(既存の点か、新しい位置)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Start {
    pub id: Option<EntityId>,
    pub p: P3,
    /// 原点に吸い付いた(新しい点を原点に固定する)
    pub origin: bool,
    /// 軸のガイドで作図面の外に置いた(新しい点を作図面に乗せない。まとめてレビュー R5)
    pub off: bool,
}

/// 3D スケッチの編集器の状態。
pub struct Sketch3dEditor {
    /// 寸法の見せ方(スケッチの設定。画面の更新ごとに組み立ての共有の値から読む)
    pub(crate) fmt: jc_sketch::DimFormat,
    pub(crate) cam: jc_ui::view3d::Camera,
    scene: jc_ui::view3d::Scene,
    fitted: bool,
    pub(crate) tool: Tool3,
    /// 作図面の向き
    pub(crate) plane: AxisPlane,
    /// 作図面が通る点(スケッチの座標)
    pub(crate) anchor: P3,
    /// 描いている作図面(`None` なら座標の平面に平行な `plane` の作図面)
    pub(crate) work: Option<u32>,
    /// 面を選ばずに平面の拘束・寸法を付ける時の相手(`None` なら今の作図面)
    pub(crate) target: Option<PlaneRef>,
    /// 描きかけの始めの点
    pub(crate) start: Option<Start>,
    /// 選んだ物(選んだ順)
    pub(crate) selection: Vec<Sel>,
    /// 選んだ拘束(パネルの一覧か、図の印で)
    pub(crate) picked_constraint: Option<ConstraintId>,
    drag: Option<view::Drag>,
    /// 寸法の値の札を引いて動かしている所
    label_drag: Option<view::LabelDrag>,
    /// 寸法の値を直している欄(寸法と、打っている文字)
    pub(crate) edit: Option<(ConstraintId, String)>,
    /// 作図面の値(ずらす距離か傾き)を打っている欄
    pub(crate) plane_edit: Option<(u32, String)>,
    /// 新しい線と円を補助線で描く
    pub(crate) construction: bool,
    /// 取り込みの道具で取り込む物(2D の投影の「写す物」と同じ考え)
    pub(crate) include_pick: IncludePick,
    /// 投影の道具で写す物
    pub(crate) project_pick: ProjectPick,
    /// 交差の道具で、押したボディの断面をすべて写す(偽なら押した線だけ)
    pub(crate) intersect_body: bool,
    /// 投影・交差の道具で使う、見えているボディの網と名前の付いた形(道具を使う時に集める)
    pub(crate) geoms: Vec<jc_sketch_3d::body::BodyGeom>,
    /// `geoms` を集めたボディ
    geoms_of: Vec<(FeatureId, jc_core::ShapeHandle)>,
    /// 投影・交差の道具の、今の作図面へ写した下絵(作図面が替わると求め直す)
    pub(crate) on_plane: Option<view::OnPlane>,
    /// ビューの上に出す知らせ
    pub(crate) note: Option<String>,
    /// 線分を描きながら打った長さ
    pub(crate) typed: String,
    feature: Option<FeatureId>,
    /// 前のコマで、ほかの欄に打っていたか(Tab を欄の移動に任せるか)
    typing_before: bool,
    /// 直前のビューの範囲(試験で画面の位置を求める)
    pub(crate) rect: egui::Rect,
    /// 作図面の基準(スケッチの座標を世界へ写す平面)
    pub(crate) frame: jc_core::Plane,
    /// 見えているボディ(スケッチより前)と、その取り込める角・辺・円(世界座標)
    pub(crate) bodies: Vec<(FeatureId, jc_core::ShapeHandle)>,
    pub(crate) body_items: Vec<BodyItem>,
    /// 直前のコマで描いた寸法の値の札(プレス/プル・ダブルクリックの当たり)
    pub(crate) dim_rects: Vec<(ConstraintId, egui::Rect)>,
    /// 前に使った描く道具(右クリックの「繰り返し」)と、最近の道具(新しい順)
    pub(crate) last_tool: Option<Tool3>,
    pub(crate) recent: Vec<Tool3>,
    /// 右クリックした所(メニューの「OK」で、打った長さの線分を引く向き)
    menu_at: Option<egui::Pos2>,
}

impl Default for Sketch3dEditor {
    fn default() -> Self {
        Sketch3dEditor {
            fmt: jc_sketch::DimFormat::default(),
            cam: jc_ui::view3d::Camera::default(),
            scene: jc_ui::view3d::Scene::default(),
            fitted: false,
            tool: Tool3::Select,
            plane: AxisPlane::Xy,
            anchor: [0.0; 3],
            work: None,
            target: None,
            start: None,
            selection: Vec::new(),
            picked_constraint: None,
            drag: None,
            label_drag: None,
            edit: None,
            plane_edit: None,
            construction: false,
            include_pick: IncludePick::Parts,
            project_pick: ProjectPick::Parts,
            intersect_body: false,
            geoms: Vec::new(),
            geoms_of: Vec::new(),
            on_plane: None,
            note: None,
            typed: String::new(),
            feature: None,
            typing_before: false,
            rect: egui::Rect::NOTHING,
            frame: jc_core::Plane::XY,
            bodies: Vec::new(),
            body_items: Vec::new(),
            dim_rects: Vec::new(),
            last_tool: None,
            recent: Vec::new(),
            menu_at: None,
        }
    }
}

/// 文書の 3D スケッチ(解いた位置を写したもの)と解。解がまだなければ保存の位置で。
pub(crate) fn sketch_of(
    host: &EditorHost<'_>,
    entry: &FeatureEntry,
) -> Option<(Sketch3d, Solution3d)> {
    let mut sk: Sketch3d = serde_json::from_value(entry.data.clone()).ok()?;
    let mut sol = host
        .output(entry.id, "sketch")
        .and_then(Solution3d::from_value)
        .unwrap_or_default();
    // 解が古い(足したばかりの点がない)時は、保存の位置で補う
    for e in &sk.entities {
        match e {
            Entity3d::Point { id, p } => {
                sol.points.entry(*id).or_insert(*p);
            }
            Entity3d::Circle { id, radius, .. } => {
                sol.radii.entry(*id).or_insert(*radius);
            }
            Entity3d::Line { .. } => {}
        }
    }
    sk.adopt(&sol);
    Some((sk, sol))
}

/// 寸法の値(mm・ラジアン)。式が計算できない寸法は入らない。
pub(crate) fn values_of(host: &EditorHost<'_>, sk: &Sketch3d) -> HashMap<ConstraintId, f64> {
    let params = host.param_values();
    dimension_values3d(sk, &|n| params.get(n).copied()).unwrap_or_default()
}

fn to_value(s: &Sketch3d) -> serde_json::Value {
    serde_json::to_value(s).expect("JSON にできる")
}

/// 解の状態の言葉(2D のスケッチと同じ)。
pub(crate) fn status_text(sol: &Solution3d) -> String {
    if sol.is_blank() {
        return jc_sketch::NO_SHAPE.into();
    }
    match &sol.status {
        SolveStatus::WellConstrained => "完全拘束".into(),
        SolveStatus::UnderConstrained { dof } => format!("拘束不足(自由度 {dof})"),
        SolveStatus::OverConstrained { redundant } => {
            format!("余分な拘束 {} 個", redundant.len())
        }
        SolveStatus::Inconsistent { conflicting } => {
            format!("満たせない拘束 {} 個", conflicting.len())
        }
    }
}

/// 拘束の相手の言葉(パネルの一覧)。
fn refs_text(sk: &Sketch3d, kind: &Kind3d) -> String {
    let name = |id: &EntityId| match sk.entity(*id) {
        Some(Entity3d::Point { .. }) => format!("点 {}", id.0),
        Some(Entity3d::Line { .. }) => format!("線 {}", id.0),
        Some(Entity3d::Circle { .. }) => format!("円 {}", id.0),
        None => format!("? {}", id.0),
    };
    let plane = |p: &PlaneRef| match p {
        PlaneRef::Axis { plane } => plane.label().to_string(),
        PlaneRef::Face { lines } => format!("面({} 辺)", lines.0.len()),
        PlaneRef::Work { id } => sk
            .work_plane(*id)
            .map_or_else(|| format!("作図面 {id}"), |w| w.name.clone()),
    };
    match kind {
        Kind3d::Coincident { a, b }
        | Kind3d::Parallel { a, b }
        | Kind3d::Perpendicular { a, b }
        | Kind3d::Equal { a, b }
        | Kind3d::Distance { a, b, .. }
        | Kind3d::AxisDistance { a, b, .. }
        | Kind3d::Angle { a, b, .. } => format!("{}・{}", name(a), name(b)),
        Kind3d::Fix { point } => name(point),
        Kind3d::Along { line, .. } | Kind3d::Length { line, .. } => name(line),
        Kind3d::ParallelToPlane { line, plane: p } | Kind3d::PlaneAngle { line, plane: p, .. } => {
            format!("{}・{}", name(line), plane(p))
        }
        Kind3d::PointOnLine { point, line } | Kind3d::Midpoint { point, line } => {
            format!("{}・{}", name(point), name(line))
        }
        Kind3d::PointOnPlane { point, plane: p }
        | Kind3d::PlaneDistance {
            point, plane: p, ..
        } => {
            format!("{}・{}", name(point), plane(p))
        }
        Kind3d::FaceAngle { a, b, .. } => format!("{}・{}", plane(a), plane(b)),
        Kind3d::Radius { circle, .. } | Kind3d::Diameter { circle, .. } => name(circle),
    }
}

/// 作図面の向き(点は作図面の中心、`u`・`v` は作図面の上の 2 本の向き)。
pub(crate) fn work_frame(sk: &Sketch3d, sol: &Solution3d, id: u32) -> Option<DrawPlane> {
    let w = sk.work_plane(id)?;
    let (o, n) = plane_at(sk, sol, &PlaneRef::Work { id })?;
    let p = |e: &EntityId| sol.points.get(e).copied();
    let dir = |l: &EntityId| {
        let (a, b) = sk.line_ends(*l)?;
        Some(geom::sub(p(&b)?, p(&a)?))
    };
    let (axes, u, u_line) = match &w.def {
        PlaneDef::Offset { base, .. } => {
            let (a, b) = base.axes();
            (Some((a, b)), a.unit(), None)
        }
        PlaneDef::ThreePoints { a, b, .. } => {
            // 最初の 2 点を結ぶ線分があれば、それを軸の線にする
            let line = sk.entities.iter().find_map(|e| match e {
                Entity3d::Line { id, a: x, b: y, .. }
                    if (x == a && y == b) || (x == b && y == a) =>
                {
                    Some(*id)
                }
                _ => None,
            });
            (None, geom::sub(p(b)?, p(a)?), line)
        }
        PlaneDef::Tilted { line, .. } => (None, dir(line)?, Some(*line)),
        PlaneDef::Face { lines } => {
            let l = *lines.0.first()?;
            (None, dir(&l)?, Some(l))
        }
    };
    let mut u = geom::sub(u, geom::scale(n, geom::dot(u, n)));
    if geom::len(u) < 1e-9 {
        u = basis(n).0;
    }
    let u = geom::unit(u);
    Some(DrawPlane {
        origin: o,
        n,
        u,
        v: geom::cross(n, u),
        axes,
        work: Some(id),
        u_line,
    })
}

/// 作図面の値の欄の言葉(ずらす距離か傾き。ほかの決め方は値を持たない)。
pub(crate) fn plane_value_text(f: &jc_sketch::DimFormat, def: &PlaneDef) -> Option<String> {
    match def {
        PlaneDef::Offset { offset, .. } => {
            let u = jc_core::display_unit();
            Some(format!(
                "{:.*} {}",
                f.length_digits as usize,
                u.from_mm(*offset),
                u.name()
            ))
        }
        PlaneDef::Tilted { angle, .. } => Some(format!(
            "{:.*} deg",
            f.angle_digits as usize,
            angle.to_degrees()
        )),
        _ => None,
    }
}

impl Sketch3dEditor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 描く道具を選ぶ(描きかけは捨てる)。
    pub fn set_tool(&mut self, tool: Tool3) {
        self.tool = tool;
        self.start = None;
        self.typed.clear();
        // 右クリックの「最近の道具」(新しい順に 3 つ)
        if tool != Tool3::Select {
            self.recent.retain(|t| *t != tool);
            self.recent.insert(0, tool);
            self.recent.truncate(3);
        }
    }

    /// 道具を終えて選ぶ道具へ戻る(Esc、右クリックの OK・キャンセル、Enter)。描いた物は残し、
    /// 描きかけは捨てる。終えた道具は右クリックの「繰り返し」に出す。
    pub(crate) fn finish_tool(&mut self) {
        if self.tool != Tool3::Select {
            self.last_tool = Some(self.tool);
        }
        self.set_tool(Tool3::Select);
    }

    /// 何も打たずに Enter: つないでいる線分はつなぐのを終える(2D と同じ。道具は続く)。それ以外は
    /// 道具を終える(まとめてレビュー R3)。
    pub(crate) fn enter(&mut self) {
        if self.tool == Tool3::Line && self.start.is_some() {
            self.start = None;
        } else if self.tool != Tool3::Select {
            self.finish_tool();
        }
    }

    /// 作図面を替える(作図面は今の点を通ったまま)。
    pub fn set_plane(&mut self, plane: AxisPlane) {
        self.plane = plane;
        self.work = None;
    }

    /// 作った作図面に描く(描く位置は作図面の中心へ)。
    pub(crate) fn set_work(&mut self, id: u32, sk: &Sketch3d, sol: &Solution3d) {
        self.work = Some(id);
        if let Some(f) = work_frame(sk, sol, id) {
            self.anchor = f.origin;
        }
    }

    /// 面を選ばずに平面の拘束を付ける時の相手(選んでいなければ今の作図面)。
    pub(crate) fn target_plane(&self) -> PlaneRef {
        self.target.clone().unwrap_or(match self.work {
            Some(id) => PlaneRef::Work { id },
            None => PlaneRef::Axis { plane: self.plane },
        })
    }

    /// 描く平面: 作図面か、今の点を通り座標の平面に平行な平面。
    pub(crate) fn draw_plane(&self, sk: &Sketch3d, sol: &Solution3d) -> DrawPlane {
        let through = self.start.map_or(self.anchor, |s| s.p);
        if let Some(id) = self.work
            && let Some(mut f) = work_frame(sk, sol, id)
        {
            f.origin = f.project(through);
            return f;
        }
        let (a, b) = self.plane.axes();
        DrawPlane {
            origin: through,
            n: self.plane.normal().unit(),
            u: a.unit(),
            v: b.unit(),
            axes: Some((a, b)),
            work: None,
            u_line: None,
        }
    }

    /// 今の作図面を真正面から見る。
    /// 向きは 3D のビューと同じく 0.3 秒かけて替え、その間にスケッチの形全体が収まる所へ移す。
    /// 点がまだ無ければ作図面の真ん中を回す中心にする。
    pub(crate) fn look_at_plane(&mut self, sk: &Sketch3d, sol: &Solution3d) {
        let p = self.draw_plane(sk, sol);
        let o = jc_sketch_3d::to_world(&self.frame, p.origin);
        let tip = jc_sketch_3d::to_world(&self.frame, geom::add(p.origin, p.n));
        let n = [0, 1, 2].map(|i| (tip[i] - o[i]) as f32);
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for q in sol.points.values() {
            let w = jc_sketch_3d::to_world(&self.frame, *q);
            for i in 0..3 {
                lo[i] = lo[i].min(w[i] as f32);
                hi[i] = hi[i].max(w[i] as f32);
            }
        }
        if lo[0] <= hi[0] && self.rect.is_positive() {
            // 収まる中心と倍率は 3D のビューの「すべて表示」と同じ求め方(向きに依らない球で測る)
            let mut fit = self.cam;
            fit.fit_box(lo, hi, self.rect);
            self.cam.look_along_to(n, fit.target, fit.zoom);
        } else {
            self.cam.set_pivot(o.map(|v| v as f32));
            self.cam.look_along(n);
        }
    }

    /// 値を打つ欄が開いているか。
    pub(crate) fn editing(&self) -> bool {
        self.edit.is_some() || self.plane_edit.is_some()
    }

    /// 選びから作図面を作り、そこへ描くようにする。値を持つ作図面は値を打つ欄を開く。
    fn make_work_plane(&mut self, host: &EditorHost<'_>, entry: &FeatureEntry) -> Option<Sketch3d> {
        let (sk, sol) = sketch_of(host, entry)?;
        let base = match self.work.and_then(|id| sk.work_plane(id)).map(|w| &w.def) {
            Some(PlaneDef::Offset { base, .. }) => *base,
            _ => self.plane,
        };
        let at = self.draw_plane(&sk, &sol).origin;
        let def =
            match ops::work_plane_def(&sk, &sol, &self.selection, base, at, &self.target_plane()) {
                Ok(d) => d,
                Err(e) => {
                    self.note = Some(e);
                    return None;
                }
            };
        let mut s = sk.clone();
        let id = s.add_work_plane(def.clone());
        self.selection.clear();
        self.start = None;
        self.set_work(id, &s, &sol);
        self.plane_edit = plane_value_text(&self.fmt, &def).map(|t| (id, t));
        self.note = Some(format!(
            "{}({})を作りました。この作図面に描きます",
            s.work_plane(id).map_or("", |w| w.name.as_str()),
            def.label()
        ));
        Some(s)
    }

    /// 作図面の値(ずらす距離か傾き)を直し、作図面に乗る点を解き直す。
    pub(crate) fn set_plane_value(
        &mut self,
        host: &EditorHost<'_>,
        sk: &Sketch3d,
        id: u32,
        text: &str,
    ) -> Option<Sketch3d> {
        let angle = matches!(
            sk.work_plane(id).map(|w| &w.def),
            Some(PlaneDef::Tilted { .. })
        );
        let src = jc_ui::with_unit_of(text.trim(), if angle { "0 deg" } else { "0 mm" });
        let want = if angle {
            jc_core::Dim::ANGLE
        } else {
            jc_core::Dim::LENGTH
        };
        let v = match host.eval(&src) {
            Ok(q) if q.dim == want => q.value,
            _ => {
                self.note = Some(format!(
                    "「{}」は{}になりません",
                    text.trim(),
                    if angle { "角度" } else { "長さ" }
                ));
                return None;
            }
        };
        let mut s = sk.clone();
        match s.work_plane_mut(id).map(|w| &mut w.def) {
            Some(PlaneDef::Offset { offset, .. }) => *offset = v,
            Some(PlaneDef::Tilted { angle, .. }) => *angle = v,
            _ => return None,
        }
        self.note = None;
        Some(self.resolved(host, s))
    }

    /// 解いた位置を保存したスケッチ(次に描き足す時の始めの値)。
    fn resolved(&self, host: &EditorHost<'_>, mut s: Sketch3d) -> Sketch3d {
        let values = values_of(host, &s);
        if let Ok(sol) = jc_sketch_3d::solve3d(&s, &values) {
            s.adopt(&sol);
        }
        s
    }

    /// ボディの角・辺・円を取り込む(取り込んだ要素は元の形に付いて動き、ソルバでは動かない)。
    pub(crate) fn include(&mut self, sk: &Sketch3d, item: &BodyItem) -> Option<Sketch3d> {
        let mut s = sk.clone();
        let before = s.includes.len();
        let id = jc_sketch_3d::body::include_item(&mut s, item, &self.frame);
        if s.includes.len() == before {
            self.note = Some("もう取り込んであります".into());
            self.selection = vec![Sel::Ent(id)];
            return None;
        }
        self.note = None;
        Some(s)
    }

    /// ボディの角・辺・円をまとめて取り込む(面の縁かボディ全体)。全部取り込み済みなら知らせる。
    pub(crate) fn include_all(&mut self, sk: &Sketch3d, items: &[BodyItem]) -> Option<Sketch3d> {
        if let [item] = items {
            return self.include(sk, item);
        }
        let mut s = sk.clone();
        let before = s.includes.len();
        for item in items {
            jc_sketch_3d::body::include_item(&mut s, item, &self.frame);
        }
        if s.includes.len() == before {
            self.note = Some("もう取り込んであります".into());
            return None;
        }
        self.note = Some(format!("{} 個を取り込みました", s.includes.len() - before));
        Some(s)
    }

    /// 投影・交差で写す先の平面: 今の作図面。座標の平面に平行な作図面が原点を通らなければ、その高さの
    /// 作図面を作る(後から高さを直せる。写した線も付いて動く)。
    fn plane_ref(&mut self, s: &mut Sketch3d) -> PlaneRef {
        if let Some(id) = self.work {
            return PlaneRef::Work { id };
        }
        let off = self.anchor[self.plane.normal().index()];
        if off.abs() < 1e-9 {
            return PlaneRef::Axis { plane: self.plane };
        }
        let id = s.add_work_plane(PlaneDef::Offset {
            base: self.plane,
            offset: off,
        });
        self.work = Some(id);
        PlaneRef::Work { id }
    }

    /// 今の作図面へボディの形を写す(投影・交差の道具のクリック)。同じ物が写してあれば知らせる。
    pub(crate) fn add_onto(
        &mut self,
        sk: &Sketch3d,
        make: impl FnOnce(PlaneRef) -> DerivedDef,
    ) -> Option<Sketch3d> {
        let mut s = sk.clone();
        let work = self.work;
        let def = make(self.plane_ref(&mut s));
        if let Some(d) = s.derived.iter().find(|d| d.def == def) {
            self.work = work;
            self.selection = vec![Sel::Derived(d.id)];
            self.note = Some("もう写してあります".into());
            return None;
        }
        let label = def.label();
        let id = s.add_derived(def);
        self.selection.clear();
        self.note = Some(format!(
            "{label} {id} を作りました(ボディと作図面が変わると付いて行く)"
        ));
        Some(s)
    }

    /// ボディから線を求める(交差曲線・サーフェスに投影)。相手は見えているボディすべて。
    fn make_derived(
        &mut self,
        what: &str,
        host: &EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<Sketch3d> {
        let (sk, sol) = sketch_of(host, entry)?;
        let bodies: Vec<FeatureId> = host
            .bodies_before(entry.id)
            .into_iter()
            .map(|b| b.0)
            .collect();
        if bodies.is_empty() {
            self.note =
                Some("交わるボディがありません(スケッチより前に見えているボディが相手)".into());
            return None;
        }
        let mut s = sk.clone();
        let dp = self.draw_plane(&sk, &sol);
        let def = {
            let curves: Vec<EntityId> = self
                .selection
                .iter()
                .filter_map(|x| match x {
                    Sel::Ent(id) => Some(*id),
                    _ => None,
                })
                .filter(|id| {
                    matches!(
                        sk.entity(*id),
                        Some(Entity3d::Line { .. } | Entity3d::Circle { .. })
                    )
                })
                .collect();
            if curves.is_empty() {
                self.note = Some(
                    "線分か円を選んでから押す(線を作図面に垂直な向きに伸ばしてボディと交える)"
                        .into(),
                );
                return None;
            }
            if what == "isect" {
                DerivedDef::Intersection {
                    curves,
                    dir: dp.n,
                    bodies,
                }
            } else {
                DerivedDef::OnSurface {
                    curves,
                    dir: dp.n,
                    bodies,
                }
            }
        };
        let label = def.label();
        let id = s.add_derived(def);
        self.selection = vec![Sel::Derived(id)];
        self.note = Some(format!("{label} {id} を作りました(元が変わると付いて行く)"));
        Some(s)
    }

    /// 選びに形の拘束を付ける(付けられなければ知らせを出す)。
    fn apply_constraint(
        &mut self,
        name: &str,
        host: &EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<Sketch3d> {
        let (sk, _) = sketch_of(host, entry)?;
        let kinds = match ops::constraint_kinds(name, &sk, &self.selection, &self.target_plane()) {
            Ok(k) => k,
            Err(e) => {
                self.note = Some(e);
                return None;
            }
        };
        self.add(host, &sk, kinds).map(|(s, _)| s)
    }

    /// 選びに寸法を付け、値を打つ欄を開く。
    fn apply_dimension(
        &mut self,
        name: &str,
        host: &EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<Sketch3d> {
        let (sk, sol) = sketch_of(host, entry)?;
        let kind = match ops::dimension_kind(
            &self.fmt,
            name,
            &sk,
            &sol,
            &self.selection,
            &self.target_plane(),
        ) {
            Ok(k) => k,
            Err(e) => {
                self.note = Some(e);
                return None;
            }
        };
        let (s, ids) = self.add(host, &sk, vec![kind])?;
        // 置いたらすぐ値を打てるようにする(被駆動の寸法は直せない)
        if let Some(c) = ids.first().and_then(|id| s.constraint(*id))
            && !c.reference
        {
            self.edit = Some((c.id, c.kind.value().unwrap_or_default().to_string()));
        }
        Some(s)
    }

    fn add(
        &mut self,
        host: &EditorHost<'_>,
        sk: &Sketch3d,
        kinds: Vec<Kind3d>,
    ) -> Option<(Sketch3d, Vec<ConstraintId>)> {
        let values = values_of(host, sk);
        match ops::add_constraints(sk, kinds, &values, &|src| {
            host.eval(src).ok().map(|q| q.value)
        }) {
            Ok((s, ids, note)) => {
                self.note = note;
                Some((s, ids))
            }
            Err(e) => {
                self.note = Some(e);
                None
            }
        }
    }

    /// 選んだ線と円を補助線にする(もう一度で戻す)。線も円も選んでいなければ、新しく描く物を
    /// 補助線にするかを替える(2D のスケッチの X と同じ)。
    fn toggle_construction(&mut self, sk: &Sketch3d) -> Option<Sketch3d> {
        let ids: Vec<EntityId> = self
            .selection
            .iter()
            .filter_map(|s| match s {
                Sel::Ent(id) => Some(*id),
                Sel::Face(_) | Sel::Plane(_) | Sel::Derived(_) => None,
            })
            .filter(|id| {
                matches!(
                    sk.entity(*id),
                    Some(Entity3d::Line { .. } | Entity3d::Circle { .. })
                )
            })
            .collect();
        if ids.is_empty() {
            self.construction = !self.construction;
            return None;
        }
        let to = !ids
            .iter()
            .all(|id| sk.entity(*id).is_some_and(|e| e.is_construction()));
        let mut s = sk.clone();
        for id in ids {
            if let Some(
                Entity3d::Line { construction, .. } | Entity3d::Circle { construction, .. },
            ) = s.entity_mut(id)
            {
                *construction = to;
            }
        }
        Some(s)
    }

    /// 選んだ要素と拘束を消す。
    fn delete_selected(&mut self, sk: &Sketch3d) -> Option<Sketch3d> {
        let mut s = sk.clone();
        if let Some(c) = self.picked_constraint.take() {
            s.constraints.retain(|x| x.id != c);
            return Some(s);
        }
        let ids: Vec<EntityId> = self
            .selection
            .iter()
            .filter_map(|x| match x {
                Sel::Ent(id) => Some(*id),
                Sel::Face(_) | Sel::Plane(_) | Sel::Derived(_) => None,
            })
            .collect();
        let planes: Vec<u32> = self
            .selection
            .iter()
            .filter_map(|x| match x {
                Sel::Plane(id) => Some(*id),
                _ => None,
            })
            .collect();
        let derived: Vec<u32> = self
            .selection
            .iter()
            .filter_map(|x| match x {
                Sel::Derived(id) => Some(*id),
                _ => None,
            })
            .collect();
        if ids.is_empty() && planes.is_empty() && derived.is_empty() {
            return None;
        }
        s.derived.retain(|d| !derived.contains(&d.id));
        for id in planes {
            s.remove_work_plane(id);
        }
        s.remove_entities(&ids);
        if self.work.is_some_and(|w| s.work_plane(w).is_none()) {
            self.work = None;
        }
        self.selection.clear();
        Some(s)
    }

    /// パネル: 解の状態、作図面、選びに付けられる拘束と寸法、拘束の一覧。
    fn panel_ui(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<Sketch3d> {
        let (sk, sol) = sketch_of(host, entry)?;
        let mut out = None;
        let status = status_text(&sol);
        let color = match sol.status {
            _ if sol.is_blank() => ui.visuals().text_color(),
            SolveStatus::WellConstrained => jc_ui::theme::look(ui.ctx()).pal.inks.sketch_solved,
            SolveStatus::UnderConstrained { .. } => ui.visuals().text_color(),
            _ => ui.visuals().error_fg_color,
        };
        ui.colored_label(color, status);
        if let Some(e) = host.failure(entry.id) {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label("作図面");
            for p in [AxisPlane::Xy, AxisPlane::Yz, AxisPlane::Zx] {
                if ui
                    .selectable_label(self.work.is_none() && self.plane == p, p.label())
                    .on_hover_text("線・矩形・円を描く平面の向き(Tab で順に替える)")
                    .clicked()
                {
                    self.set_plane(p);
                }
            }
            for w in &sk.planes {
                if ui
                    .selectable_label(self.work == Some(w.id), &w.name)
                    .on_hover_text(format!("{}に描く(Tab で順に替える)", w.def.label()))
                    .clicked()
                {
                    self.set_work(w.id, &sk, &sol);
                }
            }
            if ui
                .small_button("正面から見る")
                .on_hover_text(LOOK_AT_HINT)
                .clicked()
            {
                self.look_at_plane(&sk, &sol);
                ui.ctx().request_repaint();
            }
        });
        if self.work.is_none() {
            // 作図面の位置(作図面に垂直な座標)。数を打つと、その高さの作図面に描ける
            let n = self.plane.normal();
            ui.horizontal(|ui| {
                ui.label(format!("作図面の位置({})", n.label().to_lowercase()));
                let id = ui.id().with("anchor3d");
                let cur = format!("{:.2}", self.anchor[n.index()]);
                let mut text = ui
                    .data(|d| d.get_temp::<String>(id))
                    .unwrap_or_else(|| cur.clone());
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0));
                if r.has_focus() || r.changed() {
                    ui.data_mut(|d| d.insert_temp(id, text.clone()));
                }
                if r.lost_focus() {
                    ui.data_mut(|d| d.remove::<String>(id));
                    match host.eval(&jc_ui::with_unit_of(text.trim(), "0 mm")) {
                        Ok(q) if q.dim == jc_core::Dim::LENGTH => self.anchor[n.index()] = q.value,
                        _ => self.note = Some(format!("「{}」は長さになりません", text.trim())),
                    }
                }
                ui.label("mm");
            });
        }
        if ui
            .button("作図面を作る…")
            .on_hover_text(ops::WORK_PLANE_HINT)
            .clicked()
        {
            out = self.make_work_plane(host, entry);
        }
        // 作った作図面: 値(ずらす距離・傾き)、傾ける側、消す
        let mut set_plane: Option<(u32, String)> = None;
        let mut flip = None;
        let mut drop = None;
        for w in &sk.planes {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{}({})", w.name, w.def.label()));
                if let Some(v) = plane_value_text(&self.fmt, &w.def) {
                    let id = ui.id().with(("wplane3d", w.id));
                    let mut text = ui
                        .data(|d| d.get_temp::<String>(id))
                        .unwrap_or_else(|| v.clone());
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0));
                    if r.has_focus() || r.changed() {
                        ui.data_mut(|d| d.insert_temp(id, text.clone()));
                    }
                    if r.lost_focus() {
                        ui.data_mut(|d| d.remove::<String>(id));
                        if text.trim() != v {
                            set_plane = Some((w.id, text.clone()));
                        }
                    }
                }
                if let PlaneDef::Tilted { flip: f, .. } = &w.def
                    && ui
                        .small_button("反対へ")
                        .on_hover_text("傾ける側を替える")
                        .clicked()
                {
                    flip = Some((w.id, !*f));
                }
                if ui
                    .small_button("削除")
                    .on_hover_text("作図面と、作図面を相手にした拘束を消す")
                    .clicked()
                {
                    drop = Some(w.id);
                }
            });
        }
        if let Some((id, text)) = set_plane {
            out = self.set_plane_value(host, &sk, id, &text).or(out);
        }
        if let Some((id, f)) = flip {
            let mut s = sk.clone();
            if let Some(PlaneDef::Tilted { flip, .. }) = s.work_plane_mut(id).map(|w| &mut w.def) {
                *flip = f;
            }
            out = Some(self.resolved(host, s));
        }
        if let Some(id) = drop {
            let mut s = sk.clone();
            s.remove_work_plane(id);
            if self.work == Some(id) {
                self.work = None;
            }
            self.selection.retain(|x| *x != Sel::Plane(id));
            if self.target == Some(PlaneRef::Work { id }) {
                self.target = None;
            }
            out = Some(s);
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("相手の平面");
            let cur = self.target_plane();
            let mut t = cur.clone();
            let hint =
                "面を選ばずに「平面との角度」「平面との距離」「平面の上」などを付ける時の相手";
            for p in [AxisPlane::Xy, AxisPlane::Yz, AxisPlane::Zx] {
                ui.radio_value(&mut t, PlaneRef::Axis { plane: p }, p.label())
                    .on_hover_text(hint);
            }
            for w in &sk.planes {
                ui.radio_value(&mut t, PlaneRef::Work { id: w.id }, &w.name)
                    .on_hover_text(hint);
            }
            if t != cur {
                self.target = Some(t);
            }
        });
        ui.checkbox(&mut self.construction, "補助線で描く(X)");
        // ボディから取り込んだ物と、求めた線(元のボディが変わると付いて行く)
        if self.tool == Tool3::Include {
            ui.label("取り込む物");
            let pick = &mut self.include_pick;
            ui.radio_value(pick, IncludePick::Parts, "角・辺・円(押した物)");
            ui.radio_value(pick, IncludePick::Face, "面の縁(押した面の辺と円)");
            ui.radio_value(pick, IncludePick::Body, "ボディの角・辺・円すべて");
            ui.label("ボディをクリックして取り込む(Esc でやめる)");
        }
        // 投影・交差: 写す物(2D の投影・交差と同じ)。写した線は今の作図面に乗り、元のボディと
        // 作図面が変わると付いて行く
        if self.tool == Tool3::Project {
            ui.label("写す物");
            let pick = &mut self.project_pick;
            ui.radio_value(
                pick,
                ProjectPick::Parts,
                "面・辺・点(押した角・辺・円の中心、面なら面の縁)",
            );
            ui.radio_value(
                pick,
                ProjectPick::Silhouette,
                "ボディのシルエット(押したボディの外形)",
            );
            ui.radio_value(pick, ProjectPick::Edges, "ボディの辺と角すべて");
            ui.label("ボディをクリックして今の作図面へ写す(Esc でやめる)");
        }
        if self.tool == Tool3::Intersect {
            ui.label("写す物");
            let body = &mut self.intersect_body;
            ui.radio_value(body, false, "押した線(面ごとの断面)");
            ui.radio_value(body, true, "ボディ全体(押したボディの断面すべて)");
            ui.label("作図面の上の断面の線をクリックして写す(Esc でやめる)");
        }
        if !sk.includes.is_empty() || !sk.derived.is_empty() {
            ui.separator();
            if !sk.includes.is_empty() {
                ui.label(format!("ボディから取り込んだ要素 {} 個", sk.includes.len()))
                    .on_hover_text("元の角・辺・円が変わると付いて行く。ソルバでは動かない");
            }
            let mut drop = None;
            for d in &sk.derived {
                ui.horizontal(|ui| {
                    let on = self.selection.contains(&Sel::Derived(d.id));
                    let n = sol.derived.get(&d.id).map_or(0, Vec::len);
                    let text = if n == 0 {
                        egui::RichText::new(format!("{} {}(交わりなし)", d.def.label(), d.id))
                            .color(ui.visuals().error_fg_color)
                    } else {
                        egui::RichText::new(format!("{} {}(線 {n} 本)", d.def.label(), d.id))
                    };
                    if ui.selectable_label(on, text).clicked() {
                        self.selection = if on { vec![] } else { vec![Sel::Derived(d.id)] };
                    }
                    if ui.small_button("削除").clicked() {
                        drop = Some(d.id);
                    }
                });
            }
            if let Some(id) = drop {
                let mut s = sk.clone();
                s.derived.retain(|d| d.id != id);
                self.selection.retain(|x| *x != Sel::Derived(id));
                out = Some(s);
            }
        }
        ui.separator();
        // 選んだ物と、付けられる拘束・寸法(選んでから押す)
        let p = ops::classify(&sk, &self.selection);
        ui.label(format!(
            "選択: 点 {}・線分 {}・円 {}・面と作図面 {}",
            p.points.len(),
            p.lines.len(),
            p.circles.len(),
            p.faces.len()
        ));
        if !p.is_empty() {
            let target = self.target_plane();
            let cons: Vec<&str> = ops::CONSTRAINTS
                .iter()
                .map(|c| c.0)
                .filter(|n| ops::constraint_kinds(n, &sk, &self.selection, &target).is_ok())
                .collect();
            let dims: Vec<&str> = ops::DIMENSIONS
                .iter()
                .map(|c| c.0)
                .filter(|n| {
                    ops::dimension_kind(&self.fmt, n, &sk, &sol, &self.selection, &target).is_ok()
                })
                .collect();
            ui.horizontal_wrapped(|ui| {
                for name in cons {
                    if ui.button(name).clicked() {
                        out = self.apply_constraint(name, host, entry);
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                for name in dims {
                    if ui.button(format!("{name}…")).clicked() {
                        out = self.apply_dimension(name, host, entry);
                    }
                }
            });
        }
        ui.separator();
        ui.label(format!("拘束と寸法 {} 個", sk.constraints.len()));
        let f = self.fmt;
        let mut remove = None;
        let mut toggle = None;
        let mut set_value: Option<(ConstraintId, String)> = None;
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .id_salt("constraints3d")
            .show(ui, |ui| {
                for c in &sk.constraints {
                    ui.horizontal(|ui| {
                        let on = self.picked_constraint == Some(c.id);
                        let bad = match &sol.status {
                            SolveStatus::Inconsistent { conflicting } => {
                                conflicting.contains(&c.id)
                            }
                            SolveStatus::OverConstrained { redundant } => redundant.contains(&c.id),
                            _ => false,
                        };
                        let mut text = egui::RichText::new(format!(
                            "{} {}",
                            c.kind.label(),
                            refs_text(&sk, &c.kind)
                        ));
                        if bad {
                            text = text.color(ui.visuals().error_fg_color);
                        }
                        if ui.selectable_label(on, text).clicked() {
                            self.picked_constraint = (!on).then_some(c.id);
                        }
                        if let Some(v) = c.kind.value() {
                            if c.reference {
                                let now = sol.measured.get(&c.id).copied().unwrap_or(0.0);
                                let shown = if c.kind.is_angle() {
                                    f.angle(now)
                                } else {
                                    f.length(now)
                                };
                                ui.label(format!("({shown})"))
                                    .on_hover_text("測るだけの寸法(被駆動)");
                            } else {
                                let id = ui.id().with(("dim3d", c.id.0));
                                let mut text = ui
                                    .data(|d| d.get_temp::<String>(id))
                                    .unwrap_or_else(|| v.to_string());
                                let r = ui
                                    .add(egui::TextEdit::singleline(&mut text).desired_width(90.0));
                                if r.has_focus() || r.changed() {
                                    ui.data_mut(|d| d.insert_temp(id, text.clone()));
                                }
                                if r.lost_focus() {
                                    ui.data_mut(|d| d.remove::<String>(id));
                                    if text.trim() != v {
                                        set_value = Some((c.id, text.clone()));
                                    }
                                }
                            }
                        }
                        // 半径と直径の切り替え(値は同じ大きさの円になるよう直す)
                        let swap = match c.kind {
                            Kind3d::Radius { .. } => Some("直径にする"),
                            Kind3d::Diameter { .. } => Some("半径にする"),
                            _ => None,
                        };
                        if let Some(name) = swap
                            && ui.small_button(name).clicked()
                        {
                            toggle = Some(c.id);
                        }
                        if ui.small_button("外す").clicked() {
                            remove = Some(c.id);
                        }
                    });
                }
            });
        if let Some(id) = remove {
            let mut s = sk.clone();
            s.constraints.retain(|c| c.id != id);
            if self.picked_constraint == Some(id) {
                self.picked_constraint = None;
            }
            out = Some(s);
        }
        if let Some(id) = toggle {
            let mut s = sk.clone();
            if s.toggle_radius(id) {
                out = Some(s);
            }
        }
        if let Some((id, text)) = set_value {
            out = self.set_dimension(host, &sk, id, &text).or(out);
        }
        if let Some(n) = &self.note {
            ui.separator();
            ui.colored_label(jc_ui::theme::look(ui.ctx()).pal.inks.sketch_note, n);
        }
        out
    }

    /// 寸法の値を直す(式が計算できなければ断る)。数だけなら寸法の単位を補う。
    pub(crate) fn set_dimension(
        &mut self,
        host: &EditorHost<'_>,
        sk: &Sketch3d,
        id: ConstraintId,
        text: &str,
    ) -> Option<Sketch3d> {
        let c = sk.constraint(id)?;
        let angle = c.kind.is_angle();
        let src = jc_ui::with_unit_of(text.trim(), if angle { "0 deg" } else { "0 mm" });
        let want = if angle {
            jc_core::Dim::ANGLE
        } else {
            jc_core::Dim::LENGTH
        };
        match host.eval(&src) {
            Ok(q) if q.dim == want => {}
            Ok(_) => {
                self.note = Some(format!(
                    "「{src}」は{}になりません",
                    if angle { "角度" } else { "長さ" }
                ));
                return None;
            }
            Err(e) => {
                self.note = Some(format!("この値では置けません: {e}"));
                return None;
            }
        }
        let mut s = sk.clone();
        if let Some(v) = s
            .constraints
            .iter_mut()
            .find(|c| c.id == id)
            .and_then(|c| c.kind.value_mut())
        {
            *v = src;
        }
        self.note = None;
        // 解いた位置を保存しておく(次に描き足す時の始めの値)
        let values = values_of(host, &s);
        if let Ok(sol) = jc_sketch_3d::solve3d(&s, &values) {
            s.adopt(&sol);
        }
        Some(s)
    }
}

fn tool_id(t: Tool3) -> String {
    format!("tool3:{t:?}")
}

impl FeatureEditor for Sketch3dEditor {
    fn fresh(&self) -> Box<dyn FeatureEditor> {
        Box::new(Sketch3dEditor::new())
    }

    fn handles(&self, type_id: &str) -> bool {
        type_id == SKETCH3D
    }

    fn takes_view(&self) -> bool {
        true
    }

    fn view(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        self.fmt = jc_sketch::dim_format(host.registry);
        view::view(self, ui, host, entry).map(|s| to_value(&s))
    }

    fn panel(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        self.fmt = jc_sketch::dim_format(host.registry);
        self.panel_ui(ui, host, entry).map(|s| to_value(&s))
    }

    fn panel_title(&self) -> Option<(&'static str, &'static str)> {
        Some(("3D スケッチ", "sketch3d"))
    }

    fn edit_label(&self) -> &'static str {
        "3D スケッチを編集"
    }

    fn tools(&self, host: &EditorHost<'_>, entry: &FeatureEntry) -> Vec<ToolGroup> {
        // リボンのまとまりの色(2D のスケッチと同じ)
        let works: Vec<(u32, String, &'static str)> = sketch_of(host, entry)
            .map(|(sk, _)| {
                sk.planes
                    .iter()
                    .map(|w| (w.id, w.name.clone(), w.def.label()))
                    .collect()
            })
            .unwrap_or_default();
        let draw = |(t, label, icon, key, hint): &(Tool3, &str, &str, &str, &str)| {
            let hint = match jc_ui::keys::label(key) {
                k if k.is_empty() => hint.to_string(),
                k => format!("{hint}({k})"),
            };
            EditorTool::new(&tool_id(*t), label, icon, &hint).on(self.tool == *t)
        };
        let cons = |(name, icon, what): &(&str, &str, &str)| {
            EditorTool::new(
                &format!("c3:{name}"),
                name,
                icon,
                &format!("{name}: {what}を選んでから押す"),
            )
        };
        let dims = |(name, icon, what): &(&str, &str, &str)| {
            EditorTool::new(
                &format!("d3:{name}"),
                name,
                icon,
                &format!("{name}: {what}を選んでから押す。押すと値を打つ欄が開く"),
            )
        };
        let plane = |p: AxisPlane| {
            EditorTool::new(
                &format!("plane3:{}", p.label()),
                p.label(),
                "wplane",
                &format!("{} に平行な作図面に描く(Tab で順に替える)", p.label()),
            )
            .on(self.work.is_none() && self.plane == p)
        };
        vec![
            ToolGroup {
                name: "3D 作成".into(),
                tools: DRAW3[..3].iter().map(draw).collect(),
                more: DRAW3[3..].iter().map(draw).collect(),
                ..Default::default()
            },
            ToolGroup {
                name: "作図面".into(),
                tools: [AxisPlane::Xy, AxisPlane::Yz, AxisPlane::Zx]
                    .into_iter()
                    .map(plane)
                    .chain([
                        EditorTool::new(
                            "act3:workplane",
                            "作図面を作る",
                            "wplaneadd",
                            &format!("作図面を作る: {}", ops::WORK_PLANE_HINT),
                        ),
                        EditorTool::new("act3:lookat", "正面から見る", "lookat", LOOK_AT_HINT),
                    ])
                    .collect(),
                more: works
                    .iter()
                    .map(|(id, name, how)| {
                        EditorTool::new(
                            &format!("wp3:{id}"),
                            name,
                            "wplane",
                            &format!("{name}({how})に描く(Tab で順に替える)"),
                        )
                        .on(self.work == Some(*id))
                    })
                    .collect(),
                ..Default::default()
            },
            ToolGroup {
                name: "取り込み".into(),
                tools: vec![
                    EditorTool::new(
                        &tool_id(Tool3::Include),
                        "3D 取り込み",
                        "project",
                        "ボディの角・辺・円をクリックしてスケッチに取り込む。元の形が変わると付いて行く",
                    )
                    .on(self.tool == Tool3::Include),
                    EditorTool::new(
                        &tool_id(Tool3::Project),
                        "投影",
                        "project",
                        "ボディの角・辺・面・シルエットをクリックして、今の作図面へ垂直に写す。ボディと作図面が変わると付いて行く",
                    )
                    .on(self.tool == Tool3::Project),
                    EditorTool::new(
                        &tool_id(Tool3::Intersect),
                        "交差",
                        "intersect",
                        "ボディを今の作図面で切った線をクリックして写す。ボディと作図面が変わると付いて行く",
                    )
                    .on(self.tool == Tool3::Intersect),
                ],
                more: vec![
                    EditorTool::new(
                        "act3:isect",
                        "交差曲線",
                        "isectcurve",
                        "選んだ線分か円を作図面に垂直に伸ばし、ボディの表と交わる線を作る",
                    ),
                    EditorTool::new(
                        "act3:onsurf",
                        "サーフェスに投影",
                        "projsurf",
                        "選んだ線分か円を作図面に垂直な向きで、一番近いボディの表へ写す",
                    ),
                ],
                ..Default::default()
            },
            ToolGroup {
                name: "3D 修正".into(),
                tools: vec![
                    EditorTool::new(
                        "act3:construction",
                        "補助線の切り替え",
                        "dashline",
                        "選んだ線と円を補助線にする(もう一度で戻す)。何も選んでいなければ、新しく描く物を補助線にする(X)",
                    )
                    .on(self.construction),
                    EditorTool::new("act3:delete", "削除", "trash", "選んだ要素か拘束を消す(Delete)"),
                ],
                more: vec![],
                ..Default::default()
            },
            ToolGroup {
                name: "3D 拘束".into(),
                tools: ops::CONSTRAINTS[..5].iter().map(cons).collect(),
                more: ops::CONSTRAINTS[5..].iter().map(cons).collect(),
                ..Default::default()
            },
            ToolGroup {
                name: "3D 寸法".into(),
                tools: ops::DIMENSIONS[..1].iter().map(dims).collect(),
                more: ops::DIMENSIONS[1..].iter().map(dims).collect(),
                ..Default::default()
            },
            ToolGroup {
                name: "選択".into(),
                tools: vec![
                    EditorTool::new(&tool_id(Tool3::Select), "選ぶ", "select", "点・線・円・面をクリックで選ぶ。Ctrl か Shift で足す。点はドラッグで動かす")
                        .on(self.tool == Tool3::Select),
                ],
                more: vec![],
                ..Default::default()
            },
        ]
    }

    fn use_tool(
        &mut self,
        id: &str,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        let tools = [
            Tool3::Select,
            Tool3::Include,
            Tool3::Project,
            Tool3::Intersect,
        ]
        .into_iter()
        .chain(DRAW3.iter().map(|d| d.0));
        if let Some(t) = tools.into_iter().find(|t| tool_id(*t) == id) {
            self.set_tool(t);
            return None;
        }
        if let Some(p) = id.strip_prefix("plane3:") {
            if let Some(p) = [AxisPlane::Xy, AxisPlane::Yz, AxisPlane::Zx]
                .into_iter()
                .find(|x| x.label() == p)
            {
                self.set_plane(p);
            }
            return None;
        }
        if id == "act3:workplane" {
            return self.make_work_plane(host, entry).map(|s| to_value(&s));
        }
        if id == "act3:lookat" {
            if let Some((sk, sol)) = sketch_of(host, entry) {
                self.look_at_plane(&sk, &sol);
            }
            return None;
        }
        if let Some(w) = id.strip_prefix("wp3:").and_then(|w| w.parse::<u32>().ok()) {
            if let Some((sk, sol)) = sketch_of(host, entry)
                && sk.work_plane(w).is_some()
            {
                self.set_work(w, &sk, &sol);
            }
            return None;
        }
        if let Some(what) = id.strip_prefix("act3:")
            && ["isect", "onsurf"].contains(&what)
        {
            return self.make_derived(what, host, entry).map(|s| to_value(&s));
        }
        if let Some(name) = id.strip_prefix("c3:") {
            return self
                .apply_constraint(name, host, entry)
                .map(|s| to_value(&s));
        }
        if let Some(name) = id.strip_prefix("d3:") {
            return self
                .apply_dimension(name, host, entry)
                .map(|s| to_value(&s));
        }
        let (sk, _) = sketch_of(host, entry)?;
        match id {
            "act3:construction" => self.toggle_construction(&sk),
            "act3:delete" => self.delete_selected(&sk),
            _ => None,
        }
        .map(|s| to_value(&s))
    }

    fn status_hints(&self) -> Vec<(String, String)> {
        let s = |k: &str, t: &str| (k.to_string(), t.to_string());
        let mut v = match self.tool {
            Tool3::Select => vec![
                s("クリック", "選ぶ(Ctrl・Shift で足す)"),
                s("ドラッグ", "点を動かす・囲んで選ぶ"),
                s("ダブルクリック", "寸法の値を直す"),
            ],
            Tool3::Line if self.start.is_some() => vec![
                s("クリック", "次の点"),
                s("数", "長さを打って Enter"),
                s("Esc", "つなぐのをやめる"),
            ],
            Tool3::Circle if self.start.is_some() => vec![s("クリック", "円周の点")],
            Tool3::Rectangle if self.start.is_some() => vec![s("クリック", "対角の点")],
            Tool3::Include => vec![s("クリック", "ボディの角・辺・円を取り込む")],
            Tool3::Project => vec![s("クリック", "ボディの形を今の作図面へ写す")],
            Tool3::Intersect => vec![s("クリック", "作図面の上の断面の線を写す")],
            _ => vec![s("クリック", "置く(点の上ならその点)")],
        };
        v.push(s("Tab", "作図面を替える"));
        v.push(s("右ドラッグ", "回す"));
        v.push(s("ホイール押し", "動かす"));
        v
    }

    fn reset(&mut self) {
        self.on_plane = None;
        self.start = None;
        self.drag = None;
        self.edit = None;
        self.plane_edit = None;
        self.selection.clear();
        self.picked_constraint = None;
        self.typed.clear();
        self.note = None;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
