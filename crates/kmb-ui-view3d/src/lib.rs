//! 3D のビュー。ボディ・経路・目安の線を CPU([`view3d`])か GPU([`gpu3d`])で描き、面・辺・点を拾う
//! (箱の木 `pick_bvh`)。画面の外枠(kmb-ui)とスケッチの編集器が使う部品で、egui と wgpu に依存するが、
//! 文書の機能(CAD・CAM)と画面の外枠は知らない。色は画面のテーマから [`view3d::Palette`] で受け取る。
//! 測る道具で拾う物と測る計算は kmb-measure。

pub mod gpu3d;
mod ink;
pub mod perf;
mod pick_bvh;
pub mod render;
pub mod view3d;

pub use ink::{
    Inks, Scale, corner, fs, icon, inks, inks_now, mono, motion, prop, scale, set_inks, set_scale,
};
pub use render::RenderSettings;
pub use view3d::{FaceLooks, Pick};

use egui::Color32;
use serde::{Deserialize, Serialize};

/// 写実の見た目(材質の質感と環境の光)。画面が毎フレーム [`view3d::Scene::effects`] に入れる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Effects {
    /// 外観・材料を付けたボディを、材質の質感(木目・金属の映り込み・つや)で描く
    pub materials: bool,
    /// 外観の無いボディも既定の材質で描く(レンダリングの作業空間)
    pub all_bodies: bool,
    /// 環境の光
    pub env: kmb_render::EnvKind,
}

impl Default for Effects {
    fn default() -> Self {
        Effects {
            materials: true,
            all_bodies: false,
            env: kmb_render::EnvKind::StudioLight,
        }
    }
}

/// 3D のビューの表示の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shading {
    /// 陰影と辺(既定)
    #[default]
    ShadedEdges,
    /// 陰影と辺に、面に隠れた辺を薄い点線で重ねる(U19。奥のポケットや裏の段差を透かして見る)
    ShadedHidden,
    /// 陰影だけ(辺を描かない)
    Shaded,
    /// 線だけ(面を塗らず、隠れた辺も描く)
    Wireframe,
}

impl Shading {
    pub const ALL: [Shading; 4] = [
        Shading::ShadedEdges,
        Shading::ShadedHidden,
        Shading::Shaded,
        Shading::Wireframe,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Shading::ShadedEdges => "シェーディング、エッジ表示のみ",
            Shading::ShadedHidden => "シェーディング、隠線エッジ表示",
            Shading::Shaded => "シェーディング",
            Shading::Wireframe => "ワイヤフレーム",
        }
    }

    /// 面を塗るか。
    pub fn faces(self) -> bool {
        self != Shading::Wireframe
    }

    /// 辺を描くか。
    pub fn edges(self) -> bool {
        self != Shading::Shaded
    }

    /// 面に隠れた辺を薄い点線で描くか(陰影と隠れ線)。
    pub fn hidden_dashed(self) -> bool {
        self == Shading::ShadedHidden
    }
}

/// 投影の仕方(表示のメニュー「表示設定」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    /// 平行投影(既定)
    #[default]
    Parallel,
    /// 透視投影
    Perspective,
    /// 見る向きが X・Y・Z のどれかと平行な時は平行投影、それ以外は透視投影
    Auto,
}

impl Projection {
    pub const ALL: [Projection; 3] = [
        Projection::Parallel,
        Projection::Perspective,
        Projection::Auto,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Projection::Parallel => "平行投影",
            Projection::Perspective => "透視投影",
            Projection::Auto => "自動",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Projection::Parallel => "遠近をつけず、平行な線は平行に見せる",
            Projection::Perspective => "遠くの物ほど小さく見せる",
            Projection::Auto => {
                "上・前・右など X・Y・Z の軸の向きから見る時は平行投影、\
                 それ以外の向きでは透視投影にする"
            }
        }
    }

    /// 視点 `cam` の今の向きで透視投影にするか。
    pub fn perspective_for(self, cam: &view3d::Camera) -> bool {
        match self {
            Projection::Parallel => false,
            Projection::Perspective => true,
            Projection::Auto => !cam.axis_aligned(),
        }
    }
}

/// 2 色を混ぜる(`k` は b の割合)。
pub fn mix(a: Color32, b: Color32, k: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}
