//! 有限の見本文書と選びで見つかる操作を、分野で絞らず一つずつ実行する。
use kmb_core::{
    Action, ArgKind, Context, Document, EdgePick, FacePick, FeatureId, FeatureStatus, Gesture,
    Input, Modules, Outcome,
};
use serde_json::json;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[path = "../tests/support/wide.rs"]
mod support;

type Fixture = (&'static str, fn() -> Document);
type Readiness = (usize, Reverse<usize>);
type Candidate = (usize, Selection, Option<String>, Readiness);

#[derive(Clone, Debug, Default)]
struct Selection {
    selected: Option<FeatureId>,
    picked: Option<FacePick>,
    edges: Vec<EdgePick>,
    also_picked: Vec<FacePick>,
    hit: Option<[f64; 3]>,
    args: serde_json::Value,
}
impl Selection {
    fn context<'a>(&'a self, cx: Context<'a>) -> Context<'a> {
        Context {
            selected: self.selected,
            picked: self.picked.as_ref(),
            edges: &self.edges,
            also_picked: &self.also_picked,
            args: &self.args,
            ..cx
        }
    }
}

fn contexts(
    doc: &Document,
    m: &Modules,
    report: &kmb_core::RecomputeReport,
) -> Result<Vec<Selection>, String> {
    let mut out = vec![Selection::default()];
    out.extend(doc.data().features.iter().map(|f| Selection {
        selected: Some(f.id),
        ..Default::default()
    }));
    let cx = Context::new(doc.data(), report, &m.registry, &m.kernel);
    let mut first_faces = Vec::new();
    for (body, shape) in cx.visible_bodies() {
        // 面に属する最大の三角形の内側を、実際にクリックできる代表点にする。
        let mesh = m.kernel.mesh(shape, 0.05).map_err(|e| e.to_string())?;
        if mesh.triangles.len() != mesh.triangle_faces.len() {
            return Err("三角形と面の対応が揃いません".into());
        }
        let mut points = BTreeMap::<String, (f64, [f64; 3])>::new();
        for (t, face) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
            let name = mesh
                .face_names
                .get(*face as usize)
                .ok_or("網の面の番号が不正です")?;
            let [a, b, c] = t.map(|i| mesh.positions[i as usize].map(f64::from));
            let ab: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
            let ac: [f64; 3] = std::array::from_fn(|i| c[i] - a[i]);
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let area = cross.into_iter().map(|n| n * n).sum::<f64>();
            let point = std::array::from_fn(|i| (a[i] + b[i] + c[i]) / 3.0);
            if area.is_finite()
                && area > 1e-16
                && points.get(name).is_none_or(|(old, _)| area > *old)
            {
                points.insert(name.clone(), (area, point));
            }
        }
        for face in m.kernel.faces(shape).map_err(|e| e.to_string())? {
            let hit = points.get(&face.name).map(|(_, point)| *point);
            let pick = FacePick {
                feature: body,
                face: face.name,
            };
            if first_faces.iter().all(|f: &FacePick| f.feature != body) {
                first_faces.push(pick.clone());
            }
            out.push(Selection {
                picked: Some(pick),
                hit,
                ..Default::default()
            });
        }
        let mut edges = Vec::new();
        for list in [
            m.kernel.edges(shape),
            m.kernel.arc_edges(shape),
            m.kernel.curve_edges(shape),
        ] {
            // 直線だけの形など、対応していない辺の種類は候補に無い。
            match list {
                Ok(list) => {
                    for edge in list {
                        if !edges.contains(&edge.faces) {
                            edges.push(edge.faces);
                        }
                    }
                }
                Err(kmb_core::KernelError::Unsupported(_)) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        out.extend(edges.into_iter().map(|faces| Selection {
            edges: vec![EdgePick {
                feature: body,
                faces,
            }],
            ..Default::default()
        }));
    }
    if let [a, b, ..] = first_faces.as_slice() {
        out.push(Selection {
            picked: Some(a.clone()),
            also_picked: vec![b.clone()],
            ..Default::default()
        });
    }
    Ok(out)
}

fn study_document() -> Document {
    support::study_document(crate::reference_document, kmb_fea::STATIC)
}

fn positive_cam_document() -> Document {
    let cam = crate::reference_cam();
    support::positive_cam(cam.part.doc, cam.tools, cam.setup)
}

fn skip_reason(action: &Action) -> Option<String> {
    if let Some(reason) = support::long_reason(&action.id) {
        return Some(reason.into());
    }
    if !matches!(action.input, Input::None) {
        return Some(format!("入力の窓が必要: {:?}", action.input));
    }
    let required: Vec<_> = action
        .args
        .iter()
        .filter(|a| a.required && a.default.is_none())
        .map(|a| a.key.as_str())
        .collect();
    if !required.is_empty() {
        return Some(format!("追加の引数が必要: {}", required.join(", ")));
    }
    action.disabled.clone()
}

/// 手順が位置を求め、同じキーのベクトル引数を宣言していれば、面の上の実点を渡す。
/// 未指定の参照を求める手順が少ない選びを先にする。手順の無い操作は判別できない。
fn prepare_selection(
    m: &Modules,
    action: &Action,
    selection: &Selection,
    cx: &Context<'_>,
) -> (Selection, usize) {
    let handles = m.extensions.handles(&action.id, cx).unwrap_or_default();
    let mut prepared = selection.clone();
    if let Some(point) = selection.hit.filter(|_| cx.picked_plane().is_some()) {
        for h in &handles {
            if !h.optional
                && matches!(h.gesture, Gesture::Point { .. })
                && action
                    .args
                    .iter()
                    .any(|a| a.key == h.key && matches!(a.kind, ArgKind::Vector))
            {
                if !prepared.args.is_object() {
                    prepared.args = json!({});
                }
                prepared
                    .args
                    .as_object_mut()
                    .unwrap()
                    .insert(h.key.clone(), json!(point));
            }
        }
    }
    let remaining = handles
        .iter()
        .filter(|h| {
            !h.optional
                && matches!(
                    h.gesture,
                    Gesture::Face
                        | Gesture::AnyFace
                        | Gesture::Edges
                        | Gesture::Choice
                        | Gesture::Regions
                )
        })
        .count();
    (prepared, remaining)
}

fn one_action(
    make: fn() -> Document,
    selection: &Selection,
    action: &str,
) -> Result<serde_json::Value, String> {
    let m = kmb_bundle::modules();
    let mut doc = make();
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    let before = doc.data().clone();
    let undo = doc.undo_labels().len();
    let geometry = support::geometry(&Context::new(doc.data(), &report, &m.registry, &m.kernel))?;
    let cx = selection.context(Context::new(doc.data(), &report, &m.registry, &m.kernel));
    let mut calculation_errors = Vec::new();
    let result = match m.extensions.run(action, &cx) {
        Err(problem) => {
            if problem.message.trim().is_empty() {
                return Err("種類付きの誤りの理由が空です".into());
            }
            json!({"result":"problem","kind":problem.kind,"reason":problem.message})
        }
        Ok(Outcome::Change { label, edits }) => {
            doc.apply_edits(&label, edits).map_err(|e| e.to_string())?;
            let changed = doc.recompute(&m.registry, m.kernel.as_ref());
            calculation_errors = changed
                .features
                .iter()
                .filter_map(|f| match &f.status {
                    FeatureStatus::Error { message } => Some(format!("{}: {message}", f.name)),
                    FeatureStatus::Blocked { by, .. } => Some(format!("{}: {by}", f.name)),
                    _ => None,
                })
                .collect();
            if !doc.undo() {
                return Err("変更を取り消せません".into());
            }
            json!({"result":"changed_and_undone"})
        }
        Ok(Outcome::Job(_)) => {
            json!({"result":"not_run","reason":"仕事の本体は長いので実行しない"})
        }
        Ok(outcome) => json!({"result":"read_only","outcome":outcome.brief()}),
    };
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    let after_geometry =
        support::geometry(&Context::new(doc.data(), &report, &m.registry, &m.kernel))?;
    support::restored(
        &before,
        doc.data(),
        &geometry,
        &after_geometry,
        undo,
        doc.undo_labels().len(),
    )?;
    if calculation_errors.is_empty() {
        Ok(result)
    } else {
        Ok(
            json!({"result":"failure", "reason":format!("変更後の計算に誤り: {}", calculation_errors.join("、")), "undo_restored":true}),
        )
    }
}

/// 確かめること: 同じ判定が、取り消しを忘れた実行器を見逃さず、正しく戻した実行器を受ける。
#[test]
fn the_shared_undo_verifier_detects_an_executor_that_does_not_undo() {
    let execute = |undo: bool| {
        let mut doc = Document::new();
        let before = doc.data().clone();
        let properties = kmb_core::Properties {
            title: "変えた名前".into(),
            ..Default::default()
        };
        doc.apply(kmb_core::Transaction::new(
            "名前",
            vec![kmb_core::Op::SetProperties {
                properties: Box::new(properties),
            }],
        ))
        .unwrap();
        if undo {
            assert!(doc.undo());
        }
        support::restored(
            &before,
            doc.data(),
            &vec![],
            &vec![],
            0,
            doc.undo_labels().len(),
        )
    };
    assert!(execute(false).is_err());
    assert!(execute(true).is_ok());
}

/// 確かめること: 見本と有限な選びで見つかった全操作を、実行か理由付き未実行のどちらかへ記録する。
#[test]
#[ignore = "全部の操作は時間がかかるので、週に 1 回の回と手で流す"]
fn wide_actions_are_typed_and_document_changes_are_undoable() {
    let start = std::time::Instant::now();
    let mut records = support::Report::new("actions");
    let fixtures: [Fixture; 6] = [
        ("設計", crate::reference_document),
        ("加工の正例", positive_cam_document),
        ("2.5D加工", crate::reference_cam_document),
        ("3D加工", crate::reference_cam_3d_document),
        ("組立", crate::drawing_assembly_document),
        ("研究", study_document),
    ];
    let mut choices: BTreeMap<String, Candidate> = BTreeMap::new();
    for (fixture, (name, make)) in fixtures.iter().enumerate() {
        let m = kmb_bundle::modules();
        let mut doc = make();
        let report = doc.recompute(&m.registry, m.kernel.as_ref());
        let selections = contexts(&doc, &m, &report).expect("見本の選びを列挙できる");
        let mut discovered = std::collections::BTreeSet::new();
        for selection in &selections {
            let cx = selection.context(Context::new(doc.data(), &report, &m.registry, &m.kernel));
            for action in m.extensions.actions(&cx) {
                discovered.insert(action.id.clone());
                let reason = skip_reason(&action);
                let closed_profile = selection
                    .selected
                    .and_then(|id| cx.output(id, "profile"))
                    .and_then(<kmb_core::Profile as kmb_core::Payload>::from_value)
                    .is_some_and(|p| !p.regions.is_empty());
                let wall_menu = action.is_at(kmb_core::Place::FaceMenu)
                    && cx.picked.is_some()
                    && cx.picked_plane().is_none();
                let base = usize::from(*name == "加工の正例" && action.group == "cam") * 4
                    + usize::from(action.group == "cam" && wall_menu) * 2
                    + usize::from(action.group == "cam" && closed_profile);
                // 同じ文書の適性で参照待ちがゼロなら、それ以降の同点候補を調べ直さない。
                if reason.is_none()
                    && choices.get(&action.id).is_some_and(|(_, _, old, rank)| {
                        old.is_none() && (rank.0 > base || (rank.0 == base && rank.1.0 == 0))
                    })
                {
                    continue;
                }
                let (prepared, remaining) = if reason.is_none() {
                    prepare_selection(&m, &action, selection, &cx)
                } else {
                    (selection.clone(), usize::MAX)
                };
                let score = (base, Reverse(remaining));
                let replace = choices
                    .get(&action.id)
                    .is_none_or(|(_, _, old, old_score)| {
                        (old.is_some() && reason.is_none())
                            || (old.is_none() && reason.is_none() && score > *old_score)
                    });
                if replace {
                    choices.insert(action.id, (fixture, prepared, reason, score));
                }
            }
        }
        records.event(json!({"wide":"coverage","fixture":name,"contexts":selections.len(),"actions":discovered.len(),"gap":"有限の見本と選びだけ。解いた研究結果・既存ジョイントの駆動・リンク元の変更は含まない"}));
    }
    let mut counts = BTreeMap::<String, usize>::new();
    let mut failures = Vec::new();
    for (action, (fixture, selection, reason, readiness)) in choices {
        let (name, make) = fixtures[fixture];
        if let Some(reason) = reason {
            *counts.entry("not_run".into()).or_default() += 1;
            records.event(json!({"wide":"action","fixture":name,"action":action,"result":"not_run","reason":reason}));
            continue;
        }
        records.event(json!({"wide":"starting_action","fixture":name,"action":action,"remaining_reference_steps":readiness.1.0}));
        let result = catch_unwind(AssertUnwindSafe(|| one_action(make, &selection, &action)));
        let result = match result {
            Ok(Ok(result)) => result,
            Ok(Err(reason)) => {
                json!({"result":"failure","reason":reason})
            }
            Err(_) => {
                json!({"result":"failure","reason":"panic"})
            }
        };
        if result["result"] == "failure" {
            failures.push(format!("{name}/{action}: {}", result["reason"]));
        }
        *counts
            .entry(result["result"].as_str().unwrap_or("unknown").into())
            .or_default() += 1;
        records.event(json!({"wide":"action","fixture":name,"action":action,"selection":format!("{selection:?}"),"outcome":result}));
    }
    records.event(
        json!({"wide":"action_counts","counts":counts,"seconds":start.elapsed().as_secs_f64()}),
    );
    assert!(
        failures.is_empty(),
        "広い操作の失敗:\n{}",
        failures.join("\n")
    );
}
