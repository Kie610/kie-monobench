//! 3 段の網の代表値から、落ち着き方と誤差の見積もりを求める。

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Measure {
    pub name: String,
    pub unit: &'static str,
    pub value: f64,
}

impl Measure {
    pub fn new(name: impl Into<String>, unit: &'static str, value: f64) -> Self {
        Self {
            name: name.into(),
            unit,
            value,
        }
    }
}

pub(super) struct Level {
    pub result: SimResult,
    pub values: Vec<Measure>,
    pub stats: crate::tetmesh::MeshStats,
    pub nodes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Estimate {
    pub class: &'static str,
    pub order: Option<f64>,
    pub gci: Option<f64>,
    pub extrapolated: Option<f64>,
    pub note: &'static str,
}

/// 細・中・粗の順。要求した比ではなく、出来上がった網の体積と節点数から求めた大きさを使う。
pub(super) fn estimate(h: [f64; 3], value: [f64; 3]) -> Estimate {
    let plain = |class| Estimate {
        class,
        order: None,
        gci: None,
        extrapolated: None,
        note: "",
    };
    if h.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || value.iter().any(|v| !v.is_finite())
        || h[1] <= h[0]
        || h[2] <= h[1]
    {
        return plain("網の大きさの順が揃わないため見積もれません");
    }
    let [a, b, c] = value;
    let (e21, e32) = (b - a, c - b);
    let tol = 1e-9
        * value
            .iter()
            .map(|v| v.abs())
            .fold(f64::MIN_POSITIVE, f64::max);
    if e21.abs() < tol && e32.abs() < tol {
        return plain("3 段で同じ値(落ち着いている)");
    }
    if e21 * e32 < 0.0 {
        return plain("上下に揺れて落ち着かない");
    }
    if e32 == 0.0 || e21 == 0.0 {
        return plain("次数を見積もれません");
    }
    let ratio = e21 / e32;
    if ratio >= 1.0 {
        return plain("細かくするほど離れる(特異点か網が粗すぎる)");
    }
    let (r21, r32) = (h[1] / h[0], h[2] / h[1]);
    let ln = (e32 / e21).abs().ln();
    let sign = (e32 / e21).signum();
    let mut p = ln.abs() / r21.ln();
    for _ in 0..20 {
        let q = ((r21.powf(p) - sign) / (r32.powf(p) - sign)).ln();
        let next = (ln + q).abs() / r21.ln();
        if !next.is_finite() || next <= 0.0 {
            return plain("次数を見積もれません");
        }
        let delta = (next - p).abs();
        p = next;
        if delta < 1e-6 {
            break;
        }
    }
    let den = r21.powf(p) - 1.0;
    if !den.is_finite() || den <= 0.0 {
        return plain("次数を見積もれません");
    }
    let gci = (a != 0.0)
        .then(|| 1.25 * ((a - b) / a).abs() / den)
        .filter(|v| v.is_finite());
    let extrapolated = ((r21.powf(p) * a - b) / den)
        .is_finite()
        .then(|| (r21.powf(p) * a - b) / den);
    Estimate {
        class: "落ち着いていく",
        order: Some(p),
        gci,
        extrapolated,
        note: if !(0.5..=4.0).contains(&p) {
            "次数が範囲の外なので、見積もりは目安になりません"
        } else {
            ""
        },
    }
}

impl Job {
    pub(super) fn converge(&self, ratio: f64, c: &Control) -> Result<SimResult, String> {
        self.supports(&kmb_study::Mode::Converge { ratio })?;
        let fine = if self.auto {
            self.auto_size(c)?
        } else {
            self.h
        };
        let mut levels = Vec::new();
        for (i, h) in [fine * ratio * ratio, fine * ratio, fine]
            .into_iter()
            .enumerate()
        {
            if c.cancelled() {
                return Err(CANCELLED.into());
            }
            let level = self.level(c, Some(h), Some(i))?;
            if c.cancelled() || level.result.partial {
                return Err(CANCELLED.into());
            }
            levels.push(level);
        }
        c.step(3);
        c.progress(0.98, "比べる");
        let hs = [2, 1, 0].map(|i| (levels[i].stats.volume.abs() / levels[i].nodes as f64).cbrt());
        let coarse = &levels[0].stats;
        let shape_ok = (self.volume <= 0.0
            || ((coarse.volume - self.volume) / self.volume).abs() <= 0.03)
            && coarse.miss_max <= 0.5 * coarse.h;
        let mut result = levels[2].result.clone();
        if levels[2].values.is_empty() {
            return Err(
                "剛体の動きを除いて比べる振動数がありません。求める数を増やしてください".into(),
            );
        }
        if self.auto && fine > self.h * 1.001 {
            result.note.push_str(&format!(
                "\n自動の要素の大きさを {} mm から {} mm にして、要素の上限に収めました",
                fmt(self.h),
                fmt(fine)
            ));
        }
        for j in 0..levels[2].values.len() {
            let m = &levels[2].values[j];
            let mut values = [0.0; 3];
            for (i, level) in [2, 1, 0].into_iter().enumerate() {
                let found = levels[level]
                    .values
                    .iter()
                    .find(|v| v.name == m.name)
                    .ok_or("網の段ごとに比べる値が違います")?;
                values[i] = found.value;
            }
            let e = estimate(hs, values);
            let mut note = if shape_ok {
                e.class.to_string()
            } else {
                "粗い網が形を表せていません。要素の大きさを小さくしてから確かめてください".into()
            };
            let mut detail = format!(
                "{} {}(粗 {}・中 {}・細 {})",
                fmt(m.value),
                m.unit,
                fmt(values[2]),
                fmt(values[1]),
                fmt(values[0])
            );
            if shape_ok {
                if let Some(p) = e.order {
                    detail.push_str(&format!("・次数 {}", fmt(p)));
                }
                if let Some(gci) = e.gci {
                    detail.push_str(&format!("・誤差の見積もり {}%", fmt(100.0 * gci)));
                }
                if let Some(v) = e.extrapolated {
                    detail.push_str(&format!("・外挿 {} {}", fmt(v), m.unit));
                }
                if !e.note.is_empty() {
                    note.push_str(&format!("。{}", e.note));
                }
            }
            if m.name == "全体の最大のミーゼス応力" && !result.marks.is_empty() {
                note.push_str("。特異点の候補のそばでは、網を細かくしても応力が上がり続けます。印から離れた値も確かめてください");
            }
            result.numbers.push((format!("収束: {}", m.name), detail));
            result.note.push_str(&format!("\n収束: {}: {note}", m.name));
            result.series.push(kmb_core::Series {
                name: format!("{}({})", m.name, m.unit),
                x_name: "要素の大きさ mm".into(),
                points: hs.into_iter().zip(values).map(|(h, v)| [h, v]).collect(),
                note,
            });
        }
        if c.cancelled() {
            return Err(CANCELLED.into());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_cantilever_reports_three_meshes_and_the_singularity_warning() {
        let e = 70_000.0;
        let nu = 0.3;
        let job = Job {
            kind: Kind::Static,
            mat: *fea_material("アルミ").unwrap(),
            h: 2.5,
            auto: false,
            volume: 10000.0,
            mesh: crate::validation_tests::box_mesh([100.0, 10.0, 10.0]),
            solve: Solve::Static(StaticInput {
                e,
                nu,
                fixed: vec!["x0".into()],
                loads: vec![Load {
                    faces: vec!["x1".into()],
                    kind: LoadKind::Force(100.0),
                    dir: Dir::Fixed([0.0, 0.0, -1.0]),
                }],
            }),
        };
        let result = job.converge(1.5, &Control::new()).unwrap();
        assert_eq!(result.series.len(), 3);
        let s = result
            .series
            .iter()
            .find(|s| s.name.starts_with("最大の変位"))
            .unwrap();
        assert_eq!(s.points.len(), 3);
        let hs = std::array::from_fn(|i| s.points[i][0]);
        let vs = std::array::from_fn(|i| s.points[i][1]);
        let est = estimate(hs, vs);
        let inertia = 10.0f64.powi(4) / 12.0;
        let g = e / (2.0 * (1.0 + nu));
        let theory = 100.0 * 100.0f64.powi(3) / (3.0 * e * inertia)
            + 100.0 * 100.0 / (5.0 / 6.0 * g * 100.0);
        eprintln!("片持ち梁: h={hs:?}, 変位={vs:?}, {est:?}, 理論値={theory}");
        assert!((1.5..=4.0).contains(&est.order.expect("落ち着いていく")));
        assert!((est.extrapolated.unwrap() - theory).abs() < (vs[0] - theory).abs());
        let global = result
            .series
            .iter()
            .find(|s| s.name.starts_with("全体の最大"))
            .unwrap();
        assert!(global.note.contains("特異点"));
        let away = result
            .series
            .iter()
            .find(|s| s.name.starts_with("特異点を除いた"))
            .unwrap();
        assert!(!away.note.is_empty());
    }
    #[test]
    fn convergence_keeps_fine_thermal_frames_and_reports_heat_and_modes() {
        let base = |kind, solve| Job {
            kind,
            mat: *fea_material("").unwrap(),
            h: 1.0,
            auto: false,
            volume: 8.0,
            mesh: crate::validation_tests::box_mesh([2.0; 3]),
            solve,
        };
        let mut input = crate::validation_tests::transient_case();
        input.frames = 2;
        let steady = base(Kind::Thermal, Solve::Thermal(input.thermal.clone()));
        let a = steady.converge(1.5, &Control::new()).unwrap();
        let b = steady.converge(1.5, &Control::new()).unwrap();
        assert_eq!(a.series, b.series);
        assert_eq!(a.numbers, b.numbers);
        assert_eq!(a.series.len(), 2);
        assert!(
            a.series
                .iter()
                .any(|s| s.name.starts_with("空気へ逃げた熱"))
        );
        let transient = base(Kind::Thermal, Solve::Transient(input.clone()));
        let result = transient.converge(1.5, &Control::new()).unwrap();
        assert_eq!(result.frames.last().unwrap().time, Some(input.end));
        assert_eq!(result.series.len(), 4, "時刻の推移2本と、3段の網の比較2本");
        assert!(!result.partial);
        let modal = base(
            Kind::Modal,
            Solve::Modal(ModalInput {
                e: 70_000.0,
                nu: 0.3,
                density: 2.7,
                fixed: vec!["x0".into()],
                count: 3,
            }),
        );
        let result = modal.converge(1.5, &Control::new()).unwrap();
        assert_eq!(result.series.len(), 3);
        assert!(
            result
                .series
                .iter()
                .all(|s| s.points.len() == 3 && s.points.iter().all(|p| p[1] > 0.0))
        );
    }
    #[test]
    fn convergence_uses_actual_unequal_mesh_ratios() {
        for h in [[1.0, 1.5, 2.25], [1.0, 1.4, 2.3]] {
            let v = h.map(|h| 10.0 + h * h);
            let e = estimate(h, v);
            assert!((e.order.unwrap() - 2.0).abs() < 1e-5, "{e:?}");
            assert!((e.extrapolated.unwrap() - 10.0).abs() < 1e-5);
            assert!((e.gci.unwrap() - 1.25 / 11.0).abs() < 1e-5);
        }
    }
    #[test]
    fn convergence_classifies_constant_oscillating_and_diverging_values() {
        let h = [1.0, 1.5, 2.25];
        assert!(estimate(h, [0.0; 3]).class.contains("同じ"));
        assert!(estimate(h, [1.0, 1.1, 1.0]).class.contains("揺れ"));
        assert!(estimate(h, [5.0, 3.0, 2.0]).class.contains("離れる"));
        assert!(estimate([1.0; 3], [1., 2., 3.]).order.is_none());
        assert!(estimate(h, [f64::NAN, 2., 3.]).order.is_none());
        let out = estimate(h, h.map(|h| 1.0 + h.powi(5)));
        assert!(out.order.unwrap() > 4.0 && !out.note.is_empty());
    }
}
