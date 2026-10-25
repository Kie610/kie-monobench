use super::*;

#[test]
fn letters_are_single_lines_of_the_given_height() {
    let (segs, missing) = text_segments("A1", Font::Sans, [10.0, 5.0], 7.0);
    assert_eq!(missing, 0);
    assert!(!segs.is_empty());
    let ys: Vec<f64> = segs
        .iter()
        .flat_map(|s| match s {
            Segment::Line { a, b } => [a[1], b[1]],
            _ => unreachable!(),
        })
        .collect();
    let (lo, hi) = (
        ys.iter().copied().fold(f64::MAX, f64::min),
        ys.iter().copied().fold(f64::MIN, f64::max),
    );
    // 大文字の A は基準線から大文字の高さまで
    assert!(
        (lo - 5.0).abs() < 1e-9 && (hi - 12.0).abs() < 1e-9,
        "{lo} {hi}"
    );
    let (_, missing) = text_segments("あA", Font::Roman, [0.0, 0.0], 5.0);
    assert_eq!(missing, 1);
}

fn bounds(segs: &[Segment]) -> ([f64; 2], [f64; 2]) {
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    for s in segs {
        let Segment::Line { a, b } = s else {
            unreachable!()
        };
        for q in [a, b] {
            for i in 0..2 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
            }
        }
    }
    (lo, hi)
}

#[test]
fn the_chosen_point_of_the_text_sits_on_the_origin() {
    let mut st = Style::new(Font::Sans, 10.0);
    st.align = HAlign::Center;
    st.valign = VAlign::Middle;
    let l = lay_out("HI", &st, [50.0, 20.0]);
    let (lo, hi) = bounds(&l.segments);
    // H と I は大文字の高さいっぱいなので、中央は枠の中央に一致する
    assert!(((lo[0] + hi[0]) / 2.0 - 50.0).abs() < 0.6, "{lo:?} {hi:?}");
    assert!(((lo[1] + hi[1]) / 2.0 - 20.0).abs() < 1e-9, "{lo:?} {hi:?}");
    // 枠は線の外接の四角形
    assert_eq!(bounds(&l.frame), (lo, hi));
    st.align = HAlign::Right;
    st.valign = VAlign::Top;
    let (lo2, hi2) = bounds(&lay_out("HI", &st, [0.0, 0.0]).segments);
    assert!(hi2[0] <= 1e-9 && hi2[0] > -2.0, "{lo2:?} {hi2:?}");
    assert!(hi2[1].abs() < 1e-9, "{hi2:?}");
}

#[test]
fn rotation_spacing_and_width_change_the_layout() {
    let st = Style::new(Font::Sans, 5.0);
    let plain = lay_out("AB", &st, [0.0, 0.0]);
    let (_, hi) = bounds(&plain.segments);
    let wide = lay_out(
        "AB",
        &Style {
            spacing: 2.0,
            width: 2.0,
            ..st
        },
        [0.0, 0.0],
    );
    let (_, whi) = bounds(&wide.segments);
    assert!(whi[0] > hi[0] * 2.0 + 1.9, "{hi:?} {whi:?}");
    let turned = lay_out(
        "AB",
        &Style {
            angle: std::f64::consts::FRAC_PI_2,
            ..st
        },
        [0.0, 0.0],
    );
    let (tlo, thi) = bounds(&turned.segments);
    // 90° 回すと、横に並んでいた字が縦(+y)に並ぶ
    assert!(thi[1] > hi[0] - 1e-9 && tlo[0] < -4.9, "{tlo:?} {thi:?}");
}

#[test]
fn several_lines_align_line_by_line() {
    let mut st = Style::new(Font::Sans, 5.0);
    st.align = HAlign::Right;
    st.valign = VAlign::Bottom;
    let l = lay_out("WIDE\nI", &st, [0.0, 0.0]);
    let (lo, hi) = bounds(&l.segments);
    // 右揃え: どの行も右端が 0 の左にそろう。下: 最後の行の基準線が 0
    assert!(hi[0] <= 1e-9 && lo[1].abs() < 1e-9, "{lo:?} {hi:?}");
    assert!(hi[1] > 5.0 * 1.6, "2 行分の高さ: {hi:?}");
}

#[test]
fn the_longest_line_is_fitted_to_the_width() {
    let st = Style {
        spacing: 0.5,
        ..Style::new(Font::Sans, 5.0)
    };
    let text = "AB\nWIDE ONE";
    let stretched = st.fit_width(text, 30.0, false).unwrap();
    assert_eq!(stretched.height, 5.0);
    let (lo, hi) = bounds(&lay_out(text, &stretched, [0.0, 0.0]).segments);
    // 最後の字の右の余白の分だけ線は幅より短い
    assert!(
        hi[0] - lo[0] <= 30.0 + 1e-9 && hi[0] - lo[0] > 28.0,
        "{lo:?} {hi:?}"
    );
    let scaled = st.fit_width(text, 30.0, true).unwrap();
    assert_eq!(scaled.width, 1.0);
    assert!(scaled.height < 5.0, "{}", scaled.height);
    let (lo2, hi2) = bounds(&lay_out(text, &scaled, [0.0, 0.0]).segments);
    assert!(
        (hi2[0] - lo2[0] - (hi[0] - lo[0])).abs() < 0.5,
        "{lo2:?} {hi2:?}"
    );
    assert!(
        st.fit_width("A B C", 0.9, false).is_err(),
        "間隔だけで幅を超える"
    );
}

#[test]
fn text_along_a_line_is_the_same_as_turned_text() {
    let mut st = Style::new(Font::Roman, 4.0);
    st.align = HAlign::Center;
    let path = Path::new(
        Segment::Line {
            a: [10.0, 0.0],
            b: [10.0, 100.0],
        },
        false,
    );
    let along = lay_along("Ab", &st, &path, [30.0, 2.0]).unwrap();
    st.angle = std::f64::consts::FRAC_PI_2;
    let turned = lay_out("Ab", &st, [8.0, 30.0]);
    assert_eq!(along.segments.len(), turned.segments.len());
    for (p, q) in along.segments.iter().zip(&turned.segments) {
        let (Segment::Line { a, b }, Segment::Line { a: c, b: d }) = (p, q) else {
            unreachable!()
        };
        for i in 0..2 {
            assert!((a[i] - c[i]).abs() < 1e-9 && (b[i] - d[i]).abs() < 1e-9);
        }
    }
    assert!(along.warnings.is_empty(), "{:?}", along.warnings);
    let past = lay_along("LONG TEXT", &st, &path, [98.0, 0.0]).unwrap();
    assert_eq!(past.warnings.len(), 1, "端をはみ出す");
}

#[test]
fn text_on_a_circle_faces_the_centre_or_outwards() {
    let r = 30.0;
    let mut st = Style::new(Font::Sans, 5.0);
    st.align = HAlign::Center;
    let dist = |l: &Laid| {
        let mut ds = Vec::new();
        for s in &l.segments {
            let Segment::Line { a, b } = s else {
                unreachable!()
            };
            ds.push(a[0].hypot(a[1]));
            ds.push(b[0].hypot(b[1]));
        }
        let lo = ds.iter().copied().fold(f64::MAX, f64::min);
        let hi = ds.iter().copied().fold(f64::MIN, f64::max);
        (lo, hi)
    };
    let circle = |reverse| {
        Path::new(
            Segment::Circle {
                center: [0.0, 0.0],
                radius: r,
            },
            reverse,
        )
    };
    // 逆にたどる: 真上で外向き。字は円の外、中心は真上
    let out = lay_along("TOP", &st, &circle(true), [0.0, 0.0]).unwrap();
    let (lo, hi) = dist(&out);
    assert!(lo > r - 0.01 && hi < r + 5.0 + 0.5, "{lo} {hi}");
    let (blo, bhi) = bounds(&out.segments);
    // 字の送り幅で中央に揃えるので、線の外接の中央は少しずれる
    assert!(
        (blo[0] + bhi[0]).abs() < 1.0 && blo[1] > 0.0,
        "{blo:?} {bhi:?}"
    );
    // 反時計回り: 字の上は中心を向く。半周先(真下)に置けば正立して円の内側
    let half = std::f64::consts::PI * r;
    let inner = lay_along("BOTTOM", &st, &circle(false), [half, 0.0]).unwrap();
    let (lo, hi) = dist(&inner);
    assert!(lo > r - 5.0 - 0.5 && hi < r + 0.5, "{lo} {hi}");
    let (blo, _) = bounds(&inner.segments);
    assert!(blo[1] < -r + 0.6, "{blo:?}");
    // 長すぎれば重なると知らせ、中心を越えれば誤り
    let long = "X".repeat(60);
    assert_eq!(
        lay_along(&long, &st, &circle(true), [0.0, 0.0])
            .unwrap()
            .warnings
            .len(),
        1
    );
    assert!(lay_along("A", &st, &circle(false), [0.0, r + 1.0]).is_err());
}

#[test]
fn old_documents_read_the_same() {
    let t: Text = serde_json::from_value(serde_json::json!({"text": "A", "x": "1 mm"})).unwrap();
    assert_eq!(t.align, HAlign::Left);
    assert_eq!(t.valign, VAlign::Baseline);
    let v = serde_json::to_value(&t).unwrap();
    assert!(v.get("align").is_none() && v.get("anchor").is_none(), "{v}");
}

#[test]
fn connected_lines_are_followed_as_one_path() {
    use std::f64::consts::PI;
    // L 字(下の辺と右の辺)の先に、右の辺に接する四半円(中心 (30, 30)、半径 10)をつなぐ。
    // 右の辺は上から下へ描いてあり、並びもばらばら。離れた線は入らない
    let bottom = Segment::Line {
        a: [0.0, 0.0],
        b: [40.0, 0.0],
    };
    let right = Segment::Line {
        a: [40.0, 30.0],
        b: [40.0, 0.0],
    };
    let arc = Segment::Arc {
        center: [30.0, 30.0],
        radius: 10.0,
        start: 0.0,
        end: PI / 2.0,
    };
    let away = Segment::Line {
        a: [100.0, 0.0],
        b: [120.0, 0.0],
    };
    let (path, branched) = Path::chain(right.clone(), &[arc.clone(), away, bottom.clone()], false);
    assert!(!branched && !path.closed());
    assert_eq!(path.len(), 3);
    let len = 40.0 + 30.0 + PI / 2.0 * 10.0;
    assert!((path.length() - len).abs() < 1e-9);
    // 右の辺の本来の向き(上から下)に合わせ、円弧の終わりから始まって下の辺の始めで終わる
    let (p0, _, _) = path.at(0.0);
    assert!(
        (p0[0] - 30.0).abs() < 1e-9 && (p0[1] - 40.0).abs() < 1e-9,
        "{p0:?}"
    );
    let (p1, t1, _) = path.at(len);
    assert!(p1[0].abs() < 1e-9 && p1[1].abs() < 1e-9, "{p1:?}");
    assert!((t1[0] + 1.0).abs() < 1e-9, "終わりは左向き {t1:?}");
    // 区間の境目で点が飛ばない
    for k in 1..400 {
        let s = len * k as f64 / 400.0;
        let (a, _, _) = path.at(s - 1e-6);
        let (b, _, _) = path.at(s + 1e-6);
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-5, "{s}");
    }
    // 離れた線の上の長さと、線の上の長さは行き来できる(円弧の所だけ縮む)
    for s in [0.0, 5.0, 20.0, 50.0, len] {
        let x = path.offset_length(s, 2.0).unwrap();
        assert!((path.along_offset(x, 2.0).unwrap() - s).abs() < 1e-9);
    }
    // 円弧は時計回りにたどるので、左(外)に離れた線の上ではその分だけ長い
    assert!((path.offset_length(len, 2.0).unwrap() - (len + PI / 2.0 * 2.0)).abs() < 1e-9);
    // 文字は線に沿い、角の先の字は回る。字の上は進む向きの左(L 字の外)
    let st = Style::new(Font::Sans, 3.0);
    let laid = lay_along("ABCDEFGHIJKLMNOP", &st, &path, [16.0, 0.0]).unwrap();
    assert!(laid.warnings.is_empty(), "{:?}", laid.warnings);
    let mut sides = [0; 3];
    for seg in &laid.segments {
        let Segment::Line { a, .. } = seg else {
            unreachable!()
        };
        let to_arc = ((a[0] - 30.0).hypot(a[1] - 30.0) - 10.0).abs();
        let to_right = (a[0] - 40.0).abs() + (-a[1]).max(a[1] - 30.0).max(0.0);
        let to_bottom = a[1].abs() + (-a[0]).max(a[0] - 40.0).max(0.0);
        let d = to_arc.min(to_right).min(to_bottom);
        assert!(d < 3.6, "{a:?} は線から {d}");
        assert!(!(a[0] < 39.9 && a[1] > 0.1 && a[1] < 30.0), "{a:?} が内側");
        if a[1] > 30.0 {
            sides[0] += 1;
        } else if a[0] > 40.0 {
            sides[1] += 1;
        } else {
            sides[2] += 1;
        }
    }
    // 位置 16 から 16 文字(約 40 mm): 右の辺から下の辺へ回り込む
    assert!(sides[1] > 0 && sides[2] > 0, "{sides:?}");
    // 3 本が出会う所では止める
    let spur = Segment::Line {
        a: [40.0, 0.0],
        b: [50.0, -10.0],
    };
    let (short, branched) = Path::chain(right, &[arc, bottom, spur], false);
    assert!(branched);
    assert_eq!(short.len(), 2);
    // 四角は輪になる
    let sq = [
        Segment::Line {
            a: [0.0, 0.0],
            b: [10.0, 0.0],
        },
        Segment::Line {
            a: [10.0, 0.0],
            b: [10.0, 10.0],
        },
        Segment::Line {
            a: [0.0, 10.0],
            b: [10.0, 10.0],
        },
        Segment::Line {
            a: [0.0, 0.0],
            b: [0.0, 10.0],
        },
    ];
    let (ring, _) = Path::chain(sq[0].clone(), &sq[1..], false);
    assert!(ring.closed() && (ring.length() - 40.0).abs() < 1e-9);
}

#[test]
fn text_follows_the_whole_ellipse() {
    let mut s = jc_sketch::Sketch::default();
    let id = s
        .add_ellipse((None, [0.0, 0.0]), (None, [30.0, 0.0]), [4.0, 12.0])
        .unwrap();
    let sol = jc_sketch::solve(&s, &std::collections::HashMap::new()).unwrap();
    let segs: Vec<(EntityId, Segment)> = jc_sketch::entity_segments(&s, &sol, true)
        .into_iter()
        .filter(|(e, _)| *e == id)
        .collect();
    let first = segs[0].1.clone();
    let rest: Vec<Segment> = segs[1..].iter().map(|(_, g)| g.clone()).collect();
    let (path, branched) = Path::chain(first, &rest, false);
    assert!(!branched);
    let total: f64 = path.length();
    // ラマヌジャンの近似式(この楕円では 1e-4 mm より良い)
    let pi = std::f64::consts::PI;
    let perimeter =
        pi * (3.0 * (30.0 + 12.0) - ((3.0 * 30.0 + 12.0) * (30.0 + 3.0 * 12.0_f64)).sqrt());
    assert!(path.closed());
    assert!((total - perimeter).abs() < 0.05, "{total} {perimeter}");
}
