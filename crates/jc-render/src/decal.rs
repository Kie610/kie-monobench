//! デカール(面に貼る絵)。文書のデカールの機能の出力
//! ([`jc_look::decor::Decal`])を、面の材質([`crate::Look`])に重ねる絵にする。
//!
//! 面には絵を貼る座標(UV)が無いので、面の平面へまっすぐ投影して貼る(プロジェクターで映すのと
//! 同じ)。面の平面は、今の網のその面の三角形から決める: 中心は面積で重みを付けた重心、向きは
//! 面積で重みを付けた法線の和。絵の上は世界の上(Z)を面へ映した向き(上か下を向いた面は世界の Y)。
//! 曲がった面は投影の向きから傾くほど絵が伸び、裏側(投影の向きと反対を向く所)には貼らない。

use crate::math::{V3, add, cross, dot, len, lerp, normalize, scale, srgb_to_linear, sub};
use jc_core::Mesh;
use std::sync::{Arc, Mutex};

/// 貼る絵の長い辺の上限(画素)。文書の絵はもっと大きくてもよいが、描く時はこの大きさに縮める。
pub const MAX_SIDE: u32 = 2048;

/// 読んだ絵(RGBA、上の行から)。
#[derive(Debug)]
pub struct DecalImage {
    pub key: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 面の上の置き方(面の平面に投影する時の、絵の左下の角と、右と上の向きを幅と高さで割った物)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecalFrame {
    pub origin: V3,
    /// 絵の右の向き / 幅(点との内積が絵の横の 0〜1)
    pub u: V3,
    /// 絵の上の向き / 高さ
    pub v: V3,
    /// 投影の向き(面の外向き、長さ 1)
    pub n: V3,
}

/// 面の材質に重ねるデカール。
#[derive(Debug, Clone)]
pub struct DecalLook {
    pub image: Arc<DecalImage>,
    /// 置き方(絵の中身は持たない)
    pub place: jc_look::decor::Decal,
    /// 今の網から決めた面の平面([`DecalLook::fit`] の前は `None` で、貼らない)
    pub frame: Option<DecalFrame>,
}

impl PartialEq for DecalLook {
    fn eq(&self, o: &Self) -> bool {
        self.image.key == o.image.key && self.place == o.place && self.frame == o.frame
    }
}

/// 読んだ絵を覚えておく数(画面は文書が変わるたびに材質を作り直すので、毎回読み直さない)。
const KEEP: usize = 16;

/// 読んだ絵の控え。使う側(画面の窓、レンダリングの操作)が 1 つずつ持つ(プロセス全体の static に
/// 置かない)。
#[derive(Default)]
pub struct DecalImages(Mutex<Vec<Arc<DecalImage>>>);

/// 絵のファイルの中身(base64)を、長い辺 `max_side` 以下の画素(幅, 高さ, RGBA)にする関数。
/// 絵の形式を読むのは描き手の外の役目なので、使う側が渡す(画面は jc-session、書き出しは jc-image)。
pub type Decode<'a> = &'a dyn Fn(&str, u32) -> Result<(u32, u32, Vec<u8>), String>;

/// 絵の中身 `base64` を `decode` で読む(覚えた絵があれば読まない)。読めなければ `None`(誤りはログへ)。
pub fn image(
    images: &DecalImages,
    key: u64,
    base64: impl FnOnce() -> Option<String>,
    decode: Decode<'_>,
) -> Option<Arc<DecalImage>> {
    let mut cache = images.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = cache.iter().position(|d| d.key == key) {
        let d = cache.remove(i);
        cache.push(d.clone());
        return Some(d);
    }
    let (width, height, rgba) = match decode(&base64()?, MAX_SIDE) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("デカールの絵を読めません: {e}");
            return None;
        }
    };
    let d = Arc::new(DecalImage {
        key,
        width,
        height,
        rgba,
    });
    if cache.len() >= KEEP {
        cache.remove(0);
    }
    cache.push(d.clone());
    Some(d)
}

impl DecalLook {
    pub fn new(image: Arc<DecalImage>, place: jc_look::decor::Decal) -> DecalLook {
        DecalLook {
            image,
            place,
            frame: None,
        }
    }

    /// 網 `mesh` の面 `face`(`mesh.face_names` の番号)の平面に合わせる。面の三角形が無いか、
    /// 向きが決まらなければ貼らない。
    pub fn fit(&self, mesh: &Mesh, face: usize) -> DecalLook {
        DecalLook {
            frame: frame_of(mesh, face, &self.place),
            ..self.clone()
        }
    }

    /// 点 `p`・法線 `n` の色 `base`(線形)に絵を重ねた色と、絵が覆う割合(0〜1)。
    pub fn over(&self, base: V3, p: V3, n: V3) -> (V3, f32) {
        let Some(f) = &self.frame else {
            return (base, 0.0);
        };
        if dot(n, f.n) <= 0.05 {
            return (base, 0.0);
        }
        let d = sub(p, f.origin);
        let (mut s, mut t) = (dot(d, f.u), dot(d, f.v));
        if !(0.0..=1.0).contains(&s) || !(0.0..=1.0).contains(&t) {
            return (base, 0.0);
        }
        if self.place.flip_h {
            s = 1.0 - s;
        }
        if self.place.flip_v {
            t = 1.0 - t;
        }
        let (c, a) = self.sample(s, 1.0 - t);
        let cover = a * self.place.opacity as f32;
        (lerp(base, c, cover), cover)
    }

    /// 絵の (s, t)(左上が 0, 0)の色(線形)と不透明さ。隣の 4 画素を混ぜる。
    fn sample(&self, s: f32, t: f32) -> (V3, f32) {
        let im = &self.image;
        if im.width == 0 || im.height == 0 {
            return ([0.0; 3], 0.0);
        }
        let x = (s * im.width as f32 - 0.5).clamp(0.0, (im.width - 1) as f32);
        let y = (t * im.height as f32 - 0.5).clamp(0.0, (im.height - 1) as f32);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(im.width - 1), (y0 + 1).min(im.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let px = |x: u32, y: u32| {
            let i = ((y * im.width + x) * 4) as usize;
            let c = &im.rgba[i..i + 4];
            let a = c[3] as f32 / 255.0;
            // 不透明さを掛けた色で混ぜる(透けた所の色がにじまない)
            (
                [
                    srgb_to_linear(c[0]) * a,
                    srgb_to_linear(c[1]) * a,
                    srgb_to_linear(c[2]) * a,
                ],
                a,
            )
        };
        let (c00, a00) = px(x0, y0);
        let (c10, a10) = px(x1, y0);
        let (c01, a01) = px(x0, y1);
        let (c11, a11) = px(x1, y1);
        let c = lerp(lerp(c00, c10, fx), lerp(c01, c11, fx), fy);
        let a = (a00 + (a10 - a00) * fx) * (1.0 - fy) + (a01 + (a11 - a01) * fx) * fy;
        if a <= 1e-6 {
            return ([0.0; 3], 0.0);
        }
        (scale(c, 1.0 / a), a)
    }
}

/// ボディ(最初に作った機能)ごとの、面の名前 → デカール。
pub type Decals =
    std::collections::BTreeMap<jc_core::FeatureId, std::collections::BTreeMap<String, DecalLook>>;

/// 機能の出力(デカールの出力 [`jc_look::decor::DECAL`] の値。タイムラインの順)から、面ごとの
/// デカール。同じ面に 2 枚あれば後の機能の物(1 つの面に貼れるのは 1 枚)。`root_of` は面を選んだ
/// ボディの機能から、ボディの最初の機能を引く。絵は `decode` で読み、読めない物は除く(ログに出す)。
pub fn decals_of<'a>(
    outputs: impl IntoIterator<Item = &'a jc_core::Value>,
    root_of: impl Fn(jc_core::FeatureId) -> jc_core::FeatureId,
    images: &DecalImages,
    decode: Decode<'_>,
) -> Decals {
    let mut out = Decals::new();
    for v in outputs {
        let Some(place) = jc_look::decor::Decal::without_image(v) else {
            continue;
        };
        let Some(img) = image(
            images,
            place.key,
            || jc_look::decor::Decal::image_of(v).map(str::to_string),
            decode,
        ) else {
            continue;
        };
        out.entry(root_of(place.body))
            .or_default()
            .insert(place.face.clone(), DecalLook::new(img, place));
    }
    out
}

/// 面ごとの材質 `faces`(`mesh.face_names` の順。`None` はボディの材質 `body`)に、デカールを重ねる。
/// 網の平面への合わせは、描く側が網と一緒に行う([`crate::Look::fit_decal`])。
pub fn with_decals(
    faces: Vec<Option<crate::Look>>,
    names: &[String],
    body: &crate::Look,
    decals: Option<&std::collections::BTreeMap<String, DecalLook>>,
) -> Vec<Option<crate::Look>> {
    let Some(decals) = decals.filter(|d| !d.is_empty()) else {
        return faces;
    };
    names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let own = faces.get(i).cloned().flatten();
            match decals.get(n) {
                Some(d) => Some(own.unwrap_or_else(|| body.clone()).with_decal(d.clone())),
                None => own,
            }
        })
        .collect()
}

/// 面の平面と、絵の置き方から、投影の枠。
fn frame_of(mesh: &Mesh, face: usize, d: &jc_look::decor::Decal) -> Option<DecalFrame> {
    let mut area = 0.0f32;
    let mut center = [0.0f32; 3];
    let mut normal = [0.0f32; 3];
    for (t, f) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        if *f as usize != face {
            continue;
        }
        let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
        let nn = cross(sub(b, a), sub(c, a));
        let w = len(nn) * 0.5;
        if w <= 0.0 {
            continue;
        }
        area += w;
        center = add(center, scale(add(add(a, b), c), w / 3.0));
        normal = add(normal, scale(nn, 0.5));
    }
    if area <= 0.0 || len(normal) < 0.05 * area {
        return None;
    }
    let center = scale(center, 1.0 / area);
    let n = normalize(normal);
    // 絵の上: 世界の Z を面へ映した向き。面が上か下を向いていれば世界の Y
    let world_up = if n[2].abs() > 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let up0 = normalize(sub(world_up, scale(n, dot(world_up, n))));
    let right0 = cross(up0, n);
    // 回す(面の外から見て左回り)
    let (s, c) = (d.angle as f32).sin_cos();
    let right = add(scale(right0, c), scale(up0, s));
    let up = add(scale(up0, c), scale(right0, -s));
    let w = d.width as f32;
    if w <= 0.0 || d.px[0] == 0 {
        return None;
    }
    let h = w * d.px[1] as f32 / d.px[0] as f32;
    let mid = add(
        center,
        add(
            scale(right, d.offset[0] as f32),
            scale(up, d.offset[1] as f32),
        ),
    );
    let origin = sub(mid, add(scale(right, w / 2.0), scale(up, h / 2.0)));
    Some(DecalFrame {
        origin,
        u: scale(right, 1.0 / w),
        v: scale(up, 1.0 / h),
        n,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正面(−Y)を向いた 20 × 10 mm の板(面 0)と、上を向いた面(面 1)。
    fn plate() -> Mesh {
        Mesh {
            positions: vec![
                [-10.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [10.0, 0.0, 10.0],
                [-10.0, 0.0, 10.0],
                [-10.0, 0.0, 10.0],
                [10.0, 0.0, 10.0],
                [10.0, 5.0, 10.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3], [4, 5, 6]],
            triangle_faces: vec![0, 0, 1],
            face_names: vec!["front".into(), "top".into()],
            ..Mesh::default()
        }
    }

    fn place() -> jc_look::decor::Decal {
        jc_look::decor::Decal {
            body: jc_core::FeatureId(1),
            face: "front".into(),
            image: String::new(),
            key: 1,
            px: [2, 1],
            offset: [0.0, 0.0],
            width: 10.0,
            angle: 0.0,
            opacity: 1.0,
            flip_h: false,
            flip_v: false,
        }
    }

    /// 左が赤、右が青の 2 × 1 の絵。
    fn red_blue() -> Arc<DecalImage> {
        Arc::new(DecalImage {
            key: 1,
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        })
    }

    #[test]
    fn a_decal_sits_at_the_middle_of_the_face_upright_and_only_on_the_front() {
        let d = DecalLook::new(red_blue(), place()).fit(&plate(), 0);
        let f = d.frame.unwrap();
        // 正面の外向きは −Y、絵の右は +X、上は +Z。中心 (0, 0, 5)、幅 10・高さ 5
        assert!(len(sub(f.n, [0.0, -1.0, 0.0])) < 1e-5);
        assert!(len(sub(f.origin, [-5.0, 0.0, 2.5])) < 1e-4);
        let grey = [0.5; 3];
        let n = [0.0, -1.0, 0.0];
        // 左の端は赤、右の端は青、絵の外と裏は元の色
        let (left, cover) = d.over(grey, [-4.9, 0.0, 5.0], n);
        assert_eq!(cover, 1.0);
        assert!(left[0] > 0.9 && left[2] < 0.1, "{left:?}");
        let (right, _) = d.over(grey, [4.9, 0.0, 5.0], n);
        assert!(right[2] > 0.9 && right[0] < 0.1, "{right:?}");
        assert_eq!(d.over(grey, [-8.0, 0.0, 5.0], n), (grey, 0.0));
        assert_eq!(d.over(grey, [0.0, 0.0, 5.0], [0.0, 1.0, 0.0]), (grey, 0.0));
        // 左右の反転と、不透明度の半分
        let mut p = place();
        p.flip_h = true;
        p.opacity = 0.5;
        let d = DecalLook::new(red_blue(), p).fit(&plate(), 0);
        let (left, cover) = d.over(grey, [-4.9, 0.0, 5.0], n);
        assert!((cover - 0.5).abs() < 1e-6);
        assert!(
            (left[2] - 0.75).abs() < 0.02 && (left[0] - 0.25).abs() < 0.02,
            "{left:?}"
        );
    }

    #[test]
    fn a_turned_decal_and_an_upward_face() {
        // 90 度回すと、絵の右が上(+Z)を向く: 下の端が赤
        let mut p = place();
        p.angle = std::f64::consts::FRAC_PI_2;
        let d = DecalLook::new(red_blue(), p).fit(&plate(), 0);
        let (low, _) = d.over([0.0; 3], [0.0, 0.0, 0.2], [0.0, -1.0, 0.0]);
        assert!(low[0] > 0.9, "{low:?}");
        // 上を向いた面は、絵の上が世界の +Y
        let d = DecalLook::new(red_blue(), place()).fit(&plate(), 1);
        let f = d.frame.unwrap();
        assert!(dot(f.v, [0.0, 1.0, 0.0]) > 0.0);
        assert!(len(sub(f.n, [0.0, 0.0, 1.0])) < 1e-5);
        // 無い面には貼らない
        assert!(
            DecalLook::new(red_blue(), place())
                .fit(&plate(), 5)
                .frame
                .is_none()
        );
    }
}
