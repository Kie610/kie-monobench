//! 形状カーネルの契約。SMLib と truck のどちらのカーネルも同じ確かめを通す。
//! 各カーネルは自分のテストから [`kernel_contract_tests!`](crate::kernel_contract_tests) を呼ぶ。
//! カーネルごとに違ってよい所(面の分け方など)は [`KernelQuirks`] で渡す。

use kmb_core::kernel::{
    BooleanOp, Kernel, KernelError, Plane, Profile, ProfileEdge, ProfileLoop, Region, Seam, Segment,
};

/// カーネルごとに違ってよい所と、カーネルの型ごとの関数。
pub struct KernelQuirks<K> {
    /// 持っている形の数(Kernel の外の関数なので、カーネルごとに渡す)
    pub shape_count: fn(&K) -> usize,
    /// 穴(円)の側面の名前。SMLib は 1 面、truck は円を 2 つに割るので 2 面
    pub hole_side_faces: &'static [&'static str],
    /// 読み込んだ STEP が足し引きに使える立体になるか。SMLib は自前の読み手で立体に組み、
    /// truck は網で読む(足し引きできない)
    pub step_is_solid: bool,
}

/// 左下 (x0, y0)・幅 w・高さ h の長方形の輪。辺の名前は `e<base>` から反時計回りに 4 つ。
/// カーネルの試験が同じ形を使うので公開する。
pub fn rect_loop(x0: f64, y0: f64, w: f64, h: f64, base: u32) -> ProfileLoop {
    let p = [[x0, y0], [x0 + w, y0], [x0 + w, y0 + h], [x0, y0 + h]];
    ProfileLoop {
        edges: (0..4)
            .map(|i| ProfileEdge {
                name: format!("e{}", base + i as u32),
                segment: Segment::Line {
                    a: p[i],
                    b: p[(i + 1) % 4],
                },
                reversed: false,
            })
            .collect(),
    }
}

/// 中心 c・半径 r の円 1 つの輪(穴にする時は `reversed`)。
pub fn circle_loop(c: [f64; 2], r: f64, name: &str, reversed: bool) -> ProfileLoop {
    ProfileLoop {
        edges: vec![ProfileEdge {
            name: name.into(),
            segment: Segment::Circle {
                center: c,
                radius: r,
            },
            reversed,
        }],
    }
}

/// 60×40 の板の真ん中に半径 4 の穴。外の辺は e1〜e4、穴は e9。
pub fn plate_profile() -> Profile {
    Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(0.0, 0.0, 60.0, 40.0, 1),
            holes: vec![circle_loop([30.0, 20.0], 4.0, "e9", true)],
        }],
    }
}

fn rect_profile(x0: f64, y0: f64, w: f64, h: f64, base: u32) -> Profile {
    Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(x0, y0, w, h, base),
            holes: vec![],
        }],
    }
}

fn polygon(points: &[[f64; 2]]) -> Profile {
    let n = points.len();
    Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: (0..n)
                    .map(|i| ProfileEdge {
                        name: format!("e{}", i + 1),
                        segment: Segment::Line {
                            a: points[i],
                            b: points[(i + 1) % n],
                        },
                        reversed: false,
                    })
                    .collect(),
            },
            holes: vec![],
        }],
    }
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

fn face_names(k: &impl Kernel, h: kmb_core::kernel::ShapeHandle) -> Vec<String> {
    k.faces(h).unwrap().into_iter().map(|f| f.name).collect()
}

pub fn extrude_plate_with_hole<K: Kernel>(k: &K, q: &KernelQuirks<K>) {
    let h = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let m = k.mass(h).unwrap();
    let expect = 60.0 * 40.0 * 10.0 - std::f64::consts::PI * 16.0 * 10.0;
    assert!(close(m.volume, expect, 1e-4), "{} vs {expect}", m.volume);
    let area = 2.0 * (2400.0 - std::f64::consts::PI * 16.0)
        + 200.0 * 10.0
        + 2.0 * std::f64::consts::PI * 4.0 * 10.0;
    assert!(close(m.area, area, 1e-3), "{} vs {area}", m.area);
    assert!(close(m.min[0], 0.0, 1e-6) && close(m.max[0], 60.0, 1e-6));
    assert!(close(m.max[2], 10.0, 1e-6));
    let names = face_names(k, h);
    for n in ["f1:start", "f1:end", "f1:side:e1", "f1:side:e4"]
        .iter()
        .chain(q.hole_side_faces)
    {
        assert!(names.contains(&n.to_string()), "{n} がない: {names:?}");
    }
    let faces = k.faces(h).unwrap();
    let top = faces.iter().find(|f| f.name == "f1:end").unwrap();
    let p = top.planar.unwrap();
    assert!(close(p.normal[2], 1.0, 1e-9) && close(p.point[2], 10.0, 1e-9));
    let bottom = faces.iter().find(|f| f.name == "f1:start").unwrap();
    assert!(close(bottom.planar.unwrap().normal[2], -1.0, 1e-9));
}

/// 角(頂点)は囲む面の名前で呼べ、名前は重ならず、形を作り直しても同じ。穴の円の継ぎ目は角にしない。
pub fn corners_are_named_by_their_faces<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let names = |h| {
        let mut v: Vec<(String, [f64; 3])> = kmb_core::vertices(k, h)
            .into_iter()
            .map(|v| (v.name, v.at))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };
    let h = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let v = names(h);
    // 板の角は 8 つ(穴の壁を半分ずつの面で作るカーネルでは、穴の縁の半円の端も角になる)
    let outer = v
        .iter()
        .filter(|(_, at)| (at[0] - 30.0).hypot(at[1] - 20.0) > 5.0)
        .count();
    assert_eq!(outer, 8, "{v:?}");
    let mut uniq: Vec<&String> = v.iter().map(|x| &x.0).collect();
    uniq.dedup();
    assert_eq!(uniq.len(), v.len(), "名前が重なる: {v:?}");
    // 上の面の原点の角は、上面と、原点で交わる 2 つの側面で呼ぶ
    let corner = v
        .iter()
        .find(|(_, at)| {
            close(at[0], 0.0, 1e-9) && close(at[1], 0.0, 1e-9) && close(at[2], 10.0, 1e-9)
        })
        .expect("原点の上の角");
    assert!(corner.0.contains("f1:end"), "{}", corner.0);
    assert_eq!(corner.0.split('|').count(), 3, "{}", corner.0);
    let again = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    assert_eq!(names(again), v);
}

pub fn extrude_downward_is_still_outward<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let h = k.extrude(&plate_profile(), -5.0, 0.0, "f1").unwrap();
    let m = k.mass(h).unwrap();
    // 向きが裏返ると体積が負になる。下へ押し出しても上と同じ体積
    let expect = (60.0 * 40.0 - std::f64::consts::PI * 16.0) * 5.0;
    assert!(close(m.volume, expect, 1e-4), "{} vs {expect}", m.volume);
    assert!(close(m.min[2], -5.0, 1e-6) && close(m.max[2], 0.0, 1e-6));
}

pub fn pocket_and_names_survive_boolean<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let plate = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    // 上の面から 5 mm 掘るポケット。上側は 0.01 mm はみ出させる
    let tool = k
        .extrude(&rect_profile(5.0, 5.0, 15.0, 10.0, 20), 5.0, 10.01, "f2")
        .unwrap();
    let cut = k.boolean(BooleanOp::Subtract, plate, tool).unwrap();
    let m = k.mass(cut).unwrap();
    let expect = 60.0 * 40.0 * 10.0 - std::f64::consts::PI * 160.0 - 15.0 * 10.0 * 5.0;
    assert!(close(m.volume, expect, 1e-4), "{} vs {expect}", m.volume);
    let names = face_names(k, cut);
    // 上の面は残り、ポケットの底は道具の始まりの面から来る
    assert!(names.contains(&"f1:end".to_string()), "{names:?}");
    assert!(names.contains(&"f2:start".to_string()), "{names:?}");
    assert!(names.contains(&"f2:side:e20".to_string()), "{names:?}");
    assert!(!names.iter().any(|n| n == "unnamed"), "{names:?}");
}

pub fn disjoint_subtract_keeps_the_target<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let a = k
        .extrude(&rect_profile(0.0, 0.0, 10.0, 10.0, 1), 0.0, 10.0, "a")
        .unwrap();
    let b = k
        .extrude(&rect_profile(30.0, 0.0, 10.0, 10.0, 5), 0.0, 10.0, "b")
        .unwrap();
    let d = k.boolean(BooleanOp::Subtract, a, b).unwrap();
    assert!(close(k.mass(d).unwrap().volume, 1000.0, 1e-6));
    let u = k.boolean(BooleanOp::Union, a, b).unwrap();
    assert!(close(k.mass(u).unwrap().volume, 2000.0, 1e-6));
    let i = k.boolean(BooleanOp::Intersect, a, b).unwrap();
    assert!(k.mass(i).unwrap().volume.abs() < 1e-9);
}

/// 箱(`lo`〜`hi`)。名前は `tag`。
fn cuboid<K: Kernel>(
    k: &K,
    lo: [f64; 3],
    hi: [f64; 3],
    tag: &str,
) -> kmb_core::kernel::ShapeHandle {
    let p = rect_profile(lo[0], lo[1], hi[0] - lo[0], hi[1] - lo[1], 1);
    k.extrude(&p, lo[2], hi[2], tag).unwrap()
}

fn volume<K: Kernel>(k: &K, h: kmb_core::kernel::ShapeHandle) -> f64 {
    k.mass(h).unwrap().volume
}

/// 分かれた面の切れ端の名前は、計算し直しても同じ。上の面を溝で 2 つに分け
/// (`a:end#0` は y の小さい方)、さらに y の大きい方を切り欠きで 2 つに分ける。同じ平面の元の面が
/// 2 つある時、結果の面はそれを含む元の面の名前を継ぐ(y の小さい方は `a:end#0` のまま、大きい方の
/// 切れ端は `a:end#1#0`・`a:end#1#1`)。truck は以前、同じ平面の元の面のうち先に見つけた名前を
/// 継いだので、殻の並び(実行ごとに変わる)により `a:end#0#…` と `a:end#1#…` が入れ替わった。
pub fn pieces_of_a_split_face_keep_their_names<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let mut seen: Vec<Vec<(String, [i64; 2])>> = Vec::new();
    for _ in 0..5 {
        let a = cuboid(k, [0.0; 3], [30.0, 20.0, 10.0], "a");
        let slot = cuboid(k, [-1.0, 8.0, 5.0], [31.0, 12.0, 11.0], "s");
        let r = k.boolean(BooleanOp::Subtract, a, slot).unwrap();
        let notch = cuboid(k, [12.0, 11.0, 7.0], [18.0, 21.0, 11.0], "n");
        let r = k.boolean(BooleanOp::Subtract, r, notch).unwrap();
        let m = k.mesh(r, 0.1).unwrap();
        // 上の面(a:end…)ごとの外形の箱の中心(0.1 mm に丸める)
        let mut top: Vec<(String, [i64; 2])> = Vec::new();
        for (fi, name) in m.face_names.iter().enumerate() {
            if !name.starts_with("a:end") {
                continue;
            }
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for (t, _) in m
                .triangles
                .iter()
                .zip(&m.triangle_faces)
                .filter(|(_, f)| **f as usize == fi)
            {
                for &v in t {
                    let p = m.positions[v as usize];
                    for i in 0..2 {
                        lo[i] = lo[i].min(p[i]);
                        hi[i] = hi[i].max(p[i]);
                    }
                }
            }
            let c = [0, 1].map(|i| ((lo[i] + hi[i]) * 5.0).round() as i64);
            top.push((name.clone(), c));
        }
        top.sort();
        seen.push(top);
    }
    let want = vec![
        ("a:end#0".to_string(), [150, 40]),
        ("a:end#1#0".to_string(), [60, 160]),
        ("a:end#1#1".to_string(), [240, 160]),
    ];
    for s in &seen {
        assert_eq!(*s, want);
    }
}

/// 閉じた空洞(中に浮いた穴)のある箱を、空洞に触れない所で切り取る・交わらせる・足す。
/// 空洞の殻は内向きなので、向きを見ずに内外を数えると空洞の外を「中身でない」と読み、
/// 結果が空になったり本体が消えたりする。
pub fn boolean_beside_a_sealed_void_keeps_the_body<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let solid = cuboid(k, [0.0; 3], [40.0, 40.0, 20.0], "a");
    let void = cuboid(k, [10.0, 10.0, 5.0], [20.0, 20.0, 15.0], "v");
    let body = k.boolean(BooleanOp::Subtract, solid, void).unwrap();
    let v0 = 40.0 * 40.0 * 20.0 - 1000.0;
    assert!(
        close(volume(k, body), v0, 1e-6),
        "空洞: {}",
        volume(k, body)
    );
    // 右の縁を 10 mm 落とす(空洞には触れない)
    let edge = cuboid(k, [30.0, -5.0, 10.0], [50.0, 45.0, 25.0], "t");
    let cut = k.boolean(BooleanOp::Subtract, body, edge).unwrap();
    let want = v0 - 10.0 * 40.0 * 10.0;
    assert!(
        close(volume(k, cut), want, 1e-6),
        "切り取り: {} vs {want}",
        volume(k, cut)
    );
    let common = k.boolean(BooleanOp::Intersect, body, edge).unwrap();
    assert!(
        close(volume(k, common), 4000.0, 1e-6),
        "交差: {} vs 4000",
        volume(k, common)
    );
    let joined = k.boolean(BooleanOp::Union, body, edge).unwrap();
    let want = v0 + 20.0 * 50.0 * 15.0 - 4000.0;
    assert!(
        close(volume(k, joined), want, 1e-6),
        "結合: {} vs {want}",
        volume(k, joined)
    );
    // 外形の箱が本体を覆う道具で上を 3 mm 落とす(空洞は z 5〜15 で残る)
    let top = cuboid(k, [-100.0, -100.0, 17.0], [100.0, 100.0, 100.0], "u");
    let cut = k.boolean(BooleanOp::Subtract, body, top).unwrap();
    let want = 40.0 * 40.0 * 17.0 - 1000.0;
    assert!(
        close(volume(k, cut), want, 1e-6),
        "覆う道具: {} vs {want}",
        volume(k, cut)
    );
}

/// 回転体(半径 1〜24、高さ 60 の管)の中に閉じた輪の空洞(半径 3〜5、z 29〜31)を作り、下の縁を
/// 切り取る・交わらせる(軸に触れる回転体は truck が空洞と関係なく
/// 切り取れないので、軸に細い穴を通す)。回転体の網の誤差は空洞より大きいので、空洞の無い同じ形の
/// 結果から空洞の体積を引いた値と比べる(網の作り直しで 1e-6 ほどずれるので 1e-5。空洞は 9e-4)。
pub fn revolved_body_with_a_void_cuts_beside_it<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let tau = std::f64::consts::TAU;
    // ZX 平面(x 軸が z、y 軸が x)で z 0〜60、半径 1〜24
    let rect = |z0: f64, r0: f64, h: f64, w: f64| Profile {
        plane: Plane::ZX,
        regions: vec![Region {
            outer: rect_loop(z0, r0, h, w, 1),
            holes: vec![],
        }],
    };
    let z = [0.0, 0.0, 1.0];
    let cylinder = k
        .revolve(&rect(0.0, 0.0, 60.0, 24.0), [0.0; 3], z, tau, "b")
        .unwrap();
    let void = k
        .revolve(&rect(29.0, 0.0, 2.0, 2.0), [0.0; 3], z, tau, "v")
        .unwrap();
    let body = k.boolean(BooleanOp::Subtract, cylinder, void).unwrap();
    let vv = volume(k, void);
    assert!(
        close(volume(k, body), volume(k, cylinder) - vv, 1e-5),
        "空洞: {} vs {}",
        volume(k, body),
        volume(k, cylinder) - vv
    );
    // 側面の上の箱: 弦 y = 15 より外、z 10〜20(truck は回転体の底の面と交わる切り取りが苦手で、空洞と関係なく失敗する)
    let edge = cuboid(k, [-30.0, 15.0, 10.0], [30.0, 30.0, 20.0], "t");
    let plain = k.boolean(BooleanOp::Subtract, cylinder, edge).unwrap();
    let cut = k.boolean(BooleanOp::Subtract, body, edge).unwrap();
    let want = volume(k, plain) - vv;
    assert!(
        close(volume(k, cut), want, 1e-5),
        "切り取り: {} vs {want}",
        volume(k, cut)
    );
    let plain = k.boolean(BooleanOp::Intersect, cylinder, edge).unwrap();
    let common = k.boolean(BooleanOp::Intersect, body, edge).unwrap();
    assert!(
        volume(k, plain) > 100.0 && close(volume(k, common), volume(k, plain), 1e-5),
        "交差: {} vs {}",
        volume(k, common),
        volume(k, plain)
    );
}

pub fn revolve_ring<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    // ZX 平面(x 軸が z、y 軸が x)で、半径 10〜15、高さ 0〜10 の長方形を z 軸まわりに回す
    let profile = Profile {
        plane: Plane::ZX,
        regions: vec![Region {
            outer: rect_loop(0.0, 10.0, 10.0, 5.0, 1),
            holes: vec![],
        }],
    };
    let tau = std::f64::consts::TAU;
    let h = k
        .revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], tau, "f1")
        .unwrap();
    let m = k.mass(h).unwrap();
    let expect = std::f64::consts::PI * (225.0 - 100.0) * 10.0;
    // 回転面の網は truck の許容差どおりに細かくならず、体積の誤差は 0.1% 程度(実測)
    assert!(close(m.volume, expect, 2e-3), "{} vs {expect}", m.volume);
    let names = face_names(k, h);
    assert!(
        names.iter().all(|n| n.starts_with("f1:side:e")),
        "{names:?}"
    );
    // 半周なら始まりと終わりの面ができる
    let h = k
        .revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], tau / 2.0, "f2")
        .unwrap();
    let m = k.mass(h).unwrap();
    assert!(
        close(m.volume, expect / 2.0, 2e-3),
        "{} vs {}",
        m.volume,
        expect / 2.0
    );
    let names = face_names(k, h);
    assert!(names.contains(&"f2:start".to_string()), "{names:?}");
    assert!(names.contains(&"f2:end".to_string()), "{names:?}");
}

pub fn revolve_rejects_crossing_the_axis<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let profile = Profile {
        plane: Plane::ZX,
        regions: vec![Region {
            outer: rect_loop(0.0, -5.0, 10.0, 10.0, 1),
            holes: vec![],
        }],
    };
    let err = k
        .revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], 1.0, "f1")
        .unwrap_err();
    assert!(err.to_string().contains("またいで"), "{err}");
}

pub fn transform_mesh_step_and_retain<K: Kernel>(k: &K, q: &KernelQuirks<K>) {
    let a = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let b = k
        .transform(a, &kmb_core::kernel::translation([100.0, 0.0, 0.0]), "@1")
        .unwrap();
    let m = k.mass(b).unwrap();
    assert!(close(m.min[0], 100.0, 1e-6));
    assert!(k.faces(b).unwrap().iter().any(|f| f.name == "f1:end@1"));
    let mesh = k.mesh(b, 0.05).unwrap();
    assert_eq!(mesh.triangles.len(), mesh.triangle_faces.len());
    assert!(!mesh.triangles.is_empty());
    // 2 回目は作った網を返す。消した形の網は返さない
    assert_eq!(k.mesh(b, 0.05).unwrap(), mesh);
    assert!(k.mesh(a, 0.05).is_ok());
    let step = k.export_step(&[a, b]).unwrap();
    assert!(step.starts_with("ISO-10303-21;"), "{}", &step[..40]);
    assert_eq!((q.shape_count)(k), 2);
    k.retain(&[b].into_iter().collect());
    assert_eq!((q.shape_count)(k), 1);
    assert!(matches!(k.mass(a), Err(KernelError::UnknownShape(_))));
    assert!(matches!(k.mesh(a, 0.05), Err(KernelError::UnknownShape(_))));
    assert_eq!(k.mesh(b, 0.05).unwrap(), mesh);
}

pub fn transform_refuses_a_matrix_that_is_not_a_placement<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    // 遠近の成分・つぶれた向き・数でない値の行列は断る。鏡映と拡大は通す
    let a = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let id = kmb_core::kernel::IDENTITY;
    let mut perspective = id;
    perspective[3][2] = 0.01;
    let mut flat = id;
    flat[2][2] = 0.0;
    let mut nan = id;
    nan[0][3] = f64::NAN;
    for m in [perspective, flat, nan] {
        assert!(k.transform(a, &m, "@x").is_err(), "{m:?}");
    }
    let mut mirror = id;
    mirror[0][0] = -1.0;
    let v = k
        .mass(k.transform(a, &mirror, "@m").unwrap())
        .unwrap()
        .volume;
    assert!(close(v, k.mass(a).unwrap().volume, 1e-6), "{v}");
    let twice = [
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 2.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    assert!(k.transform(a, &twice, "@2").is_ok());
}

pub fn straight_edges_between_planes_know_if_they_are_convex<K: Kernel>(
    k: &K,
    _: &KernelQuirks<K>,
) {
    // L 字: (20, 10) の角だけがへこんでいる
    let l = polygon(&[
        [0.0, 0.0],
        [30.0, 0.0],
        [30.0, 10.0],
        [20.0, 10.0],
        [20.0, 20.0],
        [0.0, 20.0],
    ]);
    let h = k.extrude(&l, 0.0, 5.0, "f1").unwrap();
    let edges = k.edges(h).unwrap();
    assert_eq!(edges.len(), 18, "{edges:?}");
    let concave: Vec<_> = edges.iter().filter(|e| !e.convex).collect();
    assert_eq!(concave.len(), 1, "{concave:?}");
    assert_eq!(
        concave[0].faces,
        ["f1:side:e3".to_string(), "f1:side:e4".to_string()]
    );
    let (a, b) = (concave[0].a, concave[0].b);
    assert!(close(a[0], 20.0, 1e-9) && close(a[1], 10.0, 1e-9) && close(b[1], 10.0, 1e-9));
    assert!(close((a[2] - b[2]).abs(), 5.0, 1e-9));
}

/// どの辺も、両側の面が角を作るか(丸められる辺)、接していて角の無い継ぎ目かが分かっている
/// (接してつながる辺のたどりが、分からない辺の上を黙ってたどらないため)。両側が平らな面なら、
/// 向きが同じ時だけ継ぎ目、ほかは角。形は高さと幅の違う 2 つの箱を重ねて足した物(同じ平面に乗る
/// 面を作らない。truck は同じ平面の面どうしの足し算に失敗する)。
pub fn edges_know_whether_they_are_corners_or_smooth_seams<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let a = k
        .extrude(&rect_profile(0.0, 0.0, 10.0, 10.0, 1), 0.0, 5.0, "a")
        .unwrap();
    let b = k
        .extrude(&rect_profile(8.0, 2.0, 10.0, 6.0, 5), -1.0, 8.0, "b")
        .unwrap();
    let h = k.boolean(BooleanOp::Union, a, b).unwrap();
    let faces = k.faces(h).unwrap();
    let normal = |name: &str| {
        faces
            .iter()
            .find(|f| f.name == name)
            .and_then(|f| f.planar)
            .map(|p| p.normal)
    };
    let mut all = k.edges(h).unwrap();
    all.extend(k.arc_edges(h).unwrap());
    all.extend(k.curve_edges(h).unwrap());
    assert!(!all.is_empty());
    for e in &all {
        assert_ne!(e.seam, Seam::Unknown, "{e:?}");
        if let (Some(na), Some(nb)) = (normal(&e.faces[0]), normal(&e.faces[1])) {
            let parallel = kmb_core::kernel::dot(na, nb) > 0.999;
            let want = if parallel { Seam::Smooth } else { Seam::Corner };
            assert_eq!(e.seam, want, "{e:?}");
        }
    }
}

/// 自分で書き出した STEP を読み直すと同じ形になる。truck は網のボディ、SMLib は立体になる
/// (試験の名前は網だけだった頃のまま)。
pub fn a_step_written_by_us_reads_back_as_a_mesh_body<K: Kernel>(k: &K, q: &KernelQuirks<K>) {
    let a = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let before = k.mass(a).unwrap();
    let step = k.export_step(&[a]).unwrap();
    let b = k.import_step(&step).unwrap();
    let after = k.mass(b).unwrap();
    assert!(
        close(after.volume, before.volume, 2e-3),
        "体積 {} 元 {}",
        after.volume,
        before.volume
    );
    assert!(close(after.area, before.area, 2e-3), "{}", after.area);
    for i in 0..3 {
        assert!((after.min[i] - before.min[i]).abs() < 1e-3);
        assert!((after.max[i] - before.max[i]).abs() < 1e-3);
    }
    let faces = k.faces(b).unwrap();
    assert!(faces.iter().all(|f| f.name.starts_with("step0:f")));
    // 上面は平面として分かる
    assert!(faces.iter().any(|f| {
        f.planar
            .is_some_and(|p| p.normal[2] > 0.999 && (p.point[2] - 10.0).abs() < 1e-6)
    }));
    let mesh = k.mesh(b, 0.05).unwrap();
    assert_eq!(mesh.triangles.len(), mesh.triangle_faces.len());
    if q.step_is_solid {
        // 立体なので足し引きでき、元の形と重ねても体積は変わらない
        assert!(!k.edges(b).unwrap().is_empty());
        let u = k.boolean(BooleanOp::Union, b, a).unwrap();
        assert!(close(k.mass(u).unwrap().volume, before.volume, 2e-3));
    } else {
        // 網の形は足し引きできない(理由を言う)
        let err = k.boolean(BooleanOp::Union, b, a).unwrap_err();
        assert!(err.to_string().contains("足し引き"), "{err}");
        assert!(k.edges(b).unwrap().is_empty());
    }
    k.retain(&[a].into_iter().collect());
    assert!(matches!(k.mass(b), Err(KernelError::UnknownShape(_))));
}

/// JC が作る色々な形(穴の板・ポケット・空洞・回転の輪・半周の回転・円すい台)を STEP に書いて
/// 読み直すと、体積と外形の箱が元と同じ(立体に組めるカーネルでは面の数も)。読み手を直すたびに
/// 読む力が崩れないよう守る。空洞は殻の向きの印(ORIENTED_CLOSED_SHELL の .T./.F.)を逆にした物も
/// 読む(書いた CAD によって印の付け方が違い、印で裏返すと空洞が中身として足された)。
/// truck は網で読み、ポケットの読み直しが 60 秒で終わらず、回転の輪の体積が倍(内側の面が裏向き)に
/// なる(2026-10-25 に確かめた)。直すまで契約の一覧には入れず、SMLib の kernel.rs から呼ぶ
pub fn shapes_we_make_read_back_from_step<K: Kernel>(k: &K, q: &KernelQuirks<K>) {
    let tau = std::f64::consts::TAU;
    let z = [0.0, 0.0, 1.0];
    let on_zx = |outer: ProfileLoop| Profile {
        plane: Plane::ZX,
        regions: vec![Region {
            outer,
            holes: vec![],
        }],
    };
    let plate = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let pocket = {
        let p = k.extrude(&plate_profile(), 0.0, 10.0, "f2").unwrap();
        let c = cuboid(k, [5.0, 5.0, 6.0], [20.0, 35.0, 11.0], "p");
        k.boolean(BooleanOp::Subtract, p, c).unwrap()
    };
    let hollow = {
        let a = cuboid(k, [0.0; 3], [40.0; 3], "a");
        let b = cuboid(k, [10.0; 3], [30.0; 3], "b");
        k.boolean(BooleanOp::Subtract, a, b).unwrap()
    };
    let ring = k
        .revolve(
            &on_zx(rect_loop(0.0, 10.0, 10.0, 5.0, 1)),
            [0.0; 3],
            z,
            tau,
            "r",
        )
        .unwrap();
    let half = k
        .revolve(
            &on_zx(rect_loop(0.0, 10.0, 10.0, 5.0, 1)),
            [0.0; 3],
            z,
            tau / 2.0,
            "h",
        )
        .unwrap();
    let line = |a: [f64; 2], b: [f64; 2], name: &str| ProfileEdge {
        name: name.into(),
        segment: Segment::Line { a, b },
        reversed: false,
    };
    let cone = k
        .revolve(
            &on_zx(ProfileLoop {
                edges: vec![
                    line([0.0, 0.0], [0.0, 8.0], "e1"),
                    line([0.0, 8.0], [12.0, 3.0], "e2"),
                    line([12.0, 3.0], [12.0, 0.0], "e3"),
                    line([12.0, 0.0], [0.0, 0.0], "e4"),
                ],
            }),
            [0.0; 3],
            z,
            tau,
            "c",
        )
        .unwrap();
    for (what, h) in [
        ("穴の板", plate),
        ("ポケット", pocket),
        ("空洞", hollow),
        ("回転の輪", ring),
        ("半周の回転", half),
        ("円すい台", cone),
    ] {
        let before = k.mass(h).unwrap();
        let step = k.export_step(&[h]).unwrap();
        let mut texts = vec![step.clone()];
        if what == "空洞" {
            assert!(step.contains("BREP_WITH_VOIDS"), "空洞のある形として書く");
            let flags: Vec<&str> = step
                .lines()
                .filter(|l| l.contains("ORIENTED_CLOSED_SHELL"))
                .collect();
            assert_eq!(flags.len(), 1, "{flags:?}");
            let (from, to) = if flags[0].contains(".T.") {
                (".T.", ".F.")
            } else {
                (".F.", ".T.")
            };
            texts.push(step.replace(flags[0], &flags[0].replace(from, to)));
        }
        for text in &texts {
            let back = k
                .import_step(text)
                .unwrap_or_else(|e| panic!("{what}: {e}"));
            let after = k.mass(back).unwrap();
            // truck は網で読むので、曲面の折れ線の分だけずれる(実測 0.1% 程度)
            assert!(
                close(after.volume, before.volume, 2e-3),
                "{what}: 体積 {} 元 {}",
                after.volume,
                before.volume
            );
            for i in 0..3 {
                assert!(
                    (after.min[i] - before.min[i]).abs() < 1e-3
                        && (after.max[i] - before.max[i]).abs() < 1e-3,
                    "{what}: 箱 {:?}..{:?} 元 {:?}..{:?}",
                    after.min,
                    after.max,
                    before.min,
                    before.max
                );
            }
            if q.step_is_solid {
                assert_eq!(
                    k.faces(back).unwrap().len(),
                    k.faces(h).unwrap().len(),
                    "{what}: 面の数"
                );
            }
        }
    }
}

/// 長さの単位が inch や m の STEP は、ファイルの数を mm に直して読む(数をそのまま mm と見ると
/// 25.4 分の 1・1000 分の 1 の大きさになる)。自分で書いた mm の STEP の単位だけを書き替えて確かめる。
pub fn a_step_in_inch_or_metre_reads_at_its_true_size<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let a = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let before = k.mass(a).unwrap();
    let step = k.export_step(&[a]).unwrap();
    let mm = "SI_UNIT(.MILLI.,.METRE.)";
    assert_eq!(step.matches(mm).count(), 1, "mm の単位を 1 つだけ書く");
    // inch は換算の量(25.4 mm)を添えた単位で書く。足す実体はデータの節の終わりに置く
    let inch = step.replacen(mm, "CONVERSION_BASED_UNIT('INCH',#999001)", 1);
    let end = inch.rfind("ENDSEC;").unwrap();
    let inch = format!(
        "{}#999001=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#999002);\n\
         #999002=(LENGTH_UNIT() NAMED_UNIT(*) {mm});\n{}",
        &inch[..end],
        &inch[end..]
    );
    // m の読み方(接頭辞の無い SI 単位)は kmb-format の試験で確かめる。網で読むカーネルは m の小さな
    // 数の網づくりが遅い(数秒)ので、ここでは cm で SI の接頭辞の掛け方を確かめる
    for (text, scale) in [
        (step.replace(mm, "SI_UNIT(.CENTI.,.METRE.)"), 10.0),
        (inch, 25.4),
    ] {
        let b = k.import_step(&text).unwrap();
        let after = k.mass(b).unwrap();
        let want = before.volume * scale * scale * scale;
        assert!(
            close(after.volume, want, 2e-3),
            "{scale} 倍: 体積 {} 元 {want}",
            after.volume
        );
        for i in 0..3 {
            let want = before.max[i] * scale;
            assert!(
                (after.max[i] - want).abs() < 1e-3 * scale,
                "{scale} 倍: {:?}",
                after.max
            );
        }
    }
}

/// 公差つきの STEP によくある書き方: 立体と一緒に面だけのボディ(OPEN_SHELL)が置かれ、注記の実体に
/// 空の並び `()` がある。立体だけを、面の名前を変えずに読む。形の実体が壊れていれば、誤りがその行を指す。
pub fn a_step_with_surface_bodies_reads_only_its_solid<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    let a = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let before = k.mass(a).unwrap();
    let step = k.export_step(&[a]).unwrap();
    let plain = k.import_step(&step).unwrap();
    // 面だけのボディは立体の殻の最初の面を指す(足す実体はデータの節の終わりに置く)
    let at = step.find("CLOSED_SHELL").unwrap();
    let face = &step[at + step[at..].find('#').unwrap() + 1..];
    let face = &face[..face.find(|c: char| !c.is_ascii_digit()).unwrap()];
    let end = step.rfind("ENDSEC;").unwrap();
    let text = format!(
        "{}#999001=OPEN_SHELL('',(#{face}));\n\
         #999002=SHELL_BASED_SURFACE_MODEL('',(#999001));\n\
         #999003=STYLED_ITEM('',(),#999002);\n{}",
        &step[..end],
        &step[end..]
    );
    let b = k.import_step(&text).unwrap();
    let after = k.mass(b).unwrap();
    assert!(
        close(after.volume, before.volume, 1e-3),
        "体積 {} 元 {}",
        after.volume,
        before.volume
    );
    assert_eq!(face_names(k, b), face_names(k, plain));
    // 形の実体(点)を壊すと、誤りはその行を指す
    let at = text.find("CARTESIAN_POINT").unwrap();
    let arg = at + text[at..].find("',").unwrap() + 2;
    let broken = format!("{}?{}", &text[..arg], &text[arg..]);
    let line = text[..at].matches('\n').count() + 1;
    let err = k.import_step(&broken).unwrap_err().to_string();
    assert!(
        err.contains(&format!("{line} 行目")),
        "{line} 行目を指さない: {err}"
    );
}

pub fn broken_step_is_an_error_not_a_panic<K: Kernel>(k: &K, _: &KernelQuirks<K>) {
    assert!(k.import_step("ISO-10303-21;\nこわれた").is_err());
    assert!(k.import_step("").is_err());
}

pub fn step_files_from_another_cad_read_with_the_same_volume<K: Kernel>(
    k: &K,
    _: &KernelQuirks<K>,
) {
    for (file, volume, area) in [
        ("reference.step", 22806.876, 7678.499),
        ("reference-fillet.step", 22772.540, 7637.296),
    ] {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../kmb-kernel-truck/testdata/")
            .to_string()
            + file;
        let step = std::fs::read_to_string(&path).unwrap();
        let h = k.import_step(&step).unwrap();
        let m = k.mass(h).unwrap();
        assert!(close(m.volume, volume, 1e-3), "{file}: 体積 {}", m.volume);
        assert!(close(m.area, area, 1e-3), "{file}: 表面積 {}", m.area);
        assert!((m.max[0] - 60.0).abs() < 1e-3 && (m.max[2] - 15.0).abs() < 1e-3);
        // 面の境目の辺も網に入る(画面に辺を描き、測る道具で点と辺を拾う)。
        // 外形の箱の 12 本より多く、どの辺も外形の箱の中にある
        let mesh = k.mesh(h, 0.05).unwrap();
        assert!(mesh.edges.len() > 12, "{file}: 辺 {}", mesh.edges.len());
        for p in mesh.edges.iter().flatten() {
            for i in 0..3 {
                let v = p[i] as f64;
                assert!(
                    v >= m.min[i] - 1e-3 && v <= m.max[i] + 1e-3,
                    "{file}: {p:?}"
                );
            }
        }
    }
}

/// 長い板(3000 mm)の小さな穴(半径 2)は、板の上の面が溝で分かれても穴のまま(
/// 足し引きの許しが大きい方の形に合わせて大きくなり、小さな穴の縁が面積の無い輪になって、穴と
/// 面の切れ端の見分けが崩れた)。穴の周りを箱で切り出すと穴が残り、穴に合うピンを足すと埋まる。
/// truck は同じ誤りを持ち(切り出しが穴の無い 4000 mm³ になる)、直すには third_party の truck の
/// 写しへの手当てが要るので、契約の一覧には入れず SMLib の differences.rs から呼ぶ。
pub fn a_small_hole_in_a_long_plate_stays_a_hole_when_its_face_is_divided<K: Kernel>(k: &K) {
    use std::f64::consts::PI;
    let (len, r) = (3000.0, 2.0);
    let plate = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(0.0, 0.0, len, 100.0, 1),
            holes: vec![circle_loop([1500.0, 50.0], r, "e9", true)],
        }],
    };
    let p = k.extrude(&plate, 0.0, 10.0, "p").unwrap();
    // 穴の横の溝で上の面を 2 つに分ける
    let groove = cuboid(k, [1400.0, -1.0, 8.0], [1410.0, 101.0, 11.0], "g");
    let p = k.boolean(BooleanOp::Subtract, p, groove).unwrap();
    let hole = PI * r * r * 10.0;
    let want = len * 100.0 * 10.0 - hole - 10.0 * 100.0 * 2.0;
    assert!(
        close(volume(k, p), want, 1e-6),
        "{} vs {want}",
        volume(k, p)
    );
    // 穴の周りを切り出す
    let around = cuboid(k, [1490.0, 40.0, -1.0], [1510.0, 60.0, 11.0], "b");
    let piece = k.boolean(BooleanOp::Intersect, p, around).unwrap();
    let want = 20.0 * 20.0 * 10.0 - hole;
    assert!(
        close(volume(k, piece), want, 1e-6),
        "切り出し {} vs {want}",
        volume(k, piece)
    );
    // 穴に合うピン(上下に 1 mm ずつ出る)を足すと穴が埋まる
    let pin = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: circle_loop([1500.0, 50.0], r, "c1", false),
            holes: vec![],
        }],
    };
    let pin = k.extrude(&pin, -1.0, 11.0, "pin").unwrap();
    let joined = k.boolean(BooleanOp::Union, piece, pin).unwrap();
    let want = 20.0 * 20.0 * 10.0 + PI * r * r * 2.0;
    assert!(
        close(volume(k, joined), want, 1e-6),
        "ピン {} vs {want}",
        volume(k, joined)
    );
}

/// 交わり(共通部分)で、少しだけはみ出す角を切り落とす(確かめられない交わりを
/// 片方のまま返し、削るはずの歯が 0.27 mm 残った)。半径 10 の円柱と、角が 0.27 mm はみ出す四角の柱の
/// 共通部分は、四角から 4 つの角を円で切り落とした形になる(四角のまま返さない)。
pub fn an_intersection_trims_corners_that_stick_out_a_little<K: Kernel>(
    k: &K,
    _: &KernelQuirks<K>,
) {
    let (r, h) = (10.0_f64, 10.0);
    let a = (r + 0.27) / 2f64.sqrt();
    let cyl = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: circle_loop([0.0, 0.0], r, "c1", false),
            holes: vec![],
        }],
    };
    let cyl = k.extrude(&cyl, 0.0, h, "c").unwrap();
    let sq = cuboid(k, [-a, -a, -1.0], [a, a, h + 1.0], "s");
    let i = k.boolean(BooleanOp::Intersect, cyl, sq).unwrap();
    // 4 分の 1(0 ≤ x, y ≤ a)の面積: x < b は高さ a、b < x < a は円の下
    let b = (r * r - a * a).sqrt();
    let prim = |x: f64| (x * (r * r - x * x).sqrt() + r * r * (x / r).asin()) / 2.0;
    let want = 4.0 * (a * b + prim(a) - prim(b)) * h;
    // truck の体積は網から出すので 3e-5 ほどずれる(SMLib は 1e-9 で合う)
    let got = volume(k, i);
    assert!(close(got, want, 1e-4), "{got} vs {want}");
    assert!(got < 4.0 * a * a * h - 1.0, "角が残った: {got}");
}

/// 表示の網で、穴を切り抜いた面の三角形は面の上に留まる(切り抜いた面の三角形が
/// 穴を横切ったり、面から浮いたりした)。60×40 の板の半径 4 の穴で、穴の側面の三角形の頂点は
/// 半径 4 の上、中心は網の許しの内、上の面の三角形はすべて z = 10 で、面積の和は穴を除いた面積
/// (穴を横切る三角形があれば穴の分だけ増える)。
pub fn a_display_mesh_of_a_face_with_a_hole_stays_on_it<K: Kernel>(k: &K, q: &KernelQuirks<K>) {
    let h = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    for tol in [0.1, 0.01] {
        let m = k.mesh(h, tol).unwrap();
        let at = |v: u32| m.positions[v as usize].map(f64::from);
        let radius = |p: [f64; 3]| ((p[0] - 30.0).powi(2) + (p[1] - 20.0).powi(2)).sqrt();
        let (mut side, mut top, mut area) = (0, 0, 0.0);
        for (t, &f) in m.triangles.iter().zip(&m.triangle_faces) {
            let name = m.face_names[f as usize].as_str();
            let p = t.map(at);
            let c: [f64; 3] = [0, 1, 2].map(|i| (p[0][i] + p[1][i] + p[2][i]) / 3.0);
            if q.hole_side_faces.contains(&name) {
                side += 1;
                for v in p {
                    assert!((radius(v) - 4.0).abs() < 1e-4, "{tol}: 穴の頂点 {v:?}");
                }
                assert!(
                    (4.0 - radius(c)) <= tol * 1.01 && radius(c) <= 4.0 + 1e-4,
                    "{tol}: 穴の三角形の中心 {c:?}"
                );
            } else if name == "f1:end" {
                top += 1;
                for v in p {
                    assert!((v[2] - 10.0).abs() < 1e-5, "{tol}: 上の面の頂点 {v:?}");
                }
                let (u, v) = (
                    kmb_core::kernel::sub(p[1], p[0]),
                    kmb_core::kernel::sub(p[2], p[0]),
                );
                area += (u[0] * v[1] - u[1] * v[0]).abs() / 2.0;
            }
        }
        assert!(side > 0 && top > 0, "{tol}: 側面 {side}、上の面 {top}");
        // 穴の多角形は円の内側なので、面積は円で抜いた値より最大で円周 × 許しだけ多い
        let want = 2400.0 - std::f64::consts::PI * 16.0;
        let slack = 2.0 * std::f64::consts::PI * 4.0 * tol;
        assert!(
            area >= want - 1e-3 && area <= want + slack,
            "{tol}: 上の面の面積 {area} vs {want}"
        );
    }
}

/// カーネルの契約を、確かめごとに 1 つのテストとして並べる。
/// `$make` はカーネルを作る関数、`$quirks` は [`KernelQuirks`] の定数(どちらも呼んだモジュールの中の名前)。
pub fn instances_place_the_same_shape_and_fold_into_one_step<K: Kernel>(
    k: &K,
    q: &KernelQuirks<K>,
) {
    use kmb_core::kernel::{mat_mul, rotation, translation};
    let a = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let m = translation([100.0, 0.0, 0.0]);
    let b = k.instance(a, &m).unwrap();
    assert_eq!(k.instance_of(b), Some((a, m)));
    assert_eq!(k.instance_of(a), None);
    // 同じ置き方は同じ出現。動かすだけの移動も出現
    assert_eq!(k.instance(a, &m).unwrap(), b);
    assert_eq!(k.transform(a, &m, "").unwrap(), b);
    // 面の名前・体積・網は元を動かした物と同じ
    assert_eq!(face_names(k, b), face_names(k, a));
    let (va, vb) = (k.mass(a).unwrap(), k.mass(b).unwrap());
    assert!(
        close(vb.volume, va.volume, 1e-6),
        "{} vs {}",
        vb.volume,
        va.volume
    );
    assert!(close(vb.min[0], va.min[0] + 100.0, 1e-6));
    assert_eq!(
        k.mesh(b, 0.05).unwrap().triangles.len(),
        k.mesh(a, 0.05).unwrap().triangles.len()
    );
    // 出現の出現は 1 段に畳む。戻すと元そのもの
    let turn = rotation([0.0; 3], [0.0, 0.0, 1.0], 0.5);
    let c = k.instance(b, &turn).unwrap();
    let (base, cm) = k.instance_of(c).unwrap();
    assert_eq!(base, a);
    let want = mat_mul(&turn, &m);
    assert!(
        cm.iter()
            .flatten()
            .zip(want.iter().flatten())
            .all(|(x, y)| close(*x, *y, 1e-12))
    );
    assert_eq!(k.instance(b, &translation([-100.0, 0.0, 0.0])).unwrap(), a);
    // 鏡映も置ける(体積は同じ)。拡大は出現にしない
    let mut mirror = kmb_core::kernel::IDENTITY;
    mirror[0][0] = -1.0;
    let d = k.instance(a, &mirror).unwrap();
    assert!(close(k.mass(d).unwrap().volume, va.volume, 1e-6));
    let mut twice = kmb_core::kernel::IDENTITY;
    twice[0][0] = 2.0;
    assert!(k.instance(a, &twice).is_err());
    assert_eq!(k.instance_of(k.transform(a, &twice, "").unwrap()), None);
    // 名前を足す移動(パターンの写し)は出現にしない
    assert_eq!(k.instance_of(k.transform(a, &m, "@1").unwrap()), None);
    // 出現を足し引きすると普通の形になる
    let tool = k
        .extrude(&rect_profile(105.0, 5.0, 10.0, 10.0, 20), 5.0, 10.01, "f2")
        .unwrap();
    let cut = k.boolean(BooleanOp::Subtract, b, tool).unwrap();
    assert_eq!(k.instance_of(cut), None);
    // 出現が生きている間は元も残す
    k.retain(&[b].into_iter().collect());
    assert_eq!((q.shape_count)(k), 2);
    assert_eq!(k.instance_of(b), Some((a, m)));
    assert!(k.mass(a).is_ok());
}

#[macro_export]
macro_rules! kernel_contract_tests {
    ($make:ident, $quirks:ident) => {
        $crate::kernel_contract_tests!(@each $make, $quirks;
            extrude_plate_with_hole,
            corners_are_named_by_their_faces,
            extrude_downward_is_still_outward,
            pocket_and_names_survive_boolean,
            disjoint_subtract_keeps_the_target,
            boolean_beside_a_sealed_void_keeps_the_body,
            pieces_of_a_split_face_keep_their_names,
            revolved_body_with_a_void_cuts_beside_it,
            revolve_ring,
            revolve_rejects_crossing_the_axis,
            transform_mesh_step_and_retain,
            transform_refuses_a_matrix_that_is_not_a_placement,
            instances_place_the_same_shape_and_fold_into_one_step,
            straight_edges_between_planes_know_if_they_are_convex,
            edges_know_whether_they_are_corners_or_smooth_seams,
            a_step_written_by_us_reads_back_as_a_mesh_body,
            broken_step_is_an_error_not_a_panic,
            a_step_in_inch_or_metre_reads_at_its_true_size,
            a_step_with_surface_bodies_reads_only_its_solid,
            step_files_from_another_cad_read_with_the_same_volume,
            an_intersection_trims_corners_that_stick_out_a_little,
            a_display_mesh_of_a_face_with_a_hole_stays_on_it,
        );
    };
    (@each $make:ident, $quirks:ident; $($name:ident,)*) => {
        mod kernel_contract {
            $(
                #[test]
                fn $name() {
                    $crate::kernel::$name(&super::$make(), &super::$quirks)
                }
            )*
        }
    };
}
