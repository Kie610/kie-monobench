use super::*;
use jc_cam_tool::{ToolKind, ToolLibrary};

fn slot(z: f64) -> Toolpath {
    Toolpath {
        name: "溝".into(),
        tool: ToolLibrary::starter().tools[0].clone(),
        rpm: 10000.0,
        safe_z: 5.0,
        retract_z: 1.0,
        edge_break: 0.0,
        dwells: vec![],
        spans: vec![],
        links: Default::default(),
        moves: vec![
            Move::Rapid {
                to: [-5.0, 10.0, 5.0],
            },
            Move::Rapid {
                to: [-5.0, 10.0, 1.0],
            },
            Move::Linear {
                to: [-5.0, 10.0, z],
                feed: 200.0,
            },
            Move::Linear {
                to: [25.0, 10.0, z],
                feed: 600.0,
            },
            Move::Rapid {
                to: [25.0, 10.0, 5.0],
            },
        ],
    }
}

#[test]
fn a_slot_removes_the_expected_volume() {
    let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.1);
    let part = ZMap {
        z: vec![f64::NEG_INFINITY; stock.z.len()],
        ..stock.clone()
    };
    let r = simulate(&stock, -10.0, &part, &[slot(-2.0)], SimOptions::default());
    // 幅 6 mm、長さ 20 mm、深さ 2 mm の溝
    assert!(
        (r.removed_volume - 240.0).abs() < 3.0,
        "{}",
        r.removed_volume
    );
    assert!(r.is_ok(), "{}", r.summary());
    assert!((r.stock_volume - 4000.0).abs() < 1e-6);
}

#[test]
fn gouges_and_rapid_cuts_are_found() {
    let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.1);
    // 部品の上面が -1 なのに -2 まで削る
    let part = ZMap {
        z: vec![-1.0; stock.z.len()],
        ..stock.clone()
    };
    let r = simulate(&stock, -10.0, &part, &[slot(-2.0)], SimOptions::default());
    assert!(!r.gouges.is_empty());
    assert!((r.gouges[0].amount - 1.0).abs() < 1e-6);
    let mut bad = slot(-2.0);
    bad.moves[3] = Move::Rapid {
        to: [25.0, 10.0, -2.0],
    };
    let r = simulate(&stock, -10.0, &part, &[bad], SimOptions::default());
    assert!(!r.rapid_cuts.is_empty());
}

#[test]
fn a_full_slot_engages_half_the_tool_and_a_side_cut_less() {
    let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.1);
    let part = ZMap {
        z: vec![f64::NEG_INFINITY; stock.z.len()],
        ..stock.clone()
    };
    let r = simulate(&stock, -10.0, &part, &[slot(-2.0)], SimOptions::default());
    let e = &r.engagement[0];
    // 溝は工具の前半分がすべて材料に触れる(入りと抜けの途中は少ない)
    assert!((e.max - 180.0).abs() < 1e-9, "{e:?}");
    assert!(e.slot_length > 15.0, "{e:?}");
    // 同じ溝の横を、工具の半径の半分だけずらしてもう一度削る(横の切り込み 25 %)
    let mut side = slot(-2.0);
    side.name = "横".into();
    let r_tool = side.tool.radius();
    for m in &mut side.moves {
        let to = match m {
            Move::Rapid { to } | Move::Linear { to, .. } => to,
            _ => continue,
        };
        to[1] += r_tool / 2.0;
    }
    let r = simulate(
        &stock,
        -10.0,
        &part,
        &[slot(-2.0), side],
        SimOptions::default(),
    );
    let e = r.engagement.iter().find(|e| e.op == "横").unwrap();
    // 横の切り込み ae = D/4 の係合角は arccos(1 - 2 ae / D) = 60°
    assert!((e.mean - 60.0).abs() < 12.0, "{e:?}");
    assert!(e.slot_length < 1.0, "{e:?}");
    assert!(r.summary().contains("工具のかかり具合"), "{}", r.summary());
}

#[test]
fn a_deep_slot_beside_a_tall_wall_hits_the_shank_and_the_holder() {
    // 高さ 30 mm のストックに、深さ 25 mm の溝。刃長は 20 mm なので、刃の上端より上の
    // 材料にシャンクが当たる
    let stock = ZMap::stock([0.0, 0.0, -30.0], [20.0, 20.0, 0.0], 0.1);
    let part = ZMap {
        z: vec![f64::NEG_INFINITY; stock.z.len()],
        ..stock.clone()
    };
    let mut deep = slot(-25.0);
    deep.tool.flute_length = 20.0;
    let r = simulate(&stock, -30.0, &part, &[deep.clone()], SimOptions::default());
    assert!(
        r.collisions.iter().any(|c| c.part == ToolPart::Shank),
        "{}",
        r.summary()
    );
    assert!(!r.is_ok());
    assert!(r.summary().contains("シャンクの当たり"), "{}", r.summary());
    // 浅い溝は当たらない。突き出し 22 mm・ホルダー 16 mm なら、深さ 25 mm ではホルダーも当たる
    let r = simulate(&stock, -30.0, &part, &[slot(-2.0)], SimOptions::default());
    assert!(r.collisions.is_empty(), "{}", r.summary());
    deep.tool.shank_diameter = Some(6.0);
    deep.tool.stickout = Some(22.0);
    deep.tool.holder_diameter = Some(16.0);
    let r = simulate(&stock, -30.0, &part, &[deep], SimOptions::default());
    assert!(
        r.collisions.iter().any(|c| c.part == ToolPart::Holder),
        "{}",
        r.summary()
    );
    // 続く当たりは始めの 1 件だけ
    assert!(r.collisions.len() <= 6, "{:?}", r.collisions);
}

#[test]
fn a_holder_passing_over_a_clamp_is_found() {
    let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.1);
    let part = ZMap {
        z: vec![f64::NEG_INFINITY; stock.z.len()],
        ..stock.clone()
    };
    // 溝の脇(y 3〜7 の外、y = 15〜25)に高さ 12 mm のクランプ。工具は y = 10 を通る
    let clamp = ([5.0, 15.0, -10.0], [15.0, 25.0, 2.0]);
    let mut wide = slot(-2.0);
    wide.tool.stickout = Some(15.0);
    wide.tool.holder_diameter = Some(16.0);
    let r = simulate_with(
        &stock,
        -10.0,
        &part,
        &[wide.clone()],
        SimOptions::default(),
        &[clamp],
    );
    // ホルダーの下端は -2 + 15 = 13 でクランプ(上端 2)より上。当たらない
    assert!(r.fixture_hits.is_empty(), "{}", r.summary());
    // 刃と突き出しが短いとホルダーの下端が 1 になり、半径 8 mm がクランプ(5 mm 先)に入る
    wide.tool.flute_length = 2.5;
    wide.tool.stickout = Some(3.0);
    let r = simulate_with(
        &stock,
        -10.0,
        &part,
        &[wide],
        SimOptions::default(),
        &[clamp],
    );
    assert!(!r.fixture_hits.is_empty(), "{}", r.summary());
    assert_eq!(r.fixture_hits[0].part, ToolPart::Holder);
    // 入り始めは箱の角に浅く掛かる(真横を通る所の深さ 3 mm まで)
    let a = r.fixture_hits[0].event.amount;
    assert!(a > 0.0 && a <= 3.0 + 1e-9, "{a}");
    assert!(!r.is_ok() && r.summary().contains("固定具にホルダーが当たる"));
    // 続く当たりは始めの 1 件だけ
    assert_eq!(r.fixture_hits.len(), 1, "{:?}", r.fixture_hits);
}

#[test]
fn the_machine_shape_finds_travel_head_bed_and_clearance() {
    let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.1);
    let part = ZMap {
        z: vec![f64::NEG_INFINITY; stock.z.len()],
        ..stock.clone()
    };
    let mut tp = slot(-2.0);
    tp.tool.flute_length = 10.0;
    tp.tool.stickout = Some(15.0);
    // 主軸の鼻は先端の 15 + 30 = 45 上
    assert_eq!(nose_height(&tp.tool), 45.0);
    let wide = MachineShape {
        travel: ([-100.0, -100.0], [100.0, 100.0]),
        nose: [0.0, 300.0],
        ..Default::default()
    };
    let run = |stock: &ZMap, shape: &MachineShape, fixtures: &[(P3, P3)]| {
        simulate_on_machine(
            stock,
            -10.0,
            &|_| part.clone(),
            std::slice::from_ref(&tp),
            &[0.1],
            SimOptions::default(),
            fixtures,
            Some(shape),
        )
    };
    let r = run(&stock, &wide, &[]);
    assert!(r.machine_hits.is_empty() && r.is_ok(), "{}", r.summary());

    // X の作業範囲 0〜20 の外へ出る(溝は -5〜25)。両側とも続く所の始めの 1 件だけ
    let narrow = MachineShape {
        travel: ([0.0, -100.0], [20.0, 100.0]),
        ..wide.clone()
    };
    let r = run(&stock, &narrow, &[]);
    let kinds: Vec<_> = r.machine_hits.iter().map(|h| h.kind).collect();
    assert_eq!(
        kinds,
        vec![
            MachineKind::Travel {
                axis: 0,
                high: false
            },
            MachineKind::Travel {
                axis: 0,
                high: true
            }
        ],
        "{}",
        r.summary()
    );
    assert!(r.summary().contains("X の作業範囲を+側へはみ出す"));
    assert!(!r.is_ok());

    // 主軸の鼻は -2 + 45 = 43 まで下がる。下の限りが 44 なら届かない
    let low = MachineShape {
        nose: [44.0, 300.0],
        ..wide.clone()
    };
    let r = run(&stock, &low, &[]);
    let h = &r.machine_hits[0];
    assert_eq!(
        h.kind,
        MachineKind::Travel {
            axis: 2,
            high: false
        }
    );
    assert!((h.event.amount - 1.0).abs() < 1e-6, "{}", h.event.amount);

    // 溝の脇(y 15〜18)の高さ 60 の柱に、径 52 の主軸が当たる。シャンクは届かない
    let mut tall = stock.clone();
    for j in 150..180 {
        for i in 0..tall.nx {
            tall.z[j * tall.nx + i] = 60.0;
        }
    }
    let head = MachineShape {
        spindle: [52.0, 50.0],
        ..wide.clone()
    };
    let r = run(&tall, &head, &[]);
    assert!(r.collisions.is_empty(), "{:?}", r.collisions);
    let h = r
        .machine_hits
        .iter()
        .find(|h| h.kind == MachineKind::HeadStock(HeadPart::Spindle))
        .expect("主軸が柱に当たる");
    // 柱の上 60 − 主軸の鼻(下りる途中から)
    assert!(
        (h.event.amount - (60.0 - 43.0)).abs() < 1e-6,
        "{}",
        h.event.amount
    );
    assert!(r.summary().contains("主軸が材料に当たる"));

    // 主軸の上の Z の台(幅 100・奥行き 80、主軸の鼻から 50 上の 95 から)が背の高いクランプに当たる
    let clamp = ([5.0, 40.0, -10.0], [15.0, 45.0, 120.0]);
    let carriage = MachineShape {
        spindle: [52.0, 50.0],
        carriage: [100.0, 80.0, 60.0],
        ..wide.clone()
    };
    let r = run(&stock, &carriage, &[clamp]);
    assert!(
        r.machine_hits
            .iter()
            .any(|h| h.kind == MachineKind::HeadFixture(HeadPart::Carriage)),
        "{}",
        r.summary()
    );
    assert!(
        r.fixture_hits.is_empty(),
        "刃とホルダーはクランプに届かない"
    );

    // ベッドの上面が -1 なら、-2 の溝は 1 mm 食い込む。削ってよい深さが 1.5 mm なら当たりにせず、
    // 削った深さだけを知らせる
    let mut bed = MachineShape {
        bed: Some(([-10.0, -10.0, -16.0], [30.0, 30.0, -1.0])),
        bed_allowance: 1.5,
        ..wide.clone()
    };
    let r = run(&stock, &bed, &[]);
    assert!(r.is_ok(), "{}", r.summary());
    assert!((r.bed_depth - 1.0).abs() < 1e-6);
    assert!(r.summary().contains("ベッドを最大 1.00 mm 削る"));
    bed.bed_allowance = 0.5;
    let r = run(&stock, &bed, &[]);
    let h = &r.machine_hits[0];
    assert_eq!((h.kind, r.machine_hits.len()), (MachineKind::Bed, 1));
    assert!((h.event.amount - 1.0).abs() < 1e-6);
    assert!(!r.summary().contains("削ってよい深さの内"));

    // 門の下の高さ -0.5 は、上面 0 のストックが通らない(経路によらず 1 件)
    let gate = MachineShape {
        clearance: Some(-0.5),
        ..wide
    };
    let r = run(&stock, &gate, &[]);
    assert_eq!(r.machine_hits.len(), 1);
    assert_eq!(r.machine_hits[0].kind, MachineKind::Clearance);
    assert_eq!(r.machine_hits[0].event.op, "ストック");
    assert!((r.machine_hits[0].event.amount - 0.5).abs() < 1e-6);
}

#[test]
fn a_bull_nose_leaves_its_corner_radius_in_the_slot() {
    let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.1);
    let part = ZMap {
        z: vec![f64::NEG_INFINITY; stock.z.len()],
        ..stock.clone()
    };
    let mut op = slot(-2.0);
    op.tool.kind = ToolKind::BullNose;
    op.tool.corner_radius = Some(1.0);
    let r = simulate(&stock, -10.0, &part, &[op], SimOptions::default());
    let m = &r.result;
    let z_at = |y: f64| {
        let j = ((y - m.origin[1]) / m.cell).floor() as usize;
        let i = ((10.0 - m.origin[0]) / m.cell).floor() as usize;
        m.z[j * m.nx + i]
    };
    // 溝の中ほどは平ら、径 6 の端に近い所は角 R の分だけ高い
    assert!((z_at(10.05) + 2.0).abs() < 1e-9);
    assert!((z_at(11.95) + 2.0).abs() < 1e-9);
    let d: f64 = 2.55;
    let want = -2.0 + 1.0 - (1.0 - (d - 2.0) * (d - 2.0)).sqrt();
    assert!((z_at(10.0 + d) - want).abs() < 1e-6, "{}", z_at(10.0 + d));
}

#[test]
fn a_long_neck_passes_through_a_slot_the_flutes_have_cut() {
    // 深さ 12 mm の溝を削ってから、同じ所を深さ 25 mm まで削る。刃長 20 mm、シャンク 8 mm。
    // 首下長がなければ刃のすぐ上からシャンクが太くなり、溝の壁に当たる
    let stock = ZMap::stock([0.0, 0.0, -30.0], [20.0, 20.0, 0.0], 0.1);
    let part = ZMap {
        z: vec![f64::NEG_INFINITY; stock.z.len()],
        ..stock.clone()
    };
    let mut first = slot(-12.0);
    first.tool.flute_length = 20.0;
    first.tool.shank_diameter = Some(8.0);
    let mut second = slot(-25.0);
    second.name = "深い溝".into();
    second.tool = first.tool.clone();
    let ops = [first.clone(), second.clone()];
    let r = simulate(&stock, -30.0, &part, &ops, SimOptions::default());
    assert!(
        r.collisions.iter().any(|c| c.part == ToolPart::Shank),
        "{}",
        r.summary()
    );
    // 首下長 28 mm(刃の径の首)なら、首は削った溝の中を通り、シャンクは材料より上
    for op in [&mut first, &mut second] {
        op.tool.neck_length = Some(28.0);
    }
    let r = simulate(
        &stock,
        -30.0,
        &part,
        &[first, second],
        SimOptions::default(),
    );
    assert!(r.collisions.is_empty(), "{}", r.summary());
}

/// `x` の高さ `z` で、Y の向きに端から端まで削る経路。
fn along_y(name: &str, x: f64, z: f64) -> Toolpath {
    let mut tp = slot(z);
    tp.name = name.into();
    tp.moves = vec![
        Move::Rapid { to: [x, -5.0, 5.0] },
        Move::Rapid { to: [x, -5.0, 1.0] },
        Move::Linear {
            to: [x, -5.0, z],
            feed: 200.0,
        },
        Move::Linear {
            to: [x, 25.0, z],
            feed: 600.0,
        },
        Move::Rapid { to: [x, 25.0, 5.0] },
    ];
    tp
}

#[test]
fn a_coarse_roughing_grid_moves_to_the_fine_grid_without_false_gouges() {
    // 部品は x ≥ 9.6 の高さ 0 の壁、ストックの上面は 1。荒削りは 1 mm の格子で壁の 0.05 mm
    // 手前まで溝を掘り、仕上げは 0.1 mm の格子で壁の溝の側から部品の上面を削る
    let r = ToolLibrary::starter().tools[0].radius();
    let part_at = |g: &ZMap| ZMap {
        z: (0..g.z.len())
            .map(|k| {
                let x = g.center(k % g.nx, k / g.nx)[0];
                if x >= 9.6 { 0.0 } else { f64::NEG_INFINITY }
            })
            .collect(),
        ..g.clone()
    };
    let paths = [
        along_y("荒削り", 9.55 - r, -2.0),
        along_y("上面", 9.7 + r, 0.0),
    ];
    let stock = ZMap::stock([0.0, 0.0, -5.0], [20.0, 20.0, 1.0], 1.0);
    let opt = SimOptions {
        stage_cells: 20,
        ..SimOptions::default()
    };
    let mixed = simulate_grids(&stock, -5.0, &part_at, &paths, &[1.0, 0.1], opt, &[]);
    // 粗いセル [9, 10] の中心 9.5 は削れ、細かくすると 9.6〜10 が部品より低くなるが、
    // 仕上げの経路はそこを食い込みに数えない
    assert!(mixed.gouges.is_empty(), "{}", mixed.summary());
    // 結果は最後の経路の格子で、途中の形は経路ごと
    assert_eq!((mixed.result.cell, mixed.result.nx), (0.1, 200));
    assert_eq!(mixed.stages.len(), 3);
    // 同じ経路を全部 0.1 mm で削った時と、削った体積は格子の粗さの分しか違わない
    let fine = ZMap::stock([0.0, 0.0, -5.0], [20.0, 20.0, 1.0], 0.1);
    let uniform = simulate_with(&fine, -5.0, &part_at(&fine), &paths, opt, &[]);
    assert!(uniform.gouges.is_empty(), "{}", uniform.summary());
    let (a, b) = (mixed.removed_volume, uniform.removed_volume);
    assert!((a - b).abs() < 0.1 * b, "{a} {b}");
    // 本当に壁へ食い込んだ時は、粗い格子のままでも数える
    let deep = [along_y("食い込み", 12.0 - r, -2.0)];
    let g = simulate_grids(&stock, -5.0, &part_at, &deep, &[1.0], opt, &[]);
    assert!(!g.gouges.is_empty());
}

#[test]
fn resampling_takes_the_height_of_the_cell_under_each_new_centre() {
    let mut m = ZMap::stock([0.0, 0.0, -1.0], [4.0, 2.0, 0.0], 1.0);
    m.z[1] = -1.0; // (1, 0): x 1〜2
    let fine = m.resampled(0.5);
    assert_eq!((fine.nx, fine.ny, fine.cell), (8, 4, 0.5));
    assert_eq!(&fine.z[..8], &[0.0, 0.0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0]);
    let back = fine.resampled(1.0);
    assert_eq!(back, m);
}

#[test]
fn each_stage_is_kept_coarse_and_becomes_a_mesh() {
    let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.1);
    let part = ZMap {
        z: vec![f64::NEG_INFINITY; stock.z.len()],
        ..stock.clone()
    };
    let opt = SimOptions {
        stage_cells: 50,
        ..SimOptions::default()
    };
    let mut second = slot(-4.0);
    second.name = "深い溝".into();
    let r = simulate(&stock, -10.0, &part, &[slot(-2.0), second], opt);
    let names: Vec<&str> = r.stages.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(names, ["素材", "溝", "深い溝"]);
    let (_, after) = &r.stages[1];
    assert_eq!((after.nx, after.ny), (50, 50));
    // 溝の真ん中(y = 10)の粗いセルは、溝の深さ
    let j = (10.0 / after.cell) as usize;
    assert!((after.z[j * after.nx + 25] + 2.0).abs() < 1e-9);
    assert!((r.stages[2].1.z[j * after.nx + 25] + 4.0).abs() < 1e-9);
    assert_eq!(
        r.stages[0].1.z.iter().cloned().fold(f64::MIN, f64::max),
        0.0
    );
    // 上の面の三角形は上から見て反時計回り(画面の裏の面を描かない処理で消えない向き)
    let m = after.to_mesh(-10.0);
    assert_eq!(m.triangles.len(), m.triangle_faces.len());
    let up = m.triangles[..2 * 49 * 49].iter().all(|t| {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0
    });
    assert!(up);
    let zs: Vec<f32> = m.positions.iter().map(|p| p[2]).collect();
    assert!(zs.iter().all(|z| (-10.0..=0.0).contains(z)));
    // 既定では段ごとの形を取っておかない
    assert_eq!(SimOptions::default().stage_cells, 0);
}

#[test]
fn leftovers_are_marked_once_per_square_millimetre_with_the_largest_first() {
    // 0.5 mm の格子の 4 × 2 mm。部品の高さ 0、許し 0.05 mm
    let part = ZMap::stock([0.0, 0.0, 0.0], [4.0, 2.0, 0.0], 0.5);
    let mut map = part.clone();
    map.z[0] = 0.3; // (0, 0) の区画
    map.z[1] = 0.2; // 同じ区画で低い方
    map.z[8 + 6] = 1.0; // (3, 0) の区画、j = 1, i = 6
    map.z[3] = 0.04; // 許しより低い
    let base = ZMap {
        z: part.z.iter().map(|z| z + 0.05).collect(),
        ..part.clone()
    };
    let m = leftover_marks(&map, &base, 0.05);
    assert_eq!(m.len(), 2, "{m:?}");
    assert!((m[0].1 - 1.0).abs() < 1e-9 && (m[0].0[2] - 1.0).abs() < 1e-9);
    assert_eq!([m[0].0[0], m[0].0[1]], [3.25, 0.75]);
    assert!((m[1].1 - 0.3).abs() < 1e-9);
    assert_eq!([m[1].0[0], m[1].0[1]], [0.25, 0.25]);
}

#[test]
fn skipping_cells_gives_the_same_report_as_looking_at_every_cell() {
    let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.1);
    // 部品はでこぼこ(所々で削りすぎる高さ)
    let part_at = |m: &ZMap| {
        let mut p = m.clone();
        for j in 0..m.ny {
            for i in 0..m.nx {
                let c = m.center(i, j);
                p.z[j * m.nx + i] = -2.5 + (c[0] * 0.7).sin() + (c[1] * 0.45).cos() * 0.8;
            }
        }
        p
    };
    let lib = ToolLibrary::starter();
    let mut paths = Vec::new();
    for (n, (tool, z, dy)) in [
        (0, -1.0, 0.0),
        (0, -2.0, 2.5),
        (1, -2.6, 5.0),
        (0, -3.0, -3.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut tp = slot(z);
        tp.name = format!("経路 {n}");
        tp.tool = lib.tools[tool.min(lib.tools.len() - 1)].clone();
        tp.edge_break = if n == 2 { 0.3 } else { 0.0 };
        for m in &mut tp.moves {
            if let Move::Rapid { to } | Move::Linear { to, .. } = m {
                to[1] += dy;
            }
        }
        // 斜めに戻る(早送りで削る所もある)
        tp.moves.push(Move::Linear {
            to: [25.0, 2.0 + dy, z + 0.5],
            feed: 600.0,
        });
        tp.moves.push(Move::Rapid {
            to: [-5.0, 18.0, z + 0.2],
        });
        paths.push(tp);
    }
    let cells = [0.2, 0.2, 0.1, 0.1];
    let opt = SimOptions {
        stage_cells: 50,
        ..SimOptions::default()
    };
    let fast = run(
        &stock,
        -10.0,
        &part_at,
        &paths,
        &cells,
        opt,
        &[],
        None,
        false,
    );
    let all = run(
        &stock,
        -10.0,
        &part_at,
        &paths,
        &cells,
        opt,
        &[],
        None,
        true,
    );
    assert!(!all.gouges.is_empty() && !all.rapid_cuts.is_empty());
    assert_eq!(fast, all);
}
