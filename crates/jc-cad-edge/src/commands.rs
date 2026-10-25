//! フィレット・面取りの種類の登録と、辺や面を選んで足す操作。

use super::*;

/// 半径(距離)が面の幅に収まらない時の直し方の例(フィレットと面取り)。
trait SizeFixes {
    fn fixes_for_size(self) -> Self;
}

impl SizeFixes for jc_core::Descriptor {
    fn fixes_for_size(self) -> Self {
        self.fix("大きすぎ", "半径(距離)を小さくする")
            .fix("大きすぎ", "「収まらないエッジだけ下げる」を入れる")
            .fix(
                "大きすぎ",
                "辺の選択を減らすか、丸める辺を分けて別の機能にする",
            )
    }
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    shell::register(reg);
    reg.register_described(
        solid(FILLET, "フィレット")
            .icon("fillet")
            .hint("辺を丸める。選んだ辺から、なめらかにつながる辺もたどる")
            .aliases("ふぃれっと fillet まるめ round")
            .retarget(Retarget::Pick {
                key: "rows.0.edges".into(),
            })
            .field(Field::output("target", "対象", "body"))
            .field(Field::rows(
                "rows",
                "エッジと半径",
                vec![
                    Field::pick("edges", "エッジ・面", &[RefKind::Edge, RefKind::Face], 1, None),
                    Field::expr("radius", "半径").length().help(
                        "一定: 半径。弦の長さ: 丸めの両端(両側の面に接する所)の間の距離。徐変: 始めの半径",
                    ),
                    Field::choice("kind", "半径タイプ", RADIUS_KINDS).help(
                        "一定: 辺に沿って同じ半径。弦の長さ: 面の角が変わっても丸めの幅をそろえる。徐変: 始めから終わりへ半径を変える(途中の点も置ける)。始めは辺の端のうち X の小さい方(同じなら Y、Z の小さい方)",
                    ),
                    Field::opt_expr("end_radius", "終わりの半径").length()
                        .help("空なら始めと同じ")
                        .only("@kind", "variable", "constant"),
                    Field::text("mid_radii", "途中の半径")
                        .help("「位置 = 半径」を ; で区切る。位置は始めの端を 0、終わりの端を 1 とした割合(例: 0.5 = 3 mm; 0.75 = 2 mm)。20 個まで")
                        .only("@kind", "variable", "constant"),
                    Field::choice("continuity", "連続性", CONTINUITIES).help(
                        "接線(G1): 円の断面で、面と接線までつながる。曲率(G2): 曲率までなめらかにつながる断面(光の映りがなめらか)",
                    ),
                ],
            ).help("選択セットごとに辺と半径を持つ。+ を押してから辺か面をクリックすると、新しい選択セットになる。面を選ぶとそのまわりの辺をすべて丸める"))
            .field(Field::opt_number("weight", "接線ウェイト").unitless().help(
                "曲率(G2)の行の断面のふくらみ。空なら 1。大きいほど面の近くまで面に沿う。接線(G1)の行には効かない",
            ))
            .field(Field::bool("chain", "つながるエッジも入れる").help(
                "選んだ辺から、なめらかに(2° 以内で)つながる辺もたどって丸める。上流で辺が増えても、計算のたびにたどり直す",
            ))
            .field(Field::bool("fit", "収まらないエッジだけ下げる").help(
                "半径が面の幅に収まらない辺だけを、収まる最大の半径に下げる(下げた辺は警告に出る)",
            ))
            .field(Field::choice("corner", "コーナーのタイプ", FILLET_CORNERS).help(
                "選んだ 3 本の辺が 1 つの角に集まる所の形。交わり: 丸めを延ばして交え、角に折れ目が集まる。球: 3 つの面に接する球でなめらかにつなぐ",
            ).only("method", "cut", "cut"))
            // 画面から作った時と同じ角の形にする(保存済みの文書は省いた欄を交わりで読む)
            .create_defaults(serde_json::json!({"corner": "ball", "chain": true, "method": "blend"}))
            .role(jc_interchange::role::kind::EDIT_FROM_FACE)
            .fixes_for_size()
            .upgrade(upgrade::<Fillet>),
        |d| Ok(Box::new(parse::<Fillet>(d)?)),
    );
    reg.register_described(
        solid(CHAMFER, "面取り")
            .icon("chamfer")
            .hint("辺を面取りする。選んだ辺から、なめらかにつながる辺もたどる")
            .aliases("めんとり chamfer")
            .retarget(Retarget::Pick {
                key: "rows.0.edges".into(),
            })
            .field(Field::output("target", "対象", "body"))
            .field(Field::choice("kind", "種類", CHAMFER_KINDS).help(
                "等距離: 両側の面から同じ距離。2 つの距離: 距離 1 の面と、もう片方の面で距離を変える。距離と角度: 距離 1 の面からの距離と、その面からの角度。距離 1 は上を向いた面(円の辺では平らな面)の側で、「反転」で反対の面にする",
            ))
            .field(Field::rows(
                "rows",
                "エッジと距離",
                vec![
                    Field::pick("edges", "エッジ・面", &[RefKind::Edge, RefKind::Face], 1, None),
                    Field::expr("distance", "距離").length(),
                    Field::opt_expr("distance2", "距離 2").length()
                        .help("空なら距離 1 と同じ")
                        .only("kind", "two_distances", "equal"),
                    Field::opt_expr("angle", "角度").angle()
                        .help("空なら 45 deg")
                        .only("kind", "distance_angle", "equal"),
                    Field::bool("flip", "反転").unless("kind", "equal").help(
                        "距離 1 を、反対の面の側にする",
                    ),
                ],
            ).help("選択セットごとに辺と距離を持つ。+ を押してから辺か面をクリックすると、新しい選択セットになる。面を選ぶとそのまわりの辺をすべて面取りする"))
            .field(Field::bool("chain", "つながるエッジも入れる").help(
                "選んだ辺から、なめらかに(2° 以内で)つながる辺もたどって面取りする。上流で辺が増えても、計算のたびにたどり直す",
            ))
            .field(Field::bool("fit", "収まらないエッジだけ下げる").help(
                "距離が面の幅に収まらない辺だけを、収まる最大の距離に下げる(下げた辺は警告に出る)",
            ))
            .field(Field::choice("corner", "コーナーのタイプ", CHAMFER_CORNERS).help(
                "選んだ 3 本の辺が 1 つの角に集まる所の形。交わり: 面取りを延ばして交え、角が点になる。三角の面: 角に平らな三角の面を置く",
            ).only("method", "cut", "cut"))
            .create_defaults(serde_json::json!({"corner": "patch", "chain": true, "method": "blend"}))
            .role(jc_interchange::role::kind::EDIT_FROM_FACE)
            .fixes_for_size()
            .upgrade(upgrade::<Chamfer>),
        |d| Ok(Box::new(parse::<Chamfer>(d)?)),
    );
}

use jc_core::{Action, Context, EdgePick, Extension, Gesture, Handle, Outcome, Place};

const NEED_EDGE: &str = "丸める辺を選んでいません";

/// 先に選んでいた辺か面があるか(あれば辺と面を選ぶ手順を飛ばす)。
fn preselected(cx: &Context<'_>) -> bool {
    !cx.edges.is_empty() || !cx.picked_faces().is_empty()
}

/// 辺と面を選ぶ手順の値・CLI・MCP の `edges` の 1 つ: 辺 `{"feature": 機能, "faces": [面, 面]}`
/// か、種類つきの参照(`{"kind": "face", "feature": 機能, "face": 面}` など)。
fn arg_ref(v: &serde_json::Value) -> Option<Ref> {
    if v.get("kind").is_some() {
        return serde_json::from_value(v.clone()).ok();
    }
    let e: EdgePick = serde_json::from_value(v.clone()).ok()?;
    Some(Ref::Edge {
        feature: e.feature,
        faces: e.faces,
    })
}

/// 掛ける辺と面: 指定の `edges`(辺と面を選ぶ手順の値、CLI・MCP)か、先に選んだ辺と面
/// (面はそのまわりの辺すべて)。1 つのボディのものだけ。
fn selection(cx: &Context<'_>) -> Result<(FeatureId, Vec<Ref>), String> {
    let refs: Vec<Ref> = match cx.arg("edges") {
        Some(v) => {
            let list = match v {
                serde_json::Value::Array(a) => a.clone(),
                one => vec![one.clone()],
            };
            list.iter().map(arg_ref).collect::<Option<Vec<_>>>().ok_or(
                "edges は [{\"feature\": 機能, \"faces\": [面, 面]}] か、面なら {\"kind\": \"face\", \"feature\": 機能, \"face\": 面} で指定します",
            )?
        }
        None => cx
            .edges
            .iter()
            .map(|e| Ref::Edge {
                feature: e.feature,
                faces: e.faces.clone(),
            })
            .chain(cx.picked_faces().into_iter().map(|p| Ref::Face {
                feature: p.feature,
                face: p.face.clone(),
            }))
            .collect(),
    };
    let first = refs.first().ok_or(NEED_EDGE)?.feature();
    if refs
        .iter()
        .any(|r| !matches!(r, Ref::Edge { .. } | Ref::Face { .. }))
    {
        return Err("辺か面を選んでください".into());
    }
    if refs.iter().any(|r| r.feature() != first) {
        return Err("辺と面は 1 つのボディから選んでください".into());
    }
    Ok((first, refs))
}

/// 面から足すフィレットの半径の既定(mm)。
const FILLET_RADIUS: f64 = 1.0;
/// 面から足す面取りの距離の既定(mm)。
const CHAMFER_DISTANCE: f64 = 0.5;

/// 選んだ辺と、選んだ面のまわりの出っ張った辺(出っ張った辺がなければへこんだ辺)を
/// 丸める(`round`)か面取りする。値は `radius`・`distance`(mm)で
/// 指定でき、なければ既定の値で足す(画面は「作っている途中」にして欄で直させる)。
fn edge_feature(cx: &Context<'_>, round: bool) -> Result<Outcome, String> {
    let (target, select) = selection(cx)?;
    let value = |key: &str, default: f64| -> Result<String, String> {
        let v = cx.arg_f64(key)?.unwrap_or(default);
        if v <= 0.0 {
            return Err(format!("{key} は 0 より大きくしてください"));
        }
        Ok(format!("{} mm", mm(v)))
    };
    Ok(if round {
        Outcome::insert(
            "フィレットを追加",
            FILLET,
            format!("フィレット{}", cx.count_of(FILLET) + 1),
            serde_json::to_value(Fillet {
                target,
                rows: vec![FilletRow::new(
                    select,
                    value("radius", FILLET_RADIUS)?,
                    false,
                )],
                fit: false,
                corner: Corner::Ball,
                chain: true,
                method: Method::Blend,
                weight: None,
            })
            .expect("JSON にできる"),
        )
    } else {
        Outcome::insert(
            "面取りを追加",
            CHAMFER,
            format!("面取り{}", cx.count_of(CHAMFER) + 1),
            serde_json::to_value(Chamfer {
                target,
                kind: ChamferKind::Equal,
                rows: vec![ChamferRow::new(
                    select,
                    value("distance", CHAMFER_DISTANCE)?,
                    false,
                )],
                fit: false,
                corner: Corner::Patch,
                chain: true,
                method: Method::Blend,
            })
            .expect("JSON にできる"),
        )
    })
}

/// フィレットと面取りの操作。
pub struct EdgeCommands;

impl Extension for EdgeCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let face = cx.picked_plane().is_some();
        let on_face: &[Place] = if face { &[Place::FaceMenu] } else { &[] };
        let edge_hint = "押してから辺か面をクリックすると、それに掛けて値を決める欄へ進む(面はそのまわりの辺すべて。いくつも選ぶ時は Ctrl を押しながらクリック)。先に辺や面を選んでいればそれに掛ける";
        vec![
            Action::new(FILLET_FACE, "フィレット", "cad")
                .arg(jc_core::Arg::pick(
                    "edges",
                    "辺 [{\"feature\": 機能, \"faces\": [面, 面]}] か面 {\"kind\": \"face\", \"feature\": 機能, \"face\": 面}(省くと選んでいる辺・面)",
                    &[jc_core::RefKind::Edge, jc_core::RefKind::Face],
                ))
                .arg(jc_core::Arg::length("radius", "半径(mm)").default(FILLET_RADIUS))
                .role(jc_interchange::role::action::FILLET_EDGES)
                .menu("修正")
                .icon("fillet")
                .aliases("ふぃれっと fillet まるめ round")
                .hint(edge_hint)
                .at(&[Place::Toolbar])
                .at(on_face),
            Action::new(CHAMFER_FACE, "面取り", "cad")
                .arg(jc_core::Arg::pick(
                    "edges",
                    "辺 [{\"feature\": 機能, \"faces\": [面, 面]}] か面 {\"kind\": \"face\", \"feature\": 機能, \"face\": 面}(省くと選んでいる辺・面)",
                    &[jc_core::RefKind::Edge, jc_core::RefKind::Face],
                ))
                .arg(jc_core::Arg::length("distance", "距離(mm)").default(CHAMFER_DISTANCE))
                .menu("修正")
                .icon("chamfer")
                .aliases("めんとり chamfer")
                .hint(edge_hint)
                .at(&[Place::Toolbar])
                .at(on_face),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        Some(match id {
            FILLET_FACE => edge_feature(cx, true),
            CHAMFER_FACE => edge_feature(cx, false),
            _ => return None,
        })
    }

    /// 先に辺も面も選んでいなければ、辺と面を選ぶ手順から始める(
    /// 面はそのまわりの辺すべて)。
    /// 選び終えたら既定の値で足し、画面は値を欄で直す「作っている途中」にする。
    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if (id != FILLET_FACE && id != CHAMFER_FACE) || preselected(cx) {
            return None;
        }
        let what = if id == FILLET_FACE {
            "丸める"
        } else {
            "面取りする"
        };
        Some(vec![Handle {
            chain: true,
            faces: true,
            ..Handle::new(
                "edges",
                &format!(
                    "{what}辺か面をクリック(面はまわりの辺すべて。つながる辺も入る。いくつも選ぶ時は Ctrl を押しながら)"
                ),
                Gesture::Edges,
            )
        }])
    }
}
