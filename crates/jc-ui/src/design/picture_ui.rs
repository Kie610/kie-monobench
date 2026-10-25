//! 下絵(キャンバス)の絵を機能の出力([`jc_core::Picture`])から読み、3D のビューとスケッチの編集に渡す。
//! 絵のファイルを読むのは `key` ごとに 1 回だけで、読んだ画素は egui の一時の覚えに置く
//! (画面の状態の型には足さない)。

use jc_core::{Picture, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// 読んだ絵(幅, 高さ, RGBA)。
pub type Rgba = Arc<(u32, u32, Vec<u8>)>;

/// 画面に貼る絵の長い辺の上限(画素)。文書には 4096 まで入るが、なぞるにはこれで足りる。
const SHOW_SIDE: u32 = 2048;

/// 読んだ絵の覚え(`key` → 画素。読めなかった絵は `None` を覚えて、毎フレーム読み直さない)。
#[derive(Clone, Default)]
struct Decoded(HashMap<u64, Option<Rgba>>);

/// 出力 `v` の絵の画素(覚えていればそれを、無ければ読んで覚える)。
pub fn rgba(ctx: &egui::Context, v: &Value) -> Option<Rgba> {
    let key = Picture::key_of(v)?;
    let id = egui::Id::new("jc-picture-rgba");
    if let Some(hit) = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Decoded>(id)
            .0
            .get(&key)
            .cloned()
    }) {
        return hit;
    }
    let read = Picture::image_of(v)
        .ok_or_else(|| "絵が入っていません".to_string())
        .and_then(|b| jc_session::canvas::decode(b, SHOW_SIDE));
    let got = match read {
        Ok(px) => Some(Arc::new(px)),
        Err(e) => {
            log::warn!("下絵の絵を読めません: {e}");
            None
        }
    };
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Decoded>(id)
            .0
            .insert(key, got.clone())
    });
    got
}

/// 出力 `v` の絵を 3D のビューに重ねる形にする。`plane` を渡すと、その平面と重なる絵だけを返す
/// (スケッチの編集で、同じ平面の下絵だけを下に描く)。
pub fn overlay(
    ctx: &egui::Context,
    v: &Value,
    plane: Option<&jc_core::Plane>,
) -> Option<jc_core::OverlayPicture> {
    let (pl, corners, opacity, key) = Picture::placement_of(v)?;
    if let Some(s) = plane
        && !coplanar(s, &pl)
    {
        return None;
    }
    Some(jc_core::OverlayPicture {
        corners: corners.map(|c| pl.to_world(c)),
        key,
        rgba: rgba(ctx, v)?,
        opacity: opacity as f32,
    })
}

/// 2 つの平面が重なるか(法線が平行で、片方の原点がもう片方の平面の上)。
fn coplanar(a: &jc_core::Plane, b: &jc_core::Plane) -> bool {
    use jc_core::kernel::{cross, dot, sub};
    let (na, nb) = (cross(a.x_axis, a.y_axis), cross(b.x_axis, b.y_axis));
    let c = cross(na, nb);
    dot(c, c) < 1e-12 && dot(sub(b.origin, a.origin), na).abs() < 1e-6
}
