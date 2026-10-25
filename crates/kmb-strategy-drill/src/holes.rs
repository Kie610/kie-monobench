//! 穴の機能と加工の結び付け。
//!
//! 穴の機能は、出力 `hole` に穴の情報([`HoleInfo`]: 種類・ネジ・径・数・ザグリ・皿)を入れる。
//! 形と面の名前の決まりは受け渡しの部品(kmb-interchange)にあり、加工はその名前で、部品の縦の穴
//! ([`Bore`])のうちどれがその穴の機能のものかを見分ける([`hole_face`])。
//! CAM は CAD の crate に依存しない決まりなので、穴の機能のデータは読まない。
//!
//! 工具は、径の合う工具を候補として並べ、既定は単純な規則で 1 本入れるところまでにする
//! (工具の最適な組み合わせを自動で選ぶ働きは作らない。docs/避ける形.md)。

use kmb_cam_geom::Bore;
use kmb_cam_tool::{Tool, ToolKind, ToolLibrary};
use kmb_core::FeatureId;
use kmb_interchange::hole::hole_face;
use kmb_interchange::{HoleInfo, HoleKind, HolePart, Tap};

/// 穴 `b` が穴の機能 `hole` の部分 `part` なら、その点の番号。皿は穴そのものの壁で見分ける。
pub fn bore_of(b: &Bore, hole: FeatureId, part: HolePart) -> Option<u32> {
    let want = match part {
        HolePart::Counterbore => HolePart::Counterbore,
        HolePart::Wall | HolePart::Countersink => HolePart::Wall,
    };
    b.faces
        .iter()
        .filter_map(|f| hole_face(f, hole))
        .find(|(_, p)| *p == want)
        .map(|(n, _)| n)
}

/// 工具で穴(径 `diameter`)を削る方法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// まっすぐ下げる(ドリルか、同じ径の平)
    Plunge,
    /// らせんで広げる(穴より細い平)
    Helical,
    /// もみつけ・面取り(V ビット)
    Spot,
}

/// 径が合うとみなす差(mm)。
pub const SAME: f64 = 0.05;

/// 工具 `t` で、部分 `part`(径 `diameter`)を削る方法。削れなければ `None`。
pub fn how(t: &Tool, diameter: f64, part: HolePart, spot: bool) -> Option<How> {
    let same = (t.diameter - diameter).abs() <= SAME;
    match (t.kind, part) {
        (ToolKind::VBit, HolePart::Countersink) => Some(How::Spot),
        (ToolKind::VBit, HolePart::Wall) if spot => Some(How::Spot),
        (_, HolePart::Countersink) | (ToolKind::VBit, _) => None,
        (_, _) if spot => None,
        (ToolKind::Drill, _) if same => Some(How::Plunge),
        (ToolKind::FlatEnd, _) if diameter / 2.0 > t.radius() + SAME => Some(How::Helical),
        (ToolKind::FlatEnd, _) if same => Some(How::Plunge),
        _ => None,
    }
}

/// 部分を削れる工具の候補(ライブラリの順)。
pub fn candidates(lib: &ToolLibrary, diameter: f64, part: HolePart, spot: bool) -> Vec<&Tool> {
    lib.tools
        .iter()
        .filter(|t| how(t, diameter, part, spot).is_some())
        .collect()
}

/// 既定の工具。単純な規則で 1 本だけ決める(利用者が替える前提):
/// - もみつけ・皿: 刃先角が `angle` の V ビット、なければ最初の V ビット
/// - 穴・ザグリ: 同じ径のドリル、なければ穴より細い平のうち最も太いもの(らせん)、
///   なければ同じ径の平
pub fn default_tool(
    lib: &ToolLibrary,
    diameter: f64,
    part: HolePart,
    spot: bool,
    angle: Option<f64>,
) -> Option<u32> {
    let c = candidates(lib, diameter, part, spot);
    if spot || part == HolePart::Countersink {
        return c
            .iter()
            .find(|t| angle.is_some_and(|a| (t.tip_angle - a).abs() < 0.5))
            .or(c.first())
            .map(|t| t.number);
    }
    let pick = |h: How, kind: ToolKind| {
        c.iter()
            .filter(|t| t.kind == kind && how(t, diameter, part, spot) == Some(h))
            .max_by(|a, b| a.diameter.total_cmp(&b.diameter))
            .map(|t| t.number)
    };
    pick(How::Plunge, ToolKind::Drill)
        .or_else(|| pick(How::Helical, ToolKind::FlatEnd))
        .or_else(|| pick(How::Plunge, ToolKind::FlatEnd))
}

/// ライブラリに合う工具がない時に要る工具。
#[derive(Debug, Clone, PartialEq)]
pub struct Need {
    pub kind: ToolKind,
    /// ドリルは穴の径。V ビットは刃先角(度)を `angle` に持ち、径は 0
    pub diameter: f64,
    pub angle: f64,
}

impl Need {
    pub fn text(&self) -> String {
        match self.kind {
            ToolKind::VBit => format!("V ビット({:.0}°)", self.angle),
            _ => format!(
                "ドリル {} mm(または {} mm より細い平エンドミル)",
                num(self.diameter),
                num(self.diameter - 2.0 * SAME)
            ),
        }
    }
}

/// 足す加工の操作の 1 つ(穴の機能のまとまりごと)。
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedOp {
    /// 操作の名前の後ろ半分(例: 「もみつけ」)。穴を直すと古くなる寸法は入れない
    pub step: &'static str,
    pub part: HolePart,
    /// V ビットのもみつけ
    pub spot: bool,
    /// 削る部分の径
    pub diameter: f64,
    /// 既定の工具。合う工具がなければ `None`
    pub tool: Option<u32>,
    pub need: Option<Need>,
}

/// 穴の機能 1 つの加工の案。
#[derive(Debug, Clone, PartialEq)]
pub struct HolePlan {
    /// まとまりの名前(例: 「M3 ネジ穴」)
    pub label: String,
    pub ops: Vec<PlannedOp>,
}

/// 数を短く書く(`2.50` → `2.5`)。
pub fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 穴の属性から、削る操作の並びを決める。ネジ山は作らない(タップは使わない前提)。
/// - ネジ穴: もみつけ(V)、下穴
/// - 通し穴・径を決めた穴: 穴(ドリルで削るなら、前にもみつけ)
/// - ザグリ: 先にザグリ、次に穴
/// - 皿: 穴の後に、V で皿の径まで面取り
pub fn plan(hole: &HoleInfo, lib: &ToolLibrary) -> HolePlan {
    let s = hole;
    let size = hole
        .size
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("M3");
    let d = num(s.diameter);
    let mut label = match hole.tap {
        Tap::Tapped => format!("{size} ネジ穴"),
        Tap::Clearance => format!("{size} 通し穴 Ø{d}"),
        Tap::Simple => format!("Ø{d} の穴"),
    };
    match hole.kind {
        HoleKind::Simple => {}
        HoleKind::Counterbore => label.push_str("(ザグリ)"),
        HoleKind::Countersink => label.push_str("(皿)"),
    }
    let op = |step: &'static str, part: HolePart, spot: bool, diameter: f64, angle: Option<f64>| {
        let tool = default_tool(lib, diameter, part, spot, angle);
        let need = tool.is_none().then(|| {
            if spot || part == HolePart::Countersink {
                Need {
                    kind: ToolKind::VBit,
                    diameter: 0.0,
                    angle: angle.unwrap_or(90.0),
                }
            } else {
                Need {
                    kind: ToolKind::Drill,
                    diameter,
                    angle: 118.0,
                }
            }
        });
        PlannedOp {
            step,
            part,
            spot,
            diameter,
            tool,
            need,
        }
    };
    let mut ops = Vec::new();
    if let (HoleKind::Counterbore, Some((cd, _))) = (hole.kind, s.counterbore) {
        ops.push(op("ザグリ", HolePart::Counterbore, false, cd, None));
    }
    let wall_name = if hole.tap == Tap::Tapped {
        "下穴"
    } else {
        "穴"
    };
    let wall = op(wall_name, HolePart::Wall, false, s.diameter, None);
    let drilled = wall
        .tool
        .and_then(|n| lib.get(n))
        .is_some_and(|t| t.kind == ToolKind::Drill);
    // ドリルは位置がずれやすいので、前にもみつけをする(ネジ穴は工具が無くても案に出す)
    if hole.tap == Tap::Tapped || drilled {
        ops.push(op("もみつけ", HolePart::Wall, true, s.diameter, None));
    }
    ops.push(wall);
    if let (HoleKind::Countersink, Some((sd, angle))) = (hole.kind, s.countersink) {
        ops.push(op(
            "皿の面取り",
            HolePart::Countersink,
            false,
            sd,
            Some(angle.to_degrees()),
        ));
    }
    HolePlan { label, ops }
}

/// 要る工具を、ライブラリに足せる形にする。番号は空いている最初の番号から。
/// 切削条件は、径がいちばん近い平の工具の条件を写し、ドリルは 1 回に下げる深さを径と同じにする
/// (案なので、利用者が工具ライブラリで直せる)。
pub fn tools_for(lib: &ToolLibrary, needs: &[Need]) -> Vec<Tool> {
    let mut out: Vec<Tool> = Vec::new();
    let mut next = 1;
    let free = |n: u32, out: &Vec<Tool>| lib.get(n).is_none() && !out.iter().any(|t| t.number == n);
    for need in needs {
        let dup = out.iter().any(|t| {
            t.kind == need.kind
                && match need.kind {
                    ToolKind::VBit => (t.tip_angle - need.angle).abs() < 0.5,
                    _ => (t.diameter - need.diameter).abs() <= 1e-6,
                }
        });
        if dup {
            continue;
        }
        while !free(next, &out) {
            next += 1;
        }
        let near = |d: f64| {
            lib.tools
                .iter()
                .filter(|t| t.kind == ToolKind::FlatEnd)
                .min_by(|a, b| (a.diameter - d).abs().total_cmp(&(b.diameter - d).abs()))
                .or(lib.tools.first())
                .cloned()
        };
        let Some(mut t) = near(if need.kind == ToolKind::VBit {
            3.175
        } else {
            need.diameter
        }) else {
            continue;
        };
        t.number = next;
        t.kind = need.kind;
        t.tip_diameter = None;
        t.extra = Default::default();
        match need.kind {
            ToolKind::VBit => {
                t.diameter = 3.175;
                t.shank_diameter = Some(3.175);
                t.flutes = 1;
                t.tip_angle = need.angle;
                t.flute_length = 4.0;
                t.name = format!("V {:.0}° 3.175 mm", need.angle);
            }
            _ => {
                let d = need.diameter;
                t.diameter = d;
                t.shank_diameter = Some(d);
                t.flutes = 2;
                t.tip_angle = 118.0;
                t.flute_length = (d * 10.0).clamp(12.0, 40.0);
                t.name = format!("ドリル {} mm", num(d));
                for p in &mut t.presets {
                    p.cutting.stepdown = d;
                }
            }
        }
        out.push(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> ToolLibrary {
        ToolLibrary::starter()
    }

    fn drill(n: u32, d: f64) -> Tool {
        let mut t = lib().tools[0].clone();
        t.number = n;
        t.kind = ToolKind::Drill;
        t.diameter = d;
        t.name = format!("ドリル {d}");
        t
    }

    #[test]
    fn the_default_tool_is_a_drill_of_the_same_size_or_the_thickest_flat_that_fits() {
        let mut l = lib();
        // 平 6・3.175・2、ボール、V 90°
        assert_eq!(default_tool(&l, 2.5, HolePart::Wall, false, None), Some(3));
        assert_eq!(default_tool(&l, 6.6, HolePart::Wall, false, None), Some(1));
        // 6 mm ちょうどは、細い平のらせん(同じ径の平で突くより先)
        assert_eq!(default_tool(&l, 6.0, HolePart::Wall, false, None), Some(2));
        assert_eq!(default_tool(&l, 1.5, HolePart::Wall, false, None), None);
        assert_eq!(default_tool(&l, 2.5, HolePart::Wall, true, None), Some(5));
        l.tools.push(drill(9, 2.5));
        assert_eq!(default_tool(&l, 2.5, HolePart::Wall, false, None), Some(9));
        let c: Vec<u32> = candidates(&l, 2.5, HolePart::Wall, false)
            .iter()
            .map(|t| t.number)
            .collect();
        assert_eq!(c, vec![3, 9]);
    }

    /// 穴の機能が出す穴の情報(種類・ネジは `json`、径は `d`)。
    fn hole(json: serde_json::Value, d: f64) -> HoleInfo {
        let mut v = serde_json::json!({
            "diameter": d,
            "count": 4,
            "counterbore": [6.5, 3.3],
            "countersink": [6.3, std::f64::consts::FRAC_PI_2],
        });
        for (k, x) in json.as_object().unwrap() {
            v[k] = x.clone();
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_tapped_hole_is_spotted_and_then_drilled_to_the_tap_drill_size() {
        let mut l = lib();
        let h = hole(serde_json::json!({"tap": "tapped", "size": "M3"}), 2.5);
        let p = plan(&h, &l);
        assert_eq!(p.label, "M3 ネジ穴");
        let steps: Vec<(&str, bool, Option<u32>)> =
            p.ops.iter().map(|o| (o.step, o.spot, o.tool)).collect();
        assert_eq!(
            steps,
            vec![("もみつけ", true, Some(5)), ("下穴", false, Some(3))]
        );
        // V ビットが無ければ、要る工具として知らせる
        l.tools.retain(|t| t.kind != ToolKind::VBit);
        let p = plan(&h, &l);
        assert_eq!(p.ops[0].tool, None);
        assert_eq!(p.ops[0].need.as_ref().unwrap().text(), "V ビット(90°)");
    }

    #[test]
    fn counterbores_come_first_and_countersinks_last() {
        let l = lib();
        let cb = hole(
            serde_json::json!({"kind": "counterbore", "tap": "clearance"}),
            3.4,
        );
        let p = plan(&cb, &l);
        assert_eq!(p.label, "M3 通し穴 Ø3.4(ザグリ)");
        let parts: Vec<HolePart> = p.ops.iter().map(|o| o.part).collect();
        assert_eq!(parts, vec![HolePart::Counterbore, HolePart::Wall]);
        let cs = hole(serde_json::json!({"kind": "countersink"}), 4.0);
        let p = plan(&cs, &l);
        let parts: Vec<HolePart> = p.ops.iter().map(|o| o.part).collect();
        assert_eq!(parts, vec![HolePart::Wall, HolePart::Countersink]);
        assert_eq!(p.ops[1].tool, Some(5));
    }

    #[test]
    fn missing_drills_are_offered_with_free_numbers() {
        let l = lib();
        let needs = vec![
            Need {
                kind: ToolKind::Drill,
                diameter: 1.5,
                angle: 118.0,
            },
            Need {
                kind: ToolKind::Drill,
                diameter: 1.5,
                angle: 118.0,
            },
        ];
        let t = tools_for(&l, &needs);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].number, 6);
        assert_eq!(t[0].kind, ToolKind::Drill);
        assert_eq!(t[0].name, "ドリル 1.5 mm");
        assert!(t[0].check().is_ok(), "{:?}", t[0].check());
        assert!(t[0].presets.iter().all(|p| p.cutting.stepdown == 1.5));
    }
}
