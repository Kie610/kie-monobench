//! 戦略の純関数の入口。
//!
//! 加工の機能の `evaluate`(ホスト)が、式・参照・カーネルの網を解いて [`StrategyInput`] を作り、
//! 戦略の `plan` は文書とカーネルに触らずに、それだけから経路を作って [`StrategyOutput`] を返す。
//! ホストは返った経路に、円弧のあてはめ・固定具の回避・つなぎの置き換えを当てて出力にする
//! ([`Prepared::finish_plan`])。どちらも JSON で往復できるので、組み込みの戦略と WASM の戦略が
//! 同じ形で受け取れる。数はすべて解いた後の値(長さは mm、角度は式の値のまま)。
//!
//! どの戦略も `plan` を持ち、`evaluate` は式・参照・網・工具ライブラリを解いて `plan` を呼ぶだけに
//! する(xtask の層の点検が、戦略の crate の中で経路を作る所とカーネルに触る所を数える)。

use crate::{HeightFrom, OpHeights, Prepared, SetupInfo, invalid, split_levels};
use kmb_cam_geom::{Area, MachiningModel};
use kmb_cam_tool::{Cutting, Tool};
use kmb_core::{EvalError, FeatureId, Outputs};
use kmb_toolpath::Toolpath;
use serde::{Deserialize, Serialize};
use std::cell::Cell;

/// セットアップの値のうち、戦略が使うもの(カーネルのハンドルを除く。作業座標系の mm)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetupView {
    pub stock_min: [f64; 3],
    pub stock_max: [f64; 3],
    pub part_min: [f64; 3],
    pub part_max: [f64; 3],
    /// 安全高さ・送り高さ(加工の欄で直した後)
    pub safe_z: f64,
    pub retract_z: f64,
    pub material: String,
    /// 経路が避ける範囲の箱(固定具の外形に余白を足したもの)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keep_out: Vec<([f64; 3], [f64; 3])>,
    /// 固定具の外形の箱
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixtures: Vec<([f64; 3], [f64; 3])>,
    /// 戦略が使う機械の値
    #[serde(default)]
    pub machine: MachineView,
}

/// 機械の値のうち、戦略が使うもの(ホストが機械の値から決める。戦略は機械の表を読まない)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineView {
    /// 加速度(mm/s²。0 なら見込まない)
    #[serde(default)]
    pub acceleration: f64,
    /// 鏡で光を振るレーザー(はみ出しが要らない)
    #[serde(default)]
    pub galvo: bool,
    /// 機械の条件の表の、この材料とこの工具の径の溝の 1 段の深さ(mm)。表に無ければ `None`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_stepdown: Option<f64>,
}

/// 機械の切削条件の表の、溝で削る時の 1 段の深さ(mm)。
/// 材料の名前は別名(aluminum・mdf など)も同じとみなす。表のない機械や材料なら `None`。
pub fn machine_stepdown(
    machine: &kmb_post::MachineProfile,
    material: &str,
    diameter: f64,
) -> Option<f64> {
    let name = kmb_cam_tool::material(material).map_or(material.trim(), |m| m.name);
    machine.slot_stepdown(name, diameter)
}

impl SetupView {
    pub fn of(info: &SetupInfo, tool: &Tool) -> Self {
        let m = info.machine_profile();
        SetupView {
            stock_min: info.stock_min,
            stock_max: info.stock_max,
            part_min: info.part_min,
            part_max: info.part_max,
            safe_z: info.safe_z,
            retract_z: info.retract_z,
            material: info.material.clone(),
            keep_out: info.keep_out.clone(),
            fixtures: info.fixtures.clone(),
            machine: MachineView {
                acceleration: m.acceleration,
                galvo: m
                    .laser
                    .as_ref()
                    .is_some_and(|l| l.drive == kmb_post::LaserDrive::Galvo),
                slot_stepdown: machine_stepdown(m, &info.material, tool.diameter),
            },
        }
    }
}

/// 加工ごとのトップ・ボトムの決め方(基準とオフセット)と、基準の高さ
/// (ストックの上面・底面、部品の上面・底面)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeightRule {
    pub top: (HeightFrom, f64),
    pub bottom: (HeightFrom, f64),
    pub refs: [f64; 4],
}

impl HeightRule {
    pub(crate) fn of(h: &OpHeights) -> Self {
        HeightRule {
            top: h.top,
            bottom: h.bottom,
            refs: h.refs,
        }
    }

    fn resolve(&self, (from, offset): (HeightFrom, f64), auto: f64) -> f64 {
        OpHeights {
            refs: self.refs,
            ..Default::default()
        }
        .resolve((from, offset), auto)
    }
}

/// ホストが解いた後の入力の外の形(曲線・輪郭など、ほかの機能から読んだもの)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sources {
    /// 閉じた輪郭の領域(作業座標系。スケッチの輪郭など)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub areas: Vec<Area>,
}

/// 戦略の純関数に渡す一式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyInput {
    /// 経路の名前(機能の ID)
    pub name: String,
    /// 戦略ごとの解いた値(各戦略の型を JSON にしたもの)
    pub params: serde_json::Value,
    pub setup: SetupView,
    pub tool: Tool,
    pub cutting: Cutting,
    /// 最後の段の厚さ(mm)。None なら段を等しく分ける
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_step: Option<f64>,
    pub heights: HeightRule,
    /// 加工に使う形(作業座標系の三角形・平らな面・円筒面)
    pub model: MachiningModel,
    #[serde(default)]
    pub sources: Sources,
}

/// 戦略の純関数が返すもの。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyOutput {
    pub toolpath: Toolpath,
    /// 利用者に知らせること(ホストの警告の後ろに足す)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// 使ったトップ高さ・ボトム高さ(画面に色の面で出す)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
    /// 入る位置を置ける所(画面に出す)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_area: Option<EntryRegion>,
    /// 戦略が直した切削条件(1 段の深さを機械の表から決めた時など)。出力の条件に使う
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutting: Option<Cutting>,
    /// 戦略だけが出す出力(タブの付く経路など)。名前ごとにそのまま出力に足す
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub extra: kmb_core::Outputs,
}

/// 入る位置を置ける所: 高さ `z` の領域。`helix` ならそこはらせんで入れる所。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryRegion {
    pub z: f64,
    pub areas: Vec<Area>,
    pub helix: bool,
}

impl StrategyInput {
    /// 戦略ごとの値を読む。
    pub fn params<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        serde_json::from_value(self.params.clone())
            .map_err(|e| format!("加工の値が読めません: {e}"))
    }

    pub fn r(&self) -> f64 {
        self.tool.radius()
    }

    /// トップ高さ(`auto` は加工が決める高さ)。[`Prepared::top`] と同じ決め方。
    pub fn top(&self, auto: f64) -> f64 {
        self.heights.resolve(self.heights.top, auto)
    }

    /// ボトム高さ(`auto` は加工が決める底)。
    pub fn bottom(&self, auto: f64) -> f64 {
        self.heights.resolve(self.heights.bottom, auto)
    }

    /// 上から `top` まで、`bottom` まで下げる高さの列。[`Prepared::levels`] と同じ。
    pub fn levels(&self, top: f64, bottom: f64) -> Vec<f64> {
        split_levels(top, bottom, self.cutting.stepdown, self.last_step)
    }

    /// 刃長より深く切り込まないか。[`Prepared::check_depth`] と同じ文。
    pub fn check_depth(&self, top: f64, bottom: f64) -> Result<(), String> {
        depth_problem(&self.tool, top, bottom).map_or(Ok(()), Err)
    }

    /// トップがボトムより上か。[`Prepared::check_range`] と同じ文。
    pub fn check_range(&self, top: f64, bottom: f64) -> Result<(), String> {
        range_problem(top, bottom).map_or(Ok(()), Err)
    }

    /// 刃長で届く底。届かなければ知らせを `warnings` に足す。[`Prepared::reachable_bottom`] と同じ。
    pub fn reachable_bottom(&self, top: f64, bottom: f64, warnings: &mut Vec<String>) -> f64 {
        reachable_bottom(&self.tool, top, bottom, warnings)
    }

    /// 経路の入れ物。[`Prepared::toolpath`] と同じ。
    pub fn toolpath(&self) -> Toolpath {
        empty_toolpath(
            self.name.clone(),
            &self.tool,
            &self.cutting,
            [self.setup.safe_z, self.setup.retract_z],
        )
    }
}

/// 戦略の `plan` の中の作業場: 入力から作り、経路を作り終えたら [`Planner::output`] で返す。
/// [`Prepared`] と同じ名前の欄と働き(`r`・`top`・`bottom`・`levels`・`check_depth` など)を
/// 持つので、戦略の計算は `Prepared` から移しても書き方が変わらない。文書とカーネルには触れない。
pub struct Planner {
    pub name: String,
    pub info: SetupView,
    pub tool: Tool,
    pub cutting: Cutting,
    pub model: MachiningModel,
    pub warnings: Vec<String>,
    pub last_step: Option<f64>,
    heights: HeightRule,
    input_cutting: Cutting,
    used_top: Cell<Option<f64>>,
    used_bottom: Cell<Option<f64>>,
}

impl Planner {
    pub fn new(input: &StrategyInput) -> Self {
        Planner {
            name: input.name.clone(),
            info: input.setup.clone(),
            tool: input.tool.clone(),
            cutting: input.cutting,
            model: input.model.clone(),
            warnings: Vec::new(),
            last_step: input.last_step,
            heights: input.heights.clone(),
            input_cutting: input.cutting,
            used_top: Cell::new(None),
            used_bottom: Cell::new(None),
        }
    }

    pub fn r(&self) -> f64 {
        self.tool.radius()
    }

    /// トップ高さ。[`Prepared::top`] と同じ(使った高さを覚える)。
    pub fn top(&self, auto: f64) -> f64 {
        let z = self.heights.resolve(self.heights.top, auto);
        keep_top(&self.used_top, z);
        z
    }

    /// ボトム高さ。[`Prepared::bottom`] と同じ。
    pub fn bottom(&self, auto: f64) -> f64 {
        let z = self.heights.resolve(self.heights.bottom, auto);
        keep_bottom(&self.used_bottom, z);
        z
    }

    /// [`Prepared::check_range`] と同じ。
    pub fn check_range(&self, top: f64, bottom: f64) -> Result<(), EvalError> {
        range_problem(top, bottom).map_or(Ok(()), |m| Err(invalid(m)))
    }

    /// [`Prepared::levels`] と同じ。
    pub fn levels(&self, top: f64, bottom: f64) -> Vec<f64> {
        split_levels(top, bottom, self.cutting.stepdown, self.last_step)
    }

    /// [`Prepared::reachable_bottom`] と同じ。
    pub fn reachable_bottom(&mut self, top: f64, bottom: f64) -> f64 {
        reachable_bottom(&self.tool, top, bottom, &mut self.warnings)
    }

    /// [`Prepared::check_depth`] と同じ。
    pub fn check_depth(&self, top: f64, bottom: f64) -> Result<(), EvalError> {
        depth_problem(&self.tool, top, bottom).map_or(Ok(()), |m| Err(invalid(m)))
    }

    /// 経路の入れ物。[`Prepared::toolpath`] と同じ。
    pub fn toolpath(&self) -> Toolpath {
        empty_toolpath(
            self.name.clone(),
            &self.tool,
            &self.cutting,
            [self.info.safe_z, self.info.retract_z],
        )
    }

    /// 作った経路と、知らせ・使った高さを返す。
    pub fn output(self, toolpath: Toolpath) -> StrategyOutput {
        let cutting = (self.cutting != self.input_cutting).then_some(self.cutting);
        StrategyOutput {
            toolpath,
            warnings: self.warnings,
            top: self.used_top.get(),
            bottom: self.used_bottom.get(),
            entry_area: None,
            cutting,
            extra: Default::default(),
        }
    }
}

impl AsRef<MachiningModel> for Planner {
    fn as_ref(&self) -> &MachiningModel {
        &self.model
    }
}

impl AsRef<MachiningModel> for Prepared {
    fn as_ref(&self) -> &MachiningModel {
        &self.model
    }
}

impl kmb_toolpath_moves::Cutter for Planner {
    fn radius(&self) -> f64 {
        self.r()
    }

    fn feed(&self) -> f64 {
        self.cutting.feed
    }

    fn plunge(&self) -> f64 {
        self.cutting.plunge
    }
}

/// `plan` の誤りを返す文にする(データの誤りは文だけ。ホストが同じ種類の誤りに戻す)。
pub fn plan_error(e: EvalError) -> String {
    match e {
        EvalError::InvalidData(m) => m,
        e => e.to_string(),
    }
}

impl Prepared {
    /// 戦略の純関数に渡す一式を作る。`name` は経路の名前(機能の ID)。
    pub fn input(
        &self,
        name: FeatureId,
        params: impl Serialize,
        sources: Sources,
    ) -> StrategyInput {
        StrategyInput {
            name: name.to_string(),
            params: serde_json::to_value(params).expect("加工の値は JSON にできる"),
            setup: SetupView::of(&self.info, &self.tool),
            tool: self.tool.clone(),
            cutting: self.cutting,
            last_step: self.last_step,
            heights: HeightRule::of(&self.heights),
            model: self.model.clone(),
            sources,
        }
    }

    /// 値を持たない入力の作業場(戦略の単体試験で、計算の関数を直に呼ぶ時に使う)。
    pub fn planner(&self) -> Planner {
        Planner::new(&self.input(FeatureId(0), serde_json::Value::Null, Sources::default()))
    }

    /// 戦略の純関数の結果を出力にする(警告と使った高さを足してから [`Prepared::finish`])。
    pub fn finish_plan(
        &mut self,
        setup: FeatureId,
        out: Result<StrategyOutput, String>,
    ) -> Result<Outputs, EvalError> {
        let out = out.map_err(invalid)?;
        self.warnings.extend(out.warnings);
        if let Some(c) = out.cutting {
            self.cutting = c;
        }
        // 戦略が決めた高さ(解いた後)を、Prepared::top・bottom と同じく使った高さとして覚える
        if let Some(z) = out.top {
            keep_top(&self.heights.used_top, z);
        }
        if let Some(z) = out.bottom {
            keep_bottom(&self.heights.used_bottom, z);
        }
        let mut outputs = self.finish(setup, out.toolpath)?;
        if let Some(e) = out.entry_area {
            kmb_toolpath_moves::add_entry_area(&mut outputs, e.z, &e.areas, e.helix);
        }
        outputs.extend(out.extra);
        Ok(outputs)
    }
}

// ---- Prepared・Planner・StrategyInput が同じ決め方・同じ文で使う部品 ----

/// 使ったトップ高さを覚える(いちばん高い値)。
pub(crate) fn keep_top(used: &Cell<Option<f64>>, z: f64) {
    used.set(Some(used.get().map_or(z, |t| t.max(z))));
}

/// 使ったボトム高さを覚える(いちばん低い値)。
pub(crate) fn keep_bottom(used: &Cell<Option<f64>>, z: f64) {
    used.set(Some(used.get().map_or(z, |b| b.min(z))));
}

/// トップ高さ `top` とボトム高さ `bottom` の組が使えない時の文(ボトムがトップより下にない)。
pub(crate) fn range_problem(top: f64, bottom: f64) -> Option<String> {
    (bottom >= top - 1e-9).then(|| {
        format!(
            "ボトム高さ Z{bottom:.3} がトップ高さ Z{top:.3} より下にありません(高さの欄を確かめてください)"
        )
    })
}

/// `top` から `bottom` までが工具の刃長より深い時の文。
pub(crate) fn depth_problem(tool: &Tool, top: f64, bottom: f64) -> Option<String> {
    let depth = top - bottom;
    (depth > tool.flute_length + 1e-9).then(|| {
        format!(
            "深さ {depth:.2} mm は工具 {} の刃長 {} mm を超えています",
            tool.name, tool.flute_length
        )
    })
}

/// 深さを決めていない加工の底: 刃長で届かなければ刃長までにし、知らせを `warnings` に足す。
pub(crate) fn reachable_bottom(
    tool: &Tool,
    top: f64,
    bottom: f64,
    warnings: &mut Vec<String>,
) -> f64 {
    let lim = top - tool.flute_length;
    if bottom >= lim - 1e-9 {
        return bottom;
    }
    warnings.push(format!(
        "深さを決めていないので、工具 {} の刃長 {} mm までで止めます(その下の {:.2} mm は残ります。深さの欄で変えられます)",
        tool.name,
        tool.flute_length,
        lim - bottom
    ));
    lim
}

/// 動きの無い経路の入れ物。`[safe_z, retract_z]` は安全高さと送り高さ。
pub(crate) fn empty_toolpath(
    name: String,
    tool: &Tool,
    cutting: &Cutting,
    [safe_z, retract_z]: [f64; 2],
) -> Toolpath {
    Toolpath {
        name,
        tool: tool.clone(),
        rpm: cutting.rpm,
        safe_z,
        retract_z,
        edge_break: 0.0,
        dwells: vec![],
        spans: vec![],
        links: Default::default(),
        moves: Vec::new(),
    }
}
