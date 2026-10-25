//! 加工の「段の帯」: 素材から各操作の後の形までを並べ、選んだ段の形を 3D のビューに出す。
//!
//! 形はシミュレーション(高さの格子)で削った結果を、操作ごとに粗い格子で残したもの。
//! 段を押した時にシミュレーションが今の経路のものでなければ、走らせてからその段を出す。
//! 画面は帯を描き、押された段の操作を実行するだけ。

use crate::SETUP;
use crate::commands::{CLEAR_STAGE, CamCommands, SHOW_STAGE, SimSource};
use crate::paths_cache::SetupRef;
use kmb_core::{Context, OverlayShape, Stage, StageBand};
use std::sync::Arc;

impl CamCommands {
    /// 帯の対象のセットアップ(選んでいる機能のセットアップ、なければ最初のもの)。
    pub(crate) fn band_setup(&self, cx: &Context<'_>) -> Option<SetupRef> {
        self.setup_of_selection(cx).or_else(|| {
            let id = cx.data.features.iter().find(|f| f.type_id == SETUP)?.id;
            self.setup_paths(cx, id).ok()
        })
    }

    /// 最後のシミュレーションが、セットアップ `s` の今の経路のものか。
    fn fresh(&self, s: &SetupRef) -> bool {
        self.sim
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|m| m.setup == s.setup && m.print == s.print())
    }

    pub(crate) fn stage_band(&self, cx: &Context<'_>) -> Option<StageBand> {
        let s = self.band_setup(cx)?;
        let stages = std::iter::once(Stage {
            label: "素材".into(),
            action: format!("{SHOW_STAGE}0"),
            hint: "削る前の素材".into(),
        })
        .chain(s.paths.iter().enumerate().map(|(i, p)| Stage {
            label: format!("{} {} T{}", i + 1, p.name, p.tool.number),
            action: format!("{SHOW_STAGE}{}", i + 1),
            hint: format!(
                "工具 T{} {} で、この操作まで削った形",
                p.tool.number, p.tool.name
            ),
        }))
        .collect();
        let fresh = self.fresh(&s);
        let shown = self
            .sim
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|_| fresh)
            .map(|m| (m.selected, m.source));
        let (selected, note) = match shown {
            Some((sel, source)) => (sel, self.stage_note(source)),
            None => (
                None,
                "段を押すとシミュレーションを走らせて、その操作の後の形を出します".into(),
            ),
        };
        let playback = self.playback(&s);
        Some(StageBand {
            title: s.name.clone(),
            stages,
            clear: selected.map(|_| CLEAR_STAGE.to_string()),
            selected,
            note,
            play: (!s.paths.is_empty()).then(|| crate::playback::PLAY.to_string()),
            playback,
        })
    }

    /// 3D のビューに出す段の形(今の経路のシミュレーションで、段を選んでいる時)。
    pub(crate) fn shown_stage(&self, cx: &Context<'_>) -> Option<Arc<OverlayShape>> {
        let s = self.band_setup(cx)?;
        if !self.fresh(&s) {
            return None;
        }
        let sim = self.sim.lock().unwrap_or_else(|e| e.into_inner());
        let m = sim.as_ref()?;
        m.stages.get(m.selected?).cloned()
    }

    pub(crate) fn show_stage(
        &self,
        cx: &Context<'_>,
        id: &str,
    ) -> Result<kmb_core::Outcome, String> {
        let k: usize = id
            .strip_prefix(SHOW_STAGE)
            .and_then(|n| n.parse().ok())
            .ok_or("段の番号が読めません")?;
        let s = self.band_setup(cx).ok_or("セットアップがありません")?;
        if k > s.paths.len() {
            return Err(format!("段 {k} はありません"));
        }
        // 段を出すなら再生はやめる(形が重ならないように)
        *self.play.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if self.fresh(&s) {
            let mut sim = self.sim.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(m) = sim.as_mut()
                && let Some(shape) = m.stages.get(k)
            {
                m.selected = Some(k);
                return Ok(kmb_core::Outcome::Message(format!(
                    "{} の形を出しています",
                    shape.label
                )));
            }
        }
        // シミュレーションがまだか古いので、走らせてからこの段を出す
        self.simulate(cx, s.setup, SimSource::Paths, Some(k))
    }

    pub(crate) fn clear_stage(&self) -> kmb_core::Outcome {
        if let Some(m) = self.sim.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            m.selected = None;
        }
        kmb_core::Outcome::Message("部品の形に戻しました".into())
    }
}
