//! 断面で切って中を見る。形は変えず、3D のビューで平面の片側を描かず、
//! 切り口を斜めの縞で塗る(既定はボディごとの色。外側も同じ色にできる)。平面は原点の平面か
//! 選んだ平らな面に平行で、位置を値か引き手で動かす。文書には入れない画面の状態(閉じると元の見え方に戻る)。

use crate::Workspace;
use crate::view3d;
use egui::Rect;

/// 断面の平面の元。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SectionPlane {
    /// 原点の平面(XY・YZ・ZX)
    Origin(usize),
    /// 選んだ平らな面に平行(名前と外向きの法線)
    Face { name: String, normal: [f32; 3] },
}

/// 断面の道具の状態。閉じても残し(`open` が偽)、次に開く時に置いた所から始める。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SectionView {
    /// 開いて切っているか
    pub open: bool,
    pub plane: SectionPlane,
    /// 平面の位置(法線の向きの距離 mm。原点の平面なら座標の値)
    pub offset: f32,
    /// 残す側を入れ替える
    pub flip: bool,
    /// 切り口をボディごとの色で塗る(既定。M11)
    pub by_body: bool,
    /// ボディの外側も切り口と同じ色で塗る
    pub outside: bool,
}

/// 原点の平面の名前と、切って見る側から見る法線(XY は上から、YZ は右から、ZX は前から見る)。
pub(crate) const ORIGIN_PLANES: [(&str, [f32; 3]); 3] = [
    ("XY 平面", [0.0, 0.0, 1.0]),
    ("YZ 平面", [1.0, 0.0, 0.0]),
    ("ZX 平面", [0.0, -1.0, 0.0]),
];

impl SectionView {
    /// 法線(長さ 1。裏返す前)。
    pub fn normal(&self) -> [f32; 3] {
        match &self.plane {
            SectionPlane::Origin(i) => ORIGIN_PLANES[*i].1,
            SectionPlane::Face { normal, .. } => *normal,
        }
    }

    /// 3D のビューに渡す断面(法線の向きの側を描かない。裏返すと反対側)。
    pub fn section(&self) -> view3d::Section {
        let n = self.normal();
        let k = if self.flip { -1.0 } else { 1.0 };
        view3d::Section {
            normal: n.map(|v| v * k),
            offset: self.offset * k,
            by_body: self.by_body,
            outside: self.outside,
        }
    }

    /// 平面の名前(欄と札)。
    pub fn label(&self) -> String {
        match &self.plane {
            SectionPlane::Origin(i) => ORIGIN_PLANES[*i].0.to_string(),
            SectionPlane::Face { name, .. } => format!("面 {name} に平行"),
        }
    }
}

/// 場面の外形を法線 `n` に写した範囲(ボディが無ければ -50〜50)。
fn reach(scene: &view3d::Scene, n: [f32; 3]) -> (f32, f32) {
    let (lo, hi) = scene.bounds();
    if lo[0] > hi[0] {
        return (-50.0, 50.0);
    }
    let mut r = (f32::INFINITY, f32::NEG_INFINITY);
    for i in 0..8 {
        let c = [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ];
        let d = c[0] * n[0] + c[1] * n[1] + c[2] * n[2];
        r = (r.0.min(d), r.1.max(d));
    }
    r
}

/// 選んだ面が平らなら、外向きの法線(どの三角形の法線も 1 度より近い)。
fn flat_normal(scene: &view3d::Scene, pick: &view3d::Pick) -> Option<[f32; 3]> {
    let b = scene.bodies.iter().find(|b| b.feature == pick.feature)?;
    let m = &b.mesh;
    let k = m.face_names.iter().position(|n| *n == pick.face)? as u32;
    let mut first: Option<[f32; 3]> = None;
    for (t, f) in m.triangles.iter().zip(&m.triangle_faces) {
        if *f != k {
            continue;
        }
        let p = t.map(|i| m.positions[i as usize]);
        let u = [0, 1, 2].map(|i| p[1][i] - p[0][i]);
        let v = [0, 1, 2].map(|i| p[2][i] - p[0][i]);
        let c = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        if l < 1e-9 {
            continue;
        }
        let n = c.map(|x| x / l);
        match first {
            None => first = Some(n),
            Some(f) if f[0] * n[0] + f[1] * n[1] + f[2] * n[2] < 1f32.to_radians().cos() => {
                return None;
            }
            Some(_) => {}
        }
    }
    first
}

impl Workspace {
    /// 断面で見ているか。
    pub fn is_sectioning(&self) -> bool {
        self.cur.section.as_ref().is_some_and(|s| s.open)
    }

    /// 断面の道具を開く・閉じる。開く時に平らな面を選んでいればその面に平行、無ければ ZX 平面で、
    /// 形の真ん中で切る。面を選ばずに開き直した時は、前に閉じた時の平面・位置・向きから始める
    /// (切り口を置き直さなくてよい)。
    pub fn toggle_section(&mut self) {
        if self.is_sectioning() {
            self.close_section();
            return;
        }
        let face = self.cur.picked.as_ref().and_then(|p| {
            flat_normal(&self.cur.scene, p).map(|normal| SectionPlane::Face {
                name: p.face.clone(),
                normal,
            })
        });
        if face.is_none()
            && let Some(last) = &mut self.cur.section
        {
            last.open = true;
            log::info!(
                "断面: 前の {}、位置 {:.2} mm から",
                last.label(),
                last.offset
            );
            return;
        }
        let plane = face.unwrap_or(SectionPlane::Origin(2));
        let mut s = SectionView {
            open: true,
            plane,
            offset: 0.0,
            flip: false,
            by_body: true,
            outside: false,
        };
        let (lo, hi) = reach(&self.cur.scene, s.normal());
        s.offset = (lo + hi) / 2.0;
        log::info!("断面: {}、位置 {:.2} mm", s.label(), s.offset);
        self.cur.section = Some(s);
    }

    /// 断面をやめる(平面と位置は残し、次に開く時にそこから始める)。
    pub(crate) fn close_section(&mut self) {
        if let Some(s) = self.cur.section.as_mut().filter(|s| s.open) {
            log::info!("断面をやめる");
            s.open = false;
        }
    }

    /// 断面の平面を替える(位置は形の真ん中にする)。
    fn set_section_plane(&mut self, plane: SectionPlane) {
        let Some(s) = &mut self.cur.section else {
            return;
        };
        s.plane = plane;
        let (lo, hi) = reach(&self.cur.scene, s.normal());
        s.offset = (lo + hi) / 2.0;
    }

    /// 断面の欄(ビューの右上、軸の玉の下)。
    pub(crate) fn section_panel(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(s) = self.cur.section.clone().filter(|s| s.open) else {
            return;
        };
        let l = self.win.look;
        let (lo, hi) = reach(&self.cur.scene, s.normal());
        let margin = ((hi - lo) * 0.05).max(0.5);
        let face = self.cur.picked.as_ref().and_then(|p| {
            flat_normal(&self.cur.scene, p).map(|normal| SectionPlane::Face {
                name: p.face.clone(),
                normal,
            })
        });
        let mut next = s.clone();
        let mut plane: Option<SectionPlane> = None;
        let mut close = false;
        let screen = ui.ctx().content_rect();
        let offset = egui::vec2(rect.right() - screen.right() - 10.0, rect.top() + 150.0);
        egui::Area::new(egui::Id::new("section_panel"))
            .anchor(egui::Align2::RIGHT_TOP, offset)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(l.pal.panel)
                    .stroke(egui::Stroke::new(1.0, l.ws()))
                    .corner_radius(crate::theme::corner(6.0))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(250.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("断面").strong().color(l.ws()));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button("閉じる")
                                        .on_hover_text("Esc でも閉じる。形は変わらない")
                                        .clicked()
                                    {
                                        close = true;
                                    }
                                },
                            );
                        });
                        ui.label(
                            egui::RichText::new(
                                "平面の片側を描かずに中を見る。形は変えない(閉じると元に戻る)",
                            )
                            .size(crate::theme::fs(11.0))
                            .color(l.pal.ink2),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("平面")
                                .size(crate::theme::fs(11.0))
                                .weak(),
                        );
                        ui.horizontal_wrapped(|ui| {
                            for (i, (name, _)) in ORIGIN_PLANES.iter().enumerate() {
                                let on = s.plane == SectionPlane::Origin(i);
                                if ui.selectable_label(on, *name).clicked() && !on {
                                    plane = Some(SectionPlane::Origin(i));
                                }
                            }
                        });
                        let face_on = matches!(s.plane, SectionPlane::Face { .. });
                        let r = ui
                            .add_enabled(
                                face.is_some() || face_on,
                                egui::Button::selectable(
                                    face_on,
                                    match &s.plane {
                                        SectionPlane::Face { .. } => s.label(),
                                        _ => "選んだ平らな面に平行".to_string(),
                                    },
                                ),
                            )
                            .on_disabled_hover_text("平らな面を 1 つ選んでから押す");
                        if r.clicked()
                            && let Some(f) = &face
                        {
                            plane = Some(f.clone());
                        }
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("位置(mm)")
                                .size(crate::theme::fs(11.0))
                                .weak(),
                        );
                        ui.add(
                            egui::Slider::new(&mut next.offset, (lo - margin)..=(hi + margin))
                                .clamping(egui::SliderClamping::Never)
                                .max_decimals(2),
                        );
                        ui.checkbox(&mut next.flip, "反対側を残す");
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("色")
                                .size(crate::theme::fs(11.0))
                                .weak(),
                        );
                        ui.checkbox(&mut next.by_body, "ボディごとに色分け")
                            .on_hover_text(
                                "切り口をボディごとに違う色で塗る。外すと、どのボディの切り口も同じ色",
                            );
                        ui.add_enabled(
                            next.by_body,
                            egui::Checkbox::new(&mut next.outside, "外側も同じ色で塗る"),
                        )
                        .on_hover_text("ボディの外側の面も、そのボディの切り口と同じ色で塗る")
                        .on_disabled_hover_text("「ボディごとに色分け」を入れると選べる");
                    });
            });
        if close {
            self.close_section();
            return;
        }
        if let Some(p) = plane {
            self.set_section_plane(p);
        } else if next != s {
            self.cur.section = Some(next);
        }
    }
}
