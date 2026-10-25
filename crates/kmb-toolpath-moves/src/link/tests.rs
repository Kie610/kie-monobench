use super::*;
use kmb_cam_tool::ToolLibrary;

const SAFE: f64 = 5.0;
const RETRACT: f64 = 1.0;

fn tool() -> Tool {
    // 平の 3.175 mm(半径 1.5875 mm)
    ToolLibrary::starter().tools[0].clone()
}

fn tp(moves: Vec<Move>) -> Toolpath {
    Toolpath {
        name: "試し".into(),
        tool: tool(),
        rpm: 10000.0,
        safe_z: SAFE,
        retract_z: RETRACT,
        edge_break: 0.0,
        dwells: vec![],
        spans: vec![],
        links: LinkInfo::default(),
        moves,
    }
}

fn scene<'a>(tris: &'a [[[f64; 3]; 3]], keep_out: &'a [(P3, P3)]) -> Scene<'a> {
    Scene {
        stock_min: [0.0, 0.0, -5.0],
        stock_max: [60.0, 20.0, 0.0],
        triangles: tris,
        keep_out,
    }
}

/// `approach` と同じ: 安全高さへ上げ、横へ早送りし、退避高さまで下げる。
fn approach(m: &mut Vec<Move>, xy: [f64; 2]) {
    let c = m.last().unwrap().to();
    m.push(Move::Rapid {
        to: [c[0], c[1], SAFE],
    });
    m.push(Move::Rapid {
        to: [xy[0], xy[1], SAFE],
    });
    m.push(Move::Rapid {
        to: [xy[0], xy[1], RETRACT],
    });
}

fn feed(to: P3) -> Move {
    Move::Linear { to, feed: 600.0 }
}

/// x0..x1 × y0..y1 を高さ z で往復に削る(工具の中心の範囲)。
fn zigzag(m: &mut Vec<Move>, (x0, x1): (f64, f64), (y0, y1): (f64, f64), z: f64) {
    approach(m, [x0, y0]);
    m.push(feed([x0, y0, z]));
    let mut y = y0;
    let mut right = true;
    loop {
        m.push(feed([if right { x1 } else { x0 }, y, z]));
        right = !right;
        if y >= y1 - 1e-9 {
            break;
        }
        y = (y + 1.0).min(y1);
        m.push(feed([if right { x0 } else { x1 }, y, z]));
    }
}

fn start() -> Vec<Move> {
    vec![Move::Rapid {
        to: [0.0, 0.0, SAFE],
    }]
}

fn finish(m: &mut Vec<Move>) {
    let c = m.last().unwrap().to();
    m.push(Move::Rapid {
        to: [c[0], c[1], SAFE],
    });
}

#[test]
fn a_link_inside_the_cleared_area_stays_down() {
    let mut m = start();
    zigzag(&mut m, (3.0, 17.0), (3.0, 17.0), -2.0);
    // 削った所の中の別の所へ(終わりの (17, 17) から 7 mm ほど)
    approach(&mut m, [12.0, 12.0]);
    m.push(feed([12.0, 12.0, -2.0]));
    m.push(feed([12.0, 12.0, -3.0]));
    finish(&mut m);
    let mut t = tp(m);
    let c = relink(&mut t, &scene(&[], &[]), &LinkOptions::default());
    // 最初の入り(まだ削っていない)はつなぎではない
    assert_eq!(
        c,
        Counts {
            stay_down: 1,
            ..Default::default()
        }
    );
    assert!(
        kmb_toolpath::check(&t).is_empty(),
        "{:?}",
        kmb_toolpath::check(&t)
    );
    let l = t
        .links
        .links
        .iter()
        .find(|l| l.kind == LinkKind::StayDown)
        .unwrap();
    assert!((l.z - (-2.0 + LIFT)).abs() < 1e-9, "{l:?}");
    // つなぎの中に安全高さへの早送りはない
    assert!(
        t.moves[l.start..l.end].iter().all(|m| m.to()[2] < 0.0),
        "{:?}",
        &t.moves[l.start..l.end]
    );
    assert!(t.links.air.len() >= 2);
}

#[test]
fn a_link_over_uncut_stock_keeps_the_full_retract() {
    let mut m = start();
    zigzag(&mut m, (3.0, 10.0), (3.0, 17.0), -2.0);
    // 削っていない所を越えて、右の離れた所へ
    zigzag(&mut m, (30.0, 40.0), (3.0, 17.0), -2.0);
    finish(&mut m);
    let mut t = tp(m.clone());
    let c = relink(&mut t, &scene(&[], &[]), &LinkOptions::default());
    assert_eq!(
        c,
        Counts {
            full: 1,
            ..Default::default()
        }
    );
    assert_eq!(t.moves, m, "元の経路のまま");
}

#[test]
fn a_long_link_over_cut_area_retracts_lower() {
    let mut m = start();
    // 0..60 の全面を深さ 3 mm まで削る(フェイス)
    zigzag(&mut m, (0.0, 60.0), (0.0, 20.0), -3.0);
    // 遠い所(下げたまま移る距離より遠い)へ
    approach(&mut m, [2.0, 2.0]);
    m.push(feed([2.0, 2.0, -3.0]));
    m.push(feed([2.0, 2.0, -4.0]));
    finish(&mut m);
    let mut t = tp(m.clone());
    let c = relink(&mut t, &scene(&[], &[]), &LinkOptions::default());
    assert_eq!(c.low, 1, "{c:?}");
    let l = t
        .links
        .links
        .iter()
        .find(|l| l.kind == LinkKind::Low)
        .unwrap();
    // 削った面 -3 + 余裕 5 = 2(退避高さ 1 より上、安全高さ 5 より下)
    assert!((l.z - 2.0).abs() < 1e-6, "{l:?}");
    assert!(
        kmb_toolpath::check(&t).is_empty(),
        "{:?}",
        kmb_toolpath::check(&t)
    );
    // 余裕を「完全退避」にすると元のまま
    let mut t2 = tp(m.clone());
    let opt = LinkOptions {
        fit_retract: false,
        ..Default::default()
    };
    let c2 = relink(&mut t2, &scene(&[], &[]), &opt);
    assert_eq!(c2.low, 0, "{c2:?}");
    assert_eq!(t2.moves, m, "元の経路のまま");
}

#[test]
fn a_part_between_the_ends_is_lifted_over() {
    // 削った所の真ん中に、高さ -1 の部品の島(x 9..11)
    let tris = [
        [[9.0, 0.0, -1.0], [11.0, 0.0, -1.0], [11.0, 20.0, -1.0]],
        [[9.0, 0.0, -1.0], [11.0, 20.0, -1.0], [9.0, 20.0, -1.0]],
    ];
    let mut m = start();
    zigzag(&mut m, (3.0, 6.0), (3.0, 17.0), -2.0);
    zigzag(&mut m, (14.0, 17.0), (3.0, 17.0), -2.0);
    finish(&mut m);
    let mut t = tp(m);
    let c = relink(&mut t, &scene(&tris, &[]), &LinkOptions::default());
    // 島の上のストックは削っていないので、上面を越える全退避
    assert_eq!(
        c,
        Counts {
            full: 1,
            ..Default::default()
        }
    );
    let _ = t;
}

#[test]
fn the_part_is_never_cut_by_a_stay_down_link() {
    // 削った所(-2)の中に、部品の島(高さ -1.5、x 9..11、y 0..20)。ストックは島の上も削った
    // ことにするため、全体を -1.5 まで先に削る
    let tris = [
        [[9.0, 0.0, -1.5], [11.0, 0.0, -1.5], [11.0, 20.0, -1.5]],
        [[9.0, 0.0, -1.5], [11.0, 20.0, -1.5], [9.0, 20.0, -1.5]],
    ];
    let mut m = start();
    zigzag(&mut m, (0.0, 20.0), (0.0, 20.0), -1.5);
    zigzag(&mut m, (3.0, 6.0), (3.0, 17.0), -2.0);
    approach(&mut m, [14.0, 10.0]);
    m.push(feed([14.0, 10.0, -1.5]));
    m.push(feed([14.0, 10.0, -2.0]));
    finish(&mut m);
    let mut t = tp(m);
    let c = relink(&mut t, &scene(&tris, &[]), &LinkOptions::default());
    assert_eq!(c.lift, 1, "{c:?}");
    let l = t
        .links
        .links
        .iter()
        .find(|l| l.kind == LinkKind::Lift)
        .unwrap();
    assert!(
        (l.z - (-1.5 + LIFT)).abs() < 1e-6,
        "部品の上 + 持ち上げ: {l:?}"
    );
}

#[test]
fn fixtures_keep_the_full_retract() {
    let zone = [([8.0, -5.0, -5.0], [12.0, 25.0, 4.5])];
    let mut m = start();
    zigzag(&mut m, (0.0, 20.0), (0.0, 20.0), -2.0);
    // 終わりの (20, 20) から、固定具の手前の (17, 15) へ。次に固定具の向こうの (3, 10) へ
    approach(&mut m, [17.0, 15.0]);
    m.push(feed([17.0, 15.0, -2.0]));
    m.push(feed([17.0, 15.0, -3.0]));
    approach(&mut m, [3.0, 10.0]);
    m.push(feed([3.0, 10.0, -2.0]));
    m.push(feed([3.0, 10.0, -3.0]));
    finish(&mut m);
    let mut t = tp(m);
    let c = relink(&mut t, &scene(&[], &zone), &LinkOptions::default());
    assert_eq!(
        c,
        Counts {
            stay_down: 1,
            full: 1,
            ..Default::default()
        }
    );
}

#[test]
fn a_tangent_wall_does_not_block() {
    // 縦の壁(x = 10)に工具が接して終わる。壁から離れる向きのつなぎは下げたままでよい
    let r = tool().radius();
    let tris = [
        [[10.0, 0.0, -3.0], [10.0, 20.0, -3.0], [10.0, 20.0, 0.0]],
        [[10.0, 0.0, -3.0], [10.0, 20.0, 0.0], [10.0, 0.0, 0.0]],
    ];
    let p = super::Part::new(&tris);
    assert_eq!(
        p.max_near([10.0 - r, 5.0], [2.0, 5.0], r - 1e-4),
        f64::NEG_INFINITY
    );
    // 少しでも壁に寄れば、壁の上端が見える
    assert!((p.max_near([10.0 - r + 0.01, 5.0], [2.0, 5.0], r - 1e-4) - 0.0).abs() < 1e-9);
}

#[test]
fn triangle_max_is_exact_for_a_slope() {
    // z = x の斜面。線分 (0,0)–(0,10) から 1 以内の最大は x = 1 の所
    let t = [[-5.0, -5.0, -5.0], [5.0, -5.0, 5.0], [0.0, 20.0, 0.0]];
    let z = tri_max_near(&t, [0.0, 0.0], [0.0, 10.0], 1.0).unwrap();
    assert!((z - 1.0).abs() < 1e-9, "{z}");
}

#[test]
fn a_rapid_that_ends_below_the_retract_height_is_not_rapided_down_again() {
    // 削った穴の中で真上へ早送りで上げ(退避高さより下で止める)、横へ送りで移る(らせんの穴の
    // 内側の周から外側の周へ)。つなぎを置き換えても、退避高さより下へ早送りで下りない
    let mut m = start();
    zigzag(&mut m, (0.0, 60.0), (0.0, 20.0), -4.0);
    approach(&mut m, [10.0, 10.0]);
    m.push(feed([10.0, 10.0, -4.0]));
    m.push(Move::Rapid {
        to: [10.0, 10.0, -1.0],
    });
    m.push(feed([12.0, 10.0, -1.0]));
    m.push(feed([12.0, 10.0, -4.0]));
    finish(&mut m);
    let t = tp(m);
    assert!(kmb_toolpath::check(&t).is_empty());
    // 下げたまま移らない設定でも
    for stay_down in [STAY_DOWN, 0.0] {
        let mut t = t.clone();
        let opt = LinkOptions {
            stay_down,
            ..Default::default()
        };
        let _ = relink(&mut t, &scene(&[], &[]), &opt);
        assert!(
            kmb_toolpath::check(&t).is_empty(),
            "{:?}\n{:?}",
            kmb_toolpath::check(&t),
            t.moves
        );
    }
}

#[test]
fn the_entry_and_lead_marks_follow_the_moves_after_relinking() {
    let mut m = start();
    zigzag(&mut m, (3.0, 17.0), (3.0, 17.0), -2.0);
    // 削った所の中の別の所へ下りる(入りの印)、そこから少し削る(リードの印)
    approach(&mut m, [12.0, 12.0]);
    let entry = m.len();
    m.push(feed([12.0, 12.0, -3.0]));
    let lead = m.len();
    m.push(feed([14.0, 12.0, -3.0]));
    finish(&mut m);
    let mut t = tp(m);
    t.spans = vec![
        kmb_toolpath::Span {
            start: entry,
            end: entry + 1,
            role: kmb_toolpath::Role::Entry,
        },
        kmb_toolpath::Span {
            start: lead,
            end: lead + 1,
            role: kmb_toolpath::Role::Lead,
        },
    ];
    relink(&mut t, &scene(&[], &[]), &LinkOptions::default());
    let roles = t.roles();
    // 入りの印は (12, 12) へ下りる送りに、リードの印は (14, 12) へ削る送りに付いたまま
    let at = |role| {
        t.moves
            .iter()
            .zip(&roles)
            .filter(|(_, r)| **r == Some(role))
            .map(|(m, _)| m.to())
            .collect::<Vec<_>>()
    };
    let e = at(kmb_toolpath::Role::Entry);
    assert!(
        !e.is_empty()
            && e.iter()
                .all(|p| (p[0] - 12.0).abs() < 1e-9 && (p[1] - 12.0).abs() < 1e-9),
        "{e:?} {:?}",
        t.moves
    );
    assert!(e.iter().any(|p| (p[2] + 3.0).abs() < 1e-9));
    assert_eq!(at(kmb_toolpath::Role::Lead), vec![[14.0, 12.0, -3.0]]);
}
