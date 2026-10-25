use super::*;

fn near(a: P3, b: P3) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
}

#[test]
fn turning_about_an_axis_is_right_handed() {
    let v = rotate_vec([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2);
    assert!(near(v, [0.0, 1.0, 0.0]), "{v:?}");
    let p = rotate([2.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], std::f64::consts::PI);
    assert!(near(p, [0.0, 0.0, 0.0]), "{p:?}");
}

#[test]
fn unknown_keys_and_unused_fields_are_read_as_written() {
    let f: DatumPlaneFeature = serde_json::from_value(json!({
        "method": "angle", "base": {"kind": "xy"}, "axis": {"kind": "x"}, "angle": "15 deg", "distance": "3 mm"
    }))
    .unwrap();
    assert_eq!(f.method, Method::Angle);
    // 作り方を切り替えても前の距離は残る(読まないだけ)
    assert_eq!(f.distance, "3 mm");
    assert!(serde_json::from_value::<DatumPlaneFeature>(json!({"method": "offset", "dist": "1"})).is_err());
}
