//! 速い描き方。三角形を画素(縦横 `supersample` 倍)に塗って、見えている三角形を覚え、画素ごとに
//! 光線と三角形の交わりから位置と法線を出して材質を塗る。環境の光は [`Env`] の式で近似し、
//! 主の光の影と、床の上の接地の陰は影の地図(上から見た奥行き)で落とす。縁の柔らかさは
//! 影の地図の周りの何点かの平均(PCF、Reeves 1987)で付ける。

use crate::env::{Env, env_brdf};
use crate::math::*;
use crate::scene::vertex_normal;
use crate::{Background, Camera, Image, RenderScene};

/// 描き方の指定。
#[derive(Debug, Clone, PartialEq)]
pub struct RasterOptions {
    pub width: u32,
    pub height: u32,
    /// 縦横この倍で描いて平均する(縁のぎざぎざを減らす。1〜4)
    pub supersample: u32,
    pub background: Background,
    /// 影を落とす
    pub shadows: bool,
    /// 床を描く(影が落ちる。透明の背景では描かない。映り込みは光線追跡だけ)
    pub ground: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        RasterOptions {
            width: 960,
            height: 720,
            supersample: 2,
            background: Background::Studio,
            shadows: true,
            ground: true,
        }
    }
}

/// 世界の三角形。
pub(crate) struct Tri {
    pub p: [V3; 3],
    pub n: [V3; 3],
    pub obj: u32,
}

/// 物の三角形を世界の三角形の並びにする(面積 0 の三角形は除く)。
pub(crate) fn triangles(scene: &RenderScene) -> Vec<Tri> {
    let mut out = Vec::with_capacity(scene.triangles());
    for (oi, o) in scene.objects.iter().enumerate() {
        let m = &o.mesh;
        for t in &m.triangles {
            let p = t.map(|i| m.positions[i as usize]);
            let g = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            if dot(g, g) < 1e-20 {
                continue;
            }
            let face = normalize(g);
            out.push(Tri {
                p,
                n: t.map(|i| vertex_normal(m, i, face)),
                obj: oi as u32,
            });
        }
    }
    out
}

/// 光線と三角形の交わり(距離、重心座標の b・c)。Möller–Trumbore。両面とも当たる。
pub(crate) fn hit_tri(o: V3, d: V3, t: &Tri) -> Option<(f32, f32, f32)> {
    let e1 = sub(t.p[1], t.p[0]);
    let e2 = sub(t.p[2], t.p[0]);
    let h = cross(d, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, t.p[0]);
    let u = dot(s, h) * inv;
    if !(-1e-4..=1.0 + 1e-4).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < -1e-4 || u + v > 1.0 + 1e-4 {
        return None;
    }
    let dist = dot(e2, q) * inv;
    (dist > 0.0).then_some((dist, u, v))
}

/// 交わった所の位置と、見る側へ向けた法線(なめらかな法線と三角形の向き)。
pub(crate) fn surface(t: &Tri, b: f32, c: f32, view: V3) -> (V3, V3, V3) {
    let a = 1.0 - b - c;
    let p = add(add(scale(t.p[0], a), scale(t.p[1], b)), scale(t.p[2], c));
    let mut g = normalize(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
    let mut n = normalize(add(
        add(scale(t.n[0], a), scale(t.n[1], b)),
        scale(t.n[2], c),
    ));
    if dot(g, view) < 0.0 {
        g = scale(g, -1.0);
    }
    if dot(n, g) < 0.0 {
        n = scale(n, -1.0);
    }
    (p, n, g)
}

/// 影の地図(光の向きから見た、いちばん光に近い面の高さ)。
pub(crate) struct ShadowMap {
    size: usize,
    /// 光の向きと、地図の横・縦の向き
    dir: V3,
    u: V3,
    v: V3,
    center: V3,
    /// 地図の半分の幅(mm)
    half: f32,
    depth: Vec<f32>,
}

impl ShadowMap {
    /// 向き `dir`(光へ向かう)から見た地図。`center`・`radius` の球と床の広がりを覆う。
    pub(crate) fn new(tris: &[Tri], dir: V3, center: V3, radius: f32, size: usize) -> ShadowMap {
        let (u, v) = onb(dir);
        let mut m = ShadowMap {
            size,
            dir,
            u,
            v,
            center,
            half: radius,
            depth: vec![f32::NEG_INFINITY; size * size],
        };
        for t in tris {
            let q = t.p.map(|p| m.to_map(p));
            let min_x = q
                .iter()
                .map(|q| q.0)
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(0.0) as usize;
            let max_x = q
                .iter()
                .map(|q| q.0)
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .min(size as f32 - 1.0);
            let min_y = q
                .iter()
                .map(|q| q.1)
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(0.0) as usize;
            let max_y = q
                .iter()
                .map(|q| q.1)
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .min(size as f32 - 1.0);
            if max_x < 0.0 || max_y < 0.0 {
                continue;
            }
            let area = edge(q[0], q[1], q[2]);
            if area.abs() < 1e-12 {
                continue;
            }
            for y in min_y..=max_y as usize {
                for x in min_x..=max_x as usize {
                    let s = (x as f32 + 0.5, y as f32 + 0.5, 0.0);
                    let w0 = edge(q[1], q[2], s) / area;
                    let w1 = edge(q[2], q[0], s) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                        continue;
                    }
                    let d = w0 * q[0].2 + w1 * q[1].2 + w2 * q[2].2;
                    let k = y * size + x;
                    if d > m.depth[k] {
                        m.depth[k] = d;
                    }
                }
            }
        }
        m
    }

    /// 地図の上の位置(画素)と高さ。
    fn to_map(&self, p: V3) -> (f32, f32, f32) {
        let d = sub(p, self.center);
        let k = self.size as f32 / (2.0 * self.half);
        (
            (dot(d, self.u) + self.half) * k,
            (dot(d, self.v) + self.half) * k,
            dot(d, self.dir),
        )
    }

    /// 1 画素の幅(mm)。
    pub(crate) fn texel(&self) -> f32 {
        2.0 * self.half / self.size as f32
    }

    /// 点 `p`(法線 `n`)が光に当たる割合(0〜1)。周りの `radius` mm の何点かの平均。
    pub(crate) fn visibility(&self, p: V3, n: V3, radius: f32, rot: f32) -> f32 {
        let tx = self.texel();
        // 法線の向きに少し浮かせて、自分の面で影にならないようにする
        let p = madd(p, n, tx * 1.5);
        let (x, y, d) = self.to_map(p);
        let r = (radius / tx).max(1.0);
        let (s, c) = rot.sin_cos();
        let mut lit = 0.0;
        for (px, py) in POISSON {
            let ox = (px * c - py * s) * r;
            let oy = (px * s + py * c) * r;
            let xi = (x + ox).floor();
            let yi = (y + oy).floor();
            if xi < 0.0 || yi < 0.0 || xi >= self.size as f32 || yi >= self.size as f32 {
                lit += 1.0;
                continue;
            }
            let stored = self.depth[yi as usize * self.size + xi as usize];
            if d >= stored - tx * 1.0 {
                lit += 1.0;
            }
        }
        lit / POISSON.len() as f32
    }
}

/// 円の中に散らした 16 点。
const POISSON: [(f32, f32); 16] = [
    (-0.942, -0.399),
    (0.946, -0.769),
    (-0.094, -0.929),
    (0.345, 0.294),
    (-0.916, 0.458),
    (-0.815, -0.879),
    (-0.383, 0.277),
    (0.975, 0.756),
    (0.443, -0.975),
    (0.538, -0.474),
    (-0.265, -0.419),
    (0.792, 0.191),
    (-0.242, 0.997),
    (-0.814, 0.914),
    (0.200, 0.786),
    (0.144, -0.141),
];

fn edge(a: (f32, f32, f32), b: (f32, f32, f32), c: (f32, f32, f32)) -> f32 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// 影と床の陰を出すのに使う、描く物の大きさ。
pub(crate) struct Stage {
    pub center: V3,
    pub radius: f32,
    pub floor_z: f32,
    pub key: Option<ShadowMap>,
    pub sky: Option<ShadowMap>,
}

impl Stage {
    pub(crate) fn new(scene: &RenderScene, tris: &[Tri], env: &Env, shadows: bool) -> Stage {
        let (min, max) = scene.bounds();
        let center = scale(add(min, max), 0.5);
        let radius = (len(sub(max, min)) * 0.5).max(1.0);
        let floor_z = min[2];
        let (key, sky) = if shadows && !tris.is_empty() {
            // 主の光の影は低い光で長く伸びるので、地図を広めにとる
            let reach = radius * (1.0 + (1.0 / env.key().dir[2].max(0.25)).min(3.0));
            (
                Some(ShadowMap::new(tris, env.key().dir, center, reach, 2048)),
                Some(ShadowMap::new(
                    tris,
                    [0.0, 0.0, 1.0],
                    center,
                    radius * 2.5,
                    512,
                )),
            )
        } else {
            (None, None)
        };
        Stage {
            center,
            radius,
            floor_z,
            key,
            sky,
        }
    }

    /// 主の光に当たる割合。
    pub(crate) fn key_vis(&self, p: V3, n: V3, rot: f32) -> f32 {
        self.key
            .as_ref()
            .map_or(1.0, |m| m.visibility(p, n, self.radius * 0.012, rot))
    }

    /// 床の点が空に開いている割合(物の下と周りは暗くなる。接地の陰)。
    pub(crate) fn floor_sky(&self, p: V3, rot: f32) -> f32 {
        self.sky.as_ref().map_or(1.0, |m| {
            let near = m.visibility(p, [0.0, 0.0, 1.0], self.radius * 0.06, rot);
            let wide = m.visibility(p, [0.0, 0.0, 1.0], self.radius * 0.25, rot + 1.3);
            0.25 + 0.75 * (0.55 * near + 0.45 * wide)
        })
    }
}

/// 物の面の色(線形。トーンマップの前)。`view` は面から視点へ向かう向き、`key_vis` は主の光に
/// 当たる割合(影を見ない時は 1)。画面の CPU の描き方も使う。
pub fn shade_surface(env: &Env, look: &crate::Look, p: V3, n: V3, view: V3, key_vis: f32) -> V3 {
    let (albedo, metal) = look.surface_at(p, n);
    let rough = look.roughness();
    let ndv = dot(n, view).max(1e-4);
    let r = sub(scale(n, 2.0 * dot(n, view)), view);
    let f0 = lerp([0.04; 3], albedo, metal);
    let diffuse_color = scale(albedo, 1.0 - metal);
    let diff = add(
        add(env.diffuse(n, true), env.lamp_diffuse(p, n)),
        scale(env.diffuse_light(env.key(), n), key_vis),
    );
    let (a, b) = env_brdf(rough, ndv);
    // 影の中は映り込みも少し弱める(光源の映り込みは影を見ないので、影の中で光らないように)
    let spec = scale(
        mul(env.specular(r, rough), add(scale(f0, a), [b; 3])),
        1.0 - 0.4 * (1.0 - key_vis),
    );
    add(mul(diffuse_color, diff), spec)
}

/// 床の色(線形)。`sky` は空に開いている割合、`key` は主の光に当たる割合。
pub(crate) fn shade_floor(env: &Env, sky: f32, key: f32) -> V3 {
    let up = [0.0, 0.0, 1.0];
    let light = add(
        scale(env.diffuse(up, true), sky),
        scale(env.diffuse_light(env.key(), up), key),
    );
    mul(env.floor_albedo, light)
}

/// 描く。
pub fn render(scene: &RenderScene, cam: &Camera, env: &Env, opt: &RasterOptions) -> Image {
    let started = std::time::Instant::now();
    let env = &{
        let mut e = env.clone();
        e.lamps = scene.lamps.clone();
        e
    };
    let w = opt.width.clamp(1, crate::MAX_SIZE) as usize;
    let h = opt.height.clamp(1, crate::MAX_SIZE) as usize;
    let ss = opt.supersample.clamp(1, 4) as usize;
    let (sw, sh) = (w * ss, h * ss);
    let aspect = w as f32 / h as f32;
    let tris = triangles(scene);
    let stage = Stage::new(scene, &tris, env, opt.shadows);
    let staged = started.elapsed().as_secs_f64() * 1000.0;

    // 1. 見えている三角形を覚える(奥行きは小さいほど手前)
    let mut depth = vec![f32::INFINITY; sw * sh];
    let mut id = vec![u32::MAX; sw * sh];
    for (ti, t) in tris.iter().enumerate() {
        let Some(q) = project3(cam, t, aspect, sw, sh) else {
            continue;
        };
        let area = edge(q[0], q[1], q[2]);
        if area.abs() < 1e-12 {
            continue;
        }
        let min_x = q
            .iter()
            .map(|q| q.0)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_x = q
            .iter()
            .map(|q| q.0)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil();
        let min_y = q
            .iter()
            .map(|q| q.1)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_y = q
            .iter()
            .map(|q| q.1)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil();
        if max_x < 0.0 || max_y < 0.0 {
            continue;
        }
        let max_x = (max_x as usize).min(sw - 1);
        let max_y = (max_y as usize).min(sh - 1);
        // 透視投影では 1/奥行き が画面の上で直線に変わる
        let key = |z: f32| if cam.perspective() { 1.0 / z } else { -z };
        let kz = q.map(|q| key(q.2));
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let s = (x as f32 + 0.5, y as f32 + 0.5, 0.0);
                let w0 = edge(q[1], q[2], s) / area;
                let w1 = edge(q[2], q[0], s) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-6 || w1 < -1e-6 || w2 < -1e-6 {
                    continue;
                }
                let k = w0 * kz[0] + w1 * kz[1] + w2 * kz[2];
                let z = if cam.perspective() { 1.0 / k } else { -k };
                let i = y * sw + x;
                if z < depth[i] {
                    depth[i] = z;
                    id[i] = ti as u32;
                }
            }
        }
    }

    let rastered = started.elapsed().as_secs_f64() * 1000.0;
    // 2. 画素ごとに塗る
    let mut out = vec![[0u8; 4]; w * h];
    crate::par_rows(&mut out, w, |y, row| {
        for (x, px) in row.iter_mut().enumerate() {
            let mut acc = [0.0f32; 3];
            let mut alpha = 0.0;
            // 床と背景はなめらかなので、1 画素で 1 回だけ塗って使い回す(影の点を何度も見ると重い)
            let mut miss: Option<(V3, f32)> = None;
            // 縁でない画素(どの点も同じ三角形)は、真ん中で 1 回だけ塗る
            let first = id[(y * ss) * sw + x * ss];
            let inner = first != u32::MAX
                && (0..ss)
                    .all(|sy| (0..ss).all(|sx| id[(y * ss + sy) * sw + x * ss + sx] == first));
            if inner {
                let u = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
                let v = 1.0 - (y as f32 + 0.5) / h as f32 * 2.0;
                let (o, d) = cam.ray(u, v, aspect);
                let rot = hash_angle(x * ss, y * ss);
                let (c, _) = shade_ray(
                    scene,
                    &tris,
                    env,
                    &stage,
                    opt.background,
                    opt.ground,
                    o,
                    d,
                    first,
                    rot,
                );
                let c = tonemap(c);
                *px = [
                    linear_to_srgb(c[0]),
                    linear_to_srgb(c[1]),
                    linear_to_srgb(c[2]),
                    255,
                ];
                continue;
            }
            for sy in 0..ss {
                for sx in 0..ss {
                    let gx = x * ss + sx;
                    let gy = y * ss + sy;
                    let u = (gx as f32 + 0.5) / sw as f32 * 2.0 - 1.0;
                    let v = 1.0 - (gy as f32 + 0.5) / sh as f32 * 2.0;
                    let (o, d) = cam.ray(u, v, aspect);
                    let rot = hash_angle(gx, gy);
                    let ti = id[gy * sw + gx];
                    let (c, a) = if ti == u32::MAX {
                        *miss.get_or_insert_with(|| {
                            let (c, a) = shade_ray(
                                scene,
                                &tris,
                                env,
                                &stage,
                                opt.background,
                                opt.ground,
                                o,
                                d,
                                ti,
                                rot,
                            );
                            (tonemap(c), a)
                        })
                    } else {
                        let (c, a) = shade_ray(
                            scene,
                            &tris,
                            env,
                            &stage,
                            opt.background,
                            opt.ground,
                            o,
                            d,
                            ti,
                            rot,
                        );
                        (tonemap(c), a)
                    };
                    acc = add(acc, c);
                    alpha += a;
                }
            }
            let n = (ss * ss) as f32;
            let c = scale(acc, 1.0 / n);
            let a = alpha / n;
            *px = crate::finish_pixel(c, a, opt.background);
        }
    });
    log::debug!(
        "写実の絵(速い描き方) {w}×{h}・{ss} 倍・三角形 {}: {:.0} ms(影の地図まで {staged:.0}、塗る前まで {rastered:.0})",
        tris.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    Image {
        width: w as u32,
        height: h as u32,
        rgba: out.into_iter().flatten().collect(),
    }
}

/// 画素ごとに変える影の点の回し方(縞を散らす)。
fn hash_angle(x: usize, y: usize) -> f32 {
    let mut h = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h & 0xffff) as f32 / 65535.0 * std::f32::consts::TAU
}

fn project3(
    cam: &Camera,
    t: &Tri,
    aspect: f32,
    sw: usize,
    sh: usize,
) -> Option<[(f32, f32, f32); 3]> {
    let mut q = [(0.0, 0.0, 0.0); 3];
    for (out, p) in q.iter_mut().zip(t.p) {
        let (x, y, z) = cam.project(p, aspect)?;
        *out = ((x * 0.5 + 0.5) * sw as f32, (0.5 - y * 0.5) * sh as f32, z);
    }
    Some(q)
}

/// 1 本の光線の色(線形)と不透明さ。`ti` は見えている三角形(無ければ `u32::MAX`)。
#[allow(clippy::too_many_arguments)]
fn shade_ray(
    scene: &RenderScene,
    tris: &[Tri],
    env: &Env,
    stage: &Stage,
    bg: Background,
    ground: bool,
    o: V3,
    d: V3,
    ti: u32,
    rot: f32,
) -> (V3, f32) {
    let view = scale(d, -1.0);
    if let Some(t) = tris.get(ti as usize)
        && let Some((_, b, c)) = hit_tri(o, d, t).or_else(|| {
            // 縁の画素は光線が三角形を少し外れる。三角形の平面との交わりで代える
            plane_hit(o, d, t)
        })
    {
        let (p, n, _) = surface(t, b, c, view);
        let look = &scene.objects[t.obj as usize].look;
        let key = stage.key_vis(p, n, rot);
        return (shade_surface(env, look, p, n, view, key), 1.0);
    }
    if bg == Background::Transparent {
        return ([0.0; 3], 0.0);
    }
    if ground && d[2] < -1e-6 {
        let k = (stage.floor_z - o[2]) / d[2];
        if k > 0.0 {
            let q = madd(o, d, k);
            let sky = stage.floor_sky(q, rot);
            let key = stage.key_vis(q, [0.0, 0.0, 1.0], rot);
            let floor = shade_floor(env, sky, key);
            if let Background::Color(_) = bg {
                // 単色の背景: 床は影だけを受ける(影の濃さを不透明さにして、仕上げで背景の色に重ねる)
                let lum = |c: V3| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                let open = lum(shade_floor(env, 1.0, 1.0)).max(1e-6);
                let far = len(sub(
                    [q[0], q[1], 0.0],
                    [stage.center[0], stage.center[1], 0.0],
                ));
                let t = smoothstep(stage.radius * 2.0, stage.radius * 7.0, far);
                let shadow = (1.0 - lum(floor) / open).clamp(0.0, 1.0);
                return ([0.0; 3], shadow * (1.0 - t));
            }
            // 遠くの床は背景へ溶かす(スタジオの床と背景のつなぎ目を見せない)
            let far = len(sub(
                [q[0], q[1], 0.0],
                [stage.center[0], stage.center[1], 0.0],
            ));
            let t = smoothstep(stage.radius * 2.0, stage.radius * 7.0, far);
            if bg.see_through() {
                // 単色の背景: 遠くの床は透かして、仕上げで重ねる色へ溶かす
                return (scale(floor, 1.0 - t), 1.0 - t);
            }
            return (lerp(floor, env.backdrop(d), t), 1.0);
        }
    }
    if bg.see_through() {
        return ([0.0; 3], 0.0);
    }
    (scale(env.backdrop(d), env.exposure), 1.0)
}

/// 光線と三角形の平面の交わり(重心座標は外れていてもよい)。
fn plane_hit(o: V3, d: V3, t: &Tri) -> Option<(f32, f32, f32)> {
    let e1 = sub(t.p[1], t.p[0]);
    let e2 = sub(t.p[2], t.p[0]);
    let n = cross(e1, e2);
    let den = dot(n, d);
    if den.abs() < 1e-12 {
        return None;
    }
    let dist = dot(n, sub(t.p[0], o)) / den;
    let p = madd(o, d, dist);
    // 重心座標
    let v2 = sub(p, t.p[0]);
    let d00 = dot(e1, e1);
    let d01 = dot(e1, e2);
    let d11 = dot(e2, e2);
    let d20 = dot(v2, e1);
    let d21 = dot(v2, e2);
    let den = d00 * d11 - d01 * d01;
    if den.abs() < 1e-20 {
        return None;
    }
    let b = ((d11 * d20 - d01 * d21) / den).clamp(0.0, 1.0);
    let c = ((d00 * d21 - d01 * d20) / den).clamp(0.0, 1.0 - b);
    Some((dist, b, c))
}
