//! シーンの設定のうち、絵の見え方を決める物(「環境」と「カメラ」)。
//!
//! - 環境: 明るさ(lx)・環境の回転・背景(環境・単色・透明)・グラウンド面(出す・反射・粗さ)
//! - カメラ: 焦点距離(35 mm 判の mm)・露出(EV)・被写界深度(ぼかし。焦点は回す中心)
//!
//! 明るさと露出は掛け合わせて環境の光の倍率にする(1000 lx・9.5 EV が今までの明るさ)。露出を 1 EV 上げると半分の明るさになる(写真と同じ向き)。単色の背景は、明るさと露出に
//! よらず打った色のまま出す。
//!
//! 保存は jc-io-render のシーンの設定(`render.scene`)の JSON で、画面(jc-ui)も同じ JSON から読む。
//! 鍵の名前はここだけに書く([`SceneLook::from_json`]・[`SceneLook::write_json`])。

use crate::{Background, Camera, Env, EnvKind};
use serde_json::{Map, Value, json};

/// 床(グラウンド面)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ground {
    /// 床を描く(影が落ちる)
    pub show: bool,
    /// 床に映り込む
    pub reflect: bool,
    /// 映り込みのぼけ(0 は鏡、1 はほぼ映らない)
    pub roughness: f32,
}

impl Default for Ground {
    fn default() -> Self {
        Ground {
            show: true,
            reflect: false,
            roughness: 0.1,
        }
    }
}

/// 見え方の設定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneLook {
    /// 環境の明るさ(lx)
    pub brightness_lx: f32,
    /// 環境の回転(度。Z のまわりに、上から見て左回りが正)
    pub rotation_deg: f32,
    pub background: Background,
    pub ground: Ground,
    /// 焦点距離(mm、35 mm 判)
    pub focal_mm: f32,
    /// 露出(EV)
    pub exposure_ev: f32,
    /// 被写界深度でぼかす
    pub dof: bool,
    /// ぼかしの強さ(1 で、焦点の距離の 1 % の半径のレンズ)
    pub blur: f32,
}

/// 今までの明るさになる値。
pub const DEFAULT_LX: f32 = 1000.0;
pub const DEFAULT_EV: f32 = 9.5;
/// 今までの画角(縦 30 度)にほぼ合う焦点距離。
pub const DEFAULT_FOCAL_MM: f32 = 45.0;

impl Default for SceneLook {
    fn default() -> Self {
        SceneLook {
            brightness_lx: DEFAULT_LX,
            rotation_deg: 0.0,
            background: Background::Studio,
            ground: Ground::default(),
            focal_mm: DEFAULT_FOCAL_MM,
            exposure_ev: DEFAULT_EV,
            dof: false,
            blur: 1.0,
        }
    }
}

impl SceneLook {
    /// 環境の光に掛ける倍率(明るさと露出から)。
    pub fn exposure_scale(&self) -> f32 {
        self.brightness_lx / DEFAULT_LX * (DEFAULT_EV - self.exposure_ev).exp2()
    }

    /// 透視投影の縦の画角(度)。
    pub fn fov_y_deg(&self) -> f32 {
        crate::camera::fov_from_focal(self.focal_mm)
    }

    /// 環境 `kind` を、視点の水平の向き `toward_eye` と環境の回転・明るさ・露出で作る。
    pub fn env(&self, kind: EnvKind, toward_eye: [f32; 3]) -> Env {
        let (s, c) = self.rotation_deg.to_radians().sin_cos();
        let t = toward_eye;
        let mut env = Env::new(kind, [t[0] * c - t[1] * s, t[0] * s + t[1] * c, t[2]]);
        env.exposure *= self.exposure_scale();
        env.lamp_exposure = (DEFAULT_EV - self.exposure_ev).exp2();
        env
    }

    /// 被写界深度を視点に入れる(焦点の距離が分かっている透視投影の時だけ)。
    pub fn apply_camera(&self, cam: &mut Camera) {
        cam.aperture = if self.dof && cam.perspective() && cam.focus > 0.0 {
            self.blur.max(0.0) * cam.focus * 0.01
        } else {
            0.0
        };
    }

    /// JSON から。書いていない鍵は `base` の値。
    pub fn from_json(v: &Value, base: &SceneLook) -> Result<SceneLook, String> {
        let num = |k: &str, cur: f32, lo: f32, hi: f32, what: &str| -> Result<f32, String> {
            match v.get(k) {
                None | Some(Value::Null) => Ok(cur),
                Some(x) => match x.as_f64() {
                    Some(n) if n >= lo as f64 && n <= hi as f64 => Ok(n as f32),
                    _ => Err(format!("{what}は {lo}〜{hi} にしてください")),
                },
            }
        };
        let flag = |k: &str, cur: bool| -> Result<bool, String> {
            match v.get(k) {
                None | Some(Value::Null) => Ok(cur),
                Some(Value::Bool(b)) => Ok(*b),
                Some(_) => Err(format!("{k} は true か false です")),
            }
        };
        let color = match v.get("background_color").and_then(Value::as_str) {
            None => match base.background {
                Background::Color(c) => c,
                _ => DEFAULT_COLOR,
            },
            Some(s) => parse_color(s)
                .ok_or_else(|| format!("背景の色 {s} は #rrggbb の形で書いてください"))?,
        };
        let background = match v.get("background").and_then(Value::as_str) {
            None => match base.background {
                Background::Color(_) => Background::Color(color),
                b => b,
            },
            Some("studio") => Background::Studio,
            Some("transparent") => Background::Transparent,
            Some("color") => Background::Color(color),
            Some(_) => return Err("background は studio・color・transparent のどれかです".into()),
        };
        Ok(SceneLook {
            brightness_lx: num(
                "brightness",
                base.brightness_lx,
                0.0,
                100000.0,
                "明るさ(lx)",
            )?,
            rotation_deg: num(
                "env_rotation",
                base.rotation_deg,
                -360.0,
                360.0,
                "環境の回転",
            )?,
            background,
            ground: Ground {
                show: flag("ground", base.ground.show)?,
                reflect: flag("ground_reflection", base.ground.reflect)?,
                roughness: num("ground_roughness", base.ground.roughness, 0.0, 1.0, "粗さ")?,
            },
            focal_mm: num("focal_length", base.focal_mm, 5.0, 1000.0, "焦点距離(mm)")?,
            exposure_ev: num("exposure", base.exposure_ev, -15.0, 25.0, "露出(EV)")?,
            dof: flag("depth_of_field", base.dof)?,
            blur: num("blur", base.blur, 0.0, 10.0, "ぼかし")?,
        })
    }

    /// JSON の鍵を `m` に書く([`Self::from_json`] の逆)。
    pub fn write_json(&self, m: &mut Map<String, Value>) {
        let (bg, color) = match self.background {
            Background::Studio => ("studio", DEFAULT_COLOR),
            Background::Transparent => ("transparent", DEFAULT_COLOR),
            Background::Color(c) => ("color", c),
        };
        let round = |v: f32| (v as f64 * 1e4).round() / 1e4;
        for (k, v) in [
            ("brightness", json!(round(self.brightness_lx))),
            ("env_rotation", json!(round(self.rotation_deg))),
            ("background", json!(bg)),
            ("background_color", json!(color_hex(color))),
            ("ground", json!(self.ground.show)),
            ("ground_reflection", json!(self.ground.reflect)),
            ("ground_roughness", json!(round(self.ground.roughness))),
            ("focal_length", json!(round(self.focal_mm))),
            ("exposure", json!(round(self.exposure_ev))),
            ("depth_of_field", json!(self.dof)),
            ("blur", json!(round(self.blur))),
        ] {
            m.insert(k.into(), v);
        }
    }
}

/// 静止画の縦横比の選び方(鍵, 画面の名前, 横÷縦)。
/// 横÷縦が 0 の物は決まった比ではない(`viewport` は画面のビュー、`custom` は幅と高さのまま)。
pub const ASPECTS: [(&str, &str, f32); 7] = [
    ("viewport", "ビューポートの縦横比", 0.0),
    ("1:1", "1:1 正方形", 1.0),
    ("4:3", "4:3 プレゼンテーション", 4.0 / 3.0),
    ("16:9", "16:9 ワイドスクリーン", 16.0 / 9.0),
    ("5:4", "5:4 横", 5.0 / 4.0),
    ("4:5", "4:5 縦", 4.0 / 5.0),
    ("custom", "カスタム縦横比", 0.0),
];

/// 縦横比の鍵 `key` の横÷縦。`viewport` は画面のビューの比、`custom`(と知らない鍵)は幅と高さの比。
pub fn aspect_ratio(key: &str, width: u32, height: u32, viewport: Option<f32>) -> f32 {
    let own = width.max(1) as f32 / height.max(1) as f32;
    match ASPECTS.iter().find(|a| a.0 == key) {
        Some(("viewport", ..)) => viewport.filter(|r| *r > 0.0).unwrap_or(own),
        Some((_, _, r)) if *r > 0.0 => *r,
        _ => own,
    }
}

/// 単色の背景の既定。
pub const DEFAULT_COLOR: [u8; 3] = [200, 200, 200];

/// `#rrggbb`(# は省いてもよい)。
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

pub fn color_hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips_and_checks_ranges() {
        let mut look = SceneLook {
            brightness_lx: 2000.0,
            rotation_deg: 45.0,
            background: Background::Color([10, 20, 30]),
            ground: Ground {
                show: false,
                reflect: true,
                roughness: 0.3,
            },
            focal_mm: 120.0,
            exposure_ev: 8.5,
            dof: true,
            blur: 2.0,
        };
        let mut m = Map::new();
        look.write_json(&mut m);
        let v = Value::Object(m);
        assert_eq!(SceneLook::from_json(&v, &SceneLook::default()), Ok(look));
        // 書いていない鍵は元のまま。単色の背景の色だけ替える
        look.background = Background::Color([255, 0, 0]);
        assert_eq!(
            SceneLook::from_json(&json!({"background_color": "#ff0000"}), &{
                let mut l = look;
                l.background = Background::Color([10, 20, 30]);
                l
            }),
            Ok(look)
        );
        assert!(SceneLook::from_json(&json!({"exposure": 30}), &look).is_err());
        assert!(SceneLook::from_json(&json!({"background_color": "red"}), &look).is_err());
        assert!(SceneLook::from_json(&json!({"background": "sky"}), &look).is_err());
    }

    #[test]
    fn a_cropped_camera_puts_points_where_the_whole_view_did() {
        // 800×600 の画面の中の、右寄りの 300×300 の枠
        let (view, frame) = ([400.0, 300.0, 800.0, 600.0], [500.0, 280.0, 300.0, 300.0]);
        let to_px =
            |s: (f32, f32, f32), r: [f32; 4]| (r[0] + s.0 * r[2] / 2.0, r[1] - s.1 * r[3] / 2.0);
        for fov in [30.0, 0.0] {
            let mut cam = Camera::fit([0.0; 3], [20.0, 10.0, 5.0], -60.0, 30.0, fov, 800.0 / 600.0);
            cam.shift = [0.1, -0.05];
            let crop = cam.cropped(view, frame);
            for p in [[3.0, 4.0, 1.0], [15.0, 2.0, 4.0], [10.0, 10.0, 0.0]] {
                let a = to_px(cam.project(p, 800.0 / 600.0).unwrap(), view);
                let b = to_px(crop.project(p, 1.0).unwrap(), frame);
                assert!(
                    (a.0 - b.0).abs() < 0.05 && (a.1 - b.1).abs() < 0.05,
                    "{fov}: {a:?} {b:?}"
                );
            }
        }
    }

    #[test]
    fn aspect_presets_give_the_ratio_and_custom_keeps_the_size() {
        assert_eq!(aspect_ratio("16:9", 1600, 1000, None), 16.0 / 9.0);
        assert_eq!(aspect_ratio("custom", 1600, 1000, Some(2.0)), 1.6);
        assert_eq!(aspect_ratio("viewport", 1600, 1000, Some(2.0)), 2.0);
        // 画面のビューの比が分からない(CLI・MCP)時は幅と高さのまま
        assert_eq!(aspect_ratio("viewport", 1600, 1000, None), 1.6);
    }

    #[test]
    fn brightness_and_exposure_scale_the_light_like_a_camera() {
        let base = SceneLook::default();
        assert_eq!(base.exposure_scale(), 1.0);
        let brighter = SceneLook {
            brightness_lx: 2000.0,
            ..base
        };
        assert!((brighter.exposure_scale() - 2.0).abs() < 1e-6);
        // 露出を 1 EV 上げると半分
        let ev = SceneLook {
            exposure_ev: 10.5,
            ..base
        };
        assert!((ev.exposure_scale() - 0.5).abs() < 1e-6);
        // 既定の焦点距離は今までの画角(30 度)に近い
        assert!(
            (base.fov_y_deg() - 30.0).abs() < 0.2,
            "{}",
            base.fov_y_deg()
        );
    }

    #[test]
    fn rotating_the_environment_turns_the_key_light_around_z() {
        let toward = [0.0, -1.0, 0.0];
        let a = SceneLook::default().env(EnvKind::StudioLight, toward);
        let b = SceneLook {
            rotation_deg: 90.0,
            ..SceneLook::default()
        }
        .env(EnvKind::StudioLight, toward);
        let (ka, kb) = (a.key().dir, b.key().dir);
        // 高さは同じで、水平の向きが 90 度回る
        assert!((ka[2] - kb[2]).abs() < 1e-5);
        let dot = ka[0] * kb[0] + ka[1] * kb[1];
        assert!(dot.abs() < 1e-4, "{ka:?} {kb:?}");
    }

    #[test]
    fn depth_of_field_opens_the_lens_only_when_on_and_in_perspective() {
        let mut cam = Camera::fit([0.0; 3], [10.0; 3], -60.0, 30.0, 30.0, 1.5);
        assert!(cam.focus > 0.0);
        let on = SceneLook {
            dof: true,
            blur: 1.0,
            ..SceneLook::default()
        };
        on.apply_camera(&mut cam);
        assert!((cam.aperture - cam.focus * 0.01).abs() < 1e-5);
        SceneLook::default().apply_camera(&mut cam);
        assert_eq!(cam.aperture, 0.0);
        // 焦点の平面の点はぼけない: 穴のどこを通っても同じ点を通る
        on.apply_camera(&mut cam);
        let (o1, d1) = cam.ray_lens(0.2, 0.1, 1.5, [0.9, 0.1]);
        let (o2, d2) = cam.ray_lens(0.2, 0.1, 1.5, [0.4, 0.7]);
        let at = |o: [f32; 3], d: [f32; 3]| {
            let fw = cam.forward;
            let t = (cam.focus - (0..3).map(|i| (o[i] - cam.eye[i]) * fw[i]).sum::<f32>())
                / (0..3).map(|i| d[i] * fw[i]).sum::<f32>();
            [0, 1, 2].map(|i| o[i] + d[i] * t)
        };
        let (p1, p2) = (at(o1, d1), at(o2, d2));
        assert!(
            (0..3).all(|i| (p1[i] - p2[i]).abs() < 1e-2),
            "{p1:?} {p2:?}"
        );
        assert_ne!(o1, o2);
    }
}
