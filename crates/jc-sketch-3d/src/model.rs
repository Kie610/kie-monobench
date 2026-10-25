//! 3D スケッチの要素と拘束。
//! 点は 3 次元の座標を持ち、線分は 2 点を、円は中心と半径と向き(法線)を持つ。
//! 座標はスケッチの基準の平面の座標(x・y が平面の上、z が法線の向き)。

use jc_core::FeatureId;
use jc_sketch::{ConstraintId, EntityId, ProjectionSource, SketchPlane};
use serde::{Deserialize, Serialize};

pub type P3 = [f64; 3];

/// 3D スケッチ(機能のデータ)。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Sketch3d {
    /// 基準の平面(座標の x・y・z の向きを決める。既定は XY 平面)
    #[serde(default)]
    pub plane: SketchPlane,
    #[serde(default)]
    pub entities: Vec<Entity3d>,
    #[serde(default)]
    pub constraints: Vec<Constraint3d>,
    /// 作図面(名前を付けて持つ平面。描く平面と、拘束・寸法の相手にする)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub planes: Vec<WorkPlane3d>,
    /// 取り込んだボディの角・辺
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<Include3d>,
    /// ボディから求める線(交差・交差曲線・サーフェスに投影)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived: Vec<Derived3d>,
    /// 寸法の名前(`d1` など。2D のスケッチと同じ。[`jc_sketch::dims`])
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub dim_names: std::collections::BTreeMap<ConstraintId, String>,
}

/// 取り込んだボディの角(点)・辺(線分か円)・円の中心(点)。計算の時に元の形から位置を
/// 取り直し、ソルバでは動かさない(ほかの線の拘束の相手にはなる)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Include3d {
    pub entity: EntityId,
    #[serde(flatten)]
    pub source: ProjectionSource,
}

/// ボディから求める線。計算の時に、元のボディと線から求め直す(スケッチの要素ではなく、
/// 寸法や拘束の相手にはならない)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Derived3d {
    pub id: u32,
    #[serde(flatten)]
    pub def: DerivedDef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum DerivedDef {
    /// ボディと平面の交わり(断面の線)。`pieces` が空ならボディの断面すべて、
    /// あれば面ごとの断面のうちその線だけ(2D の交差の「押した線」と同じ)
    Section {
        plane: PlaneRef,
        bodies: Vec<FeatureId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pieces: Vec<SectionPiece>,
    },
    /// 線を `dir` の向きに伸ばした面と、ボディの表の交わり
    Intersection {
        curves: Vec<EntityId>,
        dir: P3,
        bodies: Vec<FeatureId>,
    },
    /// 線を `dir` の向きにボディの表へ写した線(一番近い表)
    OnSurface {
        curves: Vec<EntityId>,
        dir: P3,
        bodies: Vec<FeatureId>,
    },
    /// ボディの形を平面へ垂直に写した線(投影。写す物は 2D の投影と同じ 3 択。まとめてレビュー R4)
    Project {
        plane: PlaneRef,
        what: ProjectWhat,
        bodies: Vec<FeatureId>,
    },
}

/// 面ごとの断面の線(面の名前と、その面の中でつながる順の何番目か。2D の交差と同じ数え方)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionPiece {
    pub face: String,
    pub index: u32,
}

/// 投影で写す物(2D の投影の「写す物」と同じ)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectWhat {
    /// 押した角・辺・円の中心(面なら面の縁の辺)
    Parts { sources: Vec<ProjectionSource> },
    /// ボディを平面へ写した外形
    Silhouette,
    /// ボディの辺と角すべて
    Edges,
}

impl DerivedDef {
    pub fn bodies(&self) -> &[FeatureId] {
        match self {
            DerivedDef::Section { bodies, .. }
            | DerivedDef::Intersection { bodies, .. }
            | DerivedDef::OnSurface { bodies, .. }
            | DerivedDef::Project { bodies, .. } => bodies,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            DerivedDef::Section { .. } => "交差",
            DerivedDef::Intersection { .. } => "交差曲線",
            DerivedDef::OnSurface { .. } => "サーフェスに投影",
            DerivedDef::Project { .. } => "投影",
        }
    }

    /// 写す先の平面(交差と投影)。
    pub fn plane(&self) -> Option<&PlaneRef> {
        match self {
            DerivedDef::Section { plane, .. } | DerivedDef::Project { plane, .. } => Some(plane),
            _ => None,
        }
    }
}

/// 作図面。座標の平面をずらしたもの、3 点を通るもの、線分を通り傾けたもの、面の上のもの。
/// 点や線から決まる作図面は、その点や線が動けば付いて動く。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkPlane3d {
    pub id: u32,
    pub name: String,
    #[serde(flatten)]
    pub def: PlaneDef,
}

/// 作図面の決め方。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum PlaneDef {
    /// 座標の平面に平行で、法線の向きに `offset`(mm)ずらした平面
    Offset { base: AxisPlane, offset: f64 },
    /// 3 点を通る平面
    ThreePoints {
        a: EntityId,
        b: EntityId,
        c: EntityId,
    },
    /// 線分を通り、相手の平面から `angle`(ラジアン)傾けた平面(線分が相手の平面に
    /// 乗っている時、相手の平面との角度が `angle` になる)。`flip` で傾ける側を替える
    Tilted {
        line: EntityId,
        base: PlaneRef,
        angle: f64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
    },
    /// 面の平面
    Face { lines: FaceRef },
}

impl PlaneDef {
    /// 決めるのに使う要素(点・線分。面の線分も含む)。
    pub fn refs(&self) -> Vec<EntityId> {
        match self {
            PlaneDef::Offset { .. } => vec![],
            PlaneDef::ThreePoints { a, b, c } => vec![*a, *b, *c],
            PlaneDef::Tilted { line, base, .. } => {
                let mut v = vec![*line];
                if let PlaneRef::Face { lines } = base {
                    v.extend(lines.0.iter().copied());
                }
                v
            }
            PlaneDef::Face { lines } => lines.0.clone(),
        }
    }

    /// 決め方の言葉(パネルに出す)。
    pub fn label(&self) -> &'static str {
        match self {
            PlaneDef::Offset { .. } => "ずらした平面",
            PlaneDef::ThreePoints { .. } => "3 点を通る",
            PlaneDef::Tilted { .. } => "線を通り傾ける",
            PlaneDef::Face { .. } => "面の上",
        }
    }
}

/// 座標の軸(スケッチの基準の平面の x・y と法線の z)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    pub fn unit(self) -> P3 {
        let mut v = [0.0; 3];
        v[self.index()] = 1.0;
        v
    }

    pub fn label(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::Y => "Y",
            Axis::Z => "Z",
        }
    }
}

/// 座標の平面(原点を通る。角度の寸法の相手)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AxisPlane {
    Xy,
    Yz,
    Zx,
}

impl AxisPlane {
    pub const ALL: [AxisPlane; 3] = [AxisPlane::Xy, AxisPlane::Yz, AxisPlane::Zx];

    /// 平面の法線の軸。
    pub fn normal(self) -> Axis {
        match self {
            AxisPlane::Xy => Axis::Z,
            AxisPlane::Yz => Axis::X,
            AxisPlane::Zx => Axis::Y,
        }
    }

    /// 平面の上の 2 つの軸(描く時の横と縦)。
    pub fn axes(self) -> (Axis, Axis) {
        match self {
            AxisPlane::Xy => (Axis::X, Axis::Y),
            AxisPlane::Yz => (Axis::Y, Axis::Z),
            AxisPlane::Zx => (Axis::Z, Axis::X),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AxisPlane::Xy => "XY 平面",
            AxisPlane::Yz => "YZ 平面",
            AxisPlane::Zx => "ZX 平面",
        }
    }
}

/// 3D スケッチの要素。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entity3d {
    Point {
        id: EntityId,
        p: P3,
    },
    Line {
        id: EntityId,
        a: EntityId,
        b: EntityId,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        construction: bool,
    },
    /// 円(`normal` の向きの平面の上。向きは描いた時の平面で決まり、解く時は動かない。
    /// 作図面 `plane` に描いた円は、その作図面の向きに付いて回る)
    Circle {
        id: EntityId,
        center: EntityId,
        radius: f64,
        normal: P3,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        construction: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plane: Option<u32>,
    },
}

impl Entity3d {
    pub fn id(&self) -> EntityId {
        match self {
            Entity3d::Point { id, .. }
            | Entity3d::Line { id, .. }
            | Entity3d::Circle { id, .. } => *id,
        }
    }

    /// 使う点(線分の両端、円の中心)。
    pub fn points(&self) -> Vec<EntityId> {
        match self {
            Entity3d::Point { .. } => vec![],
            Entity3d::Line { a, b, .. } => vec![*a, *b],
            Entity3d::Circle { center, .. } => vec![*center],
        }
    }

    pub fn is_construction(&self) -> bool {
        match self {
            Entity3d::Point { .. } => false,
            Entity3d::Line { construction, .. } | Entity3d::Circle { construction, .. } => {
                *construction
            }
        }
    }
}

/// 面(閉じた線分の輪で、点がみな 1 つの平面に乗るもの)を、輪の線分の番号で指す。
/// 番号は小さい順に並べる(同じ面を同じ値にする)。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FaceRef(pub Vec<EntityId>);

impl FaceRef {
    pub fn new(mut lines: Vec<EntityId>) -> Self {
        lines.sort();
        lines.dedup();
        FaceRef(lines)
    }
}

/// 角度の相手の平面(座標の平面か、見つけた面)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaneRef {
    Axis {
        plane: AxisPlane,
    },
    Face {
        lines: FaceRef,
    },
    /// 作図面
    Work {
        id: u32,
    },
}

/// 拘束の種類。寸法(`value` を持つもの)は式で書き、パラメータを参照できる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind3d {
    /// 2 点を同じ位置に
    Coincident {
        a: EntityId,
        b: EntityId,
    },
    /// 点(円なら中心と半径)を今の位置に固定する
    Fix {
        point: EntityId,
    },
    /// 線分を軸に沿わせる
    Along {
        line: EntityId,
        axis: Axis,
    },
    /// 線分を座標の平面か面に平行にする(線が平面の上を走る向き)
    ParallelToPlane {
        line: EntityId,
        plane: PlaneRef,
    },
    Parallel {
        a: EntityId,
        b: EntityId,
    },
    Perpendicular {
        a: EntityId,
        b: EntityId,
    },
    /// 線分どうしなら長さ、円どうしなら半径を等しく
    Equal {
        a: EntityId,
        b: EntityId,
    },
    PointOnLine {
        point: EntityId,
        line: EntityId,
    },
    Midpoint {
        point: EntityId,
        line: EntityId,
    },
    /// 点を面(その面の平面)の上に
    PointOnPlane {
        point: EntityId,
        plane: PlaneRef,
    },
    // ---- 寸法 ----
    /// 線分の長さ
    Length {
        line: EntityId,
        value: String,
    },
    /// 2 点の間の距離
    Distance {
        a: EntityId,
        b: EntityId,
        value: String,
    },
    /// 2 点の軸の向きの差(高さの差など)。`b - a` の向きは作った時の向き(`flip` で逆)
    AxisDistance {
        a: EntityId,
        b: EntityId,
        axis: Axis,
        value: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
    },
    /// 2 本の線分の間の角度(0〜180°。線分の a→b の向きで測る)
    Angle {
        a: EntityId,
        b: EntityId,
        value: String,
    },
    /// 線分と平面(座標の平面か面)の間の角度(0〜90°)
    PlaneAngle {
        line: EntityId,
        plane: PlaneRef,
        value: String,
    },
    /// 2 つの面(か座標の平面)の間の角度(平面どうしの角度、0〜90°)
    FaceAngle {
        a: PlaneRef,
        b: PlaneRef,
        value: String,
    },
    /// 点と平面(座標の平面か面)の距離
    PlaneDistance {
        point: EntityId,
        plane: PlaneRef,
        value: String,
    },
    Radius {
        circle: EntityId,
        value: String,
    },
    Diameter {
        circle: EntityId,
        value: String,
    },
}

impl Kind3d {
    /// 寸法の式(寸法でなければ `None`)。
    pub fn value(&self) -> Option<&str> {
        match self {
            Kind3d::Length { value, .. }
            | Kind3d::Distance { value, .. }
            | Kind3d::AxisDistance { value, .. }
            | Kind3d::Angle { value, .. }
            | Kind3d::PlaneAngle { value, .. }
            | Kind3d::FaceAngle { value, .. }
            | Kind3d::PlaneDistance { value, .. }
            | Kind3d::Radius { value, .. }
            | Kind3d::Diameter { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn value_mut(&mut self) -> Option<&mut String> {
        match self {
            Kind3d::Length { value, .. }
            | Kind3d::Distance { value, .. }
            | Kind3d::AxisDistance { value, .. }
            | Kind3d::Angle { value, .. }
            | Kind3d::PlaneAngle { value, .. }
            | Kind3d::FaceAngle { value, .. }
            | Kind3d::PlaneDistance { value, .. }
            | Kind3d::Radius { value, .. }
            | Kind3d::Diameter { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn is_angle(&self) -> bool {
        matches!(
            self,
            Kind3d::Angle { .. } | Kind3d::PlaneAngle { .. } | Kind3d::FaceAngle { .. }
        )
    }

    /// 関わる要素(線分・点・円。面の線分も含む)。
    pub fn refs(&self) -> Vec<EntityId> {
        let plane = |p: &PlaneRef| match p {
            PlaneRef::Axis { .. } | PlaneRef::Work { .. } => vec![],
            PlaneRef::Face { lines } => lines.0.clone(),
        };
        match self {
            Kind3d::Coincident { a, b }
            | Kind3d::Parallel { a, b }
            | Kind3d::Perpendicular { a, b }
            | Kind3d::Equal { a, b }
            | Kind3d::Distance { a, b, .. }
            | Kind3d::AxisDistance { a, b, .. }
            | Kind3d::Angle { a, b, .. } => vec![*a, *b],
            Kind3d::Fix { point } => vec![*point],
            Kind3d::Along { line, .. } | Kind3d::Length { line, .. } => vec![*line],
            Kind3d::PointOnLine { point, line } | Kind3d::Midpoint { point, line } => {
                vec![*point, *line]
            }
            Kind3d::ParallelToPlane { line, plane: p }
            | Kind3d::PlaneAngle { line, plane: p, .. } => {
                let mut v = vec![*line];
                v.extend(plane(p));
                v
            }
            Kind3d::PointOnPlane { point, plane: p }
            | Kind3d::PlaneDistance {
                point, plane: p, ..
            } => {
                let mut v = vec![*point];
                v.extend(plane(p));
                v
            }
            Kind3d::FaceAngle { a, b, .. } => {
                let mut v = plane(a);
                v.extend(plane(b));
                v
            }
            Kind3d::Radius { circle, .. } | Kind3d::Diameter { circle, .. } => vec![*circle],
        }
    }

    /// 相手の平面(座標の平面・面・作図面)。
    pub fn planes(&self) -> Vec<&PlaneRef> {
        match self {
            Kind3d::ParallelToPlane { plane, .. }
            | Kind3d::PlaneAngle { plane, .. }
            | Kind3d::PointOnPlane { plane, .. }
            | Kind3d::PlaneDistance { plane, .. } => vec![plane],
            Kind3d::FaceAngle { a, b, .. } => vec![a, b],
            _ => vec![],
        }
    }

    /// 拘束の名前(パネルと印に出す)。
    pub fn label(&self) -> &'static str {
        match self {
            Kind3d::Coincident { .. } => "一致",
            Kind3d::Fix { .. } => "固定",
            Kind3d::Along { axis: Axis::X, .. } => "X に沿う",
            Kind3d::Along { axis: Axis::Y, .. } => "Y に沿う",
            Kind3d::Along { axis: Axis::Z, .. } => "Z に沿う",
            Kind3d::ParallelToPlane { .. } => "平面に平行",
            Kind3d::Parallel { .. } => "平行",
            Kind3d::Perpendicular { .. } => "直交",
            Kind3d::Equal { .. } => "等しい",
            Kind3d::PointOnLine { .. } => "線の上",
            Kind3d::Midpoint { .. } => "中点",
            Kind3d::PointOnPlane { .. } => "平面の上",
            Kind3d::Length { .. } => "長さ",
            Kind3d::Distance { .. } => "距離",
            Kind3d::AxisDistance { axis: Axis::X, .. } => "X の差",
            Kind3d::AxisDistance { axis: Axis::Y, .. } => "Y の差",
            Kind3d::AxisDistance { axis: Axis::Z, .. } => "Z の差",
            Kind3d::Angle { .. } => "角度",
            Kind3d::PlaneAngle { .. } => "平面との角度",
            Kind3d::FaceAngle { .. } => "面の角度",
            Kind3d::PlaneDistance { .. } => "平面との距離",
            Kind3d::Radius { .. } => "半径",
            Kind3d::Diameter { .. } => "直径",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Constraint3d {
    pub id: ConstraintId,
    #[serde(flatten)]
    pub kind: Kind3d,
    /// 被駆動の寸法(測るだけで形を決めない)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reference: bool,
    /// 寸法の値の札を引いて動かした量(スケッチの座標の向きと長さ)。測る形の上の基準の点
    /// (線の真ん中、円周など)からのずれなので、形を動かしても札が付いて来る。なければ既定の位置
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<[f64; 3]>,
}

impl Sketch3d {
    pub fn entity(&self, id: EntityId) -> Option<&Entity3d> {
        self.entities.iter().find(|e| e.id() == id)
    }

    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Entity3d> {
        self.entities.iter_mut().find(|e| e.id() == id)
    }

    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint3d> {
        self.constraints.iter().find(|c| c.id == id)
    }

    fn next_entity(&self) -> EntityId {
        EntityId(
            self.entities
                .iter()
                .map(|e| e.id().0 + 1)
                .max()
                .unwrap_or(1),
        )
    }

    fn next_constraint(&self) -> ConstraintId {
        ConstraintId(
            self.constraints
                .iter()
                .map(|c| c.id.0 + 1)
                .max()
                .unwrap_or(1),
        )
    }

    /// 点の保存されている座標。
    pub fn point(&self, id: EntityId) -> Option<P3> {
        match self.entity(id)? {
            Entity3d::Point { p, .. } => Some(*p),
            _ => None,
        }
    }

    pub fn add_point(&mut self, p: P3) -> EntityId {
        let id = self.next_entity();
        self.entities.push(Entity3d::Point { id, p });
        id
    }

    pub fn add_line(&mut self, a: EntityId, b: EntityId) -> EntityId {
        let id = self.next_entity();
        self.entities.push(Entity3d::Line {
            id,
            a,
            b,
            construction: false,
        });
        id
    }

    /// 2 点を足して線分でつなぐ(線分の番号を返す)。
    pub fn add_segment(&mut self, a: P3, b: P3) -> EntityId {
        let (a, b) = (self.add_point(a), self.add_point(b));
        self.add_line(a, b)
    }

    pub fn add_circle(&mut self, center: EntityId, radius: f64, normal: P3) -> EntityId {
        let id = self.next_entity();
        self.entities.push(Entity3d::Circle {
            id,
            center,
            radius,
            normal: crate::geom::unit(normal),
            construction: false,
            plane: None,
        });
        id
    }

    pub fn add_constraint(&mut self, kind: Kind3d) -> ConstraintId {
        let id = self.next_constraint();
        self.constraints.push(Constraint3d {
            id,
            kind,
            reference: false,
            label: None,
        });
        id
    }

    /// 半径の寸法を直径の寸法に、直径を半径に変える(値は同じ大きさの円になるよう直す)。
    /// 半径・直径の寸法でなければ偽。2D の [`jc_sketch::Sketch::toggle_radius`] と同じ。
    pub fn toggle_radius(&mut self, id: ConstraintId) -> bool {
        let Some(c) = self.constraints.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        c.kind = match &c.kind {
            Kind3d::Radius { circle, value } => Kind3d::Diameter {
                circle: *circle,
                value: jc_sketch::dims::scaled(value, 2.0),
            },
            Kind3d::Diameter { circle, value } => Kind3d::Radius {
                circle: *circle,
                value: jc_sketch::dims::scaled(value, 0.5),
            },
            _ => return false,
        };
        true
    }

    /// 線分の両端。
    pub fn line_ends(&self, id: EntityId) -> Option<(EntityId, EntityId)> {
        match self.entity(id)? {
            Entity3d::Line { a, b, .. } => Some((*a, *b)),
            _ => None,
        }
    }

    /// 要素と、それだけに使われていた点、関わる拘束を消す。
    pub fn remove_entities(&mut self, ids: &[EntityId]) {
        let mut gone: std::collections::BTreeSet<EntityId> = ids.iter().copied().collect();
        // 線・円を消すなら、ほかに使われていない端の点も消す
        for id in ids {
            if let Some(e) = self.entity(*id) {
                for p in e.points() {
                    let used = self.entities.iter().any(|o| {
                        !gone.contains(&o.id()) && o.id() != *id && o.points().contains(&p)
                    });
                    if !used {
                        gone.insert(p);
                    }
                }
            }
        }
        // 点を消すなら、その点を使う線・円も消す
        let more: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|e| e.points().iter().any(|p| gone.contains(p)))
            .map(|e| e.id())
            .collect();
        gone.extend(more);
        self.entities.retain(|e| !gone.contains(&e.id()));
        self.constraints
            .retain(|c| c.kind.refs().iter().all(|r| !gone.contains(r)));
        self.includes.retain(|i| !gone.contains(&i.entity));
        for d in &mut self.derived {
            if let DerivedDef::Intersection { curves, .. } | DerivedDef::OnSurface { curves, .. } =
                &mut d.def
            {
                curves.retain(|c| !gone.contains(c));
            }
        }
        self.derived.retain(|d| match &d.def {
            DerivedDef::Intersection { curves, .. } | DerivedDef::OnSurface { curves, .. } => {
                !curves.is_empty()
            }
            DerivedDef::Section { plane, .. } | DerivedDef::Project { plane, .. } => match plane {
                PlaneRef::Face { lines } => lines.0.iter().all(|l| !gone.contains(l)),
                _ => true,
            },
        });
        // 決めるのに使った点や線が消えた作図面も消す
        let lost: Vec<u32> = self
            .planes
            .iter()
            .filter(|w| w.def.refs().iter().any(|r| gone.contains(r)))
            .map(|w| w.id)
            .collect();
        for id in lost {
            self.remove_work_plane(id);
        }
    }

    /// 取り込んだ要素か。
    pub fn is_included(&self, id: EntityId) -> bool {
        self.includes.iter().any(|i| i.entity == id)
    }

    /// 取り込んだ要素の点(ソルバで動かさない点)と、取り込んだ円。
    pub fn included_parts(&self) -> (Vec<EntityId>, Vec<EntityId>) {
        let mut points = Vec::new();
        let mut circles = Vec::new();
        for i in &self.includes {
            match self.entity(i.entity) {
                Some(Entity3d::Point { id, .. }) => points.push(*id),
                Some(Entity3d::Line { a, b, .. }) => points.extend([*a, *b]),
                Some(Entity3d::Circle { id, center, .. }) => {
                    points.push(*center);
                    circles.push(*id);
                }
                None => {}
            }
        }
        points.sort();
        points.dedup();
        (points, circles)
    }

    /// ボディから求める線を足す。番号を返す。
    pub fn add_derived(&mut self, def: DerivedDef) -> u32 {
        let id = self.derived.iter().map(|d| d.id + 1).max().unwrap_or(1);
        self.derived.push(Derived3d { id, def });
        id
    }

    pub fn work_plane(&self, id: u32) -> Option<&WorkPlane3d> {
        self.planes.iter().find(|w| w.id == id)
    }

    pub fn work_plane_mut(&mut self, id: u32) -> Option<&mut WorkPlane3d> {
        self.planes.iter_mut().find(|w| w.id == id)
    }

    /// 作図面を足す(名前は「作図面N」)。番号を返す。
    pub fn add_work_plane(&mut self, def: PlaneDef) -> u32 {
        let id = self.planes.iter().map(|w| w.id + 1).max().unwrap_or(1);
        self.planes.push(WorkPlane3d {
            id,
            name: format!("作図面{id}"),
            def,
        });
        id
    }

    /// 作図面を消す。その作図面を相手にした拘束と、その作図面から傾けた作図面も消す。
    pub fn remove_work_plane(&mut self, id: u32) {
        let mut gone = vec![id];
        let mut k = 0;
        while k < gone.len() {
            let g = gone[k];
            for w in &self.planes {
                if let PlaneDef::Tilted {
                    base: PlaneRef::Work { id: b },
                    ..
                } = &w.def
                    && *b == g
                    && !gone.contains(&w.id)
                {
                    gone.push(w.id);
                }
            }
            k += 1;
        }
        self.planes.retain(|w| !gone.contains(&w.id));
        self.derived
            .retain(|d| !matches!(d.def.plane(), Some(PlaneRef::Work { id }) if gone.contains(id)));
        self.constraints.retain(|c| {
            !c.kind
                .planes()
                .iter()
                .any(|p| matches!(p, PlaneRef::Work { id } if gone.contains(id)))
        });
        for e in &mut self.entities {
            if let Entity3d::Circle { plane, .. } = e
                && plane.is_some_and(|p| gone.contains(&p))
            {
                *plane = None;
            }
        }
    }

    /// 解いた位置を保存されている座標へ写す(次に解く時の始めの値)。
    pub fn adopt(&mut self, sol: &crate::Solution3d) {
        for e in &mut self.entities {
            match e {
                Entity3d::Point { id, p } => {
                    if let Some(q) = sol.points.get(id) {
                        *p = *q;
                    }
                }
                Entity3d::Circle {
                    id, radius, normal, ..
                } => {
                    if let Some(r) = sol.radii.get(id) {
                        *radius = *r;
                    }
                    if let Some(n) = sol.normals.get(id) {
                        *normal = *n;
                    }
                }
                Entity3d::Line { .. } => {}
            }
        }
    }
}
