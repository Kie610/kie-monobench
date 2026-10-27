//! 回転(`solid.revolve@1`)。
use kmb_cad::*;
use kmb_core::kernel::{self as k};
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, RefKind, Registry,
};
use kmb_core::{Payload, PointId, Points};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const REVOLVE: &str = "solid.revolve@1";

mod redundant;

/// 回転で使う、網による輪郭の近似の許容差(mm)。
const FACE_MESH_TOLERANCE: f64 = 0.001;

/// 回転軸。スケッチの座標軸、原点の X・Y・Z 軸か、スケッチの 2 点を通る直線。`Auto` は輪郭から
/// 決める([`auto_axis`])。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Axis {
    Auto,
    SketchX,
    SketchY,
    OriginX,
    OriginY,
    OriginZ,
    /// スケッチの 2 点を通る直線。`sketch` を省くと輪郭のスケッチ。
    /// 欄で「スケッチの線」を選んだだけの時は点がまだ無い(線のクリックで入る)
    Points {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<FeatureId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        a: Option<PointId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        b: Option<PointId>,
    },
    /// ボディの辺(両側の面の名前で指す。まっすぐな辺はその直線、円の辺はその円の軸。
    /// [`kmb_cad::AxisRef::Edge`] と同じ)
    Edge {
        feature: FeatureId,
        faces: [String; 2],
    },
    /// 作業軸の機能(kmb-cad-datum。[`kmb_cad::AxisRef::Datum`] と同じ)
    Datum {
        feature: FeatureId,
    },
    /// 3D スケッチの線(kmb-sketch-3d。[`kmb_cad::AxisRef::Segment`] と同じ)
    Segment {
        sketch: FeatureId,
        line: u32,
    },
}

/// スケッチの輪郭を軸まわりに回す。例: `{"profile": 1, "axis": {"kind": "sketch_x"}}`
/// (軸の `auto` は前の版で作った文書のためだけに読む。新しく作る時は軸を必ず選ぶ)
/// `regions` を書くと、スケッチの線で分けた領域のうち、その名前の領域だけを回す(押し出しと同じ)。
/// 軸は輪郭と同じ平面に置く(平面から外れた軸は理由つきで止まる)。
/// 向き `direction` は押し出しと同じ並び(`normal` は軸の右ねじの向き、`reverse` は逆、`symmetric` は
/// 輪郭の両側へ角度の半分ずつ、`two_sides` は表へ `angle`・裏へ `angle2`)。範囲 `"extent": "to_face"` は
/// 角度の代わりに、軸を含む平らな面 `to_face` まで回す。面の代わりに
/// 頂点(`{"feature": 5, "vertex": "…"}`)なら、軸とその点を含む平面まで。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revolve {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<FeatureId>,
    #[serde(default, skip_serializing_if = "Source::is_sketch")]
    pub source: Source,
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub faces: Vec<kmb_core::Ref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<String>,
    pub axis: Axis,
    #[serde(default = "full_turn")]
    pub angle: String,
    #[serde(default)]
    pub operation: Operation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<FeatureId>,
    #[serde(default, skip_serializing_if = "is_normal")]
    pub direction: Direction,
    /// 裏側の角度(向きが `two_sides` の時)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle2: Option<String>,
    #[serde(default, skip_serializing_if = "RevolveExtent::is_angle")]
    pub extent: RevolveExtent,
    /// 範囲が「対象まで」の時の面(機能 `feature` の出力 `body` の、軸を含む平らな面)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_face: Option<RevolveStop>,
}

/// 回す輪郭の元。面はその縁を、スケッチと同じ輪郭として使う。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Sketch,
    Faces,
}

impl Source {
    fn is_sketch(&self) -> bool {
        *self == Self::Sketch
    }
}

impl Default for Revolve {
    fn default() -> Self {
        Self {
            profile: None,
            source: Source::Sketch,
            faces: vec![],
            regions: vec![],
            axis: Axis::Auto,
            angle: full_turn(),
            operation: Operation::New,
            target: None,
            direction: Direction::Normal,
            angle2: None,
            extent: RevolveExtent::Angle,
            to_face: None,
        }
    }
}

fn is_normal(d: &Direction) -> bool {
    *d == Direction::Normal
}

/// 回す範囲: 角度か、選んだ面まで。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevolveExtent {
    #[default]
    Angle,
    ToFace,
}

impl RevolveExtent {
    fn is_angle(&self) -> bool {
        *self == RevolveExtent::Angle
    }
}

/// 回転を止める面(機能 `feature` の出力 `body` の面 `face`)か頂点 `vertex`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevolveStop {
    pub feature: FeatureId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub face: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertex: Option<String>,
}

impl Exprs for Revolve {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut v = vec![&mut self.angle];
        v.extend(self.angle2.as_mut());
        v
    }
}

const TAU: f64 = std::f64::consts::TAU;

impl Revolve {
    fn sketch(&self) -> Result<FeatureId, EvalError> {
        self.profile
            .ok_or_else(|| EvalError::InvalidData("回すスケッチ(輪郭)を選んでください".into()))
    }

    fn sketch_source(&self) -> Option<FeatureId> {
        self.profile.filter(|_| self.source == Source::Sketch)
    }

    fn axis_sketch(&self, given: Option<FeatureId>) -> Result<FeatureId, EvalError> {
        given.or(self.sketch_source()).ok_or_else(|| EvalError::InvalidData(
            "面を輪郭にする時は、軸にするスケッチの番号も指定するか、面と同じ平面にあるボディの辺・作業軸を選んでください".into()))
    }

    fn face_source(&self) -> Result<(FeatureId, Vec<String>), EvalError> {
        let mut feature = None;
        let mut names = Vec::new();
        for r in &self.faces {
            let kmb_core::Ref::Face { feature: f, face } = r else {
                return Err(EvalError::InvalidData(
                    "回す輪郭にはボディの平らな面を選んでください".into(),
                ));
            };
            if feature.is_some_and(|id| id != *f) {
                return Err(EvalError::InvalidData(
                    "回す面は1つのボディから選んでください".into(),
                ));
            }
            feature = Some(*f);
            if !names.contains(face) {
                names.push(face.clone());
            }
        }
        Ok((
            feature.ok_or_else(|| EvalError::InvalidData("回す平らな面を選んでください".into()))?,
            names,
        ))
    }

    fn profiles(&self, ctx: &EvalCtx<'_>) -> Result<(Vec<k::Profile>, Vec<String>), EvalError> {
        if self.source == Source::Faces {
            let (feature, names) = self.face_source()?;
            let groups = kmb_cad::face_profiles_with_tolerance(
                ctx.kernel(),
                body_of(ctx, feature)?,
                &names,
                FACE_MESH_TOLERANCE,
            )
            .map_err(EvalError::InvalidData)?;
            let mut warnings = Vec::new();
            let mut profiles = Vec::new();
            for group in groups {
                if group.profile.approximated && warnings.is_empty() {
                    warnings.push("面の縁に直線と円弧でない線があるので、細かい折れ線で近い形にして回しました".into());
                }
                let p = group.profile.profile;
                for region in p.regions {
                    profiles.push(k::Profile {
                        plane: p.plane,
                        regions: vec![region],
                    });
                }
            }
            return Ok((profiles, warnings));
        }
        let sketch = self.sketch()?;
        let p = if self.regions.is_empty() {
            profile_from(ctx, sketch)?
        } else {
            kmb_core::ProfileFaces::from_value(ctx.output(sketch, "faces")?)
                .ok_or_else(|| {
                    EvalError::InvalidData(format!("{sketch} は領域を選べるスケッチではありません"))
                })?
                .profile(&self.regions)
                .map_err(EvalError::InvalidData)?
        };
        Ok((vec![p], vec![]))
    }

    /// 回し始める角度(輪郭の平面から、軸の右ねじの向きが正)と回す角度(0 より大きく 2π 以下)。
    fn span(
        &self,
        ctx: &EvalCtx<'_>,
        profile: &k::Profile,
        origin: k::P3,
        dir: k::P3,
    ) -> Result<(f64, f64), EvalError> {
        let bad = |m: &str| EvalError::InvalidData(m.into());
        let (start, sweep) = match self.extent {
            RevolveExtent::Angle => {
                let a = angle(ctx, &self.angle, "角度")?;
                if a <= 0.0 {
                    return Err(bad("角度は 0 より大きくしてください"));
                }
                match self.direction {
                    Direction::Normal => (0.0, a),
                    Direction::Reverse => (-a, a),
                    Direction::Symmetric => (-a / 2.0, a),
                    Direction::TwoSides => {
                        let b = match &self.angle2 {
                            Some(src) if !src.trim().is_empty() => angle(ctx, src, "裏側の角度")?,
                            _ => 0.0,
                        };
                        if b < 0.0 {
                            return Err(bad("裏側の角度は 0 以上にしてください"));
                        }
                        (-b, a + b)
                    }
                }
            }
            RevolveExtent::ToFace => {
                let t = self.face_angle(ctx, profile, origin, dir)?;
                match self.direction {
                    Direction::Normal => (0.0, t),
                    Direction::Reverse => (-(TAU - t), TAU - t),
                    Direction::Symmetric | Direction::TwoSides => {
                        return Err(bad(
                            "「対象まで」は向きが「法線の向き」か「逆向き」の時だけ使えます",
                        ));
                    }
                }
            }
        };
        // 合わせて一周を超える時は一周にする
        Ok(if sweep >= TAU - 1e-9 {
            (0.0, TAU)
        } else {
            (start, sweep)
        })
    }

    /// 輪郭の平面から、軸を含む平らな面までの角度(軸の右ねじの向き、0 より大きく 2π 未満)。
    fn face_angle(
        &self,
        ctx: &EvalCtx<'_>,
        profile: &k::Profile,
        origin: k::P3,
        dir: k::P3,
    ) -> Result<f64, EvalError> {
        let bad = |m: &str| EvalError::InvalidData(m.into());
        let t = self.to_face.as_ref().ok_or_else(|| {
            bad("止める面をまだ選んでいません。「回転を止める面を選ぶ」を押して、軸を含む平らな面をクリックしてください")
        })?;
        let body = body_of(ctx, t.feature)?;
        let u = k::normalize(dir);
        // 軸に垂直な向きで、輪郭のある側と面のある側
        let perp = |q: k::P3| {
            let d = k::sub(q, origin);
            k::normalize(k::sub(d, k::scale(u, k::dot(d, u))))
        };
        let angle_to = |q: k::P3| -> Result<f64, EvalError> {
            let from = perp(regions::centroid(profile));
            let to = perp(q);
            let a = k::dot(k::cross(from, to), u).atan2(k::dot(from, to));
            let a = if a <= 1e-9 { a + TAU } else { a };
            if a >= TAU - 1e-9 {
                return Err(bad(
                    "止める面か頂点が輪郭と同じ平面にあります。別の物を選んでください",
                ));
            }
            Ok(a)
        };
        if let Some(v) = &t.vertex {
            let q = kmb_cad::stop_at::vertex_at(ctx.kernel(), body, v)
                .map_err(EvalError::InvalidData)?;
            let d = k::sub(q, origin);
            let off = k::sub(d, k::scale(u, k::dot(d, u)));
            if k::dot(off, off).sqrt() < 1e-6 {
                return Err(bad(
                    "止める頂点が回転軸の上にあります。軸から離れた頂点を選んでください",
                ));
            }
            return angle_to(q);
        }
        let face = ctx
            .kernel()
            .faces(body)
            .map_err(kernel_err)?
            .into_iter()
            .find(|f| f.name == t.face)
            .ok_or_else(|| {
                EvalError::InvalidData(format!(
                    "止める面 {} が見つかりません(形が変わって面が無くなった時は、選び直してください)",
                    t.face
                ))
            })?;
        let p = face
            .planar
            .ok_or_else(|| bad("止める面が平らではありません(軸を含む平らな面だけ使えます)"))?;
        let m = k::normalize(p.normal);
        if k::dot(m, u).abs() > 1e-6 || k::dot(k::sub(p.point, origin), m).abs() > 1e-3 {
            return Err(bad(
                "止める面が回転軸を含んでいません(軸がその面の上にある平らな面か、頂点を選んでください)",
            ));
        }
        angle_to(face_middle(ctx, body, &t.face).unwrap_or(p.point))
    }
}

/// 面 `face` の網の点の真ん中(面の上の点。面の代表の点が軸の上にあると向きが決まらないため)。
fn face_middle(ctx: &EvalCtx<'_>, body: kmb_core::ShapeHandle, face: &str) -> Option<k::P3> {
    let mesh = ctx.kernel().mesh(body, 0.1).ok()?;
    let fi = mesh.face_names.iter().position(|n| n == face)? as u32;
    let mut sum = [0.0; 3];
    let mut n = 0.0;
    for (t, f) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        if *f != fi {
            continue;
        }
        for i in t {
            let p = mesh.positions[*i as usize];
            for k in 0..3 {
                sum[k] += p[k] as f64;
            }
            n += 1.0;
        }
    }
    (n > 0.0).then(|| sum.map(|v| v / n))
}

impl Feature for Revolve {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = match self.source {
            Source::Sketch => vec![output(self.sketch()?, "profile")],
            Source::Faces => vec![output(self.face_source()?.0, "body")],
        };
        if self.source == Source::Sketch && !self.regions.is_empty() {
            v.push(output(self.sketch()?, "faces"));
        }
        if let Axis::Points { sketch, .. } = self.axis {
            v.push(output(self.axis_sketch(sketch)?, "points"));
        }
        if let Axis::Edge { feature, .. } = &self.axis {
            v.push(output(*feature, "body"));
        }
        if let Axis::Datum { feature } = &self.axis {
            v.extend(kmb_cad::AxisRef::Datum { feature: *feature }.inputs());
        }
        if let Axis::Segment { sketch, line } = &self.axis {
            v.extend(
                kmb_cad::AxisRef::Segment {
                    sketch: *sketch,
                    line: *line,
                }
                .inputs(),
            );
        }
        if self.extent == RevolveExtent::ToFace
            && let Some(t) = &self.to_face
        {
            v.push(output(t.feature, "body"));
        }
        v.extend(target_inputs(self.operation, self.target)?);
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        keep_target(self.operation, self.target)
    }

    fn keeps(&self) -> Vec<FeatureId> {
        if self.source == Source::Faces && self.operation == Operation::New {
            self.face_source().map(|(f, _)| vec![f]).unwrap_or_default()
        } else {
            vec![]
        }
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let (profiles, warnings) = self.profiles(ctx)?;
        let parts = profiles
            .into_iter()
            .map(|profile| self.recipe(ctx, profile))
            .collect::<Result<Vec<_>, _>>()?;
        let parts = redundant::remove_contained(parts);
        let recipe = if parts.len() == 1 {
            parts.into_iter().next().unwrap()
        } else {
            Recipe::Many { parts }
        };
        let mut out = finish(ctx, recipe, self.target)?;
        let mut all = kmb_core::warnings_of(&out);
        all.extend(warnings);
        if !all.is_empty() {
            kmb_core::add_warnings(&mut out, all);
        }
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

impl Revolve {
    fn recipe(&self, ctx: &EvalCtx<'_>, profile: k::Profile) -> Result<Recipe, EvalError> {
        let plane = profile.plane;
        let (origin, dir) = match &self.axis {
            Axis::Auto => {
                let (o, d) = auto_axis(&profile).ok_or_else(|| {
                    EvalError::InvalidData(
                        "回転軸を決められません(輪郭の片側に置ける軸がありません)。軸を選んでください"
                            .into(),
                    )
                })?;
                (
                    plane.to_world(o),
                    k::sub(
                        plane.to_world([o[0] + d[0], o[1] + d[1]]),
                        plane.to_world(o),
                    ),
                )
            }
            Axis::SketchX => (plane.origin, plane.x_axis),
            Axis::SketchY => (plane.origin, plane.y_axis),
            Axis::OriginX => kmb_cad::axis::origin_axis(0),
            Axis::OriginY => kmb_cad::axis::origin_axis(1),
            Axis::OriginZ => kmb_cad::axis::origin_axis(2),
            Axis::Points { sketch, a, b } => {
                let (Some(a), Some(b)) = (a, b) else {
                    return Err(EvalError::InvalidData(
                        "回転軸の線をまだ選んでいません。右クリックの「回転軸を選ぶ」で、原点の軸かスケッチの線をクリックしてください"
                            .into(),
                    ));
                };
                kmb_cad::axis::sketch_line(ctx, self.axis_sketch(*sketch)?, *a, *b)?
            }
            Axis::Edge { feature, faces } => kmb_cad::AxisRef::Edge {
                feature: *feature,
                faces: faces.clone(),
            }
            .resolve(ctx)?
            .expect("辺の軸は通る点と向きを持つ"),
            Axis::Datum { feature } => kmb_cad::AxisRef::Datum { feature: *feature }
                .resolve(ctx)?
                .expect("作業軸は通る点と向きを持つ"),
            Axis::Segment { sketch, line } => kmb_cad::AxisRef::Segment {
                sketch: *sketch,
                line: *line,
            }
            .resolve(ctx)?
            .expect("3D スケッチの線は通る点と向きを持つ"),
        };
        // 輪郭の平面に垂直な軸では回せない
        let n = plane.normal();
        if k::dot(n, dir).abs() > (1.0 - 1e-9) * k::dot(dir, dir).sqrt() * k::dot(n, n).sqrt() {
            return Err(EvalError::InvalidData(
                "回転軸が輪郭の平面に垂直です。輪郭の平面に沿う軸を選んでください".into(),
            ));
        }
        // 軸は輪郭と同じ平面に置く。平面から外れた軸で回すと、意図しない形になる。
        if !kmb_cad::axis::on_plane(&plane, origin, dir) {
            return Err(EvalError::InvalidData(NOT_ON_PLANE.into()));
        }
        let (start, sweep) = self.span(ctx, &profile, origin, dir)?;
        let mut profile = profile;
        if start != 0.0 {
            // 回し始める所まで、輪郭を軸のまわりに回しておく
            profile.plane = transform_plane(&k::rotation(origin, dir, start), &profile.plane);
        }
        Ok(Recipe::Revolve {
            operation: self.operation,
            profile,
            origin,
            dir,
            angle: sweep,
        })
    }
}

const NOT_ON_PLANE: &str =
    "回転軸が輪郭の平面の上にありません。輪郭と同じ平面にある線か軸を選んでください";

/// 輪郭がすべて片側にある(またがない)直線か。`o` を通り向き `d`(平面の座標)。
fn one_sided(pts: &[k::P2], o: k::P2, d: k::P2) -> bool {
    let len = d[0].hypot(d[1]);
    if len < 1e-12 {
        return false;
    }
    let side = |p: &k::P2| ((p[0] - o[0]) * d[1] - (p[1] - o[1]) * d[0]) / len;
    let tol = 1e-7;
    pts.iter().all(|p| side(p) <= tol) || pts.iter().all(|p| side(p) >= -tol)
}

/// 輪郭の直線の辺のうち、直線(`o`、`d`)の上に乗るものがあるか。
fn has_edge_on(lines: &[(k::P2, k::P2)], o: k::P2, d: k::P2) -> bool {
    let len = d[0].hypot(d[1]);
    let dist = |p: k::P2| (((p[0] - o[0]) * d[1] - (p[1] - o[1]) * d[0]) / len).abs();
    lines
        .iter()
        .any(|(a, b)| dist(*a) < 1e-7 && dist(*b) < 1e-7)
}

/// 「自動」の回転軸(平面の座標で、通る点と向き)。輪郭をまたがない軸のうち、
/// (1) 輪郭の辺が乗るスケッチの X・Y 軸、(2) 輪郭に触れなくてもまたがない X・Y 軸、
/// (3) 輪郭の直線の辺(長いものから)の順に選ぶ。半断面を X 軸の上に描いた軸などは (1) になる。
pub fn auto_axis(profile: &k::Profile) -> Option<(k::P2, k::P2)> {
    let pts: Vec<k::P2> = profile
        .regions
        .iter()
        .flat_map(|r| r.outer.polyline())
        .collect();
    if pts.is_empty() {
        return None;
    }
    let mut lines: Vec<(k::P2, k::P2)> = profile
        .regions
        .iter()
        .flat_map(|r| r.outer.edges.iter())
        .filter_map(|e| match e.segment {
            k::Segment::Line { a, b } => Some((a, b)),
            _ => None,
        })
        .collect();
    let axes = [([0.0, 0.0], [1.0, 0.0]), ([0.0, 0.0], [0.0, 1.0])];
    if let Some(a) = axes
        .iter()
        .find(|(o, d)| one_sided(&pts, *o, *d) && has_edge_on(&lines, *o, *d))
    {
        return Some(*a);
    }
    if let Some(a) = axes.iter().find(|(o, d)| one_sided(&pts, *o, *d)) {
        return Some(*a);
    }
    let len = |(a, b): &(k::P2, k::P2)| (b[0] - a[0]).hypot(b[1] - a[1]);
    lines.sort_by(|x, y| len(y).total_cmp(&len(x)));
    lines
        .iter()
        .map(|(a, b)| (*a, [b[0] - a[0], b[1] - a[1]]))
        .find(|(o, d)| one_sided(&pts, *o, *d))
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(REVOLVE, "回転")
            .icon("revolve")
            .hint("スケッチや平らな面の輪郭を軸まわりに回す。軸は輪郭と同じ平面に置き、自分で選ぶ")
            .aliases("かいてん revolve")
            .field(Field::choice("source", "回す物", &[("sketch", "スケッチのプロファイル"), ("faces", "平らな面")]))
            .field(Field::output("profile", "プロファイル", "profile").only("source", "sketch", "sketch"))
            .field(Field::pick("faces", "面", &[RefKind::PlanarFace], 1, None).only("source", "faces", "sketch"))
            .field(Field::choice(
                "axis.kind",
                "軸",
                &[
                    ("auto", "自動(前の版で作った回転)"),
                    ("sketch_x", "スケッチの X 軸"),
                    ("sketch_y", "スケッチの Y 軸"),
                    ("origin_x", "原点の X 軸"),
                    ("origin_y", "原点の Y 軸"),
                    ("origin_z", "原点の Z 軸"),
                    ("points", "スケッチの線(クリックで選ぶ)"),
                    ("edge", "ボディの辺"),
                    ("datum", "作業軸"),
                    ("segment", "3D スケッチの線"),
                ],
            )
            .hide_choice("auto")
            .hide_choice("edge")
            .hide_choice("datum")
            .hide_choice("segment")
            .pick_on("points", REVOLVE_AXIS))
            .field(
                Field::choice(
                    "extent",
                    "範囲",
                    &[("angle", "角度"), ("to_face", "対象まで")],
                )
                .help("「対象まで」にすると、回転を止める平らな面(軸を含む面)か頂点をクリックする手順に入る")
                .pick_on("to_face", REVOLVE_TO_FACE),
            )
            .field(Field::expr("angle", "角度").angle().help("範囲が「対象まで」の時は使わない"))
            .field(
                Field::info("to_face.face", "止める面")
                    .help("「回転を止める面を選ぶ」を押して、軸を含む平らな面か頂点をクリックして選ぶ"),
            )
            .field(
                Field::info("to_face.vertex", "止める頂点")
                    .help("頂点を選んだ時は、軸とその点を含む平面まで回す"),
            )
            .field(Field::choice(
                "direction",
                "方向",
                &[
                    ("normal", "法線の向き"),
                    ("reverse", "逆向き"),
                    ("symmetric", "対称"),
                    ("two_sides", "両方向"),
                ],
            ))
            .field(
                Field::opt_expr("angle2", "裏側の角度").angle()
                    .only("direction", "two_sides", "normal")
                    .help("空なら 0。向きが「両方向」の時、輪郭の裏側へ回す角度"),
            )
            .fields(operation_fields())
            .fix("", "回転の軸が輪郭を横切っていないか、軸を選び直して確かめる")
            .fix("平面の上にありません", "右クリックの「回転軸を選ぶ」で、輪郭と同じ平面にある原点の軸かスケッチの線を選び直す"),
        |d| Ok(Box::new(parse::<Revolve>(d)?)),
    );
}

/// 回転軸を、原点の軸かスケッチの線のクリックで選ぶ操作。
pub const REVOLVE_AXIS: &str = "cad.revolve_axis";
/// 輪郭 → 軸 → 角度をクリックで選んで回転を作る操作(ツールバーの「回転」)。
pub const REVOLVE_SKETCH: &str = "cad.revolve";
/// 選んでいる回転の、回す領域を選び直す操作(作った後に 3D のビューで領域を足す・外す)。
pub const REVOLVE_REGIONS: &str = "cad.revolve_regions";

use kmb_cad::regions;
use kmb_core::{Action, Context, Extension, Gesture, Handle, HandleChoice, Outcome, Place};

/// 回転の操作(作る、軸を選ぶ)。
pub struct RevolveCommands;

/// 選んでいる回転。
struct Selected {
    id: FeatureId,
    revolve: Revolve,
}

/// 領域を選び直せる回転(輪郭のスケッチの領域が 2 つ以上ある時だけ)。
fn repickable(cx: &Context<'_>) -> Option<Selected> {
    selected_revolve(cx).filter(|a| {
        a.revolve.source == Source::Sketch
            && a.revolve
                .profile
                .is_some_and(|p| regions::can_repick(cx, p))
    })
}

fn selected_revolve(cx: &Context<'_>) -> Option<Selected> {
    let id = cx.selected?;
    let entry = cx.data.feature(id)?;
    if entry.type_id != REVOLVE {
        return None;
    }
    let revolve: Revolve = serde_json::from_value(entry.data.clone()).ok()?;
    if revolve.source == Source::Sketch {
        cx.data.feature(revolve.profile?)?;
    }
    Some(Selected { id, revolve })
}

/// 輪郭のスケッチの平面。
fn profile_plane(cx: &Context<'_>, profile: FeatureId) -> Option<k::Plane> {
    regions::profile_of(cx, profile)
        .map(|p| p.plane)
        .or_else(|| Points::from_value(cx.output(profile, "points")?).map(|p| p.plane))
}

/// 軸を選ぶ手順の候補: 輪郭 `profile` のスケッチの線(値は線の番号 `n`。`bare` なら
/// `{"line": n}`)と、輪郭と同じ平面にある原点の軸・ほかの見えているスケッチの線。
fn axis_step_choices(
    cx: &Context<'_>,
    before: FeatureId,
    profile: Option<FeatureId>,
    plane: Option<k::Plane>,
    bare: bool,
) -> Vec<HandleChoice> {
    let on = |p: k::P3, d: k::P3| plane.is_some_and(|pl| kmb_cad::axis::on_plane(&pl, p, d));
    let mut choices: Vec<HandleChoice> = profile
        .into_iter()
        .flat_map(|p| kmb_cad::axis::sketch_lines(cx, p))
        .filter(|(_, _, _, e)| on(e[0], k::sub(e[1], e[0])))
        .map(|(n, _, _, ends)| HandleChoice {
            value: if bare { json!({"line": n}) } else { json!(n) },
            label: format!("線 {n}"),
            outline: vec![ends.to_vec()],
            ..Default::default()
        })
        .collect();
    choices.extend(kmb_cad::axis::axis_choices_where(cx, before, profile, on));
    choices
}

const AXIS_LABEL: &str =
    "回転軸にする線をクリック(輪郭と同じ平面にある原点の軸・スケッチの線・ボディの辺)";
const ANGLE_LABEL: &str =
    "マウスを回して角度を決め、クリックで回転を作る。Enter だけなら一周(360 度)";

/// 軸を選ぶ手順(候補が無ければ、そうと案内する)。
fn axis_handle(choices: Vec<HandleChoice>) -> Handle {
    let label = if choices.is_empty() {
        "輪郭と同じ平面に、軸にできる線や辺がありません。Esc でやめて、スケッチに軸の線を描いてください"
    } else {
        AXIS_LABEL
    };
    Handle {
        choices,
        show_origin: true,
        ..Handle::new("axis", label, Gesture::Choice)
    }
}

/// 手順の値・CLI の指定の軸(`"x"`・`"y"`・`"z"`、輪郭のスケッチの線 `{"line": n}` か線の番号、
/// ほかのスケッチの線 `{"sketch": s, "line": n}`)を、回転の軸に。
fn axis_from(
    v: &serde_json::Value,
    cx: &Context<'_>,
    profile: Option<FeatureId>,
) -> Result<Axis, String> {
    let own = |n: u64| -> Result<Axis, String> {
        let profile = profile.ok_or("面を回す時は、軸にするスケッチの番号も指定してください")?;
        let (_, a, b, _) = kmb_cad::axis::sketch_lines(cx, profile)
            .into_iter()
            .find(|l| l.0 == n)
            .ok_or_else(|| format!("輪郭のスケッチに線 {n} がありません"))?;
        Ok(Axis::Points {
            sketch: None,
            a: Some(a),
            b: Some(b),
        })
    };
    if let Some(n) = v.as_u64() {
        return own(n);
    }
    if v.get("sketch").is_none()
        && let Some(n) = v.get("line").and_then(|n| n.as_u64())
    {
        return own(n);
    }
    Ok(match kmb_cad::AxisRef::from_choice(v, cx)? {
        kmb_cad::AxisRef::OriginX => Axis::OriginX,
        kmb_cad::AxisRef::OriginY => Axis::OriginY,
        kmb_cad::AxisRef::OriginZ => Axis::OriginZ,
        kmb_cad::AxisRef::Line { sketch, a, b } => Axis::Points {
            // 輪郭のスケッチなら省く(輪郭を替えても、その線のまま)
            sketch: sketch.filter(|s| Some(*s) != profile),
            a,
            b,
        },
        kmb_cad::AxisRef::Edge { feature, faces } => Axis::Edge { feature, faces },
        kmb_cad::AxisRef::Datum { feature } => Axis::Datum { feature },
        kmb_cad::AxisRef::Segment { sketch, line } => Axis::Segment { sketch, line },
        kmb_cad::AxisRef::Vector => return Err("軸を選んでいません".into()),
    })
}

/// 軸の通る点と向き(世界座標)。スケッチの X・Y 軸・自動は輪郭の平面から決める。
fn axis_line(
    cx: &Context<'_>,
    axis: &Axis,
    profile: Option<FeatureId>,
    p: &k::Profile,
) -> Option<(k::P3, k::P3)> {
    let plane = p.plane;
    Some(match axis {
        Axis::Auto => {
            let (o, d) = auto_axis(p)?;
            let w = plane.to_world(o);
            (w, k::sub(plane.to_world([o[0] + d[0], o[1] + d[1]]), w))
        }
        Axis::SketchX => (plane.origin, plane.x_axis),
        Axis::SketchY => (plane.origin, plane.y_axis),
        Axis::OriginX => kmb_cad::axis::origin_axis(0),
        Axis::OriginY => kmb_cad::axis::origin_axis(1),
        Axis::OriginZ => kmb_cad::axis::origin_axis(2),
        Axis::Points { sketch, a, b } => {
            // 計算と同じく、点 a → b の向き
            let sol = Points::from_value(cx.output(sketch.or(profile)?, "points")?)?;
            let pa = sol.plane.to_world(*sol.points.get(&(*a)?)?);
            let pb = sol.plane.to_world(*sol.points.get(&(*b)?)?);
            (pa, k::sub(pb, pa))
        }
        Axis::Edge { feature, faces } => {
            let Some(kmb_core::Value::Shape(shape)) = cx.output(*feature, "body") else {
                return None;
            };
            kmb_cad::axis::edge_axis(cx.kernel.as_ref(), *shape, faces)?
        }
        Axis::Datum { feature } => kmb_cad::axis::datum_axis_now(cx, *feature)?,
        Axis::Segment { sketch, line } => kmb_cad::axis::segment_axis_now(cx, *sketch, *line)?,
    })
}

/// 回転の範囲を、クリックした軸を含む平らな面までにする操作。
pub const REVOLVE_TO_FACE: &str = "cad.revolve_to_face";
/// 回転の操作を、このすぐ前に並べる(穴のモジュールの最初の操作。設計の作成は押し出し → 回転 → 穴の
/// 順。Kie 2026-10-21 の画板)
const BEFORE_HOLE: &str = "cad.hole_to_face";

/// 回転 `a` の範囲を、クリックした面か頂点(CLI・MCP は引数 `face` か `vertex` と `feature`)までに
/// する。面は回転より前の形から探す。
fn revolve_to_face(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let a = selected_revolve(cx).ok_or("回転を選んでいません")?;
    let (pick, given) = kmb_cad::stop_at::stop_pick(cx)
        .ok_or("回転を止める平らな面か頂点をクリックしてください")?;
    let at = |f: FeatureId| cx.data.index_of(f).unwrap_or(usize::MAX);
    let has = |f: FeatureId| {
        matches!(cx.output(f, "body"), Some(kmb_core::Value::Shape(b))
            if kmb_cad::stop_at::has(cx.kernel.as_ref(), *b, &pick))
    };
    let feature = [given, a.revolve.target]
        .into_iter()
        .flatten()
        .find(|f| at(*f) < at(a.id) && has(*f))
        .ok_or("この回転より後で出来た面や、曲がった面は選べません。回転より前からある平らな面か頂点を選んでください")?;
    let (face, vertex) = pick.parts();
    let mut r = a.revolve.clone();
    r.extent = RevolveExtent::ToFace;
    r.to_face = Some(RevolveStop {
        feature,
        face,
        vertex,
    });
    if matches!(r.direction, Direction::Symmetric | Direction::TwoSides) {
        r.direction = Direction::Normal;
    }
    Ok(Outcome::Change {
        label: "回転を止める面までにする".into(),
        edits: vec![kmb_core::Edit::SetData {
            id: a.id,
            data: serde_json::to_value(&r).map_err(|e| e.to_string())?,
        }],
    })
}

/// 選んだ面かスケッチの領域を、作る操作と手順で同じように読む。
fn chosen_source(cx: &Context<'_>) -> Result<(Revolve, k::Profile), String> {
    if cx.arg("profile").is_some() && cx.arg("faces").is_some() {
        return Err(
            "輪郭と面は同時に指定できません。profile と faces のどちらかを選んでください".into(),
        );
    }
    if cx.arg("faces").is_some()
        || (cx.arg("profile").is_none()
            && cx.arg("regions").is_none()
            && !cx.picked_faces().is_empty())
    {
        let faces = match cx.arg("faces") {
            Some(v) => Field::info("faces", "").refs(&json!({"faces": v})),
            None => cx
                .picked_faces()
                .into_iter()
                .map(|p| kmb_core::Ref::from(p.clone()))
                .collect(),
        };
        let data = Revolve {
            source: Source::Faces,
            faces,
            ..Default::default()
        };
        let (feature, names) = data.face_source().map_err(|e| e.to_string())?;
        let Some(kmb_core::Value::Shape(h)) = cx.output(feature, "body") else {
            return Err("回す面のボディがありません".into());
        };
        let groups = kmb_cad::face_profiles_with_tolerance(
            cx.kernel.as_ref(),
            *h,
            &names,
            FACE_MESH_TOLERANCE,
        )?;
        let p = groups
            .into_iter()
            .next()
            .ok_or("回す面に輪郭がありません")?
            .profile
            .profile;
        return Ok((data, p));
    }
    let (sketch, keys) =
        regions::chosen_in_args(cx).ok_or("回す平らな面かスケッチの領域を選んでください")?;
    let p = regions::chosen_profile(cx, sketch, &keys)?;
    Ok((
        Revolve {
            profile: Some(sketch),
            regions: keys,
            ..Default::default()
        },
        p,
    ))
}

/// 回転を作る(輪郭・軸・角度は手順の値か CLI・MCP の指定。角度を省くと 360 度)。
fn add_revolve(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let (mut data, profile) = chosen_source(cx)?;
    let v = cx.arg("axis").filter(|v| !v.is_null()).ok_or(
        "回転軸を選んでいません(axis に x・y・z か、{\"line\": 輪郭のスケッチの線の番号} か、{\"sketch\": スケッチ, \"line\": 線の番号})",
    )?;
    let axis = axis_from(v, cx, data.profile)?;
    if let Some((o, d)) = axis_line(cx, &axis, data.profile, &profile)
        && !kmb_cad::axis::on_plane(&profile.plane, o, d)
    {
        return Err(NOT_ON_PLANE.into());
    }
    let deg = cx.arg_f64("angle")?.unwrap_or(360.0); // 宣言の既定と同じ
    if !(deg > 0.0 && deg <= 360.0) {
        return Err("角度は 0 より大きく 360 度以下にしてください".into());
    }
    let s = format!("{:.2}", deg);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    data.axis = axis;
    data.angle = format!("{s} deg");
    Ok(Outcome::insert(
        "回転を追加",
        REVOLVE,
        format!("回転{}", cx.count_of(REVOLVE) + 1),
        serde_json::to_value(data).map_err(|e| e.to_string())?,
    ))
}

/// 回転を作る手順: 輪郭(領域)をクリック(Ctrl で複数)→ 軸の線をクリック → 角度をマウスか数字で
/// (Enter だけなら 360 度)。
fn create_handles(cx: &Context<'_>) -> Vec<Handle> {
    if let Ok((data, profile)) = chosen_source(cx)
        && data.source == Source::Faces
    {
        let mut v = vec![axis_handle(axis_step_choices(
            cx,
            FeatureId(u64::MAX),
            None,
            Some(profile.plane),
            true,
        ))];
        if let Some((o, d)) = cx
            .arg("axis")
            .and_then(|a| axis_from(a, cx, None).ok())
            .and_then(|a| axis_line(cx, &a, None, &profile))
        {
            v.push(Handle {
                outline: regions::outline(&profile),
                optional: true,
                ..Handle::new(
                    "angle",
                    ANGLE_LABEL,
                    Gesture::Angle {
                        origin: o,
                        axis: d,
                        from: regions::centroid(&profile),
                    },
                )
            });
        }
        return v;
    }
    let mut v = vec![Handle {
        choices: regions::region_choices(cx, &regions::region_sketches(cx)),
        ..Handle::new(
            "regions",
            "回す領域をクリックすると軸へ(いくつも選ぶ時は Ctrl を押しながら)",
            Gesture::Regions,
        )
    }];
    let Some((sketch, keys)) = cx
        .arg("regions")
        .is_some()
        .then(|| regions::chosen_in_args(cx))
        .flatten()
    else {
        // 輪郭を選ぶ前は、後の手順を案内だけ並べる(輪郭を選ぶと出し直す)
        v.push(Handle::new("axis", AXIS_LABEL, Gesture::Choice));
        v.push(Handle::new(
            "angle",
            ANGLE_LABEL,
            Gesture::Angle {
                origin: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                from: [1.0, 0.0, 0.0],
            },
        ));
        return v;
    };
    let choices = axis_step_choices(
        cx,
        FeatureId(u64::MAX),
        Some(sketch),
        profile_plane(cx, sketch),
        true,
    );
    let profile = regions::chosen_profile(cx, sketch, &keys).ok();
    // 軸は自動で選ばない(輪郭から決めた軸が意図と違うことがあるので、必ずクリックで選ぶ)
    let step = axis_handle(choices);
    v.push(step);
    let Some(axis) = cx.arg("axis") else {
        return v;
    };
    let Some(profile) = profile else {
        return v;
    };
    let line = axis_from(axis, cx, Some(sketch))
        .ok()
        .and_then(|a| axis_line(cx, &a, Some(sketch), &profile));
    let Some((o, d)) = line else {
        return v;
    };
    v.push(Handle {
        outline: regions::outline(&profile),
        optional: true,
        ..Handle::new(
            "angle",
            ANGLE_LABEL,
            Gesture::Angle {
                origin: o,
                axis: d,
                from: regions::centroid(&profile),
            },
        )
    });
    v
}

impl Extension for RevolveCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let ok = selected_revolve(cx).is_some();
        let none = regions::candidates(cx).is_empty()
            && cx.picked_faces().is_empty()
            && cx.arg("faces").is_none();
        vec![
            Action::new(REVOLVE_TO_FACE, "回転を止める面を選ぶ", "cad")
                .before(BEFORE_HOLE)
                .args(kmb_cad::stop_at::args())
                .menu("作成")
                .icon("revolve")
                .aliases("かいてん revolve たいしょう まで めん to object")
                .hint("選んでいる回転の範囲を、クリックした平らな面(回転軸を含む面)までにする")
                .keep_edit()
                .at(match selected_revolve(cx) {
                    Some(a) if a.revolve.extent == RevolveExtent::ToFace => {
                        &[Place::FeatureMenu, Place::Panel]
                    }
                    Some(_) => &[Place::FeatureMenu],
                    None => &[],
                })
                .disabled_if((!ok).then_some("回転を選んでから使う")),
            Action::new(REVOLVE_SKETCH, "回転", "cad")
                .before(BEFORE_HOLE)
                // 欄で足す種類 `REVOLVE` は、クリックで作るこの操作に置き換えた(2026-10-18)
                .replaces(REVOLVE)
                .args(regions::args())
                .arg(kmb_core::Arg::pick("faces", "回す平らな面", &[RefKind::PlanarFace]))
                .arg(kmb_core::Arg::value(
                    "axis",
                    "回転軸。x・y・z か、{\"line\": 輪郭のスケッチの線の番号} か {\"sketch\": スケッチ, \"line\": 線の番号}",
                ))
                .arg(kmb_core::Arg::angle("angle", "角度(0 より大きく 360 度以下)").default(360.0))
                .menu("作成")
                .icon("revolve")
                .aliases("かいてん revolve")
                .hint("平らな面を先に選ぶか、押してからスケッチの領域をクリック(いくつも選ぶ時は Ctrl を押しながら)、同じ平面の軸をクリックし、角度をマウスか数字で決める(Enter だけなら一周)")
                .at(&[Place::Toolbar])
                .disabled_if(none.then_some("閉じた輪郭のあるスケッチがありません")),
            Action::new(REVOLVE_AXIS, "回転軸を選ぶ", "cad")
                .before(BEFORE_HOLE)
                .arg(
                    kmb_core::Arg::value(
                        "line",
                        "軸(輪郭のスケッチの線の番号、x・y・z、{\"sketch\": スケッチ, \"line\": 線の番号})",
                    )
                    .required(),
                )
                .menu("作成")
                .icon("revolve")
                .aliases("かいてん じく 軸 revolve axis 線 原点")
                .hint("選んでいる回転の軸を、輪郭と同じ平面にある原点の X・Y・Z 軸か、スケッチの線(輪郭のスケッチは補助線も)をクリックして決める")
                .at(if ok { &[Place::FeatureMenu] } else { &[] })
                .disabled_if((!ok).then_some("回転を選んでから使う")),
            Action::new(REVOLVE_REGIONS, "領域を選び直す", "cad")
                .before(BEFORE_HOLE)
                .args(regions::args())
                .menu("作成")
                .icon("revolve")
                .aliases("かいてん りょういき 輪郭 プロファイル えらびなおす revolve profile region")
                .hint("選んでいる回転の、回す領域をクリックで足す・外す(今の領域を選んだまま始まる)。Enter で決める")
                .keep_edit()
                .at(if repickable(cx).is_some() {
                    &[Place::FeatureMenu, Place::Panel]
                } else {
                    &[]
                })
                .disabled_if(
                    repickable(cx)
                        .is_none()
                        .then_some("線で分けた領域が 2 つ以上あるスケッチの回転を選んでから使う"),
                ),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        if id == REVOLVE_SKETCH {
            return Some(add_revolve(cx));
        }
        if id == REVOLVE_TO_FACE {
            return Some(revolve_to_face(cx));
        }
        if id == REVOLVE_REGIONS {
            return Some((|| {
                let a = repickable(cx).ok_or("領域を選び直せる回転を選んでいません")?;
                let mut r = a.revolve.clone();
                r.regions =
                    regions::repicked(cx, a.revolve.profile.ok_or("回すスケッチがありません")?)?;
                Ok(Outcome::Change {
                    label: "回す領域を選び直す".into(),
                    edits: vec![kmb_core::Edit::SetData {
                        id: a.id,
                        data: serde_json::to_value(&r).map_err(|e| e.to_string())?,
                    }],
                })
            })());
        }
        (id == REVOLVE_AXIS).then(|| {
            let a = selected_revolve(cx).ok_or("回転を選んでいません")?;
            let v = cx.arg("line")
                .ok_or("line に軸(輪郭のスケッチの線の番号、x・y・z、{\"sketch\": スケッチ, \"line\": 線の番号})を指定します")?;
            let axis = axis_from(v, cx, a.revolve.sketch_source())?;
            let mut r = a.revolve.clone();
            r.axis = axis;
            Ok(Outcome::Change {
                label: "回転軸を選ぶ".into(),
                edits: vec![kmb_core::Edit::SetData {
                    id: a.id,
                    data: serde_json::to_value(&r).map_err(|e| e.to_string())?,
                }],
            })
        })
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if id == REVOLVE_SKETCH {
            return Some(create_handles(cx));
        }
        if id == REVOLVE_TO_FACE {
            selected_revolve(cx)?;
            return Some(vec![Handle {
                vertices: true,
                ..Handle::new(
                    "face",
                    "回転を止める平らな面(回転軸を含む面)か頂点をクリック",
                    Gesture::Face,
                )
            }]);
        }
        if id == REVOLVE_REGIONS {
            let a = repickable(cx)?;
            return Some(vec![regions::repick_handle(
                cx,
                a.revolve.profile?,
                &a.revolve.regions,
                "回す領域をクリック(もう一度で外す)。Enter で決める",
            )]);
        }
        if id != REVOLVE_AXIS {
            return None;
        }
        let a = selected_revolve(cx)?;
        // 輪郭のスケッチの線(隠していても出す)、原点の軸、ほかの見えているスケッチの線。
        // どれも輪郭と同じ平面にあるものだけ(垂直な軸や、平面から外れた線は選べない)
        let plane = if a.revolve.source == Source::Sketch {
            profile_plane(cx, a.revolve.profile?)
        } else {
            let (f, names) = a.revolve.face_source().ok()?;
            let Some(kmb_core::Value::Shape(h)) = cx.output(f, "body") else {
                return None;
            };
            kmb_cad::face_profiles_with_tolerance(
                cx.kernel.as_ref(),
                *h,
                &names,
                FACE_MESH_TOLERANCE,
            )
            .ok()?
            .first()
            .map(|g| g.profile.profile.plane)
        };
        let choices = axis_step_choices(cx, a.id, a.revolve.sketch_source(), plane, false);
        Some(vec![Handle {
            key: "line".into(),
            ..axis_handle(choices)
        }])
    }
}

/// この crate の名乗り(登録する種類と操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-revolve", register).with_extension(RevolveCommands)
}

#[cfg(test)]
mod face_source_tests {
    use super::*;

    /// 確かめること: 輪郭と面を同時に指定しても、片方を黙って捨てない。
    #[test]
    fn revolve_profile_and_faces_cannot_be_given_together() {
        let mut doc = kmb_core::Document::new();
        let reg = Registry::new();
        let kernel: std::sync::Arc<dyn kmb_core::Kernel> = std::sync::Arc::new(k::NullKernel);
        let report = doc.recompute(&reg, kernel.as_ref());
        let args = json!({"profile":2,"faces":{"kind":"face","feature":1,"face":"top"}});
        let cx = Context {
            args: &args,
            ..Context::new(doc.data(), &report, &reg, &kernel)
        };
        let error = chosen_source(&cx).unwrap_err();
        assert!(error.contains("同時"), "{error}");
    }

    #[test]
    fn face_arguments_enable_revolve_without_a_sketch() {
        let mut doc = kmb_core::Document::new();
        let reg = Registry::new();
        let kernel: std::sync::Arc<dyn kmb_core::Kernel> = std::sync::Arc::new(k::NullKernel);
        let report = doc.recompute(&reg, kernel.as_ref());
        let args = json!({"faces":{"kind":"face","feature":1,"face":"top"}});
        let cx = Context {
            args: &args,
            ..Context::new(doc.data(), &report, &reg, &kernel)
        };
        assert!(
            RevolveCommands
                .actions(&cx)
                .iter()
                .find(|a| a.id == REVOLVE_SKETCH)
                .unwrap()
                .disabled
                .is_none()
        );
    }

    #[test]
    fn a_hidden_sketch_number_is_not_an_axis_source_for_faces() {
        let r = Revolve {
            source: Source::Faces,
            profile: Some(FeatureId(1)),
            ..Default::default()
        };
        assert!(r.axis_sketch(None).is_err());
        assert_eq!(r.axis_sketch(Some(FeatureId(2))).unwrap(), FeatureId(2));
    }

    #[test]
    fn old_sketch_revolves_keep_their_saved_data() {
        let old =
            json!({"profile":1,"axis":{"kind":"sketch_x"},"angle":"360 deg","operation":"new"});
        let r: Revolve = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(r.source, Source::Sketch);
        assert_eq!(serde_json::to_value(&r).unwrap(), old);
    }

    #[test]
    fn a_face_source_does_not_require_a_sketch_profile() {
        let value = json!({"source":"faces", "faces":{"kind":"face","feature":1,"face":"f1:end"}, "axis":{"kind":"origin_x"}});
        assert!(serde_json::from_value::<Revolve>(value).is_ok());
    }
}
