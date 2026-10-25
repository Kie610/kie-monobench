//! 道具が使う形の計算(打った値、向き、円弧、トリムの区間など)。

use super::*;

/// 打ち込んだ寸法の値(mm)。数だけ(`25`、`25 mm`、`1 in`)なら読み、式やパラメータなら `None`
/// (その時は寸法の拘束だけを付け、形は解き直しで合わせる)。
pub(crate) fn typed_length(text: &str) -> Option<f64> {
    length_mm(&typed_value(text)).filter(|v| *v > 0.0)
}

/// 「幅, 高さ」と打ち込んだ 2 つの寸法(「×」でも区切れる)。2 つそろわなければ `None`。
pub(crate) fn typed_pair(text: &str) -> Option<(String, String)> {
    let (a, b) = text.split_once([',', '、', '×'])?;
    let (a, b) = (a.trim(), b.trim());
    (!a.is_empty() && !b.is_empty()).then(|| (typed_value(a), typed_value(b)))
}

/// 打ち込んだ寸法を寸法の式にする。数だけなら文書の単位を付ける(`25` → `25 mm`、インチの文書は `25 in`)。
pub(crate) fn typed_value(text: &str) -> String {
    match split_length(text) {
        Some((n, u)) => format!("{n} {}", u.name()),
        None => text.trim().to_string(),
    }
}

/// 数と長さの単位(`25`、`25 mm`、`1in`)に分ける。単位が無ければ文書の単位(候補 19)。
fn split_length(text: &str) -> Option<(&str, jc_core::LengthUnit)> {
    use jc_core::LengthUnit;
    let t = text.trim();
    // m は mm・cm の後ろに当てる
    for u in [
        LengthUnit::Mm,
        LengthUnit::Cm,
        LengthUnit::In,
        LengthUnit::Ft,
        LengthUnit::M,
    ] {
        if let Some(n) = t.strip_suffix(u.name()).map(str::trim)
            && plain_number(n).is_some()
        {
            return Some((n, u));
        }
    }
    plain_number(t)
        .is_some()
        .then(|| (t, jc_core::display_unit()))
}

/// 単位の無い 1 つの数(`-2.5`、`1e3`)。式の読み方と同じく、全角の数字・小数点・符号も受ける。
/// `10-2` のような式は数でない(後ろに単位を足すと意味が変わる)。
fn plain_number(text: &str) -> Option<f64> {
    let sign = |c: char| matches!(c, '+' | '-' | '＋' | '－' | '−');
    let mut prev = None;
    for (i, c) in text.chars().enumerate() {
        let digit = c.is_ascii_digit() || ('０'..='９').contains(&c) || matches!(c, '.' | '．');
        let ok = digit
            || matches!(c, 'e' | 'E') && i > 0
            || sign(c) && (i == 0 || matches!(prev, Some('e' | 'E')));
        if !ok {
            return None;
        }
        prev = Some(c);
    }
    jc_core::eval_str(text, &|_| None)
        .ok()
        .filter(|q| q.dim.is_none())
        .map(|q| q.value)
}

/// 角を丸める・落とす道具で何も打たなかった時の値(設定の単位で書く)。mm の文書は 2 mm、
/// インチ・フィートの文書は切りの良い 1/16 in。
pub(crate) fn default_corner() -> String {
    use jc_core::LengthUnit;
    match jc_core::display_unit() {
        LengthUnit::In | LengthUnit::Ft => "0.0625 in".into(),
        u => u.show(jc_core::Quantity::mm(2.0)),
    }
}

/// 長さ(mm)を設定の単位で書く(知らせの文に入れる)。
pub(crate) fn shown_length(mm: f64) -> String {
    jc_core::display_unit().show(jc_core::Quantity::mm(mm))
}

/// 数と長さの単位だけの式(`25 mm`、`0.5 in`)の値(mm)。単位が無ければ文書の単位で読む。
pub(crate) fn length_mm(text: &str) -> Option<f64> {
    let (n, u) = split_length(text)?;
    let v = plain_number(n)? * u.mm();
    v.is_finite().then_some(v)
}

/// 固定した寸法の値に合わせたカーソルの位置(描いている道具の次の点)。固定していない値は
/// カーソルで決まる。線の角度は X 軸から反時計回りに測る。
pub(crate) fn aimed(editor: &Editor, w: [f64; 2]) -> [f64; 2] {
    let h = &editor.hud;
    let Some(start) = editor.pending else {
        return w;
    };
    let s = start.pos;
    let (dx, dy) = (w[0] - s[0], w[1] - s[1]);
    let d = dx.hypot(dy);
    let u = if d > 1e-9 {
        [dx / d, dy / d]
    } else {
        [1.0, 0.0]
    };
    let along = |len: f64| [s[0] + u[0] * len, s[1] + u[1] * len];
    match (editor.tool, editor.step()) {
        (Tool::Line, _) => match (h.fixed("長さ"), h.fixed("角度")) {
            (len, Some(deg)) => {
                // つなげて描く時は前の線分から、カーソルのある側へ回して測る
                let a = match editor.line_base {
                    Some(b) => {
                        let side = (b.cos() * dy - b.sin() * dx).signum();
                        b + if side == 0.0 { 1.0 } else { side } * deg.to_radians()
                    }
                    None => deg.to_radians(),
                };
                let (sin, cos) = a.sin_cos();
                let l = len.unwrap_or((dx * cos + dy * sin).max(0.0));
                [s[0] + cos * l, s[1] + sin * l]
            }
            (Some(len), None) => along(len),
            (None, None) => w,
        },
        (Tool::Circle, 1) => h.fixed("直径").map_or(w, |v| along(v / 2.0)),
        // 2 点の円: 打った直径だけ、最初の端からカーソルの向きへ
        (Tool::TwoPointCircle, 1) => h.fixed("直径").map_or(w, along),
        (Tool::Rectangle | Tool::CenterRectangle | Tool::RoundedRectangle, 1) => {
            let k = if editor.tool != Tool::CenterRectangle {
                1.0
            } else {
                0.5
            };
            let sx = if dx < 0.0 { -1.0 } else { 1.0 };
            let sy = if dy < 0.0 { -1.0 } else { 1.0 };
            [
                h.fixed("幅").map_or(w[0], |v| s[0] + sx * v * k),
                h.fixed("高さ").map_or(w[1], |v| s[1] + sy * v * k),
            ]
        }
        (Tool::Arc, 1) | (Tool::Slot, 1) => {
            let label = if editor.tool == Tool::Arc {
                "半径"
            } else {
                "長さ"
            };
            h.fixed(label).map_or(w, along)
        }
        (Tool::Arc, 2) => match (h.fixed("角度"), editor.pending2) {
            (Some(deg), Some(st)) => {
                let r = (st.pos[0] - s[0]).hypot(st.pos[1] - s[1]);
                let a0 = (st.pos[1] - s[1]).atan2(st.pos[0] - s[0]);
                let sign = if arc_ccw(editor) { 1.0 } else { -1.0 };
                let a = a0 + sign * deg.to_radians();
                [s[0] + r * a.cos(), s[1] + r * a.sin()]
            }
            _ => w,
        },
        (Tool::Slot, 2) => match (h.fixed("幅"), editor.pending2) {
            (Some(width), Some(b)) => {
                // 中心を結ぶ線からカーソルの側へ、幅の半分だけ離した位置
                let d = [b.pos[0] - s[0], b.pos[1] - s[1]];
                let l = d[0].hypot(d[1]).max(1e-9);
                let n = [-d[1] / l, d[0] / l];
                let side = if dx * n[0] + dy * n[1] < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let half = width / 2.0;
                [s[0] + side * n[0] * half, s[1] + side * n[1] * half]
            }
            _ => w,
        },
        (Tool::Polygon, 1) => h.fixed("半径").map_or(w, along),
        _ => w,
    }
}

/// 円弧を反時計回りに描くか(始点からカーソルを動かした向き。まだ動かしていなければ反時計回り)。
pub(crate) fn arc_ccw(editor: &Editor) -> bool {
    editor.arc_turn.is_none_or(|(_, sweep)| sweep >= 0.0)
}

/// 長穴の幅の半分(カーソル `w` から、中心 `a`・`b` を結ぶ線までの距離)。
pub(crate) fn slot_half(a: [f64; 2], b: [f64; 2], w: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]).max(1e-9);
    ((w[0] - a[0]) * d[1] - (w[1] - a[1]) * d[0]).abs() / l
}

/// 長穴の端の半円(描いている途中の見本)。端の中心 `x` から幅の向きに `across`(長さは幅の半分)
/// だけ離れた 2 点を結び、`away` の側へふくらむ。できあがりの形と同じく、丸ごとの円にしない。
pub(crate) fn slot_cap(x: [f64; 2], across: [f64; 2], away: [f64; 2]) -> Vec<[f64; 2]> {
    let p = [x[0] + across[0], x[1] + across[1]];
    let q = [x[0] - across[0], x[1] - across[1]];
    let pts = arc_points(x, p, q);
    let m = pts[pts.len() / 2];
    if (m[0] - x[0]) * away[0] + (m[1] - x[1]) * away[1] >= 0.0 {
        pts
    } else {
        arc_points(x, q, p)
    }
}

/// 多角形の辺の数(欄に打った値。なければ 6)。
pub(crate) fn polygon_sides(editor: &Editor) -> usize {
    editor
        .hud
        .fixed("辺の数")
        .map(|n| n.round() as usize)
        .filter(|n| (3..=64).contains(n))
        .unwrap_or(editor.polygon_n.clamp(3, 64))
}

/// 2 本の線分が共有する端の点。
pub(crate) fn shared_end(sketch: &Sketch, a: EntityId, b: EntityId) -> Option<EntityId> {
    let ends = |id| match sketch.entity(id) {
        Some(Entity::Line { a, b, .. }) => Some([*a, *b]),
        _ => None,
    };
    let (p, q) = (ends(a)?, ends(b)?);
    p.into_iter().find(|x| q.contains(x))
}

/// なぞった道 `path` が通る区間を、順にトリムする(網羅版 D1)。1 つも消せなければ `None`。
/// 消すたびに解き直して、次の点の下の区間を探す。
pub(crate) fn trim_along(
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
    path: &[[f64; 2]],
    tol: f64,
) -> Option<Sketch> {
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    let mut cur = sol.clone();
    let mut any = false;
    for w in path {
        let hit = s
            .hits(&cur, w[0], w[1], tol * 0.5)
            .into_iter()
            .find(|h| !matches!(s.entity(h.entity), Some(Entity::Point { .. }) | None));
        let Some(h) = hit else { continue };
        let mut next = s.clone();
        if !next.trim(h.entity, *w) {
            continue;
        }
        let empty = HashMap::new();
        match jc_sketch::solve(&next, values.unwrap_or(&empty)) {
            Ok(sol2) => {
                next.adopt_solution(&sol2);
                s = next;
                cur = sol2;
                any = true;
            }
            Err(_) => break,
        }
    }
    any.then_some(s)
}

/// 点 `p` で向き `dir`(rad)に接し、`e` を通る円弧の折れ線(見本用)。まっすぐなら `None`。
pub(crate) fn tangent_arc_points(p: [f64; 2], dir: f64, e: [f64; 2]) -> Option<Vec<[f64; 2]>> {
    let n = [-dir.sin(), dir.cos()];
    let pe = [p[0] - e[0], p[1] - e[1]];
    let k = n[0] * pe[0] + n[1] * pe[1];
    let q = pe[0] * pe[0] + pe[1] * pe[1];
    if k.abs() < 1e-9 || q < 1e-12 {
        return None;
    }
    let r = -q / (2.0 * k);
    let c = [p[0] + n[0] * r, p[1] + n[1] * r];
    let a0 = (p[1] - c[1]).atan2(p[0] - c[0]);
    let a1 = (e[1] - c[1]).atan2(e[0] - c[0]);
    // 左へ曲がる(r > 0)なら反時計回り
    let mut sweep = a1 - a0;
    if r > 0.0 {
        sweep = sweep.rem_euclid(std::f64::consts::TAU);
    } else {
        sweep = -(-sweep).rem_euclid(std::f64::consts::TAU);
    }
    let ra = r.abs();
    Some(
        (0..=48)
            .map(|k| {
                let a = a0 + sweep * k as f64 / 48.0;
                [c[0] + ra * a.cos(), c[1] + ra * a.sin()]
            })
            .collect(),
    )
}

/// 点 `p` を端に持つ線分(最後に描いたもの)。
/// 点 `p` で終わる線分か円弧(新しいものから)。接線円弧を続ける元。
pub(crate) fn curve_ending_at(sketch: &Sketch, p: EntityId) -> Option<EntityId> {
    sketch.entities.iter().rev().find_map(|e| match e {
        Entity::Line { id, a, b, .. } if *a == p || *b == p => Some(*id),
        Entity::Arc { id, start, end, .. } if *start == p || *end == p => Some(*id),
        _ => None,
    })
}

pub(crate) fn line_ending_at(sketch: &Sketch, p: EntityId) -> Option<EntityId> {
    sketch.entities.iter().rev().find_map(|e| match e {
        Entity::Line { id, a, b, .. } if *a == p || *b == p => Some(*id),
        _ => None,
    })
}

/// 円弧を折れ線にする(反時計回り)
pub(crate) fn arc_points(c: [f64; 2], s: [f64; 2], e: [f64; 2]) -> Vec<[f64; 2]> {
    let r = (s[0] - c[0]).hypot(s[1] - c[1]);
    let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
    let mut a1 = (e[1] - c[1]).atan2(e[0] - c[0]);
    if a1 <= a0 {
        a1 += std::f64::consts::TAU;
    }
    let n = 48;
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// 角の丸い長方形の半径(mm)。欄に打った値か、打たなければ短い辺の 1/5(
/// 寸法の値にするので 0.01 mm に丸める)。
pub(crate) fn rounded_radius(editor: &Editor, p: [f64; 2], q: [f64; 2]) -> f64 {
    editor.hud.fixed("角の半径").unwrap_or_else(|| {
        let short = (q[0] - p[0]).abs().min((q[1] - p[1]).abs());
        (short / 5.0 * 100.0).round() / 100.0
    })
}

/// mm の値を寸法の式の数にする(小数 2 桁まで、末尾の 0 は落とす)。
pub(crate) fn round_mm(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 角を半径 `r` で丸めた矩形(対角の隅 `p`・`q`)の外形を、予告に描く折れ線にする。
pub(crate) fn rounded_outline(p: [f64; 2], q: [f64; 2], r: f64) -> Vec<[f64; 2]> {
    let (x0, x1) = (p[0].min(q[0]), p[0].max(q[0]));
    let (y0, y1) = (p[1].min(q[1]), p[1].max(q[1]));
    // 角の中心と、円弧の始まりの角度(反時計回りに 90 度)
    let corners = [
        ([x1 - r, y0 + r], -90f64),
        ([x1 - r, y1 - r], 0.0),
        ([x0 + r, y1 - r], 90.0),
        ([x0 + r, y0 + r], 180.0),
    ];
    let mut out = Vec::new();
    for (c, a0) in corners {
        for k in 0..=8 {
            let a = (a0 + 90.0 * f64::from(k) / 8.0).to_radians();
            out.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
        }
    }
    out.push(out[0]);
    out
}

/// 引いた点 `point` を `w` で離した時の相手(K21)。スケッチの点・線のほか、原点・既存の形の角と辺・
/// 線分の中点も相手にするので、それらを仮に足したスケッチと、付ける拘束、その名前を返す。
pub(crate) fn drop_target(
    sketch: &Sketch,
    sol: &Solution,
    refs: &[jc_sketch::RefItem],
    point: EntityId,
    w: [f64; 2],
    tol: f64,
) -> Option<(crate::snap::Augmented, ConstraintKind, &'static str)> {
    let wants = crate::snap::Wants {
        points: true,
        edges: true,
        midpoints: true,
        crossings: false,
    };
    let aug = crate::snap::augment(sketch, sol, refs, wants);
    let (k, label) = crate::drag::drop_on(&aug.sketch, &aug.sol, point, w, tol)?;
    Some((aug, k, label))
}

/// 離した所の拘束 `k` を足し、使わなかった仮の要素を除く。付けると余分になる(もう決まっている)
/// 時と、解けなくなる時は付けずに理由を返す。
pub(crate) fn attach_dropped(
    aug: crate::snap::Augmented,
    k: ConstraintKind,
    values: Option<&DimValues>,
) -> Result<Sketch, &'static str> {
    let mut s = aug.sketch.clone();
    s.adopt_solution(&aug.sol);
    if let Some(v) = values {
        if jc_sketch::redundant_with(&s, v, &k).is_some() {
            return Err("離した所の拘束は、ほかの拘束で決まっているので付けません");
        }
        let mut t = s.clone();
        t.add_constraint(k.clone());
        match jc_sketch::solve(&t, v) {
            Ok(sol) if !matches!(sol.status, SolveStatus::Inconsistent { .. }) => {}
            _ => {
                return Err(
                    "離した所の拘束を付けると解けなくなるので付けません(Ctrl を押して離すと付けない)",
                );
            }
        }
    }
    s.add_constraint(k);
    Ok(aug.prune(s, &[]).0)
}
