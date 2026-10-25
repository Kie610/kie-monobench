//! ジョイントの動きを見る。ジョイントの欄のスライダの
//! 横の ▶ を押すと、範囲(下限〜上限。無い側はスライダの既定の範囲)の端から端までを往復して見せる。
//! 動かすのは 3D のビューの網だけで、形・値・取り消しは変えない(アニメーションと同じ
//! [`jc_ui_view3d::view3d::Scene::set_poses`])。■ を押す・パネルを閉じる・部品を引く・作業空間を
//! 替えると止まり、元の位置に戻る。

use crate::Workspace;
use crate::layout::Space;
use jc_core::FeatureId;
use jc_interchange::animation::drive_of;

/// 見せている途中の様子(egui の一時の記憶に置く)。
#[derive(Clone, Copy, Debug)]
struct Preview {
    feature: FeatureId,
    /// 範囲(スライダの単位。回転は度、スライダは mm)
    lo: f64,
    hi: f64,
    /// 押した時刻(秒)
    t0: f64,
    /// ボタンを最後に描いたコマ(描かれなくなったらパネルが閉じたので止める)
    seen: u64,
}

fn preview_id() -> egui::Id {
    egui::Id::new("joint-preview")
}

/// 回転は 1 秒に 90 度、スライダは 1 秒に 40 mm で動かす(往復で 2 秒より速くしない)。
fn period(slide: bool, span: f64) -> f64 {
    let speed = if slide { 40.0 } else { 90.0 };
    (2.0 * span / speed).max(2.0)
}

/// 押してから `t` 秒の値(範囲 `lo`〜`hi` の三角波。`start` から上へ動き始める)。
fn wave(lo: f64, hi: f64, start: f64, period: f64, t: f64) -> f64 {
    let span = hi - lo;
    if span <= 1e-9 {
        return lo;
    }
    let u0 = ((start.clamp(lo, hi) - lo) / span) / 2.0;
    let u = (u0 + t / period).rem_euclid(1.0);
    if u < 0.5 {
        lo + 2.0 * u * span
    } else {
        hi - (2.0 * u - 1.0) * span
    }
}

/// スライダの横の ▶/■。機能 `feature` がジョイントの今の様子(動き)を出している時だけ出す。
/// `lo`・`hi` はスライダの範囲(スライダの単位)。
pub(crate) fn button(ui: &mut egui::Ui, ws: &Workspace, feature: FeatureId, lo: f64, hi: f64) {
    if ws.win.layout.space == Space::Animation || drive_of(ws.cur.work.report(), feature).is_none()
    {
        return;
    }
    let ctx = ui.ctx().clone();
    let pass = ctx.cumulative_pass_nr();
    let mut cur = ctx.data(|d| d.get_temp::<Preview>(preview_id()));
    let on = cur.is_some_and(|p| p.feature == feature);
    let r = ui
        .small_button(if on { "■" } else { "▶" })
        .on_hover_text(if on {
            "動きを止めて元の位置に戻す"
        } else {
            "範囲の端から端まで動かして見せる(形と値は変えない)"
        });
    if r.clicked() {
        cur = if on {
            log::info!("ジョイント {feature} の動きを見るのをやめる");
            None
        } else {
            log::info!("ジョイント {feature} の動きを見る({lo:.1}〜{hi:.1})");
            Some(Preview {
                feature,
                lo,
                hi,
                t0: ctx.input(|i| i.time),
                seen: pass,
            })
        };
    } else if let Some(p) = cur.as_mut().filter(|p| p.feature == feature) {
        // 範囲の欄を直したら、新しい範囲で続ける
        p.lo = lo;
        p.hi = hi;
        p.seen = pass;
    }
    ctx.data_mut(|d| match cur {
        Some(p) => {
            d.insert_temp(preview_id(), p);
        }
        None => d.remove::<Preview>(preview_id()),
    });
}

impl Workspace {
    /// ジョイントの動きを見ている途中か(アニメーションの作業空間の外で、ボディを動かして見せる)。
    pub(crate) fn joint_previewing(&self, ctx: &egui::Context) -> bool {
        ctx.data(|d| d.get_temp::<Preview>(preview_id())).is_some()
    }

    /// 動きを見ている間、コマごとにボディを今の値の位置へ動かす。[`Workspace::sync_animation`] の後に
    /// 毎フレーム呼ぶ。
    pub(crate) fn sync_joint_preview(&mut self, ctx: &egui::Context) {
        let Some(p) = ctx.data(|d| d.get_temp::<Preview>(preview_id())) else {
            return;
        };
        let pass = ctx.cumulative_pass_nr();
        let drive = (self.win.layout.space != Space::Animation
            && !self.joint_dragging()
            && p.seen + 2 >= pass)
            .then(|| drive_of(self.cur.work.report(), p.feature))
            .flatten();
        let Some(drive) = drive else {
            log::info!("ジョイント {} の動きを見るのを終える", p.feature);
            ctx.data_mut(|d| d.remove::<Preview>(preview_id()));
            if self.win.layout.space != Space::Animation && self.cur.scene.is_posed() {
                self.cur.scene.set_poses(Default::default());
            }
            return;
        };
        let t = ctx.input(|i| i.time) - p.t0;
        let start = drive.to_shown(drive.value);
        let v = wave(
            p.lo,
            p.hi,
            start,
            period(drive.slide, p.hi - p.lo),
            t.max(0.0),
        );
        self.cur.scene.set_poses(self.preview_poses(p.feature, v));
        ctx.request_repaint();
    }

    /// ジョイント `joint` の値を `v`(スライダの単位)にした時の、表示しているボディごとの動き
    /// (連動するジョイントも一緒に動く)。
    pub(crate) fn preview_poses(
        &self,
        joint: FeatureId,
        v: f64,
    ) -> std::collections::BTreeMap<FeatureId, jc_core::kernel::Matrix4> {
        let data = self.cur.work.doc().data();
        let values = std::collections::BTreeMap::from([(joint, v)]);
        let by_root = jc_interchange::animation::joint_poses(
            data,
            &self.win.registry,
            self.cur.work.report(),
            &values,
        );
        jc_interchange::animation::bodies_of_roots(
            data,
            &self.win.registry,
            self.cur.work.report(),
            &by_root,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_goes_up_from_the_start_then_back() {
        // 0〜90 度、今 0 度、往復 2 秒: 0.5 秒で 45、1 秒で 90、1.5 秒で 45、2 秒で 0
        let at = |t| wave(0.0, 90.0, 0.0, 2.0, t);
        assert!((at(0.0) - 0.0).abs() < 1e-9);
        assert!((at(0.5) - 45.0).abs() < 1e-9);
        assert!((at(1.0) - 90.0).abs() < 1e-9);
        assert!((at(1.5) - 45.0).abs() < 1e-9);
        assert!((at(2.0) - 0.0).abs() < 1e-9);
        // 途中の値から始めると、その値から動き始める
        assert!((wave(-90.0, 90.0, 30.0, 4.0, 0.0) - 30.0).abs() < 1e-9);
        // 範囲が無い(幅 0)なら動かない
        assert_eq!(wave(10.0, 10.0, 10.0, 2.0, 1.0), 10.0);
        assert_eq!(period(false, 90.0), 2.0);
        assert_eq!(period(true, 200.0), 10.0);
    }
}
