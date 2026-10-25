//! 機能の設定欄。欄は機能の記述子([`kmb_core::Descriptor`])から作るので、
//! 機能を足しても画面のコードは変えなくてよい。欄にない項目は「詳細(JSON)」で編集する。

use crate::Workspace;
use kmb_core::{FeatureEntry, FeatureId, FeatureStatus, Field, FieldKind};
use serde_json::Value as Json;

/// `before` より前にあり、出力 `slot` を持つ機能。
fn features_with(ws: &Workspace, before: FeatureId, slot: &str) -> Vec<(FeatureId, String)> {
    let mut out = Vec::new();
    for f in &ws.cur.work.doc().data().features {
        if f.id == before {
            break;
        }
        if ws.output(f.id, slot).is_some() {
            out.push((f.id, f.name.clone()));
        }
    }
    out
}

/// ボディ 1 つ分の段階(ボディを変えた機能の、タイムラインの順)。
struct BodyStages {
    /// ボディの名前(付けた名前か「ボディN」。最後まで残らないボディは最初に作った機能の名前で)
    title: String,
    /// (機能, 機能の名前)。最後が `before` より前の最新の形
    stages: Vec<(FeatureId, String)>,
}

/// `before` より前の、ボディを出す機能を、ボディごとの段階にまとめる(ボディは最初に作った順)。
fn body_stages(ws: &Workspace, before: FeatureId) -> Vec<BodyStages> {
    let mut out: Vec<(FeatureId, BodyStages)> = Vec::new();
    for (id, name) in features_with(ws, before, "body") {
        let root = ws.body_root(id);
        match out.iter_mut().find(|b| b.0 == root) {
            Some(b) => b.1.stages.push((id, name)),
            None => {
                let title = ws.body_name_of(root);
                // 最後まで残らないボディ(ほかのボディに足した・引いた道具)は番号がない
                let title = if title == "ボディ0" {
                    ws.body_label(root)
                } else {
                    title
                };
                out.push((
                    root,
                    BodyStages {
                        title,
                        stages: vec![(id, name)],
                    },
                ));
            }
        }
    }
    out.into_iter().map(|b| b.1).collect()
}

/// 段階の 1 行の名前(「穴1 の後」)と、最新か途中かの添え書き。
fn stage_text(b: &BodyStages, k: usize) -> (String, String) {
    let n = b.stages.len();
    let note = if k + 1 == n {
        "最新".to_string()
    } else {
        format!("{}/{n} 段目", k + 1)
    };
    (format!("{} の後", b.stages[k].1), note)
}

/// 加工の「部品」などボディを選ぶ欄: どのボディの、どの段階(どの機能まで)の形かが分かるよう、
/// ボディごとにまとめて段階を並べる。
fn body_stage_field(
    ui: &mut egui::Ui,
    ws: &Workspace,
    entry: &FeatureEntry,
    f: &Field,
    data: &mut Json,
    key: egui::Id,
    optional: bool,
) {
    let bodies = body_stages(ws, entry.id);
    let cur = f.get(data).and_then(Json::as_u64).map(FeatureId);
    let found = cur.and_then(|id| {
        bodies
            .iter()
            .find_map(|b| b.stages.iter().position(|s| s.0 == id).map(|k| (b, k)))
    });
    let shown = match (cur, found) {
        (None, _) => "(なし)".to_string(),
        (Some(_), Some((b, k))) => {
            // 欄は狭いので、ボディと段階(最新か何段目か)を前に、機能の名前を後ろに置く
            // (はみ出す時は後ろを省き、全部は欄に乗せると出る)
            let (text, note) = stage_text(b, k);
            let note = note.replace(" 段目", "");
            format!("{}・{note}({text})", b.title)
        }
        (Some(id), None) => format!("{id}(使えません)"),
    };
    egui::ComboBox::from_id_salt(key)
        .selected_text(shown.clone())
        // 欄の幅はほかの欄にそろえる(広げると編集パネルの右の「ƒ」がはみ出す)
        .width(110.0)
        .truncate()
        .show_ui(ui, |ui| {
            if optional && ui.selectable_label(cur.is_none(), "(なし)").clicked() {
                f.set(data, None);
            }
            for b in &bodies {
                ui.label(egui::RichText::new(&b.title).strong());
                for (k, (id, _)) in b.stages.iter().enumerate() {
                    let (text, note) = stage_text(b, k);
                    let at = ws
                        .cur
                        .work
                        .doc()
                        .data()
                        .index_of(*id)
                        .map_or(String::new(), |i| {
                            format!("タイムラインの {} 番目の機能。", i + 1)
                        });
                    if ui
                        .selectable_label(cur == Some(*id), format!("    {text}({note})"))
                        .on_hover_text(format!(
                            "{at}{} の、{} まで作った形を使う",
                            b.title, b.stages[k].1
                        ))
                        .clicked()
                    {
                        f.set(data, Some(Json::from(id.0)));
                    }
                }
            }
        })
        .response
        .on_hover_text(format!(
            "{shown}。一覧から、ボディとどの機能の後の形を削るかを選ぶ"
        ));
}

/// 加工の機能(セットアップ・加工)か。
fn is_cam(ws: &Workspace, entry: &FeatureEntry) -> bool {
    ws.win.registry.descriptor(&entry.type_id).is_some_and(|d| {
        d.category == kmb_core::category::SETUP || d.category == kmb_core::category::OPERATION
    })
}

fn as_text(v: Option<&Json>) -> String {
    match v {
        Some(Json::String(s)) => s.clone(),
        // 辺の一覧(面の名前の組)は数だけ
        Some(Json::Array(a)) if !a.is_empty() && a.iter().all(Json::is_array) => {
            format!("{} 本", a.len())
        }
        Some(Json::Array(a)) => a
            .iter()
            .map(|x| x.as_str().map_or_else(|| x.to_string(), String::from))
            .collect::<Vec<_>>()
            .join("、"),
        Some(Json::Null) | None => String::new(),
        Some(v) => v.to_string(),
    }
}

/// 式の値を、スライダの単位(`deg` なら度、ほかは式の値のまま)の数にする。
fn slider_value(q: kmb_core::Quantity, unit: &str) -> f64 {
    if unit == "deg" && q.dim == kmb_core::Dim::ANGLE {
        q.value.to_degrees()
    } else {
        q.value
    }
}

/// 小数 4 桁までの数(余分な 0 を書かない)。
fn short_number(v: f64) -> String {
    format!("{}", (v * 1e4).round() / 1e4 + 0.0)
}

/// 欄 `f` の値を収める範囲を決めるスライダ。スライダの付いた値の欄なら自分の物、戻る位置の欄
/// ([`kmb_core::Slider::rest`])なら値の欄の物。
fn range_slider(ws: &Workspace, entry: &FeatureEntry, f: &Field) -> Option<kmb_core::Slider> {
    if let Some(s) = &f.slider {
        return Some(s.clone());
    }
    let FieldKind::Expr { optional: true } = f.kind else {
        return None;
    };
    ws.win
        .registry
        .descriptor(&entry.type_id)?
        .fields
        .into_iter()
        .find_map(|g| g.slider.filter(|s| s.rest == f.key))
}

/// スライダ `s` の範囲(欄 `min`・`max` の値。空か誤りの側は `None`)。
fn slider_bounds(ws: &Workspace, s: &kmb_core::Slider, data: &Json) -> (Option<f64>, Option<f64>) {
    let read = |key: &str| {
        let t = as_text(Field::info(key, "").get(data));
        (!t.trim().is_empty())
            .then(|| ws.eval_expr(&t).ok())
            .flatten()
            .map(|q| slider_value(q, &s.unit))
    };
    (read(&s.min), read(&s.max))
}

/// 式 `text` の値が範囲 `s` の外なら、その範囲を書いた理由(
/// 範囲の外の値は赤くして受け付けない)。範囲が無い・式が誤りの時は `None`(誤りは別に出す)。
fn out_of_range(ws: &Workspace, s: &kmb_core::Slider, data: &Json, text: &str) -> Option<String> {
    let (lo, hi) = slider_bounds(ws, s, data);
    if lo.is_none() && hi.is_none() {
        return None;
    }
    let v = slider_value(ws.eval_expr(text).ok()?, &s.unit);
    let eps = 1e-9;
    if !(lo.is_some_and(|l| v < l - eps) || hi.is_some_and(|h| v > h + eps)) {
        return None;
    }
    let side = |x: Option<f64>| {
        x.map_or_else(|| "―".into(), |x| format!("{} {}", short_number(x), s.unit))
    };
    Some(format!("範囲 {}〜{} の外", side(lo), side(hi)))
}

/// 式の欄の下のスライダ([`kmb_core::Slider`])。引くと式を `<数> <単位>` に書き換える(引いている間も
/// 形が動いて見える)。範囲は欄 `min`・`max` の値で、空か誤りなら既定の範囲。
fn slider_ui(
    ui: &mut egui::Ui,
    ws: &Workspace,
    entry: &FeatureEntry,
    f: &Field,
    s: &kmb_core::Slider,
    data: &mut Json,
) {
    let (min, max) = slider_bounds(ws, s, data);
    let lo = min.unwrap_or(s.lo);
    let hi = max.unwrap_or(s.hi).max(lo + 1e-6);
    let cur = ws
        .eval_expr(&as_text(f.get(data)))
        .map(|q| slider_value(q, &s.unit))
        .unwrap_or(0.0);
    let mut v = cur.clamp(lo, hi);
    // 欄の名前の後ろを幅いっぱいに使う。値は上の欄に出るので、スライダには出さない(値で幅が揺れない)
    let r = ui
        .horizontal(|ui| {
            ui.label(field_label(ws, f));
            // 範囲の端から端まで動かして見せる(形と値は変えない)
            crate::joint_preview_ui::button(ui, ws, entry.id, lo, hi);
            ui.spacing_mut().slider_width = (ui.available_width() - 4.0).max(80.0);
            ui.add(
                egui::Slider::new(&mut v, lo..=hi)
                    .show_value(false)
                    .clamping(egui::SliderClamping::Always),
            )
        })
        .inner;
    let r = r.on_hover_text(format!(
        "引くと動いて見える({lo:.0}〜{hi:.0} {}。下限・上限を入れるとその範囲。細かい値は上の欄に打つ)",
        s.unit
    ));
    // 引き始めたら、上の欄の打ちかけの字を捨てて欄から外す(欄の外を押した時に、打ちかけの数が
    // 引いた値を上書きしない。採用分 10-24 u9)
    if r.drag_started() || r.changed() {
        let key = crate::doc_id(ui.ctx(), ("field", entry.id, &f.key));
        ui.data_mut(|d| d.remove::<String>(key));
        ui.memory_mut(|m| m.stop_text_input());
    }
    if r.changed() && (v - cur).abs() > 1e-9 {
        log::debug!(
            "{} の {} をスライダで {v:.1} {}",
            entry.name,
            f.label,
            s.unit
        );
        // 1 桁に丸め、整数なら小数を書かない(「60.0」と書くと欄に「= 60」の添え書きが出て、表の幅が揺れる)
        let v = (v * 10.0).round() / 10.0 + 0.0;
        let text = if v.fract() == 0.0 {
            format!("{v:.0} {}", s.unit)
        } else {
            format!("{v:.1} {}", s.unit)
        };
        f.set(data, Some(Json::String(text)));
    }
    // 戻る位置があれば、離すとその値に戻す(部品を 3D のビューで引いた時と同じ)
    let rest = as_text(Field::info(&s.rest, "").get(data));
    if (r.drag_stopped() || r.clicked())
        && !s.rest.is_empty()
        && !rest.trim().is_empty()
        && let Ok(q) = ws.eval_expr(&rest)
    {
        let v = slider_value(q, &s.unit).clamp(lo, hi);
        let text = format!("{} {}", short_number(v), s.unit);
        log::info!(
            "{} の {} を離したので戻る位置 {text} に戻す",
            entry.name,
            f.label
        );
        f.set(data, Some(Json::String(text)));
    }
}

/// 画面に出す欄の名前。長さの欄の名前に書いた「(mm…)」は文書の長さの単位で出す
/// (インチの文書の「切削送り(mm/min)」は「切削送り(in/min)」)。長さの欄は打った数を文書の
/// 単位で読むので、名前が mm のままだと、500 と打った送りが 500 in/min(12700 mm/min)になるのに
/// 名前は mm/min と読めてしまう。
pub(crate) fn field_label(ws: &Workspace, f: &Field) -> String {
    let unit = ws.length_unit();
    if f.unit == Some(kmb_core::FieldUnit::Length) && unit != kmb_core::LengthUnit::Mm {
        f.label.replacen("(mm", &format!("({}", unit.name()), 1)
    } else {
        f.label.clone()
    }
}

/// 記述子の欄を 1 つ描く。値を変えたら `data` を書き換える。
pub(crate) fn field_ui(
    ui: &mut egui::Ui,
    ws: &Workspace,
    entry: &FeatureEntry,
    f: &Field,
    data: &mut Json,
    newly_applies: bool,
) {
    marked_field_ui(ui, ws, entry, f, data, newly_applies, false);
}

/// [`field_ui`] の、名前を今の手順として強調するもの(`mark`)。
fn marked_field_ui(
    ui: &mut egui::Ui,
    ws: &Workspace,
    entry: &FeatureEntry,
    f: &Field,
    data: &mut Json,
    newly_applies: bool,
    mark: bool,
) {
    let text = if mark {
        egui::RichText::new(field_label(ws, f))
            .strong()
            .color(crate::theme::look(ui.ctx()).ws())
    } else {
        egui::RichText::new(field_label(ws, f))
    };
    // 選ぶ欄は中身の一覧で縦に伸びるので、名前は上の段(ボタン)の高さに置く
    let label = if matches!(f.kind, FieldKind::Pick { .. }) {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.add_space(3.0);
            ui.label(text)
        })
        .inner
    } else {
        ui.label(text)
    };
    if !f.help.is_empty() {
        label.on_hover_text(&f.help);
    }
    value_ui(ui, ws, entry, f, data, newly_applies);
}

/// 欄の値の部分(名前は除く)。行の並びの行の中でも使う。
fn value_ui(
    ui: &mut egui::Ui,
    ws: &Workspace,
    entry: &FeatureEntry,
    f: &Field,
    data: &mut Json,
    newly_applies: bool,
) {
    let key = crate::doc_id(ui.ctx(), ("field", entry.id, &f.key));
    match &f.kind {
        FieldKind::Expr { optional } => {
            let cur = as_text(f.get(data));
            ui.horizontal(|ui| {
                // 作り始めた機能は、最初の式の欄にキーボードを移す(すぐ数字を打てるように)
                // (表が大きさを測るだけの描画では移せないので、移せるまで続ける)
                let focus = ws.cur.focus_new.get() == Some(entry.id);
                // 省ける欄は、空の時に説明(例: 空なら貫通)を薄く出す
                let hint = if *optional { f.help.as_str() } else { "" };
                // 範囲のある欄(ジョイントの値と戻る位置)は、範囲の外の値を赤くして受け付けない
                let range = range_slider(ws, entry, f);
                let check = |t: &str| {
                    let s = range.as_ref()?;
                    out_of_range(ws, s, data, &typed_expr(ws, f, &cur, t))
                        .map(|why| format!("{} は{why}なので入れられません", t.trim()))
                };
                let (mut new, focused) =
                    expr_field_reserving(ui, key, &cur, focus, hint, 0.0, &check);
                if focus && focused {
                    ws.cur.focus_new.set(None);
                }
                // 打つ手を止めたら、打っている途中の値で形を付いて来させる(決めるのは前と同じく
                // Enter か欄の外。採用分 10-22 k10)
                if new.is_none() && focused {
                    new = typing_preview(ui, key, &cur, &|t| {
                        let v = typed_expr(ws, f, &cur, t);
                        v != cur
                            && check(t).is_none()
                            && ws.eval_expr(&v).is_ok_and(|q| {
                                kmb_session::params::unit_mismatch(f.unit.as_ref(), q.dim).is_none()
                            })
                    });
                }
                // パラメータ名の候補。押すと式の後ろに足す
                let names: Vec<String> = ws
                    .cur
                    .work
                    .doc()
                    .data()
                    .params
                    .iter()
                    .map(|p| p.name.clone())
                    .collect();
                if !names.is_empty() {
                    ui.menu_button("ƒ", |ui| {
                        for n in &names {
                            if ui.button(n).clicked() {
                                let base = new.clone().unwrap_or_else(|| cur.clone());
                                new = Some(if base.trim().is_empty() {
                                    n.clone()
                                } else {
                                    format!("{} {n}", base.trim_end())
                                });
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("パラメータの名前を入れる");
                }
                if let Some(v) = new {
                    let v = typed_expr(ws, f, &cur, &v);
                    if v.is_empty() && *optional {
                        f.set(data, None);
                    } else {
                        f.set(data, Some(Json::String(v)));
                    }
                }
                // 範囲の外で受け付けなかった印(次に欄を触るまで)。欄の横は狭いので、理由は吹き出しに
                if let Some(why) = refused(ui.ctx(), key) {
                    ui.label(
                        egui::RichText::new("範囲外")
                            .color(ui.visuals().error_fg_color)
                            .small(),
                    )
                    .on_hover_text(why);
                }
                // 今の値を単位つきで出す(式の誤りは赤で)
                let now = as_text(f.get(data));
                if !now.trim().is_empty() {
                    match ws.eval_expr(&now) {
                        // 長さの欄に角度のような取り違えは、計算を待たずにここで知らせる(U48)
                        Ok(q)
                            if kmb_session::params::unit_mismatch(f.unit.as_ref(), q.dim)
                                .is_some() =>
                        {
                            let why = kmb_session::params::unit_mismatch(f.unit.as_ref(), q.dim)
                                .unwrap_or_default();
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("単位の誤り: {why}"))
                                        .color(ui.visuals().error_fg_color)
                                        .small(),
                                )
                                .wrap(),
                            );
                        }
                        // 打った値そのものなら繰り返さない(式の時だけ値を出す。長さは文書の単位で)
                        Ok(q) if ws.shown(q).replace(' ', "") == now.replace(' ', "") => {}
                        Ok(q) => {
                            ui.weak(format!("= {}", ws.shown(q)));
                        }
                        // 理由は吹き出しにせず欄の横に出す(U48: 「未定義の名前: foo」)
                        Err(e) => {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("式の誤り: {e}"))
                                        .color(ui.visuals().error_fg_color)
                                        .small(),
                                )
                                .wrap(),
                            );
                        }
                    }
                }
            });
        }
        FieldKind::Number { integer, optional } => {
            let cur = as_text(f.get(data));
            // 候補があれば一覧から選ぶ(工具など)
            if let Some(list) = suggestions(ws, entry.id, data, f).filter(|l| !l.is_empty()) {
                let shown = list
                    .iter()
                    .find(|c| c.value == cur)
                    .map_or_else(|| format!("{cur}(一覧にない)"), |c| c.label.clone());
                egui::ComboBox::from_id_salt(key)
                    .selected_text(shown)
                    .width((ui.available_width() - 8.0).clamp(90.0, 180.0))
                    .show_ui(ui, |ui| {
                        for c in &list {
                            if ui.selectable_label(c.value == cur, &c.label).clicked() {
                                let v = if *integer {
                                    c.value.parse::<u64>().ok().map(Json::from)
                                } else {
                                    c.value.parse::<f64>().ok().map(Json::from)
                                };
                                if v.is_some() {
                                    f.set(data, v);
                                }
                            }
                        }
                    });
                return;
            }
            if let Some(v) = expr_field(ui, key, &cur) {
                let v = v.trim();
                if v.is_empty() && *optional {
                    f.set(data, None);
                } else if *integer {
                    // 読めない値は捨てる(欄は元の値に戻る)
                    if let Ok(n) = v.parse::<u64>() {
                        f.set(data, Some(Json::from(n)));
                    }
                } else if let Ok(x) = v.parse::<f64>() {
                    f.set(data, Some(Json::from(x)));
                }
            }
        }
        FieldKind::Text | FieldKind::File => {
            let cur = f.get(data).and_then(Json::as_str).unwrap_or("").to_string();
            ui.horizontal(|ui| {
                if let Some(v) = expr_field(ui, key, &cur) {
                    let v = v.trim().to_string();
                    f.set(data, (!v.is_empty()).then_some(Json::String(v)));
                }
                // 候補(材料の名前など)。押すと欄に入れる
                if let Some(list) = suggestions(ws, entry.id, data, f).filter(|l| !l.is_empty()) {
                    // 三角だけの一覧の口(字の幅を持たない)
                    egui::ComboBox::from_id_salt(key.with("suggest"))
                        .selected_text("")
                        .width(0.0)
                        .show_ui(ui, |ui| {
                            for c in &list {
                                if ui.selectable_label(c.value == cur, &c.label).clicked() {
                                    f.set(data, Some(Json::String(c.value.clone())));
                                }
                            }
                        })
                        .response
                        .on_hover_text("一覧から選ぶ");
                }
            });
        }
        FieldKind::Bool => {
            let mut b = f.get(data).and_then(Json::as_bool).unwrap_or(false);
            if ui.checkbox(&mut b, "").changed() {
                f.set(data, Some(Json::Bool(b)));
            }
        }
        FieldKind::Choice { choices } => {
            let cur = f.get(data).and_then(Json::as_str).map(String::from);
            // 項目がなければ最初の選択肢(既定の値)とみなす
            let shown = if cur.as_deref() == Some(crate::panel_ui::MIXED) {
                crate::panel_ui::MIXED.to_string()
            } else {
                choices
                    .iter()
                    .find(|c| Some(&c.value) == cur.as_ref())
                    .or(choices.first())
                    .map_or(String::new(), |c| c.label.clone())
            };
            egui::ComboBox::from_id_salt(key)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    // 前の版の値は、その値の時だけ一覧に出す
                    for c in choices
                        .iter()
                        .filter(|c| !f.hidden.contains(&c.value) || cur.as_ref() == Some(&c.value))
                    {
                        if ui
                            .selectable_label(cur.as_ref() == Some(&c.value), &c.label)
                            .clicked()
                        {
                            f.set(data, Some(Json::String(c.value.clone())));
                        }
                    }
                });
        }
        FieldKind::Output { slot, optional } if slot == "body" && is_cam(ws, entry) => {
            if f.get(data).is_none()
                && newly_applies
                && let Some(last) = features_with(ws, entry.id, slot).last()
            {
                // 使い始めた欄が空なら、直前の候補を入れる
                f.set(data, Some(Json::from(last.0.0)));
            }
            body_stage_field(ui, ws, entry, f, data, key, *optional);
        }
        FieldKind::Output { slot, optional } => {
            let cands = features_with(ws, entry.id, slot);
            let cur = f.get(data).and_then(Json::as_u64).map(FeatureId);
            if cur.is_none() && newly_applies {
                // 使い始めた欄が空なら、直前の候補を入れる
                if let Some(last) = cands.last() {
                    f.set(data, Some(Json::from(last.0.0)));
                }
            }
            let cur = f.get(data).and_then(Json::as_u64).map(FeatureId);
            let shown = cur.map_or("(なし)".to_string(), |id| {
                cands
                    .iter()
                    .find(|c| c.0 == id)
                    .map_or(format!("{id}(使えません)"), |c| c.1.clone())
            });
            egui::ComboBox::from_id_salt(key)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if *optional && ui.selectable_label(cur.is_none(), "(なし)").clicked() {
                        f.set(data, None);
                    }
                    for (id, name) in &cands {
                        if ui.selectable_label(cur == Some(*id), name).clicked() {
                            f.set(data, Some(Json::from(id.0)));
                        }
                    }
                });
        }
        FieldKind::Vector => {
            let cur = f
                .get(data)
                .and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok());
            // 点の欄(既定の単位が長さ)は文書の長さの単位で見せて読む(保存は mm)
            let scale = if f.unit == Some(kmb_core::FieldUnit::Length) {
                ws.length_unit().mm()
            } else {
                1.0
            };
            // 面のクリックで入った長い小数(-0.000000000000004 など)は丸めて見せる
            let text = cur.map_or(String::new(), |v| {
                format!(
                    "{}, {}, {}",
                    short_num(v[0] / scale),
                    short_num(v[1] / scale),
                    short_num(v[2] / scale)
                )
            });
            // 点の欄(「軸の通る点」など)に向きの X・Y・Z を入れないようにする
            let point = f.key.ends_with("origin") || f.key.ends_with("point");
            // 長さの点の欄は、3D のビューの頂点のクリックでも埋められる
            let pickable = crate::select_ui::is_point_field(f) && ws.is_editing(entry.id);
            ui.horizontal(|ui| {
                // 右のボタン(選ぶ・原点・自動、または X・Y・Z)の分を残し、パネルの外へはみ出さない
                let beside = match (point, pickable) {
                    (true, true) => 110.0,
                    (true, false) => 70.0,
                    _ => 60.0,
                };
                if let Some(v) =
                    expr_field_reserving(ui, key, &text, false, "", beside, &|_| None).0
                    && let Some(v) = parse_vector(&v)
                {
                    f.set(data, Some(Json::from(v.map(|x| x * scale).to_vec())));
                }
                if point {
                    if pickable {
                        let on = ws.is_point_pick(entry.id, &f.key);
                        if ui
                            .add(egui::Button::new("選ぶ").small().selected(on))
                            .on_hover_text(if on {
                                "3D のビューの頂点のクリックがこの欄に入ります。押すと止める(Esc でも)"
                            } else {
                                "押してから、3D のビューで頂点(ボディの角)をクリックして座標を入れる"
                            })
                            .clicked()
                        {
                            ws.toggle_point_pick(entry.id, &f.key);
                        }
                    }
                    if ui
                        .small_button("原点")
                        .on_hover_text("原点(0, 0, 0)にする")
                        .clicked()
                    {
                        f.set(data, Some(Json::from(vec![0.0, 0.0, 0.0])));
                    }
                    if cur.is_some()
                        && ui
                            .small_button("自動")
                            .on_hover_text("空にして、形の外形の中心を使う")
                            .clicked()
                    {
                        f.set(data, None);
                    }
                    return;
                }
                for (name, v) in [
                    ("X", [1.0, 0.0, 0.0]),
                    ("Y", [0.0, 1.0, 0.0]),
                    ("Z", [0.0, 0.0, 1.0]),
                ] {
                    if ui
                        .small_button(name)
                        .on_hover_text(format!("{name} の向きにする"))
                        .clicked()
                    {
                        f.set(data, Some(Json::from(v.to_vec())));
                    }
                }
            });
        }
        FieldKind::Pick { accept, min, max } => {
            pick_field_ui(ui, ws, entry, f, data, (accept, *min, *max), true)
        }
        // 行の並びは表として別に描く([`rows_ui`])
        FieldKind::Rows { .. } => {}
        FieldKind::Info => {
            ui.label(info_text(ws, f.get(data)));
        }
    }
}

/// 見るだけの欄の中身。面の名前(f8:end など)と頂点の名前は、利用者に見せる名前にする。
pub(crate) fn info_text(ws: &Workspace, v: Option<&Json>) -> String {
    match v {
        // 頂点の名前は、交わる面の名前を | でつないだ物(f8:end|f8:side0|…)
        Some(Json::String(n)) if n.contains('|') && n.split('|').all(is_face_name) => {
            let face = n.split('|').next().unwrap_or_default();
            match ws.face_label(face).split_once('(') {
                Some((_, owner)) => format!("頂点({owner}"),
                None => "頂点".into(),
            }
        }
        Some(Json::String(n)) if is_face_name(n) => ws.face_label(n),
        Some(Json::Array(a))
            if !a.is_empty() && a.iter().all(|x| x.as_str().is_some_and(is_face_name)) =>
        {
            a.iter()
                .filter_map(Json::as_str)
                .map(|n| ws.face_label(n))
                .collect::<Vec<_>>()
                .join("、")
        }
        v => as_text(v),
    }
}

/// 選ぶ欄([`FieldKind::Pick`]): 「N 個を選択 ×」の箱。押すと 3D のビューのクリックを受ける
/// 状態になり(作業空間の色の枠)、中身を 1 つずつ × つきで並べる。項目にマウスを乗せると、
/// ビューでその物を明るくする。いくつでも受ける欄には、一覧から足す ▾ も付ける(隠れたボディ用)。
fn pick_field_ui(
    ui: &mut egui::Ui,
    ws: &Workspace,
    entry: &FeatureEntry,
    f: &Field,
    data: &mut Json,
    (accept, min, max): (&[kmb_core::RefKind], usize, Option<usize>),
    list: bool,
) {
    let refs = f.refs(data);
    let active = ws.is_pick_field(entry.id, &f.key);
    let l = crate::theme::look(ui.ctx());
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let text = if refs.is_empty() {
                format!("{}を選ぶ", crate::select_ui::kinds_text(accept))
            } else {
                format!("{} 個を選択", refs.len())
            };
            let stroke = if active {
                egui::Stroke::new(1.5, l.ws())
            } else {
                egui::Stroke::new(1.0, l.pal.edge)
            };
            let fill = if active {
                crate::theme::mix(l.pal.panel, l.ws(), 0.15)
            } else {
                l.pal.panel
            };
            let b = ui
                .add(
                    egui::Button::new(egui::RichText::new(text).size(crate::theme::fs(11.5)))
                        .stroke(stroke)
                        .fill(fill),
                )
                .on_hover_text(if active {
                    "3D のビューのクリックがこの欄に入っています。押すと止める(Esc でも)"
                } else {
                    "押してから、3D のビューでクリックして選ぶ"
                });
            if b.clicked() {
                ws.toggle_pick_field(entry.id, &f.key);
            }
            if !refs.is_empty()
                && ui
                    .small_button("×")
                    .on_hover_text("選んだ物を全部外す")
                    .clicked()
            {
                f.set_refs(data, &[]);
            }
            // ボディと機能を受ける欄は、一覧からも選べる(使われて見えないボディもある)
            let feature = accept.contains(&kmb_core::RefKind::Feature);
            if feature || accept.contains(&kmb_core::RefKind::Body) {
                let cands: Vec<FeatureId> = if feature {
                    // この機能より前の、パターン・ミラーに使える機能
                    ws.recipe_features_before(entry.id)
                } else {
                    // この機能の手前で残っているボディ(後の機能が受け継いだ途中の版は出さない)
                    kmb_core::visible_bodies_before(
                        ws.cur.work.doc().data(),
                        &ws.win.registry,
                        ws.cur.work.report(),
                        Some(entry.id),
                    )
                    .into_iter()
                    .map(|(f, _)| f)
                    .collect()
                };
                // ほかの欄に入っているボディは選べない(3D のビューのクリックと同じ)
                let taken: Vec<kmb_core::Ref> = ws
                    .win
                    .registry
                    .descriptor(&entry.type_id)
                    .iter()
                    .flat_map(|d| &d.fields)
                    .filter(|o| {
                        o.key != f.key
                            && matches!(o.kind, FieldKind::Pick { .. })
                            && crate::select_ui::is_feature_field(o) == feature
                    })
                    .flat_map(|o| o.refs(data))
                    .collect();
                ui.menu_button("一覧", |ui| {
                    for id in &cands {
                        let r = kmb_core::Ref::Body(*id);
                        let on = refs.contains(&r);
                        let item = ui.add_enabled(
                            !taken.contains(&r),
                            egui::Button::selectable(on, ws.field_ref_label(accept, &r)),
                        );
                        if item.clicked() {
                            let mut now = f.refs(data);
                            if max == Some(1) {
                                now = vec![r];
                            } else if on {
                                now.retain(|x| *x != r);
                            } else {
                                now.push(r);
                            }
                            if feature {
                                ws.in_timeline_order(&mut now);
                            }
                            f.set_refs(data, &now);
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("一覧から選ぶ");
            }
        });
        if list && active {
            pick_list_ui(ui, ws, f, data);
        }
        if refs.len() < min {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("あと {} 個選んでください", min - refs.len()),
            );
        }
    });
}

/// 選ぶ欄の中身を 1 つずつ × つきで並べる。項目にマウスを乗せると、ビューでその物を明るくする。
fn pick_list_ui(ui: &mut egui::Ui, ws: &Workspace, f: &Field, data: &mut Json) {
    let refs = f.refs(data);
    if refs.is_empty() {
        return;
    }
    let accept: &[kmb_core::RefKind] = match &f.kind {
        FieldKind::Pick { accept, .. } => accept,
        _ => &[],
    };
    let l = crate::theme::look(ui.ctx());
    let mut hover = None;
    let mut remove = None;
    for (i, r) in refs.iter().enumerate() {
        ui.horizontal(|ui| {
            let row = ui.label(
                egui::RichText::new(ws.field_ref_label(accept, r))
                    .size(crate::theme::fs(11.0))
                    .color(l.pal.ink2),
            );
            if row.hovered() {
                hover = Some(r.clone());
            }
            if ui
                .small_button("×")
                .on_hover_text("この 1 つを外す")
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
    *ws.cur.pick_field.hover.borrow_mut() = hover;
    if let Some(i) = remove {
        let mut now = refs.clone();
        now.remove(i);
        f.set_refs(data, &now);
    }
}

/// ベクトルの欄に見せる数。小数 6 桁で丸め、末尾の 0 と -0 を消す。
pub(crate) fn short_num(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    let r = if r == 0.0 { 0.0 } else { r };
    let t = format!("{r:.6}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    t.to_string()
}

/// 欄の候補([`kmb_core::Suggest`])。出力から読む候補は、その機能が計算できている時だけ。
fn suggestions(
    ws: &Workspace,
    id: FeatureId,
    data: &Json,
    f: &Field,
) -> Option<Vec<kmb_core::Choice>> {
    match f.suggest.as_ref()? {
        kmb_core::Suggest::Own { slot } => kmb_core::choices_of(ws.output(id, slot)?),
        kmb_core::Suggest::Values { values } => Some(values.clone()),
        kmb_core::Suggest::Output { field, slot } => {
            let id = Field::info(field, "").get(data)?.as_u64()?;
            kmb_core::choices_of(ws.output(FeatureId(id), slot)?)
        }
    }
}

/// 数だけを打った時は、今の値の単位を付ける(「1 mm」の欄に「3」と打てば「3 mm」。
/// 単位を省ける)。今の値が単位つきの数でなければ(式、個数、空)、打ったまま。
pub fn with_unit_of(new: &str, cur: &str) -> String {
    let unit = cur
        .trim()
        .split_once(' ')
        .filter(|(n, u)| n.parse::<f64>().is_ok() && u.chars().all(char::is_alphabetic))
        .map(|(_, u)| u);
    match unit {
        Some(u) if new.parse::<f64>().is_ok() => format!("{new} {u}"),
        _ => new.to_string(),
    }
}

/// 機能の面の名前か(`f8:end`、`f8:side:e1` のように `f<番号>:` で始まる)。
fn is_face_name(s: &str) -> bool {
    s.strip_prefix('f')
        .and_then(|r| r.split_once(':'))
        .is_some_and(|(id, _)| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
}

/// 「1, 0, 0」や「1 0 0」を 3 つの数にする。
pub(crate) fn parse_vector(s: &str) -> Option<[f64; 3]> {
    let v: Vec<f64> = s
        .split([',', ' ', '、'])
        .filter(|t| !t.is_empty())
        .map(|t| t.parse().ok())
        .collect::<Option<_>>()?;
    <[f64; 3]>::try_from(v).ok()
}

/// 記述子の欄を並べる。変えたら新しいデータを返す。
fn descriptor_form(ui: &mut egui::Ui, ws: &Workspace, entry: &FeatureEntry) -> Option<Json> {
    let desc = ws.win.registry.descriptor(&entry.type_id)?;
    // 前の形のデータは今の形にしてから欄に出す(直さなければ文書はそのまま)
    let base = desc.upgraded(&entry.data);
    let entry = &FeatureEntry {
        data: base.clone(),
        ..entry.clone()
    };
    let mut data = base.clone();
    let step = select_steps(ui, ws, entry, &desc, &data);
    // 狭いパネル(既定 312 px)でも欄がはみ出さないよう、列の幅に上限を置く(長い名前は折り返す)
    let col = ((ui.available_width() - 12.0) / 2.0).max(110.0);
    // 行の並びはパネルの幅いっぱいの表にし、その前後の欄はそれぞれ 2 列の表にする
    let mut start = 0;
    while start < desc.fields.len() {
        if matches!(desc.fields[start].kind, FieldKind::Rows { .. }) {
            rows_ui(ui, ws, entry, &desc.fields[start], &mut data);
            start += 1;
            continue;
        }
        let end = (start..desc.fields.len())
            .find(|&i| matches!(desc.fields[i].kind, FieldKind::Rows { .. }))
            .unwrap_or(desc.fields.len());
        egui::Grid::new(("form", entry.id, start))
            .num_columns(2)
            .max_col_width(col)
            .striped(false)
            .show(ui, |ui| {
                for f in &desc.fields[start..end] {
                    let before = f.applies(&entry.data);
                    if !f.applies(&data) {
                        // 使わない欄の値は消す(例: 新しいボディの押し出しの対象)
                        if f.get(&data).is_some() {
                            f.set(&mut data, None);
                        }
                        continue;
                    }
                    if matches!(f.kind, FieldKind::Info) && f.get(&data).is_none() {
                        continue;
                    }
                    let newly = !before || f.get(&data).is_none() && f.when.is_some();
                    let mark = step.as_deref() == Some(f.key.as_str());
                    marked_field_ui(ui, ws, entry, f, &mut data, newly, mark);
                    ui.end_row();
                }
            });
        // スライダは表の下にパネルの幅いっぱいで出す(表の列の中では短く、1 画素で何度も動く)
        for f in &desc.fields[start..end] {
            if let Some(s) = &f.slider
                && f.applies(&data)
            {
                slider_ui(ui, ws, entry, f, s, &mut data);
            }
        }
        start = end;
    }
    if !desc.hint.is_empty() {
        ui.weak(&desc.hint);
    }
    desc.prune_rows(&mut data);
    (data != base).then_some(data)
}

/// 選ぶ手順の多い機能(選ぶ欄が 2 つ以上。パターン・ミラーなど)を作っている・直している間、欄の上に
/// 「1/3: 並べる機能を選ぶ」と今の手順を出す。今の手順は、
/// クリックを受けている欄、クリックで選んでいる途中の欄(軸を選ぶ手順など)、まだ足りない欄の順に探す。
/// 今の手順の欄の名前を返す(欄の名前を強調する)。カーソルを乗せただけでは何も変えない。
fn select_steps(
    ui: &mut egui::Ui,
    ws: &Workspace,
    entry: &FeatureEntry,
    desc: &kmb_core::Descriptor,
    data: &Json,
) -> Option<String> {
    let open = ws.is_editing(entry.id) || ws.cur.creating.is_some_and(|(id, _)| id == entry.id);
    if !open {
        return None;
    }
    let steps: Vec<&Field> = desc
        .fields
        .iter()
        .filter(|f| {
            f.applies(data) && (matches!(f.kind, FieldKind::Pick { .. }) || f.pick.is_some())
        })
        .collect();
    if steps.len() < 2 {
        return None;
    }
    let gesture = ws.cur.gesture.as_ref().map(|g| g.action.as_str());
    let now = steps
        .iter()
        .position(|f| match (&f.kind, &f.pick) {
            (FieldKind::Pick { .. }, _) => ws.is_pick_field(entry.id, &f.key),
            (_, Some(p)) => gesture == Some(p.action.as_str()),
            _ => false,
        })
        .or_else(|| {
            steps.iter().position(|f| match &f.kind {
                FieldKind::Pick { min, .. } => f.refs(data).len() < *min,
                _ => false,
            })
        });
    let l = crate::theme::look(ui.ctx());
    let n = steps.len();
    let (text, color) = match now {
        Some(i) => (format!("{}/{n}: {}を選ぶ", i + 1, steps[i].label), l.ws()),
        None => (
            format!("{n}/{n}: 選び終えました。値を確かめて OK(Enter)"),
            l.pal.ink2,
        ),
    };
    ui.label(
        egui::RichText::new(text)
            .size(crate::theme::fs(12.0))
            .strong()
            .color(color),
    );
    now.map(|i| steps[i].key.clone())
}

/// 行の並びの欄([`FieldKind::Rows`])を表にする。行ごとに番号・選ぶ欄・値の欄・×。下の「+」を押すと
/// 新しい行を待つ状態になり、次に 3D のビューでクリックした物で行ができる(
/// 空の行は残さない)。行の選ぶ欄を押すと、その行の物が 3D のビューで光り、クリックがその行に入る。
fn rows_ui(ui: &mut egui::Ui, ws: &Workspace, entry: &FeatureEntry, f: &Field, data: &mut Json) {
    let FieldKind::Rows { fields } = &f.kind else {
        return;
    };
    // 機能の欄で使わなくなった列(面取りの種類ごとの距離 2・角度など)は出さない。行の中の欄で決まる
    // 列(フィレットの徐変の終わりの半径など)は、使う行が 1 つでもあれば出す
    let fields: Vec<&Field> = fields
        .iter()
        .filter(|s| f.applies_in_some_row(s, data))
        .collect();
    let l = crate::theme::look(ui.ctx());
    let n = f.row_count(data);
    let pick = fields
        .iter()
        .copied()
        .find(|s| matches!(s.kind, FieldKind::Pick { .. }));
    let active_row =
        |i: usize| pick.is_some_and(|p| ws.is_pick_field(entry.id, &f.row_field(i, p).key));
    let pending = active_row(n);
    let title = ui.label(egui::RichText::new(&f.label).strong());
    if !f.help.is_empty() {
        title.on_hover_text(&f.help);
    }
    let mut remove = None;
    // 列が 3 つ以上(面取りの 2 つの距離・反転など)だと欄の幅に入らないので、行ごとに縦に並べる
    let stacked = fields.len() > 2;
    if stacked {
        for i in 0..n {
            let num = egui::RichText::new(f.row_title(i, data)).size(crate::theme::fs(11.5));
            ui.horizontal(|ui| {
                ui.label(if active_row(i) {
                    num.color(l.ws()).strong()
                } else {
                    num
                });
                if n > 1
                    && ui
                        .small_button("×")
                        .on_hover_text("この選択セットを消す")
                        .clicked()
                {
                    remove = Some(i);
                }
            });
            egui::Grid::new(("rows", entry.id, &f.key, i))
                .num_columns(2)
                .show(ui, |ui| {
                    for s in &fields {
                        let rf = f.row_field(i, s);
                        if !rf.applies(data) {
                            continue;
                        }
                        ui.label(
                            egui::RichText::new(format!("    {}", s.label))
                                .size(crate::theme::fs(11.0))
                                .color(l.pal.ink2),
                        );
                        ui.horizontal(|ui| match &rf.kind {
                            FieldKind::Pick { accept, min, max } => {
                                pick_field_ui(ui, ws, entry, &rf, data, (accept, *min, *max), false)
                            }
                            _ => value_ui(ui, ws, entry, &rf, data, false),
                        });
                        ui.end_row();
                    }
                });
        }
        if pending {
            ui.label(
                egui::RichText::new(format!("{}: 3D のビューでクリック", kmb_core::row_name(n)))
                    .size(crate::theme::fs(11.0))
                    .color(l.ws()),
            );
        }
    } else {
        egui::Grid::new(("rows", entry.id, &f.key))
            .num_columns(fields.len() + 2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("");
                for s in &fields {
                    ui.label(
                        egui::RichText::new(&s.label)
                            .size(crate::theme::fs(11.0))
                            .color(l.pal.ink2),
                    );
                }
                ui.label("");
                ui.end_row();
                for i in 0..n {
                    let num =
                        egui::RichText::new(format!("{}", i + 1)).size(crate::theme::fs(11.5));
                    ui.label(if active_row(i) {
                        num.color(l.ws()).strong()
                    } else {
                        num
                    });
                    for s in &fields {
                        let rf = f.row_field(i, s);
                        if !rf.applies(data) {
                            ui.label("");
                            continue;
                        }
                        ui.horizontal(|ui| match &rf.kind {
                            // 行の中は数だけ。中身の一覧は表の下に出す(列の幅が崩れないように)
                            FieldKind::Pick { accept, min, max } => {
                                pick_field_ui(ui, ws, entry, &rf, data, (accept, *min, *max), false)
                            }
                            _ => value_ui(ui, ws, entry, &rf, data, false),
                        });
                    }
                    if n > 1 {
                        if ui
                            .small_button("×")
                            .on_hover_text("この選択セットを消す")
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
                if pending {
                    ui.label(
                        egui::RichText::new(format!("{}", n + 1))
                            .size(crate::theme::fs(11.5))
                            .color(l.ws())
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("3D のビューでクリック")
                            .size(crate::theme::fs(11.0))
                            .color(l.ws()),
                    );
                    ui.end_row();
                }
            });
    }
    // クリックを受けている行の中身(× で 1 つずつ外す)
    if let Some((i, p)) = pick.and_then(|p| (0..n).find(|&i| active_row(i)).map(|i| (i, p))) {
        ui.label(
            egui::RichText::new(format!("{} の中身", kmb_core::row_name(i)))
                .size(crate::theme::fs(11.0))
                .color(l.pal.ink2),
        );
        pick_list_ui(ui, ws, &f.row_field(i, p), data);
    }
    if let Some(i) = remove
        && let Some(Json::Array(rows)) = f.get(data).cloned()
    {
        let mut rows = rows;
        rows.remove(i);
        f.set(data, Some(Json::Array(rows)));
        // 消した行より後ろの行がクリックを受けていたら、受けるのをやめる(番号がずれるため)
        if (i..=n).any(active_row) {
            ws.leave_rows_pick();
        }
    }
    if let Some(p) = pick {
        ui.horizontal(|ui| {
            let add = ui
                .add_enabled(!pending, egui::Button::new("+"))
                .on_hover_text("新しい選択セットを足す。押してから 3D のビューで辺か面をクリックすると、前のセットの値のまま新しいセットになる");
            if add.clicked() {
                ws.toggle_pick_field(entry.id, &f.row_field(n, p).key);
            }
            if pending {
                if ui.small_button("キャンセル").clicked() {
                    ws.toggle_pick_field(entry.id, &f.row_field(n, p).key);
                }
            } else {
                ui.label(
                    egui::RichText::new("+ で選択セットを足す(セットごとに違う値)")
                        .size(crate::theme::fs(11.0))
                        .color(l.pal.ink3),
                );
            }
        });
    }
}

/// 式の欄に打った文字 `v` を、入れる値にする(規則は [`kmb_session::params::typed_expr`]。
/// 欄の名前や今の値の単位からは推し量らない)。
pub(crate) fn typed_expr(ws: &Workspace, f: &Field, _cur: &str, v: &str) -> String {
    kmb_session::params::typed_expr(f.unit.as_ref(), v, ws.length_unit())
}

/// 確定した時だけ新しい文字列を返す一行の入力欄。
pub(crate) fn expr_field(ui: &mut egui::Ui, key: egui::Id, current: &str) -> Option<String> {
    expr_field_focus(ui, key, current, false, "").0
}

/// Enter で確定した欄があったことを覚える名前(編集パネルが「作っている途中」を決めるのに使う)。
pub(crate) fn enter_id() -> egui::Id {
    egui::Id::new("expr_field_enter")
}

/// [`expr_field`] と同じ。`focus` なら欄にキーボードを移す(作り始めた機能の最初の欄)。
/// 欄にキーボードがあるかも返す。`hint` は空の欄に薄く出す説明。
pub(crate) fn expr_field_focus(
    ui: &mut egui::Ui,
    key: egui::Id,
    current: &str,
    focus: bool,
    hint: &str,
) -> (Option<String>, bool) {
    expr_field_reserving(ui, key, current, focus, hint, 0.0, &|_| None)
}

/// 打つ手を止めてから形を付いて来させるまでの時間(打つたびに計算し直さない)。
const TYPING_PREVIEW_DELAY: f64 = 0.3;

/// 打っている欄 `key` の打ちかけの字が `TYPING_PREVIEW_DELAY` 秒変わらず、`ok`(計算できる値)なら
/// その字を返す(形を付いて来させる)。打ちかけが今の値と同じ・空なら返さない。
fn typing_preview(
    ui: &egui::Ui,
    key: egui::Id,
    current: &str,
    ok: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let text = ui.data(|d| d.get_temp::<String>(key))?;
    let now = ui.input(|i| i.time);
    let since = key.with("typing_since");
    let (seen, at) = ui
        .data(|d| d.get_temp::<(String, f64)>(since))
        .filter(|(t, _)| *t == text)
        .unwrap_or((text.clone(), now));
    ui.data_mut(|d| d.insert_temp(since, (seen, at)));
    if text == current || text.trim().is_empty() {
        return None;
    }
    let wait = TYPING_PREVIEW_DELAY - (now - at);
    if wait > 0.0 {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(wait));
        return None;
    }
    if !ok(&text) {
        return None;
    }
    log::debug!("打っている途中の値 {text} で形を付いて来させる");
    Some(text)
}

/// 範囲の外のため受け付けなかった欄と理由を覚える名前(覚えるのは最後の 1 つだけ。どれかの欄を
/// 触る・直すのを終えると消す)。
fn refused_id() -> egui::Id {
    egui::Id::new("expr_field_refused")
}

/// 欄 `key` で受け付けなかった値の理由。
pub(crate) fn refused(ctx: &egui::Context, key: egui::Id) -> Option<String> {
    ctx.data(|d| d.get_temp::<(egui::Id, String)>(refused_id()))
        .filter(|(k, _)| *k == key)
        .map(|(_, why)| why)
}

/// [`expr_field_focus`] と同じ。欄の右に並べるボタンの幅 `beside` を残す(ベクトルの欄の X・Y・Z など)。
/// `check` が理由を返す値は、打っている間は赤い字にし、確定しても受け付けない(理由は
/// [`refused`] で読め、欄は前の値に戻す)。
fn expr_field_reserving(
    ui: &mut egui::Ui,
    key: egui::Id,
    current: &str,
    focus: bool,
    hint: &str,
    beside: f32,
    check: &dyn Fn(&str) -> Option<String>,
) -> (Option<String>, bool) {
    let mut text = ui.data_mut(|d| {
        d.get_temp::<String>(key)
            .unwrap_or_else(|| current.to_string())
    });
    // 右の ƒ(式の候補)の分を残し、狭いパネルでもはみ出さない幅
    let w = (ui.available_width() - 30.0 - beside).clamp(70.0, 140.0);
    // 欄の名前は決めておく(上の注意の行が増減しても、打っている欄からキーボードが外れないように)
    // 打っている名前の候補を欄の下に出す(U51)
    let c = crate::complete::begin(ui, key.with("edit"));
    let bad = text != current && check(&text).is_some();
    let mut edit = egui::TextEdit::singleline(&mut text)
        .desired_width(w)
        .hint_text(hint);
    if bad {
        edit = edit.text_color(ui.visuals().error_fg_color);
    }
    let mut out_edit = c.filter(edit).show(ui);
    c.end(ui, &out_edit.response, &mut text, &[]);
    if focus && !out_edit.response.has_focus() && !ui.is_sizing_pass() {
        // 全体を選んでおき、打った数字で置き換える
        out_edit.response.request_focus();
        let all = egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text.chars().count()),
        );
        out_edit.state.cursor.set_char_range(Some(all));
        out_edit.state.store(ui.ctx(), out_edit.response.id);
    }
    let r = out_edit.response;
    let mut out = (r.lost_focus() && text != current).then(|| text.clone());
    if r.gained_focus() {
        ui.data_mut(|d| d.remove::<(egui::Id, String)>(refused_id()));
    }
    let refused = out.as_deref().and_then(check);
    if let Some(why) = refused.clone() {
        log::info!("範囲の外の値を受け付けない: {why}");
        ui.data_mut(|d| d.insert_temp(refused_id(), (key, why)));
        out = None;
    }
    // 受け付けなかった値の Enter ではコマンドを決めない
    if r.lost_focus() && refused.is_none() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        ui.data_mut(|d| d.insert_temp(enter_id(), true));
    }
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    let focused = r.has_focus();
    (out, focused)
}

/// 機能の設定欄を描く。データを変えたら新しいデータを返す。`editable` が偽(選んでいるだけ)の時は
/// 欄を灰色にして直せなくするが、操作のボタン(書き出す、入る位置を置く など)と直し方のボタンは押せる。
pub fn feature_panel(
    ui: &mut egui::Ui,
    ws: &mut Workspace,
    entry: &FeatureEntry,
    editable: bool,
) -> Option<serde_json::Value> {
    if !editable {
        // 直すのを終えたら、受け付けなかった印は消す
        ui.data_mut(|d| d.remove::<(egui::Id, String)>(refused_id()));
    }
    // 名前は編集パネルの見出しに出る。ここには計算できなかった時の理由と直し方だけ
    let failed = !matches!(
        ws.cur.work.report().feature(entry.id),
        Some(FeatureStatus::Ok { .. })
    );
    if failed {
        ui.horizontal_wrapped(|ui| match ws.cur.work.report().feature(entry.id) {
            Some(FeatureStatus::Error { message }) => {
                ui.colored_label(ui.visuals().error_fg_color, ws.plain_text(message));
                if ws.can_repair_with_picked(entry.id)
                    && ui
                        .button("選んだ面に付け替える")
                        .on_hover_text("参照している面を、3D 画面で選んだ面に替えます")
                        .clicked()
                {
                    ws.repair_with_picked(entry.id);
                }
            }
            Some(FeatureStatus::Blocked { by, .. }) => {
                ui.colored_label(ui.visuals().warn_fg_color, format!("{by} が使えません"));
                if let Some(root) = ws.root_cause(entry.id)
                    && ui
                        .button("原因のフィーチャへ移る")
                        .on_hover_text("最初に失敗した上流の機能を選ぶ")
                        .clicked()
                {
                    ws.select(root);
                }
            }
            _ => {}
        });
    }
    // 実行ボタンで解く機能(解析の研究): 様子・実行・進み具合・中止
    if let Some(st) = ws.run_state(entry.id)
        && let Some(id) = crate::result_ui::run_header(ui, &st, &ws.win.look)
    {
        ws.start_action(&id);
    }
    let warn = ui.visuals().warn_fg_color;
    for w in ws.notes(entry.id) {
        ui.colored_label(warn, format!("注意: {w}"));
    }
    // 計算できている機能も、3D で選んだ面に替えられる(面取り加工・輪郭など。Z11)
    if !failed
        && ws.can_retarget_to_picked(entry.id)
        && ui
            .button("選んだ面に付け替える")
            .on_hover_text("使う面を、3D 画面で選んだ面に替えます")
            .clicked()
    {
        ws.retarget_to_picked(entry.id);
    }
    // モジュールの操作のうち、この機能の欄に出すもの(セットアップの書き出しなど)。
    // 欄より上に置き、狭いパネルでは折り返す(右端で切れて見えなくならないように)
    let actions: Vec<_> = ws
        .actions_for(entry.id)
        .into_iter()
        .filter(|a| a.is_at(kmb_core::Place::Panel))
        .collect();
    if !actions.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for a in &actions {
                // クリックで置く手順がある操作(固定具・入る位置)はその流れに入る
                if crate::action_button(ui, a).clicked() {
                    if a.keep_edit {
                        ws.begin_pick(&a.id);
                    } else {
                        ws.start_action(&a.id);
                    }
                }
            }
        });
        ui.separator();
    }
    ui.add_enabled_ui(editable, |ui| feature_form(ui, ws, entry))
        .inner
}

/// 記述子の欄、専用の編集器の欄、詳細(JSON)、要約。
fn feature_form(ui: &mut egui::Ui, ws: &mut Workspace, entry: &FeatureEntry) -> Option<Json> {
    let mut out = None;
    if let Some(d) = descriptor_form(ui, ws, entry) {
        out = Some(d);
    }
    // 専用の編集器の欄(工具ライブラリなど)。同じ画面の更新で記述子の欄が変えたデータは、
    // 編集器にも見せる(Enter で欄を決めると同時に直すのを終えても、編集器が足す物が落ちない。
    // 文字の編集器が、打ち足した字の形を入れるなど)
    let now;
    let entry = match &out {
        Some(d) => {
            now = FeatureEntry {
                data: d.clone(),
                ..entry.clone()
            };
            &now
        }
        None => entry,
    };
    let (special, edited) = ws.with_editors(|eds, host| {
        let mut special = false;
        let mut edited = None;
        for ed in eds.iter_mut() {
            if ed.handles(&entry.type_id) {
                special = true;
                if let Some(d) = ed.panel(ui, host, entry) {
                    edited = Some(d);
                }
            } else if let Some(d) = ed.assist(ui, host, entry) {
                // 補いの欄(加工の工具の欄に PC のライブラリの工具を出すなど)
                edited = Some(d);
            }
        }
        (special, edited)
    });
    if edited.is_some() {
        out = edited;
    }
    let raw = ws
        .win
        .registry
        .descriptor(&entry.type_id)
        .is_none_or(|d| d.raw_editor);
    if raw {
        let fields = ws
            .win
            .registry
            .descriptor(&entry.type_id)
            .is_some_and(|d| !d.fields.is_empty())
            || special;
        let current = serde_json::to_string_pretty(&entry.data).unwrap_or_default();
        if ws
            .cur
            .data_edit
            .as_ref()
            .is_none_or(|(id, _)| *id != entry.id)
        {
            ws.cur.data_edit = Some((entry.id, current.clone()));
        }
        egui::CollapsingHeader::new("詳細(JSON)")
            .id_salt(("json", entry.id))
            .default_open(!fields)
            .show(ui, |ui| {
                if let Some((_, text)) = &mut ws.cur.data_edit {
                    ui.add(
                        egui::TextEdit::multiline(text)
                            .code_editor()
                            .desired_rows(6)
                            .desired_width(f32::INFINITY),
                    );
                    if ui.button("適用").clicked() {
                        match serde_json::from_str(text) {
                            Ok(v) => out = Some(v),
                            Err(e) => ws.win.fail(format!("JSON として読めません: {e}")),
                        }
                    }
                }
            });
    }
    if let Some(s) = ws.summary(entry.id) {
        ui.label(s);
    }
    out
}
