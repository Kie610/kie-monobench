//! 描いている途中の寸法の欄。形の脇に長さ・直径などの値を出し、
//! 数字を打つと今の欄に入る。Tab で次の欄へ移ると、打った値はその欄に固定され、
//! マウスを動かしても変わらない(固定していない値はマウスで決まる)。クリックか Enter で形を作り、
//! 打った値は寸法の拘束として付ける。

use crate::view::Tool;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};

/// 欄の値の種類。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Unit {
    Mm,
    Deg,
    Count,
}

/// 寸法の欄 1 つ。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Field {
    pub label: &'static str,
    pub unit: Unit,
    /// 打っている値(空ならマウスの位置で測った値を出す)
    pub text: String,
    /// Tab か Enter で固定した
    pub locked: bool,
}

impl Field {
    /// 固定した値か、打っている途中の値(寸法の式)。
    pub fn expr(&self) -> Option<String> {
        let t = self.text.trim();
        if t.is_empty() {
            return None;
        }
        Some(match self.unit {
            Unit::Mm => crate::view::typed_value(t),
            Unit::Deg => {
                let n = t.strip_suffix("deg").unwrap_or(t).trim();
                match n.parse::<f64>() {
                    Ok(_) => format!("{n} deg"),
                    Err(_) => t.to_string(),
                }
            }
            Unit::Count => t.to_string(),
        })
    }

    /// 数として読める値(mm・度・個数)。式やパラメータは `None`(形はマウスで決め、寸法で合わせる)。
    pub fn number(&self) -> Option<f64> {
        let e = self.expr()?;
        let n = match self.unit {
            Unit::Mm => crate::view::length_mm(&e),
            Unit::Deg => e.strip_suffix(" deg")?.trim().parse::<f64>().ok(),
            Unit::Count => e.parse::<f64>().ok(),
        }?;
        n.is_finite().then_some(n)
    }

    /// 形を決めるのに使う値: 固定した値か、打っている途中の値。
    pub fn fixed(&self) -> Option<f64> {
        if self.text.trim().is_empty() {
            None
        } else {
            self.number()
        }
    }
}

/// 道具と段ごとの欄(名前と種類)。欄のない道具・段は `None`。
pub(crate) fn spec(tool: Tool, step: usize) -> Option<&'static [(&'static str, Unit)]> {
    Some(match (tool, step) {
        (Tool::Line, s) if s >= 1 => &[("長さ", Unit::Mm), ("角度", Unit::Deg)],
        (Tool::Circle | Tool::TwoPointCircle, 1) => &[("直径", Unit::Mm)],
        (Tool::Rectangle | Tool::CenterRectangle, 1) => &[("幅", Unit::Mm), ("高さ", Unit::Mm)],
        (Tool::RoundedRectangle, 1) => {
            &[("幅", Unit::Mm), ("高さ", Unit::Mm), ("角の半径", Unit::Mm)]
        }
        (Tool::Arc, 1) => &[("半径", Unit::Mm)],
        (Tool::Arc, 2) => &[("角度", Unit::Deg)],
        (Tool::Slot, 1) => &[("長さ", Unit::Mm)],
        (Tool::Slot, 2) => &[("幅", Unit::Mm)],
        (Tool::Polygon, 1) => &[("辺の数", Unit::Count), ("半径", Unit::Mm)],
        (Tool::Offset, s) if s >= 1 => &[("距離", Unit::Mm)],
        _ => return None,
    })
}

/// 描いている途中の欄の並び。
#[derive(Clone, Debug, Default)]
pub(crate) struct Hud {
    /// どの道具の何段目の欄か(変わったら作り直す)
    pub key: Option<(Tool, usize)>,
    pub fields: Vec<Field>,
    /// 今打つ欄
    pub active: usize,
    /// 次に打つ文字で欄を打ち直す(固定した欄へ Tab で戻った時)
    pub fresh: bool,
    /// 前の段で固定した値(円弧の半径、長穴の長さ)。形を作る時に寸法にする
    pub carried: Vec<(&'static str, String)>,
}

/// キーの結果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Key {
    None,
    /// Enter: 固定した値で形を作る
    Enter,
    /// 何も打たずに Enter(つなげて描いている線を終える)
    EnterEmpty,
}

impl Hud {
    /// 道具と段に合わせて欄を用意する。段が進んだら、固定した値を `carried` に移す。
    pub fn sync(&mut self, tool: Tool, step: usize) {
        let key = Some((tool, step));
        if self.key == key {
            return;
        }
        match self.key {
            Some((t, s)) if t == tool && s < step && tool != Tool::Line => {
                // 固定したか、打ってからクリックした値を寸法として持ち越す
                for f in &self.fields {
                    if let Some(e) = f.expr() {
                        self.carried.push((f.label, e));
                    }
                }
            }
            _ => self.carried.clear(),
        }
        self.key = key;
        self.active = 0;
        self.fresh = false;
        self.fields = spec(tool, step)
            .unwrap_or(&[])
            .iter()
            .map(|(label, unit)| Field {
                label,
                unit: *unit,
                text: String::new(),
                locked: false,
            })
            .collect();
    }

    /// 欄を消す(描きかけを捨てた・形を作った)。
    pub fn clear(&mut self) {
        *self = Hud::default();
    }

    /// 値を打った欄がある(形の位置を寸法で決める。吸着はしない)。
    pub fn pinned(&self) -> bool {
        self.fields.iter().any(|f| !f.text.trim().is_empty())
    }

    /// 打った値も固定もない。
    pub fn is_blank(&self) -> bool {
        self.fields.iter().all(|f| f.text.is_empty() && !f.locked)
    }

    /// 打った値と固定を消す(Esc の 1 段目)。
    pub fn reset_values(&mut self) {
        for f in &mut self.fields {
            f.text.clear();
            f.locked = false;
        }
        self.active = 0;
        self.fresh = false;
    }

    pub fn field(&self, label: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.label == label)
    }

    /// 形を決める値(固定した値か、打っている途中の値)。
    pub fn fixed(&self, label: &str) -> Option<f64> {
        self.field(label).and_then(Field::fixed)
    }

    /// 寸法にする式(固定したか打った値。前の段で固定した値も含む)。
    pub fn dim(&self, label: &str) -> Option<String> {
        self.field(label).and_then(Field::expr).or_else(|| {
            self.carried
                .iter()
                .find(|(l, _)| *l == label)
                .map(|(_, e)| e.clone())
        })
    }

    /// 今の欄の打ち込みを固定する。「30, 20」のように区切って打った時は、続く欄へ分ける。
    fn lock_active(&mut self) {
        let Some(f) = self.fields.get_mut(self.active) else {
            return;
        };
        let text = f.text.clone();
        let parts: Vec<&str> = text.split([',', '、', '×']).map(str::trim).collect();
        if parts.len() > 1 {
            for (k, p) in parts.into_iter().enumerate() {
                if let Some(f) = self.fields.get_mut(self.active + k) {
                    f.text = p.to_string();
                    f.locked = !p.is_empty();
                }
            }
        } else if !f.text.trim().is_empty() {
            f.locked = true;
        }
    }

    /// 打った文字・Backspace・Tab・Enter を受ける。
    pub fn keys(&mut self, events: &[egui::Event]) -> Key {
        let mut out = Key::None;
        let n = self.fields.len().max(1);
        for e in events {
            match e {
                egui::Event::Text(t) => {
                    let Some(f) = self.fields.get_mut(self.active) else {
                        continue;
                    };
                    if std::mem::take(&mut self.fresh) {
                        f.text.clear();
                        f.locked = false;
                    }
                    f.text.push_str(t);
                }
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    pressed: true,
                    ..
                } => {
                    self.fresh = false;
                    if let Some(f) = self.fields.get_mut(self.active) {
                        f.text.pop();
                        if f.text.is_empty() {
                            f.locked = false;
                        }
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Tab,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    self.lock_active();
                    self.active = if modifiers.shift {
                        (self.active + n - 1) % n
                    } else {
                        (self.active + 1) % n
                    };
                    // 固定した欄へ戻ったら、打つと打ち直しになる
                    self.fresh = self.fields.get(self.active).is_some_and(|f| f.locked);
                }
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => {
                    self.lock_active();
                    out = if self.fields.iter().any(|f| f.locked) {
                        Key::Enter
                    } else {
                        Key::EnterEmpty
                    };
                }
                _ => {}
            }
        }
        out
    }
}

/// 欄の札を描く。`live` はマウスの位置で測った値(固定していない欄に出す)。札の場所を返す。
pub(crate) fn draw_field(
    painter: &egui::Painter,
    at: Pos2,
    f: &Field,
    live: Option<f64>,
    active: bool,
    look: &kmb_ui::theme::Look,
    accent: Color32,
) -> Rect {
    let shown = if f.text.is_empty() {
        live.map_or_else(|| "—".to_string(), |v| fmt(v, f.unit))
    } else {
        f.text.clone()
    };
    // 打つ位置の縦棒(フォントに無い字を使わず線で描く)
    let caret = if active { 4.0 } else { 0.0 };
    let ink = if f.locked || !f.text.is_empty() {
        look.pal.ink
    } else {
        look.pal.ink2
    };
    let g = painter.layout_no_wrap(shown, kmb_ui::theme::prop(12.5), ink);
    let lock_w = if f.locked { 14.0 } else { 0.0 };
    let size = g.size() + Vec2::new(14.0 + lock_w + caret, 6.0);
    let r = Rect::from_center_size(at, size);
    let (fill, stroke) = if active {
        (look.pal.paper, Stroke::new(1.6, accent))
    } else {
        (look.pal.panel, Stroke::new(1.0, look.pal.edge))
    };
    painter.rect(
        r,
        kmb_ui::theme::corner(3.0),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    if f.locked {
        kmb_ui::theme::paint_icon(
            painter,
            Rect::from_center_size(Pos2::new(r.left() + 10.0, r.center().y), Vec2::splat(12.0)),
            "lock",
            accent,
        );
    }
    let text_at = Pos2::new(r.left() + 7.0 + lock_w, r.top() + 3.0);
    if active {
        let x = text_at.x + g.size().x + 2.0;
        painter.line_segment(
            [Pos2::new(x, r.top() + 4.0), Pos2::new(x, r.bottom() - 4.0)],
            Stroke::new(1.2, accent),
        );
    }
    painter.galley(text_at, g, ink);
    r
}

/// 欄に出す値。
pub(crate) fn fmt(v: f64, unit: Unit) -> String {
    let trim = |s: String| {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    };
    match unit {
        // 長さは文書の単位で(インチは 3 桁・フィートは 4 桁。打った値と同じ単位で読み直す)
        Unit::Mm => {
            let u = kmb_core::display_unit();
            let digits = u.digits();
            trim(format!("{:.digits$}", u.from_mm(v)))
        }
        Unit::Deg => format!("{}°", trim(format!("{v:.1}"))),
        Unit::Count => format!("{}", v.round() as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: egui::Key, shift: bool) -> egui::Event {
        egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: if shift {
                egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::NONE
            },
        }
    }

    #[test]
    fn tab_locks_the_typed_value_and_moves_on() {
        let mut h = Hud::default();
        h.sync(Tool::Rectangle, 1);
        h.keys(&[egui::Event::Text("30".into()), key(egui::Key::Tab, false)]);
        assert!(h.fields[0].locked);
        assert_eq!(h.active, 1);
        assert_eq!(h.fixed("幅"), Some(30.0));
        assert_eq!(h.fixed("高さ"), None);
        // 打たずに Tab: 固定しない
        h.keys(&[key(egui::Key::Tab, false)]);
        assert!(!h.fields[1].locked);
        // 固定した欄へ戻って打つと打ち直し
        assert_eq!(h.active, 0);
        assert!(h.fresh);
        h.keys(&[egui::Event::Text("4".into())]);
        assert_eq!(h.fields[0].text, "4");
        assert_eq!(
            h.keys(&[key(egui::Key::Enter, false)]),
            Key::Enter,
            "固定した値があれば Enter で作る"
        );
        assert_eq!(h.dim("幅").as_deref(), Some("4 mm"));
    }

    #[test]
    fn comma_splits_into_the_following_fields_and_angles_take_deg() {
        let mut h = Hud::default();
        h.sync(Tool::Rectangle, 1);
        assert_eq!(
            h.keys(&[
                egui::Event::Text("30, 20".into()),
                key(egui::Key::Enter, false)
            ]),
            Key::Enter
        );
        assert_eq!((h.fixed("幅"), h.fixed("高さ")), (Some(30.0), Some(20.0)));
        h.sync(Tool::Line, 1);
        h.keys(&[key(egui::Key::Tab, true), egui::Event::Text("45".into())]);
        assert_eq!(h.active, 1);
        assert_eq!(h.dim("角度").as_deref(), Some("45 deg"));
        assert_eq!(h.keys(&[key(egui::Key::Backspace, false)]), Key::None);
        assert_eq!(h.fields[1].text, "4");
    }

    #[test]
    fn values_fixed_in_an_earlier_step_are_carried() {
        let mut h = Hud::default();
        h.sync(Tool::Arc, 1);
        h.keys(&[egui::Event::Text("12".into()), key(egui::Key::Tab, false)]);
        h.sync(Tool::Arc, 2);
        assert_eq!(h.dim("半径").as_deref(), Some("12 mm"));
        assert!(h.is_blank());
        h.sync(Tool::Circle, 1);
        assert_eq!(h.dim("半径"), None);
        assert_eq!(
            h.keys(&[key(egui::Key::Enter, false)]),
            Key::EnterEmpty,
            "何も打たない Enter"
        );
    }
}
