//! 歯の輪・腕・ボスの領域と、軽くする穴・軸のキー溝。寸法は mm。
//! 完全な輪郭と厚さごとの領域を同じ境界から作り、2D の輪郭に分割線を混ぜない。

use crate::tooth::{arc, polar};
use kmb_core::{Payload, Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

pub const PARTS_OUTPUT: &str = "parts";

/// 歯車のボディが厚さを分ける領域。同じ歯車 crate の輪郭とボディの間だけで受け渡す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parts {
    pub rim: Profile,
    pub web: Option<Profile>,
    pub boss: Option<Profile>,
}

impl Payload for Parts {
    const TYPE: &'static str = "gear.parts@1";
}

/// 穴とボスの寸法。キー溝の上書きは、片方だけなら残りを表から選ぶ。
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Params {
    pub count: u32,
    pub hole_diameter: f64,
    pub circle_diameter: f64,
    pub boss_diameter: f64,
    pub keyway: bool,
    pub key_width: Option<f64>,
    pub key_depth: Option<f64>,
}

/// JIS B 1301 の平行キー: 軸の穴の径の区分と、キーの幅・穴の側の溝の深さ。
/// 最初は 6 mm 以上 8 mm 以下、以降は下の端を含まず上の端を含む。
pub fn key_size(diameter: f64) -> Option<(f64, f64)> {
    if !(6.0..=65.0).contains(&diameter) {
        return None;
    }
    [
        (8.0, 2.0, 1.0),
        (10.0, 3.0, 1.4),
        (12.0, 4.0, 1.8),
        (17.0, 5.0, 2.3),
        (22.0, 6.0, 2.8),
        (30.0, 8.0, 3.3),
        (38.0, 10.0, 3.3),
        (44.0, 12.0, 3.3),
        (50.0, 14.0, 3.8),
        (58.0, 16.0, 4.3),
        (65.0, 18.0, 4.4),
    ]
    .into_iter()
    .find(|(upper, _, _)| diameter <= *upper)
    .map(|(_, width, depth)| (width, depth))
}

fn reverse(mut ring: ProfileLoop) -> ProfileLoop {
    ring.edges.reverse();
    for edge in &mut ring.edges {
        edge.reversed = !edge.reversed;
    }
    ring
}

fn circle(name: &str, center: [f64; 2], radius: f64) -> ProfileLoop {
    ProfileLoop {
        edges: vec![ProfileEdge {
            name: name.into(),
            segment: Segment::Circle { center, radius },
            reversed: false,
        }],
    }
}

/// 軸の穴とキー溝を一つの穴の輪として作る。重なる穴を別々に渡さない。
fn bore(radius: f64, key: Option<(f64, f64)>) -> ProfileLoop {
    let Some((width, depth)) = key else {
        return reverse(circle("bore", [0.0; 2], radius));
    };
    let x = width / 2.0;
    let y = (radius * radius - x * x).sqrt();
    let points = [[x, y], [x, radius + depth], [-x, radius + depth], [-x, y]];
    let mut edges: Vec<_> = ["bore:key:right", "bore:key:top", "bore:key:left"]
        .into_iter()
        .enumerate()
        .map(|(i, name)| ProfileEdge {
            name: name.into(),
            segment: Segment::Line {
                a: points[i],
                b: points[i + 1],
            },
            reversed: false,
        })
        .collect();
    edges.push(arc("bore", [0.0; 2], radius, points[3], points[0], true));
    reverse(ProfileLoop { edges })
}

pub struct Layout {
    pub profile: Profile,
    pub parts: Parts,
}

/// 歯底よりモジュール一つ分内側をリムの内縁とする。穴はその内側の腕にだけ並べる。
pub fn layout(
    outer: ProfileLoop,
    root_radius: f64,
    module: f64,
    bore_radius: f64,
    p: &Params,
) -> Result<Layout, String> {
    let bad = || "穴とボスの寸法は有限の 0 以上の値にしてください".to_string();
    if [
        p.hole_diameter,
        p.circle_diameter,
        p.boss_diameter,
        bore_radius,
    ]
    .iter()
    .any(|v| !v.is_finite() || *v < 0.0)
    {
        return Err(bad());
    }
    if p.count > 1000 {
        return Err("軽くする穴の数は 0 から 1000 までにしてください".into());
    }
    let rim_inner = root_radius - module;
    let boss_radius = p.boss_diameter / 2.0;
    if boss_radius > 0.0 && (boss_radius <= bore_radius || boss_radius >= rim_inner) {
        return Err(format!(
            "ボスの外径は軸の穴の径 {:.3} mm より大きく、リムの内径 {:.3} mm より小さくしてください",
            2.0 * bore_radius,
            2.0 * rim_inner
        ));
    }
    let key = if p.keyway {
        if bore_radius <= 0.0 {
            return Err("キー溝には軸の穴が必要です".into());
        }
        let selected = key_size(2.0 * bore_radius);
        let width = p.key_width.or(selected.map(|v| v.0));
        let depth = p.key_depth.or(selected.map(|v| v.1));
        let (Some(width), Some(depth)) = (width, depth) else {
            return Err(
                "軸の穴の径がキー溝の表の範囲(6〜65 mm)外です。幅と深さを指定してください".into(),
            );
        };
        if !width.is_finite()
            || width <= 0.0
            || width >= 2.0 * bore_radius
            || !depth.is_finite()
            || depth <= 0.0
        {
            return Err(
                "キー溝の幅は 0 より大きく軸の穴の径より小さく、深さは 0 より大きくしてください"
                    .into(),
            );
        }
        let limit = if boss_radius > 0.0 {
            boss_radius
        } else {
            rim_inner
        };
        if (width / 2.0).hypot(bore_radius + depth) >= limit {
            return Err(format!(
                "キー溝が{}の外へ出ます。幅か深さを小さくしてください",
                if boss_radius > 0.0 { "ボス" } else { "腕" }
            ));
        }
        Some((width, depth))
    } else {
        None
    };
    let bore_loop = (bore_radius > 0.0).then(|| bore(bore_radius, key));
    let mut holes = Vec::new();
    if p.count > 0 {
        let ring = p.circle_diameter / 2.0;
        let inner = boss_radius.max(bore_radius);
        let mut maximum = 2.0 * (ring - inner).min(rim_inner - ring).max(0.0);
        if p.count > 1 {
            maximum = maximum.min(2.0 * ring * (std::f64::consts::PI / p.count as f64).sin());
        }
        if p.hole_diameter <= 0.0 || p.hole_diameter >= maximum {
            return Err(format!(
                "軽くする穴がリム・ボス・隣の穴にかかります。置ける穴の径は {:.3} mm 未満です",
                maximum
            ));
        }
        // ボスが無い時も、キー溝の先が軽くする穴へつながらないことを確かめる。
        if let Some((width, depth)) = key {
            let ymin = (bore_radius * bore_radius - width * width / 4.0).sqrt();
            for i in 0..p.count {
                let c = polar(ring, TAU * i as f64 / p.count as f64);
                let nearest = [
                    c[0].clamp(-width / 2.0, width / 2.0),
                    c[1].clamp(ymin, bore_radius + depth),
                ];
                if (c[0] - nearest[0]).hypot(c[1] - nearest[1]) <= p.hole_diameter / 2.0 {
                    return Err(
                        "軽くする穴がキー溝にかかります。穴の径か並べる円の径を変えてください"
                            .into(),
                    );
                }
            }
        }
        for i in 0..p.count {
            holes.push(reverse(circle(
                &format!("lightening:{i}"),
                polar(ring, TAU * i as f64 / p.count as f64),
                p.hole_diameter / 2.0,
            )));
        }
    }
    let mut all_holes = holes.clone();
    all_holes.extend(bore_loop.clone());
    let profile = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: outer.clone(),
            holes: all_holes,
        }],
    };
    let with_region = |outer, holes| Profile {
        plane: Plane::XY,
        regions: vec![Region { outer, holes }],
    };
    let parts = if rim_inner > bore_radius {
        let rim = with_region(
            outer,
            vec![reverse(circle("rim:inner", [0.0; 2], rim_inner))],
        );
        let web_hole = if boss_radius > 0.0 {
            Some(reverse(circle("boss:outer", [0.0; 2], boss_radius)))
        } else {
            bore_loop.clone()
        };
        holes.extend(web_hole);
        Parts {
            rim,
            web: Some(with_region(circle("rim:inner", [0.0; 2], rim_inner), holes)),
            boss: (boss_radius > 0.0).then(|| {
                with_region(
                    circle("boss:outer", [0.0; 2], boss_radius),
                    bore_loop.into_iter().collect(),
                )
            }),
        }
    } else {
        Parts {
            rim: profile.clone(),
            web: None,
            boss: None,
        }
    };
    Ok(Layout { profile, parts })
}

impl Layout {
    pub fn placed(mut self, plane: Plane, phase: f64, center: [f64; 2]) -> Self {
        let place = |profile: &mut Profile| {
            profile.plane = plane;
            for region in &mut profile.regions {
                for ring in std::iter::once(&mut region.outer).chain(&mut region.holes) {
                    for edge in &mut ring.edges {
                        *edge = crate::tooth::translate(&crate::tooth::rotate(edge, phase), center);
                    }
                }
            }
        };
        place(&mut self.profile);
        place(&mut self.parts.rim);
        if let Some(web) = &mut self.parts.web {
            place(web);
        }
        if let Some(boss) = &mut self.parts.boss {
            place(boss);
        }
        self
    }
}

#[cfg(test)]
mod tests;
