//! 走査線の試験。部品を三角形で作り、経路の点を確かめる。

use super::*;
use kmb_cam_dropcutter::limits::{region_of, slope_range};
use kmb_cam_dropcutter::{Boundary, Contain, Region};
use kmb_cam_setup::testing::{mesh_of, prepared};
use kmb_cam_tool::{Tool, ToolLibrary};

fn quad(a: P3, b: P3, c: P3, d: P3) -> Vec<[P3; 3]> {
    vec![[a, b, c], [a, c, d]]
}

fn top(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Vec<[P3; 3]> {
    quad([x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z])
}

fn ball() -> Tool {
    ToolLibrary::starter().tools[3].clone()
}

/// 床(高さ 0、x -10〜20)の上に、x 0〜10・y -5〜5・高さ 6 の箱。
fn block() -> Planner {
    let faces = vec![
        ("floor", top(-10.0, -10.0, 20.0, 10.0, 0.0)),
        ("block", top(0.0, -5.0, 10.0, 5.0, 6.0)),
    ];
    prepared(
        &mesh_of(&faces),
        [-10.0, -10.0, 0.0],
        [20.0, 10.0, 7.0],
        ball(),
    )
    .planner()
}

fn opts(p: &Planner) -> Opts {
    Opts {
        leave: 0.0,
        cell: 0.05,
        floor: p.info.stock_min[2],
        angle: None,
        along: Along::X,
        levels: false,
        direction: RasterDir::Both,
        limits: Resolved::default(),
        entry: None,
    }
}

/// 箱の高さより下の点での、工具の中心から箱までの横の距離 − 工具の半径(壁に残る厚さ)。
fn wall_gap(p: &Planner, passes: &[Vec<P3>]) -> f64 {
    passes
        .iter()
        .flatten()
        .filter(|q| q[2] < 5.0)
        .map(|q| {
            let dx = (0.0 - q[0]).max(q[0] - 10.0).max(0.0);
            let dy = (-5.0 - q[1]).max(q[1] - 5.0).max(0.0);
            dx.hypot(dy) - p.r()
        })
        .fold(f64::INFINITY, f64::min)
}

fn count(v: &[Vec<P3>]) -> usize {
    v.iter().map(|x| x.len()).sum()
}

#[test]
fn radial_leave_keeps_stock_on_steep_walls() {
    let p = block();
    let z_only = Opts {
        leave: 0.5,
        ..opts(&p)
    };
    let both = Opts {
        leave: 0.5,
        limits: Resolved {
            leave_radial: 0.5,
            ..Default::default()
        },
        ..opts(&p)
    };
    let a = wall_gap(&p, &raster_passes(&p, &z_only).unwrap());
    let b = wall_gap(&p, &raster_passes(&p, &both).unwrap());
    // Z に足すだけでは縦の壁に厚さが残らない(格子の幅ほど)。径方向に 0.5 mm 残すと 0.5 mm 離れる
    assert!(a < 0.1, "Z だけ: {a}");
    assert!((0.45..0.6).contains(&b), "径方向も: {b}");
    // 床の上の高さはどちらも 0.5 mm
    for o in [&z_only, &both] {
        let floor = raster_passes(&p, o)
            .unwrap()
            .iter()
            .flatten()
            .map(|q| q[2])
            .fold(f64::INFINITY, f64::min);
        assert!((floor - 0.5).abs() < 1e-6, "{floor}");
    }
}

#[test]
fn old_documents_read_the_same() {
    // 新しい欄のない文書は、新しい欄が既定のまま読める
    let v = serde_json::json!({
        "setup": 1, "tools": 2, "tool": 4, "leave": "0 mm", "along": "x",
        "resolution": "0.05 mm", "levels": true
    });
    let r: Raster3d = serde_json::from_value(v).unwrap();
    assert_eq!(r.limits, Limits::default());
    assert_eq!(r.direction, RasterDir::Both);
    // 書き戻しても新しい欄は増えない
    let back = serde_json::to_value(&r).unwrap();
    for k in [
        "leave_radial",
        "boundary",
        "faces",
        "contain",
        "slope_from",
        "direction",
    ] {
        assert!(back.get(k).is_none(), "{k}");
    }
}

#[test]
fn the_boundary_keeps_the_cut_inside() {
    let p = block();
    let all = raster_passes(&p, &opts(&p)).unwrap();
    let with = |region| Opts {
        limits: Resolved {
            region,
            ..Default::default()
        },
        ..opts(&p)
    };
    let faces = |c, off| region_of(&p, Boundary::Faces, &["block".into()], c, off, 0.1).unwrap();
    // 選んだ面(箱の上面)の中だけ
    let fb = raster_passes(&p, &with(faces(Contain::Center, 0.0))).unwrap();
    for q in fb.iter().flatten() {
        assert!(
            (-0.06..=10.06).contains(&q[0]) && (-5.06..=5.06).contains(&q[1]),
            "{q:?}"
        );
    }
    assert!(count(&fb) * 3 < count(&all));
    // 工具が丸ごと中 < 中心が中 < 工具が触れる所まで
    let a = raster_passes(&p, &with(faces(Contain::Inside, 0.0))).unwrap();
    let c = raster_passes(&p, &with(faces(Contain::Outside, 0.0))).unwrap();
    assert!(
        count(&a) < count(&fb) && count(&fb) < count(&c),
        "{} {} {}",
        count(&a),
        count(&fb),
        count(&c)
    );
    // 追加のずらしで広げると、点が増える
    let wider = raster_passes(&p, &with(faces(Contain::Center, 2.0))).unwrap();
    assert!(count(&wider) > count(&fb));
    // シルエット(床を含む部品の全体)なら、限らない時とほぼ同じ
    let sil = region_of(&p, Boundary::Silhouette, &[], Contain::Center, 0.0, 0.1).unwrap();
    let s = raster_passes(&p, &with(sil)).unwrap();
    assert!(count(&s) * 10 > count(&all) * 8);
}

#[test]
fn slope_limits_pick_steep_or_gentle_parts() {
    // x 0〜10 で 0 から 10 へ上る 45° の斜面と、その両側の平らな所
    let faces = vec![
        ("low", top(-10.0, -5.0, 0.0, 5.0, 0.0)),
        (
            "slope",
            quad(
                [0.0, -5.0, 0.0],
                [10.0, -5.0, 10.0],
                [10.0, 5.0, 10.0],
                [0.0, 5.0, 0.0],
            ),
        ),
        ("high", top(10.0, -5.0, 20.0, 5.0, 10.0)),
    ];
    let p = prepared(
        &mesh_of(&faces),
        [-10.0, -5.0, 0.0],
        [20.0, 5.0, 11.0],
        ball(),
    )
    .planner();
    let base = Opts {
        cell: 0.1,
        ..opts(&p)
    };
    let with = |from: Option<f64>, to: Option<f64>| Opts {
        limits: Resolved {
            slope: slope_range(from.map(f64::to_radians), to.map(f64::to_radians)).unwrap(),
            ..Default::default()
        },
        ..base.clone()
    };
    // 斜面の上(ボールの先が斜面に当たる所。斜面の両端の丸めを除く)を削る長さと、全部の長さ
    // (点は一直線に並ぶと間引くので、長さで比べる)
    let on_slope = |v: &[Vec<P3>]| {
        let (mut mid, mut all) = (0.0, 0.0);
        for w in v.iter().flat_map(|s| s.windows(2)) {
            let l = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            all += l;
            if (1.5..8.5).contains(&((w[0][0] + w[1][0]) / 2.0)) {
                mid += l;
            }
        }
        (mid, all)
    };
    let steep = raster_passes(&p, &with(Some(30.0), None)).unwrap();
    // 急な所だけ: 斜面とその両端の丸め(工具の半径ほど)の外の、平らな所の点はない
    // (部品の外周で工具が縁から落ちる所は急なので除いて見る)
    for q in steep
        .iter()
        .flatten()
        .filter(|q| q[1].abs() < 3.0 && q[0] < 18.0)
    {
        assert!((-1.3..11.7).contains(&q[0]), "{q:?}");
    }
    let (m, n) = on_slope(&steep);
    let (m_all, n_all) = on_slope(&raster_passes(&p, &base).unwrap());
    assert!(
        m > 0.85 * m_all && n < 0.6 * n_all,
        "急な所だけ: 斜面 {m} / {m_all}、全部 {n} / {n_all}"
    );
    // なだらかな所だけ: 斜面の上は削らない
    let (m, n) = on_slope(&raster_passes(&p, &with(None, Some(30.0))).unwrap());
    assert!(
        m < 1.0 && n > 0.5 * n_all,
        "なだらかな所だけ: 斜面 {m}、全部 {n} / {n_all}"
    );
}

#[test]
fn one_way_rows_all_go_the_same_way() {
    let p = block();
    let both = raster_passes(&p, &opts(&p)).unwrap();
    assert_eq!(both.len(), 1, "往復は 1 本につながる");
    for (dir, sign) in [(RasterDir::Climb, -1.0), (RasterDir::Conventional, 1.0)] {
        let v = raster_passes(
            &p,
            &Opts {
                direction: dir,
                ..opts(&p)
            },
        )
        .unwrap();
        assert!(v.len() > 10, "行ごとに分かれる: {}", v.len());
        for pass in &v {
            let (a, b) = (pass[0], pass[pass.len() - 1]);
            assert!((b[0] - a[0]) * sign > 0.0, "{dir:?}: {a:?} → {b:?}");
            assert!((b[1] - a[1]).abs() < 1e-9, "行は X の向き");
        }
    }
    // Y の向きのダウンカットは +Y へ削る(行は +X へ進む)
    let v = raster_passes(
        &p,
        &Opts {
            direction: RasterDir::Climb,
            along: Along::Y,
            ..opts(&p)
        },
    )
    .unwrap();
    assert!(v.iter().all(|s| s[s.len() - 1][1] > s[0][1]));
    // 角度を付けても、一方向なら全部の行が同じ向き
    let v = raster_passes(
        &p,
        &Opts {
            direction: RasterDir::Climb,
            angle: Some(30f64.to_radians()),
            ..opts(&p)
        },
    )
    .unwrap();
    let d = [30f64.to_radians().cos(), 30f64.to_radians().sin()];
    assert!(v.iter().all(|s| {
        let (a, b) = (s[0], s[s.len() - 1]);
        (b[0] - a[0]) * d[0] + (b[1] - a[1]) * d[1] < 0.0
    }));
}

/// 床(高さ 0、x・y -15〜15)の上に、中心 (0, 0, 0)・半径 8 の半球。
fn dome() -> Planner {
    let (rad, n, m) = (8.0f64, 64, 16);
    let at = |i: usize, k: usize| {
        let a = std::f64::consts::TAU * i as f64 / n as f64;
        let e = std::f64::consts::FRAC_PI_2 * k as f64 / m as f64;
        [
            rad * e.cos() * a.cos(),
            rad * e.cos() * a.sin(),
            rad * e.sin(),
        ]
    };
    let mut sphere = Vec::new();
    for i in 0..n {
        for k in 0..m {
            sphere.extend(quad(at(i, k), at(i + 1, k), at(i + 1, k + 1), at(i, k + 1)));
        }
    }
    let faces = vec![
        ("floor", top(-15.0, -15.0, 15.0, 15.0, 0.0)),
        ("dome", sphere),
    ];
    prepared(
        &mesh_of(&faces),
        [-15.0, -15.0, 0.0],
        [15.0, 15.0, 9.0],
        ball(),
    )
    .planner()
}

fn circ(p: &Planner, pattern: Pattern) -> CircOpts {
    CircOpts {
        pattern,
        leave: 0.0,
        cell: 0.05,
        floor: p.info.stock_min[2],
        center: Some([0.0, 0.0]),
        inner: 0.0,
        outer: Some(10.0),
        step: 0.5,
        limits: Resolved::default(),
        levels: false,
        direction: RasterDir::Both,
    }
}

/// ボールの先端が半球の上で止まる高さ。
fn on_dome(p: &Planner, q: P3) -> f64 {
    let (big, rb) = (8.0 + p.r(), p.r());
    let rho = q[0].hypot(q[1]);
    if rho < big {
        ((big * big - rho * rho).sqrt() - rb).max(0.0)
    } else {
        0.0
    }
}

fn xy_len(passes: &[Vec<P3>]) -> f64 {
    passes
        .iter()
        .flat_map(|s| s.windows(2))
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum()
}

fn rho(q: &P3) -> f64 {
    q[0].hypot(q[1])
}

/// 先端の高さが半球の上か(削り込まない)。格子は横に 1 マスほど控えめに見るので、急な所ほど
/// 少し浮く(中心へ 0.08 mm 寄せた所の高さまでは許す)。
fn rides(p: &Planner, q: P3) -> bool {
    let want = on_dome(p, q);
    let k = (1.0 - 0.08 / rho(&q).max(0.08)).max(0.0);
    let up = on_dome(p, [q[0] * k, q[1] * k, 0.0]);
    q[2] > want - 0.03 && q[2] < up + 0.03
}

#[test]
fn the_spiral_follows_the_dome_from_outside_in() {
    let p = dome();
    let passes = circular_passes(&p, &circ(&p, Pattern::Spiral)).unwrap();
    assert_eq!(passes.len(), 1, "一続き");
    let s = &passes[0];
    assert!((rho(&s[0]) - 10.0).abs() < 0.05, "外から: {:?}", s[0]);
    assert!(rho(s.last().unwrap()) < 0.05, "中心まで: {:?}", s.last());
    for q in s {
        assert!(rides(&p, *q), "{q:?} は {} のはず", on_dome(&p, *q));
    }
    // 長さは π R² / 間隔 ほど。間隔を倍にすると半分ほど
    let l1 = xy_len(&passes);
    let want = std::f64::consts::PI * 100.0 / 0.5;
    assert!((l1 / want - 1.0).abs() < 0.05, "{l1} と {want}");
    let wide = circular_passes(
        &p,
        &CircOpts {
            step: 1.0,
            ..circ(&p, Pattern::Spiral)
        },
    )
    .unwrap();
    let l2 = xy_len(&wide);
    assert!((l2 / l1 - 0.5).abs() < 0.05, "{l2} / {l1}");
    // 内と外の半径の間だけ
    let ring = circular_passes(
        &p,
        &CircOpts {
            inner: 3.0,
            outer: Some(6.0),
            ..circ(&p, Pattern::Spiral)
        },
    )
    .unwrap();
    for q in ring.iter().flatten() {
        assert!((3.0 - 1e-6..=6.0 + 1e-6).contains(&rho(q)), "{q:?}");
    }
}

#[test]
fn radial_lines_go_out_and_in_at_equal_angles() {
    let tau = std::f64::consts::TAU;
    let lines = radial_xy([1.0, 2.0], 2.0, 10.0, 0.5, 0.05);
    let n = (tau * 10.0 / 0.5).ceil() as usize;
    assert_eq!(lines.len(), n);
    for (k, l) in lines.iter().enumerate() {
        let r = |q: [f64; 2]| (q[0] - 1.0).hypot(q[1] - 2.0);
        let (a, b) = (r(l[0]), r(*l.last().unwrap()));
        if k % 2 == 0 {
            assert!((a - 2.0).abs() < 1e-9 && (b - 10.0).abs() < 1e-9);
        } else {
            assert!((a - 10.0).abs() < 1e-9 && (b - 2.0).abs() < 1e-9);
        }
        // 線の上の点はどれも同じ角度
        let want = tau * k as f64 / n as f64;
        for q in l {
            let d = ((q[1] - 2.0).atan2(q[0] - 1.0) - want).rem_euclid(tau);
            assert!(d.min(tau - d) < 1e-9);
        }
    }
    // 部品の上では面をなぞる
    let p = dome();
    let passes = circular_passes(&p, &circ(&p, Pattern::Radial)).unwrap();
    assert_eq!(passes.len(), 1, "外と内の端を面の上でつないで一続き");
    for q in passes.iter().flatten() {
        assert!(rides(&p, *q), "{q:?} は {}", on_dome(&p, *q));
    }
}

#[test]
fn circular_paths_keep_to_the_boundary_and_slope() {
    let p = dome();
    // x < 0 の半分だけ
    let half = Region::new(&kmb_cam_dropcutter::areas_from_loops(vec![vec![
        [-30.0, -30.0],
        [0.0, -30.0],
        [0.0, 30.0],
        [-30.0, 30.0],
    ]]));
    let o = CircOpts {
        limits: Resolved {
            region: Some(half),
            ..Default::default()
        },
        ..circ(&p, Pattern::Spiral)
    };
    let passes = circular_passes(&p, &o).unwrap();
    assert!(passes.len() > 5, "周ごとに切れる: {}", passes.len());
    // 範囲の外の短い所は面をなぞってつなぐので、中心の近くだけ少しはみ出す
    for q in passes.iter().flatten() {
        assert!(q[0] <= 0.0 || rho(q) < 2.0 * p.tool.diameter, "{q:?}");
    }
    // 30° より緩い所だけ: 半球の縁の近く(急な所)はなぞらない
    let gentle = circular_passes(
        &p,
        &CircOpts {
            outer: Some(7.5),
            limits: Resolved {
                slope: slope_range(None, Some(30f64.to_radians())).unwrap(),
                ..Default::default()
            },
            ..circ(&p, Pattern::Spiral)
        },
    )
    .unwrap();
    let big = 8.0 + p.r();
    let max_rho = gentle.iter().flatten().map(rho).fold(0.0, f64::max);
    // 先端の面の傾きが 30° になるのは中心から (R + r) sin 30°
    assert!(max_rho < big * 0.5 + 0.3, "{max_rho}");
    assert!(max_rho > big * 0.5 - 0.5, "{max_rho}");
}

#[test]
fn circular_documents_do_not_store_the_pattern() {
    let v = serde_json::json!({
        "setup": 1, "tools": 2, "tool": 4, "leave": "0 mm", "resolution": "0.05 mm"
    });
    let c: Circular = serde_json::from_value(v).unwrap();
    assert_eq!(c.pattern, Pattern::Spiral);
    let back = serde_json::to_value(Circular::new(c.base.clone(), Pattern::Radial)).unwrap();
    for k in [
        "pattern",
        "center",
        "inner",
        "outer",
        "step",
        "levels",
        "direction",
    ] {
        assert!(back.get(k).is_none(), "{k}");
    }
}

// 確かめること: 渦巻きと放射も、走査線と同じ欄名と選択肢を共通の宣言から出す。
#[test]
fn circular_level_and_direction_fields_match_the_raster_declaration() {
    let mut reg = kmb_core::Registry::new();
    register(&mut reg);
    let raster = reg.descriptor(RASTER3D).unwrap();
    for kind in [SPIRAL3D, RADIAL3D] {
        let circular = reg.descriptor(kind).unwrap();
        for key in ["levels", "direction"] {
            let expected = raster.fields.iter().find(|f| f.key == key).unwrap();
            let actual = circular.fields.iter().find(|f| f.key == key).unwrap();
            assert_eq!(actual.label, expected.label);
            assert_eq!(actual.kind, expected.kind);
            assert_eq!(actual.unit, expected.unit);
        }
    }
}

fn circular_options(p: &Planner, pattern: Pattern, levels: bool, direction: RasterDir) -> CircOpts {
    let mut v = serde_json::to_value(circ(p, pattern)).unwrap();
    v["levels"] = serde_json::json!(levels);
    v["direction"] = serde_json::to_value(direction).unwrap();
    v["cell"] = serde_json::json!(0.1);
    v["step"] = serde_json::json!(2.0);
    serde_json::from_value(v).unwrap()
}

// 確かめること: 欄を省いた文書と戦略の入力は、段なし・往復の経路をそのまま出す。
#[test]
fn circular_inputs_without_levels_or_direction_keep_the_default_path() {
    let old = serde_json::json!({"setup": 1, "tools": 2, "tool": 4});
    let c: Circular = serde_json::from_value(old.clone()).unwrap();
    let mut explicit = old;
    explicit["levels"] = serde_json::json!(false);
    explicit["direction"] = serde_json::json!("both");
    assert_eq!(c, serde_json::from_value::<Circular>(explicit).unwrap());
    let p = dome();
    for pattern in [Pattern::Spiral, Pattern::Radial] {
        let o = circular_options(&p, pattern, false, RasterDir::Both);
        let expected = circular_passes(&p, &o).unwrap();
        let mut data = serde_json::to_value(&o).unwrap();
        data.as_object_mut().unwrap().remove("levels");
        data.as_object_mut().unwrap().remove("direction");
        let restored: CircOpts = serde_json::from_value(data).unwrap();
        let actual = circular_passes(&p, &restored).unwrap();
        assert_eq!(actual, expected);
        let mut a = p.toolpath();
        let mut b = p.toolpath();
        cut_passes(&p, &mut a, actual);
        cut_passes(&p, &mut b, expected);
        assert_eq!(a, b);
    }
}

// 確かめること: 解いた入力の JSON 往復だけで、段分けと向きを含む同じ戦略の結果を再現する。
#[test]
fn circular_plan_is_reproducible_from_its_serialized_input() {
    let mesh = mesh_of(&[("floor", top(-12.0, -12.0, 12.0, 12.0, 0.0))]);
    let mut prepared = prepared(&mesh, [-12.0, -12.0, 0.0], [12.0, 12.0, 3.0], ball());
    prepared.cutting.stepdown = 1.0;
    let p = prepared.planner();
    for pattern in [Pattern::Spiral, Pattern::Radial] {
        let o = circular_options(&p, pattern, true, RasterDir::Climb);
        let input = prepared.input(kmb_core::FeatureId(1), &o, Default::default());
        let data = serde_json::to_string(&input).unwrap();
        let restored = serde_json::from_str(&data).unwrap();
        let output = circular::plan(&input).unwrap();
        assert_eq!(circular::plan(&restored).unwrap(), output);
        assert!(!output.toolpath.moves.is_empty());
    }
}

// 確かめること: 段高さは下限で、工具は半球に食い込まず、最後の段は面をなぞる。
#[test]
fn circular_levels_stay_above_each_level_and_the_part() {
    let mut p = dome();
    p.cutting.stepdown = 3.0;
    for pattern in [Pattern::Spiral, Pattern::Radial] {
        let o = circular_options(&p, pattern, false, RasterDir::Both);
        let surface = circular_passes(&p, &o).unwrap();
        assert_eq!(surface.len(), 1);
        let lowest = surface[0]
            .iter()
            .map(|q| q[2])
            .fold(f64::INFINITY, f64::min);
        let levels = p.levels(p.info.stock_max[2], lowest);
        assert!(levels.len() > 1);
        let passes =
            circular_passes(&p, &circular_options(&p, pattern, true, RasterDir::Both)).unwrap();
        assert_eq!(passes.len(), levels.len(), "{pattern:?}");
        for (n, (pass, z)) in passes.iter().zip(&levels).enumerate() {
            assert!(pass.iter().all(|q| q[2] >= z - 1e-9));
            assert!(pass.iter().any(|q| (q[2] - z).abs() < 1e-9));
            for q in pass {
                assert!(q[2] >= on_dome(&p, *q) - 0.03, "{pattern:?}: {q:?}");
            }
            let first = if n % 2 == 0 {
                surface[0].first().unwrap()
            } else {
                surface[0].last().unwrap()
            };
            assert_eq!(pass[0], [first[0], first[1], first[2].max(*z)]);
        }
        let mut final_pass = passes.last().unwrap().clone();
        if (levels.len() - 1) % 2 == 1 {
            final_pass.reverse();
        }
        assert_eq!(final_pass, surface[0]);
        let mut tp = p.toolpath();
        cut_passes(&p, &mut tp, passes);
        assert_eq!(
            tp.moves
                .iter()
                .filter(|m| matches!(m, Move::Rapid { .. }))
                .count(),
            3
        );
    }
}

// 確かめること: 主軸が時計回りなら、渦巻きはダウンカットで時計回り、アップカットで反時計回り。
#[test]
fn circular_spiral_directions_turn_opposite_ways_and_keep_going_inward() {
    let p = dome();
    for (dir, sign) in [(RasterDir::Climb, -1.0), (RasterDir::Conventional, 1.0)] {
        let passes =
            circular_passes(&p, &circular_options(&p, Pattern::Spiral, false, dir)).unwrap();
        assert_eq!(passes.len(), 1);
        let pass = &passes[0];
        assert!((rho(&pass[0]) - 10.0).abs() < 1e-9);
        assert!(rho(pass.last().unwrap()) < 1e-9);
        let turn: f64 = pass
            .windows(2)
            .map(|w| w[0][0] * w[1][1] - w[0][1] * w[1][0])
            .sum();
        assert!(turn * sign > 0.0, "{dir:?}: {turn}");
        assert!(pass.iter().all(|q| q[2] >= on_dome(&p, *q) - 0.03));
    }
}

// 確かめること: 一方向の放射は各段の各線が同じ半径方向で、線ごとに上げて移る。
#[test]
fn circular_one_way_radial_lines_retract_and_keep_the_same_direction_on_all_levels() {
    let mut p = dome();
    p.cutting.stepdown = 3.0;
    for (dir, sign) in [(RasterDir::Climb, -1.0), (RasterDir::Conventional, 1.0)] {
        for levels in [false, true] {
            let passes =
                circular_passes(&p, &circular_options(&p, Pattern::Radial, levels, dir)).unwrap();
            let lines = (std::f64::consts::TAU * 10.0 / 2.0).ceil() as usize;
            let n = if levels { p.levels(9.0, 0.0).len() } else { 1 };
            assert_eq!(passes.len(), lines * n);
            for pass in &passes {
                assert!((rho(pass.last().unwrap()) - rho(&pass[0])) * sign > 0.0);
                let end = if sign > 0.0 {
                    pass.last().unwrap()
                } else {
                    &pass[0]
                };
                assert!(
                    pass.iter()
                        .all(|q| (q[0] * end[1] - q[1] * end[0]).abs() < 1e-8)
                );
            }
            let mut tp = p.toolpath();
            let count = passes.len();
            cut_passes(&p, &mut tp, passes);
            let lifts = tp
                .moves
                .windows(2)
                .filter(|w| {
                    w[0].to()[2] < tp.safe_z
                        && matches!(w[1], Move::Rapid { to } if to[2] == tp.safe_z)
                })
                .count();
            assert_eq!(lifts, count, "{dir:?}, 段: {levels}");
        }
    }
}
