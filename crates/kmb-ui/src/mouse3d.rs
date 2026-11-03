//! 3D マウスの値から、1 コマ分の視点の量を作る(設定の「マウス」の「3D マウス」)。
//!
//! 機器の値(-1〜1)→ 0 合わせ → 遊び → 緩い出だし → 入り切り・主な軸だけ・反転 → 速さ × 時間。
//! 動かし方は手で持った形を動かす向き(押した向きへ形が動く)。どのビューが受けるかは 3D のビュー
//! ([`kmb_ui_view3d::view3d::set_nav`])が決める。

use crate::Workspace;
use kmb_ui_view3d::view3d::{Nav, NavPress};
use serde::{Deserialize, Serialize};

/// 1 コマの時間の上限(秒)。1 コマが遅れても視点が飛ばない。
pub const MAX_DT: f32 = 0.05;
/// 速さの範囲(倍)。
pub const SPEED_RANGE: std::ops::RangeInclusive<f32> = 0.1..=4.0;
/// 遊びの範囲。
pub const DEAD_ZONE_RANGE: std::ops::RangeInclusive<f32> = 0.0..=0.5;

/// 速さ 1・いっぱいに倒した時の 1 秒の量: 回す 150 度、画面移動はビューの高さ 1 つ分、ズーム 3 倍。
const TURN_PER_SEC: f32 = 150.0 * std::f32::consts::PI / 180.0;
const ZOOM_PER_SEC: f32 = 1.098_612_3; // ln 3

/// 軸の名前(設定の頁の反転の欄)。機器の値の並び(移動 X・Y・Z、回転 X・Y・Z)と同じ順。
pub const AXIS_NAMES: [&str; 6] = [
    "左右に押す",
    "前後に押す",
    "上下に引く",
    "前後に傾ける",
    "左右に傾ける",
    "ひねる",
];

/// ズームを当てる手の動き。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomAxis {
    /// 前後に押す(上下に引くと上下の画面移動)
    #[default]
    Push,
    /// 上下に引く(前後に押すと上下の画面移動)
    Lift,
}

/// 3D マウスのボタンの働き。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonAction {
    #[default]
    None,
    Fit,
    Iso,
    Front,
    Top,
    Right,
}

impl ButtonAction {
    pub const ALL: [ButtonAction; 6] = [
        ButtonAction::Fit,
        ButtonAction::Iso,
        ButtonAction::Front,
        ButtonAction::Top,
        ButtonAction::Right,
        ButtonAction::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ButtonAction::None => "なし",
            ButtonAction::Fit => "全体表示",
            ButtonAction::Iso => "等角",
            ButtonAction::Front => "前",
            ButtonAction::Top => "上",
            ButtonAction::Right => "右",
        }
    }

    /// 視点の操作(視点のボタンの表の何番目か)。
    fn press(self) -> Option<NavPress> {
        let view = |label: &str| {
            kmb_ui_view3d::view3d::VIEW_BUTTONS
                .iter()
                .position(|b| b.label == label)
                .map(NavPress::View)
        };
        match self {
            ButtonAction::None => None,
            ButtonAction::Fit => Some(NavPress::Fit),
            ButtonAction::Iso => view("等角"),
            ButtonAction::Front => view("前"),
            ButtonAction::Top => view("上"),
            ButtonAction::Right => view("右"),
        }
    }
}

/// 3D マウスの設定(配置のファイルに入る。無い欄は既定)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mouse3dSettings {
    /// 3D マウスで視点を動かすか
    pub enabled: bool,
    /// 全体の速さ(0.1〜4 倍)
    pub speed: f32,
    /// オービット(傾ける・ひねる)を使うか
    pub rotate: bool,
    /// 画面移動とズーム(押す・引く)を使うか
    pub pan_zoom: bool,
    /// いちばん大きく動かした軸だけを使うか
    pub dominant: bool,
    pub zoom_axis: ZoomAxis,
    /// 軸ごとの反転([`AXIS_NAMES`] の順)
    pub invert: [bool; 6],
    /// 遊び(これより小さい値は 0。0〜0.5)
    pub dead_zone: f32,
    /// 手を放した時の値(「今の位置を 0 にする」で覚え、毎コマ引く)
    pub zero: [f32; 6],
    pub button1: ButtonAction,
    pub button2: ButtonAction,
}

impl Default for Mouse3dSettings {
    fn default() -> Self {
        Mouse3dSettings {
            enabled: true,
            speed: 1.0,
            rotate: true,
            pan_zoom: true,
            dominant: false,
            zoom_axis: ZoomAxis::Push,
            invert: [false; 6],
            dead_zone: 0.05,
            zero: [0.0; 6],
            button1: ButtonAction::Fit,
            button2: ButtonAction::Iso,
        }
    }
}

/// 範囲に収める(数でなければ既定)。
fn clamp(v: f32, r: &std::ops::RangeInclusive<f32>, or: f32) -> f32 {
    if v.is_finite() {
        v.clamp(*r.start(), *r.end())
    } else {
        or
    }
}

/// 0 合わせ・遊び・曲線・軸の入り切り・反転を適用した、視点に効く軸の量。
pub(crate) fn live_axes(axes: [f32; 6], s: &Mouse3dSettings) -> [f32; 6] {
    let dz = clamp(s.dead_zone, &DEAD_ZONE_RANGE, 0.05);
    let mut v = [0.0f32; 6];
    for i in 0..6 {
        let x = (axes[i] - s.zero[i]).clamp(-1.0, 1.0);
        let x = if x.is_finite() { x } else { 0.0 };
        let m = ((x.abs() - dz) / (1.0 - dz)).max(0.0);
        v[i] = x.signum() * m.powf(1.5);
        let on = if i < 3 { s.pan_zoom } else { s.rotate };
        if !on {
            v[i] = 0.0;
        }
        if s.invert[i] {
            v[i] = -v[i];
        }
    }
    if s.dominant
        && let Some(k) = (0..6).max_by(|&a, &b| v[a].abs().total_cmp(&v[b].abs()))
    {
        for (i, x) in v.iter_mut().enumerate() {
            if i != k {
                *x = 0.0;
            }
        }
    }
    v
}

/// 機器の値から、時間とビューの高さに合わせた視点の量を作る。
pub fn nav_of(axes: [f32; 6], s: &Mouse3dSettings, dt: f32, view_height: f32) -> Nav {
    let v = live_axes(axes, s);
    let t = clamp(s.speed, &SPEED_RANGE, 1.0) * dt.clamp(0.0, MAX_DT);
    let px = view_height.max(1.0) * t;
    // 機器の軸: 移動 X は右、Y は手前、Z は下。回転はそれぞれの軸のまわり(符号は機器による。
    // 設定の反転で合わせる)
    let [x, y, z, rx, ry, rz] = v;
    let (zoom, up) = match s.zoom_axis {
        ZoomAxis::Push => (y, z),
        ZoomAxis::Lift => (-z, y),
    };
    Nav {
        pan: egui::Vec2::new(x, up) * px,
        zoom: zoom * ZOOM_PER_SEC * t,
        spin: -rz * TURN_PER_SEC * t,
        tilt: rx * TURN_PER_SEC * t,
        roll: ry * TURN_PER_SEC * t,
        press: None,
    }
}

impl Workspace {
    /// このコマの 3D マウスの値を読み、3D のビューへ渡す(毎コマ 1 回、ビューを描く前に)。
    pub(crate) fn feed_mouse3d(&mut self, ctx: &egui::Context) {
        let Some(m) = &self.win.mouse3d else {
            self.win
                .keycast
                .prepare_mouse3d(&self.win.layout.keycast, false);
            return;
        };
        // 押されたボタンは使わない時も受け取って捨てる(後でまとめて働かない)
        let snap = m.snapshot();
        let s = &self.win.layout.mouse3d;
        let (focused, dt, now) = ctx.input(|i| (i.focused, i.stable_dt, i.time));
        self.win.keycast.prepare_mouse3d(
            &self.win.layout.keycast,
            s.enabled && matches!(snap.status, kmb_ui_mouse3d::Status::Connected { .. }),
        );
        let axes = if s.enabled && focused {
            live_axes(snap.axes, s)
        } else {
            [0.0; 6]
        };
        self.win
            .keycast
            .take_mouse3d(axes, &snap.buttons_pressed, now);
        if !s.enabled || !focused {
            kmb_ui_view3d::view3d::set_nav(ctx, Nav::default());
            return;
        }
        let mut nav = nav_of(snap.axes, s, dt, self.win.view_rect.height());
        for b in snap.buttons_pressed {
            let action = match b {
                1 => s.button1,
                2 => s.button2,
                _ => {
                    log::info!("3D マウスのボタン {b} は割り当てが無い");
                    continue;
                }
            };
            if let Some(p) = action.press() {
                nav.press = Some(p);
            }
        }
        if !nav.is_none() {
            ctx.request_repaint();
        }
        kmb_ui_view3d::view3d::set_nav(ctx, nav);
    }

    /// 設定の「マウス」の「3D マウス」の欄。
    pub(crate) fn mouse3d_settings(&mut self, ui: &mut egui::Ui) {
        let state = self.win.mouse3d.as_ref().map_or_else(
            || "探していない(安全モードでは探さない)".to_string(),
            |m| m.status().label(),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label("3D マウス");
            ui.weak(state);
        });
        let s = &mut self.win.layout.mouse3d;
        ui.checkbox(&mut s.enabled, "3D マウスで視点を動かす")
            .on_hover_text(
                "JC の窓が前にある時、ポインタの下の 3D のビュー(無ければ最後にポインタを乗せたビュー)が動く。\n\
                 押した向きへ形が動く。回す中心はマウスで回す時と同じ",
            );
        if !s.enabled {
            return;
        }
        ui.horizontal(|ui| {
            ui.label("速さ");
            ui.add(egui::Slider::new(&mut s.speed, SPEED_RANGE).logarithmic(true));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.rotate, "オービット");
            ui.checkbox(&mut s.pan_zoom, "画面移動とズーム");
            ui.checkbox(&mut s.dominant, "主な軸だけ")
                .on_hover_text("いちばん大きく動かした軸だけを使う(まっすぐ動かしやすい)");
        });
        ui.horizontal(|ui| {
            ui.label("ズーム");
            ui.radio_value(&mut s.zoom_axis, ZoomAxis::Push, "前後に押す");
            ui.radio_value(&mut s.zoom_axis, ZoomAxis::Lift, "上下に引く");
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("反転");
            for (i, name) in AXIS_NAMES.iter().enumerate() {
                ui.checkbox(&mut s.invert[i], *name);
            }
        });
        ui.horizontal(|ui| {
            ui.label("遊び");
            ui.add(egui::Slider::new(&mut s.dead_zone, DEAD_ZONE_RANGE))
                .on_hover_text(
                    "これより小さく動かした分は使わない(放しても少しずつ動く時に上げる)",
                );
        });
        let mut zero = false;
        ui.horizontal(|ui| {
            zero = ui
                .button("今の位置を 0 にする")
                .on_hover_text(
                    "手を放した時の値を 0 として覚える(放しても動き続ける時に、放してから押す)",
                )
                .clicked();
            if ui.button("既定に戻す").clicked() {
                *s = Mouse3dSettings::default();
            }
        });
        for (label, b) in [("ボタン 1", &mut s.button1), ("ボタン 2", &mut s.button2)] {
            ui.horizontal(|ui| {
                ui.label(label);
                egui::ComboBox::from_id_salt(label)
                    .selected_text(b.label())
                    .show_ui(ui, |ui| {
                        for a in ButtonAction::ALL {
                            ui.selectable_value(b, a, a.label());
                        }
                    });
            });
        }
        if zero && let Some(m) = &self.win.mouse3d {
            let axes = m.snapshot().axes;
            self.win.layout.mouse3d.zero = axes;
            log::info!("3D マウス: 今の位置を 0 にした({axes:?})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.02;
    const H: f32 = 600.0;

    #[test]
    fn dead_zone_curve_and_speed_limit() {
        let s = Mouse3dSettings::default();
        // 遊びの中は 0
        assert!(nav_of([0.04, 0.0, 0.0, 0.0, 0.04, 0.0], &s, DT, H).is_none());
        // いっぱいに倒すと 1 秒で 150 度、遅れたコマでも 0.05 秒分まで
        let full = nav_of([0.0, 0.0, 0.0, 0.0, 0.0, -1.0], &s, DT, H);
        assert!(
            (full.spin - TURN_PER_SEC * DT).abs() < 1e-5,
            "{}",
            full.spin
        );
        let late = nav_of([0.0, 0.0, 0.0, 0.0, 0.0, -1.0], &s, 2.0, H);
        assert!((late.spin - TURN_PER_SEC * MAX_DT).abs() < 1e-5);
        // 半分に倒すと緩い出だしで半分より小さい
        let half = nav_of([0.5, 0.0, 0.0, 0.0, 0.0, 0.0], &s, DT, H);
        let one = nav_of([1.0, 0.0, 0.0, 0.0, 0.0, 0.0], &s, DT, H);
        assert!(half.pan.x > 0.0 && half.pan.x < one.pan.x * 0.5);
        assert!((one.pan.x - H * DT).abs() < 1e-3, "ビューの高さ 1 つ分/秒");
        // 速さは範囲に収める
        let fast = Mouse3dSettings {
            speed: 100.0,
            ..s.clone()
        };
        let f = nav_of([1.0, 0.0, 0.0, 0.0, 0.0, 0.0], &fast, DT, H);
        assert!((f.pan.x - one.pan.x * 4.0).abs() < 1e-3);
    }

    #[test]
    fn toggles_dominant_axis_invert_zero_and_zoom_axis() {
        let s = Mouse3dSettings::default();
        let a = [0.3, 0.6, 0.2, 0.4, 0.0, 0.0];
        let n = nav_of(a, &s, DT, H);
        assert!(n.pan.x > 0.0 && n.zoom > 0.0 && n.pan.y > 0.0 && n.tilt > 0.0);
        // 主な軸だけ: 前後に押した分(いちばん大きい)だけ
        let d = nav_of(
            a,
            &Mouse3dSettings {
                dominant: true,
                ..s.clone()
            },
            DT,
            H,
        );
        assert!(d.zoom > 0.0 && d.pan == egui::Vec2::ZERO && !d.turns());
        // オービットを切ると回らない。画面移動とズームを切ると回るだけ
        let r = nav_of(
            a,
            &Mouse3dSettings {
                rotate: false,
                ..s.clone()
            },
            DT,
            H,
        );
        assert!(!r.turns() && r.zoom > 0.0);
        let p = nav_of(
            a,
            &Mouse3dSettings {
                pan_zoom: false,
                ..s.clone()
            },
            DT,
            H,
        );
        assert!(p.turns() && p.zoom == 0.0 && p.pan == egui::Vec2::ZERO);
        // 反転
        let mut inv = s.clone();
        inv.invert[0] = true;
        assert!(nav_of(a, &inv, DT, H).pan.x < 0.0);
        // 0 合わせ: 覚えた値は動いていないと見る
        let z = Mouse3dSettings {
            zero: a,
            ..s.clone()
        };
        assert!(nav_of(a, &z, DT, H).is_none());
        // ズームを上下に引くへ替えると、前後に押すのは上下の画面移動
        let l = Mouse3dSettings {
            zoom_axis: ZoomAxis::Lift,
            ..s.clone()
        };
        let n = nav_of([0.0, 0.6, 0.0, 0.0, 0.0, 0.0], &l, DT, H);
        assert!(n.zoom == 0.0 && n.pan.y > 0.0);
        // 数でない値は動かさない
        assert!(nav_of([f32::NAN; 6], &s, DT, H).is_none());
    }

    #[test]
    fn buttons_map_to_view_presses_and_old_settings_still_load() {
        assert_eq!(ButtonAction::Fit.press(), Some(NavPress::Fit));
        for a in [
            ButtonAction::Iso,
            ButtonAction::Front,
            ButtonAction::Top,
            ButtonAction::Right,
        ] {
            assert!(matches!(a.press(), Some(NavPress::View(_))), "{a:?}");
        }
        assert_eq!(ButtonAction::None.press(), None);
        let old: Mouse3dSettings = serde_json::from_str(r#"{"speed": 2.0}"#).unwrap();
        assert_eq!(old.speed, 2.0);
        assert_eq!(old.button1, ButtonAction::Fit);
        let back: Mouse3dSettings =
            serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(back, old);
    }

    #[test]
    fn a_fake_3d_mouse_turns_the_design_view_until_it_is_turned_off() {
        let m = kmb_ui_mouse3d::Mouse3d::fake();
        // 報告 2 = 回転。ひねりをいっぱいに
        m.feed(&[2, 0, 0, 0, 0, 0xA2, 0xFE]);
        let ws = Workspace::new(
            kmb_samples::reference_document(),
            None,
            kmb_bundle::modules(),
        )
        .with_mouse3d(m);
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
        h.run();
        let yaw0 = h.state().cur.camera.yaw;
        h.state_mut()
            .win
            .mouse3d
            .as_ref()
            .unwrap()
            .feed(&[2, 0, 0, 0, 0, 0xA2, 0xFE]);
        h.run();
        let yaw1 = h.state().cur.camera.yaw;
        assert!((yaw1 - yaw0).abs() > 1e-4, "回った: {yaw0} → {yaw1}");
        // 切ると動かない
        h.state_mut().win.layout.mouse3d.enabled = false;
        h.state_mut()
            .win
            .mouse3d
            .as_ref()
            .unwrap()
            .feed(&[2, 0, 0, 0, 0, 0xA2, 0xFE]);
        h.run();
        assert_eq!(h.state().cur.camera.yaw, yaw1);
    }
}
