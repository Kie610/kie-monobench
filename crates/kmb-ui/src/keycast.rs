//! 録画・解説向けの、今押している入力の表示。入力は窓のメモリだけに置く。

use crate::{keys, theme};
use egui::{Color32, Key, Modifiers, PointerButton, Rect, Stroke, Vec2};
use serde::{Deserialize, Serialize};

/// 操作の表示を置く、中央の 3D ビューの下側の場所。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeycastPlace {
    #[default]
    LeftBottom,
    CenterBottom,
}

/// 録画・解説向けの操作の表示。入力そのものは保存しない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeycastSettings {
    pub enabled: bool,
    pub place: KeycastPlace,
    pub scale: f32,
    pub keys: bool,
    pub mouse: bool,
    pub mouse3d: bool,
    pub names: bool,
    pub hold_secs: f32,
}

impl Default for KeycastSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            place: KeycastPlace::LeftBottom,
            scale: 1.0,
            keys: true,
            mouse: true,
            mouse3d: true,
            names: true,
            hold_secs: 2.0,
        }
    }
}

fn bounded(v: f32, min: f32, max: f32, default: f32) -> f32 {
    if v.is_finite() {
        v.clamp(min, max)
    } else {
        default
    }
}

fn fade_alpha(remaining: f64, motion: f32) -> f32 {
    if motion >= 1.0 {
        (remaining / 0.3).clamp(0.0, 1.0) as f32
    } else {
        1.0
    }
}

impl KeycastSettings {
    fn scale(&self) -> f32 {
        bounded(self.scale, 0.75, 2.0, 1.0)
    }
    fn hold(&self) -> f64 {
        bounded(self.hold_secs, 0.5, 5.0, 2.0) as f64
    }
}

/// 描く行。文書・ログ・画像の書き出しには渡さない。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Row {
    Keys {
        chord: String,
        name: Option<String>,
        count: u32,
        down: bool,
        remaining: f64,
    },
    MouseCommand {
        name: String,
        count: u32,
        remaining: f64,
    },
    Mouse {
        down: [bool; 3],
        wheel: i8,
        drag: bool,
        double: bool,
    },
    Mouse3d {
        axes: [f32; 6],
        buttons: Vec<u8>,
    },
}

#[derive(Clone)]
struct Entry {
    key: Option<Key>,
    modifiers_only: bool,
    mods: Modifiers,
    chord: String,
    name: Option<String>,
    count: u32,
    down: bool,
    released: f64,
}

#[derive(Default)]
pub(crate) struct Keycast {
    settings: KeycastSettings,
    entries: Vec<Entry>,
    modifiers: String,
    modifiers_used: bool,
    mouse: [bool; 3],
    wheel: (i8, f64),
    drag: bool,
    double_until: f64,
    axes: [f32; 6],
    buttons: Vec<(u8, f64)>,
    mouse3d_visible: bool,
    frame_key: Option<String>,
    frame_click: bool,
}

fn modifiers(m: Modifiers) -> String {
    let mut parts = Vec::new();
    if m.command || m.ctrl || m.mac_cmd {
        parts.push("Ctrl");
    }
    if m.shift {
        parts.push("Shift");
    }
    if m.alt {
        parts.push("Alt");
    }
    parts.join("+")
}

fn modifier_key(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
    )
}

fn allowed(key: Key, mods: Modifiers, typing: bool) -> bool {
    if modifier_key(key) {
        return false;
    }
    !typing
        || mods.command
        || mods.ctrl
        || mods.mac_cmd
        || mods.alt
        || matches!(key, Key::Enter | Key::Escape | Key::Tab)
}

fn names_id() -> egui::Id {
    egui::Id::new("keycast_command_name")
}
fn enabled_id() -> egui::Id {
    egui::Id::new("keycast_enabled")
}

#[derive(Clone, Default)]
struct CommandName {
    text: String,
    key: bool,
}

/// 固定の画面名だけを渡す。入力された値をこの口に渡さない。
pub(crate) fn command_name(ctx: &egui::Context, text: String, key: bool) {
    ctx.data_mut(|d| {
        if d.get_temp::<bool>(enabled_id()) != Some(true) {
            return;
        }
        if d.get_temp::<CommandName>(names_id()).is_some_and(|n| n.key) {
            return;
        }
        d.insert_temp(names_id(), CommandName { text, key });
    });
}

/// 固定名の項目を画面で押した時だけ、その項目と同じ名前を渡す。
pub(crate) fn command_response(response: egui::Response, name: String) -> egui::Response {
    if response.clicked() {
        command_name(&response.ctx, name, false);
    }
    response
}

impl Keycast {
    fn change_modifiers(&mut self, next: Modifiers, now: f64) {
        let next = modifiers(next);
        if self.modifiers == next {
            return;
        }
        if !self.modifiers.is_empty() && !self.modifiers_used {
            self.entries.push(Entry {
                key: None,
                modifiers_only: true,
                mods: Modifiers::NONE,
                chord: self.modifiers.clone(),
                name: None,
                count: 1,
                down: false,
                released: now,
            });
        }
        self.modifiers = next;
        self.modifiers_used = false;
    }

    pub(crate) fn take_input(
        &mut self,
        input: &egui::InputState,
        typing: bool,
        s: &KeycastSettings,
    ) {
        if !s.enabled {
            *self = Self::default();
            return;
        }
        self.settings = s.clone();
        let now = input.time;
        self.frame_key = None;
        self.frame_click = input.pointer.button_clicked(PointerButton::Primary);
        self.entries
            .retain(|e| e.down || now - e.released < s.hold());
        for e in &mut self.entries {
            let down = e.key.is_some_and(|k| input.keys_down.contains(&k))
                && modifiers(e.mods) == modifiers(input.modifiers)
                && input.focused;
            if e.down && !down {
                e.released = now;
            }
            e.down = down;
        }
        if !s.keys {
            self.modifiers.clear();
            self.modifiers_used = false;
            self.entries
                .retain(|e| e.key.is_none() && !e.modifiers_only);
        }
        for event in &input.events {
            match event {
                egui::Event::ModifiersChanged(mods) if s.keys && input.focused => {
                    self.change_modifiers(*mods, now);
                }
                egui::Event::Key { key, modifiers, .. }
                    if s.keys && input.focused && modifier_key(*key) =>
                {
                    self.change_modifiers(*modifiers, now);
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if s.keys && allowed(*key, *modifiers, typing) => {
                    self.change_modifiers(*modifiers, now);
                    self.modifiers_used = true;
                    let chord = keys::chord_name((*modifiers, *key));
                    self.frame_key = Some(chord.clone());
                    self.entries.push(Entry {
                        key: Some(*key),
                        modifiers_only: false,
                        mods: *modifiers,
                        chord,
                        name: None,
                        count: 1,
                        down: input.keys_down.contains(key),
                        released: now,
                    });
                }
                egui::Event::PointerButton {
                    button: PointerButton::Primary,
                    pressed: true,
                    ..
                } => self.frame_click = true,
                egui::Event::MouseWheel { delta, .. } if s.mouse && delta.y != 0.0 => {
                    self.wheel = (if delta.y > 0.0 { 1 } else { -1 }, now + 0.5);
                }
                _ => {}
            }
        }
        if s.keys {
            self.change_modifiers(
                if input.focused {
                    input.modifiers
                } else {
                    Modifiers::NONE
                },
                now,
            );
            if !self.entries.iter().any(|e| e.down && e.key.is_some()) {
                self.modifiers_used = false;
            }
        }
        self.mouse = [
            PointerButton::Primary,
            PointerButton::Middle,
            PointerButton::Secondary,
        ]
        .map(|b| s.mouse && input.pointer.button_down(b));
        self.drag = self.mouse.iter().any(|d| *d)
            && input
                .pointer
                .press_origin()
                .zip(input.pointer.latest_pos())
                .is_some_and(|(a, b)| a.distance(b) > 4.0);
        if s.mouse
            && [
                PointerButton::Primary,
                PointerButton::Middle,
                PointerButton::Secondary,
            ]
            .iter()
            .any(|b| input.pointer.button_double_clicked(*b))
        {
            self.double_until = now + s.hold();
        }
        if !s.mouse {
            self.wheel = (0, 0.0);
            self.double_until = 0.0;
        }
        self.buttons.retain(|(_, t)| now - *t < s.hold());
    }

    pub(crate) fn prepare_mouse3d(&mut self, settings: &KeycastSettings, visible: bool) {
        self.settings = settings.clone();
        self.mouse3d_visible = visible;
        if !visible {
            self.axes = [0.0; 6];
            self.buttons.clear();
        }
    }

    pub(crate) fn take_mouse3d(&mut self, axes: [f32; 6], buttons: &[u8], now: f64) {
        if !self.settings.enabled || !self.settings.mouse3d {
            return;
        }
        self.axes = axes;
        for &b in buttons {
            self.buttons.retain(|(old, _)| *old != b);
            self.buttons.push((b, now));
        }
    }

    fn finish_names(&mut self, name: Option<CommandName>, now: f64) {
        if self.settings.enabled
            && self.settings.names
            && let Some(name) = name
        {
            if let Some(chord) = &self.frame_key {
                if let Some(e) = self.entries.iter_mut().rev().find(|e| &e.chord == chord) {
                    e.name = Some(name.text);
                }
            } else if self.frame_click && self.settings.mouse {
                self.entries.push(Entry {
                    key: None,
                    modifiers_only: false,
                    mods: Modifiers::NONE,
                    chord: String::new(),
                    name: Some(name.text),
                    count: 1,
                    down: false,
                    released: now,
                });
            }
        }
        // 名前を付けた後でまとめ、同じキーでも違うコマンドは別の行にする。
        let mut merged: Vec<Entry> = Vec::new();
        for mut e in self.entries.drain(..) {
            if !self.settings.names {
                e.name = None;
            }
            if let Some(last) = merged.last_mut()
                && last.chord == e.chord
                && last.name == e.name
            {
                let count = last.count.saturating_add(e.count);
                *last = Entry { count, ..e };
            } else {
                merged.push(e);
            }
        }
        if merged.len() > 3 {
            merged.drain(..merged.len() - 3);
        }
        self.entries = merged;
    }

    pub(crate) fn rows(&self, now: f64) -> Vec<Row> {
        let s = &self.settings;
        if !s.enabled {
            return Vec::new();
        }
        let mut rows = Vec::new();
        if s.mouse {
            rows.push(Row::Mouse {
                down: self.mouse,
                wheel: if now < self.wheel.1 { self.wheel.0 } else { 0 },
                drag: self.drag,
                double: now < self.double_until,
            });
        }
        if s.mouse3d && self.mouse3d_visible {
            rows.push(Row::Mouse3d {
                axes: self.axes,
                buttons: self
                    .buttons
                    .iter()
                    .filter(|(_, t)| now - t < s.hold())
                    .map(|(b, _)| *b)
                    .collect(),
            });
        }
        let active = self.entries.iter().any(|e| e.down && e.key.is_some());
        if s.keys && !self.modifiers.is_empty() && !active {
            rows.push(Row::Keys {
                chord: self.modifiers.clone(),
                name: None,
                count: 1,
                down: true,
                remaining: s.hold(),
            });
        }
        for e in self.entries.iter().rev() {
            let remaining = s.hold() - (now - e.released);
            if !e.down && remaining <= 0.0 {
                continue;
            }
            if (e.key.is_some() || e.modifiers_only) && s.keys {
                rows.push(Row::Keys {
                    chord: e.chord.clone(),
                    name: if s.names { e.name.clone() } else { None },
                    count: e.count,
                    down: e.down,
                    remaining,
                });
            } else if e.key.is_none()
                && s.mouse
                && s.names
                && let Some(name) = &e.name
            {
                rows.push(Row::MouseCommand {
                    name: name.clone(),
                    count: e.count,
                    remaining,
                });
            }
        }
        // 修飾キーだけの行を含め、キー・コマンドの行は合わせて 3 行まで。
        let fixed = rows
            .iter()
            .take_while(|r| matches!(r, Row::Mouse { .. } | Row::Mouse3d { .. }))
            .count();
        rows.truncate(fixed + 3);
        rows
    }

    pub(crate) fn show(
        &self,
        ctx: &egui::Context,
        view_rect: Rect,
        look: theme::Look,
        s: &KeycastSettings,
    ) {
        let now = ctx.input(|i| i.time);
        let rows = self.rows(now);
        if rows.is_empty() || !view_rect.is_positive() {
            return;
        }
        let size = theme::fs(20.0) * s.scale();
        let pos = match s.place {
            KeycastPlace::LeftBottom => {
                egui::pos2(view_rect.left() + 12.0, view_rect.bottom() - 90.0)
            }
            KeycastPlace::CenterBottom => egui::pos2(
                view_rect.center().x,
                view_rect.bottom() - 20.0 - 32.0 * theme::band() - theme::fs(12.0),
            ),
        };
        let pivot = match s.place {
            KeycastPlace::LeftBottom => egui::Align2::LEFT_BOTTOM,
            KeycastPlace::CenterBottom => egui::Align2::CENTER_BOTTOM,
        };
        let background_alpha = rows
            .iter()
            .map(|row| match row {
                Row::Keys {
                    remaining,
                    down: false,
                    ..
                }
                | Row::MouseCommand { remaining, .. } => fade_alpha(*remaining, look.scale.motion),
                _ => 1.0,
            })
            .fold(0.0f32, f32::max);
        // 下端を固定する領域は、行の高さが変わるコマで先に測り直す。
        // 寸法の指標だけを覚え、キーや操作名の控えは増やさない。
        let geometry = egui::Id::new((
            size.to_bits(),
            view_rect.width().to_bits(),
            rows.len(),
            rows.iter()
                .filter_map(|row| match row {
                    Row::Mouse3d { axes, buttons } => {
                        Some((axes.iter().filter(|v| **v != 0.0).count(), buttons.len()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
        ));
        let resize = ctx.data_mut(|d| {
            let id = egui::Id::new("input_keycast_geometry");
            let previous = d.get_temp::<egui::Id>(id);
            d.insert_temp(id, geometry);
            previous.is_some_and(|previous| previous != geometry)
        });
        egui::Area::new(egui::Id::new("input_keycast"))
            .sizing_pass(resize)
            .order(egui::Order::Foreground)
            .interactable(false)
            .constrain_to(view_rect)
            .pivot(pivot)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                ui.set_clip_rect(view_rect);
                ui.set_max_width((view_rect.width() - 24.0).max(1.0));
                egui::Frame::new()
                    .fill(look.pal.chrome.gamma_multiply(0.85 * background_alpha))
                    .corner_radius(5.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        // 下からマウス、3D マウス、新しいキーの順に積む。
                        for row in rows.iter().rev() {
                            let (remaining, down) = match row {
                                Row::Keys {
                                    remaining, down, ..
                                } => (*remaining, *down),
                                Row::MouseCommand { remaining, .. } => (*remaining, false),
                                _ => (s.hold(), true),
                            };
                            let opacity = if down {
                                1.0
                            } else {
                                fade_alpha(remaining, look.scale.motion)
                            };
                            ui.scope(|ui| {
                                ui.multiply_opacity(opacity);
                                let r = ui
                                    .horizontal(|ui| match row {
                                        Row::Keys {
                                            chord,
                                            name,
                                            count,
                                            down,
                                            ..
                                        } => {
                                            let prefix = chord.strip_suffix('+').unwrap_or(chord);
                                            for cap in
                                                prefix.split('+').filter(|cap| !cap.is_empty())
                                            {
                                                key_cap(ui, cap, size, *down, look);
                                            }
                                            if chord.ends_with('+') {
                                                key_cap(ui, "+", size, *down, look);
                                            }
                                            if let Some(name) = name {
                                                ui.label(
                                                    egui::RichText::new(name)
                                                        .size(size * 0.75)
                                                        .color(look.pal.ink2),
                                                );
                                            }
                                            if *count > 1 {
                                                ui.label(
                                                    egui::RichText::new(format!("×{count}"))
                                                        .size(size * 0.75)
                                                        .color(look.pal.ink),
                                                );
                                            }
                                        }
                                        Row::MouseCommand { name, count, .. } => {
                                            mouse(ui, [true, false, false], size, look);
                                            ui.label(
                                                egui::RichText::new(name)
                                                    .size(size * 0.75)
                                                    .color(look.pal.ink2),
                                            );
                                            if *count > 1 {
                                                ui.label(format!("×{count}"));
                                            }
                                        }
                                        Row::Mouse {
                                            down,
                                            wheel,
                                            drag,
                                            double,
                                        } => {
                                            mouse(ui, *down, size, look);
                                            for (text, remaining) in [
                                                (
                                                    if *wheel > 0 {
                                                        "▲"
                                                    } else if *wheel < 0 {
                                                        "▼"
                                                    } else {
                                                        ""
                                                    },
                                                    self.wheel.1 - now,
                                                ),
                                                (
                                                    if *drag { "ドラッグ" } else { "" },
                                                    f64::INFINITY,
                                                ),
                                                (
                                                    if *double { "×2" } else { "" },
                                                    self.double_until - now,
                                                ),
                                            ] {
                                                if !text.is_empty() {
                                                    ui.label(
                                                        egui::RichText::new(text)
                                                            .size(size * 0.75)
                                                            .color(look.pal.ink.gamma_multiply(
                                                                fade_alpha(
                                                                    remaining,
                                                                    look.scale.motion,
                                                                ),
                                                            )),
                                                    );
                                                }
                                            }
                                        }
                                        Row::Mouse3d { axes, buttons } => {
                                            let (rect, _) = ui.allocate_exact_size(
                                                Vec2::new(size * 3.0, size * 2.1),
                                                egui::Sense::hover(),
                                            );
                                            for (i, value) in axes.iter().enumerate() {
                                                let y = rect.top() + (i as f32 + 0.5) * size * 0.34;
                                                let a = egui::pos2(rect.center().x, y);
                                                ui.painter().line_segment(
                                                    [
                                                        egui::pos2(rect.left(), y),
                                                        egui::pos2(rect.right(), y),
                                                    ],
                                                    Stroke::new(1.0, look.pal.edge),
                                                );
                                                if *value != 0.0 {
                                                    ui.painter().line_segment(
                                                        [
                                                            a,
                                                            a + Vec2::new(
                                                                value * rect.width() * 0.5,
                                                                0.0,
                                                            ),
                                                        ],
                                                        Stroke::new(3.0, look.ws()),
                                                    );
                                                }
                                            }
                                            ui.vertical(|ui| {
                                                for (i, value) in axes.iter().enumerate() {
                                                    if value.abs() > 0.0 {
                                                        ui.label(
                                                            egui::RichText::new(
                                                                crate::mouse3d::AXIS_NAMES[i],
                                                            )
                                                            .size(size * 0.65)
                                                            .color(look.pal.ink),
                                                        );
                                                    }
                                                }
                                                for b in buttons {
                                                    let remaining = self
                                                        .buttons
                                                        .iter()
                                                        .find(|(button, _)| button == b)
                                                        .map_or(0.0, |(_, time)| {
                                                            s.hold() - (now - time)
                                                        });
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "3D マウス ボタン {b}"
                                                        ))
                                                        .size(size * 0.65)
                                                        .color(look.pal.ink.gamma_multiply(
                                                            fade_alpha(
                                                                remaining,
                                                                look.scale.motion,
                                                            ),
                                                        )),
                                                    );
                                                }
                                            });
                                        }
                                    })
                                    .response;
                                r.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Label,
                                        true,
                                        row.label(),
                                    )
                                });
                            });
                        }
                    });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

impl Row {
    fn label(&self) -> String {
        let text = match self {
            Self::Keys { chord, name, .. } => format!(
                "{chord}{}",
                name.as_ref().map(|n| format!(" {n}")).unwrap_or_default()
            ),
            Self::MouseCommand { name, .. } => format!("マウス {name}"),
            Self::Mouse { down, .. } => format!(
                "マウス{}",
                ["左", "中", "右"]
                    .iter()
                    .zip(down)
                    .filter(|(_, d)| **d)
                    .map(|(n, _)| format!(" {n}"))
                    .collect::<String>()
            ),
            Self::Mouse3d { .. } => "3D マウス".into(),
        };
        format!("操作の表示: {text}")
    }
}

fn key_cap(ui: &mut egui::Ui, text: &str, size: f32, down: bool, look: theme::Look) {
    egui::Frame::new()
        .fill(if down {
            look.ws()
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::new(
            1.0,
            if down { look.ws() } else { look.pal.edge },
        ))
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(size).color(if down {
                look.pal.chrome
            } else {
                look.pal.ink
            }));
        });
}

fn mouse(ui: &mut egui::Ui, down: [bool; 3], size: f32, look: theme::Look) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(size * 1.25, size * 1.65), egui::Sense::hover());
    let p = ui.painter();
    p.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.3, look.pal.edge),
        egui::StrokeKind::Inside,
    );
    for (i, on) in [down[0], down[2]].into_iter().enumerate() {
        let r = Rect::from_min_size(
            rect.min + Vec2::new(i as f32 * rect.width() * 0.5, 0.0),
            Vec2::new(rect.width() * 0.5, rect.height() * 0.48),
        );
        if on {
            p.rect_filled(r.shrink(1.5), 3.0, look.ws());
        }
    }
    let wheel = Rect::from_center_size(
        rect.center_top() + Vec2::new(0.0, rect.height() * 0.26),
        Vec2::new(size * 0.18, size * 0.45),
    );
    p.rect(
        wheel,
        2.0,
        if down[1] { look.ws() } else { look.pal.chrome },
        Stroke::new(1.0, look.pal.ink2),
        egui::StrokeKind::Inside,
    );
    p.line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1.0, look.pal.edge),
    );
}

impl crate::Workspace {
    /// 登録済みの固定名との一致だけを許す。動的な操作名には利用者の名前や値が入る。
    pub(crate) fn keycast_fixed_name(&self, text_id: &str, name: &str) -> Option<String> {
        kmb_i18n::Lang::ALL
            .into_iter()
            .any(|lang| {
                self.win
                    .texts
                    .find(lang, text_id)
                    .is_some_and(|fixed| fixed == name)
            })
            .then(|| name.to_string())
    }

    pub(crate) fn keycast_item_name(&self, key: &str) -> Option<String> {
        let item = self.item_of(key)?;
        if let Some(id) = key.strip_prefix("a:") {
            self.keycast_fixed_name(&kmb_i18n::action_id(id), &item.label)
        } else if let Some(id) = key.strip_prefix("f:") {
            self.keycast_fixed_name(&kmb_i18n::feature_id(id), &item.label)
        } else {
            Some(item.label)
        }
    }

    pub(crate) fn keycast_command_name(
        &self,
        command: &crate::commands_ui::Command,
    ) -> Option<String> {
        use crate::commands_ui::CommandId;
        match &command.id {
            CommandId::Action(id) => {
                self.keycast_fixed_name(&kmb_i18n::action_id(id), &command.name)
            }
            CommandId::Add(id) => self.keycast_fixed_name(&kmb_i18n::feature_id(id), &command.name),
            // 基本項目と編集器の道具の名前は、固定の一覧で決まる。
            _ => Some(command.name.clone()),
        }
    }

    pub(crate) fn take_keycast(&mut self, ctx: &egui::Context) {
        let s = &self.win.layout.keycast;
        ctx.data_mut(|d| {
            d.insert_temp(enabled_id(), s.enabled);
            d.remove_temp::<CommandName>(names_id());
        });
        let typing = ctx.egui_wants_keyboard_input();
        ctx.input(|i| self.win.keycast.take_input(i, typing, s));
    }

    pub(crate) fn show_keycast(&mut self, ctx: &egui::Context) {
        let s = &self.win.layout.keycast;
        let name = ctx.data_mut(|d| {
            d.insert_temp(enabled_id(), s.enabled);
            d.remove_temp::<CommandName>(names_id())
        });
        if !s.enabled {
            self.win.keycast = Keycast::default();
            return;
        }
        self.win.keycast.settings = s.clone();
        self.win.keycast.mouse3d_visible &= self.win.layout.mouse3d.enabled;
        // 欄を描く途中で入力先が移ったコマも、最初の文字を描く前に除く。
        if ctx.egui_wants_keyboard_input() {
            self.win
                .keycast
                .entries
                .retain(|e| e.key.is_none_or(|key| allowed(key, e.mods, true)));
        }
        self.win.keycast.finish_names(name, ctx.input(|i| i.time));
        self.win.keycast.show(
            ctx,
            self.win.view_rect,
            self.win.look,
            &self.win.layout.keycast,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_frame(ctx: &egui::Context, raw: egui::RawInput, mut frame: impl FnMut(&egui::Context)) {
        ctx.run_ui(raw, |ui| frame(ui.ctx()))
            .drop_without_applying_deltas();
    }

    fn on() -> KeycastSettings {
        KeycastSettings {
            enabled: true,
            mouse: false,
            mouse3d: false,
            ..Default::default()
        }
    }

    fn key(key: Key, mods: Modifiers, pressed: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: mods,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn input(
        ctx: &egui::Context,
        k: &mut Keycast,
        s: &KeycastSettings,
        now: f64,
        mut events: Vec<egui::Event>,
        mods: Modifiers,
        typing: bool,
        name: Option<CommandName>,
    ) {
        events.insert(0, egui::Event::ModifiersChanged(mods));
        run_frame(
            ctx,
            egui::RawInput {
                time: Some(now),
                events,
                ..Default::default()
            },
            |ctx| {
                ctx.input(|i| k.take_input(i, typing, s));
            },
        );
        k.finish_names(name, now);
    }

    #[test]
    fn disabled_drops_input_and_released_keys_expire_after_the_hold_time() {
        let ctx = egui::Context::default();
        let mut k = Keycast::default();
        input(
            &ctx,
            &mut k,
            &KeycastSettings::default(),
            1.0,
            vec![key(Key::F8, Modifiers::NONE, true)],
            Modifiers::NONE,
            false,
            None,
        );
        assert!(k.rows(1.0).is_empty() && k.entries.is_empty());
        input(
            &ctx,
            &mut k,
            &on(),
            2.0,
            vec![key(Key::F9, Modifiers::NONE, true)],
            Modifiers::NONE,
            false,
            None,
        );
        assert!(matches!(&k.rows(20.0)[0], Row::Keys { chord, down: true, .. } if chord == "F9"));
        input(
            &ctx,
            &mut k,
            &on(),
            21.0,
            vec![key(Key::F9, Modifiers::NONE, false)],
            Modifiers::NONE,
            false,
            None,
        );
        assert!(!k.rows(22.9).is_empty());
        assert!(k.rows(23.0).is_empty());
        input(
            &ctx,
            &mut k,
            &KeycastSettings::default(),
            24.0,
            vec![],
            Modifiers::NONE,
            false,
            None,
        );
        assert!(k.entries.is_empty());
    }

    #[test]
    fn chord_order_repetitions_modifiers_and_three_row_limit() {
        let ctx = egui::Context::default();
        let mut k = Keycast::default();
        let mods = Modifiers {
            command: true,
            shift: true,
            alt: true,
            ..Modifiers::NONE
        };
        input(
            &ctx,
            &mut k,
            &on(),
            1.0,
            vec![key(Key::Z, mods, true)],
            mods,
            false,
            None,
        );
        input(
            &ctx,
            &mut k,
            &on(),
            1.1,
            vec![key(Key::Z, mods, true)],
            mods,
            false,
            None,
        );
        assert!(
            matches!(&k.rows(1.1)[0], Row::Keys { chord, count: 2, .. } if chord == "Ctrl+Shift+Alt+Z")
        );
        for (i, key) in [Key::A, Key::B, Key::C, Key::D].into_iter().enumerate() {
            input(
                &ctx,
                &mut k,
                &on(),
                1.2 + i as f64 * 0.1,
                vec![self::key(key, Modifiers::NONE, true)],
                Modifiers::NONE,
                false,
                None,
            );
        }
        assert_eq!(k.rows(1.6).len(), 3);
        assert!(matches!(&k.rows(1.6)[0], Row::Keys { chord, .. } if chord == "D"));
        let ctx = egui::Context::default();
        input(
            &ctx,
            &mut k,
            &on(),
            2.0,
            vec![key(Key::ShiftLeft, Modifiers::SHIFT, true)],
            Modifiers::SHIFT,
            false,
            None,
        );
        assert!(
            matches!(&k.rows(2.0)[0], Row::Keys { chord, down: true, name: None, .. } if chord == "Shift")
        );
        input(
            &ctx,
            &mut k,
            &on(),
            2.1,
            vec![key(Key::ShiftLeft, Modifiers::NONE, false)],
            Modifiers::NONE,
            false,
            None,
        );
        assert!(
            k.rows(2.2)
                .iter()
                .any(|row| matches!(row, Row::Keys { chord, down: false, .. } if chord == "Shift")),
            "修飾キーだけの札も離した後に表示時間だけ残る"
        );
        input(
            &ctx,
            &mut k,
            &on(),
            2.3,
            vec![
                key(Key::F8, Modifiers::NONE, true),
                key(Key::F8, Modifiers::NONE, false),
            ],
            Modifiers::NONE,
            false,
            None,
        );
        assert!(
            k.rows(2.4)
                .iter()
                .any(|row| matches!(row, Row::Keys { chord, down: false, .. } if chord == "Shift")),
            "次の組を押しても、修飾キーの表示時間を短くしない"
        );
        assert!(k.rows(4.4).is_empty());
        input(
            &ctx,
            &mut k,
            &on(),
            5.0,
            vec![
                key(Key::ShiftLeft, Modifiers::SHIFT, true),
                key(Key::ShiftLeft, Modifiers::NONE, false),
            ],
            Modifiers::NONE,
            false,
            None,
        );
        assert!(
            matches!(&k.rows(5.1)[0], Row::Keys { chord, down: false, .. } if chord == "Shift"),
            "一つのコマで押して離した修飾キーも出す"
        );
    }

    #[test]
    fn repeated_events_in_one_frame_are_counted_before_limiting_rows() {
        let ctx = egui::Context::default();
        let mut k = Keycast::default();
        input(
            &ctx,
            &mut k,
            &on(),
            1.0,
            (0..4)
                .map(|_| key(Key::F8, Modifiers::NONE, true))
                .collect(),
            Modifiers::NONE,
            false,
            None,
        );
        assert!(matches!(
            &k.rows(1.0)[..],
            [Row::Keys { chord, count: 4, .. }] if chord == "F8"
        ));
    }

    #[test]
    fn typing_never_keeps_plain_letters_digits_spaces_or_shift_letters() {
        let ctx = egui::Context::default();
        let mut k = Keycast::default();
        let events = [Key::A, Key::Num4, Key::Space]
            .into_iter()
            .map(|k| key(k, Modifiers::NONE, true))
            .chain([key(Key::B, Modifiers::SHIFT, true)])
            .collect();
        input(
            &ctx,
            &mut k,
            &on(),
            1.0,
            events,
            Modifiers::NONE,
            true,
            None,
        );
        assert!(k.rows(1.0).is_empty() && k.entries.is_empty());
        for (i, (code, mods)) in [
            (Key::Z, Modifiers::CTRL),
            (Key::Enter, Modifiers::NONE),
            (Key::Escape, Modifiers::NONE),
            (Key::Tab, Modifiers::NONE),
            (Key::A, Modifiers::ALT),
        ]
        .into_iter()
        .enumerate()
        {
            input(
                &ctx,
                &mut k,
                &on(),
                1.1 + i as f64 * 0.1,
                vec![key(code, mods, true)],
                mods,
                true,
                None,
            );
            assert!(
                matches!(&k.rows(1.1)[0], Row::Keys { chord, .. } if chord == &keys::chord_name((mods, code)))
            );
        }
    }

    #[test]
    fn typing_focus_started_in_the_same_frame_hides_the_first_letter() {
        let ctx = egui::Context::default();
        let mut ws = crate::Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules());
        ws.win.layout.keycast = on();
        let mut value = String::new();
        let mut field = Rect::NOTHING;
        let mut field_id = egui::Id::NULL;
        ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let response = ui.text_edit_singleline(&mut value);
                field = response.rect;
                field_id = response.id;
            });
        })
        .drop_without_applying_deltas();
        assert!(!ctx.egui_wants_keyboard_input());
        let pos = field.center();
        ctx.run_ui(
            egui::RawInput {
                time: Some(1.0),
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    },
                    key(Key::A, Modifiers::NONE, true),
                    egui::Event::Text("a".into()),
                ],
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx().clone();
                ws.take_keycast(&ctx);
                // 入力を受け取った後、欄を描く途中で入力先が移る境界を作る。
                ctx.memory_mut(|m| m.request_focus(field_id));
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.text_edit_singleline(&mut value);
                });
                assert_eq!(value, "a", "欄を押した同じコマで文字を打つ");
                assert!(ctx.egui_wants_keyboard_input());
                ws.show_keycast(&ctx);
            },
        )
        .drop_without_applying_deltas();
        assert!(
            !ws.win
                .keycast
                .rows(1.0)
                .iter()
                .any(|row| matches!(row, Row::Keys { chord, .. } if chord == "A")),
            "欄へ入った最初の文字を表示しない"
        );
        ctx.memory_mut(|m| m.surrender_focus(field_id));
        run_frame(
            &ctx,
            egui::RawInput {
                time: Some(1.1),
                ..Default::default()
            },
            |ctx| {
                ws.take_keycast(ctx);
                assert!(!ctx.egui_wants_keyboard_input());
                ws.show_keycast(ctx);
            },
        );
        assert!(
            !ws.win
                .keycast
                .rows(1.1)
                .iter()
                .any(|row| matches!(row, Row::Keys { chord, .. } if chord == "A")),
            "表示時間の内に欄から出ても、打った文字が履歴から戻らない"
        );
    }

    #[test]
    fn mouse_buttons_wheel_drag_and_double_click_are_transient() {
        let ctx = egui::Context::default();
        let mut k = Keycast::default();
        let s = KeycastSettings {
            mouse: true,
            ..on()
        };
        let at = egui::pos2(50.0, 50.0);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        input(
            &ctx,
            &mut k,
            &s,
            1.0,
            vec![egui::Event::PointerMoved(at), press(true)],
            Modifiers::NONE,
            false,
            None,
        );
        assert!(matches!(
            k.rows(1.0)[0],
            Row::Mouse {
                down: [true, false, false],
                drag: false,
                ..
            }
        ));
        input(
            &ctx,
            &mut k,
            &s,
            1.1,
            vec![
                egui::Event::PointerMoved(at + Vec2::new(5.0, 0.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: Vec2::new(0.0, 1.0),
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
            false,
            None,
        );
        assert!(matches!(
            k.rows(1.1)[0],
            Row::Mouse {
                drag: true,
                wheel: 1,
                ..
            }
        ));
        assert!(matches!(k.rows(1.7)[0], Row::Mouse { wheel: 0, .. }));
        // 二度押しは動かさず、短い間隔で押して放す。
        let ctx = egui::Context::default();
        for (now, pressed) in [(2.0, true), (2.05, false), (2.1, true), (2.15, false)] {
            input(
                &ctx,
                &mut k,
                &s,
                now,
                vec![egui::Event::PointerMoved(at), press(pressed)],
                Modifiers::NONE,
                false,
                None,
            );
        }
        assert!(matches!(
            k.rows(2.15)[0],
            Row::Mouse {
                down: [false, false, false],
                double: true,
                ..
            }
        ));
        assert!(matches!(k.rows(4.2)[0], Row::Mouse { double: false, .. }));
        input(
            &ctx,
            &mut k,
            &s,
            5.0,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: Vec2::new(0.0, -1.0),
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
            false,
            None,
        );
        assert!(matches!(k.rows(5.0)[0], Row::Mouse { wheel: -1, .. }));
    }

    #[test]
    fn command_names_attach_once_to_keys_or_clicks_and_can_be_hidden() {
        let ctx = egui::Context::default();
        let mut k = Keycast::default();
        let name = || {
            Some(CommandName {
                text: "取り消す".into(),
                key: true,
            })
        };
        for now in [1.0, 1.1] {
            input(
                &ctx,
                &mut k,
                &on(),
                now,
                vec![key(Key::Z, Modifiers::CTRL, true)],
                Modifiers::CTRL,
                false,
                name(),
            );
        }
        assert_eq!(k.rows(1.1).len(), 1);
        assert!(
            matches!(&k.rows(1.1)[0], Row::Keys { chord, name: Some(n), count: 2, .. } if chord == "Ctrl+Z" && n == "取り消す")
        );
        let s = KeycastSettings {
            names: false,
            ..on()
        };
        input(
            &ctx,
            &mut k,
            &s,
            1.2,
            vec![key(Key::F8, Modifiers::NONE, true)],
            Modifiers::NONE,
            false,
            name(),
        );
        assert!(
            k.rows(1.2)
                .iter()
                .all(|r| matches!(r, Row::Keys { name: None, .. }))
        );
        let mut k = Keycast::default();
        let s = KeycastSettings {
            mouse: true,
            ..on()
        };
        input(
            &ctx,
            &mut k,
            &s,
            2.0,
            vec![],
            Modifiers::NONE,
            false,
            name(),
        );
        assert_eq!(
            k.rows(2.0).len(),
            1,
            "画面の入力を通らないコマンドは出さない"
        );
        input(
            &ctx,
            &mut k,
            &s,
            2.1,
            vec![egui::Event::PointerButton {
                pos: egui::Pos2::ZERO,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
            false,
            name(),
        );
        assert!(matches!(&k.rows(2.1)[1], Row::MouseCommand { name, .. } if name == "取り消す"));
    }

    #[test]
    fn temp_names_give_keyboard_priority_and_use_the_display_language() {
        let ctx = egui::Context::default();
        ctx.data_mut(|d| d.insert_temp(enabled_id(), true));
        let row = keys::KEYS.iter().find(|r| r.id == "undo").unwrap();
        assert_eq!(keys::row_text(&ctx, row), "取り消す");
        command_name(&ctx, "取り消す".into(), true);
        command_name(&ctx, "押し出し".into(), false);
        assert_eq!(
            ctx.data_mut(|d| d.remove_temp::<CommandName>(names_id()))
                .unwrap()
                .text,
            "取り消す"
        );
        let mut ws = crate::Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules());
        ws.win.layout.appearance.language = "en".into();
        ws.win.words().share(&ctx);
        assert_eq!(keys::row_text(&ctx, row), "Undo");
        ctx.data_mut(|d| d.insert_temp(enabled_id(), false));
        command_name(&ctx, "Undo".into(), true);
        assert!(
            ctx.data_mut(|d| d.remove_temp::<CommandName>(names_id()))
                .is_none()
        );
    }

    #[test]
    fn command_hooks_share_fixed_names_with_shortcuts_items_and_editor_keys() {
        let ctx = egui::Context::default();
        let mut ws = crate::Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules());
        ws.win.repaint = Some(ctx.clone());
        ws.win.layout.keycast = KeycastSettings {
            mouse: true,
            ..on()
        };
        let undo = keys::KEYS.iter().find(|r| r.id == "undo").unwrap();
        run_frame(
            &ctx,
            egui::RawInput {
                time: Some(1.0),
                events: vec![key(Key::Z, Modifiers::CTRL, true)],
                ..Default::default()
            },
            |ctx| {
                ws.take_keycast(ctx);
                ws.run_key(ctx, undo);
                ws.show_keycast(ctx);
            },
        );
        assert!(ws.win.keycast.rows(1.0).iter().any(|r| matches!(r, Row::Keys { chord, name: Some(n), .. } if chord == "Ctrl+Z" && n == "取り消す")));
        let item = ws.item_of("u:measure").unwrap().label;
        run_frame(
            &ctx,
            egui::RawInput {
                time: Some(1.1),
                events: vec![egui::Event::PointerButton {
                    pos: egui::pos2(10.0, 10.0),
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| {
                ws.take_keycast(ctx);
                ws.run_item("u:measure");
                ws.show_keycast(ctx);
            },
        );
        assert!(
            ws.win
                .keycast
                .rows(1.1)
                .iter()
                .any(|r| matches!(r, Row::MouseCommand { name, .. } if name == &item))
        );
        run_frame(
            &ctx,
            egui::RawInput {
                time: Some(1.2),
                events: vec![key(Key::L, Modifiers::NONE, true)],
                ..Default::default()
            },
            |ctx| {
                ws.take_keycast(ctx);
                assert!(keys::pressed(ctx, "sketch_line"));
                ws.show_keycast(ctx);
            },
        );
        assert!(ws.win.keycast.rows(1.2).iter().any(|r| matches!(r, Row::Keys { chord, name: Some(n), .. } if chord == "L" && n == "スケッチ: 線分")));
    }

    #[test]
    fn dynamic_command_names_never_reach_the_input_overlay() {
        use crate::commands_ui::{Command, CommandId};
        let ws = crate::Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules());
        let action = ws
            .actions()
            .into_iter()
            .find(|a| {
                ws.keycast_fixed_name(&kmb_i18n::action_id(&a.id), &a.label)
                    .is_some()
            })
            .unwrap();
        let mut command = Command {
            id: CommandId::Action(action.id.clone()),
            name: action.label.clone(),
            aliases: String::new(),
            disabled: None,
        };
        assert_eq!(ws.keycast_command_name(&command), Some(action.label));
        command.name = "秘密のファイル名・部品名・式 123".into();
        assert!(
            ws.keycast_command_name(&command).is_none(),
            "固定 ID に名前や値を継ぎ足しても出さない"
        );
        command.id = CommandId::Action(format!("{}.秘密のテンプレート", action.id));
        assert!(
            ws.keycast_command_name(&command).is_none(),
            "利用者の名前を持つ動的 ID は出さない"
        );
        command.id = CommandId::Add("利用者の名前".into());
        assert!(ws.keycast_command_name(&command).is_none());
    }

    #[test]
    fn old_layout_defaults_and_invalid_numbers_are_bounded() {
        let old: crate::layout::Layout = serde_json::from_str("{}").unwrap();
        assert_eq!(old.keycast, KeycastSettings::default());
        let mut layout = crate::layout::Layout {
            keycast: KeycastSettings {
                enabled: true,
                place: KeycastPlace::CenterBottom,
                scale: 1.5,
                hold_secs: 3.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let back: crate::layout::Layout =
            serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
        assert_eq!(back.keycast, layout.keycast);
        layout.reset_arrangement();
        assert_eq!(layout.keycast, back.keycast);
        let s = KeycastSettings {
            scale: f32::NAN,
            hold_secs: f32::INFINITY,
            ..on()
        };
        assert_eq!((s.scale(), s.hold()), (1.0, 2.0));
        let s = KeycastSettings {
            scale: 50.0,
            hold_secs: -1.0,
            ..on()
        };
        assert_eq!((s.scale(), s.hold()), (2.0, 0.5));
    }

    #[test]
    fn fake_mouse3d_axes_use_the_navigation_curve_and_do_not_steal_buttons() {
        let device = kmb_ui_mouse3d::Mouse3d::fake();
        device.feed(&[2, 0, 0, 0, 0, 0xA2, 0xFE]);
        device.feed(&[3, 1, 0, 0, 0]);
        let mut ws = crate::Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
            .with_mouse3d(device);
        ws.win.layout.keycast = KeycastSettings {
            mouse3d: true,
            ..on()
        };
        let ctx = egui::Context::default();
        run_frame(&ctx, egui::RawInput::default(), |ctx| ws.feed_mouse3d(ctx));
        let rows = ws.win.keycast.rows(0.0);
        assert!(
            matches!(&rows[0], Row::Mouse3d { axes, buttons } if axes[5] == -1.0 && buttons == &[1])
        );
        let nav = ctx
            .data_mut(|d| d.get_temp::<kmb_ui_view3d::view3d::Nav>(egui::Id::new("kmb-view3d-nav")))
            .unwrap();
        assert_eq!(nav.press, Some(kmb_ui_view3d::view3d::NavPress::Fit));
        assert!(
            ws.win
                .mouse3d
                .as_ref()
                .unwrap()
                .snapshot()
                .buttons_pressed
                .is_empty()
        );
        ws.win.layout.mouse3d.enabled = false;
        run_frame(&ctx, egui::RawInput::default(), |ctx| ws.feed_mouse3d(ctx));
        assert!(ws.win.keycast.rows(0.0).is_empty());
        let s = crate::mouse3d::Mouse3dSettings::default();
        assert_eq!(crate::mouse3d::live_axes([0.04; 6], &s), [0.0; 6]);
    }

    #[test]
    fn overlay_does_not_intercept_the_view_below_it() {
        let k = Keycast {
            settings: KeycastSettings {
                mouse: true,
                ..on()
            },
            ..Default::default()
        };
        let mut h = egui_kittest::Harness::builder()
            .with_size(Vec2::new(600.0, 400.0))
            .build_ui_state(
                |ui, clicks: &mut usize| {
                    let ctx = ui.ctx().clone();
                    let r = ui.interact(
                        Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(600.0, 400.0)),
                        egui::Id::new("view_below"),
                        egui::Sense::click(),
                    );
                    if r.clicked() {
                        *clicks += 1;
                    }
                    k.show(
                        &ctx,
                        Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(600.0, 400.0)),
                        theme::Look::default(),
                        &k.settings,
                    );
                },
                0usize,
            );
        h.run_steps(2);
        h.hover_at(egui::pos2(30.0, 290.0));
        h.drag_at(egui::pos2(30.0, 290.0));
        h.run_steps(2);
        h.drop_at(egui::pos2(30.0, 290.0));
        h.run_steps(2);
        assert_eq!(*h.state(), 1);
    }

    #[test]
    fn a_new_key_row_stays_above_the_axis_on_its_first_frame() {
        use egui_kittest::kittest::Queryable;
        for scale in [1.0, 2.5] {
            let mut ws = crate::Workspace::new(
                kmb_samples::reference_document(),
                None,
                kmb_bundle::modules(),
            );
            ws.win.layout.keycast.enabled = true;
            ws.win.layout.appearance.text_scale = scale;
            let mut h = egui_kittest::Harness::builder()
                .with_size(Vec2::new(1280.0, 1000.0))
                .build_ui_state(|ui, ws: &mut crate::Workspace| ws.ui(ui), ws);
            h.run_steps(4);
            assert!(h.query_by_label("操作の表示: マウス").is_some());
            h.input_mut()
                .events
                .push(key(Key::F8, Modifiers::NONE, true));
            h.step();
            let limit = h.state().win.view_rect.bottom() - 75.0;
            if let Some(mouse) = h.query_by_label("操作の表示: マウス") {
                let mouse = mouse.rect();
                assert!(
                    mouse.bottom() <= limit,
                    "字の倍率 {scale}: 新しい行の最初のコマも軸の上に収める: {mouse:?}, 下端の限度 {limit}"
                );
            }
            // 領域を測るコマは描かず、次のコマには新しい行を正しい位置で出す。
            h.step();
            assert!(h.query_by_label("操作の表示: F8").is_some());
            assert!(h.get_by_label("操作の表示: マウス").rect().bottom() <= limit);
        }
    }

    fn check_context_menu_name(item: &str) {
        let ws = crate::Workspace::new(
            kmb_samples::reference_document(),
            None,
            kmb_bundle::modules(),
        );
        let mut h = egui_kittest::Harness::builder()
            .with_size(Vec2::new(1600.0, 900.0))
            .build_ui_state(|ui, ws: &mut crate::Workspace| ws.ui(ui), ws);
        let script = format!(
            r#"key Ctrl+,
click "マウスとキー"
type ~"設定を探す" "操作の表示"
click "押したキーとマウスを画面に出す(録画・解説向け)"
expect "コマンドの名前も出す" exists
key Ctrl+,
pick f8:end
rclick ~"選んだ面:"
click "{item}"
expect "操作の表示: マウス {item}" exists
"#
        );
        let result = crate::cmdline_tests::run(&mut h, &script);
        assert!(result.ok, "{}", result.report());
    }

    #[test]
    fn context_menu_measure_uses_the_clicked_fixed_name() {
        check_context_menu_name("測る");
    }

    #[test]
    fn context_menu_registered_action_uses_the_clicked_fixed_name() {
        check_context_menu_name("面にスケッチ");
    }

    #[test]
    fn ui_tools_enable_the_overlay_and_find_the_pressed_key() {
        let ws = crate::Workspace::new(
            kmb_samples::reference_document(),
            None,
            kmb_bundle::modules(),
        );
        let mut h = egui_kittest::Harness::builder()
            .with_size(Vec2::new(1600.0, 900.0))
            .build_ui_state(|ui, ws: &mut crate::Workspace| ws.ui(ui), ws);
        let result = crate::cmdline_tests::run(
            &mut h,
            include_str!("../../../apps/kmb-app/tests/ui/操作の表示.jcs"),
        );
        assert!(result.ok, "{}", result.report());
    }
}
