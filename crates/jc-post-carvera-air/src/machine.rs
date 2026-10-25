//! Carvera Air の機械の値(作業範囲・ベッド・レーザーと補正の命令・1 段の深さの表)。
//!
//! 汎用の crate(jc-post・jc-cam-*)は機械を名指ししない。組み立て(jc-bundle)が [`machine`] を
//! マシンライブラリに入れ、セットアップは選んだ機械の値を文書に写して持つ。

use jc_post::{Bed, Laser, LaserDrive, Leveling, MachineFrame, MachineProfile, SlotStepdown};

/// マシンライブラリの ID(書き出すポストの ID も同じ)。
pub const ID: &str = "carvera-air";

/// Carvera Air の値。作業範囲と回転数は機械の公表値(C)、工具番号の範囲は仮(A)。
/// 早送り・加速度・角の値・軸の最高速は、機械の制御ソフトとファームの既定値から(A。
/// 実機の config は未確認。公表の最高速は 4000 mm/min とあり、設定の既定 3000 と違う)。
/// コレットは 3.175 mm が標準で、4・6・6.35 mm が別売り。
/// レーザー(M321・M322、出力は S0〜1)と高さの補正(G32・M370)は機械の対応 G-code から
/// (A。実機は未確認)。
pub fn profile() -> MachineProfile {
    MachineProfile {
        id: ID.into(),
        post: ID.into(),
        name: "Carvera Air".into(),
        travel: [300.0, 200.0, 130.0],
        max_rpm: 13000.0,
        min_rpm: 1000.0,
        tool_numbers: (1, 99),
        rapid_feed: 3000.0,
        acceleration: 150.0,
        junction_deviation: 0.05,
        max_rate: [3000.0, 3000.0, 2000.0],
        measure_after_tool_change: true,
        air: false,
        collets: vec![3.175, 4.0, 6.0, 6.35],
        bed: Some(bed()),
        frame: Some(frame()),
        atc_slots: 0,
        outer: [500.0, 450.0, 450.0],
        home: "右奥・上(機械の座標は負)".into(),
        // ファームの対応表にあるのは G54 だけ。O 番号の行も載っていない
        wcs_count: 1,
        program_number: false,
        optional_stop: false,
        laser: Some(Laser {
            s_max: 1.0,
            enter: vec!["M321".into()],
            exit: vec!["M322".into()],
            on: String::new(),
            off: String::new(),
            // Smoothieware 系のレーザーは送りの間だけ焼く(A、実機は未確認)
            pierce: None,
            drive: LaserDrive::Gantry,
            notes: vec![
                "純正のレーザーの付属品を使う。M321 で工具を外してレーザーに替え、M322 で戻す(A、実機は未確認)".into(),
            ],
        }),
        leveling: Some(Leveling {
            probe: "G32 R1 X{x} Y{y} A{w} B{l} I{nx} J{ny} H{h}".into(),
            clear: "M370".into(),
            notes: vec![
                "付属のプローブで格子状に測る。補正は M370 か再起動まで続く(A、実機は未確認)"
                    .into(),
            ],
        }),
        slot_stepdown: slot_stepdown(),
    }
}

/// 機械に付く工具の表の、平エンドミル(Spiral O)の 1 段の深さ(mm)。
/// Carvera Air と Carvera で共通。材料の対応(木材・アクリル・樹脂 → Hardwood/Plastic、
/// MDF → Softwood)は推測(A)。
fn slot_stepdown() -> Vec<SlotStepdown> {
    let row = |material: &str, depths: [f64; 3]| SlotStepdown {
        material: material.into(),
        depths,
    };
    vec![
        row("アルミ", [0.2, 0.15, 0.1]),
        row("真鍮", [0.1, 0.1, 0.05]),
        row("木材", [1.0, 0.5, 0.5]),
        row("アクリル", [1.0, 0.5, 0.5]),
        row("樹脂", [1.0, 0.5, 0.5]),
        row("MDF", [2.0, 1.0, 1.0]),
    ]
}

/// Carvera Air の機械の形。門の下の高さ 120 mm は公開の仕様の値(確認済み)。作業範囲の置き場所(ベッドの上の真ん中)、
/// 主軸の鼻のいちばん下の高さ、主軸と Z の台の大きさ、ベッドを削ってよい深さは公開の資料に
/// 無く、推定(A)。
pub fn frame() -> MachineFrame {
    MachineFrame {
        travel_corner: [3.0, 11.0],
        nose_low: 30.0,
        spindle: [52.0, 50.0],
        carriage: [100.0, 80.0, 60.0],
        clearance: 120.0,
        // MDF のベッドは貫通の分を削ってよい(穴あけ・輪郭の「さらに下げる深さ」の 0.3 mm ほどと、
        // 貫通のねじ切りが下へ抜ける 0.6 mm ほど)
        bed_allowance: 1.0,
        notes: vec![
            "作業範囲の置き場所・主軸の鼻の高さ・主軸と Z の台の大きさは推定。手元の機械で測って、設定の「機械」で直す".into(),
        ],
    }
}

/// Carvera Air のベッド。MDF 306×222×15 mm と M5 の穴 66 個は公式のベッドの STEP から(C、
/// data/bed_holes.json)。原点(L 字の基準の角)がベッドの角から 15, 15 なのと、
/// L 字の腕の長さ、工具長センサーの位置は試作の値で、実機で測っていない(U)。
pub fn bed() -> Bed {
    #[derive(serde::Deserialize)]
    struct Holes {
        holes: Vec<[f64; 2]>,
    }
    let h: Holes = serde_json::from_str(include_str!("../data/bed_holes.json"))
        .expect("ベッドの穴の一覧は JSON");
    Bed {
        size: [306.0, 222.0, 15.0],
        material: "MDF".into(),
        thread: "M5".into(),
        hole_diameter: 6.0,
        holes: h.holes,
        origin: [15.0, 15.0],
        fence: Some([100.0, 15.0]),
        sensor: Some([280.0, 196.0]),
        notes: vec![
            "原点の L 字の角の位置(ベッドの角から 15, 15)と工具長センサーの位置は推定。手元の機械で測って直す".into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slot_depth_follows_the_diameter_classes() {
        let m = profile();
        assert_eq!(m.slot_stepdown("アルミ", 3.175), Some(0.2));
        assert_eq!(m.slot_stepdown("アルミ", 2.0), Some(0.15));
        assert_eq!(m.slot_stepdown("アルミ", 1.0), Some(0.1));
        assert_eq!(m.slot_stepdown("知らない材料", 3.0), None);
        assert_eq!(
            jc_post::machines::generic_fanuc().slot_stepdown("アルミ", 3.175),
            None
        );
    }
}
