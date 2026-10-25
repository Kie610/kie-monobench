use serde::{Deserialize, Serialize};

pub use kmb_sketch_solver::{ConstraintId, EntityId};

/// スケッチの要素。線分・円・円弧は点を ID で参照し、点を共有すればつながる。
/// 座標は mm。保存されている座標は、ソルバの初期値として使う。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entity {
    Point {
        id: EntityId,
        x: f64,
        y: f64,
    },
    Line {
        id: EntityId,
        a: EntityId,
        b: EntityId,
        #[serde(default)]
        construction: bool,
    },
    Circle {
        id: EntityId,
        center: EntityId,
        radius: f64,
        #[serde(default)]
        construction: bool,
    },
    /// 中心まわりに、始点から終点へ反時計回りの円弧
    Arc {
        id: EntityId,
        center: EntityId,
        start: EntityId,
        end: EntityId,
        #[serde(default)]
        construction: bool,
    },
    /// 楕円。`major` は長軸の端(中心からの向きと長さが長半径)、`minor` は短軸の端(中心からの
    /// 距離が短半径)。中心→major と中心→minor は直角(中の式)。`ends` があれば楕円弧で、
    /// 始点から終点へ中心のまわりに反時計回り(始点と終点は楕円の上。中の式)
    Ellipse {
        id: EntityId,
        center: EntityId,
        major: EntityId,
        minor: EntityId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends: Option<(EntityId, EntityId)>,
        #[serde(default)]
        construction: bool,
    },
    /// スプライン。`control` が偽ならフィット点(曲線が点を通る。弦長でパラメータを決めた
    /// 3 次の補間で、端は not-a-knot の条件。端の曲率を円弧にそろえられるよう自然条件にしない。
    /// `degree` と `knots` は使わない)、真なら制御点(clamped の B スプライン。次数は `degree`
    /// で、点が足りなければ下げる。`knots` が無ければ等間隔)。`closed` は閉じた(周期の)
    /// スプライン(閉じた制御点のスプラインの節点は、いつも等間隔)
    Spline {
        id: EntityId,
        points: Vec<EntityId>,
        #[serde(default)]
        control: bool,
        #[serde(default)]
        closed: bool,
        #[serde(default = "cubic")]
        degree: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        #[serde(default)]
        construction: bool,
    },
    /// 円錐曲線。始点・終点と、両端の接線が交わる点 `apex`、形の値 `rho`(0 < rho < 1。
    /// 0.5 が放物線、小さいと楕円、大きいと双曲線)。重み rho / (1 - rho) の有理 2 次ベジエ
    Conic {
        id: EntityId,
        start: EntityId,
        end: EntityId,
        apex: EntityId,
        rho: f64,
        #[serde(default)]
        construction: bool,
    },
}

fn cubic() -> u8 {
    3
}

impl Entity {
    pub fn id(&self) -> EntityId {
        match self {
            Entity::Point { id, .. }
            | Entity::Line { id, .. }
            | Entity::Circle { id, .. }
            | Entity::Arc { id, .. }
            | Entity::Ellipse { id, .. }
            | Entity::Spline { id, .. }
            | Entity::Conic { id, .. } => *id,
        }
    }

    /// 要素が使う点(点そのものは空)。
    pub fn points(&self) -> Vec<EntityId> {
        match self {
            Entity::Point { .. } => vec![],
            Entity::Line { a, b, .. } => vec![*a, *b],
            Entity::Circle { center, .. } => vec![*center],
            Entity::Arc {
                center, start, end, ..
            } => vec![*center, *start, *end],
            Entity::Ellipse {
                center,
                major,
                minor,
                ends,
                ..
            } => {
                let mut v = vec![*center, *major, *minor];
                if let Some((s, e)) = ends {
                    v.extend([*s, *e]);
                }
                v
            }
            Entity::Spline { points, .. } => points.clone(),
            Entity::Conic {
                start, end, apex, ..
            } => vec![*start, *end, *apex],
        }
    }

    /// 曲線の端の点(始点, 終点)。閉じた曲線(円・楕円・閉じたスプライン)と点は `None`。
    pub fn ends(&self) -> Option<(EntityId, EntityId)> {
        match self {
            Entity::Line { a, b, .. } => Some((*a, *b)),
            Entity::Arc { start, end, .. } | Entity::Conic { start, end, .. } => {
                Some((*start, *end))
            }
            Entity::Ellipse { ends, .. } => *ends,
            Entity::Spline {
                points,
                closed: false,
                ..
            } if points.len() >= 2 => Some((points[0], points[points.len() - 1])),
            _ => None,
        }
    }

    /// 曲線(点でない)か。
    pub fn is_curve(&self) -> bool {
        !matches!(self, Entity::Point { .. })
    }

    /// 補助線か。
    pub fn is_construction(&self) -> bool {
        match self {
            Entity::Point { .. } => false,
            Entity::Line { construction, .. }
            | Entity::Circle { construction, .. }
            | Entity::Arc { construction, .. }
            | Entity::Ellipse { construction, .. }
            | Entity::Spline { construction, .. }
            | Entity::Conic { construction, .. } => *construction,
        }
    }

    /// 補助線の印(点は `None`)。
    pub fn construction_mut(&mut self) -> Option<&mut bool> {
        match self {
            Entity::Point { .. } => None,
            Entity::Line { construction, .. }
            | Entity::Circle { construction, .. }
            | Entity::Arc { construction, .. }
            | Entity::Ellipse { construction, .. }
            | Entity::Spline { construction, .. }
            | Entity::Conic { construction, .. } => Some(construction),
        }
    }

    /// 番号を `id` に、使う点を `f` で付け替えた写し(形の値はそのまま)。
    pub fn with_points(&self, id: EntityId, f: &dyn Fn(EntityId) -> EntityId) -> Entity {
        let mut e = self.clone();
        match &mut e {
            Entity::Point { id: i, .. } => *i = id,
            Entity::Line { id: i, a, b, .. } => {
                *i = id;
                *a = f(*a);
                *b = f(*b);
            }
            Entity::Circle { id: i, center, .. } => {
                *i = id;
                *center = f(*center);
            }
            Entity::Arc {
                id: i,
                center,
                start,
                end,
                ..
            } => {
                *i = id;
                *center = f(*center);
                *start = f(*start);
                *end = f(*end);
            }
            Entity::Ellipse {
                id: i,
                center,
                major,
                minor,
                ends,
                ..
            } => {
                *i = id;
                *center = f(*center);
                *major = f(*major);
                *minor = f(*minor);
                if let Some((s, t)) = ends {
                    (*s, *t) = (f(*s), f(*t));
                }
            }
            Entity::Spline { id: i, points, .. } => {
                *i = id;
                for p in points {
                    *p = f(*p);
                }
            }
            Entity::Conic {
                id: i,
                start,
                end,
                apex,
                ..
            } => {
                *i = id;
                *start = f(*start);
                *end = f(*end);
                *apex = f(*apex);
            }
        }
        e
    }
}

/// 拘束の種類。寸法(`value` を持つもの)は式で書き、パラメータを参照できる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConstraintKind {
    Coincident {
        a: EntityId,
        b: EntityId,
    },
    /// 点を保存されている座標に固定する
    Fix {
        point: EntityId,
    },
    Horizontal {
        line: EntityId,
    },
    Vertical {
        line: EntityId,
    },
    Parallel {
        a: EntityId,
        b: EntityId,
    },
    Perpendicular {
        a: EntityId,
        b: EntityId,
    },
    /// 線分どうしなら長さ、円・円弧どうしなら半径を等しくする
    Equal {
        a: EntityId,
        b: EntityId,
    },
    PointOnLine {
        point: EntityId,
        line: EntityId,
    },
    PointOnCircle {
        point: EntityId,
        circle: EntityId,
    },
    Tangent {
        line: EntityId,
        circle: EntityId,
    },
    /// 円・円弧 a と b が接する。外から接するか内から接するかは、保存されている位置で決める
    TangentCircles {
        a: EntityId,
        b: EntityId,
    },
    /// 円・円弧 a と b の中心が同じ
    Concentric {
        a: EntityId,
        b: EntityId,
    },
    /// 線分 b の両端が、線分 a を延ばした直線の上にある
    Collinear {
        a: EntityId,
        b: EntityId,
    },
    Midpoint {
        point: EntityId,
        line: EntityId,
    },
    /// 点 a と点 b が線 line について対称(中点が線上にあり、a-b が線に直角)
    Symmetric {
        a: EntityId,
        b: EntityId,
        line: EntityId,
    },
    /// 2 点間の距離
    Distance {
        a: EntityId,
        b: EntityId,
        value: String,
    },
    /// 点から直線(線分を延ばした線)までの距離
    PointLineDistance {
        point: EntityId,
        line: EntityId,
        value: String,
    },
    HorizontalDistance {
        a: EntityId,
        b: EntityId,
        value: String,
    },
    VerticalDistance {
        a: EntityId,
        b: EntityId,
        value: String,
    },
    Length {
        line: EntityId,
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
    /// 線分 a から線分 b へ反時計回りに測った角度
    Angle {
        a: EntityId,
        b: EntityId,
        value: String,
    },
    /// 点が曲線(楕円・スプライン・円錐曲線。線分・円・円弧も可)の上にある
    PointOnCurve {
        point: EntityId,
        curve: EntityId,
    },
    /// 曲線 a と b が接する(G1)。端を共有していれば、そこで向きがそろう。共有していなければ、
    /// どこか 1 点で触れて向きがそろう。どの曲線(線分・円・円弧・楕円・スプライン・円錐曲線)でもよい
    TangentCurve {
        a: EntityId,
        b: EntityId,
    },
    /// 端を共有する曲線 a と b の、そこでの向きと曲率がそろう(G2)。線分の曲率は 0、
    /// 円弧は 1/r(曲がる向きの符号も)
    Curvature {
        a: EntityId,
        b: EntityId,
    },
    /// 円・円弧 `circle` の円周から、点・直線・円(円弧)`other` までの距離。`far` が偽なら
    /// 近い側(最短)、真なら遠い側(最長)。中心から測る
    /// 時は `Distance`・`PointLineDistance` を使う
    CircleDistance {
        circle: EntityId,
        other: EntityId,
        #[serde(default)]
        far: bool,
        value: String,
    },
    /// 中心線 `line` から点(または線分の端)`point` までの距離の 2 倍(回転体の直径の寸法)
    LinearDiameter {
        point: EntityId,
        line: EntityId,
        value: String,
    },
    /// オフセット(同心と同じく 1 つの拘束)。写した線がみな元から
    /// 同じ距離にある。距離はまとまりで 1 つの値(ソルバの補助の変数)で、値は別の寸法で決める
    /// (寸法を消しても、同じ距離の関係は残る)
    Offset {
        parts: Vec<OffsetPart>,
    },
    /// 正多角形。つながった順の辺 `sides` の頂点が補助線の円 `circle`
    /// の上にあり、辺の長さが等しい
    Polygon {
        circle: EntityId,
        sides: Vec<EntityId>,
    },
    /// 後から直せるパターン(円形状・矩形状)の並び。写しが元を定義どおりに写した位置にある
    /// ([`crate::Pattern`]。元を動かすと写しが付いて来る)
    Pattern {
        pattern: crate::PatternId,
    },
}

/// オフセットの拘束の関係の 1 つ(どれも式 1 つ)。距離はどの関係も同じ値を使う。
/// 作る時に、ほかの関係で既に決まる関係は入れない(長穴の線は、接する円弧で決まるなど)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "rel", rename_all = "snake_case")]
pub enum OffsetPart {
    /// 写しの線分 `to` が元の線分 `from` と平行
    Parallel { from: EntityId, to: EntityId },
    /// 写しの線分 `to` の始点から、元の線分 `from` を延ばした直線までがオフセットの距離
    Distance { from: EntityId, to: EntityId },
    /// 写しの円・円弧 `to`(中心は元と共有)の半径が、元の円・円弧 `from` の半径とオフセットの
    /// 距離だけ違う
    Radius { from: EntityId, to: EntityId },
    /// 角を丸めた円弧 `arc`(中心は元の角の点)の半径がオフセットの距離
    Corner { arc: EntityId },
    /// 写しの曲線 `a` と `b` が、共有する端で接する
    Tangent { a: EntityId, b: EntityId },
}

impl OffsetPart {
    /// 関わる要素。
    pub fn refs(&self) -> Vec<EntityId> {
        match self {
            OffsetPart::Parallel { from, to }
            | OffsetPart::Distance { from, to }
            | OffsetPart::Radius { from, to } => vec![*from, *to],
            OffsetPart::Corner { arc } => vec![*arc],
            OffsetPart::Tangent { a, b } => vec![*a, *b],
        }
    }

    /// 写した側の要素(元の要素は含めない)。
    pub fn made(&self) -> Vec<EntityId> {
        match self {
            OffsetPart::Parallel { to, .. }
            | OffsetPart::Distance { to, .. }
            | OffsetPart::Radius { to, .. } => vec![*to],
            OffsetPart::Corner { arc } => vec![*arc],
            OffsetPart::Tangent { a, b } => vec![*a, *b],
        }
    }

    /// 参照先を `g` で付け替える(どれかが無ければ `None`)。
    pub fn remap(&self, g: &dyn Fn(EntityId) -> Option<EntityId>) -> Option<OffsetPart> {
        Some(match self {
            OffsetPart::Parallel { from, to } => OffsetPart::Parallel {
                from: g(*from)?,
                to: g(*to)?,
            },
            OffsetPart::Distance { from, to } => OffsetPart::Distance {
                from: g(*from)?,
                to: g(*to)?,
            },
            OffsetPart::Radius { from, to } => OffsetPart::Radius {
                from: g(*from)?,
                to: g(*to)?,
            },
            OffsetPart::Corner { arc } => OffsetPart::Corner { arc: g(*arc)? },
            OffsetPart::Tangent { a, b } => OffsetPart::Tangent {
                a: g(*a)?,
                b: g(*b)?,
            },
        })
    }
}

impl ConstraintKind {
    /// 寸法の式。寸法でない拘束は `None`。
    pub fn value(&self) -> Option<&str> {
        match self {
            ConstraintKind::Distance { value, .. }
            | ConstraintKind::PointLineDistance { value, .. }
            | ConstraintKind::HorizontalDistance { value, .. }
            | ConstraintKind::VerticalDistance { value, .. }
            | ConstraintKind::Length { value, .. }
            | ConstraintKind::Radius { value, .. }
            | ConstraintKind::Diameter { value, .. }
            | ConstraintKind::CircleDistance { value, .. }
            | ConstraintKind::LinearDiameter { value, .. }
            | ConstraintKind::Angle { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn value_mut(&mut self) -> Option<&mut String> {
        match self {
            ConstraintKind::Distance { value, .. }
            | ConstraintKind::PointLineDistance { value, .. }
            | ConstraintKind::HorizontalDistance { value, .. }
            | ConstraintKind::VerticalDistance { value, .. }
            | ConstraintKind::Length { value, .. }
            | ConstraintKind::Radius { value, .. }
            | ConstraintKind::Diameter { value, .. }
            | ConstraintKind::CircleDistance { value, .. }
            | ConstraintKind::LinearDiameter { value, .. }
            | ConstraintKind::Angle { value, .. } => Some(value),
            _ => None,
        }
    }

    /// 寸法が角度か(そうでなければ長さ)。
    pub fn is_angle(&self) -> bool {
        matches!(self, ConstraintKind::Angle { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Constraint {
    pub id: ConstraintId,
    #[serde(flatten)]
    pub kind: ConstraintKind,
    /// 参照寸法(形を決めず、測った値を見せるだけ)。ほかの拘束で既に決まっている寸法は、
    /// 置く時にこれになる([`crate::preview_dimension`])。ソルバは使わない。
    #[serde(default, skip_serializing_if = "is_false")]
    pub reference: bool,
    /// 寸法を置いた位置(スケッチの平面の絶対座標)。古いファイルの書き方で、読むと
    /// [`Constraint::label`] へ移す([`Sketch::label_at`])。新しくは書かない
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 2]>,
    /// 寸法の札を置いた位置。寸法の形(測る線・円・交点)に対する相対の座標で、図形を動かしても
    /// 札が付いて来る([`Sketch::label_at`]・[`Sketch::place_label`])。なければ形の外側に出す
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<[f64; 2]>,
}

impl Constraint {
    /// 形を決める拘束(参照寸法でない)。
    pub fn new(id: ConstraintId, kind: ConstraintKind) -> Self {
        Self {
            id,
            kind,
            reference: false,
            at: None,
            label: None,
        }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// スケッチを置く平面。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchPlane {
    #[default]
    Xy,
    Yz,
    Zx,
    /// ソリッドの平らな面の上。面は名前で参照するので、寸法を変えても外れない。
    Face {
        feature: kmb_core::FeatureId,
        face: String,
        /// スケッチの原点にする所([`FaceOrigin`])。無ければ世界の原点を写した点(古いスケッチ)
        #[serde(default, skip_serializing_if = "FaceOrigin::is_world")]
        origin: FaceOrigin,
    },
    /// 作業平面(機能 `feature` の出力 [`kmb_interchange::DATUM_OUTPUT`])の上。原点と X・Y の
    /// 向きは作業平面のまま。作業平面を作れなければ、このスケッチも誤りになる(ほかの平面へ移さない)
    Datum { feature: kmb_core::FeatureId },
}

impl SketchPlane {
    pub fn is_xy(&self) -> bool {
        *self == SketchPlane::Xy
    }

    /// 平面を決めるのに読む出力(面ならボディ、作業平面ならその形)。
    pub fn inputs(&self) -> Vec<kmb_core::InputRef> {
        let (feature, slot) = match self {
            SketchPlane::Face { feature, .. } => (*feature, "body"),
            SketchPlane::Datum { feature } => (*feature, kmb_interchange::DATUM_OUTPUT),
            _ => return Vec::new(),
        };
        vec![kmb_core::InputRef::Output {
            feature,
            slot: slot.into(),
        }]
    }

    /// 平面の元の機能(面のボディか作業平面)。XY・YZ・ZX は `None`。
    pub fn source(&self) -> Option<kmb_core::FeatureId> {
        match self {
            SketchPlane::Face { feature, .. } | SketchPlane::Datum { feature } => Some(*feature),
            _ => None,
        }
    }

    /// 面の上の平面(原点は世界の原点を写した点。[`FaceOrigin::World`])。
    pub fn face(feature: kmb_core::FeatureId, face: impl Into<String>) -> SketchPlane {
        SketchPlane::Face {
            feature,
            face: face.into(),
            origin: FaceOrigin::World,
        }
    }
}

/// 面の上のスケッチの原点。X・Y の向きは面の法線から決まり([`kmb_core::Plane::from_point_normal`])、
/// 原点だけを面の角へ移す。角は、そこで交わる面の名前で指すので、寸法を変えても同じ角に付いて行く。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "at", rename_all = "snake_case")]
pub enum FaceOrigin {
    /// 世界の原点を面の平面へ写した点(2026-10-18 より前に作った面のスケッチ。今のまま残す)
    #[default]
    World,
    /// 面の角(そこで交わる面の名前。名前の順)。新しく作る面のスケッチは、面の左下の角
    Vertex { faces: Vec<String> },
    /// 面の円の辺の中心(角の無い丸い面。平らな面と円筒の面の名前)
    Center { faces: [String; 2] },
}

impl FaceOrigin {
    pub fn is_world(&self) -> bool {
        *self == FaceOrigin::World
    }
}

/// ほかの形から写した要素(投影)。位置は計算のたびに
/// 元の形から取り直し([`crate::refresh_projections`])、ソルバでは動かさない。
/// 寸法や拘束の相手にすると、元の形の寸法を変えた時に描いた形がついてくる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    /// 写した要素(点・線分・円・円弧)
    pub entity: EntityId,
    #[serde(flatten)]
    pub source: ProjectionSource,
}

/// 投影の元。ボディの角・辺は面の名前で指すので、寸法を変えても外れない。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum ProjectionSource {
    /// スケッチの原点(XY・YZ・ZX と古い面のスケッチでは世界の原点を写した点、新しい面の
    /// スケッチでは面の角。[`FaceOrigin`])
    Origin,
    /// ボディの角(そこで交わる面の名前。名前の順)
    Vertex {
        feature: kmb_core::FeatureId,
        faces: Vec<String>,
    },
    /// ボディの辺(両側の面の名前)。まっすぐな辺は線分、円の辺は円か円弧
    Edge {
        feature: kmb_core::FeatureId,
        faces: [String; 2],
    },
    /// ボディの円の辺の中心(穴やボスの中心)
    Center {
        feature: kmb_core::FeatureId,
        faces: [String; 2],
    },
    /// スケッチの平面がボディの面 `face` を切る線の `index` 番目(交差。元の形に付いて行く。面ごとに、つながる順に数える)
    Section {
        feature: kmb_core::FeatureId,
        face: String,
        index: u32,
    },
    /// ボディをスケッチの平面へ写した外形(シルエット)のうち、元の辺に当たらない部分の
    /// `index` 番目(横に寝た円柱の側面の外形など。辺に当たる部分は [`Self::Edge`] で持つ)
    Silhouette {
        feature: kmb_core::FeatureId,
        index: u32,
    },
    /// 前のスケッチ `feature` の要素 `item`(線分・円・円弧・単独の点。寸法や拘束の道具で
    /// 別のスケッチの線を押すと写す)。元のスケッチを直すと付いて行く
    Sketch {
        feature: kmb_core::FeatureId,
        item: EntityId,
    },
}

impl ProjectionSource {
    /// 元のボディの機能(原点と、別のスケッチから写した物は `None`。別のスケッチは
    /// [`Self::sketch`])。
    pub fn feature(&self) -> Option<kmb_core::FeatureId> {
        match self {
            ProjectionSource::Origin | ProjectionSource::Sketch { .. } => None,
            ProjectionSource::Vertex { feature, .. }
            | ProjectionSource::Edge { feature, .. }
            | ProjectionSource::Center { feature, .. }
            | ProjectionSource::Section { feature, .. }
            | ProjectionSource::Silhouette { feature, .. } => Some(*feature),
        }
    }

    /// 別のスケッチから写した物なら、そのスケッチの機能。
    pub fn sketch(&self) -> Option<kmb_core::FeatureId> {
        match self {
            ProjectionSource::Sketch { feature, .. } => Some(*feature),
            _ => None,
        }
    }

    /// 同じ元か(辺の両側の面の順は問わない)。
    pub fn same(&self, other: &ProjectionSource) -> bool {
        use ProjectionSource as P;
        match (self, other) {
            (
                P::Edge {
                    feature: f,
                    faces: a,
                },
                P::Edge {
                    feature: g,
                    faces: b,
                },
            )
            | (
                P::Center {
                    feature: f,
                    faces: a,
                },
                P::Center {
                    feature: g,
                    faces: b,
                },
            ) => f == g && (a == b || (a[0] == b[1] && a[1] == b[0])),
            _ => self == other,
        }
    }
}

/// スケッチ。読む時は [`SketchFile`] を通して古い書き方を読み替える。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, from = "SketchFile")]
pub struct Sketch {
    #[serde(default, skip_serializing_if = "SketchPlane::is_xy")]
    pub plane: SketchPlane,
    /// 平面から法線の向きにずらして描く距離(式。例 `"12 mm"`、`"h + 2 mm"`)。負なら裏側へ
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
    #[serde(default)]
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// ほかの形から写した要素
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projections: Vec<Projection>,
    /// 中心線にした線分(補助線の一種。一点鎖線で描き、回転の軸や直径の寸法に使う)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub centerlines: Vec<EntityId>,
    /// 後から直せるパターン(定義と作った写し。[`crate::Pattern`])
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<crate::pattern::Pattern>,
    /// 寸法の名前(`d1` など。文書の中で一意で、ほかの寸法・欄・パラメータの式から
    /// 参照できる。[`crate::names`])
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub dim_names: std::collections::BTreeMap<ConstraintId, String>,
    /// 次に配る番号の控え([`NextIds`])
    #[serde(default, skip_serializing_if = "NextIds::is_unset")]
    pub next_ids: NextIds,
}

/// 次に配る要素・拘束の番号。消した番号を使い回さないために持つ。0 は控えが無い
/// (古いスケッチ)で、今ある最大+1 から数える。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextIds {
    #[serde(default)]
    pub entity: u32,
    #[serde(default)]
    pub constraint: u32,
}

impl NextIds {
    fn is_unset(&self) -> bool {
        *self == NextIds::default()
    }
}

/// 保存されたスケッチ(読むだけ。[`Sketch`] と同じ欄)。読んだ後に古い書き方を読み替える
/// ([`Sketch::upgrade`])。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SketchFile {
    #[serde(default)]
    plane: SketchPlane,
    #[serde(default)]
    offset: Option<String>,
    #[serde(default)]
    entities: Vec<Entity>,
    #[serde(default)]
    constraints: Vec<Constraint>,
    #[serde(default)]
    projections: Vec<Projection>,
    #[serde(default)]
    centerlines: Vec<EntityId>,
    #[serde(default)]
    patterns: Vec<crate::pattern::Pattern>,
    #[serde(default)]
    dim_names: std::collections::BTreeMap<ConstraintId, String>,
    #[serde(default)]
    next_ids: NextIds,
}

impl From<SketchFile> for Sketch {
    fn from(f: SketchFile) -> Self {
        let mut s = Sketch {
            plane: f.plane,
            offset: f.offset,
            entities: f.entities,
            constraints: f.constraints,
            projections: f.projections,
            centerlines: f.centerlines,
            patterns: f.patterns,
            dim_names: f.dim_names,
            next_ids: f.next_ids,
        };
        s.upgrade();
        s
    }
}

impl Sketch {
    /// 古い書き方を読み替える。パターンの写しに写した拘束(写しごとの寸法など)は外し、
    /// パターンの並びの拘束([`ConstraintKind::Pattern`])にまとめる。写しと元の対応も付ける。
    pub(crate) fn upgrade(&mut self) {
        self.patterns_upgrade();
    }

    /// 中心線か。
    pub fn is_centerline(&self, id: EntityId) -> bool {
        self.centerlines.contains(&id)
    }

    /// 線分を中心線にする・中心線を普通の線に戻す。中心線は補助線にもする(輪郭に入らない)。
    /// 線分でなければ何もしない。
    pub fn toggle_centerline(&mut self, id: EntityId) {
        if !matches!(self.entity(id), Some(Entity::Line { .. })) {
            return;
        }
        let on = !self.is_centerline(id);
        if on {
            self.centerlines.push(id);
        } else {
            self.centerlines.retain(|c| *c != id);
        }
        for e in &mut self.entities {
            if e.id() == id
                && let Some(c) = e.construction_mut()
            {
                *c = on;
            }
        }
    }

    /// 写した要素か。
    pub fn is_projected(&self, id: EntityId) -> bool {
        self.projections.iter().any(|p| p.entity == id)
    }

    /// 投影で位置の決まる点と円(ソルバで動かさない)。写した線分・円弧の端と中心も含む。
    pub fn projected_parts(&self) -> (std::collections::BTreeSet<EntityId>, Vec<EntityId>) {
        let mut points = std::collections::BTreeSet::new();
        let mut circles = Vec::new();
        for p in &self.projections {
            match self.entities.iter().find(|e| e.id() == p.entity) {
                Some(Entity::Point { id, .. }) => {
                    points.insert(*id);
                }
                Some(Entity::Line { a, b, .. }) => points.extend([*a, *b]),
                Some(Entity::Circle { id, center, .. }) => {
                    points.insert(*center);
                    circles.push(*id);
                }
                Some(e) => points.extend(e.points()),
                None => {}
            }
        }
        (points, circles)
    }

    /// 丸めた角に残した仮の角の点。2 本の線の延長に乗り、その 2 本に同じ円弧が接している点。
    /// 式や寸法の基準にだけ使い、穴の位置や 3D の点にはしない。
    pub fn rounded_corners(&self) -> std::collections::BTreeSet<EntityId> {
        use std::collections::{BTreeMap, BTreeSet};
        let mut on: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
        let mut touch: BTreeMap<EntityId, BTreeSet<EntityId>> = BTreeMap::new();
        for c in &self.constraints {
            match c.kind {
                ConstraintKind::PointOnLine { point, line } => {
                    on.entry(point).or_default().push(line)
                }
                ConstraintKind::Tangent { line, circle } => {
                    touch.entry(line).or_default().insert(circle);
                }
                _ => {}
            }
        }
        on.into_iter()
            .filter(|(_, lines)| {
                lines.iter().enumerate().any(|(i, a)| {
                    lines[i + 1..].iter().any(|b| {
                        a != b
                            && touch
                                .get(a)
                                .zip(touch.get(b))
                                .is_some_and(|(x, y)| !x.is_disjoint(y))
                    })
                })
            })
            .map(|(p, _)| p)
            .collect()
    }

    /// 線分・円・円弧に使われていない単独の点(穴の位置などに使う)。
    /// 一致の拘束で線の端などにつながっている点、固定した点(寸法の基準に使う原点)、
    /// 写した点(角や原点の参照)は含めない。
    pub fn lone_points(&self) -> Vec<EntityId> {
        let mut used = std::collections::BTreeSet::new();
        for e in &self.entities {
            used.extend(e.points());
        }
        let mut lone: Vec<EntityId> = self
            .entities
            .iter()
            .filter_map(|e| match e {
                Entity::Point { id, .. } if !used.contains(id) => Some(*id),
                _ => None,
            })
            .collect();
        let (projected, _) = self.projected_parts();
        let corners = self.rounded_corners();
        lone.retain(|p| !projected.contains(p) && !corners.contains(p));
        for c in &self.constraints {
            match c.kind {
                ConstraintKind::Coincident { a, b } => {
                    if used.contains(&a) {
                        lone.retain(|p| *p != b);
                    }
                    if used.contains(&b) {
                        lone.retain(|p| *p != a);
                    }
                }
                ConstraintKind::Fix { point } => lone.retain(|p| *p != point),
                _ => {}
            }
        }
        lone
    }
}
