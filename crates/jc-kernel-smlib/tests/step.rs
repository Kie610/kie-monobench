//! STEP の読み込み(M56): 自前の読み手で SMLib の立体に組み、穴・ポケット(足し引き)に使える。
//! 網で読む truck に任せないので、`SmlibKernel::new()`(任せる先なし)で読めることを確かめる。
use jc_core::kernel::*;
use jc_kernel_smlib::SmlibKernel;
use std::f64::consts::{PI, TAU};

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

fn testdata(file: &str) -> String {
    std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../jc-kernel-truck/testdata/").to_string() + file,
    )
    .unwrap()
}

fn line(a: [f64; 2], b: [f64; 2], name: &str) -> ProfileEdge {
    ProfileEdge {
        name: name.into(),
        segment: Segment::Line { a, b },
        reversed: false,
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> ProfileLoop {
    let p = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    ProfileLoop {
        edges: (0..4)
            .map(|i| line(p[i], p[(i + 1) % 4], &format!("e{}", i + 1)))
            .collect(),
    }
}

fn circle(c: [f64; 2], r: f64, reversed: bool) -> ProfileLoop {
    ProfileLoop {
        edges: vec![ProfileEdge {
            name: "e9".into(),
            segment: Segment::Circle {
                center: c,
                radius: r,
            },
            reversed,
        }],
    }
}

fn on_xy(outer: ProfileLoop, holes: Vec<ProfileLoop>) -> Profile {
    Profile {
        plane: Plane::XY,
        regions: vec![Region { outer, holes }],
    }
}

/// 上の面(z = top)から深さ depth の円柱の穴を引く。
fn drill(
    k: &SmlibKernel,
    h: ShapeHandle,
    c: [f64; 2],
    r: f64,
    top: f64,
    depth: f64,
) -> ShapeHandle {
    let tool = k
        .extrude(
            &on_xy(circle(c, r, false), vec![]),
            top - depth,
            top + 1.0,
            "hole",
        )
        .unwrap();
    k.boolean(BooleanOp::Subtract, h, tool).unwrap()
}

#[test]
fn step_files_from_another_cad_become_solids_that_take_a_hole() {
    let k = SmlibKernel::new();
    for (file, volume, area) in [
        ("reference.step", 22806.876, 7678.499),
        ("reference-fillet.step", 22772.540, 7637.296),
    ] {
        let h = k
            .import_step(&testdata(file))
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        let (solid, solids, _) = k.check(h).unwrap();
        assert!(solid && solids == 1, "{file}: {solid} {solids}");
        let m = k.mass(h).unwrap();
        assert!(close(m.volume, volume, 1e-4), "{file}: 体積 {}", m.volume);
        assert!(close(m.area, area, 1e-4), "{file}: 表面積 {}", m.area);
        let faces = k.faces(h).unwrap();
        assert!(
            faces.iter().all(|f| f.name.starts_with("step0:f")),
            "{file}: {:?}",
            faces.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
        // 板の上の面(z = 10)は平面として分かる(穴・押し出しの向きに使う)
        let top = faces
            .iter()
            .find(|f| {
                f.planar
                    .is_some_and(|p| p.normal[2] > 0.999 && (p.point[2] - 10.0).abs() < 1e-6)
            })
            .unwrap_or_else(|| panic!("{file}: 上の面が無い"));
        assert!(!k.edges(h).unwrap().is_empty(), "{file}: 辺");
        // 板の上の面に直径 4 mm・深さ 3 mm の穴(見本の部品の穴・ポケット・ボスと重ならない所)
        let hole = PI * 2.0 * 2.0 * 3.0;
        let drilled = drill(&k, h, [20.0, 30.0], 2.0, 10.0, 3.0);
        let after = k.mass(drilled).unwrap().volume;
        assert!(
            close(after, m.volume - hole, 1e-4),
            "{file}: 穴の後 {after}、前 {} − 穴 {hole}",
            m.volume
        );
        // 穴を空けた後も、読み込んだ上の面の名前は残る
        let names: Vec<String> = k
            .faces(drilled)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert!(names.contains(&top.name), "{file}: {names:?}");
    }
}

#[test]
fn a_step_we_wrote_reads_back_as_the_same_solid() {
    let k = SmlibKernel::new();
    // 穴の空いた板、円すい台、円環、球の欠けた形
    let plate = k
        .extrude(
            &on_xy(
                rect(0.0, 0.0, 60.0, 40.0),
                vec![circle([30.0, 20.0], 4.0, true)],
            ),
            0.0,
            10.0,
            "f1",
        )
        .unwrap();
    let revolve = |outer: ProfileLoop, name: &str| {
        k.revolve(
            &Profile {
                plane: Plane::ZX,
                regions: vec![Region {
                    outer,
                    holes: vec![],
                }],
            },
            [0.0; 3],
            [0.0, 0.0, 1.0],
            TAU,
            name,
        )
        .unwrap()
    };
    let cone = revolve(
        ProfileLoop {
            edges: vec![
                line([0.0, 0.0], [0.0, 8.0], "e1"),
                line([0.0, 8.0], [12.0, 3.0], "e2"),
                line([12.0, 3.0], [12.0, 0.0], "e3"),
                line([12.0, 0.0], [0.0, 0.0], "e4"),
            ],
        },
        "f2",
    );
    let torus = revolve(circle([5.0, 20.0], 3.0, false), "f3");
    for (what, h) in [("板", plate), ("円すい台", cone), ("円環", torus)] {
        let before = k.mass(h).unwrap();
        let n = k.faces(h).unwrap().len();
        let step = k.export_step(&[h]).unwrap();
        let back = k
            .import_step(&step)
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        let after = k.mass(back).unwrap();
        assert!(
            close(after.volume, before.volume, 1e-6),
            "{what}: 体積 {} 元 {}",
            after.volume,
            before.volume
        );
        // 表面積は面ごとの数値積分なので、曲面の切り抜き方が違うと少しずれる
        assert!(
            close(after.area, before.area, 1e-4),
            "{what}: 表面積 {} 元 {}",
            after.area,
            before.area
        );
        assert_eq!(k.faces(back).unwrap().len(), n, "{what}: 面の数");
        // 元の形と足しても、元の形のまま(同じ所に重なる)
        let u = k.boolean(BooleanOp::Union, back, h).unwrap();
        assert!(
            close(k.mass(u).unwrap().volume, before.volume, 1e-6),
            "{what}: 足した体積"
        );
    }
}

#[test]
fn names_are_written_with_their_japanese_characters_and_read_back() {
    let k = SmlibKernel::new();
    let plate = k
        .extrude(&on_xy(rect(0.0, 0.0, 20.0, 10.0), vec![]), 0.0, 5.0, "f1")
        .unwrap();
    let before = k.mass(plate).unwrap().volume;
    let step = k
        .export_step_named(&[plate], &["ボディ1".into()], "部品 It's \\ 🙂")
        .unwrap();
    // 部品(U+90E8 U+54C1)、' と \ は 2 つ、基本面の外の字は \X4\
    assert!(
        step.contains("PRODUCT('\\X2\\90E854C1\\X0\\ It''s \\\\ \\X4\\0001F642\\X0\\'"),
        "{step}"
    );
    assert!(
        step.contains("MANIFOLD_SOLID_BREP('\\X2\\30DC30C730A3\\X0\\1'"),
        "{step}"
    );
    let back = k.import_step(&step).unwrap();
    assert!(close(k.mass(back).unwrap().volume, before, 1e-6));
    // 名前を渡さない書き出しは前と同じ仮の名前
    let step = k.export_step(&[plate]).unwrap();
    assert!(step.contains("MANIFOLD_SOLID_BREP('body1'"), "{step}");
    assert!(step.contains("PRODUCT('kieMonoBench'"), "{step}");
}

#[test]
fn a_step_that_cannot_be_a_solid_falls_back_to_the_mesh() {
    // 面だけの殻は立体にならないので、任せる先(truck)の網で読む
    let open = testdata("reference.step").replace("CLOSED_SHELL", "OPEN_SHELL");
    let k = SmlibKernel::new();
    let err = k.import_step(&open).unwrap_err();
    assert!(err.to_string().contains("STEP"), "{err}");
    let k = SmlibKernel::new().with_step_import(jc_kernel_truck::TruckKernel::new());
    assert!(k.import_step(&testdata("reference.step")).is_ok());
}

#[test]
fn a_cone_that_ends_in_a_point_becomes_a_solid() {
    // 円すいの面の縁が底の円だけ・頂点の点の輪あり・継ぎ目の直線ありの 3 通り
    let k = SmlibKernel::new();
    let volume = 256.0 * PI;
    let area = PI * 64.0 + PI * 8.0 * (64.0f64 + 144.0).sqrt();
    for file in [
        "cone-point.step",
        "cone-point-vertex.step",
        "cone-point-seam.step",
    ] {
        let h = k
            .import_step(&testdata(file))
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        let m = k.mass(h).unwrap();
        assert!(close(m.volume, volume, 1e-6), "{file}: 体積 {}", m.volume);
        assert!(close(m.area, area, 1e-4), "{file}: 表面積 {}", m.area);
        let (solid, solids, _) = k.check(h).unwrap();
        assert!(solid && solids == 1, "{file}");
    }
}
