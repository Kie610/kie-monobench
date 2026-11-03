//! 今の視点から見たボディの線を、2D の DXF にする。
//!
//! 線は網の稜線(面の境目)と、曲面の輪郭(視線に対して表と裏が入れ替わる所)。視線に沿った
//! 平行投影で、1 mm を 1 単位(mm)のまま書く(レーザー加工の下地にそのまま使える大きさ)。
//! 見えるかどうかは、線を短く刻んだ各区間の真ん中の点で、手前に三角形があるかで決める。
//! 隠れ線は省くか、破線の画層 `HIDDEN` に分ける。
//!
//! 書き出しは DXF R12(AC1009)の最小の形(HEADER・TABLES・ENTITIES)。線分は LINE、つながる線は POLYLINE。

#[cfg(test)]
use kmb_core::kernel::dot;
use kmb_core::kernel::{Mesh, P3};
use kmb_format::dxf;

#[cfg(test)]
use kmb_view_lines::silhouettes;
pub use kmb_view_lines::{View, runs_bounds, view_runs};

/// 書き出しの結果(DXF の文字列と、見える線・隠れ線の LINE・POLYLINE の数)。
#[derive(Debug, Clone, PartialEq)]
pub struct ViewDxf {
    pub dxf: String,
    pub visible: usize,
    pub hidden: usize,
}

/// 数を短く書く(`-0` は `0`)。
fn num(v: f64) -> String {
    let v = if v.abs() < 5e-7 { 0.0 } else { v };
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    }
}

/// 網 `meshes` とスケッチの線 `sketches` を視点 `view` から見た線を DXF にする。`hidden` が真なら
/// 隠れ線も破線の画層に書く。`chord` は網にしたときの許容差 mm(曲面の縁の線と面の網のずれを、
/// 隠れの判定で見込む)。
pub fn view_to_dxf(
    meshes: &[Mesh],
    sketches: &[Vec<P3>],
    view: &View,
    hidden: bool,
    chord: f64,
) -> ViewDxf {
    let runs = view_runs(meshes, sketches, view, chord);
    let (lo, hi) = runs_bounds(&runs);
    let tol = crate::polylines::tolerance(lo, hi);

    let mut o = dxf::R12::begin(
        num,
        lo,
        hi,
        &[
            ("0", 7, dxf::CONTINUOUS),
            ("VISIBLE", 7, dxf::CONTINUOUS),
            ("HIDDEN", 8, dxf::DASHED),
        ],
    );
    let (mut visible, mut hid) = (0, 0);
    for seen in [true, false] {
        if !seen && !hidden {
            continue;
        }
        let layer = if seen { "VISIBLE" } else { "HIDDEN" };
        let polys = crate::polylines::clean(&runs, seen, tol);
        if seen {
            visible = polys.len();
        } else {
            hid = polys.len();
        }
        for poly in polys {
            match poly[..] {
                [a, b] => o.line(layer, a, b),
                _ => o.polyline(layer, &poly),
            }
        }
    }
    ViewDxf {
        dxf: o.finish(),
        visible,
        hidden: hid,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// DXF の各実体から、画層ごとの線分を取り出す。
    fn written_segments(text: &str, layer: &str) -> Vec<[[f64; 2]; 2]> {
        let lines: Vec<_> = text.lines().collect();
        let (chunks, remainder) = lines.as_chunks::<2>();
        assert!(remainder.is_empty(), "DXF の組は番号と値の 2 行");
        let pairs: Vec<_> = chunks
            .iter()
            .map(|p| (p[0].trim().parse::<i32>().unwrap(), p[1]))
            .collect();
        let mut records = Vec::new();
        let mut record = Vec::new();
        for p in pairs {
            if p.0 == 0 && !record.is_empty() {
                records.push(std::mem::take(&mut record));
            }
            record.push(p);
        }
        if !record.is_empty() {
            records.push(record);
        }
        let mut previous = None;
        let mut segments = Vec::new();
        for record in records {
            let field = |code| record.iter().find(|p| p.0 == code).map(|p| p.1);
            let at = |code| {
                [
                    field(code).unwrap().parse::<f64>().unwrap(),
                    field(code + 10).unwrap().parse::<f64>().unwrap(),
                ]
            };
            match record[0].1 {
                "LINE" if field(8) == Some(layer) => segments.push([at(10), at(11)]),
                "POLYLINE" | "SEQEND" => previous = None,
                "VERTEX" if field(8) == Some(layer) => {
                    let point = at(10);
                    if let Some(a) = previous {
                        segments.push([a, point]);
                    }
                    previous = Some(point);
                }
                _ => {}
            }
        }
        segments
    }

    /// 重複や連結の前後で、両画層の線が同じ位置を覆うこと。
    fn assert_projection_is_preserved(
        meshes: &[Mesh],
        sketches: &[Vec<P3>],
        view: &View,
        hidden: bool,
        out: &ViewDxf,
    ) {
        let runs = view_runs(meshes, sketches, view, 0.02);
        let (lo, hi) = runs_bounds(&runs);
        let tol = crate::polylines::tolerance(lo, hi) * 2.0 + 1e-6;
        let near = |p: [f64; 2], [a, b]: [[f64; 2]; 2]| {
            let d = [b[0] - a[0], b[1] - a[1]];
            let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1])
                / (d[0] * d[0] + d[1] * d[1]).max(1e-30))
            .clamp(0.0, 1.0);
            (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1]) <= tol
        };
        for (seen, layer) in [(true, "VISIBLE"), (false, "HIDDEN")] {
            if !seen && !hidden {
                continue;
            }
            let raw: Vec<_> = runs
                .iter()
                .filter(|r| r.1 == seen)
                .flat_map(|r| r.0.windows(2).map(|w| [w[0], w[1]]))
                .collect();
            let written = written_segments(&out.dxf, layer);
            for (from, to) in [(&raw, &written), (&written, &raw)] {
                for [a, b] in from {
                    for t in [0.0, 0.5, 1.0] {
                        let p = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                        assert!(to.iter().any(|s| near(p, *s)), "{layer}: {p:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn overlapping_sketch_lines_are_written_as_one_line() {
        let lines = [
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            vec![[5.0, 0.0, 0.0], [15.0, 0.0, 0.0]],
        ];
        let top = View::named("top").unwrap();
        let out = view_to_dxf(&[], &lines, &top, false, 0.02);
        assert_eq!(out.dxf.matches("\nLINE\n").count(), 1, "{}", out.dxf);
        assert_eq!((out.visible, out.hidden), (1, 0));
        assert!(out.dxf.contains(" 10\n0.0\n 20\n0.0"));
        assert!(out.dxf.contains(" 11\n15.0\n 21\n0.0"));
    }

    #[test]
    fn coincident_visible_and_hidden_lines_keep_their_layers() {
        let m = boxed([0.0; 3], [10.0, 10.0, 2.0]);
        let lines = [
            vec![[2.0, 5.0, 3.0], [8.0, 5.0, 3.0]],
            vec![[2.0, 5.0, -1.0], [8.0, 5.0, -1.0]],
        ];
        let top = View::named("top").unwrap();
        let with = view_to_dxf(std::slice::from_ref(&m), &lines, &top, true, 0.02);
        assert_eq!((with.visible, with.hidden), (2, 1));
        assert_eq!(with.dxf.matches("\nLINE\n").count(), 2);
        for layer in ["VISIBLE", "HIDDEN"] {
            assert!(
                with.dxf.contains(&format!(
                    "\nLINE\n  8\n{layer}\n 10\n2.0\n 20\n5.0\n 30\n0.0\n 11\n8.0\n 21\n5.0"
                )),
                "{layer}: {}",
                with.dxf
            );
        }
        let without = view_to_dxf(&[m], &lines, &top, false, 0.02);
        assert_eq!((without.visible, without.hidden), (2, 0));
        assert_eq!(without.dxf.matches("\nLINE\n").count(), 1);
        assert!(!without.dxf.contains("  8\nHIDDEN\n"));
    }

    /// 軸に平行な箱の網(面ごとに頂点を分け、外向きの巻き。稜線は 12 本)。
    pub(crate) fn boxed(lo: P3, hi: P3) -> Mesh {
        let mut m = Mesh::default();
        let c = |x: usize, y: usize, z: usize| {
            [
                if x == 0 { lo[0] } else { hi[0] },
                if y == 0 { lo[1] } else { hi[1] },
                if z == 0 { lo[2] } else { hi[2] },
            ]
        };
        // 各面の 4 隅(外から見て反時計回り)
        let faces = [
            [c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0)], // 下
            [c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)], // 上
            [c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1)], // 前
            [c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0)], // 後ろ
            [c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0)], // 左
            [c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)], // 右
        ];
        for (k, f) in faces.iter().enumerate() {
            let base = m.positions.len() as u32;
            for p in f {
                m.positions.push(p.map(|v| v as f32));
                m.normals.push([0.0; 3]);
            }
            m.triangles.push([base, base + 1, base + 2]);
            m.triangles.push([base, base + 2, base + 3]);
            m.triangle_faces.extend([k as u32, k as u32]);
            m.face_names.push(format!("f{k}"));
        }
        let e = |a: P3, b: P3| vec![a.map(|v| v as f32), b.map(|v| v as f32)];
        for (a, b) in [
            ((0, 0, 0), (1, 0, 0)),
            ((0, 1, 0), (1, 1, 0)),
            ((0, 0, 1), (1, 0, 1)),
            ((0, 1, 1), (1, 1, 1)),
            ((0, 0, 0), (0, 1, 0)),
            ((1, 0, 0), (1, 1, 0)),
            ((0, 0, 1), (0, 1, 1)),
            ((1, 0, 1), (1, 1, 1)),
            ((0, 0, 0), (0, 0, 1)),
            ((1, 0, 0), (1, 0, 1)),
            ((0, 1, 0), (0, 1, 1)),
            ((1, 1, 0), (1, 1, 1)),
        ] {
            m.edges.push(e(c(a.0, a.1, a.2), c(b.0, b.1, b.2)));
        }
        m
    }

    /// 表示中のスケッチの線も書く。ボディの線と同じく、手前にボディがあれば隠れ線
    #[test]
    fn sketch_lines_are_written_with_the_bodies() {
        let m = boxed([0.0; 3], [40.0, 20.0, 10.0]);
        let top = View::named("top").unwrap();
        // 箱の横の床の線(見える)と、箱の下に隠れた床の線
        let beside = vec![[50.0, 0.0, 0.0], [60.0, 10.0, 0.0]];
        let under = vec![[10.0, 5.0, 0.0], [30.0, 8.0, 0.0]];
        let sketches = [beside, under];
        let r = view_to_dxf(std::slice::from_ref(&m), &sketches, &top, true, 0.02);
        // 箱の輪郭を 1 つの折れ線、横と下のスケッチを各 1 本で書く。
        assert_eq!((r.visible, r.hidden), (2, 1), "{}", r.dxf);
        assert_projection_is_preserved(std::slice::from_ref(&m), &sketches, &top, true, &r);
        assert_eq!(r.hidden, 1, "{}", r.dxf);
        assert!(r.dxf.contains("60.0"), "{}", r.dxf);
        let svg = crate::view_svg::view_to_svg(
            &[m],
            &[vec![[50.0, 0.0, 0.0], [60.0, 10.0, 0.0]]],
            &top,
            false,
            0.02,
        );
        // 箱の輪郭 1 つと、横の線 1 本
        assert_eq!(svg.visible, 2, "{}", svg.svg);
    }

    #[test]
    fn a_box_from_the_top_shows_its_outline_and_hides_nothing_under_it() {
        let m = boxed([0.0; 3], [40.0, 20.0, 10.0]);
        let top = View::named("top").unwrap();
        let r = view_to_dxf(std::slice::from_ref(&m), &[], &top, true, 0.02);
        // 上面と下面で重なる 4 辺は、1 つの閉じた折れ線にする。
        // 縦の 4 本は点に見えるので書かない
        assert_eq!(r.hidden, 0, "{}", r.dxf);
        assert_eq!(r.visible, 1);
        assert_eq!(written_segments(&r.dxf, "VISIBLE").len(), 4);
        assert_projection_is_preserved(std::slice::from_ref(&m), &[], &top, true, &r);
        assert!(r.dxf.contains("$INSUNITS"));
        assert!(r.dxf.contains("40.0"), "1 mm を 1 単位のまま書く");
        assert!(r.dxf.trim_end().ends_with("EOF"));
    }

    #[test]
    fn a_box_from_an_iso_view_has_three_hidden_edges() {
        let m = boxed([0.0; 3], [40.0, 20.0, 10.0]);
        let iso = View::named("iso").unwrap();
        let with = view_to_dxf(std::slice::from_ref(&m), &[], &iso, true, 0.02);
        // 等角の視点からは、奥の角に集まる 3 本の稜線が隠れる
        assert_eq!((with.visible, with.hidden), (3, 2));
        assert_eq!(written_segments(&with.dxf, "HIDDEN").len(), 3);
        assert_eq!(written_segments(&with.dxf, "VISIBLE").len(), 9);
        assert_projection_is_preserved(std::slice::from_ref(&m), &[], &iso, true, &with);
        assert!(with.dxf.contains("HIDDEN") && with.dxf.contains("DASHED"));
        let without = view_to_dxf(&[m], &[], &iso, false, 0.02);
        assert_eq!(without.hidden, 0);
        assert_eq!(without.visible, 3);
        assert_eq!(written_segments(&without.dxf, "VISIBLE").len(), 9);
        assert!(!without.dxf.contains("\n  8\nHIDDEN\n"), "隠れ線を書かない");
    }

    #[test]
    fn a_front_box_hides_the_part_of_a_box_behind_it() {
        // 前の低い箱(高さ 5)が、後ろの高い箱(高さ 20)の下の方を隠す
        let front = boxed([0.0, 0.0, 0.0], [40.0, 10.0, 5.0]);
        let back = boxed([10.0, 20.0, 0.0], [30.0, 30.0, 20.0]);
        let view = View::named("front").unwrap();
        let meshes = [front, back];
        let r = view_to_dxf(&meshes, &[], &view, true, 0.02);
        // 後ろの箱の縦の稜線は、下の 5 mm だけ隠れる
        // 重なる縁を除き、前の箱は 4 辺の輪、後ろの箱の見える所は 3 辺の折れ線にする。
        // 後ろの縦の縁の隠れる部分は、それぞれ独立した線分になる。
        assert_eq!((r.visible, r.hidden), (2, 2));
        assert_eq!(written_segments(&r.dxf, "VISIBLE").len(), 7);
        assert_eq!(written_segments(&r.dxf, "HIDDEN").len(), 2);
        assert_projection_is_preserved(&meshes, &[], &view, true, &r);
    }

    /// 円柱(半径 5・高さ 10)を 24 角柱で近づけた網の側面(1 つの面)。
    fn prism_side() -> Mesh {
        let mut m = Mesh::default();
        let n = 24;
        for i in 0..n {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let b = std::f64::consts::TAU * (i + 1) as f64 / n as f64;
            let p = |t: f64, z: f64| [(5.0 * t.cos()) as f32, (5.0 * t.sin()) as f32, z as f32];
            let base = m.positions.len() as u32;
            m.positions
                .extend([p(a, 0.0), p(b, 0.0), p(b, 10.0), p(a, 10.0)]);
            m.normals.extend([[0.0; 3]; 4]);
            m.triangles.push([base, base + 1, base + 2]);
            m.triangles.push([base, base + 2, base + 3]);
            m.triangle_faces.extend([0, 0]);
        }
        m
    }

    #[test]
    fn silhouettes_come_from_faces_that_turn_away() {
        // 上から見ると輪郭は無く、前から見ると両脇に出る
        let m = prism_side();
        let front = View::named("front").unwrap();
        let s = silhouettes(&m, &front);
        assert_eq!(s.len(), 2, "左右の 2 本");
        for l in &s {
            assert!((l[0][2] - l[1][2]).abs() > 9.0, "縦の線");
        }
        assert!(silhouettes(&m, &View::named("top").unwrap()).is_empty());
    }

    /// 同じ形の DXF は毎回同じ中身(輪郭の線を HashMap の順で書くと、起動ごとに線の順が変わる。
    /// HashMap は作るたびに順の種が替わるので、1 つの試験の中で何度も作れば違いが出る)。
    #[test]
    fn the_same_view_writes_the_same_dxf_every_time() {
        let m = prism_side();
        let front = View::named("front").unwrap();
        let first = view_to_dxf(std::slice::from_ref(&m), &[], &front, true, 0.02).dxf;
        for _ in 0..20 {
            assert_eq!(
                view_to_dxf(std::slice::from_ref(&m), &[], &front, true, 0.02).dxf,
                first
            );
        }
    }

    #[test]
    fn views_are_orthonormal_and_reject_a_bad_up() {
        let v = View::new([1.0, 1.0, 1.0], [0.0, 0.0, 1.0]).unwrap();
        for (a, b) in [(v.right, v.up), (v.up, v.eye), (v.eye, v.right)] {
            assert!(dot(a, b).abs() < 1e-12);
        }
        assert!(View::new([0.0, 0.0, 1.0], [0.0, 0.0, 1.0]).is_none());
        assert!(View::named("斜め").is_none());
    }

    #[test]
    fn the_root_edge_of_a_round_boss_stays_visible_where_the_face_mesh_is_finer() {
        // 円柱(半径 5・高さ 5)の側面を 64 分割、付け根の縁の線を 32 分割にする(実際のカーネルも
        // 線と面を別々に網にし、線の弦の真ん中が側面の網より内側に入る)
        let (r, h, nf, ne) = (5.0f64, 5.0f64, 64, 32);
        let at = |k: usize, n: usize, z: f64| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            [r * a.cos(), r * a.sin(), z].map(|v| v as f32)
        };
        let mut m = Mesh::default();
        for k in 0..nf {
            let base = m.positions.len() as u32;
            for p in [
                at(k, nf, 0.0),
                at(k + 1, nf, 0.0),
                at(k + 1, nf, h),
                at(k, nf, h),
            ] {
                m.positions.push(p);
                m.normals.push([0.0; 3]);
            }
            m.triangles.push([base, base + 1, base + 2]);
            m.triangles.push([base, base + 2, base + 3]);
            m.triangle_faces.extend([0, 0]);
        }
        // 上のふた(扇形)
        let center = m.positions.len() as u32;
        m.positions.push([0.0, 0.0, h as f32]);
        m.normals.push([0.0; 3]);
        for k in 0..nf {
            let base = m.positions.len() as u32;
            m.positions.extend([at(k, nf, h), at(k + 1, nf, h)]);
            m.normals.extend([[0.0; 3]; 2]);
            m.triangles.push([center, base, base + 1]);
            m.triangle_faces.push(1);
        }
        m.face_names.extend(["側面".into(), "上".into()]);
        m.edges.push((0..=ne).map(|k| at(k, ne, 0.0)).collect());
        m.edges.push((0..=ne).map(|k| at(k, ne, h)).collect());
        let iso = View::named("iso").unwrap();
        let out = view_to_dxf(std::slice::from_ref(&m), &[], &iso, true, 0.02);
        // 上の縁、輪郭、付け根の手前の半周は、つながる端をまとめて 3 つの実体になる。
        // 隠れ線は付け根の奥の半周だけ。線の位置は両画層で保つ。
        assert_eq!((out.visible, out.hidden), (3, 1), "{}", out.dxf);
        assert_projection_is_preserved(std::slice::from_ref(&m), &[], &iso, true, &out);
    }
}
