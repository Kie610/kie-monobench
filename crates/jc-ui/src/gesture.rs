//! クリックで形を作る流れ: コマンド → 基準をクリック → マウスを動かすと値が追う → クリックで決める
//! → Enter で確定、Esc でやめる。数字はいつでも打てる。ボタンを押したまま動かす操作は使わない。
//!
//! 手順の形(距離・平面の上の点・径)はモジュールが [`jc_core::Handle`] で出し、
//! ここではマウスの位置を値にするだけ。決めた値はモジュールの操作の指定(`args`)に入れて実行する。

use crate::view3d::Camera;
use egui::{Pos2, Rect};
use jc_core::{Gesture, Handle, HandleChoice};
use serde_json::{Map, Value as Json};

/// 進めている「クリックで作る」操作。
pub struct Run {
    pub action: String,
    pub label: String,
    /// 決めた値
    pub args: Map<String, Json>,
    pub handles: Vec<Handle>,
    /// 今決めている手順(`handles.len()` なら全部決めて、Enter を待っている)
    pub step: usize,
    /// マウスの位置での値(今の手順)
    pub live: Option<Json>,
    /// 打った数字(mm)
    pub typed: String,
    /// 候補を足す・外す手順([`Gesture::Regions`])で選んだ候補の値
    pub picks: Vec<Json>,
}

impl Run {
    pub fn current(&self) -> Option<&Handle> {
        self.handles.get(self.step)
    }

    pub fn ready(&self) -> bool {
        self.step >= self.handles.len()
    }

    /// 状態の欄に出す案内。
    pub fn hint(&self) -> String {
        match self.current() {
            Some(h) => {
                let live = self
                    .live
                    .as_ref()
                    .map(|v| describe_for(h, v))
                    .unwrap_or_default();
                let typed = if self.typed.is_empty() {
                    String::new()
                } else {
                    format!("(入力: {})", typed_text(h, &self.typed))
                };
                format!("{}: {}。{live}{typed}  Esc でやめる", self.label, h.label)
            }
            None => format!("{}: Enter で確定、Esc でやめる", self.label),
        }
    }
}

/// 手順 `h` の値を短い文に(候補なら候補の名前)。
pub fn describe_for(h: &Handle, v: &Json) -> String {
    match h.gesture {
        Gesture::Choice if h.faces && v == "face" => "この面".into(),
        Gesture::Choice => h
            .choices
            .iter()
            .find(|c| c.value == *v)
            .map_or_else(|| describe(v), |c| c.label.clone()),
        Gesture::Edges if h.chain => {
            format!("{} 本とつながる辺", v.as_array().map_or(0, Vec::len))
        }
        Gesture::Edges => format!("{} 本", v.as_array().map_or(0, Vec::len)),
        // 点は、見せる座標系(セットアップの座標など)で
        Gesture::Point { .. } => match (h.frame, point(v)) {
            (Some(f), Some(p)) => describe(&Json::from(f.to_local(p).to_vec())),
            _ => describe(v),
        },
        Gesture::Regions => match v.as_array() {
            Some(a) => format!("{} 個の領域", a.len()),
            None => "全部".into(),
        },
        Gesture::Angle { .. } => format!("{:.1}°", v.as_f64().unwrap_or(0.0)),
        _ => describe(v),
    }
}

/// 値を短い文に。
pub fn describe(v: &Json) -> String {
    match v {
        // 長さは文書の単位で(候補 19)
        Json::Number(n) => jc_core::display_unit().length(n.as_f64().unwrap_or(0.0), 2),
        Json::Array(a) => {
            let u = jc_core::display_unit();
            let c: Vec<String> = a
                .iter()
                .map(|x| format!("{:.2}", u.from_mm(x.as_f64().unwrap_or(0.0))))
                .collect();
            format!("({})", c.join(", "))
        }
        _ => v.to_string(),
    }
}

fn f32s(p: [f64; 3]) -> [f32; 3] {
    p.map(|v| v as f32)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn snap(v: f32, step: f32) -> f32 {
    if step > 0.0 {
        (v / step).round() * step
    } else {
        v
    }
}

/// 視線と平面(点 `o`、法線 `n`)の交点。
fn on_plane(cam: &Camera, rect: Rect, pos: Pos2, o: [f32; 3], n: [f32; 3]) -> Option<[f32; 3]> {
    let (ro, rd) = cam.ray(pos, rect);
    let den = dot(rd, n);
    if den.abs() < 1e-4 {
        return None;
    }
    let t = dot([o[0] - ro[0], o[1] - ro[1], o[2] - ro[2]], n) / den;
    Some([0, 1, 2].map(|i| ro[i] + rd[i] * t))
}

/// 吸い付く点に吸い付く、画面の上の距離(点)。
const SNAP_PX: f32 = 12.0;

/// マウスの位置での手順の値。
pub fn value_at(h: &Handle, cam: &Camera, rect: Rect, pos: Pos2) -> Option<Json> {
    match h.gesture {
        Gesture::Distance { origin, dir } => {
            // 視線と、origin を通る dir の直線の、いちばん近い所
            let (o, d) = (f32s(origin), f32s(dir));
            let (ro, rd) = cam.ray(pos, rect);
            let w = [o[0] - ro[0], o[1] - ro[1], o[2] - ro[2]];
            let (a, b, c) = (dot(d, d), dot(d, rd), dot(rd, rd));
            let den = a * c - b * b;
            let s = if den.abs() < 1e-6 {
                // 線が視線と平行: 画面の上下で決める
                let c0 = cam.project(o, rect).0;
                (c0.y - pos.y) / cam.zoom
            } else {
                (b * dot(rd, w) - c * dot(d, w)) / den
            };
            Some(Json::from(snap(s, 0.5) as f64))
        }
        Gesture::Point {
            origin,
            normal,
            snap: step,
        } => {
            // 吸い付く点が画面で近ければ、その点
            let near = h
                .snap_to
                .iter()
                .map(|q| (q, cam.project(f32s(*q), rect).0.distance(pos)))
                .filter(|(_, d)| *d < SNAP_PX)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((q, _)) = near {
                return Some(Json::from(q.to_vec()));
            }
            let p = on_plane(cam, rect, pos, f32s(origin), f32s(normal))?;
            let p = p.map(|v| v as f64);
            // 格子は、数字を見せる座標系(セットアップの座標など)で揃える
            let q = match h.frame {
                Some(f) => {
                    let l = f.to_local(p).map(|v| snap(v as f32, step as f32) as f64);
                    // 平面から外れないように、平面の向きの座標は格子に揃えない
                    let n = f.dir_to_local(normal);
                    let k = (0..3)
                        .max_by(|i, j| n[*i].abs().total_cmp(&n[*j].abs()))
                        .unwrap_or(2);
                    let mut l = l;
                    l[k] = f.to_local(p)[k];
                    f.to_world(l)
                }
                None => p.map(|v| snap(v as f32, step as f32) as f64),
            };
            Some(Json::from(q.to_vec()))
        }
        Gesture::Diameter { center, normal } => {
            let c = f32s(center);
            let p = on_plane(cam, rect, pos, c, f32s(normal))?;
            let r = dot(
                [p[0] - c[0], p[1] - c[1], p[2] - c[2]],
                [p[0] - c[0], p[1] - c[1], p[2] - c[2]],
            )
            .sqrt();
            Some(Json::from(snap(2.0 * r, 0.5).max(0.5) as f64))
        }
        Gesture::Angle { origin, axis, from } => angle_at(origin, axis, from, cam, rect, pos),
        // 面・辺はクリックしたもので決める(ビューが選ぶ。マウスの位置だけでは決めない)
        Gesture::Face | Gesture::AnyFace | Gesture::Edges | Gesture::Regions => None,
        Gesture::Choice => choice_at(h, cam, rect, pos).map(|c| h.choices[c].value.clone()),
    }
}

/// 角度の刻み(度)。
const ANGLE_STEP: f64 = 5.0;

/// 点 `p` を、`origin` を通る向き `axis`(長さ 1)の軸まわりに `deg` 度回す(右ねじの向き)。
pub fn rotate(p: [f64; 3], origin: [f64; 3], axis: [f64; 3], deg: f64) -> [f64; 3] {
    let (s, c) = deg.to_radians().sin_cos();
    let v = [0, 1, 2].map(|i| p[i] - origin[i]);
    let kv = axis[0] * v[0] + axis[1] * v[1] + axis[2] * v[2];
    let cr = [
        axis[1] * v[2] - axis[2] * v[1],
        axis[2] * v[0] - axis[0] * v[2],
        axis[0] * v[1] - axis[1] * v[0],
    ];
    [0, 1, 2].map(|i| origin[i] + v[i] * c + cr[i] * s + axis[i] * kv * (1.0 - c))
}

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (l > 1e-12).then(|| v.map(|x| x / l))
}

/// マウスの位置での角度。点 `from` を軸まわりに回した円を画面に写した楕円の上で、マウスの向きの
/// 角度を解く。円が線に潰れて見える時(輪郭を正面から見ている時)は、1 度ずつ写してマウスに
/// いちばん近い所(同じ所に写る 2 つは手前の方)にする。
fn angle_at(
    origin: [f64; 3],
    axis: [f64; 3],
    from: [f64; 3],
    cam: &Camera,
    rect: Rect,
    pos: Pos2,
) -> Option<Json> {
    let axis = unit(axis)?;
    let w = [0, 1, 2].map(|i| from[i] - origin[i]);
    let along = w[0] * axis[0] + w[1] * axis[1] + w[2] * axis[2];
    let c = [0, 1, 2].map(|i| origin[i] + axis[i] * along);
    let u = [0, 1, 2].map(|i| from[i] - c[i]);
    if u.iter().map(|x| x * x).sum::<f64>() < 1e-18 {
        return None;
    }
    let v = [
        axis[1] * u[2] - axis[2] * u[1],
        axis[2] * u[0] - axis[0] * u[2],
        axis[0] * u[1] - axis[1] * u[0],
    ];
    let pc = cam.project(f32s(c), rect).0;
    let pu = cam.project(f32s([0, 1, 2].map(|i| c[i] + u[i])), rect).0 - pc;
    let pv = cam.project(f32s([0, 1, 2].map(|i| c[i] + v[i])), rect).0 - pc;
    let m = pos - pc;
    let det = pu.x * pv.y - pu.y * pv.x;
    let big = pu.length().max(pv.length());
    let deg = if det.abs() > 0.15 * big * big {
        let a = (m.x * pv.y - m.y * pv.x) / det;
        let b = (pu.x * m.y - pu.y * m.x) / det;
        (b as f64).atan2(a as f64).to_degrees()
    } else {
        let mut best: Option<(f32, f32, f64)> = None;
        for k in 1..=360 {
            let deg = k as f64;
            let (q, z) = cam.project(f32s(rotate(from, origin, axis, deg)), rect);
            let d = q.distance(pos);
            let better = match best {
                None => true,
                Some((bd, bz, _)) => d < bd - 1e-3 || (d < bd + 1e-3 && z > bz),
            };
            if better {
                best = Some((d, z, deg));
            }
        }
        best?.2
    };
    let deg = (deg / ANGLE_STEP).round() * ANGLE_STEP;
    let deg = if deg <= 0.0 { deg + 360.0 } else { deg };
    Some(Json::from(deg.clamp(ANGLE_STEP, 360.0)))
}

/// マウスの位置にある候補の番号。閉じた線は内側でも、開いた線は近くで拾う。
/// 内側に入る候補が重なる時は、面積の小さい(内側の)もの。
pub fn choice_at(h: &Handle, cam: &Camera, rect: Rect, pos: Pos2) -> Option<usize> {
    let mut best: Option<((f32, bool), usize)> = None;
    for (k, c) in h.choices.iter().enumerate() {
        let lines: Vec<Vec<Pos2>> = outline(c, cam, rect)
            .iter()
            .map(|l| l.iter().map(|p| cam.project(f32s(*p), rect).0).collect())
            .collect();
        // 内側(閉じた線の偶奇)なら面積を、近い線なら 0 を候補の重みにする
        let mut inside = false;
        let mut area = 0.0;
        let mut near = f32::INFINITY;
        for l in &lines {
            let closed = l.len() > 2 && l[0].distance(l[l.len() - 1]) < 0.5;
            for w in l.windows(2) {
                let (a, b) = (w[0], w[1]);
                let ab = b - a;
                let t = if ab.length_sq() < 1e-6 {
                    0.0
                } else {
                    ((pos - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
                };
                near = near.min((a + ab * t).distance(pos));
                if closed {
                    if (a.y > pos.y) != (b.y > pos.y)
                        && pos.x < a.x + (pos.y - a.y) / (b.y - a.y) * (b.x - a.x)
                    {
                        inside = !inside;
                    }
                    area += a.x * b.y - b.x * a.y;
                }
            }
        }
        // 線の近くで並んだ時は、押した所を内に含む候補を採る(大きな領域の穴の縁に沿う小さな領域が、
        // 外の領域の線に負けて選べなくなるため)
        let weight = if near < SNAP_PX {
            (0.0, !inside)
        } else if inside {
            (area.abs() / 2.0, false)
        } else {
            continue;
        };
        if best.is_none_or(|(w, _)| weight.0 < w.0 || (weight.0 == w.0 && !weight.1 && w.1)) {
            best = Some((weight, k));
        }
    }
    best.map(|b| b.1)
}

/// 候補の線(世界座標)。原点の平面や軸([`HandleChoice::sized`])は、画面の上で一定の大きさに伸ばす。
pub fn outline(c: &HandleChoice, cam: &Camera, rect: Rect) -> Vec<Vec<[f64; 3]>> {
    if !c.sized {
        return c.outline.clone();
    }
    let s = crate::view3d::origin_size(cam, rect) as f64;
    c.outline
        .iter()
        .map(|l| l.iter().map(|p| p.map(|x| x * s)).collect())
        .collect()
}

/// [`preview`] の、原点の平面や軸の候補を画面の上の大きさに伸ばしたもの(ビューに描く時に使う)。
pub fn preview_in(h: &Handle, v: &Json, cam: &Camera, rect: Rect) -> Vec<Vec<[f64; 3]>> {
    match h.gesture {
        Gesture::Choice => h
            .choices
            .iter()
            .find(|c| c.value == *v)
            .map(|c| outline(c, cam, rect))
            .unwrap_or_default(),
        _ => preview(h, v),
    }
}

/// 値の手前の形(線)。距離なら輪郭を値だけ動かし、点なら十字、径なら円。
/// 候補は線のまま(原点の平面や軸は長さ 1。ビューでは [`preview_in`])。
pub fn preview(h: &Handle, v: &Json) -> Vec<Vec<[f64; 3]>> {
    match h.gesture {
        Gesture::Distance { origin, dir } => {
            let s = v.as_f64().unwrap_or(0.0);
            let off = |p: [f64; 3]| [0, 1, 2].map(|i| p[i] + dir[i] * s);
            let mut out: Vec<Vec<[f64; 3]>> = h
                .outline
                .iter()
                .map(|l| l.iter().map(|p| off(*p)).collect())
                .collect();
            out.push(vec![origin, off(origin)]);
            // 輪郭の角から何本か、縦の線
            for l in &h.outline {
                let step = (l.len() / 4).max(1);
                for p in l.iter().step_by(step) {
                    out.push(vec![*p, off(*p)]);
                }
            }
            out
        }
        Gesture::Point { normal, .. } => {
            let Some(p) = point(v) else { return vec![] };
            circle(p, normal, 1.0)
        }
        Gesture::Diameter { center, normal } => {
            let d = v.as_f64().unwrap_or(0.0);
            circle(center, normal, d / 2.0)
        }
        Gesture::Angle { origin, axis, from } => {
            let Some(axis) = unit(axis) else {
                return vec![];
            };
            let deg = v.as_f64().unwrap_or(0.0);
            let turn = |p: [f64; 3], a: f64| rotate(p, origin, axis, a);
            // 回した後の輪郭と、輪郭の何点かと 0 度の点が通る円弧
            let mut out: Vec<Vec<[f64; 3]>> = h
                .outline
                .iter()
                .map(|l| l.iter().map(|p| turn(*p, deg)).collect())
                .collect();
            let arc = |p: [f64; 3]| -> Vec<[f64; 3]> {
                let n = (deg / 5.0).ceil().max(1.0) as usize;
                (0..=n)
                    .map(|i| turn(p, deg * i as f64 / n as f64))
                    .collect()
            };
            out.push(arc(from));
            for l in &h.outline {
                let step = (l.len() / 4).max(1);
                out.extend(l.iter().step_by(step).map(|p| arc(*p)));
            }
            out
        }
        Gesture::Face | Gesture::AnyFace | Gesture::Edges => vec![],
        // 選んだ候補の線
        Gesture::Regions => h
            .choices
            .iter()
            .filter(|c| v.as_array().is_some_and(|a| a.contains(&c.value)))
            .flat_map(|c| c.outline.clone())
            .collect(),
        // 選んだ候補は太く描く(ビューが描く)
        Gesture::Choice => h
            .choices
            .iter()
            .find(|c| c.value == *v)
            .map(|c| c.outline.clone())
            .unwrap_or_default(),
    }
}

fn point(v: &Json) -> Option<[f64; 3]> {
    serde_json::from_value(v.clone()).ok()
}

fn circle(c: [f64; 3], n: [f64; 3], r: f64) -> Vec<Vec<[f64; 3]>> {
    let pl = jc_core::Plane::from_point_normal(c, n);
    let local = pl.to_local(c);
    let ring = (0..=48)
        .map(|k| {
            let a = k as f64 / 48.0 * std::f64::consts::TAU;
            pl.to_world([local[0] + r * a.cos(), local[1] + r * a.sin()])
        })
        .collect();
    let cross = |d: [f64; 2]| {
        vec![
            pl.to_world([local[0] - d[0], local[1] - d[1]]),
            pl.to_world([local[0] + d[0], local[1] + d[1]]),
        ]
    };
    vec![
        ring,
        cross([r.max(1.0) * 1.3, 0.0]),
        cross([0.0, r.max(1.0) * 1.3]),
    ]
}

/// 候補を足す・外す手順で、候補 `k` をクリックした時: 選んでいれば外し、なければ足す。
/// 別の組の候補を足す時は、それまでの選びを外す(押し出す領域は 1 つのスケッチから)。
pub fn toggle_pick(h: &Handle, picks: &mut Vec<Json>, k: usize) {
    let c = &h.choices[k];
    if let Some(i) = picks.iter().position(|v| *v == c.value) {
        picks.remove(i);
        return;
    }
    let group_of = |v: &Json| h.choices.iter().find(|c| c.value == *v).map(|c| c.group);
    if picks.iter().any(|v| group_of(v) != Some(c.group)) {
        picks.clear();
    }
    picks.push(c.value.clone());
}

/// 閉じた線の内側を塗る(画面の 1 行ずつ、偶奇の規則。穴は塗らない)。
pub fn fill(painter: &egui::Painter, rect: Rect, lines: &[Vec<Pos2>], color: egui::Color32) {
    let (mut top, mut bottom) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in lines.iter().flatten() {
        top = top.min(p.y);
        bottom = bottom.max(p.y);
    }
    let (top, bottom) = (
        top.max(rect.top()).floor(),
        bottom.min(rect.bottom()).ceil(),
    );
    let mut mesh = egui::Mesh::default();
    let mut xs: Vec<f32> = Vec::new();
    let mut y = top;
    while y < bottom {
        let yc = y + 0.5;
        xs.clear();
        for l in lines {
            for w in l.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (a.y <= yc) != (b.y <= yc) {
                    xs.push(a.x + (yc - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
        }
        xs.sort_by(f32::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            let r = Rect::from_min_max(
                Pos2::new(pair[0].max(rect.left()), y),
                Pos2::new(pair[1].min(rect.right()), y + 1.0),
            );
            if r.width() > 0.0 {
                mesh.add_colored_rect(r, color);
            }
        }
        y += 1.0;
    }
    painter.add(mesh);
}

/// 打った数字を今の手順の値にする。距離と径は 1 つの数(文書の単位で打ち、値は mm)。平面の上の点は
/// 「X, Y」か「X, Y, Z」(文書の単位、文書の座標)。2 つなら、平面の向きにいちばん近い軸を除いた 2 つの座標
/// (上の面なら X と Y)。どちらも平面の上へ写す。
pub fn typed_value(h: &Handle, typed: &str) -> Option<Json> {
    // 打つ数は文書の単位(インチの文書で 0.5 は 12.7 mm。候補 19)
    let unit = jc_core::display_unit().mm();
    match h.gesture {
        Gesture::Distance { .. } => Some(Json::from(typed.trim().parse::<f64>().ok()? * unit)),
        Gesture::Diameter { .. } => {
            let v: f64 = typed.trim().parse().ok()?;
            (v > 0.0).then(|| Json::from(v * unit))
        }
        Gesture::Angle { .. } => {
            let v: f64 = typed.trim().parse().ok()?;
            (v > 0.0 && v <= 360.0).then(|| Json::from(v))
        }
        Gesture::Point { origin, normal, .. } => {
            // 座標系があれば、その座標で打った数として読む
            let (origin, normal) = match h.frame {
                Some(f) => (f.to_local(origin), f.dir_to_local(normal)),
                None => (origin, normal),
            };
            let nums: Vec<f64> = typed
                .split([',', ' ', '、'])
                .filter(|t| !t.is_empty())
                .map(|t| t.parse::<f64>().ok().map(|v| v * unit))
                .collect::<Option<_>>()?;
            let mut p = origin;
            match nums[..] {
                [x, y, z] => p = [x, y, z],
                [a, b] => {
                    let k = (0..3)
                        .max_by(|i, j| normal[*i].abs().total_cmp(&normal[*j].abs()))
                        .unwrap_or(2);
                    let rest: Vec<usize> = (0..3).filter(|i| *i != k).collect();
                    p[rest[0]] = a;
                    p[rest[1]] = b;
                }
                _ => return None,
            }
            // 平面の上へ写す
            let nn: f64 = normal.iter().map(|v| v * v).sum();
            if nn < 1e-12 {
                return None;
            }
            let d: f64 = (0..3).map(|i| (p[i] - origin[i]) * normal[i]).sum::<f64>() / nn;
            let q = [0, 1, 2].map(|i| p[i] - normal[i] * d);
            let q = h.frame.map_or(q, |f| f.to_world(q));
            Some(Json::from(q.to_vec()))
        }
        _ => None,
    }
}

/// 今の手順で数字を打てるか(打てる手順なら、打ち方の案内)。
pub fn typing_hint(h: &Handle) -> Option<&'static str> {
    match h.gesture {
        Gesture::Distance { .. } | Gesture::Diameter { .. } => {
            Some("数を打って Enter でも決められます(mm。欄を押さなくてよい)")
        }
        Gesture::Point { .. } => {
            Some("位置は「X, Y」(mm)と打って Enter でも決められます(欄を押さなくてよい)")
        }
        Gesture::Angle { .. } => {
            Some("角度(度)を打って Enter でも決められます(欄を押さなくてよい)")
        }
        _ if h.optional => Some("数を打って Enter、打たずに Enter なら省く"),
        _ => None,
    }
}

/// 打っている値の見せ方(点は「X, Y」、ほかは文書の単位)。
pub fn typed_text(h: &Handle, typed: &str) -> String {
    match h.gesture {
        Gesture::Point { .. } => typed.to_string(),
        Gesture::Angle { .. } => format!("{typed}°"),
        _ => format!("{typed} {}", jc_core::display_unit().name()),
    }
}

/// 今の手順で打てる文字か(数、小数点、負の符号。点の手順は区切りも)。
pub fn typable(h: &Handle, c: char) -> bool {
    c.is_ascii_digit()
        || c == '.'
        || c == '-'
        || matches!(h.gesture, Gesture::Point { .. }) && matches!(c, ',' | ' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam_top() -> (Camera, Rect) {
        let mut c = Camera::default();
        c.set_view(-90.0, 89.9);
        c.zoom = 1.0;
        (c, Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 200.0)))
    }

    #[test]
    fn a_point_can_be_typed_as_x_y_on_the_plane() {
        // 上の面(z = 8、上向き): 「8, 20」は X と Y、Z は面の高さ
        let h = Handle {
            key: "center".into(),
            label: String::new(),
            gesture: Gesture::Point {
                origin: [0.0, 0.0, 8.0],
                normal: [0.0, 0.0, 1.0],
                snap: 0.5,
            },
            outline: vec![],
            snap_to: vec![],
            choices: vec![],
            optional: false,
            faces: false,
            frame: None,
            fill: false,
            picked: vec![],
            ops: None,
            show_origin: false,
            shape: None,
            chain: false,
            vertices: false,
        };
        assert_eq!(
            typed_value(&h, "8, 20"),
            Some(serde_json::json!([8.0, 20.0, 8.0]))
        );
        assert_eq!(
            typed_value(&h, "8 20"),
            Some(serde_json::json!([8.0, 20.0, 8.0]))
        );
        // 3 つなら文書の座標。平面の上へ写す
        assert_eq!(
            typed_value(&h, "1,2,3"),
            Some(serde_json::json!([1.0, 2.0, 8.0]))
        );
        assert_eq!(typed_value(&h, "5"), None);
        // 横の面(x = 60、右向き): 2 つは Y と Z
        let side = Handle {
            gesture: Gesture::Point {
                origin: [60.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                snap: 0.5,
            },
            ..h.clone()
        };
        assert_eq!(
            typed_value(&side, "10, 4"),
            Some(serde_json::json!([60.0, 10.0, 4.0]))
        );
        assert!(typable(&h, ',') && typable(&h, ' ') && !typable(&h, 'a'));
        assert!(typing_hint(&h).is_some_and(|t| t.contains("X, Y")));
    }

    #[test]
    fn mouse_positions_become_values() {
        let (cam, rect) = cam_top();
        // 上から見て、z=0 の平面の上の点
        let h = Handle {
            key: "point".into(),
            label: String::new(),
            gesture: Gesture::Point {
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 0.0, 1.0],
                snap: 0.5,
            },
            outline: vec![],
            snap_to: vec![],
            choices: vec![],
            optional: false,
            faces: false,
            frame: None,
            fill: false,
            picked: vec![],
            ops: None,
            show_origin: false,
            shape: None,
            chain: false,
            vertices: false,
        };
        let v = value_at(&h, &cam, rect, Pos2::new(130.0, 80.0)).unwrap();
        let p: [f64; 3] = serde_json::from_value(v).unwrap();
        assert!(
            (p[0] - 30.0).abs() < 0.6 && (p[1] - 20.0).abs() < 0.6,
            "{p:?}"
        );
        assert!(p[2].abs() < 1e-3);
        // 径: 中心から 10 mm 離れた所で 20 mm
        let h = Handle {
            gesture: Gesture::Diameter {
                center: [0.0, 0.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            },
            ..h
        };
        let v = value_at(&h, &cam, rect, Pos2::new(110.0, 100.0)).unwrap();
        assert!((v.as_f64().unwrap() - 20.0).abs() < 0.6, "{v}");
        // 距離: 横から見て、z の向きの直線を上へ 15 mm
        let mut side = Camera::default();
        side.set_view(-90.0, 0.0);
        side.zoom = 1.0;
        let h = Handle {
            gesture: Gesture::Distance {
                origin: [0.0, 0.0, 0.0],
                dir: [0.0, 0.0, 1.0],
            },
            ..h
        };
        let v = value_at(&h, &side, rect, Pos2::new(140.0, 85.0)).unwrap();
        assert!((v.as_f64().unwrap() - 15.0).abs() < 0.6, "{v}");
        // 打った数字
        assert_eq!(typed_value(&h, "12.5"), Some(Json::from(12.5)));
        assert_eq!(typed_value(&h, "abc"), None);
    }

    #[test]
    fn a_choice_is_picked_inside_its_outline_and_the_inner_one_wins() {
        let (cam, rect) = cam_top();
        let square = |c: f64, r: f64| {
            vec![vec![
                [c - r, c - r, 0.0],
                [c + r, c - r, 0.0],
                [c + r, c + r, 0.0],
                [c - r, c + r, 0.0],
                [c - r, c - r, 0.0],
            ]]
        };
        let h = Handle {
            choices: vec![
                jc_core::HandleChoice {
                    value: Json::from(1),
                    label: "外".into(),
                    outline: square(0.0, 40.0),
                    group: 0,
                    ..Default::default()
                },
                jc_core::HandleChoice {
                    value: Json::from(2),
                    label: "内".into(),
                    outline: square(0.0, 10.0),
                    group: 0,
                    ..Default::default()
                },
            ],
            ..Handle::new("profile", "", Gesture::Choice)
        };
        // 画面の中心が (0, 0)。内側の四角の中は内側の候補、外の四角だけの所は外の候補
        assert_eq!(choice_at(&h, &cam, rect, Pos2::new(100.0, 100.0)), Some(1));
        assert_eq!(choice_at(&h, &cam, rect, Pos2::new(130.0, 70.0)), Some(0));
        assert_eq!(choice_at(&h, &cam, rect, Pos2::new(5.0, 5.0)), None);
        assert_eq!(describe_for(&h, &Json::from(2)), "内");
    }

    #[test]
    fn an_angle_follows_the_mouse_around_the_axis() {
        // X 軸まわりに点 (5, 10, 0) を回す。上から見ると円は線に潰れるので、同じ所に写る
        // 2 つの角度のうち手前(上)の方にする
        let (cam, rect) = cam_top();
        let h = Handle::new(
            "angle",
            "",
            Gesture::Angle {
                origin: [0.0; 3],
                axis: [1.0, 0.0, 0.0],
                from: [5.0, 10.0, 0.0],
            },
        );
        let at = |p: [f64; 3]| cam.project(f32s(p), rect).0;
        let deg = |p| value_at(&h, &cam, rect, at(p));
        assert_eq!(deg([5.0, 0.0, 10.0]), Some(serde_json::json!(90.0)));
        assert_eq!(deg([5.0, -10.0, 0.0]), Some(serde_json::json!(180.0)));
        assert_eq!(deg([5.0, 10.0, 0.0]), Some(serde_json::json!(360.0)));
        // 斜めから見れば、マウスの向きで決まる(270 度 = 下)
        let mut cam2 = Camera::default();
        cam2.set_view(-60.0, 30.0);
        cam2.zoom = 5.0;
        for want in [45.0, 135.0, 270.0] {
            let p = rotate([5.0, 10.0, 0.0], [0.0; 3], [1.0, 0.0, 0.0], want);
            let got = value_at(&h, &cam2, rect, cam2.project(f32s(p), rect).0);
            assert_eq!(got, Some(serde_json::json!(want)), "{want}");
        }
        assert_eq!(typed_value(&h, "45"), Some(serde_json::json!(45.0)));
        assert_eq!(typed_value(&h, "0"), None);
        assert_eq!(typed_value(&h, "400"), None);
        assert_eq!(describe_for(&h, &serde_json::json!(90.0)), "90.0°");
    }
}
