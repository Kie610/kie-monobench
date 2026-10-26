//! シェル(`solid.shell@1`): 立体をくり抜いて、決めた厚みの殻にする。
//! 選んだ面は取り除いて開ける。ボディを選ぶと、開けずに中を空洞にする。カーネルの
//! [`kmb_core::Kernel::shell`] を使う(今は SMLib だけ)。
//!
//! 削って空ける所(内側へ厚みを付ければ内側の面と開ける面で囲まれた所、外側なら元の形)を
//! `tool_preview` の切り取りとして出す。直している間、画面が赤い半透明で描くので、開ける面の
//! 選び間違いに OK の前に気付ける。
use kmb_cad::*;
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Ref, RefKind, Registry, Value,
};
use serde::{Deserialize, Serialize};

pub const SHELL: &str = "solid.shell@1";

/// 厚みを付ける向き。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellDirection {
    /// 今の外形を残し、内側へ厚みを付ける
    #[default]
    Inside,
    /// 今の形を空洞にし、外側へ厚みを付ける
    Outside,
}

/// 例: `{"faces": [{"kind": "face", "feature": 7, "face": "f7:end"}], "thickness": "2 mm"}`。
/// `faces` にボディ(`7`)を入れると、開けずに中を空洞にする。どれも 1 つのボディの物。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shell {
    /// 開ける面か、空洞にするボディ
    #[serde(default, with = "kmb_core::one_or_many")]
    pub faces: Vec<Ref>,
    pub thickness: String,
    #[serde(default)]
    pub direction: ShellDirection,
    /// 選んだ面に接して(なめらかに)つながる面も開ける(接線チェーン。画面から作る時の
    /// 既定は入。この欄の無い古い文書は切)
    #[serde(default)]
    pub tangent_chain: bool,
}

impl Shell {
    /// くり抜くボディ(選んだ物の機能。どれも同じボディ)。
    fn body(&self) -> Result<FeatureId, EvalError> {
        let first = self.faces.first().ok_or_else(|| {
            EvalError::InvalidData("開ける面か、空洞にするボディを選んでください".into())
        })?;
        if self.faces.iter().any(|r| r.feature() != first.feature()) {
            return Err(EvalError::InvalidData(
                "シェルの面は 1 つのボディから選んでください".into(),
            ));
        }
        Ok(first.feature())
    }

    /// 開ける面の名前(ボディを選んだ時は空)。
    fn open_faces(&self) -> Result<Vec<String>, EvalError> {
        let body = self.faces.iter().any(|r| matches!(r, Ref::Body(_)));
        let mut names = Vec::new();
        for r in &self.faces {
            match r {
                Ref::Face { face, .. } => names.push(face.clone()),
                Ref::Body(_) => {}
                Ref::Edge { .. } | Ref::Vertex { .. } => {
                    return Err(EvalError::InvalidData(
                        "シェルには面かボディを選んでください(辺や頂点は使えません)".into(),
                    ));
                }
            }
        }
        if body && !names.is_empty() {
            return Err(EvalError::InvalidData(
                "ボディを選んだ時は面を選べません(開ける面だけか、ボディだけにしてください)".into(),
            ));
        }
        Ok(names)
    }
}

impl Exprs for Shell {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.thickness]
    }
}

impl Feature for Shell {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.body()?, "body")];
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        self.body()
            .map(|b| vec![("body".into(), output(b, "body"))])
            .unwrap_or_default()
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let t = length(ctx, &self.thickness, "厚さ")?;
        if t <= 0.0 {
            return Err(EvalError::InvalidData(
                "厚みは 0 より大きくしてください".into(),
            ));
        }
        let mut open = self.open_faces()?;
        let body = body_of(ctx, self.body()?)?;
        let src = ctx.kernel().mesh(body, PREVIEW_TOLERANCE).ok();
        if let Some(m) = src.as_ref().filter(|_| !open.is_empty()) {
            if self.tangent_chain {
                // カーネルが継ぎ目を答えられれば、フィレットの辺のたどりと同じ答えでたどる(細かい
                // 丸めでも網の粗さに左右されない)。答えられない時だけ網の法線で見る
                open = smooth_chain(ctx.kernel(), body, &open)
                    .unwrap_or_else(|| tangent_faces(m, &open));
            }
            // 全部の面を開けると殻が残らない(カーネルの分かりにくい誤りの前に理由を出す)
            let all: std::collections::BTreeSet<&str> =
                m.face_names.iter().map(String::as_str).collect();
            if all.iter().all(|f| open.iter().any(|o| o == f)) {
                return Err(EvalError::InvalidData(if self.tangent_chain {
                    "接線でつながる面をたどると全部の面が開き、本体が残りません。「接線チェーン」を外して、開ける面だけを選んでください".into()
                } else {
                    "全部の面を開けると本体が残りません。開ける面を減らしてください".into()
                }));
            }
        }
        let signed = match self.direction {
            ShellDirection::Inside => t,
            ShellDirection::Outside => -t,
        };
        let tag = tag(ctx);
        let h = ctx.kernel().shell(body, &open, signed, &tag).map_err(|e| {
            EvalError::InvalidData(format!(
                "シェルを作れませんでした({e})。厚みを小さくするか、開ける面を選び直してください"
            ))
        })?;
        let mut out: Outputs = [("body".to_string(), Value::Shape(h))].into();
        if let Some(p) = src.and_then(|m| removed(ctx.kernel(), &m, h, &tag, self.direction)) {
            out.insert(
                kmb_core::TOOL_PREVIEW.into(),
                kmb_core::Payload::to_value(&p),
            );
        }
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

/// 予告の三角形の数の上限(結果に JSON で残るので重くしない。超えたら予告を出さない)
const PREVIEW_TRIANGLES: usize = 20_000;
/// 予告の網の粗さ(mm)
const PREVIEW_TOLERANCE: f64 = 0.1;

/// 削って空ける所の形(切り取りの予告)。内側へ厚みを付けた時は、殻の内側の面(名前が
/// `{tag}:in:…` か `{tag}:face`)を、空ける所の外向きに裏返した物(開ける面の側は閉じない。
/// 開ける面は縁の分だけ空ける所より大きいため)。外側なら元の形そのもの。
fn removed(
    kernel: &dyn kmb_core::Kernel,
    src: &kmb_core::Mesh,
    shelled: kmb_core::ShapeHandle,
    tag: &str,
    direction: ShellDirection,
) -> Option<kmb_core::ToolPreview> {
    let tris = |m: &kmb_core::Mesh, keep: &dyn Fn(&str) -> bool, flip: bool| {
        let at = |i: u32| m.positions[i as usize].map(f64::from);
        m.triangles
            .iter()
            .zip(&m.triangle_faces)
            .filter(|(_, f)| m.face_names.get(**f as usize).is_some_and(|n| keep(n)))
            .map(|(t, _)| {
                let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
                if flip { [a, c, b] } else { [a, b, c] }
            })
            .collect::<Vec<_>>()
    };
    let faces = match direction {
        ShellDirection::Outside => tris(src, &|_| true, false),
        ShellDirection::Inside => {
            let inner = kernel.mesh(shelled, PREVIEW_TOLERANCE).ok()?;
            let (a, b) = (format!("{tag}:in:"), format!("{tag}:face"));
            tris(&inner, &|n| n.starts_with(&a) || n.starts_with(&b), true)
        }
    };
    (!faces.is_empty() && faces.len() <= PREVIEW_TRIANGLES).then(|| kmb_core::ToolPreview {
        op: kmb_core::ToolOp::Cut,
        lines: Vec::new(),
        faces,
        profile: Vec::new(),
        arrows: Vec::new(),
    })
}

/// カーネルの辺の継ぎ目で、選んだ面からなめらかにつながる面をたどる([`kmb_cad::smooth_faces`])。
/// 辺を列挙できない・継ぎ目の分からない辺がある時は `None`。
fn smooth_chain(
    kernel: &dyn kmb_core::Kernel,
    body: kmb_core::ShapeHandle,
    seeds: &[String],
) -> Option<Vec<String>> {
    let mut all = kernel.edges(body).ok()?;
    all.extend(kernel.arc_edges(body).ok()?);
    all.extend(kernel.curve_edges(body).ok()?);
    kmb_cad::smooth_faces(&all, seeds)
}

/// 選んだ面 `seeds` から、接してつながる面をたどった名前(選んだ面が先、たどった面は後ろ)。
/// 2 つの面が同じ位置に網の頂点を持ち、そこでの法線の差が [`kmb_cad::TANGENT_DEG`] 以内なら、
/// 接してつながるとみなす(網の頂点の法線は曲面の法線。縁は両側の面が同じ点で網を切る)。
fn tangent_faces(m: &kmb_core::Mesh, seeds: &[String]) -> Vec<String> {
    use std::collections::{BTreeSet, HashMap};
    // 頂点ごとの面(網の頂点は面ごとに別に持つ)
    let mut face_of = vec![None; m.positions.len()];
    for (t, f) in m.triangles.iter().zip(&m.triangle_faces) {
        for &i in t {
            face_of[i as usize] = Some(*f as usize);
        }
    }
    // 同じ位置(1 µm で丸める)の頂点を束ねる
    let mut at: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    for (i, f) in face_of.iter().enumerate() {
        if f.is_some() {
            let k = m.positions[i].map(|v| (f64::from(v) * 1e3).round() as i64);
            at.entry(k).or_default().push(i);
        }
    }
    let cos = kmb_cad::TANGENT_DEG.to_radians().cos();
    let normal = |i: usize| m.normals.get(i).map(|n| n.map(f64::from));
    let mut next = vec![BTreeSet::new(); m.face_names.len()];
    for list in at.values() {
        for &a in list {
            for &b in list {
                let (Some(fa), Some(fb)) = (face_of[a], face_of[b]) else {
                    continue;
                };
                if fa != fb
                    && let (Some(na), Some(nb)) = (normal(a), normal(b))
                    && na[0] * nb[0] + na[1] * nb[1] + na[2] * nb[2] >= cos
                {
                    next[fa].insert(fb);
                }
            }
        }
    }
    let mut out: Vec<String> = seeds.to_vec();
    let mut todo: Vec<usize> = (0..m.face_names.len())
        .filter(|&f| seeds.contains(&m.face_names[f]))
        .collect();
    let mut seen: BTreeSet<usize> = todo.iter().copied().collect();
    while let Some(f) = todo.pop() {
        for &g in &next[f] {
            if seen.insert(g) {
                out.push(m.face_names[g].clone());
                todo.push(g);
            }
        }
    }
    out
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(SHELL, "シェル")
            .menu("修正")
            .icon("shell")
            .pinned()
            // 修正の前の並び: プレス/プル・フィレット・面取り・シェル・ブーリアン・ボディを分割・移動/コピー
            // (Kie 2026-10-21 の画板)
            .order(-40)
            .hint("立体をくり抜いて殻にする。選んだ面は取り除いて開け、ボディを選ぶと中を空洞にする")
            .aliases("しぇる shell くりぬき 中空")
            .field(
                Field::pick("faces", "面/ボディ", &[RefKind::Face, RefKind::Body], 1, None).help(
                    "開ける面をクリックで足す(もう一度で外す)。ビューの左上で「ボディ」にしてボディをクリックすると、開けずに中を空洞にする",
                ),
            )
            // 面の選びのすぐ下
            .field(Field::bool("tangent_chain", "接線チェーン").help(
                "選んだ面になめらかに(2° 以内で)つながる面も開ける。丸めた角に囲まれた面を選ぶと、つながる面を全部たどる(全部の面になると本体が残らないので断る)。外すと選んだ面だけを開ける",
            ))
            .field(Field::expr("thickness", "厚さ").length())
            .field(Field::choice(
                "direction",
                "方向",
                &[("inside", "内側"), ("outside", "外側")],
            ))
            .template(serde_json::json!({"thickness": "1 mm", "direction": "inside", "tangent_chain": true})),
        |d| Ok(Box::new(parse::<Shell>(d)?)),
    );
}
