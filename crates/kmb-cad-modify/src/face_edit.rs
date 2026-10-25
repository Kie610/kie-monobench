//! 平らな面を直す機能: 勾配(`solid.draft@1`)、面を置換(`solid.replace_face@1`)、面を削除
//! (`solid.delete_faces@1`)。どれもタイムラインに
//! 機能として 1 つずつ残る(履歴の外で形を変えない)。面を新しい平面に置き換える計算は [`crate::face_cell`]。
//!
//! 面を複数選ぶと 1 つずつ順に直す(前の面を直した形の上で、次の面のまわりの面を見る。隣り合う壁に
//! 勾配を付けても角が合う)。前の面を直して名前が替わった面は、元の平面で探し直す。
use crate::face_cell::{self, Half};
use kmb_cad::plane::PlaneRef;
use kmb_cad::*;
use kmb_core::kernel::{self as k, P3, ShapeHandle};
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Ref, RefKind, Registry, Value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const DRAFT: &str = "solid.draft@1";
pub const REPLACE_FACE: &str = "solid.replace_face@1";
pub const DELETE_FACES: &str = "solid.delete_faces@1";
/// 勾配の引き出し方向(固定の平面)を、原点の平面か平らな面のクリックで選ぶ操作。
pub const DRAFT_PLANE: &str = "cad.draft_plane";

/// 選んだ面のボディ(どれも同じボディ)と、面の名前。
fn face_set(faces: &[Ref], what: &str) -> Result<(FeatureId, Vec<String>), EvalError> {
    let first = faces
        .first()
        .ok_or_else(|| EvalError::InvalidData(format!("{what}を選んでください")))?;
    if faces.iter().any(|r| r.feature() != first.feature()) {
        return Err(EvalError::InvalidData(format!(
            "{what}は 1 つのボディから選んでください"
        )));
    }
    let names = faces
        .iter()
        .map(|r| match r {
            Ref::Face { face, .. } => Ok(face.clone()),
            _ => Err(EvalError::InvalidData(format!(
                "{what}には面を選んでください(辺・頂点・ボディは使えません)"
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((first.feature(), names))
}

/// 面 `name` の平面(点と外向きの法線)。平らでなければ `None`。
fn plane_of(
    ctx: &EvalCtx<'_>,
    body: ShapeHandle,
    name: &str,
) -> Result<Option<(P3, P3)>, EvalError> {
    let faces = ctx.kernel().faces(body).map_err(kernel_err)?;
    let f = faces
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| EvalError::InvalidData(format!("面 {name} がボディにありません")))?;
    Ok(f.planar.map(|p| (p.point, k::normalize(p.normal))))
}

/// 面を 1 つずつ直す。`step` は (今の形, 今の面の名前, 元の面の平面) から新しい形を返す。
fn each_face(
    ctx: &EvalCtx<'_>,
    body: ShapeHandle,
    names: &[String],
    mut step: impl FnMut(ShapeHandle, &str, (P3, P3), &str) -> Result<ShapeHandle, String>,
) -> Result<ShapeHandle, EvalError> {
    let planes = names
        .iter()
        .map(|n| {
            plane_of(ctx, body, n)?.ok_or_else(|| {
                EvalError::InvalidData(format!(
                    "面 {n} は平らではありません(平らな面だけを選んでください)"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let tag = tag(ctx);
    let mut b = body;
    for (i, (name, pl)) in names.iter().zip(&planes).enumerate() {
        let now = if plane_of(ctx, b, name).is_ok() {
            name.clone()
        } else {
            face_cell::find_on_plane(ctx.kernel(), b, pl.0, pl.1).ok_or_else(|| {
                EvalError::InvalidData(format!(
                    "前の面を直した後に、面 {name} が見つかりません(前の面に飲み込まれました)"
                ))
            })?
        };
        b = step(b, &now, *pl, &format!("{tag}:{i}"))
            .map_err(|e| EvalError::InvalidData(format!("面 {name}: {e}")))?;
    }
    Ok(b)
}

fn three_deg() -> String {
    "3 deg".into()
}

fn origin_xy() -> PlaneRef {
    PlaneRef::OriginXy
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 勾配。例: `{"faces": [{"kind": "face", "feature": 2, "face": "f2:side:e1"}], "plane_ref":
/// {"kind": "origin_xy"}, "angle": "3 deg"}`。引き出し方向は固定の平面(`plane_ref`)の法線で、面は
/// 固定の平面との交わりの線を軸に回り、引き出し方向から `angle` だけ傾いた面になる(正の角度で、
/// 引き出し方向へ進むほどボディが細る。型から抜く向き)。`flip` で引き出し方向を逆にする。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    #[serde(default, with = "kmb_core::one_or_many")]
    pub faces: Vec<Ref>,
    #[serde(default = "origin_xy")]
    pub plane_ref: PlaneRef,
    #[serde(default = "three_deg")]
    pub angle: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
}

impl Exprs for Draft {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.angle]
    }
}

/// 面(点 `pf`・外向きの法線 `nf`)を、固定の平面(点 `pp`・法線 `d`)との交わりの線を軸に回し、
/// 引き出し方向 `d` から角度 `a`(rad)だけ傾けた平面(新しい面の外向きの法線)。
pub fn draft_plane(pf: P3, nf: P3, pp: P3, d: P3, a: f64) -> Result<(P3, P3), String> {
    let d = k::normalize(d);
    let along = k::dot(nf, d);
    let h = k::sub(nf, k::scale(d, along));
    if k::dot(h, h) < 1e-12 {
        return Err("引き出し方向に垂直な面には勾配を付けられません".into());
    }
    let h = k::normalize(h);
    // 2 つの平面の交わりの線の上の点(n1・x = h1、n2・x = h2 を満たす n1・n2 の張る向きの点)
    let (h1, h2) = (k::dot(nf, pf), k::dot(d, pp));
    let det = 1.0 - along * along;
    let p0 = k::add(
        k::scale(nf, (h1 - h2 * along) / det),
        k::scale(d, (h2 - h1 * along) / det),
    );
    let (s, c) = a.sin_cos();
    Ok((p0, k::add(k::scale(h, c), k::scale(d, s))))
}

impl Feature for Draft {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(face_set(&self.faces, "勾配を付ける面")?.0, "body")];
        v.extend(self.plane_ref.inputs());
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        face_set(&self.faces, "勾配を付ける面")
            .map(|(b, _)| vec![("body".into(), output(b, "body"))])
            .unwrap_or_default()
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let (id, names) = face_set(&self.faces, "勾配を付ける面")?;
        let a = angle(ctx, &self.angle, "角度")?;
        if a.abs() >= 89f64.to_radians() {
            return Err(EvalError::InvalidData(format!(
                "角度 `{}` は -89 度から 89 度の間にしてください",
                self.angle
            )));
        }
        let (pp, mut d) = self
            .plane_ref
            .resolve(ctx, "引き出し方向")?
            .ok_or_else(|| EvalError::InvalidData("引き出し方向の平面を選んでください".into()))?;
        if self.flip {
            d = k::scale(d, -1.0);
        }
        let body = body_of(ctx, id)?;
        let kernel = ctx.kernel();
        let h = each_face(ctx, body, &names, |b, name, (pf, nf), tag| {
            let (q, qn) = draft_plane(pf, nf, pp, d, a)?;
            face_cell::replace(kernel, b, name, &Half::new(q, qn, "new"), tag)
        })?;
        log::info!("面 {names:?} に {:.3} 度の勾配を付けた", a.to_degrees());
        Ok([("body".to_string(), Value::Shape(h))].into())
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

/// 面を置換。例: `{"faces": [<面>], "target": [<平らな面>]}`。元の面(ソース面)を、ターゲット面の
/// 平面に置き換える(まわりの面を延ばす・縮める)。ターゲット面は別のボディの面でもよい。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceFace {
    #[serde(default, with = "kmb_core::one_or_many")]
    pub faces: Vec<Ref>,
    #[serde(default, with = "kmb_core::one_or_many")]
    pub target: Vec<Ref>,
}

impl ReplaceFace {
    fn target(&self) -> Result<(FeatureId, String), EvalError> {
        match self.target.as_slice() {
            [Ref::Face { feature, face }] => Ok((*feature, face.clone())),
            [] => Err(EvalError::InvalidData(
                "ターゲット面を選んでください".into(),
            )),
            _ => Err(EvalError::InvalidData(
                "ターゲット面は平らな面を 1 つ選んでください".into(),
            )),
        }
    }
}

impl Feature for ReplaceFace {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(face_set(&self.faces, "ソース面")?.0, "body")];
        if let Ok((t, _)) = self.target() {
            v.push(output(t, "body"));
        }
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        face_set(&self.faces, "ソース面")
            .map(|(b, _)| vec![("body".into(), output(b, "body"))])
            .unwrap_or_default()
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let (id, names) = face_set(&self.faces, "ソース面")?;
        let (tf, tname) = self.target()?;
        let tb = body_of(ctx, tf)?;
        let (tp, tn) = plane_of(ctx, tb, &tname)?.ok_or_else(|| {
            EvalError::InvalidData(format!(
                "ターゲット面 {tname} は平らではありません(平らな面を選んでください)"
            ))
        })?;
        let body = body_of(ctx, id)?;
        let kernel = ctx.kernel();
        let h = each_face(ctx, body, &names, |b, name, (_, nf), tag| {
            // 新しい面の外向きは元の面と同じ側(ターゲット面の外向きが逆でも使える)
            let qn = if k::dot(tn, nf) >= 0.0 {
                tn
            } else {
                k::scale(tn, -1.0)
            };
            face_cell::replace(kernel, b, name, &Half::new(tp, qn, "new"), tag)
        })?;
        log::info!("面 {names:?} を面 {tname} の平面に置き換えた");
        Ok([("body".to_string(), Value::Shape(h))].into())
    }
}

/// 面を削除。例: `{"faces": [<面>]}`。選んだ面を消し、まわりの面を延ばしてふさぐ。平らな面は
/// まわりの平らな面を延ばし(面取りの面を消すと角に戻る)、丸めの面はカーネルの
/// [`kmb_core::Kernel::remove_blend_faces`] で両側の面を延ばして鋭い辺に戻す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteFaces {
    #[serde(default, with = "kmb_core::one_or_many")]
    pub faces: Vec<Ref>,
}

impl Feature for DeleteFaces {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![output(face_set(&self.faces, "消す面")?.0, "body")])
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        face_set(&self.faces, "消す面")
            .map(|(b, _)| vec![("body".into(), output(b, "body"))])
            .unwrap_or_default()
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let (id, names) = face_set(&self.faces, "消す面")?;
        let body = body_of(ctx, id)?;
        let kernel = ctx.kernel();
        // 丸めの面はまとめて消す(丸めの続く角は、1 つずつ消すと間の形が閉じない)
        let mut curved = Vec::new();
        let mut flat = Vec::new();
        for n in &names {
            match plane_of(ctx, body, n)? {
                Some(_) => flat.push(n.clone()),
                None => curved.push(n.clone()),
            }
        }
        let mut b = body;
        if !curved.is_empty() {
            b = kernel
                .remove_blend_faces(b, &curved, &format!("{}:blend", tag(ctx)))
                .map_err(|e| EvalError::InvalidData(format!("面 {curved:?}: {e}")))?;
        }
        let h = if flat.is_empty() {
            b
        } else {
            each_face(ctx, b, &flat, |b, name, _, tag| {
                face_cell::remove(kernel, b, name, tag)
            })?
        };
        log::info!(
            "面 {names:?} を消してふさいだ(丸めの面 {}・平らな面 {})",
            curved.len(),
            flat.len()
        );
        Ok([("body".to_string(), Value::Shape(h))].into())
    }
}

/// 引き出し方向の平面の選択肢(数の欄では決めない)。
const PULL_PLANES: [(&str, &str); 4] = [
    ("origin_xy", "原点の XY 平面"),
    ("origin_yz", "原点の YZ 平面"),
    ("origin_zx", "原点の ZX 平面"),
    ("face", "平らな面(クリックで選ぶ)"),
];

pub fn register(reg: &mut Registry) {
    let planar = |key: &str, label: &str, max: Option<usize>| {
        Field::pick(key, label, &[RefKind::PlanarFace], 1, max)
    };
    reg.register_described(
        solid(DRAFT, "勾配")
            .menu("修正")
            .icon("draft")
            .hint("選んだ平らな面を、引き出し方向の平面との交わりを軸に傾ける(型から抜くための勾配)。まわりの面は延びるか縮む")
            .aliases("こうばい 勾配 ぬきこうばい 抜き勾配 draft taper テーパー 傾ける")
            .field(planar("faces", "面", None).help(
                "勾配を付ける平らな面をクリックで足す(もう一度で外す)。どれも 1 つのボディの面",
            ))
            .field(
                Field::choice("plane_ref.kind", "引き出し方向", &PULL_PLANES)
                    .pick_on("face", DRAFT_PLANE)
                    .help("この平面の法線が引き出し方向。面はこの平面との交わりの線を軸に回る"),
            )
            .field(
                Field::expr("angle", "角度")
                    .angle()
                    .help("引き出し方向から傾ける角度。正で、引き出し方向へ進むほど細る"),
            )
            .field(Field::bool("flip", "反転").help("引き出し方向を逆にする"))
            .template(json!({"plane_ref": {"kind": "origin_xy"}, "angle": "3 deg"})),
        |d| Ok(Box::new(parse::<Draft>(d)?)),
    );
    reg.register_described(
        solid(REPLACE_FACE, "面を置換")
            .menu("修正")
            .icon("replace_face")
            .hint("選んだ平らな面(ソース面)を、ターゲット面の平面に置き換える。まわりの面は延びるか縮む")
            .aliases("めんをちかん 面を置換 ちかん 置換 replace face そろえる 揃える")
            .field(planar("faces", "ソース面", None).help(
                "置き換える平らな面をクリックで足す。どれも 1 つのボディの面",
            ))
            .field(
                planar("target", "ターゲット面", Some(1))
                    .help("置き換え先の平らな面(別のボディの面でもよい)"),
            )
            .template(json!({})),
        |d| Ok(Box::new(parse::<ReplaceFace>(d)?)),
    );
    reg.register_described(
        solid(DELETE_FACES, "面を削除")
            .menu("修正")
            .icon("delete_face")
            .hint("選んだ面を消し、まわりの面を延ばしてふさぐ(フィレットの丸めや面取りの面を消すと角に戻る)")
            .aliases("めんをさくじょ 面を削除 さくじょ 削除 delete face けす 消す ふさぐ")
            .field(
                Field::pick("faces", "面", &[RefKind::Face], 1, None)
                    .help("消す面をクリックで足す。平らな面(まわりも平らな面)か、フィレットの丸めの面"),
            )
            .template(json!({})),
        |d| Ok(Box::new(parse::<DeleteFaces>(d)?)),
    );
}
