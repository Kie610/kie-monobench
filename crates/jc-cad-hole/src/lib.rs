//! スケッチの点にあける穴(`solid.hole@1`)。種類(単純・ザグリ・皿)、止まり穴の先端の角度、
//! ネジ(通し穴・ネジ穴。ネジ山の形は作らず、径を表から決めて属性として残す)を持つ。
mod shape;
pub mod thread;
pub use shape::{Fit, HoleKind, Piece, Section, THREADS, Tap, ThreadSize, Tip, pieces, thread};
pub use thread::{THREAD, THREAD_ON_FACE, Thread, ThreadCommands};

use jc_cad::*;
use jc_core::kernel::{self as k, Profile, ProfileEdge, ProfileLoop, Region, Segment};
use jc_core::{EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Registry, Value};
use jc_core::{Payload, PointId, Points};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const HOLE: &str = "solid.hole@1";

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// スケッチの点に穴をあける。深さを省くと貫通。穴はスケッチの平面から法線の逆向きに掘る
/// (形が平面の表側にしかなければ表側へ掘る)。範囲 `"extent": "to_face"` は、深さの代わりに
/// `to_face`(`{"feature": 2, "face": "f2:start"}`)の平らな面まで掘る。
/// 面が穴の面と平行でなければ穴の底をその面で切る(底は面に沿って斜めになり、先端の角は付けない)。
/// 頂点(`{"feature": 2, "vertex": "…"}`)なら、その点を通る穴の面と平行な平面まで。
/// 例: `{"target": 2, "sketch": 3, "points": [5, 6], "diameter": "5 mm", "depth": "8 mm"}`、
/// `{"target": 2, "sketch": 3, "kind": "counterbore", "tap": "clearance", "size": "M3"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hole {
    pub target: FeatureId,
    pub sketch: FeatureId,
    /// 穴の位置の点。空ならスケッチの単独の点すべて
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<PointId>,
    /// 穴の径。ネジ(`tap`)を選んだ時は空で、表から決める
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub diameter: String,
    /// 面から、径いっぱいの部分の終わりまでの深さ(先端の円すいは含まない)。省くと貫通
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: HoleKind,
    /// ザグリの径・深さ(空ならネジの表、なければ穴の径から)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterbore_diameter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterbore_depth: Option<String>,
    /// 皿の径・角度(空ならネジの表か穴の径の 2 倍、90°)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub countersink_diameter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub countersink_angle: Option<String>,
    /// 止まり穴の底(貫通では使わない)
    #[serde(default, skip_serializing_if = "is_default")]
    pub tip: Tip,
    /// 先端の角度(空なら 118°)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_angle: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tap: Tap,
    /// ネジのサイズ(例: `M3`。省くと M3)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    /// 通し穴のはめあい
    #[serde(default, skip_serializing_if = "is_default")]
    pub fit: Fit,
    #[serde(default, skip_serializing_if = "is_default")]
    pub extent: HoleExtent,
    /// 範囲が「対象まで」の時の面(機能 `feature` の出力 `body` の平らな面)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_face: Option<HoleStop>,
}

/// 穴の深さの決め方: 深さの欄(空なら貫通)か、選んだ面まで。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleExtent {
    #[default]
    Distance,
    ToFace,
}

/// 穴を止める面(機能 `feature` の出力 `body` の面 `face`)か頂点 `vertex`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoleStop {
    pub feature: FeatureId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub face: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertex: Option<String>,
}

impl Exprs for Hole {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut v = Vec::new();
        if !self.diameter.is_empty() {
            v.push(&mut self.diameter);
        }
        for d in [
            &mut self.depth,
            &mut self.counterbore_diameter,
            &mut self.counterbore_depth,
            &mut self.countersink_diameter,
            &mut self.countersink_angle,
            &mut self.tip_angle,
        ]
        .into_iter()
        .flatten()
        {
            v.push(d);
        }
        v
    }
}

/// 道具の上の面を穴の面からはみ出させる量(押し出しと同じ候補を順に試す)。
const ABOVE: [f64; 4] = EXTENSIONS;

impl Hole {
    /// 単純な貫通穴(径は空)。残りの項目を `..Hole::new(target, sketch)` で埋めるのに使う。
    pub fn new(target: FeatureId, sketch: FeatureId) -> Hole {
        Hole {
            target,
            sketch,
            points: vec![],
            diameter: String::new(),
            depth: None,
            kind: HoleKind::Simple,
            counterbore_diameter: None,
            counterbore_depth: None,
            countersink_diameter: None,
            countersink_angle: None,
            tip: Tip::Flat,
            tip_angle: None,
            tap: Tap::Simple,
            size: None,
            fit: Fit::Normal,
            extent: HoleExtent::Distance,
            to_face: None,
        }
    }

    /// 止まり穴か(深さを打ったか、面まで掘る)。
    fn blind(&self) -> bool {
        self.depth.is_some() || self.extent == HoleExtent::ToFace
    }

    /// 範囲が「対象まで」の時の深さ: スケッチの平面から、掘る向き(平面 `plane` の法線の逆)に
    /// 止める面の平面まで。面が穴の面と平行でなければ、穴の中心 `centers`(半径 `radius`)のうち
    /// いちばん深い所より先まで掘り、底を切る平面も返す。頂点なら、その点を通る平行な平面まで。
    fn depth_to_face(
        &self,
        ctx: &EvalCtx<'_>,
        plane: &k::Plane,
        centers: &[k::P2],
        radius: f64,
    ) -> Result<(f64, Option<StopPlane>), EvalError> {
        let bad = |m: &str| EvalError::InvalidData(m.into());
        let t = self.to_face.as_ref().ok_or_else(|| {
            bad("止める面をまだ選んでいません。「穴を止める面を選ぶ」を押して、平らな面か頂点をクリックしてください")
        })?;
        let body = body_of(ctx, t.feature)?;
        let n = k::normalize(plane.normal());
        let deeper = |d: f64| -> Result<f64, EvalError> {
            if d <= 1e-6 {
                return Err(bad(
                    "止める面が掘る向きの先にありません。穴の面より奥にある面か頂点を選んでください",
                ));
            }
            Ok(d)
        };
        if let Some(v) = &t.vertex {
            let p = stop_at::vertex_at(ctx.kernel(), body, v).map_err(EvalError::InvalidData)?;
            return Ok((deeper(-k::dot(k::sub(p, plane.origin), n))?, None));
        }
        let face = ctx
            .kernel()
            .faces(body)
            .map_err(kernel_err)?
            .into_iter()
            .find(|f| f.name == t.face)
            .ok_or_else(|| {
                EvalError::InvalidData(format!(
                    "止める面 {} が見つかりません(形が変わって面が無くなった時は、選び直してください)",
                    t.face
                ))
            })?;
        let p = face
            .planar
            .ok_or_else(|| bad("止める面が平らではありません(平らな面か頂点だけ使えます)"))?;
        let m = k::normalize(p.normal);
        let c = k::dot(m, n);
        if c.abs() > 1.0 - 1e-6 {
            return Ok((deeper(-k::dot(k::sub(p.point, plane.origin), n))?, None));
        }
        if c.abs() < 1e-3 {
            return Err(bad(
                "止める面が掘る向きと平行なので、そこまで届きません。掘る向きに向かい合う面を選んでください",
            ));
        }
        // 穴の円の縁(中心から半径の所)ごとに、掘る向き(-n)に進んで面に当たるまでの深さ
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for q in centers {
            let w = plane.to_world(*q);
            for i in 0..16 {
                let a = std::f64::consts::TAU * i as f64 / 16.0;
                let e = k::add(
                    w,
                    k::add(
                        k::scale(plane.x_axis, radius * a.cos()),
                        k::scale(plane.y_axis, radius * a.sin()),
                    ),
                );
                // e - n s が面の上: dot(e - n s - p, m) = 0
                let s = k::dot(k::sub(e, p.point), m) / c;
                lo = lo.min(s);
                hi = hi.max(s);
            }
        }
        if lo <= 1e-6 {
            return Err(bad(
                "止める面(を広げた平面)が穴の入口と交わっているか、掘る向きの先にありません。穴の全体が面の手前にある時だけ使えます",
            ));
        }
        // 捨てる側: 掘る向きに面を越えた側
        let ahead = if c < 0.0 { 1.0 } else { -1.0 };
        Ok((
            hi + 1.0,
            Some(StopPlane {
                point: p.point,
                normal: k::scale(m, ahead),
            }),
        ))
    }

    fn positive(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
        let v = length(ctx, src, what)?;
        if v <= 0.0 {
            return Err(EvalError::InvalidData(format!(
                "{what}は 0 より大きくしてください"
            )));
        }
        Ok(v)
    }

    fn opt_length(
        ctx: &EvalCtx<'_>,
        src: &Option<String>,
        what: &str,
    ) -> Result<Option<f64>, EvalError> {
        src.as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| Self::positive(ctx, s, what))
            .transpose()
    }

    fn opt_angle(
        ctx: &EvalCtx<'_>,
        src: &Option<String>,
        what: &str,
    ) -> Result<Option<f64>, EvalError> {
        src.as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| angle(ctx, s, what))
            .transpose()
    }

    /// ネジのサイズの名前(省いたら欄の最初の選択肢。画面は項目がなければ最初の選択肢を出す)。
    fn size_name(&self) -> &str {
        self.size
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_SIZE)
    }

    /// ネジの呼び(ネジを選んだ時だけ)。ネジ穴は表(M2〜M12 並目)に無い呼びも、ねじの表にあれば
    /// 打って使える(例: `M6x0.75`、`M16`)。通し穴は通し穴・ザグリ・皿の径が要るので表の呼びだけ。
    fn thread(&self) -> Result<Option<(thread::Size, Option<&'static ThreadSize>)>, EvalError> {
        if self.tap == Tap::Simple {
            return Ok(None);
        }
        let name = self.size_name();
        let table = || {
            THREADS
                .iter()
                .map(|t| t.name)
                .collect::<Vec<_>>()
                .join("、")
        };
        let size = thread::parse_size(name).ok_or_else(|| {
            EvalError::InvalidData(format!(
                "ネジのサイズ {name} は読めません(表の {} を選ぶか、M6x0.75・M16 のように打ってください)",
                table()
            ))
        })?;
        let row = size
            .coarse
            .then(|| THREADS.iter().find(|t| (t.major - size.major).abs() < 1e-9))
            .flatten();
        if row.is_none() && self.tap == Tap::Clearance {
            return Err(EvalError::InvalidData(format!(
                "通し穴の径は表の {} だけです({name} はネジ穴にするか、径を決めた穴にしてください)",
                table()
            )));
        }
        Ok(Some((size, row)))
    }

    /// 穴の径(ネジを選んだ時は表から)。
    fn diameter(&self, ctx: &EvalCtx<'_>) -> Result<f64, EvalError> {
        match self.thread()? {
            Some((s, _)) if self.tap == Tap::Tapped => Ok(s.tap_drill()),
            Some((_, Some(t))) => Ok(t.clearance[self.fit as usize]),
            // 通し穴は表の呼びだけ(thread で確かめた)
            Some((s, None)) => Err(EvalError::InvalidData(format!(
                "通し穴の径の表に {} がありません",
                s.label()
            ))),
            None if self.diameter.trim().is_empty() => {
                Err(EvalError::InvalidData("穴の径を入れてください".into()))
            }
            None => Self::positive(ctx, &self.diameter, "穴の径"),
        }
    }

    /// 部品が複数ある穴(ザグリ・皿・先端)を作る。皿の道具の上の面が穴の面と重なると
    /// ブーリアンが失敗することがあるので、はみ出す量を変えて作り直す。
    #[allow(clippy::too_many_arguments)]
    fn countersunk(
        &self,
        ctx: &EvalCtx<'_>,
        section: &Section,
        plane: &k::Plane,
        centers: &[(&PointId, k::P2)],
        through: bool,
        stop: Option<StopPlane>,
        target: jc_core::ShapeHandle,
    ) -> Result<Outputs, EvalError> {
        let mut last = None;
        for above in ABOVE {
            let recipe = recipe(
                &Section { above, ..*section },
                plane,
                centers,
                through,
                stop,
            )?;
            match apply_recipe(ctx, &recipe, Some(target), &tag(ctx)) {
                Ok(body) => {
                    return Ok([
                        ("body".to_string(), Value::Shape(body)),
                        ("recipe".to_string(), Payload::to_value(&recipe)),
                    ]
                    .into());
                }
                Err(e @ EvalError::Kernel(_)) => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| EvalError::Kernel("ブーリアンに失敗しました".into())))
    }

    /// 深さと形以外の寸法(径・ザグリ・皿・先端)。
    fn section(&self, ctx: &EvalCtx<'_>, depth: f64) -> Result<Section, EvalError> {
        let dia = self.diameter(ctx)?;
        let t = self.thread()?.and_then(|(_, row)| row);
        let cb_d = Self::opt_length(ctx, &self.counterbore_diameter, "ザグリの径")?;
        let cb_h = Self::opt_length(ctx, &self.counterbore_depth, "ザグリの深さ")?;
        let cs_d = Self::opt_length(ctx, &self.countersink_diameter, "皿の径")?;
        let cs_a = Self::opt_angle(ctx, &self.countersink_angle, "皿の角度")?;
        let tip_a = Self::opt_angle(ctx, &self.tip_angle, "先端の角度")?;
        Ok(Section {
            diameter: dia,
            depth,
            kind: self.kind,
            counterbore: (
                cb_d.unwrap_or(t.map_or(dia * 1.8, |t| t.counterbore.0)),
                cb_h.unwrap_or(t.map_or(dia, |t| t.counterbore.1)),
            ),
            countersink: (
                cs_d.unwrap_or(t.map_or(dia * 2.0, |t| t.countersink)),
                cs_a.unwrap_or(90f64.to_radians()),
            ),
            tip: (self.tip == Tip::Angled && self.blind())
                .then(|| tip_a.unwrap_or(118f64.to_radians())),
            above: 0.0,
        })
    }
}

/// 点 `center`(世界座標)を中心に、断面 `parts` を軸 `n` のまわりに回す道具。`x` は平面の中の向き。
fn revolved(parts: &[shape::Part], center: k::P3, x: k::P3, n: k::P3) -> Recipe {
    let mut outer = ProfileLoop {
        edges: parts
            .iter()
            .map(|(name, a, b)| ProfileEdge {
                name: (*name).into(),
                segment: Segment::Line { a: *a, b: *b },
                reversed: false,
            })
            .collect(),
    };
    if outer.signed_area() < 0.0 {
        outer.reverse();
    }
    Recipe::Revolve {
        operation: Operation::Cut,
        profile: Profile {
            plane: k::Plane {
                origin: center,
                x_axis: x,
                y_axis: n,
            },
            regions: vec![Region {
                outer,
                holes: vec![],
            }],
        },
        origin: center,
        dir: n,
        angle: std::f64::consts::TAU,
    }
}

/// 穴の道具の作り方。部品が 1 つ(まっすぐな穴)なら押し出しそのもの。
fn recipe(
    s: &Section,
    plane: &k::Plane,
    centers: &[(&PointId, k::P2)],
    through: bool,
    stop: Option<StopPlane>,
) -> Result<Recipe, EvalError> {
    let mut parts = Vec::new();
    for piece in shape::pieces(s).map_err(EvalError::InvalidData)? {
        match piece {
            shape::Piece::Cylinder {
                name,
                radius,
                bottom,
            } => {
                let regions = centers
                    .iter()
                    .map(|(p, c)| Region {
                        outer: ProfileLoop {
                            edges: vec![ProfileEdge {
                                // まっすぐな穴の面の名前は `edge_name` で決まる(後の機能と加工がこの名前で参照する)
                                name: jc_interchange::hole::edge_name(p.0, name),
                                segment: Segment::Circle { center: *c, radius },
                                reversed: false,
                            }],
                        },
                        holes: vec![],
                    })
                    .collect();
                parts.push(Recipe::Extrude {
                    operation: Operation::Cut,
                    profile: Profile {
                        plane: *plane,
                        regions,
                    },
                    from: bottom,
                    to: 0.0,
                    through: through && name == "wall",
                    stop: stop.filter(|_| name == "wall"),
                    taper: 0.0,
                });
            }
            shape::Piece::Revolved(section) => {
                for (_, c) in centers {
                    parts.push(revolved(
                        &section,
                        plane.to_world(*c),
                        plane.x_axis,
                        plane.normal(),
                    ));
                }
            }
        }
    }
    Ok(if parts.len() == 1 {
        parts.pop().expect("1 つある")
    } else {
        Recipe::Many { parts }
    })
}

impl Feature for Hole {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.target, "body"), output(self.sketch, "points")];
        if self.extent == HoleExtent::ToFace
            && let Some(t) = &self.to_face
            && t.feature != self.target
        {
            v.push(output(t.feature, "body"));
        }
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        vec![("body".into(), output(self.target, "body"))]
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let sol = Points::from_value(ctx.output(self.sketch, "points")?).ok_or_else(|| {
            EvalError::InvalidData(format!("{} はスケッチではありません", self.sketch))
        })?;
        let target = body_of(ctx, self.target)?;
        // 掘る向き: スケッチの平面から法線の逆向きが基本。形が平面の表側(法線の向き)にしか
        // なければ、表側へ掘る(下の面 z=0 のスケッチの点で、上にある板に穴をあける時など)。
        // 逆向きだけを掘ると、計算は通るのに何もない所を掘って穴が無い形になる
        let (min, max) = ctx.kernel().bounds(target).map_err(kernel_err)?;
        let (mut below, mut above): (f64, f64) = (0.0, 0.0);
        for i in 0..8 {
            let c = [
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            ];
            let d = k::dot(k::sub(c, sol.plane.origin), sol.plane.normal());
            below = below.max(-d);
            above = above.max(d);
        }
        // 外形の箱は少し大きめのことがあるので、裏側が全体の 1% 未満なら「表側にしかない」とみなす
        let flip = above > 1e-6 && below < 0.01 * (above + below);
        // 表へ掘る時は、平面の y を逆にして法線を返す(点の座標も合わせて返す)
        let plane = if flip {
            k::Plane {
                y_axis: k::scale(sol.plane.y_axis, -1.0),
                ..sol.plane
            }
        } else {
            sol.plane
        };
        let points = if self.points.is_empty() {
            &sol.lone
        } else {
            &self.points
        };
        if points.is_empty() {
            return Err(EvalError::InvalidData(
                "穴の位置の点がありません(スケッチに、線や円に使っていない点を描いてください)"
                    .into(),
            ));
        }
        let mut centers = Vec::new();
        for p in points {
            let c = sol.points.get(p).ok_or_else(|| {
                EvalError::InvalidData(format!("穴の位置の点 {p} がスケッチにありません"))
            })?;
            centers.push((p, if flip { [c[0], -c[1]] } else { *c }));
        }
        let (depth, stop) = match &self.depth {
            _ if self.extent == HoleExtent::ToFace => {
                let at: Vec<k::P2> = centers.iter().map(|(_, c)| *c).collect();
                self.depth_to_face(ctx, &plane, &at, self.diameter(ctx)? / 2.0)?
            }
            Some(src) => (Self::positive(ctx, src, "穴の深さ")?, None),
            // 貫通: 外形の箱のいちばん深い角より 1 mm 先まで。長すぎる道具は
            // truck のブーリアンで誤った結果になることがあった(実測)ため、必要な分だけにする
            None => ((if flip { above } else { below }) + 1.0, None),
        };
        let mut section = self.section(ctx, depth)?;
        if stop.is_some() {
            // 斜めの面で切る底は面に沿う(先端の角は付けない)
            section.tip = None;
        }
        let through = !self.blind();
        let first = recipe(&section, &plane, &centers, through, stop)?;
        let out = if !matches!(first, Recipe::Many { .. }) {
            // まっすぐな穴は円の押し出し(面と重なる所の扱いは押し出しと同じ)
            finish(ctx, first, Some(self.target))?
        } else {
            self.countersunk(ctx, &section, &plane, &centers, through, stop, target)?
        };
        let mut out = out;
        if let Some(info) = out
            .get("recipe")
            .and_then(Recipe::from_value)
            .and_then(|r| self.info(&r))
        {
            out.insert(jc_interchange::HOLE_OUTPUT.into(), Payload::to_value(&info));
        }
        // 黙った失敗を防ぐ: 何も削れなければ誤りにする(点が形の外にある時など)
        if let Some(Value::Shape(body)) = out.get("body")
            && let (Ok(a), Ok(b)) = (ctx.kernel().volume(target), ctx.kernel().volume(*body))
            && (a - b).abs() <= 1e-6 * a.abs().max(1.0)
        {
            return Err(EvalError::InvalidData(
                "穴が形に当たっていません(穴の位置の点が形の外にあるか、掘る向きに形がありません)"
                    .into(),
            ));
        }
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

impl Hole {
    /// 加工に渡す穴の情報(道具の作り方 `recipe` から寸法を読む)。
    fn info(&self, recipe: &Recipe) -> Option<jc_interchange::HoleInfo> {
        use jc_interchange::hole as x;
        let mut i = x::HoleInfo {
            kind: match self.kind {
                HoleKind::Simple => x::HoleKind::Simple,
                HoleKind::Counterbore => x::HoleKind::Counterbore,
                HoleKind::Countersink => x::HoleKind::Countersink,
            },
            tap: match self.tap {
                Tap::Simple => x::Tap::Simple,
                Tap::Clearance => x::Tap::Clearance,
                Tap::Tapped => x::Tap::Tapped,
            },
            size: self.size.clone(),
            ..Default::default()
        };
        let parts: Vec<&Recipe> = match recipe {
            Recipe::Many { parts } => parts.iter().collect(),
            r => vec![r],
        };
        for r in parts {
            match r {
                Recipe::Extrude {
                    profile, from, to, ..
                } => {
                    for reg in &profile.regions {
                        for e in &reg.outer.edges {
                            let Segment::Circle { radius, .. } = e.segment else {
                                continue;
                            };
                            if x::is_point_name(&e.name) {
                                i.diameter = radius * 2.0;
                                i.count += 1;
                            } else if e
                                .name
                                .strip_suffix("_counterbore")
                                .is_some_and(x::is_point_name)
                            {
                                i.counterbore = Some((radius * 2.0, (to - from).abs()));
                            }
                        }
                    }
                }
                Recipe::Revolve { profile, .. } => {
                    for reg in &profile.regions {
                        for e in &reg.outer.edges {
                            // 皿の円すいの母線: 平面の x が半径、y が軸の向きの高さ(面が 0)
                            if e.name != "countersink" {
                                continue;
                            }
                            let Segment::Line { a, b } = e.segment else {
                                continue;
                            };
                            let (top, low) = if a[1] >= b[1] { (a, b) } else { (b, a) };
                            let rise = top[1] - low[1];
                            if rise <= 1e-9 {
                                continue;
                            }
                            let slope = (top[0] - low[0]).abs() / rise;
                            let big = top[0] - top[1] * slope;
                            i.countersink = Some((big * 2.0, 2.0 * slope.atan()));
                        }
                    }
                }
                Recipe::Sweep { .. } | Recipe::Loft { .. } | Recipe::Many { .. } => {}
            }
        }
        (i.count > 0).then_some(i)
    }
}

const KINDS: &[(&str, &str)] = &[
    ("simple", "単純"),
    ("counterbore", "ザグリ"),
    ("countersink", "皿"),
];
const TIPS: &[(&str, &str)] = &[("flat", "平ら"), ("angled", "ドリルの先")];
const TAPS: &[(&str, &str)] = &[
    ("simple", "なし(径を指定)"),
    ("clearance", "通し穴"),
    ("tapped", "ねじ穴(下穴)"),
];
// 画面は項目がなければ最初の選択肢を出すので、既定の値を最初に置く
const FITS: &[(&str, &str)] = &[("normal", "中"), ("close", "精"), ("loose", "粗")];
/// ネジのサイズを省いた時の値(欄の最初の選択肢)。
pub(crate) const DEFAULT_SIZE: &str = "M3";

/// 穴の設定欄(欄で指定する穴と、面に穴の作っている途中のパネルで同じものを使う)。
pub fn hole_fields() -> Vec<Field> {
    // よく使う M3 を最初(既定)にし、M4 から M12、最後に M2・M2.5 を並べる
    let mut sizes: Vec<&str> = THREADS.iter().map(|t| t.name).collect();
    let first = sizes.iter().position(|s| *s == DEFAULT_SIZE).unwrap_or(0);
    sizes.rotate_left(first);
    vec![
        Field::choice("kind", "穴のタイプ", KINDS),
        Field::choice("tap", "ねじ穴のタイプ", TAPS)
            .help("ネジ山の形は作らない。径を表から決め、ネジの属性として残す"),
        Field::text("size", "サイズ")
            .suggest(&sizes)
            .unless("tap", "simple")
            .help("一覧の呼び(並目)を選ぶか、M6x0.75(細目)・M16 のように打つ(ネジ穴だけ。通し穴は一覧の呼び)"),
        Field::choice("fit", "はめあい", FITS)
            .only("tap", "clearance", "simple")
            .help("通し穴の径(ISO 273 の精・中・粗)"),
        Field::expr("diameter", "直径").length().only("tap", "simple", "simple"),
        Field::choice(
            "extent",
            "範囲",
            &[
                ("distance", "深さ(空なら貫通)"),
                ("to_face", "対象まで"),
            ],
        )
        .help("「対象まで」にすると、穴を止める平らな面か頂点をクリックする手順に入る。斜めの面なら底が面に沿う")
        .pick_on("to_face", HOLE_TO_FACE),
        Field::opt_expr("depth", "深さ").length()
            .only("extent", "distance", "distance")
            .help("空なら貫通"),
        Field::info("to_face.face", "止める面")
            .help("「穴を止める面を選ぶ」を押して、平らな面か頂点をクリックして選ぶ"),
        Field::info("to_face.vertex", "止める頂点")
            .help("頂点を選んだ時は、その点を通る、穴の面と平行な平面まで掘る"),
        Field::opt_expr("counterbore_diameter", "ザグリの径").length()
            .only("kind", "counterbore", "simple")
            .help("空ならネジの表(なければ穴の径の 1.8 倍)"),
        Field::opt_expr("counterbore_depth", "ザグリの深さ").length()
            .only("kind", "counterbore", "simple")
            .help("空ならネジの表(なければ穴の径と同じ)"),
        Field::opt_expr("countersink_diameter", "皿の径").length()
            .only("kind", "countersink", "simple")
            .help("空ならネジの表(なければ穴の径の 2 倍)"),
        Field::opt_expr("countersink_angle", "皿の角度").angle()
            .only("kind", "countersink", "simple")
            .help("空なら 90 deg"),
        Field::choice("tip", "先端", TIPS).help("止まり穴の底。貫通では使わない"),
        Field::opt_expr("tip_angle", "先端の角度").angle()
            .only("tip", "angled", "flat")
            .help("空なら 118 deg"),
    ]
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    let mut d = solid(HOLE, "穴")
        .menu("作成")
        .icon("hole")
        .hint("スケッチの単独の点(線や円に使っていない点)すべてに穴をあける")
        .aliases("あな hole ザグリ 皿 ネジ穴 タップ")
        .role(jc_interchange::role::kind::EDIT_FROM_FACE)
        // 面の上のスケッチなら、その面のボディに開ける(直前のボディでなく)
        .field(Field::output("target", "対象", "body").follows("sketch"))
        .field(Field::output("sketch", "点のスケッチ", "points"));
    for f in hole_fields() {
        d = d.field(f);
    }
    reg.register_described(d.template(json!({"diameter": "5 mm"})), |d| {
        Ok(Box::new(parse::<Hole>(d)?))
    });
    thread::register(reg);
}

use jc_core::{Action, Arg, Context, Extension, Gesture, Handle, Outcome, Place};

/// 選んだ平らな面に、クリックした位置で穴をあける操作。
pub const HOLE_ON_FACE: &str = "cad.hole_on_face";
/// 選んでいる穴の深さを、クリックした平らな面までにする操作。
pub const HOLE_TO_FACE: &str = "cad.hole_to_face";
/// 選んでいる穴に、同じ面の上の点を足す操作(同じ径・種類の穴を続けてクリックで足す)。
pub const HOLE_ADD: &str = "cad.hole_add";
/// 面の上のスケッチの種類(スケッチのモジュールに依らず、保存の形だけを使う)。
const SKETCH: &str = "cad.sketch@1";
const NEED_FACE: &str = "穴をあける平らな面を選んでいません";

/// 穴の操作。
pub struct HoleCommands;

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// 穴の径の既定(mm)。
const DIAMETER: f64 = 5.0;

/// 穴の操作の引数(CLI・MCP の細かい指定。画面は面と点だけ)。
fn hole_args() -> Vec<Arg> {
    let mut v = vec![
        Arg::feature(
            "feature",
            "穴をあけるボディの機能の番号(省くと選んでいる面の機能)",
        ),
        Arg::text("face", "穴をあける平らな面の名前(省くと選んでいる面)"),
        Arg::vector("point", "穴の中心(世界座標の mm。省くと面の中心)"),
        Arg::length("diameter", "径(mm)").default(DIAMETER),
        Arg::length("depth", "深さ(mm。省くと貫通)"),
    ];
    for (k, h) in [
        ("kind", "穴の種類"),
        ("tip", "底の形"),
        ("tap", "ねじの種類(simple はねじ無し)"),
        ("size", "ねじの呼び"),
        ("fit", "はめあい"),
    ] {
        // 値は穴の機能の同じ名前の欄と同じ(中身は穴の機能が確かめる)
        v.push(Arg::text(k, &format!("{h}(穴の機能の欄 {k} の値)")));
    }
    v
}

/// 面の上の点 `point` に、径 `diameter`・深さ `depth`(省くと貫通)の穴をあける
/// (点だけのスケッチと穴を 1 回で足す)。
fn hole_on_face(cx: &Context<'_>) -> Result<Outcome, String> {
    let (pick, face) = chosen_face(cx)?;
    let pick = &pick;
    let point = match cx.arg("point").and_then(|v| v.as_array()) {
        Some(a) if a.len() == 3 => {
            let c: Vec<f64> = a.iter().filter_map(|x| x.as_f64()).collect();
            if c.len() != 3 {
                return Err("point は [x, y, z] の数で指定します".into());
            }
            [c[0], c[1], c[2]]
        }
        Some(_) => return Err("point は [x, y, z] の数で指定します".into()),
        None => face.point,
    };
    let dia = cx.arg_f64("diameter")?.unwrap_or(DIAMETER);
    if dia <= 0.0 {
        return Err("穴の径は 0 より大きくしてください".into());
    }
    // 面の上のスケッチと同じ平面の座標(スケッチのモジュールと同じ決め方)
    let local = k::Plane::from_point_normal(face.point, face.normal).to_local(point);
    let n = cx.count_of(SKETCH) + 1;
    let at = [round(local[0]), round(local[1])];
    let mut sketch = json!({
        "plane": {"kind": "face", "feature": pick.feature.0, "face": pick.face},
        "entities": [{"type": "point", "id": 1, "x": at[0], "y": at[1]}],
        "constraints": []
    });
    let plane = k::Plane::from_point_normal(face.point, face.normal);
    edge_dimensions(cx, pick.feature, &pick.face, &plane, 1, at, &mut sketch);
    let mut hole = json!({
        "target": pick.feature.0,
        "sketch": {"$new": 0},
        "diameter": format!("{} mm", round(dia)),
    });
    // 深さ(省けば貫通)
    if let Some(d) = cx.arg_f64("depth")? {
        if d <= 0.0 {
            return Err("穴の深さは 0 より大きくしてください(省くと貫通)".into());
        }
        hole["depth"] = json!(format!("{} mm", round(d)));
    }
    // 種類・ネジなど(CLI・MCP から。画面では作った後の欄で直す)。中身は穴の機能が確かめる
    for key in ["kind", "tip", "tap", "size", "fit"] {
        if let Some(v) = cx.arg(key).and_then(|v| v.as_str()) {
            hole[key] = json!(v);
        }
    }
    if hole.get("tap").is_some_and(|t| t != "simple") {
        // 径はネジの表から決める
        hole.as_object_mut()
            .expect("穴のデータは表")
            .remove("diameter");
    }
    Ok(Outcome::Change {
        label: "面に穴を追加".into(),
        edits: vec![
            jc_core::Edit::Insert {
                type_id: SKETCH.into(),
                name: format!("穴の位置{n}"),
                data: sketch,
            },
            jc_core::Edit::Insert {
                type_id: HOLE.into(),
                name: format!("穴{}", cx.count_of(HOLE) + 1),
                data: hole,
            },
        ],
    })
}

/// 穴をあける面: 選んでいる面か、CLI・MCP の引数 `feature`(ボディの機能)と `face`(面の名前)。
/// 置けない面(曲面・無い面)は、手順を止めずにすぐ理由を返す。
fn chosen_face(cx: &Context<'_>) -> Result<(jc_core::FacePick, jc_core::PlanarFace), String> {
    let given = cx
        .arg("feature")
        .and_then(|v| v.as_u64())
        .map(FeatureId)
        .zip(cx.arg("face").and_then(|v| v.as_str()));
    let pick = match (cx.picked, given) {
        (Some(p), _) => p.clone(),
        (None, Some((feature, face))) => jc_core::FacePick {
            feature,
            face: face.to_string(),
        },
        (None, None) => return Err(NEED_FACE.into()),
    };
    let Some(jc_core::Value::Shape(body)) = cx.output(pick.feature, "body") else {
        return Err(format!("{} はボディではありません", pick.feature));
    };
    let f = cx
        .kernel
        .faces(*body)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|f| f.name == pick.face)
        .ok_or_else(|| format!("{} に面 {} がありません", pick.feature, pick.face))?;
    match f.planar {
        Some(plane) => Ok((pick, plane)),
        None => Err(CURVED.into()),
    }
}

/// 曲面に穴を置こうとした時の理由。
const CURVED: &str =
    "曲面には穴をあけられません。平らな面を選んでください(穴は平らな面にだけ置けます)";

/// 選んでいる穴が、面の上のスケッチの点にあけたものなら、その穴・点のスケッチと面。
struct HoleFace {
    hole: FeatureId,
    data: Hole,
    sketch: FeatureId,
    sketch_data: serde_json::Value,
    /// 穴の後のボディの、スケッチを置いた面(足す点の吸い付きと、面の中かの確かめに使う)
    pick: jc_core::FacePick,
    face: jc_core::PlanarFace,
    /// スケッチの平面(点の座標を平面の座標にする)
    plane: k::Plane,
}

fn hole_face(cx: &Context<'_>) -> Option<HoleFace> {
    let id = cx.selected?;
    let entry = cx.data.feature(id).filter(|f| f.type_id == HOLE)?;
    let data: Hole = serde_json::from_value(entry.data.clone()).ok()?;
    let sk = cx
        .data
        .feature(data.sketch)
        .filter(|f| f.type_id == SKETCH)?;
    let face = sk.data.get("plane")?;
    if face.get("kind")?.as_str()? != "face" {
        return None;
    }
    let face_name = face.get("face")?.as_str()?.to_string();
    let plane = cx
        .output(data.sketch, "points")
        .and_then(Points::from_value)?
        .plane;
    let Some(jc_core::Value::Shape(body)) = cx.output(id, "body") else {
        return None;
    };
    let planar = cx
        .kernel
        .faces(*body)
        .ok()?
        .into_iter()
        .find(|f| f.name == face_name)?
        .planar?;
    Some(HoleFace {
        hole: id,
        sketch: data.sketch,
        sketch_data: sk.data.clone(),
        data,
        pick: jc_core::FacePick {
            feature: id,
            face: face_name,
        },
        face: planar,
        plane,
    })
}

/// 点 `p`(世界座標)が、ボディの面 `pick` の三角形の上にあるか(面の平面に写して調べる)。
fn inside_face(cx: &Context<'_>, pick: &jc_core::FacePick, plane: &k::Plane, p: [f64; 3]) -> bool {
    let Some(jc_core::Value::Shape(body)) = cx.output(pick.feature, "body") else {
        return true;
    };
    let Ok(mesh) = cx.kernel.mesh(*body, 0.05) else {
        return true;
    };
    let Some(fi) = mesh.face_names.iter().position(|n| *n == pick.face) else {
        return true;
    };
    let q = plane.to_local(p);
    let local = |i: u32| plane.to_local(mesh.positions[i as usize].map(f64::from));
    mesh.triangles
        .iter()
        .zip(&mesh.triangle_faces)
        .filter(|(_, f)| **f as usize == fi)
        .any(|(t, _)| {
            let [a, b, c] = t.map(local);
            let cross = |o: k::P2, u: k::P2, v: k::P2| {
                (u[0] - o[0]) * (v[1] - o[1]) - (u[1] - o[1]) * (v[0] - o[0])
            };
            let (d1, d2, d3) = (cross(a, b, q), cross(b, c, q), cross(c, a, q));
            let neg = d1 < -1e-9 || d2 < -1e-9 || d3 < -1e-9;
            let pos = d1 > 1e-9 || d2 > 1e-9 || d3 > 1e-9;
            !(neg && pos)
        })
}

/// 選んでいる穴の点のスケッチに、点 `point` を足す(同じ穴が 1 つ増える)。
fn hole_add(cx: &Context<'_>) -> Result<Outcome, String> {
    let h = hole_face(cx).ok_or("面の上のスケッチの点にあけた穴を選んでいません")?;
    let point = match cx.arg("point").and_then(|v| v.as_array()) {
        Some(a) if a.len() == 3 && a.iter().all(|x| x.is_f64() || x.is_i64()) => {
            [0, 1, 2].map(|i| a[i].as_f64().unwrap_or(0.0))
        }
        _ => return Err("point は [x, y, z] の数で指定します".into()),
    };
    let face_plane = k::Plane::from_point_normal(h.face.point, h.face.normal);
    if !inside_face(cx, &h.pick, &face_plane, point) {
        return Err("穴をあける面の中をクリックしてください".into());
    }
    let local = h.plane.to_local(point);
    let local = [round(local[0]), round(local[1])];
    let mut sketch = h.sketch_data.clone();
    let entities = sketch
        .get_mut("entities")
        .and_then(|e| e.as_array_mut())
        .ok_or("穴の点のスケッチが読めません")?;
    let taken = entities.iter().any(|e| {
        e.get("type").and_then(|t| t.as_str()) == Some("point")
            && (e.get("x").and_then(|v| v.as_f64()).unwrap_or(f64::NAN) - local[0]).abs() < 1e-6
            && (e.get("y").and_then(|v| v.as_f64()).unwrap_or(f64::NAN) - local[1]).abs() < 1e-6
    });
    if taken {
        return Err("そこにはもう穴があります".into());
    }
    let next = entities
        .iter()
        .filter_map(|e| e.get("id").and_then(|v| v.as_u64()))
        .max()
        .unwrap_or(0)
        + 1;
    entities.push(json!({"type": "point", "id": next, "x": local[0], "y": local[1]}));
    // 足した点も、近い縁からの寸法で置く(板の幅を変えるとついてくる)
    if let Some((feature, face)) = sketch.get("plane").and_then(|p| {
        Some((
            FeatureId(p.get("feature")?.as_u64()?),
            p.get("face")?.as_str()?.to_string(),
        ))
    }) {
        edge_dimensions(cx, feature, &face, &h.plane, next, local, &mut sketch);
    }
    let mut edits = vec![jc_core::Edit::SetData {
        id: h.sketch,
        data: sketch,
    }];
    // 点を絞っている穴なら、足した点も入れる(空ならスケッチの単独の点すべてにあく)
    if !h.data.points.is_empty() {
        let mut data = h.data.clone();
        data.points.push(PointId(next as u32));
        edits.push(jc_core::Edit::SetData {
            id: h.hole,
            data: serde_json::to_value(&data).map_err(|e| e.to_string())?,
        });
    }
    Ok(Outcome::Change {
        label: "同じ穴を足す".into(),
        edits,
    })
}

/// 選んでいる穴(ID とデータ)。
fn selected_hole(cx: &Context<'_>) -> Option<(FeatureId, Hole)> {
    let id = cx.selected?;
    let e = cx.data.feature(id).filter(|f| f.type_id == HOLE)?;
    Some((id, serde_json::from_value(e.data.clone()).ok()?))
}

/// 穴 `id` の深さを、クリックした面(CLI・MCP は引数 `face` と `feature`)までにする。面は穴より
/// 前の形から探す(穴をあけた後のボディの面を選んでも、元の対象のボディの同じ面を使う)。
fn hole_to_face(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, mut h) = selected_hole(cx).ok_or("穴を選んでいません")?;
    let (pick, given) =
        stop_at::stop_pick(cx).ok_or("穴を止める平らな面か頂点をクリックしてください")?;
    let at = |f: FeatureId| cx.data.index_of(f).unwrap_or(usize::MAX);
    let has = |f: FeatureId| {
        matches!(cx.output(f, "body"), Some(jc_core::Value::Shape(b))
            if stop_at::has(cx.kernel.as_ref(), *b, &pick))
    };
    let feature = [given, Some(h.target)]
        .into_iter()
        .flatten()
        .find(|f| at(*f) < at(id) && has(*f))
        .ok_or("この穴より後で出来た面や、曲がった面は選べません。穴より前からある平らな面か頂点を選んでください")?;
    let (face, vertex) = pick.parts();
    h.extent = HoleExtent::ToFace;
    h.to_face = Some(HoleStop {
        feature,
        face,
        vertex,
    });
    Ok(Outcome::Change {
        label: "穴の深さを止める面までにする".into(),
        edits: vec![jc_core::Edit::SetData {
            id,
            data: serde_json::to_value(&h).map_err(|e| e.to_string())?,
        }],
    })
}

impl Extension for HoleCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let face = cx.picked_plane().is_some();
        let hole = selected_hole(cx);
        // 面を選んでいなくても押せる(押してから面をクリックする)
        vec![
            Action::new(HOLE_TO_FACE, "穴を止める面を選ぶ", "cad")
                .args(stop_at::args())
                .menu("作成")
                .icon("hole")
                .aliases("あな hole たいしょう まで めん 深さ to object")
                .hint("選んでいる穴の深さを、クリックした平らな面(斜めの面なら底が面に沿う)か頂点までにする")
                .keep_edit()
                .at(match &hole {
                    Some((_, h)) if h.extent == HoleExtent::ToFace => {
                        &[Place::FeatureMenu, Place::Panel]
                    }
                    Some(_) => &[Place::FeatureMenu],
                    None => &[],
                })
                .disabled_if(hole.is_none().then_some("穴を選んでから使う")),
            Action::new(HOLE_ON_FACE, "穴", "cad")
                .args(hole_args())
                .role(jc_interchange::role::action::HOLE_ON_FACE)
                .menu("作成")
                .icon("hole")
                .aliases("あな hole めん 面に穴 ザグリ 皿 ネジ穴 タップ")
                .hint("押してから、平らな面 → 穴の中心 → 径の順にクリックすると穴ができる。続けて中心をクリックすると同じ穴を足せる。深さ・ザグリや皿・ネジは欄で直し、OK か Enter で決める。Esc でやめる")
                .then(HOLE_ADD)
                .at(if face {
                    &[Place::Toolbar, Place::FaceMenu]
                } else {
                    &[Place::Toolbar]
                }),
            Action::new(HOLE_ADD, "同じ穴を足す", "cad")
                .arg(Arg::vector("point", "足す点(世界座標の mm)").required())
                .menu("作成")
                .icon("hole")
                .aliases("あな hole たす ふやす 追加")
                .hint("選んでいる穴と同じ面に、クリックした位置で同じ穴を足す。Enter で終える")
                .repeat()
                .at(if hole_face(cx).is_some() {
                    &[Place::FeatureMenu]
                } else {
                    &[]
                })
                .disabled_if(
                    hole_face(cx)
                        .is_none()
                        .then_some("面の上のスケッチの点にあけた穴を選んでから使う"),
                ),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        match id {
            HOLE_ON_FACE => Some(hole_on_face(cx)),
            HOLE_ADD => Some(hole_add(cx)),
            HOLE_TO_FACE => Some(hole_to_face(cx)),
            _ => None,
        }
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if id == HOLE_TO_FACE {
            selected_hole(cx)?;
            return Some(vec![Handle {
                vertices: true,
                ..Handle::new(
                    "face",
                    "穴を止める平らな面(斜めの面なら底が面に沿う)か頂点をクリック",
                    Gesture::Face,
                )
            }]);
        }
        if id == HOLE_ADD {
            let h = hole_face(cx)?;
            return Some(vec![Handle {
                snap_to: snap_points(cx, &h.pick, &h.face),
                optional: true,
                ..Handle::new(
                    "point",
                    "続けて面をクリックすると同じ穴を足す(角・辺の中点・穴の中心に吸い付く)。Enter で決める",
                    Gesture::Point {
                        origin: h.face.point,
                        normal: h.face.normal,
                        snap: 0.5,
                    },
                )
            }]);
        }
        if id != HOLE_ON_FACE {
            return None;
        }
        const POINT: &str =
            "面の上をクリックして穴の中心を決める(角・辺の中点・穴の中心・スケッチの点に吸い付く)";
        const DIAMETER: &str = "マウスを動かして径を決め、クリックで穴を作る(数字でも打てる)。深さや種類は後の欄で直す";
        let face_step = || Handle::new("face", "穴をあける平らな面をクリック", Gesture::Face);
        let Some(f) = cx.picked_plane() else {
            // まだ面がない: 面を選ぶ手順から始める(後の手順は面を選んでから出し直す)
            return Some(vec![
                face_step(),
                Handle::new("point", POINT, Gesture::Face),
                Handle::new("diameter", DIAMETER, Gesture::Face),
            ]);
        };
        let point = cx
            .arg("point")
            .and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok());
        let center = point.unwrap_or(f.point);
        let mut v = vec![
            Handle {
                snap_to: cx
                    .picked
                    .map(|p| snap_points(cx, p, &f))
                    .unwrap_or_default(),
                ..Handle::new(
                    "point",
                    POINT,
                    Gesture::Point {
                        origin: f.point,
                        normal: f.normal,
                        snap: 0.5,
                    },
                )
            },
            Handle::new(
                "diameter",
                DIAMETER,
                Gesture::Diameter {
                    center,
                    normal: f.normal,
                },
            ),
        ];
        // 面を選ぶ手順を済ませてから出し直した時は、その手順を済みとして先頭に残す
        if cx.arg("face").is_some() {
            v.insert(0, face_step());
        }
        Some(v)
    }
}

/// 穴の中心(スケッチの点 `point`、平面の座標 `at`)から、面の縁のうち近い 2 本(平行でないもの)
/// までの距離を、位置のスケッチの寸法にする。縁はスケッチに写した線(投影)として置くので、
/// 板の幅などを変えると穴が縁についてくる。
/// 既に写してある縁はそれを使う。縁がまっすぐでなければ何もしない。`feature` と `face` は
/// スケッチを置いた面(穴より前のボディ)。
fn edge_dimensions(
    cx: &Context<'_>,
    feature: FeatureId,
    face: &str,
    plane: &k::Plane,
    point: u64,
    at: [f64; 2],
    sketch: &mut serde_json::Value,
) {
    let Some(jc_core::Value::Shape(body)) = cx.output(feature, "body") else {
        return;
    };
    // 面の縁のまっすぐな辺(スケッチの平面の座標)と、穴の中心からの距離
    // (距離, 端, 端, 両側の面)
    type Near = (f64, [f64; 2], [f64; 2], [String; 2]);
    let mut edges: Vec<Near> = cx
        .kernel
        .edges(*body)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.arc.is_none() && e.faces.iter().any(|f| f == face))
        .filter_map(|e| {
            let (a, b) = (plane.to_local(e.a), plane.to_local(e.b));
            let d = [b[0] - a[0], b[1] - a[1]];
            let len = d[0].hypot(d[1]);
            if len < 1e-6 {
                return None;
            }
            // 直線(辺を延ばした線)までの距離
            let dist = ((at[0] - a[0]) * d[1] - (at[1] - a[1]) * d[0]).abs() / len;
            Some((dist, a, b, e.faces))
        })
        .collect();
    edges.sort_by(|x, y| x.0.total_cmp(&y.0));
    let Some(first) = edges.first().cloned() else {
        return;
    };
    let unit = |a: [f64; 2], b: [f64; 2]| {
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
    };
    let u = unit(first.1, first.2);
    let second = edges.iter().skip(1).find(|e| {
        let v = unit(e.1, e.2);
        (u[0] * v[1] - u[1] * v[0]).abs() > 0.1
    });
    let max_id = |list: &serde_json::Value| {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| e.get("id").and_then(|v| v.as_u64()))
            .max()
            .unwrap_or(0)
    };
    let obj = sketch.as_object_mut().expect("スケッチは表");
    for key in ["entities", "constraints", "projections"] {
        obj.entry(key).or_insert_with(|| json!([]));
    }
    for (dist, a, b, faces) in std::iter::once(&first).chain(second) {
        if *dist < 1e-6 {
            continue;
        }
        // 同じ縁を写した線があれば使い回す(面の順は問わない)
        let same = |p: &serde_json::Value| {
            p["from"] == "edge"
                && p["feature"].as_u64() == Some(feature.0)
                && p["faces"].as_array().is_some_and(|f| {
                    f.len() == 2 && faces.iter().all(|n| f.iter().any(|g| g == n.as_str()))
                })
        };
        let line = match obj["projections"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| same(p)))
        {
            Some(p) => p["entity"].as_u64().unwrap_or(0),
            None => {
                let next = max_id(&obj["entities"]) + 1;
                let (pa, pb, line) = (next, next + 1, next + 2);
                let ents = obj["entities"].as_array_mut().expect("要素の一覧");
                ents.push(json!({"type": "point", "id": pa, "x": a[0], "y": a[1]}));
                ents.push(json!({"type": "point", "id": pb, "x": b[0], "y": b[1]}));
                ents.push(
                    json!({"type": "line", "id": line, "a": pa, "b": pb, "construction": true}),
                );
                obj["projections"].as_array_mut().expect("投影の一覧").push(
                    json!({"entity": line, "from": "edge", "feature": feature.0, "faces": faces}),
                );
                line
            }
        };
        let id = max_id(&obj["constraints"]) + 1;
        obj["constraints"]
            .as_array_mut()
            .expect("拘束の一覧")
            .push(json!({
                "id": id,
                "type": "point_line_distance",
                "point": point,
                "line": line,
                "value": format!("{} mm", round(*dist)),
            }));
    }
}

/// 穴の中心が吸い付く点: 面のまっすぐな辺の端と中点、面の内側の縁(既存の穴やボス)の中心、
/// 面と同じ平面のスケッチの点。
fn snap_points(
    cx: &Context<'_>,
    pick: &jc_core::FacePick,
    f: &jc_core::PlanarFace,
) -> Vec<[f64; 3]> {
    let Some(jc_core::Value::Shape(body)) = cx.output(pick.feature, "body") else {
        return vec![];
    };
    let on_plane = |p: [f64; 3]| k::dot(k::sub(p, f.point), f.normal).abs() < 1e-3;
    let mut out: Vec<[f64; 3]> = Vec::new();
    let mut push = |p: [f64; 3]| {
        if !out
            .iter()
            .any(|q| k::dot(k::sub(*q, p), k::sub(*q, p)) < 1e-6)
        {
            out.push(p);
        }
    };
    for e in cx.kernel.edges(*body).unwrap_or_default() {
        if e.faces.contains(&pick.face) {
            push(e.a);
            push(e.b);
            push(k::scale(k::add(e.a, e.b), 0.5));
        }
    }
    if let Ok(mesh) = cx.kernel.mesh(*body, 0.05) {
        let plane = k::Plane::from_point_normal(f.point, f.normal);
        for c in inner_loop_centers(&mesh, &pick.face, &plane) {
            push(c);
        }
    }
    for id in cx.candidates("points") {
        if let Some(pts) = cx
            .output(FeatureId(id), "points")
            .and_then(Points::from_value)
        {
            for p in pts.points.values() {
                let w = pts.plane.to_world(*p);
                if on_plane(w) {
                    push(w);
                }
            }
        }
    }
    out
}

/// 面の縁の輪のうち、外側の輪を除いたもの(穴やボスの縁)の中心(輪の点の平均)。
fn inner_loop_centers(mesh: &k::Mesh, face: &str, plane: &k::Plane) -> Vec<[f64; 3]> {
    use std::collections::HashMap;
    let Some(fi) = mesh.face_names.iter().position(|n| n == face) else {
        return vec![];
    };
    type Key = (i64, i64, i64);
    let key = |i: u32| -> Key {
        let p = mesh.positions[i as usize];
        let r = |v: f32| (v as f64 * 1e4).round() as i64;
        (r(p[0]), r(p[1]), r(p[2]))
    };
    let mut uses: HashMap<(Key, Key), i32> = HashMap::new();
    for (t, _) in mesh
        .triangles
        .iter()
        .zip(&mesh.triangle_faces)
        .filter(|(_, f)| **f as usize == fi)
    {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ka, kb) = (key(a), key(b));
            *uses
                .entry(if ka <= kb { (ka, kb) } else { (kb, ka) })
                .or_default() += 1;
        }
    }
    // 縁の線分を、端点のつながりで輪に分ける
    let border: Vec<(Key, Key)> = uses
        .into_iter()
        .filter(|(_, n)| *n == 1)
        .map(|(k, _)| k)
        .collect();
    let mut next: HashMap<Key, Vec<Key>> = HashMap::new();
    for (a, b) in &border {
        next.entry(*a).or_default().push(*b);
        next.entry(*b).or_default().push(*a);
    }
    let mut seen = std::collections::HashSet::new();
    let mut loops: Vec<Vec<Key>> = Vec::new();
    for start in next.keys() {
        if !seen.insert(*start) {
            continue;
        }
        let mut lp = vec![*start];
        let mut stack = vec![*start];
        while let Some(p) = stack.pop() {
            for q in &next[&p] {
                if seen.insert(*q) {
                    lp.push(*q);
                    stack.push(*q);
                }
            }
        }
        loops.push(lp);
    }
    let to_p = |k: &Key| [k.0 as f64 * 1e-4, k.1 as f64 * 1e-4, k.2 as f64 * 1e-4];
    let span = |lp: &Vec<Key>| {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in lp.iter().map(to_p) {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        k::dot(k::sub(hi, lo), k::sub(hi, lo))
    };
    // いちばん大きい輪が外側
    let Some(outer) = (0..loops.len()).max_by(|a, b| span(&loops[*a]).total_cmp(&span(&loops[*b])))
    else {
        return vec![];
    };
    loops
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != outer)
        .filter_map(|(_, lp)| {
            let pts: Vec<[f64; 2]> = lp.iter().map(|q| plane.to_local(to_p(q))).collect();
            circle_center(&pts).map(|c| plane.to_world(c))
        })
        .collect()
}

/// 点の並びが円の上にあれば、その中心(面の上の座標)。円でなければ(ポケットの角など)None。
/// 頂点の平均だと、三角形の分け方が場所で粗い時(truck)に中心がずれるので、円をあてはめる
/// (x² + y² + D x + E y + F = 0 の最小二乗)。
fn circle_center(pts: &[[f64; 2]]) -> Option<[f64; 2]> {
    if pts.len() < 6 {
        return None;
    }
    // 桁落ちを避けるため、平均を原点にして解く
    let n = pts.len() as f64;
    let m = pts
        .iter()
        .fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
    let (mut a, mut b) = ([[0.0f64; 3]; 3], [0.0f64; 3]);
    for p in pts {
        let (x, y) = (p[0] - m[0], p[1] - m[1]);
        let row = [x, y, 1.0];
        let rhs = -(x * x + y * y);
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += row[i] * row[j];
            }
            b[i] += row[i] * rhs;
        }
    }
    let [d, e, f] = solve3(a, b)?;
    let c = [-d / 2.0, -e / 2.0];
    let r2 = c[0] * c[0] + c[1] * c[1] - f;
    if r2 <= 0.0 {
        return None;
    }
    let r = r2.sqrt();
    // 頂点はどれも円の上(丸めの 1e-4 mm 程度の差だけ)にあるはず
    let off = pts
        .iter()
        .map(|p| ((p[0] - m[0] - c[0]).hypot(p[1] - m[1] - c[1]) - r).abs())
        .fold(0.0, f64::max);
    (off < 1e-3 + 1e-3 * r).then_some([c[0] + m[0], c[1] + m[1]])
}

/// 3 × 3 の連立一次方程式(クラメルの公式)。
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(a);
    if d.abs() < 1e-12 {
        return None;
    }
    let mut x = [0.0; 3];
    for (k, xk) in x.iter_mut().enumerate() {
        let mut m = a;
        for i in 0..3 {
            m[i][k] = b[i];
        }
        *xk = det(m) / d;
    }
    Some(x)
}

/// この crate の名乗り(登録する種類と操作)。組み立て(jc-bundle)が 1 回だけ並べる。
pub fn module() -> jc_core::Module {
    jc_core::Module::new("jc-cad-hole", register)
        .with_extension(HoleCommands)
        .with_extension(ThreadCommands)
}
