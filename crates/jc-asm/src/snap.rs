//! 合わせるジョイントの原点の候補。
//!
//! クリックした面の上に候補を出し、クリックで選ぶ(取り方を先に切り替えずに、3 つの取り方の候補を
//! 一度に出す)。
//! - 単純: 面の中心、円の中心、頂点、辺の中点
//! - 2 面の間: この面と平行な、同じボディの面との真ん中(板の厚さの真ん中、溝の幅の真ん中)
//! - 2 辺の交点: 面の 2 本のまっすぐな辺を延ばした交点(角を丸めた・面取りした角の元の角)
//!
//! 円筒の面なら、両端の円の中心と軸の真ん中。原点の Z は面の外向き(円筒なら端の平らな面の外向き)、
//! X は辺の上の候補ならその辺の向き、ほかは面のいちばん長いまっすぐな辺の向き。

use crate::joint::{Frame, perpendicular};
use jc_core::kernel::{self as k, EdgeInfo, P3};
use jc_core::{Context, FacePick, FeatureId, HandleChoice, Value};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// 原点の候補 1 つ(手順の候補の値)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapPick {
    /// クリックした面のボディ(最後の機能)
    pub feature: FeatureId,
    pub frame: Frame,
    /// 取り方の名前(「円の中心」「2 面の間」など)
    pub label: String,
}

struct Found {
    point: P3,
    z: P3,
    x: P3,
    label: String,
}

fn near(a: P3, b: P3) -> bool {
    let d = k::sub(a, b);
    k::dot(d, d) < 1e-8
}

/// 面 `at` の上の原点の候補。
pub fn choices(cx: &Context<'_>, at: &FacePick) -> Vec<HandleChoice> {
    let Some(Value::Shape(h)) = cx.output(at.feature, "body") else {
        return Vec::new();
    };
    let faces = cx.kernel.faces(*h).unwrap_or_default();
    let Some(face) = faces.iter().find(|f| f.name == at.face) else {
        return Vec::new();
    };
    let on = |e: &EdgeInfo| e.faces.contains(&at.face);
    let straight: Vec<EdgeInfo> = cx
        .kernel
        .edges(*h)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| on(e) && !near(e.a, e.b))
        .collect();
    let arcs: Vec<EdgeInfo> = cx
        .kernel
        .arc_edges(*h)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| on(e) && e.arc.is_some())
        .collect();
    // 面の外形の点(大きさと中心を見る)
    let mut pts: Vec<P3> = straight.iter().flat_map(|e| [e.a, e.b]).collect();
    pts.extend(arcs.iter().flat_map(|e| e.polyline(24)));
    if pts.is_empty() {
        return Vec::new();
    }
    let (lo, hi) = pts
        .iter()
        .fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), p| {
            (
                [0, 1, 2].map(|i| lo[i].min(p[i])),
                [0, 1, 2].map(|i| hi[i].max(p[i])),
            )
        });
    let size = k::dot(k::sub(hi, lo), k::sub(hi, lo)).sqrt();
    let mut found = Vec::new();
    match face.planar {
        Some(pf) => {
            let n = k::normalize(pf.normal);
            let flat = |p: P3| k::sub(p, k::scale(n, k::dot(k::sub(p, pf.point), n)));
            let dir = |e: &EdgeInfo| k::normalize(k::sub(e.b, e.a));
            let x = straight
                .iter()
                .max_by(|a, b| {
                    let len = |e: &EdgeInfo| k::dot(k::sub(e.b, e.a), k::sub(e.b, e.a));
                    len(a).total_cmp(&len(b))
                })
                .map_or_else(|| perpendicular(n), dir);
            let center = flat([0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0));
            let mut add = |point: P3, x: P3, label: String| {
                found.push(Found {
                    point,
                    z: n,
                    x,
                    label,
                })
            };
            add(center, x, "面の中心".into());
            for e in &arcs {
                let a = e.arc.expect("円の辺だけを残した");
                add(a.center, x, format!("円の中心(半径 {:.2} mm)", a.radius));
            }
            for e in &straight {
                add(e.a, dir(e), "頂点".into());
                add(e.b, dir(e), "頂点".into());
            }
            for e in &straight {
                add(k::scale(k::add(e.a, e.b), 0.5), dir(e), "辺の中点".into());
            }
            // 2 面の間: 平行な面との隔たりごとに 1 つ
            let mut gaps: Vec<f64> = Vec::new();
            for f in faces.iter().filter(|f| f.name != at.face) {
                let Some(q) = f.planar else { continue };
                if k::dot(k::normalize(q.normal), n).abs() < 1.0 - 1e-6 {
                    continue;
                }
                let gap = k::dot(k::sub(q.point, pf.point), n);
                if gap.abs() < 1e-6 || gaps.iter().any(|g| (g - gap).abs() < 1e-6) {
                    continue;
                }
                gaps.push(gap);
                add(
                    k::add(center, k::scale(n, gap / 2.0)),
                    x,
                    format!("2 面の間({:.2} mm 離れた面との真ん中)", gap.abs()),
                );
            }
            // 2 辺の交点: 延ばした交点が頂点でなく、面の近くにある物
            let verts: Vec<P3> = straight.iter().flat_map(|e| [e.a, e.b]).collect();
            for (i, e1) in straight.iter().enumerate() {
                for e2 in &straight[i + 1..] {
                    let (d1, d2) = (dir(e1), dir(e2));
                    let c = k::cross(d1, d2);
                    let cc = k::dot(c, c);
                    if cc < 1e-10 {
                        continue;
                    }
                    // e1.a + s d1 = e2.a + t d2 の s(同じ平面の上の 2 直線)
                    let w = k::sub(e2.a, e1.a);
                    let s = k::dot(k::cross(w, d2), c) / cc;
                    let q = k::add(e1.a, k::scale(d1, s));
                    if verts.iter().any(|v| near(*v, q))
                        || k::dot(k::sub(q, center), k::sub(q, center)).sqrt() > size
                    {
                        continue;
                    }
                    add(q, d1, "2 辺の交点".into());
                }
            }
        }
        None => {
            // 円筒の面: 端の円の中心(Z は端の平らな面の外向き)と、その真ん中
            let normal_of = |name: &str| {
                faces
                    .iter()
                    .find(|f| f.name == name)
                    .and_then(|f| f.planar)
                    .map(|p| k::normalize(p.normal))
            };
            let mut ends: Vec<(P3, P3)> = Vec::new();
            for e in &arcs {
                let a = e.arc.expect("円の辺だけを残した");
                if ends.iter().any(|(c, _)| near(*c, a.center)) {
                    continue;
                }
                let z = normal_of(&e.faces[0]).unwrap_or_else(|| k::normalize(a.axis));
                ends.push((a.center, z));
            }
            for (c, z) in &ends {
                found.push(Found {
                    point: *c,
                    z: *z,
                    x: perpendicular(*z),
                    label: "円の中心(端)".into(),
                });
            }
            if let [(c1, z1), (c2, _), ..] = ends[..] {
                found.push(Found {
                    point: k::scale(k::add(c1, c2), 0.5),
                    z: z1,
                    x: perpendicular(z1),
                    label: "軸の真ん中".into(),
                });
            }
        }
    }
    // 同じ点は先に出た取り方だけ
    let mut out: Vec<Found> = Vec::new();
    for f in found {
        if !out.iter().any(|o| near(o.point, f.point)) {
            out.push(f);
        }
    }
    let r = (size * 0.03).clamp(0.3, 2.0);
    out.into_iter()
        .map(|f| {
            let pick = SnapPick {
                feature: at.feature,
                frame: Frame {
                    origin: f.point,
                    z: f.z,
                    x: f.x,
                },
                label: f.label.clone(),
            };
            HandleChoice {
                value: json!(pick),
                label: f.label,
                outline: marker(f.point, f.z, f.x, r),
                group: 0,
                sized: false,
                color: None,
            }
        })
        .collect()
}

/// 候補の印: 点のまわりの小さな円(内側をクリックしても選べる)と、Z の向きの短い線。
fn marker(p: P3, z: P3, x: P3, r: f64) -> Vec<Vec<P3>> {
    let (z, x) = (k::normalize(z), k::normalize(x));
    let y = k::cross(z, x);
    let circle = (0..=16)
        .map(|i| {
            let t = i as f64 / 16.0 * std::f64::consts::TAU;
            k::add(
                p,
                k::add(k::scale(x, r * t.cos()), k::scale(y, r * t.sin())),
            )
        })
        .collect();
    vec![circle, vec![p, k::add(p, k::scale(z, r * 2.5))]]
}
