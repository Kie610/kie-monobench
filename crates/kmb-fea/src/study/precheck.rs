//! 網を作らず、研究の条件の不足と直し方を並べる。

use super::{Kind, StudyData};
use kmb_core::EvalError;
use kmb_study::Check;

pub fn precheck(
    kind: Kind,
    d: &StudyData,
    read: &dyn Fn(&str, &str) -> Result<f64, EvalError>,
) -> Vec<Check> {
    let mut out = Vec::new();
    let mut add = |error, what: &str, fix: &str| {
        out.push(Check {
            error,
            what: what.into(),
            fix: fix.into(),
        })
    };
    let value = |s: &str, default: f64| {
        if s.trim().is_empty() {
            Some(default)
        } else {
            read(s, "点検の値").ok().filter(|v| v.is_finite())
        }
    };
    match kind {
        Kind::Static | Kind::Topology => {
            if d.fixed.is_empty() {
                add(
                    true,
                    "固定する面がありません",
                    "動かさない面を固定に足してください",
                );
            }
            if !d
                .loads
                .iter()
                .any(|r| !r.faces.is_empty() && value(&r.value, 0.0).is_some_and(|v| v != 0.0))
            {
                add(
                    true,
                    "荷重が無いか全部 0 です",
                    "荷重の面を選び、0 でない力か圧力を入れてください",
                );
            }
            if d.loads
                .iter()
                .any(|r| !r.faces.is_empty() && r.faces.iter().any(|f| d.fixed.contains(f)))
            {
                add(
                    false,
                    "荷重の面が固定の面と同じです",
                    "固定した面にかかる分の荷重は効きません。動く面に荷重を置いてください",
                );
            }
            if kind == Kind::Topology
                && value(&d.volume, 40.0).is_none_or(|v| !(0.0..=100.0).contains(&v))
            {
                add(
                    true,
                    "残す体積が範囲の外です",
                    "残す体積を 0〜100% にしてください",
                );
            }
        }
        Kind::Modal => {
            if d.fixed.is_empty() {
                add(
                    false,
                    "固定する面がありません",
                    "初めの 6 つは 0 Hz の剛体の動きです。必要なら固定する面を足してください",
                );
            }
        }
        Kind::Thermal => {
            let ambient = value(&d.ambient, 20.0);
            if d.temps.iter().all(|r| r.faces.is_empty())
                && !d
                    .convs
                    .iter()
                    .any(|r| !r.faces.is_empty() && value(&r.value, 0.0).is_some_and(|v| v > 0.0))
                && !value(&d.rest_h, 0.0).is_some_and(|v| v > 0.0)
            {
                add(
                    true,
                    "温度を決める面も熱を逃がす所もありません",
                    "温度を決める面か、空気へ逃げる面・残りの面の熱伝達係数を足してください",
                );
            }
            if (d.time != "transient" || value(&d.initial, ambient.unwrap_or(20.0)) == ambient)
                && !d
                    .heats
                    .iter()
                    .any(|r| !r.faces.is_empty() && value(&r.value, 0.0).is_some_and(|v| v != 0.0))
                && d.temps
                    .iter()
                    .all(|r| r.faces.is_empty() || value(&r.value, 20.0) == ambient)
            {
                add(
                    false,
                    "熱の入り口が無く、温度を決める面はまわりの温度と同じです",
                    "どこも同じ温度になります。温度差を見たい時は熱か違う温度を入れてください",
                );
            }
            if d.time == "transient" {
                let end = value(&d.end, 0.0);
                if end.is_none_or(|v| v <= 0.0) {
                    add(
                        true,
                        "終わりの時刻が正しくありません",
                        "0 より大きい秒数を入れてください",
                    );
                }
                if value(&d.frames, 50.0)
                    .is_none_or(|v| !(2.0..=200.0).contains(&v) || v.fract() != 0.0)
                {
                    add(
                        true,
                        "コマの数が正しくありません",
                        "2 から 200 の整数を入れてください",
                    );
                }
                for r in &d.heats {
                    let from = value(&r.from, 0.0);
                    let until = value(&r.until, end.unwrap_or(0.0));
                    if from
                        .zip(until)
                        .zip(end)
                        .is_none_or(|((a, b), e)| a < 0.0 || b <= a || b > e)
                    {
                        add(
                            true,
                            "熱の入れ始めと入れ終わりが正しくありません",
                            "0 ≦ 入れ始め < 入れ終わり ≦ 終わりの時刻にしてください",
                        );
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::study::{LoadRow, ValueRow};
    use kmb_core::{FeatureId, Ref};
    fn read(s: &str, _: &str) -> Result<f64, EvalError> {
        s.parse().map_err(|_| EvalError::InvalidData("数".into()))
    }
    fn face(s: &str) -> Ref {
        Ref::Face {
            feature: FeatureId(1),
            face: s.into(),
        }
    }
    #[test]
    fn precheck_reports_both_missing_conditions_and_clears_them_together() {
        let mut d = StudyData::default();
        let checks = precheck(Kind::Static, &d, &read);
        assert_eq!(checks.len(), 2);
        assert!(checks.iter().all(|c| c.error && !c.fix.is_empty()));
        d.fixed.push(face("left"));
        d.loads.push(LoadRow {
            faces: vec![face("right")],
            value: "100".into(),
            ..Default::default()
        });
        assert!(precheck(Kind::Static, &d, &read).is_empty());
        d.loads[0].faces = d.fixed.clone();
        assert!(!precheck(Kind::Static, &d, &read)[0].error);
    }
    #[test]
    fn precheck_warns_for_uniform_heat_and_free_modal_and_validates_time() {
        let mut d = StudyData {
            temps: vec![ValueRow {
                faces: vec![face("left")],
                value: "20".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let checks = precheck(Kind::Thermal, &d, &read);
        assert_eq!(checks.len(), 1);
        assert!(!checks[0].error);
        assert!(!precheck(Kind::Modal, &StudyData::default(), &read)[0].error);
        d.time = "transient".into();
        d.end = "0".into();
        d.frames = "1".into();
        assert_eq!(
            precheck(Kind::Thermal, &d, &read)
                .iter()
                .filter(|c| c.error)
                .count(),
            2
        );
        d.end = "10".into();
        d.frames = "20".into();
        d.initial = "100".into();
        assert!(
            precheck(Kind::Thermal, &d, &read).is_empty(),
            "初めに持つ熱を、熱が無いと知らせない"
        );
        d.heats.push(ValueRow {
            faces: vec![face("right")],
            value: "2".into(),
            from: "5".into(),
            until: "3".into(),
        });
        assert_eq!(
            precheck(Kind::Thermal, &d, &read)
                .iter()
                .filter(|c| c.error)
                .count(),
            1
        );
    }
}
