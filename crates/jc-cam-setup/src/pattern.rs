//! 加工のパターン(`cam.pattern@1`)。元の加工の経路を、矩形の並び・円形の並び・鏡写しで写す。
//!
//! 1 枚の板から同じ部品を何個も削る時に使う。元の加工はそのまま残り、この機能は写し
//! (元を除く)だけを 1 本の経路として出す。元の加工を直すと写しも作り直される。
//! 写しは作業座標系の XY で動かし(Z は変えない)、円弧は中心も写す。鏡写しでは円弧と輪の
//! 回る向きが逆になり、ダウンカットがアップカットになる(その逆も)ので注意を出す。
//! 写した後に固定具の避ける範囲をもう一度よけ、ストックの外を削る写しには注意を出す
//! (機械の作業範囲を超えるかは、書き出しの時にポストが調べる)。

use crate::{
    SetupInfo, avoid, expr_refs, finish, invalid, length, output, parse, passes_over, toolpath_of,
};
use jc_core::{
    Descriptor, Dim, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Registry,
    add_warnings, category,
};
use jc_toolpath::{Move, Toolpath, Xform2};
use serde::{Deserialize, Serialize};

pub const PATTERN: &str = "cam.pattern@1";

/// 写しの数の上限(元を含む)。打ち間違いで経路が膨らまないように
pub const MAX_COPIES: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    /// X と Y の格子に並べる
    #[default]
    Rect,
    /// 点のまわりに回して並べる
    Circle,
    /// 線に対して鏡写しにする
    Mirror,
}

/// 鏡写しの線の向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MirrorAxis {
    /// 縦の線(x = 一定)で、左と右を入れ替える
    #[default]
    LeftRight,
    /// 横の線(y = 一定)で、手前と奥を入れ替える
    FrontBack,
}

/// パターンの機能のデータ。長さと角度は式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    pub setup: FeatureId,
    /// 写す元の加工(出力 `toolpath` を持つ機能)
    pub source: FeatureId,
    #[serde(default)]
    pub kind: PatternKind,
    /// 矩形: X の数(元を含む)
    #[serde(default = "two")]
    pub count_x: u32,
    /// 矩形: Y の数(元を含む)
    #[serde(default = "one")]
    pub count_y: u32,
    #[serde(default = "thirty_mm")]
    pub spacing_x: String,
    #[serde(default = "thirty_mm")]
    pub spacing_y: String,
    /// 円形: 数(元を含む)
    #[serde(default = "four")]
    pub count: u32,
    /// 円形: 回す中心。空ならストックの中央
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center_x: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center_y: Option<String>,
    /// 円形: 並べる角度の範囲。360 deg なら一周に等しく、それより小さければ両端を含めて並べる
    #[serde(default = "full_turn")]
    pub angle: String,
    /// 鏡写しの線の向き
    #[serde(default)]
    pub mirror: MirrorAxis,
    /// 鏡写しの線の位置(左右なら x、手前奥なら y)。空ならストックの中央
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

fn one() -> u32 {
    1
}
fn two() -> u32 {
    2
}
fn four() -> u32 {
    4
}
fn thirty_mm() -> String {
    "30 mm".into()
}
fn full_turn() -> String {
    "360 deg".into()
}

impl Pattern {
    /// 写し方の一覧(元は含まない)。
    pub fn transforms(
        &self,
        ctx: &EvalCtx<'_>,
        info: &SetupInfo,
    ) -> Result<Vec<Xform2>, EvalError> {
        let mid = |k: usize| (info.stock_min[k] + info.stock_max[k]) / 2.0;
        let opt_len = |src: &Option<String>, what: &str, k: usize| match src {
            Some(s) => length(ctx, s, what),
            None => Ok(mid(k)),
        };
        let limit = |n: u32| {
            if n > MAX_COPIES {
                Err(invalid(format!(
                    "数は元を含めて {MAX_COPIES} 個までにしてください"
                )))
            } else {
                Ok(())
            }
        };
        Ok(match self.kind {
            PatternKind::Rect => {
                if self.count_x == 0 || self.count_y == 0 {
                    return Err(invalid("X と Y の数は 1 以上にしてください"));
                }
                limit(self.count_x.saturating_mul(self.count_y))?;
                if self.count_x * self.count_y < 2 {
                    return Err(invalid("X か Y の数を 2 以上にしてください"));
                }
                let dx = length(ctx, &self.spacing_x, "X の間隔")?;
                let dy = length(ctx, &self.spacing_y, "Y の間隔")?;
                let mut v = Vec::new();
                for j in 0..self.count_y {
                    for i in 0..self.count_x {
                        if i > 0 || j > 0 {
                            v.push(Xform2::translate(i as f64 * dx, j as f64 * dy));
                        }
                    }
                }
                v
            }
            PatternKind::Circle => {
                if self.count < 2 {
                    return Err(invalid("数は 2 以上にしてください"));
                }
                limit(self.count)?;
                let q = ctx.expr(&self.angle)?;
                if q.dim != Dim::ANGLE {
                    return Err(invalid(format!(
                        "角度 `{}` は角度の単位(deg)が必要です(値: {q})",
                        self.angle
                    )));
                }
                let full = std::f64::consts::TAU;
                if !(q.value.abs() > 1e-9 && q.value.abs() <= full + 1e-9) {
                    return Err(invalid("角度は 0 より大きく 360 deg 以下にしてください"));
                }
                let step = if (q.value.abs() - full).abs() < 1e-9 {
                    q.value / self.count as f64
                } else {
                    q.value / (self.count - 1) as f64
                };
                let c = [
                    opt_len(&self.center_x, "中心の X", 0)?,
                    opt_len(&self.center_y, "中心の Y", 1)?,
                ];
                (1..self.count)
                    .map(|k| Xform2::rotate(c, step * k as f64))
                    .collect()
            }
            PatternKind::Mirror => {
                let front_back = self.mirror == MirrorAxis::FrontBack;
                let k = if front_back { 1 } else { 0 };
                vec![Xform2::mirror(
                    front_back,
                    opt_len(&self.at, "線の位置", k)?,
                )]
            }
        })
    }
}

/// 経路 `moves` の切削の動き(早送りを除く)が、XY の箱 `lo`〜`hi` の外へ出るか。
fn cuts_outside(moves: &[Move], lo: [f64; 2], hi: [f64; 2]) -> bool {
    let Some(first) = moves.first() else {
        return false;
    };
    let mut cur = first.to();
    for m in &moves[1..] {
        if !matches!(m, Move::Rapid { .. }) {
            for p in m.points(cur, 1.0) {
                if p[0] < lo[0] || p[0] > hi[0] || p[1] < lo[1] || p[1] > hi[1] {
                    return true;
                }
            }
        }
        cur = m.to();
    }
    false
}

impl Feature for Pattern {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.setup, "setup"), output(self.source, "toolpath")];
        let exprs = [&self.spacing_x, &self.spacing_y, &self.angle]
            .into_iter()
            .chain(self.center_x.iter())
            .chain(self.center_y.iter())
            .chain(self.at.iter());
        v.extend(expr_refs(exprs)?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let info = SetupInfo::from_value(ctx.output(self.setup, "setup")?)
            .ok_or_else(|| invalid(format!("{} はセットアップではありません", self.setup)))?;
        let (setup, src) = toolpath_of(ctx.output(self.source, "toolpath")?)
            .ok_or_else(|| invalid(format!("{} は加工の経路を持ちません", self.source)))?;
        if setup != self.setup {
            return Err(invalid(format!(
                "元の加工 {} は別のセットアップ {setup} の加工です",
                self.source
            )));
        }
        let xs = self.transforms(ctx, &info)?;
        let mut warnings = Vec::new();
        // ストックの外を削る写し(工具の半径までのはみ出しは、外周を削る加工なので許す)
        let r = src.tool.radius() + 1e-3;
        let lo = [info.stock_min[0] - r, info.stock_min[1] - r];
        let hi = [info.stock_max[0] + r, info.stock_max[1] + r];
        let mut moves = Vec::new();
        let mut spans = Vec::new();
        let mut outside = Vec::new();
        for (k, x) in xs.iter().enumerate() {
            let copy: Vec<Move> = src.moves.iter().map(|m| x.apply(m)).collect();
            if cuts_outside(&copy, lo, hi) {
                outside.push((k + 2).to_string());
            }
            // 入り・リード・つなぎの印も写しの動きに合わせて写す
            let at = moves.len();
            spans.extend(src.spans.iter().map(|s| jc_toolpath::Span {
                start: s.start + at,
                end: s.end + at,
                role: s.role,
            }));
            moves.extend(copy);
        }
        if !outside.is_empty() {
            warnings.push(format!(
                "写し {} 個目(元を 1 個目として)がストックの外を削ります。間隔か数かストックの大きさを見直してください",
                outside.join("・")
            ));
        }
        if xs.iter().any(Xform2::mirrors) {
            warnings.push(
                "鏡写しでは円弧と輪の回る向きが逆になり、ダウンカットがアップカットに(アップカットはダウンカットに)なります。仕上がりと工具の負荷が変わります"
                    .into(),
            );
        }
        // つなぎの記録も写しごとにずらす(元の番号のままだと、写しの低い退避が安全高さより下の
        // 早送りと見なされる)
        let links = src.links.repeat(src.moves.len(), xs.len());
        let mut tp = Toolpath {
            name: ctx.feature_id().to_string(),
            moves,
            spans,
            links,
            ..src
        };
        let zones = &info.keep_out;
        if avoid(&mut tp, zones) > 0 {
            warnings.push(
                "写しが固定具の避ける範囲に掛かる所は削らずに上を越えます(固定具の下は残ります)"
                    .into(),
            );
        }
        if zones.iter().any(|z| z.1[2] + 1.0 > tp.safe_z) && passes_over(&tp, zones) {
            warnings.push("固定具が安全高さより高いので、写しの早送りが当たるかもしれません(安全高さを上げてください)".into());
        }
        let mut out = finish(self.setup, tp)?;
        add_warnings(&mut out, warnings);
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let exprs = [&mut c.spacing_x, &mut c.spacing_y, &mut c.angle]
            .into_iter()
            .chain(c.center_x.iter_mut())
            .chain(c.center_y.iter_mut())
            .chain(c.at.iter_mut());
        Ok(crate::rename_all(exprs, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        Descriptor::new(PATTERN, category::OPERATION, "加工のパターン")
            .parent("setup")
            .icon("campat")
            .aliases("ぱたーん pattern はいれつ array くりかえし こぴー copy みらー mirror かがみ えんけい circular くけい rectangular")
            .hint("元の加工の経路を、格子・円・鏡写しで写す(1 枚の板から同じ部品を何個も削る)。元の加工はそのまま残る")
            .field(Field::output("setup", "セットアップ", "setup"))
            .field(Field::output("source", "元の加工", "toolpath"))
            .field(Field::choice(
                "kind",
                "並べ方",
                &[
                    ("rect", "矩形(X と Y の格子)"),
                    ("circle", "円形(点のまわり)"),
                    ("mirror", "ミラー"),
                ],
            ))
            .field(Field::integer("count_x", "X の数(元を含む)").unitless().only("kind", "rect", "rect"))
            .field(Field::integer("count_y", "Y の数(元を含む)").unitless().only("kind", "rect", "rect"))
            .field(Field::expr("spacing_x", "X の間隔").length().help("負なら左へ並べる").only("kind", "rect", "rect"))
            .field(Field::expr("spacing_y", "Y の間隔").length().help("負なら手前へ並べる").only("kind", "rect", "rect"))
            .field(Field::integer("count", "数(元を含む)").unitless().only("kind", "circle", "rect"))
            .field(
                Field::opt_expr("center_x", "中心の X").length()
                    .help("空ならストックの中央")
                    .only("kind", "circle", "rect"),
            )
            .field(
                Field::opt_expr("center_y", "中心の Y").length()
                    .help("空ならストックの中央")
                    .only("kind", "circle", "rect"),
            )
            .field(
                Field::expr("angle", "角度の範囲").angle()
                    .help("360 deg なら一周に等しく並べる。それより小さければ両端を含めて並べる。負なら時計回り")
                    .only("kind", "circle", "rect"),
            )
            .field(
                Field::choice(
                    "mirror",
                    "鏡写しの線",
                    &[
                        ("left_right", "縦の線(左と右を入れ替える)"),
                        ("front_back", "横の線(手前と奥を入れ替える)"),
                    ],
                )
                .help("回る向きが逆になり、ダウンカットがアップカットになる")
                .only("kind", "mirror", "rect"),
            )
            .field(
                Field::opt_expr("at", "線の位置").length()
                    .help("縦の線なら X、横の線なら Y。空ならストックの中央")
                    .only("kind", "mirror", "rect"),
            ),
        |d| Ok(Box::new(parse::<Pattern>(d)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MESH_TOLERANCE, ToolpathOutput};
    use jc_cam_tool::ToolLibrary;
    use jc_contract::{Case, UPSTREAM_IDS, check_feature};
    use jc_core::{Document, FeatureEntry, FeatureStatus, NullKernel, Op, Transaction};
    use jc_toolpath::ArcDir;

    const SOURCE: &str = "test.cam_source";

    /// セットアップと経路の両方を出す見本の機能(形状カーネルなしでパターンを試すため)。
    /// 経路は (10, 10) を中心に半径 2 の円を反時計回りに 1 周する。
    struct Source;

    fn info() -> SetupInfo {
        SetupInfo {
            body: jc_core::ShapeHandle(1),
            origin: [0.0; 3],
            stock_min: [0.0, 0.0, -5.0],
            stock_max: [100.0, 60.0, 0.0],
            safe_z: 5.0,
            retract_z: 1.0,
            clearance_z: 20.0,
            wcs: 1,
            material: String::new(),
            flip: Default::default(),
            flip_center: [0.0; 3],
            tolerance: MESH_TOLERANCE,
            keep_out: vec![],
            fixtures: vec![],
            rot: None,
            more_bodies: vec![],
            part_min: [0.0; 3],
            part_max: [100.0, 60.0, 0.0],
            stock_body: None,
            program: None,
            comment: String::new(),
            bed_offset: [0.0; 2],
            machine: jc_post::machines::GENERIC_GRBL.into(),
            machine_values: jc_post::machines::generic_grbl(),
            leveling: None,
        }
    }

    impl Feature for Source {
        fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
            Ok(vec![])
        }

        fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
            let tp = Toolpath {
                name: "元".into(),
                tool: ToolLibrary::starter().tools[2].clone(),
                rpm: 12000.0,
                safe_z: 5.0,
                retract_z: 1.0,
                edge_break: 0.0,
                dwells: vec![],
                spans: vec![],
                links: Default::default(),
                moves: vec![
                    Move::Rapid {
                        to: [12.0, 10.0, 5.0],
                    },
                    Move::Linear {
                        to: [12.0, 10.0, -1.0],
                        feed: 100.0,
                    },
                    Move::Arc {
                        to: [12.0, 10.0, -1.0],
                        center: [10.0, 10.0],
                        dir: ArcDir::Ccw,
                        feed: 400.0,
                    },
                    Move::Rapid {
                        to: [12.0, 10.0, 5.0],
                    },
                ],
            };
            let mut out = Outputs::new();
            out.insert("setup".into(), info().to_value());
            out.insert(
                "toolpath".into(),
                jc_core::Payload::to_value(&ToolpathOutput {
                    setup: ctx.feature_id(),
                    path: tp,
                }),
            );
            Ok(out)
        }
    }

    fn registry() -> Registry {
        let mut r = Registry::new();
        r.register(SOURCE, |_| Ok(Box::new(Source)));
        crate::register(&mut r);
        r
    }

    /// 見本の機能の後にパターン `data` を置いて計算し、出力の経路と注意か、誤りを返す。
    fn eval(data: serde_json::Value) -> Result<(Toolpath, Vec<String>), String> {
        let mut doc = Document::new();
        let entry = |id: u64, type_id: &str, data: serde_json::Value| FeatureEntry {
            id: FeatureId(id),
            type_id: type_id.into(),
            name: type_id.into(),
            data,
            suppressed: false,
        };
        while doc.data().next_id <= 10 {
            doc.alloc_feature_id();
        }
        doc.apply(Transaction::new(
            "試験",
            vec![
                Op::InsertFeature {
                    index: None,
                    entry: entry(1, SOURCE, serde_json::json!({})),
                },
                Op::InsertFeature {
                    index: None,
                    entry: entry(2, PATTERN, data),
                },
            ],
        ))
        .unwrap();
        let r = doc.recompute(&registry(), &NullKernel);
        match r.feature(FeatureId(2)) {
            Some(FeatureStatus::Ok { outputs, .. }) => Ok((
                toolpath_of(&outputs["toolpath"]).unwrap().1,
                jc_core::warnings_of(outputs),
            )),
            other => Err(format!("{other:?}")),
        }
    }

    /// 経路の中の円弧の中心と向き。
    fn arcs(tp: &Toolpath) -> Vec<([f64; 2], ArcDir)> {
        tp.moves
            .iter()
            .filter_map(|m| match m {
                Move::Arc { center, dir, .. } => Some((*center, *dir)),
                _ => None,
            })
            .collect()
    }

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9
    }

    /// 受け入れ (c): 矩形の並びの写しの数と位置。元は含めない。
    #[test]
    fn a_rectangular_pattern_copies_the_path_on_a_grid() {
        let (tp, warn) = eval(serde_json::json!({
            "setup": 1, "source": 1, "kind": "rect",
            "count_x": 3, "count_y": 2, "spacing_x": "30 mm", "spacing_y": "25 mm"
        }))
        .unwrap();
        let a = arcs(&tp);
        assert_eq!(a.len(), 5, "3 × 2 から元を除いた 5 個");
        let want = [
            [40.0, 10.0],
            [70.0, 10.0],
            [10.0, 35.0],
            [40.0, 35.0],
            [70.0, 35.0],
        ];
        for (got, w) in a.iter().zip(want) {
            assert!(close(got.0, w), "{got:?} {w:?}");
            assert_eq!(got.1, ArcDir::Ccw);
        }
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(tp.tool.number, 3);
        assert_eq!(tp.rpm, 12000.0);
        assert!(jc_toolpath::check(&tp).is_empty());
    }

    /// 円形の並び: 一周なら等しく、範囲を決めれば両端を含める。
    #[test]
    fn a_circular_pattern_turns_the_copies_around_the_center() {
        let (tp, _) = eval(serde_json::json!({
            "setup": 1, "source": 1, "kind": "circle",
            "count": 4, "center_x": "30 mm", "center_y": "10 mm"
        }))
        .unwrap();
        let a = arcs(&tp);
        assert_eq!(a.len(), 3);
        for (got, w) in a.iter().zip([[30.0, -10.0], [50.0, 10.0], [30.0, 30.0]]) {
            assert!(close(got.0, w), "{got:?} {w:?}");
            assert_eq!(got.1, ArcDir::Ccw, "回しても回る向きは同じ");
        }
        let (tp, _) = eval(serde_json::json!({
            "setup": 1, "source": 1, "kind": "circle",
            "count": 3, "center_x": "30 mm", "center_y": "10 mm", "angle": "90 deg"
        }))
        .unwrap();
        let a = arcs(&tp);
        // 45 度と 90 度
        let r = 20.0 / std::f64::consts::SQRT_2;
        assert!(close(a[0].0, [30.0 - r, 10.0 - r]), "{a:?}");
        assert!(close(a[1].0, [30.0, -10.0]), "{a:?}");
    }

    /// 受け入れ (c): 鏡写しは円弧の向きが逆になり、ダウンカットがアップカットになる注意を出す。
    #[test]
    fn a_mirrored_copy_turns_the_other_way_and_warns() {
        let (tp, warn) = eval(serde_json::json!({
            "setup": 1, "source": 1, "kind": "mirror"
        }))
        .unwrap();
        // 線を決めなければストックの中央(x = 50)
        let a = arcs(&tp);
        assert_eq!(a.len(), 1);
        assert!(close(a[0].0, [90.0, 10.0]), "{a:?}");
        assert_eq!(a[0].1, ArcDir::Cw);
        assert!(warn.iter().any(|w| w.contains("アップカット")), "{warn:?}");
        let (tp, _) = eval(serde_json::json!({
            "setup": 1, "source": 1, "kind": "mirror", "mirror": "front_back", "at": "20 mm"
        }))
        .unwrap();
        assert!(close(arcs(&tp)[0].0, [10.0, 30.0]));
    }

    /// ストックの外を削る写しには注意を出す。数が多すぎる・少なすぎるのは誤り。
    #[test]
    fn copies_outside_the_stock_are_reported() {
        let (_, warn) = eval(serde_json::json!({
            "setup": 1, "source": 1, "kind": "rect", "count_x": 5, "spacing_x": "30 mm"
        }))
        .unwrap();
        assert!(
            warn.iter()
                .any(|w| w.contains("写し 4・5 個目") && w.contains("ストックの外")),
            "{warn:?}"
        );
        assert!(eval(serde_json::json!({"setup": 1, "source": 1, "count_x": 1})).is_err());
        assert!(
            eval(serde_json::json!({"setup": 1, "source": 1, "count_x": 11, "count_y": 10}))
                .is_err()
        );
        assert!(
            eval(serde_json::json!({"setup": 1, "source": 1, "kind": "circle", "angle": "5 mm"}))
                .is_err()
        );
    }

    #[test]
    fn the_pattern_and_the_manual_nc_keep_the_feature_contract() {
        let reg = registry();
        let src = UPSTREAM_IDS[0];
        check_feature(
            &reg,
            &Case {
                type_id: PATTERN,
                params: &[("gap", "35 mm")],
                upstream: &[(SOURCE, serde_json::json!({}))],
                data: serde_json::json!({"setup": src, "source": src, "spacing_x": "gap"}),
                bad_data: &[
                    serde_json::json!({"setup": src, "source": src, "count_x": 0}),
                    serde_json::json!({"setup": src, "source": src, "kind": "spiral"}),
                    serde_json::json!({"setup": src}),
                ],
            },
        );
        check_feature(
            &reg,
            &Case {
                type_id: crate::MANUAL_NC,
                params: &[],
                upstream: &[(SOURCE, serde_json::json!({}))],
                data: serde_json::json!({"setup": src, "kind": "stop", "text": "クランプ"}),
                bad_data: &[
                    serde_json::json!({"setup": src, "kind": "message"}),
                    serde_json::json!({"setup": src, "kind": "dwell", "seconds": -1}),
                ],
            },
        );
    }
}
