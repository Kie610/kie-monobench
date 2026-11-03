//! 確かめること: 研究「頭の HRTF」の頭の向きの決め方、世界に置いた頭の計算の左右、中止、データの往復。

use super::*;

const A: f64 = 87.5;

/// 中心 `c`(mm)・半径 A の球の頭の表示用の網。頭の座標(前・左・上)を世界の `axes` へ回して置く。
fn head(c: [f64; 3], axes: [[f64; 3]; 3]) -> Mesh {
    let s = icosphere(A, 2);
    Mesh {
        positions: s
            .vertices
            .iter()
            .map(|p| {
                [0, 1, 2].map(|k| {
                    (c[k] + axes[0][k] * p[0] + axes[1][k] * p[1] + axes[2][k] * p[2]) as f32
                })
            })
            .collect(),
        triangles: s.triangles,
        triangle_faces: vec![0; 320],
        face_names: vec!["頭".into()],
        ..Mesh::default()
    }
}

/// 顔が -Y、左の耳が +X を向く頭(世界の座標で頭の向きを替えても左右を取り違えない)。
fn task() -> HeadTask {
    let c = [10.0, 20.0, 30.0];
    let left = add(c, [A, 0.0, 0.0]);
    let right = add(c, [-A, 0.0, 0.0]);
    let axes = head_axes(left, right, axis("-y")).unwrap();
    HeadTask {
        mesh: head(c, axes),
        body: 7,
        conditions: Conditions {
            left,
            right,
            axes,
            grading: None,
            ear_radius: 0.0,
            fs: 48_000.0,
            taps: 128,
            max_hz: 1500.0,
            directions: Directions::Rings { step: 10.0 },
            distance: None,
            temperature: 20.0,
        },
    }
}

/// 確かめること: 顔の向きは耳を結ぶ向きに直交させ、上は前 × 左。耳が重なる・顔の向きが耳の向きと
/// 同じなら誤り。
#[test]
fn head_axes_follow_the_ears_and_the_face() {
    let a = head_axes([0.0, 0.0, 0.0], [-150.0, 0.0, 0.0], [0.3, -1.0, 0.0]).unwrap();
    assert_eq!(a[1], [1.0, 0.0, 0.0]);
    assert!(norm(sub(a[0], [0.0, -1.0, 0.0])) < 1e-12);
    assert!(norm(sub(a[2], [0.0, 0.0, 1.0])) < 1e-12);
    assert!(head_axes([0.0; 3], [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]).is_err());
    assert!(head_axes([0.0; 3], [-150.0, 0.0, 0.0], [1.0, 0.0, 0.0]).is_err());
}

/// 確かめること: 世界に置いた球の頭を解くと、方向の球の左の耳の側で左が大きく(音量差が正)、左の耳に
/// 先に届く(時間差が負)。網は頭の位置に描き、量とコマの数がそろう。
#[test]
fn a_placed_sphere_head_hears_left_on_the_left() {
    let t = task();
    let r = t.solve(&Control::new()).unwrap();
    assert!(!r.partial);
    assert_eq!(r.kind, "hrtf");
    assert_eq!(r.bodies, vec![7]);
    // 網(作り直していないので 1 つ)と方向の球
    assert_eq!(r.parts.len(), 2);
    let Geometry::Surface { points, .. } = &r.parts[0].geom else {
        panic!("網が表面でない")
    };
    let mid = [0, 1, 2]
        .map(|k| points.iter().map(|p| f64::from(p[k])).sum::<f64>() / points.len() as f64);
    assert!(
        norm(sub(mid, [10.0, 20.0, 30.0])) < 1.0,
        "網の真ん中 {mid:?}"
    );
    // 左・右・音量差・時間差・辺の長さ
    assert_eq!(r.quantities.len(), 5);
    assert_eq!(r.frames.len(), 4);
    assert_eq!(r.frames[3].label, "1.50 kHz");
    let ball = icosphere(1.0, BALLOON);
    let near = |d: [f64; 3]| {
        (0..ball.vertices.len())
            .min_by(|&a, &b| {
                norm(sub(ball.vertices[a], d)).total_cmp(&norm(sub(ball.vertices[b], d)))
            })
            .unwrap()
    };
    let (l, rr) = (near([0.0, 1.0, 0.0]), near([0.0, -1.0, 0.0]));
    let values = |q: usize| match &r.frames[3].values[q] {
        Values::F32(v) => v.to_vec(),
        other => panic!("値の形が違う: {other:?}"),
    };
    let ild = values(2);
    assert!(
        ild[l] > 3.0 && ild[rr] < -3.0,
        "音量差 左 {} 右 {}",
        ild[l],
        ild[rr]
    );
    let itd = values(3);
    // 球の時間差の目安は a(θ + sin θ)/c ≈ 650 µs
    assert!(
        itd[l] < -300.0 && itd[rr] > 300.0,
        "時間差 左 {} 右 {}",
        itd[l],
        itd[rr]
    );
    // 辺の長さは球の網の辺(約 25 mm)
    let len = values(4);
    assert!(len.iter().all(|&x| (15.0..35.0).contains(&x)));
}

/// 確かめること: 中止を頼まれていたら、解かずに中止で返す。
#[test]
fn cancel_stops_before_solving() {
    let c = Control::new();
    c.cancel();
    assert_eq!(task().solve(&c).unwrap_err(), CANCELLED);
}

/// 確かめること: 研究のデータは JSON で往復でき、空の欄は書かない。
#[test]
fn data_round_trips_through_json() {
    let d = HeadData {
        left: Some([0.0, 70.0, 0.0]),
        right: Some([0.0, -70.0, 0.0]),
        near: "2".into(),
        regrade: "both".into(),
        ..HeadData::default()
    };
    let v = serde_json::to_value(&d).unwrap();
    assert!(v.get("far").is_none());
    assert_eq!(serde_json::from_value::<HeadData>(v).unwrap(), d);
}
