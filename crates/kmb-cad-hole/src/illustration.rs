//! 穴の欄の説明図。寸法の既定とねじの表は、形を作る断面の計算と共有する。
use super::*;
use kmb_core::{ChoiceIllustration, ChoiceLines, Context, Dim};
use kmb_interchange::ink::line::{FAINT, MAIN};

pub(super) const DRAW: ChoiceIllustration = ChoiceIllustration(draw);

fn draw(cx: &Context<'_>, data: &serde_json::Value, key: &str, value: &str) -> ChoiceLines {
    diagram(cx, data, key, value).unwrap_or_default()
}

fn diagram(
    cx: &Context<'_>,
    data: &serde_json::Value,
    key: &str,
    value: &str,
) -> Option<ChoiceLines> {
    let mut data = data.clone();
    data.as_object_mut()?.insert(key.into(), value.into());
    let hole: Hole = serde_json::from_value(data).ok()?;
    let params = cx.report.param_values();
    let read = |src: &str, angle: bool| -> Result<f64, EvalError> {
        let q = kmb_core::eval_str(src, &|name| params.get(name).copied())
            .map_err(|e| EvalError::InvalidData(e.to_string()))?;
        let dim = if angle { Dim::ANGLE } else { Dim::LENGTH };
        if !q.dim.is_none() && q.dim != dim {
            return Err(EvalError::InvalidData(
                "説明図の寸法の単位が違います".into(),
            ));
        }
        let v = if angle && q.dim.is_none() {
            q.value.to_radians()
        } else {
            q.value
        };
        if v <= 0.0 || kmb_core::Quantity::new(v, dim).size_problem().is_some() {
            return Err(EvalError::InvalidData(
                "説明図の寸法は正の有限の値が必要です".into(),
            ));
        }
        Ok(v)
    };
    let length = |src: &str, _: &str| read(src, false);
    let angle = |src: &str, _: &str| read(src, true);
    let dia = hole.diameter_with(&length).ok()?;
    // 貫通・対象までは確定した底の深さを持たないので、径の 2 倍を説明図の奥行きにする。
    let depth = if hole.extent == HoleExtent::ToFace {
        dia * 2.0
    } else {
        hole.depth
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|src| read(src, false))
            .transpose()
            .ok()?
            .unwrap_or(dia * 2.0)
    };
    let mut section = hole.section_with(depth, &length, &angle).ok()?;
    if hole.extent == HoleExtent::ToFace {
        section.tip = None;
    }
    pieces(&section).ok()?;
    let r = dia / 2.0;
    let (mouth, shoulder) = match section.kind {
        HoleKind::Simple => (r, 0.0),
        HoleKind::Counterbore => (section.counterbore.0 / 2.0, section.counterbore.1),
        HoleKind::Countersink => {
            let big = section.countersink.0 / 2.0;
            (big, (big - r) / (section.countersink.1 / 2.0).tan())
        }
    };
    let tip = section.tip.map_or(0.0, |a| r / (a / 2.0).tan());
    let half = mouth.max(r) + dia * 0.45;
    let bottom = if hole.blind() {
        depth + tip + dia * 0.35
    } else {
        depth
    };
    let mut out = if hole.blind() {
        vec![(
            vec![[-half, 0.0], [-half, bottom], [half, bottom], [half, 0.0]],
            MAIN,
        )]
    } else {
        [-1.0, 1.0]
            .into_iter()
            .map(|sign| {
                (
                    vec![
                        [sign * half, 0.0],
                        [sign * half, bottom],
                        [sign * r, bottom],
                    ],
                    MAIN,
                )
            })
            .collect()
    };
    for sign in [-1.0, 1.0] {
        let mut wall = vec![[sign * half, 0.0], [sign * mouth, 0.0]];
        match section.kind {
            HoleKind::Simple => {}
            HoleKind::Counterbore => {
                wall.push([sign * mouth, shoulder]);
                wall.push([sign * r, shoulder]);
            }
            HoleKind::Countersink => wall.push([sign * r, shoulder]),
        }
        wall.push([sign * r, depth]);
        if hole.extent == HoleExtent::ToFace {
            wall.last_mut()?[1] += sign * r * 0.35;
        }
        out.push((wall, MAIN));
    }
    if hole.extent == HoleExtent::ToFace {
        out.push((vec![[-r, depth - r * 0.35], [r, depth + r * 0.35]], MAIN));
        out.push((
            vec![[-half, depth - half * 0.35], [half, depth + half * 0.35]],
            FAINT,
        ));
    } else if hole.blind() {
        out.push((vec![[-r, depth], [0.0, depth + tip], [r, depth]], MAIN));
    } else {
        // 開いた端と短い破断線で貫通を示す。
        for sign in [-1.0, 1.0] {
            out.push((
                vec![
                    [sign * r - dia * 0.12, depth - dia * 0.08],
                    [sign * r + dia * 0.12, depth + dia * 0.08],
                ],
                FAINT,
            ));
        }
    }
    if hole.tap == Tap::Tapped {
        let major = hole.thread().ok()??.0.major / 2.0;
        for sign in [-1.0, 1.0] {
            out.push((vec![[sign * major, shoulder], [sign * major, depth]], FAINT));
        }
    }
    out.push((vec![[0.0, -dia * 0.1], [0.0, bottom]], FAINT));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_hole_choice_has_a_finite_section_illustration() {
        let data = kmb_core::DocumentData::default();
        let report = kmb_core::RecomputeReport::default();
        let reg = Registry::new();
        let kernel: std::sync::Arc<dyn kmb_core::Kernel> =
            std::sync::Arc::new(kmb_core::NullKernel);
        let cx = Context::new(&data, &report, &reg, &kernel);
        let base = json!({"target":1,"sketch":2,"diameter":"5 mm","depth":"20 mm"});
        for field in hole_fields()
            .into_iter()
            .filter(|f| ["kind", "extent", "tap", "tip"].contains(&f.key.as_str()))
        {
            let kmb_core::FieldKind::Choice { choices } = &field.kind else {
                panic!("選ぶ欄")
            };
            for choice in choices {
                let lines = (field.illustration.expect("説明図").0)(
                    &cx,
                    &base,
                    &field.key,
                    &choice.value,
                );
                assert!(!lines.is_empty(), "{} {}", field.key, choice.value);
                assert!(
                    lines
                        .iter()
                        .all(|(p, _)| p.len() >= 2 && p.iter().flatten().all(|v| v.is_finite()))
                );
            }
        }
    }
    #[test]
    fn illustrations_use_current_dimensions_parameters_and_tip_angle() {
        let data = kmb_core::DocumentData::default();
        let mut report = kmb_core::RecomputeReport::default();
        report.params.push(kmb_core::ParamResult {
            id: kmb_core::ParamId(1),
            name: "dia".into(),
            status: kmb_core::ParamStatus::Ok {
                value: kmb_core::Quantity::new(8.0, Dim::LENGTH),
            },
        });
        let reg = Registry::new();
        let kernel: std::sync::Arc<dyn kmb_core::Kernel> =
            std::sync::Arc::new(kmb_core::NullKernel);
        let cx = Context::new(&data, &report, &reg, &kernel);
        let base =
            json!({"target":1,"sketch":2,"diameter":"dia","depth":"12 mm","tip_angle":"90 deg"});
        let before = base.clone();
        let lines = draw(&cx, &base, "tip", "angled");
        assert!(
            lines
                .iter()
                .any(|(p, _)| p == &vec![[-4.0, 12.0], [0.0, 16.0], [4.0, 12.0]])
        );
        assert_eq!(base, before);
        let flat = draw(&cx, &base, "tip", "flat");
        assert_ne!(lines, flat);
        assert!(
            draw(
                &cx,
                &json!({"target":1,"sketch":2,"diameter":"unknown"}),
                "kind",
                "simple"
            )
            .is_empty()
        );
        let field = hole_fields().remove(0);
        assert!(field.illustration.is_some());
        assert!(
            serde_json::to_value(field)
                .unwrap()
                .get("illustration")
                .is_none()
        );
    }
    #[test]
    fn a_through_hole_illustration_leaves_the_bottom_open_and_refuses_oversized_values() {
        let data = kmb_core::DocumentData::default();
        let report = kmb_core::RecomputeReport::default();
        let reg = Registry::new();
        let kernel: std::sync::Arc<dyn kmb_core::Kernel> =
            std::sync::Arc::new(kmb_core::NullKernel);
        let cx = Context::new(&data, &report, &reg, &kernel);
        let base = json!({"target":1,"sketch":2,"diameter":"5 mm"});
        let lines = draw(&cx, &base, "kind", "counterbore");
        assert!(!lines.is_empty());
        assert!(
            !lines
                .iter()
                .filter(|(_, ink)| *ink == MAIN)
                .flat_map(|(p, _)| p.windows(2))
                .any(|p| p[0][0] * p[1][0] < 0.0 && p[0][1] == p[1][1]),
            "貫通穴の中央を横切る底の線は描かない"
        );
        assert!(
            draw(
                &cx,
                &json!({"target":1,"sketch":2,"diameter":"1e300 mm"}),
                "kind",
                "simple"
            )
            .is_empty()
        );
    }
}
