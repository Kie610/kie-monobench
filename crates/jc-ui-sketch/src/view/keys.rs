//! 2D スケッチのビューのキー: 寸法の欄への数字と Tab、値の打ち込み、Esc で 1 段ずつ戻す、
//! 1 文字のキーで道具を選ぶ、S で道具を探す、右クリックのメニューを開く。

use super::*;

/// キーの段で決まった、道具の操作の段が使う物。
pub(super) struct Keys {
    /// ほかの欄に入力していない(キーをスケッチが使ってよい)
    pub(super) free_keys: bool,
    /// 描いている途中の寸法の欄が出ている
    pub(super) hud_on: bool,
    /// Enter で打った値を決めた
    pub(super) enter: bool,
    /// 位置を決める前の Enter(選び終える・中心線でミラーする)
    pub(super) raw_enter: bool,
    /// 何も打たずに Enter(つなげて描いている線を終える)
    pub(super) enter_empty: bool,
    /// X で選んだ線を補助線に切り替える
    pub(super) x_toggle: bool,
}

/// このコマのキーと右クリックを扱う。
#[allow(clippy::too_many_arguments)]
pub(super) fn keys(
    ui: &mut egui::Ui,
    state: &mut ViewState,
    editor: &mut Editor,
    base_sketch: &Sketch,
    base_sol: &Solution,
    response: &egui::Response,
    t: &Transform,
    tol: f64,
) -> Keys {
    // 描いている途中の寸法の欄(線・円・矩形・円弧・長穴・多角形)。数字は今の欄に入り、Tab で次の欄へ
    // 移ると打った値を固定する(ほかの欄に入力している時は除く)
    let free_keys = ui.ctx().memory(|m| m.focused().is_none());
    let hud_on = crate::hud::spec(editor.tool, editor.step()).is_some()
        && editor.pending.is_some()
        && free_keys;
    let mut enter = false;
    // 位置を決める前の Enter(選び終える・中心線でミラーする)
    let raw_enter = free_keys && ui.input(|i| i.key_pressed(egui::Key::Enter));
    // 何も打たずに Enter: つなげて描いている線を終える
    let mut enter_empty = false;
    // スケッチの中の Tab は寸法の欄の切り替えにだけ使う。欄が無い時(始点の前・選ぶ道具)も、
    // Tab で焦点がリボンのボタンへ移り、続く Enter でメニューが開かないよう、次のコマで止めてもらう
    if free_keys {
        jc_ui::keep_tab(ui.ctx());
    }
    // X: 選んだ線を補助線に切り替える。描く道具の中では、次に描く線を補助線にする。
    // 寸法の欄に打っている途中は字として欄に入れる
    let typing_field = hud_on
        && editor
            .hud
            .fields
            .get(editor.hud.active)
            .is_some_and(|f| !f.text.is_empty());
    let x_key = free_keys
        && !typing_field
        && editor.typed.is_empty()
        && jc_ui::keys::pressed(ui.ctx(), "sketch_construction");
    let mut x_toggle = false;
    if x_key {
        if draws(editor.tool) {
            editor.draw_construction = !editor.draw_construction;
            editor.draw_centerline = false;
        } else if editor.pending.is_none() {
            x_toggle = true;
        }
    }
    if hud_on {
        editor.hud.sync(editor.tool, editor.step());
        let mut events = ui.input(|i| i.events.clone());
        if x_key {
            events.retain(|e| !matches!(e, egui::Event::Text(t) if t.eq_ignore_ascii_case("x")));
        }
        match editor.hud.keys(&events) {
            crate::hud::Key::Enter => enter = true,
            crate::hud::Key::EnterEmpty => enter_empty = true,
            crate::hud::Key::None => {}
        }
    } else if editor.pending.is_none() {
        editor.hud.clear();
    }
    // 多角形: 中心を決める前に打った数字は辺の数にする(網羅版 C8)
    if editor.pending.is_some()
        || !matches!(
            editor.tool,
            Tool::Polygon | Tool::OuterPolygon | Tool::EdgePolygon
        )
    {
        editor.polygon_typed = false;
    }
    if matches!(
        editor.tool,
        Tool::Polygon | Tool::OuterPolygon | Tool::EdgePolygon
    ) && editor.pending.is_none()
        && free_keys
        && let Some(n) = ui.input(|i| {
            i.events.iter().rev().find_map(|e| match e {
                egui::Event::Text(t) => t.parse::<usize>().ok(),
                _ => None,
            })
        })
    {
        // 続けて打った 2 桁(1 → 2 で 12)も受ける
        let two = editor.polygon_n * 10 + n;
        editor.polygon_n =
            if editor.polygon_n < 10 && (3..=64).contains(&two) && editor.polygon_typed {
                two
            } else {
                n
            };
        editor.polygon_typed = true;
    }
    // 角の丸め・面取り: 数字を打つと半径・距離になる(カーソルの脇に出す。網羅版 D3・D4)。
    // 面取りは「2, 3」で距離 2 つ、「2<45」で距離と角度
    if matches!(editor.tool, Tool::Fillet | Tool::Chamfer) && free_keys {
        ui.input(|i| {
            for e in &i.events {
                match e {
                    egui::Event::Text(t)
                        if t.chars().all(|c| c.is_ascii_digit() || ".,< ".contains(c)) =>
                    {
                        editor.typed.push_str(t)
                    }
                    egui::Event::Key {
                        key: egui::Key::Backspace,
                        pressed: true,
                        ..
                    } => {
                        editor.typed.pop();
                    }
                    _ => {}
                }
            }
        });
    }
    // ほかの道具(並べる・動かすなど)は、描いている途中に値を打ち込める
    let point = editor.tool == Tool::Point;
    let typing =
        crate::more_tools::takes_typing(editor) && (editor.pending.is_some() || point) && free_keys;
    if typing {
        ui.input(|i| {
            for e in &i.events {
                match e {
                    // 点の道具は、数・符号・小数点で打ち始める(ほかの字は道具のキーのまま)
                    egui::Event::Text(t)
                        if point
                            && editor.typed.is_empty()
                            && !t.starts_with(|c: char| {
                                c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | '(')
                            }) => {}
                    egui::Event::Text(t) => editor.typed.push_str(t),
                    egui::Event::Key {
                        key: egui::Key::Backspace,
                        pressed: true,
                        ..
                    } => {
                        editor.typed.pop();
                    }
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        ..
                    } => {
                        enter = !editor.typed.trim().is_empty();
                        enter_empty = !enter;
                    }
                    _ => {}
                }
            }
        });
    }
    // Esc は 1 段ずつ戻す: 値を直す欄 → 打ち込み → 道具(描きかけを捨てて選ぶ道具へ。選びは残す)
    // → 選び(選びは次の Esc で外れる)
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if editor.auto.take().is_some() {
            // 自動拘束の予告をやめる(何も付けない)
        } else if editor.others.take().is_some() {
            // 重なったものの一覧を閉じる
            editor.others_hover = None;
        } else if editor.placing.take().is_some() {
            // 置いた寸法の値を打つのをやめる(選びはそのまま。置き直せる)
        } else if editor.edit_dim.take().is_some() {
            // 寸法の値を直すのをやめる(選びと道具はそのまま)
        } else if editor.drag.take().is_some()
            || editor.window.take().is_some()
            || editor.dim_drag.take().is_some()
            || std::mem::take(&mut editor.arc_drag)
        {
            // 引いている途中: 動かすのと囲むのをやめる(形は元のまま)
        } else if !editor.typed.is_empty() {
            editor.typed.clear();
        } else if !editor.hud.is_blank() {
            // 打った寸法の値と固定を外す(描きかけの点は残す)
            editor.hud.reset_values();
        } else if editor.tool != Tool::Select {
            editor.finish(editor.tool);
        } else if !editor.selection.is_empty()
            || !editor.regions.is_empty()
            || editor.picked_constraint.is_some()
        {
            editor.selection.clear();
            editor.regions.clear();
            editor.picked_constraint = None;
        } else {
            // 戻す物が何も無い時に素早く 2 回押したら、スケッチを終える(1 段ずつ戻す途中の
            // Esc は数えないので、選びを外した直後の 1 回では終わらない)
            let now = ui.input(|i| i.time);
            if editor
                .empty_esc
                .take()
                .is_some_and(|t| now - t <= ESC_TWICE)
            {
                editor.app_ask = Some(jc_ui::APP_FINISH);
            } else {
                editor.empty_esc = Some(now);
            }
        }
    }
    // 1 文字のキーで道具を選ぶ(描いている途中は数字や式を打つので効かせない)
    if free_keys
        && editor.pending.is_none()
        && editor.typed.is_empty()
        && let Some((_, tool)) = KEY_TOOLS
            .iter()
            .find(|(id, _)| jc_ui::keys::pressed(ui.ctx(), id))
    {
        match tool {
            Some(t) => editor.set_tool(*t),
            None => editor.wants_extrude = true,
        }
    }
    // S: 道具を名前で探す窓(画面のコマンド検索に、スケッチの道具も出す)。I: 測る
    if free_keys && editor.pending.is_none() && editor.typed.is_empty() {
        let (s_key, i_key) = (
            jc_ui::keys::pressed(ui.ctx(), "search_here"),
            jc_ui::keys::pressed(ui.ctx(), "measure"),
        );
        if s_key {
            editor.app_ask = Some(jc_ui::APP_SEARCH);
        }
        if i_key {
            editor.set_tool(Tool::Measure);
        }
        // 1・2・3: 空いた所から引いた時の選び方(窓・自由形状・なぞる)
        if editor.tool == Tool::Select {
            for (key, m) in [
                (egui::Key::Num1, crate::display::SelectMode::Window),
                (egui::Key::Num2, crate::display::SelectMode::Lasso),
                (egui::Key::Num3, crate::display::SelectMode::Paint),
            ] {
                if ui.input(|i| i.key_pressed(key) && i.modifiers.is_none()) {
                    editor.select_mode = m;
                }
            }
        }
    }
    // 右クリック: 道具の中・何も選んでいない時・選んだ時で項目を分けたメニュー(menu.rs。下で出す)。
    // 選ぶ道具で、選んでいない線や拘束の印の上を右クリックしたら、それを選んでから出す
    if response.secondary_clicked() {
        let p = response.interact_pointer_pos();
        editor.menu_at = p.map(|p| t.to_world(p));
        if editor.tool == Tool::Select
            && let Some(p) = p
        {
            let w = t.to_world(p);
            let label = state
                .labels
                .iter()
                .find(|(_, r)| r.contains(p))
                .map(|(id, _)| *id);
            if let Some(id) = label.or_else(|| crate::marks::at(&state.marks, p)) {
                // 拘束の印か寸法の値の札: その拘束を選んでから出す
                editor.selection.clear();
                editor.regions.clear();
                editor.picked_constraint = Some(id);
            } else if let Some(h) = base_sketch.hit_test(base_sol, w[0], w[1], tol)
                && !editor.selection.contains(&h.entity)
            {
                editor.selection.clear();
                editor.regions.clear();
                editor.picked_constraint = None;
                editor.selection.insert(h.entity);
            }
        }
        editor.menu = Some(crate::menu::Context {
            tool: editor.tool,
            selection: crate::constraints_ui::selected(editor, base_sketch),
            last_tool: editor.last_tool,
            recent: editor.recent.clone(),
            can_paste: crate::clipboard::held(ui.ctx()).is_some(),
            picked_constraint: editor.picked_constraint.and_then(|id| {
                base_sketch
                    .constraints
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| crate::constraints_ui::constraint_name(&c.kind))
            }),
            round_dim: editor.picked_constraint.and_then(|id| {
                match base_sketch.constraints.iter().find(|c| c.id == id)?.kind {
                    ConstraintKind::Diameter { .. } => Some(true),
                    ConstraintKind::Radius { .. } => Some(false),
                    _ => None,
                }
            }),
            drawing_construction: editor.draw_construction,
            busy: editor.pending.is_some(),
            offset_mode: (editor.tool == Tool::Offset)
                .then_some((editor.offset.mode, editor.offset.round)),
            dim_options: match (&editor.ghost, editor.tool) {
                (Some((k, _)), Tool::Dimension) => crate::dim_choice::options(
                    k,
                    &editor.dim_choice,
                    base_sketch,
                    &crate::constraints_ui::selected(editor, base_sketch),
                ),
                _ => Vec::new(),
            },
        });
    }
    Keys {
        free_keys,
        hud_on,
        enter,
        raw_enter,
        enter_empty,
        x_toggle,
    }
}
