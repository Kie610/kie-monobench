//! 処理の手順を通した試験。データのファイルは使わず、輪の格子の上に合成の HRIR を組む。

use std::collections::BTreeMap;

use crate::fft::{Fft, advance};
use crate::grid::Rings;
use crate::table::{Layout, read};
use crate::{Dir, ErrorKind, Frame, Grid, HrirSet, Recipe, run};

const FS: f64 = 48000.0;

/// 刻み 30° の輪の格子に、方向と耳で大きさと遅れが変わる短い IR(長さ 256)を置いた集まり。
/// `shape` で IR の形を変える(大きさの違う 2 つの集まりを作るため)。
fn synth(shape: f64) -> HrirSet {
    let g = Rings::new(30.0).unwrap();
    let fft = Fft::new(256);
    let mut dirs = vec![];
    let mut ir = vec![];
    for (az, el) in g.dirs() {
        dirs.push(Dir { az, el, dist: 1.5 });
        let pair = std::array::from_fn(|ear| {
            let side = if ear == 0 { 1.0 } else { -1.0 };
            let lat = side * az.to_radians().sin() * el.to_radians().cos();
            let mut h = vec![0.0; 256];
            h[0] = 1.0 + 0.3 * lat;
            h[1] = -0.5 * shape + 0.1 * lat;
            h[2] = 0.25 * shape;
            h[3] = -0.1 * (1.0 + el / 90.0);
            // 近い耳ほど早く届く遅れ(20〜30 sample、小数を含む)。
            advance(&fft, &h, -(25.0 - 4.7 * lat))
        });
        ir.push(pair);
    }
    let m = dirs.len();
    let mut meta = BTreeMap::new();
    meta.insert("Title".into(), format!("合成 {shape}"));
    meta.insert("License".into(), "試験用".into());
    HrirSet {
        fs: FS,
        dirs,
        ir,
        delay: vec![[0.0, 0.0]; m],
        onset: None,
        frame: Frame::Linear,
        grid: Grid::Rings { step: 30.0 },
        meta,
        history: vec![],
    }
}

fn sets(list: &[(&str, HrirSet)]) -> BTreeMap<String, HrirSet> {
    list.iter()
        .map(|(n, s)| (n.to_string(), s.clone()))
        .collect()
}

fn recipe(json: &str) -> Recipe {
    Recipe::from_json(json).unwrap()
}

/// 確かめること: 手順の JSON は書いて読んで同じになり、知らない欄・知らない段・欠けた欄は誤りになる。
#[test]
fn recipe_json_round_trip_and_rejects_unknown() {
    let r = recipe(
        r#"{"steps":[
            {"op":"onset","set":"b"},
            {"op":"combine","magnitude":"a","phase":"b","out":"ab"},
            {"op":"shift","set":"ab","from":"b","add":3},
            {"op":"trim","set":"ab","before":4,"after":128,"rolloff_hz":20000},
            {"op":"table","sets":["ab"],"lead":4,"before":4}
        ]}"#,
    );
    let back = Recipe::from_json(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(back, r);
    for bad in [
        r#"{"steps":[{"op":"trim","set":"a","before":1,"after":2,"bogus":1}]}"#,
        r#"{"steps":[{"op":"resample","set":"a"}]}"#,
        r#"{"steps":[{"op":"combine","magnitude":"a","out":"c"}]}"#,
        r#"{"steps":[],"extra":1}"#,
    ] {
        let e = Recipe::from_json(bad).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Input, "{bad}");
    }
}

fn layout_of(files: &[crate::OutFile]) -> Layout {
    let f = files.iter().find(|f| f.name == "layout.json").unwrap();
    serde_json::from_slice(&f.bytes).unwrap()
}

fn bytes<'a>(files: &'a [crate::OutFile], name: &str) -> &'a [u8] {
    &files.iter().find(|f| f.name == name).unwrap().bytes
}

/// 確かめること: 立ち上がりを外して書いた分けた表を、格子の引き方で鳴らすと元の HRIR のスペクトルに
/// float16 の丸めの範囲で戻る(ナイキストを除く)。ファイルの大きさと layout.json の値も決まりどおり。
#[test]
fn table_plays_back_the_original() {
    let a = synth(1.0);
    let mut s = sets(&[("a", a.clone())]);
    let files = run(
        &recipe(
            r#"{"steps":[{"op":"onset","set":"a"},
            {"op":"table","sets":["a"],"spectrum_step":30,"onset_step":30}]}"#,
        ),
        &mut s,
    )
    .unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        ["spectrum-0.f16", "onset-0.f16", "rings.f32", "layout.json"]
    );
    let lay = layout_of(&files);
    let rows = Rings::new(30.0).unwrap().len();
    assert_eq!((lay.fft, lay.bins, lay.datasets), (512, 257, 1));
    assert_eq!((lay.spectrum_rows, lay.onset_entries), (rows, rows));
    assert_eq!(lay.onset_height, 1);
    assert_eq!(bytes(&files, "spectrum-0.f16").len(), rows * 257 * 8);
    assert_eq!(bytes(&files, "onset-0.f16").len(), 1024 * 4);
    assert_eq!(bytes(&files, "rings.f32").len(), 2 * 7 * 2 * 4);
    let t = read(
        &lay,
        bytes(&files, "spectrum-0.f16"),
        bytes(&files, "onset-0.f16"),
    )
    .unwrap();
    let fft = Fft::new(512);
    let mut worst = 0.0f64;
    for (i, d) in a.dirs.iter().enumerate() {
        let got = crate::table::play(&t, &lay, d.az, d.el).unwrap();
        for (ear, got) in got.iter().enumerate() {
            let want = fft.spectrum(&a.ir[i][ear]);
            let peak = want.iter().map(|v| v.norm()).fold(0.0, f64::max);
            // ナイキストの項は除く: 実数の IR では小数の進めの位相を持てず、実部だけが残る。
            for k in 0..256 {
                worst = worst.max((got[k] - want[k]).norm() / peak);
            }
        }
    }
    assert!(worst < 2e-3, "最大の差 {worst}");
}

/// 確かめること: 同じ集まりどうしの組み合わせは元に戻り、違う集まりでは大きさが magnitude 側、
/// 遅れと立ち上がりが phase 側になる。
#[test]
fn combine_takes_magnitude_and_time() {
    let (a, b) = (synth(0.2), synth(1.4));
    let mut s = sets(&[("a", a), ("b", b)]);
    run(
        &recipe(
            r#"{"steps":[{"op":"onset","set":"a"},{"op":"onset","set":"b"},
            {"op":"combine","magnitude":"b","phase":"b","out":"bb"},
            {"op":"combine","magnitude":"a","phase":"b","out":"ab"}]}"#,
        ),
        &mut s,
    )
    .unwrap();
    let fft = Fft::new(512);
    let (sa, sb, bb, ab) = (&s["a"], &s["b"], &s["bb"], &s["ab"]);
    for i in 0..sb.ir.len() {
        for ear in 0..2 {
            for (x, y) in bb.ir[i][ear].iter().zip(&sb.ir[i][ear]) {
                assert!((x - y).abs() < 1e-6);
            }
            let (ha, hab) = (fft.spectrum(&sa.ir[i][ear]), fft.spectrum(&ab.ir[i][ear]));
            for k in 0..=256 {
                assert!((ha[k].norm() - hab[k].norm()).abs() < 1e-4 * ha[0].norm().max(1.0));
            }
        }
    }
    assert_eq!(ab.delay, sb.delay);
    assert_eq!(ab.frame, Frame::Circular);
    assert!(ab.history.last().unwrap().contains("合成 1.4"));
}

/// 確かめること: shift で立ち上がりを別の集まり + 3 sample に合わせ、trim で窓の外が 0 になり、
/// 高い所のならしでナイキストの近くが消える。表の共通の遅れは最小の立ち上がりから決まる。
#[test]
fn shift_trim_and_common_delay() {
    let (a, b) = (synth(0.2), synth(1.4));
    let mut s = sets(&[("a", a), ("b", b)]);
    let files = run(
        &recipe(
            r#"{"steps":[{"op":"onset","set":"a"},{"op":"onset","set":"b"},
            {"op":"shift","set":"a","from":"b","add":3},
            {"op":"trim","set":"a","before":4,"after":64},
            {"op":"trim","set":"b","before":4,"after":64,"rolloff_hz":20000},
            {"op":"table","sets":["a","b"],"spectrum_step":30,"onset_step":30,"lead":4,"before":4}]}"#,
        ),
        &mut s,
    )
    .unwrap();
    let (sa, sb) = (&s["a"], &s["b"]);
    for (da, db) in sa.delay.iter().zip(&sb.delay) {
        assert!((da[0] - db[0] - 3.0).abs() < 1e-12 && (da[1] - db[1] - 3.0).abs() < 1e-12);
    }
    assert!(
        sa.ir
            .iter()
            .flatten()
            .all(|h| h[64..508].iter().all(|&v| v == 0.0))
    );
    let fft = Fft::new(512);
    let nyq = fft.spectrum(&sb.ir[3][0])[256];
    assert!(nyq.norm() < 1e-12, "{nyq:?}");
    let lay = layout_of(&files);
    assert_eq!(lay.names, ["a", "b"]);
    let lo = sb
        .delay
        .iter()
        .flatten()
        .cloned()
        .fold(f64::INFINITY, f64::min);
    assert_eq!(lay.common_delay[1], (lo.floor() - 8.0).max(0.0));
    assert_eq!(lay.common_delay[0], ((lo + 3.0).floor() - 8.0).max(0.0));
    assert!(lay.common_delay[1] > 0.0);
}

/// 確かめること: そろっていない入力は、何段目かを添えた入力の誤りで止まる(黙って直さない)。
#[test]
fn mismatches_stop_with_input_errors() {
    let a = synth(1.0);
    let mut b = synth(1.0);
    b.dirs[5].az += 1.0;
    let cases = [
        (
            r#"{"steps":[{"op":"combine","magnitude":"a","phase":"b","out":"c"}]}"#,
            "段 1(combine)",
        ),
        (
            r#"{"steps":[{"op":"table","sets":["a"]}]}"#,
            "立ち上がりが無い",
        ),
        (
            r#"{"steps":[{"op":"trim","set":"a","before":4,"after":64}]}"#,
            "輪の枠でない",
        ),
        (
            r#"{"steps":[{"op":"onset","set":"zz"}]}"#,
            "集まり zz が無い",
        ),
        (
            r#"{"steps":[{"op":"onset","set":"a","frame":128}]}"#,
            "枠の長さ 128 を超える",
        ),
    ];
    for (json, want) in cases {
        let mut s = sets(&[("a", a.clone()), ("b", b.clone())]);
        let e = run(&recipe(json), &mut s).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Input, "{json}");
        assert!(e.message.contains(want), "{}", e.message);
    }
    let mut bad = a.clone();
    bad.ir[0][1].pop();
    let mut s = sets(&[("a", bad)]);
    let e = run(&recipe(r#"{"steps":[{"op":"onset","set":"a"}]}"#), &mut s).unwrap_err();
    assert!(e.message.contains("長さ"), "{}", e.message);
}

/// 確かめること: 立ち上がりを外してから格子を替えて SOFA に書くと、元の方向と重なる方向では鳴らす音が
/// 元と同じで、間の方向の立ち上がりは囲む方向の立ち上がりの間に入る。読み直すと方向と属性が残る。
#[test]
fn regrid_then_write_sofa() {
    let mut s = sets(&[("a", synth(1.0))]);
    let files = crate::run_with(
        &recipe(
            r#"{"steps": [
                {"op": "onset", "set": "a"},
                {"op": "regrid", "set": "a", "to": {"kind": "rings", "step": 15}, "out": "b"},
                {"op": "write", "set": "b", "delay": "keep"}
            ]}"#,
        ),
        &mut s,
        &crate::RunOptions {
            now: Some("2026-11-01 12:00:00".into()),
        },
    )
    .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "b.sofa");
    let a = &s["a"];
    let b = &s["b"];
    assert_eq!(b.grid, Grid::Rings { step: 15.0 });
    assert_eq!(b.dirs.len(), Rings::new(15.0).unwrap().len());
    let pa = a.played_onset().unwrap();
    let pb = b.played_onset().unwrap();
    let lo = pa.iter().flatten().copied().fold(f64::INFINITY, f64::min);
    let hi = pa
        .iter()
        .flatten()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    for (i, d) in b.dirs.iter().enumerate() {
        for v in pb[i] {
            assert!(v >= lo - 1e-9 && v <= hi + 1e-9);
        }
        // 30° の格子と重なる方向は元の値そのもの。
        if let Some(j) = a.dirs.iter().position(|x| {
            (x.el - d.el).abs() < 1e-9 && (x.el.abs() > 89.0 || (x.az - d.az).abs() < 1e-9)
        }) {
            assert_eq!(b.ir[i], a.ir[j]);
            assert_eq!(b.delay[i], a.delay[j]);
        }
    }
    let back = crate::sofa::read(&files[0].bytes).unwrap();
    assert!(back.same_dirs(b));
    assert_eq!(back.meta["Title"], "合成 1");
    assert_eq!(back.meta["DateModified"], "2026-11-01 12:00:00");
}

/// 確かめること: 立ち上がりを外さずに三角形で混ぜようとすると、混ぜる前に止まる。最近傍なら通り、
/// 別の集まりの格子にもそろえられる。
#[test]
fn regrid_needs_onset_removed_to_mix() {
    let mut s = sets(&[("a", synth(1.0)), ("b", synth(0.5))]);
    let e = run(
        &recipe(
            r#"{"steps": [{"op": "regrid", "set": "a", "to": {"kind": "rings", "step": 15}}]}"#,
        ),
        &mut s,
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Input);
    assert!(e.message.contains("onset"), "{}", e.message);
    run(
        &recipe(
            r#"{"steps": [
                {"op": "regrid", "set": "a", "method": "nearest", "to": {"kind": "gauss_legendre", "el": 7, "az": 12}},
                {"op": "regrid", "set": "b", "method": "nearest", "to": {"kind": "like", "set": "a"}}
            ]}"#,
        ),
        &mut s,
    )
    .unwrap();
    assert_eq!(s["a"].dirs.len(), 84);
    assert!(s["a"].same_dirs(&s["b"]));
    assert_eq!(s["b"].grid, Grid::GaussLegendre { el: 7, az: 12 });
}

/// 確かめること: 遅延の表を集まりより細かい格子で作る時、`onset_method` が `bilinear` なら間の方向の
/// 立ち上がりは仰角に線形な値をそのまま補う。既定の最近傍では隣の方向の値の段になる。手順の JSON の
/// `onset_method` で選べる。
#[test]
fn onset_table_interpolates_between_directions() {
    let mut s = sets(&[("a", synth(1.0))]);
    run(&recipe(r#"{"steps":[{"op":"onset","set":"a"}]}"#), &mut s).unwrap();
    let mut a = s["a"].clone();
    let f = |el: f64| [20.0 + el / 10.0, 30.0 - el / 20.0];
    a.delay = vec![[0.0, 0.0]; a.dirs.len()];
    a.onset = Some(a.dirs.iter().map(|d| f(d.el)).collect());
    let mut spec = crate::table::TableSpec {
        spectrum_step: 30.0,
        onset_step: 10.0,
        ..Default::default()
    };
    let near = crate::table::build(&a, &spec).unwrap();
    spec.onset_method = crate::regrid::Method::Bilinear;
    let lin = crate::table::build(&a, &spec).unwrap();
    let mut stepped = 0;
    for (i, (_, el)) in Rings::new(10.0).unwrap().dirs().into_iter().enumerate() {
        let want = f(el);
        for (ear, w) in want.iter().enumerate() {
            assert!((lin.onset[i][ear] - w).abs() < 1e-9, "{el} {ear}");
            if (near.onset[i][ear] - w).abs() > 0.1 {
                stepped += 1;
            }
        }
    }
    assert!(stepped > 0);
    s.insert("a".into(), a);
    let files = run(
        &recipe(
            r#"{"steps":[{"op":"table","sets":["a"],"spectrum_step":30,"onset_step":10,"onset_method":"bilinear"}]}"#,
        ),
        &mut s,
    )
    .unwrap();
    assert_eq!(
        layout_of(&files).onset_entries,
        Rings::new(10.0).unwrap().len()
    );
}
