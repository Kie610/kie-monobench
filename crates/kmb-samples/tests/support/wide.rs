// 広い試験が使う、文書全体・形・取り消しの共通の判定。画面の値を変更する口は持たない。
use kmb_core::{Context, DocumentData};

pub type Geometry = Vec<(u64, [i64; 7])>;

pub fn geometry(cx: &Context<'_>) -> Result<Geometry, String> {
    let mut out = Vec::new();
    for (id, shape) in cx.visible_bodies() {
        let mass = cx.kernel.mass(shape).map_err(|e| e.to_string())?;
        let (lo, hi) = cx.kernel.bounds(shape).map_err(|e| e.to_string())?;
        if !mass.volume.is_finite() || !lo.into_iter().chain(hi).all(f64::is_finite) {
            return Err("形の寸法か体積が有限ではありません".into());
        }
        let mut values = [0; 7];
        values[0] = (mass.volume * 1e3).round() as i64;
        for (i, n) in lo.into_iter().chain(hi).enumerate() {
            values[i + 1] = (n * 1e6).round() as i64;
        }
        out.push((id.0, values));
    }
    out.sort_by_key(|(id, _)| *id);
    Ok(out)
}

pub fn restored(
    before: &DocumentData,
    after: &DocumentData,
    before_geometry: &Geometry,
    after_geometry: &Geometry,
    before_undo: usize,
    after_undo: usize,
) -> Result<(), String> {
    // ID の割当は取り消しても進む。保存時と同じく、この値だけを比べ方から外す。
    let mut after = after.clone();
    after.next_id = before.next_id;
    if before != &after {
        return Err("取り消しても文書の中身が戻りません".into());
    }
    if before_geometry != after_geometry {
        return Err("取り消しても体積か外形が戻りません".into());
    }
    if before_undo != after_undo {
        return Err("取り消しの履歴が起点に戻りません".into());
    }
    Ok(())
}

/// 同期で開始し得る長い計算。発見した故障をここへ移して試験を通してはいけない。
pub fn long_reason(id: &str) -> Option<&'static str> {
    match id {
        "study.run" => Some("解析の本計算は長いので、実行しない"),
        "asm.motion_study" => Some("ジョイントの連続した干渉計算は実行しない"),
        "cam.simulate" | "cam.simulate_gcode" | "cam.play.start" => {
            Some("材料を削るシミュレーションの本計算は実行しない")
        }
        "cam.export_sim_video" | "render.still" | "render.turntable" | "render.animation_video" => {
            Some("画像・動画を作る長い本計算は実行しない")
        }
        "u:canvas_render" => Some("キャンバス内レンダリングは継続する計算なので実行しない"),
        _ => None,
    }
}

/// 成功時に標準出力が隠れても、操作ごとの結果を検査のフォルダに残す。
pub struct Report(std::fs::File);
impl Report {
    pub fn new(name: &str) -> Self {
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")
            });
        let dir = target.join("verify/wide/cases");
        std::fs::create_dir_all(&dir).expect("広い試験の記録フォルダを作れる");
        let path = dir.join(format!("{name}.jsonl"));
        eprintln!("広い試験の記録: {}", path.display());
        Self(std::fs::File::create(path).expect("広い試験の記録を作れる"))
    }
    pub fn event(&mut self, value: serde_json::Value) {
        use std::io::Write;
        eprintln!("{value}");
        writeln!(self.0, "{value}").expect("広い試験の結果を書ける");
        self.0.flush().expect("広い試験の結果を残せる");
    }
}

/// 工具の種類・届く長さ・ストックの余白を備えた加工の正例。製品の既定値は変えない。
pub fn positive_cam(
    mut doc: kmb_core::Document,
    tools: kmb_core::FeatureId,
    setup_id: kmb_core::FeatureId,
) -> kmb_core::Document {
    use kmb_cam_commands::{Tool, ToolKind, ToolLibrary};
    let mut library: ToolLibrary =
        serde_json::from_value(doc.data().feature(tools).unwrap().data.clone()).unwrap();
    // 小径の穴にも入る、既存の 3.175 mm の平工具を既定の番号にも用意する。
    let mut first = library
        .tools
        .iter()
        .find(|t| t.number == 2)
        .unwrap()
        .clone();
    first.number = 1;
    // 既定のピン穴の深さ 19 mm に、5 mm の届く長さを加える。
    first.flute_length = 24.0;
    let index = library.tools.iter().position(|t| t.number == 1).unwrap();
    library.tools[index] = first;
    let mut laser = ToolLibrary::laser_tool();
    laser.number = 7;
    library.tools.push(laser);
    library.tools.push(Tool {
        number: 8,
        name: "ねじ切り 6 mm".into(),
        kind: ToolKind::ThreadMill,
        diameter: 6.0,
        flute_length: 14.0,
        tip_angle: 60.0,
        shank_diameter: Some(6.0),
        thread: Some(kmb_cam_commands::tool::ThreadForm {
            teeth: 1,
            pitch: None,
            neck_diameter: Some(4.0),
        }),
        ..library.tools[0].clone()
    });
    let v = library.tools.iter_mut().find(|t| t.number == 5).unwrap();
    v.name = "V 30度 3.175 mm".into();
    v.tip_angle = 30.0;
    v.flute_length = 10.0;
    library.check().unwrap();
    let mut setup = doc.data().feature(setup_id).unwrap().data.clone();
    setup["stock"]["margin_front"] = serde_json::json!("15 mm");
    setup["stock"]["margin_back"] = serde_json::json!("15 mm");
    doc.apply_edits(
        "加工の正例",
        vec![
            kmb_core::Edit::SetData {
                id: tools,
                data: serde_json::to_value(library).unwrap(),
            },
            kmb_core::Edit::SetData {
                id: setup_id,
                data: setup,
            },
        ],
    )
    .unwrap();
    doc
}

pub fn study_document(make: fn() -> kmb_core::Document, study_type: &str) -> kmb_core::Document {
    let m = kmb_bundle::modules();
    let mut doc = make();
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = Context::new(doc.data(), &report, &m.registry, &m.kernel);
    let (body, shape) = cx.visible_bodies()[0];
    let faces = m.kernel.faces(shape).unwrap();
    let fixed = faces
        .iter()
        .find(|f| f.planar.is_some_and(|p| p.normal[2] < -0.99))
        .unwrap();
    let load = faces
        .iter()
        .find(|f| f.planar.is_some_and(|p| p.normal[2] > 0.99))
        .unwrap();
    let data = serde_json::json!({"material": "アルミ", "fixed": {"kind":"face", "feature":body.0,"face":fixed.name},
        "loads":[{"faces":{"kind":"face","feature":body.0,"face":load.name},"kind":"force","value":"100","dir":"-z"}],"size":"5 mm"});
    doc.apply_edits(
        "研究",
        vec![kmb_core::Edit::Insert {
            type_id: study_type.into(),
            name: "静的応力".into(),
            data,
        }],
    )
    .unwrap();
    // 評価済みの形のキャッシュは、その形を持つカーネルの外へ持ち出さない。
    kmb_core::Document::from_json(&doc.to_json()).expect("研究の見本を保存形から読める")
}
