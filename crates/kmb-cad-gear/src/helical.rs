//! 歯直角の諸元から正面の断面と、歯の幅に沿うねじりを求める。

use crate::{kinds::Params, tooth};
use kmb_cad::{Operation, Recipe};
use kmb_core::{PlanarPath, Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
use kmb_interchange::{GearKind, GearSpec, gear::Hand};

/// 歯直角のモジュールと圧力角を、軸に直角な断面の値へ移す。
pub fn transverse(normal: &Params, beta: f64) -> Result<Params, String> {
    if !beta.is_finite() || !(0.0..std::f64::consts::FRAC_PI_2).contains(&beta) {
        return Err("ねじれ角は 0 deg 以上、90 deg より小さくしてください".into());
    }
    if !normal.pressure_angle.is_finite()
        || normal.pressure_angle <= 0.0
        || normal.pressure_angle >= 45f64.to_radians()
    {
        return Err("歯直角の圧力角は 0 deg より大きく 45 deg より小さくしてください".into());
    }
    let cos = beta.cos();
    let module = normal.module / cos;
    let pressure_angle = (normal.pressure_angle.tan() / cos).atan();
    if !module.is_finite() || module <= 0.0 || !pressure_angle.is_finite() {
        return Err("ねじれ角から有限の正面のモジュールと圧力角を求められません".into());
    }
    if beta == 0.0 {
        return Ok(*normal);
    }
    Ok(Params {
        module,
        pressure_angle,
        ..*normal
    })
}

/// 軸の正の向きへ `width` 進む時のねじり。右歯は正、左歯は負。
pub fn twist(width: f64, spec: &GearSpec) -> Result<f64, String> {
    if !width.is_finite()
        || width <= 0.0
        || !spec.pitch_radius.is_finite()
        || spec.pitch_radius <= 0.0
    {
        return Err("歯の幅とピッチ円の半径は有限の 0 より大きい数にしてください".into());
    }
    if !spec.helix_angle.is_finite()
        || !(0.0..std::f64::consts::FRAC_PI_2).contains(&spec.helix_angle)
    {
        return Err("ねじれ角は 0 deg 以上、90 deg より小さくしてください".into());
    }
    let sign = match spec.hand {
        Hand::Right => 1.0,
        Hand::Left => -1.0,
        Hand::Both => {
            return Err(
                "ねじれの向きは右か左を選んでください。やまばはボディの欄で選びます".into(),
            );
        }
    };
    let value = sign * width * spec.helix_angle.tan() / spec.pitch_radius;
    if !value.is_finite() {
        return Err("ねじり角が大きすぎます。歯の幅かねじれ角を小さくしてください".into());
    }
    Ok(value)
}

fn section(profile: &Profile, spec: &GearSpec, height: f64, phase: f64) -> Profile {
    let mut p = profile.clone();
    let center = p.plane.to_local(spec.axis.origin);
    p.plane.origin = std::array::from_fn(|i| p.plane.origin[i] + height * spec.axis.direction[i]);
    for region in &mut p.regions {
        for ring in std::iter::once(&mut region.outer).chain(&mut region.holes) {
            for e in &mut ring.edges {
                *e = tooth::translate(
                    &tooth::rotate(&tooth::translate(e, [-center[0], -center[1]]), phase),
                    center,
                );
            }
        }
    }
    p
}

fn sweep(
    profile: &Profile,
    spec: &GearSpec,
    from: f64,
    to: f64,
    phase: f64,
    turn: f64,
    op: Operation,
) -> Recipe {
    let p = section(profile, spec, from, phase);
    let path = PlanarPath {
        plane: Plane {
            origin: std::array::from_fn(|i| spec.axis.origin[i] + from * spec.axis.direction[i]),
            x_axis: spec.axis.reference,
            y_axis: spec.axis.direction,
        },
        edges: vec![ProfileEdge {
            name: "width".into(),
            segment: Segment::Line {
                a: [0.0, 0.0],
                b: [0.0, to - from],
            },
            reversed: false,
        }],
    };
    Recipe::Sweep {
        operation: op,
        profile: p,
        path,
        twist: turn,
        scale: 1.0,
    }
}

/// 中央の溝をまっすぐにつなげる、軸と同心の円形の穴だけか。
pub fn concentric_holes(profile: &Profile, spec: &GearSpec) -> bool {
    let center = profile.plane.to_local(spec.axis.origin);
    profile.regions.iter().flat_map(|r| &r.holes).all(|hole| {
        matches!(hole.edges.as_slice(), [ProfileEdge {segment: Segment::Circle {center: c,..},..}] if (c[0]-center[0]).hypot(c[1]-center[1]) <= 1e-9)
    })
}

/// 輪郭を幅方向へねじる道具。やまばは中央で左右の歯すじを合わせる。
pub fn recipe(
    profile: &Profile,
    spec: &GearSpec,
    from: f64,
    to: f64,
    herringbone: bool,
    gap: f64,
    op: Operation,
) -> Result<Recipe, String> {
    let width = to - from;
    if !from.is_finite() || !to.is_finite() || !width.is_finite() || width <= 0.0 {
        return Err("歯の幅は有限の 0 より大きい数にしてください".into());
    }
    if !gap.is_finite() || gap < 0.0 || gap >= width {
        return Err("中央の溝の幅は 0 以上、歯の幅より小さくしてください".into());
    }
    if !herringbone && gap != 0.0 {
        return Err("中央の溝は、やまばの時に指定してください".into());
    }
    let full_turn = twist(width, spec)?;
    if !herringbone {
        return Ok(sweep(profile, spec, from, to, 0.0, full_turn, op));
    }
    if op == Operation::Intersect {
        return Err(
            "やまば歯車の交差はまだ作れません。新規ボディ・結合・切り取りを選んでください".into(),
        );
    }
    let middle = from + width / 2.0;
    let half = (width - gap) / 2.0;
    let turn = twist(half, spec)?;
    let first = sweep(profile, spec, from, middle - gap / 2.0, 0.0, turn, op);
    let second = sweep(profile, spec, middle + gap / 2.0, to, turn, -turn, op);
    let mut parts = vec![first];
    if gap > 0.0 {
        let offset = spec.transverse_module() * (1.25 - spec.profile_shift);
        let radius = match spec.kind {
            GearKind::External => spec.pitch_radius - offset,
            GearKind::Internal => {
                spec.pitch_radius + spec.transverse_module() * (1.25 + spec.profile_shift)
            }
            _ => return Err("やまばは外歯か内歯の歯車に指定してください".into()),
        };
        if !radius.is_finite() || radius <= 0.0 {
            return Err("中央の溝の歯底円を作れません".into());
        }
        let center = profile.plane.to_local(spec.axis.origin);
        let root_circle = ProfileLoop {
            edges: vec![ProfileEdge {
                name: "groove_root".into(),
                segment: Segment::Circle { center, radius },
                reversed: spec.kind == GearKind::Internal,
            }],
        };
        let region = if spec.kind == GearKind::Internal {
            let outer = profile
                .regions
                .first()
                .ok_or("内歯車の輪郭がありません")?
                .outer
                .clone();
            Region {
                outer,
                holes: vec![root_circle],
            }
        } else {
            Region {
                outer: root_circle,
                holes: profile
                    .regions
                    .iter()
                    .flat_map(|r| r.holes.iter().cloned())
                    .collect(),
            }
        };
        // 中央の円柱を先につなぎ、離れた左右の半分を順に結合できるようにする。
        parts.push(Recipe::Extrude {
            operation: op,
            profile: Profile {
                plane: profile.plane,
                regions: vec![region],
            },
            from: middle - gap / 2.0,
            to: middle + gap / 2.0,
            through: false,
            stop: None,
            taper: 0.0,
        });
    }
    parts.push(second);
    Ok(Recipe::Many { parts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;
    fn spec(hand: Hand) -> GearSpec {
        GearSpec {
            kind: GearKind::External,
            form: Default::default(),
            module: 2.0,
            normal_module: true,
            pressure_angle: 20f64.to_radians(),
            teeth: 20,
            profile_shift: 0.0,
            pitch_radius: 20.0 / 30f64.to_radians().cos(),
            axis: Default::default(),
            phase: 0.0,
            face_width: None,
            helix_angle: 30f64.to_radians(),
            hand,
            cone_angle: 0.0,
            chain_pitch: None,
            starts: 1,
        }
    }
    #[test]
    fn transverse_module_pressure_and_pitch_follow_normal_values() {
        for beta in [0.0f64, 15.0, 30.0, 45.0] {
            let b = beta.to_radians();
            let p = transverse(
                &Params {
                    module: 2.0,
                    teeth: 20,
                    ..Params::default()
                },
                b,
            )
            .unwrap();
            assert!((p.module - 2.0 / b.cos()).abs() < 1e-12);
            assert!((p.pressure_angle.tan() - 20f64.to_radians().tan() / b.cos()).abs() < 1e-12);
            let o = crate::kinds::outline(GearKind::External, &p).unwrap();
            let r = o
                .reference
                .iter()
                .find(|(name, _, _)| *name == "pitch")
                .unwrap()
                .2;
            assert!((2.0 * r - p.module * 20.0).abs() < 1e-12);
        }
    }
    #[test]
    fn twist_changes_sign_with_hand_and_scales_with_width() {
        let s = spec(Hand::Right);
        let want = 20.0 * s.helix_angle.tan() / s.pitch_radius;
        assert!((twist(20.0, &s).unwrap() - want).abs() < 1e-12);
        assert!((twist(20.0, &spec(Hand::Left)).unwrap() + want).abs() < 1e-12);
        assert!((twist(10.0, &s).unwrap() * 2.0 - want).abs() < 1e-12);
        assert!(transverse(&Params::default(), PI / 2.0).is_err());
        assert!(
            transverse(
                &Params {
                    pressure_angle: 200f64.to_radians(),
                    ..Params::default()
                },
                0.0
            )
            .is_err()
        );
        assert!(
            twist(
                f64::MAX,
                &GearSpec {
                    helix_angle: 80f64.to_radians(),
                    ..s
                }
            )
            .is_err()
        );
    }
    #[test]
    fn the_second_half_starts_at_the_first_halfs_end_phase() {
        let s = spec(Hand::Right);
        let edges = crate::kinds::outline(GearKind::External, &Params::default())
            .unwrap()
            .edges;
        let p = Profile {
            plane: Plane::XY,
            regions: vec![Region {
                outer: ProfileLoop { edges },
                holes: vec![],
            }],
        };
        let Recipe::Many { parts } = recipe(&p, &s, 0.0, 20.0, true, 2.0, Operation::New).unwrap()
        else {
            panic!("やまば")
        };
        let Recipe::Sweep { twist: turn, .. } = &parts[0] else {
            panic!("前半")
        };
        let Recipe::Sweep {
            profile: second,
            twist: back,
            ..
        } = parts.last().unwrap()
        else {
            panic!("後半")
        };
        let expected = section(&p, &s, 11.0, *turn);
        assert_eq!(*second, expected);
        assert!((turn + back).abs() < 1e-12);
    }

    #[test]
    fn internal_grooves_keep_the_outer_ring_and_clear_the_root_circle() {
        let mut s = spec(Hand::Right);
        s.kind = GearKind::Internal;
        s.profile_shift = 0.2;
        let p = Profile {
            plane: Plane::XY,
            regions: vec![Region {
                outer: ProfileLoop {
                    edges: vec![ProfileEdge {
                        name: "rim".into(),
                        segment: Segment::Circle {
                            center: [0.0, 0.0],
                            radius: 40.0,
                        },
                        reversed: false,
                    }],
                },
                holes: vec![],
            }],
        };
        let Recipe::Many { parts } = recipe(&p, &s, 0.0, 8.0, true, 1.0, Operation::New).unwrap()
        else {
            panic!("やまば")
        };
        assert_eq!(parts.len(), 3);
        let Recipe::Extrude {
            profile, from, to, ..
        } = &parts[1]
        else {
            panic!("中央を先につなぐ")
        };
        assert_eq!((*from, *to), (3.5, 4.5));
        assert_eq!(profile.regions[0].outer, p.regions[0].outer);
        let hole = &profile.regions[0].holes[0];
        assert!(hole.signed_area() < 0.0);
        let Segment::Circle { radius, .. } = hole.edges[0].segment else {
            panic!("歯底円")
        };
        assert!((radius - (s.pitch_radius + s.transverse_module() * 1.45)).abs() < 1e-12);
    }

    #[test]
    fn herringbone_halves_share_the_middle_phase_and_return_to_zero() {
        let s = spec(Hand::Right);
        let p = Profile {
            plane: Plane::XY,
            regions: vec![],
        };
        let Recipe::Many { parts } =
            recipe(&p, &s, -10.0, 10.0, true, 0.0, Operation::New).unwrap()
        else {
            panic!("左右の半分")
        };
        assert_eq!(parts.len(), 2);
        let Recipe::Sweep {
            twist: first,
            path: a,
            ..
        } = &parts[0]
        else {
            panic!("前半")
        };
        let Recipe::Sweep {
            twist: second,
            path: b,
            profile: p,
            ..
        } = parts.last().unwrap()
        else {
            panic!("後半")
        };
        assert!((first + second).abs() < 1e-12);
        assert_eq!(a.plane.origin, [0.0, 0.0, -10.0]);
        assert_eq!(b.plane.origin, [0.0, 0.0, 0.0]);
        assert_eq!(p.plane.origin, [0.0; 3]);
        assert!(recipe(p, &s, 0.0, 20.0, true, 20.0, Operation::New).is_err());
        assert!(recipe(p, &s, 0.0, 20.0, true, 0.0, Operation::Intersect).is_err());
    }
}
