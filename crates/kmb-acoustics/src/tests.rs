//! 確かめること: 閉じた式、低周波の傾き、局所の山と谷、刻みの収束、入力の矛盾。
use super::*;
use std::f64::consts::{PI, TAU};

fn driver() -> Driver {
    Driver {
        re: 6.0,
        fs: 30.0,
        qms: Some(5.0),
        qes: Some(0.4),
        vas: 0.04,
        sd: 0.013,
        xmax: Some(0.004),
        pe: Some(50.0),
        ..Default::default()
    }
}
fn close(a: f64, b: f64, relative: f64) {
    assert!(
        (a - b).abs() <= relative * b.abs(),
        "{a} != {b}, 許容 {relative}"
    );
}
fn sealed() -> Enclosure {
    Enclosure::Sealed(Sealed::default())
}
fn vented() -> Enclosure {
    Enclosure::Vented(Vented::default())
}
fn result(enclosure: &Enclosure, resolution: u32) -> Analysis {
    analyze(
        &driver(),
        enclosure,
        &Excitation::default(),
        &FrequencyGrid {
            points_per_octave: resolution,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn sealed_resonance_q_and_driver_values_match_closed_forms() {
    let d = driver();
    let v = d.derive().unwrap();
    let a = result(&sealed(), 48);
    close(
        v.cms,
        d.vas / (AIR_DENSITY * SOUND_SPEED.powi(2) * d.sd.powi(2)),
        1e-14,
    );
    close(v.mms, 1.0 / ((TAU * d.fs).powi(2) * v.cms), 1e-14);
    close(a.summary.fc_hz.unwrap(), d.fs * 3.0_f64.sqrt(), 1e-14);
    close(
        a.summary.qtc.unwrap(),
        (1.0 / (1.0 / 5.0 + 1.0 / 0.4)) * 3.0_f64.sqrt(),
        1e-14,
    );
    assert!(a.summary.fb_hz.is_none() && a.response.port_velocity.is_none());
    let absorbed = Enclosure::Sealed(Sealed {
        absorption: 0.2,
        ..Default::default()
    });
    let changed = result(&absorbed, 48);
    close(
        changed.summary.fc_hz.unwrap(),
        d.fs * (1.0 + d.vas / 0.024).sqrt(),
        1e-14,
    );
}

#[test]
fn port_length_and_tuning_match_the_helmholtz_equation() {
    let area = PI * 0.025_f64.powi(2);
    let length = port_length(0.02, area, 1, 40.0, PortEnds::BaffleAndOpen).unwrap();
    assert!((length - 0.146).abs() < 0.001, "{length}");
    let a = result(&vented(), 48);
    close(a.summary.fb_hz.unwrap(), 40.0, 1e-14);
    close(a.summary.port_length_m.unwrap(), length, 1e-14);
    for ends in [
        PortEnds::BaffleAndOpen,
        PortEnds::BothFlanged,
        PortEnds::BothOpen,
    ] {
        let length = port_length(0.02, area, 2, 40.0, ends).unwrap();
        let b = Vented {
            port: Port {
                count: 2,
                area_m2: area,
                length_m: length,
                ends,
            },
            ..Default::default()
        };
        let a = result(&Enclosure::Vented(b.clone()), 48);
        close(a.summary.fb_hz.unwrap(), 40.0, 1e-14);
        close(
            a.summary.port_resonance_hz.unwrap(),
            SOUND_SPEED / (2.0 * b.port.effective_length().unwrap()),
            1e-14,
        );
    }
    assert!(port_length(0.02, PI * 0.0025_f64.powi(2), 1, 40.0, PortEnds::default()).is_err());
}

#[test]
fn the_high_frequency_one_watt_sensitivity_matches_efficiency() {
    let d = driver();
    let k = d.derive().unwrap();
    let eta =
        AIR_DENSITY * k.bl.powi(2) * d.sd.powi(2) / (TAU * SOUND_SPEED * d.re * k.mms.powi(2));
    let expected =
        10.0 * (AIR_DENSITY * SOUND_SPEED * eta / (TAU * REFERENCE_PRESSURE.powi(2))).log10();
    // 感度の閉じた式は 1 W。入力電圧は直流抵抗への 1 W に合わせる。
    let e = Excitation {
        voltage_rms: d.re.sqrt(),
        ..Default::default()
    };
    for b in [sealed(), vented()] {
        let s = sample(&d, &b, &e, 20_000.0).unwrap();
        assert!(
            (s.spl_db - expected).abs() < 0.1,
            "{} != {expected}",
            s.spl_db
        );
    }
}

#[test]
fn the_low_frequency_slopes_are_twelve_and_twenty_four_db_per_octave() {
    let mut d = driver();
    d.fs = 15.0;
    let b = sealed();
    let fc = d.fs * 3.0_f64.sqrt();
    let slope = sample(&d, &b, &Excitation::default(), fc / 4.0)
        .unwrap()
        .spl_db
        - sample(&d, &b, &Excitation::default(), fc / 8.0)
            .unwrap()
            .spl_db;
    assert!((slope - 12.0).abs() < 1.0, "密閉 {slope}");
    let mut v = Vented::default();
    v.port.length_m = port_length(v.volume_m3, v.port.area_m2, 1, 20.0, v.port.ends).unwrap();
    let b = Enclosure::Vented(v);
    let slope = sample(&d, &b, &Excitation::default(), 5.0).unwrap().spl_db
        - sample(&d, &b, &Excitation::default(), 2.5).unwrap().spl_db;
    assert!((slope - 24.0).abs() < 1.0, "バスレフ {slope}");
    assert!(sample(&d, &b, &Excitation::default(), 1.0).is_ok());
}

#[test]
fn tuning_makes_a_cone_valley_and_a_port_velocity_peak() {
    let a = result(&vented(), 96);
    let d = driver();
    let e = Excitation::default();
    let at = sample(&d, &vented(), &e, 40.0).unwrap();
    let lower = sample(&d, &vented(), &e, 28.0).unwrap();
    let upper = sample(&d, &vented(), &e, 52.0).unwrap();
    assert!(at.excursion_mm < lower.excursion_mm && at.excursion_mm < upper.excursion_mm);
    let peak = a.summary.maximum_port_velocity.unwrap();
    assert!(
        at.port_velocity.unwrap() > 0.9 * peak.value,
        "fb {:?}, 山 {peak:?}",
        at.port_velocity
    );
    assert!((peak.frequency_hz / 40.0 - 1.0).abs() < 0.2, "{peak:?}");
}

#[test]
fn impedance_has_one_sealed_peak_and_two_vented_peaks() {
    let peaks = |a: &Analysis| {
        a.response
            .impedance_ohm
            .windows(3)
            .enumerate()
            .filter(|(_, v)| v[1] > v[0] && v[1] > v[2])
            .map(|(i, _)| a.response.freq[i + 1])
            .collect::<Vec<_>>()
    };
    let a = result(&sealed(), 96);
    let p = peaks(&a);
    assert_eq!(p.len(), 1);
    close(p[0], a.summary.fc_hz.unwrap(), 0.02);
    let p = peaks(&result(&vented(), 96));
    assert_eq!(p.len(), 2, "{p:?}");
    assert!(p[0] < 40.0 && p[1] > 40.0);
}

#[test]
fn doubling_the_grid_converges_f3_and_the_maximum_group_delay() {
    for b in [sealed(), vented()] {
        let a = result(&b, 48);
        let fine = result(&b, 96);
        close(a.summary.f3_hz.unwrap(), fine.summary.f3_hz.unwrap(), 0.005);
        let max = |a: &Analysis| {
            a.response
                .group_delay_ms
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
        };
        close(max(&a), max(&fine), 0.005);
        assert!(a.response.group_delay_ms.iter().all(|x| x.is_finite()));
    }
}

#[test]
fn missing_or_contradictory_driver_values_are_input_problems() {
    let mut d = driver();
    d.qes = None;
    assert!(d.derive().is_err());
    let qts = driver().derive().unwrap().qts;
    d.qts = Some(qts);
    close(d.derive().unwrap().qes, 0.4, 1e-14);
    d = driver();
    d.qms = None;
    d.qts = Some(qts);
    close(d.derive().unwrap().qms, 5.0, 1e-14);
    d.qms = None;
    d.qes = None;
    assert!(d.derive().is_err());
    d = driver();
    d.qts = Some(qts * 1.03);
    let e = d.derive().unwrap_err();
    assert_eq!(e.kind, kmb_core::ProblemKind::Input);
    assert!(e.message.contains("入力値") && e.message.contains("計算値"));
    for is_bl in [false, true] {
        let mut d = driver();
        let k = d.derive().unwrap();
        if is_bl {
            d.bl = Some(k.bl * 1.06);
        } else {
            d.mms = Some(k.mms * 1.06);
        }
        assert!(d.derive().is_err());
    }
    let mut d = driver();
    let k = d.derive().unwrap();
    d.bl = Some(k.bl * 1.01);
    d.mms = Some(k.mms * 0.99);
    assert_eq!(d.derive().unwrap(), k);
}

#[test]
fn invalid_numbers_and_box_values_do_not_enter_the_circuit() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut d = driver();
        d.fs = bad;
        assert!(d.derive().is_err());
    }
    let mut d = driver();
    d.le = -0.01;
    assert!(d.derive().is_err());
    let mut b = Vented::default();
    b.port.count = 0;
    assert!(result_error(&Enclosure::Vented(b)));
    let b = Sealed {
        absorption: -0.1,
        ..Default::default()
    };
    assert!(result_error(&Enclosure::Sealed(b)));
    let b = Vented {
        ql: 0.0,
        ..Default::default()
    };
    assert!(result_error(&Enclosure::Vented(b)));
    assert!(
        analyze(
            &driver(),
            &sealed(),
            &Excitation::default(),
            &FrequencyGrid {
                low_hz: 1.0,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(sample(&driver(), &sealed(), &Excitation::default(), 0.0).is_err());
}
fn result_error(b: &Enclosure) -> bool {
    analyze(
        &driver(),
        b,
        &Excitation::default(),
        &FrequencyGrid::default(),
    )
    .is_err()
}

#[test]
fn warnings_ranges_and_input_power_are_preserved() {
    let e = Excitation {
        voltage_rms: 100.0,
        ..Default::default()
    };
    let a = analyze(&driver(), &vented(), &e, &FrequencyGrid::default()).unwrap();
    assert!(!a.summary.port_above_17.as_ref().unwrap().is_empty());
    assert!(!a.summary.port_above_10.as_ref().unwrap().is_empty());
    assert!(!a.summary.xmax_exceeded.as_ref().unwrap().is_empty());
    assert_eq!(a.summary.allowed_input_w, Some(50.0));
    assert!(a.notes.iter().any(|s| s.contains("17 m/s")));
    assert!(a.notes.iter().any(|s| s.contains("20 °C")));
    for r in a.summary.xmax_exceeded.unwrap() {
        assert!(r.low_hz >= 10.0 && r.low_hz < r.high_hz && r.high_hz <= 2000.0);
    }
}

#[test]
fn distance_and_radiation_change_level_but_add_no_propagation_delay() {
    let d = driver();
    let b = vented();
    let a = result(&b, 48);
    let e = Excitation {
        distance_m: 2.0,
        radiation: Radiation::FullSpace,
        ..Default::default()
    };
    let far = analyze(&d, &b, &e, &FrequencyGrid::default()).unwrap();
    for (a, b) in a.response.spl_db.iter().zip(&far.response.spl_db) {
        assert!((a - b - 20.0 * 4.0_f64.log10()).abs() < 1e-10);
    }
    for (a, b) in a
        .response
        .group_delay_ms
        .iter()
        .zip(&far.response.group_delay_ms)
    {
        assert!((a - b).abs() < 1e-10);
    }
}

#[test]
fn equal_inputs_are_deterministic_and_json_round_trips() {
    for b in [sealed(), vented()] {
        let a = result(&b, 48);
        assert_eq!(a, result(&b, 48));
        let json = serde_json::to_string(&a).unwrap();
        let back: Analysis = serde_json::from_str(&json).unwrap();
        assert_json_close(
            &serde_json::to_value(&a).unwrap(),
            &serde_json::to_value(&back).unwrap(),
        );
        let n = a.response.freq.len();
        assert!((360..=380).contains(&n));
        assert!(a.response.freq.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(a.response.spl_db.len(), n);
        assert_eq!(a.response.impedance_ohm.len(), n);
        assert_eq!(a.response.group_delay_ms.len(), n);
    }
    let json = serde_json::to_string(&driver()).unwrap();
    assert_eq!(driver(), serde_json::from_str::<Driver>(&json).unwrap());
    let absent: Driver = serde_json::from_str("{}").unwrap();
    assert_eq!(
        absent.derive().unwrap_err().kind,
        kmb_core::ProblemKind::Input
    );
}

// JSON の小数を読む時の最下位の丸めだけを許す。計算そのものの再現は別に完全一致を求める。
fn assert_json_close(a: &serde_json::Value, b: &serde_json::Value) {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            assert!(
                (a - b).abs() <= 8.0 * f64::EPSILON * a.abs().max(b.abs()),
                "JSON の数 {a} != {b}"
            );
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                assert_json_close(a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (key, a) in a {
                assert_json_close(a, &b[key]);
            }
        }
        _ => assert_eq!(a, b),
    }
}

#[test]
fn inductance_raises_high_frequency_impedance_and_reduces_output() {
    let mut d = driver();
    let b = sealed();
    let e = Excitation::default();
    let without = sample(&d, &b, &e, 20_000.0).unwrap();
    d.le = 0.001;
    let with = sample(&d, &b, &e, 20_000.0).unwrap();
    assert!(with.impedance_ohm > 10.0 * without.impedance_ohm);
    assert!(with.impedance_phase_deg > 80.0);
    assert!(with.spl_db < without.spl_db - 20.0);
}
