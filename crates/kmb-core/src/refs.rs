//! 選んだ物の参照(面・辺・頂点・ボディ)と、頂点の名前。
//!
//! 面は名前、辺は両側の面の名前の組で指す([`crate::FacePick`]・[`crate::EdgePick`])。頂点は
//! それを囲む面の名前の組で指す(スケッチの投影の角と同じ考え方)。どれも形を計算し直しても
//! 同じ名前になるので、機能のデータに入れておける。

use crate::{EdgeInfo, FeatureId, Kernel, ShapeHandle};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

type P3 = [f64; 3];

/// ボディの角(頂点)。名前は囲む面の名前を `|` でつないだもの。同じ面の組の角が 2 つ以上あれば
/// (例: 1 枚の面の両端)、座標の小さい順に 2 つ目から `~1`、`~2` を付ける。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VertexInfo {
    pub name: String,
    pub faces: Vec<String>,
    pub at: P3,
}

/// 3D のビューで選んだ頂点(機能の出力 `body` の頂点の名前)。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VertexPick {
    pub feature: FeatureId,
    pub vertex: String,
}

/// 位置を数えるための丸め(0.1 µm)
fn key(p: P3) -> (i64, i64, i64) {
    let r = |v: f64| (v * 1e4).round() as i64;
    (r(p[0]), r(p[1]), r(p[2]))
}

/// 辺の一覧([`Kernel::edges`] と [`Kernel::arc_edges`])から角を集める。辺の端の位置ごとに、
/// そこへ来る辺の面を集める。一周の円の継ぎ目は角にしない(どこに置くかは形の作り方で決まるため)。
pub fn vertices_of_edges(edges: &[EdgeInfo]) -> Vec<VertexInfo> {
    let full = |e: &EdgeInfo| {
        e.arc
            .is_some_and(|a| (a.sweep.abs() - std::f64::consts::TAU).abs() < 1e-6)
    };
    let mut corners: BTreeMap<(i64, i64, i64), (P3, Vec<String>)> = BTreeMap::new();
    for e in edges.iter().filter(|e| !full(e)) {
        for p in [e.a, e.b] {
            let c = corners.entry(key(p)).or_insert((p, Vec::new()));
            for f in &e.faces {
                if !c.1.contains(f) {
                    c.1.push(f.clone());
                }
            }
        }
    }
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = Vec::new();
    // BTreeMap なので座標の小さい順(同じ面の組の番号が毎回同じになる)
    for (at, mut faces) in corners.into_values() {
        faces.sort_by_key(|a| crate::natural_key(a));
        let base = faces.join("|");
        let n = seen.entry(base.clone()).or_insert(0);
        let name = if *n == 0 { base } else { format!("{base}~{n}") };
        *n += 1;
        out.push(VertexInfo { name, faces, at });
    }
    out
}

/// ボディ `shape` の角。辺を列挙できないカーネル(網だけの形など)では空。
pub fn vertices(kernel: &dyn Kernel, shape: ShapeHandle) -> Vec<VertexInfo> {
    let mut edges = kernel.edges(shape).unwrap_or_default();
    edges.extend(kernel.arc_edges(shape).unwrap_or_default());
    vertices_of_edges(&edges)
}

/// 面の名前(例: `f7:end`、パターンの写しの `f7@2:side:e1`)から、その面を作った機能。
pub fn face_feature(face: &str) -> Option<FeatureId> {
    let head = face.strip_prefix('f')?;
    let end = head
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(head.len());
    head[..end].parse().ok().map(FeatureId)
}

/// 選択の欄が受ける物の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Vertex,
    Edge,
    /// 面(曲面も)
    Face,
    /// 平らな面だけ
    PlanarFace,
    /// ボディ(最後にそのボディを作った機能で指す)
    Body,
    /// 道具の作り方を持つ機能(押し出し・回転・穴・パターン・ミラー)。値はボディと同じく
    /// 機能の番号([`Ref::Body`])で持つ。3D のビューでは、クリックした面を作った機能になる
    Feature,
}

impl RefKind {
    /// 画面に出す名前。
    pub fn label(self) -> &'static str {
        match self {
            RefKind::Vertex => "頂点",
            RefKind::Edge => "辺",
            RefKind::Face => "面",
            RefKind::PlanarFace => "平らな面",
            RefKind::Body => "ボディ",
            RefKind::Feature => "機能",
        }
    }
}

/// 選んだ物 1 つ。ボディは機能の番号だけ(`5`)で書き、ほかは種類つきの組で書く
/// (`{"kind": "face", "feature": 7, "face": "f2:end"}`)。ボディを番号で書くのは、前から
/// ボディの欄(ブーリアンの対象など)を番号で持っていたのと同じ形にするため。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ref {
    Face {
        feature: FeatureId,
        face: String,
    },
    Edge {
        feature: FeatureId,
        faces: [String; 2],
    },
    Vertex {
        feature: FeatureId,
        vertex: String,
    },
    Body(FeatureId),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Tagged {
    Face {
        feature: FeatureId,
        face: String,
    },
    Edge {
        feature: FeatureId,
        faces: [String; 2],
    },
    Vertex {
        feature: FeatureId,
        vertex: String,
    },
    Body {
        feature: FeatureId,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Repr {
    Body(FeatureId),
    Tagged(Tagged),
}

impl Serialize for Ref {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let r = match self.clone() {
            Ref::Body(f) => Repr::Body(f),
            Ref::Face { feature, face } => Repr::Tagged(Tagged::Face { feature, face }),
            Ref::Edge { feature, faces } => Repr::Tagged(Tagged::Edge { feature, faces }),
            Ref::Vertex { feature, vertex } => Repr::Tagged(Tagged::Vertex { feature, vertex }),
        };
        r.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Ref {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Repr::deserialize(d)? {
            Repr::Body(f) | Repr::Tagged(Tagged::Body { feature: f }) => Ref::Body(f),
            Repr::Tagged(Tagged::Face { feature, face }) => Ref::Face { feature, face },
            Repr::Tagged(Tagged::Edge { feature, faces }) => Ref::Edge { feature, faces },
            Repr::Tagged(Tagged::Vertex { feature, vertex }) => Ref::Vertex { feature, vertex },
        })
    }
}

impl Ref {
    /// 物を持つボディの機能(ボディならその機能)。
    pub fn feature(&self) -> FeatureId {
        match self {
            Ref::Face { feature, .. } | Ref::Edge { feature, .. } | Ref::Vertex { feature, .. } => {
                *feature
            }
            Ref::Body(f) => *f,
        }
    }

    /// 種類(面は曲面としての種類。平らかどうかは形を見ないと分からない)。
    pub fn kind(&self) -> RefKind {
        match self {
            Ref::Face { .. } => RefKind::Face,
            Ref::Edge { .. } => RefKind::Edge,
            Ref::Vertex { .. } => RefKind::Vertex,
            Ref::Body(_) => RefKind::Body,
        }
    }

    /// 種類の集まり `accept` が受けるか。`planar` は面が平らか(面の時だけ聞く)。
    pub fn accepted_by(&self, accept: &[RefKind], planar: &dyn Fn(&Ref) -> bool) -> bool {
        match self.kind() {
            RefKind::Face => {
                accept.contains(&RefKind::Face)
                    || accept.contains(&RefKind::PlanarFace) && planar(self)
            }
            // 機能はボディと同じく番号で持つ
            RefKind::Body => accept.contains(&RefKind::Body) || accept.contains(&RefKind::Feature),
            k => accept.contains(&k),
        }
    }
}

impl From<crate::FacePick> for Ref {
    fn from(p: crate::FacePick) -> Self {
        Ref::Face {
            feature: p.feature,
            face: p.face,
        }
    }
}

impl From<crate::EdgePick> for Ref {
    fn from(p: crate::EdgePick) -> Self {
        Ref::Edge {
            feature: p.feature,
            faces: p.faces,
        }
    }
}

impl From<VertexPick> for Ref {
    fn from(p: VertexPick) -> Self {
        Ref::Vertex {
            feature: p.feature,
            vertex: p.vertex,
        }
    }
}

/// 1 つなら値そのもの、2 つ以上なら並びで書く項目(`"tool": 5` と `"tool": [5, 7]`)の serde。
/// 前に 1 つだけ持っていた項目を、データの形を変えずにいくつでも持てるようにする。
pub mod one_or_many {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        Many(Vec<T>),
        One(T),
    }

    pub fn serialize<T: Serialize, S: Serializer>(v: &[T], s: S) -> Result<S::Ok, S::Error> {
        match v {
            [one] => one.serialize(s),
            _ => v.serialize(s),
        }
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<T>, D::Error> {
        Ok(match OneOrMany::deserialize(d)? {
            OneOrMany::Many(v) => v,
            OneOrMany::One(x) => vec![x],
        })
    }
}
