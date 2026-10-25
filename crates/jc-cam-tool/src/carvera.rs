//! Carvera Air に付く工具の一式。
//!
//! 形は、Carvera Air の標準の工具表(Example Tools・PCB Tools)に出る径・刃長・全長と、
//! 同じ形の工具の値による。
//! 切削条件も公開の工具表の材料ごとの値(Hardwood → 木材、Plastic → アクリル、Aluminum → アルミ、
//! Brass → 真鍮、PCB はそのままの名前)。工具表に送り幅がない工具は、始めの工具と同じ割合にした(推測)。
//!
//! 彫刻の刃の角度は工具表の名前の「15°」「30°」を片側の角度と読み、
//! 全角(30°、60°)にした(公開の工具表の同じ工具が「30 deg」で taperAngle 15 のため。推測を含む)。

use crate::{Cutting, Extra, Preset, Tool, ToolKind};

/// 材料ごとの条件(回転数、送り、下降、切り込み、送り幅)。
fn presets(rows: &[(&str, [f64; 5])]) -> Vec<Preset> {
    rows.iter()
        .map(|(m, [rpm, feed, plunge, stepdown, stepover])| Preset {
            material: (*m).into(),
            cutting: Cutting {
                rpm: *rpm,
                feed: *feed,
                plunge: *plunge,
                stepdown: *stepdown,
                stepover: *stepover,
            },
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn tool(
    name: &str,
    kind: ToolKind,
    diameter: f64,
    flute_length: f64,
    overall_length: f64,
    flutes: u32,
    tip_angle: f64,
    tip_diameter: Option<f64>,
    presets: Vec<Preset>,
) -> Tool {
    Tool {
        number: 0,
        name: name.into(),
        kind,
        diameter,
        flute_length,
        flutes,
        tip_angle,
        shank_diameter: Some(3.175),
        overall_length: Some(overall_length),
        tip_diameter,
        stickout: None,
        holder_diameter: None,
        corner_radius: None,
        neck_length: None,
        thread: None,
        presets,
        extra: Extra::new(),
    }
}

/// Carvera Air に付く工具。番号は 0(足す時に空いている番号を振る)。
pub fn carvera_air_tools() -> Vec<Tool> {
    // 公開の工具表の、金属も削れる 1 枚刃の平(Spiral O Metal 3.175×12)
    let metal_flat = presets(&[
        ("木材", [12000.0, 1000.0, 300.0, 1.0, 0.4]),
        ("アクリル", [12000.0, 1000.0, 300.0, 1.0, 0.4]),
        ("アルミ", [12000.0, 500.0, 200.0, 0.2, 0.3]),
        ("真鍮", [12000.0, 300.0, 100.0, 0.1, 0.3]),
    ]);
    // 長い刃の 1 枚刃の平(Spiral O 3.175×25)。金属の条件はない
    let long_flat = presets(&[
        ("木材", [10000.0, 1000.0, 300.0, 1.0, 0.4]),
        ("アクリル", [10000.0, 1000.0, 300.0, 1.0, 0.4]),
    ]);
    // 1 枚刃の彫刻(送り幅は工具表の mm を上端の径 3.175 mm で割った割合)
    let engrave = |stepover_mm: f64| {
        let s = stepover_mm / 3.175;
        presets(&[
            ("木材", [12000.0, 1000.0, 500.0, 1.0, s]),
            ("アクリル", [12000.0, 1000.0, 500.0, 1.0, s]),
            ("アルミ", [12000.0, 500.0, 200.0, 0.2, s]),
            ("真鍮", [12000.0, 300.0, 100.0, 0.1, s]),
            ("PCB", [12000.0, 500.0, 200.0, 0.1, s]),
        ])
    };
    let chamfer = presets(&[
        ("木材", [12000.0, 1000.0, 500.0, 1.0, 1.0 / 3.175]),
        ("アクリル", [12000.0, 1000.0, 500.0, 1.0, 1.0 / 3.175]),
        ("アルミ", [12000.0, 600.0, 200.0, 0.2, 0.2 / 3.175]),
        ("真鍮", [12000.0, 500.0, 200.0, 0.1, 0.1 / 3.175]),
        ("PCB", [12000.0, 500.0, 200.0, 0.1, 0.1 / 3.175]),
    ]);
    // コーン(基板の外形の切り抜き)。送り幅は工具表にないので 0.4(推測)
    let corn = || presets(&[("PCB", [12000.0, 500.0, 300.0, 0.3, 0.4])]);
    let mut v = vec![
        tool(
            "平 3.175 mm 刃長 25",
            ToolKind::FlatEnd,
            3.175,
            25.0,
            45.0,
            1,
            118.0,
            None,
            long_flat,
        ),
        Tool {
            // 首下長は Carvera Air の工具表の値
            neck_length: Some(26.0),
            ..tool(
                "平 3.175 mm 刃長 12(金属)",
                ToolKind::FlatEnd,
                3.175,
                12.0,
                38.0,
                1,
                118.0,
                None,
                metal_flat,
            )
        },
        tool(
            "彫刻 30° 先 0.2 mm",
            ToolKind::TaperEnd,
            3.175,
            10.0,
            37.0,
            1,
            30.0,
            Some(0.2),
            engrave(0.06),
        ),
        tool(
            "彫刻 30° 先 0.3 mm 刃長 2",
            ToolKind::TaperEnd,
            3.175,
            2.0,
            46.0,
            1,
            30.0,
            Some(0.3),
            engrave(0.09),
        ),
        tool(
            "面取り 90° 3.175 mm",
            ToolKind::VBit,
            3.175,
            8.0,
            38.0,
            2,
            90.0,
            None,
            chamfer,
        ),
        tool(
            "彫刻 60° 先 0.1 mm",
            ToolKind::TaperEnd,
            3.175,
            10.0,
            37.0,
            1,
            60.0,
            Some(0.1),
            engrave(0.03),
        ),
    ];
    for d in [0.6, 0.7, 0.8, 0.9] {
        v.push(tool(
            &format!("コーン {d} mm"),
            ToolKind::FlatEnd,
            d,
            6.0,
            38.0,
            2,
            118.0,
            None,
            corn(),
        ));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolLibrary;

    #[test]
    fn the_carvera_tools_are_valid_and_added_once_after_the_starter_tools() {
        let tools = carvera_air_tools();
        assert_eq!(tools.len(), 10);
        let mut lib = ToolLibrary::starter();
        let added = lib.add_tools(tools.clone(), true);
        assert_eq!(added, (6..=15).collect::<Vec<_>>());
        lib.check().unwrap();
        // 始めの 5 本は残る
        assert_eq!(lib.get(1).unwrap().name, "平 6 mm");
        // 2 回目は同じ名前を足さない
        assert!(lib.add_tools(tools, true).is_empty());
        assert_eq!(lib.tools.len(), 15);
        let e = lib
            .tools
            .iter()
            .find(|t| t.name == "彫刻 30° 先 0.2 mm")
            .unwrap();
        assert_eq!((e.tip_angle, e.tip_diameter), (30.0, Some(0.2)));
        // 彫刻の刃の先端の幅は 0.2 mm で、深さ 1 mm で片側 15° だけ広がる
        let r1 = e.radius_at(1.0).unwrap();
        assert!((r1 - (0.1 + 15f64.to_radians().tan())).abs() < 1e-9);
    }
}
