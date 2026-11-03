//! 変位の線図からカムと従節の輪郭を求め、別のボディとして押し出す。

use crate::{Gear, tooth};
use kmb_cad::{Exprs, Operation, Recipe, angle, expr_refs, finish, length, output};
use kmb_core::{
    Curves, EvalCtx, EvalError, Feature, FeatureId, InputRef, Outputs, Payload, Profile,
    ProfileEdge, ProfileLoop, Region, Registry, Segment,
};
use kmb_interchange::{
    GearAxis,
    gear::{CAM_OUTPUT, CamSegment, CamSpec, Follower, FollowerTip, MotionLaw},
};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

pub const CAM: &str = "mech.cam@1";
pub const FOLLOWER_BODY: &str = "mech.cam_follower@1";
pub const FOLLOWER_PROFILE: &str = "follower_profile";

fn plane_default() -> kmb_cad::plane::PlaneRef {
    kmb_cad::plane::PlaneRef::OriginXy
}
fn base_default() -> String {
    "20 mm".into()
}
fn roller_default() -> String {
    "2 mm".into()
}
fn arm_default() -> String {
    "30 mm".into()
}
fn pivot_default() -> String {
    "40 mm".into()
}
fn zero() -> String {
    "0 mm".into()
}
fn width_default() -> String {
    "5 mm".into()
}
fn diagram_default() -> String {
    "上がり 120 deg 10 mm サイクロイド, 止まり 60 deg, 下がり 120 deg 単弦, 止まり 60 deg".into()
}
fn default_value<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    #[default]
    Ccw,
    Cw,
}
impl Rotation {
    fn sign(self) -> f64 {
        if self == Self::Ccw { 1.0 } else { -1.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cam {
    #[serde(default = "plane_default")]
    pub plane: kmb_cad::plane::PlaneRef,
    #[serde(default = "zero")]
    pub x: String,
    #[serde(default = "zero")]
    pub y: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<FeatureId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_point: Option<kmb_core::PointId>,
    #[serde(default = "base_default")]
    pub base_radius: String,
    #[serde(default, skip_serializing_if = "default_value")]
    pub rotation: Rotation,
    #[serde(default)]
    pub follower: Follower,
    #[serde(default)]
    pub tip: FollowerTip,
    #[serde(default = "roller_default")]
    pub roller_radius: String,
    #[serde(default = "zero")]
    pub offset: String,
    #[serde(default = "arm_default")]
    pub arm_length: String,
    #[serde(default = "pivot_default")]
    pub pivot_x: String,
    #[serde(default = "zero")]
    pub pivot_y: String,
    #[serde(default = "diagram_default")]
    pub diagram: String,
    #[serde(default = "width_default")]
    pub width: String,
}
impl Default for Cam {
    fn default() -> Self {
        Self {
            plane: kmb_cad::plane::PlaneRef::OriginXy,
            x: zero(),
            y: zero(),
            phase: None,
            anchor: None,
            anchor_point: None,
            base_radius: base_default(),
            rotation: Default::default(),
            follower: Default::default(),
            tip: Default::default(),
            roller_radius: roller_default(),
            offset: zero(),
            arm_length: arm_default(),
            pivot_x: pivot_default(),
            pivot_y: zero(),
            diagram: diagram_default(),
            width: width_default(),
        }
    }
}
impl Exprs for Cam {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut fields = vec![
            &mut self.x,
            &mut self.y,
            &mut self.base_radius,
            &mut self.roller_radius,
            &mut self.offset,
            &mut self.arm_length,
            &mut self.pivot_x,
            &mut self.pivot_y,
            &mut self.width,
        ];
        fields.extend(self.phase.iter_mut());
        fields
    }
}

#[derive(Debug, Clone)]
struct Term {
    direction: i32,
    span: String,
    lift: Option<String>,
    law: MotionLaw,
}
impl Term {
    fn render(&self) -> String {
        let verb = match self.direction {
            1 => "上がり",
            -1 => "下がり",
            _ => "止まり",
        };
        match &self.lift {
            Some(lift) => format!(
                "{verb} {} {lift} {}",
                self.span,
                match self.law {
                    MotionLaw::Uniform => "等速",
                    MotionLaw::SimpleHarmonic => "単弦",
                    MotionLaw::Cycloidal => "サイクロイド",
                    MotionLaw::Polynomial345 => "3-4-5",
                }
            ),
            None if self.direction < 0 => format!(
                "{verb} {} {}",
                self.span,
                match self.law {
                    MotionLaw::Uniform => "等速",
                    MotionLaw::SimpleHarmonic => "単弦",
                    MotionLaw::Cycloidal => "サイクロイド",
                    MotionLaw::Polynomial345 => "3-4-5",
                }
            ),
            None => format!("{verb} {}", self.span),
        }
    }
}

/// 区間の文法。数値と単位を一組ずつ読み、式の意味は共通の式評価へ渡す。
fn terms(src: &str) -> Result<Vec<Term>, String> {
    let mut result = Vec::new();
    for row in src
        .split([',', '、', ';', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let words: Vec<_> = row.split_whitespace().collect();
        let direction = match words.first().copied() {
            Some("上がり" | "rise") => 1,
            Some("下がり" | "fall") => -1,
            Some("止まり" | "dwell") => 0,
            _ => {
                return Err(format!(
                    "区間 `{row}` は上がり・下がり・止まりから始めてください"
                ));
            }
        };
        let implicit_fall = direction < 0 && words.len() == 4;
        let expected = if direction == 0 {
            3
        } else if implicit_fall {
            4
        } else {
            6
        };
        if words.len() != expected {
            return Err(format!(
                "区間 `{row}` は角度と単位、変位と単位、運動の法則を空白で区切ってください"
            ));
        }
        let law = if direction == 0 {
            MotionLaw::Uniform
        } else {
            match words[if implicit_fall { 3 } else { 5 }] {
                "等速" | "uniform" => MotionLaw::Uniform,
                "単弦" | "調和" | "harmonic" => MotionLaw::SimpleHarmonic,
                "サイクロイド" | "cycloidal" => MotionLaw::Cycloidal,
                "3-4-5" | "多項式" | "polynomial345" => MotionLaw::Polynomial345,
                _ => {
                    return Err(format!(
                        "区間 `{row}` の運動の法則は等速・単弦・サイクロイド・3-4-5から選んでください"
                    ));
                }
            }
        };
        result.push(Term {
            direction,
            span: format!("{} {}", words[1], words[2]),
            lift: (direction != 0 && !implicit_fall).then(|| format!("{} {}", words[3], words[4])),
            law,
        });
    }
    if result.is_empty() {
        return Err("変位の線図を入力してください".into());
    }
    Ok(result)
}

fn validate_program(segments: &[CamSegment]) -> Result<(), String> {
    if segments
        .iter()
        .any(|s| !s.span.is_finite() || s.span <= 0.0 || !s.lift.is_finite())
    {
        return Err("区間の角は有限の正の値、変位は有限の値にしてください".into());
    }
    let span: f64 = segments.iter().map(|s| s.span).sum();
    if (span - TAU).abs() > 1e-8 {
        return Err(format!(
            "区間の角の和は360 degにしてください(現在 {:.6} deg)",
            span.to_degrees()
        ));
    }
    let mut lift = 0.0f64;
    for segment in segments {
        lift += segment.lift;
        if !lift.is_finite() || lift < -1e-8 {
            return Err("変位は0以上の位置から動かし、基礎円より内側へ下げないでください".into());
        }
    }
    if !lift.is_finite() || lift.abs() > 1e-8 {
        return Err("一周後の変位を0へ戻してください".into());
    }
    Ok(())
}

fn displacement(segments: &[CamSegment], angle: f64) -> f64 {
    let mut theta = angle.rem_euclid(TAU);
    let mut at = 0.0;
    for s in segments {
        if theta <= s.span {
            return at + s.lift * s.law.ratio(theta / s.span);
        }
        theta -= s.span;
        at += s.lift;
    }
    at
}
/// 一度ごとの変位の差を、既存の諸元の区間へ入れる。角は軸の正の回転で測る。
fn sampled_segments(segments: &[CamSegment], rotation: Rotation) -> Vec<CamSegment> {
    let mut at = 0.0;
    (1..=360)
        .map(|degree| {
            let value = displacement(segments, (degree as f64).to_radians() * rotation.sign());
            let segment = CamSegment {
                span: 1f64.to_radians(),
                lift: value - at,
                law: MotionLaw::Uniform,
            };
            at = value;
            segment
        })
        .collect()
}

#[cfg(test)]
fn derivative(law: MotionLaw, u: f64) -> f64 {
    match law {
        MotionLaw::Uniform => 1.0,
        MotionLaw::SimpleHarmonic => PI / 2.0 * (PI * u).sin(),
        MotionLaw::Cycloidal => 1.0 - (TAU * u).cos(),
        MotionLaw::Polynomial345 => 30.0 * u * u * (1.0 - u) * (1.0 - u),
    }
}
#[cfg(test)]
fn second(law: MotionLaw, u: f64) -> f64 {
    match law {
        MotionLaw::Uniform => 0.0,
        MotionLaw::SimpleHarmonic => PI * PI / 2.0 * (PI * u).cos(),
        MotionLaw::Cycloidal => TAU * (TAU * u).sin(),
        MotionLaw::Polynomial345 => 60.0 * u * (1.0 - u) * (1.0 - 2.0 * u),
    }
}
fn rot(p: [f64; 2], a: f64) -> [f64; 2] {
    let (s, c) = a.sin_cos();
    [c * p[0] - s * p[1], s * p[0] + c * p[1]]
}
fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: [f64; 2], k: f64) -> [f64; 2] {
    [a[0] * k, a[1] * k]
}
fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn norm(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}
fn unit(a: [f64; 2]) -> [f64; 2] {
    mul(a, 1.0 / norm(a))
}
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

#[derive(Debug, Clone)]
struct ShapeInput {
    base: f64,
    roller: f64,
    offset: f64,
    follower: Follower,
    tip: FollowerTip,
    arm: f64,
    pivot: [f64; 2],
    rotation: Rotation,
    segments: Vec<CamSegment>,
    rest: f64,
}
impl ShapeInput {
    fn prepare(mut self) -> Result<Self, String> {
        validate_program(&self.segments)?;
        if !self.base.is_finite()
            || self.base <= 0.0
            || !self.roller.is_finite()
            || self.roller < 0.0
            || !(self.offset * self.offset).is_finite()
        {
            return Err(
                "基礎円の半径は有限の正の値、ローラーの半径は有限の0以上の値にしてください".into(),
            );
        }
        if self.tip == FollowerTip::Roller && self.roller <= 0.0 {
            return Err("ローラーの半径は0より大きくしてください".into());
        }
        let radius = self.base
            + if self.tip == FollowerTip::Roller {
                self.roller
            } else {
                0.0
            };
        if !(radius * radius * TAU).is_finite() {
            return Err("カムの寸法が大きすぎます".into());
        }
        if self.follower == Follower::Translating {
            if self.tip != FollowerTip::Flat && self.offset.abs() >= radius {
                return Err("ずれは基礎円とローラーを合わせた半径より小さくしてください".into());
            }
            self.rest = if self.tip == FollowerTip::Flat {
                self.base
            } else {
                (radius * radius - self.offset * self.offset).sqrt()
            };
        } else {
            let d = norm(self.pivot);
            if !(self.arm * self.arm).is_finite()
                || self.arm <= 0.0
                || !(d * d).is_finite()
                || d <= 0.0
            {
                return Err("揺動の腕の長さと支点までの距離は有限の正の値にしてください".into());
            }
            if self.tip == FollowerTip::Roller && self.arm <= self.roller {
                return Err("揺動の腕の長さはローラーの半径より大きくしてください".into());
            }
            let bearing = self.pivot[1].atan2(self.pivot[0]);
            if self.tip == FollowerTip::Flat {
                let c = (-self.base - self.arm) / d;
                if c.abs() >= 1.0 {
                    return Err("揺動の平らな先を基礎円に接する位置へ置けません。腕の長さか支点を見直してください".into());
                }
                let a = c.acos();
                let candidates = [bearing + a, bearing - a];
                self.rest = if candidates[0].sin() >= candidates[1].sin() {
                    candidates[0]
                } else {
                    candidates[1]
                };
            } else {
                let along = (radius * radius - self.arm * self.arm + d * d) / (2.0 * d);
                let squared = radius * radius - along * along;
                if squared <= 0.0 || !squared.is_finite() {
                    return Err(
                        "揺動の先を基礎円に置けません。腕の長さか支点を見直してください".into(),
                    );
                }
                let foot = mul(self.pivot, along / d);
                let side = mul([-self.pivot[1], self.pivot[0]], squared.sqrt() / d);
                let candidates = [add(foot, side), sub(foot, side)];
                let c = if candidates[0][1] >= candidates[1][1] {
                    candidates[0]
                } else {
                    candidates[1]
                };
                let v = sub(c, self.pivot);
                self.rest = v[1].atan2(v[0]);
            }
        }
        if !self.rest.is_finite() {
            return Err("カムと従節の寸法から有限の形を求められません".into());
        }
        Ok(self)
    }
    fn station(&self, theta: f64) -> ([f64; 2], [f64; 2]) {
        let lift = displacement(&self.segments, theta);
        if self.follower == Follower::Translating {
            ([self.offset, self.rest + lift], [0.0, 1.0])
        } else {
            let v = tooth::polar(1.0, self.rest + lift);
            (
                add(self.pivot, mul(v, self.arm)),
                if self.tip == FollowerTip::Flat {
                    mul(v, -1.0)
                } else {
                    v
                },
            )
        }
    }
    fn pitch(&self, theta: f64) -> [f64; 2] {
        rot(self.station(theta).0, -self.rotation.sign() * theta)
    }
    fn point(&self, theta: f64) -> [f64; 2] {
        let sign = self.rotation.sign();
        let h = 1e-5;
        match self.tip {
            FollowerTip::Knife => self.pitch(theta),
            FollowerTip::Roller => {
                let c = self.pitch(theta);
                let t = unit(sub(self.pitch(theta + h), self.pitch(theta - h)));
                add(c, mul([sign * t[1], -sign * t[0]], self.roller))
            }
            FollowerTip::Flat => {
                let (c, v) = self.station(theta);
                let n = rot(v, -sign * theta);
                let height = dot(c, v);
                let h_at = |t: f64| {
                    let (c, v) = self.station(t);
                    dot(c, v)
                };
                let dh = (h_at(theta + h) - h_at(theta - h)) / (2.0 * h);
                let (_, before) = self.station(theta - h);
                let (_, after) = self.station(theta + h);
                let angular = (before[0] * after[1] - before[1] * after[0])
                    .atan2(dot(before, after))
                    / (2.0 * h)
                    - sign;
                add(mul(n, height), mul([-n[1], n[0]], dh / angular))
            }
        }
    }
}

fn polygon(points: Vec<[f64; 2]>, prefix: &str) -> ProfileLoop {
    let mut edges: Vec<_> = (0..points.len())
        .map(|i| ProfileEdge {
            name: format!("{prefix}:{i}"),
            segment: Segment::Line {
                a: points[i],
                b: points[(i + 1) % points.len()],
            },
            reversed: false,
        })
        .collect();
    if (ProfileLoop {
        edges: edges.clone(),
    })
    .signed_area()
        < 0.0
    {
        edges.reverse();
        for e in &mut edges {
            e.reversed = !e.reversed;
        }
    }
    ProfileLoop { edges }
}
fn follower_ring(input: &ShapeInput) -> ProfileLoop {
    let (c, n) = input.station(0.0);
    let a = (input.base * 0.05)
        .max(0.5)
        .min(if input.follower == Follower::Oscillating {
            input.arm * 0.2
        } else {
            input.base * 0.2
        });
    let (direction, length) = if input.follower == Follower::Oscillating {
        (mul(n, -1.0), input.arm)
    } else {
        ([0.0, 1.0], input.base)
    };
    let phase = direction[1].atan2(direction[0]) - PI / 2.0;
    let place = |p: [f64; 2]| add(c, rot(p, phase));
    match input.tip {
        FollowerTip::Roller => {
            let half = a.min(input.roller * 0.5);
            let y = (input.roller * input.roller - half * half).sqrt();
            let left = place([-half, y]);
            let right = place([half, y]);
            let mut edges = vec![tooth::arc("roller", c, input.roller, left, right, true)];
            let end = if input.follower == Follower::Oscillating {
                length
            } else {
                y + length
            };
            let points = [right, place([half, end]), place([-half, end]), left];
            for i in 0..3 {
                edges.push(ProfileEdge {
                    name: format!("stem:{i}"),
                    segment: Segment::Line {
                        a: points[i],
                        b: points[i + 1],
                    },
                    reversed: false,
                });
            }
            ProfileLoop { edges }
        }
        FollowerTip::Knife => polygon(
            vec![
                c,
                place([a, 2.0 * a]),
                place([a, length]),
                place([-a, length]),
                place([-a, 2.0 * a]),
            ],
            "knife",
        ),
        FollowerTip::Flat => {
            let mut half = a;
            for degree in 0..360 {
                let t = (degree as f64).to_radians();
                let p = rot(input.point(t), input.rotation.sign() * t);
                let (q, v) = input.station(t);
                half = half.max(dot(sub(p, q), [-v[1], v[0]]).abs() + a);
            }
            let tangent = [-n[1], n[0]];
            let local = |x: f64, y: f64| add(add(c, mul(tangent, -x)), mul(n, y));
            if input.follower == Follower::Oscillating && half > a * (1.0 + 1e-8) {
                polygon(
                    vec![
                        local(-half, 0.0),
                        local(half, 0.0),
                        local(half, 2.0 * a),
                        local(a, 2.0 * a),
                        local(a, input.arm),
                        local(-a, input.arm),
                        local(-a, 2.0 * a),
                        local(-half, 2.0 * a),
                    ],
                    "flat_arm",
                )
            } else {
                polygon(
                    vec![
                        local(-half, 0.0),
                        local(half, 0.0),
                        local(half, 2.0 * a),
                        local(-half, 2.0 * a),
                    ],
                    "flat",
                )
            }
        }
    }
}

fn outlines(input: &ShapeInput) -> Result<(ProfileLoop, ProfileLoop, Vec<String>), String> {
    let mut warnings = Vec::new();
    let mut pressure = 0.0f64;
    let mut minimum = f64::INFINITY;
    for degree in 0..360 {
        let t = (degree as f64).to_radians();
        let h = 1e-4;
        let p = input.point(t);
        let tangent = sub(input.point(t + h), input.point(t - h));
        if p.iter().chain(tangent.iter()).any(|x| !x.is_finite()) || norm(tangent) < 1e-12 {
            return Err(format!(
                "{degree} degでカムの輪郭が尖るか有限でなくなります。基礎円や運動の線図を見直してください"
            ));
        }
        if input.tip == FollowerTip::Roller {
            let p = input.pitch(t);
            let prev = input.pitch(t - h);
            let next = input.pitch(t + h);
            let d = mul(sub(next, prev), 1.0 / (2.0 * h));
            let dd = mul(sub(add(next, prev), mul(p, 2.0)), 1.0 / (h * h));
            let cross = (d[0] * dd[1] - d[1] * dd[0]).abs();
            if cross > 1e-12 {
                minimum = minimum.min(norm(d).powi(3) / cross);
            }
            if input.follower == Follower::Translating {
                let motion = rot([0.0, 1.0], -input.rotation.sign() * t);
                let normal = unit([-d[1], d[0]]);
                pressure = pressure.max(dot(normal, motion).abs().clamp(0.0, 1.0).acos());
            }
        }
    }
    if pressure > 30f64.to_radians() {
        warnings.push(format!(
            "圧力角が30 degを超える所があります(最大 {:.1} deg)",
            pressure.to_degrees()
        ));
    }
    if minimum < input.roller {
        warnings.push(format!("ピッチ曲線の曲率半径 {:.3} mm がローラーの半径 {:.3} mm より小さく、ずらした輪郭が尖る所があります",minimum,input.roller));
    }
    if input
        .segments
        .iter()
        .any(|s| s.law == MotionLaw::Uniform && s.lift != 0.0)
    {
        warnings.push("等速の上がり・下がりは端で速度が変わります".into());
    }
    let curve = |t: f64| {
        let h = 1e-5;
        let p = input.point(t);
        let d = unit(sub(input.point(t + h), input.point(t - h)));
        (p, d)
    };
    let mut edges = Vec::new();
    for degree in 0..360 {
        let a = (degree as f64).to_radians();
        let b = ((degree + 1) as f64).to_radians();
        let mut from = input.point(a);
        for (k, seg) in tooth::fit_biarcs(&curve, a, b, tooth::tolerance(input.base))
            .into_iter()
            .enumerate()
        {
            let e = tooth::edge(format!("c{degree}:{k}"), seg, from);
            from = e.to_point();
            edges.push(e);
        }
    }
    if edges.is_empty() {
        return Err("カムの輪郭を作れません".into());
    }
    let mut ring = ProfileLoop { edges };
    if ring.signed_area() < 0.0 {
        ring.edges.reverse();
        for e in &mut ring.edges {
            e.reversed = !e.reversed;
        }
    }
    let follower = follower_ring(input);
    for loop_ in [&ring, &follower] {
        let area = loop_.signed_area();
        if !area.is_finite() || area <= 0.0 {
            return Err(
                "有限の面積を持つカムと従節の輪郭を作れません。寸法と運動の線図を見直してください"
                    .into(),
            );
        }
    }
    Ok((ring, follower, warnings))
}

impl Cam {
    fn placement(&self) -> Gear {
        Gear {
            plane: self.plane.clone(),
            x: self.x.clone(),
            y: self.y.clone(),
            phase: self.phase.clone(),
            anchor: self.anchor,
            anchor_point: self.anchor_point,
            ..Default::default()
        }
    }
    fn program(&self, ctx: &EvalCtx<'_>) -> Result<Vec<CamSegment>, EvalError> {
        let parsed = terms(&self.diagram).map_err(EvalError::InvalidData)?;
        let mut segments = Vec::new();
        let mut at = 0.0;
        for row in parsed {
            let span = angle(ctx, &row.span, "区間の角")?;
            let lift = match row.lift {
                None if row.direction < 0 => -at,
                None => 0.0,
                Some(src) => {
                    let value = if self.follower == Follower::Oscillating {
                        angle(ctx, &src, "揺動の変位")?
                    } else {
                        length(ctx, &src, "直動の変位")?
                    };
                    if value < 0.0 {
                        return Err(EvalError::InvalidData(
                            "区間の変位は0以上の値で打ち、上がりか下がりで向きを選んでください"
                                .into(),
                        ));
                    }
                    value * row.direction as f64
                }
            };
            at += lift;
            segments.push(CamSegment {
                span,
                lift,
                law: row.law,
            });
        }
        validate_program(&segments).map_err(EvalError::InvalidData)?;
        Ok(segments)
    }
}
impl Feature for Cam {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut refs = self.placement().inputs()?;
        for r in expr_refs(self)? {
            if !refs.contains(&r) {
                refs.push(r);
            }
        }
        for row in terms(&self.diagram).map_err(EvalError::InvalidData)? {
            for src in std::iter::once(row.span).chain(row.lift) {
                for r in kmb_core::expr_inputs(&src)? {
                    if !refs.contains(&r) {
                        refs.push(r);
                    }
                }
            }
        }
        Ok(refs)
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let segments = self.program(ctx)?;
        let input = ShapeInput {
            base: length(ctx, &self.base_radius, "基礎円の半径")?,
            roller: length(ctx, &self.roller_radius, "ローラーの半径")?,
            offset: length(ctx, &self.offset, "ずれ")?,
            follower: self.follower,
            tip: self.tip,
            arm: length(ctx, &self.arm_length, "腕の長さ")?,
            pivot: [
                length(ctx, &self.pivot_x, "支点 x")?,
                length(ctx, &self.pivot_y, "支点 y")?,
            ],
            rotation: self.rotation,
            segments,
            rest: 0.0,
        }
        .prepare()
        .map_err(EvalError::InvalidData)?;
        let width = length(ctx, &self.width, "カムの厚さ")?;
        if !width.is_finite() || width <= 0.0 {
            return Err(EvalError::InvalidData(
                "カムの厚さは有限の0より大きい値にしてください".into(),
            ));
        }
        let (plane, center) = self.placement().origin(ctx)?;
        let phase = self
            .phase
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map_or(Ok(0.0), |s| angle(ctx, s, "回す角"))?;
        let (cam, follower, warnings) = outlines(&input).map_err(EvalError::InvalidData)?;
        let place = |ring: ProfileLoop| ProfileLoop {
            edges: ring
                .edges
                .iter()
                .map(|e| tooth::translate(&tooth::rotate(e, phase), center))
                .collect(),
        };
        let profile = Profile {
            plane,
            regions: vec![Region {
                outer: place(cam),
                holes: vec![],
            }],
        };
        let follower = Profile {
            plane,
            regions: vec![Region {
                outer: place(follower),
                holes: vec![],
            }],
        };
        let table = sampled_segments(&input.segments, input.rotation);
        let reference = if input.follower == Follower::Oscillating {
            rot(unit(input.pivot), phase)
        } else {
            rot([1.0, 0.0], phase)
        };
        let ref_world = std::array::from_fn(|i| {
            plane.x_axis[i] * reference[0] + plane.y_axis[i] * reference[1]
        });
        let spec = CamSpec {
            follower: self.follower,
            tip: self.tip,
            base_radius: input.base,
            roller_radius: if self.tip == FollowerTip::Roller {
                input.roller
            } else {
                0.0
            },
            offset: input.offset,
            arm: (self.follower == Follower::Oscillating).then_some((input.arm, norm(input.pivot))),
            segments: table,
            axis: GearAxis {
                origin: plane.to_world(center),
                direction: plane.normal(),
                reference: ref_world,
            },
        };
        let recipe = Recipe::Extrude {
            operation: Operation::New,
            profile: profile.clone(),
            from: 0.0,
            to: width,
            through: false,
            stop: None,
            taper: 0.0,
        };
        let mut out = finish(ctx, recipe, None)?;
        out.insert(
            "curves".into(),
            Curves {
                plane,
                segments: profile.regions[0]
                    .outer
                    .edges
                    .iter()
                    .map(|e| e.segment.clone())
                    .collect(),
            }
            .to_value(),
        );
        out.insert("profile".into(), profile.to_value());
        out.insert(FOLLOWER_PROFILE.into(), follower.to_value());
        out.insert(CAM_OUTPUT.into(), spec.to_value());
        kmb_core::add_warnings(&mut out, warnings);
        Ok(out)
    }
    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut copy = self.clone();
        let mut changed = false;
        for src in copy.exprs_mut() {
            let renamed = kmb_core::rename_reference(src, old, new)?;
            changed |= renamed != *src;
            *src = renamed;
        }
        let mut parsed = terms(&copy.diagram).map_err(EvalError::InvalidData)?;
        for row in &mut parsed {
            let renamed = kmb_core::rename_reference(&row.span, old, new)?;
            changed |= renamed != row.span;
            row.span = renamed;
            if let Some(src) = &mut row.lift {
                let renamed = kmb_core::rename_reference(src, old, new)?;
                changed |= renamed != *src;
                *src = renamed;
            }
        }
        if changed {
            copy.diagram = parsed
                .iter()
                .map(Term::render)
                .collect::<Vec<_>>()
                .join(", ");
            Ok(Some(
                serde_json::to_value(copy).expect("カムはJSONにできる"),
            ))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowerBody {
    pub cam: FeatureId,
    #[serde(default = "width_default")]
    pub width: String,
}
impl Exprs for FollowerBody {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.width]
    }
}
impl Feature for FollowerBody {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut refs = vec![output(self.cam, FOLLOWER_PROFILE)];
        refs.extend(expr_refs(self)?);
        Ok(refs)
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let profile = Profile::from_value(ctx.output(self.cam, FOLLOWER_PROFILE)?)
            .ok_or_else(|| EvalError::InvalidData("従節の輪郭を持つカムを選んでください".into()))?;
        let width = length(ctx, &self.width, "従節の厚さ")?;
        if !width.is_finite() || width <= 0.0 {
            return Err(EvalError::InvalidData(
                "従節の厚さは有限の0より大きい値にしてください".into(),
            ));
        }
        let mut out = finish(
            ctx,
            Recipe::Extrude {
                operation: Operation::New,
                profile: profile.clone(),
                from: 0.0,
                to: width,
                through: false,
                stop: None,
                taper: 0.0,
            },
            None,
        )?;
        out.insert("profile".into(), profile.to_value());
        Ok(out)
    }
    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        kmb_cad::rename(self, old, new)
    }
}

pub fn register(reg: &mut Registry) {
    use kmb_core::{Descriptor, Field};
    reg.register_described(
        Descriptor::new(CAM, kmb_core::category::SOLID, "カム")
            .icon("mechanism")
            .hint("変位の線図からカムと従節の輪郭を作り、別のボディへ押し出す")
            .aliases("かむ cam follower")
            .field(Field::choice(
                "plane.kind",
                "平面",
                &[
                    ("origin_xy", "原点の XY 平面"),
                    ("origin_yz", "原点の YZ 平面"),
                    ("origin_zx", "原点の ZX 平面"),
                ],
            ))
            .field(Field::expr("base_radius", "基礎円の半径").length())
            .field(Field::choice(
                "rotation",
                "回る向き",
                &[("ccw", "反時計回り"), ("cw", "時計回り")],
            ))
            .field(Field::choice(
                "follower",
                "従節の種類",
                &[("translating", "直動"), ("oscillating", "揺動")],
            ))
            .field(Field::choice(
                "tip",
                "先の形",
                &[
                    ("knife", "ナイフ"),
                    ("roller", "ローラー"),
                    ("flat", "平ら"),
                ],
            ))
            .field(
                Field::expr("roller_radius", "ローラーの半径")
                    .length()
                    .only("tip", "roller", "roller"),
            )
            .field(Field::expr("offset", "ずれ").length().only(
                "follower",
                "translating",
                "translating",
            ))
            .field(Field::expr("arm_length", "腕の長さ").length().only(
                "follower",
                "oscillating",
                "translating",
            ))
            .field(Field::expr("pivot_x", "支点 x").length().only(
                "follower",
                "oscillating",
                "translating",
            ))
            .field(Field::expr("pivot_y", "支点 y").length().only(
                "follower",
                "oscillating",
                "translating",
            ))
            .field(Field::text("diagram", "変位の線図").help(
                "角度と変位は数と単位を空白で区切る。直動は mm、揺動は deg。区間の角の和は360 deg",
            ))
            .field(Field::expr("width", "カムの厚さ").length())
            .field(Field::opt_output("anchor", "中心の点のスケッチ", "points"))
            .field(Field::expr("x", "中心 x").length())
            .field(Field::expr("y", "中心 y").length())
            .field(Field::opt_expr("phase", "回す角").angle())
            .create_defaults(serde_json::to_value(Cam::default()).expect("カムはJSONにできる"))
            .companion(FOLLOWER_BODY, serde_json::json!({"cam":{"$new":0}})),
        |d| {
            let cam: Cam = serde_json::from_value(d.clone())
                .map_err(|e| EvalError::InvalidData(e.to_string()))?;
            Ok(Box::new(cam))
        },
    );
    reg.register_described(
        kmb_cad::solid(FOLLOWER_BODY, "カムの従節")
            .icon("mechanism")
            .hint("カムの従節の輪郭を別のボディへ押し出す")
            .field(Field::output("cam", "カム", FOLLOWER_PROFILE))
            .field(Field::expr("width", "従節の厚さ").length()),
        |d| {
            let f: FollowerBody = serde_json::from_value(d.clone())
                .map_err(|e| EvalError::InvalidData(e.to_string()))?;
            Ok(Box::new(f))
        },
    );
}

/// カムの配置と作成を画面・CLI・MCPの同じ引数宣言で受ける。
pub struct CamCommands;
pub const CAM_ADD: &str = "mech.cam";
const NEW_CAM: FeatureId = FeatureId(u64::MAX);

fn command_args() -> Vec<kmb_core::Arg> {
    use kmb_core::Arg;
    let mut args = kmb_cad::plane::args();
    args.extend([
        Arg::vector("point", "中心の世界座標(mm)。省くと平面の原点"),
        Arg::angle("phase", "回す角").default(0.0),
        Arg::length("base_radius", "基礎円の半径").default(20.0),
        Arg::choice(
            "rotation",
            "回る向き",
            &[("ccw", "反時計回り"), ("cw", "時計回り")],
        )
        .default("ccw"),
        Arg::choice(
            "follower",
            "従節の種類",
            &[("translating", "直動"), ("oscillating", "揺動")],
        )
        .default("translating"),
        Arg::choice(
            "tip",
            "先の形",
            &[
                ("knife", "ナイフ"),
                ("roller", "ローラー"),
                ("flat", "平ら"),
            ],
        )
        .default("roller"),
        Arg::length("roller_radius", "ローラーの半径").default(2.0),
        Arg::length("offset", "ずれ").default(0.0),
        Arg::length("arm_length", "揺動の腕の長さ").default(30.0),
        Arg::length("pivot_x", "揺動の支点 x").default(40.0),
        Arg::length("pivot_y", "揺動の支点 y").default(0.0),
        Arg::text("diagram", "変位の線図").default(diagram_default()),
        Arg::length("width", "カムの厚さ").default(5.0),
        Arg::length("follower_width", "従節の厚さ").default(5.0),
    ]);
    args
}

/// クリックや宣言した引数で選んだ平面と、中心の世界座標を読む座標系。
fn command_plane(
    cx: &kmb_core::Context<'_>,
) -> Result<(kmb_cad::plane::PlaneRef, kmb_core::Plane), String> {
    use kmb_cad::plane::PlaneRef;
    use kmb_core::{Plane, Value};
    let plane = if cx.arg("plane").is_some() {
        PlaneRef::from_choice(cx, NEW_CAM, "カムを置く平面")?
    } else if let (Some(picked), Some(_)) = (cx.picked, cx.picked_plane()) {
        PlaneRef::Face {
            feature: Some(picked.feature),
            face: Some(picked.face.clone()),
        }
    } else {
        PlaneRef::OriginXy
    };
    let frame = match &plane {
        PlaneRef::OriginXy => Plane::XY,
        PlaneRef::OriginYz => Plane::YZ,
        PlaneRef::OriginZx => Plane::ZX,
        PlaneRef::Datum { feature } => cx
            .output(*feature, kmb_interchange::DATUM_OUTPUT)
            .and_then(kmb_interchange::DatumPlane::from_value)
            .map(|p| p.plane)
            .ok_or_else(|| "選んだ作業平面を読めません".to_string())?,
        PlaneRef::Face {
            feature: Some(feature),
            face: Some(face),
        } => {
            let Some(Value::Shape(shape)) = cx.output(*feature, "body") else {
                return Err("カムを置く面のボディを読めません".into());
            };
            let f = cx
                .kernel
                .faces(*shape)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|f| &f.name == face)
                .and_then(|f| f.planar)
                .ok_or_else(|| "カムを置く面は平らな面にしてください".to_string())?;
            Plane::from_point_normal(f.point, f.normal)
        }
        _ => return Err("カムを置く平面を選んでください".into()),
    };
    Ok((plane, frame))
}

impl kmb_core::Extension for CamCommands {
    fn actions(&self, _cx: &kmb_core::Context<'_>) -> Vec<kmb_core::Action> {
        vec![
            kmb_core::Action::new(CAM_ADD, "カム", "cad")
                .replaces(CAM)
                .args(command_args())
                .menu("作成")
                .icon("mechanism")
                .aliases("かむ cam じゅうせつ follower")
                .hint("平面と中心を選び、変位の線図からカムと従節を別のボディで作る")
                .at(&[kmb_core::Place::Menu]),
        ]
    }
    fn run(
        &self,
        id: &str,
        cx: &kmb_core::Context<'_>,
    ) -> Option<Result<kmb_core::Outcome, kmb_core::Problem>> {
        if id != CAM_ADD {
            return None;
        }
        Some((|| {
            let (plane, frame) = command_plane(cx)?;
            let center = if let Some(point) = cx.arg("point") {
                let point: [f64; 3] = serde_json::from_value(point.clone())
                    .map_err(|_| "中心は世界座標の3つの数で指定してください")?;
                frame.to_local(point)
            } else {
                [0.0; 2]
            };
            let numeric = |key: &str, default: f64| cx.arg_f64(key).map(|v| v.unwrap_or(default));
            let base = numeric("base_radius", 20.0)?;
            let width = numeric("width", 5.0)?;
            let follower_width = numeric("follower_width", 5.0)?;
            if !base.is_finite()
                || base <= 0.0
                || !width.is_finite()
                || width <= 0.0
                || !follower_width.is_finite()
                || follower_width <= 0.0
            {
                return Err("基礎円の半径とボディの厚さは有限の0より大きい値にしてください".into());
            }
            let follower = if cx.arg_str("follower") == Some("oscillating") {
                Follower::Oscillating
            } else {
                Follower::Translating
            };
            let tip = match cx.arg_str("tip").unwrap_or("roller") {
                "knife" => FollowerTip::Knife,
                "flat" => FollowerTip::Flat,
                _ => FollowerTip::Roller,
            };
            let data = Cam {
                plane,
                x: format!("{} mm", center[0]),
                y: format!("{} mm", center[1]),
                phase: Some(format!("{} deg", numeric("phase", 0.0)?)),
                base_radius: format!("{base} mm"),
                rotation: if cx.arg_str("rotation") == Some("cw") {
                    Rotation::Cw
                } else {
                    Rotation::Ccw
                },
                follower,
                tip,
                roller_radius: format!("{} mm", numeric("roller_radius", 2.0)?),
                offset: format!("{} mm", numeric("offset", 0.0)?),
                arm_length: format!("{} mm", numeric("arm_length", 30.0)?),
                pivot_x: format!("{} mm", numeric("pivot_x", 40.0)?),
                pivot_y: format!("{} mm", numeric("pivot_y", 0.0)?),
                diagram: cx
                    .arg_str("diagram")
                    .map(str::to_string)
                    .unwrap_or_else(diagram_default),
                width: format!("{width} mm"),
                ..Default::default()
            };
            terms(&data.diagram)?;
            let edits = vec![
                kmb_core::Edit::Insert {
                    type_id: CAM.into(),
                    name: format!("カム{}", cx.count_of(CAM) + 1),
                    data: serde_json::to_value(data).expect("カムはJSONにできる"),
                },
                kmb_core::Edit::Insert {
                    type_id: FOLLOWER_BODY.into(),
                    name: format!("カムの従節{}", cx.count_of(FOLLOWER_BODY) + 1),
                    data: serde_json::json!({"cam":{"$new":0},"width":format!("{follower_width} mm")}),
                },
            ];
            Ok(kmb_core::Outcome::Change {
                label: "カムを追加".into(),
                edits,
            })
        })())
    }
    fn handles(&self, id: &str, cx: &kmb_core::Context<'_>) -> Option<Vec<kmb_core::Handle>> {
        use kmb_core::{Gesture, Handle};
        if id != CAM_ADD {
            return None;
        }
        let given = cx.arg("plane").is_some();
        let plane = || Handle {
            choices: kmb_cad::plane::plane_choices(cx, NEW_CAM),
            faces: true,
            ..Handle::new("plane", "カムを置く平面をクリック", Gesture::Choice)
        };
        if !given && cx.picked_plane().is_none() {
            return Some(vec![
                plane(),
                Handle::new("point", "カムの中心をクリック", Gesture::Face),
            ]);
        }
        let (_, frame) = command_plane(cx).ok()?;
        let point = Handle::new(
            "point",
            "カムの中心をクリック",
            Gesture::Point {
                origin: frame.origin,
                normal: frame.normal(),
                snap: 0.5,
            },
        );
        Some(if given {
            vec![plane(), point]
        } else {
            vec![point]
        })
    }
}

#[cfg(test)]
#[path = "cam_tests.rs"]
mod tests;
