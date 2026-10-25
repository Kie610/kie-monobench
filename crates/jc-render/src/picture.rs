//! 形を絵にする。画面の無い所(MCP の `screenshot`)で、ボディの表示用の網を CPU で描いて PNG にする。
//! 画面の部品(egui・wgpu)は使わない。色と光の当て方は、3D のビューの明るい見た目
//! (`jc-ui-view3d` の CPU の描き方)に合わせる: 背景は薄い灰緑、ボディは色味のない白・灰色、
//! 稜線は濃い灰色。平行投影で、ボディ全体が入るように大きさを合わせる。
//!
//! 描き方: 縦横 2 倍の大きさで三角形を奥行きつきで塗り(頂点の明るさを補う)、稜線を少し手前に
//! 寄せて隠れていない所だけ描いてから、2×2 の平均で縮める(縁のぎざぎざを減らす)。
//! PNG は無圧縮の行を zlib で固めて書く(jc-format の PNG の書き手)。視点の向きは [`crate::camera::basis`]。

use crate::camera::basis;
use jc_core::{FacePick, FeatureId, Mesh};

/// 視点の名前(MCP の `view`)、画面のボタンの名前、向き(度。画面の視点のボタンと同じ)。
pub const VIEWS: [(&str, &str, f32, f32); 8] = [
    ("iso", "等角", -60.0, 30.0),
    ("iso_back", "等角(後)", 120.0, 30.0),
    ("top", "上", -90.0, 89.9),
    ("bottom", "下", -90.0, -89.9),
    ("front", "前", -90.0, 0.0),
    ("back", "後", 90.0, 0.0),
    ("right", "右", 0.0, 0.0),
    ("left", "左", 180.0, 0.0),
];

/// 名前の視点の向き(ヨー・ピッチの度)。
pub fn view(name: &str) -> Option<(f32, f32)> {
    VIEWS
        .iter()
        .find(|v| v.0 == name)
        .map(|&(_, _, yaw, pitch)| (yaw, pitch))
}

/// 描くボディ 1 つ。
pub struct Body<'a> {
    pub feature: FeatureId,
    pub mesh: &'a Mesh,
}

/// 描き方の指定。
#[derive(Debug, Clone)]
pub struct Options {
    /// 絵の大きさ(画素)
    pub width: u32,
    pub height: u32,
    /// 向き(度。画面の視点と同じ。ヨーは Z のまわり、ピッチは上下)
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    /// 目立たせる面(選んだ面と同じ色)
    pub faces: Vec<FacePick>,
    /// 目立たせる辺(ボディと辺の番号。太く差し色で)
    pub edges: Vec<(FeatureId, usize)>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 800,
            height: 600,
            yaw_deg: -60.0,
            pitch_deg: 30.0,
            faces: Vec::new(),
            edges: Vec::new(),
        }
    }
}

/// 描いた絵(RGB、上の行から)。
#[derive(Debug, Clone)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    /// 1 mm あたりの画素数(平行投影なので絵のどこでも同じ)
    pub px_per_mm: f64,
}

/// 大きさの上限(縦横とも)。
pub const MAX_SIZE: u32 = 2048;

type Rgb = [u8; 3];
const BACKGROUND: Rgb = [0xe6, 0xea, 0xe9];
/// 外観の無いボディの色(鋼の灰色。画面の既定の色と同じ)
const BODY: Rgb = [160, 160, 160];
/// 設計の作業空間の差し色(画面の選んだ面の色の元)
const ACCENT: Rgb = [0x17, 0x76, 0x6a];

/// ボディの稜線の色(画面の明るい画面の `line_color` と同じ: ボディの色を暗い側へ寄せる)。
fn line_color(body: Rgb) -> Rgb {
    mix(body, [40, 44, 52], 0.75)
}

fn mix(a: Rgb, b: Rgb, k: f32) -> Rgb {
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * k).round() as u8)
}

/// ボディごとの色(画面の `body_tint` と同じ: 2 つ目からは明るさだけを変える)。
fn body_tint(index: usize) -> Rgb {
    const SHADES: [(f32, bool); 4] = [(0.55, true), (0.2, false), (0.3, true), (0.32, false)];
    if index == 0 {
        return BODY;
    }
    let (k, lighter) = SHADES[(index - 1) % SHADES.len()];
    mix(BODY, if lighter { [255; 3] } else { [60; 3] }, k)
}

type V3 = [f32; 3];
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn normalize(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { a.map(|v| v / l) } else { a }
}

/// 縦横 `ss` 倍で描く所。奥行きは大きいほど手前。
struct Canvas {
    w: usize,
    h: usize,
    color: Vec<Rgb>,
    depth: Vec<f32>,
}

impl Canvas {
    /// 三角形を塗る(頂点は画素の座標・奥行き・明るさ)。
    fn fill(&mut self, v: [(f32, f32, f32, f32); 3], base: Rgb) {
        let area = (v[1].0 - v[0].0) * (v[2].1 - v[0].1) - (v[1].1 - v[0].1) * (v[2].0 - v[0].0);
        if area.abs() < 1e-9 {
            return;
        }
        let x0 = v
            .iter()
            .map(|p| p.0)
            .fold(f32::MAX, f32::min)
            .floor()
            .max(0.0) as usize;
        let y0 = v
            .iter()
            .map(|p| p.1)
            .fold(f32::MAX, f32::min)
            .floor()
            .max(0.0) as usize;
        let x1 = (v.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as usize).min(self.w);
        let y1 = (v.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as usize).min(self.h);
        let edge = |a: (f32, f32, f32, f32), b: (f32, f32, f32, f32), x: f32, y: f32| {
            (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0)
        };
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = edge(v[1], v[2], px, py) / area;
                let w1 = edge(v[2], v[0], px, py) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * v[0].2 + w1 * v[1].2 + w2 * v[2].2;
                let i = y * self.w + x;
                if z <= self.depth[i] {
                    continue;
                }
                self.depth[i] = z;
                let k = w0 * v[0].3 + w1 * v[1].3 + w2 * v[2].3;
                self.color[i] = base.map(|c| (c as f32 * k).clamp(0.0, 255.0) as u8);
            }
        }
    }

    /// 線を引く。`width` 画素の太さで、奥行きが前の物より手前の所だけ。
    fn line(&mut self, a: (f32, f32, f32), b: (f32, f32, f32), color: Rgb, width: usize) {
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let steps = len.ceil().clamp(1.0, 100_000.0) as usize;
        let half = (width / 2) as isize;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let x = (a.0 + (b.0 - a.0) * t) as isize;
            let y = (a.1 + (b.1 - a.1) * t) as isize;
            let z = a.2 + (b.2 - a.2) * t;
            for dy in -half..(width as isize - half) {
                for dx in -half..(width as isize - half) {
                    let (x, y) = (x + dx, y + dy);
                    if x < 0 || y < 0 || x >= self.w as isize || y >= self.h as isize {
                        continue;
                    }
                    let i = y as usize * self.w + x as usize;
                    if z >= self.depth[i] {
                        self.color[i] = color;
                    }
                }
            }
        }
    }
}

/// ボディを描く。ボディが無ければ背景だけの絵になる。
pub fn render(bodies: &[Body<'_>], o: &Options) -> Picture {
    let (width, height) = (o.width.clamp(16, MAX_SIZE), o.height.clamp(16, MAX_SIZE));
    // 大きい絵は 2 倍にしない(メモリを食いすぎないように)
    let ss: usize = if width as u64 * height as u64 <= 1_048_576 {
        2
    } else {
        1
    };
    let (w, h) = (width as usize * ss, height as usize * ss);
    let mut c = Canvas {
        w,
        h,
        color: vec![BACKGROUND; w * h],
        depth: vec![f32::MIN; w * h],
    };
    let (right, up, eye) = basis(o.yaw_deg, o.pitch_deg);
    // 全体が入る大きさ(縁に 1 割ほど余白)
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for b in bodies {
        for p in &b.mesh.positions {
            let q = [dot(*p, right), dot(*p, up), dot(*p, eye)];
            for i in 0..3 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
            }
        }
    }
    let has = lo[0] <= hi[0];
    let (bw, bh) = if has {
        ((hi[0] - lo[0]).max(1e-3), (hi[1] - lo[1]).max(1e-3))
    } else {
        (1.0, 1.0)
    };
    let scale = 0.85 * (w as f32 / bw).min(h as f32 / bh);
    let (cx, cy) = if has {
        ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0)
    } else {
        (0.0, 0.0)
    };
    let to_px = |p: V3| {
        (
            w as f32 / 2.0 + (dot(p, right) - cx) * scale,
            h as f32 / 2.0 - (dot(p, up) - cy) * scale,
            dot(p, eye),
        )
    };
    let diag = if has {
        dot(sub(hi, lo), sub(hi, lo)).sqrt()
    } else {
        1.0
    };
    // 光は jc-ui-view3d の FlatLight と同じ式と値: 主の光は視点の向きに世界の左上の向きを混ぜ、
    // 視点からの光・白いつや・平らな面の中の濃淡(手前が明るく奥が暗い)を足す
    let key = normalize([eye[0] * 0.8 - 0.3, eye[1] * 0.8 + 0.2, eye[2] * 0.8 + 0.6]);
    let half = normalize([key[0] + eye[0], key[1] + eye[1], key[2] + eye[2]]);
    let center = [0, 1, 2].map(|i| cx * right[i] + cy * up[i]);
    let dist = (0.5 * diag).max(1.0) * 2.4;
    let smooth = |x: f32| {
        let t = x.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let flat = |p: V3, n: V3| {
        let mut n = normalize(n);
        if dot(n, eye) < 0.0 {
            n = n.map(|v| -v);
        }
        let front = dot(n, eye).max(0.0);
        let shine = dot(n, half).max(0.0).powf(40.0) * 0.25;
        let d = sub(p, center);
        let along = dot(d, eye);
        let vv = normalize([0, 1, 2].map(|i| eye[i] * dist - (d[i] - eye[i] * along)));
        let rz = |v: V3| 2.0 * dot(n, v) * n[2] - v[2];
        let env = (smooth(rz(vv)) - smooth(rz(eye))) * 0.4;
        (
            0.42 + 0.38 * dot(n, key).max(0.0) + 0.25 * front + env,
            shine,
        )
    };
    let bias = 0.003 * diag;
    for (bi, b) in bodies.iter().enumerate() {
        let m = b.mesh;
        let color = body_tint(bi);
        let picked = mix(color, ACCENT, 0.55);
        let pts: Vec<(f32, f32, f32)> = m.positions.iter().map(|p| to_px(*p)).collect();
        // 白いつやは色の掛け算に直して入れる(画面の CPU の描画と同じ)
        let lum = ((0.299 * f32::from(color[0])
            + 0.587 * f32::from(color[1])
            + 0.114 * f32::from(color[2]))
            / 255.0)
            .max(0.05);
        let shade: Vec<f32> = m
            .normals
            .iter()
            .zip(&m.positions)
            .map(|(n, p)| {
                let (k, shine) = flat(*p, *n);
                k + shine / lum
            })
            .collect();
        for (t, face) in m.triangles.iter().zip(&m.triangle_faces) {
            let p = |i: usize| m.positions[t[i] as usize];
            // 裏を向いた三角形は描かない(閉じたボディなら表の面で隠れる)
            if dot(cross(sub(p(1), p(0)), sub(p(2), p(0))), eye) <= 0.0 {
                continue;
            }
            let name = m.face_names.get(*face as usize);
            let hit = o
                .faces
                .iter()
                .any(|f| f.feature == b.feature && Some(&f.face) == name);
            let v = [0, 1, 2].map(|i| {
                let q = pts[t[i] as usize];
                let k = shade.get(t[i] as usize).copied().unwrap_or(1.0);
                (q.0, q.1, q.2, k)
            });
            c.fill(v, if hit { picked } else { color });
        }
        for (ei, e) in m.edges.iter().enumerate() {
            let hit = o.edges.iter().any(|&(f, i)| f == b.feature && i == ei);
            let (col, width) = if hit {
                (ACCENT, 3 * ss)
            } else {
                (line_color(color), ss)
            };
            for s in e.windows(2) {
                let (a, d) = (to_px(s[0]), to_px(s[1]));
                c.line((a.0, a.1, a.2 + bias), (d.0, d.1, d.2 + bias), col, width);
            }
        }
    }
    // 2×2 の平均で縮める
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let mut sum = [0u32; 3];
            for dy in 0..ss {
                for dx in 0..ss {
                    let px = c.color[(y * ss + dy) * w + x * ss + dx];
                    for i in 0..3 {
                        sum[i] += px[i] as u32;
                    }
                }
            }
            let n = (ss * ss) as u32;
            rgb.extend(sum.map(|s| ((s + n / 2) / n) as u8));
        }
    }
    Picture {
        width,
        height,
        rgb,
        px_per_mm: (scale / ss as f32) as f64,
    }
}

impl Picture {
    /// PNG のバイト列(8 ビットの RGB)。
    pub fn png(&self) -> Vec<u8> {
        jc_format::png::write_rgb(self.width, self.height, &self.rgb)
    }

    /// [`Picture::png`] が書いた PNG(8 ビットの RGB、行の前処理なし、飛び越しなし)を読み戻す
    /// (版の縮小図を画面に出す)。ほかの形の PNG は `None`。`px_per_mm` は 0。
    pub fn from_png(bytes: &[u8]) -> Option<Picture> {
        let (width, height, rgb) = jc_format::png::read_rgb(bytes, MAX_SIZE * 2)?;
        Some(Picture {
            width,
            height,
            rgb,
            px_per_mm: 0.0,
        })
    }

    /// 点 (x, y) の色。
    pub fn at(&self, x: u32, y: u32) -> [u8; 3] {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }
}

#[cfg(test)]
mod tests;
