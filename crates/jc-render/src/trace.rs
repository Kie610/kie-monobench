//! 光線追跡(経路追跡)。画素ごとに光の通り道を視点からたどり、回す(パス)ほどきれいになる。
//!
//! - 材質: 拡散(Lambert)と、粗さのある映り込み(GGX の微小面。Walter ほか 2007)を、金属の度合いと
//!   フレネル(Schlick の近似)で混ぜる。
//! - 光源: 環境のソフトボックスを直接たどり(NEE)、材質の向きに選んだ光線が光源に当たった分と
//!   多重重要度サンプリング(MIS、Veach 1997 のバランスの式)で混ぜる。背景の濃淡は材質の向きで拾う。
//! - 深さ 3 からはロシアンルーレットで打ち切る。
//!
//! 1 回目に当たった所の色・法線・奥行きを残し、[`crate::denoise`] がノイズを減らすのに使う。

use crate::bvh::Bvh;
use crate::env::{Env, Light};
use crate::math::*;
use crate::raster::{Tri, surface, triangles};
use crate::{Background, Camera, Ground, Image, Look, RenderScene};

/// 光線追跡の指定。
#[derive(Debug, Clone, PartialEq)]
pub struct TraceOptions {
    pub width: u32,
    pub height: u32,
    pub background: Background,
    /// 床(出す・映り込み。透明の背景では描かない)
    pub ground: Ground,
    /// 跳ね返りの上限
    pub bounces: u32,
}

impl Default for TraceOptions {
    fn default() -> Self {
        TraceOptions {
            width: 960,
            height: 720,
            background: Background::Studio,
            ground: Ground::default(),
            bounces: 5,
        }
    }
}

/// 1 回目に当たった所の情報(ノイズを減らす時の手がかり)。
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Aov {
    pub albedo: V3,
    pub normal: V3,
    pub depth: f32,
}

/// 進むほどきれいになる光線追跡。
pub struct Tracer {
    tris: Vec<Tri>,
    looks: Vec<Look>,
    bvh: Bvh,
    cam: Camera,
    env: Env,
    opt: TraceOptions,
    floor_z: f32,
    center: V3,
    radius: f32,
    /// 色の和(線形)と、不透明さの和
    sum: Vec<[f32; 4]>,
    aov: Vec<Aov>,
    /// 回した数
    pub passes: u32,
}

/// 床(見えない地面)の材質の印。
const FLOOR: u32 = u32::MAX;

impl Tracer {
    pub fn new(scene: &RenderScene, cam: Camera, mut env: Env, opt: TraceOptions) -> Tracer {
        let started = std::time::Instant::now();
        env.lamps = scene.lamps.clone();
        let tris = triangles(scene);
        let bvh = Bvh::build(&tris);
        let (min, max) = scene.bounds();
        log::debug!(
            "光線追跡の用意: 三角形 {}、箱の木 {:.0} ms",
            tris.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        let w = opt.width.clamp(1, crate::MAX_SIZE) as usize;
        let h = opt.height.clamp(1, crate::MAX_SIZE) as usize;
        Tracer {
            looks: scene.objects.iter().map(|o| o.look.clone()).collect(),
            tris,
            bvh,
            cam,
            env,
            floor_z: min[2],
            center: scale(add(min, max), 0.5),
            radius: (len(sub(max, min)) * 0.5).max(1.0),
            sum: vec![[0.0; 4]; w * h],
            aov: vec![Aov::default(); w * h],
            passes: 0,
            opt: TraceOptions {
                width: w as u32,
                height: h as u32,
                ..opt
            },
        }
    }

    pub fn width(&self) -> u32 {
        self.opt.width
    }

    pub fn height(&self) -> u32 {
        self.opt.height
    }

    /// 1 画素につき 1 本ずつ足す。
    pub fn pass(&mut self) {
        let w = self.opt.width as usize;
        let h = self.opt.height as usize;
        let aspect = w as f32 / h as f32;
        let pass = self.passes;
        let mut rows: Vec<([f32; 4], Aov)> = self
            .sum
            .iter()
            .zip(&self.aov)
            .map(|(s, a)| (*s, *a))
            .collect();
        let me = &*self;
        crate::par_rows(&mut rows, w, |y, row| {
            for (x, (s, a)) in row.iter_mut().enumerate() {
                let seed = ((pass as u64) << 40) ^ ((y as u64) << 20) ^ x as u64;
                let mut rng = Rng::new(seed);
                let u = (x as f32 + rng.f()) / w as f32 * 2.0 - 1.0;
                let v = 1.0 - (y as f32 + rng.f()) / h as f32 * 2.0;
                let lens = [rng.f(), rng.f()];
                let (o, d) = me.cam.ray_lens(u, v, aspect, lens);
                let (c, alpha, aov) = me.path(o, d, &mut rng);
                for k in 0..3 {
                    s[k] += c[k];
                }
                s[3] += alpha;
                if pass == 0 {
                    *a = aov;
                } else {
                    // 手がかりも回すたびに平均する(縁でなめらかになる)
                    let k = 1.0 / (pass as f32 + 1.0);
                    a.albedo = lerp(a.albedo, aov.albedo, k);
                    a.normal = lerp(a.normal, aov.normal, k);
                    a.depth += (aov.depth - a.depth) * k;
                }
            }
        });
        for (i, (s, a)) in rows.into_iter().enumerate() {
            self.sum[i] = s;
            self.aov[i] = a;
        }
        self.passes += 1;
    }

    /// 今の絵。`denoise` ならノイズを減らす。
    pub fn image(&self, denoise: bool) -> Image {
        let n = self.passes.max(1) as f32;
        let w = self.opt.width as usize;
        let h = self.opt.height as usize;
        let mut color: Vec<V3> = self
            .sum
            .iter()
            .map(|s| [s[0] / n, s[1] / n, s[2] / n])
            .collect();
        let alpha: Vec<f32> = self.sum.iter().map(|s| s[3] / n).collect();
        if denoise && self.passes > 0 {
            color = crate::denoise::atrous(&color, &self.aov, w, h, self.passes);
        }
        let rgba = color
            .iter()
            .zip(&alpha)
            .flat_map(|(c, a)| crate::finish_pixel(tonemap(*c), *a, self.opt.background))
            .collect();
        Image {
            width: w as u32,
            height: h as u32,
            rgba,
        }
    }

    /// 近い交わり(距離、材質の番号(床は FLOOR)、位置、法線、三角形の向き)。
    fn intersect(&self, o: V3, d: V3) -> Option<(f32, u32, V3, V3, V3)> {
        let view = scale(d, -1.0);
        let tri = self.bvh.hit(&self.tris, o, d, f32::INFINITY);
        let floor = if self.opt.background != Background::Transparent
            && self.opt.ground.show
            && d[2] < -1e-7
        {
            let k = (self.floor_z - o[2]) / d[2];
            (k > 1e-4).then_some(k)
        } else {
            None
        };
        match (tri, floor) {
            (Some((t, ti, b, c)), f) if f.is_none_or(|f| t < f) => {
                let tr = &self.tris[ti as usize];
                let (p, n, g) = surface(tr, b, c, view);
                Some((t, tr.obj, p, n, g))
            }
            (_, Some(f)) => {
                let up = [0.0, 0.0, 1.0];
                Some((f, FLOOR, madd(o, d, f), up, up))
            }
            _ => None,
        }
    }

    /// 1 本の光の通り道の色と不透明さ、1 回目の手がかり。
    fn path(&self, o: V3, d: V3, rng: &mut Rng) -> (V3, f32, Aov) {
        let mut aov = Aov {
            albedo: [1.0; 3],
            normal: [0.0; 3],
            depth: 1e9,
        };
        let mut color = [0.0f32; 3];
        let mut beta = [1.0f32; 3];
        let first = d;
        let (mut o, mut d) = (o, d);
        // 前の跳ね返りで材質の向きに選んだ確率密度(光源に当たった時の MIS 用。視点からは None)
        let mut last_pdf: Option<f32> = None;
        let mut fade = 0.0;
        for depth in 0..self.opt.bounces.max(1) {
            let Some((t, mat, p, n, g)) = self.intersect(o, d) else {
                // 何にも当たらない: 背景と光源
                if depth == 0 {
                    // 透明と単色は透かす(単色は絵を仕上げる時に重ねる)
                    if self.opt.background.see_through() {
                        return ([0.0; 3], 0.0, aov);
                    }
                    // 視点から直接見える所は、光源の円盤を描かない(背景だけ)
                    color = add(
                        color,
                        mul(beta, scale(self.env.backdrop(d), self.env.exposure)),
                    );
                    break;
                }
                let mut l = scale(self.env.backdrop(d), self.env.exposure);
                for li in &self.env.lights {
                    if dot(d, li.dir) >= li.cos_half {
                        let w = match last_pdf {
                            Some(pb) => {
                                let pl = self.light_pdf(li);
                                pb / (pb + pl)
                            }
                            None => 1.0,
                        };
                        l = add(l, scale(li.radiance, w * self.env.exposure));
                    }
                }
                color = add(color, mul(beta, l));
                break;
            };
            if depth == 0 && mat == FLOOR && matches!(self.opt.background, Background::Color(_)) {
                return self.shadow_catcher(p, g, rng, aov);
            }
            let view = scale(d, -1.0);
            let (albedo, metal, rough) = if mat == FLOOR {
                let g = &self.opt.ground;
                if g.reflect {
                    // 映り込む床: 磨いた石のように、色は床のまま粗さだけ下げる
                    (self.env.floor_albedo, 0.0, g.roughness.clamp(0.02, 1.0))
                } else {
                    (self.env.floor_albedo, 0.0, 1.0)
                }
            } else {
                let lk = &self.looks[mat as usize];
                let (albedo, metal) = lk.surface_at(p, n);
                (albedo, metal, lk.roughness())
            };
            if depth == 0 {
                aov = Aov {
                    albedo: albedo.map(|v| v.max(0.02)),
                    normal: n,
                    depth: t,
                };
                if mat == FLOOR {
                    // 遠くの床は背景へ溶かす(速い描き方と同じ)
                    let far = len(sub(
                        [p[0], p[1], 0.0],
                        [self.center[0], self.center[1], 0.0],
                    ));
                    fade = smoothstep(self.radius * 2.0, self.radius * 7.0, far);
                }
            }
            let bsdf = Bsdf::new(albedo, metal, rough, n, view);
            let po = madd(p, g, self.radius * 1e-4 + 1e-3);
            // 光源を直接たどる(NEE)
            for li in &self.env.lights {
                let wi = sample_cone(li, rng);
                let ndl = dot(n, wi);
                if ndl <= 0.0 || dot(g, wi) <= 0.0 {
                    continue;
                }
                if self.bvh.occluded(&self.tris, po, wi, f32::INFINITY) {
                    continue;
                }
                let (f, pb) = bsdf.eval(wi);
                let pl = self.light_pdf(li);
                let w = pl / (pl + pb);
                let contrib = scale(mul(f, li.radiance), w * self.env.exposure / pl);
                color = add(
                    color,
                    mul(
                        beta,
                        clamp_lum(contrib, if depth == 0 { 1e9 } else { 30.0 }),
                    ),
                );
            }
            // 点光源とスポットライトを直接たどる(大きさのある光源は球の中の点を選び、影の縁をぼかす)
            for lp in &self.env.lamps {
                let at = madd(lp.pos, ball(rng), lp.radius);
                let to = sub(at, po);
                let d2 = dot(to, to).max(1e-6);
                let dist = d2.sqrt();
                let wi = scale(to, 1.0 / dist);
                if dot(n, wi) <= 0.0 || dot(g, wi) <= 0.0 {
                    continue;
                }
                let spot = lp.spot(scale(wi, -1.0));
                if spot <= 0.0 || self.bvh.occluded(&self.tris, po, wi, dist) {
                    continue;
                }
                let (f, _) = bsdf.eval(wi);
                let k = spot * self.env.lamp_exposure / d2.max(lp.radius * lp.radius);
                let contrib = scale(mul(f, lp.power), k);
                color = add(
                    color,
                    mul(
                        beta,
                        clamp_lum(contrib, if depth == 0 { 1e9 } else { 30.0 }),
                    ),
                );
            }
            // 材質の向きに次の光線を選ぶ
            let Some((wi, f, pdf)) = bsdf.sample(rng) else {
                break;
            };
            if pdf <= 1e-8 || dot(g, wi) <= 0.0 {
                break;
            }
            beta = mul(beta, scale(f, 1.0 / pdf));
            last_pdf = Some(pdf);
            if depth >= 2 {
                let q = max_of(beta).clamp(0.05, 0.95);
                if rng.f() > q {
                    break;
                }
                beta = scale(beta, 1.0 / q);
            }
            o = po;
            d = wi;
        }
        if fade > 0.0 {
            if self.opt.background.see_through() {
                // 単色の背景では、遠くの床は透かして背景の色へ溶かす
                return (scale(color, 1.0 - fade), 1.0 - fade, aov);
            }
            color = lerp(
                color,
                scale(self.env.backdrop(first), self.env.exposure),
                fade,
            );
        }
        (color, 1.0, aov)
    }

    /// 単色の背景の床(影だけを受ける床。床そのものは背景の色に溶け、
    /// 影の所だけ暗くなる)。光源ごとと空の拡散の光の重みで、届いた割合を 1 本ずつ見る。返す色は黒で、
    /// 不透明さが影の濃さ(仕上げで背景の色に重ねる)。
    fn shadow_catcher(&self, p: V3, g: V3, rng: &mut Rng, aov: Aov) -> (V3, f32, Aov) {
        let up = [0.0, 0.0, 1.0];
        let po = madd(p, g, self.radius * 1e-4 + 1e-3);
        let lum = |c: V3| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        // 1 回に光源ごと・空ごとに 4 本ずつ見る(不透明さはノイズ取りが効かないので、回す数の割に
        // ざらつかないように)。空は物の大きさほどの近くだけを見る(遠くの物で床全体を暗くしない)
        const N: usize = 4;
        let w_sky = lum(self.env.diffuse_backdrop(up));
        let mut lit = 0.0;
        for _ in 0..N {
            let (u, v) = (rng.f(), rng.f());
            let r = u.sqrt();
            let phi = std::f32::consts::TAU * v;
            let dir = [r * phi.cos(), r * phi.sin(), (1.0 - u).max(0.0).sqrt()];
            if !self.bvh.occluded(&self.tris, po, dir, self.radius) {
                lit += w_sky / N as f32;
            }
        }
        let mut total = w_sky;
        for li in &self.env.lights {
            let w = lum(self.env.diffuse_light(li, up));
            total += w;
            for _ in 0..N {
                let wi = sample_cone(li, rng);
                if wi[2] > 0.0 && !self.bvh.occluded(&self.tris, po, wi, f32::INFINITY) {
                    lit += w / N as f32;
                }
            }
        }
        for lp in &self.env.lamps {
            let w = lum(scale(lp.white_at(po, up), self.env.lamp_exposure));
            if w <= 0.0 {
                continue;
            }
            total += w;
            for _ in 0..N {
                let at = madd(lp.pos, ball(rng), lp.radius);
                let to = sub(at, po);
                let dist = len(to);
                if dist > 1e-6
                    && !self
                        .bvh
                        .occluded(&self.tris, po, scale(to, 1.0 / dist), dist)
                {
                    lit += w / N as f32;
                }
            }
        }
        let shadow = if total > 0.0 { 1.0 - lit / total } else { 0.0 };
        // 遠くの床の影は薄める(物から離れた所に縞を出さない)
        let far = len(sub(
            [p[0], p[1], 0.0],
            [self.center[0], self.center[1], 0.0],
        ));
        let fade = smoothstep(self.radius * 2.0, self.radius * 7.0, far);
        ([0.0; 3], shadow * (1.0 - fade), aov)
    }

    fn light_pdf(&self, l: &Light) -> f32 {
        1.0 / l.solid_angle().max(1e-9)
    }
}

/// 寄与の明るさを `max` までに抑える(まれな光の通り道の白い点を出さない)。
fn clamp_lum(c: V3, max: f32) -> V3 {
    let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    if l > max { scale(c, max / l) } else { c }
}

/// 半径 1 の球の中の点を一様に選ぶ。
fn ball(rng: &mut Rng) -> V3 {
    loop {
        let p = [
            rng.f() * 2.0 - 1.0,
            rng.f() * 2.0 - 1.0,
            rng.f() * 2.0 - 1.0,
        ];
        if dot(p, p) <= 1.0 {
            return p;
        }
    }
}

/// 光源の円盤の中の向きを一様に選ぶ。
fn sample_cone(l: &Light, rng: &mut Rng) -> V3 {
    let u = rng.f();
    let v = rng.f();
    let cos = 1.0 - u * (1.0 - l.cos_half);
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    let phi = std::f32::consts::TAU * v;
    to_world([sin * phi.cos(), sin * phi.sin(), cos], l.dir)
}

/// 1 か所の材質(拡散と GGX の映り込み)。
struct Bsdf {
    albedo: V3,
    metal: f32,
    alpha: f32,
    n: V3,
    v: V3,
    f0: V3,
    /// 映り込みを選ぶ確率
    p_spec: f32,
}

impl Bsdf {
    fn new(albedo: V3, metal: f32, rough: f32, n: V3, v: V3) -> Bsdf {
        let f0 = lerp([0.04; 3], albedo, metal);
        let ndv = dot(n, v).max(1e-4);
        let fv = fresnel(f0, ndv);
        let lum = |c: V3| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let spec = lum(fv);
        let diff = lum(albedo) * (1.0 - metal);
        let p_spec = if diff + spec > 0.0 {
            (spec / (diff + spec)).clamp(0.1, 0.95)
        } else {
            0.5
        };
        Bsdf {
            albedo,
            metal,
            alpha: (rough * rough).max(1e-3),
            n,
            v,
            f0,
            p_spec: if metal >= 0.999 { 1.0 } else { p_spec },
        }
    }

    /// 向き `l` の値(cos を掛けた BSDF)と、その向きを選ぶ確率密度。
    fn eval(&self, l: V3) -> (V3, f32) {
        let n = self.n;
        let ndl = dot(n, l);
        let ndv = dot(n, self.v).max(1e-4);
        if ndl <= 0.0 {
            return ([0.0; 3], 0.0);
        }
        let h = normalize(add(l, self.v));
        let ndh = dot(n, h).max(0.0);
        let vdh = dot(self.v, h).max(1e-4);
        let d = ggx_d(ndh, self.alpha);
        let gs = smith_g(ndl, ndv, self.alpha);
        let f = fresnel(self.f0, vdh);
        let spec = scale(f, d * gs / (4.0 * ndv));
        let kd = scale(sub([1.0; 3], f), 1.0 - self.metal);
        let diff = scale(mul(kd, self.albedo), ndl / std::f32::consts::PI);
        let pdf_spec = d * ndh / (4.0 * vdh);
        let pdf_diff = ndl / std::f32::consts::PI;
        let pdf = self.p_spec * pdf_spec + (1.0 - self.p_spec) * pdf_diff;
        (add(spec, diff), pdf)
    }

    /// 次の向きを選ぶ(向き、cos を掛けた値、確率密度)。
    fn sample(&self, rng: &mut Rng) -> Option<(V3, V3, f32)> {
        let l = if rng.f() < self.p_spec {
            // GGX の分布から半分の向きを選んで映す
            let u = rng.f();
            let v = rng.f();
            let a2 = self.alpha * self.alpha;
            let cos = ((1.0 - u) / (1.0 + (a2 - 1.0) * u)).max(0.0).sqrt();
            let sin = (1.0 - cos * cos).max(0.0).sqrt();
            let phi = std::f32::consts::TAU * v;
            let h = to_world([sin * phi.cos(), sin * phi.sin(), cos], self.n);
            sub(scale(h, 2.0 * dot(self.v, h)), self.v)
        } else {
            let u = rng.f();
            let v = rng.f();
            let r = u.sqrt();
            let phi = std::f32::consts::TAU * v;
            to_world(
                [r * phi.cos(), r * phi.sin(), (1.0 - u).max(0.0).sqrt()],
                self.n,
            )
        };
        let l = normalize(l);
        let (f, pdf) = self.eval(l);
        (pdf > 0.0).then_some((l, f, pdf))
    }
}

fn fresnel(f0: V3, c: f32) -> V3 {
    let k = (1.0 - c).clamp(0.0, 1.0).powi(5);
    [0, 1, 2].map(|i| f0[i] + (1.0 - f0[i]) * k)
}

fn ggx_d(ndh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = ndh * ndh * (a2 - 1.0) + 1.0;
    a2 / (std::f32::consts::PI * d * d).max(1e-12)
}

/// Smith の遮蔽(高さの相関を見る形)。
fn smith_g(ndl: f32, ndv: f32, a: f32) -> f32 {
    let a2 = a * a;
    let gv = ndl * (ndv * ndv * (1.0 - a2) + a2).sqrt();
    let gl = ndv * (ndl * ndl * (1.0 - a2) + a2).sqrt();
    2.0 * ndl * ndv / (gv + gl).max(1e-9)
}

/// 決まった回数だけ回した絵。
pub fn render(
    scene: &RenderScene,
    cam: &Camera,
    env: &Env,
    opt: &TraceOptions,
    passes: u32,
    denoise: bool,
) -> Image {
    render_with(scene, cam, env, opt, passes, denoise, |_| true)
}

/// [`render`] と同じだが、1 回回すごとに `after(回した数)` を呼ぶ。偽を返すとそこで止め、
/// それまでの回数で絵を仕上げる(進み具合を知らせ、途中で止められるように)。
pub fn render_with(
    scene: &RenderScene,
    cam: &Camera,
    env: &Env,
    opt: &TraceOptions,
    passes: u32,
    denoise: bool,
    mut after: impl FnMut(u32) -> bool,
) -> Image {
    let started = std::time::Instant::now();
    let mut t = Tracer::new(scene, *cam, env.clone(), opt.clone());
    for _ in 0..passes.max(1) {
        t.pass();
        if !after(t.passes) {
            break;
        }
    }
    log::info!(
        "光線追跡 {}×{}・{} 回: {:.1} 秒",
        t.width(),
        t.height(),
        t.passes,
        started.elapsed().as_secs_f64()
    );
    t.image(denoise)
}
