//! 絵と動画と glTF を書き出す部品。画面や操作は持たない(絵は呼ぶ側が描いて [`Rgba`] で渡す)。
//!
//! - [`gltf`] … ボディの網を glTF 2.0 の 1 つのファイル(.glb)に書く。ほかの 3D のソフトで開ける。
//! - [`gif`] … 何枚もの絵を、ループする GIF に書く。全部の絵で共通の 256 色の表を使う。
//! - [`png_seq`] … 絵を 1 枚ずつ PNG にする(連番 PNG)。
//! - MP4 … Windows では OS の H.264(`mf`。Media Foundation)で書く。使えない時と Windows 以外は
//!   [`av1`](rav1e。アプリに入っている)で AV1 にし、[`mp4`](自前の書き手)で詰める。
//!   どちらも何も落とさず、使用料の要る部品をアプリに入れない。
//! - [`yuv`] … 動画の色(YUV 4:2:0)に直す。
//! - [`appearance`] … 外観の操作(ボディ・部品・面に付ける、このデザイン内の外観を直す)。
//! - [`commands`] … 操作(glTF の書き出し、静止画、回す動画)とシーンの設定。
//!   描くのは jc-render。
//! - [`video`] … 形式([`VideoFormat`])を選んで、集めた絵をまとめてファイルの中身にする。

pub mod appearance;
pub mod av1;
pub mod commands;
pub mod gif;
pub mod gltf;
#[cfg(windows)]
pub mod mf;
pub mod mp4;
pub mod png_seq;
pub mod video;
pub mod yuv;

pub use commands::RenderCommands;
pub use gltf::{GltfBody, write_glb};

/// 絵と動画の操作のモジュール。
pub fn module() -> jc_core::Module {
    // シーンの設定の入れ物は組み立てごとに 1 つ。ほかのモジュール(加工の動画)は共有の値で読む
    let store = std::sync::Arc::new(commands::SceneStore::default());
    let shared = store.clone();
    // デカールの読んだ絵の控えも組み立てごとに 1 つ
    let images = std::sync::Arc::new(jc_render::decal::DecalImages::default());
    jc_core::Module::new("jc-io-render", move |reg| {
        // 文書ごとのシーンの設定(区画 `render.scene`)
        reg.declare_section::<commands::SceneSection>();
        // ボディ・部品・面の外観と「このデザイン内」の外観(区画 `render.appearances`)
        reg.declare_section::<jc_look::appearance::Appearances>();
        reg.provide(shared.clone());
        reg.provide(images.clone());
    })
    .with_extension(RenderCommands { store })
}
pub use video::{Mp4Codec, VideoFormat, encode};

/// 1 枚の絵。画素は RGBA を 1 バイトずつ(sRGB)、行は上から並べる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    /// 透明な黒で埋めた絵。
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        }
    }

    /// 画素の数と幅・高さが合っているか。
    pub fn is_valid(&self) -> bool {
        self.pixels.len() == self.width as usize * self.height as usize * 4
    }

    /// (x, y) の画素(左上が 0, 0)。
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
}

/// 全部の絵が同じ大きさで、画素の数も合っているかを確かめ、その大きさを返す。
pub(crate) fn frame_size(frames: &[Rgba]) -> Result<(u32, u32), String> {
    let first = frames.first().ok_or("絵が 1 枚もありません")?;
    if first.width == 0 || first.height == 0 {
        return Err("絵の幅か高さが 0 です".into());
    }
    for (i, f) in frames.iter().enumerate() {
        if (f.width, f.height) != (first.width, first.height) {
            return Err(format!(
                "{} 枚目の絵の大きさ({}×{})が 1 枚目({}×{})と違います",
                i + 1,
                f.width,
                f.height,
                first.width,
                first.height
            ));
        }
        if !f.is_valid() {
            return Err(format!(
                "{} 枚目の絵の画素の数が幅と高さに合いません",
                i + 1
            ));
        }
    }
    Ok((first.width, first.height))
}

/// 1 秒あたりの枚数が使える値か。
pub(crate) fn check_fps(fps: f32) -> Result<(), String> {
    if fps.is_finite() && fps > 0.0 && fps <= 240.0 {
        Ok(())
    } else {
        Err(format!(
            "1 秒あたりの枚数({fps})は 0 より大きく 240 以下にしてください"
        ))
    }
}

/// 試験で使う、色の帯と動く四角の絵。
#[cfg(test)]
pub(crate) fn test_frames(width: u32, height: u32, count: usize) -> Vec<Rgba> {
    (0..count)
        .map(|k| {
            let mut img = Rgba::new(width, height);
            for y in 0..height {
                for x in 0..width {
                    let i = ((y * width + x) * 4) as usize;
                    let inside = x >= (k as u32 * 3) % width
                        && x < (k as u32 * 3) % width + width / 4
                        && y >= height / 3
                        && y < height * 2 / 3;
                    let c = if inside {
                        [230, 60, 40, 255]
                    } else {
                        [(x * 255 / width) as u8, (y * 255 / height) as u8, 128, 255]
                    };
                    img.pixels[i..i + 4].copy_from_slice(&c);
                }
            }
            img
        })
        .collect()
}
