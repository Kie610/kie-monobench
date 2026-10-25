//! 等高線の試験。部品を三角形で作り、輪と経路を確かめる。

use super::*;
use kmb_cam_dropcutter::limits::{region_of, slope_range};
use kmb_cam_dropcutter::{Boundary, Contain};
use kmb_cam_setup::testing::{mesh_of, prepared};
use kmb_cam_tool::{Tool, ToolLibrary};
use kmb_toolpath::P3;

fn quad(a: P3, b: P3, c: P3, d: P3) -> Vec<[P3; 3]> {
    vec![[a, b, c], [a, c, d]]
}

fn top(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Vec<[P3; 3]> {
    quad([x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z])
}

fn ball() -> Tool {
    ToolLibrary::starter().tools[3].clone()
}

/// 床(高さ 0)の上に、箱 A(x 0〜10、y -5〜5、高さ 6)と、x 14〜24 で 0 から 5 へ上る
/// なだらかな斜面(約 27°)の付いた台。
fn part() -> Planner {
    let faces = vec![
        ("floor", top(-10.0, -10.0, 34.0, 10.0, 0.0)),
        ("a", top(0.0, -5.0, 10.0, 5.0, 6.0)),
        (
            "ramp",
            quad(
                [14.0, -5.0, 0.0],
                [24.0, -5.0, 5.0],
                [24.0, 5.0, 5.0],
                [14.0, 5.0, 0.0],
            ),
        ),
        ("deck", top(24.0, -5.0, 30.0, 5.0, 5.0)),
    ];
    prepared(
        &mesh_of(&faces),
        [-10.0, -10.0, 0.0],
        [34.0, 10.0, 7.0],
        ball(),
    )
    .planner()
}

fn opts(p: &Planner) -> Opts {
    Opts {
        leave: 0.0,
        cell: 0.1,
        step: 1.0,
        bottom: 0.0,
        top: p.model.max[2],
        ramp: DEFAULT_RAMP_DEG.to_radians(),
        order: Order::TopDown,
        limits: Resolved::default(),
        entry: None,
    }
}

/// 箱 A の周りの輪の点での、箱までの横の距離 − 工具の半径(壁に残る厚さ)の最小。
fn gap_a(p: &Planner, lv: &Levels) -> f64 {
    lv.rings
        .iter()
        .filter(|r| r.z > 1.0 && r.z < 5.0)
        .flat_map(|r| r.pts.iter())
        .filter(|q| q[0] < 12.0)
        .map(|q| {
            let dx = (0.0 - q[0]).max(q[0] - 10.0).max(0.0);
            let dy = (-5.0 - q[1]).max(q[1] - 5.0).max(0.0);
            dx.hypot(dy) - p.r()
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn radial_leave_keeps_stock_on_steep_walls() {
    let p = part();
    let z_only = levels(
        &p,
        &Opts {
            leave: 0.5,
            ..opts(&p)
        },
    )
    .unwrap();
    let both = levels(
        &p,
        &Opts {
            leave: 0.5,
            limits: Resolved {
                leave_radial: 0.5,
                ..Default::default()
            },
            ..opts(&p)
        },
    )
    .unwrap();
    let (a, b) = (gap_a(&p, &z_only), gap_a(&p, &both));
    // Z に足すだけでは縦の壁に厚さが残らない(格子の幅ほど)。径方向に 0.5 mm 残すと 0.5 mm 離れる
    assert!(a < 0.15, "Z だけ: {a}");
    assert!((0.45..0.7).contains(&b), "径方向も: {b}");
}

#[test]
fn bottom_up_climbs_the_levels() {
    let p = part();
    for order in [Order::TopDown, Order::BottomUp] {
        let o = Opts { order, ..opts(&p) };
        let lv = levels(&p, &o).unwrap();
        let zs: Vec<f64> = lv.rings.iter().map(|r| r.z).collect();
        let sorted = zs.windows(2).all(|w| match order {
            Order::TopDown => w[1] <= w[0],
            Order::BottomUp => w[1] >= w[0],
        });
        assert!(sorted, "{order:?}: {zs:?}");
        let mut tp = Toolpath {
            name: "t".into(),
            tool: p.tool.clone(),
            rpm: 10000.0,
            safe_z: p.info.safe_z,
            retract_z: p.info.retract_z,
            edge_break: 0.0,
            moves: vec![],
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
        };
        moves(&p, &lv, &o, &mut tp);
        assert!(kmb_toolpath::check(&tp).is_empty());
        // 下から上なら、切削の高さは下がらない(輪に沿って下りる所を除けば上がるだけ)
        if order == Order::BottomUp {
            let cut: Vec<f64> = tp
                .moves
                .iter()
                .filter(|m| !matches!(m, Move::Rapid { .. }))
                .map(|m| m.to()[2])
                .filter(|z| *z < p.info.stock_max[2])
                .collect();
            let first = cut[0];
            assert!(first < 1.5, "いちばん下から始める: {first}");
        }
    }
}

#[test]
fn the_boundary_and_slope_limit_the_rings() {
    let p = part();
    let all = levels(&p, &opts(&p)).unwrap();
    let near = |lv: &Levels, x0: f64, x1: f64| {
        lv.rings
            .iter()
            .flat_map(|r| r.pts.iter())
            .filter(|q| (x0..x1).contains(&q[0]))
            .count()
    };
    assert!(near(&all, -5.0, 12.0) > 0 && near(&all, 12.0, 40.0) > 0);
    // 範囲を箱 A の上面 + 工具の半径 + 0.5 mm にすると、箱 A の周りだけ
    let a = levels(
        &p,
        &Opts {
            limits: Resolved {
                region: region_of(
                    &p,
                    Boundary::Faces,
                    &["a".into()],
                    Contain::Outside,
                    0.5,
                    0.1,
                )
                .unwrap(),
                ..Default::default()
            },
            ..opts(&p)
        },
    )
    .unwrap();
    assert!(near(&a, -5.0, 12.0) > 0 && near(&a, 12.0, 40.0) == 0);
    // 急な所だけ(60° より急)にすると、なだらかな斜面(約 27°)の輪はなくなる
    let steep = levels(
        &p,
        &Opts {
            limits: Resolved {
                slope: slope_range(Some(60f64.to_radians()), None).unwrap(),
                ..Default::default()
            },
            ..opts(&p)
        },
    )
    .unwrap();
    // 斜面の上を横切る輪の長さ(輪の点は一直線に並ぶと間引くので、区間の中点で見る)
    let on_ramp = |lv: &Levels| {
        let mut len = 0.0;
        for r in &lv.rings {
            let n = r.pts.len();
            let m = if r.closed { n } else { n - 1 };
            for k in 0..m {
                let (a, b) = (r.pts[k], r.pts[(k + 1) % n]);
                let c = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                if (15.5..22.5).contains(&c[0]) && c[1].abs() < 3.0 {
                    len += (b[0] - a[0]).hypot(b[1] - a[1]);
                }
            }
        }
        len
    };
    assert!(
        on_ramp(&all) > 10.0,
        "限らなければ斜面も回る: {}",
        on_ramp(&all)
    );
    assert!(
        on_ramp(&steep) < 1e-9,
        "急な所だけなら斜面は回らない: {}",
        on_ramp(&steep)
    );
    assert!(near(&steep, -5.0, 12.0) > 0, "箱の壁は回る");
    // 開いた線になった所も、経路は正しい
    let o = Opts {
        limits: Resolved {
            slope: slope_range(Some(60f64.to_radians()), None).unwrap(),
            ..Default::default()
        },
        ..opts(&p)
    };
    let mut tp = Toolpath {
        name: "t".into(),
        tool: p.tool.clone(),
        rpm: 10000.0,
        safe_z: p.info.safe_z,
        retract_z: p.info.retract_z,
        edge_break: 0.0,
        moves: vec![],
        dwells: vec![],
        spans: vec![],
        links: Default::default(),
    };
    moves(&p, &steep, &o, &mut tp);
    assert!(kmb_toolpath::check(&tp).is_empty());
}

#[test]
fn old_documents_read_the_same() {
    let v = serde_json::json!({
        "setup": 1, "tools": 2, "tool": 4, "leave": "0 mm", "step": "0.5 mm",
        "resolution": "0.1 mm"
    });
    let w: Waterline = serde_json::from_value(v).unwrap();
    assert_eq!(w.limits, Limits::default());
    assert_eq!(w.order, Order::TopDown);
    let back = serde_json::to_value(&w).unwrap();
    for k in ["leave_radial", "boundary", "order", "slope_to"] {
        assert!(back.get(k).is_none(), "{k}");
    }
}
