//! CAM の操作(画面・CLI・MCP に出す): セットアップと加工の追加、残り削り、
//! G-code と段取り表の書き出し、シミュレーション、経路とストックの重ね描き、機械の検査。

use crate::paths_cache::SetupRef;
use crate::{OpBase, Post, SETUP, Setup, SetupPaths, TOOLS, TOOLS_V1, ToolKind, ToolLibrary};
use kmb_cam_setup::{FaceMenu, Needs, Picks, Strategy};
use kmb_cam_setup::{SetupInfo, is_rest_data};
use kmb_core::{
    Action, BoxKind, Context, Extension, FeatureId, FeatureStatus, Field, JobReport, LineKind,
    MarkKind, Outcome, Overlay, Place, Problem, ProblemKind, Settings,
};
use kmb_post::MachineProfile;
use kmb_post::machines::MachineLibrary;
use serde_json::json;
use std::sync::{Arc, Mutex, RwLock};

/// 操作の ID。
pub const ADD_SETUP: &str = "cam.add.setup";
pub const REST_POCKET: &str = "cam.rest_pocket";
pub const EXPORT_GCODE: &str = "cam.export_gcode";
pub const EXPORT_GCODE_AS: &str = "cam.export_gcode_as";
pub const VIEW_GCODE: &str = "cam.view_gcode";
pub const SETUP_SHEET: &str = "cam.setup_sheet";
pub const SIMULATE: &str = "cam.simulate";
/// 書き出す G-code を読み直して削る
pub const SIMULATE_GCODE: &str = "cam.simulate_gcode";
/// 段の帯の段を出す操作の前置き(後ろに段の番号。0 が素材)
pub const SHOW_STAGE: &str = "cam.stage.show:";
/// 段の形をやめて部品を出す
pub const CLEAR_STAGE: &str = "cam.stage.clear";

/// 段の形に使う粗い格子の、長い辺のセルの数(画面の描画が重くならない大きさ)。
const STAGE_CELLS: usize = 200;

/// CAM の操作。どの機械とポストを使うかは組み立てが決める。
pub struct CamCommands {
    /// 組み立てが選んだ機械とポストの一覧
    machines: MachineLibrary,
    /// 新しいセットアップの機械。設定の画面で変えられる(PC 全体の設定)。セットアップは足した時に
    /// この値を文書に写して持つ
    machine: Arc<RwLock<MachineProfile>>,
    /// 一覧の最初の機械(「初めの値に戻す」に使う)
    default_machine: MachineProfile,
    /// 設定で選んだポストのプラグイン(WASM のファイル)。なければ機械の値が選ぶポストを使う
    post_plugin: Arc<RwLock<Option<std::path::PathBuf>>>,
    /// 最後のシミュレーションの印。経路が変わったら出さない
    pub(crate) sim: Arc<Mutex<Option<SimMarks>>>,
    /// 動きの単位の再生。経路が変わったら出さない
    pub(crate) play: Arc<Mutex<Option<crate::playback::PlayRun>>>,
    /// シミュレーションの格子の大きさ(mm、PC 全体の設定)
    sim_cell: Arc<RwLock<f64>>,
    /// シミュレーションで荒削りを削る格子の大きさ(0 なら `sim_cell` と同じ)
    sim_rough_cell: Arc<RwLock<f64>>,
    /// 経路の読み直しの写し(画面が毎コマ求めるので、再計算の後だけ読み直す)
    pub(crate) paths: crate::paths_cache::PathsCache,
    /// 加工の既定とテンプレート
    pub(crate) reuse: crate::reuse::Reuse,
    /// 戦略の crate が持つ操作(戦略の並び順)
    pub(crate) commands: Vec<Arc<dyn Extension>>,
    /// 足せる加工の戦略(ツールバーの並び順)
    pub(crate) strategies: Arc<[Strategy]>,
}

impl CamCommands {
    /// 組み立てが選んだ機械とポストの一覧(最初が既定)、足せる加工の戦略(各戦略の crate の
    /// `module()` から)。
    pub fn new(machines: MachineLibrary, strategies: Arc<[Strategy]>) -> Self {
        let machine = machines
            .profiles()
            .into_iter()
            .next()
            .expect("マシンライブラリに機械が 1 台以上ある");
        let mut strategies = strategies.to_vec();
        strategies.sort_by_key(|s| s.order);
        let strategies: Arc<[Strategy]> = strategies.into();
        CamCommands {
            machines,
            machine: Arc::new(RwLock::new(machine.clone())),
            default_machine: machine,
            post_plugin: Default::default(),
            sim: Default::default(),
            play: Default::default(),
            sim_cell: Arc::new(RwLock::new(crate::SimOptions::default().cell)),
            sim_rough_cell: Arc::new(RwLock::new(SIM_ROUGH_CELL)),
            paths: Default::default(),
            reuse: crate::reuse::Reuse::new(strategies.clone()),
            commands: strategies
                .iter()
                .filter_map(|s| s.commands)
                .map(|c| c())
                .collect(),
            strategies,
        }
    }

    /// 種類 `type_id` の残り削りに使える工具の種類(残り削りを足せなければ空)。
    fn rest_of(&self, type_id: &str) -> &'static [ToolKind] {
        self.strategies
            .iter()
            .find(|s| s.type_id == type_id)
            .map_or(&[], |s| s.rest)
    }

    /// ポストのプラグインを選ぶ(空なら外す)。読めないファイルなら何も変えない。
    fn set_post_plugin(&self, value: &serde_json::Value) -> Result<(), String> {
        let path = match value.get("plugin") {
            None | Some(serde_json::Value::Null) => return Ok(()),
            Some(v) => v
                .as_str()
                .ok_or("プラグインはファイルのパス(文字)にしてください")?
                .trim(),
        };
        let new = if path.is_empty() {
            None
        } else {
            let p = std::path::PathBuf::from(path);
            // 読めて、ポストとして動くかを先に確かめる
            self.machines.load_post(&p)?;
            Some(p)
        };
        *self.post_plugin.write().expect("ポストの設定の鍵") = new;
        Ok(())
    }

    /// 設定で選んだ機械(新しいセットアップの機械。設定で変えた値)。
    pub fn machine(&self) -> MachineProfile {
        self.machine.read().expect("機械の設定の鍵").clone()
    }

    /// セットアップの機械の値(セットアップが文書に写して持つ値。経路の計算と同じ)。
    pub fn machine_for(&self, info: &SetupInfo) -> MachineProfile {
        info.machine_profile().clone()
    }

    /// 機械の値が選ぶポスト(マシンライブラリから)。
    pub fn post_for(&self, m: &MachineProfile) -> Result<Arc<dyn Post + Send + Sync>, String> {
        self.machines.post(&m.post).ok_or_else(|| {
            format!(
                "機械「{}」のポスト「{}」がありません(ポストのプラグインを選ぶと書き出せます)",
                m.name, m.post
            )
        })
    }

    /// 機械の選び(マシンライブラリの ID と名前)。
    fn choices(&self) -> Vec<(String, String)> {
        self.machines.choices()
    }

    /// 機械の設定の欄(機械の選びはマシンライブラリ)。
    fn machine_fields(&self) -> Vec<Field> {
        let choices = self.choices();
        let choices: Vec<(&str, &str)> = choices
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        machine_fields(&choices)
    }
}

/// ベッドの線(セットアップの座標)。`corner` は素材の左手前の下の角(L 字の角に当てる所)。
pub fn bed_lines(bed: &kmb_post::Bed, corner: [f64; 3]) -> Vec<Vec<[f64; 3]>> {
    let z = corner[2];
    // ベッドの左手前の角
    let (bx, by) = (corner[0] - bed.origin[0], corner[1] - bed.origin[1]);
    let at = |x: f64, y: f64| [bx + x, by + y, z];
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        vec![at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1), at(x0, y0)]
    };
    let circle = |cx: f64, cy: f64, r: f64| {
        (0..=16)
            .map(|k| {
                let a = k as f64 / 16.0 * std::f64::consts::TAU;
                at(cx + r * a.cos(), cy + r * a.sin())
            })
            .collect::<Vec<_>>()
    };
    let mut out = vec![rect(0.0, 0.0, bed.size[0], bed.size[1])];
    for h in &bed.holes {
        out.push(circle(h[0], h[1], bed.hole_diameter / 2.0));
    }
    if let Some([len, w]) = bed.fence {
        out.push(rect(0.0, 0.0, w, len));
        out.push(rect(0.0, 0.0, len, w));
    }
    if let Some(s) = bed.sensor {
        out.push(circle(bed.origin[0] + s[0], bed.origin[1] + s[1], 5.0));
    }
    out
}

/// 機械の設定の名前。
pub const MACHINE_SETTINGS: &str = "cam.machine";
/// ポストの設定の名前。
pub const POST_SETTINGS: &str = "cam.post";
/// シミュレーションの設定の名前。
pub const SIM_SETTINGS: &str = "cam.sim";

/// シミュレーションの格子の大きさの選び(mm と、その説明)。
/// 荒削りの加工を削る格子の既定(mm)。仕上げの 0.1 mm の倍で、削る点の数が 4 分の 1 ほどになる
const SIM_ROUGH_CELL: f64 = 0.2;

const SIM_ROUGH_CELLS: [(&str, &str); 5] = [
    ("0", "仕上げと同じ"),
    ("0.1", "0.1 mm"),
    ("0.2", "0.2 mm(標準)"),
    ("0.5", "0.5 mm(粗い・速い)"),
    ("1", "1 mm(とても粗い・速い)"),
];

/// 結果の窓に出す、加工ごとの格子の行(大きさごとにまとめる。全部同じなら 1 行)。
fn grid_lines(cells: &[(String, f64)]) -> Vec<String> {
    let mut sizes: Vec<f64> = cells.iter().map(|c| c.1).collect();
    sizes.sort_by(f64::total_cmp);
    sizes.dedup();
    match sizes.as_slice() {
        [] => vec![],
        [one] => vec![format!("  格子 {} mm", kmb_post::num(*one))],
        _ => std::iter::once("  格子(加工ごと)".to_string())
            .chain(sizes.iter().rev().map(|c| {
                let names: Vec<&str> = cells
                    .iter()
                    .filter(|x| x.1 == *c)
                    .map(|x| x.0.as_str())
                    .collect();
                format!("    {} mm: {}", kmb_post::num(*c), names.join("、"))
            }))
            .collect(),
    }
}

const SIM_CELLS: [(&str, &str); 4] = [
    ("0.05", "0.05 mm(細かい・遅い)"),
    ("0.1", "0.1 mm(標準)"),
    ("0.2", "0.2 mm(粗い)"),
    ("0.5", "0.5 mm(とても粗い・速い)"),
];

/// 機械の設定の欄。名前・工具番号・コレットは機械の作りで決まるので、説明だけにする。
fn machine_fields(choices: &[(&str, &str)]) -> Vec<Field> {
    let speed = "加工時間の目安に使う";
    let frame = "主軸の中心が動ける範囲の手前左の角(ベッドの左手前の角から)。シミュレーションで作業範囲のはみ出しを見る";
    let head = "主軸の頭の形(主軸の鼻から上へ、丸い主軸とその上の Z の台の箱)。シミュレーションで材料と固定具への当たりを見る。0 なら見ない";
    vec![
        Field::choice("machine", "機械", choices).help(
            "新しいセットアップの機械。替えると、下の値はその機械の初めの値になる。今あるセットアップの機械は、セットアップの欄で替える",
        ),
        Field::number("travel_x", "作業範囲 X(mm)").plain("mm"),
        Field::number("travel_y", "作業範囲 Y(mm)").plain("mm"),
        Field::number("travel_z", "作業範囲 Z(mm)").plain("mm"),
        Field::number("min_rpm", "主軸の最低回転数(rpm)").plain("rpm"),
        Field::number("max_rpm", "主軸の最高回転数(rpm)").plain("rpm"),
        Field::number("rapid_feed", "早送り(mm/min)").plain("mm/min").help(speed),
        Field::number("acceleration", "加速度(mm/s²)").plain("mm/s²")
            .help("加工時間の目安に使う。0 なら加減速を見込まない"),
        Field::number("junction_deviation", "角の回りやすさ(mm)").plain("mm")
            .help("角を回る速さを決める値(junction deviation)。加工時間の目安に使う"),
        Field::number("max_rate_x", "X の最高速(mm/min)").plain("mm/min").help("0 なら制限しない"),
        Field::number("max_rate_y", "Y の最高速(mm/min)").plain("mm/min").help("0 なら制限しない"),
        Field::number("max_rate_z", "Z の最高速(mm/min)").plain("mm/min").help("0 なら制限しない"),
        Field::bool("measure_after_tool_change", "工具交換の後に工具長を測る"),
        Field::bool("air", "主軸を回す間エアを出す"),
        Field::number("travel_corner_x", "作業範囲の手前左の角 X(mm)").plain("mm").help(frame),
        Field::number("travel_corner_y", "作業範囲の手前左の角 Y(mm)").plain("mm").help(frame),
        Field::number("nose_low", "主軸の鼻のいちばん下の高さ(mm)").plain("mm")
            .help("主軸をいちばん下げた時の、コレットナットの付け根の高さ(ベッドの上面から)。工具の長さと合わせて、Z の作業範囲を見る"),
        Field::number("spindle_diameter", "主軸の径(mm)").plain("mm").help(head),
        Field::number("spindle_length", "主軸の長さ(mm)").plain("mm").help(head),
        Field::number("carriage_width", "Z の台の幅 X(mm)").plain("mm").help(head),
        Field::number("carriage_depth", "Z の台の奥行き Y(mm)").plain("mm").help(head),
        Field::number("carriage_height", "Z の台の高さ(mm)").plain("mm").help(head),
        Field::number("clearance", "門の下の高さ(mm)").plain("mm")
            .help("ベッドの上面から門(ガントリー)の下まで。ストックと固定具がこれより高いと通らない。0 なら見ない"),
        Field::number("bed_allowance", "ベッドを削ってよい深さ(mm)").plain("mm")
            .help("貫通の穴や切り抜きで、刃がベッド(捨て板)を削ってよい深さ。これより深いと当たりにする"),
    ]
}

fn machine_value(m: &MachineProfile) -> serde_json::Value {
    let f = m.frame.clone().unwrap_or_default();
    json!({
        "machine": m.id,
        "travel_x": m.travel[0],
        "travel_y": m.travel[1],
        "travel_z": m.travel[2],
        "min_rpm": m.min_rpm,
        "max_rpm": m.max_rpm,
        "rapid_feed": m.rapid_feed,
        "acceleration": m.acceleration,
        "junction_deviation": m.junction_deviation,
        "max_rate_x": m.max_rate[0],
        "max_rate_y": m.max_rate[1],
        "max_rate_z": m.max_rate[2],
        "measure_after_tool_change": m.measure_after_tool_change,
        "air": m.air,
        "travel_corner_x": f.travel_corner[0],
        "travel_corner_y": f.travel_corner[1],
        "nose_low": f.nose_low,
        "spindle_diameter": f.spindle[0],
        "spindle_length": f.spindle[1],
        "carriage_width": f.carriage[0],
        "carriage_depth": f.carriage[1],
        "carriage_height": f.carriage[2],
        "clearance": f.clearance,
        "bed_allowance": f.bed_allowance,
    })
}

/// 設定の値を機械に当てたもの。ない項目は今の値のまま。正しくない値なら理由を返す。
fn machine_with(m: &MachineProfile, v: &serde_json::Value) -> Result<MachineProfile, String> {
    let fields = machine_fields(&[]);
    let label = |k: &str| {
        fields
            .iter()
            .find(|f| f.key == k)
            .map_or(k.to_string(), |f| f.label.clone())
    };
    let num = |k: &str, cur: f64| -> Result<f64, String> {
        match v.get(k) {
            None | Some(serde_json::Value::Null) => Ok(cur),
            Some(x) => x
                .as_f64()
                .filter(|x| x.is_finite() && *x >= 0.0)
                .ok_or_else(|| format!("{}は 0 以上の数にしてください", label(k))),
        }
    };
    let flag = |k: &str, cur: bool| -> Result<bool, String> {
        match v.get(k) {
            None | Some(serde_json::Value::Null) => Ok(cur),
            Some(x) => x
                .as_bool()
                .ok_or_else(|| format!("{}は真か偽にしてください", label(k))),
        }
    };
    let mut out = m.clone();
    for (k, axis) in ["x", "y", "z"].iter().enumerate() {
        let key = format!("travel_{axis}");
        out.travel[k] = num(&key, m.travel[k])?;
        if out.travel[k] <= 0.0 {
            return Err(format!("{}は 0 より大きくしてください", label(&key)));
        }
        out.max_rate[k] = num(&format!("max_rate_{axis}"), m.max_rate[k])?;
    }
    out.min_rpm = num("min_rpm", m.min_rpm)?;
    out.max_rpm = num("max_rpm", m.max_rpm)?;
    if out.min_rpm > out.max_rpm {
        return Err("主軸の最低回転数が最高回転数を超えています".into());
    }
    out.rapid_feed = num("rapid_feed", m.rapid_feed)?;
    if out.rapid_feed <= 0.0 {
        return Err(format!(
            "{}は 0 より大きくしてください",
            label("rapid_feed")
        ));
    }
    out.acceleration = num("acceleration", m.acceleration)?;
    out.junction_deviation = num("junction_deviation", m.junction_deviation)?;
    out.measure_after_tool_change = flag("measure_after_tool_change", m.measure_after_tool_change)?;
    out.air = flag("air", m.air)?;
    // 機械の形。作業範囲の角だけは負でもよい(ベッドの角より手前・左から動ける機械)
    let signed = |k: &str, cur: f64| -> Result<f64, String> {
        match v.get(k) {
            None | Some(serde_json::Value::Null) => Ok(cur),
            Some(x) => x
                .as_f64()
                .filter(|x| x.is_finite())
                .ok_or_else(|| format!("{}は数にしてください", label(k))),
        }
    };
    let had = m.frame.clone();
    let mut f = had.clone().unwrap_or_default();
    f.travel_corner = [
        signed("travel_corner_x", f.travel_corner[0])?,
        signed("travel_corner_y", f.travel_corner[1])?,
    ];
    f.nose_low = signed("nose_low", f.nose_low)?;
    f.spindle = [
        num("spindle_diameter", f.spindle[0])?,
        num("spindle_length", f.spindle[1])?,
    ];
    f.carriage = [
        num("carriage_width", f.carriage[0])?,
        num("carriage_depth", f.carriage[1])?,
        num("carriage_height", f.carriage[2])?,
    ];
    f.clearance = num("clearance", f.clearance)?;
    f.bed_allowance = num("bed_allowance", f.bed_allowance)?;
    // 形の無い機械は、どれかの値を入れた時だけ形を持つ
    out.frame = (had.is_some() || f != kmb_post::MachineFrame::default()).then_some(f);
    Ok(out)
}

/// シミュレーションの印(作業座標系)と、それを出した時のセットアップと経路の指紋。
pub(crate) struct SimMarks {
    pub(crate) setup: FeatureId,
    pub(crate) print: u64,
    marks: Vec<([f64; 3], MarkKind, f64)>,
    /// 途中の形(素材と各経路の後。世界座標の網)
    pub(crate) stages: Vec<Arc<kmb_core::OverlayShape>>,
    /// 3D のビューに出している段
    pub(crate) selected: Option<usize>,
    /// 何を削ったか(「経路」か「G-code」)
    pub(crate) source: &'static str,
}

/// 何を削るか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SimSource {
    /// 経路(ポストに渡す前)
    Paths,
    /// 書き出す G-code を読み直したもの
    Gcode,
}

/// 経路の指紋(名前、工具、動きの行き先)。印が今の経路のものかを見分ける。
pub(crate) fn fingerprint(paths: &[crate::Toolpath]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in paths {
        p.name.hash(&mut h);
        p.tool.number.hash(&mut h);
        p.moves.len().hash(&mut h);
        for m in &p.moves {
            for v in m.to() {
                v.to_bits().hash(&mut h);
            }
        }
    }
    h.finish()
}

fn is_tools(t: &str) -> bool {
    t == TOOLS || t == TOOLS_V1
}

pub(crate) fn last_setup(cx: &Context<'_>) -> Option<FeatureId> {
    cx.last_where(|t| t == SETUP)
}

pub(crate) fn last_tools(cx: &Context<'_>) -> Option<FeatureId> {
    cx.last_where(is_tools)
}

/// 選んでいるセットアップ(加工の操作を選んでいれば、その操作のセットアップ)。
pub(crate) fn selected_setup(cx: &Context<'_>) -> Option<FeatureId> {
    let sel = cx.selected?;
    if cx.selected_type()? == SETUP {
        return Some(sel);
    }
    let f = cx.data.feature(sel)?;
    if !crate::is_operation(cx.registry, &f.type_id) {
        return None;
    }
    let s = FeatureId(f.data.get("setup")?.as_u64()?);
    (cx.data.feature(s)?.type_id == SETUP).then_some(s)
}

/// セットアップそのものを選んでいる時だけ、そのセットアップ(加工の操作を選んでいる時は `None`)。
pub(crate) fn picked_setup(cx: &Context<'_>) -> Option<FeatureId> {
    (cx.selected_type()? == SETUP).then_some(cx.selected?)
}

/// セットアップ `id` の値。
pub(crate) fn setup_of(cx: &Context<'_>, id: FeatureId) -> Option<Setup> {
    serde_json::from_value(cx.data.feature(id)?.data.clone()).ok()
}

/// 画面で置いた位置や大きさを文書に書く前に 0.001 mm に丸める。
pub(crate) fn round_um(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// 彫刻と V 彫りの元(出力 `slot` を持つ機能): 選んでいる機能が持てばそれ、なければ最後のもの。
fn source_of(cx: &Context<'_>, slot: &str) -> Option<FeatureId> {
    if let Some(s) = cx.selected
        && cx.output(s, slot).is_some()
    {
        return Some(s);
    }
    // 選んでいなければ、使える物を出す最後の機能。線は、点だけのスケッチ(穴の位置など)や
    // 補助線だけのスケッチを飛ばす(足した直後に「使える線がありません」にならないように)
    let usable = |v: &kmb_core::Value| {
        slot != "curves" || kmb_core::Curves::from_value(v).is_some_and(|c| !c.segments.is_empty())
    };
    cx.data
        .features
        .iter()
        .rev()
        .find(|f| cx.output(f.id, slot).is_some_and(usable))
        .map(|f| f.id)
}

fn library(cx: &Context<'_>, tools: FeatureId) -> Option<ToolLibrary> {
    serde_json::from_value(cx.data.feature(tools)?.data.clone()).ok()
}

/// 工具ライブラリで、種類 `kind` の最初の工具の番号。
fn tool_of_kind(cx: &Context<'_>, tools: FeatureId, kind: ToolKind) -> Option<u32> {
    library(cx, tools)?
        .tools
        .iter()
        .find(|t| t.kind == kind)
        .map(|t| t.number)
}

/// ファイル名に使えるセットアップの名前(空白と、ファイル名に使えない字を除く)。
pub(crate) fn file_part(name: &str) -> String {
    let s: String = name
        .chars()
        .filter(|c| !c.is_whitespace() && !r#"\/:*?"<>|"#.contains(*c))
        .collect();
    if s.is_empty() { "setup".into() } else { s }
}

/// いちばん細い平の工具。
fn smallest_flat(cx: &Context<'_>, tools: FeatureId) -> Option<u32> {
    library(cx, tools)?
        .tools
        .iter()
        .filter(|t| t.kind == ToolKind::FlatEnd)
        .min_by(|a, b| a.diameter.total_cmp(&b.diameter))
        .map(|t| t.number)
}

/// ポケットと負荷制御粗取りの底の面: 最後に選んだ面と、一緒に選んだほかの平らな面。
/// 選んでいなければ、スケッチの閉じた輪郭(選んでいるスケッチ)を使う(面は空)。
fn floor_faces(cx: &Context<'_>) -> Result<(String, Vec<String>), String> {
    let Some(p) = cx.picked.filter(|_| cx.picked_plane().is_some()) else {
        if selected_profile(cx).is_some() {
            return Ok((String::new(), Vec::new()));
        }
        return Err("底の面を選んでいません".into());
    };
    let others = cx
        .also_picked
        .iter()
        .filter(|f| f.feature == p.feature && f.face != p.face && is_planar(cx, f))
        .map(|f| f.face.clone())
        .collect();
    Ok((p.face.clone(), others))
}

/// タイムラインで選んでいる、閉じた輪郭を出す機能(スケッチ)。
fn selected_profile(cx: &Context<'_>) -> Option<FeatureId> {
    let s = cx.selected?;
    cx.output(s, "profile").is_some().then_some(s)
}

/// スケッチ `id` からポケットを作る時の深さ: 平面が部品のいちばん高い上向きの面より下なら空
/// (平面の高さが底)、そうでなければ 2 mm(部品の上面に描いた輪郭を掘る)。
fn sketch_depth(cx: &Context<'_>, id: FeatureId) -> Option<String> {
    let plane = kmb_core::Profile::from_value(cx.output(id, "profile")?)?
        .plane
        .origin[2];
    let top = cx
        .visible_bodies()
        .iter()
        .filter_map(|b| cx.kernel.faces(b.1).ok())
        .flatten()
        .filter_map(|f| f.planar)
        .filter(|p| p.normal[2] > 0.999)
        .map(|p| p.point[2])
        .fold(f64::NEG_INFINITY, f64::max);
    (plane >= top - 1e-3).then(|| "2 mm".to_string())
}

/// 面 `f` が平らか。
fn is_planar(cx: &Context<'_>, f: &kmb_core::FacePick) -> bool {
    Context {
        picked: Some(f),
        ..*cx
    }
    .picked_plane()
    .is_some()
}

/// 加工を足せないなら、その理由。
fn why_not(cx: &Context<'_>, s: &Strategy) -> Option<&'static str> {
    if last_setup(cx).is_none() || last_tools(cx).is_none() {
        return Some("先にセットアップを足す");
    }
    let ok = match s.needs {
        Needs::Nothing => true,
        Needs::FloorOrProfile => cx.picked_plane().is_some() || selected_profile(cx).is_some(),
        Needs::Curves => source_of(cx, "curves").is_some(),
        Needs::Profile => source_of(cx, "profile").is_some(),
    };
    (!ok).then(|| s.needs.why())
}

/// ポケット・輪郭・負荷制御粗取り `id` の残り削りに使う工具: 今の工具より先(いちばん下)の細い
/// 工具のうち、いちばん太いもの。使える工具の種類 `rest` は戦略が決める(ポケットと輪郭は平・V・
/// テーパー、負荷制御粗取りは平だけ)。
fn rest_tool_for(cx: &Context<'_>, rest: &[ToolKind], id: FeatureId) -> Option<u32> {
    let f = cx.data.feature(id)?;
    if rest.is_empty() || is_rest_data(&f.data) {
        return None;
    }
    let p = serde_json::from_value::<OpBase>(f.data.clone()).ok()?;
    let lib = library(cx, p.tools)?;
    let now = lib.tools.iter().find(|t| t.number == p.tool)?;
    lib.tools
        .iter()
        .filter(|t| rest.contains(&t.kind) && t.tip_radius() < now.tip_radius() - 1e-6)
        .max_by(|a, b| a.tip_radius().total_cmp(&b.tip_radius()))
        .map(|t| t.number)
}

/// セットアップを足す。機械は設定で選んだ機械 `machine`(設定で直した値を文書に写す)。
fn add_setup(cx: &Context<'_>, machine: &MachineProfile) -> Result<Outcome, kmb_core::Problem> {
    let body = cx
        .visible_bodies()
        .last()
        .map(|b| b.0)
        .ok_or("加工するボディがありません")?;
    let mut edits = Vec::new();
    if last_tools(cx).is_none() {
        edits.push(kmb_core::Edit::Insert {
            type_id: TOOLS.into(),
            name: "工具".into(),
            data: serde_json::to_value(ToolLibrary::starter()).expect("JSON にできる"),
        });
    }
    let mut setup = Setup::new(body);
    setup.set_machine(machine.clone());
    // 設計でボディに付けた物理材料を、ストックの材料(切削条件のプリセットを選ぶ)の既定にする(M26)
    if let Some(m) =
        kmb_look::material::of(cx.data).get(kmb_core::body_root(cx.data, cx.registry, body))
    {
        setup.stock.material = m.to_string();
    }
    edits.push(kmb_core::Edit::Insert {
        type_id: SETUP.into(),
        name: format!("セットアップ {}", cx.count_of(SETUP) + 1),
        data: serde_json::to_value(setup).expect("JSON にできる"),
    });
    Ok(Outcome::Change {
        label: "セットアップを追加".into(),
        edits,
    })
}

/// 足す加工の初めの値を作る時の、今の文書と選び。
struct Here<'a, 'c> {
    cx: &'a Context<'c>,
    setup: FeatureId,
    tools: FeatureId,
}

impl Picks for Here<'_, '_> {
    fn base(&self) -> OpBase {
        OpBase::new(self.setup, self.tools, 1)
    }

    fn tool_of_kind(&self, kind: ToolKind) -> Option<u32> {
        tool_of_kind(self.cx, self.tools, kind)
    }

    fn smallest_flat(&self) -> Option<u32> {
        smallest_flat(self.cx, self.tools)
    }

    fn planar_face(&self) -> Option<String> {
        let planar = self.cx.picked_plane().is_some();
        self.cx.picked.filter(|_| planar).map(|p| p.face.clone())
    }

    fn wall_face(&self) -> Option<String> {
        let planar = self.cx.picked_plane().is_some();
        self.cx.picked.filter(|_| !planar).map(|p| p.face.clone())
    }

    fn floor_faces(&self) -> Result<(String, Vec<String>), String> {
        floor_faces(self.cx)
    }

    fn selected_profile(&self) -> Option<FeatureId> {
        selected_profile(self.cx)
    }

    fn sketch_depth(&self, id: FeatureId) -> Option<String> {
        sketch_depth(self.cx, id)
    }

    fn source(&self, slot: &str) -> Option<FeatureId> {
        source_of(self.cx, slot)
    }

    fn setup_data(&self) -> Option<serde_json::Value> {
        Some(self.cx.data.feature(self.setup)?.data.clone())
    }

    fn wall_refs(&self) -> Vec<kmb_core::Ref> {
        self.cx
            .picked_faces()
            .into_iter()
            .filter(|f| {
                Context {
                    picked: Some(f),
                    ..*self.cx
                }
                .picked_plane()
                .is_none()
            })
            .map(|f| kmb_core::Ref::Face {
                feature: f.feature,
                face: f.face.clone(),
            })
            .collect()
    }

    fn edge_refs(&self) -> Vec<kmb_core::Ref> {
        self.cx
            .edges
            .iter()
            .map(|e| kmb_core::Ref::Edge {
                feature: e.feature,
                faces: e.faces.clone(),
            })
            .collect()
    }
}

/// 選んでいるセットアップ(なければ最後のセットアップ)に、戦略 `s` の加工を足す。初めの値は
/// 戦略が決める(工具は 1 番か、戦略が選ぶ種類の工具。ポケットは選んだ面を底にする)。
pub(crate) fn add_operation(cx: &Context<'_>, s: &Strategy) -> Result<Outcome, kmb_core::Problem> {
    if let Some(why) = why_not(cx, s) {
        return Err(format!("{}: {why}", s.label).into());
    }
    // 選んでいるセットアップ(またはその加工)に足す(Z5: いつも最後のセットアップに足していた)
    let setup = selected_setup(cx)
        .or_else(|| last_setup(cx))
        .ok_or("セットアップがありません")?;
    let tools = last_tools(cx).ok_or("工具ライブラリがありません")?;
    let data = (s.make)(&Here { cx, setup, tools })?;
    let n = cx
        .data
        .features
        .iter()
        .filter(|f| crate::is_operation(cx.registry, &f.type_id))
        .count()
        + 1;
    Ok(Outcome::insert(
        "加工を追加",
        s.type_id,
        format!("{}{n}", s.label),
        data,
    ))
}

/// ポケット・輪郭・負荷制御粗取り `id` の後に、細い工具でその削り残しだけを削る同じ種類の加工を足す。
fn add_rest_pocket(
    cx: &Context<'_>,
    rest: &[ToolKind],
    id: FeatureId,
) -> Result<Outcome, kmb_core::Problem> {
    let tool = rest_tool_for(cx, rest, id).ok_or("今の工具より先の細い工具がありません")?;
    let f = cx.data.feature(id).ok_or("機能がありません")?;
    let prev = f
        .data
        .get("tool")
        .and_then(|v| v.as_u64())
        .ok_or("工具がありません")?;
    let mut d = f.data.clone();
    // 「工具の順番に足す」と同じく、前の工具の切削条件の上書き・仕上げ・入る位置は写さない
    crate::tool_order::make_rest(&mut d, prev as u32, tool);
    Ok(Outcome::insert(
        "残り削りを追加",
        &f.type_id,
        format!("{} の残り", f.name),
        d,
    ))
}

impl CamCommands {
    /// 全部のセットアップの経路(タイムラインの順)。
    pub(crate) fn paths_all(&self, cx: &Context<'_>) -> Vec<SetupPaths> {
        (*self.paths.all(cx).0).clone()
    }

    pub(crate) fn setup_paths(
        &self,
        cx: &Context<'_>,
        setup: FeatureId,
    ) -> Result<SetupRef, String> {
        self.paths
            .get(cx, setup)
            .ok_or_else(|| "セットアップの計算ができていません".into())
    }

    /// G-code の中身。指定 `post` があれば、そのファイルのポストのプラグイン(WASM)で書く。
    /// なければ設定で選んだプラグイン、それもなければ組み立てが渡したポストで書く。
    pub(crate) fn gcode_text(
        &self,
        cx: &Context<'_>,
        s: &SetupPaths,
    ) -> Result<(String, String), String> {
        self.with_post(cx, s.info.machine_profile(), |post, m| {
            crate::gcode(s, post, m)
        })
    }

    /// ポスト(`gcode_text` と同じ選び方)と機械 `machine`(セットアップの機械の ID)で `f` を呼び、
    /// 結果とポストの名前を返す。
    pub(crate) fn with_post<T>(
        &self,
        cx: &Context<'_>,
        m: &MachineProfile,
        f: impl FnOnce(&dyn Post, &MachineProfile) -> Result<T, String>,
    ) -> Result<(T, String), String> {
        let chosen = self.post_plugin.read().expect("ポストの設定の鍵").clone();
        let plugin = cx
            .arg_str("post")
            .map(std::path::PathBuf::from)
            .or(chosen)
            .map(|p| self.machines.load_post(&p))
            .transpose()?;
        let builtin;
        let post: &dyn Post = match &plugin {
            Some(p) => p.as_ref(),
            None => {
                builtin = self.post_for(m)?;
                builtin.as_ref()
            }
        };
        Ok((f(post, m)?, post.id().to_string()))
    }

    /// G-code の書き出し。
    fn gcode(&self, cx: &Context<'_>, setup: FeatureId) -> Result<Outcome, kmb_core::Problem> {
        let s = self.setup_paths(cx, setup)?;
        if s.info.sections.is_some() { return self.section_files(cx,&s); }
        let (g, post) = self.gcode_text(cx, &s)?;
        let minutes: f64 = s
            .paths
            .iter()
            .map(|p| p.minutes(&self.machine_for(&s.info).motion()))
            .sum();
        let info = json!({
            "setup": setup.0,
            "post": post,
            "lines": g.lines().count(),
            "tools": s.program().tools().iter().map(|t| t.number).collect::<Vec<_>>(),
            "minutes": (minutes * 10.0).round() / 10.0,
        });
        Ok(Outcome::File {
            suffix: format!("{}.{}", file_part(&s.name), kmb_post::file_extension(&post)),
            contents: g.into_bytes(),
            info,
        })
    }

    pub(crate) fn section_files(&self, cx: &Context<'_>, s: &crate::SetupPaths) -> Result<Outcome,kmb_core::Problem> {
        let (files,post)=self.with_post(cx,s.info.machine_profile(),|post,m|kmb_cam_program::gcode_files(s,post,m))?;
        let info=json!({"setup":s.setup.0,"post":post,"sections":files.len()});
        let mut files:Vec<_>=files.into_iter().map(|(name,text)|(name,text.into_bytes())).collect();
        if files.len()==1 {
            let (suffix,contents)=files.remove(0);
            Ok(Outcome::File { suffix,contents,info })
        } else { Ok(Outcome::Files { files,info }) }
    }

    fn sheet(&self, cx: &Context<'_>, setup: FeatureId) -> Result<Outcome, kmb_core::Problem> {
        let s = self.setup_paths(cx, setup)?;
        Ok(Outcome::File {
            suffix: format!("{}-段取り.md", file_part(&s.name)),
            contents: crate::setup_sheet(&s, &self.machine_for(&s.info)).into_bytes(),
            info: json!({"setup": setup.0}),
        })
    }

    /// 前のセットアップ(同じボディとストック)で削った形から続けて削ってみる。
    /// 荒削りの加工は粗い格子、ほかは細かい格子で削る(加工の欄「シミュレーションの精度」と設定)。
    /// 指定 `cell` で格子の大きさ(mm)を変えられる(`rough_cell` も指定しなければ全部の加工をこの大きさで)。`source` が G-code なら、書き出す G-code を
    /// 読み直した経路で削る。終わったら段 `show`(あれば)を 3D のビューに出す。
    pub(crate) fn simulate(
        &self,
        cx: &Context<'_>,
        setup: FeatureId,
        source: SimSource,
        show: Option<usize>,
    ) -> Result<Outcome, kmb_core::Problem> {
        let mut opt = crate::SimOptions {
            stage_cells: STAGE_CELLS,
            cell: *self.sim_cell.read().expect("シミュレーションの設定の鍵"),
            rough_cell: *self
                .sim_rough_cell
                .read()
                .expect("シミュレーションの設定の鍵"),
            ..Default::default()
        };
        if let Some(cell) = cx.arg_f64("cell")? {
            if !(cell > 0.01 && cell <= 2.0) {
                return Err("格子の大きさは 0.01〜2 mm にしてください".into());
            }
            opt.cell = cell;
            opt.rough_cell = 0.0;
        }
        if let Some(cell) = cx.arg_f64("rough_cell")? {
            if !(cell == 0.0 || (cell > 0.01 && cell <= 2.0)) {
                return Err(
                    "荒削りの格子の大きさは 0(細かい格子と同じ)か 0.01〜2 mm にしてください".into(),
                );
            }
            opt.rough_cell = cell;
        }
        let mut all = (*self.paths.all(cx).0).clone();
        let i = all
            .iter()
            .position(|s| s.setup == setup)
            .ok_or("セットアップの計算ができていません")?;
        if let Some((id, m)) = all[i].failed.first() {
            return Err(format!("{id} の経路ができていません: {m}").into());
        }
        let print = fingerprint(&all[i].paths);
        let what = match source {
            SimSource::Paths => "経路",
            SimSource::Gcode => "G-code",
        };
        if source == SimSource::Gcode {
            // 前のセットアップも含めて、機械に送る G-code そのものを削る
            for s in all
                .iter_mut()
                .take(i + 1)
                .filter(|s| s.failed.is_empty() && !s.paths.is_empty())
            {
                if s.info.sections.is_some() {
                    *s=self.with_post(cx,s.info.machine_profile(),|post,m|kmb_cam_program::with_section_gcode_paths(s,post,m))?.0;
                } else {
                    let (g, _) = self.gcode_text(cx, s)?;
                    *s = crate::with_gcode_paths(s, &g)?;
                }
            }
        }
        // 加工ごとの格子の大きさ(結果の窓と info に出す)
        let cells: Vec<(String, f64)> = all[i]
            .paths
            .iter()
            .map(|p| p.name.clone())
            .zip(crate::path_cells(&all[i], &opt))
            .collect();
        let kernel = cx.kernel.clone();
        let store = self.sim.clone();
        Ok(Outcome::Job(Box::new(
            move |_: &kmb_core::JobControl| match crate::simulate_chain(
                kernel.as_ref(),
                &all,
                i,
                opt,
            ) {
                Ok(r) => {
                    let info = &all[i].info;
                    // 食い込みと削り残しの位置を、3D のビューに重ねるために覚える
                    let marks = r
                        .gouges
                        .iter()
                        .map(|e| (e.at, kmb_interchange::ink::mark::GOUGE, e.amount))
                        .chain(r.collisions.iter().chain(&r.fixture_hits).map(|c| {
                            (
                                c.event.at,
                                kmb_interchange::ink::mark::GOUGE,
                                c.event.amount,
                            )
                        }))
                        .chain(r.machine_hits.iter().map(|h| {
                            (
                                h.event.at,
                                kmb_interchange::ink::mark::GOUGE,
                                h.event.amount,
                            )
                        }))
                        .chain(
                            r.leftovers
                                .iter()
                                .map(|(p, d)| (*p, kmb_interchange::ink::mark::LEFTOVER, *d)),
                        )
                        .collect();
                    let stages = r
                        .stages
                        .iter()
                        .enumerate()
                        .map(|(k, (name, map))| {
                            Arc::new(crate::stage_shape(info, map, name, print ^ (k as u64 + 1)))
                        })
                        .collect();
                    *store.lock().unwrap_or_else(|e| e.into_inner()) = Some(SimMarks {
                        setup,
                        print,
                        marks,
                        stages,
                        selected: show,
                        source: what,
                    });
                    let summary = format!("{what}で削った結果: {}", r.summary());
                    // 窓に出す行: 1 行目の数を 1 つずつの行に分ける
                    let body = r.summary();
                    let (first, rest) = body.split_once('\n').unwrap_or((&body, ""));
                    // 機械の値に形が無ければ、機械との当たりを見ていないことを書く(黙って「0 件」に見せない)
                    let no_frame = info.machine_profile().frame.is_none().then(|| {
                        format!(
                            "  {} の値に機械の形が無いので、作業範囲のはみ出しと主軸の頭・ベッドの当たりは見ていません",
                            info.machine_profile().name
                        )
                    });
                    let report: Vec<String> = std::iter::once(format!("{what}で削った結果"))
                        .chain(first.split('、').map(|x| format!("  {x}")))
                        .chain(no_frame)
                        .chain(grid_lines(&cells))
                        .chain(rest.lines().map(String::from))
                        .collect();
                    JobReport {
                        ok: r.is_ok(),
                        text: summary.replace('\n', " "),
                        info: json!({
                            "ok": r.is_ok(),
                            "source": what,
                            "summary": summary,
                            "title": "シミュレーションの結果",
                            "report": report,
                            "removed_by_operation": r.removed_by_op.iter()
                                .map(|(n, v)| json!({"name": n, "volume": (v * 10.0).round() / 10.0}))
                                .collect::<Vec<_>>(),
                            "engagement": r.engagement.iter()
                                .map(|e| json!({
                                    "name": e.op,
                                    "mean_deg": e.mean.round(),
                                    "max_deg": e.max.round(),
                                    "slot_length": (e.slot_length * 10.0).round() / 10.0,
                                    "length": (e.length * 10.0).round() / 10.0,
                                }))
                                .collect::<Vec<_>>(),
                            "collisions": r.collisions.iter()
                                .map(|c| json!({
                                    "name": c.event.op,
                                    "part": c.part.label(),
                                    "at": c.event.at,
                                    "depth": (c.event.amount * 100.0).round() / 100.0,
                                }))
                                .collect::<Vec<_>>(),
                            "fixture_hits": r.fixture_hits.iter()
                                .map(|c| json!({
                                    "name": c.event.op,
                                    "part": c.part.label(),
                                    "at": c.event.at,
                                    "depth": (c.event.amount * 100.0).round() / 100.0,
                                }))
                                .collect::<Vec<_>>(),
                            "machine_hits": r.machine_hits.iter()
                                .map(|h| json!({
                                    "name": h.event.op,
                                    "kind": h.kind.label(),
                                    "at": h.event.at,
                                    "amount": (h.event.amount * 100.0).round() / 100.0,
                                }))
                                .collect::<Vec<_>>(),
                            "machine_shape": info.machine_profile().frame.is_some(),
                            "bed_depth": (r.bed_depth * 100.0).round() / 100.0,
                            "cell": opt.cell,
                            "rough_cell": opt.rough(),
                            "cells": cells.iter()
                                .map(|(n, c)| json!({"name": n, "cell": c}))
                                .collect::<Vec<_>>(),
                            "leftover_marks": r.leftovers.len(),
                            "stages": r.stages.len(),
                        }),
                    }
                }
                Err(e) => JobReport::failed(e),
            },
        )))
    }

    /// 段の帯の、削った形の説明(画面の形と、食い込みを調べた格子)。
    pub(crate) fn stage_note(&self, source: &str) -> String {
        let fine = *self.sim_cell.read().expect("シミュレーションの設定の鍵");
        let rough = *self
            .sim_rough_cell
            .read()
            .expect("シミュレーションの設定の鍵");
        let grid = if rough > 0.0 && rough != fine {
            format!(
                "荒削りは {} mm、ほかは {} mm の格子",
                kmb_post::num(rough),
                kmb_post::num(fine)
            )
        } else {
            format!("{} mm の格子", kmb_post::num(fine))
        };
        format!("{source}で削った形(画面の形は粗い格子。食い込みの検査は{grid})")
    }

    /// 選んでいる機能が属するセットアップ(セットアップそのものか、加工の機能のセットアップ)。
    pub(crate) fn setup_of_selection(&self, cx: &Context<'_>) -> Option<SetupRef> {
        let sel = cx.selected?;
        let entry = cx.data.feature(sel)?;
        let setup = if entry.type_id == SETUP {
            sel
        } else if crate::is_operation(cx.registry, &entry.type_id) {
            FeatureId(entry.data.get("setup")?.as_u64()?)
        } else {
            return None;
        };
        self.setup_paths(cx, setup).ok()
    }
}

impl Extension for CamCommands {
    /// 段の帯の段と「再生」は、シミュレーションがまだなら走らせるので、シミュレーションの引数を受け取る。
    fn unlisted(&self, id: &str) -> Option<Vec<kmb_core::Arg>> {
        (id.starts_with(SHOW_STAGE) || id == crate::playback::PLAY).then(sim_args)
    }

    fn settings(&self) -> Vec<Settings> {
        let m = self.machine();
        let mut notes = vec![
            format!("機械: {}", m.name),
            format!("工具番号: {}〜{}", m.tool_numbers.0, m.tool_numbers.1),
        ];
        if !m.collets.is_empty() {
            let c: Vec<String> = m.collets.iter().map(|d| kmb_post::num(*d)).collect();
            notes.push(format!("コレット: {} mm", c.join("、")));
        }
        notes.push(if m.atc_slots == 0 {
            "工具交換: 手で替える(自動交換なし)".into()
        } else {
            format!("工具交換: {} 本の自動交換", m.atc_slots)
        });
        if let Some(b) = &m.bed {
            notes.push(format!(
                "ベッド: {} {}×{}×{} mm、{} の穴 {} 個",
                b.material,
                kmb_post::num(b.size[0]),
                kmb_post::num(b.size[1]),
                kmb_post::num(b.size[2]),
                b.thread,
                b.holes.len()
            ));
            notes.push(format!(
                "作業の原点: 左手前の L 字の角(ベッドの角から {}, {})",
                kmb_post::num(b.origin[0]),
                kmb_post::num(b.origin[1])
            ));
            if let Some(s) = b.sensor {
                notes.push(format!(
                    "工具長センサー: X {}・Y {}(原点から)",
                    kmb_post::num(s[0]),
                    kmb_post::num(s[1])
                ));
            }
            notes.extend(b.notes.iter().cloned());
        }
        if !m.home.is_empty() {
            notes.push(format!("原点復帰: {}", m.home));
        }
        if m.outer.iter().all(|v| *v > 0.0) {
            notes.push(format!(
                "外形: {}×{}×{} mm",
                kmb_post::num(m.outer[0]),
                kmb_post::num(m.outer[1]),
                kmb_post::num(m.outer[2])
            ));
        }
        let plugin = self
            .post_plugin
            .read()
            .expect("ポストの設定の鍵")
            .as_ref()
            .map_or(String::new(), |p| p.display().to_string());
        let mut all = vec![
            Settings {
                id: MACHINE_SETTINGS.into(),
                label: "機械".into(),
                group: "cam".into(),
                notes,
                fields: self.machine_fields(),
                value: machine_value(&m),
                default: machine_value(&self.default_machine),
                role: Some(kmb_interchange::role::settings::MACHINE),
            },
            Settings {
                id: SIM_SETTINGS.into(),
                label: "シミュレーション".into(),
                group: "cam".into(),
                notes: vec![
                    "格子が細かいほど、食い込みと削り残しを細かく調べられるが、時間がかかる".into(),
                    "荒削り(負荷一定・ポケット・フェイス・3D の荒削り)は粗い格子、ほかは細かい格子で削る。加工ごとに編集パネルの「シミュレーションの精度」で変えられる".into(),
                    "動きの単位の再生は、画面が重くならないよう別の粗い格子で削る".into(),
                ],
                fields: vec![
                    Field::choice("cell", "格子の大きさ(仕上げ)", &SIM_CELLS)
                        .help("仕上げなど、荒削りでない加工を削る格子"),
                    Field::choice("rough_cell", "荒削りの格子", &SIM_ROUGH_CELLS).help(
                        "荒削りの加工を削る格子。粗いほど速い。仕上げの加工に移る所で、削った形を細かい格子へ移す",
                    ),
                ],
                value: json!({
                    "cell": kmb_post::num(*self.sim_cell.read().expect("シミュレーションの設定の鍵")),
                    "rough_cell": kmb_post::num(*self.sim_rough_cell.read().expect("シミュレーションの設定の鍵")),
                }),
                default: json!({"cell": "0.1", "rough_cell": kmb_post::num(SIM_ROUGH_CELL)}),
                role: None,
            },
            Settings {
                id: POST_SETTINGS.into(),
                label: "ポスト(G-code の書き方)".into(),
                group: "cam".into(),
                notes: vec![format!(
                    "内蔵: {}",
                    self.post_for(&self.machine())
                        .map_or_else(|e| e, |p| p.id().to_string())
                )],
                fields: vec![Field::file("plugin", "プラグイン(.wasm のファイル)").help(
                    "空なら内蔵のポスト。CLI の --post や MCP の post で指定したものが先に使われる",
                )],
                value: json!({"plugin": plugin}),
                default: json!({"plugin": ""}),
                role: None,
            },
        ];
        all.push(self.reuse.settings());
        all
    }

    fn set_data_dir(&self, dir: Option<&std::path::Path>) {
        self.reuse.set_data_dir(dir);
    }

    fn set_settings(&self, id: &str, value: &serde_json::Value) -> Option<Result<(), String>> {
        if id == POST_SETTINGS {
            return Some(self.set_post_plugin(value));
        }
        if id == crate::reuse::DEFAULTS_SETTINGS {
            return Some(self.reuse.set_defaults(value));
        }
        if id == SIM_SETTINGS {
            let num = |key: &str| {
                let v = value.get(key);
                v.and_then(|v| v.as_f64()).or_else(|| {
                    v.and_then(|v| v.as_str())
                        .and_then(|s| s.trim().parse().ok())
                })
            };
            let cell = match num("cell") {
                Some(c) if (0.01..=2.0).contains(&c) => c,
                _ => return Some(Err("格子の大きさは 0.01〜2 mm にしてください".into())),
            };
            // 前の版の設定(荒削りの格子がない)は、荒削りの格子をそのままにする
            let rough = match (value.get("rough_cell"), num("rough_cell")) {
                (None, _) => None,
                (_, Some(c)) if c == 0.0 || (0.01..=2.0).contains(&c) => Some(c),
                _ => {
                    return Some(Err(
                        "荒削りの格子は 0(仕上げと同じ)か 0.01〜2 mm にしてください".into(),
                    ));
                }
            };
            *self.sim_cell.write().expect("シミュレーションの設定の鍵") = cell;
            if let Some(r) = rough {
                *self
                    .sim_rough_cell
                    .write()
                    .expect("シミュレーションの設定の鍵") = r;
            }
            return Some(Ok(()));
        }
        if id != MACHINE_SETTINGS {
            return None;
        }
        let mut m = self.machine.write().expect("機械の設定の鍵");
        // 機械を替えたら、その機械の初めの値にする。ほかの欄が前の機械の値のまま(画面で機械だけ
        // 替えた)なら使わず、違えば(保存した設定を読んだ時)その機械に当てる
        if let Some(id) = value.get("machine").and_then(|v| v.as_str())
            && id != m.id
        {
            let Some(new) = self.machines.get(id) else {
                return Some(Err(format!("機械「{id}」はマシンライブラリにありません")));
            };
            let same = machine_with(&m, value).is_ok_and(|v| v == *m);
            return Some(if same {
                *m = new;
                Ok(())
            } else {
                machine_with(&new, value).map(|v| *m = v)
            });
        }
        Some(machine_with(&m, value).map(|new| *m = new))
    }

    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let mut v = vec![
            Action::new(ADD_SETUP, "セットアップ", "cam")
                .menu("セットアップ")
                .icon("setup")
                .aliases("せっとあっぷ setup すとっく stock")
                .hint("最後のボディを加工するセットアップを足す")
                .at(&[Place::Toolbar])
                .disabled_if(
                    cx.visible_bodies()
                        .is_empty()
                        .then_some("加工するボディがない"),
                ),
        ];
        for s in self.strategies.iter() {
            let why = why_not(cx, s);
            let mut a = Action::new(s.action, s.label, "cam")
                .menu(s.menu)
                .icon(s.icon)
                .aliases(s.aliases)
                .hint(s.hint)
                // 前に出さない物も ▾ の一覧の物とはっきり書く(下で右クリックを足しても一覧から消えない)
                .at(if s.front {
                    &[Place::Toolbar]
                } else {
                    &[Place::Menu]
                })
                .disabled_if(why);
            // 面を右クリックした時のメニュー(穴の壁なら、その穴の穴あけ)
            let on_face = match s.face_menu {
                FaceMenu::No => false,
                FaceMenu::AnyFace => cx.picked.is_some(),
                FaceMenu::Wall => cx.picked.is_some() && cx.picked_plane().is_none(),
            };
            if on_face && why.is_none() {
                a.places.push(Place::FaceMenu);
            }
            v.push(a);
        }
        if let Some(sel) = cx.selected
            && let Some(t) = cx.selected_type()
            && !self.rest_of(t).is_empty()
            && cx.data.feature(sel).is_some_and(|f| !is_rest_data(&f.data))
        {
            v.push(
                Action::new(REST_POCKET, "取残し加工", "cam")
                    .menu("2D")
                    .icon("rest")
                    .aliases("のこり rest のこりけずり")
                    .hint("この工具が届かなかった隅や細い溝だけを、ライブラリの次に先の細い工具で削る(ポケットと輪郭は V・テーパーも)")
                    .at(&[Place::FeatureMenu])
                    .disabled_if(
                        rest_tool_for(cx, self.rest_of(t), sel)
                            .is_none()
                            .then_some("工具ライブラリに、今の工具より先の細い工具がありません"),
                    ),
            );
        }
        let setup = selected_setup(cx).is_some();
        let here: &[Place] = if setup {
            &[Place::FeatureMenu, Place::Panel]
        } else {
            &[]
        };
        let need = (!setup).then_some("タイムラインでセットアップを選んでから使う");
        v.extend([
            Action::new(EXPORT_GCODE, "G-code を書き出す", "cam")
                .arg(post_arg())
                .role(kmb_interchange::role::action::EXPORT_GCODE)
                .menu("出力")
                .icon("gcodeout")
                .at(&[Place::Toolbar, Place::Timeline])
                .aliases("gcode ncかきだし export")
                .hint(&format!(
                    "{} 向け。文書と同じ場所に書く",
                    self.machine().name
                ))
                .at(here)
                .disabled_if(need),
            Action::new(EXPORT_GCODE_AS, "G-code を名前を付けて書き出す…", "cam")
                .arg(post_arg())
                .menu("出力")
                .icon("gcodeas")
                .aliases("gcode nc かきだし ほぞんさき export save as")
                .hint("書き出す場所と名前を選ぶ")
                // 前には出さず ▾ の一覧に常に置く(Kie 2026-10-21 の
                // 画板)。セットアップを選んでもリボンのボタンがずれない
                .at(&[Place::Menu])
                .at(here)
                .input(kmb_core::Input::SaveFile {
                    title: "G-code を書き出す".into(),
                    filter: "G-code".into(),
                    extensions: vec!["nc".into(), "gcode".into(), "cnc".into()],
                })
                .disabled_if(need),
            Action::new(SETUP_SHEET, "加工指示書を書き出す", "cam")
                .role(kmb_interchange::role::action::SETUP_SHEET)
                .menu("出力")
                .icon("sheet")
                .at(&[Place::Toolbar])
                .aliases("だんどり setup sheet ひょう")
                .hint("ストックの置き方、原点、工具の順を Markdown で文書の隣に書く")
                .at(if setup { &[Place::FeatureMenu] } else { &[] })
                .disabled_if(need),
            Action::new(SIMULATE, "シミュレーション", "cam")
                .args(sim_args())
                .role(kmb_interchange::role::action::SIMULATE)
                .menu("確かめる")
                .icon("simulate")
                .at(&[Place::Toolbar])
                .aliases("しみゅれーしょん simulate simulation")
                .hint("ストックを削ってみて、食い込み・削り残し・シャンクとホルダーの当たりを調べる。下の段の帯で各操作の後の形を見られる")
                .at(here)
                .disabled_if(need),
            // シミュレーションと並べて、書き出す前に確かめる所に置く(前に出すのはシミュレーションだけ。
            // Kie 2026-10-21 の画板)
            Action::new(VIEW_GCODE, "G-code を見る", "cam")
                .arg(post_arg())
                .menu("確かめる")
                .icon("gcodeview")
                .at(&[Place::Menu, Place::Timeline])
                .aliases("gcode nc みる かくにん view preview")
                .hint("書き出す前に中身を行番号つきで確かめる。先頭に時間の目安")
                .at(here)
                .disabled_if(need),
            Action::new(SIMULATE_GCODE, "G-code で確かめる", "cam")
                .args(sim_args())
                .arg(post_arg())
                .menu("確かめる")
                .icon("gcodecheck")
                .at(&[Place::Menu])
                .aliases("じーこーど gcode backplot ばっくぷろっと")
                .hint("書き出す G-code を読み直して削ってみる(機械に送るものそのもの。ポストの誤りも見つかる)")
                // 設定欄のボタンの列に足すと欄が広がり 3D のビューが狭くなるので、右クリックに置く
                .at(if setup { &[Place::FeatureMenu] } else { &[] })
                .disabled_if(need),
        ]);
        let info = self.setup_of_selection(cx).map(|s| s.info.clone());
        v.extend(crate::fixtures::actions(cx, info.as_ref()));
        v.extend(crate::nest::actions(cx));
        v.extend(crate::setup_ops::actions(cx, info.as_ref()));
        v.extend(crate::entry::actions(cx, info.as_ref(), &self.strategies));
        v.extend(crate::picks::actions(cx, &self.strategies));
        v.extend(crate::chains::actions(cx, info.as_ref(), &self.strategies));
        v.extend(crate::tabs::actions(cx, info.as_ref(), &self.strategies));
        v.extend(crate::tool_order::actions(cx, &self.strategies));
        v.extend(self.play_actions(cx));
        // 戦略の crate が持つ操作(穴あけの「穴から加工を作る」など)
        for c in &self.commands {
            v.extend(c.actions(cx));
        }
        v.extend(crate::nc_program::actions(cx));
        v.extend(self.reuse.actions(cx));
        v
    }

    fn stages(&self, cx: &Context<'_>) -> Option<kmb_core::StageBand> {
        self.stage_band(cx)
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<kmb_core::Handle>> {
        let info = self.setup_of_selection(cx).map(|s| s.info.clone());
        let bed = match &info {
            Some(i) => self.machine_for(i).bed,
            None => self.machine().bed,
        };
        crate::fixtures::handles(id, cx, info.as_ref(), bed.as_ref())
            .or_else(||crate::nest::handles(id,cx))
            .or_else(|| crate::setup_ops::handles(id, cx, info.as_ref(), bed.as_ref()))
            .or_else(|| crate::entry::handles(id, cx, info.as_ref(), &self.strategies))
            .or_else(|| crate::chains::handles(id, cx, info.as_ref(), &self.strategies))
            .or_else(|| crate::tabs::handles(id, cx, info.as_ref(), &self.strategies))
            .or_else(|| self.commands.iter().find_map(|c| c.handles(id, cx)))
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        // 何も選んでいなければ(CLI・MCP で省いた時)、最初のセットアップ
        // セットアップが無い・選んでいない時は、今の文脈では使えない操作
        let unavailable = |m: &str| Problem::new(ProblemKind::Unavailable, m);
        let need_setup = || match cx.selected {
            Some(_) => {
                selected_setup(cx).ok_or_else(|| unavailable("セットアップを選んでいません"))
            }
            None => cx
                .data
                .features
                .iter()
                .find(|f| f.type_id == SETUP)
                .map(|f| f.id)
                .ok_or_else(|| unavailable("セットアップがありません")),
        };
        if let Some(r) = crate::picks::run(id, cx, &self.strategies) {
            return Some(r);
        }
        if let Some(r)=crate::nest::run(id,cx,&self.machine()) { return Some(r); }
        if let Some(r) = crate::tool_order::run(id, cx, &self.strategies) {
            return Some(r);
        }
        if let Some(r) = self.reuse.run(id, cx) {
            return Some(r);
        }
        if let Some(r) = self.commands.iter().find_map(|c| c.run(id, cx)) {
            return Some(r);
        }
        if crate::setup_ops::IDS.contains(&id) {
            let info = self.setup_of_selection(cx).map(|s| s.info.clone());
            return crate::setup_ops::run(id, cx, info.as_ref());
        }
        if let Some(r) = crate::chains::run(id, cx, &self.strategies) {
            return Some(r);
        }
        if let Some(r) = crate::tabs::run(id, cx, &self.strategies) {
            return Some(r);
        }
        if let Some(r) = self.run_nc_program(id, cx) {
            return Some(r);
        }
        if [
            crate::ADD_FIXTURE,
            crate::CLEAR_FIXTURES,
            crate::PLACE_ENTRY,
            crate::AUTO_ENTRY,
        ]
        .contains(&id)
        {
            let info = self.setup_of_selection(cx).map(|s| s.info.clone());
            return crate::fixtures::run(id, cx, info.as_ref())
                .or_else(|| crate::entry::run(id, cx, info.as_ref(), &self.strategies));
        }
        Some(match id {
            ADD_SETUP => add_setup(cx, &self.machine()),
            REST_POCKET => cx
                .selected
                .ok_or_else(|| "加工を選んでいません".into())
                .and_then(|s| {
                    let rest = cx
                        .data
                        .feature(s)
                        .map_or(&[][..], |f| self.rest_of(&f.type_id));
                    add_rest_pocket(cx, rest, s)
                }),
            EXPORT_GCODE => need_setup().and_then(|s| self.gcode(cx, s)),
            SETUP_SHEET => need_setup().and_then(|s| self.sheet(cx, s)),
            EXPORT_GCODE_AS => need_setup().and_then(|s| self.gcode(cx, s)),
            VIEW_GCODE => need_setup().and_then(|s| {
                let paths = self.setup_paths(cx, s)?;
                let name=paths.name.clone();
                let preview=if paths.info.sections.is_some() {
                    let (body,_) = self.gcode_text(cx,&paths)?;
                    Outcome::Text { title:format!("G-code: {name}"),body }
                } else { self.gcode(cx,s)? };
                match preview {
                    Outcome::File { contents, .. } => {
                        let body = String::from_utf8_lossy(&contents).into_owned();
                        Ok(Outcome::Text {
                            title: format!("G-code: {name}({} 行)", body.lines().count()),
                            body,
                        })
                    }
                    other => Ok(other),
                }
            }),
            SIMULATE => need_setup().and_then(|s| self.simulate(cx, s, SimSource::Paths, None)),
            SIMULATE_GCODE => {
                need_setup().and_then(|s| self.simulate(cx, s, SimSource::Gcode, None))
            }
            CLEAR_STAGE => Ok(self.clear_stage()),
            _ if id.starts_with("cam.play.")
                || id == crate::playback::EXPORT_STOCK
                || id == crate::playback::EXPORT_SIM_VIDEO =>
            {
                self.play_run(cx, id)
            }
            _ if id.starts_with(SHOW_STAGE) => self.show_stage(cx, id),
            // 足す加工は、利用者が保存した既定があればその値で始める
            _ => {
                let s = self.strategies.iter().find(|s| s.action == id)?;
                add_operation(cx, s).map(|o| self.reuse.with_defaults(cx, o))
            }
        })
    }

    /// 選んでいる機能の経路(セットアップなら全部、加工ならその経路)と、
    /// そのセットアップのストック・原点・作業範囲。
    fn overlay(&self, cx: &Context<'_>) -> Overlay {
        let mut o = Overlay {
            shape: self.shown_stage(cx),
            ..Overlay::default()
        };
        // 再生している時は、その時刻の形と工具、今の操作の経路を出す(選びに依らない)
        let play = self
            .band_setup(cx)
            .and_then(|s| Some((self.play_overlay(&s)?, s)));
        if let Some(((shape, lines), s)) = play {
            o.shape = Some(shape);
            o.lines = lines;
            frame_lines(&self.machine_for(&s.info), &s.info, &mut o);
            return o;
        }
        let Some(s) = self.setup_of_selection(cx) else {
            return o;
        };
        let Some(entry) = cx.selected.and_then(|id| cx.data.feature(id)) else {
            return o;
        };
        // 途中の形を出している時は、その形を作った操作の経路だけを重ねる(他の経路で形が隠れるため)
        let stage = o.shape.as_ref().and_then(|_| {
            self.sim
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .and_then(|m| m.selected)
        });
        for (_, tp) in s.paths.iter().enumerate().filter(|(k, _)| match stage {
            Some(n) if entry.type_id == SETUP => *k + 1 == n,
            _ => entry.type_id == SETUP || s.sources.get(*k).map(|p| p.id) == Some(entry.id),
        }) {
            lines_of(tp, &s.info, &mut o.lines);
        }
        // 加工を選んでいる時は、その加工の高さを色の面で出す
        if entry.type_id != SETUP
            && stage.is_none()
            && let Some(FeatureStatus::Ok { outputs, .. }) = cx.report.feature(entry.id)
            && let Some(h) = outputs.get("heights").and_then(kmb_cam_setup::heights_of)
        {
            height_planes(&s.info, &h, &mut o.planes);
        }
        let i = &s.info;
        if o.shape.is_none() && i.sections.is_some()
            && let Ok(mesh)=i.part_mesh(cx.kernel.as_ref()) {
            o.shape=Some(Arc::new(kmb_core::OverlayShape { key:crate::nest::overlay_key(i,s.print()),label:"板への配置".into(),mesh,..Default::default() }));
        }
        // 最後のシミュレーションの食い込みと削り残し(経路が変わっていなければ)。
        // 印は最後の形のものなので、途中の形を出している時は出さない
        if let Some(m) = self.sim.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
            && stage.is_none()
            && m.setup == s.setup
            && m.print == s.print()
        {
            o.marks
                .extend(m.marks.iter().map(|(p, k, a)| (i.to_world(*p), *k, *a)));
        }
        frame_lines(&self.machine_for(&s.info), &s.info, &mut o);
        o
    }

    /// セットアップなら、ストックの大きさと機械の検査(作業範囲、回転数、コレット)の結果。
    fn notes(&self, cx: &Context<'_>, id: FeatureId) -> Vec<String> {
        let mut out = Vec::new();
        if cx.data.feature(id).is_none_or(|f| f.type_id != SETUP) {
            return out;
        }
        out.extend(later_design(cx, id));
        out.extend(crate::nest::unplaced_notes(cx,id));
        let Ok(s) = self.setup_paths(cx, id) else {
            return out;
        };
        let m = &self.machine_for(&s.info);
        let i = &s.info;
        for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
            let size = i.sections.as_ref().map_or(i.stock_max[k]-i.stock_min[k],|s|s.list.iter().map(|s|s.stock_max[k]-s.stock_min[k]).fold(0.0,f64::max));
            if size > m.travel[k] {
                out.push(format!(
                    "ストックの {axis} の大きさ {size:.1} mm が {} の作業範囲 {} mm を超えています",
                    m.name, m.travel[k]
                ));
            }
        }
        if !s.paths.is_empty() {
            // セットアップの注意にもう出ている文(作業座標系・高さの補正)は並べない
            let shown = match cx.report.feature(id) {
                Some(FeatureStatus::Ok { outputs, .. }) => kmb_core::warnings_of(outputs),
                _ => Vec::new(),
            };
            let checks = match self.post_for(m) {
                Ok(post) => match &s.info.sections {
                    Some(sections)=>sections.list.iter().flat_map(|sec|post.check(&s.section(sec.index).program(),m)).collect(),
                    None=>post.check(&s.program(),m),
                },
                Err(e) => vec![kmb_post::Diagnostic {
                    severity: kmb_post::Severity::Error,
                    message: e,
                }],
            };
            for d in checks {
                if !shown.contains(&d.message) {
                    out.push(d.message);
                }
            }
        }
        out
    }

    /// セットアップの一覧(加工の名前と、経路を作れなかった加工)。
    fn status(&self, cx: &Context<'_>) -> Vec<(String, serde_json::Value)> {
        let setups: Vec<serde_json::Value> = self
            .paths
            .all(cx)
            .0
            .iter()
            .map(|s| {
                let mut o = json!({
                    "id": s.setup.0,
                    "name": s.name,
                    "operations": s.paths.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
                    "failed": s.failed.iter().map(|(id, m)| json!({"id": id.0, "reason": m})).collect::<Vec<_>>(),
                });
                if let Some(w) = later_design(cx, s.setup) {
                    o["warnings"] = json!([w]);
                }
                o
            })
            .collect();
        vec![("setups".into(), json!(setups))]
    }

    /// 加工の機能の経路の要約。セットアップなら、加工の数と全体の時間の目安。
    fn summary(&self, cx: &Context<'_>, id: FeatureId) -> Option<String> {
        let entry = cx.data.feature(id)?;
        if entry.type_id == SETUP {
            let s = self.paths.get(cx, id)?;
            let program = s.program();
            let changes = program
                .ops
                .iter()
                .filter(|op| matches!(op, kmb_toolpath::Op::ToolChange { .. }))
                .count();
            let i = &s.info;
            let size = |lo: [f64; 3], hi: [f64; 3]| {
                format!(
                    "{:.1} × {:.1} × {:.1} mm",
                    hi[0] - lo[0],
                    hi[1] - lo[1],
                    hi[2] - lo[2]
                )
            };
            let origin = serde_json::from_value::<Setup>(entry.data.clone())
                .ok()
                .and_then(|d| d.origin_at)
                .map(|o| format!("、原点は{}", o.describe()))
                .unwrap_or_default();
            return Some(format!(
                "ストック {}(部品 {}){origin}。加工 {} 個、工具交換 {changes} 回、時間の目安 {}(工具交換と工具長の測定は含まない)",
                size(i.stock_min, i.stock_max),
                size(i.part_min, i.part_max),
                s.paths.len(),
                kmb_cam_program::sheet::duration(program.minutes(&self.machine_for(i).motion()))
            ));
        }
        if !crate::is_operation(cx.registry, &entry.type_id)
            || !matches!(cx.report.feature(id), Some(FeatureStatus::Ok { .. }))
        {
            return None;
        }
        let order = (crate::tool_order::chain(cx, &self.strategies, id).len() > 1)
            .then(|| crate::tool_order::describe(cx, &self.strategies, id))
            .flatten();
        let path = self.paths.all(cx).0.iter().find_map(|s| {
                // 時間の目安はセットアップが写して持つ機械の値で(経路・ポストと同じ)
                let motion = self.machine_for(&s.info).motion();
                s.sources.iter().position(|o| o.id == id).map(|k| &s.paths[k]).map(|p| {
                    let (cut, air) = p.lengths();
                    // 横の送り(一番速い切削の送り)からチップロードを出す
                    let feed = p
                        .moves
                        .iter()
                        .filter_map(|m| match m {
                            kmb_toolpath::Move::Linear { feed, .. }
                            | kmb_toolpath::Move::Arc { feed, .. } => Some(*feed),
                            _ => None,
                        })
                        .fold(0.0f64, f64::max);
                    let chip = feed / (p.rpm * p.tool.flutes.max(1) as f64);
                    // 線の色の見方(入り・リード・つなぎの印がある経路だけ)
                    let legend = if p.spans.is_empty() {
                        String::new()
                    } else {
                        "\n線の色: 緑の実線=切削、赤紫=入り(らせん・斜め・真下)、黄土=リード、灰の細かい破線=つなぎ、赤の破線=早送り"
                            .to_string()
                    };
                    if let Some(power) = p.laser_power() {
                        // レーザーは回転数の欄が出力。長さは出力 0 の送り(つなぎ)も含む
                        return format!(
                            "工具 T{} {}、出力 {:.0} %、速さ {feed:.0} mm/min、焼く・動く長さ {:.0} mm、早送り {:.0} mm、時間の目安 {:.1} 分{legend}",
                            p.tool.number,
                            p.tool.name,
                            power * 100.0,
                            cut,
                            air,
                            p.minutes(&motion),
                        );
                    }
                    format!(
                        "工具 T{} {}、{} rpm、送り {feed:.0} mm/min(チップロード {chip:.4} mm)、切削 {:.0} mm、早送り {:.0} mm、時間の目安 {:.1} 分\n{}{legend}",
                        p.tool.number,
                        p.tool.name,
                        p.rpm,
                        cut,
                        air,
                        p.minutes(&motion),
                        kmb_cam_program::sheet::link_summary(p, &motion)
                    )
                })
            });
        match (path, order) {
            (Some(p), Some(o)) => Some(format!("{p}\n{o}")),
            (p, o) => p.or(o),
        }
    }
}

/// 加工の高さ `h` を、ストックの上から見た大きさの平面にする(世界座標)。
pub(crate) fn height_planes(
    info: &crate::SetupInfo,
    h: &kmb_cam_setup::HeightsOutput,
    out: &mut Vec<kmb_core::OverlayPlane>,
) {
    use kmb_core::PlaneInk;
    let (lo, hi) = (info.stock_min, info.stock_max);
    let mut add = |z: Option<f64>, ink: PlaneInk, name: &str| {
        let Some(z) = z else {
            return;
        };
        let corners = [
            [lo[0], lo[1], z],
            [hi[0], lo[1], z],
            [hi[0], hi[1], z],
            [lo[0], hi[1], z],
        ]
        .map(|p| info.to_world(p));
        out.push(kmb_core::OverlayPlane {
            corners,
            ink,
            label: format!("{name} Z{z:.2}"),
        });
    };
    add(Some(h.safe), kmb_interchange::ink::plane::SAFE, "安全高さ");
    add(Some(h.feed), kmb_interchange::ink::plane::FEED, "送り高さ");
    add(h.top, kmb_interchange::ink::plane::TOP, "トップ高さ");
    add(h.bottom, kmb_interchange::ink::plane::BOTTOM, "ボトム高さ");
}

/// 重ねる線の種類(描き方の役割と凡例の名前)。
pub(crate) const CUT: LineKind = LineKind::new(kmb_interchange::ink::line::MAIN, "切削");
pub(crate) const RAPID: LineKind = LineKind::new(kmb_interchange::ink::line::TRAVEL, "早送り");
pub(crate) const RETRACT: LineKind =
    LineKind::new(kmb_interchange::ink::line::TRAVEL_LOUD, "完全退避");
pub(crate) const ENTRY: LineKind = LineKind::new(kmb_interchange::ink::line::APPROACH, "進入");
pub(crate) const LEAD: LineKind = LineKind::new(kmb_interchange::ink::line::EASE, "リード");
pub(crate) const LINK: LineKind = LineKind::new(kmb_interchange::ink::line::CONNECT, "リンク");
/// 再生で、これから削る経路
pub(crate) const AHEAD: LineKind = LineKind::new(kmb_interchange::ink::line::FAINT, "これから");
pub(crate) const MACHINE: LineKind = LineKind::new(kmb_interchange::ink::line::SURROUND, "機械");

/// ストックの箱・原点・作業範囲・ベッド・固定具を重ねる。
fn frame_lines(machine: &MachineProfile, i: &crate::SetupInfo, o: &mut Overlay) {
    // 裏返したセットアップは、部品の向きに戻して重ねる
    let box_of = |a: [f64; 3], b: [f64; 3]| {
        let (p, q) = (i.to_world(a), i.to_world(b));
        (
            [0, 1, 2].map(|k| p[k].min(q[k])),
            [0, 1, 2].map(|k| p[k].max(q[k])),
        )
    };
    let stocks:Vec<_>=i.sections.as_ref().map_or_else(||vec![(i.stock_min,i.stock_max)],|s|s.list.iter().map(|s|(s.stock_min,s.stock_max)).collect());
    for (min,max) in stocks {
        let (lo,hi)=box_of(min,max);
        o.boxes.push((lo,hi,BoxKind::new(kmb_interchange::ink::boxes::WORK,"ストック")));
    }
    o.points.push(i.to_world([0.0; 3]));
    // 置き方の回転の逆で、作業座標系の軸 k は世界座標で行列の k 行目の向き
    o.axes = Some(i.matrix());
    // 機械の作業範囲(主軸の中心が動ける X・Y)。機械の形があればその置き場所、無ければ L 字の
    // 基準の角(ストックの手前左の下の角から、ベッドの上の置き場所だけ戻した所)を、作業範囲の
    // 手前左の下の角に当てて置いた場合
    let t = machine.travel;
    let m = crate::setup_ops::bed_corner(i);
    let lo = crate::machine_shape(i).map_or([m[0], m[1]], |s| s.travel.0);
    let (elo, ehi) = box_of(
        [lo[0], lo[1], m[2]],
        [lo[0] + t[0], lo[1] + t[1], m[2] + t[2]],
    );
    o.boxes.push((
        elo,
        ehi,
        BoxKind::new(kmb_interchange::ink::boxes::LIMIT, "作業範囲"),
    ));
    // ベッドとネジ穴
    if let Some(bed) = &machine.bed {
        for l in bed_lines(bed, m) {
            o.lines
                .push((l.iter().map(|p| i.to_world(*p)).collect(), MACHINE));
        }
    }
    crate::fixtures::lines(i, &mut o.lines);
}

/// 経路を、動きの種類(早送り・切削・入り・リード・つなぎ)で分けた折れ線(世界座標)にする。
/// 入りとリードとつなぎは加工が付けた印で分け、印のない送りでもストックの上面より上だけを動くものは
/// つなぎにする。
pub(crate) fn lines_of(
    tp: &crate::Toolpath,
    info: &crate::SetupInfo,
    out: &mut Vec<(Vec<[f64; 3]>, LineKind)>,
) {
    lines_of_range(tp, info, 0..tp.moves.len(), out);
}

/// [`lines_of`] の、動き `range` だけの版(最初の動きの行き先から描き始める)。動きの番号は経路の
/// 中の番号のまま見るので、途中から描いても印やつなぎの種類はずれない。
pub(crate) fn lines_of_range(
    tp: &crate::Toolpath,
    info: &crate::SetupInfo,
    range: std::ops::Range<usize>,
    out: &mut Vec<(Vec<[f64; 3]>, LineKind)>,
) {
    let end = range.end.min(tp.moves.len());
    if range.start >= end {
        return;
    }
    let w = |p: [f64; 3]| info.to_world(p);
    let roles = tp.roles();
    let mut cur = tp.moves[range.start].to();
    let mut line = vec![w(cur)];
    let mut kind = line_kind(tp, &roles, info, range.start, cur);
    for k in range.start + 1..end {
        let m = &tp.moves[k];
        let kk = line_kind(tp, &roles, info, k, cur);
        if kk != kind {
            if line.len() > 1 {
                out.push((std::mem::take(&mut line), kind));
            }
            line = vec![w(cur)];
            kind = kk;
        }
        for q in m.points(cur, 0.5) {
            line.push(w(q));
        }
        cur = m.to();
    }
    if line.len() > 1 {
        out.push((line, kind));
    }
}

/// 経路の `k` 番目の動き(`from` から)の線の種類。`roles` は [`crate::Toolpath::roles`]。
/// 早送りは、全退避のつなぎの中なら目立たせる。
pub(crate) fn line_kind(
    tp: &crate::Toolpath,
    roles: &[Option<kmb_toolpath::Role>],
    info: &crate::SetupInfo,
    k: usize,
    from: [f64; 3],
) -> LineKind {
    let m = &tp.moves[k];
    if matches!(m, kmb_toolpath::Move::Rapid { .. }) {
        return match tp.links.link_at(k) {
            Some(l) if l.kind == kmb_toolpath::LinkKind::Full => RETRACT,
            _ => RAPID,
        };
    }
    let top = info.stock_max[2] + 1e-6;
    match roles.get(k).copied().flatten() {
        Some(kmb_toolpath::Role::Entry) => ENTRY,
        Some(kmb_toolpath::Role::Lead) => LEAD,
        Some(kmb_toolpath::Role::Link) => LINK,
        None if from[2] >= top && m.to()[2] >= top => LINK,
        None => CUT,
    }
}

/// セットアップのボディを、後ろに足した設計の機能が受け継いでいれば、その知らせ。セットアップは
/// ボディを「作った時の最後の機能」の番号で持つので、後から末尾に足した機能(フィレットなど)の
/// 形は削らない(ボディの長く使える ID ができるまでは知らせるだけ)。
fn later_design(cx: &Context<'_>, setup: FeatureId) -> Option<String> {
    let body = cx
        .data
        .feature(setup)?
        .data
        .get("body")?
        .as_u64()
        .map(FeatureId)?;
    let heir = kmb_core::body_heir(cx.data, cx.registry, cx.report, body);
    if heir == body {
        return None;
    }
    let name = |id: FeatureId| {
        cx.data
            .feature(id)
            .map_or_else(|| id.to_string(), |f| f.name.clone())
    };
    Some(format!(
        "部品に後から足した機能「{}」が加工に入っていません(このセットアップは「{}」の時点の形を削ります。入れるには、セットアップのボディを選び直す)",
        name(heir),
        name(body)
    ))
}

/// G-code を作る操作の引数 `post`(ポストのプラグインのファイル)。
pub(crate) fn post_arg() -> kmb_core::Arg {
    kmb_core::Arg::text(
        "post",
        "ポストのプラグイン(WASM のコンポーネントのファイル)。省くと設定で選んだポストか機械の内蔵のポスト",
    )
}

/// シミュレーションの引数(省くと設定の格子)。
fn sim_args() -> Vec<kmb_core::Arg> {
    vec![
        kmb_core::Arg::length(
            "cell",
            "格子の大きさ(0.01〜2 mm)。渡すと荒削りの格子も使わずこの格子だけで削る。省くと設定の値",
        ),
        kmb_core::Arg::length(
            "rough_cell",
            "荒削りの格子の大きさ(0 は細かい格子と同じ、または 0.01〜2 mm)。省くと設定の値",
        ),
    ]
}
