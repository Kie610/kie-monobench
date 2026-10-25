//! スケッチの要素を条件で探す(読むだけの操作)。種類・補助線か・位置が決まっているか・長さ・半径・
//! 範囲で絞り、解いた座標で表にする。画面のコマンド検索・CLI・MCP(`run_action`)が同じ宣言で使う
//! (AI が大きなスケッチの全部のデータを読まずに、直したい要素の番号を得る)。

use crate::{Entity, SKETCH, Sketch, Solution, selected_sketch};
use kmb_core::{Action, Arg, Context, Extension, FeatureId, Outcome, Table, TableColumn};
use serde_json::Value;

/// 操作の ID。
pub const FIND: &str = "cad.sketch.find";

/// 種類の選び(引数の値, 表に出す名前)。
const KINDS: &[(&str, &str)] = &[
    ("point", "点"),
    ("line", "線分"),
    ("circle", "円"),
    ("arc", "円弧"),
    ("ellipse", "楕円"),
    ("spline", "スプライン"),
    ("conic", "円錐曲線"),
];

/// スケッチの要素を探す操作の名乗り。
#[derive(Default)]
pub struct SketchFind;

impl Extension for SketchFind {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let none = selected_sketch(cx).is_none() && cx.arg("sketch").is_none();
        vec![
            Action::new(FIND, "スケッチの要素を探す", "cad")
                .args([
                    Arg::feature("sketch", "探すスケッチ(機能の番号。省くと選んでいるスケッチ)"),
                    Arg::choice("kind", "要素の種類(省くと全部)", KINDS),
                    Arg::flag("construction", "真なら補助線だけ、偽なら補助線でない物だけ(省くと両方)"),
                    Arg::choice(
                        "fixed",
                        "位置が拘束で決まっているか(省くと両方)",
                        &[("yes", "決まっている"), ("no", "まだ動く")],
                    ),
                    Arg::length("min_length", "線分の長さの下限(mm)"),
                    Arg::length("max_length", "線分の長さの上限(mm)"),
                    Arg::length("min_radius", "円・円弧の半径の下限(mm)"),
                    Arg::length("max_radius", "円・円弧の半径の上限(mm)"),
                    Arg::list(
                        "box",
                        "範囲 [x の下, y の下, x の上, y の上](mm、スケッチの座標)。使う点が全部この中にある要素だけ",
                    ),
                ])
                .aliases("すけっち ようそ さがす けんさく find query entities")
                .hint("選んでいるスケッチの点・線・円などを、種類・補助線・拘束の決まり具合・長さ・半径・範囲で絞って表にする")
                .disabled_if(none.then_some("スケッチを選んでから使う")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        (id == FIND).then(|| find(cx))
    }
}

/// 探す条件。
#[derive(Default)]
struct Filter<'a> {
    kind: Option<&'a str>,
    construction: Option<bool>,
    fixed: Option<bool>,
    length: (Option<f64>, Option<f64>),
    radius: (Option<f64>, Option<f64>),
    area: Option<[f64; 4]>,
}

fn filter<'a>(cx: &Context<'a>) -> Result<Filter<'a>, String> {
    let area = match cx.arg("box") {
        None => None,
        Some(v) => {
            let n: Vec<f64> = v
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default();
            match n[..] {
                [x0, y0, x1, y1] if x0 <= x1 && y0 <= y1 => Some([x0, y0, x1, y1]),
                _ => {
                    return Err(
                        "box は [x の下, y の下, x の上, y の上] の 4 つの数(下 ≦ 上)で書く".into(),
                    );
                }
            }
        }
    };
    Ok(Filter {
        kind: cx.arg_str("kind"),
        construction: cx.arg("construction").and_then(Value::as_bool),
        fixed: cx.arg_str("fixed").map(|f| f == "yes"),
        length: (cx.arg_f64("min_length")?, cx.arg_f64("max_length")?),
        radius: (cx.arg_f64("min_radius")?, cx.arg_f64("max_radius")?),
        area,
    })
}

/// 要素の種類の引数の値。
fn kind_of(e: &Entity) -> &'static str {
    match e {
        Entity::Point { .. } => "point",
        Entity::Line { .. } => "line",
        Entity::Circle { .. } => "circle",
        Entity::Arc { .. } => "arc",
        Entity::Ellipse { .. } => "ellipse",
        Entity::Spline { .. } => "spline",
        Entity::Conic { .. } => "conic",
    }
}

fn is_construction(e: &Entity) -> bool {
    match e {
        Entity::Point { .. } => false,
        Entity::Line { construction, .. }
        | Entity::Circle { construction, .. }
        | Entity::Arc { construction, .. }
        | Entity::Ellipse { construction, .. }
        | Entity::Spline { construction, .. }
        | Entity::Conic { construction, .. } => *construction,
    }
}

/// 解いた形の、線分の長さと円・円弧の半径。
fn measures(e: &Entity, sol: &Solution) -> (Option<f64>, Option<f64>) {
    let p = |id| sol.points.get(&id).copied();
    let dist = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    match e {
        Entity::Line { a, b, .. } => (p(*a).zip(p(*b)).map(|(a, b)| dist(a, b)), None),
        Entity::Circle { id, .. } => (None, sol.radii.get(id).copied()),
        Entity::Arc { center, start, .. } => {
            (None, p(*center).zip(p(*start)).map(|(c, s)| dist(c, s)))
        }
        _ => (None, None),
    }
}

fn within(v: Option<f64>, (lo, hi): (Option<f64>, Option<f64>)) -> bool {
    if lo.is_none() && hi.is_none() {
        return true;
    }
    v.is_some_and(|v| lo.is_none_or(|lo| v >= lo - 1e-9) && hi.is_none_or(|hi| v <= hi + 1e-9))
}

/// 要素の点(点はそれ自身)の解いた座標。
fn points(e: &Entity, sol: &Solution) -> Vec<[f64; 2]> {
    let ids = match e {
        Entity::Point { id, .. } => vec![*id],
        _ => e.points(),
    };
    ids.iter()
        .filter_map(|i| sol.points.get(i).copied())
        .collect()
}

fn find(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, sketch) = match cx.arg("sketch").and_then(Value::as_u64) {
        Some(n) => {
            let f = cx
                .data
                .feature(FeatureId(n))
                .filter(|f| f.type_id == SKETCH)
                .ok_or(format!("機能 {n} はスケッチではありません"))?;
            let s: Sketch = serde_json::from_value(f.data.clone()).map_err(|e| e.to_string())?;
            (FeatureId(n), s)
        }
        None => selected_sketch(cx).ok_or("スケッチを選んでから使う")?,
    };
    let sol = cx
        .output(id, "sketch")
        .and_then(crate::solution_of)
        .ok_or("スケッチが解けていないので探せません(先に誤りを直す)")?;
    let f = filter(cx)?;
    let mm = |v: f64| format!("{:.3}", v);
    let rows: Vec<Vec<String>> = sketch
        .entities
        .iter()
        .filter(|e| f.kind.is_none_or(|k| k == kind_of(e)))
        .filter(|e| f.construction.is_none_or(|c| c == is_construction(e)))
        .filter(|e| f.fixed.is_none_or(|x| x == sol.fixed.contains(&e.id())))
        .filter_map(|e| {
            let (length, radius) = measures(e, &sol);
            let pts = points(e, &sol);
            let inside = f.area.is_none_or(|[x0, y0, x1, y1]| {
                !pts.is_empty()
                    && pts
                        .iter()
                        .all(|p| (x0..=x1).contains(&p[0]) && (y0..=y1).contains(&p[1]))
            });
            (within(length, f.length) && within(radius, f.radius) && inside).then(|| {
                let kind = KINDS
                    .iter()
                    .find(|(k, _)| *k == kind_of(e))
                    .map_or("", |(_, l)| l);
                vec![
                    e.id().0.to_string(),
                    kind.to_string(),
                    if is_construction(e) { "補助" } else { "" }.to_string(),
                    if sol.fixed.contains(&e.id()) {
                        "決まっている"
                    } else {
                        "動く"
                    }
                    .to_string(),
                    length.map(mm).unwrap_or_default(),
                    radius.map(mm).unwrap_or_default(),
                    pts.iter()
                        .map(|p| format!("({}, {})", mm(p[0]), mm(p[1])))
                        .collect::<Vec<_>>()
                        .join(" "),
                ]
            })
        })
        .collect();
    let col = |id: &str, label: &str, numeric: bool| TableColumn {
        id: id.into(),
        label: label.into(),
        numeric,
        shown: true,
    };
    Ok(Outcome::Table {
        title: format!("スケッチの要素({} 個)", rows.len()),
        table: Table {
            key: FIND.into(),
            columns: vec![
                col("id", "番号", true),
                col("kind", "種類", false),
                col("construction", "補助線", false),
                col("fixed", "位置", false),
                col("length", "長さ mm", true),
                col("radius", "半径 mm", true),
                col("points", "点 mm", false),
            ],
            rows,
            empty: "条件に合う要素はありません。".into(),
            ..Table::default()
        },
    })
}
