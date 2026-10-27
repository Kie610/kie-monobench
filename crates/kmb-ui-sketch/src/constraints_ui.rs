//! 選んだ要素に拘束と寸法を付けるパネル。選んだ要素の組み合わせで、使える拘束だけを出す。

use crate::annot::line_ends;
use crate::view::{self, DimValues, Editor, Tool};
use kmb_sketch::{ConstraintKind as K, Entity, EntityId, Sketch, Solution, SolveStatus};
use kmb_ui::theme;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Point,
    Line,
    Round,
    /// 楕円・スプライン・円錐曲線
    Curve,
}

pub(crate) fn kind(sketch: &Sketch, id: EntityId) -> Option<Kind> {
    Some(match sketch.entity(id)? {
        Entity::Point { .. } => Kind::Point,
        Entity::Line { .. } => Kind::Line,
        Entity::Circle { .. } | Entity::Arc { .. } => Kind::Round,
        Entity::Ellipse { .. } | Entity::Spline { .. } | Entity::Conic { .. } => Kind::Curve,
    })
}

/// 要素を形づくる点(線分の両端、円の中心、円弧の中心と両端)。
fn own_points(sketch: &Sketch, id: EntityId) -> Vec<EntityId> {
    sketch.entity(id).map(|e| e.points()).unwrap_or_default()
}

/// 2 本の線が端の点を分け合うか。
fn shares_end(sketch: &Sketch, a: EntityId, b: EntityId) -> bool {
    let ends = |id| sketch.entity(id).and_then(|e| e.ends());
    match (ends(a), ends(b)) {
        (Some((p, q)), Some((r, t))) => p == r || p == t || q == r || q == t,
        _ => false,
    }
}

fn fmt_mm(v: f64) -> String {
    format!("{} mm", trim(v))
}

fn trim(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 寸法の式。入力が空なら、いまの寸法を測った値を使う。
fn dim_expr(editor: &Editor, measured: String) -> String {
    let t = editor.dim_text.trim();
    if t.is_empty() {
        measured
    } else {
        // 数だけを打てば単位を補う(補わないと寸法が計算できず、スケッチと後ろの形が全部消える)。
        // 角度は deg、長さは文書の単位(測った値の mm を補うと、インチの文書で「2」が 2 mm になる)
        if measured.ends_with(" deg") {
            kmb_ui::with_unit_of(t, &measured)
        } else {
            view::typed_value(t)
        }
    }
}

/// 形の拘束(リボンの「拘束」)。名前、アイコン、選ぶもの。水平と垂直は 1 つの道具で、線分の
/// 向きに近い方を付ける(Kie 2026-10-21)。
pub(crate) const GEOMETRIC: [(&str, &str, &str); 15] = [
    (HV, "hv", "線分を 1 本か、点・円を 2 つ"),
    ("一致", "coinc", "点を 2 つ"),
    ("平行", "parallel", "線分を 2 本"),
    ("等しい", "equal", "線分だけか、円・円弧だけを 2 つ以上"),
    ("直交", "perp", "線分を 2 本"),
    ("固定", "fix", "点を 1 つ"),
    ("線上", "online", "点と線分"),
    ("中点", "midpoint", "点と線分"),
    ("円周上", "oncircle", "点と円"),
    ("接線", "tangent", "線分と円か、円を 2 つ"),
    ("同心円", "concentric", "円を 2 つ"),
    ("同一直線上", "collinear", "線分を 2 本"),
    (
        "対称",
        "symmetric",
        "点 2 つか線分 2 本か円 2 つと、対称の線にする線分",
    ),
    (
        "曲率",
        "curvature",
        "端でつながる曲線(楕円・スプライン・円錐曲線)と線",
    ),
    ("ポリゴン", "polycons", "線分だけで閉じた輪の 1 本"),
];

/// 水平と垂直を 1 つにした拘束の道具の名前。
pub(crate) const HV: &str = "水平/垂直";

/// 拘束のまとまりの、前に出す道具・2 段の小さなボタン・▾ の一覧だけの道具(Kie 2026-10-21 の画板)。
pub(crate) const CONSTRAINTS_FRONT: [&str; 4] = [HV, "一致", "平行", "等しい"];
pub(crate) const CONSTRAINTS_SMALL: [&str; 8] = [
    "接線",
    "直交",
    "固定",
    "中点",
    "同心円",
    "同一直線上",
    "対称",
    "曲率",
];
pub(crate) const CONSTRAINTS_MORE: [&str; 3] = ["線上", "円周上", "ポリゴン"];

/// 線分 `line` の向きに近い方(水平か垂直)の拘束の名前。横と縦が同じなら水平。
pub(crate) fn horizontal_or_vertical(
    sketch: &Sketch,
    sol: &Solution,
    line: EntityId,
) -> Option<&'static str> {
    let (p, q) = line_ends(sketch, sol, line)?;
    Some(if (q[0] - p[0]).abs() >= (q[1] - p[1]).abs() {
        "水平"
    } else {
        "垂直"
    })
}

/// 水平・垂直に並べる 2 点: 点か円・円弧の 2 つ(円・円弧は中心の点)。ほかの選びなら `None`。
pub(crate) fn point_pair(sketch: &Sketch, sel: &[EntityId]) -> Option<(EntityId, EntityId)> {
    let point = |id: EntityId| match sketch.entity(id)? {
        Entity::Point { .. } => Some(id),
        Entity::Circle { center, .. } | Entity::Arc { center, .. } => Some(*center),
        _ => None,
    };
    let [a, b] = sel else {
        return None;
    };
    let (a, b) = (point(*a)?, point(*b)?);
    (a != b).then_some((a, b))
}

/// 選んだ要素を水平(`horizontal` が偽なら垂直)にする拘束: 線分 1 本ならその線分、
/// 点・円を 2 つなら点(円は中心)どうし。
pub(crate) fn align(sketch: &Sketch, sel: &[EntityId], horizontal: bool) -> Option<K> {
    if let [line] = sel {
        matches!(sketch.entity(*line), Some(Entity::Line { .. })).then_some(())?;
        return Some(if horizontal {
            K::Horizontal { line: *line }
        } else {
            K::Vertical { line: *line }
        });
    }
    let (a, b) = point_pair(sketch, sel)?;
    Some(if horizontal {
        K::HorizontalPoints { a, b }
    } else {
        K::VerticalPoints { a, b }
    })
}

/// 2 点(円は中心)の並びに近い方(水平か垂直)の拘束の名前。横と縦が同じなら水平。
pub(crate) fn points_horizontal_or_vertical(
    sketch: &Sketch,
    sol: &Solution,
    sel: &[EntityId],
) -> Option<&'static str> {
    let (a, b) = point_pair(sketch, sel)?;
    let (p, q) = (sol.points.get(&a)?, sol.points.get(&b)?);
    Some(if (q[0] - p[0]).abs() >= (q[1] - p[1]).abs() {
        "水平"
    } else {
        "垂直"
    })
}

/// 選んだ要素(`sel`)の組み合わせに付けられる拘束。名前と拘束の組で返す。
/// `dims` が真なら寸法(値のある拘束)、偽なら形の拘束。
pub(crate) fn options(
    editor: &Editor,
    sketch: &Sketch,
    sol: &Solution,
    sel: &[EntityId],
    dims: bool,
) -> Vec<(&'static str, K)> {
    let kinds: Vec<Kind> = sel.iter().filter_map(|id| kind(sketch, *id)).collect();
    let mut v: Vec<(&'static str, K)> = Vec::new();
    match (kinds.as_slice(), sel) {
        ([Kind::Point], [p]) => v.push(("固定", K::Fix { point: *p })),
        ([Kind::Line], [l]) => {
            v.push(("水平", K::Horizontal { line: *l }));
            v.push(("垂直", K::Vertical { line: *l }));
            if let Some((p, q)) = line_ends(sketch, sol, *l) {
                let len = fmt_mm((q[0] - p[0]).hypot(q[1] - p[1]));
                v.push((
                    "長さ",
                    K::Length {
                        line: *l,
                        value: dim_expr(editor, len),
                    },
                ));
            }
        }
        ([Kind::Round], [c]) => {
            if let Some(r) = kmb_sketch::radius(sketch, sol, *c) {
                v.push((
                    "直径",
                    K::Diameter {
                        circle: *c,
                        value: dim_expr(editor, fmt_mm(2.0 * r)),
                    },
                ));
                v.push((
                    "半径",
                    K::Radius {
                        circle: *c,
                        value: dim_expr(editor, fmt_mm(r)),
                    },
                ));
            }
        }
        ([Kind::Point, Kind::Point], [a, b]) => {
            v.push(("一致", K::Coincident { a: *a, b: *b }));
            if let (Some(p), Some(q)) = (sol.points.get(a), sol.points.get(b)) {
                let d = fmt_mm((q[0] - p[0]).hypot(q[1] - p[1]));
                v.push((
                    "距離",
                    K::Distance {
                        a: *a,
                        b: *b,
                        value: dim_expr(editor, d),
                    },
                ));
                let dx = fmt_mm((q[0] - p[0]).abs());
                v.push((
                    "水平距離",
                    K::HorizontalDistance {
                        a: *a,
                        b: *b,
                        value: dim_expr(editor, dx),
                    },
                ));
                let dy = fmt_mm((q[1] - p[1]).abs());
                v.push((
                    "垂直距離",
                    K::VerticalDistance {
                        a: *a,
                        b: *b,
                        value: dim_expr(editor, dy),
                    },
                ));
            }
        }
        ([Kind::Line, Kind::Line], [a, b]) => {
            v.push(("平行", K::Parallel { a: *a, b: *b }));
            v.push(("直交", K::Perpendicular { a: *a, b: *b }));
            v.push(("等しい", K::Equal { a: *a, b: *b }));
            v.push(("同一直線上", K::Collinear { a: *a, b: *b }));
            if let (Some((p, q)), Some((r, s))) =
                (line_ends(sketch, sol, *a), line_ends(sketch, sol, *b))
            {
                let a1 = (q[1] - p[1]).atan2(q[0] - p[0]);
                let a2 = (s[1] - r[1]).atan2(s[0] - r[0]);
                let mut d = (a2 - a1).to_degrees();
                while d < 0.0 {
                    d += 360.0;
                }
                let value = dim_expr(editor, format!("{} deg", trim(d)));
                v.push((
                    "角度",
                    K::Angle {
                        a: *a,
                        b: *b,
                        value,
                    },
                ));
            }
        }
        ([Kind::Round, Kind::Round], [a, b]) => {
            v.push(("等しい", K::Equal { a: *a, b: *b }));
            v.push(("接線", K::TangentCircles { a: *a, b: *b }));
            v.push(("同心円", K::Concentric { a: *a, b: *b }));
        }
        (k, ids) if k.len() == 2 && k.contains(&Kind::Point) && k.contains(&Kind::Line) => {
            let (p, l) = if k[0] == Kind::Point {
                (ids[0], ids[1])
            } else {
                (ids[1], ids[0])
            };
            v.push(("線上", K::PointOnLine { point: p, line: l }));
            v.push(("中点", K::Midpoint { point: p, line: l }));
            if let (Some(x), Some((a, b))) = (sol.points.get(&p), line_ends(sketch, sol, l)) {
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let d = ((dx * (x[1] - a[1]) - dy * (x[0] - a[0])) / dx.hypot(dy).max(1e-12)).abs();
                v.push((
                    "点と線の距離",
                    K::PointLineDistance {
                        point: p,
                        line: l,
                        value: dim_expr(editor, fmt_mm(d)),
                    },
                ));
            }
        }
        (k, ids) if k.len() == 2 && k.contains(&Kind::Point) && k.contains(&Kind::Round) => {
            let (p, c) = if k[0] == Kind::Point {
                (ids[0], ids[1])
            } else {
                (ids[1], ids[0])
            };
            v.push((
                "円周上",
                K::PointOnCircle {
                    point: p,
                    circle: c,
                },
            ));
        }
        (k, ids) if k.len() == 2 && k.contains(&Kind::Line) && k.contains(&Kind::Round) => {
            let (l, c) = if k[0] == Kind::Line {
                (ids[0], ids[1])
            } else {
                (ids[1], ids[0])
            };
            v.push(("接線", K::Tangent { line: l, circle: c }));
        }
        (k, ids) if k.len() == 2 && k.contains(&Kind::Point) && k.contains(&Kind::Curve) => {
            let (p, c) = if k[0] == Kind::Point {
                (ids[0], ids[1])
            } else {
                (ids[1], ids[0])
            };
            v.push(("曲線上", K::PointOnCurve { point: p, curve: c }));
        }
        // 楕円・スプライン・円錐曲線とほかの線: 接する(G1)と曲率(G2)。曲率は端でつながる時だけ
        (k, [a, b]) if k.len() == 2 && k.contains(&Kind::Curve) && !k.contains(&Kind::Point) => {
            v.push(("接線", K::TangentCurve { a: *a, b: *b }));
            if shares_end(sketch, *a, *b) {
                v.push(("曲率", K::Curvature { a: *a, b: *b }));
            }
        }
        // 円弧どうしはつながる所で曲率もそろえられる
        (k, ids)
            if k.len() == 3
                && k.iter().filter(|x| **x == Kind::Point).count() == 2
                && k.contains(&Kind::Line) =>
        {
            let line = ids[k.iter().position(|x| *x == Kind::Line).unwrap()];
            let pts: Vec<EntityId> = ids.iter().copied().filter(|i| *i != line).collect();
            v.push((
                "対称",
                K::Symmetric {
                    a: pts[0],
                    b: pts[1],
                    line,
                },
            ));
        }
        _ => {}
    }
    // 点どうし(円・円弧は中心)を水平・垂直に並べる
    if let Some((a, b)) = point_pair(sketch, sel) {
        v.push(("水平", K::HorizontalPoints { a, b }));
        v.push(("垂直", K::VerticalPoints { a, b }));
    }
    // 寸法の道具と同じ寸法(円と点・線・円は中心から・円周の近い側から・遠い側から、
    // 中心線と点は中心線からの直径)もパネルのボタンに出す(網羅版 F2・F3)
    if dims && let Some(base) = kmb_sketch::dimension_for(sketch, sol, sel) {
        use crate::dim_choice::{DimChoice, Gap, apply};
        let round = kinds.contains(&Kind::Round) && sel.len() == 2;
        let mut extra = vec![base.clone()];
        if round {
            for gap in [Gap::Near, Gap::Far] {
                let c = DimChoice {
                    gap: Some(gap),
                    ..DimChoice::default()
                };
                extra.push(apply(base.clone(), &c, sketch, sel));
            }
        }
        for mut k in extra {
            let Some(m) = kmb_sketch::measure(sketch, sol, &k) else {
                continue;
            };
            let label = match &k {
                K::CircleDistance { far: false, .. } => "円周から(近い側)",
                K::CircleDistance { far: true, .. } => "円周から(遠い側)",
                K::LinearDiameter { .. } => "中心線からの直径",
                _ if round => "中心からの距離",
                _ => continue,
            };
            if let Some(val) = k.value_mut() {
                *val = dim_expr(editor, fmt_mm(m));
            }
            v.push((label, k));
        }
    }
    v.retain(|(_, k)| k.value().is_some() == dims);
    v
}

/// 選んでいる要素(消えたものは除く)。
pub(crate) fn selected(editor: &Editor, sketch: &Sketch) -> Vec<EntityId> {
    editor.selection.iter().copied().filter(|id| sketch.entity(*id).is_some()).collect()
}

/// 選んでいる要素をクリックの順に並べる。まとめて選んだ要素は番号順で続く。
pub(crate) fn selected_in_order(editor: &Editor, sketch: &Sketch) -> Vec<EntityId> {
    editor
        .selection_order
        .iter()
        .filter(|id| editor.selection.contains(id))
        .chain(editor.selection.iter().filter(|id| !editor.selection_order.contains(id)))
        .copied()
        .filter(|id| sketch.entity(*id).is_some())
        .collect()
}

/// 選んだ要素への操作(リボンの「修正」と、パネルのボタン)。変えたスケッチを返す。
pub(crate) fn selection_action(
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    action: &str,
) -> Option<Sketch> {
    let sel = selected(editor, sketch);
    if sel.is_empty() {
        return None;
    }
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    match action {
        "delete" => {
            s.remove_entities(&sel);
            editor.selection.clear();
        }
        "construction" => {
            for id in &sel {
                s.toggle_construction(*id);
            }
        }
        // 線分を中心線にする・戻す(全部が中心線なら戻す)
        "centerline" => {
            let lines: Vec<EntityId> = sel
                .iter()
                .copied()
                .filter(|id| matches!(s.entity(*id), Some(Entity::Line { .. })))
                .collect();
            if lines.is_empty() {
                return None;
            }
            let all = lines.iter().all(|id| s.is_centerline(*id));
            for id in lines {
                if s.is_centerline(id) == all {
                    s.toggle_centerline(id);
                }
            }
        }
        "offset_out" | "offset_in" | "offset_both" => {
            let value = dim_expr(editor, "2 mm".into());
            // 形を作る時の距離。式が数でなければ 2 mm で作り、解く時に寸法の値へ合わせる
            let d = value
                .trim()
                .trim_end_matches("mm")
                .trim()
                .parse::<f64>()
                .unwrap_or(2.0);
            // 1 本を選んでも、端でつながった線分と円弧の全体をずらす。オフセットの道具で
            // 「チェーン選択」を外していれば、選んだ線だけ(K27)
            let mut whole: Vec<EntityId> = Vec::new();
            for id in &sel {
                let chain = if editor.offset.single {
                    vec![*id]
                } else {
                    sketch.chain_of(*id)
                };
                for c in chain {
                    if !whole.contains(&c) {
                        whole.push(c);
                    }
                }
            }
            let round = editor.offset.round;
            let mut why = None;
            let new = match action {
                "offset_out" => crate::offset_tool::keep(
                    s.offset_corners_why(&whole, d, &value, true, round),
                    &mut why,
                ),
                "offset_in" => crate::offset_tool::keep(
                    s.offset_corners_why(&whole, d, &value, false, round),
                    &mut why,
                ),
                _ => {
                    let mut both = s.offset_corners(&whole, d, &value, true, round);
                    let inner = crate::offset_tool::keep(
                        s.offset_corners_why(&whole, d, &value, false, round),
                        &mut why,
                    );
                    if inner.is_empty() && !both.is_empty() {
                        // 外だけ作れた時も黙らない(内は半径が 0 以下になるなど)
                        editor.notice = Some(format!(
                            "内側は作れませんでした({})。外側だけ作りました",
                            why.unwrap_or("作れる形になりません")
                        ));
                    }
                    both.extend(inner);
                    both
                }
            };
            if new.is_empty() {
                editor.notice = Some(format!(
                    "オフセットできませんでした({})",
                    why.unwrap_or("作れる形になりません")
                ));
                return None;
            }
            editor.selection = new.into_iter().collect();
            editor.dim_text.clear();
        }
        _ => return None,
    }
    Some(s)
}

/// 右の編集パネル(試作の「拘束と寸法」): 解けた様子、寸法の値、選んだ要素の寸法と操作、
/// 付いている拘束の一覧。形の拘束(水平・一致など)はリボンにある。
pub(crate) fn constraint_panel(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
) -> Option<Sketch> {
    let l = theme::look(ui.ctx());
    let dims = sketch
        .constraints
        .iter()
        .filter(|c| c.kind.value().is_some())
        .count();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        let color = match sol.status {
            _ if sol.is_blank() => view::free_ink(&l),
            SolveStatus::WellConstrained => l.pal.ok,
            SolveStatus::UnderConstrained { .. } => view::free_ink(&l),
            SolveStatus::OverConstrained { .. } => l.pal.warn,
            SolveStatus::Inconsistent { .. } => l.pal.err,
        };
        let status = view::status_text(sol);
        theme::chip(ui, &status, Some(color))
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &status));
        theme::chip(
            ui,
            &format!("拘束 {}", sketch.constraints.len() - dims),
            None,
        );
        theme::chip(ui, &format!("寸法 {dims}"), None);
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for (c, t) in [
            (l.pal.ink, "決まった"),
            (view::free_ink(&l), "動ける"),
            (l.pal.warn, "余分"),
            (l.pal.err, "矛盾"),
        ] {
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, c);
            ui.label(
                egui::RichText::new(t)
                    .size(kmb_ui::theme::fs(11.0))
                    .color(l.pal.ink2),
            );
            ui.add_space(6.0);
        }
    });
    ui.checkbox(&mut editor.show_marks, "拘束を表示")
        .on_hover_text("水平・平行・一致などの拘束を、図の要素の脇に印で出す。印をクリックで選び、Delete で外す");
    ui.add_space(4.0);
    // 値の欄は選ぶ要素で動かないよう、選んだ要素の拘束の一覧より上に置く
    let field = ui
        .horizontal(|ui| {
            ui.label("寸法の値");
            ui.add(
                egui::TextEdit::singleline(&mut editor.dim_text)
                    .id(egui::Id::new(DIM_FIELD))
                    .hint_text("空なら現在の値")
                    .desired_width(140.0),
            )
        })
        .inner;
    // 欄で Enter を押すか、打ったまま欄の外を左クリックすると決める(Esc はやめる)。
    // 寸法の道具では、欄に入力が無くても Enter で今の寸法を置く(測った値か打った値)
    if field.changed() {
        editor.dim_dirty = true;
    }
    // 外のクリックで決めるのは、人が打った時だけ(測った値で埋めただけの欄は、外を押しても何もしない)
    let left = match crate::view::leave_field(ui, &field) {
        crate::view::Leave::Commit
            if !editor.dim_dirty
                && !ui.input(|i| {
                    i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Tab)
                }) =>
        {
            crate::view::Leave::Cancel
        }
        l => l,
    };
    if left == crate::view::Leave::Keep {
        field.request_focus();
    }
    // 寸法の道具で、置く寸法が図のカーソルについている時に図を押した: 押した所に置くので、
    // 決めるのは図の側に任せる(パネルの外を押した時だけ)
    let to_view = left == crate::view::Leave::Commit
        && editor.tool == Tool::Dimension
        && editor.placing.is_none()
        && editor.ghost.is_some()
        && ui
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| !ui.clip_rect().contains(p));
    if to_view {
        editor.dim_typed_click = true;
    }
    if left != crate::view::Leave::Stay && left != crate::view::Leave::Keep && !to_view {
        editor.dim_dirty = false;
    }
    let enter =
        (left == crate::view::Leave::Commit && !to_view && !editor.dim_text.trim().is_empty())
            || ((field.lost_focus()
                || (editor.tool == Tool::Dimension
                    && editor.placing.is_none()
                    && ui.ctx().memory(|m| m.focused().is_none())))
                && ui.input(|i| i.key_pressed(egui::Key::Enter)));
    let sel = selected(editor, sketch);
    // 付いている拘束の一覧(外す、寸法は参照 ⇔ 駆動を切り替える)。要素を選んでいれば、
    // それを決めている拘束を先に出す(試作の「穴 2 を決めている拘束」)
    let mut remove = None;
    let mut toggle = None;
    let mut row = |ui: &mut egui::Ui, c: &kmb_sketch::Constraint| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            kmb_ui::theme::paint_icon(ui.painter(), r, "lock", l.pal.ink3);
            let refs = match &c.kind {
                K::Pattern { pattern } => pattern.to_string(),
                _ => kmb_sketch::refs(&c.kind)
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            // 寸法は名前(d1 など。ほかの式から参照できる)と式
            let value = c
                .kind
                .value()
                .map(|v| match sketch.dim_name(c.id) {
                    Some(n) => format!(" {n} = {v}"),
                    None => format!(" = {v}"),
                })
                .unwrap_or_default();
            let text = egui::RichText::new(format!(
                "{} {}({refs}){value}{}",
                c.id,
                constraint_name(&c.kind),
                if c.reference { "(参照)" } else { "" }
            ))
            .size(kmb_ui::theme::fs(11.5));
            // ボタンを右に先に置き、残りの幅に収まらない文は折り返す(ボタンに重ねない)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("外す")
                    .on_hover_text("この拘束を消す")
                    .clicked()
                {
                    remove = Some(c.id);
                }
                if c.kind.value().is_some() {
                    let (label, hint) = if c.reference {
                        ("駆動にする", "この寸法で形を決める(今の形で測った値から)")
                    } else {
                        (
                            "参照にする",
                            "形を決めずに、測った値を括弧つきで出すだけにする",
                        )
                    };
                    if ui.small_button(label).on_hover_text(hint).clicked() {
                        toggle = Some((c.id, c.reference));
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(text).wrap());
                });
            });
        });
    };
    if !sel.is_empty() {
        let touching: Vec<&kmb_sketch::Constraint> = sketch
            .constraints
            .iter()
            .filter(|c| {
                kmb_sketch::refs(&c.kind).iter().any(|r| {
                    sel.contains(r) || sel.iter().any(|e| own_points(sketch, *e).contains(r))
                }) || (matches!(c.kind, K::Pattern { .. })
                    && crate::marks::members(sketch, &c.kind)
                        .iter()
                        .any(|m| sel.contains(m)))
            })
            .collect();
        theme::section(
            ui,
            "選んだ要素を決めている拘束",
            &touching.len().to_string(),
        );
        if touching.is_empty() {
            ui.weak("まだ拘束はありません");
        }
        for c in touching {
            row(ui, c);
        }
        ui.add_space(4.0);
    }

    if editor.tool == Tool::Dimension
        && let Some(values) = values
        && let Some(s) = dimension_tool(ui, editor, sketch, sol, values, &sel, enter)
    {
        return Some(s);
    }
    let kinds: Vec<Kind> = sel.iter().filter_map(|id| kind(sketch, *id)).collect();
    let mut chosen: Option<K> = None;
    let mut action: Option<&str> = None;
    theme::section(ui, "選んだ要素", &format!("{} 個", sel.len()));
    if sel.is_empty() {
        ui.label(
            egui::RichText::new(
                "図の線・円・点をクリックで選ぶと、付けられる寸法が出ます(Shift で複数選択)。水平・一致などの拘束は上の帯。付いた寸法の値は、図の数字をダブルクリックで直せます",
            )
            .size(kmb_ui::theme::fs(11.5))
            .color(l.pal.ink3),
        );
    } else {
        let dims = options(editor, sketch, sol, &sel, true);
        // 付けられる寸法が 1 つなら、値の欄で Enter を押すとそれを付ける
        if enter && let [(_, k)] = &dims[..] {
            chosen = Some(k.clone());
        }
        ui.horizontal_wrapped(|ui| {
            if dims.is_empty() {
                ui.weak("この組み合わせに付けられる寸法はありません");
            } else if dims.len() == 1 {
                ui.weak("値を打って Enter か、");
            }
            for (label, k) in dims {
                if ui.button(label).clicked() {
                    chosen = Some(k);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("削除").clicked() {
                action = Some("delete");
            }
            if kinds.iter().any(|k| *k != Kind::Point) {
                if ui.button("補助線の切り替え").clicked() {
                    action = Some("construction");
                }
                if kinds.contains(&Kind::Line) && ui.button("中心線の切り替え").clicked() {
                    action = Some("centerline");
                }
                for (id, label) in [
                    ("offset_out", "外へオフセット"),
                    ("offset_in", "内へオフセット"),
                    ("offset_both", "両側へオフセット"),
                ] {
                    if ui
                        .button(label)
                        .on_hover_text(
                            "端でつながった線分・円弧と円を、寸法の値(空なら 2 mm)だけずらして写す(1 本を選べば連なり全体)",
                        )
                        .clicked()
                    {
                        action = Some(id);
                    }
                }
            }
        });
    }
    let mut out = None;
    if let Some(k) = chosen {
        let mut s = sketch.clone();
        s.adopt_solution(sol);
        // 寸法はほかの拘束で既に決まっていないかを調べ、決まっていれば参照寸法として置く
        // (寸法の道具と同じ)
        match (k.value().is_some(), values) {
            (true, Some(values)) => match s.add_dimension(k, values) {
                Ok((id, preview)) => {
                    if editor.dim_choice.driven
                        && let Some(c) = s.constraints.iter_mut().find(|c| c.id == id)
                    {
                        c.reference = true;
                    }
                    editor.notice = preview.note(&s);
                    editor.dim_error = None;
                    editor.dim_text.clear();
                    out = Some(s);
                }
                Err(e) => editor.dim_error = Some(e),
            },
            _ => {
                s.add_constraint(k);
                editor.dim_text.clear();
                out = Some(s);
            }
        }
    }
    if let Some(a) = action
        && let Some(s) = selection_action(editor, sketch, sol, a)
    {
        out = Some(s);
    }

    egui::CollapsingHeader::new(format!("拘束({})", sketch.constraints.len())).show(ui, |ui| {
        for c in &sketch.constraints {
            row(ui, c);
        }
    });
    if let Some(id) = remove {
        let mut s = sketch.clone();
        s.adopt_solution(sol);
        s.remove_constraint(id);
        out = Some(s);
    }
    if let (Some((id, driving)), Some(values)) = (toggle, values) {
        let mut s = sketch.clone();
        s.adopt_solution(sol);
        match s.set_driving(id, driving, values) {
            Ok(()) => {
                editor.dim_error = None;
                out = Some(s);
            }
            Err(e) => editor.dim_error = Some(e),
        }
    }
    if let Some(e) = &editor.dim_error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    out
}

/// 寸法の値の欄の ID(寸法の道具で選んだ時に、ここへ入力を移す)。
pub(crate) const DIM_FIELD: &str = "sketch_dim_text";

/// 寸法の道具: 選んだ要素に合う寸法を出し、値の欄に測った値を入れて入力を移す。
/// 寸法の種類は図でカーソルについている寸法(置いた後はその寸法)に合わせる。斜めの線の上下へ
/// 出せば横の距離、左右へ出せば縦の距離を測り、Enter でもその寸法を付ける(網羅版 L8)。
/// ほかの拘束で既に決まっていれば、その旨を出して参照寸法として置く。Enter で 1 回の変更として置く。
fn dimension_tool(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: &DimValues,
    sel: &[EntityId],
    enter: bool,
) -> Option<Sketch> {
    let shown = match (&editor.placing, &editor.ghost) {
        (Some(p), _) => Some(p.kind.clone()),
        (None, Some(g)) if editor.ghost_sel == sel => Some(g.0.clone()),
        _ => None,
    };
    let kind = shown.or_else(|| kmb_sketch::dimension_for(sketch, sol, sel));
    let Some(kind) = kind else {
        editor.dim_kind = None;
        if !sel.is_empty() {
            ui.weak("この選び方に付ける寸法はありません(線分、円、点 2 つ、点と線分、線分 2 本)");
        }
        return None;
    };
    let preview = match kmb_sketch::preview_dimension(sketch, values, &kind) {
        Ok(p) => p,
        Err(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e.to_string());
            return None;
        }
    };
    if editor.dim_kind.as_ref() != Some(&kind) {
        // 選びか、カーソルで選ぶ寸法の種類が変わった: 値の欄を測った値にして、入力を移す
        // (全部選んで打ち直せる)。種類だけが変わった時に人が打った値は残す
        let same_sel = editor.dim_sel == sel;
        let typed = editor.dim_kind.take().is_some_and(|old| {
            same_sel
                && kmb_sketch::preview_dimension(sketch, values, &old)
                    .is_ok_and(|p| p.text != editor.dim_text)
        });
        editor.dim_kind = Some(kind.clone());
        editor.dim_sel = sel.to_vec();
        if !typed {
            editor.dim_text = preview.text.clone();
            editor.dim_dirty = false;
            let id = egui::Id::new(DIM_FIELD);
            // 寸法の道具では入力を移さない(1 文字のキーで道具を選べるように。網羅版 F8)。
            // 数字から打ち始めると図の側がこの欄へ移す
            if editor.tool != Tool::Dimension {
                ui.ctx().memory_mut(|m| m.request_focus(id));
            }
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(editor.dim_text.chars().count()),
                );
                st.cursor.set_char_range(Some(all));
                st.store(ui.ctx(), id);
            }
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("{}の寸法", constraint_name(&kind)));
        ui.weak("値を打って Enter か、図を左クリックした所に置く(Esc でやめる)");
    });
    if let Some(note) = preview.note(sketch) {
        ui.colored_label(ui.visuals().warn_fg_color, note);
    }
    if !enter {
        return None;
    }
    let mut k = kind;
    if let Some(v) = k.value_mut() {
        *v = kmb_ui::with_unit_of(editor.dim_text.trim(), &preview.text);
    }
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    match s.add_dimension(k, values) {
        Ok((id, _)) => {
            if editor.dim_choice.driven
                && let Some(c) = s.constraints.iter_mut().find(|c| c.id == id)
            {
                c.reference = true;
            }
            editor.selection.clear();
            editor.dim_kind = None;
            editor.dim_text.clear();
            editor.dim_error = None;
            Some(s)
        }
        Err(e) => {
            editor.dim_error = Some(e);
            None
        }
    }
}

pub(crate) fn constraint_name(k: &K) -> &'static str {
    match k {
        K::Coincident { .. } => "一致",
        K::Fix { .. } => "固定",
        K::Horizontal { .. } | K::HorizontalPoints { .. } => "水平",
        K::Vertical { .. } | K::VerticalPoints { .. } => "垂直",
        K::Parallel { .. } => "平行",
        K::Perpendicular { .. } => "直交",
        K::Equal { .. } => "等しい",
        K::PointOnLine { .. } => "線上",
        K::PointOnCircle { .. } => "円周上",
        K::Tangent { .. } | K::TangentCircles { .. } | K::TangentCurve { .. } => "接線",
        K::PointOnCurve { .. } => "曲線上",
        K::Curvature { .. } => "曲率",
        K::CircleDistance { far: false, .. } => "円周からの最短距離",
        K::CircleDistance { far: true, .. } => "円周からの最長距離",
        K::LinearDiameter { .. } => "中心線からの直径",
        K::Concentric { .. } => "同心円",
        K::Collinear { .. } => "同一直線上",
        K::Midpoint { .. } => "中点",
        K::Symmetric { .. } | K::SymmetricPairs { .. } => "対称",
        K::Distance { .. } => "距離",
        K::PointLineDistance { .. } => "点と線の距離",
        K::HorizontalDistance { .. } => "水平距離",
        K::VerticalDistance { .. } => "垂直距離",
        K::Length { .. } => "長さ",
        K::Radius { .. } => "半径",
        K::Diameter { .. } => "直径",
        K::Angle { .. } => "角度",
        K::Offset { .. } => "オフセット",
        K::Polygon { .. } => "ポリゴン",
        K::Pattern { .. } => "パターン",
    }
}

#[cfg(test)]
mod dim_tests {
    use super::*;

    /// 寸法に数だけを打つと、長さは文書の単位、角度は deg を補う(測った値の mm を補うと、
    /// インチの文書で「2」が 2 mm になる)。
    #[test]
    fn a_bare_dimension_takes_the_document_unit() {
        let mut e = Editor {
            dim_text: "2".into(),
            ..Editor::default()
        };
        kmb_core::set_display_unit(kmb_core::LengthUnit::In);
        let length = dim_expr(&e, fmt_mm(10.0));
        let angle = dim_expr(&e, "30 deg".into());
        kmb_core::set_display_unit(kmb_core::LengthUnit::Mm);
        assert_eq!(length, "2 in");
        assert_eq!(angle, "2 deg");
        e.dim_text.clear();
        assert_eq!(dim_expr(&e, fmt_mm(10.0)), "10 mm");
    }
}
