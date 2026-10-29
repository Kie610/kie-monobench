//! 三面図・断面・詳細を持つ図面の見本。
use crate::sample::{DocBuilder, SketchBuilder};
use kmb_core::{Document, Transaction};
use kmb_drawing::*;

fn view(id: &str, kind: ViewKind, at: [f64; 2]) -> DrawingView {
    DrawingView {
        id: id.into(),
        label: String::new(),
        kind,
        scale: Ratio::default(),
        at,
        options: ViewOptions {
            hidden: true,
            ..Default::default()
        },
        rest: Rest::new(),
    }
}
fn attach(mut doc: Document) -> Document {
    let mut sheet = Sheet::new("sheet", "形の図面");
    sheet.views = vec![
        view(
            "front",
            ViewKind::Base {
                source: Source::All,
                orientation: Orientation::Front,
                rotate: 0,
            },
            [50., 60.],
        ),
        view(
            "top",
            ViewKind::Projected {
                parent: "front".into(),
                side: Side::Top,
            },
            [70., 0.],
        ),
        view(
            "right",
            ViewKind::Projected {
                parent: "front".into(),
                side: Side::Right,
            },
            [110., 0.],
        ),
        view(
            "section",
            ViewKind::Section {
                parent: "front".into(),
                line: [[20., -5.], [20., 25.]],
                letter: "A".into(),
                uncut: vec![],
            },
            [160., 0.],
        ),
        view(
            "detail",
            ViewKind::Detail {
                parent: "front".into(),
                center: [20., 10.],
                radius: 8.,
                scale: Ratio { num: 2., den: 1. },
                letter: "B".into(),
            },
            [200., 140.],
        ),
    ];
    let drawings = Drawings {
        sheets: vec![sheet],
        ..Default::default()
    };
    doc.apply(Transaction::new(
        "図面",
        vec![doc.data().section_op(&drawings).expect("図面の区画")],
    ))
    .expect("図面を置く");
    doc.clear_history();
    doc
}
fn model(hole: bool, cylinder: bool, assembly: bool) -> Document {
    let mut b = DocBuilder::new();
    b.param("width", "40 mm");
    b.param("depth", "20 mm");
    b.param("height", "10 mm");
    let mut roots = vec![];
    for i in 0..if assembly { 2 } else { 1 } {
        let mut s = SketchBuilder::xy();
        let o = s.fixed_point(
            if cylinder { 20. } else { i as f64 * 55. },
            if cylinder { 10. } else { 0. },
        );
        if cylinder {
            s.circle(o, "20 mm", 10.);
        } else {
            s.rectangle(o, "width", "depth", [i as f64 * 55., 0., 40., 20.]);
            if hole {
                let c = s.point_from(o, "20 mm", "10 mm", [20., 10.]);
                s.circle(c, "8 mm", 4.);
            }
        }
        let sk = b.sketch("輪郭", s);
        roots.push(b.feature(
            kmb_cad_extrude::EXTRUDE,
            "ボディ",
            kmb_cad_extrude::Extrude {
                profile: Some(sk),
                distance: "height".into(),
                ..Default::default()
            },
        ));
    }
    let mut doc = b.finish("図面の見本");
    if assembly {
        let id = doc.alloc_feature_id().0;
        doc.apply(Transaction::new(
            "組立",
            vec![kmb_core::Op::SetComponents {
                components: vec![kmb_core::Component {
                    id,
                    name: "組立".into(),
                    parent: None,
                    bodies: roots,
                }],
            }],
        ))
        .expect("組立の部品");
    }
    attach(doc)
}
pub fn drawing_box_document() -> Document {
    model(false, false, false)
}
pub fn drawing_hole_document() -> Document {
    model(true, false, false)
}
pub fn drawing_cylinder_document() -> Document {
    model(false, true, false)
}
pub fn drawing_assembly_document() -> Document {
    model(false, false, true)
}
