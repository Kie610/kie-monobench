//! 形を調べる(測る・面と辺の一覧・絵)。画面の測る道具と同じ計算(kmb-measure)を、画面の網と同じ
//! 許容差で作った網に当てる。CLI・MCP が使う。

use crate::Headless;
use kmb_core::{FacePick, FeatureId, Mesh};
use kmb_measure::{Basis, Target};
use serde_json::{Value as Json, json};

impl Headless {
    /// 残ったボディの表示用の網(画面と同じ許容差)。再計算に誤りがあれば断る。
    pub fn meshes(&self) -> Result<Vec<(FeatureId, Mesh)>, String> {
        self.bodies()?;
        let bodies =
            kmb_core::visible_bodies(self.doc().data(), &self.modules().registry, self.report());
        Ok(kmb_measure::meshes(self.modules().kernel.as_ref(), &bodies))
    }

    /// 選んだ物(1〜2 つ)を測る。値の行は画面の測る道具の窓と同じ(2 つなら ΔX・ΔY・ΔZ も出す)。
    /// `shapes` は選んだ物ごとの形(種類と数値)。丸い物が入っていれば `basis`(`center`・`min`・
    /// `max`、省くと `center`)で中心・最短・最長を選ぶ。
    pub fn measure(&self, picks: &[MeasurePick], basis: Option<&str>) -> Result<Json, String> {
        let basis = match basis.unwrap_or("center") {
            "center" => Basis::Center,
            "min" => Basis::Min,
            "max" => Basis::Max,
            b => return Err(format!("測り方 {b} は知りません(center・min・max)")),
        };
        let picks = picks
            .iter()
            .map(MeasurePick::target)
            .collect::<Result<Vec<_>, _>>()?;
        if picks.is_empty() || picks.len() > 2 {
            return Err("測る物は 1 つか 2 つです".into());
        }
        let picks = picks.as_slice();
        let meshes = self.meshes()?;
        let mut shapes = Vec::new();
        for t in picks {
            let g = kmb_measure::resolve(&meshes, t).ok_or_else(|| missing(&meshes, t))?;
            shapes.push(json!(g.kind));
        }
        let body = match picks {
            [Target::Face(p)] => {
                let bodies = kmb_core::visible_bodies(
                    self.doc().data(),
                    &self.modules().registry,
                    self.report(),
                );
                bodies
                    .iter()
                    .find(|(f, _)| *f == p.feature)
                    .and_then(|(_, h)| self.modules().kernel.mass(*h).ok())
                    .map(|props| {
                        let data = self.doc().data();
                        let root = kmb_core::body_root(data, &self.modules().registry, p.feature);
                        kmb_measure::BodyFacts {
                            props,
                            material: body_material(data, root),
                        }
                    })
            }
            _ => None,
        };
        let r = kmb_measure::measure(&meshes, picks, basis, true, body);
        let rows: Vec<Json> = r
            .rows
            .iter()
            .map(|(k, v)| json!({"name": k, "value": v}))
            .collect();
        let mut out = json!({"rows": rows, "shapes": shapes});
        if let Some((a, b)) = r.line {
            out["line"] = json!([a, b]);
        }
        if !r.bases.is_empty() {
            out["basis"] = json!(basis_name(r.basis));
            out["bases"] = json!(r.bases.iter().map(|b| basis_name(*b)).collect::<Vec<_>>());
        }
        Ok(out)
    }

    /// 残ったボディの面(名前)と辺(番号)と、それぞれの形。`measure` で指す時に使う。
    /// `body` を指すと、そのボディだけ。
    pub fn topology(&self, body: Option<FeatureId>) -> Result<Json, String> {
        let meshes = self.meshes()?;
        if let Some(b) = body
            && !meshes.iter().any(|(f, _)| *f == b)
        {
            return Err(format!("ボディ {} は残ったボディにありません", b.0));
        }
        let shape = |t: &Target| kmb_measure::resolve(&meshes, t).map(|g| json!(g.kind));
        let bodies: Vec<Json> = meshes
            .iter()
            .filter(|(f, _)| body.is_none_or(|b| b == *f))
            .map(|(f, m)| {
                let faces: Vec<Json> = m
                    .face_names
                    .iter()
                    .map(|n| {
                        let t = Target::Face(FacePick {
                            feature: *f,
                            face: n.clone(),
                        });
                        json!({"name": n, "shape": shape(&t)})
                    })
                    .collect();
                let edges: Vec<Json> = (0..m.edges.len())
                    .map(|i| {
                        let t = Target::Edge {
                            feature: *f,
                            edge: i,
                        };
                        json!({"index": i, "shape": shape(&t)})
                    })
                    .collect();
                let name = self
                    .doc()
                    .data()
                    .features
                    .iter()
                    .find(|x| x.id == *f)
                    .map(|x| x.name.clone());
                json!({"id": f.0, "name": name, "faces": faces, "edges": edges})
            })
            .collect();
        Ok(json!({"bodies": bodies}))
    }
}

/// 測る物 1 つ。`point` か、`body` と `face`・`edge`・`center` のどれか 1 つで指す
/// ([`Headless::topology`] の名前と番号)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeasurePick {
    /// ボディの機能の ID
    pub body: Option<u64>,
    /// 面の名前
    pub face: Option<String>,
    /// 辺の番号
    pub edge: Option<usize>,
    /// 円・円弧の辺の中心を点として使う(辺の番号)
    pub center: Option<usize>,
    /// 点の座標 [x, y, z](mm)
    pub point: Option<[f64; 3]>,
}

impl MeasurePick {
    /// 測る計算の形にする。
    fn target(&self) -> Result<Target, String> {
        let given = [
            self.face.is_some(),
            self.edge.is_some(),
            self.center.is_some(),
            self.point.is_some(),
        ];
        if given.iter().filter(|g| **g).count() != 1 {
            return Err("測る物は face・edge・center・point のどれか 1 つで指します".into());
        }
        if let Some(at) = self.point {
            return Ok(Target::Vertex {
                feature: FeatureId(self.body.unwrap_or(0)),
                at,
            });
        }
        let feature = FeatureId(
            self.body
                .ok_or("face・edge・center には body(topology の bodies[].id)も要ります")?,
        );
        Ok(if let Some(face) = &self.face {
            Target::Face(FacePick {
                feature,
                face: face.clone(),
            })
        } else if let Some(edge) = self.edge {
            Target::Edge { feature, edge }
        } else {
            Target::Center {
                feature,
                edge: self.center.expect("どれか 1 つはある"),
            }
        })
    }
}

fn basis_name(b: Basis) -> &'static str {
    match b {
        Basis::Center => "center",
        Basis::Min => "min",
        Basis::Max => "max",
    }
}

/// 見つからない物の理由。
fn missing(meshes: &[(FeatureId, Mesh)], t: &Target) -> String {
    let feature = match t {
        Target::Vertex { feature, .. }
        | Target::Center { feature, .. }
        | Target::Edge { feature, .. } => *feature,
        Target::Face(p) => p.feature,
    };
    let Some((_, m)) = meshes.iter().find(|(f, _)| *f == feature) else {
        return format!(
            "ボディ {} は残ったボディにありません(topology で一覧を見られます)",
            feature.0
        );
    };
    match t {
        Target::Face(p) => format!(
            "ボディ {} に面 {} がありません(topology で一覧を見られます)",
            feature.0, p.face
        ),
        Target::Edge { edge, .. } | Target::Center { edge, .. } => format!(
            "ボディ {} の辺は 0〜{} 番です({edge} 番は円か円弧でないか、ありません)",
            feature.0,
            m.edges.len().saturating_sub(1)
        ),
        Target::Vertex { .. } => "点を測れません".into(),
    }
}

/// 絵の指定([`Headless::picture`])。
pub struct PictureSpec {
    /// 視点の名前([`kmb_render::picture::VIEWS`]。省くと等角)
    pub view: Option<String>,
    /// 向きを度で(ヨー・ピッチ)。`view` より先に使う
    pub angles: Option<(f32, f32)>,
    pub width: u32,
    pub height: u32,
    /// 描くボディ(最初に作った機能)。省くと残ったボディ全部
    pub body: Option<FeatureId>,
    /// 目立たせる面と辺
    pub faces: Vec<FacePick>,
    pub edges: Vec<(FeatureId, usize)>,
}

impl Headless {
    /// 残ったボディを絵にする(画面の 3D のビューの明るい見た目に合わせ、平行投影で全体を入れる)。
    /// PNG のバイト列と、絵の説明(視点・大きさ・1 mm の画素数・描いたボディ)を返す。
    pub fn picture(&self, spec: &PictureSpec) -> Result<(Vec<u8>, Json), String> {
        let meshes = self.meshes()?;
        if let Some(b) = spec.body
            && !meshes.iter().any(|(f, _)| *f == b)
        {
            return Err(format!("ボディ {} は残ったボディにありません", b.0));
        }
        let view = spec.view.as_deref().unwrap_or("iso");
        let (yaw, pitch) = match spec.angles {
            Some(a) => a,
            None => kmb_render::picture::view(view).ok_or_else(|| {
                let names: Vec<&str> = kmb_render::picture::VIEWS.iter().map(|v| v.0).collect();
                format!("視点 {view} は知りません({})", names.join("・"))
            })?,
        };
        for f in &spec.faces {
            let t = Target::Face(f.clone());
            kmb_measure::resolve(&meshes, &t).ok_or_else(|| missing(&meshes, &t))?;
        }
        for &(feature, edge) in &spec.edges {
            let ok = meshes
                .iter()
                .any(|(f, m)| *f == feature && edge < m.edges.len());
            if !ok {
                return Err(missing(&meshes, &Target::Edge { feature, edge }));
            }
        }
        let bodies: Vec<kmb_render::picture::Body> = meshes
            .iter()
            .filter(|(f, _)| spec.body.is_none_or(|b| b == *f))
            .map(|(f, m)| kmb_render::picture::Body {
                feature: *f,
                mesh: m,
            })
            .collect();
        let p = kmb_render::picture::render(
            &bodies,
            &kmb_render::picture::Options {
                width: spec.width,
                height: spec.height,
                yaw_deg: yaw,
                pitch_deg: pitch,
                faces: spec.faces.clone(),
                edges: spec.edges.clone(),
            },
        );
        log::info!(
            "絵: {}×{}、ヨー {yaw}° ピッチ {pitch}°、ボディ {} 個",
            p.width,
            p.height,
            bodies.len()
        );
        let info = json!({
            "view": if spec.angles.is_some() { "angles" } else { view },
            "yaw": yaw,
            "pitch": pitch,
            "width": p.width,
            "height": p.height,
            "px_per_mm": (p.px_per_mm * 1000.0).round() / 1000.0,
            "bodies": bodies.iter().map(|b| b.feature.0).collect::<Vec<_>>(),
        });
        Ok((p.png(), info))
    }
}

/// ボディ(最初に作った機能 `root`)に付けた物理材料の名前と密度(表に無い名前は無いものとする)。
pub fn body_material(
    data: &kmb_core::DocumentData,
    root: kmb_core::FeatureId,
) -> Option<(String, f64)> {
    let name = kmb_look::material::material_of(data, root);
    kmb_look::material::phys_material(&name).map(|m| (m.name.to_string(), m.density))
}
