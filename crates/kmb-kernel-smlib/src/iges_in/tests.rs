//! 確かめること: 面と立体の読み込み、単位と置き方、欠けた面と未対応要素の報告。
use super::*;
use crate::SmlibKernel;
use k::{BodyKind, Kernel};

struct Fixture {
    entries: Vec<(u32, i32, u32, String)>,
    unit: u32,
    scale: f64,
}
impl Fixture {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            unit: 2,
            scale: 1.,
        }
    }
    fn add(&mut self, kind: u32, parameters: impl Into<String>) -> u32 {
        self.entries.push((kind, 0, 0, parameters.into()));
        (self.entries.len() * 2 - 1) as u32
    }
    fn line(&mut self, a: P3, b: P3) -> u32 {
        self.add(
            110,
            format!("{},{},{},{},{},{}", a[0], a[1], a[2], b[0], b[1], b[2]),
        )
    }
    fn plane(&mut self, points: &[P3], normal: P3) -> u32 {
        let lines = (0..points.len())
            .map(|i| self.line(points[i], points[(i + 1) % points.len()]))
            .collect::<Vec<_>>();
        let boundary = self.add(
            102,
            format!(
                "{},{}",
                lines.len(),
                lines
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        );
        let id = self.add(
            108,
            format!(
                "{},{},{},{},{boundary},0,0,0,1",
                normal[0],
                normal[1],
                normal[2],
                k::dot(normal, points[0])
            ),
        );
        self.entries[(id / 2) as usize].1 = 1;
        id
    }
    fn text(&self) -> String {
        let record = |text: &str, section: char, seq: usize| format!("{text:<72}{section}{seq:7}");
        let mut global = vec![String::new(); 20];
        global[0] = "1H,".into();
        global[1] = "1H;".into();
        global[2] = "4Htest".into();
        global[12] = self.scale.to_string();
        global[13] = self.unit.to_string();
        global[14] = "2HMM".into();
        global[18] = "1D-6".into();
        let g = (global.join(",") + ";")
            .as_bytes()
            .chunks(72)
            .enumerate()
            .map(|(i, b)| record(std::str::from_utf8(b).unwrap(), 'G', i + 1))
            .collect::<Vec<_>>();
        let mut d = Vec::new();
        let mut p = Vec::new();
        for (i, (kind, form, transform, params)) in self.entries.iter().enumerate() {
            let id = i * 2 + 1;
            let text = format!("{kind},{params};");
            let chunks = text.as_bytes().chunks(64).collect::<Vec<_>>();
            d.push(record(
                &format!(
                    "{kind:8}{:8}{:8}{:8}{:8}{:8}{transform:8}{:8}00000000",
                    p.len() + 1,
                    0,
                    0,
                    0,
                    0,
                    0
                ),
                'D',
                id,
            ));
            d.push(record(
                &format!(
                    "{kind:8}{:8}{:8}{:8}{form:8}{:8}{:8}{:8}{:8}",
                    0,
                    0,
                    chunks.len(),
                    0,
                    0,
                    0,
                    0
                ),
                'D',
                id + 1,
            ));
            for chunk in chunks {
                p.push(format!(
                    "{:<64}{id:8}P{:7}",
                    std::str::from_utf8(chunk).unwrap(),
                    p.len() + 1
                ));
            }
        }
        let end = record(
            &format!("S{:7}G{:7}D{:7}P{:7}", 1, g.len(), d.len(), p.len()),
            'T',
            1,
        );
        std::iter::once(record("test", 'S', 1))
            .chain(g)
            .chain(d)
            .chain(p)
            .chain([end])
            .collect::<Vec<_>>()
            .join("\n")
    }
}
fn square() -> [P3; 4] {
    [[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.]]
}
fn patch(f: &mut Fixture, z: f64) -> u32 {
    f.add(
        128,
        format!("1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,10,0,0,0,10,0,10,10,{z},0,1,0,1"),
    )
}
#[test]
fn plane_and_nurbs_patch_keep_faces_area_rims_and_unit_scale() {
    for spline in [false, true] {
        let mut f = Fixture::new();
        if spline {
            patch(&mut f, 0.);
        } else {
            f.plane(&square(), [0., 0., 1.]);
        }
        let kernel = SmlibKernel::new();
        let (shape, warnings) = kernel.import_iges(&f.text()).unwrap();
        assert_eq!(kernel.body_kind(shape).unwrap(), BodyKind::Sheet);
        let faces = kernel.faces(shape).unwrap();
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].name, "iges1:f1");
        assert!((faces[0].area - 100.).abs() < 1e-5);
        assert!(
            warnings.iter().any(|s| s.contains("閉じなかった面 1 枚")),
            "{warnings:?}"
        );
        assert_eq!(kernel.curve_edges(shape).unwrap().len(), 4);
        assert!(
            kernel
                .volume(shape)
                .unwrap_err()
                .to_string()
                .contains("体積がありません")
        );
        f.unit = 1;
        f.scale = 2.;
        let (scaled, _) = kernel.import_iges(&f.text()).unwrap();
        assert!((kernel.faces(scaled).unwrap()[0].area - 100. * 12.7_f64.powi(2)).abs() < 1e-3);
    }
}
#[test]
fn circle_bounds_and_matrix_are_exact_and_unused_entities_are_reported() {
    let mut f = Fixture::new();
    let circle = f.add(100, "0,0,0,5,0,5,0");
    let plane = f.add(108, format!("0,0,1,0,{circle},0,0,0,1"));
    f.entries[(plane / 2) as usize].1 = 1;
    let matrix = f.add(124, "1,0,0,20,0,1,0,30,0,0,1,40");
    f.entries[(plane / 2) as usize].2 = matrix;
    f.add(110, "0,0,0,1,2,3");
    f.add(999, "0");
    let kernel = SmlibKernel::new();
    let (shape, warnings) = kernel.import_iges(&f.text()).unwrap();
    let faces = kernel.faces(shape).unwrap();
    assert!(
        (faces[0].area / (25. * std::f64::consts::PI) - 1.).abs() < 5e-5,
        "area {} warnings {warnings:?} bounds {:?}",
        faces[0].area,
        kernel.bounds(shape)
    );
    let bounds = kernel.bounds(shape).unwrap();
    assert!((bounds.0[0] - 15.).abs() < 5e-6);
    assert!((bounds.0[2] - 40.).abs() < 5e-6);
    assert!(warnings.iter().any(|s| s.contains("線・曲線 1 要素")));
    assert!(warnings.iter().any(|s| s.contains("種類 999 を 1 要素")));
}
#[test]
fn invalid_faces_leave_other_faces_and_report_reasons_without_panicking() {
    let mut f = Fixture::new();
    f.plane(&square(), [0., 0., 1.]);
    f.add(144, "9999,1,0,9997");
    let kernel = SmlibKernel::new();
    let (shape, warnings) = kernel.import_iges(&f.text()).unwrap();
    assert_eq!(kernel.faces(shape).unwrap().len(), 1);
    assert!(
        warnings
            .iter()
            .any(|s| s.contains("読めませんでした") && s.contains("9999")),
        "{warnings:?}"
    );
    assert!(kernel.import_iges("garbage").is_err());
    let mut cycle = Fixture::new();
    cycle.add(102, "1,1");
    let plane = cycle.add(108, "0,0,1,0,1,0,0,0,1");
    cycle.entries[(plane / 2) as usize].1 = 1;
    assert!(
        kernel
            .import_iges(&cycle.text())
            .unwrap_err()
            .to_string()
            .contains("循環")
    );
}

fn box_fixture() -> Fixture {
    let mut f = Fixture::new();
    let pts = [
        [0., 0., 0.],
        [10., 0., 0.],
        [10., 10., 0.],
        [0., 10., 0.],
        [0., 0., 10.],
        [10., 0., 10.],
        [10., 10., 10.],
        [0., 10., 10.],
    ];
    let mut faces = Vec::new();
    for (indices, normal) in [
        ([0, 3, 2, 1], [0., 0., -1.]),
        ([4, 5, 6, 7], [0., 0., 1.]),
        ([0, 1, 5, 4], [0., -1., 0.]),
        ([1, 2, 6, 5], [1., 0., 0.]),
        ([2, 3, 7, 6], [0., 1., 0.]),
        ([3, 0, 4, 7], [-1., 0., 0.]),
    ] {
        let points = indices.map(|i| pts[i]);
        let vertices = f.add(
            502,
            format!(
                "4,{}",
                points
                    .iter()
                    .flat_map(|p| p.iter())
                    .map(f64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        );
        let mut tuples = Vec::new();
        for i in 0..4 {
            let curve = f.line(points[i], points[(i + 1) % 4]);
            tuples.push(format!(
                "{curve},{vertices},{},{vertices},{}",
                i + 1,
                (i + 1) % 4 + 1
            ));
        }
        let edges = f.add(504, format!("4,{}", tuples.join(",")));
        let boundary = f.add(
            508,
            format!(
                "4,{}",
                (1..=4)
                    .map(|i| format!("0,{edges},{i},1,0"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        );
        let surface = f.add(
            108,
            format!(
                "{},{},{},{},0,0,0,0,1",
                normal[0],
                normal[1],
                normal[2],
                k::dot(normal, points[0])
            ),
        );
        faces.push(f.add(510, format!("{surface},1,1,{boundary}")));
    }
    let shell = f.add(
        514,
        format!(
            "6,{}",
            faces
                .iter()
                .map(|id| format!("{id},1"))
                .collect::<Vec<_>>()
                .join(",")
        ),
    );
    f.add(186, format!("{shell},1,0"));
    f
}
#[test]
fn topology_box_stitches_to_a_solid_with_stable_names_and_volume() {
    let f = box_fixture();
    let kernel = SmlibKernel::new();
    let (shape, warnings) = kernel.import_iges(&f.text()).unwrap();
    assert_eq!(
        kernel.body_kind(shape).unwrap(),
        BodyKind::Solid,
        "{warnings:?}"
    );
    assert!((kernel.volume(shape).unwrap() - 1000.).abs() < 1e-5);
    assert!(warnings.is_empty(), "{warnings:?}");
    let names = kernel
        .faces(shape)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect::<BTreeSet<_>>();
    assert_eq!(names, (1..=6).map(|i| format!("iges1:f{i}")).collect());
    let (again, warnings) = kernel.import_iges(&f.text()).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(
        names,
        kernel
            .faces(again)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect()
    );
}
#[test]
fn parameter_space_trim_and_hole_keep_the_requested_area() {
    let mut f = Fixture::new();
    let surface = patch(&mut f, 0.);
    let mut boundaries = Vec::new();
    for polygon in [
        [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
        [
            [0.2, 0.2, 0.],
            [0.2, 0.8, 0.],
            [0.8, 0.8, 0.],
            [0.8, 0.2, 0.],
        ],
    ] {
        let curves = (0..4)
            .map(|i| f.line(polygon[i], polygon[(i + 1) % 4]))
            .collect::<Vec<_>>();
        let curve = f.add(
            102,
            format!(
                "4,{}",
                curves
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        );
        boundaries.push(f.add(142, format!("0,{surface},{curve},0,1")));
    }
    f.add(
        144,
        format!("{surface},1,1,{},{}", boundaries[0], boundaries[1]),
    );
    let kernel = SmlibKernel::new();
    let (shape, warnings) = kernel.import_iges(&f.text()).unwrap();
    let faces = kernel.faces(shape).unwrap();
    assert_eq!(faces.len(), 1, "{warnings:?}");
    assert!(
        (faces[0].area - 64.).abs() < 1e-5,
        "{} {warnings:?}",
        faces[0].area
    );
    assert_eq!(kernel.curve_edges(shape).unwrap().len(), 8);
}
#[test]
fn rational_surface_preserves_a_quarter_cylinder() {
    let mut f = Fixture::new();
    let w = std::f64::consts::FRAC_1_SQRT_2;
    f.add(128,format!("2,1,2,1,0,0,0,0,0,0,0,0,1,1,1,0,0,1,1,1,{w},1,1,{w},1,10,0,0,10,10,0,0,10,0,10,0,10,10,10,10,0,10,10,0,1,0,1"));
    let kernel = SmlibKernel::new();
    let (shape, warnings) = kernel.import_iges(&f.text()).unwrap();
    let faces = kernel.faces(shape).unwrap();
    assert_eq!(faces.len(), 1, "{warnings:?}");
    assert!((faces[0].area / (50. * std::f64::consts::PI) - 1.).abs() < 1e-4);
    let mesh = kernel.mesh(shape, 0.01).unwrap();
    assert!(!mesh.positions.is_empty());
    for p in mesh.positions {
        assert!((p[0].hypot(p[1]) - 10.).abs() < 1e-5, "{p:?}");
    }
}

#[test]
fn whole_analytic_spheres_and_tori_are_closed_solids() {
    for torus in [false, true] {
        let mut f = Fixture::new();
        let center = f.add(116, "0,0,0,0");
        if torus {
            let axis = f.add(123, "0,0,1");
            f.add(198, format!("{center},{axis},10,2"));
        } else {
            f.add(196, format!("{center},5"));
        }
        let kernel = SmlibKernel::new();
        let (shape, warnings) = kernel.import_iges(&f.text()).unwrap();
        assert_eq!(
            kernel.body_kind(shape).unwrap(),
            BodyKind::Solid,
            "{warnings:?}"
        );
        let expected = if torus {
            2. * std::f64::consts::PI.powi(2) * 10. * 4.
        } else {
            4. / 3. * std::f64::consts::PI * 125.
        };
        assert!(
            (kernel.volume(shape).unwrap() / expected - 1.).abs() < 2e-4,
            "{warnings:?}"
        );
    }
}

#[test]
fn bounded_cylinders_and_cones_use_model_space_curves_and_boundary_orientation() {
    for cone in [false, true] {
        let mut f = Fixture::new();
        let origin = f.add(116, "0,0,0,0");
        let axis = f.add(123, "0,0,1");
        let surface = if cone {
            f.add(
                194,
                format!("{origin},{axis},5,{}", 0.5_f64.atan().to_degrees()),
            )
        } else {
            f.add(192, format!("{origin},{axis},5"))
        };
        let r = if cone { 10. } else { 5. };
        let bottom = f.add(100, "0,0,0,5,0,0,5");
        let end = f.line([0., 5., 0.], [0., r, 10.]);
        let top = f.add(100, format!("10,0,0,{r},0,0,{r}"));
        let start = f.add(
            126,
            format!("1,1,0,0,1,0,0,0,1,1,1,1,5,0,0,{r},0,10,0,1,0,0,1"),
        );
        let boundary = f.add(
            141,
            format!("0,0,{surface},4,{bottom},1,0,{end},1,0,{top},2,0,{start},2,0"),
        );
        f.add(143, format!("0,{surface},1,{boundary}"));
        let kernel = SmlibKernel::new();
        let (shape, warnings) = kernel.import_iges(&f.text()).unwrap();
        let faces = kernel.faces(shape).unwrap();
        assert_eq!(faces.len(), 1, "{warnings:?}");
        let expected = if cone {
            std::f64::consts::FRAC_PI_2 * 7.5 * 125_f64.sqrt()
        } else {
            std::f64::consts::FRAC_PI_2 * 50.
        };
        assert!(
            (faces[0].area / expected - 1.).abs() < 2e-4,
            "{} {expected} {warnings:?}",
            faces[0].area
        );
        assert_eq!(kernel.body_kind(shape).unwrap(), BodyKind::Sheet);
    }
}

#[test]
fn mixed_closed_and_open_faces_do_not_lose_the_open_sheet_or_report_partial_volume() {
    let mut f = box_fixture();
    f.plane(
        &[
            [0., 0., 20.],
            [10., 0., 20.],
            [10., 10., 20.],
            [0., 10., 20.],
        ],
        [0., 0., 1.],
    );
    let kernel = SmlibKernel::new();
    let (shape, warnings) = kernel.import_iges(&f.text()).unwrap();
    assert_eq!(kernel.faces(shape).unwrap().len(), 7, "{warnings:?}");
    assert_eq!(
        kernel.body_kind(shape).unwrap(),
        BodyKind::Sheet,
        "{warnings:?}"
    );
    assert!(kernel.volume(shape).is_err());
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("閉じなかった面") && w.contains("mm")),
        "{warnings:?}"
    );
}
#[test]
fn an_unreadable_trim_does_not_fall_back_to_its_untrimmed_surface() {
    let mut f = Fixture::new();
    f.plane(&square(), [0., 0., 1.]);
    let s = patch(&mut f, 4.);
    f.add(144, format!("{s},1,0,9999"));
    let model = read(&f.text()).unwrap();
    assert_eq!(model.faces.len(), 1);
    assert!(model.warnings.iter().any(|w| w.contains("9999")));
}

#[test]
fn empty_boundaries_and_negative_plane_forms_cannot_create_an_unbounded_face() {
    for negative in [false, true] {
        let mut f = Fixture::new();
        let plane = f.plane(&square(), [0., 0., 1.]);
        if negative {
            f.entries[(plane / 2) as usize].1 = -1;
        } else {
            let boundary = f.add(141, format!("0,0,{plane},0"));
            f.add(143, format!("0,{plane},1,{boundary}"));
        }
        assert!(read(&f.text()).is_err());
    }
}
