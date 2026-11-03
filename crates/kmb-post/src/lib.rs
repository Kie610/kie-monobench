//! ポストの trait、機械の値(機械プロファイル)、汎用のポスト(generic・svg)とマシンライブラリ(machines)。
//!
//! ポストは [`Program`] を文字列にするだけで、経路を変えない(円弧を半周以下に分けるような、
//! 形の変わらない書き換えだけを行う)。出力の前に [`Post::check`] で機械の範囲を確かめる。

use kmb_toolpath::{Motion, Op, Program};
use serde::{Deserialize, Serialize};
use std::fmt;

pub mod generic;
pub mod machines;
pub mod svg;
pub mod writer;

/// 機械の能力と、出力の選び方。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineProfile {
    /// マシンライブラリの ID([`machines`])。セットアップはこれで機械を選ぶ
    #[serde(default)]
    pub id: String,
    /// 書き出す内蔵のポストの ID(`carvera-air`・`generic-fanuc`・`generic-grbl`)
    #[serde(default)]
    pub post: String,
    pub name: String,
    /// 動かせる範囲(mm)
    pub travel: [f64; 3],
    pub max_rpm: f64,
    pub min_rpm: f64,
    /// 使える工具番号の範囲
    pub tool_numbers: (u32, u32),
    /// 早送りの速さの目安(mm/min、時間の見積もりに使う)
    pub rapid_feed: f64,
    /// 加速度(mm/s²、時間の見積もりに使う)。0 なら加減速を見込まない
    #[serde(default)]
    pub acceleration: f64,
    /// 加工中の消費電力(W)。0 は不明で、段取り表の電気代に含めない
    #[serde(default, skip_serializing_if = "is_zero")]
    pub power_w: f64,
    /// 角を回る速さを決める値(mm、junction deviation。時間の見積もりに使う)
    #[serde(default)]
    pub junction_deviation: f64,
    /// 軸ごとの最高速(mm/min、時間の見積もりに使う)。0 の軸は制限しない
    #[serde(default)]
    pub max_rate: [f64; 3],
    /// 工具交換の後に工具長を測り直す命令を出すか
    pub measure_after_tool_change: bool,
    /// 主軸を回している間、エアを出すか
    pub air: bool,
    /// 使えるコレットの径(mm)。最初のものが標準で付いているもの。空なら調べない
    #[serde(default)]
    pub collets: Vec<f64>,
    /// ベッド(ネジ穴と、原点・工具長センサーの位置)。分からなければ `None`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed: Option<Bed>,
    /// 機械の形(作業範囲の置き場所・主軸の頭・門の下の高さ)。模擬で当たりとはみ出しを見る。
    /// 分からなければ `None`(模擬は機械の形との当たりを見ない)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<MachineFrame>,
    /// 自動で工具を替えられる本数(0 なら手で替える)
    #[serde(default)]
    pub atc_slots: u32,
    /// 機械の外形(mm、幅・奥行き・高さ)。置き場所の目安
    #[serde(default)]
    pub outer: [f64; 3],
    /// 原点復帰の位置の説明(例: 「右奥・上」)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub home: String,
    /// 使える作業座標系の数(1 なら G54 だけ、6 なら G54〜G59)
    #[serde(default = "six")]
    pub wcs_count: u32,
    /// O 番号の行を書けるか(書けなければコメントにする)
    #[serde(default)]
    pub program_number: bool,
    /// 任意停止(M1)を書けるか(書けなければ必ず止まる停止にする)
    #[serde(default)]
    pub optional_stop: bool,
    /// レーザー(付けられなければ `None`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub laser: Option<Laser>,
    /// プローブで表面の高さを格子状に測って補正する命令(なければ `None`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leveling: Option<Leveling>,
    /// 負荷制御粗取りの、溝で削る時の 1 段の深さの表(材料ごと)。空なら工具の条件を使う
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_stepdown: Vec<SlotStepdown>,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

fn six() -> u32 {
    6
}

/// 機械のレーザー。出力は 0〜1 を `s_max` 倍して S に書く。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Laser {
    /// 出力 100% の S の値(例: Smoothieware 系は 1、Grbl は 1000)
    pub s_max: f64,
    /// レーザーに替える命令(工具交換の代わり)
    #[serde(default)]
    pub enter: Vec<String>,
    /// 削る工具に戻す命令
    #[serde(default)]
    pub exit: Vec<String>,
    /// 焼き始める命令(S と一緒に書く)。空なら送りの行に S を付けるだけ
    #[serde(default)]
    pub on: String,
    /// 焼くのをやめる命令。空なら書かない(早送りでは焼かない機械)
    #[serde(default)]
    pub off: String,
    /// 止まったまま焼く(貫通の時間)の命令。`{s}` を S の値、`{t}` を秒に置き換え、行ごとに書く。
    /// `None` なら止まったままでは焼けない機械(動く間だけ焼く)で、貫通の時間を誤りにする
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pierce: Option<Vec<String>>,
    /// 光を動かす仕組み
    #[serde(default, skip_serializing_if = "LaserDrive::is_gantry")]
    pub drive: LaserDrive,
    /// 値の確かさや設定の注意
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// レーザーの光を動かす仕組み。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaserDrive {
    /// 軸が頭を動かす(ダイオードのレーザー、CNC の付属品)。加減速があるので、塗りの行の外へ
    /// はみ出して焼きむらを防ぐ
    #[default]
    Gantry,
    /// 2 枚の鏡(ガルバノ)で光を振る。鏡は軽く、はみ出しは要らない。焼き始め・終わり・角の
    /// 遅れ(TC)とレンズのゆがみの補正は、機械の制御盤とそれを動かすソフト(LightBurn など)が持つ
    Galvo,
}

impl LaserDrive {
    pub fn is_gantry(&self) -> bool {
        *self == LaserDrive::Gantry
    }
}

/// ポストの書き出すファイルの拡張子(G-code は `nc`)。
pub fn file_extension(post: &str) -> &'static str {
    if post == machines::LIGHTBURN_SVG {
        "svg"
    } else {
        "nc"
    }
}

/// 高さの補正の命令。`probe` の `{x}`・`{y}`(測る範囲の左手前)、`{w}`・`{l}`(幅と奥行き)、
/// `{nx}`・`{ny}`(格子の点の数)、`{h}`(点の間を動く高さ)を値に置き換えて書く。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Leveling {
    pub probe: String,
    /// 補正をやめる命令(プログラムの終わりに書く)。空なら書かない
    #[serde(default)]
    pub clear: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl Leveling {
    /// 格子 `g` で測る命令の行。
    pub fn probe_line(&self, g: &kmb_toolpath::LevelGrid) -> String {
        self.probe
            .replace("{x}", &num(g.x))
            .replace("{y}", &num(g.y))
            .replace("{w}", &num(g.width))
            .replace("{l}", &num(g.length))
            .replace("{nx}", &g.nx.to_string())
            .replace("{ny}", &g.ny.to_string())
            .replace("{h}", &num(g.height))
    }
}

/// 材料ごとの、溝で削る時の 1 段の深さ(mm)。`depths` は工具径 2.5 mm 以上・1.75 mm 以上・
/// それより細い時。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotStepdown {
    pub material: String,
    pub depths: [f64; 3],
}

/// 機械のベッド。座標はベッドの左手前の角から(+X 右、+Y 奥、mm)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bed {
    /// 大きさ(幅、奥行き、厚さ)
    pub size: [f64; 3],
    /// 材料の名前(例: MDF)
    pub material: String,
    /// ネジ穴の呼び(例: M5)と、描く径
    pub thread: String,
    pub hole_diameter: f64,
    pub holes: Vec<[f64; 2]>,
    /// 作業の原点(素材を押し当てる L 字の角)の位置
    pub origin: [f64; 2],
    /// L 字の基準の腕(左手前の角からの長さと幅)。なければ描かない
    #[serde(default)]
    pub fence: Option<[f64; 2]>,
    /// 工具長センサーの位置(作業の原点から)
    #[serde(default)]
    pub sensor: Option<[f64; 2]>,
    /// 値の確かさの注意(推定の値など)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// 機械の形。X・Y はベッドの左手前の角から、高さはベッドの上面から(mm)。主軸の頭は、主軸の
/// 鼻(コレットナットの付け根)の中心から上へ、丸い主軸と、その上の Z の台の箱の 2 つで表す。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineFrame {
    /// 主軸の中心が動ける範囲の、手前左の角(X・Y)。大きさは [`MachineProfile::travel`]
    pub travel_corner: [f64; 2],
    /// 主軸の鼻のいちばん下の高さ。いちばん上はこれに作業範囲 Z を足した所
    pub nose_low: f64,
    /// 主軸の丸い所の径と長さ(鼻から上)。径が 0 なら見ない
    pub spindle: [f64; 2],
    /// 主軸の上の Z の台の箱の幅(X)・奥行き(Y)・高さ(主軸の中心に置く)。幅が 0 なら見ない
    pub carriage: [f64; 3],
    /// 門(ガントリー)の下の高さ。ストックと固定具はこれより低くないと通らない。0 なら見ない
    #[serde(default)]
    pub clearance: f64,
    /// 刃がベッドを削ってよい深さ(捨て板の貫通の分)。これより深い食い込みを当たりにする
    #[serde(default)]
    pub bed_allowance: f64,
    /// 値の確かさの注意(推定の値など)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl MachineProfile {
    /// 材料 `material`(名前は呼び名をそろえたもの)と工具径の、溝の 1 段の深さ。表になければ `None`。
    pub fn slot_stepdown(&self, material: &str, diameter: f64) -> Option<f64> {
        let row = self
            .slot_stepdown
            .iter()
            .find(|r| r.material == material.trim())?;
        Some(if diameter >= 2.5 {
            row.depths[0]
        } else if diameter >= 1.75 {
            row.depths[1]
        } else {
            row.depths[2]
        })
    }

    /// 時間の見積もりに使う動きの性質。
    pub fn motion(&self) -> Motion {
        Motion {
            rapid_feed: self.rapid_feed,
            acceleration: self.acceleration,
            junction_deviation: self.junction_deviation,
            max_rate: self.max_rate,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self.severity {
            Severity::Error => "誤り",
            Severity::Warning => "注意",
        };
        write!(f, "{s}: {}", self.message)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PostError(pub Vec<Diagnostic>);

impl fmt::Display for PostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, d) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{d}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PostError {}

pub trait Post {
    fn id(&self) -> &str;
    /// 出力の前の検査。誤りがあれば出力しない。
    fn check(&self, p: &Program, m: &MachineProfile) -> Vec<Diagnostic> {
        check_common(p, m)
    }
    fn emit(&self, p: &Program, m: &MachineProfile) -> Result<String, PostError>;
}

/// どの機械にも共通の検査: 動く範囲、回転数、工具番号。
pub fn check_common(p: &Program, m: &MachineProfile) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let err = |message: String| Diagnostic {
        severity: Severity::Error,
        message,
    };
    if let Some((min, max)) = p.extents() {
        for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
            let span = max[i] - min[i];
            if span > m.travel[i] {
                out.push(err(format!(
                    "{axis} の動く幅 {span:.1} mm が {} の範囲 {} mm を超えています",
                    m.name, m.travel[i]
                )));
            }
        }
    }
    let laser = p.ops.iter().any(|o| matches!(o, Op::LaserOn { .. }));
    if laser && m.laser.is_none() {
        out.push(err(format!("{} にはレーザーがありません", m.name)));
    }
    if laser && m.laser.as_ref().is_some_and(|l| l.pierce.is_none()) && pierces(p) {
        out.push(err(format!(
            "{} のレーザーは動く間だけ焼くので、止まったまま焼く「貫通の時間」は使えません。0 にするか、進入を付けてください",
            m.name
        )));
    }
    if p.leveling.is_some() && m.leveling.is_none() {
        // セットアップの注意と同じ文(加工の画面で 2 回並べない)
        out.push(err(format!(
            "{} は G-code で高さを補正する命令を持ちません。「コントローラーで測る」にするか、補正の命令のある機械を選んでください",
            m.name
        )));
    }
    for op in &p.ops {
        match op {
            Op::Spindle { rpm } if *rpm > m.max_rpm || *rpm < m.min_rpm => {
                out.push(err(format!(
                    "回転数 {rpm} rpm は {} の範囲({}〜{} rpm)の外です",
                    m.name, m.min_rpm, m.max_rpm
                )));
            }
            Op::ToolChange { tool }
                if tool.number < m.tool_numbers.0 || tool.number > m.tool_numbers.1 =>
            {
                out.push(err(format!(
                    "工具番号 {} は {} で使えません({}〜{})",
                    tool.number, m.name, m.tool_numbers.0, m.tool_numbers.1
                )));
            }
            _ => {}
        }
    }
    out.extend(check_collets(p, m));
    if !p.ops.iter().any(|o| matches!(o, Op::Move(_))) {
        out.push(err("動きがありません".into()));
    }
    out
}

/// レーザーで焼きながら止まる所(貫通)があるか。
pub fn pierces(p: &Program) -> bool {
    let mut burning = false;
    p.ops.iter().any(|o| {
        match o {
            Op::LaserOn { power } => burning = *power > 0.0,
            Op::LaserOff => burning = false,
            Op::Dwell { .. } => return burning,
            _ => {}
        }
        false
    })
}

/// 作業座標系が機械の範囲にあるか(内蔵のポストの検査。プラグインは自分で決める)。
pub fn check_wcs(p: &Program, m: &MachineProfile) -> Vec<Diagnostic> {
    if p.wcs >= 1 && p.wcs <= m.wcs_count.max(1) {
        return Vec::new();
    }
    vec![Diagnostic {
        severity: Severity::Error,
        message: if m.wcs_count <= 1 {
            format!("{} で使える作業座標系は G54 だけです", m.name)
        } else {
            format!(
                "{} で使える作業座標系は G54〜G{} です",
                m.name,
                53 + m.wcs_count
            )
        },
    }]
}

/// 工具のシャンク径が機械のコレットに合うか。合わなければ注意、標準でないコレットが要れば知らせる。
pub fn check_collets(p: &Program, m: &MachineProfile) -> Vec<Diagnostic> {
    let Some(standard) = m.collets.first() else {
        return Vec::new();
    };
    let warn = |message: String| Diagnostic {
        severity: Severity::Warning,
        message,
    };
    let same = |a: f64, b: f64| (a - b).abs() < 0.01;
    let mut out = Vec::new();
    for t in p.tools() {
        let shank = t.shank();
        if !m.collets.iter().any(|c| same(*c, shank)) {
            let list: Vec<String> = m.collets.iter().map(|c| num(*c)).collect();
            out.push(warn(format!(
                "工具 T{} {} のシャンク径 {} mm に合うコレットが {} にありません(使える径: {} mm)",
                t.number,
                t.name,
                num(shank),
                m.name,
                list.join("・")
            )));
        } else if !same(*standard, shank) {
            out.push(warn(format!(
                "工具 T{} {} は {} mm のコレットに交換が要ります(標準は {} mm)",
                t.number,
                t.name,
                num(shank),
                num(*standard)
            )));
        }
    }
    out
}

/// 数を G-code の書式にする(小数 3 桁まで、末尾の 0 を省く)。
pub fn num(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    let r = if r == 0.0 { 0.0 } else { r };
    let s = format!("{r:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(2.50049), "2.5");
        assert_eq!(num(-12.3456), "-12.346");
        assert_eq!(num(0.1), "0.1");
    }
}
