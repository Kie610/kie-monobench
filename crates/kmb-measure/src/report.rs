//! 測った結果の表の行と寸法の線、2 つ選んだ時の測り方、ボディの値。

use super::*;

/// 測った結果(表の行と、ビューに描く寸法の線)。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub rows: Vec<(String, String)>,
    /// ビューに描く寸法の線(2 つ選んだ時は選んだ測り方の 2 点。長穴なら両端の中心)
    pub line: Option<(P, P)>,
    /// 線の横に出す値
    pub label: Option<String>,
    /// ビューに印を描く点(面を 1 つ選んだ時のボディの重心)
    pub point: Option<P>,
    /// 切り替えられる測り方(丸い物が入っている時だけ)
    pub bases: Vec<Basis>,
    /// 今の測り方
    pub basis: Basis,
    /// 強く出す行(今の測り方の値)
    pub primary: usize,
}

impl Report {
    /// 写すための文(1 行に 1 つ)。
    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(|(k, v)| format!("{k}\t{v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 長さ(mm)を画面の単位(文書の単位。CLI・MCP は mm)で、小数 3 桁で書く(候補 19)。
pub fn mm(v: f64) -> String {
    kmb_core::display_unit().length(v, 3)
}

/// 位置(mm)を画面の単位で。mm の時は単位を付けない(前と同じ)、ほかは後ろに単位の名前。
pub(crate) fn xyz(p: P) -> String {
    let u = kmb_core::display_unit();
    let s = numbers(p.map(|v| u.from_mm(v)));
    if u.is_mm() {
        s
    } else {
        format!("{s} {}", u.name())
    }
}

/// 向き(単位の無い 3 つの数)。
pub(crate) fn numbers(p: P) -> String {
    let f = |v: f64| if v.abs() < 5e-4 { 0.0 } else { v };
    format!("{:.3}, {:.3}, {:.3}", f(p[0]), f(p[1]), f(p[2]))
}

/// 面積(mm²)・体積(mm³)を画面の単位で。
pub(crate) fn area(v: f64) -> String {
    kmb_core::display_unit().power(v, 2, 3)
}

pub(crate) fn volume(v: f64) -> String {
    kmb_core::display_unit().power(v, 3, 3)
}

pub(crate) fn deg(r: f64) -> String {
    format!("{:.2}°", r.to_degrees())
}

/// 1 つ選んだ時の値。
pub fn describe(g: &Geo) -> Vec<(String, String)> {
    let row = |k: &str, v: String| (k.to_string(), v);
    match &g.kind {
        Kind::Point(p) => vec![row("位置 X, Y, Z", xyz(*p))],
        Kind::Line { a, b } => vec![
            row("長さ", mm(dist(*a, *b))),
            row("始点", xyz(*a)),
            row("終点", xyz(*b)),
        ],
        Kind::Circle(c) => {
            let mut v = vec![
                row("直径", mm(2.0 * c.radius)),
                row("半径", mm(c.radius)),
                row("中心", xyz(c.center)),
            ];
            v.push(if c.closed {
                row("周の長さ", mm(std::f64::consts::TAU * c.radius))
            } else {
                row("弧の長さ", mm(c.sweep.abs() * c.radius))
            });
            if !c.closed {
                v.push(row("弧の角度", deg(c.sweep.abs())));
            }
            v
        }
        Kind::Cylinder(c) => {
            let mut v = vec![
                row("直径", mm(2.0 * c.radius)),
                row("半径", mm(c.radius)),
                row("軸の向き", numbers(c.axis).replace(".000", "")),
                row("軸の中心", xyz(c.center)),
                row("軸の向きの長さ", mm(c.length)),
            ];
            if !c.closed {
                v.push(row("回る角度", deg(c.sweep)));
            }
            v
        }
        Kind::Curve { length } => vec![row("長さ(網で)", mm(*length))],
        Kind::Plane {
            normal, area: a, ..
        } => vec![
            row("面積", area(*a)),
            row("向き(法線)", numbers(*normal).replace(".000", "")),
        ],
        Kind::Surface { area: a } => vec![row("面積(網で)", area(*a))],
    }
}

/// 長穴の行。
pub fn describe_slot(s: &Slot) -> Vec<(String, String)> {
    vec![
        ("長穴の幅".into(), mm(s.width)),
        ("長穴の中心間".into(), mm(s.pitch())),
        ("長穴の全長".into(), mm(s.overall())),
    ]
}

/// 2 つの形の向き。`true` は面の法線(円は円の面の法線)、`false` は線の向き(円柱は軸)。
/// 角度を出せない形は `None`。
pub(crate) fn direction(g: &Geo) -> Option<(P, bool)> {
    match &g.kind {
        Kind::Line { a, b } => Some((normalize(sub(*b, *a)), false)),
        Kind::Plane { normal, .. } => Some((*normal, true)),
        Kind::Circle(c) => Some((c.normal, true)),
        Kind::Cylinder(c) => Some((c.axis, false)),
        _ => None,
    }
}

/// 端を 1 つ共有する 2 本のまっすぐな辺の、その角での開き(ラジアン、0〜π)。共有しなければ `None`。
fn corner_angle(a: &Geo, b: &Geo) -> Option<f64> {
    let (Kind::Line { a: a0, b: a1 }, Kind::Line { a: b0, b: b1 }) = (&a.kind, &b.kind) else {
        return None;
    };
    let same = |p: P, q: P| {
        let d = sub(p, q);
        dot(d, d).sqrt() < 1e-6
    };
    for (corner, p) in [(*a0, *a1), (*a1, *a0)] {
        for (c2, q) in [(*b0, *b1), (*b1, *b0)] {
            if same(corner, c2) {
                let (u, v) = (normalize(sub(p, corner)), normalize(sub(q, corner)));
                return Some(dot(u, v).clamp(-1.0, 1.0).acos());
            }
        }
    }
    None
}

/// 丸い物の測る元。円の辺は中心の点、円柱の面は軸。
#[derive(Clone, Copy)]
pub(crate) enum Anchor {
    Point(P),
    Axis { at: P, dir: P },
}

pub(crate) fn anchor(g: &Geo) -> Option<Anchor> {
    match &g.kind {
        Kind::Circle(c) => Some(Anchor::Point(c.center)),
        Kind::Cylinder(c) => Some(Anchor::Axis {
            at: c.center,
            dir: c.axis,
        }),
        _ => None,
    }
}

/// `p` から、`at` を通る向き `dir` の直線へ下ろした足。
pub(crate) fn foot(p: P, at: P, dir: P) -> P {
    add(at, scale(dir, dot(sub(p, at), dir)))
}

const PARALLEL: f64 = 1e-5;

/// 丸い物どうしの中心・軸の距離(名前, 距離, 2 点)。
pub(crate) fn anchor_anchor(x: Anchor, y: Anchor) -> Option<(&'static str, P, P)> {
    match (x, y) {
        (Anchor::Point(p), Anchor::Point(q)) => Some(("中心どうしの距離", p, q)),
        (Anchor::Point(p), Anchor::Axis { at, dir }) => {
            Some(("中心と軸の距離", p, foot(p, at, dir)))
        }
        (Anchor::Axis { at, dir }, Anchor::Point(q)) => {
            Some(("中心と軸の距離", foot(q, at, dir), q))
        }
        (Anchor::Axis { at: a, dir: u }, Anchor::Axis { at: b, dir: v }) => {
            let c = dot(u, v);
            if 1.0 - c.abs() < PARALLEL {
                return Some(("軸どうしの距離", a, foot(a, b, v)));
            }
            // ねじれた軸: 2 本の直線のいちばん近い点
            let w = sub(a, b);
            let (d, e) = (dot(u, w), dot(v, w));
            let den = 1.0 - c * c;
            let (s, t) = ((c * e - d) / den, (e - c * d) / den);
            Some(("軸どうしの距離", add(a, scale(u, s)), add(b, scale(v, t))))
        }
    }
}

/// 丸い物の中心・軸と、丸くない物の距離(名前, 2 点。前が丸い物の側)。
pub(crate) fn anchor_other(x: Anchor, g: &Geo) -> Option<(&'static str, P, P)> {
    match x {
        Anchor::Point(p) => {
            let q = match &g.kind {
                Kind::Point(q) => *q,
                // 辺は線を延ばして垂直に(穴の位置を縁から測る)
                Kind::Line { a, b } => foot(p, *a, normalize(sub(*b, *a))),
                Kind::Plane { normal, point, .. } => {
                    sub(p, scale(*normal, dot(sub(p, *point), *normal)))
                }
                _ => {
                    let pg = Geo {
                        kind: Kind::Point(p),
                        points: vec![p],
                        segs: vec![],
                        tris: vec![],
                    };
                    closest(&pg, g)?.2
                }
            };
            Some(("中心からの距離", p, q))
        }
        Anchor::Axis { at, dir } => match &g.kind {
            Kind::Point(q) => Some(("軸からの距離", foot(*q, at, dir), *q)),
            Kind::Line { a, b } if 1.0 - dot(normalize(sub(*b, *a)), dir).abs() < PARALLEL => {
                let m = scale(add(*a, *b), 0.5);
                Some(("軸からの距離", foot(m, at, dir), m))
            }
            Kind::Plane { normal, point, .. } if dot(*normal, dir).abs() < PARALLEL => Some((
                "軸からの距離",
                at,
                sub(at, scale(*normal, dot(sub(at, *point), *normal))),
            )),
            _ => None,
        },
    }
}

/// 中心・軸から測った距離(名前, 2 点。前が `a` の側)。どちらも丸くなければ `None`。
pub(crate) fn center_distance(a: &Geo, b: &Geo) -> Option<(&'static str, P, P)> {
    match (anchor(a), anchor(b)) {
        (None, None) => None,
        (Some(x), Some(y)) => anchor_anchor(x, y),
        (Some(x), None) => anchor_other(x, b),
        (None, Some(y)) => anchor_other(y, a).map(|(n, p, q)| (n, q, p)),
    }
}

/// 2 つの形のいちばん遠い点の組と距離(距離は凸なので、いちばん遠い所は網の点にある)。
pub fn farthest(a: &Geo, b: &Geo) -> Option<(f64, P, P)> {
    let verts = |g: &Geo| -> Vec<P> {
        g.points
            .iter()
            .copied()
            .chain(g.segs.iter().flatten().copied())
            .chain(g.tris.iter().flatten().copied())
            .collect()
    };
    let (va, vb) = (verts(a), verts(b));
    let mut best: Option<(f64, P, P)> = None;
    for p in &va {
        for q in &vb {
            let d = dist(*p, *q);
            if best.is_none_or(|b| d > b.0) {
                best = Some((d, *p, *q));
            }
        }
    }
    best
}

/// 2 つ選んだ時の値。`want` は測り方(丸い物が無ければ最短)、`xyz` は ΔX・ΔY・ΔZ の行を出すか。
pub fn compare(a: &Geo, b: &Geo, want: Basis, xyz_rows: bool) -> Report {
    let mut r = Report::default();
    let Some(min) = closest(a, b) else {
        return r;
    };
    let row = |k: &str, v: String| (k.to_string(), v);
    let round = anchor(a).is_some() || anchor(b).is_some();
    let center = center_distance(a, b);
    let max = if round { farthest(a, b) } else { None };
    if round {
        if center.is_some() {
            r.bases.push(Basis::Center);
        }
        r.bases.push(Basis::Min);
        if max.is_some() {
            r.bases.push(Basis::Max);
        }
    }
    r.basis = if r.bases.contains(&want) {
        want
    } else {
        r.bases.first().copied().unwrap_or(Basis::Min)
    };
    if let Some((name, p, q)) = center {
        r.rows.push(row(name, mm(dist(p, q))));
    }
    if r.basis == Basis::Min {
        r.primary = r.rows.len();
    }
    r.rows.push(row("最短の距離", mm(min.0)));
    if let Some(m) = max {
        if r.basis == Basis::Max {
            r.primary = r.rows.len();
        }
        r.rows.push(row("最長の距離", mm(m.0)));
    }
    let (p, q) = match r.basis {
        Basis::Center => center
            .map(|(_, p, q)| (p, q))
            .expect("中心がある時だけ選べる"),
        Basis::Min => (min.1, min.2),
        Basis::Max => max.map(|m| (m.1, m.2)).expect("最長がある時だけ選べる"),
    };
    r.line = Some((p, q));
    r.label = Some(mm(dist(p, q)));
    if xyz_rows {
        let delta = sub(q, p);
        r.rows.push(row("ΔX", mm(delta[0].abs())));
        r.rows.push(row("ΔY", mm(delta[1].abs())));
        r.rows.push(row("ΔZ", mm(delta[2].abs())));
    }
    // 角度と、平行な面の間隔
    if let (Some((u, un)), Some((v, vn))) = (direction(a), direction(b)) {
        let c = dot(u, v).clamp(-1.0, 1.0);
        match (un, vn) {
            (true, true) => {
                // 面(や円の面)どうし: 法線のなす角
                if 1.0 - c.abs() < PARALLEL {
                    r.rows.push(row("角度", "平行".into()));
                    if let (Kind::Plane { point: pa, .. }, Kind::Plane { point: pb, .. }) =
                        (&a.kind, &b.kind)
                    {
                        r.rows
                            .push(row("平行な面の間隔", mm(dot(sub(*pb, *pa), u).abs())));
                    }
                } else {
                    r.rows.push(row("角度", deg(c.acos())));
                }
            }
            (false, false) => {
                // 直線(や軸)どうし: 0〜90°。角を共有する 2 本の辺は、その角から出る向きどうしの
                // 角(鈍角も出す。120° の角を 60° と出さない。10-24 u14)
                let ang = match corner_angle(a, b) {
                    Some(t) => t,
                    None => c.abs().min(1.0).acos(),
                };
                r.rows.push(row(
                    "角度",
                    if ang < 1e-5 {
                        "平行".into()
                    } else {
                        deg(ang)
                    },
                ));
            }
            _ => {
                // 直線と面: 直線が面となす角(0〜90°)
                let ang = std::f64::consts::FRAC_PI_2 - c.abs().min(1.0).acos();
                r.rows.push(row(
                    "角度",
                    if ang.abs() < 1e-5 {
                        "平行".into()
                    } else {
                        deg(ang)
                    },
                ));
            }
        }
    }
    // 点・面に平行な直線と、平らな面: 面を広げた平面までの垂直の距離
    // (面に穴や段があると最短の距離は斜めになるので、高さを知りたい時はこちら。円の中心は上の行で出す)
    for (x, y) in [(a, b), (b, a)] {
        let Kind::Plane { normal, point, .. } = y.kind else {
            continue;
        };
        let at = match &x.kind {
            Kind::Point(p) => Some(*p),
            Kind::Line { a: la, b: lb } if dot(normalize(sub(*lb, *la)), normal).abs() < 1e-5 => {
                Some(*la)
            }
            _ => None,
        };
        if let Some(p) = at {
            r.rows.push(row(
                "面の平面までの距離",
                mm(dot(sub(p, point), normal).abs()),
            ));
        }
    }
    // 曲面・曲線は網(表示の許容差 0.05 mm)で測るので、そう断る
    if [a, b]
        .iter()
        .any(|g| matches!(g.kind, Kind::Surface { .. } | Kind::Curve { .. }))
    {
        r.rows
            .push(row("注", "曲面は網で測るため ±0.05 mm ほど".into()));
    }
    r
}

/// 面を 1 つ選んだ時に足す、そのボディの値(呼ぶ側がカーネルと文書から求める)。
#[derive(Clone, Debug, PartialEq)]
pub struct BodyFacts {
    pub props: MassProps,
    /// 付けた物理材料の名前と密度(g/cm³)。あれば質量も出す
    pub material: Option<(String, f64)>,
}

impl BodyFacts {
    /// 行(体積・表面積・外形・重心、材料があれば材料と質量)。
    pub fn rows(&self) -> Vec<(String, String)> {
        let b = &self.props;
        let mut rows = vec![
            ("ボディの体積".into(), volume(b.volume)),
            ("ボディの表面積".into(), area(b.area)),
            ("ボディの外形".into(), {
                let u = kmb_core::display_unit();
                let d = |i: usize| u.from_mm(b.max[i] - b.min[i]);
                format!("{:.3} × {:.3} × {:.3} {}", d(0), d(1), d(2), u.name())
            }),
        ];
        if let Some(c) = b.centroid {
            rows.push(("ボディの重心".into(), {
                let u = kmb_core::display_unit();
                let c = c.map(|v| u.from_mm(v));
                format!("({:.3}, {:.3}, {:.3}) {}", c[0], c[1], c[2], u.name())
            }));
        }
        match &self.material {
            Some((name, density)) => {
                rows.push(("材料".into(), format!("{name}({density} g/cm³)")));
                let g = kmb_look::material::mass_of(b.volume, *density);
                rows.push((
                    "質量".into(),
                    if g >= 1000.0 {
                        format!("{:.3} kg", g / 1000.0)
                    } else {
                        format!("{g:.3} g")
                    },
                ));
            }
            None => rows.push((
                "材料".into(),
                "なし(ブラウザのボディの右クリックで付けると質量が出る)".into(),
            )),
        }
        rows
    }
}

/// 選んだ物(1〜2 つ)を測った結果。画面の測る道具の窓と MCP の `measure` が同じ行を出す。
/// 1 つなら形の値(長穴の端なら長穴の寸法も)、面を 1 つなら `body`(そのボディの体積・表面積・外形。
/// 呼ぶ側がカーネルで求める)も足す。2 つなら [`compare`]。網が変わって見つからない物は飛ばす。
pub fn measure(
    scene: &(impl Meshes + ?Sized),
    picks: &[Target],
    basis: Basis,
    xyz_rows: bool,
    body: Option<BodyFacts>,
) -> Report {
    let geos: Vec<Geo> = picks.iter().filter_map(|t| resolve(scene, t)).collect();
    let body = match picks {
        [Target::Face(_)] => body,
        _ => None,
    };
    let slot = match picks {
        [t] => slot(scene, t),
        _ => None,
    };
    match geos.as_slice() {
        [g] => {
            let mut rows = describe(g);
            let mut report = Report::default();
            if let Some(s) = slot {
                rows.extend(describe_slot(&s));
                report.line = Some(s.centers);
                report.label = Some(format!("中心間 {}", mm(s.pitch())));
            }
            if let Some(b) = &body {
                rows.extend(b.rows());
                report.point = b.props.centroid;
            }
            report.rows = rows;
            report
        }
        [a, b] => compare(a, b, basis, xyz_rows),
        _ => Report::default(),
    }
}
