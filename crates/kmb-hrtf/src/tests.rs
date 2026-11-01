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

/// 確かめること: normalize(拡散音場)の後は、below_hz の 1 オクターブ上より高い所で全方向の平均の
/// 大きさが 0 dB にそろい、below_hz より下は元の大きさのまま、立ち上がりは 0 番に残る。基準に自分を
/// 渡しても同じになる。
#[test]
fn normalize_flattens_the_average() {
    let mut s = sets(&[("a", synth(1.0))]);
    let r = run(
        &recipe(
            r#"{"steps":[{"op":"onset","set":"a"},{"op":"normalize","set":"a","below_hz":1000}]}"#,
        ),
        &mut s,
    )
    .unwrap();
    assert!(r.is_empty());
    let mut before = sets(&[("a", synth(1.0))]);
    run(
        &recipe(r#"{"steps":[{"op":"onset","set":"a"}]}"#),
        &mut before,
    )
    .unwrap();
    let (a, b) = (&s["a"], &before["a"]);
    let f = a.taps();
    let fft = Fft::new(f);
    for ear in 0..2 {
        let mean = |set: &HrirSet| -> Vec<f64> {
            let mut p = vec![0.0; f / 2 + 1];
            for pair in &set.ir {
                for (k, v) in fft.spectrum(&pair[ear]).iter().take(f / 2 + 1).enumerate() {
                    p[k] += v.norm_sqr() / set.ir.len() as f64;
                }
            }
            p
        };
        let (pa, pb) = (mean(a), mean(b));
        for k in 0..=f / 2 {
            let hz = k as f64 * FS / f as f64;
            let db = 10.0 * pa[k].log10();
            if (2500.0..=20000.0).contains(&hz) {
                assert!(db.abs() < 0.5, "耳 {ear} {hz} Hz: {db} dB");
            }
            if hz < 900.0 {
                assert!(
                    (db - 10.0 * pb[k].log10()).abs() < 0.5,
                    "耳 {ear} {hz} Hz が変わった"
                );
            }
        }
        for pair in &a.ir {
            let peak = (0..f)
                .max_by(|&i, &j| pair[ear][i].abs().total_cmp(&pair[ear][j].abs()))
                .unwrap();
            assert!(peak < 8, "立ち上がりが動いた({peak})");
        }
    }
    let mut same = sets(&[("a", synth(1.0)), ("r", synth(1.0))]);
    run(
        &recipe(r#"{"steps":[{"op":"onset","set":"a"},{"op":"onset","set":"r"},{"op":"normalize","set":"a","reference":"r","below_hz":1000}]}"#),
        &mut same,
    )
    .unwrap();
    for (x, y) in same["a"].ir.iter().zip(&a.ir) {
        for ear in 0..2 {
            for (u, v) in x[ear].iter().zip(&y[ear]) {
                assert!((u - v).abs() < 1e-12);
            }
        }
    }
    // 輪の枠でない集まりには使えない
    let mut lin = sets(&[("a", synth(1.0))]);
    let e = run(
        &recipe(r#"{"steps":[{"op":"normalize","set":"a"}]}"#),
        &mut lin,
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Input);
}

/// 確かめること: credits は集まりごとに題・許諾などの属性と処理の記録を書き、無い属性は
/// 「(記載なし)」と書く。名前にフォルダの区切りを使えば誤り。
#[test]
fn credits_lists_attribution_and_changes() {
    let mut s = sets(&[("a", synth(1.0)), ("b", synth(0.5))]);
    let files = run(
        &recipe(r#"{"steps":[{"op":"onset","set":"b"},{"op":"credits","sets":["a","b"]}]}"#),
        &mut s,
    )
    .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "帰属.txt");
    let t = String::from_utf8(files[0].bytes.clone()).unwrap();
    assert!(t.contains("== a ==") && t.contains("== b =="));
    assert!(t.contains("題(Title): 合成 1"));
    assert!(t.contains("許諾(License): 試験用"));
    assert!(t.contains("組織(Organization): (記載なし)"));
    assert!(t.contains("- onset: 立ち上がりを外して遅れへ移した"));
    let e = run(
        &recipe(r#"{"steps":[{"op":"credits","sets":["a"],"name":"x/帰属.txt"}]}"#),
        &mut s,
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Input);
}
