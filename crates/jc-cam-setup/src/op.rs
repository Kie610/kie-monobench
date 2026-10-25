//! 加工の機能に共通の項目(`OpBase`)と、それを解いた材料一式(`Prepared`): 切削条件・高さ・
//! 段の分け方、経路の検査と出力。

use super::*;

/// 加工の機能に共通の項目。切削条件は空なら工具の既定値を使う。
/// 切削条件は式で持つ(パラメータで決められる)。回転数は rpm、送りは mm/min、
/// 切り込みは mm(長さの単位でもよい)、送り幅は工具径に対する比。
/// 古い文書の数(例: `"rpm": 12000`)も読める。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpBase {
    pub setup: FeatureId,
    /// 工具ライブラリの機能
    pub tools: FeatureId,
    /// 工具番号
    pub tool: u32,
    #[serde(
        default,
        deserialize_with = "num_or_expr",
        skip_serializing_if = "Option::is_none"
    )]
    pub rpm: Option<String>,
    #[serde(
        default,
        deserialize_with = "num_or_expr",
        skip_serializing_if = "Option::is_none"
    )]
    pub feed: Option<String>,
    #[serde(
        default,
        deserialize_with = "num_or_expr",
        skip_serializing_if = "Option::is_none"
    )]
    pub plunge: Option<String>,
    #[serde(
        default,
        deserialize_with = "num_or_expr",
        skip_serializing_if = "Option::is_none"
    )]
    pub stepdown: Option<String>,
    #[serde(
        default,
        deserialize_with = "num_or_expr",
        skip_serializing_if = "Option::is_none"
    )]
    pub stepover: Option<String>,
    /// 円弧のあてはめの許容差(例: `0.005 mm`)。指定すると、細かい直線を許容差の中で
    /// 円弧と長い直線にまとめる(`jc-toolpath-fit`)。空ならしない
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<String>,
    /// 最後の段の厚さ(例: `0.2 mm`)。あれば、最後の段だけこの厚さで薄く削る(仕上げの段)。
    /// 空なら全部の段を等しく分ける
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_step: Option<String>,
    /// 削った所の中を下げたまま(切削の送りで)移る、横の長さの上限。空なら
    /// [`jc_toolpath_moves::link::STAY_DOWN`]、0 ならしない
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stay_down: Option<String>,
    /// 下げたまま移る時に、通り道の材料と部品の上へ空ける高さ。空なら [`jc_toolpath_moves::link::LIFT`]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lift: Option<String>,
    /// 全退避の高さの決め方
    #[serde(default, skip_serializing_if = "RetractHeight::is_fit")]
    pub retract: RetractHeight,
    /// シミュレーションでこの加工を削る格子の細かさ(経路は変わらない)
    #[serde(default, skip_serializing_if = "SimGrid::is_auto")]
    pub sim_grid: SimGrid,
    /// 安全高さ(横に早送りする高さ)の基準とオフセット。自動ならセットアップの安全高さ
    #[serde(default, skip_serializing_if = "HeightFrom::is_auto")]
    pub safe_from: HeightFrom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_offset: Option<String>,
    /// 送り高さ(早送りで下りて、切削の送りに替える高さ)。自動ならトップ高さ + 1 mm
    #[serde(default, skip_serializing_if = "HeightFrom::is_auto")]
    pub feed_from: HeightFrom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_offset: Option<String>,
    /// トップ高さ(削り始める高さ)。自動なら加工が決める高さ(ふつうはストックの上面)
    #[serde(default, skip_serializing_if = "HeightFrom::is_auto")]
    pub top_from: HeightFrom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_offset: Option<String>,
    /// ボトム高さ(削る底)。自動なら加工が決める高さ(選んだ面・輪郭・穴の底など)
    #[serde(default, skip_serializing_if = "HeightFrom::is_auto")]
    pub bottom_from: HeightFrom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom_offset: Option<String>,
    /// 前の加工(と、裏返す前のセットアップ)で削った所を見て、材料に触れない切削を省く
    /// (`jc-cam-commands` の `setups` が行う)。既定は入
    #[serde(default, skip_serializing_if = "AirCuts::is_skip")]
    pub air: AirCuts,
}

/// 前に削った所の空振り(加工の欄「削った所の空振り」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AirCuts {
    /// 省いて、上げて移るか下げたまま短く送る
    #[default]
    Skip,
    /// 戦略が作ったままにする
    Keep,
}

impl AirCuts {
    pub fn is_skip(&self) -> bool {
        *self == AirCuts::Skip
    }

    /// 加工のデータ(`air` の項目。無ければ省く)から。
    pub fn of(data: &serde_json::Value) -> AirCuts {
        data.get("air")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }
}

/// 加工ごとの高さの基準。高さは「基準 + オフセット」で決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeightFrom {
    /// 既定(安全高さはセットアップ、送り高さはトップ高さ + 1 mm、トップとボトムは加工が決める)
    #[default]
    Auto,
    /// トップ高さから(安全高さ・送り高さ)
    Top,
    StockTop,
    StockBottom,
    ModelTop,
    ModelBottom,
    /// 作業座標系の原点(Z = 0)
    Origin,
}

impl HeightFrom {
    pub fn is_auto(&self) -> bool {
        *self == HeightFrom::Auto
    }
}

/// 解いた後の、加工ごとの高さ([`prepare`] が作る)。トップとボトムは加工が [`Prepared::top`]・
/// [`Prepared::bottom`] で決め、使った高さを覚える(画面に色の面で出すため)。
#[derive(Debug, Clone, Default)]
pub struct OpHeights {
    pub(crate) top: (HeightFrom, f64),
    pub(crate) bottom: (HeightFrom, f64),
    /// ストックの上面・底面、部品の上面・底面(作業座標系の Z)
    pub(crate) refs: [f64; 4],
    pub(crate) used_top: std::cell::Cell<Option<f64>>,
    pub(crate) used_bottom: std::cell::Cell<Option<f64>>,
}

impl OpHeights {
    pub(crate) fn resolve(&self, (from, offset): (HeightFrom, f64), auto: f64) -> f64 {
        let base = match from {
            HeightFrom::Auto | HeightFrom::Top => auto,
            HeightFrom::StockTop => self.refs[0],
            HeightFrom::StockBottom => self.refs[1],
            HeightFrom::ModelTop => self.refs[2],
            HeightFrom::ModelBottom => self.refs[3],
            HeightFrom::Origin => 0.0,
        };
        base + offset
    }
}

/// 加工の出力 `heights`: 使った高さ(作業座標系の Z。画面に色の面で出す)。トップとボトムは、加工が
/// 決めた時だけ(穴あけのように穴ごとに違う時は、いちばん高いトップといちばん低いボトム)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HeightsOutput {
    pub safe: f64,
    pub feed: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
}

impl jc_core::Payload for HeightsOutput {
    const TYPE: &'static str = "cam.heights@1";
}

/// 出力の値から、加工の高さを読む。
pub fn heights_of(v: &Value) -> Option<HeightsOutput> {
    <HeightsOutput as jc_core::Payload>::from_value(v)
}

/// シミュレーションで加工を削る格子の細かさ。大きさ(mm)は設定のシミュレーションで決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimGrid {
    /// 加工の種類で決める(荒削りの加工は粗い格子、ほかは細かい格子。種類は `jc-cam-commands` が決める)
    #[default]
    Auto,
    /// 粗い格子(荒削りの格子)
    Rough,
    /// 細かい格子(仕上げの格子)
    Fine,
}

impl SimGrid {
    pub fn is_auto(&self) -> bool {
        *self == SimGrid::Auto
    }

    /// 加工の機能のデータ(`sim_grid` の欄)から読む。無い・読めない時は自動。
    pub fn of(data: &serde_json::Value) -> SimGrid {
        data.get("sim_grid")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }
}

/// つなぎで早送りする高さの決め方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetractHeight {
    /// 通り道の材料のいちばん高い所 + 安全高さの余裕(退避高さより下げず、安全高さより上げない)
    #[default]
    Fit,
    /// いつもセットアップの安全高さ
    Safe,
}

impl RetractHeight {
    pub fn is_fit(&self) -> bool {
        *self == RetractHeight::Fit
    }
}

/// 数か式の文字列を、式の文字列として読む。
fn num_or_expr<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrExpr {
        Num(f64),
        Expr(String),
    }
    Ok(match Option::<NumOrExpr>::deserialize(d)? {
        None => None,
        Some(NumOrExpr::Num(v)) => Some(format!("{v}")),
        Some(NumOrExpr::Expr(e)) => Some(e),
    })
}

impl OpBase {
    pub fn new(setup: FeatureId, tools: FeatureId, tool: u32) -> Self {
        OpBase {
            setup,
            tools,
            tool,
            rpm: None,
            feed: None,
            plunge: None,
            stepdown: None,
            stepover: None,
            fit: None,
            last_step: None,
            stay_down: None,
            lift: None,
            retract: RetractHeight::Fit,
            sim_grid: SimGrid::Auto,
            safe_from: HeightFrom::Auto,
            safe_offset: None,
            feed_from: HeightFrom::Auto,
            feed_offset: None,
            top_from: HeightFrom::Auto,
            top_offset: None,
            bottom_from: HeightFrom::Auto,
            bottom_offset: None,
            air: AirCuts::Skip,
        }
    }

    /// 依存(セットアップ、工具ライブラリ、切削条件の式が読むパラメータ)。
    pub fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.setup, "setup"), output(self.tools, "tools")];
        v.extend(expr_refs(self.exprs())?);
        Ok(v)
    }

    fn exprs(&self) -> impl Iterator<Item = &String> {
        [
            &self.rpm,
            &self.feed,
            &self.plunge,
            &self.stepdown,
            &self.stepover,
            &self.fit,
            &self.last_step,
            &self.stay_down,
            &self.lift,
            &self.safe_offset,
            &self.feed_offset,
            &self.top_offset,
            &self.bottom_offset,
        ]
        .into_iter()
        .flatten()
    }

    pub fn exprs_mut(&mut self) -> impl Iterator<Item = &mut String> {
        [
            &mut self.rpm,
            &mut self.feed,
            &mut self.plunge,
            &mut self.stepdown,
            &mut self.stepover,
            &mut self.fit,
            &mut self.last_step,
            &mut self.stay_down,
            &mut self.lift,
            &mut self.safe_offset,
            &mut self.feed_offset,
            &mut self.top_offset,
            &mut self.bottom_offset,
        ]
        .into_iter()
        .flatten()
    }

    /// 切削条件の式と `others` の中のパラメータ名を書き換える。変わったら真。
    pub fn rename<'a>(
        &'a mut self,
        others: impl IntoIterator<Item = &'a mut String>,
        old: &str,
        new: &str,
    ) -> Result<bool, EvalError> {
        rename_all(self.exprs_mut().chain(others), old, new)
    }
}

/// 加工の機能が使う材料一式。
pub struct Prepared {
    pub info: SetupInfo,
    pub tool: Tool,
    pub cutting: Cutting,
    pub model: MachiningModel,
    /// 利用者に知らせること(材料に合う条件がない、チップロードが目安の外など)
    pub warnings: Vec<String>,
    /// 円弧のあてはめの許容差(mm)。None ならしない
    pub fit: Option<f64>,
    /// 最後の段の厚さ(mm)。None なら段を等しく分ける
    pub last_step: Option<f64>,
    /// つなぎの設定
    pub link: jc_toolpath_moves::link::LinkOptions,
    /// 加工ごとの高さ(トップとボトム。安全高さと送り高さは `info` に入れてある)
    pub heights: OpHeights,
}

impl jc_toolpath_moves::Cutter for Prepared {
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

impl Prepared {
    /// 経路を検査して出力にし、警告を添える。
    /// 固定具の避ける範囲に入る所は、範囲の上を越える。
    pub fn finish(&self, setup: FeatureId, mut tp: Toolpath) -> Result<Outputs, EvalError> {
        let mut warnings = self.warnings.clone();
        if let Some(tol) = self.fit {
            let before = tp.moves.len();
            fit_by_spans(&mut tp, tol);
            log::debug!(
                "円弧のあてはめ({tol} mm): 動き {before} → {}",
                tp.moves.len()
            );
        }
        let zones = &self.info.keep_out;
        if avoid(&mut tp, zones) > 0 {
            warnings.push(
                "固定具の避ける範囲に掛かる所は削らずに上を越えます(固定具の下は残ります)".into(),
            );
        }
        // 経路が固定具の上を通らなければ、早送りも当たらない
        if zones.iter().any(|z| z.1[2] + 1.0 > tp.safe_z) && passes_over(&tp, zones) {
            warnings.push("固定具が安全高さより高いので、早送りが当たるかもしれません(安全高さを上げてください)".into());
        }
        // つなぎは、固定具を越える動きを入れた後で置き換える(固定具の箱も避ける)
        jc_toolpath_moves::link::relink(
            &mut tp,
            &jc_toolpath_moves::link::Scene {
                stock_min: self.info.stock_min,
                stock_max: self.info.stock_max,
                triangles: &self.model.triangles,
                keep_out: zones,
            },
            &self.link,
        );
        let mut out = finish(setup, tp)?;
        add_warnings(&mut out, warnings);
        // 段取り表に出す切削条件(送り・切り込み・送り幅は経路だけでは分からないため)
        out.insert(
            "cutting".into(),
            jc_core::Payload::to_value(&CuttingOutput(self.cutting)),
        );
        out.insert(
            "heights".into(),
            jc_core::Payload::to_value(&self.heights_output()),
        );
        Ok(out)
    }

    pub fn r(&self) -> f64 {
        self.tool.radius()
    }

    /// トップ高さ(削り始める高さ)。`auto` は加工が決める高さ(ふつうはストックの上面)。
    /// 加工の欄「トップ高さ」の基準とオフセットを当てる。
    pub fn top(&self, auto: f64) -> f64 {
        let h = &self.heights;
        let z = h.resolve(h.top, auto);
        keep_top(&h.used_top, z);
        z
    }

    /// ボトム高さ(削る底)。`auto` は加工が決める底(選んだ面・輪郭・穴の底など)。
    pub fn bottom(&self, auto: f64) -> f64 {
        let h = &self.heights;
        let z = h.resolve(h.bottom, auto);
        keep_bottom(&h.used_bottom, z);
        z
    }

    /// トップ高さ `top` とボトム高さ `bottom` の組が使えるか(ボトムがトップより下)。
    pub fn check_range(&self, top: f64, bottom: f64) -> Result<(), EvalError> {
        range_problem(top, bottom).map_or(Ok(()), |m| Err(invalid(m)))
    }

    /// 使った高さ(出力 `heights`)。
    pub fn heights_output(&self) -> HeightsOutput {
        HeightsOutput {
            safe: self.info.safe_z,
            feed: self.info.retract_z,
            top: self.heights.used_top.get(),
            bottom: self.heights.used_bottom.get(),
        }
    }

    /// 経路の入れ物を作る。
    pub fn toolpath(&self, ctx: &EvalCtx<'_>) -> Toolpath {
        empty_toolpath(
            ctx.feature_id().to_string(),
            &self.tool,
            &self.cutting,
            [self.info.safe_z, self.info.retract_z],
        )
    }

    /// 上から `top` まで、`bottom` まで下げる高さの列(上から順、最後は `bottom`)。
    /// 最後の段の厚さがあれば、`bottom` のその厚さ上までを等しく分け、最後に `bottom` を足す。
    pub fn levels(&self, top: f64, bottom: f64) -> Vec<f64> {
        split_levels(top, bottom, self.cutting.stepdown, self.last_step)
    }

    /// 深さを決めていない加工の底。上面 `top` から `bottom` までが刃長より深ければ、刃長
    /// までにして注意を足す(足した直後から誤りにならないように。決めた深さが深すぎる時は誤りのまま)。
    pub fn reachable_bottom(&mut self, top: f64, bottom: f64) -> f64 {
        reachable_bottom(&self.tool, top, bottom, &mut self.warnings)
    }

    /// 刃長より深く切り込まないか。
    pub fn check_depth(&self, top: f64, bottom: f64) -> Result<(), EvalError> {
        depth_problem(&self.tool, top, bottom).map_or(Ok(()), |m| Err(invalid(m)))
    }
}

/// [`Prepared::levels`] の中身。`last` は最後の段の厚さ。深さがそれ以下なら段を等しく分ける。
pub fn split_levels(top: f64, bottom: f64, step: f64, last: Option<f64>) -> Vec<f64> {
    match last {
        Some(f) if f < top - bottom - 1e-9 => {
            let mut v = even_levels(top, bottom + f, step);
            v.push(bottom);
            v
        }
        _ => even_levels(top, bottom, step),
    }
}

/// `top` から `bottom` までを、1 段が `step` を超えない等しい段に分けた高さの列。
fn even_levels(top: f64, bottom: f64, step: f64) -> Vec<f64> {
    let n = (((top - bottom) / step) - 1e-9).ceil().max(1.0) as usize;
    (1..=n)
        .map(|i| top - (top - bottom) * i as f64 / n as f64)
        .collect()
}

/// セットアップ、工具、加工用の幾何をそろえる。
pub fn prepare(ctx: &EvalCtx<'_>, base: &OpBase) -> Result<Prepared, EvalError> {
    let mut info = SetupInfo::from_value(ctx.output(base.setup, "setup")?)
        .ok_or_else(|| invalid(format!("{} はセットアップではありません", base.setup)))?;
    let tool = ToolLibrary::tool_from(ctx, base.tools, base.tool)?;
    let (mut cutting, preset_warning) = tool.cutting_for(&info.material);
    let mut warnings: Vec<String> = preset_warning.into_iter().collect();
    for (dst, src, what, length_ok) in [
        (&mut cutting.rpm, &base.rpm, "回転数", false),
        (&mut cutting.feed, &base.feed, "送り", true),
        (&mut cutting.plunge, &base.plunge, "下降の送り", true),
        (&mut cutting.stepdown, &base.stepdown, "切込み", true),
        (&mut cutting.stepover, &base.stepover, "送り幅", false),
    ] {
        if let Some(src) = src {
            let q = ctx.expr(src)?;
            if !(q.dim == Dim::NONE || (length_ok && q.dim == Dim::LENGTH)) {
                return Err(invalid(format!(
                    "{what} `{src}` の単位が合いません(値: {q})"
                )));
            }
            let v = q.value;
            if !(v > 0.0 && v.is_finite()) {
                return Err(invalid("切削条件は 0 より大きくしてください"));
            }
            *dst = v;
        }
    }
    if cutting.stepover > 1.0 {
        return Err(invalid("送り幅は工具径の 1 倍以下にしてください"));
    }
    // レーザーは刃が無く、回転数の欄は出力なのでチップロードを見ない
    if tool.kind != jc_cam_tool::ToolKind::Laser {
        warnings.extend(tool.chip_load_warning(&cutting, &info.material));
    }
    let fit = match &base.fit {
        Some(src) => {
            let tol = length(ctx, src, "円弧のあてはめの許容差")?;
            if !(0.001..=0.1).contains(&tol) {
                return Err(invalid(
                    "円弧のあてはめの許容差は 0.001〜0.1 mm にしてください",
                ));
            }
            Some(tol)
        }
        None => None,
    };
    let mut link = jc_toolpath_moves::link::LinkOptions {
        fit_retract: base.retract == RetractHeight::Fit,
        ..Default::default()
    };
    if let Some(src) = &base.stay_down {
        link.stay_down = length(ctx, src, "下げたまま移る距離")?;
        if !(0.0..=1000.0).contains(&link.stay_down) {
            return Err(invalid("下げたまま移る距離は 0〜1000 mm にしてください"));
        }
    }
    if let Some(src) = &base.lift {
        link.lift = length(ctx, src, "持ち上げる高さ")?;
        if !(0.0..=10.0).contains(&link.lift) {
            return Err(invalid("持ち上げる高さは 0〜10 mm にしてください"));
        }
    }
    let last_step = match &base.last_step {
        Some(src) => {
            let v = length(ctx, src, "仕上げの切込み")?;
            if !(v > 0.0 && v.is_finite()) {
                return Err(invalid("最後の段の厚さは 0 より大きくしてください"));
            }
            Some(v)
        }
        None => None,
    };
    let heights = op_heights(ctx, base, &mut info)?;
    let mesh = info.part_mesh(ctx.kernel())?;
    let model = MachiningModel::from_mesh(&info.placed_mesh(&mesh), info.origin);
    Ok(Prepared {
        info,
        tool,
        cutting,
        model,
        warnings,
        fit,
        last_step,
        link,
        heights,
    })
}

/// 加工の欄の高さを解く。安全高さと送り高さは `info` の `safe_z`・`retract_z` を置き換え、
/// トップとボトムは加工が [`Prepared::top`]・[`Prepared::bottom`] で使う。
fn op_heights(
    ctx: &EvalCtx<'_>,
    base: &OpBase,
    info: &mut SetupInfo,
) -> Result<OpHeights, EvalError> {
    let off = |src: &Option<String>, what: &str| -> Result<f64, EvalError> {
        match src {
            Some(s) if !s.trim().is_empty() => length(ctx, s, what),
            _ => Ok(0.0),
        }
    };
    let stock_top = info.stock_max[2];
    let h = OpHeights {
        top: (
            base.top_from,
            off(&base.top_offset, "トップ高さのオフセット")?,
        ),
        bottom: (
            base.bottom_from,
            off(&base.bottom_offset, "ボトム高さのオフセット")?,
        ),
        refs: [
            stock_top,
            info.stock_min[2],
            info.part_max[2],
            info.part_min[2],
        ],
        ..Default::default()
    };
    // 安全高さと送り高さが見るトップ高さは、加工が決める高さをストックの上面としたもの
    let top = h.resolve(h.top, stock_top);
    let gap = (info.retract_z - stock_top).max(0.0);
    let base_of = |from: HeightFrom, auto: f64| match from {
        HeightFrom::Auto => auto,
        HeightFrom::Top => top,
        f => h.resolve((f, 0.0), 0.0),
    };
    // トップ高さを変えていなければ、セットアップの値そのまま(数の丸めで経路を変えない)
    let feed_auto = if top == stock_top {
        info.retract_z
    } else {
        top + gap
    };
    let feed = base_of(base.feed_from, feed_auto) + off(&base.feed_offset, "送り高さのオフセット")?;
    // 自動の安全高さは、セットアップの安全高さ(トップ高さを上げて送り高さが越える時はその高さ)
    let safe = base_of(base.safe_from, info.safe_z.max(feed))
        + off(&base.safe_offset, "安全高さのオフセット")?;
    if feed > safe + 1e-9 {
        return Err(invalid(format!(
            "送り高さ Z{feed:.3} が安全高さ Z{safe:.3} より上です"
        )));
    }
    if feed < top - 1e-9 {
        return Err(invalid(format!(
            "送り高さ Z{feed:.3} がトップ高さ Z{top:.3} より下です(材料へ早送りで下りてしまいます)"
        )));
    }
    info.safe_z = safe;
    info.retract_z = feed;
    Ok(h)
}

/// 円弧のあてはめを、役目の印の境目をまたがずに行う(印の付いた動きと付かない動きを
/// 1 本にまとめない)。印は付け直す。
fn fit_by_spans(tp: &mut Toolpath, tol: f64) {
    let opt = jc_toolpath_fit::FitOptions::new(tol);
    let n = tp.moves.len();
    let mut cuts: Vec<usize> = vec![0, n];
    for s in &tp.spans {
        cuts.push(s.start.min(n));
        cuts.push(s.end.min(n));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out: Vec<Move> = Vec::with_capacity(n);
    let mut starts = vec![0usize; n + 1];
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a == b {
            continue;
        }
        // 前の区切りの終わりにいることを、早送りで伝えてからまとめる(その早送りは捨てる)
        let mut chunk: Vec<Move> = Vec::with_capacity(b - a + 1);
        if let Some(prev) = out.last() {
            chunk.push(Move::Rapid { to: prev.to() });
        }
        let lead = chunk.len();
        chunk.extend_from_slice(&tp.moves[a..b]);
        let fitted = jc_toolpath_fit::fit(&chunk, &opt);
        for s in starts.iter_mut().take(b).skip(a) {
            *s = out.len();
        }
        out.extend(fitted.into_iter().skip(lead));
    }
    starts[n] = out.len();
    tp.moves = out;
    tp.remap_spans(&starts);
}

/// 経路を検査して出力にする。`setup` は経路をまとめる時に使う。
pub fn finish(setup: FeatureId, tp: Toolpath) -> Result<Outputs, EvalError> {
    if tp.moves.is_empty() {
        return Err(invalid("経路ができませんでした(工具が入る所がありません)"));
    }
    if let Some(p) = jc_toolpath::check(&tp).first() {
        return Err(EvalError::Other(format!(
            "経路の {} 番目の動きの誤り: {}",
            p.index, p.message
        )));
    }
    let mut out = Outputs::new();
    out.insert(
        "toolpath".into(),
        jc_core::Payload::to_value(&ToolpathOutput { setup, path: tp }),
    );
    Ok(out)
}

/// 加工の機能の出力 `cutting`: 経路を作った時の切削条件(工具の既定と欄の上書きを合わせたもの)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CuttingOutput(pub Cutting);

impl jc_core::Payload for CuttingOutput {
    const TYPE: &'static str = "cam.cutting@1";
}

/// 出力の値から切削条件を読む。
pub fn cutting_of(v: &Value) -> Option<Cutting> {
    <CuttingOutput as jc_core::Payload>::from_value(v).map(|c| c.0)
}

/// 出力の値から、セットアップと経路を読む。
pub fn toolpath_of(v: &Value) -> Option<(FeatureId, Toolpath)> {
    <ToolpathOutput as jc_core::Payload>::from_value(v).map(|t| (t.setup, t.path))
}

/// 斜めに入る角度の式を読む(空なら `None` = 真下に入る)。0 より大きく 45 度以下。
pub fn ramp_angle(ctx: &EvalCtx<'_>, src: &Option<String>) -> Result<Option<f64>, EvalError> {
    let Some(src) = src else {
        return Ok(None);
    };
    let q = ctx.expr(src)?;
    if q.dim != Dim::ANGLE {
        return Err(invalid(format!(
            "斜めに入る角度 `{src}` は角度の単位(deg)が必要です(値: {q})"
        )));
    }
    if !(q.value > 0.0 && q.value <= std::f64::consts::FRAC_PI_4 + 1e-12) {
        return Err(invalid(
            "斜めに入る角度は 0 より大きく 45 deg 以下にしてください",
        ));
    }
    Ok(Some(q.value))
}

/// 削る向き。主軸は時計回り(M3)とする。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutDir {
    /// ダウンカット(進む向きの右に残す壁がある)。刃が材料に厚く入って薄く抜ける。仕上げ面がよい
    #[default]
    Climb,
    /// アップカット(進む向きの左に残す壁がある)。機械のがたが大きい時や、硬い表皮を削る時に
    Conventional,
}

impl CutDir {
    pub fn is_climb(&self) -> bool {
        *self == CutDir::Climb
    }

    /// ダウンカットの向きに作った輪郭を、この向きに直す。
    pub fn apply(&self, l: Loop) -> Loop {
        match self {
            CutDir::Climb => l,
            CutDir::Conventional => l.reversed(),
        }
    }
}

/// 削る向きの欄。
pub fn direction_field() -> Field {
    Field::choice(
        "direction",
        "削る向き",
        &[("climb", "ダウンカット"), ("conventional", "アップカット")],
    )
    .help("主軸が時計回りの時。ダウンカットは仕上げ面がよい。アップカットは機械のがたが大きい時に")
}

/// 円弧で入って抜ける欄。
pub fn lead_field() -> Field {
    Field::opt_expr("lead", "リードの円弧の半径").length().help(
        "例: 2 mm。壁の手前で下りて、円弧で壁に沿い始め、円弧で離れる(壁に下りた跡が残らない)。\
         空なら壁の上で下りる",
    )
}
