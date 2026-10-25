//! 画面の映り込みに使う環境マップ。景色を正距円筒の絵に焼き、粗さごとにぼかした段(mip)と、
//! 拡散の光の小さな絵を持つ。GPU は同じ絵を貼って三線形で引き、CPU はこの型の関数で同じ値を引く。
//!
//! 絵の向きは景色の局所の枠(x = 右、y = 手前(視点の側)、z = 上)。世界の向きは [`EnvFrame`] で移す。
//! 段 k の粗さは k / (LEVELS − 1) で、ぼかしは GGX の山(法線 = 視線 = 反射の向き、Karis 2013 の近似)。

use crate::math::{V3, add, dot, lerp, mul, normalize, scale, smoothstep, sub};
use std::f32::consts::PI;

/// 段の数(512×256 から 16×8 まで半分ずつ)。
pub const LEVELS: usize = 6;
/// 一番細かい段の大きさ。
pub const BASE_W: usize = 512;
pub const BASE_H: usize = 256;
/// 拡散の光の絵の大きさ。
pub const IRR_W: usize = 32;
pub const IRR_H: usize = 16;
/// 画面の景色の明るさの倍率(トーンマップの前に掛ける)。
pub const EXPOSURE: f32 = 0.7;

/// 正距円筒の絵 1 枚(線形の輝度)。
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub w: usize,
    pub h: usize,
    pub px: Vec<V3>,
}

/// 画素 (x, y)(小数で画素の中の位置)の向き。
fn dir_of(w: usize, h: usize, x: f32, y: f32) -> V3 {
    let phi = (x / w as f32 - 0.5) * 2.0 * PI;
    let th = y / h as f32 * PI;
    [th.sin() * phi.cos(), th.sin() * phi.sin(), th.cos()]
}

/// 画素 1 つが見込む立体角(行 y)。
fn texel_solid_angle(w: usize, h: usize, y: usize) -> f32 {
    let th = (y as f32 + 0.5) / h as f32 * PI;
    th.sin() * (2.0 * PI / w as f32) * (PI / h as f32)
}

impl Level {
    fn bake(w: usize, h: usize, ss: usize, f: &(dyn Fn(V3) -> V3 + Sync)) -> Level {
        let mut px = vec![[0.0; 3]; w * h];
        crate::par_rows(&mut px, w, |y, row| {
            for (x, p) in row.iter_mut().enumerate() {
                let mut c = [0.0; 3];
                for j in 0..ss {
                    for i in 0..ss {
                        let fx = x as f32 + (i as f32 + 0.5) / ss as f32;
                        let fy = y as f32 + (j as f32 + 0.5) / ss as f32;
                        c = add(c, f(dir_of(w, h, fx, fy)));
                    }
                }
                *p = scale(c, 1.0 / (ss * ss) as f32);
            }
        });
        Level { w, h, px }
    }

    /// 縦横 1/k に縮めた絵(k×k の平均)。
    fn down(&self, k: usize) -> Level {
        let (w, h) = (self.w / k, self.h / k);
        let mut px = vec![[0.0; 3]; w * h];
        for (i, p) in px.iter_mut().enumerate() {
            let (x, y) = (i % w, i / w);
            let mut c = [0.0; 3];
            for j in 0..k {
                for q in 0..k {
                    c = add(c, self.px[(y * k + j) * self.w + x * k + q]);
                }
            }
            *p = scale(c, 1.0 / (k * k) as f32);
        }
        Level { w, h, px }
    }

    /// 向き `d`(局所の枠)の値。GPU の双線形と同じく、横は回り込み、縦は端で止める。
    pub fn sample(&self, d: V3) -> V3 {
        let u = (d[1].atan2(d[0]) / (2.0 * PI) + 0.5) * self.w as f32 - 0.5;
        let v = d[2].clamp(-1.0, 1.0).acos() / PI * self.h as f32 - 0.5;
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let get = |x: i32, y: i32| {
            let x = x.rem_euclid(self.w as i32) as usize;
            let y = y.clamp(0, self.h as i32 - 1) as usize;
            self.px[y * self.w + x]
        };
        let (xi, yi) = (x0 as i32, y0 as i32);
        lerp(
            lerp(get(xi, yi), get(xi + 1, yi), fx),
            lerp(get(xi, yi + 1), get(xi + 1, yi + 1), fx),
            fy,
        )
    }

    /// 粗さ `rough` の GGX の山で畳み込んだ `w`×`h` の絵。山が頂の 1/1000 より弱い所は足さない。
    fn prefilter(&self, rough: f32, w: usize, h: usize) -> Level {
        let a = (rough * rough).max(1e-3);
        let a2 = a * a;
        // D(h) が頂の 1/1000 になる半角の 2 倍(反射の向きと光の向きの間の角)
        let reach = (2.0 * (a * 30.6f32.sqrt()).atan()).min(PI * 0.5);
        let (sw, sh) = (self.w, self.h);
        let dirs: Vec<V3> = (0..sw * sh)
            .map(|i| dir_of(sw, sh, (i % sw) as f32 + 0.5, (i / sw) as f32 + 0.5))
            .collect();
        let dw: Vec<f32> = (0..sh).map(|y| texel_solid_angle(sw, sh, y)).collect();
        let mut px = vec![[0.0; 3]; w * h];
        crate::par_rows(&mut px, w, |y, row| {
            let th_r = (y as f32 + 0.5) / h as f32 * PI;
            let y0 = (((th_r - reach) / PI * sh as f32).floor().max(0.0)) as usize;
            let y1 = (((th_r + reach) / PI * sh as f32).ceil() as usize).min(sh);
            for (x, p) in row.iter_mut().enumerate() {
                let r = dir_of(w, h, x as f32 + 0.5, y as f32 + 0.5);
                let cx = (x as f32 + 0.5) / w as f32 * sw as f32;
                let mut c = [0.0; 3];
                let mut sum = 0.0;
                for (sy, &dw_row) in dw.iter().enumerate().take(y1).skip(y0) {
                    let th = (sy as f32 + 0.5) / sh as f32 * PI;
                    let s = th.sin();
                    // 極に近い行と山の広い時は一周、ほかは山の届く経度の幅だけ
                    let full = th_r - reach <= 0.0 || th_r + reach >= PI || reach.sin() >= s;
                    let span = if full {
                        sw
                    } else {
                        ((reach.sin() / s).asin() / (2.0 * PI) * sw as f32).ceil() as usize + 1
                    };
                    let (from, count) = if full || 2 * span + 1 >= sw {
                        (0, sw as i64)
                    } else {
                        (cx as i64 - span as i64, 2 * span as i64 + 1)
                    };
                    for k in from..from + count {
                        let i = sy * sw + k.rem_euclid(sw as i64) as usize;
                        let l = dirs[i];
                        let nl = dot(r, l);
                        if nl <= 0.0 {
                            continue;
                        }
                        let nh = dot(r, normalize(add(r, l)));
                        let q = nh * nh * (a2 - 1.0) + 1.0;
                        let wt = a2 / (PI * q * q) * nl * dw_row;
                        c = add(c, scale(self.px[i], wt));
                        sum += wt;
                    }
                }
                *p = scale(c, 1.0 / f32::max(sum, 1e-12));
            }
        });
        Level { w, h, px }
    }

    /// 拡散の光(白い面が返す輝度。cos で畳み込む)。
    fn irradiance(&self, w: usize, h: usize) -> Level {
        let (sw, sh) = (self.w, self.h);
        let mut px = vec![[0.0; 3]; w * h];
        crate::par_rows(&mut px, w, |y, row| {
            for (x, p) in row.iter_mut().enumerate() {
                let n = dir_of(w, h, x as f32 + 0.5, y as f32 + 0.5);
                let mut c = [0.0; 3];
                for sy in 0..sh {
                    let dw = texel_solid_angle(sw, sh, sy);
                    for sx in 0..sw {
                        let l = dir_of(sw, sh, sx as f32 + 0.5, sy as f32 + 0.5);
                        let k = dot(n, l);
                        if k > 0.0 {
                            c = add(c, scale(self.px[sy * sw + sx], k * dw));
                        }
                    }
                }
                *p = scale(c, 1.0 / PI);
            }
        });
        Level { w, h, px }
    }
}

/// 環境マップ一式。
#[derive(Debug, Clone, PartialEq)]
pub struct EnvMap {
    /// 粗さ k / (LEVELS − 1) の段。0 番は 512×256
    pub levels: Vec<Level>,
    /// 拡散の光
    pub irr: Level,
}

impl EnvMap {
    /// 景色 `radiance`(局所の枠の向き → 線形の輝度)から作る。
    pub fn bake(radiance: &(dyn Fn(V3) -> V3 + Sync)) -> EnvMap {
        let base = Level::bake(BASE_W, BASE_H, 3, radiance);
        let srcs = [base.down(2), base.down(4), base.down(8)];
        let mut levels = vec![base.clone()];
        for k in 1..LEVELS {
            let rough = k as f32 / (LEVELS - 1) as f32;
            let src = &srcs[(k - 1).min(2)];
            levels.push(src.prefilter(rough, BASE_W >> k, BASE_H >> k));
        }
        let irr = srcs[2].irradiance(IRR_W, IRR_H);
        EnvMap { levels, irr }
    }

    /// 設計の画面の景色(画面用のスタジオ)。初めて使う時に 1 回だけ作る。
    pub fn viewport() -> &'static EnvMap {
        static MAP: std::sync::OnceLock<EnvMap> = std::sync::OnceLock::new();
        MAP.get_or_init(|| {
            let t = std::time::Instant::now();
            let m = EnvMap::bake(&viewport_radiance);
            log::info!(
                "画面の環境マップを作った: {:.0} ms",
                t.elapsed().as_secs_f32() * 1e3
            );
            m
        })
    }

    /// 映り込み: 向き `r`(局所の枠)の、粗さ `rough` でぼかした輝度(GPU の textureSampleLevel と同じ)。
    pub fn specular(&self, r: V3, rough: f32) -> V3 {
        let lod = rough.clamp(0.0, 1.0) * (LEVELS - 1) as f32;
        let i = (lod.floor() as usize).min(LEVELS - 2);
        let t = lod - i as f32;
        lerp(self.levels[i].sample(r), self.levels[i + 1].sample(r), t)
    }

    /// 拡散の光(白い面が返す輝度)。
    pub fn diffuse(&self, n: V3) -> V3 {
        self.irr.sample(n)
    }

    /// 面の色(線形、トーンマップの前)。`view` は面から視点へ向かう向き(世界)。
    pub fn shade(
        &self,
        frame: &EnvFrame,
        albedo: V3,
        metal: f32,
        rough: f32,
        n: V3,
        view: V3,
    ) -> V3 {
        let ndv = dot(n, view).max(1e-4);
        let r = sub(scale(n, 2.0 * dot(n, view)), view);
        let f0 = lerp([0.04; 3], albedo, metal);
        let diff = mul(scale(albedo, 1.0 - metal), self.diffuse(frame.local(n)));
        let (a, b) = crate::env::env_brdf(rough, ndv);
        // 反射が面の下へ回る所を弱める(Filament の水平線の減衰)
        let fade = (1.0 + 1.1 * dot(r, n)).clamp(0.0, 1.0).powi(2);
        let spec = mul(
            self.specular(frame.local(r), rough),
            add(scale(f0, a), [b; 3]),
        );
        scale(add(diff, scale(spec, fade)), EXPOSURE)
    }
}

/// 世界の向きを景色の局所の枠(右・手前・上)へ移す枠。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvFrame {
    pub x: V3,
    pub y: V3,
    pub z: V3,
}

impl EnvFrame {
    /// 上下は世界に固定し、水平の向きを `toward_eye`(注視点から視点へ)に合わせる
    /// (jc_render::Env::new と同じ向きの決め方)。
    pub fn horizontal(toward_eye: V3) -> EnvFrame {
        let f = normalize([toward_eye[0], toward_eye[1], 0.0]);
        let f = if f == [0.0; 3] { [0.0, -1.0, 0.0] } else { f };
        EnvFrame {
            x: [-f[1], f[0], 0.0],
            y: f,
            z: [0.0, 0.0, 1.0],
        }
    }

    /// 世界に固定(手前を −Y、正面図の向きにする)。
    pub fn world() -> EnvFrame {
        EnvFrame::horizontal([0.0, -1.0, 0.0])
    }

    pub fn local(&self, d: V3) -> V3 {
        [dot(d, self.x), dot(d, self.y), dot(d, self.z)]
    }
}

/// 画面のスタジオの主の光の向き(方位・仰角、度。方位は右 = 0°、手前 = 90°)。
pub const VIEWPORT_KEY: (f32, f32) = (51.0, 63.0);

/// 画面用のスタジオ: 空・地平・床のグラデーションに、地平の明るい帯 2 本と、小さく明るい主の光。
/// 値は見本の画面(暗い画面・既定の鋼)の写真から逆算した。写真の球の
/// 画素ごとに反射の向きを求めて明るさの表にし、この関数の値を変えながら、JC と同じ描き方で描いた
/// 球と平らな 3 面(上・手前・右)が写真に近づくように当てはめた(球の差は平均 8、3 面は ±1)。写真の絵そのものは持たず、形を数式で作る。
/// 景色は世界に固定する。主の光は小さい(見込む半径 0.08)ので、平らな面に映っても
/// 面の一部の明るみで済む。床は空と同じくらいの明るさにして、曲面が下を映しても沈まないようにする。
pub fn viewport_radiance(d: V3) -> V3 {
    const SKY: f32 = 0.25;
    const HORIZON: f32 = 0.41;
    const GROUND: f32 = 0.25;
    // (方位, 仰角, 方位の半幅, 方位のぼかし幅, 仰角の半幅, 仰角のぼかし幅, 輝度)(度): 右手前の低い帯と、
    // 奥の少し高い帯
    const BANDS: [[f32; 7]; 2] = [
        [11.0, 3.0, 35.0, 30.0, 8.0, 10.5, 0.46],
        [180.0, 22.0, 25.0, 30.0, 8.0, 22.0, 0.55],
    ];
    let el = d[2].clamp(-1.0, 1.0).asin().to_degrees();
    let az = d[1].atan2(d[0]).to_degrees();
    let mut c = if el >= 0.0 {
        HORIZON + (SKY - HORIZON) * (el / 90.0).powf(0.65)
    } else {
        HORIZON + (GROUND - HORIZON) * smoothstep(0.0, 15.0, -el)
    };
    for [band_az, band_el, ha, sa, he, se, k] in BANDS {
        let da = (az - band_az).rem_euclid(360.0);
        let da = da.min(360.0 - da);
        c += k
            * (1.0 - smoothstep(ha, ha + sa, da))
            * (1.0 - smoothstep(he, he + se, (el - band_el).abs()));
    }
    let (ka, ke) = (VIEWPORT_KEY.0.to_radians(), VIEWPORT_KEY.1.to_radians());
    let key = [ke.cos() * ka.cos(), ke.cos() * ka.sin(), ke.sin()];
    if dot(d, key) >= 0.08f32.cos() {
        c += 60.0;
    }
    [c; 3]
}

/// 既定の鋼の粗さ(画面)。0.08 ではフィレットの映り込みが網の上で波打つ(報告の 3 節)。
pub const STEEL_ROUGHNESS: f32 = 0.12;

/// 32 ビットの浮動小数を 16 ビット(半精度)の並びに直す(GPU の Rgba16Float に詰める)。
pub fn f16_bits(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x7f_ffff;
    if v.is_nan() {
        return 0x7e00;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    if exp <= 0 {
        if exp < -10 {
            return sign;
        }
        let m = (man | 0x80_0000) >> (1 - exp);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    // 丸め(繰り上がりで指数が増えてもそのまま正しい値になる)
    sign | (((exp as u32) << 10) + ((man + 0x1000) >> 13)) as u16
}

impl Level {
    /// RGBA16Float の並び(A は 1)。
    pub fn rgba16f(&self) -> Vec<u16> {
        let one = f16_bits(1.0);
        self.px
            .iter()
            .flat_map(|c| [f16_bits(c[0]), f16_bits(c[1]), f16_bits(c[2]), one])
            .collect()
    }
}
