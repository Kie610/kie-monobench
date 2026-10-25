//! 写実の絵を描く。ボディの網に外観の材質(金属・粗さ・木目)を当て、スタジオの環境の光と影で描く。
//! CPU だけで描くので、GPU の無い所(CLI・MCP・試験)でも同じ絵になる。
//!
//! 描き方は 2 つ。
//!
//! - [`raster`] … 速い描き方。三角形を画素に塗り、環境の光を式で近似し(拡散と、粗さでぼかした映り込み)、
//!   主の光の影を影の地図で落とす。動画のコマと、画面の「写実」の見本はこれ。
//! - [`trace`] … 光線追跡(経路追跡)。光源を直接たどる(NEE)と、材質の向きに選ぶのを多重重要度
//!   サンプリング(MIS)で混ぜる古典的な方法(Kajiya 1986、Veach 1997)。回すほどきれいになり、
//!   ノイズは [`denoise`] の À-Trous の平滑化(Dammertz ほか 2010)で減らす。光の候補を画素の間や
//!   前のコマから選び直す方式は使わない(docs/避ける形.md)。
//!
//! 環境([`env`])は式で書いたスタジオ(背景の濃淡と、丸いソフトボックスの光)で、画像を同梱しない。
//! GPU のシェーダー(kmb-ui-view3d の gpu3d)も同じ式を持つ。木目の式は `kmb_look::material`。

pub mod bvh;
pub mod camera;
pub mod decal;
pub mod denoise;
pub mod env;
pub mod envmap;
pub mod lamp;
pub mod look;
pub mod math;
pub mod picture;
pub mod raster;
pub mod scene;
pub mod settings;
pub mod trace;

pub use camera::Camera;
pub use env::{Env, EnvKind};
pub use envmap::{EnvFrame, EnvMap};
pub use look::Look;
pub use scene::{Object, RenderScene};
pub use settings::{Ground, SceneLook};

/// 描いた絵(sRGB の RGBA、上の行から)。
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// 画素 (x, y) の RGBA。
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

/// 背景。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Background {
    /// 環境の背景と床(床に影が落ちる)
    #[default]
    Studio,
    /// 透明(床も描かない。PNG を別の絵に重ねる時)
    Transparent,
    /// 単色(sRGB。床は描けて影が落ちる。明るさと露出は効かない)
    Color([u8; 3]),
}

impl Background {
    /// 何にも当たらない所を透けさせて、後で色を重ねるか(透明と単色)。
    pub(crate) fn see_through(self) -> bool {
        self != Background::Studio
    }
}

/// 線形の色を、透けた所に単色の背景を重ねて sRGB の RGBA にする(`c` は不透明さを掛けた色)。
pub(crate) fn finish_pixel(c: math::V3, a: f32, bg: Background) -> [u8; 4] {
    use math::{linear_to_srgb, scale, srgb_to_linear};
    match bg {
        Background::Color(b) => {
            let k = 1.0 - a.clamp(0.0, 1.0);
            let t = [0, 1, 2].map(|i| c[i] + srgb_to_linear(b[i]) * k);
            [
                linear_to_srgb(t[0]),
                linear_to_srgb(t[1]),
                linear_to_srgb(t[2]),
                255,
            ]
        }
        _ => {
            // 半分透けた縁は色を割り戻す(透明の背景に重ねた時に縁が黒ずまない)
            let t = if a > 0.0 && a < 1.0 {
                scale(c, 1.0 / a)
            } else {
                c
            };
            [
                linear_to_srgb(t[0]),
                linear_to_srgb(t[1]),
                linear_to_srgb(t[2]),
                (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            ]
        }
    }
}

/// 絵の大きさの上限(縦横とも)。
pub const MAX_SIZE: u32 = 4096;

/// 横に並べて描くスレッドの数。
pub(crate) fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// `rows` 行を、スレッドに分けて 1 行ずつ `f(y, row)` で埋める(`row` は 1 行の値)。
pub(crate) fn par_rows<T: Send>(data: &mut [T], width: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    if width == 0 || data.is_empty() {
        return;
    }
    let rows = data.len() / width;
    let n = threads().min(rows.max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    // 行を少しずつ取りに行く(重い行が片寄っても待たない)
    let chunks: Vec<std::sync::Mutex<&mut [T]>> =
        data.chunks_mut(width).map(std::sync::Mutex::new).collect();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                loop {
                    let y = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if y >= rows {
                        break;
                    }
                    let mut row = chunks[y].lock().unwrap_or_else(|e| e.into_inner());
                    f(y, &mut row);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests;
