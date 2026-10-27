//! 2D スケッチ。要素(点・線分・円・円弧・楕円・スプライン・円錐曲線)と拘束を持ち、寸法は式で
//! パラメータを参照できる。楕円・スプライン・円錐曲線は、輪郭や彫刻の線に出す時に線分と円弧の
//! 並び(バイアーク、許容差 [`ARC_TOLERANCE`])にする。
//!
//! タイムラインには `cad.sketch@1` として載る。出力は `sketch` スロットに
//! [`SOLUTION_TYPE`] 型の [`Solution`]、`profile` スロットに [`PROFILE_TYPE`] 型の
//! 閉じた輪郭([`kmb_core::Profile`])、`curves` スロットに補助線でない線の全部
//! ([`kmb_core::Curves`]。彫刻に使う)、`paths` スロットに [`PATHS_TYPE`] 型の、補助線も含めた
//! 線分・円弧・円と要素の番号(文字を沿わせる線に使う)、`points` スロットにコアの点の集まり
//! ([`kmb_core::Points`]。穴や回転の軸が読む)を入れる。他のモジュールは `points`・`profile`・
//! `curves` だけを読めば、この crate に依存しなくて済む。

mod arrange;
mod auto_constrain;
mod chain;
mod clip;
mod commands;
mod curve;
mod dimension;
mod display;
mod draw;
mod draw_core;
mod edit;
mod ends;
mod find;
mod freeform;
mod geom;
mod groups;
mod import;
mod label;
mod model;
mod modify;
mod names;
mod offset;
mod origin;
mod pattern;
mod profile;
mod reference;
mod section;
mod silhouette;
mod solver;
mod tools;

pub use auto_constrain::{
    AUTO_ANGLE_DEG, AUTO_GAP, AutoGroup, AutoOptions, AutoResult, Suggestion,
    apply as apply_suggestions, auto_constrain, measured_values, with_origin,
};
pub use clip::Clip;
pub use commands::{
    ADD_SKETCH, REDEFINE_PLANE, SKETCH_ON_FACE, SketchCommands, insert_feature, insert_file,
    picked_sketch_plane, selected_sketch,
};
pub use curve::{
    conic_preview, ellipse_minor_point, ellipse_preview, spline_preview, spline_preview_degree,
};
pub use dimension::{
    DimensionPreview, describe, dimension_for, measure, preview_dimension, radius,
};
pub use display::{DISPLAY_SETTINGS, DimFormat, MAX_DIGITS, SketchSettings, dim_format};
pub use draw::{circle_tangent_to_lines, circumcenter};
pub use draw_core::edge_polygon_preview;
pub use edit::{Hit, refs};
pub use ends::{LooseEnds, loose_ends, on_curve, tie_ends};
pub use import::{
    APPROX_TOL, Affine, Imported, Place, Shape, WELD_TOL, approximate, approximate_tol,
    ellipse_from_conjugate, shapes_bounds, transform_shape,
};
pub use kmb_sketch_solver::curve::ARC_TOLERANCE;
pub use label::line_crossing;
pub use model::{
    Constraint, ConstraintId, ConstraintKind, Entity, EntityId, FaceOrigin, NextIds, OffsetPart,
    Projection, ProjectionSource, Sketch, SketchPlane,
};
pub use modify::{Chamfer, Continuity, Links, Moved, Transform};
pub use origin::{Corner, FaceSpot, corner_spot, default_origin, face_plane, face_spots};
pub use pattern::{
    AngularSpacing, CircularPattern, DistanceType, Pattern, PatternAxis, PatternCenter,
    PatternCopy, PatternDef, PatternDirection, PatternId, RectangularPattern,
};
pub use profile::{curves_of, entity_segments, faces_of, open_ends_of, profile_of};
pub use reference::{
    ITEMS_TYPE, ItemShape, RefItem, RefShape, SketchItem, SketchItems, body_edge_refs, body_refs,
    origin_ref, refresh_projections, refresh_projections_with_values, sketch_items, sketch_refs,
};
pub use section::{SECTION_TOL, section, section_pieces, section_refs, section_segments};
pub use silhouette::{BODY_MESH_TOL, face_under, samples as reference_samples, silhouette_refs};
pub use solver::{
    NO_SHAPE, SketchError, Solution, SolveStatus, inconsistency_reason, redundant_with, solve,
    solve_dragging,
};
pub use tools::Spot;

/// 寸法の名前と式の部品。3D スケッチ(kmb-sketch-3d)も同じ決まりで名前を付け、式を計算する。
pub mod dims {
    pub use crate::label::scaled;
    pub use crate::names::{
        DimConstraint, DimExpr, checked, dim_exprs, eval_dims, is_own_name, name_dims, rename_in,
        set_dim_value,
    };
}

use kmb_core::{
    Dim, EvalCtx, EvalError, Feature, InputRef, Outputs, Payload, Plane, Profile, Registry, Value,
    expr_inputs,
};
use std::collections::HashMap;

pub const SKETCH: &str = "cad.sketch@1";
pub const SOLUTION_TYPE: &str = "sketch.solution@1";
pub const PROFILE_TYPE: &str = kmb_core::PROFILE_TYPE;
/// 補助線も含めた線分・円弧・円と要素の番号(`Vec<(EntityId, Segment)>`)。コアの
/// [`kmb_core::Lines`] と同じ形で、ほかの機能(スイープの経路)はコアの型で読む
pub const PATHS_TYPE: &str = kmb_core::LINES_TYPE;

pub fn register(reg: &mut Registry) {
    reg.register_described(
        kmb_core::Descriptor::new(SKETCH, kmb_core::category::SKETCH, "スケッチ")
            .icon("sketch")
            .hint("平面の上に線と円を描き、拘束と寸法で形を決める")
            .aliases("すけっち sketch")
            .retarget(kmb_core::Retarget::Plane {
                key: "plane".into(),
            })
            .no_raw_editor(),
        |d| {
            let s: Sketch = serde_json::from_value(d.clone())
                .map_err(|e| EvalError::InvalidData(e.to_string()))?;
            Ok(Box::new(s))
        },
    );
}

/// 出力の値から輪郭を取り出す。
pub fn profile_value(v: &Value) -> Option<Profile> {
    Profile::from_value(v)
}

/// スケッチの平面を、面の参照ならカーネルに問い合わせて求める。
pub fn resolve_plane(plane: &SketchPlane, ctx: &EvalCtx<'_>) -> Result<Plane, EvalError> {
    resolve_plane_noting(plane, ctx).map(|(p, _)| p)
}

/// [`resolve_plane`] と、面の上のスケッチの原点の角が見つからなかった時の知らせ。
pub fn resolve_plane_noting(
    plane: &SketchPlane,
    ctx: &EvalCtx<'_>,
) -> Result<(Plane, Option<String>), EvalError> {
    Ok(match plane {
        SketchPlane::Xy => (Plane::XY, None),
        SketchPlane::Yz => (Plane::YZ, None),
        SketchPlane::Zx => (Plane::ZX, None),
        SketchPlane::Face {
            feature,
            face,
            origin,
        } => {
            let shape = match ctx.output(*feature, "body")? {
                Value::Shape(h) => *h,
                _ => {
                    return Err(EvalError::InvalidData(
                        "参照先がソリッドではありません".into(),
                    ));
                }
            };
            face_plane(ctx.kernel(), shape, face, origin).map_err(EvalError::InvalidData)?
        }
        SketchPlane::Datum { feature } => (kmb_interchange::datum_plane(ctx, *feature)?, None),
    })
}

/// 補助線も含めた線分・円弧・円と要素の番号(`paths` の出力)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Paths(pub Vec<(EntityId, kmb_core::Segment)>);

impl Payload for Paths {
    const TYPE: &'static str = PATHS_TYPE;
}

impl Payload for Solution {
    const TYPE: &'static str = SOLUTION_TYPE;
}

/// 解けない時だけ付ける派生出力の説明。既存の解と保存する文書の形は変えない。
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct SolveDiagnosis(String);

impl Payload for SolveDiagnosis {
    const TYPE: &'static str = "sketch.diagnosis@1";
}

/// 出力の値から、補助線も含めた線を取り出す。
pub fn paths_of(v: &Value) -> Option<Vec<(EntityId, kmb_core::Segment)>> {
    Paths::from_value(v).map(|p| p.0)
}

/// 出力の値から解を取り出す。
pub fn solution_of(v: &Value) -> Option<Solution> {
    Solution::from_value(v)
}

/// 解の点を、コアの点の集まりにする。
pub fn points_of(sol: &Solution, plane: Plane) -> kmb_core::Points {
    kmb_core::Points {
        plane,
        points: sol
            .points
            .iter()
            .map(|(id, p)| (kmb_core::PointId(id.0), *p))
            .collect(),
        lone: sol.lone.iter().map(|id| kmb_core::PointId(id.0)).collect(),
    }
}

impl Feature for Sketch {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut out: Vec<InputRef> = self.plane.inputs();
        for f in self.projections.iter().filter_map(|p| p.source.feature()) {
            let i = InputRef::Output {
                feature: f,
                slot: "body".into(),
            };
            if !out.contains(&i) {
                out.push(i);
            }
        }
        // 別のスケッチから写した要素は、そのスケッチの要素の出力を読む(K8)
        for f in self.projections.iter().filter_map(|p| p.source.sketch()) {
            let i = InputRef::Output {
                feature: f,
                slot: "items".into(),
            };
            if !out.contains(&i) {
                out.push(i);
            }
        }
        let offset = self.offset.as_deref();
        let values = self
            .constraints
            .iter()
            .filter(|c| !c.reference)
            .filter_map(|c| c.kind.value());
        for v in offset.into_iter().chain(values) {
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
            dimension_values(self, &|n| ctx.param(n).ok()).map_err(EvalError::InvalidData)?;
        let (base, noted) = resolve_plane_noting(&self.plane, ctx)?;
        let plane = match &self.offset {
            Some(src) => base.offset(
                offset_value(src, &|s| ctx.expr(s).map_err(|e| e.to_string()))
                    .map_err(EvalError::InvalidData)?,
            ),
            None => base,
        };
        // 写した要素の位置を元の形から取り直す(元の寸法を変えると、描いた形がついてくる)
        let mut sketch = self.clone();
        let mut warnings: Vec<String> = noted.into_iter().collect();
        if !sketch.projections.is_empty() {
            let mut refs = vec![origin_ref(&plane)];
            let mut bodies: Vec<kmb_core::FeatureId> = sketch
                .projections
                .iter()
                .filter_map(|p| p.source.feature())
                .collect();
            bodies.sort();
            bodies.dedup();
            for f in bodies {
                if let Value::Shape(h) = ctx.output(f, "body")? {
                    let h = *h;
                    let edges = body_refs(ctx.kernel(), f, h, &plane);
                    // 交差とシルエットは網から求め直す(画面と同じ細かさで、線の数え方をそろえる)
                    let uses = |pick: fn(&ProjectionSource) -> bool| {
                        sketch
                            .projections
                            .iter()
                            .any(|p| p.source.feature() == Some(f) && pick(&p.source))
                    };
                    let section = uses(|s| matches!(s, ProjectionSource::Section { .. }));
                    let outline = uses(|s| matches!(s, ProjectionSource::Silhouette { .. }));
                    if (section || outline)
                        && let Ok(mesh) = ctx.kernel().mesh(h, BODY_MESH_TOL)
                    {
                        if section {
                            refs.extend(section_refs(&mesh, &plane, f));
                        }
                        if outline {
                            refs.extend(silhouette_refs(&mesh, &plane, f, &edges));
                        }
                    }
                    refs.extend(edges);
                    // 面の縁を写した辺は、平面から遠い面の辺のこともある
                    refs.extend(body_edge_refs(ctx.kernel(), f, h, &plane));
                }
            }
            let mut sketches: Vec<kmb_core::FeatureId> = sketch
                .projections
                .iter()
                .filter_map(|p| p.source.sketch())
                .collect();
            sketches.sort();
            sketches.dedup();
            for f in sketches {
                if let Some(items) = SketchItems::from_value(ctx.output(f, "items")?) {
                    refs.extend(sketch_refs(&items, f, &plane));
                }
            }
            let lost = refresh_projections_with_values(&mut sketch, &refs, &values)
                .map_err(EvalError::InvalidData)?;
            if !lost.is_empty() {
                let ids: Vec<String> = lost.iter().map(ToString::to_string).collect();
                warnings.push(format!(
                    "写した要素 {} の元の辺や角が見つかりません(前の位置のまま)",
                    ids.join(", ")
                ));
            }
        }
        let sketch = &sketch;
        let solution = solve(sketch, &values).map_err(|e| EvalError::InvalidData(e.to_string()))?;
        let (profile, _) = profile_of(&default_profile_sketch(sketch), &solution, plane);
        let faces = faces_of(sketch, &solution, plane);
        let curves = curves_of(sketch, &solution, plane);
        let paths = Paths(entity_segments(sketch, &solution, true));
        let items = sketch_items(sketch, &solution, plane);
        let mut out: Outputs = [
            ("points".to_string(), points_of(&solution, plane).to_value()),
            ("sketch".to_string(), solution.to_value()),
            ("profile".to_string(), profile.to_value()),
            ("faces".to_string(), faces.to_value()),
            ("curves".to_string(), curves.to_value()),
            ("paths".to_string(), paths.to_value()),
            ("items".to_string(), items.to_value()),
        ]
        .into();
        if let Some(reason) = inconsistency_reason(sketch, &solution) {
            out.insert("diagnosis".into(), SolveDiagnosis(reason).to_value());
        }
        kmb_core::add_warnings(&mut out, warnings);
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        Ok(self
            .renamed(old, new)?
            .map(|s| serde_json::to_value(&s).expect("スケッチは JSON にできる")))
    }

    fn model_params(&self) -> Vec<kmb_core::ModelParam> {
        Sketch::model_params(self)
    }

    fn name_model_params(&self, names: &mut kmb_core::Names) -> Option<serde_json::Value> {
        self.with_dim_names(names)
            .map(|s| serde_json::to_value(&s).expect("スケッチは JSON にできる"))
    }

    fn set_model_param(&self, name: &str, expr: &str) -> Option<serde_json::Value> {
        self.with_dim_value(name, expr)
            .map(|s| serde_json::to_value(&s).expect("スケッチは JSON にできる"))
    }
}

/// 既定の輪郭(押し出しで領域を選ばない時の `profile`)に使う形。置いた面の縁を写した線(面に
/// スケッチを作った時の自動の投影。K7)は、ほかに描いた線があれば補助線とみなす。面の上に円を描いて
/// 領域を選ばずに押し出すと、前と同じく円だけを押し出す。領域を選べば(`faces`)面の縁で囲まれた
/// 領域も押し出せる(Kie の決定 2026-10-19: 面の縁は押し出せる輪郭として写す)。
fn default_profile_sketch(sketch: &Sketch) -> std::borrow::Cow<'_, Sketch> {
    let SketchPlane::Face { feature, face, .. } = &sketch.plane else {
        return std::borrow::Cow::Borrowed(sketch);
    };
    let outline: Vec<EntityId> = sketch
        .projections
        .iter()
        .filter(|p| {
            matches!(&p.source, ProjectionSource::Edge { feature: f, faces } if f == feature && faces.contains(face))
        })
        .map(|p| p.entity)
        .collect();
    let drawn = sketch
        .entities
        .iter()
        .any(|e| e.is_curve() && !e.is_construction() && !outline.contains(&e.id()));
    if outline.is_empty() || !drawn {
        return std::borrow::Cow::Borrowed(sketch);
    }
    let mut s = sketch.clone();
    for e in &mut s.entities {
        if outline.contains(&e.id())
            && let Some(c) = e.construction_mut()
        {
            *c = true;
        }
    }
    std::borrow::Cow::Owned(s)
}

/// 平面からずらす距離の式を mm にする(長さの単位でなければ誤り)。
pub fn offset_value(
    src: &str,
    eval: &dyn Fn(&str) -> Result<kmb_core::Quantity, String>,
) -> Result<f64, String> {
    let q = eval(src)?;
    if q.dim != Dim::LENGTH {
        return Err(format!(
            "平面からの距離 `{src}` は長さの単位が必要です(値: {q})"
        ));
    }
    Ok(q.value)
}

/// 寸法の式を評価し、ソルバに渡す値(長さは mm、角度は rad)にする。`lookup` はパラメータの
/// 名前から値を引く(同じスケッチの寸法の名前はスケッチの中で解く)。単位の無い値は mm
/// (角度は deg)として読む。単位の次元が違う式や、負の長さは誤りにする。参照寸法は解かないので
/// 値を求めない。
pub fn dimension_values(
    sketch: &Sketch,
    lookup: &dyn Fn(&str) -> Option<kmb_core::Quantity>,
) -> Result<HashMap<ConstraintId, f64>, String> {
    let dims = sketch.dim_exprs();
    let qs = names::eval_dims(&dims, lookup)?;
    let mut values = HashMap::new();
    for d in &dims {
        let v = names::checked(d.id, d.name, d.expr, qs[&d.id], d.angle, false)?;
        values.insert(d.id, v);
    }
    Ok(values)
}

/// この crate の名乗り(登録する種類と操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    // 設定の入れ物は組み立てごとに 1 つ。スケッチの編集器(画面)は共有の値で読む
    let commands = SketchCommands::default();
    let shared = commands.store.clone();
    kmb_core::Module::new("kmb-sketch", move |reg| {
        register(reg);
        reg.provide(shared.clone());
    })
    .with_extension(commands)
    .with_extension(find::SketchFind)
}

#[cfg(test)]
mod auto_constrain_tests;
#[cfg(test)]
mod dimension_tests;
#[cfg(test)]
mod draw_core_tests;
#[cfg(test)]
mod group_tests;
#[cfg(test)]
mod io_tests;
#[cfg(test)]
mod names_tests;
#[cfg(test)]
mod section_tests;
#[cfg(test)]
mod silhouette_tests;
#[cfg(test)]
mod symmetric_tests;
#[cfg(test)]
mod tests;
