//! 面取り加工(`cam.chamfer2d@1`)。V ビットで、上向きの平らな面の縁(角)を斜めに落とす。
//!
//! 面を指定しなければ、部品のいちばん高い上向きの面を使う。縁のうち、外側が低くなっている所
//! (部品の外形や、穴とポケットの口)だけを削る。外側に壁が立ち上がる縁(ボスの根元など)は、
//! 削ると壁に食い込むので飛ばして知らせる。
//!
//! 刃先の角度を `2a` とすると、縁から工具の中心までを `x = t·tan(a)`、先端の高さを
//! 面から `t + w / tan(a)` 下にすると、面の上で幅 `w` の面取りになる(`t` は先端を面取りの
//! 下端より下げる量。とがった先で削らないため)。外側は時計回り、穴は反時計回りに回る。

use jc_cam_geom::{Loop, offset_loop_outward};
use jc_cam_setup::{
    OpBase, Planner, StrategyInput, StrategyOutput, approach, expr_refs, invalid, length,
    loop_moves, plan_error, prepare, retract,
};
use jc_cam_tool::ToolKind;
use jc_core::Field;
use jc_core::{EvalCtx, EvalError, Feature, InputRef, Outputs, Registry};
use jc_toolpath::Move;
use serde::{Deserialize, Serialize};

mod edges;

pub use edges::{ALL_EDGES, USE_EDGES};

pub const CHAMFER: &str = "cam.chamfer2d@1";

/// どの縁を削るか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edges {
    /// 外周と穴の両方
    #[default]
    All,
    /// 外周だけ
    Outer,
    /// 穴(ポケットの口を含む)だけ
    Holes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chamfer2d {
    #[serde(flatten)]
    pub base: OpBase,
    /// 縁を削る上向きの平らな面の名前。空なら部品のいちばん高い上向きの面
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    #[serde(default)]
    pub edges: Edges,
    /// 面取りの幅(面の上で測る)
    #[serde(default = "half_mm")]
    pub width: String,
    /// 先端を面取りの下端より下げる量
    #[serde(default = "tip_offset")]
    pub tip_offset: String,
    /// 3D のビューで選んだ辺だけを削る(辺の両側の面の名前の組。「選んだ辺だけを面取りする」が書く)。
    /// 空なら `edges` の縁の種類で削る
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<[String; 2]>,
}

fn half_mm() -> String {
    "0.5 mm".into()
}

fn tip_offset() -> String {
    "0.2 mm".into()
}

/// 輪郭 `l` の外側(面の外。外周なら外、穴なら穴の中)がどこも `z` より低いか。
/// 外側の高さは、縁から `probe` だけ離れた点で調べる。`keep` が偽の所(削らない所)は調べない。
fn falls_away(
    model: &jc_cam_geom::MachiningModel,
    l: &Loop,
    z: f64,
    probe: f64,
    keep: &dyn Fn([f64; 2]) -> bool,
) -> bool {
    let pts = l.flatten(0.05);
    let n = pts.len();
    (0..n).all(|i| {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy);
        if len < 1e-9 || !keep([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]) {
            return true;
        }
        // 外周は反時計回り、穴は時計回りなので、どちらも右側が面の外
        let q = [
            (a[0] + b[0]) / 2.0 + dy / len * probe,
            (a[1] + b[1]) / 2.0 - dx / len * probe,
        ];
        model.top_at(q).is_none_or(|t| t < z - 1e-3)
    })
}

/// ホストが解いた値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamferPlan {
    pub w: f64,
    pub t: f64,
    pub face: Option<String>,
    pub edges: Edges,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<[String; 2]>,
}

/// 点 `p` から線分 `a`-`b` までの距離。
fn to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-18 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

/// 面 `face` の高さ `z` の縁のうち、面の名前が `others` の面と接する所(選んだ辺)を平面に写した線分。
/// 形の三角形のうち、その面の三角形の辺で両端が高さ `z` にある物。
fn edge_segments(
    model: &jc_cam_geom::MachiningModel,
    others: &[&str],
    z: f64,
) -> Vec<([f64; 2], [f64; 2])> {
    let ids: Vec<u32> = model
        .face_names
        .iter()
        .enumerate()
        .filter(|(_, n)| others.contains(&n.as_str()))
        .map(|(i, _)| i as u32)
        .collect();
    let mut v = Vec::new();
    for (t, f) in model.triangles.iter().zip(&model.triangle_faces) {
        if !ids.contains(f) {
            continue;
        }
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if (a[2] - z).abs() < 1e-3 && (b[2] - z).abs() < 1e-3 {
                v.push(([a[0], a[1]], [b[0], b[1]]));
            }
        }
    }
    v
}

/// 閉じた経路の点 `pts` のうち `on` の点が続く所を、開いた経路に切り出す。全部 `on` なら `None`
/// (閉じた経路のまま削る)。
fn runs(pts: &[[f64; 2]], on: &dyn Fn([f64; 2]) -> bool) -> Option<Vec<Vec<[f64; 2]>>> {
    let marks: Vec<bool> = pts.iter().map(|p| on(*p)).collect();
    let off = marks.iter().position(|m| !m)?;
    let n = pts.len();
    let mut out = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    // 削らない点から回り始め、1 周して戻る(始めの点をまたぐ所も 1 本にする)
    for i in 1..=n {
        let k = (off + i) % n;
        if marks[k] {
            cur.push(pts[k]);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    out.retain(|r| r.len() >= 2);
    Some(out)
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let mut p = Planner::new(input);
    let ChamferPlan {
        w,
        t,
        face,
        edges,
        selected,
    } = input.params().map_err(invalid)?;
    // 面を決めていなければ、選んだ辺が縁にある上向きの面のうち、いちばん高い面
    let face = face.or_else(|| {
        p.model
            .flats
            .iter()
            .filter(|f| f.up && selected.iter().any(|s| s.contains(&f.name)))
            .max_by(|a, b| a.z.total_cmp(&b.z))
            .map(|f| f.name.clone())
    });
    let face = match &face {
        Some(name) => p
            .model
            .flat(name)
            .filter(|f| f.up)
            .ok_or_else(|| {
                invalid(format!(
                    "面 {name} が見つかりません(参照が切れたか、上向きの水平な平面ではありません)"
                ))
            })?
            .clone(),
        None => p
            .model
            .flats
            .iter()
            .filter(|f| f.up)
            .max_by(|a, b| {
                if (a.z - b.z).abs() > 1e-6 {
                    a.z.total_cmp(&b.z)
                } else {
                    a.area.area().total_cmp(&b.area.area())
                }
            })
            .ok_or_else(|| invalid("部品に上向きの平らな面がありません"))?
            .clone(),
    };
    let half = (p.tool.tip_angle / 2.0).to_radians();
    let x = t * half.tan();
    if w + x > p.r() + 1e-9 {
        return Err(invalid(format!(
            "面取りの幅 {w:.2} mm と先端を下げる分 {x:.2} mm の和が、工具 {} の半径 {:.2} mm を超えています",
            p.tool.name,
            p.r()
        )));
    }
    let top = face.z;
    let bottom = top - t - w / half.tan();
    p.check_depth(top, bottom)?;
    let area = face.area.clone().normalized();
    // 選んだ辺だけを削る時は、面の縁のうち選んだ辺の所だけ
    let segs = if selected.is_empty() {
        None
    } else {
        let others: Vec<&str> = selected
            .iter()
            .filter_map(|[a, b]| {
                if *a == face.name {
                    Some(b.as_str())
                } else if *b == face.name {
                    Some(a.as_str())
                } else {
                    None
                }
            })
            .collect();
        if others.is_empty() {
            return Err(invalid(format!(
                "選んだ辺は面 {} の縁にありません(「辺の選びをやめる」か、面を選び直してください)",
                face.name
            )));
        }
        let segs = edge_segments(&p.model, &others, top);
        if segs.is_empty() {
            return Err(invalid(
                "選んだ辺が加工の形に見つかりません(参照が切れました)",
            ));
        }
        Some(segs)
    };
    let near = |q: [f64; 2], d: f64| {
        segs.as_ref()
            .is_none_or(|s| s.iter().any(|(a, b)| to_segment(q, *a, *b) <= d))
    };
    let mut loops: Vec<(Loop, bool)> = Vec::new();
    if edges != Edges::Holes || segs.is_some() {
        loops.push((area.outer.clone(), true));
    }
    if edges != Edges::Outer || segs.is_some() {
        loops.extend(area.holes.iter().map(|h| (h.clone(), false)));
    }
    let mut skipped = 0;
    let mut paths: Vec<Loop> = Vec::new();
    let mut open: Vec<Vec<[f64; 2]>> = Vec::new();
    for (l, outer) in loops {
        // 選んだ辺の無い縁は削らない
        if segs.is_some() && !l.flatten(0.05).iter().any(|q| near(*q, 0.05)) {
            continue;
        }
        if !falls_away(&p.model, &l, top, (w + x).max(0.1), &|q| near(q, 0.05)) {
            skipped += 1;
            continue;
        }
        // 外周は外へ、穴は穴の中へ `x` だけずらす
        let shifted = if x < 1e-6 {
            vec![if l.signed_area() < 0.0 {
                l.reversed()
            } else {
                l.clone()
            }]
        } else if outer {
            offset_loop_outward(&l, x)
        } else {
            offset_loop_outward(&l, -x)
        };
        for s in shifted {
            // 外周は時計回り、穴の中は反時計回り(どちらもダウンカット)
            let s = if outer { s.reversed() } else { s };
            // 選んだ辺の所だけを開いた経路で削る(縁が全部選んだ辺なら閉じたまま)
            match segs
                .as_ref()
                .and_then(|_| runs(&s.flatten(0.05), &|q| near(q, x + 0.05)))
            {
                Some(r) => open.extend(r),
                None => paths.push(s),
            }
        }
    }
    if skipped > 0 {
        p.warnings.push(format!(
            "外側に壁が立ち上がる縁 {skipped} 本は、壁に食い込むので削りません"
        ));
    }
    if paths.is_empty() && open.is_empty() {
        return Err(invalid("面取りできる縁がありません"));
    }
    let mut tp = p.toolpath();
    // 形にない面取りを削るので、面取りの縦の深さまでの食い込みは許す
    tp.edge_break = w / half.tan() + 0.05;
    for path in paths {
        let start = [path.verts[0].x, path.verts[0].y];
        approach(&mut tp, start);
        for z in p.levels(top, bottom) {
            tp.moves.push(Move::Linear {
                to: [start[0], start[1], z],
                feed: p.cutting.plunge,
            });
            tp.moves.extend(loop_moves(&path, z, p.cutting.feed));
        }
        retract(&mut tp);
    }
    // 開いた経路は段ごとに始めの点から削り直す(同じ向きのまま。ダウンカットを保つ)
    for run in open {
        for z in p.levels(top, bottom) {
            approach(&mut tp, run[0]);
            tp.moves.push(Move::Linear {
                to: [run[0][0], run[0][1], z],
                feed: p.cutting.plunge,
            });
            tp.moves.extend(run[1..].iter().map(|q| Move::Linear {
                to: [q[0], q[1], z],
                feed: p.cutting.feed,
            }));
        }
        retract(&mut tp);
    }
    Ok(p.output(tp))
}

impl Feature for Chamfer2d {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs([&self.width, &self.tip_offset])?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if p.tool.kind != ToolKind::VBit {
            return Err(invalid(format!(
                "面取りには V ビットを選んでください(工具 {} は V ビットではありません)",
                p.tool.name
            )));
        }
        let w = length(ctx, &self.width, "面取りの幅")?;
        let t = length(ctx, &self.tip_offset, "先端を下げる量")?;
        if w <= 0.0 || t < 0.0 {
            return Err(invalid(
                "面取りの幅は 0 より大きく、先端を下げる量は 0 以上にしてください",
            ));
        }
        let params = ChamferPlan {
            w,
            t,
            face: self.face.clone(),
            edges: self.edges,
            selected: self.selected.clone(),
        };
        let input = p.input(ctx.feature_id(), params, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        Ok(c.base
            .rename([&mut c.width, &mut c.tip_offset], old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        jc_cam_setup::operation(
            CHAMFER,
            "面取り",
            "V ビットで、いちばん高い面(または選んだ面)の縁を斜めに落とす",
            vec![
                // 面は欄に出し、3D で選んだ面に替えられる
                Field::info("face", "縁を削る面")
                    .help("空ならいちばん高い面。3D で面を選び「選んだ面に付け替える」で変える"),
                Field::choice(
                    "edges",
                    "削る縁",
                    &[
                        ("all", "外周と穴"),
                        ("outer", "外周だけ"),
                        ("holes", "穴だけ"),
                    ],
                ),
                Field::expr("width", "面取りの幅")
                    .length()
                    .help("面の上で測る"),
                Field::expr("tip_offset", "先端を下げる量")
                    .length()
                    .help("とがった先で削らないよう、面取りの下端より下げる"),
            ],
        )
        .icon("chamferop")
        .retarget(jc_core::Retarget::Face { key: "face".into() }),
        |d| Ok(Box::new(jc_cam_setup::parse::<Chamfer2d>(d)?)),
    );
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> jc_cam_setup::StrategyModule {
    use jc_cam_setup::{FaceMenu, Strategy, StrategyModule, to_data};
    use jc_cam_tool::ToolKind;
    StrategyModule::new(
        "jc-strategy-chamfer",
        register,
        vec![
            Strategy::new(CHAMFER, "cam.add.chamfer", "面取り", |p| {
                to_data(Chamfer2d {
                    base: p.with_tool(ToolKind::VBit),
                    face: p.planar_face(),
                    edges: Default::default(),
                    width: "0.5 mm".into(),
                    tip_offset: "0.2 mm".into(),
                    selected: Vec::new(),
                })
            })
            .order(10)
            .icon("chamferop")
            .not_front()
            .aliases("めんとり chamfer deburr vびっと vbit ばりとり")
            .hint("V ビットで縁を斜めに落とす。面を選ぶとその縁、選ばなければいちばん高い面の縁")
            .face_menu(FaceMenu::AnyFace)
            .commands(edges::commands),
        ],
    )
}
