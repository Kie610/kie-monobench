//! 彫刻(`cam.engrave@1`)。スケッチなどの線(開いた線も閉じた線も)の上を、決めた深さでなぞる。
//!
//! 線は出力 `curves`([`jc_core::Curves`])を持つ機能から読む。線の平面は、機械に置いた向きで
//! 水平でなければならない(部品の上面に描いたスケッチなど)。深さは線の平面から測る。
//! つながった線は持ち上げずに続けて削り、離れた線は近いものから順に回る。
//! 左右の補正を選ぶと、工具の中心を線から刃の半径(彫る深さでの)だけ横へずらし、
//! 溝の縁が線に来るようにする。閉じた線は外側か内側、開いた線は描いた向きの左か右。
//! 彫った溝は形にないので、シミュレーションは深さまでの食い込みを許す。
//!
//! 同じ crate に、決めた幅の溝を削るスロット(`cam.slot@1`、[`Slot`])、モデルの辺と線の高さに沿う
//! トレース(`cam.trace@1`、[`Trace`])、閉じた領域を V ビットで彫る V 彫り(`cam.vcarve@1`、[`VCarve`])がある。

use jc_cam_geom::{Loop, P2, offset_loop_outward, offset_open};
use jc_cam_setup::{
    Chain, OpBase, Planner, StrategyInput, StrategyOutput, approach, chains, expr_refs, invalid,
    length, output, plan_error, prepare, read_lines, retract,
};
use jc_core::{EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Registry};
use jc_toolpath::Move;
use serde::{Deserialize, Serialize};

mod slot;
mod trace;
mod vcarve;
pub use slot::{SLOT, Slot};
pub use trace::{TRACE, Trace};
pub use vcarve::{VCARVE, VCarve};

pub const ENGRAVE: &str = "cam.engrave@1";

pub(crate) fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// 工具の中心を線のどちら側に置くか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceSide {
    /// 線の上(工具の中心が線をなぞる)
    #[default]
    Center,
    /// 閉じた線は外側、開いた線は描いた向きの左
    Left,
    /// 閉じた線は内側、開いた線は描いた向きの右
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Engrave {
    #[serde(flatten)]
    pub base: OpBase,
    /// なぞる線を出す機能(スケッチなど)
    pub curves: FeatureId,
    /// 彫る深さ(線の平面から)
    #[serde(default = "depth")]
    pub depth: String,
    /// 左右の補正(既定は線の上)
    #[serde(default, skip_serializing_if = "is_default")]
    pub side: TraceSide,
}

impl Engrave {
    /// 線の上を深さ 0.3 mm でなぞる彫刻(残りの項目を `..Engrave::new(base, curves)` で埋めるのに使う)。
    pub fn new(base: OpBase, curves: FeatureId) -> Engrave {
        Engrave {
            base,
            curves,
            depth: depth(),
            side: TraceSide::Center,
        }
    }
}

fn depth() -> String {
    "0.3 mm".into()
}

/// 円弧を折れ線にする時の誤差(mm)。
const CHORD: f64 = 0.005;

/// 線 1 本を `side` の側へ `d` だけずらした線(閉じた線は閉じたまま、始めと終わりが同じ点)。
fn shifted(c: &Chain, side: TraceSide, d: f64) -> Vec<Chain> {
    if side == TraceSide::Center || d < 1e-9 {
        return vec![c.clone()];
    }
    let left = side == TraceSide::Left;
    if c.closed {
        let l = Loop::from_points(&c.pts[..c.pts.len() - 1]);
        // 左は外側、右は内側
        return offset_loop_outward(&l, if left { d } else { -d })
            .into_iter()
            .map(|l| {
                let mut pts = l.flatten(CHORD);
                pts.push(pts[0]);
                Chain {
                    pts,
                    closed: true,
                    reversed: false,
                }
            })
            .collect();
    }
    // 描いた向きの左。たどる向きが逆なら、たどる向きの右
    let sign = if left != c.reversed { 1.0 } else { -1.0 };
    offset_open(&c.pts, sign * d, CHORD)
        .into_iter()
        .map(|pts| Chain {
            pts,
            closed: false,
            reversed: c.reversed,
        })
        .collect()
}

/// ホストが解いた値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngravePlan {
    pub depth: f64,
    pub lines: Vec<Vec<P2>>,
    pub top: f64,
    pub side: TraceSide,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let p = Planner::new(input);
    let EngravePlan {
        depth,
        lines,
        top,
        side,
    } = input.params().map_err(invalid)?;
    let bottom = top - depth;
    p.check_depth(top, bottom)?;
    let mut tp = p.toolpath();
    tp.edge_break = depth + 0.05;
    // 溝の縁が線に来るよう、線の平面での刃の半径だけずらす
    let d = p.tool.radius_at(depth).unwrap_or(p.r());
    let all: Vec<Chain> = chains(lines)
        .iter()
        .flat_map(|c| shifted(c, side, d))
        .collect();
    if all.is_empty() {
        return Err(invalid(
            "左右にずらすと線がなくなりました(小さな閉じた線を内側へずらす時は、細い工具にしてください)",
        ));
    }
    for Chain {
        pts: chain, closed, ..
    } in all
    {
        approach(&mut tp, chain[0]);
        let mut forward = true;
        for z in p.levels(top, bottom) {
            // 開いた線は行って戻る(持ち上げずに次の深さへ)
            let pts: Vec<P2> = if forward {
                chain.clone()
            } else {
                chain.iter().rev().copied().collect()
            };
            tp.moves.push(Move::Linear {
                to: [pts[0][0], pts[0][1], z],
                feed: p.cutting.plunge,
            });
            tp.moves.extend(pts.iter().skip(1).map(|q| Move::Linear {
                to: [q[0], q[1], z],
                feed: p.cutting.feed,
            }));
            if !closed {
                forward = !forward;
            }
        }
        retract(&mut tp);
    }
    Ok(p.output(tp))
}

impl Feature for Engrave {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.push(output(self.curves, "curves"));
        v.extend(expr_refs([&self.depth])?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        let depth = length(ctx, &self.depth, "彫る深さ")?;
        if depth <= 0.0 {
            return Err(invalid("彫る深さは 0 より大きくしてください"));
        }
        let (lines, top) = read_lines(ctx, &p, self.curves)?;
        let params = EngravePlan {
            depth,
            lines,
            top,
            side: self.side,
        };
        let input = p.input(ctx.feature_id(), params, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        Ok(c.base
            .rename([&mut c.depth], old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    vcarve::register(reg);
    slot::register(reg);
    trace::register(reg);
    reg.register_described(
        jc_cam_setup::operation(
            ENGRAVE,
            "彫刻",
            "スケッチの線(補助線を除く)の上を、決めた深さでなぞる。文字や目盛り、V ビットでの線彫りに",
            vec![
                Field::output("curves", "なぞる線", "curves"),
                Field::expr("depth", "彫る深さ").length().help("線の平面から"),
                Field::choice(
                    "side",
                    "左右の補正",
                    &[
                        ("center", "線の上"),
                        ("left", "外側・左"),
                        ("right", "内側・右"),
                    ],
                )
                .help(
                    "工具の中心を線から刃の半径(彫る深さでの)だけずらし、溝の縁を線に合わせる。\
                     閉じた線は外側か内側、開いた線は描いた向き(始点から終点)の左か右",
                ),
            ],
        )
        .icon("engrave"),
        |d| Ok(Box::new(jc_cam_setup::parse::<Engrave>(d)?)),
    );
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> jc_cam_setup::StrategyModule {
    use jc_cam_setup::{Needs, Strategy, StrategyModule, to_data};
    use jc_cam_tool::ToolKind;
    StrategyModule::new(
        "jc-strategy-engrave",
        register,
        vec![
            Strategy::new(ENGRAVE, "cam.add.engrave", "彫刻", |p| {
                to_data(Engrave {
                    base: p.with_tool(ToolKind::VBit),
                    curves: p.source("curves").ok_or("なぞる線がありません")?,
                    depth: "0.3 mm".into(),
                    side: Default::default(),
                })
            })
            .order(8)
            .icon("engrave")
            .not_front()
            .aliases("ちょうこく engrave もじ text せんぼり")
            .hint("スケッチの線を決めた深さでなぞる。スケッチを選ぶとその線、選ばなければ最後のスケッチ")
            .needs(Needs::Curves),
            Strategy::new(VCARVE, "cam.add.vcarve", "V 彫り", |p| {
                to_data(VCarve {
                    base: p.with_tool(ToolKind::VBit),
                    profile: p.source("profile").ok_or("輪郭がありません")?,
                    max_depth: "2 mm".into(),
                    spacing: "0.1 mm".into(),
                })
            })
            .order(9)
            .icon("vcarve")
            .not_front()
            .aliases("ぶいぼり vcarve v-carve かんばん sign もじ")
            .hint("スケッチの閉じた輪郭の内側を V ビットで彫る。細い所は浅く尖らせ、太い所は最大の深さまで")
            .needs(Needs::Profile),
            Strategy::new(SLOT, "cam.add.slot", "スロット", |p| {
                to_data(Slot::new(
                    p.with_tool(ToolKind::FlatEnd),
                    p.source("curves").ok_or("溝の中心の線がありません")?,
                ))
            })
            .order(4)
            .icon("engrave")
            .not_front()
            .aliases("すろっと slot みぞ groove きーみぞ")
            .hint("スケッチの線を溝の中心にして、決めた幅と深さの溝を削る。スケッチを選ぶとその線、選ばなければ最後のスケッチ")
            .needs(Needs::Curves)
            .no_reuse(),
            Strategy::new(TRACE, "cam.add.trace", "トレース", |p| {
                // 3D のビューで辺を選んでいれば、その辺を V ビットでなぞる(縁の面取り)。
                // なければスケッチの線(選んだもの、なければ最後のもの)を細い平でなぞる
                let edges = p.edge_refs();
                let (base, curves) = if edges.is_empty() {
                    let base = jc_cam_setup::OpBase {
                        tool: p.smallest_flat().unwrap_or(1),
                        ..p.base()
                    };
                    (base, Some(p.source("curves").ok_or("なぞる辺か線がありません(辺を選ぶか、スケッチを描いてから使う)")?))
                } else {
                    (p.with_tool(ToolKind::VBit), None)
                };
                // 曲がった辺や線は細かい直線にしてなぞるので、円弧のあてはめを初めから入れる
                // (円の縁が G2・G3 になる。3D の経路と同じ)
                to_data(Trace {
                    edges,
                    curves,
                    ..Trace::new(jc_cam_setup::fitted(base))
                })
            })
            .order(5)
            .icon("trace")
            .not_front()
            .aliases("とれーす trace なぞる へん edge ばりとり deburr めんとり")
            .hint("選んだモデルの辺かスケッチの線を、線の高さに沿ってなぞる。辺を選ぶとその辺を V ビットで、選ばなければスケッチの線")
            .no_reuse(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_lines_are_cut_in_one_go() {
        let c = chains(vec![
            vec![[10.0, 0.0], [10.0, 10.0]],
            vec![[0.0, 0.0], [10.0, 0.0]],
            vec![[50.0, 50.0], [60.0, 50.0]],
            vec![[10.0, 10.0], [0.0, 0.0]],
        ]);
        assert_eq!(c.len(), 2);
        assert!(c[0].closed, "三角形は閉じている");
        assert_eq!(c[0].pts.len(), 4);
        assert!(!c[1].closed);
    }

    #[test]
    fn an_open_line_is_shifted_to_the_left_of_how_it_was_drawn() {
        // 右へ描いた線(+X)。左は +Y。逆向きにたどっても、左右は描いた向きで決まる
        let c = chains(vec![vec![[20.0, 0.0], [30.0, 0.0]]]);
        assert!(!c[0].reversed);
        let mut back = c[0].clone();
        back.pts.reverse();
        back.reversed = true;
        for ch in [&c[0], &back] {
            let l = shifted(ch, TraceSide::Left, 1.5);
            assert!(l[0].pts.iter().all(|q| (q[1] - 1.5).abs() < 1e-9), "{l:?}");
            let r = shifted(ch, TraceSide::Right, 1.5);
            assert!(r[0].pts.iter().all(|q| (q[1] + 1.5).abs() < 1e-9));
        }
        // 終わりの端が近いと、逆向きにたどる
        let c = chains(vec![vec![[30.0, 0.0], [5.0, 0.0]]]);
        assert!(c[0].reversed && c[0].pts[0] == [5.0, 0.0]);
        let l = shifted(&c[0], TraceSide::Left, 1.0);
        // 描いた向きは -X なので、左は -Y
        assert!(l[0].pts.iter().all(|q| (q[1] + 1.0).abs() < 1e-9), "{l:?}");
    }

    #[test]
    fn a_closed_line_is_shifted_outward_on_the_left_and_inward_on_the_right() {
        let sq = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ];
        let c = &chains(vec![sq])[0];
        let span = |c: &Chain| {
            let xs: Vec<f64> = c.pts.iter().map(|q| q[0]).collect();
            (
                xs.iter().copied().fold(f64::MAX, f64::min),
                xs.iter().copied().fold(f64::MIN, f64::max),
            )
        };
        let out = shifted(c, TraceSide::Left, 1.0);
        let (a, b) = span(&out[0]);
        assert!((a + 1.0).abs() < 1e-6 && (b - 11.0).abs() < 1e-6);
        assert!(out[0].closed && out[0].pts[0] == out[0].pts[out[0].pts.len() - 1]);
        let inn = shifted(c, TraceSide::Right, 1.0);
        let (a, b) = span(&inn[0]);
        assert!((a - 1.0).abs() < 1e-6 && (b - 9.0).abs() < 1e-6);
    }
}
