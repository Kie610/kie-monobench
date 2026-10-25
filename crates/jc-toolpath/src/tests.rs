use super::*;

#[test]
fn link_records_follow_the_moves() {
    let mut l = LinkInfo {
        links: vec![Link {
            start: 2,
            end: 4,
            kind: LinkKind::Low,
            z: 2.0,
        }],
        air: vec![[2, 4]],
    };
    // 5 個の動きを 2 回つなぐと、2 回目の記録は 5 つずれる
    let r = l.repeat(5, 2);
    assert_eq!((r.links[1].start, r.links[1].end), (7, 9));
    assert_eq!(r.air, vec![[2, 4], [7, 9]]);
    // 3 番目の動きの前に 1 つ足すと、それより後ろの番号が 1 つずれる
    l.remap(|i| if i >= 3 { i + 1 } else { i });
    assert_eq!((l.links[0].start, l.links[0].end), (2, 5));
    assert_eq!(l.air, vec![[2, 5]]);
}
use jc_cam_tool::ToolLibrary;

fn tp(moves: Vec<Move>) -> Toolpath {
    Toolpath {
        name: "test".into(),
        tool: ToolLibrary::starter().tools[0].clone(),
        rpm: 10000.0,
        safe_z: 5.0,
        retract_z: 1.0,
        edge_break: 0.0,
        links: LinkInfo::default(),
        moves,
        dwells: vec![],
        spans: vec![],
    }
}

#[test]
fn a_dwell_follows_the_feed_move_that_reaches_its_point() {
    let mut t = tp(vec![
        Move::Rapid {
            to: [0.0, 0.0, 5.0],
        },
        Move::Linear {
            to: [0.0, 0.0, -1.0],
            feed: 60.0,
        },
        Move::Rapid {
            to: [0.0, 0.0, 5.0],
        },
    ]);
    let before = t.minutes(&Motion::simple(1000.0));
    t.dwells.push(Dwell {
        at: [0.0, 0.0, -1.0],
        seconds: 1.5,
    });
    assert!((t.minutes(&Motion::simple(1000.0)) - before - 1.5 / 60.0).abs() < 1e-9);
    let p = Program::build("p", 1, 20.0, std::slice::from_ref(&t));
    let k = p
        .ops
        .iter()
        .position(|o| matches!(o, Op::Dwell { seconds } if *seconds == 1.5))
        .expect("止まる");
    assert!(matches!(p.ops[k - 1], Op::Move(Move::Linear { .. })));
    // 早送りで同じ所に来ても止まらない
    t.dwells[0].at = [0.0, 0.0, 5.0];
    let p = Program::build("p", 1, 20.0, std::slice::from_ref(&t));
    assert!(
        !p.ops
            .iter()
            .any(|o| matches!(o, Op::Dwell { seconds } if *seconds == 1.5))
    );
}

#[test]
fn rapid_below_safe_height_is_reported() {
    let ok = tp(vec![
        Move::Rapid {
            to: [0.0, 0.0, 5.0],
        },
        Move::Linear {
            to: [0.0, 0.0, -1.0],
            feed: 100.0,
        },
        Move::Linear {
            to: [10.0, 0.0, -1.0],
            feed: 100.0,
        },
        Move::Rapid {
            to: [10.0, 0.0, 5.0],
        },
    ]);
    assert!(check(&ok).is_empty(), "{:?}", check(&ok));
    let mut bad = ok.clone();
    bad.moves[2] = Move::Rapid {
        to: [10.0, 0.0, -1.0],
    };
    let p = check(&bad);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].index, 2);
    let mut bad = ok.clone();
    bad.moves.pop();
    assert!(check(&bad)[0].message.contains("戻っていません"));
}

#[test]
fn arcs_are_sampled_on_the_circle() {
    let m = Move::Arc {
        to: [10.0, 0.0, -2.0],
        center: [0.0, 0.0],
        dir: ArcDir::Ccw,
        feed: 100.0,
    };
    // 始点と終点が同じなので一周のらせん
    let pts = m.points([10.0, 0.0, 0.0], 0.5);
    assert!(pts.len() > 120);
    for p in &pts {
        assert!((p[0].hypot(p[1]) - 10.0).abs() < 1e-9);
    }
    assert_eq!(*pts.last().unwrap(), [10.0, 0.0, -2.0]);
    assert!(pts[pts.len() / 4][1] > 0.0, "反時計回りなら最初は y が正");
    let s = arc_sweep([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0], ArcDir::Cw);
    assert!((s + 1.5 * std::f64::consts::PI).abs() < 1e-9);
}

#[test]
fn program_changes_tools_and_stops_the_spindle() {
    let a = tp(vec![
        Move::Rapid {
            to: [0.0, 0.0, 5.0],
        },
        Move::Rapid {
            to: [0.0, 0.0, 5.0],
        },
    ]);
    let mut b = a.clone();
    b.tool = ToolLibrary::starter().tools[1].clone();
    b.rpm = 12000.0;
    let p = Program::build("p", 1, 20.0, &[a.clone(), a, b]);
    let changes = p
        .ops
        .iter()
        .filter(|o| matches!(o, Op::ToolChange { .. }))
        .count();
    assert_eq!(changes, 2);
    assert_eq!(p.tools().len(), 2);
    assert!(matches!(p.ops.last(), Some(Op::SpindleStop)));
    let stops = p
        .ops
        .iter()
        .filter(|o| matches!(o, Op::SpindleStop))
        .count();
    assert_eq!(stops, 2);
}

/// 停止を 2 つの加工の間に入れると、その前で退避して主軸を止め、後で回し直して待つ。
#[test]
fn a_stop_between_two_paths_stops_the_spindle_and_restarts_it() {
    let a = tp(vec![
        Move::Rapid {
            to: [0.0, 0.0, 5.0],
        },
        Move::Linear {
            to: [0.0, 0.0, -1.0],
            feed: 100.0,
        },
        Move::Rapid {
            to: [0.0, 0.0, 5.0],
        },
    ]);
    let mut b = a.clone();
    b.name = "後".into();
    let manual = [(
        1,
        Manual::Stop {
            optional: false,
            text: "クランプを付け替える".into(),
        },
    )];
    let p = Program::build_with("p", 1, 20.0, &[a.clone(), b], &manual);
    let at = p
        .ops
        .iter()
        .position(|o| matches!(o, Op::Stop { optional: false }))
        .expect("停止がある");
    assert_eq!(p.ops[at - 4], Op::Retract { z: 20.0 });
    assert_eq!(p.ops[at - 3], Op::SpindleStop);
    assert_eq!(
        p.ops[at - 2],
        Op::Comment {
            text: "停止: クランプを付け替える".into()
        }
    );
    assert!(matches!(&p.ops[at - 1], Op::Message { text } if text == "クランプを付け替える"));
    // 停止の後: 次の加工の名前、主軸を回し直し、回り始めを待ってから動く
    assert_eq!(p.ops[at + 1], Op::Comment { text: "後".into() });
    assert_eq!(p.ops[at + 2], Op::Spindle { rpm: 10000.0 });
    assert_eq!(p.ops[at + 3], Op::Dwell { seconds: 2.0 });
    assert!(matches!(p.ops[at + 4], Op::Move(_)));
    // 同じ工具なので交換はしない
    let changes = p
        .ops
        .iter()
        .filter(|o| matches!(o, Op::ToolChange { .. }))
        .count();
    assert_eq!(changes, 1);
    // 最後の加工の後の停留とコメントは、最後の退避と主軸の停止の前に入る
    let tail = [
        (2, Manual::Dwell { seconds: 3.0 }),
        (
            2,
            Manual::Comment {
                text: "終わり".into(),
            },
        ),
    ];
    let p = Program::build_with("p", 1, 20.0, &[a.clone(), a.clone()], &tail);
    let n = p.ops.len();
    assert_eq!(p.ops[n - 4], Op::Dwell { seconds: 3.0 });
    assert_eq!(
        p.ops[n - 3],
        Op::Comment {
            text: "終わり".into()
        }
    );
    assert_eq!(p.ops[n - 1], Op::SpindleStop);
    // 停止は待つ時間を数えない。停留は数える
    let plain = Program::build("p", 1, 20.0, &[a.clone(), a.clone()]);
    let m = Motion::simple(3000.0);
    assert!((p.minutes(&m) - plain.minutes(&m) - 3.0 / 60.0).abs() < 1e-9);
}

/// 手動 NC の命令と、新しいプログラムの命令は JSON で読み書きでき、省いた欄は既定に戻る。
#[test]
fn manual_commands_round_trip_through_json() {
    let m: Manual = serde_json::from_str(r#"{"kind":"stop"}"#).unwrap();
    assert_eq!(
        m,
        Manual::Stop {
            optional: false,
            text: String::new()
        }
    );
    assert_eq!(
        serde_json::to_string(&Op::Stop { optional: false }).unwrap(),
        r#"{"op":"stop"}"#
    );
    let o: Op = serde_json::from_str(r#"{"op":"stop","optional":true}"#).unwrap();
    assert_eq!(o, Op::Stop { optional: true });
    // 今までの命令の JSON は変わらない
    assert_eq!(
        serde_json::to_string(&Op::Dwell { seconds: 2.0 }).unwrap(),
        r#"{"op":"dwell","seconds":2.0}"#
    );
}

#[test]
fn mirrored_arcs_turn_the_other_way_and_rotations_keep_their_radius() {
    let arc = Move::Arc {
        to: [12.0, 10.0, -1.0],
        center: [10.0, 10.0],
        dir: ArcDir::Ccw,
        feed: 100.0,
    };
    let m = Xform2::mirror(false, 50.0);
    assert!(m.mirrors());
    assert_eq!(
        m.apply(&arc),
        Move::Arc {
            to: [88.0, 10.0, -1.0],
            center: [90.0, 10.0],
            dir: ArcDir::Cw,
            feed: 100.0
        }
    );
    let my = Xform2::mirror(true, 0.0);
    assert!(
        matches!(my.apply(&arc), Move::Arc { dir: ArcDir::Cw, center, .. } if center == [10.0, -10.0])
    );
    let r = Xform2::rotate([0.0, 0.0], std::f64::consts::FRAC_PI_2);
    assert!(!r.mirrors());
    let Move::Arc {
        to, center, dir, ..
    } = r.apply(&arc)
    else {
        panic!()
    };
    assert_eq!(dir, ArcDir::Ccw);
    assert!((center[0] + 10.0).abs() < 1e-9 && (center[1] - 10.0).abs() < 1e-9);
    assert!((to[0] + 10.0).abs() < 1e-9 && (to[1] - 12.0).abs() < 1e-9);
    assert_eq!(to[2], -1.0);
    let t = Xform2::translate(5.0, -2.0);
    assert_eq!(t.point([1.0, 1.0, 3.0]), [6.0, -1.0, 3.0]);
}

#[test]
fn setups_are_chained_with_a_stop_between_them() {
    let a = tp(vec![
        Move::Rapid {
            to: [0.0, 0.0, 5.0],
        },
        Move::Rapid {
            to: [1.0, 0.0, 5.0],
        },
    ]);
    let p1 = Program::build("表", 1, 20.0, std::slice::from_ref(&a));
    let p2 = Program::build("裏", 1, 25.0, &[a]);
    let all = Program::chain("全部", &[p1.clone(), p2.clone()]).unwrap();
    let stops: Vec<usize> = all
        .ops
        .iter()
        .enumerate()
        .filter(|(_, o)| matches!(o, Op::Stop { .. }))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(stops.len(), 1);
    let at = stops[0];
    // 停止の前は 1 本目の終わり(主軸の停止)と、付け替えのコメント
    assert_eq!(all.ops[at - 4], Op::SpindleStop);
    assert_eq!(all.ops[at - 3], Op::Retract { z: 25.0 });
    assert!(matches!(&all.ops[at - 2], Op::Comment { text } if text.contains("裏 へ付け替え")));
    // 停止の後は 2 本目(セットアップの名前、加工の名前、工具交換)
    assert_eq!(all.ops[at + 1], Op::Comment { text: "裏".into() });
    assert!(matches!(all.ops[at + 3], Op::ToolChange { .. }));
    assert_eq!(all.clearance_z, 25.0);
    let mut p3 = p2;
    p3.wcs = 2;
    assert!(Program::chain("x", &[p1, p3]).unwrap_err().contains("G55"));
}

fn air() -> Motion {
    Motion {
        rapid_feed: 3000.0,
        acceleration: 150.0,
        junction_deviation: 0.05,
        max_rate: [3000.0, 3000.0, 2000.0],
    }
}

#[test]
fn a_long_line_adds_the_time_to_speed_up_and_slow_down() {
    let p = tp(vec![
        Move::Rapid {
            to: [0.0, 0.0, 0.0],
        },
        Move::Linear {
            to: [100.0, 0.0, 0.0],
            feed: 600.0,
        },
    ]);
    // 10 mm/s で 10 秒、加速と減速で v / a = 1/15 秒だけ延びる
    let s = p.minutes(&air()) * 60.0;
    assert!((s - (10.0 + 10.0 / 150.0)).abs() < 1e-9, "{s}");
    assert!((p.minutes(&Motion::simple(3000.0)) * 60.0 - 10.0).abs() < 1e-9);
}

#[test]
fn corners_and_the_z_axis_slow_the_tool() {
    // 1 辺 10 mm の正方形を 3000 mm/min(50 mm/s)で。直角の角の速さは
    // √(a × jd × sin45° / (1 − sin45°)) ≈ 4.26 mm/s
    let sq = [[10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]];
    let mut moves = vec![Move::Rapid {
        to: [0.0, 0.0, 0.0],
    }];
    moves.extend(sq.iter().map(|q| Move::Linear {
        to: [q[0], q[1], 0.0],
        feed: 3000.0,
    }));
    let s = tp(moves).minutes(&air()) * 60.0;
    let a: f64 = 150.0;
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let vc = (a * 0.05 * half / (1.0 - half)).sqrt();
    // 角から角へ: 頂点まで加速して減速(50 mm/s には届かない)
    let side = |v0: f64, v1: f64| {
        let peak = ((2.0 * a * 10.0 + v0 * v0 + v1 * v1) / 2.0).sqrt();
        (2.0 * peak - v0 - v1) / a
    };
    let want = side(0.0, vc) + 2.0 * side(vc, vc) + side(vc, 0.0);
    assert!((s - want).abs() < 1e-9, "{s} {want}");
    // 式を写し間違えていないかを、手で出した数でも見る(角の速さ 4.255 mm/s、合計 1.9047 秒)。
    // 速さ一定で回る 0.8 秒より長く、角ごとに止まる 4 × 2√(10/a) ≈ 2.0656 秒より短い
    assert!((vc - 4.2552).abs() < 1e-4, "{vc}");
    assert!((s - 1.9047).abs() < 1e-4, "{s}");
    assert!(s > 40.0 / 50.0 && s < 8.0 * (10.0 / a).sqrt());
    // 動きごとの時間は辺ごとの時間で、合計は全体の時間
    let mut moves = vec![Move::Rapid {
        to: [0.0, 0.0, 0.0],
    }];
    moves.extend(sq.iter().map(|q| Move::Linear {
        to: [q[0], q[1], 0.0],
        feed: 3000.0,
    }));
    let each = tp(moves).move_seconds(&air());
    assert_eq!(each.len(), 5);
    assert_eq!(each[0], 0.0);
    assert!((each[1] - side(0.0, vc)).abs() < 1e-9 && (each[2] - side(vc, vc)).abs() < 1e-9);
    assert!((each.iter().sum::<f64>() - want).abs() < 1e-9);
    assert!(s > 0.8 * 1.5, "単純に割った 0.8 秒よりずっと長い");
    // Z の早送りは Z の最高速(2000 mm/min)に抑えられる
    let z = tp(vec![
        Move::Rapid {
            to: [0.0, 0.0, 20.0],
        },
        Move::Rapid {
            to: [0.0, 0.0, 0.0],
        },
    ])
    .minutes(&air())
        * 60.0;
    let vz = 2000.0 / 60.0;
    assert!((z - (20.0 / vz + vz / a)).abs() < 1e-9, "{z}");
}
