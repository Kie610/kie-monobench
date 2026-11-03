//! 歯車と機構の諸元。歯車の輪郭・ボディの機能が出力 [`GEAR_OUTPUT`] に入れ、組立(かみ合わせ・比)と
//! 加工(工具の太さの確かめ)が歯車の crate を知らずに読む。カムは [`CamSpec`](出力 [`CAM_OUTPUT`])、
//! 自在継手とクラッチは [`CouplingSpec`](出力 [`COUPLING_OUTPUT`])。
//!
//! 寸法は mm、角度はラジアン。種類の列挙は作る予定の全部を先に持つ。まだ形を作れない種類は、
//! 機能が「まだ作れない」誤りにする(読む側は種類で分けるだけで、ここの型は変えない)。
//!
//! 比の決まり: かみ合う 2 つの回転の比は歯数の逆比([`GearSpec::teeth`])。ベルト車・ロープ車は
//! ピッチ円の径の逆比([`GearSpec::pitch_radius`])。ラックは 1 回りで π × モジュール × 歯数 進む。

use kmb_core::Payload;
use kmb_core::kernel::P3;
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

/// 歯車の諸元を入れる出力の名前。
pub const GEAR_OUTPUT: &str = "gear";
/// カムの諸元を入れる出力の名前。
pub const CAM_OUTPUT: &str = "cam";
/// 自在継手・クラッチの諸元を入れる出力の名前。
pub const COUPLING_OUTPUT: &str = "coupling";

/// 歯車・車の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GearKind {
    /// 外歯の歯車(平歯車・はすば・やまば)
    #[default]
    External,
    /// 内歯車(輪の内側に歯)
    Internal,
    /// ラック(まっすぐな歯の並び)
    Rack,
    /// かさ歯車(歯が円錐の上に並ぶ)
    Bevel,
    /// ウォーム(ねじの形の歯)
    Worm,
    /// ウォームホイール
    WormWheel,
    /// つめ車
    Ratchet,
    /// がんぎ車(脱進機)
    EscapeWheel,
    /// 鎖車(ローラーチェーン)
    Sprocket,
    /// ゼネバ車(溝のある従車)
    Geneva,
    /// カウントホイール(打つ数を決める切り欠きの車)
    CountWheel,
    /// ピン車(ピンを歯にした車)
    PinWheel,
    /// フェース歯車(円板の面に歯)
    Face,
    /// 平ベルト車
    FlatPulley,
    /// V ベルト車
    VPulley,
    /// ロープ車
    RopeSheave,
}

impl GearKind {
    /// 全部の種類(画面の種類の一覧の並び)。
    pub const ALL: [GearKind; 16] = [
        GearKind::External,
        GearKind::Internal,
        GearKind::Rack,
        GearKind::Bevel,
        GearKind::Worm,
        GearKind::WormWheel,
        GearKind::Ratchet,
        GearKind::EscapeWheel,
        GearKind::Sprocket,
        GearKind::Geneva,
        GearKind::CountWheel,
        GearKind::PinWheel,
        GearKind::Face,
        GearKind::FlatPulley,
        GearKind::VPulley,
        GearKind::RopeSheave,
    ];

    /// 保存の値(`external` …)。
    pub fn key(self) -> &'static str {
        match self {
            GearKind::External => "external",
            GearKind::Internal => "internal",
            GearKind::Rack => "rack",
            GearKind::Bevel => "bevel",
            GearKind::Worm => "worm",
            GearKind::WormWheel => "worm_wheel",
            GearKind::Ratchet => "ratchet",
            GearKind::EscapeWheel => "escape_wheel",
            GearKind::Sprocket => "sprocket",
            GearKind::Geneva => "geneva",
            GearKind::CountWheel => "count_wheel",
            GearKind::PinWheel => "pin_wheel",
            GearKind::Face => "face",
            GearKind::FlatPulley => "flat_pulley",
            GearKind::VPulley => "v_pulley",
            GearKind::RopeSheave => "rope_sheave",
        }
    }

    /// 画面に出す名前。
    pub fn label(self) -> &'static str {
        match self {
            GearKind::External => "平歯車",
            GearKind::Internal => "内歯車",
            GearKind::Rack => "ラック",
            GearKind::Bevel => "かさ歯車",
            GearKind::Worm => "ウォーム",
            GearKind::WormWheel => "ウォームホイール",
            GearKind::Ratchet => "つめ車",
            GearKind::EscapeWheel => "がんぎ車",
            GearKind::Sprocket => "鎖車",
            GearKind::Geneva => "ゼネバ車",
            GearKind::CountWheel => "カウントホイール",
            GearKind::PinWheel => "ピン車",
            GearKind::Face => "フェース歯車",
            GearKind::FlatPulley => "平ベルト車",
            GearKind::VPulley => "V ベルト車",
            GearKind::RopeSheave => "ロープ車",
        }
    }

    /// 歯でかみ合わず、巻き掛け(ベルト・ロープ)で回す車か。比はピッチ円の径で決める。
    pub fn is_pulley(self) -> bool {
        matches!(
            self,
            GearKind::FlatPulley | GearKind::VPulley | GearKind::RopeSheave
        )
    }
}

/// 歯形の曲線。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToothForm {
    /// インボリュート
    #[default]
    Involute,
    /// サイクロイド(時計の歯車)
    Cycloid,
}

/// ねじれ(歯すじ)の向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hand {
    /// 右ねじれ
    #[default]
    Right,
    /// 左ねじれ
    Left,
    /// やまば(右と左を半分ずつ)
    Both,
}

/// 回転の軸(世界座標)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GearAxis {
    /// 軸の上の点(歯車の中心。ボディは歯の幅をこの点から軸の向きへ取る)
    pub origin: P3,
    /// 軸の向き(長さ 1)。輪郭の平面の法線
    pub direction: P3,
    /// 回す角の基準の向き(長さ 1、軸に直角)。輪郭の平面の X の向き
    pub reference: P3,
}

impl Default for GearAxis {
    fn default() -> Self {
        GearAxis {
            origin: [0.0; 3],
            direction: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
        }
    }
}

/// 歯車 1 つの諸元。輪郭の機能が作り、ボディの機能が幅と立体の値を足して出し直す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GearSpec {
    pub kind: GearKind,
    #[serde(default)]
    pub form: ToothForm,
    /// モジュール(鎖車・ベルト車・ロープ車は 0 で、ピッチ円の半径と鎖のピッチで決める)
    pub module: f64,
    /// モジュールが歯直角か(はすばを歯直角で決めた時に真。偽なら軸直角)
    #[serde(default)]
    pub normal_module: bool,
    /// 圧力角
    pub pressure_angle: f64,
    /// 歯数(ウォームは条数と同じ。ラックは並べた歯の数)
    pub teeth: u32,
    /// 転位係数
    #[serde(default)]
    pub profile_shift: f64,
    /// 基準ピッチ円の半径(ラックは 0)
    pub pitch_radius: f64,
    /// 回転の軸
    #[serde(default)]
    pub axis: GearAxis,
    /// 1 枚目の歯の中心の向き(軸の基準の向きから軸まわりに右ねじの向きに測る角)
    #[serde(default)]
    pub phase: f64,
    /// 歯の幅(輪郭だけの時は空)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_width: Option<f64>,
    /// ねじれ角(平歯車は 0)
    #[serde(default)]
    pub helix_angle: f64,
    #[serde(default)]
    pub hand: Hand,
    /// かさ歯車のピッチ円錐の角(半頂角。ほかは 0)
    #[serde(default)]
    pub cone_angle: f64,
    /// 鎖車の鎖のピッチ(ほかは空)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_pitch: Option<f64>,
    /// ウォームの条数(ほかは 1)
    #[serde(default = "one")]
    pub starts: u32,
}

fn one() -> u32 {
    1
}

impl Payload for GearSpec {
    const TYPE: &'static str = "gear.spec@1";
}

impl GearSpec {
    /// 歯 1 つ分の角(ピッチ角)。歯数が 0 なら 0。
    pub fn pitch_angle(&self) -> f64 {
        if self.teeth == 0 {
            0.0
        } else {
            TAU / self.teeth as f64
        }
    }

    /// 円ピッチ(ピッチ円の上の歯 1 つ分の長さ)。モジュールで決める歯車は π × モジュール。
    pub fn circular_pitch(&self) -> f64 {
        match self.chain_pitch {
            Some(p) => p,
            None => PI * self.module,
        }
    }

    /// 軸直角のモジュール(はすばを歯直角で決めた時は cos(ねじれ角) で割る)。
    pub fn transverse_module(&self) -> f64 {
        if self.normal_module {
            self.module / self.helix_angle.cos()
        } else {
            self.module
        }
    }
}

/// カムの従節の動き方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Follower {
    /// 直動(まっすぐ上下する)
    #[default]
    Translating,
    /// 揺動(支点のまわりに振れる)
    Oscillating,
}

/// カムの従節の先の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowerTip {
    Knife,
    #[default]
    Roller,
    Flat,
}

/// 変位の線図の区間の運動の法則(古くから使われる 4 つだけ)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionLaw {
    /// 等速
    Uniform,
    /// 単弦(単振動)
    SimpleHarmonic,
    /// サイクロイド
    #[default]
    Cycloidal,
    /// 3-4-5 の多項式
    Polynomial345,
}

impl MotionLaw {
    /// 区間の進み `u`(0〜1)での変位の割合(0〜1)。
    pub fn ratio(self, u: f64) -> f64 {
        let u = u.clamp(0.0, 1.0);
        match self {
            MotionLaw::Uniform => u,
            MotionLaw::SimpleHarmonic => (1.0 - (PI * u).cos()) / 2.0,
            MotionLaw::Cycloidal => u - (TAU * u).sin() / TAU,
            MotionLaw::Polynomial345 => u * u * u * (10.0 - 15.0 * u + 6.0 * u * u),
        }
    }
}

/// 変位の線図の 1 区間。`lift` が正なら上がり、負なら下がり、0 なら止まり。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CamSegment {
    /// 区間のカムの回る角
    pub span: f64,
    /// 区間の終わりまでの変位の増え(直動は mm、揺動はラジアン)
    pub lift: f64,
    #[serde(default)]
    pub law: MotionLaw,
}

/// カム 1 つの諸元。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamSpec {
    #[serde(default)]
    pub follower: Follower,
    #[serde(default)]
    pub tip: FollowerTip,
    /// 基礎円の半径
    pub base_radius: f64,
    /// ローラーの半径(先がローラーの時)
    #[serde(default)]
    pub roller_radius: f64,
    /// 直動の従節の偏心(軸と従節の線の隔たり)
    #[serde(default)]
    pub offset: f64,
    /// 揺動の従節の腕の長さと、カムの軸から支点までの距離
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arm: Option<(f64, f64)>,
    /// 変位の線図(区間の角の和が 1 回り)
    pub segments: Vec<CamSegment>,
    #[serde(default)]
    pub axis: GearAxis,
}

impl Payload for CamSpec {
    const TYPE: &'static str = "mech.cam@1";
}

impl CamSpec {
    /// カムの回る角 `angle` での従節の変位(区間の法則で補間する。1 回りより先は繰り返す)。
    pub fn displacement(&self, angle: f64) -> f64 {
        let total: f64 = self.segments.iter().map(|s| s.span).sum();
        if total <= 0.0 {
            return 0.0;
        }
        let mut a = angle.rem_euclid(total);
        let mut at = 0.0;
        for s in &self.segments {
            if a <= s.span || std::ptr::eq(s, self.segments.last().expect("区間がある")) {
                let u = if s.span > 0.0 { a / s.span } else { 1.0 };
                return at + s.lift * s.law.ratio(u);
            }
            a -= s.span;
            at += s.lift;
        }
        at
    }
}

/// 軸どうしをつなぐ物の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CouplingKind {
    /// 自在継手(十字の軸で交わる 2 つの軸をつなぐ)
    #[default]
    UniversalJoint,
    /// かみ合いクラッチ
    DogClutch,
    /// 摩擦板クラッチ
    FrictionClutch,
}

/// 自在継手・クラッチ 1 つの諸元。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CouplingSpec {
    pub kind: CouplingKind,
    /// 入力の軸
    pub input: GearAxis,
    /// 出力の軸
    pub output: GearAxis,
    /// 2 つの軸の交わる角(まっすぐなら 0)
    #[serde(default)]
    pub shaft_angle: f64,
    /// クラッチの入るシフトの量(入力の側の歯・板が動く長さ。自在継手は 0)
    #[serde(default)]
    pub engage_travel: f64,
}

impl Payload for CouplingSpec {
    const TYPE: &'static str = "mech.coupling@1";
}

impl CouplingSpec {
    /// 自在継手の出力の角(tan θ₂ = tan θ₁ / cos β)。1 回りごとに入力と同じ角に戻り、途中で飛ばない。
    pub fn output_angle(&self, input: f64) -> f64 {
        let turns = (input / TAU).floor();
        let a = input - turns * TAU;
        let b = a.sin().atan2(a.cos() * self.shaft_angle.cos());
        turns * TAU + b.rem_euclid(TAU)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_have_unique_keys_and_survive_json() {
        let keys: std::collections::BTreeSet<_> = GearKind::ALL.iter().map(|k| k.key()).collect();
        assert_eq!(keys.len(), GearKind::ALL.len());
        for k in GearKind::ALL {
            let v = serde_json::to_value(k).unwrap();
            assert_eq!(v, serde_json::json!(k.key()));
            assert_eq!(serde_json::from_value::<GearKind>(v).unwrap(), k);
        }
    }

    #[test]
    fn motion_laws_start_at_zero_and_end_at_one() {
        for law in [
            MotionLaw::Uniform,
            MotionLaw::SimpleHarmonic,
            MotionLaw::Cycloidal,
            MotionLaw::Polynomial345,
        ] {
            assert!(law.ratio(0.0).abs() < 1e-12, "{law:?}");
            assert!((law.ratio(1.0) - 1.0).abs() < 1e-12, "{law:?}");
            assert!((law.ratio(0.5) - 0.5).abs() < 1e-12, "{law:?}");
        }
    }

    #[test]
    fn a_cam_rises_dwells_and_falls_back() {
        let seg = |deg: f64, lift, law| CamSegment {
            span: deg.to_radians(),
            lift,
            law,
        };
        let cam = CamSpec {
            follower: Follower::Translating,
            tip: FollowerTip::Roller,
            base_radius: 20.0,
            roller_radius: 4.0,
            offset: 0.0,
            arm: None,
            segments: vec![
                seg(120.0, 10.0, MotionLaw::Cycloidal),
                seg(60.0, 0.0, MotionLaw::Uniform),
                seg(120.0, -10.0, MotionLaw::Polynomial345),
                seg(60.0, 0.0, MotionLaw::Uniform),
            ],
            axis: GearAxis::default(),
        };
        let d = |deg: f64| cam.displacement(deg.to_radians());
        assert!(d(0.0).abs() < 1e-9);
        assert!((d(60.0) - 5.0).abs() < 1e-9);
        assert!((d(150.0) - 10.0).abs() < 1e-9);
        assert!((d(240.0) - 5.0).abs() < 1e-9);
        assert!(d(330.0).abs() < 1e-9);
        assert!((d(420.0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_universal_joint_keeps_quarter_turns_and_never_jumps() {
        let j = CouplingSpec {
            kind: CouplingKind::UniversalJoint,
            input: GearAxis::default(),
            output: GearAxis::default(),
            shaft_angle: 30f64.to_radians(),
            engage_travel: 0.0,
        };
        for q in 0..8 {
            let a = q as f64 * PI / 2.0;
            assert!((j.output_angle(a) - a).abs() < 1e-9, "{q}");
        }
        let mut last = j.output_angle(0.0);
        for i in 1..=720 {
            let now = j.output_angle(i as f64 * TAU / 360.0);
            assert!(now > last && now - last < 0.05, "{i}");
            last = now;
        }
        // 45° では tan θ₂ = 1 / cos 30°
        let b = j.output_angle(PI / 4.0);
        assert!((b.tan() - 1.0 / 30f64.to_radians().cos()).abs() < 1e-9);
    }
}
