//! 3D の荒削り(`cam.rough3d@1`)と平らな所の仕上げ(`cam.flat3d@1`)。
//!
//! 3D 粗取りは、昔からある等高線ごとのオフセット荒削り。段の高さごとに、工具の先端がその高さの
//! 時に部品を削らない(残す厚さを残す)工具の中心の領域を求め、ストックの外形の箱との共通部分を
//! 送り幅ずつ内側へずらした輪でたどる。中身は [`plan`]。
//!
//! - 段の深さの既定は工具の切り込み、送り幅は工具の切削条件。上を向いた平らな面の高さにも段を足す
//! - 残す厚さは軸方向(`leave`)と径方向(`leave_radial`)。径方向は工具を太らせて下ろす
//! - 外側が開いている領域はストックの外で下りて横から入る。閉じた所は輪に沿って斜めに下りるか、
//!   入る位置(`entry`)で下りる
//! - 取残し(`rest_tool`)は、前の太い工具が届かなかった所だけを削る
//! - 輪を 1 本の渦巻きにつなぐ(`spiral`)、内側の輪の角を丸める(`smoothing`)、閉じた所へ
//!   らせんで下りる位置を、入る位置が無ければいちばん広い所にする(`entry_kind`)、ブルノーズで
//!   平らな面に山を残さないよう送り幅を抑える(`allow_cusps` を切る)
//!
//! 平らな所の仕上げは、部品の上を向いた平らな面を見つけ、その高さで面の中を輪で削る。

pub mod plan;

use jc_cam_dropcutter::Limits;
use jc_cam_dropcutter::limits::{boundary_fields, leave_radial_field};
use jc_cam_setup::{
    EntryKind, OpBase, Planner, Prepared, StrategyInput, StrategyOutput, expr_refs, invalid,
    length, plan_error, prepare, ramp_angle,
};
use jc_cam_tool::{ToolKind, ToolLibrary};
use jc_core::{EvalCtx, EvalError, Feature, Field, InputRef, Outputs, Registry};
use serde::{Deserialize, Serialize};

pub use jc_cam_dropcutter::{Boundary, Contain};
pub use plan::{DEFAULT_RAMP_DEG, Report, Settings};

pub const ROUGH3D: &str = "cam.rough3d@1";
pub const FLAT3D: &str = "cam.flat3d@1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rough3d {
    #[serde(flatten)]
    pub base: OpBase,
    /// 軸方向(上向き)に残す厚さ
    #[serde(default = "zero")]
    pub leave: String,
    /// 径方向に残す厚さ・加工の範囲
    #[serde(flatten)]
    pub limits: Limits,
    /// 段の深さ(省くと工具の切り込み)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// 部品の高さを調べる格子の幅
    #[serde(default = "resolution")]
    pub resolution: String,
    /// ストックの上面から、どこまで下を削るか(省くと部品の底かストックの底まで)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
    /// 閉じた所へ輪に沿って下りる角度(例: `3 deg`)。空なら 3 deg
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_angle: Option<String>,
    /// 入る位置(セットアップの座標の XY)。閉じた所はそこで下りて(螺旋が入れば螺旋で)から
    /// 輪の始まりへ動く
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<[f64; 2]>,
    /// 取残し: 前の太い工具の番号。その工具が届かなかった所だけを削る
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_tool: Option<u32>,
    /// 領域の輪を 1 本の渦巻きにつなぐ(輪から輪へ移る所で向きを急に変えない)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spiral: bool,
    /// 内側の輪の角を丸める半径(空なら丸めない。いちばん外の輪は壁に沿うので丸めない)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smoothing: Option<String>,
    /// 閉じた所へ下りる方法(自動・らせん・斜め)
    #[serde(default, skip_serializing_if = "EntryKind::is_auto")]
    pub entry_kind: EntryKind,
    /// らせんの直径(空なら工具径の半分)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helix_diameter: Option<String>,
    /// 平らな面に送り幅の山が残るのを許す。切ると、ブルノーズの送り幅を刃先の平らな所の幅
    /// までに抑える。既定の入も書き残す(画面の入切の欄は、項目が無いと切と見る)
    #[serde(default = "yes")]
    pub allow_cusps: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flat3d {
    #[serde(flatten)]
    pub base: OpBase,
    /// 平らな面の上に残す厚さ
    #[serde(default = "zero")]
    pub leave: String,
    /// 径方向に残す厚さ・加工の範囲(選んだ面なら、その平らな面だけ)
    #[serde(flatten)]
    pub limits: Limits,
    /// 部品の高さを調べる格子の幅
    #[serde(default = "resolution")]
    pub resolution: String,
    /// 輪に沿って下りる角度(例: `3 deg`)。空なら 3 deg
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_angle: Option<String>,
}

fn zero() -> String {
    "0 mm".into()
}

fn resolution() -> String {
    "0.2 mm".into()
}

impl Rough3d {
    pub fn new(base: OpBase) -> Rough3d {
        Rough3d {
            base,
            leave: "0.3 mm".into(),
            limits: Limits {
                leave_radial: Some("0.3 mm".into()),
                ..Default::default()
            },
            step: None,
            resolution: resolution(),
            depth: None,
            ramp_angle: None,
            entry: None,
            rest_tool: None,
            spiral: false,
            smoothing: None,
            entry_kind: EntryKind::Auto,
            helix_diameter: None,
            allow_cusps: true,
        }
    }
}

impl Flat3d {
    pub fn new(base: OpBase) -> Flat3d {
        Flat3d {
            base,
            leave: zero(),
            limits: Limits {
                leave_radial: Some("0.3 mm".into()),
                ..Default::default()
            },
            resolution: "0.1 mm".into(),
            ramp_angle: None,
        }
    }
}

/// 送り幅。山を許さない時は、ブルノーズの刃先の平らな所の幅までに抑える(それより広く送ると、
/// 平らな面に角の丸みの山が残る)。平とボールは変えない(平は径いっぱいまで平ら、ボールは平らな所が
/// 無い)。
pub fn stepover_for(
    tool: &jc_cam_tool::Tool,
    stepover: f64,
    allow_cusps: bool,
) -> Result<f64, EvalError> {
    if allow_cusps || tool.kind != ToolKind::BullNose {
        return Ok(stepover);
    }
    let flat = tool.diameter - 2.0 * tool.corner_radius.unwrap_or(0.0);
    if flat < 0.05 * tool.diameter {
        return Err(invalid(format!(
            "工具 {} は刃先の平らな所がほとんど無いので、平らな面に山を残さない送り幅にできません",
            tool.name
        )));
    }
    Ok(stepover.min(flat))
}

/// 格子の幅を読む。
fn cell_of(ctx: &EvalCtx<'_>, src: &str) -> Result<f64, EvalError> {
    let cell = length(ctx, src, "格子の幅")?;
    if !(0.05..=1.0).contains(&cell) {
        return Err(invalid("格子の幅は 0.05〜1 mm にしてください"));
    }
    Ok(cell)
}

fn check_tool(p: &Prepared, what: &str, bull: bool) -> Result<(), EvalError> {
    let ok = match p.tool.kind {
        ToolKind::FlatEnd | ToolKind::BallEnd => true,
        ToolKind::BullNose => bull,
        _ => false,
    };
    if !ok {
        return Err(invalid(format!(
            "{what}には{}のエンドミルを使ってください(工具 {})",
            if bull {
                "平・ボール・ブルノーズ"
            } else {
                "平かボール"
            },
            p.tool.name
        )));
    }
    Ok(())
}

impl Rough3d {
    /// 式を数にした設定。
    fn settings(&self, ctx: &EvalCtx<'_>, p: &mut Prepared) -> Result<Settings, EvalError> {
        let leave = length(ctx, &self.leave, "残し量")?;
        let cell = cell_of(ctx, &self.resolution)?;
        let stepdown = match &self.step {
            Some(s) => length(ctx, s, "段の深さ")?,
            None => p.cutting.stepdown,
        };
        if stepdown <= 0.0 {
            return Err(invalid("段の深さは 0 より大きくしてください"));
        }
        let limits = self.limits.resolve(ctx, p, cell.min(0.2))?;
        let top = p.top(p.info.stock_max[2]);
        let mut bottom = p.info.stock_min[2].max(p.model.min[2]);
        if let Some(d) = &self.depth {
            let d = length(ctx, d, "削る深さ")?;
            if d <= 0.0 {
                return Err(invalid("削る深さは 0 より大きくしてください"));
            }
            bottom = bottom.max(top - d);
        } else {
            bottom = p.reachable_bottom(top, bottom);
        }
        let bottom = p.bottom(bottom);
        if top - bottom < 1e-6 {
            return Err(invalid("削る深さがありません"));
        }
        p.check_depth(top, bottom)?;
        let rest = match self.rest_tool {
            Some(n) => {
                let t = ToolLibrary::tool_from(ctx, self.base.tools, n)?;
                if !matches!(t.kind, ToolKind::FlatEnd | ToolKind::BallEnd) {
                    return Err(invalid("取残しの前の工具は平かボールにしてください"));
                }
                if t.radius() <= p.r() + 1e-6 {
                    return Err(invalid(format!(
                        "取残しの前の工具 {} は、今の工具 {} より太くしてください",
                        t.name, p.tool.name
                    )));
                }
                Some(t)
            }
            None => None,
        };
        let ramp = ramp_angle(ctx, &self.ramp_angle)?.unwrap_or(DEFAULT_RAMP_DEG.to_radians());
        let stepover = stepover_for(
            &p.tool,
            p.tool.diameter * p.cutting.stepover,
            self.allow_cusps,
        )?;
        let smoothing = match &self.smoothing {
            Some(src) => {
                let v = length(ctx, src, "角の丸め")?;
                if v < 0.0 {
                    return Err(invalid("角の丸めは 0 以上にしてください"));
                }
                v
            }
            None => 0.0,
        };
        let helix = match &self.helix_diameter {
            Some(src) => {
                let d = length(ctx, src, "らせんの直径")?;
                if d <= 0.0 {
                    return Err(invalid("らせんの直径は 0 より大きくしてください"));
                }
                Some(0.5 * d)
            }
            None => None,
        };
        Ok(Settings {
            leave,
            limits,
            cell,
            stepdown,
            stepover,
            ramp,
            entry: self.entry,
            rest,
            bottom,
            spiral: self.spiral,
            smoothing,
            entry_kind: self.entry_kind,
            helix,
        })
    }
}

/// 3D 粗取りの純関数: 解いた入力(`params` は [`Settings`](plan::Settings))だけから経路を作る。
pub fn plan_rough(input: &StrategyInput) -> Result<StrategyOutput, String> {
    let mut p = Planner::new(input);
    let s: plan::Settings = input.params()?;
    let (mut tp, report) = plan::rough(&p, &s);
    tp.name = input.name.clone();
    if report.loops == 0 {
        return Err(if s.rest.is_some() {
            "削る所がありません(前の工具が届かなかった所がありません)"
        } else {
            "削る所がありません(部品がストックいっぱいか、範囲が狭すぎます)"
        }
        .into());
    }
    if let Some(e) = s.entry
        && report.at_entry == 0
    {
        p.warnings.push(format!(
            "入る位置({:.1}, {:.1})では下りられなかったので、輪に沿って斜めに入ります",
            e[0], e[1]
        ));
    }
    log::debug!("3D 粗取り: {report:?}");
    p.warnings.extend(report.warnings);
    Ok(p.output(tp))
}

/// 平らな所の仕上げの純関数: 解いた入力(`params` は設定と範囲の面)だけから経路を作る。
pub fn plan_flats(input: &StrategyInput) -> Result<StrategyOutput, String> {
    let mut p = Planner::new(input);
    let (s, faces): (plan::Settings, Vec<String>) = input.params()?;
    let (mut tp, report) = plan::flats(&p, &s, &faces);
    tp.name = input.name.clone();
    if report.loops == 0 {
        return Err(
            "削る平らな面がありません(上を向いた平らな面がないか、工具が入りません)".into(),
        );
    }
    let lowest = tp
        .moves
        .iter()
        .map(|m| m.to()[2])
        .fold(f64::INFINITY, f64::min);
    p.check_depth(p.info.stock_max[2], lowest)
        .map_err(plan_error)?;
    p.warnings.extend(report.warnings);
    Ok(p.output(tp))
}

impl Feature for Rough3d {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs(
            [&self.leave, &self.resolution]
                .into_iter()
                .chain(self.limits.exprs())
                .chain(
                    [
                        &self.step,
                        &self.depth,
                        &self.ramp_angle,
                        &self.smoothing,
                        &self.helix_diameter,
                    ]
                    .into_iter()
                    .flatten(),
                ),
        )?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        check_tool(&p, "3D 粗取り", true)?;
        let s = self.settings(ctx, &mut p)?;
        let input = p.input(ctx.feature_id(), s, Default::default());
        let out = plan_rough(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut fields = vec![&mut c.leave, &mut c.resolution];
        fields.extend(c.limits.exprs_mut());
        fields.extend(
            [
                &mut c.step,
                &mut c.depth,
                &mut c.ramp_angle,
                &mut c.smoothing,
                &mut c.helix_diameter,
            ]
            .into_iter()
            .flatten(),
        );
        Ok(c.base
            .rename(fields, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

impl Flat3d {
    fn settings(&self, ctx: &EvalCtx<'_>, p: &mut Prepared) -> Result<Settings, EvalError> {
        let leave = length(ctx, &self.leave, "残し量")?;
        let cell = cell_of(ctx, &self.resolution)?;
        // 選んだ面は平らな面そのものを選ぶのに使い、範囲には使わない
        let mut limits = self.limits.clone();
        if limits.boundary == jc_cam_dropcutter::Boundary::Faces {
            limits.boundary = jc_cam_dropcutter::Boundary::None;
        }
        let limits = limits.resolve(ctx, p, cell.min(0.2))?;
        let ramp = ramp_angle(ctx, &self.ramp_angle)?.unwrap_or(DEFAULT_RAMP_DEG.to_radians());
        let top = p.top(p.info.stock_max[2]);
        let bottom = p.bottom(p.info.stock_min[2].max(p.model.min[2]));
        p.check_range(top, bottom)?;
        p.check_depth(top, bottom.max(top - p.tool.flute_length))?;
        Ok(Settings {
            leave,
            limits,
            cell,
            stepdown: p.cutting.stepdown,
            stepover: p.tool.diameter * p.cutting.stepover,
            ramp,
            entry: None,
            rest: None,
            bottom,
            spiral: false,
            smoothing: 0.0,
            entry_kind: EntryKind::Auto,
            helix: None,
        })
    }
}

impl Feature for Flat3d {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs(
            [&self.leave, &self.resolution]
                .into_iter()
                .chain(self.limits.exprs())
                .chain(&self.ramp_angle),
        )?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        check_tool(&p, "平らな所の仕上げ", false)?;
        let s = self.settings(ctx, &mut p)?;
        let faces = if self.limits.boundary == jc_cam_dropcutter::Boundary::Faces {
            self.limits.faces.clone()
        } else {
            Vec::new()
        };
        let input = p.input(ctx.feature_id(), (s, faces), Default::default());
        let out = plan_flats(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut fields = vec![&mut c.leave, &mut c.resolution];
        fields.extend(c.limits.exprs_mut());
        fields.extend(c.ramp_angle.as_mut());
        Ok(c.base
            .rename(fields, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    let mut fields =
        vec![
        Field::expr("leave", "残し量(軸方向)").length(),
        leave_radial_field(),
        Field::opt_expr("step", "段の深さ").length().help("空なら工具の切り込み"),
        Field::expr("resolution", "格子の幅").length()
            .help("部品の高さを調べる細かさ。細かいほど正確だが遅い"),
        Field::opt_expr("depth", "削る深さ").length()
            .help("ストックの上面から。空なら部品かストックの底まで(工具の刃長まで)"),
        Field::choice("entry_kind", "進入方法", &[("auto", "自動"), ("helix", "らせん"), ("ramp", "斜め(輪に沿って)")])
            .help("閉じた所へ下りる動き。自動: 進入位置があればそこでらせん、なければ輪に沿って斜め。\
                   らせん: 進入位置、なければ領域のいちばん広い所で、らせんで下りる(入らなければ輪に沿って斜め)。\
                   斜め: いつも輪に沿って下りる。外側が開いた所は、どれでもストックの外から入る"),
        Field::opt_expr("helix_diameter", "らせんの直径").length()
            .help("工具の中心が回る円の直径。例: 3 mm。空なら工具径の半分。入らなければ領域に収まるまで小さくする")
            .only("entry_kind", "helix", "auto"),
        Field::opt_expr("ramp_angle", "ランプ角度").angle()
            .help("例: 3 deg。らせんと、輪に沿って斜めに下りる角度。空なら 3 deg"),
        Field::info("entry", "進入位置")
            .help("閉じた所はここで下りる(螺旋が入れば螺旋で)。空なら進入方法の通り"),
        Field::bool("spiral", "輪を渦巻きにつなぐ")
            .help("領域の輪を、1 周ずつ内(閉じた所は外)の輪へ移る 1 本の渦巻きにする。輪から輪へ移る所で向きを急に変えない。輪が分かれる・穴のある領域は輪のまま"),
        Field::opt_expr("smoothing", "角の丸め").length()
            .help("内側の輪の角を丸める半径。例: 2 mm。空なら丸めない。いちばん外の輪は壁に沿うので丸めない。丸めて届かなくなる所には小さな輪を足す"),
        Field::bool("allow_cusps", "平らな面の山を許す")
            .help("切ると、ブルノーズで平らな面に角の丸みの山が残らないよう、送り幅を刃先の平らな所の幅までに抑える"),
        Field::opt_integer("rest_tool", "取残し加工: 前の工具の番号").unitless().help(
            "前に同じ所を削った太い工具。その工具が届かなかった所だけを削る。空なら全部を削る",
        ),
    ];
    fields.extend(boundary_fields());
    fields.extend(jc_cam_setup::height_fields());
    reg.register_described(
        jc_cam_setup::operation(
            ROUGH3D,
            "3D 粗取り",
            "3D の部品のまわりを、段ごとにオフセットの輪で削り落とす",
            fields,
        )
        .icon("rough3d"),
        |d| Ok(Box::new(jc_cam_setup::parse::<Rough3d>(d)?)),
    );
    let mut fields = vec![
        Field::expr("leave", "残し量(軸方向)").length(),
        leave_radial_field(),
        Field::expr("resolution", "格子の幅").length(),
        Field::opt_expr("ramp_angle", "ランプ角度")
            .angle()
            .help("例: 3 deg。空なら 3 deg"),
    ];
    fields.extend(boundary_fields());
    fields.extend(jc_cam_setup::height_fields());
    reg.register_described(
        jc_cam_setup::operation(
            FLAT3D,
            "平坦部(3D)",
            "部品の上を向いた平らな面を見つけ、その高さで面の中を仕上げる",
            fields,
        )
        .icon("flat3d"),
        |d| Ok(Box::new(jc_cam_setup::parse::<Flat3d>(d)?)),
    );
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> jc_cam_setup::StrategyModule {
    use jc_cam_setup::{Strategy, StrategyModule, fitted, to_data};
    use jc_cam_tool::ToolKind;
    StrategyModule::new(
        "jc-strategy-rough3d",
        register,
        vec![
            Strategy::new(ROUGH3D, "cam.add.rough3d", "3D 粗取り", |p| {
                to_data(Rough3d::new(fitted(p.with_tool(ToolKind::FlatEnd))))
            })
            .order(20)
            .menu("3D")
            .icon("rough3d")
            .aliases("3d あらけずり rough roughing clearing pocket おふせっと offset")
            .hint("3D の部品のまわりを段ごとにオフセットの輪で削り落とす。0.3 mm 残すので、後で仕上げる")
            .entry("クリックした所で下りてから削り始める。斜めに入る角度があれば、そこで螺旋に下りる")
            .rough_grid()
            .no_reuse(),
            Strategy::new(FLAT3D, "cam.add.flat3d", "平坦部(3D)", |p| {
                let mut f = Flat3d::new(p.with_tool(ToolKind::FlatEnd));
                // 平らな面を選んでいれば、その面だけ
                if let Some(face) = p.planar_face() {
                    f.limits.boundary = Boundary::Faces;
                    f.limits.faces = vec![face];
                }
                to_data(f)
            })
            .order(24)
            .menu("3D")
            .icon("flat3d")
            .not_front()
            .aliases("たいら ひらら flat horizontal area 3d そこ ゆか")
            .hint("上を向いた平らな面を見つけて、その高さで仕上げる。面を選ぶとその面だけ")
            .no_reuse(),
        ],
    )
}

#[cfg(test)]
mod tests;
