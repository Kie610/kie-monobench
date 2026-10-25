//! 環境の光。背景の濃淡(上は暗め、地平は明るい)と、丸いソフトボックスの光源をいくつか、式で書く。
//! 光源は視点の水平の向きについてくる(どこから見ても主の光が手前の左上から当たる。ターンテーブルは
//! 物を回して光と視点を止めるのと同じ見え方)。
//!
//! 速い描き方(raster)と GPU の画面は、拡散の光と映り込みを、ここの式で近似する。光線追跡は
//! [`Env::radiance`] をそのまま使う。

use crate::math::{V3, add, dot, lerp, normalize, scale, smoothstep};

/// 環境の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EnvKind {
    /// 明るい灰色のスタジオ(既定)
    #[default]
    StudioLight,
    /// 暗いスタジオ(金属が映える)
    StudioDark,
    /// 屋外(青空と太陽)
    Outdoor,
}

impl EnvKind {
    pub const ALL: [EnvKind; 3] = [EnvKind::StudioLight, EnvKind::StudioDark, EnvKind::Outdoor];

    /// 画面の名前。
    pub fn label(self) -> &'static str {
        match self {
            EnvKind::StudioLight => "スタジオ(明るい)",
            EnvKind::StudioDark => "スタジオ(暗い)",
            EnvKind::Outdoor => "屋外",
        }
    }

    /// 保存と CLI・MCP の名前。
    pub fn key(self) -> &'static str {
        match self {
            EnvKind::StudioLight => "studio",
            EnvKind::StudioDark => "studio_dark",
            EnvKind::Outdoor => "outdoor",
        }
    }

    /// 名前(保存の名前か画面の名前)から。
    pub fn from_name(s: &str) -> Option<EnvKind> {
        EnvKind::ALL
            .into_iter()
            .find(|k| k.key() == s || k.label() == s)
    }
}

/// 丸い光源(空の上の円盤)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// 光源の中心へ向かう向き(世界、長さ 1)
    pub dir: V3,
    /// 見込む半径(ラジアン)
    pub half: f32,
    pub cos_half: f32,
    /// 輝度(線形)
    pub radiance: V3,
}

impl Light {
    /// 見込む立体角。
    pub fn solid_angle(&self) -> f32 {
        2.0 * std::f32::consts::PI * (1.0 - self.cos_half)
    }
}

/// 環境の光一式。
#[derive(Debug, Clone, PartialEq)]
pub struct Env {
    pub kind: EnvKind,
    /// 真上の背景の輝度
    top: V3,
    /// 地平の背景の輝度
    horizon: V3,
    /// 地平より下(床)の輝度。映り込みに使う
    below: V3,
    /// 床の色(線形)
    pub floor_albedo: V3,
    /// 光源。先頭が主の光(影を落とす)
    pub lights: Vec<Light>,
    /// 全体の明るさの倍率
    pub exposure: f32,
    /// 文書の点光源とスポットライト(世界に固定。[`crate::lamp`])
    pub lamps: Vec<crate::lamp::Lamp>,
    /// 点光源とスポットライトの明るさの倍率(露出だけ。環境の明るさは効かない)
    pub lamp_exposure: f32,
}

impl Env {
    /// 環境 `kind` を、視点の水平の向き `toward_eye`(注視点から視点へ。Z は無視)に合わせて作る。
    pub fn new(kind: EnvKind, toward_eye: V3) -> Env {
        let f = normalize([toward_eye[0], toward_eye[1], 0.0]);
        let f = if f == [0.0; 3] { [0.0, -1.0, 0.0] } else { f };
        let right = [-f[1], f[0], 0.0];
        // 局所の向き(右、手前、上)を世界の向きに
        let w = |x: f32, y: f32, z: f32| {
            normalize(add(add(scale(right, x), scale(f, y)), [0.0, 0.0, z]))
        };
        let light = |dir: V3, half: f32, k: f32, tint: V3| Light {
            dir,
            half,
            cos_half: half.cos(),
            radiance: scale(tint, k),
        };
        let gray = |v: f32| [v, v, v];
        let mut env = match kind {
            EnvKind::StudioLight => Env {
                kind,
                top: [0.36, 0.37, 0.39],
                horizon: gray(0.78),
                below: gray(0.5),
                floor_albedo: gray(0.62),
                lights: vec![
                    light(w(-0.55, 0.6, 0.62), 0.26, 9.0, [1.0, 0.97, 0.92]),
                    light(w(0.9, 0.3, 0.32), 0.42, 1.3, [0.9, 0.95, 1.0]),
                    light(w(0.25, -0.9, 0.5), 0.2, 5.0, gray(1.0)),
                ],
                exposure: 1.0,
                lamps: Vec::new(),
                lamp_exposure: 1.0,
            },
            EnvKind::StudioDark => Env {
                kind,
                top: gray(0.012),
                horizon: gray(0.05),
                below: gray(0.03),
                floor_albedo: gray(0.05),
                lights: vec![
                    light(w(-0.55, 0.6, 0.62), 0.24, 16.0, [1.0, 0.97, 0.92]),
                    light(w(0.9, 0.3, 0.32), 0.4, 2.2, [0.85, 0.92, 1.0]),
                    light(w(0.25, -0.9, 0.5), 0.18, 12.0, gray(1.0)),
                ],
                exposure: 1.0,
                lamps: Vec::new(),
                lamp_exposure: 1.0,
            },
            EnvKind::Outdoor => Env {
                kind,
                top: [0.16, 0.3, 0.62],
                horizon: [0.62, 0.7, 0.8],
                below: [0.22, 0.21, 0.18],
                floor_albedo: [0.42, 0.4, 0.35],
                lights: vec![light(w(-0.4, 0.5, 0.77), 0.035, 650.0, [1.0, 0.95, 0.86])],
                exposure: 1.0,
                lamps: Vec::new(),
                lamp_exposure: 1.0,
            },
        };
        // 地平より下の背景は、光の当たった床と同じ明るさにする(床と背景のつなぎ目を見せない)
        env.below = crate::math::mul(env.floor_albedo, env.diffuse([0.0, 0.0, 1.0], false));
        env
    }

    /// 主の光(影を落とす光)。
    pub fn key(&self) -> &Light {
        &self.lights[0]
    }

    /// 光源を除いた背景の輝度。
    pub fn backdrop(&self, d: V3) -> V3 {
        let z = d[2];
        if z >= 0.0 {
            lerp(self.horizon, self.top, z.powf(0.6))
        } else {
            // 地平のすぐ下は地平の色からなめらかに床へ
            lerp(self.horizon, self.below, smoothstep(0.0, 0.08, -z))
        }
    }

    /// 向き `d` から来る光の輝度(光源も含む。光線追跡が使う)。
    pub fn radiance(&self, d: V3) -> V3 {
        let mut c = self.backdrop(d);
        for l in &self.lights {
            if dot(d, l.dir) >= l.cos_half {
                c = add(c, l.radiance);
            }
        }
        scale(c, self.exposure)
    }

    /// 背景だけの拡散の光(法線 `n` の面が白なら返す輝度。光源は含めない)。
    pub fn diffuse_backdrop(&self, n: V3) -> V3 {
        let sky = lerp(self.horizon, self.top, 0.45);
        let t = 0.5 + 0.5 * n[2];
        scale(lerp(self.below, sky, t), self.exposure)
    }

    /// 光源 `l` の拡散の光(影は見ない)。
    pub fn diffuse_light(&self, l: &Light, n: V3) -> V3 {
        // 大きな光源が面の地平にかかる所は、半分ほど当たるとしてなめらかにする
        let c = dot(n, l.dir);
        let k = smoothstep(-l.half, l.half, c) * c.max(l.half * 0.5);
        scale(
            l.radiance,
            self.exposure * l.solid_angle() * k / std::f32::consts::PI,
        )
    }

    /// 点 `p`・法線 `n` の、点光源とスポットライトの拡散の光(影は見ない)。
    pub fn lamp_diffuse(&self, p: V3, n: V3) -> V3 {
        let mut c = [0.0; 3];
        for l in &self.lamps {
            c = add(c, l.white_at(p, n));
        }
        scale(c, self.lamp_exposure)
    }

    /// 拡散の光(光源も含め、影は見ない)。`skip_key` なら主の光を除く(影の地図で別に足す時)。
    pub fn diffuse(&self, n: V3, skip_key: bool) -> V3 {
        let mut c = self.diffuse_backdrop(n);
        for (i, l) in self.lights.iter().enumerate() {
            if i == 0 && skip_key {
                continue;
            }
            c = add(c, self.diffuse_light(l, n));
        }
        c
    }

    /// 映り込み: 向き `r` の、粗さ `roughness` でぼかした輝度(光源も含む)。
    /// 鏡の向きのまわりの円錐(粗さで広がる)と、光源の円盤の重なりで近似する。
    pub fn specular(&self, r: V3, roughness: f32) -> V3 {
        let lobe = (roughness * roughness * 1.6 + 0.01).min(1.4);
        let bg = lerp(
            self.backdrop(r),
            scale(self.diffuse_backdrop(r), 1.0 / self.exposure.max(1e-6)),
            smoothstep(0.0, 1.2, lobe),
        );
        let mut c = bg;
        for l in &self.lights {
            let a = dot(r, l.dir).clamp(-1.0, 1.0).acos();
            let inner = (l.half - lobe).abs();
            let outer = l.half + lobe;
            let cover = 1.0 - smoothstep(inner, outer, a);
            let ratio = (l.half / lobe).min(1.0).powi(2);
            c = add(c, scale(l.radiance, cover * ratio));
        }
        scale(c, self.exposure)
    }
}

/// 映り込みの強さの近似(粗さと見る角度から、F0 に掛ける数と足す数。Karis 2014 の近似)。
pub fn env_brdf(roughness: f32, n_dot_v: f32) -> (f32, f32) {
    let c0 = [-1.0, -0.0275, -0.572, 0.022];
    let c1 = [1.0, 0.0425, 1.04, -0.04];
    let r = [0, 1, 2, 3].map(|i| roughness * c0[i] + c1[i]);
    let a004 = (r[0] * r[0]).min((-9.28 * n_dot_v.max(0.0)).exp2()) * r[0] + r[1];
    (-1.04 * a004 + r[2], 1.04 * a004 + r[3])
}
