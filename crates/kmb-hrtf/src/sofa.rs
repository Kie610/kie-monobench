//! SOFA(AES69)の読み書き。
//!
//! 読むのは時間の IR(`DataType` が `FIR`。SimpleFreeFieldHRIR など)と、周波数の伝達関数
//! (`TF`。SimpleFreeFieldHRTF)。どちらも受け手は 2 つ(左・右)に限る。音源の位置は SOFA の球座標
//! (方位は前から左回り、仰角は上が正、度と m)にそろえ、直交座標なら変換する。
//!
//! 書くのは SimpleFreeFieldHRIR だけ。HDF5 の書き込みは `hdf5w` が受け持つ。
//! 読み込みは純 Rust の HDF5 の読み手(`hdf5-pure`)に任せる。圧縮・分割の置き方・古い版の頭など、
//! よそで作られたファイルのいろいろな形を読める。

use std::collections::BTreeMap;

use hdf5_pure::File;
use serde::{Deserialize, Serialize};

use crate::dsp::linear_from_circular;
use crate::fft::{C, Fft, advance};
use crate::hdf5w::{self, Attr, Dataset, Values};
use crate::{Dir, Frame, Grid, HrirSet, HrtfError};

/// 書き出しの時の遅れの扱い。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelayMode {
    /// 遅れを IR に入れ、`Data.Delay` は全方向で共通の 0 以上の整数にする。遅れを読まない
    /// 再生の道具でも同じ音になる(既定)。
    #[default]
    Bake,
    /// 遅れを方向ごとの `Data.Delay` に分けて書く。IR は立ち上がりをそろえたまま。
    Keep,
}

/// SOFA の書き出しの設定。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SofaSpec {
    pub delay: DelayMode,
    /// 作った日時(UTC、`yyyy-mm-dd HH:MM:SS`)。`None` なら元の属性の値を残す(無ければ書かない)。
    pub now: Option<String>,
}

/// 読み手が決まりとして見る属性。元の値より、書く物の形に合わせた値を優先する。
const FIXED: [(&str, &str); 6] = [
    ("Conventions", "SOFA"),
    ("Version", "2.1"),
    ("SOFAConventions", "SimpleFreeFieldHRIR"),
    ("SOFAConventionsVersion", "1.0"),
    ("DataType", "FIR"),
    ("RoomType", "free field"),
];

/// 無ければ足す属性(SOFA が必ず要る物)。
const REQUIRED: [(&str, &str); 6] = [
    ("Title", ""),
    ("AuthorContact", ""),
    ("Organization", ""),
    (
        "License",
        "No license provided, ask the author for permission",
    ),
    ("ListenerShortName", ""),
    ("DatabaseName", ""),
];

fn err(e: impl std::fmt::Display) -> HrtfError {
    HrtfError::input(format!("SOFA を読めない: {e}"))
}

/// SOFA のバイト列を読む。
pub fn read(bytes: &[u8]) -> Result<HrirSet, HrtfError> {
    let file = File::from_bytes(bytes.to_vec()).map_err(err)?;
    let mut meta = BTreeMap::new();
    for (k, v) in file.root().attrs().map_err(err)? {
        if let Some(s) = v.as_str() {
            meta.insert(k, s.to_string());
        }
    }
    if meta.get("Conventions").map(String::as_str) != Some("SOFA") {
        return Err(HrtfError::input(
            "SOFA でない(根の属性 Conventions が SOFA でない)",
        ));
    }
    let data_type = meta.get("DataType").cloned().unwrap_or_default();
    let array = |name: &str| -> Result<(Vec<u64>, Vec<f64>), HrtfError> {
        let ds = file
            .dataset(name)
            .map_err(|e| HrtfError::input(format!("SOFA に {name} が無い: {e}")))?;
        let shape = ds.shape().map_err(err)?;
        let v = ds.read_f64().map_err(err)?;
        Ok((shape, v))
    };
    let text_attr = |name: &str, key: &str| -> Option<String> {
        let ds = file.dataset(name).ok()?;
        let a = ds.attrs().ok()?;
        a.get(key).and_then(|v| v.as_str()).map(str::to_string)
    };

    // 音源の位置。
    let (ps, pv) = array("SourcePosition")?;
    if ps.len() != 2 || ps[1] != 3 {
        return Err(HrtfError::input(format!(
            "SourcePosition の形 {ps:?} が [M, 3] でない"
        )));
    }
    let kind = text_attr("SourcePosition", "Type").unwrap_or_else(|| "spherical".into());
    let units = text_attr("SourcePosition", "Units").unwrap_or_default();
    let mut dirs: Vec<Dir> = pv
        .chunks(3)
        .map(|p| to_dir(&kind, &units, [p[0], p[1], p[2]]))
        .collect::<Result<_, _>>()?;
    check_view(&file)?;

    let (fs, ir, n_taps) = match data_type.as_str() {
        "FIR" => {
            let (s, v) = array("Data.IR")?;
            if s.len() != 3 || s[1] != 2 {
                return Err(HrtfError::unsupported(format!(
                    "Data.IR の形 {s:?} が [M, 2, N] でない(受け手が左右の 2 つでない)"
                )));
            }
            let n = s[2] as usize;
            let (_, rate) = array("Data.SamplingRate")?;
            let fs = one_rate(&rate)?;
            let ir: Vec<[Vec<f64>; 2]> = v
                .chunks(2 * n)
                .map(|c| [c[..n].to_vec(), c[n..].to_vec()])
                .collect();
            (fs, ir, s[0] as usize)
        }
        "TF" => {
            let (s, re) = array("Data.Real")?;
            let (_, im) = array("Data.Imag")?;
            if s.len() != 3 || s[1] != 2 || re.len() != im.len() {
                return Err(HrtfError::unsupported(format!(
                    "Data.Real の形 {s:?} が [M, 2, N] でない(受け手が左右の 2 つでない)"
                )));
            }
            let (_, f) = array("N")?;
            let (fs, taps) = tf_grid(&f)?;
            let n = s[2] as usize;
            let fft = Fft::new(taps);
            let ir: Vec<[Vec<f64>; 2]> = (0..s[0] as usize)
                .map(|m| {
                    std::array::from_fn(|ear| {
                        let at = (m * 2 + ear) * n;
                        let mut sp = vec![C::new(0.0, 0.0); taps];
                        for k in 0..n {
                            sp[k] = C::new(re[at + k], im[at + k]);
                            if k > 0 && k < taps / 2 {
                                sp[taps - k] = sp[k].conj();
                            }
                        }
                        fft.real(sp)
                    })
                })
                .collect();
            (fs, ir, s[0] as usize)
        }
        other => {
            return Err(HrtfError::unsupported(format!(
                "DataType「{other}」の SOFA は読めない(FIR と TF だけ)"
            )));
        }
    };
    if dirs.len() == 1 && n_taps > 1 {
        dirs = vec![dirs[0]; n_taps];
    }
    if dirs.len() != n_taps {
        return Err(HrtfError::input(format!(
            "音源の位置 {} 個と測った数 {n_taps} が合わない",
            dirs.len()
        )));
    }
    let delay = match file.dataset("Data.Delay") {
        Ok(_) => {
            let (s, v) = array("Data.Delay")?;
            match (s.as_slice(), v.len()) {
                ([1, 2], 2) => vec![[v[0], v[1]]; n_taps],
                ([m, 2], _) if *m as usize == n_taps => v.chunks(2).map(|c| [c[0], c[1]]).collect(),
                _ => {
                    return Err(HrtfError::input(format!(
                        "Data.Delay の形 {s:?} が [1, 2] か [M, 2] でない"
                    )));
                }
            }
        }
        Err(_) => vec![[0.0, 0.0]; n_taps],
    };
    let conv = meta.get("SOFAConventions").cloned().unwrap_or_default();
    let set = HrirSet {
        fs,
        dirs,
        ir,
        delay,
        onset: None,
        frame: Frame::Linear,
        grid: Grid::Any,
        history: vec![format!("read: SOFA({conv}、{data_type})を読んだ")],
        meta,
    };
    set.check()?;
    Ok(set)
}

fn one_rate(rate: &[f64]) -> Result<f64, HrtfError> {
    let fs = *rate
        .first()
        .ok_or_else(|| HrtfError::input("Data.SamplingRate が空"))?;
    if rate.iter().any(|r| (r - fs).abs() > 1e-9) {
        return Err(HrtfError::unsupported(
            "測った物ごとにサンプリング周波数が違う SOFA は読めない",
        ));
    }
    Ok(fs)
}

/// 伝達関数の周波数の並び(0 Hz からナイキストまで等間隔)から、サンプリング周波数と IR の長さを出す。
fn tf_grid(f: &[f64]) -> Result<(f64, usize), HrtfError> {
    let bad = || {
        HrtfError::unsupported(
            "伝達関数の周波数が 0 Hz からナイキストまでの等間隔(点の数が 2 の累乗 + 1)でない",
        )
    };
    let n = f.len();
    if n < 3 || !(n - 1).is_power_of_two() || f[0].abs() > 1e-9 {
        return Err(bad());
    }
    let df = f[1] - f[0];
    if df.is_nan()
        || df <= 0.0
        || f.iter()
            .enumerate()
            .any(|(k, v)| (v - k as f64 * df).abs() > 1e-6 * df)
    {
        return Err(bad());
    }
    let taps = 2 * (n - 1);
    Ok((df * taps as f64, taps))
}

fn to_dir(kind: &str, units: &str, p: [f64; 3]) -> Result<Dir, HrtfError> {
    let kind = kind.trim().to_ascii_lowercase();
    if kind == "spherical" {
        if units.to_ascii_lowercase().contains("rad") {
            return Err(HrtfError::unsupported(
                "SourcePosition の単位がラジアン(度だけ読める)",
            ));
        }
        Ok(Dir {
            az: p[0].rem_euclid(360.0),
            el: p[1],
            dist: p[2],
        })
    } else if kind == "cartesian" {
        let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if r == 0.0 {
            return Err(HrtfError::input(
                "SourcePosition に原点の点がある(方向が無い)",
            ));
        }
        Ok(Dir {
            az: p[1].atan2(p[0]).to_degrees().rem_euclid(360.0),
            el: (p[2] / r).asin().to_degrees(),
            dist: r,
        })
    } else {
        Err(HrtfError::unsupported(format!(
            "SourcePosition の座標の種類「{kind}」は読めない"
        )))
    }
}

/// 聴く人が前(+x)を向き上(+z)が上でなければ、音源の位置の意味が変わるので止める。
fn check_view(file: &File) -> Result<(), HrtfError> {
    let get = |name: &str| -> Option<(String, Vec<f64>)> {
        let ds = file.dataset(name).ok()?;
        let kind = ds
            .attrs()
            .ok()?
            .get("Type")
            .and_then(|v| v.as_str())
            .unwrap_or("cartesian")
            .to_ascii_lowercase();
        Some((kind, ds.read_f64().ok()?))
    };
    for (name, want) in [
        ("ListenerView", [1.0, 0.0, 0.0]),
        ("ListenerUp", [0.0, 0.0, 1.0]),
    ] {
        let Some((kind, v)) = get(name) else { continue };
        if v.len() != 3 {
            return Err(HrtfError::unsupported(format!(
                "{name} が測った物ごとに違う SOFA は読めない"
            )));
        }
        let d = to_dir(&kind, "degree, degree, metre", [v[0], v[1], v[2]])?;
        let w = to_dir("cartesian", "", want)?;
        let off = (d.el - w.el).abs() > 1e-6
            || (w.el.abs() < 89.0 && {
                let da = (d.az - w.az).rem_euclid(360.0);
                da.min(360.0 - da) > 1e-6
            });
        if off {
            return Err(HrtfError::unsupported(format!(
                "{name} が決まりの向き({want:?})でない SOFA は読めない"
            )));
        }
    }
    Ok(())
}

/// SimpleFreeFieldHRIR の SOFA のバイト列にする。
pub fn write(set: &HrirSet, spec: &SofaSpec) -> Result<Vec<u8>, HrtfError> {
    set.check()?;
    let m = set.dirs.len();
    let mut history = set.history.clone();
    // 時間の並びに戻す。輪の枠は立ち上がりの前の分を頭へ回し、その分だけ遅れを減らす。
    let (mut ir, mut delay) = if set.frame == Frame::Circular {
        let lead = (set.taps() * 5 / 16) as f64;
        let ir: Vec<[Vec<f64>; 2]> = set
            .ir
            .iter()
            .map(|p| [linear_from_circular(&p[0]), linear_from_circular(&p[1])])
            .collect();
        let d = set
            .delay
            .iter()
            .map(|d| [d[0] - lead, d[1] - lead])
            .collect();
        (ir, d)
    } else {
        (set.ir.clone(), set.delay.clone())
    };
    let common = match spec.delay {
        DelayMode::Keep => None,
        DelayMode::Bake => {
            let lo = delay
                .iter()
                .flatten()
                .copied()
                .fold(f64::INFINITY, f64::min);
            let hi = delay
                .iter()
                .flatten()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            let base = lo.floor();
            let taps = set.taps() + (hi - base).ceil() as usize;
            let fft = Fft::new((2 * taps).next_power_of_two());
            for (p, d) in ir.iter_mut().zip(&delay) {
                for ear in 0..2 {
                    let mut v = advance(&fft, &p[ear], -(d[ear] - base));
                    v.truncate(taps);
                    p[ear] = v;
                }
            }
            let kept = base.max(0.0);
            if base < 0.0 {
                history.push(format!(
                    "write: 頭より前に出る共通の {} sample を外した",
                    -base
                ));
            }
            delay = vec![[kept, kept]; m];
            Some(kept)
        }
    };
    let n = ir.first().map_or(0, |p| p[0].len());

    let mut attrs: BTreeMap<String, String> = set.meta.clone();
    for (k, v) in FIXED {
        attrs.insert(k.into(), v.into());
    }
    for (k, v) in REQUIRED {
        attrs.entry(k.into()).or_insert_with(|| v.into());
    }
    attrs.insert("APIName".into(), "kieMonoBench".into());
    attrs.insert("APIVersion".into(), env!("CARGO_PKG_VERSION").into());
    if let Some(now) = &spec.now {
        attrs
            .entry("DateCreated".into())
            .or_insert_with(|| now.clone());
        attrs.insert("DateModified".into(), now.clone());
    }
    if !history.is_empty() {
        attrs.insert("History".into(), history.join("\n"));
    }
    let root: Vec<(String, String)> = attrs.into_iter().collect();

    // 次元(番号 0〜5)とデータ。
    const I: usize = 0;
    const CC: usize = 1;
    const R: usize = 2;
    const E: usize = 3;
    const N: usize = 4;
    const M: usize = 5;
    let dims = [
        ("I", 1u64),
        ("C", 3),
        ("R", 2),
        ("E", 1),
        ("N", n as u64),
        ("M", m as u64),
    ];
    let cart = |name: &str, dl: Vec<usize>, shape: Vec<u64>, v: Vec<f64>| Var {
        name: name.into(),
        dims: dl,
        shape,
        values: v,
        attrs: vec![("Type", "cartesian".into()), ("Units", "metre".into())],
    };
    let mut vars = vec![
        cart("ListenerPosition", vec![I, CC], vec![1, 3], vec![0.0; 3]),
        cart("ListenerUp", vec![I, CC], vec![1, 3], vec![0.0, 0.0, 1.0]),
        cart("ListenerView", vec![I, CC], vec![1, 3], vec![1.0, 0.0, 0.0]),
        cart(
            "ReceiverPosition",
            vec![R, CC, I],
            vec![2, 3, 1],
            vec![0.0, 0.09, 0.0, 0.0, -0.09, 0.0],
        ),
        cart(
            "EmitterPosition",
            vec![E, CC, I],
            vec![1, 3, 1],
            vec![0.0; 3],
        ),
        Var {
            name: "SourcePosition".into(),
            dims: vec![M, CC],
            shape: vec![m as u64, 3],
            values: set
                .dirs
                .iter()
                .flat_map(|d| [d.az.rem_euclid(360.0), d.el, d.dist])
                .collect(),
            attrs: vec![
                ("Type", "spherical".into()),
                ("Units", "degree, degree, metre".into()),
            ],
        },
        Var {
            name: "Data.IR".into(),
            dims: vec![M, R, N],
            shape: vec![m as u64, 2, n as u64],
            values: ir
                .iter()
                .flat_map(|p| p.iter().flatten().copied())
                .collect(),
            attrs: vec![],
        },
        Var {
            name: "Data.SamplingRate".into(),
            dims: vec![I],
            shape: vec![1],
            values: vec![set.fs],
            attrs: vec![("Units", "hertz".into())],
        },
    ];
    vars.push(match common {
        Some(c) => Var {
            name: "Data.Delay".into(),
            dims: vec![I, R],
            shape: vec![1, 2],
            values: vec![c, c],
            attrs: vec![],
        },
        None => Var {
            name: "Data.Delay".into(),
            dims: vec![M, R],
            shape: vec![m as u64, 2],
            values: delay.iter().flatten().copied().collect(),
            attrs: vec![],
        },
    });

    let mut sets: Vec<Dataset> = dims
        .iter()
        .enumerate()
        .map(|(d, &(name, len))| {
            let refs: Vec<(usize, i32)> = vars
                .iter()
                .enumerate()
                .flat_map(|(vi, v)| {
                    v.dims
                        .iter()
                        .enumerate()
                        .filter(move |(_, x)| **x == d)
                        .map(move |(pos, _)| (dims.len() + vi, pos as i32))
                })
                .collect();
            Dataset {
                name: name.into(),
                shape: vec![len],
                values: Values::F32(vec![0.0; len as usize]),
                attrs: vec![
                    ("CLASS".into(), Attr::Text("DIMENSION_SCALE".into())),
                    (
                        "NAME".into(),
                        Attr::Text(format!(
                            "This is a netCDF dimension but not a netCDF variable.{len:>10}"
                        )),
                    ),
                    ("REFERENCE_LIST".into(), Attr::ReferenceList(refs)),
                ],
            }
        })
        .collect();
    for v in vars {
        let mut attrs: Vec<(String, Attr)> = v
            .attrs
            .into_iter()
            .map(|(k, s)| (k.to_string(), Attr::Text(s)))
            .collect();
        attrs.push(("DIMENSION_LIST".into(), Attr::DimensionList(v.dims)));
        sets.push(Dataset {
            name: v.name,
            shape: v.shape,
            values: Values::F64(v.values),
            attrs,
        });
    }
    hdf5w::write(&root, &sets).map_err(|e| HrtfError::input(format!("SOFA を書けない: {}", e.0)))
}

struct Var {
    name: String,
    dims: Vec<usize>,
    shape: Vec<u64>,
    values: Vec<f64>,
    attrs: Vec<(&'static str, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 方向 6 つ(前・左・後ろ・右・上・下)に、耳と方向で値の違う短い IR を置いた集まり。
    fn small() -> HrirSet {
        let dirs: Vec<Dir> = [
            (0.0, 0.0),
            (90.0, 0.0),
            (180.0, 0.0),
            (270.0, 0.0),
            (0.0, 90.0),
            (0.0, -90.0),
        ]
        .iter()
        .map(|&(az, el)| Dir { az, el, dist: 1.5 })
        .collect();
        let ir = (0..dirs.len())
            .map(|m| {
                std::array::from_fn(|ear| {
                    (0..32)
                        .map(|t| {
                            ((m * 7 + ear * 3 + t) as f64 * 0.37).sin() * 0.5f64.powi(t as i32 / 4)
                        })
                        .collect()
                })
            })
            .collect();
        let mut meta = BTreeMap::new();
        meta.insert("Title".into(), "合成の試験".into());
        meta.insert("License".into(), "CC BY 4.0".into());
        meta.insert("Comment".into(), "知らない鍵も残る".into());
        HrirSet {
            fs: 48000.0,
            delay: (0..dirs.len())
                .map(|m| [m as f64 * 0.5, m as f64 * 0.25 + 1.0])
                .collect(),
            dirs,
            ir,
            onset: None,
            frame: Frame::Linear,
            grid: Grid::Any,
            meta,
            history: vec!["試験で組んだ".into()],
        }
    }

    /// 確かめること: 遅れを分けて書いた SOFA を読むと、方向・IR・遅れ・属性が元に戻る。
    #[test]
    fn keep_round_trips() {
        let s = small();
        let spec = SofaSpec {
            delay: DelayMode::Keep,
            now: Some("2026-11-01 00:00:00".into()),
        };
        let back = read(&write(&s, &spec).unwrap()).unwrap();
        assert_eq!(back.fs, s.fs);
        assert!(back.same_dirs(&s));
        assert_eq!(back.ir, s.ir);
        assert_eq!(back.delay, s.delay);
        assert_eq!(back.meta["Title"], "合成の試験");
        assert_eq!(back.meta["Comment"], "知らない鍵も残る");
        assert_eq!(back.meta["SOFAConventions"], "SimpleFreeFieldHRIR");
        assert_eq!(back.meta["DateModified"], "2026-11-01 00:00:00");
        assert!(back.meta["History"].contains("試験で組んだ"));
    }

    /// 確かめること: 遅れを IR に入れて書くと、鳴らす時の音(IR を遅れだけずらした物)が変わらない。
    #[test]
    fn bake_keeps_the_played_sound() {
        let mut s = small();
        // 小数の遅れは位相の傾きで入れるので、切った後ろの揺れが残る。整数の遅れで比べる。
        for (m, d) in s.delay.iter_mut().enumerate() {
            *d = [m as f64, (m % 3) as f64 + 1.0];
        }
        let back = read(&write(&s, &SofaSpec::default()).unwrap()).unwrap();
        assert!(back.delay.iter().all(|d| d == &[0.0, 0.0]));
        let fft = Fft::new(128);
        for (m, (a, b)) in s.ir.iter().zip(&back.ir).enumerate() {
            for ear in 0..2 {
                let want = advance(&fft, &a[ear], -s.delay[m][ear]);
                let got = advance(&fft, &b[ear], 0.0);
                let err = want
                    .iter()
                    .zip(&got)
                    .map(|(x, y)| (x - y).abs())
                    .fold(0.0, f64::max);
                assert!(err < 1e-9, "方向 {m} 耳 {ear} の差 {err}");
            }
        }
        // 遅れの広がり(最小 0・最大 5 sample)だけ長くなる。
        assert_eq!(back.taps(), 32 + 5);
    }

    /// 確かめること: 次元の目盛りと次元の印が、HDF5 の読み手から見える。
    #[test]
    fn dimension_scales_are_marked() {
        let bytes = write(&small(), &SofaSpec::default()).unwrap();
        let f = File::from_bytes(bytes).unwrap();
        for d in ["I", "C", "R", "E", "N", "M"] {
            let a = f.dataset(d).unwrap().attrs().unwrap();
            assert_eq!(a["CLASS"].as_str(), Some("DIMENSION_SCALE"), "{d}");
            assert!(
                a["NAME"]
                    .as_str()
                    .unwrap()
                    .starts_with("This is a netCDF dimension")
            );
        }
        let dt = f.dataset("Data.IR").unwrap().attr_datatypes().unwrap();
        assert!(dt.contains_key("DIMENSION_LIST"));
    }

    /// 確かめること: 別の作りの軽い SOFA の読み手でも、測った数・長さ・IR・次元の印が読める。
    #[test]
    fn another_reader_opens_it() {
        let s = small();
        let bytes = write(
            &s,
            &SofaSpec {
                delay: DelayMode::Keep,
                now: None,
            },
        )
        .unwrap();
        let sofa = sofar::reader::OpenOptions::new()
            .sample_rate(48000.0)
            .normalized(false)
            .open_data(&bytes)
            .unwrap();
        assert_eq!(sofa.num_measurements(), 6);
        assert_eq!(sofa.filter_len(), 32);
        let mut f = sofar::reader::Filter::new(32);
        // 左(方位 90°)の測った IR がそのまま返る。
        sofa.filter_nointerp(0.0, 1.5, 0.0, &mut f);
        for (got, want) in f.left.iter().zip(&s.ir[1][0]) {
            assert!((*got as f64 - want).abs() < 1e-6);
        }
    }

    /// 確かめること: 直交座標の音源の位置は球座標に直り、ラジアンや知らない座標は誤りになる。
    #[test]
    fn coordinates() {
        let d = to_dir("cartesian", "metre", [0.0, 2.0, 0.0]).unwrap();
        assert!((d.az - 90.0).abs() < 1e-12 && d.el.abs() < 1e-12 && (d.dist - 2.0).abs() < 1e-12);
        let d = to_dir("cartesian", "metre", [1.0, -1.0, 2f64.sqrt()]).unwrap();
        assert!((d.az - 315.0).abs() < 1e-9 && (d.el - 45.0).abs() < 1e-9);
        let d = to_dir("spherical", "degree, degree, metre", [-90.0, 10.0, 1.0]).unwrap();
        assert_eq!(d.az, 270.0);
        assert!(to_dir("spherical", "rad, rad, metre", [0.0, 0.0, 1.0]).is_err());
        assert!(to_dir("polar", "", [0.0, 0.0, 1.0]).is_err());
    }

    /// 確かめること: SOFA でない物と、受け手が 2 つでない物は誤りになる。
    #[test]
    fn rejects_what_it_cannot_read() {
        assert!(read(b"not hdf5").is_err());
        let mut s = small();
        s.meta.insert("Conventions".into(), "x".into());
        // 書く時は決まりの値で上書きするので、読み直すと SOFA として読める。
        assert!(read(&write(&s, &SofaSpec::default()).unwrap()).is_ok());
    }

    /// 確かめること: 伝達関数の周波数の並びから、サンプリング周波数と IR の長さが決まる。
    #[test]
    fn tf_frequencies() {
        let f: Vec<f64> = (0..=128).map(|k| k as f64 * 187.5).collect();
        assert_eq!(tf_grid(&f).unwrap(), (48000.0, 256));
        assert!(tf_grid(&f[1..]).is_err());
        assert!(tf_grid(&f[..100]).is_err());
    }
}
