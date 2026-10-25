//! 3D 粗取りと平らな所の仕上げの試験。部品を三角形で作り、経路を `kmb-sim` で削って確かめる。

use crate::plan::{self, Settings};
use kmb_cam_dropcutter::{Boundary, Contain, Resolved, limits::region_of};
use kmb_cam_setup::Planner;
use kmb_cam_setup::testing::{mesh_of, prepared};
use kmb_cam_tool::{Tool, ToolKind, ToolLibrary};
use kmb_sim::{SimOptions, SimReport, ZMap, simulate};
use kmb_toolpath::{Move, P3, Toolpath};

pub fn quad(a: P3, b: P3, c: P3, d: P3) -> [[P3; 3]; 2] {
    [[a, b, c], [a, c, d]]
}

/// 上を向いた長方形の面(2 つの三角形)。
pub fn top(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Vec<[P3; 3]> {
    quad([x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]).to_vec()
}

/// 中心 `c`、半径 `r`、底の高さ `z0` の半球(緯度と経度で分けた三角形)。
pub fn hemisphere(c: [f64; 2], r: f64, z0: f64, n: usize) -> Vec<[P3; 3]> {
    let p = |i: usize, j: usize| {
        let lat = std::f64::consts::FRAC_PI_2 * i as f64 / n as f64;
        let lon = std::f64::consts::TAU * j as f64 / (4 * n) as f64;
        [
            c[0] + r * lat.cos() * lon.cos(),
            c[1] + r * lat.cos() * lon.sin(),
            z0 + r * lat.sin(),
        ]
    };
    let mut t = Vec::new();
    for i in 0..n {
        for j in 0..4 * n {
            t.extend(quad(p(i, j), p(i, j + 1), p(i + 1, j + 1), p(i + 1, j)));
        }
    }
    t
}

/// 下を向いた長方形の面(部品の底)。
pub fn bottom(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Vec<[P3; 3]> {
    quad([x0, y0, z], [x0, y1, z], [x1, y1, z], [x1, y0, z]).to_vec()
}

/// 台(40 × 40 × 3、底は高さ 0)の上に、半径 12 の半球と、上面の高さ 8 の段の付いた部品。
pub fn part() -> Vec<(&'static str, Vec<[P3; 3]>)> {
    vec![
        ("bottom", bottom(-20.0, -20.0, 20.0, 20.0, 0.0)),
        ("base", top(-20.0, -20.0, 20.0, 20.0, 3.0)),
        ("dome", hemisphere([-4.0, 3.0], 12.0, 3.0, 24)),
        ("step", top(10.0, -18.0, 18.0, -6.0, 8.0)),
    ]
}

pub fn flat6() -> Tool {
    ToolLibrary::starter().tools[0].clone()
}

pub fn setup(tool: Tool) -> Planner {
    prepared(
        &mesh_of(&part()),
        [-23.0, -23.0, 0.0],
        [23.0, 23.0, 16.0],
        tool,
    )
    .planner()
}

pub fn settings(p: &Planner) -> Settings {
    Settings {
        leave: 0.0,
        limits: Resolved::default(),
        cell: 0.2,
        stepdown: p.cutting.stepdown,
        stepover: p.tool.diameter * p.cutting.stepover,
        ramp: plan::DEFAULT_RAMP_DEG.to_radians(),
        entry: None,
        rest: None,
        bottom: p.info.stock_min[2].max(p.model.min[2]),
        spiral: false,
        smoothing: 0.0,
        entry_kind: kmb_cam_setup::EntryKind::Auto,
        helix: None,
    }
}

/// 部品の高さ(シミュレーションの格子のセルの中心)。部品がない所は負の無限大。
pub fn part_map(stock: &ZMap, tris: &[[P3; 3]]) -> ZMap {
    let mut m = ZMap {
        z: vec![f64::NEG_INFINITY; stock.z.len()],
        ..stock.clone()
    };
    for j in 0..m.ny {
        for i in 0..m.nx {
            let c = m.center(i, j);
            for t in tris {
                if let Some(z) = kmb_cam_geom::z_on_triangle(t, c) {
                    let k = j * m.nx + i;
                    m.z[k] = m.z[k].max(z);
                }
            }
        }
    }
    m
}

/// ストックを経路で削る。
pub fn run(p: &Planner, paths: &[Toolpath]) -> SimReport {
    let stock = ZMap::stock(p.info.stock_min, p.info.stock_max, 0.2);
    let part = part_map(&stock, &p.model.triangles);
    simulate(
        &stock,
        p.info.stock_min[2],
        &part,
        paths,
        SimOptions {
            cell: 0.2,
            ..Default::default()
        },
    )
}

fn named(mut tp: Toolpath, name: &str) -> Toolpath {
    tp.name = name.into();
    tp
}

#[test]
fn roughing_a_dome_and_a_step_leaves_little_and_never_gouges() {
    let p = setup(flat6());
    let s = settings(&p);
    let (tp, report) = plan::rough(&p, &s);
    assert!(kmb_toolpath::check(&tp).is_empty());
    let sim = run(&p, &[named(tp.clone(), "荒削り")]);
    assert!(sim.gouges.is_empty(), "{}", sim.summary());
    assert!(sim.rapid_cuts.is_empty(), "{}", sim.summary());
    // ストック 46 × 46 × 16 = 33856 mm³。残るのは、半球の上の段差(段の深さ 1.44 mm の階段)、
    // 半球と段の間の工具が入らない隅、壁の際の格子の幅ほどの帯だけ(2026-10-16 の測定で 2.2%)
    let frac = sim.remaining_volume / sim.stock_volume;
    assert!(
        frac < 0.03,
        "削り残しがストックの {:.2}%: {}",
        frac * 100.0,
        sim.summary()
    );
    // 外側の開いた所は、ストックの外で下りて入る
    assert!(report.outside > 0, "{report:?}");
    // 平らな面の高さ(3 と 8)にも段を足すので、平らな所に段の厚さが残らない
    let zs = plan::levels(&p, &s);
    assert!(zs.iter().any(|z| (z - 3.0).abs() < 1e-9) && zs.iter().any(|z| (z - 8.0).abs() < 1e-9));
    // 工具を真下に材料へ突っ込まない: 下向きの送りで下りるのは、ストックの外か、削った所の上だけ
    // (閉じた所は輪に沿って斜めに下りる)。ここでは輪が全部つながるので、斜めの下りは数が少ない
    assert!(report.entries < report.loops, "{report:?}");
}

#[test]
fn leaving_stock_keeps_away_from_walls_and_floors() {
    let p = setup(flat6());
    let s = Settings {
        leave: 0.5,
        limits: Resolved {
            leave_radial: 0.5,
            ..Default::default()
        },
        ..settings(&p)
    };
    let (tp, _) = plan::rough(&p, &s);
    let field = kmb_cam_dropcutter::DropField::new(
        kmb_cam_dropcutter::Heights::new(
            [-30.0, -30.0],
            [30.0, 30.0],
            0.1,
            -1.0,
            &p.model.triangles,
        ),
        p.tool.clone(),
    );
    // どの切削の点でも、工具(太らせない)を下ろした高さより 0.5 mm 近く上にある(床)か、
    // 横に 0.5 mm 離れている(壁)。太らせない工具で下ろした高さとの差を見る
    let mut worst = f64::INFINITY;
    for m in &tp.moves {
        if let Move::Linear { to, .. } = m {
            let d = to[2] - field.drop_at([to[0], to[1]]);
            worst = worst.min(d);
        }
    }
    assert!(worst > 0.3, "床からの最小の高さ {worst}");
}

#[test]
fn rest_roughing_only_cuts_where_the_big_tool_could_not_reach() {
    // 前の工具: 平 6 mm。今の工具: 平 2 mm。半球と段の間や台の角の隅だけを削る
    let big = flat6();
    let small = ToolLibrary::starter().tools[2].clone();
    let p_big = setup(big.clone());
    let (tp_big, _) = plan::rough(&p_big, &settings(&p_big));
    let p = setup(small);
    let all = plan::rough(&p, &settings(&p)).0;
    let rest = plan::rough(
        &p,
        &Settings {
            rest: Some(big),
            ..settings(&p)
        },
    )
    .0;
    let len = |tp: &Toolpath| {
        let mut l = 0.0;
        let mut cur = tp.moves[0].to();
        for m in &tp.moves[1..] {
            let to = m.to();
            if !matches!(m, Move::Rapid { .. }) {
                l += (to[0] - cur[0]).hypot(to[1] - cur[1]);
            }
            cur = to;
        }
        l
    };
    // 取残しは全部を削るより、ずっと短い
    assert!(
        len(&rest) < 0.5 * len(&all),
        "{} と {}",
        len(&rest),
        len(&all)
    );
    let both = run(&p, &[named(tp_big.clone(), "太い"), named(rest, "取残し")]);
    assert!(
        both.gouges.is_empty() && both.rapid_cuts.is_empty(),
        "{}",
        both.summary()
    );
    let alone = run(&p_big, &[named(tp_big, "太い")]);
    assert!(
        both.remaining_volume < alone.remaining_volume - 1.0,
        "取残しで削り残しが減る: {} → {}",
        alone.remaining_volume,
        both.remaining_volume
    );
}

#[test]
fn the_boundary_limits_where_the_tool_goes() {
    let p = setup(flat6());
    let s = settings(&p);
    let mut lim = Resolved {
        region: region_of(
            &p,
            Boundary::Faces,
            &["step".into()],
            Contain::Outside,
            0.0,
            0.2,
        )
        .unwrap(),
        ..Default::default()
    };
    let (tp, _) = plan::rough(
        &p,
        &Settings {
            limits: lim.clone(),
            ..s.clone()
        },
    );
    // 段の面(x 10〜18、y -18〜-6)+ 工具の半径の外へは出ない
    for m in &tp.moves {
        let q = m.to();
        if q[2] < p.info.stock_max[2] && !matches!(m, Move::Rapid { .. }) {
            assert!(
                (10.0 - 3.3..=18.0 + 3.3).contains(&q[0])
                    && (-18.0 - 3.3..=-6.0 + 3.3).contains(&q[1]),
                "{q:?}"
            );
        }
    }
    let full = plan::rough(&p, &s).0;
    // 削る長さで比べる(動きの数は輪の頂点の数で変わり、輪の滑らかさで大きく増減する)
    let cut = |t: &kmb_toolpath::Toolpath| {
        let mut c = t.moves[0].to();
        let mut l = 0.0;
        for m in &t.moves[1..] {
            let q = m.to();
            if !matches!(m, Move::Rapid { .. }) {
                l += ((q[0] - c[0]).powi(2) + (q[1] - c[1]).powi(2) + (q[2] - c[2]).powi(2)).sqrt();
            }
            c = q;
        }
        l
    };
    assert!(cut(&tp) < cut(&full) / 3.0, "{} {}", cut(&tp), cut(&full));
    // 範囲を「中心」に変えると、さらに狭い
    lim.region = region_of(
        &p,
        Boundary::Faces,
        &["step".into()],
        Contain::Inside,
        0.0,
        0.2,
    )
    .unwrap();
    let inside = plan::rough(&p, &Settings { limits: lim, ..s }).0;
    assert!(cut(&inside) < cut(&tp), "{} {}", cut(&inside), cut(&tp));
}

#[test]
fn an_entry_point_is_used_in_a_closed_pocket() {
    // 台の中のポケット(上面 3 の台に、深さ 2 の四角い穴)。閉じた所なので、入る位置で下りる
    let tris = vec![
        ("rim1", top(-20.0, -20.0, 20.0, -8.0, 3.0)),
        ("rim2", top(-20.0, 8.0, 20.0, 20.0, 3.0)),
        ("rim3", top(-20.0, -8.0, -8.0, 8.0, 3.0)),
        ("rim4", top(8.0, -8.0, 20.0, 8.0, 3.0)),
        ("floor", top(-8.0, -8.0, 8.0, 8.0, 1.0)),
        ("bottom", bottom(-20.0, -20.0, 20.0, 20.0, 0.0)),
    ];
    let p = prepared(
        &mesh_of(&tris),
        [-20.0, -20.0, 0.0],
        [20.0, 20.0, 3.0],
        flat6(),
    )
    .planner();
    let s = Settings {
        entry: Some([1.0, 1.0]),
        cell: 0.1,
        ..settings(&p)
    };
    let (tp, report) = plan::rough(&p, &s);
    assert!(report.at_entry > 0, "{report:?}");
    let sim = run(&p, &[named(tp, "ポケット")]);
    assert!(
        sim.gouges.is_empty() && sim.rapid_cuts.is_empty(),
        "{}",
        sim.summary()
    );
    // 残るのは四隅の工具の半径の丸み(約 15 mm³)と壁の際の帯だけ。ストック 4800 mm³ の 1.5% 未満
    // (2026-10-16 の測定で 41 mm³ = 0.86%)
    assert!(
        sim.remaining_volume < 0.015 * sim.stock_volume,
        "{}",
        sim.summary()
    );
    // 入る位置がなければ、輪に沿って斜めに下りる(入る位置では下りない)
    let (_, r2) = plan::rough(&p, &settings(&p));
    assert_eq!(r2.at_entry, 0);
}

#[test]
fn flat_finishing_cuts_only_at_the_heights_of_flat_faces() {
    let p = setup(flat6());
    let s = settings(&p);
    let (tp, report) = plan::flats(&p, &s, &[]);
    assert!(report.loops > 0);
    let mut heights: Vec<f64> = tp
        .moves
        .iter()
        .filter(|m| matches!(m, Move::Arc { .. } | Move::Linear { .. }))
        .map(|m| m.to()[2])
        .filter(|z| *z < p.info.stock_max[2])
        .collect();
    heights.sort_by(|a, b| a.total_cmp(b));
    heights.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    // 輪をたどる高さは、台の上面 3 と段の上面 8 だけ(下りる途中の高さを除く)
    let level: Vec<f64> = tp
        .moves
        .windows(2)
        .filter(|w| (w[0].to()[2] - w[1].to()[2]).abs() < 1e-9)
        .map(|w| w[1].to()[2])
        .filter(|z| *z < p.info.stock_max[2])
        .collect();
    assert!(
        level
            .iter()
            .all(|z| (z - 3.0).abs() < 1e-9 || (z - 8.0).abs() < 1e-9),
        "{heights:?}"
    );
    // 選んだ面だけにすると、段の上面だけ
    let (tp, _) = plan::flats(&p, &s, &["step".into()]);
    for w in tp.moves.windows(2) {
        let (a, b) = (w[0].to(), w[1].to());
        if (a[2] - b[2]).abs() < 1e-9 && b[2] < p.info.stock_max[2] {
            assert!((b[2] - 8.0).abs() < 1e-9, "{b:?}");
        }
    }
    // 粗削りの後に仕上げると、平らな面の上の削り残しが減る
    let (rough, _) = plan::rough(
        &p,
        &Settings {
            leave: 0.3,
            ..settings(&p)
        },
    );
    let (flat, _) = plan::flats(&p, &s, &[]);
    let a = run(&p, &[named(rough.clone(), "荒")]);
    let b = run(&p, &[named(rough, "荒"), named(flat, "平")]);
    assert!(
        b.gouges.is_empty() && b.rapid_cuts.is_empty(),
        "{}",
        b.summary()
    );
    assert!(
        b.remaining_volume < a.remaining_volume,
        "{} → {}",
        a.remaining_volume,
        b.remaining_volume
    );
}

#[test]
fn a_ball_end_mill_can_rough_too() {
    let ball = Tool {
        kind: ToolKind::BallEnd,
        ..flat6()
    };
    let p = setup(ball);
    let (tp, _) = plan::rough(&p, &settings(&p));
    let sim = run(&p, &[named(tp, "ボール")]);
    assert!(
        sim.gouges.is_empty() && sim.rapid_cuts.is_empty(),
        "{}",
        sim.summary()
    );
}

/// 台の中の閉じたポケット(上面 3 の台に、深さ 2 の 16 × 16 の四角い穴)。
fn pocket() -> Planner {
    let tris = vec![
        ("rim1", top(-20.0, -20.0, 20.0, -8.0, 3.0)),
        ("rim2", top(-20.0, 8.0, 20.0, 20.0, 3.0)),
        ("rim3", top(-20.0, -8.0, -8.0, 8.0, 3.0)),
        ("rim4", top(8.0, -8.0, 20.0, 8.0, 3.0)),
        ("floor", top(-8.0, -8.0, 8.0, 8.0, 1.0)),
        ("bottom", bottom(-20.0, -20.0, 20.0, 20.0, 0.0)),
    ];
    prepared(
        &mesh_of(&tris),
        [-20.0, -20.0, 0.0],
        [20.0, 20.0, 3.0],
        flat6(),
    )
    .planner()
}

/// ポケットの底(高さ 1)を削る動きの点(台の外を回る輪は除く)。
fn floor_points(tp: &Toolpath) -> Vec<P3> {
    tp.moves
        .iter()
        .filter(|m| !matches!(m, Move::Rapid { .. }))
        .map(|m| m.to())
        .filter(|q| (q[2] - 1.0).abs() < 1e-6 && q[0].abs() < 5.5 && q[1].abs() < 5.5)
        .collect()
}

/// ポケットを削った結果に食い込みが無く、残りが少ないこと(四隅の丸みと壁の際の帯だけ)。
fn pocket_is_cleared(p: &Planner, tp: Toolpath, what: &str) {
    let sim = run(p, &[named(tp, what)]);
    assert!(
        sim.gouges.is_empty() && sim.rapid_cuts.is_empty(),
        "{what}: {}",
        sim.summary()
    );
    assert!(
        sim.remaining_volume < 0.015 * sim.stock_volume,
        "{what}: {}",
        sim.summary()
    );
}

#[test]
fn rings_become_one_spiral_without_a_straight_step_between_them() {
    let p = pocket();
    let base = Settings {
        cell: 0.1,
        ..settings(&p)
    };
    let plain = plan::rough(&p, &base).0;
    let s = Settings {
        spiral: true,
        ..base.clone()
    };
    let (tp, report) = plan::rough(&p, &s);
    // 工具の中心が動ける四角(±5)の縁からの距離。輪から輪へまっすぐ移ると、1 回の動きで送り幅
    // (2.4 mm)ほど変わる。渦巻きは 1 周かけて少しずつ変わる
    let room = |q: &P3| (5.0 - q[0].abs()).min(5.0 - q[1].abs());
    let jump = |tp: &Toolpath| {
        floor_points(tp)
            .windows(2)
            .map(|w| (room(&w[1]) - room(&w[0])).abs())
            .fold(0.0, f64::max)
    };
    println!(
        "縁からの距離の 1 回の変わり: 輪 {:.2} / 渦巻き {:.2}",
        jump(&plain),
        jump(&tp)
    );
    assert!(jump(&plain) > 0.8 * p.tool.diameter * p.cutting.stepover);
    assert!(
        jump(&tp) < 0.3 * p.tool.diameter * p.cutting.stepover,
        "{report:?}"
    );
    pocket_is_cleared(&p, tp, "渦巻き");
}

#[test]
fn rounded_inner_rings_keep_the_outer_ring_sharp_and_still_clear_the_pocket() {
    // 16 × 16 の四角の、工具の中心の領域(±5)を外から送り幅 1.5 で寄せる。角の丸め 2 mm
    let sq = kmb_cam_geom::Area {
        outer: kmb_cam_geom::Loop::from_points(&[
            [-5.0, -5.0],
            [5.0, -5.0],
            [5.0, 5.0],
            [-5.0, 5.0],
        ]),
        holes: vec![],
    };
    let (first, step, r) = (0.05, 1.5, 3.0);
    let corner = |a: &kmb_cam_geom::Area| {
        a.outer
            .flatten(0.001)
            .iter()
            .map(|q| (q[0] - 5.0).hypot(q[1] - 5.0))
            .fold(f64::INFINITY, f64::min)
    };
    let sharp = plan::rings(&sq, first, step, r, 0.0);
    let round = plan::rings(&sq, first, step, r, 2.0);
    // いちばん外の輪は丸めない(角まで first·√2)
    let outer = round.iter().find(|(k, _)| *k == 0).unwrap();
    assert!((corner(&outer.1) - first * 2f64.sqrt()).abs() < 1e-3);
    // 2 つ目の輪(±3.45)は、尖った輪より角から (√2 − 1) × 2 ほど遠い
    let at = |rs: &[(usize, kmb_cam_geom::Area)], k: usize| {
        rs.iter()
            .filter(|(j, _)| *j == k)
            .map(|(_, a)| a.clone())
            .max_by(|a, b| a.area().total_cmp(&b.area()))
    };
    let d = corner(&at(&round, 1).unwrap()) - corner(&at(&sharp, 1).unwrap());
    assert!(d > 0.7 * (2f64.sqrt() - 1.0) * 2.0, "輪 1: {d}");
    // 3 つ目の輪(±1.95)は半径 2 の円が入らない。丸めで消さず(消すと小さな円の輪だけが残る)、
    // 半径を小さくして丸める
    let (a, b) = (at(&round, 2).unwrap(), at(&sharp, 2).unwrap());
    assert!(
        a.area() > 0.8 * b.area(),
        "輪 2: {} / {}",
        a.area(),
        b.area()
    );
    let d = corner(&a) - corner(&b);
    assert!(d > 0.1 && d < 0.7 * (2f64.sqrt() - 1.0) * 2.0, "輪 2: {d}");
    let p = pocket();
    let s = Settings {
        cell: 0.1,
        smoothing: 2.0,
        ..settings(&p)
    };
    pocket_is_cleared(&p, plan::rough(&p, &s).0, "角の丸め");
}

#[test]
fn a_helix_goes_down_at_the_widest_place_when_no_entry_is_set() {
    let p = pocket();
    let s = Settings {
        cell: 0.1,
        entry_kind: kmb_cam_setup::EntryKind::Helix,
        ..settings(&p)
    };
    let (tp, report) = plan::rough(&p, &s);
    assert!(report.at_widest > 0 && report.at_entry == 0, "{report:?}");
    // ポケットの中で下りるのは、四角の真ん中のまわり(半径は工具の半径の半分 = 1.5)を回る
    // らせんだけ(真ん中は格子 0.1 mm の 2 つ分の細かさで探す)。ポケットの外の台の上は
    // ストックの外から横に入る
    let down: Vec<P3> = tp
        .moves
        .windows(2)
        .filter(|w| w[1].to()[2] < w[0].to()[2] - 1e-6 && !matches!(w[1], Move::Rapid { .. }))
        .map(|w| w[1].to())
        .filter(|q| q[2] < 3.0 - 1e-6 && q[0].abs() < 8.0 && q[1].abs() < 8.0)
        .collect();
    assert!(!down.is_empty());
    for q in &down {
        let r = q[0].hypot(q[1]);
        assert!(r < 1.5 + 0.2, "らせんの点 {q:?}");
    }
    pocket_is_cleared(&p, tp, "らせん");
    // 斜めを選べば、進入位置があっても輪に沿って下りる
    let s = Settings {
        entry: Some([1.0, 1.0]),
        entry_kind: kmb_cam_setup::EntryKind::Ramp,
        ..s
    };
    let (_, report) = plan::rough(&p, &s);
    assert!(report.at_entry == 0 && report.at_widest == 0, "{report:?}");
}

#[test]
fn a_bull_nose_can_rough_and_its_stepover_can_be_kept_within_the_flat() {
    let bull = Tool {
        kind: ToolKind::BullNose,
        corner_radius: Some(1.0),
        ..flat6()
    };
    // 平らな所の幅は 6 − 2 = 4 mm。山を許さなければ、送り幅 5 mm は 4 mm に抑える
    assert_eq!(crate::stepover_for(&bull, 5.0, true).unwrap(), 5.0);
    assert_eq!(crate::stepover_for(&bull, 5.0, false).unwrap(), 4.0);
    assert_eq!(crate::stepover_for(&bull, 3.0, false).unwrap(), 3.0);
    assert_eq!(crate::stepover_for(&flat6(), 5.0, false).unwrap(), 5.0);
    let round = Tool {
        corner_radius: Some(2.95),
        ..bull.clone()
    };
    assert!(crate::stepover_for(&round, 5.0, false).is_err());
    let p = setup(bull);
    let (tp, _) = plan::rough(&p, &settings(&p));
    let sim = run(&p, &[named(tp, "ブルノーズ")]);
    assert!(
        sim.gouges.is_empty() && sim.rapid_cuts.is_empty(),
        "{}",
        sim.summary()
    );
}
