//! 足した作図の道具(3 点指定の長方形、2 点・3 点・接する円、3 点の円弧、外接の多角形、長穴 3 種、
//! 面取り、並べ、動かす・回す・拡大縮小)のクリックの流れと見本。形を作るのは jc-sketch。

use crate::view::{Editor, Pick, Tool, Transform, arc_points, slot_cap, typed_length, typed_value};
use egui::{Pos2, Stroke};
use jc_sketch::{Entity, EntityId, Sketch, Solution};
use std::f64::consts::{PI, TAU};

/// この道具の流れをここで扱うか。
pub(crate) fn handles(t: Tool) -> bool {
    matches!(
        t,
        Tool::ThreePointRectangle
            | Tool::TwoPointCircle
            | Tool::ThreePointCircle
            | Tool::TangentCircle
            | Tool::TwoTangentCircle
            | Tool::Ellipse
            | Tool::FitSpline
            | Tool::ControlSpline
            | Tool::Conic
            | Tool::Blend
            | Tool::MidpointLine
            | Tool::EdgePolygon
            | Tool::ThreePointArcSlot
            | Tool::ThreePointArc
            | Tool::OuterPolygon
            | Tool::OverallSlot
            | Tool::CenterSlot
            | Tool::ArcSlot
            | Tool::Chamfer
            | Tool::LinearPattern
            | Tool::CircularPattern
            | Tool::Move
            | Tool::Rotate
            | Tool::Scale
    )
}

/// 選んだ要素に使う道具(先に選んでおく)。
pub(crate) fn needs_selection(t: Tool) -> bool {
    matches!(
        t,
        Tool::LinearPattern | Tool::CircularPattern | Tool::Move | Tool::Rotate | Tool::Scale
    )
}

pub(crate) fn hint(t: Tool, step: usize) -> &'static str {
    match (t, step) {
        (Tool::ThreePointRectangle, 0) => "1 辺の始まりをクリック",
        (Tool::ThreePointRectangle, 1) => "1 辺の終わりをクリック(この辺の向きに傾く)",
        (Tool::ThreePointRectangle, _) => "向かいの辺の位置をクリック",
        (Tool::TwoPointCircle, 0) => "直径の片端をクリック",
        (Tool::TwoPointCircle, _) => "直径のもう片端をクリック",
        (Tool::ThreePointCircle, 0) => "円周の 1 点目をクリック",
        (Tool::ThreePointCircle, 1) => "円周の 2 点目をクリック",
        (Tool::ThreePointCircle, _) => "円周の 3 点目をクリック",
        (Tool::TangentCircle, 0..=2) => "接する線分・円・円弧を 3 つ、順にクリック",
        (Tool::TangentCircle, _) => "円を置く側をクリック(中心がカーソルに近い円を作る)",
        (Tool::TwoTangentCircle, 0..=1) => "接する線分・円・円弧を 2 つ、順にクリック",
        (Tool::TwoTangentCircle, _) => "円を置く側をクリック(カーソルを通る大きさ)",
        (Tool::Ellipse, 0) => "楕円の中心をクリック",
        (Tool::Ellipse, 1) => "長軸の端をクリック(長さと向き)",
        (Tool::Ellipse, _) => "短軸の大きさの位置をクリック",
        (Tool::FitSpline, 0) => "スプラインが通る最初の点をクリック",
        (Tool::FitSpline, _) => "次に通る点をクリック。Enter で終える、最初の点を押すと閉じる",
        (Tool::ControlSpline, 0) => "最初の制御点をクリック",
        (Tool::ControlSpline, _) => "次の制御点をクリック。Enter で終える、最初の点を押すと閉じる",
        (Tool::Conic, 0) => "円錐曲線の始点をクリック",
        (Tool::Conic, 1) => "終点をクリック",
        (Tool::Conic, _) => {
            "両端の接線が交わる点をクリック(形の値 rho は先に打つ。既定は 0.5、0 と 1 の間)"
        }
        (Tool::Blend, 0) => "つなぐ 1 つ目の曲線の端をクリック",
        (Tool::Blend, _) => "2 つ目の曲線の端をクリック(パネルで G1・G2 を選ぶ)",
        (Tool::MidpointLine, 0) => "線分の中点をクリック",
        (Tool::MidpointLine, _) => "片方の端をクリック(もう片方は中点の反対側)",
        (Tool::EdgePolygon, 0) => "1 辺の始まりをクリック",
        (Tool::EdgePolygon, 1) => "1 辺の終わりをクリック",
        (Tool::EdgePolygon, _) => "多角形を置く側をクリック(辺の数は先に打つ。既定は 6)",
        (Tool::ThreePointArcSlot, 0) => "長穴の中心線の始まりをクリック",
        (Tool::ThreePointArcSlot, 1) => "中心線の終わりをクリック",
        (Tool::ThreePointArcSlot, 2) => "中心線の円弧が通る点をクリック",
        (Tool::ThreePointArcSlot, _) => "幅の位置をクリック、または幅を打って Enter",
        (Tool::ThreePointArc, 0) => "始点をクリック",
        (Tool::ThreePointArc, 1) => "終点をクリック",
        (Tool::ThreePointArc, _) => "円弧が通る点をクリック",
        (Tool::OuterPolygon, 0) => "中心をクリック",
        (Tool::OuterPolygon, _) => "辺の中点をクリック(辺の数は先に打つ。既定は 6)",
        (Tool::OverallSlot, 0) => "長穴の片方の先をクリック",
        (Tool::OverallSlot, 1) => "もう片方の先をクリック",
        (Tool::OverallSlot, _) => "幅の位置をクリック、または幅を打って Enter",
        (Tool::CenterSlot, 0) => "長穴の中心をクリック",
        (Tool::CenterSlot, 1) => "片方の端の中心をクリック",
        (Tool::CenterSlot, _) => "幅の位置をクリック、または幅を打って Enter",
        (Tool::ArcSlot, 0) => "円弧の中心をクリック",
        (Tool::ArcSlot, 1) => "長穴の中心線の始まりをクリック",
        (Tool::ArcSlot, 2) => "中心線の終わりの向きをクリック(反時計回り)",
        (Tool::ArcSlot, _) => "幅の位置をクリック、または幅を打って Enter",
        (Tool::Chamfer, _) => {
            "落とす角か線分を 2 本クリック。数字を打つ: 2 で等しい距離、2, 3 で距離 2 つ、2<45 で距離と角度(角に近い側の線分が 1 本目)"
        }
        (Tool::LinearPattern, 0) => "選んだ要素を並べる: 基準の点をクリック",
        (Tool::LinearPattern, _) => "次の 1 つの位置をクリック(間隔と向き。数は先に打つ。既定は 3)",
        (Tool::CircularPattern, 0) => "選んだ要素を並べる: 回す中心をクリック",
        (Tool::CircularPattern, _) => "数を打って Enter か、もう一度クリック(既定は 6、一周に等分)",
        (Tool::Move, 0) => "選んだ要素を動かす: 動かす元の点をクリック",
        (Tool::Move, _) => "動かす先をクリック、または「x, y」の量を打って Enter",
        (Tool::Rotate, 0) => "選んだ要素を回す: 回す中心をクリック",
        (Tool::Rotate, 1) => "角度を打って Enter(度、反時計回り)、または基準の向きをクリック",
        (Tool::Rotate, _) => "回した後の向きをクリック",
        (Tool::Scale, 0) => "選んだ要素を拡大縮小する: 中心をクリック",
        (Tool::Scale, 1) => "倍率を打って Enter、または基準の点をクリック",
        (Tool::Scale, _) => "基準の点が移る先をクリック",
        _ => "",
    }
}

/// 描いている途中に数を打ち込む道具(打ち込みは、最初の位置を決めた後)。
pub(crate) fn takes_typing(editor: &Editor) -> bool {
    match editor.tool {
        Tool::OuterPolygon
        | Tool::EdgePolygon
        | Tool::Conic
        | Tool::LinearPattern
        | Tool::CircularPattern
        | Tool::Move
        | Tool::Rotate
        | Tool::Scale => true,
        // 点は最初から「x, y」を打てる(数か符号・小数点で始めた時。道具のキーと分ける)
        Tool::Point => true,
        Tool::OverallSlot | Tool::CenterSlot => editor.pending2.is_some(),
        Tool::ArcSlot | Tool::ThreePointArcSlot => editor.pending3.is_some(),
        _ => false,
    }
}

/// 打ち込み欄の名前(図に「名前: 値」と出す)。
pub(crate) fn typed_label(t: Tool) -> Option<&'static str> {
    Some(match t {
        Tool::OuterPolygon | Tool::EdgePolygon => "辺の数",
        Tool::Conic => "rho",
        Tool::LinearPattern | Tool::CircularPattern => "数",
        Tool::Move | Tool::Point => "x, y",
        Tool::Rotate => "角度",
        Tool::Scale => "倍率",
        Tool::OverallSlot | Tool::CenterSlot | Tool::ArcSlot | Tool::ThreePointArcSlot => "幅",
        _ => return None,
    })
}

/// 道具に渡す、このフレームの入力。
pub(crate) struct Input<'a> {
    pub sketch: &'a Sketch,
    pub sol: &'a Solution,
    pub cursor: Option<[f64; 2]>,
    pub clicked: bool,
    pub enter: bool,
    pub tol: f64,
    /// Shift を押している(先に選ぶ道具で、選びを足し外しする)
    pub shift: bool,
}

impl Input<'_> {
    fn pick(&self, p: [f64; 2]) -> Pick {
        Pick {
            point: self
                .sketch
                .nearest_point(self.sol, p[0], p[1], self.tol)
                .map(|h| h.entity),
            pos: p,
        }
    }

    /// 解いた位置を写したスケッチ(ここに足していく)。
    fn start(&self) -> Sketch {
        let mut s = self.sketch.clone();
        s.adopt_solution(self.sol);
        s
    }
}

fn count(typed: &str, default: usize) -> usize {
    typed
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|n| (2..=200).contains(n))
        .unwrap_or(default)
}

fn spot(p: Pick) -> jc_sketch::Spot {
    (p.point, p.pos)
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// 中心と端の中心を結ぶ線からカーソルまでの距離(長穴の幅の半分)。
fn half_width(a: [f64; 2], b: [f64; 2], w: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]).max(1e-9);
    ((w[0] - a[0]) * d[1] - (w[1] - a[1]) * d[0]).abs() / l
}

/// 打ち込んだ幅(数なら半分にする)。数でなければ `fallback`。
fn typed_half(typed: &str, fallback: f64) -> f64 {
    typed_length(&typed_value(typed)).map_or(fallback, |v| v / 2.0)
}

/// 長穴に幅の寸法を付ける。端の円弧の直径の寸法にする。
fn slot_width_dimension(s: &mut Sketch, cap: EntityId, typed: Option<String>) {
    let Some(v) = typed else { return };
    let value = match typed_length(&v) {
        Some(x) => format!("{x} mm"),
        None => v,
    };
    s.add_constraint(jc_sketch::ConstraintKind::Diameter { circle: cap, value });
}

/// 面取りの打ち込み: 「2」等しい距離、「2, 3」距離 2 つ、「2<45」距離と角度(度)。空なら既定の値
/// (mm の文書は 2 mm、インチの文書は 1/16 in)。
pub(crate) fn chamfer_kind(text: &str) -> jc_sketch::Chamfer {
    use jc_sketch::Chamfer;
    let mm = |t: &str| {
        let v = typed_value(t.trim());
        let d = typed_length(&v).unwrap_or(2.0);
        (d, v)
    };
    let t = text.trim();
    if let Some((a, b)) = t.split_once('<')
        && let Ok(deg) = b.trim().trim_end_matches("deg").trim().parse::<f64>()
    {
        let (d, value) = mm(a);
        return Chamfer::DistanceAngle {
            d,
            angle: deg.to_radians(),
            value,
            angle_value: format!("{deg} deg"),
        };
    }
    if let Some((a, b)) = t.split_once(',') {
        let ((d1, value1), (d2, value2)) = (mm(a), mm(b));
        return Chamfer::TwoDistances {
            d1,
            d2,
            value1,
            value2,
        };
    }
    if t.is_empty() {
        let value = crate::view::default_corner();
        return Chamfer::Equal {
            d: typed_length(&value).unwrap_or(2.0),
            value,
        };
    }
    let (d, value) = mm(t);
    Chamfer::Equal { d, value }
}

/// 円錐曲線の形の値(打った値。0 と 1 の間でなければ 0.5)。
fn conic_rho(typed: &str) -> f64 {
    typed
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| *v > 0.0 && *v < 1.0)
        .unwrap_or(0.5)
}

/// クリックした所に近い、曲線の端(曲線, 端の点)。
fn curve_end_at(inp: &Input<'_>, w: [f64; 2]) -> Option<(EntityId, EntityId)> {
    let p = inp
        .sketch
        .nearest_point(inp.sol, w[0], w[1], inp.tol)?
        .entity;
    inp.sketch
        .entities
        .iter()
        .rev()
        .find_map(|e| match e.ends() {
            Some((a, b)) if a == p || b == p => Some((e.id(), p)),
            _ => None,
        })
}

/// クリックした所の線分・円・円弧(接する円の相手)。
fn touchable(inp: &Input<'_>, w: [f64; 2]) -> Option<EntityId> {
    inp.sketch
        .hits(inp.sol, w[0], w[1], inp.tol)
        .into_iter()
        .map(|h| h.entity)
        .find(|id| {
            matches!(
                inp.sketch.entity(*id),
                Some(Entity::Line { .. } | Entity::Circle { .. } | Entity::Arc { .. })
            )
        })
}

fn selected(editor: &Editor, sketch: &Sketch) -> Vec<EntityId> {
    editor
        .selection
        .iter()
        .copied()
        .filter(|id| sketch.entity(*id).is_some())
        .collect()
}

/// 道具の操作。変えたスケッチを返す。
pub(crate) fn interact(editor: &mut Editor, inp: &Input<'_>) -> Option<Sketch> {
    let typed = (inp.enter && !editor.typed.trim().is_empty()).then(|| editor.typed.clone());
    if needs_selection(editor.tool) {
        // 選び直し(網羅版 B14): 何も選ばずに始めた時はクリックで選び Enter で選び終える。
        // 選んでから始めた時も、位置を決める前なら Shift+クリックで足し外しできる
        if inp.clicked && editor.pending.is_none() && (editor.choosing || inp.shift) {
            if let Some(w) = inp.cursor
                && let Some(h) = inp.sketch.hit_test(inp.sol, w[0], w[1], inp.tol)
                && !editor.selection.remove(&h.entity)
            {
                editor.selection.insert(h.entity);
            }
            return None;
        }
        if editor.choosing {
            if inp.enter && !selected(editor, inp.sketch).is_empty() {
                editor.choosing = false;
            }
            return None;
        }
        if selected(editor, inp.sketch).is_empty() {
            return None;
        }
    }
    let w = inp.cursor;
    let click = inp.clicked.then_some(w).flatten();
    let mut out = None;
    match editor.tool {
        Tool::ThreePointRectangle | Tool::ThreePointCircle | Tool::ThreePointArc => {
            let w = click?;
            match (editor.pending, editor.pending2) {
                (None, _) => editor.pending = Some(inp.pick(w)),
                (Some(_), None) => editor.pending2 = Some(inp.pick(w)),
                (Some(a), Some(b)) => {
                    let mut s = inp.start();
                    let c = inp.pick(w);
                    let made = match editor.tool {
                        Tool::ThreePointRectangle => {
                            s.add_three_point_rectangle(spot(a), spot(b), w).is_some()
                        }
                        Tool::ThreePointCircle => s
                            .add_circle_three_points(spot(a), spot(b), spot(c))
                            .is_some(),
                        _ => s.add_arc_three_points(spot(a), spot(b), spot(c)).is_some(),
                    };
                    out = made.then_some(s);
                    editor.cancel();
                }
            }
        }
        Tool::TwoPointCircle => match editor.pending {
            None => editor.pending = Some(inp.pick(click?)),
            Some(a) => {
                // 直径を打っていれば、カーソルの向きにその長さで決める(Enter でも決まる)
                let typed = editor.hud.dim("直径");
                if click.is_none() && !(inp.enter && typed.is_some()) {
                    return None;
                }
                let w = w.unwrap_or([a.pos[0] + 1.0, a.pos[1]]);
                let b = if typed.is_some() {
                    (None, crate::view::aimed(editor, w))
                } else {
                    spot(inp.pick(w))
                };
                let mut s = inp.start();
                out = s.add_circle_two_points(spot(a), b).map(|circle| {
                    if let Some(value) = typed {
                        s.add_constraint(jc_sketch::ConstraintKind::Diameter { circle, value });
                    }
                    s
                });
                editor.cancel();
                editor.hud.clear();
            }
        },
        Tool::TangentCircle | Tool::TwoTangentCircle => {
            // 線分・円・円弧を 2 つか 3 つ選び、置く側をクリック(網羅版 C5)
            let w = click?;
            let need = if editor.tool == Tool::TangentCircle {
                3
            } else {
                2
            };
            if editor.picked.len() < need {
                if let Some(id) = touchable(inp, w)
                    && !editor.picked.contains(&id)
                {
                    editor.picked.push(id);
                }
            } else {
                let p = &editor.picked;
                let mut s = inp.start();
                let made = if need == 3 {
                    s.add_circle_tangent_three([p[0], p[1], p[2]], w)
                } else {
                    s.add_circle_tangent_two(p[0], p[1], w)
                };
                if made.is_none() {
                    editor.notice =
                        Some("接する円を作れません: 置く側を変えてクリックしてください".into());
                    return None;
                }
                out = Some(s);
                editor.cancel();
            }
        }
        Tool::Ellipse => {
            let w = click?;
            match (editor.pending, editor.pending2) {
                (None, _) => editor.pending = Some(inp.pick(w)),
                (Some(_), None) => editor.pending2 = Some(inp.pick(w)),
                (Some(c), Some(a)) => {
                    let mut s = inp.start();
                    out = s.add_ellipse(spot(c), spot(a), w).map(|_| s);
                    if out.is_none() {
                        editor.notice =
                            Some("楕円を作れません: 短軸の位置が長軸の上にあります".into());
                    }
                    editor.cancel();
                }
            }
        }
        Tool::FitSpline | Tool::ControlSpline => {
            let control = editor.tool == Tool::ControlSpline;
            if let Some(w) = click {
                let p = inp.pick(w);
                // 最初の点を押すと閉じる
                let first = editor.spline_pts.first().copied();
                let closes = editor.spline_pts.len() >= 3
                    && first.is_some_and(|f| {
                        (f.point.is_some() && f.point == p.point) || dist(f.pos, w) <= inp.tol
                    });
                if !closes {
                    editor.spline_pts.push(p);
                    return None;
                }
                let pts: Vec<jc_sketch::Spot> =
                    editor.spline_pts.iter().map(|p| spot(*p)).collect();
                let mut s = inp.start();
                out = s
                    .add_spline_degree(&pts, control, true, editor.spline_degree)
                    .map(|_| s);
                editor.cancel();
            } else if inp.enter && typed.is_none() {
                if editor.spline_pts.len() < 2 {
                    return None;
                }
                let pts: Vec<jc_sketch::Spot> =
                    editor.spline_pts.iter().map(|p| spot(*p)).collect();
                let mut s = inp.start();
                out = s
                    .add_spline_degree(&pts, control, false, editor.spline_degree)
                    .map(|_| s);
                editor.cancel();
            }
        }
        Tool::Conic => {
            let w = click?;
            match (editor.pending, editor.pending2) {
                (None, _) => editor.pending = Some(inp.pick(w)),
                (Some(_), None) => editor.pending2 = Some(inp.pick(w)),
                (Some(a), Some(b)) => {
                    let rho = conic_rho(&editor.typed);
                    let mut s = inp.start();
                    out = s
                        .add_conic(spot(a), spot(b), spot(inp.pick(w)), rho)
                        .map(|_| s);
                    if out.is_none() {
                        editor.notice = Some(
                            "円錐曲線を作れません: 交わる点が両端を結ぶ線の上にあるか、rho が 0 と 1 の間にありません"
                                .into(),
                        );
                    }
                    editor.cancel();
                }
            }
        }
        Tool::Blend => {
            // 曲線の端の点を 2 つ
            let w = click?;
            let Some(end) = curve_end_at(inp, w) else {
                editor.notice = Some("曲線の端を押してください".into());
                return None;
            };
            match editor.picked.as_slice() {
                [] => editor.picked = vec![end.0, end.1],
                [c, p] => {
                    let (c, p) = (*c, *p);
                    let g = if editor.blend_g1 {
                        jc_sketch::Continuity::G1
                    } else {
                        jc_sketch::Continuity::G2
                    };
                    let mut s = inp.start();
                    out = s.add_blend((c, p), end, g).map(|_| s);
                    if out.is_none() {
                        editor.notice =
                            Some("つなげません: 別の曲線の、離れた端を押してください".into());
                    }
                    editor.cancel();
                }
                _ => editor.cancel(),
            }
        }
        Tool::MidpointLine => {
            let w = click?;
            match editor.pending {
                None => editor.pending = Some(inp.pick(w)),
                Some(m) => {
                    let mut s = inp.start();
                    out = s.add_midpoint_line(spot(m), spot(inp.pick(w))).map(|_| s);
                    editor.cancel();
                }
            }
        }
        Tool::EdgePolygon => {
            let w = click?;
            match (editor.pending, editor.pending2) {
                (None, _) => editor.pending = Some(inp.pick(w)),
                (Some(_), None) => editor.pending2 = Some(inp.pick(w)),
                (Some(a), Some(b)) => {
                    let n = count(&editor.typed, editor.polygon_n.clamp(3, 64)).max(3);
                    editor.polygon_n = n;
                    let mut s = inp.start();
                    out = s.add_edge_polygon(spot(a), spot(b), n, w).map(|_| s);
                    editor.cancel();
                }
            }
        }
        Tool::ThreePointArcSlot => {
            let at = click.or_else(|| typed.as_ref().and(w.or(editor.pending3.map(|p| p.pos))));
            let w = at?;
            match (editor.pending, editor.pending2, editor.pending3) {
                (None, ..) => editor.pending = Some(inp.pick(w)),
                (Some(_), None, _) => editor.pending2 = Some(inp.pick(w)),
                (Some(_), Some(_), None) => editor.pending3 = Some(inp.pick(w)),
                (Some(a), Some(b), Some(m)) => {
                    let mut half = jc_sketch::circumcenter(a.pos, b.pos, m.pos)
                        .map_or(1.0, |c| (dist(c, w) - dist(c, a.pos)).abs());
                    if let Some(v) = &typed {
                        half = typed_half(v, half.max(0.5));
                    }
                    let mut s = inp.start();
                    match s.add_three_point_arc_slot(spot(a), m.pos, spot(b), half) {
                        Some([_, _, cap0, _]) => {
                            slot_width_dimension(&mut s, cap0, typed);
                            out = Some(s);
                        }
                        None => {
                            editor.notice = Some(
                                "長穴を作れません: 3 点が一直線か、幅が中心線の半径より大きい"
                                    .into(),
                            );
                        }
                    }
                    editor.cancel();
                }
            }
        }
        Tool::OuterPolygon => {
            let w = click?;
            match editor.pending {
                None => editor.pending = Some(inp.pick(w)),
                Some(c) => {
                    let n = count(&editor.typed, editor.polygon_n.clamp(3, 64)).max(3);
                    editor.polygon_n = n;
                    let mut s = inp.start();
                    out = s.add_circumscribed_polygon(spot(c), w, n).map(|_| s);
                    editor.cancel();
                }
            }
        }
        Tool::OverallSlot | Tool::CenterSlot => {
            let at = click.or_else(|| typed.as_ref().and(w.or(editor.pending2.map(|p| p.pos))));
            let w = at?;
            match (editor.pending, editor.pending2) {
                (None, _) => editor.pending = Some(inp.pick(w)),
                (Some(_), None) => editor.pending2 = Some(inp.pick(w)),
                (Some(a), Some(b)) => {
                    let mut half = half_width(a.pos, b.pos, w);
                    if let Some(v) = &typed {
                        half = typed_half(v, half.max(0.5));
                    }
                    let mut s = inp.start();
                    let made = if editor.tool == Tool::OverallSlot {
                        s.add_overall_slot(spot(a), spot(b), half)
                    } else {
                        s.add_center_slot(spot(a), spot(b), half)
                    };
                    if let Some([_, _, left, _]) = made {
                        slot_width_dimension(&mut s, left, typed);
                        out = Some(s);
                    }
                    editor.cancel();
                }
            }
        }
        Tool::ArcSlot => {
            let at = click.or_else(|| typed.as_ref().and(w.or(editor.pending3.map(|p| p.pos))));
            let w = at?;
            match (editor.pending, editor.pending2, editor.pending3) {
                (None, ..) => editor.pending = Some(inp.pick(w)),
                (Some(_), None, _) => editor.pending2 = Some(inp.pick(w)),
                (Some(_), Some(_), None) => editor.pending3 = Some(inp.pick(w)),
                (Some(c), Some(a), Some(b)) => {
                    let r = dist(c.pos, a.pos);
                    let mut half = (dist(c.pos, w) - r).abs();
                    if let Some(v) = &typed {
                        half = typed_half(v, half.max(0.5));
                    }
                    let mut s = inp.start();
                    if let Some([_, _, cap0, _]) = s.add_arc_slot(spot(c), a.pos, b.pos, half) {
                        slot_width_dimension(&mut s, cap0, typed);
                        out = Some(s);
                    }
                    editor.cancel();
                }
            }
        }
        Tool::Chamfer => {
            // 角の点か、線分を 2 本(1 本目が距離 1 と角度を測る線分。網羅版 D4)
            let w = click?;
            let (corner, first) = match inp.sketch.nearest_point(inp.sol, w[0], w[1], inp.tol) {
                Some(h) => (h.entity, None),
                None => {
                    let line = inp
                        .sketch
                        .hits(inp.sol, w[0], w[1], inp.tol)
                        .into_iter()
                        .map(|h| h.entity)
                        .find(|id| matches!(inp.sketch.entity(*id), Some(Entity::Line { .. })))?;
                    if !editor.picked.contains(&line) {
                        editor.picked.push(line);
                    }
                    if editor.picked.len() < 2 {
                        return None;
                    }
                    let pair = std::mem::take(&mut editor.picked);
                    match crate::view::shared_end(inp.sketch, pair[0], pair[1]) {
                        Some(c) => (c, Some(pair[0])),
                        None => {
                            editor.notice = Some(
                                "角を落とせません: 端でつながった 2 本の線分を押してください"
                                    .into(),
                            );
                            return None;
                        }
                    }
                }
            };
            let text = if editor.typed.trim().is_empty() {
                editor.dim_text.clone()
            } else {
                std::mem::take(&mut editor.typed)
            };
            let kind = chamfer_kind(&text);
            let mut s = inp.start();
            out = s.chamfer_corner_with(corner, first, &kind).map(|_| s);
            if out.is_none() {
                editor.notice = Some(
                    "角を落とせません: 2 本の線分が交わる角を押してください(距離が辺より長い時も作れません)"
                        .into(),
                );
            }
        }
        Tool::LinearPattern => {
            let w = click?;
            match editor.pending {
                None => editor.pending = Some(inp.pick(w)),
                Some(a) => {
                    let ids = selected(editor, inp.sketch);
                    let n = count(&editor.typed, 3);
                    let mut s = inp.start();
                    let step = [w[0] - a.pos[0], w[1] - a.pos[1]];
                    // 後から数や間隔を直せるパターンとして置く
                    let dir =
                        |angle: f64, count: usize, distance: f64| jc_sketch::PatternDirection {
                            axis: jc_sketch::PatternAxis::Angle(angle),
                            count,
                            distance,
                            distance_type: jc_sketch::DistanceType::Spacing,
                            symmetric: false,
                        };
                    let angle = step[1].atan2(step[0]);
                    let def = jc_sketch::RectangularPattern {
                        entities: ids,
                        directions: [
                            dir(angle, n, step[0].hypot(step[1])),
                            dir(angle + std::f64::consts::FRAC_PI_2, 1, 10.0),
                        ],
                        suppressed: vec![],
                    };
                    if s.add_rectangular_pattern(def).is_some() {
                        out = Some(s);
                    }
                    editor.cancel();
                }
            }
        }
        Tool::CircularPattern => match editor.pending {
            None => {
                let w = click?;
                editor.pending = Some(inp.pick(w));
            }
            Some(c) if click.is_some() || typed.is_some() => {
                let ids = selected(editor, inp.sketch);
                let n = count(&editor.typed, 6);
                let mut s = inp.start();
                // 後から直せる円形状パターン(網羅版 C15。中心は点を押したらその点に付いて行く)
                let def = jc_sketch::CircularPattern {
                    entities: ids,
                    center: match c.point {
                        Some(p) => jc_sketch::PatternCenter::Point(p),
                        None => jc_sketch::PatternCenter::At(c.pos),
                    },
                    count: n,
                    spacing: jc_sketch::AngularSpacing::Total,
                    angle: TAU,
                    suppressed: vec![],
                };
                if s.add_circular_pattern(def).is_some() {
                    out = Some(s);
                }
                editor.cancel();
            }
            Some(_) => {}
        },
        Tool::Move => match editor.pending {
            None => editor.pending = Some(inp.pick(click?)),
            Some(a) => {
                let d = match (&typed, click) {
                    (Some(v), _) => typed_offset(v)?,
                    (None, Some(w)) => [w[0] - a.pos[0], w[1] - a.pos[1]],
                    _ => return None,
                };
                let mut s = inp.start();
                transform(
                    editor,
                    inp.sketch,
                    &mut s,
                    jc_sketch::Transform::Translate(d),
                );
                out = Some(s);
                editor.cancel();
            }
        },
        Tool::Rotate => match (editor.pending, editor.pending2) {
            (None, _) => editor.pending = Some(inp.pick(click?)),
            (Some(c), ref_dir) => {
                let angle = match (&typed, ref_dir, click) {
                    (Some(v), _, _) => v
                        .trim()
                        .trim_end_matches("deg")
                        .trim()
                        .parse::<f64>()
                        .ok()?
                        .to_radians(),
                    (None, None, Some(w)) => {
                        editor.pending2 = Some(inp.pick(w));
                        return None;
                    }
                    (None, Some(r), Some(w)) => {
                        let a0 = (r.pos[1] - c.pos[1]).atan2(r.pos[0] - c.pos[0]);
                        let a1 = (w[1] - c.pos[1]).atan2(w[0] - c.pos[0]);
                        a1 - a0
                    }
                    _ => return None,
                };
                let mut s = inp.start();
                let xf = jc_sketch::Transform::Rotate {
                    center: c.pos,
                    angle,
                };
                transform(editor, inp.sketch, &mut s, xf);
                out = Some(s);
                editor.cancel();
            }
        },
        Tool::Scale => match (editor.pending, editor.pending2) {
            (None, _) => editor.pending = Some(inp.pick(click?)),
            (Some(c), ref_pt) => {
                let k = match (&typed, ref_pt, click) {
                    (Some(v), _, _) => v.trim().parse::<f64>().ok().filter(|k| *k > 0.0)?,
                    (None, None, Some(w)) => {
                        editor.pending2 = Some(inp.pick(w));
                        return None;
                    }
                    (None, Some(r), Some(w)) => {
                        let d0 = dist(c.pos, r.pos);
                        if d0 < 1e-9 {
                            return None;
                        }
                        dist(c.pos, w) / d0
                    }
                    _ => return None,
                };
                let mut s = inp.start();
                let xf = jc_sketch::Transform::Scale { center: c.pos, k };
                transform(editor, inp.sketch, &mut s, xf);
                out = Some(s);
                editor.cancel();
            }
        },
        _ => {}
    }
    out
}

/// 選んだ要素を動かす・写す(網羅版 D6)。写す時は写しを選び直す。動かした後も、選んでいない
/// 要素への拘束で引き戻されるかもしれなければ知らせる。
fn transform(editor: &mut Editor, sketch: &Sketch, s: &mut Sketch, xf: jc_sketch::Transform) {
    let ids = selected(editor, sketch);
    if editor.copy_mode {
        let new = s.copy_transformed(&ids, xf);
        if !new.is_empty() {
            editor.selection = new.into_iter().collect();
        }
        return;
    }
    let links = if editor.detach {
        jc_sketch::Links::Detach
    } else {
        jc_sketch::Links::Follow
    };
    let moved = s.move_with(&ids, xf, links);
    if !moved.holding.is_empty() {
        let names: Vec<String> = moved.holding.iter().map(|c| c.to_string()).collect();
        editor.notice = Some(format!(
            "{} の拘束で元へ戻ることがあります(「つながりを切って動かす」で外せます)",
            names.join("、")
        ));
    }
}

/// 移動に打った「X, Y」の値(mm、負も可)。数だけなら文書の単位で読む。「mm」を外して数として
/// 読むと、typed_pair が単位を補った「2 in」や、打った「1 cm」を読めず、移動が黙って何もしない。
fn typed_offset(text: &str) -> Option<[f64; 2]> {
    let (x, y) = crate::view::typed_pair(text)?;
    Some([crate::view::length_mm(&x)?, crate::view::length_mm(&y)?])
}

/// 中心 `c` の円弧の折れ線を、`a` から `b` まで `m` を通る向きで。
fn ccw_arc(c: [f64; 2], a: [f64; 2], b: [f64; 2], m: [f64; 2]) -> Vec<[f64; 2]> {
    let a0 = (a[1] - c[1]).atan2(a[0] - c[0]);
    let ang = |p: [f64; 2]| ((p[1] - c[1]).atan2(p[0] - c[0]) - a0).rem_euclid(TAU);
    if ang(m) < ang(b) {
        arc_points(c, a, b)
    } else {
        arc_points(c, b, a)
    }
}

/// 円弧の長穴の両端の半円。`arc` は中心線の点の並び(中心 `c`)で、端ごとに円弧の外へふくらむ。
fn arc_slot_caps(arc: &[[f64; 2]], c: [f64; 2], half: f64, poly: &dyn Fn(&[[f64; 2]])) {
    let n = arc.len();
    if n < 2 {
        return;
    }
    for (x, next) in [(arc[0], arc[1]), (arc[n - 1], arc[n - 2])] {
        let l = dist(c, x).max(1e-9);
        let across = [(x[0] - c[0]) / l * half, (x[1] - c[1]) / l * half];
        poly(&slot_cap(x, across, [x[0] - next[0], x[1] - next[1]]));
    }
}

/// 描いている途中の見本(細い線)。
pub(crate) fn preview(
    painter: &egui::Painter,
    t: &Transform,
    editor: &Editor,
    sketch: &Sketch,
    sol: &Solution,
    w: [f64; 2],
    stroke: Stroke,
) {
    let line = |a: [f64; 2], b: [f64; 2]| {
        painter.line_segment([t.to_screen(a), t.to_screen(b)], stroke);
    };
    let poly = |pts: &[[f64; 2]]| {
        let v: Vec<Pos2> = pts.iter().map(|p| t.to_screen(*p)).collect();
        painter.add(egui::Shape::line(v, stroke));
    };
    let circle = |c: [f64; 2], r: f64| {
        t.circle(painter, c, r, stroke);
    };
    // 道具で選んだ線・円は太く
    let thick = Stroke::new(4.0, stroke.color.gamma_multiply(0.6));
    for id in &editor.picked {
        match sketch.entity(*id) {
            Some(Entity::Line { a, b, .. }) => {
                if let (Some(p), Some(q)) = (sol.points.get(a), sol.points.get(b)) {
                    painter.line_segment([t.to_screen(*p), t.to_screen(*q)], thick);
                }
            }
            Some(Entity::Circle { center, .. }) => {
                if let (Some(c), Some(r)) = (sol.points.get(center), sol.radii.get(id)) {
                    t.circle(painter, *c, *r, thick);
                }
            }
            Some(Entity::Arc {
                center, start, end, ..
            }) => {
                if let (Some(c), Some(a), Some(b)) = (
                    sol.points.get(center),
                    sol.points.get(start),
                    sol.points.get(end),
                ) {
                    let v: Vec<Pos2> = arc_points(*c, *a, *b)
                        .iter()
                        .map(|p| t.to_screen(*p))
                        .collect();
                    painter.add(egui::Shape::line(v, thick));
                }
            }
            _ => {}
        }
    }
    // 接する円の見本(相手を選び終えたら、カーソルの側に出る円)
    match (editor.tool, editor.picked.as_slice()) {
        (Tool::TwoTangentCircle, [p, q]) => {
            if let Some((c, r)) = sketch.circle_tangent_two_preview(sol, *p, *q, w) {
                circle(c, r);
            }
        }
        (Tool::TangentCircle, [p, q, r]) => {
            if let Some((c, rr)) = sketch.circle_tangent_three_preview(sol, [*p, *q, *r], w) {
                circle(c, rr);
            }
        }
        _ => {}
    }
    // スプライン: 置いた点とカーソルを通る見本(制御点なら点を結ぶ折れ線も)
    if matches!(editor.tool, Tool::FitSpline | Tool::ControlSpline) && !editor.spline_pts.is_empty()
    {
        let control = editor.tool == Tool::ControlSpline;
        let mut pts: Vec<[f64; 2]> = editor.spline_pts.iter().map(|p| p.pos).collect();
        pts.push(w);
        if control {
            poly(&pts);
        }
        let tol = (0.3 / t.scale as f64).max(1e-4);
        let curve =
            jc_sketch::spline_preview_degree(&pts, control, false, editor.spline_degree, tol);
        if curve.len() >= 2 {
            poly(&curve);
        }
        for p in &editor.spline_pts {
            painter.circle_stroke(t.to_screen(p.pos), 3.0, stroke);
        }
        return;
    }
    let Some(a) = editor.pending.map(|p| p.pos) else {
        return;
    };
    let b = editor.pending2.map(|p| p.pos);
    match editor.tool {
        Tool::ThreePointRectangle => match b {
            None => line(a, w),
            Some(b) => {
                let d = [b[0] - a[0], b[1] - a[1]];
                let l = d[0].hypot(d[1]).max(1e-9);
                let n = [-d[1] / l, d[0] / l];
                let h = (w[0] - a[0]) * n[0] + (w[1] - a[1]) * n[1];
                let o = [n[0] * h, n[1] * h];
                poly(&[
                    a,
                    b,
                    [b[0] + o[0], b[1] + o[1]],
                    [a[0] + o[0], a[1] + o[1]],
                    a,
                ]);
            }
        },
        Tool::TwoPointCircle => {
            let w = crate::view::aimed(editor, w);
            circle([(a[0] + w[0]) / 2.0, (a[1] + w[1]) / 2.0], dist(a, w) / 2.0)
        }
        Tool::ThreePointCircle => match b {
            None => line(a, w),
            Some(b) => {
                if let Some(c) = jc_sketch::circumcenter(a, b, w) {
                    circle(c, dist(c, a));
                }
            }
        },
        Tool::ThreePointArc => match b {
            None => line(a, w),
            Some(b) => {
                if let Some(c) = jc_sketch::circumcenter(a, b, w) {
                    let a0 = (a[1] - c[1]).atan2(a[0] - c[0]);
                    let ang = |p: [f64; 2]| ((p[1] - c[1]).atan2(p[0] - c[0]) - a0).rem_euclid(TAU);
                    let pts = if ang(w) < ang(b) {
                        arc_points(c, a, b)
                    } else {
                        arc_points(c, b, a)
                    };
                    poly(&pts);
                }
            }
        },
        Tool::Ellipse => match b {
            None => line(a, w),
            Some(b) => {
                line(a, b);
                let tol = (0.3 / t.scale as f64).max(1e-4);
                poly(&jc_sketch::ellipse_preview(a, b, w, tol));
            }
        },
        Tool::Conic => match b {
            None => line(a, w),
            Some(b) => {
                line(a, w);
                line(w, b);
                let tol = (0.3 / t.scale as f64).max(1e-4);
                poly(&jc_sketch::conic_preview(
                    a,
                    b,
                    w,
                    conic_rho(&editor.typed),
                    tol,
                ));
            }
        },
        Tool::MidpointLine => line([2.0 * a[0] - w[0], 2.0 * a[1] - w[1]], w),
        Tool::EdgePolygon => match b {
            None => line(a, w),
            Some(b) => {
                let n = count(&editor.typed, editor.polygon_n.clamp(3, 64)).max(3);
                let mut pts = jc_sketch::edge_polygon_preview(a, b, n, w);
                if let Some(f) = pts.first().copied() {
                    pts.push(f);
                }
                poly(&pts);
            }
        },
        Tool::ThreePointArcSlot => match (b, editor.pending3.map(|p| p.pos)) {
            (None, _) => line(a, w),
            (Some(b), None) => {
                if let Some(c) = jc_sketch::circumcenter(a, b, w) {
                    poly(&ccw_arc(c, a, b, w));
                }
            }
            (Some(b), Some(m)) => {
                if let Some(c) = jc_sketch::circumcenter(a, b, m) {
                    let r = dist(c, a);
                    let half = (dist(c, w) - r).abs();
                    for rr in [r + half, r - half] {
                        let k = rr / r.max(1e-9);
                        let sc = |p: [f64; 2]| [c[0] + (p[0] - c[0]) * k, c[1] + (p[1] - c[1]) * k];
                        poly(&ccw_arc(c, sc(a), sc(b), sc(m)));
                    }
                    arc_slot_caps(&ccw_arc(c, a, b, m), c, half, &poly);
                }
            }
        },
        Tool::OuterPolygon => {
            let n = count(&editor.typed, 6).max(3);
            let ap = dist(a, w);
            let big = ap / (PI / n as f64).cos();
            let a0 = (w[1] - a[1]).atan2(w[0] - a[0]) - PI / n as f64;
            let pts: Vec<[f64; 2]> = (0..=n)
                .map(|k| {
                    let t = a0 + TAU * k as f64 / n as f64;
                    [a[0] + big * t.cos(), a[1] + big * t.sin()]
                })
                .collect();
            poly(&pts);
            circle(a, ap);
        }
        Tool::OverallSlot | Tool::CenterSlot => {
            let (c1, c2, half) = match b {
                None => {
                    let b = if editor.tool == Tool::CenterSlot {
                        [2.0 * a[0] - w[0], 2.0 * a[1] - w[1]]
                    } else {
                        a
                    };
                    line(b, w);
                    return;
                }
                Some(b) => (a, b, half_width(a, b, w)),
            };
            let (p, q) = if editor.tool == Tool::OverallSlot {
                let d = [c2[0] - c1[0], c2[1] - c1[1]];
                let l = d[0].hypot(d[1]).max(1e-9);
                let u = [d[0] / l * half, d[1] / l * half];
                ([c1[0] + u[0], c1[1] + u[1]], [c2[0] - u[0], c2[1] - u[1]])
            } else {
                ([2.0 * c1[0] - c2[0], 2.0 * c1[1] - c2[1]], c2)
            };
            let d = [q[0] - p[0], q[1] - p[1]];
            let l = d[0].hypot(d[1]).max(1e-9);
            let n = [-d[1] / l * half, d[0] / l * half];
            for k in [1.0, -1.0] {
                line(
                    [p[0] + k * n[0], p[1] + k * n[1]],
                    [q[0] + k * n[0], q[1] + k * n[1]],
                );
            }
            poly(&slot_cap(p, n, [p[0] - q[0], p[1] - q[1]]));
            poly(&slot_cap(q, n, [q[0] - p[0], q[1] - p[1]]));
        }
        Tool::ArcSlot => match (b, editor.pending3.map(|p| p.pos)) {
            (None, _) => line(a, w),
            (Some(s0), None) => {
                let r = dist(a, s0);
                let e = {
                    let g = (w[1] - a[1]).atan2(w[0] - a[0]);
                    [a[0] + r * g.cos(), a[1] + r * g.sin()]
                };
                poly(&arc_points(a, s0, e));
            }
            (Some(s0), Some(e)) => {
                let r = dist(a, s0);
                let half = (dist(a, w) - r).abs();
                let e = {
                    let g = (e[1] - a[1]).atan2(e[0] - a[0]);
                    [a[0] + r * g.cos(), a[1] + r * g.sin()]
                };
                for rr in [r + half, r - half] {
                    let k = rr / r.max(1e-9);
                    let sc = |p: [f64; 2]| [a[0] + (p[0] - a[0]) * k, a[1] + (p[1] - a[1]) * k];
                    poly(&arc_points(a, sc(s0), sc(e)));
                }
                arc_slot_caps(&arc_points(a, s0, e), a, half, &poly);
            }
        },
        Tool::LinearPattern => {
            let n = count(&editor.typed, 3);
            let d = [w[0] - a[0], w[1] - a[1]];
            for k in 1..n {
                let p = [a[0] + d[0] * k as f64, a[1] + d[1] * k as f64];
                painter.circle_stroke(t.to_screen(p), 5.0, stroke);
            }
            line(
                a,
                [a[0] + d[0] * (n - 1) as f64, a[1] + d[1] * (n - 1) as f64],
            );
        }
        Tool::CircularPattern => {
            circle(a, dist(a, w));
            painter.circle_stroke(t.to_screen(a), 5.0, stroke);
        }
        Tool::Move => {
            line(a, w);
            // 動かす物の見本(選んだ線・円・円弧を、基準からカーソルまでずらした所に)
            let d = [w[0] - a[0], w[1] - a[1]];
            let at = |id: &EntityId| sol.points.get(id).map(|p| [p[0] + d[0], p[1] + d[1]]);
            for id in &editor.selection {
                match sketch.entity(*id) {
                    Some(Entity::Line { a, b, .. }) => {
                        if let (Some(p), Some(q)) = (at(a), at(b)) {
                            line(p, q);
                        }
                    }
                    Some(Entity::Circle { center, .. }) => {
                        if let (Some(c), Some(r)) = (at(center), sol.radii.get(id)) {
                            circle(c, *r);
                        }
                    }
                    Some(Entity::Arc {
                        center, start, end, ..
                    }) => {
                        if let (Some(c), Some(p), Some(q)) = (at(center), at(start), at(end)) {
                            poly(&arc_points(c, p, q));
                        }
                    }
                    _ => {}
                }
            }
        }
        Tool::Rotate | Tool::Scale => {
            if let Some(b) = b {
                line(a, b);
            }
            line(a, w);
        }
        _ => {}
    }
}

#[cfg(test)]
mod offset_tests {
    use super::*;

    #[test]
    fn a_typed_move_reads_units_and_the_document_unit() {
        assert_eq!(typed_offset("3, -2"), Some([3.0, -2.0]));
        assert_eq!(typed_offset("1 cm, 0"), Some([10.0, 0.0]));
        assert_eq!(typed_offset("1 in, -1 in"), Some([25.4, -25.4]));
        assert_eq!(typed_offset("x, 1"), None);
    }
}
