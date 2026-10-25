//! 見本と基準部品。スケッチを手早く組み立てる [`SketchBuilder`] もここに置く。

use kmb_cad::{Direction, Operation};
use kmb_cad_extrude::Extrude;
use kmb_cad_hole::Hole;
use kmb_cad_pattern::LinearPattern;
use kmb_core::{Document, FeatureEntry, FeatureId, Op, Param, Transaction};
use kmb_sketch::{
    Constraint, ConstraintId, ConstraintKind as K, Entity, EntityId, Sketch, SketchPlane,
};

/// 寸法を式で与えてスケッチを組み立てる。座標は解く前の目安。
pub struct SketchBuilder {
    sketch: Sketch,
    next_entity: u32,
    next_constraint: u32,
}

impl SketchBuilder {
    pub fn new(plane: SketchPlane) -> Self {
        SketchBuilder {
            sketch: Sketch {
                plane,
                ..Default::default()
            },
            next_entity: 1,
            next_constraint: 1,
        }
    }

    /// XY 平面のスケッチ。
    pub fn xy() -> Self {
        Self::new(SketchPlane::Xy)
    }

    /// ZX 平面のスケッチ。
    pub fn zx() -> Self {
        Self::new(SketchPlane::Zx)
    }

    fn entity(&mut self) -> EntityId {
        let id = EntityId(self.next_entity);
        self.next_entity += 1;
        id
    }

    pub fn constrain(&mut self, kind: K) {
        let id = ConstraintId(self.next_constraint);
        self.next_constraint += 1;
        self.sketch.constraints.push(Constraint::new(id, kind));
    }

    pub fn point(&mut self, x: f64, y: f64) -> EntityId {
        let id = self.entity();
        self.sketch.entities.push(Entity::Point { id, x, y });
        id
    }

    /// 今の位置に固定した点。
    pub fn fixed_point(&mut self, x: f64, y: f64) -> EntityId {
        let p = self.point(x, y);
        self.constrain(K::Fix { point: p });
        p
    }

    /// `from` から横に `dx`、縦に `dy` の点。`guess` は目安の座標。
    pub fn point_from(&mut self, from: EntityId, dx: &str, dy: &str, guess: [f64; 2]) -> EntityId {
        let p = self.point(guess[0], guess[1]);
        self.constrain(K::HorizontalDistance {
            a: from,
            b: p,
            value: dx.into(),
        });
        self.constrain(K::VerticalDistance {
            a: from,
            b: p,
            value: dy.into(),
        });
        p
    }

    pub fn line(&mut self, a: EntityId, b: EntityId) -> EntityId {
        let id = self.entity();
        self.sketch.entities.push(Entity::Line {
            id,
            a,
            b,
            construction: false,
        });
        id
    }

    /// 中心 `center` のまわりを `start` から `end` へ反時計回りに回る円弧。
    pub fn arc(&mut self, center: EntityId, start: EntityId, end: EntityId) -> EntityId {
        let id = self.entity();
        self.sketch.entities.push(Entity::Arc {
            id,
            center,
            start,
            end,
            construction: false,
        });
        id
    }

    /// `corner` を左下の頂点にした長方形。辺の ID を下、右、上、左の順に返す。
    pub fn rectangle(
        &mut self,
        corner: EntityId,
        width: &str,
        height: &str,
        guess: [f64; 4],
    ) -> [EntityId; 4] {
        let [x, y, w, h] = guess;
        let p2 = self.point(x + w, y);
        let p3 = self.point(x + w, y + h);
        let p4 = self.point(x, y + h);
        let l = [
            self.line(corner, p2),
            self.line(p2, p3),
            self.line(p3, p4),
            self.line(p4, corner),
        ];
        self.constrain(K::Horizontal { line: l[0] });
        self.constrain(K::Horizontal { line: l[2] });
        self.constrain(K::Vertical { line: l[1] });
        self.constrain(K::Vertical { line: l[3] });
        self.constrain(K::Length {
            line: l[0],
            value: width.into(),
        });
        self.constrain(K::Length {
            line: l[1],
            value: height.into(),
        });
        l
    }

    pub fn circle(&mut self, center: EntityId, diameter: &str, guess_radius: f64) -> EntityId {
        let id = self.entity();
        self.sketch.entities.push(Entity::Circle {
            id,
            center,
            radius: guess_radius,
            construction: false,
        });
        self.constrain(K::Diameter {
            circle: id,
            value: diameter.into(),
        });
        id
    }

    /// 角を半径 `r` で丸めた長方形(左下 `x`・`y`、幅 `w`、高さ `h`)。点はどれも固定する。
    /// 線と弧の ID を、下の線から反時計回りに返す(線・弧・線・弧…)。
    pub fn rounded_rectangle(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) -> Vec<EntityId> {
        let (x1, y1) = (x + w, y + h);
        // 角ごとに (中心, 弧の始め, 弧の終わり)。弧は反時計回り
        let corners = [
            ([x1 - r, y + r], [x1 - r, y], [x1, y + r]),
            ([x1 - r, y1 - r], [x1, y1 - r], [x1 - r, y1]),
            ([x + r, y1 - r], [x + r, y1], [x, y1 - r]),
            ([x + r, y + r], [x, y + r], [x + r, y]),
        ];
        let pts: Vec<[EntityId; 3]> = corners
            .iter()
            .map(|(c, a, b)| {
                [
                    self.fixed_point(c[0], c[1]),
                    self.fixed_point(a[0], a[1]),
                    self.fixed_point(b[0], b[1]),
                ]
            })
            .collect();
        let mut out = Vec::new();
        for i in 0..4 {
            let prev = pts[(i + 3) % 4][2];
            out.push(self.line(prev, pts[i][1]));
            out.push(self.arc(pts[i][0], pts[i][1], pts[i][2]));
        }
        out
    }

    pub fn build(self) -> Sketch {
        self.sketch
    }
}

/// 機能を順に足していく文書の組み立て役。
pub struct DocBuilder {
    pub doc: Document,
    ops: Vec<Op>,
}

impl Default for DocBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DocBuilder {
    pub fn new() -> Self {
        DocBuilder {
            doc: Document::new(),
            ops: Vec::new(),
        }
    }

    pub fn param(&mut self, name: &str, expr: &str) {
        let id = self.doc.alloc_param_id();
        self.ops.push(Op::AddParam {
            index: None,
            param: Param::new(id, name, expr),
        });
    }

    pub fn feature(&mut self, type_id: &str, name: &str, data: impl serde::Serialize) -> FeatureId {
        let id = self.doc.alloc_feature_id();
        self.ops.push(Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: type_id.into(),
                name: name.into(),
                data: serde_json::to_value(data).expect("機能のデータは JSON にできる"),
                suppressed: false,
            },
        });
        id
    }

    /// 組んだスケッチを機能として足す。
    pub fn sketch(&mut self, name: &str, s: SketchBuilder) -> FeatureId {
        self.feature(kmb_sketch::SKETCH, name, s.build())
    }

    pub fn finish(mut self, label: &str) -> Document {
        self.doc
            .apply(Transaction::new(label, self.ops))
            .expect("組み立てた文書は正しい");
        self.doc.clear_history();
        self.doc
    }
}

/// 0°〜90° の円弧(半径 30、寸法つき)と左上の単独の点のスケッチだけの文書。半径の寸法は
/// 円弧の外の左下(約 200°)に置く(寸法の位置まで円弧を点線で延ばす見た目を撮る)。
pub fn arc_and_point() -> Document {
    let mut sk = Sketch::default();
    let c = sk.add_point(0.0, 0.0);
    sk.add_constraint(K::Fix { point: c });
    let a = sk.add_point(30.0, 0.0);
    let b = sk.add_point(0.0, 30.0);
    let arc = sk.add_arc(c, a, b);
    let r = sk.add_constraint(K::Radius {
        circle: arc,
        value: "30 mm".into(),
    });
    sk.add_point(-25.0, 25.0);
    let sol = kmb_sketch::solve(&sk, &std::collections::HashMap::from([(r, 30.0)]))
        .expect("円弧と点のスケッチは解ける");
    sk.place_label(r, [-31.0, -9.0], &sol);
    let mut d = DocBuilder::new();
    d.feature(kmb_sketch::SKETCH, "円弧と点", sk);
    d.finish("円弧と点")
}

/// 基準部品の各機能の ID。
pub struct ReferencePart {
    pub doc: Document,
    pub plate_sketch: FeatureId,
    pub plate: FeatureId,
    pub top_sketch: FeatureId,
    pub pocket: FeatureId,
    pub boss: FeatureId,
    pub holes: FeatureId,
    pub hole_pattern: FeatureId,
}

/// 基準部品: 穴あきの板に、上面からのポケット、上面のボス、四隅の貫通穴(2 つの穴を
/// 横に並べたパターン)を足したもの。寸法はすべてパラメータの式。
///
/// | 項目 | 既定値 |
/// | --- | --- |
/// | 板 | width 60 × height 40 × thickness 10 mm、中央に hole 8 mm の貫通穴 |
/// | ポケット | 左下 (8, 8) から 15 × 12 mm、深さ pocket_depth 4 mm |
/// | ボス | 中心 (47, 20)、径 10 mm、高さ 5 mm |
/// | 四隅の穴 | 径 corner_hole 3.4 mm、縁から 4 mm、貫通 |
pub fn reference_part() -> ReferencePart {
    let mut b = DocBuilder::new();
    for (n, e) in [
        ("width", "60 mm"),
        ("height", "40 mm"),
        ("thickness", "10 mm"),
        ("hole", "8 mm"),
        ("pocket_depth", "4 mm"),
        ("corner_hole", "3.4 mm"),
    ] {
        b.param(n, e);
    }

    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "width", "height", [0.0, 0.0, 60.0, 40.0]);
    let c = s.point_from(o, "width / 2", "height / 2", [30.0, 20.0]);
    s.circle(c, "hole", 4.0);
    let plate_sketch = b.feature(kmb_sketch::SKETCH, "板の輪郭", s.build());
    let plate = b.feature(
        kmb_cad_extrude::EXTRUDE,
        "板",
        Extrude {
            profile: Some(plate_sketch),
            regions: Vec::new(),
            distance: "thickness".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );

    // 上面のスケッチ: ポケットの長方形とボスの円
    let top = SketchPlane::face(plate, format!("f{}:end", plate.0));
    let mut s = SketchBuilder::new(top.clone());
    let o = s.fixed_point(0.0, 0.0);
    let pc = s.point_from(o, "8 mm", "8 mm", [8.0, 8.0]);
    s.rectangle(pc, "15 mm", "12 mm", [8.0, 8.0, 15.0, 12.0]);
    let top_sketch = b.feature(kmb_sketch::SKETCH, "ポケットの輪郭", s.build());
    let pocket = b.feature(
        kmb_cad_extrude::EXTRUDE,
        "ポケット",
        Extrude {
            profile: Some(top_sketch),
            regions: Vec::new(),
            distance: "pocket_depth".into(),
            direction: Direction::Reverse,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::Cut,
            target: Some(plate),
            ..Default::default()
        },
    );

    let mut s = SketchBuilder::new(top.clone());
    let o = s.fixed_point(0.0, 0.0);
    let bc = s.point_from(o, "47 mm", "20 mm", [47.0, 20.0]);
    s.circle(bc, "10 mm", 5.0);
    let boss_sketch = b.feature(kmb_sketch::SKETCH, "ボスの輪郭", s.build());
    let boss = b.feature(
        kmb_cad_extrude::EXTRUDE,
        "ボス",
        Extrude {
            profile: Some(boss_sketch),
            regions: Vec::new(),
            distance: "5 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::Join,
            target: Some(pocket),
            ..Default::default()
        },
    );

    // 左側の 2 つの穴を、右へ並べる
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = s.fixed_point(0.0, 0.0);
    let h1 = s.point_from(o, "4 mm", "4 mm", [4.0, 4.0]);
    let h2 = s.point_from(o, "4 mm", "height - 4 mm", [4.0, 36.0]);
    let hole_sketch = b.feature(kmb_sketch::SKETCH, "穴の位置", s.build());
    let holes = b.feature(
        kmb_cad_hole::HOLE,
        "四隅の穴",
        Hole {
            target: boss,
            sketch: hole_sketch,
            points: vec![kmb_core::PointId(h1.0), kmb_core::PointId(h2.0)],
            diameter: "corner_hole".into(),
            depth: None,
            ..Hole::new(boss, hole_sketch)
        },
    );
    let hole_pattern = b.feature(
        kmb_cad_pattern::LINEAR_PATTERN,
        "穴の並び",
        LinearPattern {
            feature: vec![holes],
            body: None,
            direction_ref: Default::default(),
            direction: [1.0, 0.0, 0.0],
            spacing: "width - 8 mm".into(),
            count: "2".into(),
            ..Default::default()
        },
    );

    ReferencePart {
        doc: b.finish("基準部品"),
        plate_sketch,
        plate,
        top_sketch,
        pocket,
        boss,
        holes,
        hole_pattern,
    }
}

/// 基準部品の体積、表面積、外形の理論値(既定の寸法と、幅だけ変えた時)。
pub fn reference_values(width: f64) -> (f64, f64, [f64; 3]) {
    use std::f64::consts::PI;
    let (h, t) = (40.0, 10.0);
    let r_hole: f64 = 4.0;
    let r_corner: f64 = 1.7;
    let r_boss: f64 = 5.0;
    let (pw, ph, pd) = (15.0, 12.0, 4.0);
    let boss_h = 5.0;
    let base = width * h;
    let holes_area = PI * r_hole * r_hole + 4.0 * PI * r_corner * r_corner;
    let volume = base * t - holes_area * t - pw * ph * pd + PI * r_boss * r_boss * boss_h;
    let area = 2.0 * (base - holes_area)          // 上下の面(ボスの上面は足元の分と相殺)
        + 2.0 * (pw + ph) * pd                      // ポケットの壁(底は上面の開口と相殺)
        + 2.0 * (width + h) * t                     // 外周の側面
        + 2.0 * PI * r_hole * t                     // 中央の穴
        + 4.0 * 2.0 * PI * r_corner * t             // 四隅の穴
        + 2.0 * PI * r_boss * boss_h; // ボスの側面
    (volume, area, [width, h, t + boss_h])
}
