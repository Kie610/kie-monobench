//! 工具と工具ライブラリ。
//!
//! 工具ライブラリはタイムラインの機能 `cam.tools@2` として文書に持つ。出力の `tools` スロットに
//! [`ToolLibrary`] を入れ、加工の機能は工具番号で工具を選ぶ。
//!
//! 工具は「形」(種類、径、刃長、刃数、シャンク径)と、材料ごとの「切削条件のプリセット」に分ける。
//! 加工はストックの材料に合うプリセットを使い、加工の機能の側で上書きできる。
//! 単位は mm、mm/min、rpm に固定する。
//!
//! 古い形式(`cam.tools@1`、工具ごとに切削条件が 1 つ)もそのまま読める。読むと既定のプリセットが 1 つの
//! 工具になる。知らない項目は消さずに持ち回る(PC 全体のライブラリを新しい版で開いても失わない)。

use kmb_core::{EvalCtx, EvalError, Feature, FeatureId, InputRef, Outputs, Registry, Value};
use serde::{Deserialize, Serialize};

mod carvera;
pub mod exchange;
pub use carvera::carvera_air_tools;

pub const TOOLS: &str = "cam.tools@2";
/// 古い形式。読み込みだけに使う
pub const TOOLS_V1: &str = "cam.tools@1";
pub const TOOLS_TYPE: &str = "cam.tools@1";
/// 工具ライブラリの出力のうち、加工の工具の欄に出す候補の一覧。
pub const TOOL_CHOICES: &str = "tool_choices";

type Extra = serde_json::Map<String, serde_json::Value>;

/// 工具の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// 先の平らなエンドミル
    FlatEnd,
    /// ボールエンドミル
    BallEnd,
    /// ドリル。`tip_angle` が先端の角度
    Drill,
    /// V ビット(面取りカッター、彫刻刀)。`tip_angle` が刃先の角度(例: 90 度、60 度)。
    /// 先はとがっているとみなす
    VBit,
    /// 先が平らなテーパーのエンドミル。`tip_diameter` が先の径、`tip_angle` が側面の開きの全角
    /// (片側の角度の 2 倍)、`diameter` が刃の上端の径
    TaperEnd,
    /// ブルノーズ(角 R 付きの平のエンドミル)。`corner_radius` が角の丸みの半径
    BullNose,
    /// 先が球のテーパーのエンドミル。`tip_diameter` が先の球の径、`tip_angle` が側面の開きの全角、
    /// `diameter` が刃の上端の径。側面は先の球に接する
    TaperBall,
    /// レーザー(削らずに焼く)。`diameter` が焼く線の幅(光の点の径。切り口の補正の幅の既定)。切削条件の回転数の欄は
    /// 出力(%)、送りは焼く速さ
    Laser,
    /// ねじ切りミル(ねじ山の形の刃で、らせんに回ってねじを切る)。`diameter` が刃(山の先)の径、
    /// `tip_angle` が山の角度(メートルねじは 60 度)、首の径と歯の数とピッチは [`Tool::thread`]
    ThreadMill,
}

impl ToolKind {
    /// 選べる種類(一覧の順)。
    pub const ALL: [ToolKind; 9] = [
        ToolKind::FlatEnd,
        ToolKind::BullNose,
        ToolKind::BallEnd,
        ToolKind::Drill,
        ToolKind::VBit,
        ToolKind::TaperEnd,
        ToolKind::TaperBall,
        ToolKind::ThreadMill,
        ToolKind::Laser,
    ];

    /// 画面と表に出す名前。
    pub fn label(self) -> &'static str {
        match self {
            ToolKind::FlatEnd => "フラットエンドミル",
            ToolKind::BallEnd => "ボールエンドミル",
            ToolKind::Drill => "ドリル",
            ToolKind::VBit => "V ビット",
            ToolKind::TaperEnd => "テーパー",
            ToolKind::BullNose => "ブルノーズ",
            ToolKind::TaperBall => "テーパーボール",
            ToolKind::Laser => "レーザー",
            ToolKind::ThreadMill => "ねじ切りミル",
        }
    }
}

/// 1 本の工具。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "ToolData", into = "ToolData")]
pub struct Tool {
    /// 機械に渡す工具番号(1 以上)。手動の工具交換で機械が求める番号の意味だけを持つ
    pub number: u32,
    pub name: String,
    pub kind: ToolKind,
    /// 刃の径(mm)
    pub diameter: f64,
    /// 刃長(mm)。これより深い切り込みは誤りにする
    pub flute_length: f64,
    /// 刃数
    pub flutes: u32,
    /// ドリルの先端角(度)
    pub tip_angle: f64,
    /// シャンク(つかむ所)の径(mm)。コレットの検査に使う。空なら刃の径と同じとみなす
    pub shank_diameter: Option<f64>,
    /// 全長(mm)
    pub overall_length: Option<f64>,
    /// 先の径(mm)。テーパーのエンドミルの時
    pub tip_diameter: Option<f64>,
    /// 突き出し(コレットの端から刃先まで、mm)。あればシミュレーションでホルダーの当たりを調べる
    pub stickout: Option<f64>,
    /// ホルダー(コレットナット)の径(mm)。突き出しと組で使う
    pub holder_diameter: Option<f64>,
    /// 角の丸みの半径(mm)。ブルノーズの時
    pub corner_radius: Option<f64>,
    /// 首下長(刃先からシャンクの始まりまで、mm)。刃より上のこの長さは刃の径の首で、
    /// その上からシャンクの径になる。空なら刃のすぐ上からシャンク
    pub neck_length: Option<f64>,
    /// ねじ切りミルの山の形(ねじ切りミルの時)
    pub thread: Option<ThreadForm>,
    /// 材料ごとの切削条件。最初のものが既定。1 つ以上
    pub presets: Vec<Preset>,
    /// 知らない項目(新しい版で足された項目など)。読んだまま書き戻す
    pub extra: Extra,
}

/// ねじ切りミルの山の形。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThreadForm {
    /// 歯(山)の段の数。1 なら 1 山(どのピッチも切れる)。2 以上なら、`pitch` の間隔で並んだ山で
    /// 1 周に何山も切る
    #[serde(default = "one")]
    pub teeth: u32,
    /// 山の間隔(mm)。2 山以上の工具だけ(この工具で切れるピッチはこれだけ)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
    /// 首(歯より上の細い所)の径(mm)。穴の壁に当たらないかを調べる。空なら刃の径の 0.65 倍とみなす
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neck_diameter: Option<f64>,
}

impl Default for ThreadForm {
    fn default() -> Self {
        ThreadForm {
            teeth: 1,
            pitch: None,
            neck_diameter: None,
        }
    }
}

fn one() -> u32 {
    1
}

/// 保存の形。古い形式の `cutting` も読む。
#[derive(Serialize, Deserialize)]
struct ToolData {
    number: u32,
    name: String,
    kind: ToolKind,
    diameter: f64,
    flute_length: f64,
    #[serde(default = "two")]
    flutes: u32,
    #[serde(default = "drill_tip")]
    tip_angle: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shank_diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    overall_length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tip_diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stickout: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    holder_diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    corner_radius: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    neck_length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thread: Option<ThreadForm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    presets: Vec<Preset>,
    /// 古い形式(`cam.tools@1`)の切削条件
    #[serde(default, skip_serializing)]
    cutting: Option<Cutting>,
    #[serde(flatten)]
    extra: Extra,
}

impl From<ToolData> for Tool {
    fn from(d: ToolData) -> Tool {
        let mut presets = d.presets;
        if presets.is_empty() {
            presets.push(Preset {
                material: String::new(),
                cutting: d.cutting.unwrap_or_default(),
            });
        }
        Tool {
            number: d.number,
            name: d.name,
            kind: d.kind,
            diameter: d.diameter,
            flute_length: d.flute_length,
            flutes: d.flutes,
            tip_angle: d.tip_angle,
            shank_diameter: d.shank_diameter,
            overall_length: d.overall_length,
            tip_diameter: d.tip_diameter,
            stickout: d.stickout,
            holder_diameter: d.holder_diameter,
            corner_radius: d.corner_radius,
            neck_length: d.neck_length,
            thread: d.thread,
            presets,
            extra: d.extra,
        }
    }
}

impl From<Tool> for ToolData {
    fn from(t: Tool) -> ToolData {
        ToolData {
            number: t.number,
            name: t.name,
            kind: t.kind,
            diameter: t.diameter,
            flute_length: t.flute_length,
            flutes: t.flutes,
            tip_angle: t.tip_angle,
            shank_diameter: t.shank_diameter,
            overall_length: t.overall_length,
            tip_diameter: t.tip_diameter,
            stickout: t.stickout,
            holder_diameter: t.holder_diameter,
            corner_radius: t.corner_radius,
            neck_length: t.neck_length,
            thread: t.thread,
            presets: t.presets,
            cutting: None,
            extra: t.extra,
        }
    }
}

fn two() -> u32 {
    2
}

fn drill_tip() -> f64 {
    118.0
}

/// 切削条件。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Cutting {
    /// 主軸の回転数(rpm)
    pub rpm: f64,
    /// 横方向の送り(mm/min)
    pub feed: f64,
    /// 下向きの送り(mm/min)
    pub plunge: f64,
    /// 1 回に下げる深さ(mm)
    pub stepdown: f64,
    /// 横の送り幅。工具径に対する割合
    pub stepover: f64,
}

impl Default for Cutting {
    fn default() -> Self {
        Cutting {
            rpm: 10000.0,
            feed: 600.0,
            plunge: 200.0,
            stepdown: 1.0,
            stepover: 0.4,
        }
    }
}

impl Cutting {
    /// 1 刃あたりの送り(mm、チップロード)= 送り ÷(回転数 × 刃数)。
    pub fn chip_load(&self, flutes: u32) -> f64 {
        self.feed / (self.rpm * flutes.max(1) as f64)
    }
}

/// 材料ごとの切削条件。`material` が空なら材料を問わない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    #[serde(default)]
    pub material: String,
    #[serde(flatten)]
    pub cutting: Cutting,
}

/// 材料と、チップロードの目安。目安は工具径 1 mm あたりの範囲で、径に比例させる。
/// 値は小型の機械(主軸 200 W 前後)向けの控えめな目安で、確定ではない(A)。手持ちの工具に合わせて直す
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    pub name: &'static str,
    /// チップロードの目安(径 1 mm あたり、mm)
    pub chip_per_mm: (f64, f64),
}

/// 知っている材料。名前はストックの材料欄とプリセットで使う。設計の物理材料の表
/// (`kmb_look::material::PHYS_MATERIALS`)にも同じ名前があり、ボディに付けた材料がそのまま加工の材料になる。
pub const MATERIALS: &[Material] = &[
    Material {
        name: "木材",
        chip_per_mm: (0.006, 0.025),
    },
    Material {
        name: "MDF",
        chip_per_mm: (0.006, 0.025),
    },
    Material {
        name: "アクリル",
        chip_per_mm: (0.006, 0.02),
    },
    Material {
        name: "樹脂",
        chip_per_mm: (0.005, 0.02),
    },
    Material {
        name: "アルミ",
        chip_per_mm: (0.002, 0.008),
    },
    Material {
        name: "真鍮",
        chip_per_mm: (0.002, 0.008),
    },
];

/// 材料の名前が同じか(前後の空白と大文字・小文字を無視し、「aluminum」と「アルミ」のような別名も同じとみなす)。
/// 名前と別名は設計の物理材料の表(`kmb_look::material`)と共通。
pub fn same_material(a: &str, b: &str) -> bool {
    kmb_look::material::same_material(a, b)
}

pub fn material(name: &str) -> Option<&'static Material> {
    MATERIALS.iter().find(|m| same_material(m.name, name))
}

/// 材料の目安から見たチップロードの範囲(mm)。知らない材料は `None`。
pub fn chip_load_range(material_name: &str, diameter: f64) -> Option<(f64, f64)> {
    let m = material(material_name)?;
    Some((m.chip_per_mm.0 * diameter, m.chip_per_mm.1 * diameter))
}

impl Tool {
    pub fn radius(&self) -> f64 {
        self.diameter / 2.0
    }

    /// シャンク径(空なら刃の径)。
    pub fn shank(&self) -> f64 {
        self.shank_diameter.unwrap_or(self.diameter)
    }

    /// 既定の切削条件(最初のプリセット)。
    pub fn cutting(&self) -> Cutting {
        self.presets.first().map(|p| p.cutting).unwrap_or_default()
    }

    /// 材料 `material` に使う切削条件。合うプリセットがなければ、材料を問わないもの、
    /// それもなければ最初のものを使い、警告を返す。材料が空なら既定のものを使う。
    pub fn cutting_for(&self, material: &str) -> (Cutting, Option<String>) {
        if material.trim().is_empty() {
            return (self.cutting(), None);
        }
        if let Some(p) = self
            .presets
            .iter()
            .find(|p| same_material(&p.material, material))
        {
            return (p.cutting, None);
        }
        let p = self
            .presets
            .iter()
            .find(|p| p.material.trim().is_empty())
            .or(self.presets.first());
        let used = match p {
            Some(p) if !p.material.is_empty() => format!("「{}」の条件", p.material),
            _ => "既定の条件".into(),
        };
        (
            p.map(|p| p.cutting).unwrap_or_default(),
            Some(format!(
                "工具 {} に材料「{}」の切削条件がないため、{used}を使います",
                self.name,
                material.trim()
            )),
        )
    }

    /// チップロードが材料の目安を外れていれば警告を返す。
    /// V ビットとテーパーは削る深さで刃の径が変わり、目安が当てはまらないので調べない。
    pub fn chip_load_warning(&self, c: &Cutting, material: &str) -> Option<String> {
        if matches!(
            self.kind,
            ToolKind::VBit | ToolKind::TaperEnd | ToolKind::TaperBall
        ) {
            return None;
        }
        let (lo, hi) = chip_load_range(material, self.diameter)?;
        let cl = c.chip_load(self.flutes);
        let what = if cl < lo {
            "小さすぎます(こすれて熱を持ちやすい)"
        } else if cl > hi {
            "大きすぎます(工具が折れやすい)"
        } else {
            return None;
        };
        Some(format!(
            "工具 {} のチップロード {cl:.4} mm は、{}の目安 {lo:.3}〜{hi:.3} mm より{what}",
            self.name,
            material.trim()
        ))
    }

    /// 形と寸法が正しいか。
    pub fn check(&self) -> Result<(), String> {
        let name = &self.name;
        if self.number == 0 {
            return Err(format!("工具 {name} の番号は 1 以上にしてください"));
        }
        if !(self.diameter > 0.0 && self.diameter.is_finite()) {
            return Err(format!("工具 {name} の径は 0 より大きくしてください"));
        }
        if !(self.flute_length > 0.0 && self.flute_length.is_finite()) {
            return Err(format!("工具 {name} の刃長は 0 より大きくしてください"));
        }
        if matches!(
            self.kind,
            ToolKind::Drill | ToolKind::VBit | ToolKind::TaperEnd | ToolKind::TaperBall
        ) && !(self.tip_angle > 0.0 && self.tip_angle < 180.0)
        {
            return Err(format!("工具 {name} の先端角は 0〜180 度にしてください"));
        }
        if self.kind == ToolKind::TaperEnd {
            match self.tip_diameter {
                Some(t) if t > 0.0 && t <= self.diameter => {}
                _ => {
                    return Err(format!(
                        "工具 {name} の先の径は 0 より大きく、刃の径以下にしてください"
                    ));
                }
            }
        }
        if self.kind == ToolKind::TaperBall {
            match self.tip_diameter {
                Some(t) if t > 0.0 && t <= self.diameter => {}
                _ => {
                    return Err(format!(
                        "工具 {name} の先の球の径は 0 より大きく、刃の径以下にしてください"
                    ));
                }
            }
        }
        if let Some(f) = self.thread_form() {
            if !(self.tip_angle > 0.0 && self.tip_angle < 180.0) {
                return Err(format!("工具 {name} の山の角度は 0〜180 度にしてください"));
            }
            if f.teeth == 0 {
                return Err(format!("工具 {name} の歯の数は 1 以上にしてください"));
            }
            match f.pitch {
                Some(p) if !(p > 0.0 && p.is_finite()) => {
                    return Err(format!("工具 {name} のピッチは 0 より大きくしてください"));
                }
                None if f.teeth > 1 => {
                    return Err(format!(
                        "工具 {name} は歯が 2 段以上なので、歯の間隔(ピッチ)を入れてください"
                    ));
                }
                _ => {}
            }
            if let Some(n) = f.neck_diameter
                && !(n > 0.0 && n < self.diameter)
            {
                return Err(format!(
                    "工具 {name} の首の径は 0 より大きく、刃の径より細くしてください"
                ));
            }
        }
        if self.kind == ToolKind::BullNose {
            match self.corner_radius {
                Some(rc) if rc > 0.0 && rc <= self.radius() => {}
                _ => {
                    return Err(format!(
                        "工具 {name} の角 R は 0 より大きく、刃の半径以下にしてください"
                    ));
                }
            }
        }
        for (v, what) in [
            (self.stickout, "突き出し"),
            (self.holder_diameter, "ホルダーの径"),
            (self.overall_length, "全長"),
            (self.neck_length, "首下長"),
        ] {
            if let Some(v) = v
                && !(v > 0.0 && v.is_finite())
            {
                return Err(format!("工具 {name} の{what}は 0 より大きくしてください"));
            }
        }
        if let Some(s) = self.stickout
            && s < self.flute_length
        {
            return Err(format!("工具 {name} の突き出しは刃長以上にしてください"));
        }
        if let Some(n) = self.neck_length
            && n < self.flute_length
        {
            return Err(format!("工具 {name} の首下長は刃長以上にしてください"));
        }
        if let Some(l) = self.overall_length {
            for (v, what) in [
                (Some(self.flute_length), "刃長"),
                (self.neck_length, "首下長"),
                (self.stickout, "突き出し"),
            ] {
                if let Some(v) = v
                    && v > l
                {
                    return Err(format!(
                        "工具 {name} の{what} {v} mm が全長 {l} mm より長くなっています"
                    ));
                }
            }
        }
        if let Some(sd) = self.shank_diameter
            && !(sd > 0.0 && sd.is_finite())
        {
            return Err(format!(
                "工具 {name} のシャンク径は 0 より大きくしてください"
            ));
        }
        if self.presets.is_empty() {
            return Err(format!("工具 {name} に切削条件がありません"));
        }
        for p in &self.presets {
            let c = &p.cutting;
            let of = if p.material.is_empty() {
                String::new()
            } else {
                format!("({})", p.material)
            };
            for (v, what) in [
                (c.rpm, "回転数"),
                (c.feed, "送り"),
                (c.plunge, "下向きの送り"),
                (c.stepdown, "切り込みの深さ"),
                (c.stepover, "送り幅"),
            ] {
                if !(v > 0.0 && v.is_finite()) {
                    return Err(format!(
                        "工具 {name}{of} の{what}は 0 より大きくしてください"
                    ));
                }
            }
            if c.stepover > 1.0 {
                return Err(format!(
                    "工具 {name}{of} の送り幅は工具径の 1 倍以下にしてください"
                ));
            }
        }
        Ok(())
    }

    /// 先端から高さ `h` での工具の半径(工具の形を表す。シミュレーションに使う)。
    pub fn radius_at(&self, h: f64) -> Option<f64> {
        let r = self.radius();
        if h < 0.0 {
            return None;
        }
        Some(match self.kind {
            ToolKind::FlatEnd | ToolKind::Laser => r,
            // 上から見た高さの地図では、歯の横の空き(ねじ山の溝)は表せないので、首の柱だけを削る形とみなす
            ToolKind::ThreadMill => self.neck_radius(),
            ToolKind::BallEnd => {
                if h >= r {
                    r
                } else {
                    (r * r - (r - h) * (r - h)).max(0.0).sqrt()
                }
            }
            ToolKind::BullNose => {
                let rc = self.corner();
                if h >= rc {
                    r
                } else {
                    r - rc + (rc * rc - (rc - h) * (rc - h)).max(0.0).sqrt()
                }
            }
            ToolKind::Drill | ToolKind::VBit | ToolKind::TaperEnd => {
                let half = (self.tip_angle / 2.0).to_radians();
                (self.tip_radius() + h * half.tan()).min(r)
            }
            ToolKind::TaperBall => {
                let (rb, half) = (self.ball_radius(), (self.tip_angle / 2.0).to_radians());
                // 側面が球に接する高さと、その所の半径
                let (ht, rt) = (rb * (1.0 - half.sin()), rb * half.cos());
                if h < ht {
                    (rb * rb - (rb - h) * (rb - h)).max(0.0).sqrt()
                } else {
                    (rt + (h - ht) * half.tan()).min(r)
                }
            }
        })
    }

    /// 先端(いちばん下)の半径。平・ボール・ブルノーズは刃の半径、ドリルと V は 0、テーパーは
    /// 先の径の半分、テーパーボールは先の球の半径。
    /// 残り削りで、前の工具より細い所に届くかを比べるのに使う。
    pub fn tip_radius(&self) -> f64 {
        match self.kind {
            ToolKind::FlatEnd | ToolKind::BallEnd | ToolKind::BullNose | ToolKind::Laser => {
                self.radius()
            }
            ToolKind::ThreadMill => self.neck_radius(),
            ToolKind::Drill | ToolKind::VBit => 0.0,
            ToolKind::TaperEnd => (self.tip_diameter.unwrap_or(0.0) / 2.0).min(self.radius()),
            ToolKind::TaperBall => self.ball_radius(),
        }
    }

    /// 深さで太さが変わる工具(V、テーパー、テーパーボール、ドリル)か。
    pub fn is_tapered(&self) -> bool {
        matches!(
            self.kind,
            ToolKind::Drill | ToolKind::VBit | ToolKind::TaperEnd | ToolKind::TaperBall
        )
    }

    /// 絵に描く刃の外形の、先端から高さ `h` での半径。ねじ切りミルは歯の山(三角)を描き、
    /// ほかは [`Tool::radius_at`] と同じ。
    pub fn outline_at(&self, h: f64) -> Option<f64> {
        let Some(f) = self.thread_form() else {
            return self.radius_at(h);
        };
        if h < 0.0 {
            return None;
        }
        let (r, neck) = (self.radius(), self.neck_radius());
        let half = (self.tip_angle / 2.0).to_radians().tan().max(1e-3);
        let pitch = f.pitch.unwrap_or(f64::INFINITY);
        // 歯の高さの半分(山の斜面が首から刃の径まで上がる高さ)。歯の間隔より高くはしない
        let rise = ((r - neck) / half).min(pitch / 2.0);
        let teeth = f.teeth.max(1) as f64;
        let k = if pitch.is_finite() {
            (h / pitch).floor().min(teeth - 1.0)
        } else {
            0.0
        };
        let local = h - if pitch.is_finite() { k * pitch } else { 0.0 };
        Some(if local <= 2.0 * rise {
            r - (local - rise).abs() / rise * (r - neck)
        } else {
            neck
        })
    }

    /// ねじ切りミルの刃の外形の折れ目 `[先端からの高さ, 半径]`(下から順、刃長の高さまで)。
    /// 歯の山の先は細く尖るので、等しい間隔で拾うと山の先を外す。絵と立体はこの点で描く。
    /// ねじ切りミルでなければ `None`
    pub fn thread_outline(&self) -> Option<Vec<[f64; 2]>> {
        let f = self.thread_form()?;
        let lcf = self.flute_length.max(0.0);
        let half = (self.tip_angle / 2.0).to_radians().tan().max(1e-3);
        let (r, neck) = (self.radius(), self.neck_radius());
        let rise = ((r - neck) / half).min(f.pitch.unwrap_or(f64::INFINITY) / 2.0);
        let mut hs = Vec::new();
        for k in 0..f.teeth.max(1) {
            let base = k as f64 * f.pitch.unwrap_or(0.0);
            hs.extend([base, base + rise, base + 2.0 * rise]);
        }
        hs.push(lcf);
        hs.retain(|h| *h <= lcf);
        hs.sort_by(f64::total_cmp);
        hs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        Some(
            hs.into_iter()
                .map(|h| [h, self.outline_at(h).unwrap_or(neck)])
                .collect(),
        )
    }

    /// ねじ切りミルの山の形(ねじ切りミルでなければ `None`。形を書いていなければ 1 山)。
    pub fn thread_form(&self) -> Option<ThreadForm> {
        (self.kind == ToolKind::ThreadMill).then(|| self.thread.unwrap_or_default())
    }

    /// 刃が首より太い(上から抜けない)工具か。ねじ切りミルは歯が切った溝に入っているので、
    /// 真上へ上げると山を削る。削った所の真上が空いていると見る省き方やつなぎ方は使えない
    pub fn undercut(&self) -> bool {
        self.kind == ToolKind::ThreadMill
    }

    /// ねじ切りミルの首の半径(空なら刃の径の 0.65 倍の半分)。ほかの工具では刃の半径。
    pub fn neck_radius(&self) -> f64 {
        match self.thread_form() {
            Some(f) => (f.neck_diameter.unwrap_or(self.diameter * 0.65) / 2.0).min(self.radius()),
            None => self.radius(),
        }
    }

    /// ブルノーズの角 R(空や刃の半径を超える値は、刃の半径に収める)。
    fn corner(&self) -> f64 {
        self.corner_radius.unwrap_or(0.0).clamp(0.0, self.radius())
    }

    /// テーパーボールの先の球の半径(空なら 0、刃の半径を超えない)。
    fn ball_radius(&self) -> f64 {
        (self.tip_diameter.unwrap_or(0.0) / 2.0).clamp(0.0, self.radius())
    }

    /// 刃の上端からシャンクの始まりまでの首の上端(刃先から、mm)。首下長が空か刃長より
    /// 短ければ刃長。
    pub fn neck_top(&self) -> f64 {
        self.neck_length
            .unwrap_or(self.flute_length)
            .max(self.flute_length)
    }

    /// 中心から距離 `d` の所で、刃が先端よりどれだけ上にあるか。刃の外なら `None`。
    pub fn height_at(&self, d: f64) -> Option<f64> {
        let r = self.radius();
        if d > r {
            return None;
        }
        Some(match self.kind {
            ToolKind::FlatEnd | ToolKind::Laser => 0.0,
            // 首の柱だけ([`Tool::radius_at`] と同じ)
            ToolKind::ThreadMill if d > self.neck_radius() => return None,
            ToolKind::ThreadMill => 0.0,
            ToolKind::BallEnd => r - (r * r - d * d).max(0.0).sqrt(),
            ToolKind::BullNose => {
                let (rc, flat) = (self.corner(), r - self.corner());
                if d <= flat {
                    0.0
                } else {
                    rc - (rc * rc - (d - flat) * (d - flat)).max(0.0).sqrt()
                }
            }
            ToolKind::Drill | ToolKind::VBit | ToolKind::TaperEnd => {
                (d - self.tip_radius()).max(0.0) / (self.tip_angle / 2.0).to_radians().tan()
            }
            ToolKind::TaperBall => {
                let (rb, half) = (self.ball_radius(), (self.tip_angle / 2.0).to_radians());
                let (ht, rt) = (rb * (1.0 - half.sin()), rb * half.cos());
                if d <= rt {
                    rb - (rb * rb - d * d).max(0.0).sqrt()
                } else {
                    ht + (d - rt) / half.tan()
                }
            }
        })
    }

    /// 先端の円錐の高さ(ドリル、V ビット、テーパー、テーパーボール以外は 0)。
    pub fn tip_length(&self) -> f64 {
        match self.kind {
            ToolKind::Drill | ToolKind::VBit | ToolKind::TaperEnd | ToolKind::TaperBall => {
                self.height_at(self.radius()).unwrap_or(0.0)
            }
            _ => 0.0,
        }
    }
}

/// 工具の一覧。番号は重ならない。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ToolLibrary {
    pub tools: Vec<Tool>,
    /// 知らない項目。読んだまま書き戻す
    #[serde(flatten)]
    pub extra: Extra,
}

impl kmb_core::Payload for ToolLibrary {
    const TYPE: &'static str = TOOLS_TYPE;
}

impl ToolLibrary {
    pub fn get(&self, number: u32) -> Option<&Tool> {
        self.tools.iter().find(|t| t.number == number)
    }

    pub fn check(&self) -> Result<(), String> {
        for (i, t) in self.tools.iter().enumerate() {
            t.check()?;
            if self.tools[..i].iter().any(|o| o.number == t.number) {
                return Err(format!("工具番号 {} が重なっています", t.number));
            }
        }
        Ok(())
    }

    /// 出力の値にする。
    pub fn to_value(&self) -> Value {
        kmb_core::Payload::to_value(self)
    }

    /// 出力の値から読む。
    pub fn from_value(v: &Value) -> Option<ToolLibrary> {
        <ToolLibrary as kmb_core::Payload>::from_value(v)
    }

    /// 機能 `feature` の工具ライブラリから番号 `number` の工具を読む。
    pub fn tool_from(
        ctx: &EvalCtx<'_>,
        feature: FeatureId,
        number: u32,
    ) -> Result<Tool, EvalError> {
        let lib = ToolLibrary::from_value(ctx.output(feature, "tools")?).ok_or_else(|| {
            EvalError::InvalidData(format!("{feature} は工具ライブラリではありません"))
        })?;
        lib.get(number)
            .cloned()
            .ok_or_else(|| EvalError::InvalidData(format!("工具番号 {number} がありません")))
    }

    /// レーザーの工具の初めの形(番号 0。足す時に空いている番号を振る)。光の点の径(切り口の幅)0.01 mm、
    /// 出力 50 %(切削条件の回転数の欄)、速さ 1200 mm/min。値は目安で、材料と機械に合わせて直す(A)。
    pub fn laser_tool() -> Tool {
        Tool {
            number: 0,
            name: "レーザー".into(),
            kind: ToolKind::Laser,
            diameter: 0.01,
            flute_length: 1.0,
            flutes: 1,
            tip_angle: 118.0,
            shank_diameter: None,
            overall_length: None,
            tip_diameter: None,
            stickout: None,
            holder_diameter: None,
            corner_radius: None,
            neck_length: None,
            thread: None,
            presets: vec![Preset {
                material: String::new(),
                cutting: Cutting {
                    rpm: 50.0,
                    feed: 1200.0,
                    plunge: 1200.0,
                    stepdown: 1.0,
                    stepover: 1.0,
                },
            }],
            extra: Extra::new(),
        }
    }

    /// 始めに入れておく工具。値は小型の機械向けの控えめな目安(A)。
    pub fn starter() -> ToolLibrary {
        let c = |rpm, feed, plunge, stepdown, stepover| Cutting {
            rpm,
            feed,
            plunge,
            stepdown,
            stepover,
        };
        let presets = |wood: Cutting, acrylic: Cutting, aluminum: Cutting| {
            [("木材", wood), ("アクリル", acrylic), ("アルミ", aluminum)]
                .into_iter()
                .map(|(m, cutting)| Preset {
                    material: m.into(),
                    cutting,
                })
                .collect()
        };
        let flat = |number, name: &str, diameter, shank, flute_length, p: Vec<Preset>| Tool {
            number,
            name: name.into(),
            kind: ToolKind::FlatEnd,
            diameter,
            flute_length,
            flutes: 2,
            tip_angle: 118.0,
            shank_diameter: Some(shank),
            overall_length: None,
            tip_diameter: None,
            stickout: None,
            holder_diameter: None,
            corner_radius: None,
            neck_length: None,
            thread: None,
            presets: p,
            extra: Extra::new(),
        };
        ToolLibrary {
            tools: vec![
                flat(
                    1,
                    "平 6 mm",
                    6.0,
                    6.0,
                    20.0,
                    presets(
                        c(10000.0, 900.0, 250.0, 1.5, 0.6),
                        c(10000.0, 800.0, 200.0, 1.0, 0.5),
                        c(10000.0, 400.0, 100.0, 0.3, 0.4),
                    ),
                ),
                flat(
                    2,
                    "平 3.175 mm",
                    3.175,
                    3.175,
                    17.0,
                    presets(
                        c(12000.0, 600.0, 200.0, 1.0, 0.4),
                        c(12000.0, 500.0, 150.0, 0.6, 0.4),
                        c(12000.0, 300.0, 80.0, 0.2, 0.3),
                    ),
                ),
                flat(
                    3,
                    "平 2 mm",
                    2.0,
                    3.175,
                    12.0,
                    presets(
                        c(12000.0, 400.0, 150.0, 0.6, 0.4),
                        c(12000.0, 350.0, 100.0, 0.4, 0.4),
                        c(12000.0, 200.0, 60.0, 0.15, 0.3),
                    ),
                ),
                Tool {
                    kind: ToolKind::BallEnd,
                    ..flat(
                        4,
                        "ボール 3.175 mm",
                        3.175,
                        3.175,
                        12.0,
                        presets(
                            c(12000.0, 800.0, 200.0, 1.0, 0.15),
                            c(12000.0, 600.0, 150.0, 0.6, 0.15),
                            c(12000.0, 400.0, 80.0, 0.2, 0.1),
                        ),
                    )
                },
                Tool {
                    kind: ToolKind::VBit,
                    flutes: 1,
                    tip_angle: 90.0,
                    ..flat(
                        5,
                        "V 90° 3.175 mm",
                        3.175,
                        3.175,
                        4.0,
                        presets(
                            c(12000.0, 600.0, 150.0, 0.5, 0.3),
                            c(12000.0, 500.0, 100.0, 0.4, 0.3),
                            c(12000.0, 300.0, 60.0, 0.15, 0.2),
                        ),
                    )
                },
            ],
            extra: Extra::new(),
        }
    }

    /// `other`(PC 全体のライブラリなど)と同じ番号の工具の中身が違う番号、
    /// および片方にしかない番号。印を出すのに使う。
    pub fn differs_from(&self, other: &ToolLibrary) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for t in &self.tools {
            if other.get(t.number) != Some(t) {
                out.push(t.number);
            }
        }
        for t in &other.tools {
            if self.get(t.number).is_none() {
                out.push(t.number);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// 番号 `number` の工具を `other` の中身にする(`other` になければ消す)。
    pub fn take_from(&mut self, other: &ToolLibrary, number: u32) {
        self.tools.retain(|t| t.number != number);
        if let Some(t) = other.get(number) {
            self.tools.push(t.clone());
            self.tools.sort_by_key(|t| t.number);
        }
    }

    /// 工具の欄の候補(番号と「T1 平 6 mm」の形の名前)。
    pub fn choices(&self) -> Value {
        kmb_core::choices_value(
            self.tools
                .iter()
                .map(|t| kmb_core::Choice {
                    value: t.number.to_string(),
                    label: format!("T{} {}", t.number, t.name),
                })
                .collect(),
        )
    }

    /// 空いている最小の工具番号。
    pub fn next_number(&self) -> u32 {
        (1..).find(|n| self.get(*n).is_none()).unwrap_or(1)
    }

    /// `tools` を足す。番号は、まだ使っていなければそのまま、使っていれば空いている最小の番号に
    /// する。`skip_same_name` なら、同じ名前の工具がもうあるものは足さない。足した工具の番号を返す。
    pub fn add_tools(&mut self, tools: Vec<Tool>, skip_same_name: bool) -> Vec<u32> {
        let mut added = Vec::new();
        for mut t in tools {
            if skip_same_name && self.tools.iter().any(|o| o.name == t.name) {
                continue;
            }
            if t.number == 0 || self.get(t.number).is_some() {
                t.number = self.next_number();
            }
            added.push(t.number);
            self.tools.push(t);
        }
        self.tools.sort_by_key(|t| t.number);
        added
    }

    /// 一覧の順に、`start` から `step` ずつ番号を振り直す。古い番号から新しい番号への対応を返す
    /// (番号が変わらない工具も含む)。機械の工具番号の範囲(1〜99)を超えるなら誤り。
    pub fn renumber(&mut self, start: u32, step: u32) -> Result<Vec<(u32, u32)>, String> {
        if start == 0 || step == 0 {
            return Err("開始の番号と増分は 1 以上にしてください".into());
        }
        let last = start as u64 + step as u64 * self.tools.len().saturating_sub(1) as u64;
        if last > 99 {
            return Err(format!(
                "最後の工具の番号が {last} になります(工具番号は 1〜99)。開始か増分を小さくしてください"
            ));
        }
        let map: Vec<(u32, u32)> = self
            .tools
            .iter()
            .enumerate()
            .map(|(i, t)| (t.number, start + step * i as u32))
            .collect();
        for (t, (_, n)) in self.tools.iter_mut().zip(&map) {
            t.number = *n;
        }
        Ok(map)
    }
}

/// 工具ライブラリの機能。データは [`ToolLibrary`] そのもの。
struct ToolsFeature(ToolLibrary);

impl Feature for ToolsFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }

    fn evaluate(&self, _ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        self.0.check().map_err(EvalError::InvalidData)?;
        let mut out = Outputs::new();
        out.insert("tools".into(), self.0.to_value());
        out.insert(TOOL_CHOICES.into(), self.0.choices());
        Ok(out)
    }
}

fn tools_factory(d: &serde_json::Value) -> Result<Box<dyn Feature>, EvalError> {
    let lib: ToolLibrary =
        serde_json::from_value(d.clone()).map_err(|e| EvalError::InvalidData(e.to_string()))?;
    Ok(Box::new(ToolsFeature(lib)))
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        kmb_core::Descriptor::new(TOOLS, kmb_core::category::TOOLS, "工具ライブラリ")
            .icon("tools")
            .hint("使う工具の形と、材料ごとの切削条件")
            // 文書に無い時、リボンの「工具ライブラリ」から足せるように
            .template(serde_json::to_value(ToolLibrary::starter()).expect("JSON にできる")),
        tools_factory,
    );
    reg.register_described(
        kmb_core::Descriptor::new(
            TOOLS_V1,
            kmb_core::category::TOOLS,
            "工具ライブラリ(古い形式)",
        )
        .hint("前の版の文書の工具ライブラリ。読み込むと材料を問わない条件が 1 つの工具になる"),
        tools_factory,
    );
}

/// この crate の名乗り(登録する種類)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cam-tool", register)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_library_is_valid_and_round_trips() {
        let lib = ToolLibrary::starter();
        lib.check().unwrap();
        let v = lib.to_value();
        assert_eq!(ToolLibrary::from_value(&v).unwrap(), lib);
        let json = serde_json::to_value(&lib).unwrap();
        let mut reg = Registry::new();
        register(&mut reg);
        assert!(reg.instantiate(TOOLS, &json).unwrap().is_ok());
    }

    #[test]
    fn material_aliases_match() {
        assert!(same_material("aluminum", "アルミ"));
        assert!(same_material(" Aluminium ", "アルミ"));
        assert!(same_material("MDF", "mdf"));
        assert!(!same_material("aluminum", "真鍮"));
        assert_eq!(material("wood").map(|m| m.name), Some("木材"));
    }

    #[test]
    fn every_cam_material_is_a_physical_material() {
        // 設計でボディに付けた材料の名前が、そのまま加工の材料になる(M26)
        for m in MATERIALS {
            let p = kmb_look::material::phys_material(m.name);
            assert_eq!(p.map(|p| p.name), Some(m.name));
        }
    }

    #[test]
    fn duplicate_numbers_and_bad_sizes_are_rejected() {
        let mut lib = ToolLibrary::starter();
        lib.tools[1].number = 1;
        assert!(lib.check().unwrap_err().contains("重なって"));
        let mut lib = ToolLibrary::starter();
        lib.tools[0].diameter = 0.0;
        assert!(lib.check().is_err());
        lib.tools[0].diameter = 6.0;
        lib.tools[0].presets[1].cutting.stepover = 1.5;
        assert!(lib.check().is_err());
    }

    #[test]
    fn tool_shapes() {
        let mut t = ToolLibrary::starter().tools[0].clone();
        assert_eq!(t.height_at(2.0), Some(0.0));
        assert_eq!(t.height_at(3.5), None);
        t.kind = ToolKind::BallEnd;
        assert!((t.height_at(3.0).unwrap() - 3.0).abs() < 1e-12);
        assert!((t.radius_at(3.0).unwrap() - 3.0).abs() < 1e-12);
        t.kind = ToolKind::Drill;
        t.tip_angle = 90.0;
        assert!((t.height_at(3.0).unwrap() - 3.0).abs() < 1e-12);
        assert!((t.tip_length() - 3.0).abs() < 1e-12);
        // テーパー: 先の径 1 mm、片側 5°(全角 10°)、上端の径 6 mm
        t.kind = ToolKind::TaperEnd;
        t.tip_angle = 10.0;
        t.tip_diameter = Some(1.0);
        t.check().unwrap();
        let tan = 5f64.to_radians().tan();
        assert_eq!(t.tip_radius(), 0.5);
        assert_eq!(t.height_at(0.4), Some(0.0));
        assert!((t.height_at(1.5).unwrap() - 1.0 / tan).abs() < 1e-9);
        assert!((t.radius_at(2.0).unwrap() - (0.5 + 2.0 * tan)).abs() < 1e-12);
        assert_eq!(t.radius_at(100.0), Some(3.0));
        assert!((t.tip_length() - 2.5 / tan).abs() < 1e-9);
        t.tip_diameter = None;
        assert!(t.check().unwrap_err().contains("先の径"));
        t.tip_diameter = Some(1.0);
        t.stickout = Some(1.0);
        assert!(t.check().unwrap_err().contains("突き出し"));
        // Z13: 全長は刃長と突き出しより短くできない
        t.stickout = None;
        t.overall_length = Some(t.flute_length / 2.0);
        assert!(t.check().unwrap_err().contains("全長"));
        t.overall_length = Some(t.flute_length * 3.0);
        t.check().unwrap();
    }

    #[test]
    fn bull_nose_and_taper_ball_shapes() {
        // ブルノーズ: 径 6 mm、角 R 1 mm。中心から 2 mm までは平ら、外は角の円
        let mut t = ToolLibrary::starter().tools[0].clone();
        t.kind = ToolKind::BullNose;
        assert!(t.check().unwrap_err().contains("角 R"));
        t.corner_radius = Some(1.0);
        t.check().unwrap();
        assert_eq!(t.height_at(2.0), Some(0.0));
        assert!((t.height_at(3.0).unwrap() - 1.0).abs() < 1e-12);
        assert!((t.height_at(2.5).unwrap() - (1.0 - 0.75f64.sqrt())).abs() < 1e-12);
        assert_eq!(t.radius_at(0.0), Some(2.0));
        assert_eq!(t.radius_at(1.0), Some(3.0));
        assert!((t.radius_at(0.5).unwrap() - (2.0 + 0.75f64.sqrt())).abs() < 1e-12);
        assert_eq!(
            (t.tip_radius(), t.tip_length(), t.is_tapered()),
            (3.0, 0.0, false)
        );
        t.corner_radius = Some(3.5);
        assert!(t.check().is_err());
        // テーパーボール: 先の球の径 1 mm、側面の開き 10°(片側 5°)、上端の径 6 mm
        let mut t = ToolLibrary::starter().tools[0].clone();
        t.kind = ToolKind::TaperBall;
        t.tip_angle = 10.0;
        assert!(t.check().unwrap_err().contains("先の球"));
        t.tip_diameter = Some(1.0);
        t.flute_length = 40.0;
        t.check().unwrap();
        let a = 5f64.to_radians();
        let (ht, rt) = (0.5 * (1.0 - a.sin()), 0.5 * a.cos());
        assert_eq!(t.height_at(0.0), Some(0.0));
        assert!((t.height_at(0.3).unwrap() - (0.5 - 0.16f64.sqrt())).abs() < 1e-12);
        // 球と側面はつながる(接する所で高さと半径が一致する)
        assert!((t.height_at(rt).unwrap() - ht).abs() < 1e-12);
        assert!((t.radius_at(ht).unwrap() - rt).abs() < 1e-12);
        assert!((t.height_at(2.0).unwrap() - (ht + (2.0 - rt) / a.tan())).abs() < 1e-9);
        assert!((t.radius_at(10.0).unwrap() - (rt + (10.0 - ht) * a.tan())).abs() < 1e-12);
        assert_eq!(t.radius_at(100.0), Some(3.0));
        assert_eq!(t.tip_radius(), 0.5);
        assert!(t.is_tapered());
        // 形の式どうしが逆になっている(高さ → 半径 → 高さ)
        for h in [0.01, 0.1, 0.3, 1.0, 5.0, 20.0] {
            let r = t.radius_at(h).unwrap();
            assert!((t.height_at(r).unwrap() - h).abs() < 1e-9, "{h}");
        }
    }

    #[test]
    fn overall_and_neck_lengths_are_checked() {
        let mut t = ToolLibrary::starter().tools[1].clone();
        t.overall_length = Some(38.0);
        t.neck_length = Some(26.0);
        t.check().unwrap();
        assert_eq!(t.neck_top(), 26.0);
        t.neck_length = Some(10.0);
        assert!(t.check().unwrap_err().contains("首下長"));
        t.neck_length = Some(40.0);
        assert!(t.check().unwrap_err().contains("全長"));
        t.neck_length = None;
        assert_eq!(t.neck_top(), t.flute_length);
        t.stickout = Some(39.0);
        assert!(t.check().unwrap_err().contains("全長"));
        // 新しい項目は空なら書かない(前の版でも読める)
        let v = serde_json::to_value(ToolLibrary::starter()).unwrap();
        assert!(v["tools"][0].get("corner_radius").is_none());
        assert!(v["tools"][0].get("neck_length").is_none());
    }

    #[test]
    fn tools_are_renumbered_in_list_order() {
        let mut lib = ToolLibrary::starter();
        let map = lib.renumber(10, 5).unwrap();
        assert_eq!(map, vec![(1, 10), (2, 15), (3, 20), (4, 25), (5, 30)]);
        assert_eq!(lib.get(15).unwrap().name, "平 3.175 mm");
        lib.check().unwrap();
        assert!(lib.renumber(90, 5).unwrap_err().contains("1〜99"));
        assert!(lib.renumber(0, 1).is_err());
    }

    #[test]
    fn old_libraries_load_with_one_preset_and_keep_unknown_items() {
        let old = serde_json::json!({"tools": [{
            "number": 5, "name": "古い", "kind": "flat_end", "diameter": 4.0, "flute_length": 10.0,
            "cutting": {"rpm": 9000.0, "feed": 500.0, "plunge": 100.0, "stepdown": 0.8, "stepover": 0.3},
            "coating": "TiAlN"
        }], "vendor": "x"});
        let lib: ToolLibrary = serde_json::from_value(old).unwrap();
        lib.check().unwrap();
        let t = &lib.tools[0];
        assert_eq!(t.presets.len(), 1);
        assert_eq!(t.cutting().rpm, 9000.0);
        assert_eq!(t.shank(), 4.0);
        let back = serde_json::to_value(&lib).unwrap();
        assert_eq!(back["vendor"], "x");
        assert_eq!(back["tools"][0]["coating"], "TiAlN");
        assert!(back["tools"][0].get("cutting").is_none());
        assert_eq!(back["tools"][0]["presets"][0]["feed"], 500.0);
        // 古い種類 ID でも評価できる
        let mut reg = Registry::new();
        register(&mut reg);
        assert!(reg.instantiate(TOOLS_V1, &back).unwrap().is_ok());
    }

    #[test]
    fn the_preset_follows_the_material() {
        let t = ToolLibrary::starter().tools[1].clone();
        let (c, w) = t.cutting_for("アルミ");
        assert_eq!((c.feed, w), (300.0, None));
        let (c, w) = t.cutting_for(" アクリル ");
        assert_eq!((c.feed, w), (500.0, None));
        let (c, w) = t.cutting_for("");
        assert_eq!((c.feed, w), (600.0, None));
        let (c, w) = t.cutting_for("チタン");
        assert_eq!(c.feed, 600.0);
        assert!(w.unwrap().contains("チタン"));
    }

    #[test]
    fn chip_load_is_checked_against_the_material() {
        let lib = ToolLibrary::starter();
        // 始めの工具の条件は、どれも目安の中にある
        for t in &lib.tools {
            for p in &t.presets {
                assert_eq!(
                    t.chip_load_warning(&p.cutting, &p.material),
                    None,
                    "{} {}",
                    t.name,
                    p.material
                );
            }
        }
        let t = &lib.tools[1];
        let mut c = t.cutting_for("アルミ").0;
        assert!((c.chip_load(2) - 0.0125).abs() < 1e-12);
        c.feed = 2000.0;
        assert!(
            t.chip_load_warning(&c, "アルミ")
                .unwrap()
                .contains("大きすぎ")
        );
        c.feed = 50.0;
        assert!(
            t.chip_load_warning(&c, "アルミ")
                .unwrap()
                .contains("小さすぎ")
        );
        assert_eq!(t.chip_load_warning(&c, "知らない材料"), None);
    }

    #[test]
    fn libraries_are_compared_and_synced_by_tool_number() {
        let global = ToolLibrary::starter();
        let mut doc = global.clone();
        assert!(doc.differs_from(&global).is_empty());
        doc.tools[0].presets[0].cutting.feed = 1.0;
        doc.tools.remove(3);
        assert_eq!(doc.differs_from(&global), vec![1, 4]);
        doc.take_from(&global, 1);
        doc.take_from(&global, 4);
        assert_eq!(doc, global);
        assert_eq!(doc.next_number(), 6);
    }
}
