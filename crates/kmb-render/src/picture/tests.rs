use super::*;

/// 原点から大きさ `s` の立方体(面は "+z" などの名前、外向きの三角形、稜線 12 本)。
fn cube(s: f32) -> Mesh {
    let mut m = Mesh::default();
    let faces: [(&str, V3, V3, V3); 6] = [
        ("+z", [0.0, 0.0, s], [s, 0.0, 0.0], [0.0, s, 0.0]),
        ("-z", [0.0, 0.0, 0.0], [0.0, s, 0.0], [s, 0.0, 0.0]),
        ("+x", [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]),
        ("-x", [0.0, 0.0, 0.0], [0.0, 0.0, s], [0.0, s, 0.0]),
        ("+y", [0.0, s, 0.0], [0.0, 0.0, s], [s, 0.0, 0.0]),
        ("-y", [0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, 0.0, s]),
    ];
    for (fi, (name, o, u, v)) in faces.iter().enumerate() {
        let n = normalize(cross(*u, *v));
        let base = m.positions.len() as u32;
        for p in [
            *o,
            [o[0] + u[0], o[1] + u[1], o[2] + u[2]],
            [o[0] + u[0] + v[0], o[1] + u[1] + v[1], o[2] + u[2] + v[2]],
            [o[0] + v[0], o[1] + v[1], o[2] + v[2]],
        ] {
            m.positions.push(p);
            m.normals.push(n);
        }
        m.triangles.push([base, base + 1, base + 2]);
        m.triangles.push([base, base + 2, base + 3]);
        m.triangle_faces.extend([fi as u32; 2]);
        m.face_names.push(name.to_string());
    }
    let c = |x: f32, y: f32, z: f32| [x * s, y * s, z * s];
    for (a, b) in [
        (c(0., 0., 0.), c(1., 0., 0.)),
        (c(1., 0., 0.), c(1., 1., 0.)),
        (c(1., 1., 0.), c(0., 1., 0.)),
        (c(0., 1., 0.), c(0., 0., 0.)),
        (c(0., 0., 1.), c(1., 0., 1.)),
        (c(1., 0., 1.), c(1., 1., 1.)),
        (c(1., 1., 1.), c(0., 1., 1.)),
        (c(0., 1., 1.), c(0., 0., 1.)),
        (c(0., 0., 0.), c(0., 0., 1.)),
        (c(1., 0., 0.), c(1., 0., 1.)),
        (c(1., 1., 0.), c(1., 1., 1.)),
        (c(0., 1., 0.), c(0., 1., 1.)),
    ] {
        m.edges.push(vec![a, b]);
    }
    m
}

fn top(o: Options) -> Options {
    let (yaw_deg, pitch_deg) = view("top").unwrap();
    Options {
        yaw_deg,
        pitch_deg,
        width: 200,
        height: 100,
        ..o
    }
}

// 元は 2 本(選ばない絵は 1 回だけ描く):
// - ボディは真ん中に稜線つきで描かれ、残りは背景
// - 選んだ面と辺は差し色で目立ち、別のボディの面の名前では変わらない
#[test]
fn a_body_is_drawn_in_the_middle_and_chosen_faces_and_edges_stand_out() {
    let m = cube(10.0);
    let bodies = [Body {
        feature: FeatureId(1),
        mesh: &m,
    }];
    let p = render(&bodies, &top(Options::default()));
    assert_eq!((p.width, p.height, p.rgb.len()), (200, 100, 200 * 100 * 3));
    // 高さに合わせて 85 % の大きさ(10 mm → 85 画素)
    assert!((p.px_per_mm - 8.5).abs() < 0.05, "{}", p.px_per_mm);
    assert_eq!(p.at(2, 2), BACKGROUND);
    let mid = p.at(100, 50);
    assert_ne!(mid, BACKGROUND);
    assert!(
        mid[0] > 150 && mid[0] == mid[1] && mid[2].abs_diff(mid[1]) <= 6,
        "ボディは鋼の灰色: {mid:?}"
    );
    // 上面の縁(x = 100 ± 42.5)に稜線
    let darkest = (54..62).map(|x| p.at(x, 50)[0]).min().unwrap();
    assert!(darkest < 120, "稜線は濃い: {darkest}");

    let plain = p;
    let lit = render(
        &bodies,
        &top(Options {
            faces: vec![FacePick {
                feature: FeatureId(1),
                face: "+z".into(),
            }],
            edges: vec![(FeatureId(1), 4)],
            ..Options::default()
        }),
    );
    let (a, b) = (plain.at(100, 50), lit.at(100, 50));
    assert!(b[1] > b[0] && a[0] == a[1], "上面は差し色: {a:?} → {b:?}");
    // 4 番の辺(上面の y = 0 の辺)は太い差し色。絵の下の縁(y = 50 + 42.5)
    assert!(
        (88..97).any(|y| {
            let e = lit.at(100, y);
            e[1] > e[0] + 30
        }),
        "辺は差し色"
    );
    // 別のボディの面の名前では変わらない
    let other = render(
        &bodies,
        &top(Options {
            faces: vec![FacePick {
                feature: FeatureId(2),
                face: "+z".into(),
            }],
            ..Options::default()
        }),
    );
    assert_eq!(other.at(100, 50), a);
}

#[test]
fn the_png_has_the_signature_the_header_and_the_pixels() {
    let m = cube(5.0);
    let p = render(
        &[Body {
            feature: FeatureId(1),
            mesh: &m,
        }],
        &Options {
            width: 64,
            height: 48,
            ..Options::default()
        },
    );
    let png = p.png();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[12..16], b"IHDR");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 64);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 48);
    // IHDR の CRC
    let crc = crc32fast::hash(&png[12..29]);
    assert_eq!(u32::from_be_bytes(png[29..33].try_into().unwrap()), crc);
    // IDAT を解くと、行ごとに 0 と RGB が並ぶ
    let len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
    assert_eq!(&png[37..41], b"IDAT");
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&png[41..41 + len]).unwrap();
    assert_eq!(raw.len(), (64 * 3 + 1) * 48);
    assert_eq!(raw[0], 0);
    assert_eq!(&raw[1..64 * 3 + 1], &p.rgb[..64 * 3]);
    assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
}

#[test]
fn every_named_view_looks_at_the_body_from_its_side() {
    let m = cube(10.0);
    for (name, _, _, _) in VIEWS {
        let (yaw_deg, pitch_deg) = view(name).unwrap();
        let p = render(
            &[Body {
                feature: FeatureId(1),
                mesh: &m,
            }],
            &Options {
                yaw_deg,
                pitch_deg,
                width: 80,
                height: 80,
                ..Options::default()
            },
        );
        assert_ne!(p.at(40, 40), BACKGROUND, "{name}");
        assert_eq!(p.at(1, 1), BACKGROUND, "{name}");
    }
    assert!(view("no_such").is_none());
    // ボディが無ければ背景だけ
    let p = render(&[], &Options::default());
    assert!(p.rgb.chunks(3).all(|c| c == BACKGROUND));
}

#[test]
fn a_written_png_is_read_back_the_same() {
    let m = cube(10.0);
    let p = render(
        &[Body {
            feature: FeatureId(1),
            mesh: &m,
        }],
        &Options {
            width: 64,
            height: 48,
            ..Default::default()
        },
    );
    let back = Picture::from_png(&p.png()).expect("自分で書いた PNG は読める");
    assert_eq!((back.width, back.height), (64, 48));
    assert_eq!(back.rgb, p.rgb);
    assert!(Picture::from_png(b"not a png").is_none());
}
