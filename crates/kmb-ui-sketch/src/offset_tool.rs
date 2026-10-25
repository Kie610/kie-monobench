//! オフセットの道具(O): 線を 1 本クリックすると端でつながった
//! 全体を選び(「チェーン選択」を外すとその 1 本だけ)、カーソルの側へずらした見本と距離の欄を出す。値を打って Enter か、クリックで作る。
//! 片側・両側・対称と、角の作り方(辺を延ばす・角を丸める)は右クリックかパネルで切り替える。
//! 作った後は選ぶ道具へ戻る(選びが新しい線になる)。

use crate::infer::shape;
use crate::view::{Editor, Pick, Transform};
use egui::{Pos2, Stroke};
use kmb_sketch::{Entity, EntityId, Sketch, Solution};

/// ずらす向き。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Mode {
    /// カーソルの側へ
    #[default]
    One,
    /// 両側へ同じ距離
    Both,
    /// 両側へ距離の半分ずつ
    Symmetric,
}

impl Mode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Mode::One => "片側",
            Mode::Both => "両側",
            Mode::Symmetric => "対称",
        }
    }
}

/// 角の作り方(角を丸めるか)と名前。
pub(crate) const CORNERS: [(bool, &str); 2] = [
    (false, "角: 辺を延ばす"),
    (true, "角: 丸める(角の点から等距離)"),
];

/// 道具の状態。
#[derive(Clone, Default)]
pub(crate) struct State {
    /// ずらす連なり(選んだ線と、端でつながった線の全部)
    pub chain: Vec<EntityId>,
    pub mode: Mode,
    /// ずらすと開く角を、元の角の点を中心にした円弧で丸める(偽なら辺を延ばして角を作る)
    pub round: bool,
    /// 押した線だけをずらす(偽なら端でつながった全体)
    pub single: bool,
    /// 押した線(か、道具を始めた時に選んでいた線)。「チェーン選択」を切り替えたら、ここから選び直す
    picked: Vec<EntityId>,
    /// 「チェーン選択」を切り替えたので、押した線から連なりを選び直す
    pub rechain: bool,
    /// 見本の作り置き(連なり、距離、外か、丸めるか)→ 足した線の形
    cache: Option<Cache>,
}

/// 見本の作り置き(連なり、距離を 1e-4 mm 単位にした数、外か、丸めるか、足した線の形)。
type Cache = (Vec<EntityId>, u64, bool, bool, Vec<Seg>);

impl State {
    /// ずらす連なりを捨てる(向きの選びは残す)。
    pub(crate) fn clear(&mut self) {
        self.chain.clear();
        self.picked.clear();
        self.cache = None;
    }
}

/// 見本に描く形。
#[derive(Clone, Copy, Debug)]
enum Seg {
    Line([f64; 2], [f64; 2]),
    Arc([f64; 2], [f64; 2], [f64; 2]),
    Circle([f64; 2], f64),
}

/// 選んだ線から、ずらす連なり(端でつながった線分・円弧の全体と、円)。`single` なら選んだ線だけ。
pub(crate) fn chain_of(sketch: &Sketch, ids: &[EntityId], single: bool) -> Vec<EntityId> {
    let mut whole: Vec<EntityId> = Vec::new();
    for id in ids {
        if matches!(sketch.entity(*id), Some(Entity::Point { .. }) | None) {
            continue;
        }
        let mut c = if single {
            Vec::new()
        } else {
            sketch.chain_of(*id)
        };
        if c.is_empty() {
            c.push(*id);
        }
        for e in c {
            if !whole.contains(&e) {
                whole.push(e);
            }
        }
    }
    whole
}

/// カーソルから連なりまでの最も近い距離と、その点。
pub(crate) fn nearest(
    sketch: &Sketch,
    sol: &Solution,
    chain: &[EntityId],
    w: [f64; 2],
) -> Option<(f64, [f64; 2])> {
    chain
        .iter()
        .filter_map(|id| shape(sketch, sol, *id))
        .map(|s| {
            let p = s.nearest(w);
            ((p[0] - w[0]).hypot(p[1] - w[1]), p)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// 距離の式(数なら mm を付ける)と、形を作る距離(mm)。
fn distance(editor: &Editor, live: f64) -> (String, f64) {
    match editor.hud.dim("距離") {
        Some(v) => {
            let d = crate::view::typed_length(&v).unwrap_or(live);
            (v, d)
        }
        None => {
            let d = (live * 100.0).round() / 100.0;
            (format!("{d} mm"), d)
        }
    }
}

/// 半分の式(対称の時)。
fn half(value: &str) -> String {
    match crate::view::typed_length(value) {
        Some(v) => format!("{} mm", v / 2.0),
        None => format!("({value}) / 2"),
    }
}

/// 作った線を返し、作らなかった理由を `why` に残す([`Sketch::offset_corners_why`])。
pub(crate) fn keep(
    (made, reason): (Vec<EntityId>, Option<&'static str>),
    why: &mut Option<&'static str>,
) -> Vec<EntityId> {
    if reason.is_some() {
        *why = reason;
    }
    made
}

/// ずらした写しを足したスケッチと、足した線(作れなければ空)。
fn made(
    sketch: &Sketch,
    sol: &Solution,
    chain: &[EntityId],
    d: f64,
    value: &str,
    outward: bool,
    round: bool,
) -> (Sketch, Vec<EntityId>) {
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    let new = s.offset_corners(chain, d, value, outward, round);
    (s, new)
}

/// 足した線の形(保存されている位置。見本に描く)。
fn segs(s: &Sketch, new: &[EntityId]) -> Vec<Seg> {
    let pos = |id: &EntityId| match s.entity(*id) {
        Some(Entity::Point { x, y, .. }) => Some([*x, *y]),
        _ => None,
    };
    new.iter()
        .filter_map(|id| match s.entity(*id)? {
            Entity::Line { a, b, .. } => Some(Seg::Line(pos(a)?, pos(b)?)),
            Entity::Arc {
                center, start, end, ..
            } => Some(Seg::Arc(pos(center)?, pos(start)?, pos(end)?)),
            Entity::Circle { center, radius, .. } => Some(Seg::Circle(pos(center)?, *radius)),
            // 楕円・スプラインのオフセット(フィット点のスプライン)は見本に描かない
            _ => None,
        })
        .collect()
}

fn seg_distance(s: &Seg, w: [f64; 2]) -> f64 {
    match *s {
        Seg::Line(a, b) => crate::infer::Shape::Seg(a, b).nearest(w),
        Seg::Arc(c, st, e) => {
            let a0 = (st[1] - c[1])
                .atan2(st[0] - c[0])
                .rem_euclid(std::f64::consts::TAU);
            let a1 = (e[1] - c[1])
                .atan2(e[0] - c[0])
                .rem_euclid(std::f64::consts::TAU);
            let span = (a1 - a0).rem_euclid(std::f64::consts::TAU);
            let r = (st[0] - c[0]).hypot(st[1] - c[1]);
            crate::infer::Shape::Arc(c, r, a0, span).nearest(w)
        }
        Seg::Circle(c, r) => crate::infer::Shape::Arc(c, r, 0.0, std::f64::consts::TAU).nearest(w),
    }
    .iter()
    .zip(w)
    .map(|(p, q)| (p - q).powi(2))
    .sum::<f64>()
    .sqrt()
}

/// カーソルの側(外へずらした方が近ければ真)と、その側へ距離 `d` ずらした形。
/// 側はカーソルまでの距離 `live` でずらした両方の写しを比べて決める(カーソルの側の写しは
/// カーソルを通る)。打った距離で比べると、内へずらしすぎて裏返った写しが近く見えることがある。
fn outward_near(
    st: &mut State,
    sketch: &Sketch,
    sol: &Solution,
    d: f64,
    live: f64,
    w: [f64; 2],
) -> Option<(bool, Vec<Seg>)> {
    let round = st.round;
    let near = |out: bool, at: f64| -> Option<(f64, Vec<Seg>)> {
        let (s, new) = made(sketch, sol, &st.chain, at, &format!("{at} mm"), out, round);
        let shapes = segs(&s, &new);
        let dist = shapes
            .iter()
            .map(|s| seg_distance(s, w))
            .fold(f64::INFINITY, f64::min);
        (!shapes.is_empty()).then_some((dist, shapes))
    };
    let probe = live.max(1e-3);
    let out = match (near(true, probe), near(false, probe)) {
        (Some(o), Some(i)) => o.0 <= i.0,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => return None,
    };
    let key = (d * 1e4).round() as u64;
    let shapes = match &st.cache {
        Some((c, k, o, r, sh)) if *c == st.chain && *k == key && *o == out && *r == round => {
            sh.clone()
        }
        _ => near(out, d).map(|n| n.1).unwrap_or_default(),
    };
    st.cache = Some((st.chain.clone(), key, out, round, shapes.clone()));
    Some((out, shapes))
}

/// 道具の操作。作ったスケッチを返す(作った後は道具を終え、選びを新しい線にする)。
pub(crate) fn interact(
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    cursor: Option<[f64; 2]>,
    clicked: bool,
    enter: bool,
    tol: f64,
) -> Option<Sketch> {
    // 「チェーン選択」を切り替えたら、押した線から選び直す
    if std::mem::take(&mut editor.offset.rechain) && !editor.offset.picked.is_empty() {
        let picked = editor.offset.picked.clone();
        editor.offset.chain = chain_of(sketch, &picked, editor.offset.single);
        editor.offset.cache = None;
        editor.selection = editor.offset.chain.iter().copied().collect();
    }
    // 道具を始めた時に選んでいた線があれば、それをずらす
    if editor.offset.chain.is_empty() && !editor.selection.is_empty() {
        let sel: Vec<EntityId> = editor.selection.iter().copied().collect();
        editor.offset.picked = sel.clone();
        editor.offset.chain = chain_of(sketch, &sel, editor.offset.single);
        if let (false, Some(w)) = (editor.offset.chain.is_empty(), cursor) {
            editor.pending = Some(Pick {
                point: None,
                pos: w,
            });
        }
    }
    if editor.offset.chain.is_empty() {
        let w = cursor.filter(|_| clicked)?;
        let h = sketch
            .hits(sol, w[0], w[1], tol)
            .into_iter()
            .find(|h| !matches!(sketch.entity(h.entity), Some(Entity::Point { .. }) | None))?;
        editor.offset.picked = vec![h.entity];
        editor.offset.chain = chain_of(sketch, &[h.entity], editor.offset.single);
        editor.selection = editor.offset.chain.iter().copied().collect();
        editor.pending = Some(Pick {
            point: None,
            pos: w,
        });
        return None;
    }
    if editor.pending.is_none()
        && let Some(w) = cursor
    {
        editor.pending = Some(Pick {
            point: None,
            pos: w,
        });
    }
    if !(clicked || enter) {
        return None;
    }
    let w = cursor.or(editor.pending.map(|p| p.pos))?;
    let live = nearest(sketch, sol, &editor.offset.chain, w).map_or(2.0, |(d, _)| d);
    let (value, d) = distance(editor, live);
    if d <= 1e-6 {
        editor.notice = Some("距離が 0 なのでオフセットできません".into());
        return None;
    }
    let chain = editor.offset.chain.clone();
    let (mode, round) = (editor.offset.mode, editor.offset.round);
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    let mut why = None;
    let new = match mode {
        Mode::One => {
            let mut st = editor.offset.clone();
            let out = outward_near(&mut st, sketch, sol, d, live, w).is_none_or(|(o, _)| o);
            keep(
                s.offset_corners_why(&chain, d, &value, out, round),
                &mut why,
            )
        }
        Mode::Both | Mode::Symmetric => {
            let (dd, vv) = if mode == Mode::Both {
                (d, value.clone())
            } else {
                (d / 2.0, half(&value))
            };
            let mut both = s.offset_corners(&chain, dd, &vv, true, round);
            let inner = keep(
                s.offset_corners_why(&chain, dd, &vv, false, round),
                &mut why,
            );
            if inner.is_empty() && !both.is_empty() {
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
    editor.offset = State {
        mode,
        round,
        ..State::default()
    };
    Some(s)
}

/// 見本(ずらした形)と、案内に使う距離の札の位置。
pub(crate) fn preview(
    painter: &egui::Painter,
    t: &Transform,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    w: [f64; 2],
    stroke: Stroke,
) -> Option<(Pos2, f64)> {
    if editor.offset.chain.is_empty() {
        return None;
    }
    let (live, at) = nearest(sketch, sol, &editor.offset.chain, w)?;
    let (_, d) = distance(editor, live);
    if d <= 1e-6 {
        return Some((t.to_screen(w), live));
    }
    let mode = editor.offset.mode;
    let draw = |shapes: &[Seg]| {
        for s in shapes {
            match *s {
                Seg::Line(a, b) => {
                    painter.line_segment([t.to_screen(a), t.to_screen(b)], stroke);
                }
                Seg::Arc(c, st, e) => {
                    let pts: Vec<Pos2> = crate::view::arc_points(c, st, e)
                        .into_iter()
                        .map(|p| t.to_screen(p))
                        .collect();
                    painter.add(egui::Shape::line(pts, stroke));
                }
                Seg::Circle(c, r) => {
                    t.circle(painter, c, r, stroke);
                }
            }
        }
    };
    match mode {
        Mode::One => {
            let mut st = editor.offset.clone();
            if let Some((_, shapes)) = outward_near(&mut st, sketch, sol, d, live, w) {
                draw(&shapes);
            }
            editor.offset.cache = st.cache;
        }
        Mode::Both | Mode::Symmetric => {
            let dd = if mode == Mode::Both { d } else { d / 2.0 };
            for out in [true, false] {
                let (s, new) = made(
                    sketch,
                    sol,
                    &editor.offset.chain,
                    dd,
                    "1 mm",
                    out,
                    editor.offset.round,
                );
                draw(&segs(&s, &new));
            }
        }
    }
    // 矢印: 連なりの近い点からカーソルの向きへ
    let (a, b) = (t.to_screen(at), t.to_screen(w));
    painter.arrow(a, b - a, Stroke::new(1.2, stroke.color));
    Some((Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0), live))
}
