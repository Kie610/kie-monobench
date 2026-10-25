//! フェイス加工(`cam.face@1`)。ストックの上面を、部品のいちばん高い所まで平らに削る。
//!
//! パスの向き(既定は X 方向)に往復しながら、送り幅ずつ横へ進む。折り返しは範囲の外で行う。
//! 一方向(ダウンカットかアップカット)を選ぶと、毎回同じ向きに削り、上げて戻る。
//! 範囲は既定でストック全体。部品の外形の箱か、スケッチの閉じた輪郭にもでき、広げる距離
//! (負で狭める)で縁を動かせる。範囲が凹んでいて 1 本の行が範囲の外をまたぐ所は、上げて移る。

use kmb_cam_geom::{Area, Loop, P2, offset_area};
use kmb_cam_setup::{
    OpBase, Sources, StrategyInput, StrategyOutput, approach, expr_refs, invalid, length, output,
    prepare,
};
use kmb_core::{Dim, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Registry};
use kmb_toolpath::Move;
use serde::{Deserialize, Serialize};

pub const FACE: &str = "cam.face@1";

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// 行の削り方。主軸は時計回り(M3)とする。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacePasses {
    /// 往復(行の終わりで横へ移り、逆向きに戻る)
    #[default]
    Both,
    /// 一方向のダウンカット(削っていない側が進む向きの右)。行ごとに上げて戻る
    Climb,
    /// 一方向のアップカット(削っていない側が進む向きの左)。行ごとに上げて戻る
    Conventional,
}

/// 削る範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceArea {
    /// ストック全体
    #[default]
    Stock,
    /// 部品の外形の箱(上から見た部品の外接する長方形)
    Part,
    /// スケッチの閉じた輪郭(`profile`)
    Sketch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    #[serde(flatten)]
    pub base: OpBase,
    /// 部品の上面より上に残す厚さ
    #[serde(default = "zero")]
    pub leave: String,
    /// パスの向き(X 軸から反時計回りの角度。例: `90 deg`)。空なら X 方向
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// 往復か、一方向か
    #[serde(default, skip_serializing_if = "is_default")]
    pub passes: FacePasses,
    /// 反対の端の行から始める
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub other_side: bool,
    /// 行の端で、工具の縁を範囲の外へ出す距離。空なら 1 mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrun: Option<String>,
    /// 削る範囲
    #[serde(default, skip_serializing_if = "is_default")]
    pub area: FaceArea,
    /// 範囲がスケッチの時の、閉じた輪郭を出すスケッチ
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<FeatureId>,
    /// 範囲を外へ広げる距離(負なら内へ狭める)。空なら 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
}

fn zero() -> String {
    "0 mm".into()
}

/// 行の端で範囲の外へ出す距離の既定(mm)。
const OVERRUN: f64 = 1.0;

impl Face {
    /// ストック全体を X 方向の往復で削るフェイス(ほかの欄は初めの値)。
    pub fn new(base: OpBase) -> Face {
        Face {
            base,
            leave: zero(),
            angle: None,
            passes: FacePasses::Both,
            other_side: false,
            overrun: None,
            area: FaceArea::Stock,
            profile: None,
            offset: None,
        }
    }

    fn exprs(&self) -> Vec<&String> {
        let mut v = vec![&self.leave];
        v.extend(
            [&self.angle, &self.overrun, &self.offset]
                .into_iter()
                .flatten(),
        );
        v
    }
}

/// 行 1 本: 横の位置 `v` と、パスの向きに沿った区間(`u` の始めと終わり、小さい順)。
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub v: f64,
    pub spans: Vec<(f64, f64)>,
}

/// 範囲の折れ線(XY)を、パスの向き `angle`(ラジアン)の座標 (u, v) にする。
fn to_uv(p: P2, angle: f64) -> P2 {
    let (s, c) = angle.sin_cos();
    [p[0] * c + p[1] * s, -p[0] * s + p[1] * c]
}

/// パスの向きの座標 (u, v) を XY に戻す。
pub fn from_uv(q: P2, angle: f64) -> P2 {
    let (s, c) = angle.sin_cos();
    [q[0] * c - q[1] * s, q[0] * s + q[1] * c]
}

/// 横の位置 `v` の線が範囲の折れ線の中を通る区間(`u` の小さい順)。
fn crossings(polys: &[Vec<P2>], v: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for poly in polys {
        let n = poly.len();
        let mut us: Vec<f64> = Vec::new();
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            if (a[1] > v) != (b[1] > v) {
                us.push(a[0] + (v - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
            }
        }
        us.sort_by(f64::total_cmp);
        out.extend(us.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])));
    }
    out
}

/// 区間をまとめる(重なる・接するものは 1 つに)。
fn merge(mut spans: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(l) if s.0 <= l.1 + 1e-9 => l.1 = l.1.max(s.1),
            _ => out.push(s),
        }
    }
    out
}

/// 範囲(XY の閉じた折れ線の列)を削る行。行は送り幅 `step` 以下の等しい間隔で、範囲の横の端から
/// 端まで並ぶ(端の行は工具の中心が縁の上)。各行の区間は、行の両側の送り幅の半分の帯にかかる範囲を
/// 覆い、両端を `ext`(工具の半径 + 外へ出す距離)だけ伸ばす。`other_side` なら反対の端の行から。
pub fn rows(polys: &[Vec<P2>], angle: f64, step: f64, ext: f64, other_side: bool) -> Vec<Row> {
    let uv: Vec<Vec<P2>> = polys
        .iter()
        .map(|p| p.iter().map(|q| to_uv(*q, angle)).collect())
        .collect();
    let (lo, hi) = uv
        .iter()
        .flatten()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| {
            (lo.min(q[1]), hi.max(q[1]))
        });
    if hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater) {
        return vec![];
    }
    let n = (((hi - lo) / step).ceil() as usize).max(1);
    let half = (hi - lo) / n as f64 / 2.0;
    let eps = 1e-7;
    let mut out: Vec<Row> = (0..=n)
        .map(|i| {
            let v = lo + (hi - lo) * i as f64 / n as f64;
            let mut spans = Vec::new();
            for k in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                let y = (v + half * k).clamp(lo + eps, hi - eps);
                spans.extend(crossings(&uv, y));
            }
            let spans = merge(
                merge(spans)
                    .into_iter()
                    .map(|(a, b)| (a - ext, b + ext))
                    .collect(),
            );
            Row { v, spans }
        })
        .filter(|r| !r.spans.is_empty())
        .collect();
    if other_side {
        out.reverse();
    }
    out
}

/// 1 段(高さ `z`)の動き。行は `rows` の順にたどる。往復なら、行の区間が 1 つずつの時は
/// 範囲の外を通って横へ移る(行の端の外側を回る)。そうでなければ上げて次の区間の始まりで下げる。
pub fn level_moves(
    tp: &mut kmb_toolpath::Toolpath,
    rows: &[Row],
    angle: f64,
    z: f64,
    passes: FacePasses,
    (feed, plunge): (f64, f64),
) {
    // 削っていない側(次の行の側)が、進む向きの右ならダウンカット
    let ahead = match rows {
        [a, b, ..] => (b.v - a.v).signum(),
        _ => 1.0,
    };
    let xyz = |u: f64, v: f64| {
        let p = from_uv([u, v], angle);
        [p[0], p[1], z]
    };
    // 前の行の終わり(u, v)と、前の行の区間が 1 つだったか
    let mut prev: Option<(f64, f64, bool)> = None;
    for (i, row) in rows.iter().enumerate() {
        let forward = match passes {
            FacePasses::Both => i % 2 == 0,
            // 未削の側が +v の時、-u 向きに進むと未削の側が右になる
            FacePasses::Climb => ahead < 0.0,
            FacePasses::Conventional => ahead > 0.0,
        };
        let spans: Vec<(f64, f64)> = if forward {
            row.spans.clone()
        } else {
            row.spans.iter().rev().map(|&(a, b)| (b, a)).collect()
        };
        let single = spans.len() == 1;
        for (k, &(a, b)) in spans.iter().enumerate() {
            let link = passes == FacePasses::Both && k == 0 && single;
            match prev {
                Some((pu, pv, true)) if link => {
                    // 行の端の外側を回って横へ移る(範囲の内に入らないよう、外側の端まで出てから)
                    let out = if forward { pu.min(a) } else { pu.max(a) };
                    for to in [xyz(out, pv), xyz(out, row.v), xyz(a, row.v)] {
                        let cur = tp.moves.last().map(|m| m.to());
                        if cur.is_none_or(|c| kmb_toolpath::dist(c, to) > 1e-9) {
                            tp.moves.push(Move::Linear { to, feed });
                        }
                    }
                }
                _ => {
                    let s = from_uv([a, row.v], angle);
                    approach(tp, s);
                    tp.moves.push(Move::Linear {
                        to: [s[0], s[1], z],
                        feed: plunge,
                    });
                }
            }
            tp.moves.push(Move::Linear {
                to: xyz(b, row.v),
                feed,
            });
            prev = Some((b, row.v, single));
        }
    }
}

/// 長方形の折れ線(反時計回り)。
fn rect(min: [f64; 3], max: [f64; 3]) -> Vec<P2> {
    vec![
        [min[0], min[1]],
        [max[0], min[1]],
        [max[0], max[1]],
        [min[0], max[1]],
    ]
}

/// フェイスの解いた値(純関数 [`plan`] の入力の `params`)。長さは mm。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FacePlan {
    pub leave: f64,
    /// パスの向き(式の角度の値)
    pub angle: f64,
    /// 行の端で範囲の外へ出す距離。None なら 1 mm
    pub overrun: Option<f64>,
    pub offset: f64,
    pub passes: FacePasses,
    pub other_side: bool,
    pub area: FaceArea,
}

/// フェイスの経路を作る純関数。文書とカーネルに触らず、`input` だけから作る
/// (範囲がスケッチの時は、ホストが輪郭を `input.sources.areas` に入れる)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    let f: FacePlan = input.params()?;
    let (min, max) = (input.setup.stock_min, input.setup.stock_max);
    let top = input.top(max[2]);
    let bottom = input.bottom(input.model.max[2] + f.leave);
    if bottom >= top - 1e-6 {
        return Err("削る厚さがありません(トップ高さがボトム高さ・部品の上面と同じか下)".into());
    }
    input.check_depth(top, bottom)?;
    let overrun = match f.overrun {
        Some(v) if !(v >= 0.0 && v.is_finite()) => {
            return Err("外へ出す距離は 0 以上にしてください".into());
        }
        Some(v) => v,
        None => OVERRUN,
    };
    let base: Vec<Area> = match f.area {
        FaceArea::Stock => vec![Area {
            outer: Loop::from_points(&rect(min, max)),
            holes: vec![],
        }],
        FaceArea::Part => vec![Area {
            outer: Loop::from_points(&rect(input.model.min, input.model.max)),
            holes: vec![],
        }],
        // 輪郭の内側の穴も削る(外周だけを使う)
        FaceArea::Sketch => input
            .sources
            .areas
            .iter()
            .map(|a| Area {
                outer: a.outer.clone(),
                holes: vec![],
            })
            .collect(),
    };
    let areas: Vec<Area> = if f.offset.abs() < 1e-9 {
        base
    } else {
        base.iter()
            .flat_map(|a| offset_area(a, -f.offset))
            .collect()
    };
    let polys: Vec<Vec<P2>> = areas.iter().map(|a| a.outer.flatten(1e-3)).collect();
    if polys.iter().all(|q| q.len() < 3) {
        return Err("削る範囲がありません(狭める距離が大きすぎます)".into());
    }
    let r = input.r();
    let step = input.tool.diameter * input.cutting.stepover;
    let rows = rows(&polys, f.angle, step, r + overrun, f.other_side);
    if rows.is_empty() {
        return Err("削る範囲がありません".into());
    }
    let mut warnings = vec![];
    // 範囲の外の行の端がストックの上にあると、そこで材料へ真下に下りる
    let (lo, hi) = ([min[0] - r, min[1] - r], [max[0] + r, max[1] + r]);
    let over_stock = rows.iter().any(|row| {
        row.spans.iter().any(|&(a, b)| {
            [a, b].iter().any(|&u| {
                let q = from_uv([u, row.v], f.angle);
                (lo[0]..=hi[0]).contains(&q[0]) && (lo[1]..=hi[1]).contains(&q[1])
            })
        })
    });
    if over_stock {
        warnings.push(
            "範囲がストックより狭いので、行の端ではストックの上で真下に下ります(下降の送り)".into(),
        );
    }
    let mut tp = input.toolpath();
    let feeds = (input.cutting.feed, input.cutting.plunge);
    for z in input.levels(top, bottom) {
        level_moves(&mut tp, &rows, f.angle, z, f.passes, feeds);
    }
    kmb_cam_setup::retract(&mut tp);
    Ok(StrategyOutput {
        toolpath: tp,
        warnings,
        top: Some(top),
        bottom: Some(bottom),
        entry_area: None,
        cutting: None,
        extra: Default::default(),
    })
}

impl Feature for Face {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs(self.exprs())?);
        if let Some(id) = self.profile.filter(|_| self.area == FaceArea::Sketch) {
            v.push(output(id, "profile"));
        }
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        // ホストの側: 式とスケッチの輪郭を解いて、純関数 `plan` の入力にする
        let mut p = prepare(ctx, &self.base)?;
        let leave = length(ctx, &self.leave, "残し量")?;
        let angle = match &self.angle {
            Some(src) => {
                let q = ctx.expr(src)?;
                if q.dim != Dim::ANGLE {
                    return Err(invalid(format!(
                        "パスの向き `{src}` は角度の単位(deg)が必要です(値: {q})"
                    )));
                }
                q.value
            }
            None => 0.0,
        };
        let overrun = match &self.overrun {
            Some(src) => Some(length(ctx, src, "外へ出す距離")?),
            None => None,
        };
        let offset = match &self.offset {
            Some(src) => length(ctx, src, "範囲を広げる距離")?,
            None => 0.0,
        };
        let mut sources = Sources::default();
        if self.area == FaceArea::Sketch {
            let id = self.profile.ok_or_else(|| {
                invalid("範囲のスケッチを選んでください(閉じた輪郭のあるスケッチ)")
            })?;
            sources.areas = kmb_cam_setup::profile_areas(ctx, &p, id)?.0;
        }
        let params = FacePlan {
            leave,
            angle,
            overrun,
            offset,
            passes: self.passes,
            other_side: self.other_side,
            area: self.area,
        };
        let input = p.input(ctx.feature_id(), params, sources);
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let others = [&mut c.angle, &mut c.overrun, &mut c.offset]
            .into_iter()
            .flatten()
            .chain(std::iter::once(&mut c.leave));
        let mut b = self.base.clone();
        Ok(b.rename(others, old, new)?.then(|| {
            c.base = b;
            serde_json::to_value(&c).expect("JSON にできる")
        }))
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        kmb_cam_setup::operation(
            FACE,
            "フェイス",
            "ストックの上面を部品の上面まで平らに削る",
            kmb_cam_setup::with_heights(vec![
                Field::expr("leave", "残し量").length(),
                Field::opt_expr("angle", "パスの向き").angle().help(
                    "例: 90 deg。X 軸から反時計回りの角度で、行をこの向きに削る。空なら X 方向。\
                     長い辺に沿わせると折り返しが減る",
                ),
                Field::choice(
                    "passes",
                    "行の削り方",
                    &[
                        ("both", "往復"),
                        ("climb", "一方向(ダウンカット)"),
                        ("conventional", "一方向(アップカット)"),
                    ],
                )
                .help("一方向は毎回同じ向きに削り、行の終わりで上げて始まりへ戻る(時間はかかるが面がそろう)"),
                Field::bool("other_side", "反対側から").help("行を反対の端から順に削る"),
                Field::opt_expr("overrun", "端の外へ出す距離").length()
                    .help("行の端で、工具の縁を範囲の外へ出す距離。空なら 1 mm"),
                Field::choice(
                    "area",
                    "削る範囲",
                    &[
                        ("stock", "ストック全体"),
                        ("part", "部品の外形の箱"),
                        ("sketch", "スケッチの輪郭"),
                    ],
                ),
                Field::opt_output("profile", "範囲のスケッチ", "profile")
                    .only("area", "sketch", "stock")
                    .help("閉じた輪郭の内側を削る(輪郭の中の穴も削る)"),
                Field::opt_expr("offset", "範囲を広げる距離").length().help(
                    "例: 2 mm で範囲の縁を外へ、-2 mm で内へ動かす。空なら 0。\
                     工具の中心は範囲の縁まで行くので、縁の外へ工具の半径ほど削る",
                ),
            ]),
        )
        .icon("face"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Face>(d)?)),
    );
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> kmb_cam_setup::StrategyModule {
    use kmb_cam_setup::{FaceMenu, Strategy, StrategyModule, to_data};
    StrategyModule::new(
        "kmb-strategy-face",
        register,
        vec![
            Strategy::new(FACE, "cam.add.face", "フェイス", |p| {
                to_data(Face::new(p.base()))
            })
            .order(2)
            .icon("face")
            .aliases("ふぇいす face facing めんだし")
            .face_menu(FaceMenu::AnyFace)
            .rough_grid(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_toolpath::Toolpath;

    fn square(w: f64, h: f64) -> Vec<Vec<P2>> {
        vec![vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]]
    }

    fn empty() -> Toolpath {
        Toolpath {
            spans: vec![],
            name: "f".into(),
            tool: kmb_cam_tool::ToolLibrary::starter().tools[0].clone(),
            rpm: 10000.0,
            safe_z: 5.0,
            retract_z: 1.0,
            edge_break: 0.0,
            dwells: vec![],
            links: Default::default(),
            moves: vec![],
        }
    }

    /// 切削の動き(送りの直線)の終わりの点。
    fn cuts(tp: &Toolpath) -> Vec<[f64; 3]> {
        tp.moves
            .iter()
            .filter(|m| matches!(m, Move::Linear { .. }))
            .map(|m| m.to())
            .collect()
    }

    /// 60 × 40、高さ 10 の部品の上面と、高さ 12 のストック。
    fn input(params: FacePlan) -> StrategyInput {
        let top = vec![
            [[0.0, 0.0, 10.0], [60.0, 0.0, 10.0], [60.0, 40.0, 10.0]],
            [[0.0, 0.0, 10.0], [60.0, 40.0, 10.0], [0.0, 40.0, 10.0]],
        ];
        let mesh = kmb_cam_setup::testing::mesh_of(&[("f1:end", top)]);
        let tool = kmb_cam_tool::ToolLibrary::starter().tools[0].clone();
        let p = kmb_cam_setup::testing::prepared(&mesh, [0.0; 3], [60.0, 40.0, 12.0], tool);
        p.input(FeatureId(7), params, Sources::default())
    }

    fn plain() -> FacePlan {
        FacePlan {
            leave: 0.0,
            angle: 0.0,
            overrun: None,
            offset: 0.0,
            passes: FacePasses::Both,
            other_side: false,
            area: FaceArea::Stock,
        }
    }

    #[test]
    fn the_plan_reads_only_its_input_and_round_trips_through_json() {
        let i = input(plain());
        let out = plan(&i).unwrap();
        assert_eq!((out.top, out.bottom), (Some(12.0), Some(10.0)));
        assert_eq!(out.toolpath.name, "F7");
        assert!(cuts(&out.toolpath).iter().all(|c| c[2] >= 10.0 - 1e-9));
        // WASM の戦略が受け取る形: JSON にして戻しても同じ入力で、同じ経路になる
        let text = serde_json::to_string(&i).unwrap();
        let back: StrategyInput = serde_json::from_str(&text).unwrap();
        assert_eq!(back, i);
        assert_eq!(plan(&back), Ok(out));
    }

    #[test]
    fn the_plan_reports_bad_values_as_text() {
        let e = plan(&input(FacePlan {
            overrun: Some(-1.0),
            ..plain()
        }));
        assert_eq!(e, Err("外へ出す距離は 0 以上にしてください".into()));
        let e = plan(&input(FacePlan {
            leave: 3.0,
            ..plain()
        }));
        assert!(e.unwrap_err().starts_with("削る厚さがありません"));
    }

    #[test]
    fn the_default_rows_run_along_x_and_turn_outside_the_stock() {
        // 60 × 40 のストック、送り幅 4.5、工具の半径 3 + 外へ 1
        let rows = rows(&square(60.0, 40.0), 0.0, 4.5, 4.0, false);
        assert_eq!(rows.len(), 10);
        assert!((rows[0].v - 0.0).abs() < 1e-9 && (rows[9].v - 40.0).abs() < 1e-9);
        for r in &rows {
            assert_eq!(r.spans.len(), 1);
            let (a, b) = r.spans[0];
            assert!((a + 4.0).abs() < 1e-6 && (b - 64.0).abs() < 1e-6, "{a} {b}");
        }
        let mut tp = empty();
        level_moves(&mut tp, &rows, 0.0, -1.0, FacePasses::Both, (800.0, 300.0));
        let c = cuts(&tp);
        // 下げて、行の端まで、横へ移って、逆向きに戻る
        assert_eq!(c[0], [-4.0, 0.0, -1.0]);
        assert_eq!(c[1], [64.0, 0.0, -1.0]);
        assert!((c[2][0] - 64.0).abs() < 1e-9 && (c[2][1] - 40.0 / 9.0).abs() < 1e-9);
        assert!((c[3][0] + 4.0).abs() < 1e-9);
        // 上げるのは始めの 1 回だけ
        let rapids = tp
            .moves
            .iter()
            .filter(|m| matches!(m, Move::Rapid { .. }))
            .count();
        assert_eq!(rapids, 2);
    }

    #[test]
    fn rows_along_y_follow_the_angle() {
        let angle = 90f64.to_radians();
        let rows = rows(&square(60.0, 40.0), angle, 4.5, 4.0, false);
        // 行は X に並ぶ(60 / 4.5 → 14 の間)。区間は Y の -4〜44
        assert_eq!(rows.len(), 15);
        let mut tp = empty();
        level_moves(
            &mut tp,
            &rows,
            angle,
            -1.0,
            FacePasses::Both,
            (800.0, 300.0),
        );
        let c = cuts(&tp);
        assert!(
            (c[0][1] + 4.0).abs() < 1e-6 && (c[1][1] - 44.0).abs() < 1e-6,
            "{c:?}"
        );
        assert!((c[0][0] - c[1][0]).abs() < 1e-6);
        // 全部の点が X の -4〜64 の内(ストックの縁 + 工具の半径 + 外へ出す距離)
        assert!(c.iter().all(|q| q[0] > -4.0 - 1e-6 && q[0] < 64.0 + 1e-6));
    }

    #[test]
    fn one_way_passes_lift_between_rows_and_cut_with_the_uncut_side_on_the_right() {
        let rows = rows(&square(20.0, 10.0), 0.0, 4.0, 4.0, false);
        let mut tp = empty();
        level_moves(&mut tp, &rows, 0.0, -1.0, FacePasses::Climb, (800.0, 300.0));
        // 行は +Y へ進むので、未削の側(+Y)を右にするには -X 向き
        let c = cuts(&tp);
        for pair in c.chunks(2) {
            assert!(pair[1][0] < pair[0][0], "{pair:?}");
        }
        let lifts = tp
            .moves
            .iter()
            .filter(|m| matches!(m, Move::Rapid { to } if (to[2] - 5.0).abs() < 1e-9))
            .count();
        assert!(lifts >= rows.len());
        let mut tp = empty();
        level_moves(
            &mut tp,
            &rows,
            0.0,
            -1.0,
            FacePasses::Conventional,
            (800.0, 300.0),
        );
        for pair in cuts(&tp).chunks(2) {
            assert!(pair[1][0] > pair[0][0]);
        }
    }

    #[test]
    fn the_other_side_starts_from_the_far_row() {
        let rows = rows(&square(20.0, 10.0), 0.0, 4.0, 4.0, true);
        assert!((rows[0].v - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_row_across_a_notch_is_split_and_lifts_over_it() {
        // U の形: 0〜30 の幅で、10〜20 の所は Y 5 から上が空く
        let u = vec![vec![
            [0.0, 0.0],
            [30.0, 0.0],
            [30.0, 20.0],
            [20.0, 20.0],
            [20.0, 5.0],
            [10.0, 5.0],
            [10.0, 20.0],
            [0.0, 20.0],
        ]];
        let rows = rows(&u, 0.0, 2.0, 1.0, false);
        let top = rows.last().unwrap();
        assert_eq!(top.spans.len(), 2, "{top:?}");
        assert!((top.spans[0].1 - 11.0).abs() < 1e-6 && (top.spans[1].0 - 19.0).abs() < 1e-6);
        let mut tp = empty();
        level_moves(&mut tp, &rows, 0.0, -1.0, FacePasses::Both, (800.0, 300.0));
        // 切り欠きの中(X 12〜18、Y 8 より上)を削らない
        let mut cur = [0.0; 3];
        for m in &tp.moves {
            let to = m.to();
            if matches!(m, Move::Linear { .. }) && to[2] < 0.0 && cur[2] < 0.0 {
                for k in 0..=20 {
                    let t = k as f64 / 20.0;
                    let q = [cur[0] + (to[0] - cur[0]) * t, cur[1] + (to[1] - cur[1]) * t];
                    assert!(
                        !(q[0] > 12.0 && q[0] < 18.0 && q[1] > 8.0),
                        "{cur:?} → {to:?}"
                    );
                }
            }
            cur = to;
        }
    }
}
