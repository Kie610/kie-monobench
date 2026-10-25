//! ボディの材質(外観の色・金属・粗さ・木目)。外観と物理材料の表は `kmb_look::material`。

use crate::math::{V3, srgb_to_linear};
use kmb_look::material::{self, Finish, GrainFrame};

/// 外観を付けていないボディの色(画面の明るい見た目のボディの色)。
pub const DEFAULT_COLOR: [u8; 3] = [214, 216, 218];

/// 描く時の材質。
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// 色(sRGB。木目なら早材の色)
    pub color: [u8; 3],
    pub finish: Finish,
    /// 木目の芯(木目のある時だけ使う。[`Look::frame_for`] で決める)
    pub frame: GrainFrame,
    /// 重ねて貼る絵(デカール。面の平面は [`Look::fit_decal`] で決める)
    pub decal: Option<std::sync::Arc<crate::decal::DecalLook>>,
}

impl Default for Look {
    fn default() -> Self {
        Look::plain(DEFAULT_COLOR)
    }
}

impl Look {
    /// 色だけの材質(塗装の既定の質感)。
    pub fn plain(color: [u8; 3]) -> Look {
        Look {
            color,
            finish: material::PLAIN,
            frame: GrainFrame {
                center: [0.0; 3],
                axis: 0,
            },
            decal: None,
        }
    }

    /// 外観の名前(無ければ物理材料の名前)の材質。どちらも無いか知らない名前なら既定の色。
    pub fn named(appearance: Option<&str>, material: Option<&str>) -> Look {
        let a = appearance.and_then(|n| material::appearance(n).map(|a| (n, a.color)));
        let m = material.and_then(|n| material::phys_material(n).map(|m| (n, m.color)));
        match a.or(m) {
            Some((name, color)) => Look {
                finish: material::finish(name),
                ..Look::plain(color)
            },
            None => Look::default(),
        }
    }

    /// 文書から引いた外観の材質。
    pub fn resolved(r: &kmb_look::appearance::Resolved) -> Look {
        Look {
            finish: r.finish,
            ..Look::plain(r.color)
        }
    }

    /// 文書の外観 `looks` で引いた外観の名前(「このデザイン内」で直した外観も。無ければ物理材料の
    /// 名前)の材質。どちらも無いか知らない名前なら既定の色。
    pub fn of(
        looks: &kmb_look::appearance::Appearances,
        appearance: Option<&str>,
        material: Option<&str>,
    ) -> Look {
        match appearance.and_then(|n| looks.resolve(n)) {
            Some(r) => Look::resolved(&r),
            None => Look::named(None, material),
        }
    }

    /// 木目の芯を、ボディの箱 `min`〜`max` に合わせる。
    pub fn frame_for(mut self, min: V3, max: V3) -> Look {
        self.frame = GrainFrame::of_box(min, max);
        self
    }

    /// 点 `p` の色(線形)。
    pub fn albedo(&self, p: V3) -> V3 {
        match &self.finish.grain {
            Some(g) => material::grain_color(p, &self.frame, self.color, g).map(|v| {
                // grain_color は sRGB の 0〜1。線形にする
                srgb_to_linear((v * 255.0).round().clamp(0.0, 255.0) as u8)
            }),
            None => self.color.map(srgb_to_linear),
        }
    }

    /// 点 `p`・法線 `n` の色(線形。デカールも重ねる)と金属らしさ。デカールは金属でない印刷として
    /// 重ね、覆う割合だけ金属らしさを下げる(鋼の上のシールが鋼の映り込みの色にならない)。
    pub fn surface_at(&self, p: V3, n: V3) -> (V3, f32) {
        let c = self.albedo(p);
        let m = self.metallic();
        match &self.decal {
            Some(d) => {
                let (c, cover) = d.over(c, p, n);
                (c, m * (1.0 - cover))
            }
            None => (c, m),
        }
    }

    /// デカールを貼った材質(面の平面は [`Look::fit_decal`] で決める)。
    pub fn with_decal(mut self, d: crate::decal::DecalLook) -> Look {
        self.decal = Some(std::sync::Arc::new(d));
        self
    }

    /// デカールを、網 `mesh` の面 `face` の平面に合わせる(デカールの無い材質はそのまま)。
    pub fn fit_decal(mut self, mesh: &kmb_core::Mesh, face: usize) -> Look {
        if let Some(d) = &self.decal {
            self.decal = Some(std::sync::Arc::new(d.fit(mesh, face)));
        }
        self
    }

    /// 粗さ(GGX の α は粗さの 2 乗。0 に近すぎると映り込みの計算が乱れるので下を抑える)。
    pub fn roughness(&self) -> f32 {
        self.finish.roughness.clamp(0.03, 1.0)
    }

    pub fn metallic(&self) -> f32 {
        self.finish.metallic.clamp(0.0, 1.0)
    }
}
