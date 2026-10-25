use super::*;

fn ev(src: &str) -> Result<Quantity, ExprError> {
    eval_str(src, &|n| match n {
        "width" => Some(Quantity::mm(40.0)),
        "angle" => Some(Quantity::deg(30.0)),
        "count" => Some(Quantity::scalar(3.0)),
        _ => None,
    })
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn units_convert_to_mm_and_rad() {
    assert!(close(ev("2 cm").unwrap().value, 20.0));
    assert!(close(ev("1in").unwrap().value, 25.4));
    // ft。名前には使えない
    assert!(close(ev("1.5 ft").unwrap().value, 457.2));
    assert!(close(ev("1 ft - 12 in").unwrap().value, 0.0));
    assert!(close(ev("2 [ft^2]").unwrap().value, 2.0 * 304.8 * 304.8));
    assert!(!crate::is_valid_name("ft"));
    assert_eq!(ev("10 mm + 1 cm").unwrap(), Quantity::mm(20.0));
    assert!(close(ev("180 deg").unwrap().value, std::f64::consts::PI));
}

#[test]
fn precedence_and_associativity() {
    assert_eq!(ev("1 + 2 * 3").unwrap(), Quantity::scalar(7.0));
    assert_eq!(ev("(1 + 2) * 3").unwrap(), Quantity::scalar(9.0));
    assert_eq!(ev("-2^2").unwrap(), Quantity::scalar(-4.0));
    assert_eq!(ev("2^3^2").unwrap(), Quantity::scalar(512.0));
    assert_eq!(ev("10 - 4 - 3").unwrap(), Quantity::scalar(3.0));
}

#[test]
fn references_and_dimensions() {
    assert_eq!(ev("width / 2").unwrap(), Quantity::mm(20.0));
    assert_eq!(ev("width * count").unwrap(), Quantity::mm(120.0));
    let area = ev("width * width").unwrap();
    assert_eq!(
        area.dim,
        Dim {
            length: 2,
            angle: 0
        }
    );
    assert_eq!(ev("sqrt(width * width)").unwrap(), Quantity::mm(40.0));
    assert!(close(ev("sin(angle)").unwrap().value, 0.5));
    assert_eq!(ev("width / (1 mm)").unwrap(), Quantity::scalar(40.0));
}

#[test]
fn errors() {
    // 長さと無次元の足し引きは、無次元を mm として読む。面積とは足せない
    assert_eq!(ev("width + 1").unwrap(), Quantity::mm(41.0));
    assert!(close(ev("angle + 15").unwrap().value.to_degrees(), 45.0));
    assert_eq!(ev("max(width; 50)").unwrap(), Quantity::mm(50.0));
    assert!(matches!(
        ev("width * width + 1"),
        Err(ExprError::DimensionMismatch { .. })
    ));
    assert!(matches!(
        ev("width + angle"),
        Err(ExprError::DimensionMismatch { .. })
    ));
    assert!(matches!(ev("height"), Err(ExprError::UnknownName(n)) if n == "height"));
    assert!(matches!(ev("foo(1)"), Err(ExprError::UnknownFunction(_))));
    assert!(matches!(ev("1 +"), Err(ExprError::Syntax { .. })));
    assert!(matches!(ev("(1"), Err(ExprError::Syntax { .. })));
    assert!(matches!(ev("mm"), Err(ExprError::Syntax { .. })));
    assert!(matches!(ev("1 / 0"), Err(ExprError::Domain(_))));
    assert!(matches!(ev("min()"), Err(ExprError::Arity { .. })));
    assert!(matches!(ev(""), Err(ExprError::Syntax { .. })));
    assert!(matches!(ev("1 $ 2"), Err(ExprError::Syntax { .. })));
}

#[test]
fn scientific_notation_does_not_eat_units() {
    assert_eq!(ev("1e3").unwrap(), Quantity::scalar(1000.0));
    assert_eq!(ev("2.5e-1 mm").unwrap(), Quantity::mm(0.25));
}

#[test]
fn references_are_listed_once_in_order() {
    let e = parse("width + max(width, count * 1 mm) + pi * width").unwrap();
    assert_eq!(
        e.references(),
        vec!["width".to_string(), "count".to_string()]
    );
}

#[test]
fn rename_keeps_formatting() {
    assert_eq!(
        rename_reference("width*2 + max(width,  1 mm)", "width", "w").unwrap(),
        "w*2 + max(w,  1 mm)"
    );
    assert_eq!(
        rename_reference("widths + width", "width", "w").unwrap(),
        "widths + w"
    );
    assert!(rename_reference("1 +", "a", "b").is_ok());
}

#[test]
fn names() {
    assert!(is_valid_name("width"));
    assert!(is_valid_name("幅_1"));
    assert!(!is_valid_name("mm"));
    assert!(!is_valid_name("sin"));
    assert!(!is_valid_name("1a"));
    assert!(!is_valid_name("a-b"));
}

#[test]
fn display() {
    assert_eq!(Quantity::mm(12.5).to_string(), "12.5 mm");
    assert_eq!(Quantity::deg(45.0).to_string(), "45 deg");
    assert_eq!(Quantity::scalar(3.0).to_string(), "3");
}

/// Kie のレビュー(2026-10-16): PI・rad などを式に使える
#[test]
fn constants_and_functions_in_expressions() {
    let q = ev("PI/4 rad").unwrap();
    assert_eq!(q.dim, Dim::ANGLE);
    assert!(close(q.value, std::f64::consts::FRAC_PI_4));
    assert!(close(ev("sin(PI/4 rad)").unwrap().value, 0.5f64.sqrt()));
    assert!(close(ev("π").unwrap().value, std::f64::consts::PI));
    assert!(close(ev("E").unwrap().value, std::f64::consts::E));
    assert!(close(ev("ln(E)").unwrap().value, 1.0));
    assert!(close(ev("log(1000)").unwrap().value, 3.0));
    assert!(close(ev("exp(0)").unwrap().value, 1.0));
    assert_eq!(ev("pow(width; 2)").unwrap().dim.length, 2);
    assert_eq!(ev("pow(2, 3)").unwrap(), Quantity::scalar(8.0));
    assert_eq!(ev("max(1 mm; 3 mm; 2 mm)").unwrap(), Quantity::mm(3.0));
    assert_eq!(ev("sign(-3 mm)").unwrap(), Quantity::scalar(-1.0));
    assert!(close(ev("tanh(0)").unwrap().value, 0.0));
    assert_eq!(ev("atanh(0)").unwrap().dim, Dim::ANGLE);
    assert!(matches!(ev("ln(0)"), Err(ExprError::Domain(_))));
    assert!(matches!(
        ev("exp(1 mm)"),
        Err(ExprError::DimensionMismatch { .. })
    ));
    assert!(matches!(
        ev("pow(1 mm; 1 mm)"),
        Err(ExprError::DimensionMismatch { .. })
    ));
    // 定数は名前に使えず、参照の一覧にも出ない
    assert!(!is_valid_name("PI"));
    assert!(!is_valid_name("E"));
    assert!(parse("PI * E * width").unwrap().references() == vec!["width".to_string()]);
}

/// 単位は直前の掛け算・割り算の並びに掛かる
#[test]
fn unit_applies_to_the_product_before_it() {
    assert!(close(ev("1/2 in").unwrap().value, 12.7));
    assert_eq!(ev("2 * 3 mm").unwrap(), Quantity::mm(6.0));
    assert_eq!(ev("(1 + 2) mm").unwrap(), Quantity::mm(3.0));
    assert_eq!(ev("10 mm / 2").unwrap(), Quantity::mm(5.0));
    assert_eq!(ev("-5 mm").unwrap(), Quantity::mm(-5.0));
    assert_eq!(
        ev("10 mm + 90 deg / 90 deg * 1 cm").unwrap(),
        Quantity::mm(20.0)
    );
    // 名前の後ろの数にだけ掛かる(前と同じ読み方)
    assert_eq!(ev("width / 2 mm").unwrap(), Quantity::scalar(20.0));
    assert_eq!(ev("width / (2 mm)").unwrap(), Quantity::scalar(20.0));
    assert_eq!(ev("(count + 1) mm").unwrap(), Quantity::mm(4.0));
    let a = ev("5 mm^2").unwrap();
    assert_eq!((a.value, a.dim.length), (5.0, 2));
    assert_eq!(ev("2 mm * 3 mm").unwrap().dim.length, 2);
    assert!(matches!(ev("2 * mm"), Err(ExprError::Syntax { .. })));
    assert!(matches!(ev("5 mm^x"), Err(ExprError::Syntax { .. })));
}

proptest::proptest! {
    /// どんな入力でも panic せず、結果か誤りを返す
    #[test]
    fn never_panics(s in "\\PC{0,40}") {
        let _ = ev(&s);
    }
}

#[test]
fn bracketed_units_bind_to_the_one_just_before() {
    assert_eq!(ev("5 [mm]").unwrap(), Quantity::mm(5.0));
    assert!(close(ev("(1/2) [in]").unwrap().value, 12.7));
    // 大かっこは直前の 1 つだけ。大かっこの無い単位は今までどおり並び全体に掛かる
    assert_eq!(
        ev("1/2 [in]").unwrap().dim,
        Dim {
            length: -1,
            angle: 0
        }
    );
    assert!(close(ev("1/2 in").unwrap().value, 12.7));
    assert_eq!(
        ev("PI/4 [rad]").unwrap().dim,
        Dim {
            length: 0,
            angle: -1
        }
    );
    assert!(close(ev("(PI/4) [rad]").unwrap().value.to_degrees(), 45.0));
    assert_eq!(ev("width + 1 [cm]").unwrap(), Quantity::mm(50.0));
    assert_eq!(
        ev("width * 2 [mm]").unwrap().dim,
        Dim {
            length: 2,
            angle: 0
        }
    );
    assert_eq!(ev("width / 2 [mm]").unwrap(), Quantity::scalar(20.0));
    // 大かっこの中の累乗と積・商。`^` より強い
    let a = ev("3 [mm^2]").unwrap();
    assert_eq!(
        (a.value, a.dim),
        (
            3.0,
            Dim {
                length: 2,
                angle: 0
            }
        )
    );
    let b = ev("2 [mm]^2").unwrap();
    assert_eq!(
        (b.value, b.dim),
        (
            4.0,
            Dim {
                length: 2,
                angle: 0
            }
        )
    );
    assert_eq!(ev("4 [mm*mm/cm]").unwrap(), Quantity::mm(0.4));
    assert!(close(ev("30 [deg]").unwrap().value.to_degrees(), 30.0));
    // 名前やかっこにも付く
    assert_eq!(ev("count [mm]").unwrap(), Quantity::mm(3.0));
    assert_eq!(ev("(1 + 2) [cm]").unwrap(), Quantity::mm(30.0));
}

#[test]
fn bracketed_unit_errors() {
    for bad in ["5 [mm", "5 [kg]", "5 []", "[mm]", "5 [mm^0]"] {
        assert!(matches!(ev(bad), Err(ExprError::Syntax { .. })), "{bad}");
    }
}

#[test]
fn units_combine_through_multiplication() {
    // 単位どうしの掛け算・割り算は次元で解く(mm*mm は mm²、mm*rad/rad は mm)
    assert_eq!(
        ev("2 mm * 3 mm").unwrap().dim,
        Dim {
            length: 2,
            angle: 0
        }
    );
    assert_eq!(ev("width * 1 rad / (1 rad)").unwrap(), Quantity::mm(40.0));
    assert_eq!(ev("sqrt(9 [mm^2])").unwrap(), Quantity::mm(3.0));
}

/// 小数は点で書く: 先頭のカンマ(`,5`)も `1,5` と同じく誤り。引数の区切りのカンマ(`max(1,5)`)は使える。
#[test]
fn a_leading_decimal_comma_is_an_error() {
    let e = ev(",5").unwrap_err().to_string();
    assert!(e.contains("点で"), "{e}");
    assert!(ev(",5 mm").is_err());
    assert!(ev("2 * (,5)").is_err());
    assert!(ev("1,5").is_err());
    assert_eq!(ev("max(1,5)").unwrap(), Quantity::scalar(5.0));
    assert_eq!(ev("max(width,5)").unwrap(), Quantity::mm(40.0));
    assert!(ev(",").is_err());
}

/// 全角の数字・小数点・符号も普通の数として受ける。全角の名前は名前のまま
#[test]
fn full_width_digits_and_signs_are_numbers() {
    assert_eq!(ev("１２．５ mm").unwrap(), Quantity::mm(12.5));
    assert_eq!(ev("－３ ＋ １").unwrap(), Quantity::scalar(-2.0));
    assert_eq!(ev("−0.5").unwrap(), Quantity::scalar(-0.5));
    assert_eq!(ev("1e３").unwrap(), Quantity::scalar(1000.0));
    assert_eq!(ev("width ＋ ５ mm").unwrap(), Quantity::mm(45.0));
}
