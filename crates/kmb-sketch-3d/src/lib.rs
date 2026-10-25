//! 3D スケッチ。点・線分・円を 3 次元に置き、
//! 寸法と拘束(軸に沿う、平行、直角、3 次元の角度、面と面の角度)で形を決める。
//! 閉じた線分の輪で平らなものを面として見つけ、角度の相手にできる。

pub mod body;
mod commands;
mod faces;
pub mod geom;
mod model;
mod names;
mod planes;
mod solve;

pub use commands::{ADD_SKETCH3D, Sketch3dCommands};
pub use faces::Face3d;
pub use model::{
    Axis, AxisPlane, Constraint3d, Derived3d, DerivedDef, Entity3d, FaceRef, Include3d, Kind3d, P3,
    PlaneDef, PlaneRef, ProjectWhat, SectionPiece, Sketch3d, WorkPlane3d,
};
pub use planes::plane_at;
pub use solve::{
    SOLUTION3D_TYPE, Solution3d, dimension_values3d, measure, solve3d, solve3d_dragging,
};

use kmb_core::{
    EvalCtx, EvalError, Feature, InputRef, Outputs, Plane, Registry, Value, expr_inputs,
};
use kmb_sketch::resolve_plane_noting;

pub const SKETCH3D: &str = "cad.sketch3d@1";

/// このモジュールの名乗り(組み立て kmb-bundle が使う)。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-sketch-3d", register).with_extension(Sketch3dCommands)
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        kmb_core::Descriptor::new(SKETCH3D, kmb_core::category::SKETCH, "3D スケッチ")
            .icon("sketch3d")
            .hint(
                "点と線を 3 次元に置き、寸法と拘束(軸に沿う・平行・直角・3 次元の角度)で形を決める",
            )
            .aliases("すけっち 3d sketch さんじげん りったい")
            .no_raw_editor(),
        |d| {
            let s: Sketch3d = serde_json::from_value(d.clone())
                .map_err(|e| EvalError::InvalidData(e.to_string()))?;
            Ok(Box::new(s))
        },
    );
}

/// スケッチの座標(x・y・z)を世界座標へ。
pub fn to_world(plane: &Plane, p: P3) -> [f64; 3] {
    let n = plane.normal();
    let q = plane.to_world([p[0], p[1]]);
    [q[0] + n[0] * p[2], q[1] + n[1] * p[2], q[2] + n[2] * p[2]]
}

/// 世界座標をスケッチの座標(x・y・z)へ。
pub fn to_local(plane: &Plane, w: [f64; 3]) -> P3 {
    let q = plane.to_local(w);
    let n = plane.normal();
    let d = geom::sub(w, plane.origin);
    [q[0], q[1], geom::dot(d, n)]
}

/// 解いた形の線(スケッチの座標の折れ線と、補助線か)。円は 64 分割。
pub fn polylines(sketch: &Sketch3d, sol: &Solution3d) -> Vec<(Vec<P3>, bool)> {
    let mut out = Vec::new();
    for e in &sketch.entities {
        match e {
            Entity3d::Line {
                a, b, construction, ..
            } => {
                if let (Some(p), Some(q)) = (sol.points.get(a), sol.points.get(b)) {
                    out.push((vec![*p, *q], *construction));
                }
            }
            Entity3d::Circle {
                id,
                center,
                normal,
                construction,
                ..
            } => {
                let (Some(c), Some(r)) = (sol.points.get(center), sol.radii.get(id)) else {
                    continue;
                };
                let (u, v) = basis(*sol.normals.get(id).unwrap_or(normal));
                let pts = (0..=64)
                    .map(|k| {
                        let t = std::f64::consts::TAU * k as f64 / 64.0;
                        let (s, co) = t.sin_cos();
                        geom::add(*c, geom::add(geom::scale(u, r * co), geom::scale(v, r * s)))
                    })
                    .collect();
                out.push((pts, *construction));
            }
            Entity3d::Point { .. } => {}
        }
    }
    // 写した角や円の中心(点 1 つ)は線にしない
    for lines in sol.derived.values() {
        out.extend(
            lines
                .iter()
                .filter(|l| l.len() >= 2)
                .map(|l| (l.clone(), false)),
        );
    }
    out
}

/// 法線に直交する 2 本の単位ベクトル(円を描く平面の向き)。
pub fn basis(normal: P3) -> (P3, P3) {
    let n = geom::unit(normal);
    let pick = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = geom::unit(geom::cross(n, pick));
    let u = if n[2].abs() > 0.9 {
        // XY 平面の円は x の向きから始める(見た目をそろえる)
        geom::unit(geom::cross([0.0, 1.0, 0.0], n))
    } else {
        u
    };
    (u, geom::cross(n, u))
}

impl Feature for Sketch3d {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut out: Vec<InputRef> = self.plane.inputs();
        // 取り込んだ形と、線を求めるボディ
        let bodies = self
            .includes
            .iter()
            .filter_map(|i| i.source.feature())
            .chain(
                self.derived
                    .iter()
                    .flat_map(|d| d.def.bodies().iter().copied()),
            );
        for f in bodies {
            let i = InputRef::Output {
                feature: f,
                slot: "body".into(),
            };
            if !out.contains(&i) {
                out.push(i);
            }
        }
        for v in self
            .constraints
            .iter()
            .filter(|c| !c.reference)
            .filter_map(|c| c.kind.value())
        {
            for i in expr_inputs(v)? {
                // 同じスケッチの寸法の名前は、スケッチの中で解く
                let own = matches!(&i, InputRef::Param { name } if self.is_own_name(name));
                if !own && !out.contains(&i) {
                    out.push(i);
                }
            }
        }
        Ok(out)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let values =
            dimension_values3d(self, &|n| ctx.param(n).ok()).map_err(EvalError::InvalidData)?;
        let (plane, noted) = resolve_plane_noting(&self.plane, ctx)?;
        // 取り込んだ形の位置を元のボディから取り直す
        let mut sketch = self.clone();
        let mut warnings: Vec<String> = noted.into_iter().collect();
        let mut shapes: Vec<(kmb_core::FeatureId, kmb_core::ShapeHandle)> = Vec::new();
        let mut wanted: Vec<kmb_core::FeatureId> = self
            .includes
            .iter()
            .filter_map(|i| i.source.feature())
            .chain(
                self.derived
                    .iter()
                    .flat_map(|d| d.def.bodies().iter().copied()),
            )
            .collect();
        wanted.sort();
        wanted.dedup();
        for f in wanted {
            if let Value::Shape(h) = ctx.output(f, "body")? {
                shapes.push((f, *h));
            }
        }
        if !sketch.includes.is_empty() {
            let items: Vec<body::BodyItem> = shapes
                .iter()
                .flat_map(|(f, h)| body::body_items(ctx.kernel(), *f, *h))
                .collect();
            let lost = body::refresh_includes(&mut sketch, &items, &plane);
            if !lost.is_empty() {
                let ids: Vec<String> = lost.iter().map(ToString::to_string).collect();
                warnings.push(format!(
                    "取り込んだ要素 {} の元の辺や角が見つかりません(前の位置のまま)",
                    ids.join(", ")
                ));
            }
        }
        let mut sol =
            solve3d(&sketch, &values).map_err(|e| EvalError::InvalidData(e.to_string()))?;
        // ボディから求める線。交差の押した線と投影は、名前の付いた形も使う(ボディごとに 1 度だけ集める)
        let mut geoms: std::collections::BTreeMap<kmb_core::FeatureId, Option<body::BodyGeom>> =
            Default::default();
        for d in &sketch.derived {
            let lines = if body::needs_geom(&d.def) {
                for (f, h) in shapes.iter().filter(|(f, _)| d.def.bodies().contains(f)) {
                    geoms
                        .entry(*f)
                        .or_insert_with(|| body::body_geom(ctx.kernel(), *f, *h));
                }
                let bodies: Vec<&body::BodyGeom> = d
                    .def
                    .bodies()
                    .iter()
                    .filter_map(|f| geoms.get(f).and_then(Option::as_ref))
                    .collect();
                body::derived_lines_geom(&sketch, &sol, &d.def, &bodies, ctx.kernel(), &plane)
            } else {
                let mut tris = Vec::new();
                for (f, h) in &shapes {
                    if d.def.bodies().contains(f)
                        && let Ok(m) = ctx.kernel().mesh(*h, 0.05)
                    {
                        tris.extend(body::mesh_tris(&m, &plane));
                    }
                }
                body::derived_lines(&sketch, &sol, &d.def, &tris)
            };
            if lines.is_empty() {
                warnings.push(format!(
                    "{}({})の線がボディから求まりません",
                    d.def.label(),
                    d.id
                ));
            }
            sol.derived.insert(d.id, lines);
        }
        let wires = kmb_core::Wires(
            polylines(&sketch, &sol)
                .into_iter()
                .map(|(pts, construction)| kmb_core::Wire {
                    points: pts.into_iter().map(|p| to_world(&plane, p)).collect(),
                    construction,
                })
                .collect(),
        );
        let mut out: Outputs = [
            ("sketch".to_string(), sol.to_value()),
            ("lines".to_string(), kmb_core::Payload::to_value(&wires)),
        ]
        .into();
        kmb_core::add_warnings(&mut out, warnings);
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        Ok(self
            .renamed(old, new)?
            .map(|s| serde_json::to_value(&s).expect("JSON にできる")))
    }

    fn model_params(&self) -> Vec<kmb_core::ModelParam> {
        Sketch3d::model_params(self)
    }

    fn name_model_params(&self, names: &mut kmb_core::Names) -> Option<serde_json::Value> {
        self.with_dim_names(names)
            .map(|s| serde_json::to_value(&s).expect("JSON にできる"))
    }

    fn set_model_param(&self, name: &str, expr: &str) -> Option<serde_json::Value> {
        self.with_dim_value(name, expr)
            .map(|s| serde_json::to_value(&s).expect("JSON にできる"))
    }
}

#[cfg(test)]
mod tests;
