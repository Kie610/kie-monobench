//! 基準部品の加工の見本。2 本のエンドミル(6 mm と 3.175 mm)で、上面のフェイス、
//! ボスの周りのポケット、ポケット、中央の穴と四隅の穴(ヘリカル)、外形の輪郭を削る。
//! 3D の見本([`reference_cam_3d`])は、上面の縁を面取りし、外周の縦の角を丸めた部品を、
//! さらにボールエンドミルの走査線で仕上げる。

use jc_cam_commands::{OpBase, SETUP, Setup, TOOLS, ToolLibrary};

use jc_strategy_contour2d::{CONTOUR, Contour, Side};

use jc_strategy_drill::{DRILL, Drill};

use jc_strategy_face::{FACE, Face};

use jc_strategy_pocket2d::{POCKET, Pocket};

use jc_strategy_raster3d::{RASTER3D, Raster3d};

use crate::sample::{ReferencePart, reference_part};
use jc_core::{Document, FeatureEntry, FeatureId, Op, Transaction};
use jc_strategy_waterline::{WATERLINE, Waterline};

/// 基準部品と、その加工の機能の ID。
pub struct ReferenceCam {
    pub part: ReferencePart,
    pub tools: FeatureId,
    pub setup: FeatureId,
    pub face: FeatureId,
    pub top_pocket: FeatureId,
    pub pocket: FeatureId,
    pub center_hole: FeatureId,
    pub corner_holes: FeatureId,
    pub contour: FeatureId,
    /// 3D の見本だけ: 面取り、フィレット、走査線の仕上げ
    pub chamfer: Option<FeatureId>,
    pub fillet: Option<FeatureId>,
    pub raster: Option<FeatureId>,
    pub waterline: Option<FeatureId>,
}

impl ReferenceCam {
    pub fn doc(&self) -> &Document {
        &self.part.doc
    }
}

pub fn reference_cam() -> ReferenceCam {
    build(false)
}

/// 3D の見本。上面の縁を 1 mm 面取りし、外周の縦の角を半径 2 mm で丸めた部品を、
/// 2D の加工の後でボール 3.175 mm の走査線で、ボスの上面から 6.5 mm(板の上面の 1.5 mm 下)まで仕上げる。
pub fn reference_cam_3d() -> ReferenceCam {
    build(true)
}

fn build(three_d: bool) -> ReferenceCam {
    let mut part = reference_part();
    let doc = &mut part.doc;
    let mut ops = Vec::new();
    let mut add = |doc: &mut Document, type_id: &str, name: &str, data: serde_json::Value| {
        let id = doc.alloc_feature_id();
        ops.push(Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: type_id.into(),
                name: name.into(),
                data,
                suppressed: false,
            },
        });
        id
    };
    let (mut chamfer, mut fillet) = (None, None);
    let mut body = part.hole_pattern;
    if three_d {
        let p = part.plate.0;
        let c = add(
            doc,
            jc_cad_edge::CHAMFER,
            "上面の縁の面取り",
            json(&jc_cad_edge::Chamfer::one(
                body,
                jc_cad_edge::EdgeSelection {
                    edges: vec![],
                    faces: vec![format!("f{p}:end")],
                    arcs: false,
                },
                "1 mm",
                false,
                jc_cad_edge::Corner::Miter,
            )),
        );
        let side = |i: u32| format!("f{p}:side:e{i}");
        let f = add(
            doc,
            jc_cad_edge::FILLET,
            "外周の角の丸め",
            json(&jc_cad_edge::Fillet::one(
                c,
                jc_cad_edge::EdgeSelection {
                    edges: [(5, 6), (6, 7), (7, 8), (5, 8)]
                        .iter()
                        .map(|(a, b)| [side(*a), side(*b)])
                        .collect(),
                    faces: vec![],
                    arcs: false,
                },
                "2 mm",
                false,
                jc_cad_edge::Corner::Miter,
            )),
        );
        (chamfer, fillet, body) = (Some(c), Some(f), f);
    }
    let tools = add(doc, TOOLS, "工具", json(&ToolLibrary::starter()));
    let setup = add(doc, SETUP, "セットアップ 1", json(&Setup::new(body)));
    let base = |tool| OpBase::new(setup, tools, tool);
    let face = add(doc, FACE, "上面のフェイス", json(&Face::new(base(1))));
    let top_pocket = add(
        doc,
        POCKET,
        "ボスの周り",
        json(&Pocket {
            ramp_angle: Some("3 deg".into()),
            ..Pocket::on_face(base(1), format!("f{}:end", part.plate.0))
        }),
    );
    let pocket = add(
        doc,
        POCKET,
        "ポケット",
        json(&Pocket {
            ramp_angle: Some("3 deg".into()),
            ..Pocket::on_face(base(2), format!("f{}:start", part.pocket.0))
        }),
    );
    let center_hole = add(
        doc,
        DRILL,
        "中央の穴",
        json(&Drill {
            diameter: Some("hole".into()),
            ..Drill::new(base(2))
        }),
    );
    let corner_holes = add(
        doc,
        DRILL,
        "四隅の穴",
        json(&Drill {
            diameter: Some("corner_hole".into()),
            ..Drill::new(base(2))
        }),
    );
    let contour = add(
        doc,
        CONTOUR,
        "外形",
        json(&Contour {
            base: base(2),
            face: None,
            side: Side::Outside,
            extra_depth: "0.3 mm".into(),
            leave: "0 mm".into(),
            ramp_angle: Some("3 deg".into()),
            tabs: 0,
            tab_width: "4 mm".into(),
            tab_height: "1.5 mm".into(),
            finish: None,
            direction: Default::default(),
            lead: None,
            chains: vec![],
            depth: None,
            entry: None,
            entry_opts: Default::default(),
            rough_passes: 0,
            rough_step: None,
            tab_shape: Default::default(),
            tab_points: Vec::new(),
            finish_opts: Default::default(),
            lead_opts: Default::default(),
            rest_tool: None,
            rest_size: Default::default(),
        }),
    );
    let raster = three_d.then(|| {
        add(
            doc,
            RASTER3D,
            "面取りと角の仕上げ",
            json(&Raster3d {
                base: base(4),
                leave: "0 mm".into(),
                along: Default::default(),
                resolution: "0.05 mm".into(),
                // 2D の加工で大半を削った後なので、1 回でなぞる
                levels: false,
                depth: Some("6.5 mm".into()),
                angle: None,
                entry: None,
                direction: Default::default(),
                limits: Default::default(),
            }),
        )
    });
    let waterline = three_d.then(|| {
        add(
            doc,
            WATERLINE,
            "縦の壁の仕上げ",
            json(&Waterline {
                base: base(4),
                leave: "0 mm".into(),
                step: Some("1 mm".into()),
                resolution: "0.1 mm".into(),
                // ボールの刃長(12 mm)に収まるよう、ストックの上面から 12 mm まで
                depth: Some("11 mm".into()),
                ramp_angle: None,
                entry: None,
                order: Default::default(),
                limits: Default::default(),
            }),
        )
    });
    doc.apply(Transaction::new("加工を追加", ops))
        .expect("組み立てた加工は正しい");
    doc.clear_history();
    ReferenceCam {
        part,
        tools,
        setup,
        face,
        top_pocket,
        pocket,
        center_hole,
        corner_holes,
        contour,
        chamfer,
        fillet,
        raster,
        waterline,
    }
}

fn json<T: serde::Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).expect("JSON にできる")
}
