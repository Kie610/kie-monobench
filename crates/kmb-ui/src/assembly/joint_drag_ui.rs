//! 3D のビューで部品を左ドラッグして動かす(部品を引いて動かす操作)。動かせる物は
//! モジュールが答え([`kmb_core::Extension::drag`])、画面は引いた量を形にする:
//! - ジョイントでつないだ部品は、その自由度の中で動かす。引いた量を欄の値(回転は角度、スライダは
//!   距離)にしてジョイントを直す。動きが 2 つ以上のジョイント(円柱・ピンスロット・平面・ボール)は、
//!   押した点がカーソルに付いてくるよう、動き全部の欄を直す(平面は回さない)。
//! - ジョイントの無い部品は、画面に平行に動かす。引いた量でモジュールの操作を実行する(組立は
//!   タイムラインに「位置」の機能を足す)。
//!
//! 回転とスライダ(欄 1 つ)は、引いている間は形を作り直さず 3D のビューのボディだけを動かし
//! (ジョイントの動きを見るのと同じ [`kmb_interchange::animation::joint_poses`])、離した時に 1 回だけ
//! 欄を直す(10-23 a1: 大きな組立でも引く間がもたつかない)。動きを見せるのは、直した形の計算が
//! 終わるまで続ける(離した瞬間に元の位置へ戻って見えない)。ほかの引き方(動き全部・自由)は、
//! 引いている間のコマを押す前の取り消しの長さまで戻してから入れ直す。どちらも 1 回引くと 1 つの
//! 取り消し単位になる。離した後は、ビューの上に「位置を残す/元に戻す」の帯を出す。残すか戻すかを決めずに
//! 別のコマンドを始めると、先に窓で聞く。
//!
//! 動かせない物(部品に入っていないボディ、固定のジョイント)の左ドラッグは範囲で選ぶ。値を式
//! (パラメータの名前など)で決めているジョイントは、式を数で上書きしないよう引かない。
//! 戻る位置を決めたジョイントは、離すとその値に戻す(押した時と違う値に戻った
//! 時だけ帯を出す)。

use crate::Workspace;
use egui::{Pos2, Rect, Vec2};
use kmb_core::{Drag, DragMotion, kernel as k};
use kmb_ui_view3d::view3d;

mod moved;

/// 回転のジョイントの回し方(押した時に決め、離すまで変えない)。
#[derive(Clone, Copy, Debug)]
enum Turn {
    /// 画面の上で、軸の点 `pivot` のまわりにカーソルが回った角度だけ回す。`sign` は部品の回る向きと
    /// 画面の回る向きを合わせる(軸がこちらを向いているか、奥を向いているか)
    Around { pivot: Pos2, sign: f64, last: Pos2 },
    /// 画面の向き `per_rad`(1 ラジアン回した時に、円のいちばん手前の点が画面で動く量)へ引いた
    /// 長さで回す。軸を横から見ていて、円が画面で細く潰れている時
    Along { per_rad: Vec2 },
}

/// 引いている途中の様子。
pub(crate) struct JointDrag {
    drag: Drag,
    /// 欄の単位(`deg` か `mm`)
    unit: String,
    /// 下限・上限(欄が空なら `None`)
    lo: Option<f64>,
    hi: Option<f64>,
    /// 離した時に戻す値(戻る位置の欄が空なら `None`)
    rest: Option<f64>,
    /// 押した時の値
    start: f64,
    /// 押した面の上の点(世界座標)
    grab: [f64; 3],
    /// 押した所(画面)
    press: Pos2,
    /// 押す前の取り消しの長さ
    depth: usize,
    /// 回転の回し方(回転でなければ `None`)
    turn: Option<Turn>,
    /// 回した量の合計(度。半周を越えても続くよう、コマごとの差を足す)
    turned: f64,
    /// 今の値
    value: f64,
    /// 自由に動かす時の、押した所を通り画面に平行な面の向き(視線)と、動かした量(世界座標の mm)
    normal: [f64; 3],
    offset: [f64; 3],
    /// 動き全部で付いてくる時([`DragMotion::Follow`])の様子
    follow: Option<Follow>,
}

/// 動き全部でカーソルに付いてくる引き方の様子。欄は [`Drag::dofs`] の順。
struct Follow {
    fields: Vec<FollowField>,
    /// 今の値(回転はラジアン、移動は mm)
    values: Vec<f64>,
    /// 押した点の、値が全部 0 の時の位置
    at: [f64; 3],
}

/// 動き全部で引く時の欄 1 つ。値は欄の単位(度か mm)。
struct FollowField {
    key: String,
    label: String,
    unit: String,
    lo: Option<f64>,
    hi: Option<f64>,
    rest: Option<f64>,
    start: f64,
    /// 引いて変えるか(止める欄と、式で決めた欄は変えない)
    moves: bool,
}

impl FollowField {
    /// 欄の単位の値と内部の値(回転はラジアン)の換算。
    fn inner(&self, v: f64) -> f64 {
        if self.unit == "deg" {
            v.to_radians()
        } else {
            v
        }
    }
    fn shown(&self, v: f64) -> f64 {
        if self.unit == "deg" {
            v.to_degrees()
        } else {
            v
        }
    }
}

impl Follow {
    /// 欄ごとの、欄の単位の今の値(範囲の無い回転は -180〜180 度に収める)。
    fn shown(&self) -> Vec<f64> {
        self.fields
            .iter()
            .zip(&self.values)
            .map(|(f, v)| {
                let v = f.shown(*v);
                if f.unit == "deg" && f.lo.is_none() && f.hi.is_none() {
                    wrap(v)
                } else {
                    v
                }
            })
            .collect()
    }

    /// 札の文(変える欄だけ。「角度 30 deg  距離 5 mm」)。
    fn text(&self) -> String {
        self.fields
            .iter()
            .zip(self.shown())
            .filter(|(f, _)| f.moves)
            .map(|(f, v)| format!("{} {}", f.label, value_text(v, &f.unit)))
            .collect::<Vec<_>>()
            .join("  ")
    }
}

/// 引いて動かした後、残すか戻すかを決めていない変更(ビューの上の帯)。
pub(crate) struct Moved {
    /// 最初に引く前の取り消しの長さ(戻す所)
    depth: usize,
    /// 最後に引いた後の取り消しの長さ(これと違えば、ほかの変更をしたので残したとみなす)
    top: usize,
    /// 何を動かしたか(最後に引いた物の説明)
    label: String,
}

/// 残すか戻すかを聞いている間、止めているコマンド。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Held {
    Action(String),
    ActionWith(String, serde_json::Value),
    Add(String),
    /// 機能を編集パネルで直し始める(タイムライン・ブラウザの二度押し、右クリックの「編集」)
    Edit(kmb_core::FeatureId),
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    k::sub(a, b)
}

fn f64s(p: [f32; 3]) -> [f64; 3] {
    p.map(f64::from)
}

/// `v` から `axis` 向きの成分を除いた物。
fn flat(v: [f64; 3], axis: [f64; 3]) -> [f64; 3] {
    sub(v, k::scale(axis, k::dot(v, axis)))
}

/// -180〜180 度に収める。
fn wrap(d: f64) -> f64 {
    let w = (d + 180.0).rem_euclid(360.0) - 180.0;
    if w <= -180.0 { w + 360.0 } else { w }
}

/// 画面の点 `c` のまわりに、点 `a` から点 `b` へ回った角度(度。-180〜180。画面の y は下向きなので、
/// 見た目の時計回りが正)。
fn screen_turn(c: Pos2, a: Pos2, b: Pos2) -> f64 {
    let (u, v) = (a - c, b - c);
    f64::from(u.x * v.y - u.y * v.x)
        .atan2(f64::from(u.x * v.x + u.y * v.y))
        .to_degrees()
}

/// 欄の式が「数 単位」だけか(式やパラメータの名前なら、引いて上書きしない)。
fn plain_number(text: &str, unit: &str) -> bool {
    let t = text.trim();
    let t = t.strip_suffix(unit).unwrap_or(t).trim();
    let t = t.strip_suffix('°').unwrap_or(t).trim();
    t.is_empty() || t.parse::<f64>().is_ok()
}

/// 値を欄の式にする(1 桁に丸め、整数なら小数を書かない。スライダと同じ書き方)。
fn value_text(v: f64, unit: &str) -> String {
    let v = (v * 10.0).round() / 10.0 + 0.0;
    if v.fract() == 0.0 {
        format!("{v:.0} {unit}")
    } else {
        format!("{v:.1} {unit}")
    }
}

/// 自由に動かした量の札(X・Y・Z を 0.1 mm で)。
fn offset_text(o: [f64; 3]) -> String {
    let [x, y, z] = o.map(|v| value_text(v, "").trim().to_string());
    format!("X {x}  Y {y}  Z {z} mm")
}

impl Workspace {
    /// 左ドラッグで部品を動かせる時か(設計の作業空間で、作る・直す・測るの途中でない)。
    fn joint_drag_allowed(&self) -> bool {
        self.win.layout.space == crate::Space::Design
            && self.cur.gesture.is_none()
            && self.cur.editing.is_none()
            && !self.is_measuring()
            && self.active_pick_field().is_none()
            && self.win.left_drag == view3d::LeftDrag::Select
            && self.cur.moved_ask.is_none()
    }

    /// 面 `pick` を引いて動かせる物。
    fn joint_drag_for(&self, pick: &view3d::Pick) -> Option<Drag> {
        let cx = self.context();
        self.win.extensions.drag(&cx, pick)
    }

    /// 引いている間は範囲で選ばない(ビューを描く前に聞く)。
    pub(crate) fn joint_dragging(&self) -> bool {
        self.cur.joint_drag.is_some()
    }

    /// ビューを描いた後に呼ぶ。左ドラッグを始めた面が動かせる部品なら引いて動かし、
    /// 動かせる部品の上ではカーソルを手の形にする。
    pub(crate) fn joint_drag_events(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect) {
        let pointer = ui.input(|i| i.pointer.clone());
        let mods = ui.input(|i| i.modifiers);
        if self.cur.joint_drag.is_none() {
            if !self.joint_drag_allowed() || mods.any() {
                return;
            }
            if resp.drag_started_by(egui::PointerButton::Primary)
                && let Some(press) = pointer.press_origin()
            {
                self.joint_drag_begin(ui, resp, rect, press);
            } else if resp.hovered()
                && !pointer.any_down()
                && let Some(p) = resp.hover_pos()
                && let Some(pick) = view3d::pick_at(&self.cur.scene, &self.cur.camera, rect, p)
                && self.joint_drag_for(&pick).is_some()
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            return;
        }
        if pointer.button_down(egui::PointerButton::Primary) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some(p) = pointer.latest_pos() {
                self.joint_drag_move(rect, p);
            }
            self.joint_drag_label(ui, rect);
        } else {
            self.joint_drag_end();
        }
    }

    fn joint_drag_begin(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, press: Pos2) {
        let Some((pick, at)) = view3d::picks_at(&self.cur.scene, &self.cur.camera, rect, press)
            .into_iter()
            .next()
        else {
            return;
        };
        let Some(drag) = self.joint_drag_for(&pick) else {
            return;
        };
        let grab = f64s(at);
        let started = if matches!(drag.motion, DragMotion::Free) {
            let (_, dir) = self.cur.camera.ray(press, rect);
            log::info!("{} を引き始める(画面に平行に)", drag.label);
            Some(self.free_drag(drag, grab, press, k::normalize(f64s(dir))))
        } else {
            self.field_drag(drag, grab, press, rect)
        };
        let Some(d) = started else {
            // 動かせない物の左ドラッグは範囲で選ぶ
            return;
        };
        // 範囲で選ぶ途中の四角を捨てる(この左ドラッグは部品を動かす)
        ui.ctx()
            .data_mut(|m| m.remove::<(Pos2, Pos2)>(resp.id.with("box")));
        self.cur.joint_drag = Some(d);
    }

    /// 自由に動かす引き方を始める(`normal` は押した所の視線)。
    pub(crate) fn free_drag(
        &self,
        drag: Drag,
        grab: [f64; 3],
        press: Pos2,
        normal: [f64; 3],
    ) -> JointDrag {
        JointDrag {
            drag,
            unit: "mm".into(),
            lo: None,
            hi: None,
            rest: None,
            start: 0.0,
            grab,
            press,
            depth: self.cur.work.doc().undo_depth(),
            turn: None,
            turned: 0.0,
            value: 0.0,
            normal,
            offset: [0.0; 3],
            follow: None,
        }
    }

    /// ジョイントの欄を直す引き方を始める(引けなければ `None`)。
    fn field_drag(
        &mut self,
        drag: Drag,
        grab: [f64; 3],
        press: Pos2,
        rect: Rect,
    ) -> Option<JointDrag> {
        if matches!(drag.motion, DragMotion::Follow) {
            return self.follow_drag(drag, grab, press);
        }
        let entry = self.cur.work.doc().data().feature(drag.feature).cloned()?;
        let field = self
            .win
            .registry
            .descriptor(&entry.type_id)
            .and_then(|d| d.fields.into_iter().find(|f| f.key == drag.field))?;
        let slider = field.slider.clone()?;
        let text_of = |key: &str| {
            entry
                .data
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let value_of = |t: &str| {
            self.eval_expr(t).ok().map(|q| {
                if slider.unit == "deg" && q.dim == kmb_core::Dim::ANGLE {
                    q.value.to_degrees()
                } else {
                    q.value
                }
            })
        };
        let cur = text_of(&drag.field);
        if !plain_number(&cur, &slider.unit) {
            log::info!("{} は式「{cur}」で決めているので引かない", drag.label);
            self.win.message = Some(format!(
                "{}: {} を式「{cur}」で決めているので、引いて動かせません(欄を数にすると引ける)",
                entry.name, field.label
            ));
            return None;
        }
        let read = |key: &str| {
            let t = text_of(key);
            (!t.trim().is_empty()).then(|| value_of(&t)).flatten()
        };
        let start = value_of(&cur).unwrap_or(0.0);
        let turn = match drag.motion {
            DragMotion::Rotate { origin, axis } => {
                let (_, dir) = self.cur.camera.ray(press, rect);
                Some(self.turn_for(origin, axis, grab, k::normalize(f64s(dir)), press, rect))
            }
            _ => None,
        };
        log::info!(
            "{} を引き始める(今 {start:.1} {}、下限 {:?}、上限 {:?}、回し方 {turn:?})",
            drag.label,
            slider.unit,
            read(&slider.min),
            read(&slider.max)
        );
        Some(JointDrag {
            unit: slider.unit.clone(),
            lo: read(&slider.min),
            hi: read(&slider.max),
            rest: read(&slider.rest),
            start,
            grab,
            press,
            depth: self.cur.work.doc().undo_depth(),
            turn,
            turned: 0.0,
            value: start,
            normal: [0.0; 3],
            offset: [0.0; 3],
            follow: None,
            drag,
        })
    }

    /// 動き全部でカーソルに付いてくる引き方を始める(変えられる欄が無ければ `None`)。
    fn follow_drag(&mut self, drag: Drag, grab: [f64; 3], press: Pos2) -> Option<JointDrag> {
        let entry = self.cur.work.doc().data().feature(drag.feature).cloned()?;
        let desc = self.win.registry.descriptor(&entry.type_id)?;
        let text_of = |key: &str| {
            entry
                .data
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let mut fields = Vec::new();
        let mut held = Vec::new();
        for dof in &drag.dofs {
            let field = desc.fields.iter().find(|f| f.key == dof.field)?;
            let slider = field.slider.clone()?;
            let value_of = |t: &str| {
                self.eval_expr(t).ok().map(|q| {
                    if slider.unit == "deg" && q.dim == kmb_core::Dim::ANGLE {
                        q.value.to_degrees()
                    } else {
                        q.value
                    }
                })
            };
            let read = |key: &str| {
                let t = text_of(key);
                (!t.trim().is_empty()).then(|| value_of(&t)).flatten()
            };
            let cur = text_of(&dof.field);
            let plain = plain_number(&cur, &slider.unit);
            if !plain && !drag.still.contains(&dof.field) {
                held.push(format!("{}「{cur}」", field.label));
            }
            fields.push(FollowField {
                key: dof.field.clone(),
                label: field.label.clone(),
                unit: slider.unit.clone(),
                lo: read(&slider.min),
                hi: read(&slider.max),
                rest: read(&slider.rest),
                start: value_of(&cur).unwrap_or(0.0),
                moves: plain && !drag.still.contains(&dof.field),
            });
        }
        if !fields.iter().any(|f| f.moves) {
            log::info!("{} は動きを全部式で決めているので引かない", drag.label);
            self.win.message = Some(format!(
                "{}: {} を式で決めているので、引いて動かせません(欄を数にすると引ける)",
                entry.name,
                held.join("・")
            ));
            return None;
        }
        if !held.is_empty() {
            log::info!(
                "{} の {} は式なので変えずに引く",
                drag.label,
                held.join("・")
            );
        }
        let values: Vec<f64> = fields.iter().map(|f| f.inner(f.start)).collect();
        let at = kmb_interchange::animation::dofs_unpose(&drag.dofs, &values, grab);
        log::info!(
            "{} を引き始める(動き全部で付いてくる。{})",
            drag.label,
            fields
                .iter()
                .map(|f| format!(
                    "{} {:.1} {}{}",
                    f.key,
                    f.start,
                    f.unit,
                    if f.moves { "" } else { " 止める" }
                ))
                .collect::<Vec<_>>()
                .join("、")
        );
        let unit = fields[0].unit.clone();
        Some(JointDrag {
            unit,
            lo: None,
            hi: None,
            rest: None,
            start: 0.0,
            grab,
            press,
            depth: self.cur.work.doc().undo_depth(),
            turn: None,
            turned: 0.0,
            value: 0.0,
            normal: [0.0; 3],
            offset: [0.0; 3],
            follow: Some(Follow { fields, values, at }),
            drag,
        })
    }

    /// 動き全部で引く時: カーソルの視線 `(o, dir)` に押した点が近づく値にする。変われば `true`。
    fn follow_to(d: &mut JointDrag, o: [f64; 3], dir: [f64; 3]) -> bool {
        let Some(f) = d.follow.as_mut() else {
            return false;
        };
        let before: Vec<String> = f
            .fields
            .iter()
            .zip(f.shown())
            .map(|(x, v)| value_text(v, &x.unit))
            .collect();
        let moves: Vec<bool> = f.fields.iter().map(|x| x.moves).collect();
        let limits: Vec<(Option<f64>, Option<f64>)> = f
            .fields
            .iter()
            .map(|x| (x.lo.map(|v| x.inner(v)), x.hi.map(|v| x.inner(v))))
            .collect();
        f.values = kmb_interchange::animation::follow(
            &d.drag.dofs,
            &f.values,
            &moves,
            &limits,
            f.at,
            o,
            dir,
        );
        let after: Vec<String> = f
            .fields
            .iter()
            .zip(f.shown())
            .map(|(x, v)| value_text(v, &x.unit))
            .collect();
        before != after
    }

    /// 回転の回し方を決める。軸がこちらか奥を向いていれば(視線との角が 70 度より小さい)、画面の
    /// 上で軸の点のまわりにカーソルを回した角度で回す。円が画面で細く潰れている時(軸を横から見て
    /// いる)と、軸のすぐ近くを押した時は、円のいちばん手前の点が動く向きへ引いた長さで回す。
    fn turn_for(
        &self,
        origin: [f64; 3],
        axis: [f64; 3],
        grab: [f64; 3],
        dir: [f64; 3],
        press: Pos2,
        rect: Rect,
    ) -> Turn {
        let cam = self.cur.camera;
        let project = |p: [f64; 3]| cam.project(p.map(|c| c as f32), rect).0;
        let v0 = flat(sub(grab, origin), axis);
        let r = k::dot(v0, v0).sqrt().max(1.0);
        let pivot3 = k::add(origin, k::scale(axis, k::dot(sub(grab, origin), axis)));
        let pivot = project(pivot3);
        let facing = k::dot(dir, axis);
        if facing.abs() >= 0.35 && (press - pivot).length() >= 12.0 {
            // 押した点を少し回した時に、画面で回る向き
            let turned = k::transform_point(&k::rotation(origin, axis, 5f64.to_radians()), grab);
            let s = screen_turn(pivot, project(grab), project(turned));
            if s.abs() > 0.5 {
                return Turn::Around {
                    pivot,
                    sign: s.signum(),
                    last: press,
                };
            }
        }
        // 円のいちばん手前の点(視線の逆向き)と、そこが正に回る向き
        let toward = flat(k::scale(dir, -1.0), axis);
        let front = if k::dot(toward, toward) > 1e-12 {
            k::normalize(toward)
        } else if k::dot(v0, v0) > 1e-12 {
            k::normalize(v0)
        } else {
            k::normalize(flat([1.0, 0.0, 0.0], axis))
        };
        let front_at = k::add(pivot3, k::scale(front, r));
        let tangent = k::cross(axis, front);
        let mut per_rad =
            (project(k::add(front_at, k::scale(tangent, r * 0.01))) - project(front_at)) * 100.0;
        // 押した点が円の奥の側なら、手前の点と逆に動く: 押した点がカーソルに付いてくる向きにする
        let mine = project(k::add(grab, k::scale(k::cross(axis, v0), 0.01))) - project(grab);
        if mine.dot(per_rad) < 0.0 {
            per_rad = -per_rad;
        }
        Turn::Along { per_rad }
    }

    /// 画面の点 `pos` まで引いた時の値を出し、変わっていれば形を直す。
    fn joint_drag_move(&mut self, rect: Rect, pos: Pos2) {
        let cam = self.cur.camera;
        let Some(d) = self.cur.joint_drag.as_mut() else {
            return;
        };
        let project = |p: [f64; 3]| cam.project(p.map(|c| c as f32), rect).0;
        let moved = pos - d.press;
        if matches!(d.drag.motion, DragMotion::Free) {
            // 視線と、押した点を通り画面に平行な面の交わる所まで動かす
            let (o, dir) = cam.ray(pos, rect);
            let (o, dir) = (f64s(o), f64s(dir));
            let facing = k::dot(dir, d.normal);
            if facing.abs() < 1e-6 {
                return;
            }
            let t = k::dot(sub(d.grab, o), d.normal) / facing;
            let offset = sub(k::add(o, k::scale(dir, t)), d.grab);
            if offset_text(offset) == offset_text(d.offset) {
                return;
            }
            d.offset = offset;
            self.joint_drag_apply();
            return;
        }
        if d.follow.is_some() {
            let (o, dir) = cam.ray(pos, rect);
            if Self::follow_to(d, f64s(o), k::normalize(f64s(dir))) {
                self.joint_drag_apply();
            }
            return;
        }
        let raw = match (d.drag.motion, d.turn) {
            (DragMotion::Slide { axis }, _) => {
                // 軸の 1 mm が画面で何画素か。軸を真上から見ていれば動かせない
                let u = project(k::add(d.grab, axis)) - project(d.grab);
                if u.length_sq() < 0.25 {
                    return;
                }
                d.start + f64::from(moved.dot(u) / u.length_sq())
            }
            (_, Some(Turn::Around { pivot, sign, last })) => {
                // 軸の点のすぐ近くでは角度が暴れるので、離れるまで足さない
                if (pos - pivot).length() < 4.0 {
                    return;
                }
                d.turned += sign * screen_turn(pivot, last, pos);
                d.turn = Some(Turn::Around {
                    pivot,
                    sign,
                    last: pos,
                });
                d.start + d.turned
            }
            (_, Some(Turn::Along { per_rad })) => {
                if per_rad.length_sq() < 0.25 {
                    return;
                }
                d.turned = f64::from(moved.dot(per_rad) / per_rad.length_sq()).to_degrees();
                d.start + d.turned
            }
            _ => return,
        };
        let mut v = raw;
        if let Some(lo) = d.lo {
            v = v.max(lo);
        }
        if let Some(hi) = d.hi {
            v = v.min(hi);
        }
        // 範囲の無い回転は -180〜180 度に収める
        if d.unit == "deg" && d.lo.is_none() && d.hi.is_none() {
            v = wrap(v);
        }
        if value_text(v, &d.unit) == value_text(d.value, &d.unit) {
            return;
        }
        // 接触セットの相手に食い込む手前で止める(モジュールが答える)
        let (drag, from) = (d.drag.clone(), d.value);
        if let Some(stop) = self
            .win
            .extensions
            .drag_limit(&self.context(), &drag, from, v)
        {
            v = stop;
        }
        let Some(d) = self.cur.joint_drag.as_mut() else {
            return;
        };
        if value_text(v, &d.unit) == value_text(d.value, &d.unit) {
            return;
        }
        d.value = v;
        self.joint_drag_show();
    }

    /// 引いている値を、形を作り直さずに 3D のビューの動きだけで見せる(回転・スライダ)。
    pub(crate) fn joint_drag_show(&mut self) {
        let Some(d) = self.cur.joint_drag.as_ref() else {
            return;
        };
        log::debug!("{}: {}(見た目だけ)", d.drag.label, value_text(d.value, &d.unit));
        let poses = self.preview_poses(d.drag.feature, d.value);
        self.cur.scene.set_poses(poses.clone());
        self.cur.drag_pose = Some(poses);
    }

    /// 引いて見せている動きを、毎フレーム出し直す。離した後は、直した形の計算が終わったら外す。
    pub(crate) fn sync_drag_pose(&mut self) {
        let Some(p) = &self.cur.drag_pose else {
            return;
        };
        if self.cur.joint_drag.is_none() && self.progress().is_none() {
            self.cur.drag_pose = None;
            self.cur.scene.set_poses(Default::default());
            return;
        }
        // 形を作り直すとビューの網が入れ替わるので、毎フレーム掛け直す(同じなら何もしない)
        let p = p.clone();
        self.cur.scene.set_poses(p);
    }

    /// 引いている量で形を直す。前のコマの変更を戻してから入れる(離した時に 1 つの取り消し単位で残る)。
    pub(crate) fn joint_drag_apply(&mut self) {
        let Some(d) = self.cur.joint_drag.as_ref() else {
            return;
        };
        let (id, key, depth, label) = (
            d.drag.feature,
            d.drag.field.clone(),
            d.depth,
            d.drag.label.clone(),
        );
        if matches!(d.drag.motion, DragMotion::Free) {
            let args = serde_json::json!({"body": id.0, "offset": d.offset});
            log::debug!("{label}: {}", offset_text(d.offset));
            self.cur.work.rollback_to(depth);
            self.supersede_job();
            match self
                .run_through(&key, self.cur.selected, None, &args)
                .map(|ran| ran.handled)
            {
                Ok(kmb_session::Handled::Changed(_)) => {
                    self.after_change(Ok(()));
                }
                Ok(_) => {}
                Err(e) => self.win.fail(e),
            }
            return;
        }
        let Some(mut data) = self
            .cur
            .work
            .doc()
            .data()
            .feature(id)
            .map(|e| e.data.clone())
        else {
            return;
        };
        let text = if let Some(f) = &d.follow {
            for (x, v) in f.fields.iter().zip(f.shown()) {
                if x.moves {
                    data[x.key.as_str()] = serde_json::Value::String(value_text(v, &x.unit));
                }
            }
            f.text()
        } else {
            let text = value_text(d.value, &d.unit);
            data[key.as_str()] = serde_json::Value::String(text.clone());
            text
        };
        log::debug!("{label}: {text}");
        self.cur.work.rollback_to(depth);
        self.supersede_job();
        let mut tx = kmb_session::edit::set_feature_data(self.cur.work.doc().data(), id, data);
        tx.label = label;
        self.apply(tx);
    }

    /// 離した時。動かしていれば、残すか戻すかを決めていない変更として覚える(前に引いた物があれば、
    /// 戻す所はそのまま)。
    pub(crate) fn joint_drag_end(&mut self) {
        let Some(mut d) = self.cur.joint_drag.take() else {
            return;
        };
        if let Some(r) = d.rest {
            self.joint_drag_rest(&mut d, r);
        } else if self.cur.drag_pose.is_some()
            && value_text(d.value, &d.unit) != value_text(d.start, &d.unit)
        {
            // 見た目だけ動かしていた値で、1 回だけ形を作り直す
            self.cur.joint_drag = Some(d);
            self.joint_drag_apply();
            d = self.cur.joint_drag.take().expect("引いている");
        }
        if d.follow
            .as_ref()
            .is_some_and(|f| f.fields.iter().any(|x| x.moves && x.rest.is_some()))
        {
            self.follow_rest(&mut d);
        }
        let text = if matches!(d.drag.motion, DragMotion::Free) {
            offset_text(d.offset)
        } else if let Some(f) = &d.follow {
            f.text()
        } else {
            value_text(d.value, &d.unit)
        };
        let changed = self.cur.work.doc().undo_depth() > d.depth;
        log::info!(
            "{} を {text} にした(押した時は {:.1}、変更 {})",
            d.drag.label,
            d.start,
            if changed { "あり" } else { "なし" }
        );
        if !changed {
            return;
        }
        // 下の帯の文には出さない(ビューの上の帯に出す。引くたびに知らせを増やさない)
        self.moved_check();
        let depth = self.cur.moved.as_ref().map_or(d.depth, |m| m.depth);
        self.cur.moved = Some(Moved {
            depth,
            top: self.cur.work.doc().undo_depth(),
            label: format!("{}  {text}", d.drag.label),
        });
    }

    /// 離した部品を戻る位置 `rest` へ戻す(引いた分と合わせて 1 つの取り消し単位。押した時と同じ値なら
    /// 何も残さない)。
    fn joint_drag_rest(&mut self, d: &mut JointDrag, rest: f64) {
        let mut rest = rest;
        if let Some(lo) = d.lo {
            rest = rest.max(lo);
        }
        if let Some(hi) = d.hi {
            rest = rest.min(hi);
        }
        let text = value_text(rest, &d.unit);
        log::info!(
            "{} を離したので戻る位置 {text} に戻す(離した時は {:.1})",
            d.drag.label,
            d.value
        );
        self.cur.work.rollback_to(d.depth);
        self.supersede_job();
        if text == value_text(d.start, &d.unit) {
            self.recompute();
        } else if let Some(mut data) = self
            .cur
            .work
            .doc()
            .data()
            .feature(d.drag.feature)
            .map(|e| e.data.clone())
        {
            data[d.drag.field.as_str()] = serde_json::Value::String(text.clone());
            let mut tx = kmb_session::edit::set_feature_data(
                self.cur.work.doc().data(),
                d.drag.feature,
                data,
            );
            tx.label = d.drag.label.clone();
            self.apply(tx);
        }
        if (d.value - d.start).abs() > 1e-9 {
            self.win.message = Some(format!(
                "{}: 離したので戻る位置 {text} に戻しました",
                d.drag.label
            ));
        }
        d.value = rest;
    }

    /// 動き全部で引いた部品を離した時、戻る位置のある欄をその値へ戻す(引いた分と合わせて 1 つの
    /// 取り消し単位。全部の欄が押した時と同じ値なら何も残さない)。
    fn follow_rest(&mut self, d: &mut JointDrag) {
        let Some(f) = d.follow.as_mut() else { return };
        for (i, x) in f.fields.iter().enumerate() {
            if let (true, Some(r)) = (x.moves, x.rest) {
                let r = x.lo.map_or(r, |lo| r.max(lo));
                let r = x.hi.map_or(r, |hi| r.min(hi));
                f.values[i] = x.inner(r);
            }
        }
        let shown = f.shown();
        let same = f
            .fields
            .iter()
            .zip(&shown)
            .all(|(x, v)| value_text(*v, &x.unit) == value_text(x.start, &x.unit));
        let text = f.text();
        log::info!("{} を離したので戻る位置に戻す({text})", d.drag.label);
        self.cur.work.rollback_to(d.depth);
        self.supersede_job();
        if same {
            self.recompute();
        } else if let Some(mut data) = self
            .cur
            .work
            .doc()
            .data()
            .feature(d.drag.feature)
            .map(|e| e.data.clone())
        {
            for (x, v) in f.fields.iter().zip(&shown) {
                if x.moves {
                    data[x.key.as_str()] = serde_json::Value::String(value_text(*v, &x.unit));
                }
            }
            let mut tx = kmb_session::edit::set_feature_data(
                self.cur.work.doc().data(),
                d.drag.feature,
                data,
            );
            tx.label = d.drag.label.clone();
            self.apply(tx);
        }
        self.win.message = Some(format!(
            "{}: 離したので戻る位置 {text} に戻しました",
            d.drag.label
        ));
    }

    /// 試験用: ジョイントの引きを、値 `value` まで引いて離したことにする。
    #[cfg(test)]
    pub(crate) fn drag_joint_to(&mut self, drag: Drag, value: f64) {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut d = self
            .field_drag(drag, [0.0; 3], Pos2::new(400.0, 300.0), rect)
            .expect("引ける");
        d.value = value;
        self.cur.joint_drag = Some(d);
        self.joint_drag_apply();
        self.joint_drag_end();
    }

    /// 試験用: 動き全部で付いてくる引きを、面の上の点 `grab` を押して、視線 `(o, dir)` まで引いて
    /// 離したことにする(途中を `steps` コマに分けて、押した所から少しずつ寄せる)。
    #[cfg(test)]
    pub(crate) fn drag_follow_to(
        &mut self,
        drag: Drag,
        grab: [f64; 3],
        targets: &[([f64; 3], [f64; 3])],
    ) -> bool {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let Some(mut d) = self.field_drag(drag, grab, Pos2::new(400.0, 300.0), rect) else {
            return false;
        };
        for (o, dir) in targets {
            if Self::follow_to(&mut d, *o, *dir) {
                self.cur.joint_drag = Some(d);
                self.joint_drag_apply();
                d = self.cur.joint_drag.take().expect("引いている");
            }
        }
        self.cur.joint_drag = Some(d);
        self.joint_drag_end();
        true
    }

    /// 引いている間、カーソルの横に今の値を出す。
    fn joint_drag_label(&self, ui: &egui::Ui, rect: Rect) {
        let Some(d) = &self.cur.joint_drag else {
            return;
        };
        let Some(p) = ui.input(|i| i.pointer.latest_pos()) else {
            return;
        };
        let l = &self.win.look;
        let value = if matches!(d.drag.motion, DragMotion::Free) {
            offset_text(d.offset)
        } else if let Some(f) = &d.follow {
            f.text()
        } else {
            value_text(d.value, &d.unit)
        };
        let text = format!("{}  {value}", d.drag.label);
        // ビューの上のボタンより前に描く(札が隠れない)
        let painter = ui
            .ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("joint-drag-label"),
            ))
            .with_clip_rect(rect);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(13.0), l.pal.ink);
        let at = p + egui::vec2(16.0, 12.0);
        let bg = Rect::from_min_size(at, galley.size()).expand(4.0);
        painter.rect_filled(bg, crate::theme::corner(4.0), l.pal.panel);
        painter.rect_stroke(
            bg,
            crate::theme::corner(4.0),
            egui::Stroke::new(1.0, l.pal.edge),
            egui::StrokeKind::Outside,
        );
        painter.galley(at, galley, l.pal.ink);
    }
}

#[cfg(test)]
mod tests;
