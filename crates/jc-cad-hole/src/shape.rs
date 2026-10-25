//! 穴の形: 種類(単純・ザグリ・皿)、先端(平ら・ドリルの角度)、ネジの表、断面の作り方。
//!
//! 道具は部品([`Piece`])を順に切り取って作る: まっすぐな穴とザグリは円の押し出し、皿と
//! ドリルの先端は軸を含む平面の上の断面(半径 r、軸の向きの高さ z。面が z = 0 で、材料は
//! z < 0)を軸のまわりに回す。段を足すとき(3D プリンター用のネジ頭の下の段差など)は
//! [`pieces`] に部品を足すだけでよい。

use serde::{Deserialize, Serialize};

/// 穴の口の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleKind {
    /// 同じ径のまっすぐな穴
    #[default]
    Simple,
    /// ザグリ(六角穴付きボルトの頭が沈む、平らな底の段)
    Counterbore,
    /// 皿(皿ネジの頭が沈む、円すいの口)
    Countersink,
}

/// 止まり穴の底の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tip {
    /// 平らな底(エンドミルで掘る穴)
    #[default]
    Flat,
    /// ドリルの先端の円すい
    Angled,
}

/// ネジとの関係。形のネジ山は作らず、径を決めるのと属性として残すのに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tap {
    /// ネジに関係しない(径を直接指定)
    #[default]
    Simple,
    /// ボルトを通す穴(径は通し穴の表から)
    Clearance,
    /// ネジ穴(径は下穴の表から。ネジ山は作らない)
    Tapped,
}

/// 通し穴のはめあい(ISO 273 の 精・中・粗)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Close,
    #[default]
    Normal,
    Loose,
}

/// メートル並目ネジ 1 サイズの寸法(mm)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadSize {
    pub name: &'static str,
    /// 呼び径
    pub major: f64,
    /// 並目のピッチ
    pub pitch: f64,
    /// 下穴の径
    pub tap_drill: f64,
    /// 通し穴の径(精・中・粗、ISO 273)
    pub clearance: [f64; 3],
    /// 六角穴付きボルト(ISO 4762・JIS B 1176)のザグリの径と深さ。深さは頭が面より沈む値
    pub counterbore: (f64, f64),
    /// 皿ネジの皿の径(90°)。理論の最大の頭の径では沈みすぎるので、実際の頭の径に寄せた値
    pub countersink: f64,
}

/// メートル並目ネジの表。下穴と通し穴は ISO 273・JIS B 1001、ザグリは六角穴付き
/// ボルト用の一般的な表(深さは頭が沈む H″)。M2・M2.5 のザグリと皿は頭の径から見積もった値。
pub const THREADS: &[ThreadSize] = &[
    ThreadSize {
        name: "M2",
        major: 2.0,
        pitch: 0.4,
        tap_drill: 1.6,
        clearance: [2.2, 2.4, 2.6],
        counterbore: (4.3, 2.2),
        countersink: 4.2,
    },
    ThreadSize {
        name: "M2.5",
        major: 2.5,
        pitch: 0.45,
        tap_drill: 2.05,
        clearance: [2.7, 2.9, 3.1],
        counterbore: (5.0, 2.7),
        countersink: 5.1,
    },
    ThreadSize {
        name: "M3",
        major: 3.0,
        pitch: 0.5,
        tap_drill: 2.5,
        clearance: [3.2, 3.4, 3.6],
        counterbore: (6.5, 3.3),
        countersink: 6.3,
    },
    ThreadSize {
        name: "M4",
        major: 4.0,
        pitch: 0.7,
        tap_drill: 3.3,
        clearance: [4.3, 4.5, 4.8],
        counterbore: (8.0, 4.4),
        countersink: 8.8,
    },
    ThreadSize {
        name: "M5",
        major: 5.0,
        pitch: 0.8,
        tap_drill: 4.2,
        clearance: [5.3, 5.5, 5.8],
        counterbore: (9.5, 5.4),
        countersink: 10.4,
    },
    ThreadSize {
        name: "M6",
        major: 6.0,
        pitch: 1.0,
        tap_drill: 5.0,
        clearance: [6.4, 6.6, 7.0],
        counterbore: (11.0, 6.5),
        countersink: 12.6,
    },
    ThreadSize {
        name: "M8",
        major: 8.0,
        pitch: 1.25,
        tap_drill: 6.8,
        clearance: [8.4, 9.0, 10.0],
        counterbore: (14.0, 8.6),
        countersink: 17.3,
    },
    ThreadSize {
        name: "M10",
        major: 10.0,
        pitch: 1.5,
        tap_drill: 8.5,
        clearance: [10.5, 11.0, 12.0],
        counterbore: (17.5, 10.8),
        countersink: 20.0,
    },
    ThreadSize {
        name: "M12",
        major: 12.0,
        pitch: 1.75,
        tap_drill: 10.2,
        clearance: [13.0, 13.5, 14.5],
        counterbore: (20.0, 13.0),
        countersink: 24.5,
    },
];

/// 名前(例: `M3`)からネジのサイズを探す。
pub fn thread(name: &str) -> Option<&'static ThreadSize> {
    THREADS
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name.trim()))
}

/// 断面を作るための寸法(mm・ラジアン)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    /// 穴の径
    pub diameter: f64,
    /// 面から、径いっぱいの部分の終わりまでの深さ(先端の円すいは含まない)
    pub depth: f64,
    pub kind: HoleKind,
    /// ザグリの径と深さ
    pub counterbore: (f64, f64),
    /// 皿の径と角度(頂角)
    pub countersink: (f64, f64),
    /// 先端の角度(頂角)。`None` は平らな底
    pub tip: Option<f64>,
    /// 面より上へはみ出させる量(道具の上の面が相手の面と重ならないように)
    pub above: f64,
}

/// 断面の名前つきの区間(半径・高さの点の組)。
pub type Part = (&'static str, [f64; 2], [f64; 2]);

/// 道具の部品。順に切り取ると穴の形になる。
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    /// 面(z = 0)から `bottom` までの円柱(押し出しで作る。面と重なる所の扱いは押し出しと同じ)
    Cylinder {
        name: &'static str,
        radius: f64,
        bottom: f64,
    },
    /// 軸のまわりに回す断面の折れ線(閉じている)
    Revolved(Vec<Part>),
}

fn closed(pts: &[(&'static str, [f64; 2])]) -> Vec<Part> {
    let n = pts.len();
    (0..n)
        .map(|i| (pts[i].0, pts[i].1, pts[(i + 1) % n].1))
        .collect()
}

/// 穴の道具の部品: ザグリ、まっすぐな穴、皿、先端の順。寸法が合わなければ理由を返す。
///
/// どの部品も、相手の面と同じ平面・同じ円筒で重ならないように作る(truck のブーリアンは
/// 重なると失敗する)。皿と先端は先に掘ったまっすぐな穴の中へ食い込ませ、交わりを横切らせる。
pub fn pieces(s: &Section) -> Result<Vec<Piece>, String> {
    let r = s.diameter / 2.0;
    if r.is_nan() || r <= 0.0 {
        return Err("穴の径は 0 より大きくしてください".into());
    }
    if s.depth.is_nan() || s.depth <= 0.0 {
        return Err("穴の深さは 0 より大きくしてください".into());
    }
    let mut out = vec![Piece::Cylinder {
        name: "wall",
        radius: r,
        bottom: -s.depth,
    }];
    match s.kind {
        HoleKind::Simple => {}
        HoleKind::Counterbore => {
            let (cd, ch) = s.counterbore;
            if cd / 2.0 <= r + 1e-6 {
                return Err(format!(
                    "ザグリの径 {cd} mm は穴の径 {} mm より大きくしてください",
                    s.diameter
                ));
            }
            if ch.is_nan() || ch <= 0.0 || ch >= s.depth - 1e-6 {
                return Err(format!(
                    "ザグリの深さ {ch} mm は 0 より大きく、穴の深さ {} mm より小さくしてください",
                    s.depth
                ));
            }
            // ザグリを先に掘る(まっすぐな穴の後に掘ると truck のブーリアンが失敗した。実測)
            out.insert(
                0,
                Piece::Cylinder {
                    name: "counterbore",
                    radius: cd / 2.0,
                    bottom: -ch,
                },
            );
        }
        HoleKind::Countersink => {
            let (sd, angle) = s.countersink;
            let big = sd / 2.0;
            if big <= r + 1e-6 {
                return Err(format!(
                    "皿の径 {sd} mm は穴の径 {} mm より大きくしてください",
                    s.diameter
                ));
            }
            if !(angle > 1e-3 && angle < std::f64::consts::PI - 1e-3) {
                return Err("皿の角度は 0° より大きく 180° より小さくしてください".into());
            }
            let slope = (angle / 2.0).tan();
            let h = (big - r) / slope;
            if h >= s.depth - 1e-6 {
                return Err(format!(
                    "皿の深さ {h:.2} mm が穴の深さ {} mm 以上になります(皿の径を小さくするか、穴を深くしてください)",
                    s.depth
                ));
            }
            // 円すいを穴の中(半径 r より内、穴の底より上)まで延ばす
            let low = ((big - r / 2.0) / slope).min((h + s.depth) / 2.0);
            let inner = big - low * slope;
            // 上も円すいのまま面の上へ出し、面と円すいが縁の円で横切るようにする
            // (縦の縁を面の上に立てると、縁が面の上に乗り truck のブーリアンが失敗した。実測)
            out.push(Piece::Revolved(closed(&[
                ("countersink_top", [inner, s.above]),
                ("countersink", [big + s.above * slope, s.above]),
                ("countersink_inner", [inner, -low]),
            ])));
        }
    }
    if let Some(a) = s.tip {
        if !(a > 1e-3 && a < std::f64::consts::PI - 1e-3) {
            return Err("先端の角度は 0° より大きく 180° より小さくしてください".into());
        }
        let d = s.depth;
        let h = r / (a / 2.0).tan();
        // 上の部分は穴の中に食い込ませ、穴の底の面と重ならないようにする
        let lap = (r / 2.0).min(d / 2.0);
        out.push(Piece::Revolved(closed(&[
            ("tip_top", [0.0, -d + lap]),
            ("tip_inner", [r / 2.0, -d + lap]),
            ("tip", [r, -d]),
            ("axis", [0.0, -d - h]),
        ])));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Section {
        Section {
            diameter: 5.0,
            depth: 10.0,
            kind: HoleKind::Simple,
            counterbore: (9.0, 4.0),
            countersink: (10.0, 90f64.to_radians()),
            tip: None,
            above: 0.1,
        }
    }

    fn revolved(p: &[Piece]) -> Vec<Vec<[f64; 2]>> {
        p.iter()
            .filter_map(|x| match x {
                Piece::Revolved(parts) => {
                    // 区間は閉じている
                    for w in parts.windows(2) {
                        assert_eq!(w[0].2, w[1].1);
                    }
                    assert_eq!(parts.last().unwrap().2, parts[0].1);
                    Some(parts.iter().map(|x| x.1).collect())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn each_kind_adds_its_pieces() {
        let wall = Piece::Cylinder {
            name: "wall",
            radius: 2.5,
            bottom: -10.0,
        };
        assert_eq!(pieces(&base()).unwrap(), vec![wall.clone()]);
        let p = pieces(&Section {
            kind: HoleKind::Counterbore,
            ..base()
        })
        .unwrap();
        assert_eq!(
            p,
            vec![
                Piece::Cylinder {
                    name: "counterbore",
                    radius: 4.5,
                    bottom: -4.0
                },
                wall.clone(),
            ]
        );
        // 皿 90°: 半径の差 2.5 と同じ深さで穴の径になり、円すいは穴の中(半径 1.25)まで続く
        let p = pieces(&Section {
            kind: HoleKind::Countersink,
            ..base()
        })
        .unwrap();
        let r = revolved(&p);
        assert_eq!(r.len(), 1);
        assert!((r[0][1][0] - 5.1).abs() < 1e-9, "{r:?}");
        assert!((r[0][2][0] - 1.25).abs() < 1e-9 && (r[0][2][1] + 3.75).abs() < 1e-9);
        // 先端 118°: 円すいの頂点は r / tan(59°) 下
        let p = pieces(&Section {
            tip: Some(118f64.to_radians()),
            ..base()
        })
        .unwrap();
        let r = revolved(&p);
        let h = 2.5 / 59f64.to_radians().tan();
        assert!((r[0][3][1] + 10.0 + h).abs() < 1e-9, "{r:?}");
        assert_eq!(r[0][2], [2.5, -10.0]);
    }

    #[test]
    fn sizes_that_do_not_fit_are_refused() {
        let e = pieces(&Section {
            kind: HoleKind::Counterbore,
            counterbore: (4.0, 2.0),
            ..base()
        })
        .unwrap_err();
        assert!(e.contains("ザグリの径"), "{e}");
        let e = pieces(&Section {
            kind: HoleKind::Counterbore,
            counterbore: (9.0, 12.0),
            ..base()
        })
        .unwrap_err();
        assert!(e.contains("ザグリの深さ"), "{e}");
        let e = pieces(&Section {
            kind: HoleKind::Countersink,
            countersink: (30.0, 90f64.to_radians()),
            ..base()
        })
        .unwrap_err();
        assert!(e.contains("皿の深さ"), "{e}");
    }

    #[test]
    fn the_thread_table_is_ordered_and_consistent() {
        for w in THREADS.windows(2) {
            assert!(w[0].major < w[1].major);
        }
        for t in THREADS {
            assert!(t.tap_drill < t.major, "{}", t.name);
            assert!(t.clearance[0] > t.major && t.clearance[0] < t.clearance[1]);
            assert!(t.clearance[1] < t.clearance[2]);
            assert!(t.counterbore.0 > t.clearance[2], "{}", t.name);
            assert!(t.countersink > t.clearance[2], "{}", t.name);
            // 下穴 ≒ 呼び径 − ピッチ
            assert!((t.major - t.pitch - t.tap_drill).abs() < 0.11, "{}", t.name);
        }
        assert_eq!(thread("m3").unwrap().tap_drill, 2.5);
    }
}
