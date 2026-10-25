//! 規格部品(ねじ・ナット・座金・玉軸受)を穴に挿す。
//!
//! - 規格部品の機能 `asm.fastener@1`([`Fastener`]): 種類・呼び・長さと、挿した穴の口の並び
//!   (中心と、面の外向きの法線)を持ち、穴ごとの形を `part:<番号>` の出力に出す。呼びと長さを
//!   変えると、挿した穴の全部で変わる。ねじ山は描かない(呼びの径の円柱)。
//! - ボディの機能 `asm.fastener_body@1`([`FastenerBody`]): 出力 1 つをボディとして出す。挿すと
//!   穴ごとに 1 つ足し、まとめて種類の名前の部品に入れる(穴のボディが部品に入っていれば、その中に)。
//! - 部品表は規格部品のボディを 1 個ずつ、今の呼びと長さの名前で数える([`crate::inspect`])。
//!
//! 呼びは、挿す時にクリックした穴の径に近い物を 1 回だけ選んで欄に入れる(穴の径を後から変えても
//! 呼びは追いかけない。部品に穴との関係を持たせない)。
//!
//! 規格は JIS のメートルねじ(並目と、六角ボルト・六角ナットの細目)。寸法は各 JIS の本体の値で、
//! 本体は ISO と同じ(JIS B 1176 = ISO 4762 など。下の各表)。名前は JIS の書き方で、並目はピッチを
//! 省き、細目は `M10×1.25` と書く(JIS B 0123)。
//!
//! 座金やナットを挿した穴の口に後からボルトなどを挿すと、その上に重ねる([`Stack`])。下の部品の
//! 呼びを変えると、上の部品も厚さの分だけ動く。

use crate::link::PART;
use jc_cad::{Exprs, expr_refs, kernel_err, length, parse, rename, solid, tag};
use jc_core::kernel::{
    self as k, BooleanOp, P2, P3, Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment,
};
use jc_core::{
    Choice, Context, EvalCtx, EvalError, Feature, FeatureId, Field, HandleChoice, InputRef,
    Outputs, Registry, ShapeHandle, Value, When, add_warnings, choices_value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const FASTENER: &str = "asm.fastener@1";
pub const FASTENER_BODY: &str = "asm.fastener_body@1";
/// 呼びの候補の出力(欄の一覧に出す)。
const SIZES: &str = "sizes";
/// 穴ごとの合い具合の出力(`fit0`, `fit1`, …)。合わなければ注意の文、合えば空。穴ごとのボディの
/// 機能が読んで、自分の注意にもする(タイムラインとブラウザのその穴の行に ⚠ が付く)。
const FIT: &str = "fit";
const FIT_TYPE: &str = "asm.fastener_fit@1";
/// 今の名前(「六角穴付きボルト M3×10(JIS B 1176)」)の出力。部品表と要約が読む。
pub const NAME: &str = "name";
const NAME_TYPE: &str = "asm.fastener_name@1";
/// 穴ごとの上の端の高さの出力(`top0`, `top1`, …)。穴の口の面から法線の向きに、部品のいちばん
/// 上(座金なら厚さ、ナットなら高さの上)までの距離。上に重ねた規格部品が読む。
const TOP: &str = "top";
const TOP_TYPE: &str = "asm.fastener_top@1";

/// 規格部品の種類。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// 六角穴付きボルト(JIS B 1176 = ISO 4762)
    #[default]
    SocketHead,
    /// 六角ボルト(全ねじ。JIS B 1180 = ISO 4017、細目は ISO 8676)
    HexBolt,
    /// 十字穴付きなべ小ねじ(JIS B 1111。寸法は国内で売られている形)
    PanHead,
    /// 十字穴付き皿小ねじ(JIS B 1111。頭は 90° の皿で、穴の皿に座る)
    FlatHead,
    /// 六角ナット(1 種。JIS B 1181 = ISO 4032、細目は ISO 8673)
    HexNut,
    /// 平座金(並形。JIS B 1256 = ISO 7089)
    Washer,
    /// 深溝玉軸受(シールド付きの外形。JIS B 1521)
    Bearing,
}

/// 種類の欄の候補(名前と JIS の番号)。
pub const KINDS: &[(&str, &str)] = &[
    ("socket_head", "六角穴付きボルト(JIS B 1176)"),
    ("hex_bolt", "六角ボルト(JIS B 1180)"),
    ("pan_head", "なべ小ねじ(JIS B 1111)"),
    ("flat_head", "皿小ねじ(JIS B 1111)"),
    ("hex_nut", "六角ナット(JIS B 1181)"),
    ("washer", "平座金(JIS B 1256)"),
    ("bearing", "玉軸受(JIS B 1521)"),
];

impl Kind {
    /// 名前(「六角穴付きボルト」)。
    pub fn label(self) -> &'static str {
        self.choice_label()
            .split_once('(')
            .map_or("規格部品", |(name, _)| name)
    }

    /// 規格の番号(「JIS B 1176」)。
    pub fn standard(self) -> &'static str {
        self.choice_label()
            .split_once('(')
            .map_or("", |(_, s)| s.trim_end_matches(')'))
    }

    fn choice_label(self) -> &'static str {
        let key = serde_json::to_value(self).expect("種類は文字にできる");
        KINDS.iter().find(|(v, _)| key == *v).map_or("", |(_, l)| l)
    }

    /// 細目のねじを選べる種類(JIS B 1180・B 1181 に細目の表がある)。
    pub fn has_fine(self) -> bool {
        matches!(self, Kind::HexBolt | Kind::HexNut)
    }

    /// 上にほかの規格部品を重ねられる種類(座金とナット)。
    pub fn stackable(self) -> bool {
        matches!(self, Kind::Washer | Kind::HexNut)
    }

    /// 長さを持つ種類(ボルトと小ねじ)。皿小ねじの長さは頭を含む全長、ほかは首の下の長さ。
    pub fn has_length(self) -> bool {
        matches!(
            self,
            Kind::SocketHead | Kind::HexBolt | Kind::PanHead | Kind::FlatHead
        )
    }

    /// 小ねじ(呼びは [`SCREWS`] の物だけ)。
    pub fn is_screw(self) -> bool {
        matches!(self, Kind::PanHead | Kind::FlatHead)
    }
}

/// メートルねじ 1 サイズの、ここで使う寸法(mm)。
#[derive(Debug, Clone, Copy)]
pub struct Metric {
    pub name: &'static str,
    /// 呼び径
    pub d: f64,
    /// 並目のピッチ(JIS B 0205)
    pub pitch: f64,
    /// 六角ボルト・六角ナットの細目のピッチ(JIS B 1180・B 1181 の表。無ければ空)
    pub fine: &'static [f64],
    /// 下穴の径(並目)
    pub tap: f64,
    /// 通し穴の径(ISO 273 の 精・中・粗)
    pub clearance: [f64; 3],
    /// 六角穴付きボルトの頭の径・頭の高さ・六角穴の二面幅・六角穴の深さ(ISO 4762)
    pub socket: [f64; 4],
    /// 六角ボルトの頭の二面幅・頭の高さ(ISO 4017)
    pub hex_head: [f64; 2],
    /// 六角ナットの二面幅・高さ(ISO 4032)
    pub nut: [f64; 2],
    /// 平座金の内径・外径・厚さ(ISO 7089)
    pub washer: [f64; 3],
}

#[allow(clippy::too_many_arguments)]
const fn m(
    name: &'static str,
    d: f64,
    pitch: f64,
    fine: &'static [f64],
    tap: f64,
    clearance: [f64; 3],
    socket: [f64; 4],
    hex_head: [f64; 2],
    nut: [f64; 2],
    washer: [f64; 3],
) -> Metric {
    Metric {
        name,
        d,
        pitch,
        fine,
        tap,
        clearance,
        socket,
        hex_head,
        nut,
        washer,
    }
}

/// JIS のメートル並目の表(M1.6〜M20。JIS B 0205 の第 1 選択と M14)。値は各 JIS の本体(= ISO)の
/// 呼びの寸法(最大と最小の間の代表の値)。
#[rustfmt::skip]
pub const METRIC: &[Metric] = &[
    m("M1.6", 1.6, 0.35, &[], 1.25, [1.7, 1.8, 2.0], [3.0, 1.6, 1.5, 0.7], [3.2, 1.1], [3.2, 1.3], [1.7, 4.0, 0.3]),
    m("M2", 2.0, 0.4, &[], 1.6, [2.2, 2.4, 2.6], [3.8, 2.0, 1.5, 1.0], [4.0, 1.4], [4.0, 1.6], [2.2, 5.0, 0.3]),
    m("M2.5", 2.5, 0.45, &[], 2.05, [2.7, 2.9, 3.1], [4.5, 2.5, 2.0, 1.1], [5.0, 1.7], [5.0, 2.0], [2.7, 6.0, 0.5]),
    m("M3", 3.0, 0.5, &[], 2.5, [3.2, 3.4, 3.6], [5.5, 3.0, 2.5, 1.3], [5.5, 2.0], [5.5, 2.4], [3.2, 7.0, 0.5]),
    m("M4", 4.0, 0.7, &[], 3.3, [4.3, 4.5, 4.8], [7.0, 4.0, 3.0, 2.0], [7.0, 2.8], [7.0, 3.2], [4.3, 9.0, 0.8]),
    m("M5", 5.0, 0.8, &[], 4.2, [5.3, 5.5, 5.8], [8.5, 5.0, 4.0, 2.5], [8.0, 3.5], [8.0, 4.7], [5.3, 10.0, 1.0]),
    m("M6", 6.0, 1.0, &[], 5.0, [6.4, 6.6, 7.0], [10.0, 6.0, 5.0, 3.0], [10.0, 4.0], [10.0, 5.2], [6.4, 12.0, 1.6]),
    m("M8", 8.0, 1.25, &[1.0], 6.8, [8.4, 9.0, 10.0], [13.0, 8.0, 6.0, 4.0], [13.0, 5.3], [13.0, 6.8], [8.4, 16.0, 1.6]),
    m("M10", 10.0, 1.5, &[1.25, 1.0], 8.5, [10.5, 11.0, 12.0], [16.0, 10.0, 8.0, 5.0], [16.0, 6.4], [16.0, 8.4], [10.5, 20.0, 2.0]),
    m("M12", 12.0, 1.75, &[1.5, 1.25], 10.2, [13.0, 13.5, 14.5], [18.0, 12.0, 10.0, 6.0], [18.0, 7.5], [18.0, 10.8], [13.0, 24.0, 2.5]),
    m("M14", 14.0, 2.0, &[1.5], 12.0, [15.0, 15.5, 16.5], [21.0, 14.0, 12.0, 7.0], [21.0, 8.8], [21.0, 12.8], [15.0, 28.0, 2.5]),
    m("M16", 16.0, 2.0, &[1.5], 14.0, [17.0, 17.5, 18.5], [24.0, 16.0, 14.0, 8.0], [24.0, 10.0], [24.0, 14.8], [17.0, 30.0, 3.0]),
    m("M20", 20.0, 2.5, &[1.5], 17.5, [21.0, 22.0, 24.0], [30.0, 20.0, 17.0, 10.0], [30.0, 12.5], [30.0, 18.0], [21.0, 37.0, 3.0]),
];

/// 十字穴付き小ねじ 1 サイズの寸法(mm)。JIS B 1111 の、国内で売られている形(なべは頭の径 M3 で
/// 5.5。ISO 7045 の 5.6 とは違う)。ミスミ・SUNCO の寸法表(2026-10-20 に読んだ)の基準の値。
#[derive(Debug, Clone, Copy)]
pub struct Screw {
    pub name: &'static str,
    /// なべの頭の径・頭の高さ・十字穴の径(参考の値)
    pub pan: [f64; 3],
    /// 皿の頭の径・頭の高さ・十字穴の径(参考の値。なべの同じ呼びの値を使う)
    pub flat: [f64; 3],
    /// 皿の穴の径(穴の機能の皿の既定と同じ値。穴の口の円から呼びを選ぶのに使う)
    pub csk: f64,
}

/// 十字穴付き小ねじの表(M2〜M6)。
#[rustfmt::skip]
pub const SCREWS: &[Screw] = &[
    Screw { name: "M2", pan: [3.5, 1.3, 2.2], flat: [4.0, 1.2, 2.2], csk: 4.2 },
    Screw { name: "M2.5", pan: [4.5, 1.7, 2.6], flat: [5.0, 1.45, 2.6], csk: 5.1 },
    Screw { name: "M3", pan: [5.5, 2.0, 3.6], flat: [6.0, 1.75, 3.6], csk: 6.3 },
    Screw { name: "M4", pan: [7.0, 2.6, 4.2], flat: [8.0, 2.3, 4.2], csk: 8.8 },
    Screw { name: "M5", pan: [9.0, 3.3, 4.9], flat: [10.0, 2.8, 4.9], csk: 10.4 },
    Screw { name: "M6", pan: [10.5, 3.9, 6.3], flat: [12.0, 3.4, 6.3], csk: 12.6 },
];

pub fn screw(size: &str) -> Option<&'static Screw> {
    let m = metric(size)?;
    SCREWS.iter().find(|s| s.name == m.name)
}

/// 深溝玉軸受 1 つの寸法(内径・外径・幅、mm)。
#[derive(Debug, Clone, Copy)]
pub struct BearingSize {
    pub name: &'static str,
    pub d: f64,
    pub od: f64,
    pub b: f64,
}

const fn b(name: &'static str, d: f64, od: f64, w: f64) -> BearingSize {
    BearingSize { name, d, od, b: w }
}

/// よく使う深溝玉軸受(ミニチュアの 6 系・60 系・62 系の一部)。
pub const BEARINGS: &[BearingSize] = &[
    b("695", 5.0, 13.0, 4.0),
    b("688", 8.0, 16.0, 5.0),
    b("623", 3.0, 10.0, 4.0),
    b("624", 4.0, 13.0, 5.0),
    b("625", 5.0, 16.0, 5.0),
    b("626", 6.0, 19.0, 6.0),
    b("608", 8.0, 22.0, 7.0),
    b("6000", 10.0, 26.0, 8.0),
    b("6001", 12.0, 28.0, 8.0),
    b("6002", 15.0, 32.0, 9.0),
    b("6200", 10.0, 30.0, 9.0),
    b("6201", 12.0, 32.0, 10.0),
];

/// ボルトの長さの候補(首の下の長さ)。
pub const LENGTHS: &[f64] = &[
    4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 60.0, 70.0,
    80.0, 100.0,
];

/// 呼び(`M10`、細目は `M10×1.25`。`x`・`X`・`*` も×として読む)のねじの表と、細目ならそのピッチ。
/// 表に無い呼びと、その呼びの細目の表に無いピッチは `None`。
pub fn thread(size: &str) -> Option<(&'static Metric, Option<f64>)> {
    let size = size.trim();
    let (name, pitch) = match size.split_once(['×', 'x', 'X', '*']) {
        Some((n, p)) => (n.trim(), Some(p.trim().parse::<f64>().ok()?)),
        None => (size, None),
    };
    let m = METRIC.iter().find(|m| m.name.eq_ignore_ascii_case(name))?;
    match pitch {
        // 並目のピッチを書いた物は並目として読む
        Some(p) if (p - m.pitch).abs() < 1e-9 => Some((m, None)),
        Some(p) => m
            .fine
            .iter()
            .any(|f| (f - p).abs() < 1e-9)
            .then_some((m, Some(p))),
        None => Some((m, None)),
    }
}

pub fn metric(size: &str) -> Option<&'static Metric> {
    thread(size).map(|(m, _)| m)
}

/// 呼びの JIS の書き方(並目は `M10`、細目は `M10×1.25`)。表に無ければ書かれたまま。
pub fn size_name(size: &str) -> String {
    match thread(size) {
        Some((m, Some(p))) => format!("{}×{}", m.name, mm(p)),
        Some((m, None)) => m.name.into(),
        None => size.trim().into(),
    }
}

pub fn bearing(name: &str) -> Option<&'static BearingSize> {
    BEARINGS.iter().find(|b| b.name == name.trim())
}

/// 穴の径 `dia` に合う呼び(穴の口の円が穴の縁なら `hole`、軸の縁なら偽)。メートルねじは呼び径・
/// 下穴・通し穴の径のいちばん近い物、軸受は穴なら外径、軸なら内径のいちばん近い物。
pub fn size_for(kind: Kind, dia: f64, hole: bool) -> String {
    let near = |x: f64| (x - dia).abs();
    if kind == Kind::Bearing {
        return BEARINGS
            .iter()
            .min_by(|a, b| {
                let (x, y) = if hole { (a.od, b.od) } else { (a.d, b.d) };
                near(x).total_cmp(&near(y))
            })
            .map_or("608", |b| b.name)
            .into();
    }
    METRIC
        .iter()
        .filter(|m| !kind.is_screw() || screw(m.name).is_some())
        .min_by(|a, b| {
            let score = |m: &Metric| {
                // 皿小ねじは皿の穴の口の円(皿の径)でも選ぶ
                let csk = match (kind, screw(m.name)) {
                    (Kind::FlatHead, Some(s)) => s.csk,
                    _ => f64::NAN,
                };
                [
                    m.d,
                    m.tap,
                    m.clearance[0],
                    m.clearance[1],
                    m.clearance[2],
                    csk,
                ]
                .into_iter()
                .filter(|x| !x.is_nan())
                .map(near)
                .fold(f64::INFINITY, f64::min)
            };
            score(a).total_cmp(&score(b))
        })
        .map_or("M3", |m| m.name)
        .into()
}

/// 深さ `depth` の穴に挿すボルトの長さの既定: 深さ以上でいちばん短い候補(なければいちばん長い物)。
pub fn length_for(depth: f64) -> f64 {
    LENGTHS
        .iter()
        .copied()
        .find(|l| *l >= depth - 1e-6)
        .unwrap_or(*LENGTHS.last().expect("候補はある"))
}

/// 挿した穴の口 1 つ(世界座標)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// 穴の口の円の中心
    pub center: P3,
    /// 穴の口の面の外向きの法線(ボルトの頭・ナット・座金が載る側)
    pub normal: P3,
    /// 穴のあるボディ(挿した時の最後の機能)。今の穴の径を読み、呼びと合わなければ知らせる
    /// (呼びは合わせ直さない)。前の版の文書には無い
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<FeatureId>,
    /// 挿した時の穴の口の円の直径(同じ中心に座ぐりの円もある時に、どの円かを決める)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<f64>,
    /// 下に重なる規格部品(座金・ナット)。その上の端から置く。前の版の文書には無い
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<Stack>,
}

/// 重ねる先: 規格部品の機能と、その何番目の穴の分か。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stack {
    pub fastener: FeatureId,
    pub index: usize,
}

/// 規格部品の機能のデータ。例: `{"kind": "socket_head", "size": "M3", "length": "10 mm",
/// "places": [{"center": [10, 10, 8], "normal": [0, 0, 1]}]}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fastener {
    #[serde(default)]
    pub kind: Kind,
    /// 呼び(`M3`、軸受は `608` など)
    pub size: String,
    /// ボルトの首の下の長さ(ナット・座金・軸受では使わない)
    #[serde(default = "default_length")]
    pub length: String,
    pub places: Vec<Place>,
    /// 面から法線の向きにずらす量(下に重なる部品があれば、その上の端から)
    #[serde(default = "zero_mm")]
    pub offset: String,
    /// 向きを逆にする(穴の口の面の内側へ向ける)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// 部品表の材料(ボディに材料を付けていない時)
    #[serde(default = "default_material")]
    pub material: String,
}

fn default_length() -> String {
    "10 mm".into()
}

fn zero_mm() -> String {
    "0 mm".into()
}

fn default_material() -> String {
    "鋼".into()
}

impl Exprs for Fastener {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.length, &mut self.offset]
    }
}

/// 長さの表示(整数なら小数なし)。
fn mm(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

impl Fastener {
    /// 部品表に出す名前(「六角穴付きボルト M3×10(JIS B 1176)」「六角ナット M10×1.25(JIS B 1181)」
    /// 「玉軸受 608(JIS B 1521)」)。並目はピッチを省く。長さの式が読めなければ式のまま。
    pub fn designation(&self, len: Option<f64>) -> String {
        let size = if self.kind == Kind::Bearing {
            self.size.trim().to_string()
        } else {
            size_name(&self.size)
        };
        let name = match (self.kind.has_length(), len) {
            (true, Some(l)) => format!("{} {size}×{}", self.kind.label(), mm(l)),
            (true, None) => format!("{} {size}×{}", self.kind.label(), self.length.trim()),
            (false, _) => format!("{} {size}", self.kind.label()),
        };
        format!("{name}({})", self.kind.standard())
    }

    /// 呼びの候補(欄の一覧)。
    fn size_choices(&self) -> Vec<Choice> {
        if self.kind == Kind::Bearing {
            BEARINGS
                .iter()
                .map(|b| Choice {
                    value: b.name.into(),
                    label: format!(
                        "{}(内径 {} × 外径 {} × 幅 {})",
                        b.name,
                        mm(b.d),
                        mm(b.od),
                        mm(b.b)
                    ),
                })
                .collect()
        } else {
            let mut v = Vec::new();
            for m in METRIC
                .iter()
                .filter(|m| !self.kind.is_screw() || screw(m.name).is_some())
            {
                v.push(Choice {
                    value: m.name.into(),
                    label: format!("{}(並目 ピッチ {})", m.name, mm(m.pitch)),
                });
                if self.kind.has_fine() {
                    v.extend(m.fine.iter().map(|p| Choice {
                        value: format!("{}×{}", m.name, mm(*p)),
                        label: format!("{}×{}(細目)", m.name, mm(*p)),
                    }));
                }
            }
            v
        }
    }
}

/// 法線 `n` に垂直な 2 つの向き(右手系: `u × v = n`)。
fn frame(n: P3) -> (P3, P3) {
    let a = if n[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let u = k::normalize(k::cross(a, n));
    (u, k::cross(n, u))
}

fn line(name: String, a: P2, b: P2) -> ProfileEdge {
    ProfileEdge {
        name,
        segment: Segment::Line { a, b },
        reversed: false,
    }
}

/// 折れ線の閉じた輪郭(反時計回りに揃える)。
fn closed(name: &str, pts: &[P2]) -> ProfileLoop {
    let n = pts.len();
    let mut l = ProfileLoop {
        edges: (0..n)
            .map(|i| line(format!("{name}{i}"), pts[i], pts[(i + 1) % n]))
            .collect(),
    };
    if l.signed_area() < 0.0 {
        l.reverse();
    }
    l
}

/// 二面幅 `s` の正六角形(中心が原点、角が u の向き)。
fn hexagon(s: f64) -> Vec<P2> {
    let r = s / 3f64.sqrt();
    (0..6)
        .map(|i| {
            let a = std::f64::consts::FRAC_PI_3 * i as f64;
            [r * a.cos(), r * a.sin()]
        })
        .collect()
}

fn circle(name: &str, r: f64) -> ProfileLoop {
    ProfileLoop {
        edges: vec![ProfileEdge {
            name: name.into(),
            segment: Segment::Circle {
                center: [0.0, 0.0],
                radius: r,
            },
            reversed: true,
        }],
    }
}

/// 形を作る道具(1 つの穴の口の分)。断面(半径 r、法線の向きの高さ z)を回す物と、
/// 六角形を法線の向きに押し出す物を組み合わせる。
struct Builder<'a> {
    kernel: &'a dyn jc_core::Kernel,
    tag: String,
    c: P3,
    n: P3,
    u: P3,
    v: P3,
}

impl Builder<'_> {
    /// 断面 `rz`(半径と高さの組の折れ線)を軸のまわりに一周回す。
    fn revolve(&self, name: &str, rz: &[P2]) -> Result<ShapeHandle, EvalError> {
        let profile = Profile {
            plane: Plane {
                origin: self.c,
                x_axis: self.u,
                y_axis: self.n,
            },
            regions: vec![Region {
                outer: closed(name, rz),
                holes: vec![],
            }],
        };
        self.kernel
            .revolve(&profile, self.c, self.n, std::f64::consts::TAU, &self.tag)
            .map_err(kernel_err)
    }

    /// 二面幅 `s` の六角柱(高さ `from`〜`to`)。`hole` があれば、その半径の丸い穴をあける。
    fn hex(
        &self,
        name: &str,
        s: f64,
        from: f64,
        to: f64,
        hole: Option<f64>,
    ) -> Result<ShapeHandle, EvalError> {
        let profile = Profile {
            plane: Plane {
                origin: self.c,
                x_axis: self.u,
                y_axis: self.v,
            },
            regions: vec![Region {
                outer: closed(name, &hexagon(s)),
                holes: hole
                    .map(|r| circle(&format!("{name}_bore"), r))
                    .into_iter()
                    .collect(),
            }],
        };
        self.kernel
            .extrude(&profile, from, to, &self.tag)
            .map_err(kernel_err)
    }

    /// 十字穴(腕の端から端まで `m`)を高さ `from`〜`to` に押し出した物(頭から引く)。
    fn cross(&self, name: &str, m: f64, from: f64, to: f64) -> Result<ShapeHandle, EvalError> {
        let (a, b) = (m / 2.0, m * 0.12);
        let pts = [
            [a, b],
            [b, b],
            [b, a],
            [-b, a],
            [-b, b],
            [-a, b],
            [-a, -b],
            [-b, -b],
            [-b, -a],
            [b, -a],
            [b, -b],
            [a, -b],
        ];
        let profile = Profile {
            plane: Plane {
                origin: self.c,
                x_axis: self.u,
                y_axis: self.v,
            },
            regions: vec![Region {
                outer: closed(name, &pts),
                holes: vec![],
            }],
        };
        self.kernel
            .extrude(&profile, from, to, &self.tag)
            .map_err(kernel_err)
    }

    fn union(&self, a: ShapeHandle, b: ShapeHandle) -> Result<ShapeHandle, EvalError> {
        self.kernel
            .boolean(BooleanOp::Union, a, b)
            .map_err(kernel_err)
    }
}

/// 規格部品 1 つの形。高さ 0 が穴の口の面で、正の向きが面の外。
fn build(
    kernel: &dyn jc_core::Kernel,
    tag: String,
    f: &Fastener,
    len: f64,
    p: &Place,
    offset: f64,
) -> Result<ShapeHandle, EvalError> {
    let mut n = k::normalize(p.normal);
    if f.flip {
        n = k::scale(n, -1.0);
    }
    let (u, v) = frame(n);
    let bld = Builder {
        kernel,
        tag,
        c: k::add(p.center, k::scale(n, offset)),
        n,
        u,
        v,
    };
    let unknown = || EvalError::InvalidData(format!("呼び `{}` は表にありません", f.size));
    match f.kind {
        Kind::Bearing => {
            let s = bearing(&f.size).ok_or_else(unknown)?;
            // 内輪と外輪の間を 0.3 mm へこませる(シールドの段)
            let (ri, ro) = (s.d / 2.0, s.od / 2.0);
            let t = (ro - ri) * 0.22;
            let dz = 0.3f64.min(s.b / 4.0);
            let w = s.b;
            bld.revolve(
                "bearing",
                &[
                    [ri, 0.0],
                    [ri + t, 0.0],
                    [ri + t, -dz],
                    [ro - t, -dz],
                    [ro - t, 0.0],
                    [ro, 0.0],
                    [ro, -w],
                    [ro - t, -w],
                    [ro - t, -w + dz],
                    [ri + t, -w + dz],
                    [ri + t, -w],
                    [ri, -w],
                ],
            )
        }
        kind => {
            let s = metric(&f.size).ok_or_else(unknown)?;
            let r = s.d / 2.0;
            match kind {
                Kind::SocketHead => {
                    let [dk, kh, sw, depth] = s.socket;
                    let body = bld.revolve(
                        "screw",
                        &[
                            [0.0, kh],
                            [dk / 2.0, kh],
                            [dk / 2.0, 0.0],
                            [r, 0.0],
                            [r, -len],
                            [0.0, -len],
                        ],
                    )?;
                    let socket = bld.hex("socket", sw, kh - depth, kh + 1.0, None)?;
                    kernel
                        .boolean(BooleanOp::Subtract, body, socket)
                        .map_err(kernel_err)
                }
                Kind::PanHead | Kind::FlatHead => {
                    let sc = screw(&f.size).ok_or_else(unknown)?;
                    let (body, top, m) = if kind == Kind::PanHead {
                        // なべ: 上の角を落とした平たい頭
                        let [dk, kh, m] = sc.pan;
                        let rk = dk / 2.0;
                        let body = bld.revolve(
                            "screw",
                            &[
                                [0.0, kh],
                                [rk * 0.8, kh],
                                [rk, kh * 0.55],
                                [rk, 0.0],
                                [r, 0.0],
                                [r, -len],
                                [0.0, -len],
                            ],
                        )?;
                        (body, kh, m)
                    } else {
                        // 皿: 頭の径から 90° の円すいで軸へ(頭の高さの表の値は首の丸みまで含むので
                        // 形には使わない)。長さは頭を含む全長。皿の穴の口(通し穴より太い円)では、
                        // 頭の円すいが穴の 90° の皿に当たる高さに置く(頭の上は面から 穴の口の半径 −
                        // 頭の半径 だけ下)。細い穴の口では頭の上を面にそろえる
                        let [dk, _, m] = sc.flat;
                        let rk = dk / 2.0;
                        let z = match p.diameter {
                            Some(dia) if dia > s.clearance[2] + 0.2 => rk - dia / 2.0,
                            _ => 0.0,
                        };
                        let body = bld.revolve(
                            "screw",
                            &[
                                [0.0, z],
                                [rk, z],
                                [r, z - (rk - r)],
                                [r, z - len],
                                [0.0, z - len],
                            ],
                        )?;
                        (body, z, m)
                    };
                    let depth = if kind == Kind::PanHead { 0.5 } else { 0.6 } * m;
                    let recess = bld.cross("recess", m, top - depth, top + 1.0)?;
                    kernel
                        .boolean(BooleanOp::Subtract, body, recess)
                        .map_err(kernel_err)
                }
                Kind::HexBolt => {
                    let [sw, kh] = s.hex_head;
                    let shank =
                        bld.revolve("shank", &[[0.0, 0.0], [r, 0.0], [r, -len], [0.0, -len]])?;
                    let head = bld.hex("head", sw, 0.0, kh, None)?;
                    bld.union(head, shank)
                }
                Kind::HexNut => {
                    let [sw, mh] = s.nut;
                    bld.hex("nut", sw, 0.0, mh, Some(r))
                }
                Kind::Washer => {
                    let [d1, d2, h] = s.washer;
                    bld.revolve(
                        "washer",
                        &[
                            [d1 / 2.0, 0.0],
                            [d2 / 2.0, 0.0],
                            [d2 / 2.0, h],
                            [d1 / 2.0, h],
                        ],
                    )
                }
                Kind::Bearing => unreachable!("上で扱った"),
            }
        }
    }
}

/// 穴の口の面(ずらしの後)から、部品の上の端までの高さ。軸受は穴の中へ作るので 0。
fn height(f: &Fastener) -> f64 {
    match (f.kind, metric(&f.size)) {
        (Kind::SocketHead, Some(m)) => m.socket[1],
        (Kind::HexBolt, Some(m)) => m.hex_head[1],
        (Kind::PanHead, _) => screw(&f.size).map_or(0.0, |s| s.pan[1]),
        (Kind::HexNut, Some(m)) => m.nut[1],
        (Kind::Washer, Some(m)) => m.washer[2],
        _ => 0.0,
    }
}

/// 直径 `dia` の穴の口(穴の縁なら `hole`、軸の縁なら偽)に、種類 `kind` の呼び `size` が合うか。
/// ねじは穴なら下穴の径から粗い通し穴の径まで、軸なら呼び径。軸受は穴なら外径、軸なら内径。
/// 呼びが表に無ければ `None`(調べない)。
pub fn fits(kind: Kind, size: &str, dia: f64, hole: bool) -> Option<bool> {
    const TOL: f64 = 0.2;
    if kind == Kind::Bearing {
        let b = bearing(size)?;
        let want = if hole { b.od } else { b.d };
        return Some((dia - want).abs() <= TOL);
    }
    let (m, fine) = thread(size)?;
    if kind.is_screw() && screw(size).is_none() {
        return None;
    }
    // 細目の下穴は呼び径からピッチを引いた径(JIS B 1004 の目安)
    let tap = fine.map_or(m.tap, |p| m.d - p);
    // 皿小ねじは皿の穴の口(頭の径から、皿の径より少し大きいまで)にも合う
    let csk = match (kind, screw(size)) {
        (Kind::FlatHead, Some(s)) => dia >= s.flat[0] - TOL && dia <= s.csk + 1.0,
        _ => false,
    };
    Some(if hole {
        csk || (dia >= tap - TOL && dia <= m.clearance[2] + TOL)
    } else {
        (dia - m.d).abs() <= TOL
    })
}

impl Fastener {
    /// 挿した穴の今の口の円(直径と、穴の縁か)。ボディを覚えていない(前の版の)物は `None`、
    /// 円が見つからなければ `Some(None)`。
    fn mouth_now(&self, ctx: &EvalCtx<'_>, p: &Place) -> Option<Option<(f64, bool)>> {
        let body = p.body?;
        let Ok(Value::Shape(h)) = ctx.output(body, "body") else {
            return Some(None);
        };
        let all = circles(ctx.kernel(), *h);
        let at = |c: P3| {
            let d = k::sub(c, p.center);
            k::dot(d, d) < 1e-6
        };
        let want = p.diameter.unwrap_or(0.0) / 2.0;
        Some(
            all.iter()
                .filter(|c| at(c.center))
                .min_by(|a, b| (a.radius - want).abs().total_cmp(&(b.radius - want).abs()))
                .map(|c| (c.radius * 2.0, c.is_hole(&all))),
        )
    }

    /// 穴ごとの注意(呼びと穴の径が合わない、穴が見つからない)。
    fn fit_notes(&self, ctx: &EvalCtx<'_>) -> Vec<Option<String>> {
        let size = self.size.trim();
        self.places
            .iter()
            .enumerate()
            .map(|(i, p)| match self.mouth_now(ctx, p)? {
                None => Some(format!(
                    "穴{} の口の円が見つかりません(穴を動かしたか消した)。挿し直してください",
                    i + 1
                )),
                Some((dia, hole)) => (fits(self.kind, size, dia, hole) == Some(false)).then(|| {
                    let better = size_for(self.kind, dia, hole);
                    let hint = if fits(self.kind, &better, dia, hole) == Some(true) {
                        format!("合うのは {better}")
                    } else {
                        "表に合う呼びがありません".into()
                    };
                    format!(
                        "穴{} の径 {} mm に呼び {size} が合いません({hint})。呼びは自動では変えません",
                        i + 1,
                        mm((dia * 100.0).round() / 100.0)
                    )
                }),
            })
            .collect()
    }
}

impl Feature for Fastener {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = expr_refs(self)?;
        for p in &self.places {
            if let Some(b) = p.body {
                let i = jc_cad::output(b, "body");
                if !v.contains(&i) {
                    v.push(i);
                }
            }
            if let Some(s) = p.on {
                v.push(InputRef::Output {
                    feature: s.fastener,
                    slot: format!("{TOP}{}", s.index),
                });
            }
        }
        Ok(v)
    }

    /// 穴のボディは径を読むだけで、受け継がない(ボディはそのまま見える)。
    fn keeps(&self) -> Vec<FeatureId> {
        self.places.iter().filter_map(|p| p.body).collect()
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        if self.places.is_empty() {
            return Err(EvalError::InvalidData("挿す穴がありません".into()));
        }
        let len = if self.kind.has_length() {
            let l = length(ctx, &self.length, "長さ")?;
            if l <= 0.0 {
                return Err(EvalError::InvalidData(
                    "長さは 0 より大きくしてください".into(),
                ));
            }
            l
        } else {
            0.0
        };
        let offset = length(ctx, &self.offset, "ずらし")?;
        let mut out = Outputs::new();
        let mut warnings = Vec::new();
        if self.kind.has_length() && !LENGTHS.iter().any(|l| (l - len).abs() < 1e-6) {
            warnings.push(format!(
                "長さ {} mm は、よく売られている長さの表にありません",
                mm(len)
            ));
        }
        let t = tag(ctx);
        let top = height(self);
        for (i, p) in self.places.iter().enumerate() {
            // 下に重なる部品の上の端(下の部品を消すと、ほかの参照と同じく理由つきで止まる)
            let base = match p.on {
                None => 0.0,
                Some(s) => match ctx.output(s.fastener, &format!("{TOP}{}", s.index))? {
                    Value::Custom { data, .. } => data.as_f64().unwrap_or(0.0),
                    _ => 0.0,
                },
            };
            let h = build(
                ctx.kernel(),
                format!("{t}p{i}"),
                self,
                len,
                p,
                base + offset,
            )?;
            out.insert(format!("{PART}{i}"), Value::Shape(h));
            out.insert(
                format!("{TOP}{i}"),
                Value::Custom {
                    type_key: TOP_TYPE.into(),
                    data: json!(base + offset + top),
                },
            );
        }
        for (i, note) in self.fit_notes(ctx).into_iter().enumerate() {
            if let Some(n) = &note {
                log::info!("{}: {n}", self.designation(Some(len)));
                warnings.push(n.clone());
            }
            out.insert(
                format!("{FIT}{i}"),
                Value::Custom {
                    type_key: FIT_TYPE.into(),
                    data: json!(note),
                },
            );
        }
        out.insert(SIZES.into(), choices_value(self.size_choices()));
        out.insert(
            NAME.into(),
            Value::Custom {
                type_key: NAME_TYPE.into(),
                data: json!(self.designation(Some(len))),
            },
        );
        add_warnings(&mut out, warnings);
        log::debug!(
            "{} を {} か所に作った",
            self.designation(Some(len)),
            self.places.len()
        );
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

/// 規格部品のボディの機能のデータ(規格部品の機能の、何番目の穴の分か)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FastenerBody {
    pub fastener: FeatureId,
    pub index: usize,
}

impl Feature for FastenerBody {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![
            InputRef::Output {
                feature: self.fastener,
                slot: format!("{PART}{}", self.index),
            },
            InputRef::Output {
                feature: self.fastener,
                slot: format!("{FIT}{}", self.index),
            },
        ])
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let v = ctx
            .output(self.fastener, &format!("{PART}{}", self.index))?
            .clone();
        let mut out: Outputs = [("body".to_string(), v)].into();
        // この穴の呼びと径が合わなければ、この穴のボディにも注意を付ける
        if let Ok(Value::Custom { data, .. }) =
            ctx.output(self.fastener, &format!("{FIT}{}", self.index))
            && let Some(n) = data.as_str()
        {
            add_warnings(&mut out, vec![n.to_string()]);
        }
        Ok(out)
    }
}

/// ボルトの時だけ使う欄(ナット・座金・軸受では消す)。
fn bolt_only(mut f: Field) -> Field {
    let not = |value: &str, and: Option<Box<When>>| When {
        key: "kind".into(),
        value: value.into(),
        equal: false,
        default: "socket_head".into(),
        and,
    };
    f.when = Some(not(
        "hex_nut",
        Some(Box::new(not(
            "washer",
            Some(Box::new(not("bearing", None))),
        ))),
    ));
    f
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(FASTENER, "規格部品")
            .menu("組立")
            .icon("fastener")
            .hint("穴に挿した JIS のボルト・小ねじ・ナット・座金・玉軸受。呼びと長さを変えると、挿した穴の全部で変わる。座金やナットの上に挿すと重なる。ねじ山は描かない")
            .aliases("きかくぶひん 規格部品 ねじ ネジ ぼると ボルト なっと ナット ざがね 座金 わっしゃ ワッシャ べありんぐ ベアリング じくうけ 軸受 こねじ 小ねじ なべ さら 皿 じゅうじあな 十字穴 fastener bolt nut washer bearing screw JIS B1111 B1176 B1180 B1181 B1256 B1521 さいめ 細目 なみめ 並目")
            .field(Field::choice("kind", "種類", KINDS).help("種類を変えたら、呼びも候補から選び直す"))
            .field(Field::text("size", "呼び").suggest_own(SIZES).help("ねじの呼び(並目は M10、細目は M10×1.25)か軸受の番号(608 など)"))
            .field(
                bolt_only(Field::expr("length", "長さ").length())
                    .suggest(&["6 mm", "8 mm", "10 mm", "12 mm", "16 mm", "20 mm", "25 mm", "30 mm"])
                    .help("首の下の長さ。皿小ねじは頭を含む全長"),
            )
            .field(Field::expr("offset", "ずらし").length().help("穴の口の面から外へずらす量。座金やナットの上に挿した物は、その上の端から"))
            .field(Field::bool("flip", "向きを反転").help("穴の口の面の内側へ向ける(穴の奥から挿す時)"))
            .field(
                Field::text("material", "材料")
                    .suggest(&["鋼", "ステンレス", "真鍮", "アルミ", "樹脂"])
                    .help("部品表の質量に使う(ボディに材料を付けていればそちら)"),
            )
            .no_raw_editor(),
        |d| Ok(Box::new(parse::<Fastener>(d)?)),
    );
    reg.register_described(
        solid(FASTENER_BODY, "規格部品のボディ")
            .icon("fastener")
            .hint("規格部品の 1 つ(穴 1 つの分)。呼びと長さは規格部品の機能で変える")
            .parent("fastener")
            .no_raw_editor(),
        |d| Ok(Box::new(parse::<FastenerBody>(d)?)),
    );
}

/// 規格部品のボディの機能から、規格部品の機能のデータ。
pub fn fastener_of(data: &jc_core::DocumentData, id: FeatureId) -> Option<(FeatureId, Fastener)> {
    let e = data.feature(id).filter(|e| e.type_id == FASTENER_BODY)?;
    let b: FastenerBody = serde_json::from_value(e.data.clone()).ok()?;
    let g = data.feature(b.fastener).filter(|e| e.type_id == FASTENER)?;
    Some((b.fastener, serde_json::from_value(g.data.clone()).ok()?))
}

/// 規格部品の機能 `id` の今の名前(計算できていなければ、式のままの名前)。
pub fn name_of(cx: &Context<'_>, id: FeatureId, f: &Fastener) -> String {
    match cx.output(id, NAME) {
        Some(Value::Custom { type_key, data }) if type_key == NAME_TYPE => data
            .as_str()
            .map_or_else(|| f.designation(None), str::to_string),
        _ => f.designation(None),
    }
}

/// 穴の口の候補 1 つ(手順の候補の値)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoleMouth {
    pub center: P3,
    pub normal: P3,
    /// 円の直径
    pub diameter: f64,
    /// 穴の縁(偽なら軸・ボスの縁)
    pub hole: bool,
    /// 穴の深さ(同じ円筒の面のもう一方の縁までの距離。分からなければ 0)
    pub depth: f64,
    /// 穴の口から、同じ軸のいちばん奥の円までの距離(皿の穴の下の通し穴まで。皿小ねじの長さに使う)
    #[serde(default)]
    pub through: f64,
    /// 皿の穴の縁(平面と円すいの境目)
    #[serde(default)]
    pub countersink: bool,
}

/// 形の円の辺 1 つ: 平面と円筒の境目(カーネルの円の辺)か、平面と円すいの境目(皿の穴の縁。
/// 曲がった辺の折れ線を円に当てはめる)。
struct Circle {
    faces: [String; 2],
    center: P3,
    /// 円の面の法線(向きは問わない)
    axis: P3,
    radius: f64,
    /// 平面の面が円の内側にあるか(円の辺だけ分かる。皿の縁は [`Circle::is_hole`] で決める)
    inside: Option<bool>,
    outline: Vec<P3>,
}

impl Circle {
    /// 穴の縁か(偽なら軸・ボスの縁)。皿の縁は、同じ軸で同じ円すいのもう一方の縁が細ければ穴。
    fn is_hole(&self, all: &[Circle]) -> bool {
        self.inside.map_or_else(
            || {
                all.iter()
                    .filter(|o| !std::ptr::eq(*o, self) && self.coaxial(o))
                    .find(|o| o.faces.iter().any(|f| self.faces.contains(f)))
                    .is_none_or(|o| o.radius < self.radius)
            },
            |i| !i,
        )
    }

    /// `o` の中心がこの円の軸の上にあり、同じ点ではない。
    fn coaxial(&self, o: &Circle) -> bool {
        let d = k::sub(o.center, self.center);
        let along = k::dot(d, self.axis);
        let off = k::sub(d, k::scale(self.axis, along));
        along.abs() > 1e-6 && k::dot(off, off) < 1e-8
    }
}

/// 折れ線が閉じた円なら中心・法線・半径(点の重心からの距離が半径の 1% 以内でそろう物)。
fn fit_circle(pts: &[P3]) -> Option<(P3, P3, f64)> {
    let first = pts.first()?;
    let last = pts.last()?;
    if pts.len() < 8 || k::dot(k::sub(*first, *last), k::sub(*first, *last)) > 1e-8 {
        return None;
    }
    let ring = &pts[..pts.len() - 1];
    let n = ring.len() as f64;
    let c = ring
        .iter()
        .fold([0.0; 3], |a, p| k::add(a, k::scale(*p, 1.0 / n)));
    let dist = |p: &P3| k::dot(k::sub(*p, c), k::sub(*p, c)).sqrt();
    let r = ring.iter().map(dist).sum::<f64>() / n;
    let q = ring.len() / 4;
    let axis = k::cross(k::sub(ring[0], c), k::sub(ring[q], c));
    (r > 1e-6 && k::dot(axis, axis) > 1e-12 && ring.iter().all(|p| (dist(p) - r).abs() <= r * 0.01))
        .then(|| (c, k::normalize(axis), r))
}

/// 形の円の辺の全部(一周の物だけ)。
fn circles(kernel: &dyn jc_core::Kernel, h: ShapeHandle) -> Vec<Circle> {
    let mut out: Vec<Circle> = kernel
        .arc_edges(h)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| {
            let a = e.arc.filter(|a| a.sweep.abs() > std::f64::consts::PI)?;
            Some(Circle {
                outline: e.polyline(48),
                faces: e.faces,
                center: a.center,
                axis: k::normalize(a.axis),
                radius: a.radius,
                inside: Some(a.inside),
            })
        })
        .collect();
    out.extend(
        kernel
            .curve_edges(h)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| {
                let (center, axis, radius) = fit_circle(&e.curve)?;
                Some(Circle {
                    faces: e.faces,
                    center,
                    axis,
                    radius,
                    inside: None,
                    outline: e.curve,
                })
            }),
    );
    out
}

/// 規格部品を挿す穴の口の候補: クリックした面のボディの円の辺のうち、クリックした面(平らな面なら
/// その縁、円筒の面ならその両端)にある物。皿の穴の縁も入る。法線は辺の平らな面の外向き。
pub fn mouth_choices(cx: &Context<'_>) -> Vec<HandleChoice> {
    let Some(p) = cx.picked else {
        return Vec::new();
    };
    let Some(Value::Shape(h)) = cx.output(p.feature, "body") else {
        return Vec::new();
    };
    let faces = cx.kernel.faces(*h).unwrap_or_default();
    let normal_of = |name: &str| {
        faces
            .iter()
            .find(|f| f.name == name)
            .and_then(|f| f.planar)
            .map(|pl| pl.normal)
    };
    let all = circles(&**cx.kernel, *h);
    let near = |a: P3, b: P3, tol: f64| {
        let d = k::sub(a, b);
        k::dot(d, d) < tol * tol
    };
    let mut seen: Vec<(P3, f64)> = Vec::new();
    let mut out = Vec::new();
    for e in &all {
        if !e.faces.contains(&p.face) {
            continue;
        }
        // 平らな面(穴の口の面)と、もう一方(穴の内側の円筒や皿の円すい)。並びは辺によって違う
        let Some((n, side)) = (0..2).find_map(|i| Some((normal_of(&e.faces[i])?, &e.faces[1 - i])))
        else {
            continue;
        };
        if seen
            .iter()
            .any(|(c, r)| near(*c, e.center, 1e-4) && (r - e.radius).abs() < 1e-4)
        {
            continue;
        }
        seen.push((e.center, e.radius));
        let dist = |c: P3| k::dot(k::sub(c, e.center), k::sub(c, e.center)).sqrt();
        // 同じ円筒(か円すい)の面の、もう一方の縁(穴の深さ)
        let depth = all
            .iter()
            .filter(|o| o.faces.contains(side))
            .map(|o| dist(o.center))
            .fold(0.0, f64::max);
        let hole = e.is_hole(&all);
        let n = k::normalize(n);
        // 同じ軸の上で、面の内側にある円のいちばん奥
        let through = all
            .iter()
            .filter_map(|o| {
                let d = k::sub(o.center, e.center);
                let along = k::dot(d, n);
                let off = k::sub(d, k::scale(n, along));
                (along < 0.0 && k::dot(off, off) < 1e-8).then_some(-along)
            })
            .fold(depth, f64::max);
        let m = HoleMouth {
            center: e.center,
            normal: n,
            diameter: e.radius * 2.0,
            hole,
            depth,
            through,
            countersink: e.inside.is_none(),
        };
        out.push(HandleChoice {
            value: json!(m),
            label: format!(
                "{} 直径 {:.2} mm",
                if hole { "穴" } else { "軸" },
                e.radius * 2.0
            ),
            outline: vec![e.outline.clone()],
            group: 0,
            sized: false,
            color: None,
        });
    }
    out
}

/// 新しい規格部品のデータ(呼びと長さは最初の穴の口から決める)。
pub fn new_fastener(kind: Kind, mouths: &[HoleMouth]) -> Fastener {
    let first = &mouths[0];
    // 皿小ねじは頭を含む全長なので、皿の下の通し穴の奥まで
    let depth = |m: &HoleMouth| {
        if kind == Kind::FlatHead {
            m.through
        } else {
            m.depth
        }
    };
    let len = length_for(mouths.iter().map(depth).fold(0.0, f64::max));
    Fastener {
        kind,
        size: size_for(kind, first.diameter, first.hole),
        length: format!("{} mm", mm(len)),
        places: mouths
            .iter()
            .map(|m| Place {
                center: m.center,
                normal: m.normal,
                body: None,
                diameter: Some(m.diameter),
                on: None,
            })
            .collect(),
        offset: zero_mm(),
        // 軸受は穴の中へ、ほかは面の外へ(形の作り方の向きで決まるので反転はしない)
        flip: false,
        material: default_material(),
    }
}

/// 穴の口 `m` に、種類 `kind` の新しい部品を挿す時に重ねる先: 同じ中心と向きに挿してある座金・
/// ナットのうち、いちばん後の物(文書の並びで後ほど上に重なっている)。重ねられない種類は `None`。
pub fn stack_on(data: &jc_core::DocumentData, kind: Kind, m: &HoleMouth) -> Option<Stack> {
    // 軸受は穴の中、皿小ねじは皿の中に入るので重ねない
    if matches!(kind, Kind::Bearing | Kind::FlatHead) {
        return None;
    }
    let near = |a: P3, b: P3| {
        let d = k::sub(a, b);
        k::dot(d, d) < 1e-8
    };
    data.features
        .iter()
        .filter(|e| e.type_id == FASTENER)
        .filter_map(|e| {
            Some((
                e.id,
                serde_json::from_value::<Fastener>(e.data.clone()).ok()?,
            ))
        })
        .filter(|(_, f)| f.kind.stackable())
        .flat_map(|(id, f)| {
            let sign = if f.flip { -1.0 } else { 1.0 };
            f.places
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    near(p.center, m.center)
                        && k::dot(k::normalize(k::scale(p.normal, sign)), m.normal) > 0.999
                })
                .map(|(i, _)| Stack {
                    fastener: id,
                    index: i,
                })
                .collect::<Vec<_>>()
        })
        .last()
}
