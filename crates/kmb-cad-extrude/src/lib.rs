//! 押し出し(`solid.extrude@1`)と、スケッチを押し出す操作。
use kmb_cad::*;
use kmb_core::{
    BooleanOp, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, RefKind, Registry,
    ShapeHandle, Value,
};
use serde::{Deserialize, Serialize};

pub const EXTRUDE: &str = "solid.extrude@1";
/// 選んだスケッチを押し出す操作。
pub const EXTRUDE_SKETCH: &str = "cad.extrude";

/// スケッチの輪郭を押し出す。
/// 例: `{"profile": 1, "distance": "thickness", "operation": "cut", "target": 2}`
/// `regions` を書くと、スケッチの線で分けた領域のうち、その名前の領域だけを押し出す
/// (例: `"regions": ["e2.0+e7.1+e9.0+e10"]`。名前はスケッチの出力 `faces` にある)。
/// 向き `"two_sides"` は表へ `distance`、裏へ `distance2`。範囲 `"extent": "all"` は距離を使わず、
/// 対象のボディを向きの先まで全部貫く(切り取りで「貫通」にする時)。向きが `"two_sides"` の時、
/// 範囲は表側だけの止め方で、裏側は `extent2`(`"distance"` は `distance2`、`"all"` は裏側を全部貫く)。
/// 例: 表は貫通・裏は 5 mm `{"direction": "two_sides", "extent": "all", "extent2": "distance", "distance2": "5 mm"}`。範囲 `"extent": "to_face"` は
/// `to_face`(`{"feature": 5, "face": "f5:end"}`、機能の出力 `body` の面)の平面まで押し出す。
/// `to_offset` を書くと、その面からずらした所で止める
/// (正は面を越えて先へ、負は面の手前)。面の代わりに頂点(`{"feature": 5, "vertex": "…"}`)なら、
/// その点を通る輪郭と平行な平面まで。
///
/// 薄い押し出し `"thin": "center"`(`inside`・`outside`)は、輪郭の線を厚さ `thickness`(既定 2 mm)の
/// 壁にして押し出す。スケッチの全部を使う時は開いた線も壁にする。
/// 始まり `"start": "offset"` は、輪郭の平面から `start_offset` だけずらした所から押し出す。
/// テーパー `taper`(角度。正は先へ行くほど細くなる)は壁を傾ける(SMLib のカーネルだけ)。
/// `op_manual` は演算を手で選んだ印(画面が付ける。あれば演算をもう自動で替えない)。
///
/// 押し出す物 `"source": "faces"` は、スケッチの代わりにボディの平らな面 `faces` の縁を輪郭にする
/// (例: `{"source": "faces", "faces": {"kind": "face", "feature": 3, "face": "f3:end"}, "distance": "5 mm",
/// "operation": "join", "target": 3}`)。面はどれも同じボディの面にする。平面の違う面は、
/// 平面ごとにそれぞれの外向きの法線へ押し出す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extrude {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<FeatureId>,
    #[serde(default, skip_serializing_if = "Source::is_sketch")]
    pub source: Source,
    /// 押し出す物が平らな面の時の面(1 つなら値、2 つ以上は並び)
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub faces: Vec<kmb_core::Ref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<String>,
    pub distance: String,
    #[serde(default)]
    pub direction: Direction,
    /// 裏側の距離(向きが `two_sides` の時)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance2: Option<String>,
    #[serde(default, skip_serializing_if = "Extent::is_distance")]
    pub extent: Extent,
    /// 向きが「両方向」の時の裏側の範囲。無ければ距離(前の版の「両方向・すべて」は両側とも
    /// すべてだったので、範囲が「すべて」なら裏側もすべて。[`Extrude::back_extent`])
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent2: Option<BackExtent>,
    /// 範囲が「対象まで」の時の面
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_face: Option<ToFace>,
    /// 範囲が「対象まで」の時の、面からのずらし(正は面を越えて先へ、負は面の手前で止める)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_offset: Option<String>,
    #[serde(default)]
    pub operation: Operation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<FeatureId>,
    /// 演算を手で選んだ(もう自動で替えない)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub op_manual: bool,
    #[serde(default, skip_serializing_if = "Thin::is_none")]
    pub thin: Thin,
    /// 薄い押し出しの壁の厚さ(空なら 2 mm)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<String>,
    #[serde(default, skip_serializing_if = "Start::is_profile")]
    pub start: Start,
    /// 始まりが「オフセット」の時の、輪郭の平面からのずらし(法線の向きが正)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<String>,
    /// 壁の傾き(角度。空か 0 ならまっすぐ)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<String>,
}

/// 薄い押し出しの壁の付け方(使わない、線の真ん中、内側、外側)。開いた線の内側はたどる向きの左。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Thin {
    #[default]
    None,
    Center,
    Inside,
    Outside,
}

impl Thin {
    fn is_none(&self) -> bool {
        *self == Thin::None
    }

    fn wall(self) -> Option<kmb_cad::thin::Wall> {
        Some(match self {
            Thin::None => return None,
            Thin::Center => kmb_cad::thin::Wall::Center,
            Thin::Inside => kmb_cad::thin::Wall::Inside,
            Thin::Outside => kmb_cad::thin::Wall::Outside,
        })
    }
}

/// 押し出しの始まり(輪郭の平面か、そこからずらした所)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    #[default]
    Profile,
    Offset,
}

impl Start {
    fn is_profile(&self) -> bool {
        *self == Start::Profile
    }
}

/// 薄い押し出しの壁の厚さの既定。
const THICKNESS: f64 = 2.0;

/// 押し出す物: スケッチの輪郭か、ボディの平らな面か。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Sketch,
    Faces,
}

impl Source {
    fn is_sketch(&self) -> bool {
        *self == Source::Sketch
    }
}

impl Default for Extrude {
    fn default() -> Self {
        Extrude {
            profile: None,
            source: Source::Sketch,
            faces: Vec::new(),
            regions: Vec::new(),
            distance: "10 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Extent::Distance,
            extent2: None,
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            op_manual: false,
            thin: Thin::None,
            thickness: None,
            start: Start::Profile,
            start_offset: None,
            taper: None,
        }
    }
}

/// 向きが「両方向」の時の、裏側の止め方(距離か、対象のボディを全部貫くか)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackExtent {
    Distance,
    All,
}

/// 前の版のデータを今の形にする: 裏側の範囲が無く範囲が「すべて」なら、裏側も「すべて」と書く
/// (前の版は両側とも貫いていた。欄に見えている値と効いている値をそろえる)。
fn upgrade(d: &mut serde_json::Value) {
    if let Some(o) = d.as_object_mut()
        && o.get("extent").and_then(|v| v.as_str()) == Some("all")
    {
        o.entry("extent2").or_insert_with(|| "all".into());
    }
}

/// 押し出しの範囲の「対象まで」の面(機能 `feature` の出力 `body` の面 `face`)か頂点 `vertex`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToFace {
    pub feature: FeatureId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub face: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertex: Option<String>,
}

impl ToFace {
    /// 面か頂点。
    fn pick(&self) -> kmb_cad::stop_at::StopPick {
        match &self.vertex {
            Some(v) => kmb_cad::stop_at::StopPick::Vertex(v.clone()),
            None => kmb_cad::stop_at::StopPick::Face(self.face.clone()),
        }
    }
}

impl Exprs for Extrude {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut v = vec![&mut self.distance];
        v.extend(self.distance2.as_mut());
        v.extend(self.to_offset.as_mut());
        v.extend(self.thickness.as_mut());
        v.extend(self.start_offset.as_mut());
        v.extend(self.taper.as_mut());
        v
    }
}

impl Extrude {
    /// 距離で決める範囲(平面からの法線の向きの区間)。
    fn range(&self, ctx: &EvalCtx<'_>) -> Result<(f64, f64), EvalError> {
        let d = self.front_distance(ctx)?;
        Ok(match self.direction {
            Direction::Normal => (0.0, d),
            Direction::Reverse => (-d, 0.0),
            Direction::Symmetric => (-d / 2.0, d / 2.0),
            Direction::TwoSides => (-self.back_distance(ctx)?, d),
        })
    }

    /// 距離(表側)。
    fn front_distance(&self, ctx: &EvalCtx<'_>) -> Result<f64, EvalError> {
        let d = length(ctx, &self.distance, "距離")?;
        if d <= 0.0 {
            return Err(EvalError::InvalidData(
                "距離は 0 より大きくしてください".into(),
            ));
        }
        Ok(d)
    }

    /// 裏側の距離(無ければ 0)。
    fn back_distance(&self, ctx: &EvalCtx<'_>) -> Result<f64, EvalError> {
        let d2 = match &self.distance2 {
            Some(src) => length(ctx, src, "裏側の距離")?,
            None => 0.0,
        };
        if d2 < 0.0 {
            return Err(EvalError::InvalidData(
                "裏側の距離は 0 以上にしてください".into(),
            ));
        }
        Ok(d2)
    }

    /// 向きが「両方向」の時の裏側の範囲(書いていなければ [`upgrade`] と同じ決め方)。
    pub fn back_extent(&self) -> BackExtent {
        self.extent2.unwrap_or(if self.extent == Extent::All {
            BackExtent::All
        } else {
            BackExtent::Distance
        })
    }

    /// 向きが「両方向」の時の範囲: 表側は範囲(`extent`)、裏側は裏側の範囲で決め、2 つを合わせる。
    /// 「対象まで」の面がまだ無い間は、表側を距離で作って注意を足す。
    fn range_two_sides(
        &self,
        ctx: &EvalCtx<'_>,
        profile: &kmb_core::Profile,
        warnings: &mut Vec<String>,
    ) -> Result<((f64, f64), Option<StopPlane>), EvalError> {
        let ((from, to), stop) = match self.extent {
            Extent::Distance => ((0.0, self.front_distance(ctx)?), None),
            Extent::ToFace if self.to_face.is_none() => {
                warnings.push(NO_FACE_YET.into());
                ((0.0, self.front_distance(ctx)?), None)
            }
            Extent::ToFace => self.range_to_face(ctx, profile)?,
            Extent::All => {
                let s = self.span(ctx, profile, "範囲")?;
                // 後ろにボディが無ければ後ろへも少し延ばす(range_all と同じ訳)
                let lo = if s.lo >= -s.eps { -s.margin } else { 0.0 };
                ((lo, s.hi.max(0.0) + s.margin), None)
            }
        };
        let (lo, hi) = match self.back_extent() {
            BackExtent::Distance => (-self.back_distance(ctx)?, 0.0),
            BackExtent::All => {
                let s = self.span(ctx, profile, "裏側の範囲")?;
                let hi = if s.hi <= s.eps { s.margin } else { 0.0 };
                (s.lo.min(0.0) - s.margin, hi)
            }
        };
        Ok(((from.min(lo), to.max(hi)), stop))
    }

    /// 「すべて」の範囲: 対象のボディの外形の箱を法線へ映した区間を、向きの先まで(少し余して)。
    fn range_all(
        &self,
        ctx: &EvalCtx<'_>,
        profile: &kmb_core::Profile,
    ) -> Result<(f64, f64), EvalError> {
        let Span {
            lo: a,
            hi: b,
            margin,
            eps,
        } = self.span(ctx, profile, "範囲")?;
        let none = || EvalError::InvalidData("押し出す向きの先に対象のボディがありません".into());
        // 平面の後ろにボディが無い時(面の上のスケッチ)は、後ろへも少し延ばす。始まりの面が
        // ボディの面と重なるとカーネルによってはブーリアンに失敗するため(truck で実測)。
        // 後ろにもボディがある時は延ばさない(後ろの材料まで削らないように)
        Ok(match self.direction {
            Direction::Normal if b > eps => (if a >= -eps { -margin } else { 0.0 }, b + margin),
            Direction::Reverse if a < -eps => (a - margin, if b <= eps { margin } else { 0.0 }),
            Direction::Normal | Direction::Reverse => return Err(none()),
            Direction::Symmetric | Direction::TwoSides => {
                (a.min(0.0) - margin, b.max(0.0) + margin)
            }
        })
    }

    /// 対象のボディを輪郭の平面の法線へ映した区間と、その余しと誤差の幅。
    /// `what` は「すべて」を選んだ欄の名前(誤りの文に使う)。
    fn span(
        &self,
        ctx: &EvalCtx<'_>,
        profile: &kmb_core::Profile,
        what: &str,
    ) -> Result<Span, EvalError> {
        let t = match (self.operation, self.target) {
            (Operation::New, _) | (_, None) => {
                return Err(EvalError::InvalidData(format!(
                    "{what}「すべて」は、切り取り・結合・交差で対象のボディがある時だけ使えます"
                )));
            }
            (_, Some(t)) => t,
        };
        let body = body_of(ctx, t)?;
        let n = profile.plane.normal();
        let o = profile.plane.origin;
        let along = |c: [f64; 3]| (0..3).map(|k| (c[k] - o[k]) * n[k]).sum::<f64>();
        let (mut a, mut b) = (f64::INFINITY, f64::NEG_INFINITY);
        // 網の頂点を法線に映す(外形の箱は truck で 1 mm 以上の余白が付くことがあり、平面の後ろに
        // ボディがあると見誤る)。網が作れなければ外形の箱の角で
        match ctx.kernel().mesh(body, 0.1) {
            Ok(m) if !m.positions.is_empty() => {
                for p in &m.positions {
                    let s = along(p.map(f64::from));
                    a = a.min(s);
                    b = b.max(s);
                }
            }
            _ => {
                let (lo, hi) = ctx.kernel().bounds(body).map_err(kernel_err)?;
                for i in 0..8 {
                    let s = along([
                        if i & 1 == 0 { lo[0] } else { hi[0] },
                        if i & 2 == 0 { lo[1] } else { hi[1] },
                        if i & 4 == 0 { lo[2] } else { hi[2] },
                    ]);
                    a = a.min(s);
                    b = b.max(s);
                }
            }
        }
        Ok(Span {
            lo: a,
            hi: b,
            margin: 1.0 + (b - a).abs() * 0.01,
            // 網や外形の箱の誤差があるので、平面に接するだけのボディは「先にない」とみなす
            eps: 1e-3 * (b - a).abs().max(1.0),
        })
    }
}

/// 対象のボディを輪郭の平面の法線へ映した区間 `lo`〜`hi`、先へ余す長さ、接するだけとみなす幅。
struct Span {
    lo: f64,
    hi: f64,
    margin: f64,
    eps: f64,
}

/// 「対象まで」の面がまだ無い間の注意。
const NO_FACE_YET: &str = "対象の面か頂点をまだ選んでいません(選ぶまでは距離で押し出します)";

impl Extrude {
    /// 「対象まで」の範囲: スケッチの平面から、選んだ平らな面の平面まで(面の側へ)。
    /// 面からのずらしがあれば、その分だけ先へ延ばす(負なら手前で止める)。面が輪郭と平行でなければ、
    /// 範囲はいちばん遠い所まで延ばし、道具の終わりをその面で切る(切る平面も返す)。
    fn range_to_face(
        &self,
        ctx: &EvalCtx<'_>,
        profile: &kmb_core::Profile,
    ) -> Result<((f64, f64), Option<StopPlane>), EvalError> {
        let t = self.to_face.as_ref().ok_or_else(|| {
            EvalError::InvalidData(
                "対象の面をまだ選んでいません。「対象の面までにする」を押して、平らな面をクリックしてください"
                    .into(),
            )
        })?;
        let body = body_of(ctx, t.feature)?;
        let s =
            face_stop(ctx.kernel(), body, &t.pick(), profile).map_err(EvalError::InvalidData)?;
        let off = match &self.to_offset {
            Some(src) if !src.trim().is_empty() => length(ctx, src, "面からのオフセット")?,
            _ => 0.0,
        };
        let sign = s.near.signum();
        let near = s.near + sign * off;
        if near * s.near <= 0.0 || near.abs() < 1e-6 {
            return Err(EvalError::InvalidData(format!(
                "面からのずらしが大きすぎます(面までは {:.2} mm。手前へのずらしはそれより小さくしてください)",
                s.near.abs()
            )));
        }
        let Some(stop) = s.stop else {
            return Ok((if near > 0.0 { (0.0, near) } else { (near, 0.0) }, None));
        };
        // 斜めの面: ずらしは押し出しの向きに面を動かす。いちばん遠い所より少し先まで押し出して切る
        let n = profile.plane.normal();
        let stop = StopPlane {
            point: [0, 1, 2].map(|i| stop.point[i] + n[i] * sign * off),
            normal: stop.normal,
        };
        let far = s.far + sign * off;
        let end = far + sign * (1.0 + far.abs() * 0.01);
        Ok((if end > 0.0 { (0.0, end) } else { (end, 0.0) }, Some(stop)))
    }
}

/// 押し出しを止める面までの距離(スケッチの平面から法線の向き、符号つき)。平行な面なら
/// `near` と `far` は同じで `stop` はない。斜めの面なら、輪郭の点のうちいちばん近い所と遠い所までと、
/// 道具を切る平面(捨てる側を向く法線)。
#[derive(Debug, Clone, Copy)]
struct FaceStop {
    near: f64,
    far: f64,
    stop: Option<StopPlane>,
}

/// スケッチの平面から、ボディ `body` の平らな面の平面か、頂点を通る輪郭と平行な平面まで。
fn face_stop(
    kernel: &dyn kmb_core::Kernel,
    body: kmb_core::ShapeHandle,
    pick: &kmb_cad::stop_at::StopPick,
    profile: &kmb_core::Profile,
) -> Result<FaceStop, String> {
    let n = profile.plane.normal();
    let face = match pick {
        kmb_cad::stop_at::StopPick::Face(f) => f.as_str(),
        kmb_cad::stop_at::StopPick::Vertex(v) => {
            let p = kmb_cad::stop_at::vertex_at(kernel, body, v)?;
            let d = (0..3)
                .map(|i| (p[i] - profile.plane.origin[i]) * n[i])
                .sum::<f64>();
            if d.abs() < 1e-6 {
                return Err(
                    "対象の頂点がスケッチと同じ平面にあります。別の頂点か面を選んでください".into(),
                );
            }
            return Ok(FaceStop {
                near: d,
                far: d,
                stop: None,
            });
        }
    };
    let face = kernel
        .faces(body)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|f| f.name == face)
        .ok_or_else(|| {
            format!(
                "対象の面 {face} が見つかりません(形が変わって面が無くなった時は、選び直してください)"
            )
        })?;
    let plane = face
        .planar
        .ok_or("対象の面が平らではありません(平らな面か頂点だけ使えます)")?;
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let len = dot(plane.normal, plane.normal).sqrt().max(1e-12);
    let m = plane.normal.map(|v| v / len);
    let c = dot(m, n);
    let o = profile.plane.origin;
    if c.abs() > 1.0 - 1e-6 {
        let d = dot(
            [
                plane.point[0] - o[0],
                plane.point[1] - o[1],
                plane.point[2] - o[2],
            ],
            n,
        );
        if d.abs() < 1e-6 {
            return Err("対象の面がスケッチと同じ平面にあります。別の面を選んでください".into());
        }
        return Ok(FaceStop {
            near: d,
            far: d,
            stop: None,
        });
    }
    if c.abs() < 1e-3 {
        return Err(
            "対象の面が押し出す向きと平行なので、そこまで届きません。押し出す向きに向かい合う面を選んでください"
                .into(),
        );
    }
    // 輪郭の点ごとに、法線の向きに進んで面に当たるまでの距離
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for l in outline(profile) {
        for p in l {
            let t = dot([0, 1, 2].map(|i| plane.point[i] - p[i]), m) / c;
            lo = lo.min(t);
            hi = hi.max(t);
        }
    }
    if !lo.is_finite() || lo * hi <= 1e-12 {
        return Err(
            "対象の面(を広げた平面)がスケッチの輪郭と交わっています。輪郭の全体が面の片側にある時だけ使えます"
                .into(),
        );
    }
    let (near, far) = if lo > 0.0 { (lo, hi) } else { (hi, lo) };
    // 捨てる側: 押し出す向きに面を越えた側
    let ahead = if c * far.signum() > 0.0 { 1.0 } else { -1.0 };
    Ok(FaceStop {
        near,
        far,
        stop: Some(StopPlane {
            point: plane.point,
            normal: m.map(|v| v * ahead),
        }),
    })
}

/// 輪郭の領域ごとの中の点(平面の座標)。
fn inner_points(profile: &kmb_core::Profile) -> Vec<kmb_core::kernel::P2> {
    profile
        .regions
        .iter()
        .filter_map(regions::inner_point)
        .collect()
}

/// スケッチの平面から法線の向きに `d`(符号つき)まで押し出した時、その途中が対象のボディの
/// 中を通るか(押し出しの区間の真ん中の高さで、輪郭の中の点がボディの中にあるか)。
fn goes_into(
    kernel: &dyn kmb_core::Kernel,
    body: kmb_core::ShapeHandle,
    profile: &kmb_core::Profile,
    d: f64,
) -> bool {
    let Ok(mesh) = kernel.mesh(body, 0.05) else {
        return false;
    };
    let n = profile.plane.normal();
    inner_points(profile).into_iter().any(|q| {
        let p = profile.plane.to_world(q);
        mesh.contains([0, 1, 2].map(|i| p[i] + n[i] * d / 2.0))
    })
}

/// 1 つの平面の道具の輪郭(薄い押し出しなら壁ごと)。平らな面の時は、その平面の面の名前と、
/// 縁を近い形にした注意。
struct Plan {
    profiles: Vec<kmb_core::Profile>,
    warn: Option<String>,
    faces: Vec<String>,
}

/// 1 つの平面の道具の作り方・下絵と、面の縁の曲線のまま押し出した道具。
struct Built {
    recipe: Recipe,
    preview: kmb_core::ToolPreview,
    exact: Option<ShapeHandle>,
}

impl Extrude {
    /// 押し出す物が平らな面の時の、面を持つボディの機能と面の名前(どれも同じボディの面)。
    fn face_source(&self) -> Result<(FeatureId, Vec<String>), EvalError> {
        let bad = |m: &str| EvalError::InvalidData(m.into());
        let mut feature = None;
        let mut names = Vec::new();
        for r in &self.faces {
            let kmb_core::Ref::Face { feature: f, face } = r else {
                return Err(bad("押し出す物には平らな面だけを選んでください"));
            };
            if *feature.get_or_insert(*f) != *f {
                return Err(bad(
                    "押し出す面は 1 つのボディから選んでください(別のボディの面が混ざっています)",
                ));
            }
            names.push(face.clone());
        }
        let feature = feature.ok_or_else(|| bad("押し出す平らな面を選んでください"))?;
        Ok((feature, names))
    }

    /// スケッチを押し出す時のスケッチ。
    fn sketch(&self) -> Result<FeatureId, EvalError> {
        self.profile
            .ok_or_else(|| EvalError::InvalidData("押し出すスケッチ(輪郭)を選んでください".into()))
    }

    /// 押し出す輪郭(スケッチの輪郭か、平らな面の縁)。平らな面は平面ごとに 1 つ。面の縁を
    /// 近い形にした組は注意と面の名前を返す(面の縁の曲線のまま押し出す道具に使う)。
    fn profiles(&self, ctx: &EvalCtx<'_>) -> Result<Vec<Plan>, EvalError> {
        if self.source == Source::Faces {
            let (feature, names) = self.face_source()?;
            let body = body_of(ctx, feature)?;
            let groups = kmb_cad::face_profiles(ctx.kernel(), body, &names)
                .map_err(EvalError::InvalidData)?;
            return Ok(groups
                .into_iter()
                .map(|g| Plan {
                    profiles: vec![g.profile.profile],
                    warn: g.profile.approximated.then(|| {
                        "面の縁に直線と円弧でない線があるので、細かい折れ線で近い形にして押し出しました"
                            .to_string()
                    }),
                    faces: g.faces,
                })
                .collect());
        }
        let profile = self.sketch()?;
        let p = if self.regions.is_empty() {
            profile_from(ctx, profile)?
        } else {
            kmb_core::ProfileFaces::from_value(ctx.output(profile, "faces")?)
                .ok_or_else(|| {
                    EvalError::InvalidData(format!(
                        "{profile} は領域を選べるスケッチではありません"
                    ))
                })?
                .profile(&self.regions)
                .map_err(EvalError::InvalidData)?
        };
        Ok(vec![Plan {
            profiles: vec![p],
            warn: None,
            faces: Vec::new(),
        }])
    }

    /// 面の縁の曲線のまま押し出した道具(始まりのずらしを入れる)。カーネルが対応していなければ無し。
    fn face_tool(
        &self,
        ctx: &EvalCtx<'_>,
        names: &[String],
        from: f64,
        to: f64,
    ) -> Result<Option<ShapeHandle>, EvalError> {
        let (feature, _) = self.face_source()?;
        let body = body_of(ctx, feature)?;
        let s = self.start_offset(ctx)?;
        match ctx
            .kernel()
            .extrude_faces(body, names, from + s, to + s, &tag(ctx))
        {
            Ok(t) => Ok(Some(t)),
            Err(kmb_core::KernelError::Unsupported(_)) => Ok(None),
            Err(e) => Err(kernel_err(e)),
        }
    }

    /// 薄い押し出しで、スケッチの線(開いた線も)を使う時か。
    fn thin_from_curves(&self) -> bool {
        self.thin != Thin::None && self.source == Source::Sketch && self.regions.is_empty()
    }

    /// 押し出す道具の輪郭を平面ごとに(薄い押し出しで線が 2 本以上なら線ごと)。始まりのずらしを入れた後。
    fn tool_plans(&self, ctx: &EvalCtx<'_>) -> Result<Vec<Plan>, EvalError> {
        let mut plans = if self.thin_from_curves() {
            vec![Plan {
                profiles: Vec::new(),
                warn: None,
                faces: Vec::new(),
            }]
        } else {
            self.profiles(ctx)?
        };
        let s = self.start_offset(ctx)?;
        for plan in &mut plans {
            plan.profiles = self.tool_profiles(ctx, std::mem::take(&mut plan.profiles))?;
            if s != 0.0 {
                for p in &mut plan.profiles {
                    p.plane = p.plane.offset(s);
                }
            }
        }
        Ok(plans)
    }

    /// 1 つの平面の道具の輪郭 `given`(薄い押し出しの線を使う時は空)を、薄い押し出しなら壁にする。
    fn tool_profiles(
        &self,
        ctx: &EvalCtx<'_>,
        given: Vec<kmb_core::Profile>,
    ) -> Result<Vec<kmb_core::Profile>, EvalError> {
        let bad = |m: String| EvalError::InvalidData(m);
        let out = match self.thin.wall() {
            Some(w) if self.thin_from_curves() => {
                let sketch = self.sketch()?;
                let curves = kmb_core::Curves::from_value(ctx.output(sketch, "curves")?)
                    .ok_or_else(|| bad(format!("{sketch} は線のあるスケッチではありません")))?;
                let chains = kmb_cad::thin::chains(&curves.segments);
                let t = self.thickness(ctx)?;
                kmb_cad::thin::walls(curves.plane, &chains, t, w).map_err(bad)?
            }
            wall => {
                let p = given
                    .into_iter()
                    .next()
                    .ok_or_else(|| bad("押し出す輪郭がありません".into()))?;
                if p.regions.is_empty() {
                    return Err(bad("スケッチに閉じた輪郭がありません".into()));
                }
                match wall {
                    None => vec![p],
                    Some(w) => {
                        // 領域の縁(外周と穴)を壁にする。穴の縁の「内側」は領域の材料の側
                        let t = self.thickness(ctx)?;
                        let mut v = Vec::new();
                        for r in &p.regions {
                            for (k, l) in std::iter::once(&r.outer).chain(&r.holes).enumerate() {
                                let side = match (k > 0, w) {
                                    (true, kmb_cad::thin::Wall::Inside) => {
                                        kmb_cad::thin::Wall::Outside
                                    }
                                    (true, kmb_cad::thin::Wall::Outside) => {
                                        kmb_cad::thin::Wall::Inside
                                    }
                                    _ => w,
                                };
                                let c = kmb_cad::thin::loop_chain(l);
                                v.push(kmb_core::Profile {
                                    plane: p.plane,
                                    regions: kmb_cad::thin::wall(
                                        &c,
                                        t,
                                        side,
                                        &format!("w{}", v.len()),
                                    )
                                    .map_err(bad)?,
                                });
                            }
                        }
                        v
                    }
                }
            }
        };
        Ok(out)
    }

    /// 1 つの平面の道具: 範囲を決め、作り方・下絵と、面の縁の曲線のまま押し出した道具(使える時)を作る。
    /// 注意は `warnings` へ足す。
    fn build(
        &self,
        ctx: &EvalCtx<'_>,
        plan: Plan,
        taper: f64,
        warnings: &mut Vec<String>,
    ) -> Result<Built, EvalError> {
        let Plan {
            profiles,
            warn,
            faces,
        } = plan;
        // 範囲と下絵は、道具の輪郭の全部を合わせた物で決める(薄い押し出しの線ごとの壁も同じ平面)
        let profile = kmb_core::Profile {
            plane: profiles[0].plane,
            regions: profiles.iter().flat_map(|p| p.regions.clone()).collect(),
        };
        let mut notes = Vec::new();
        let ((from, to), stop) = match self.extent {
            _ if self.direction == Direction::TwoSides => {
                self.range_two_sides(ctx, &profile, &mut notes)?
            }
            Extent::Distance => (self.range(ctx)?, None),
            Extent::All => (self.range_all(ctx, &profile)?, None),
            // 「対象まで」にした直後で面がまだ無い間は、距離のまま作る(形と道具の箱が消えないように)
            Extent::ToFace if self.to_face.is_none() => {
                notes.push(NO_FACE_YET.into());
                (self.range(ctx)?, None)
            }
            Extent::ToFace => self.range_to_face(ctx, &profile)?,
        };
        let loops = outline(&profile);
        let c = centroid(&profile);
        let n = profile.plane.normal();
        let arrows = [to, from]
            .into_iter()
            .filter(|t| t.abs() > 1e-9)
            .map(|t| [c, [0, 1, 2].map(|i| c[i] + n[i] * t)])
            .collect();
        let cap = kmb_cad::prism::PrismCap::new(&loops, profile.plane.normal());
        // テーパーは予告の時から傾いた壁で見せる(できる形と同じ寄せ方)
        let lean = taper.tan();
        let preview = kmb_core::ToolPreview {
            op: kmb_core::ToolOp::from_boolean(self.operation.boolean()),
            lines: if taper != 0.0 && stop.is_none() {
                tapered_lines(&cap, from, to, lean)
            } else {
                tool_lines(&profile, from, to, stop.as_ref())
            },
            faces: match &stop {
                Some(s) => cap.solid_to_plane(0.0, s.point, s.normal),
                None if taper != 0.0 => cap.solid_tapered(from, to, lean),
                None => cap.solid(from, to),
            },
            profile: loops,
            arrows,
        };
        let part = |profile: kmb_core::Profile| Recipe::Extrude {
            operation: self.operation,
            profile,
            from,
            to,
            through: false,
            stop,
            taper,
        };
        let recipe = if profiles.len() == 1 {
            part(profile)
        } else {
            // 薄い押し出しの線ごとの壁は重なることがあるので、1 つずつ効かせる(新しいボディなら足し合わせる)
            Recipe::Many {
                parts: profiles.into_iter().map(part).collect(),
            }
        };
        // 縁を近い形にした面は、面の縁の曲線のまま押し出した道具を使う(近い形の壁はもとの辺から
        // わずかにずれ、結合も切り取りも壊れた形になっていた)
        let exact = if warn.is_some()
            && self.source == Source::Faces
            && self.thin == Thin::None
            && taper == 0.0
            && stop.is_none()
        {
            self.face_tool(ctx, &faces, from, to)?
        } else {
            None
        };
        // 近い形にした注意は先頭に(面の縁のまま押し出した時は出さない)
        if exact.is_none() {
            warnings.extend(warn);
        }
        warnings.extend(notes);
        Ok(Built {
            recipe,
            preview,
            exact,
        })
    }

    /// 薄い押し出しの壁の厚さ。
    fn thickness(&self, ctx: &EvalCtx<'_>) -> Result<f64, EvalError> {
        let t = match self.thickness.as_deref().map(str::trim) {
            Some(src) if !src.is_empty() => length(ctx, src, "壁の厚さ")?,
            _ => THICKNESS,
        };
        if t <= 0.0 {
            return Err(EvalError::InvalidData(
                "壁の厚さは 0 より大きくしてください".into(),
            ));
        }
        Ok(t)
    }

    /// 始まりのずらし(始まりが「プロファイル平面」なら 0)。
    fn start_offset(&self, ctx: &EvalCtx<'_>) -> Result<f64, EvalError> {
        match (self.start, self.start_offset.as_deref().map(str::trim)) {
            (Start::Offset, Some(src)) if !src.is_empty() => length(ctx, src, "開始のオフセット"),
            _ => Ok(0.0),
        }
    }

    /// 壁の傾き(ラジアン)。
    fn taper(&self, ctx: &EvalCtx<'_>) -> Result<f64, EvalError> {
        let a = match self.taper.as_deref().map(str::trim) {
            Some(src) if !src.is_empty() => angle(ctx, src, "テーパー角度")?,
            _ => return Ok(0.0),
        };
        if a.abs() >= 89f64.to_radians() {
            return Err(EvalError::InvalidData(
                "テーパー角は -89 度から 89 度の間にしてください".into(),
            ));
        }
        Ok(a)
    }
}

impl Feature for Extrude {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = match self.source {
            Source::Sketch if self.thin_from_curves() => vec![output(self.sketch()?, "curves")],
            Source::Sketch => vec![output(self.sketch()?, "profile")],
            Source::Faces => vec![output(self.face_source()?.0, "body")],
        };
        if self.extent == Extent::ToFace
            && let Some(t) = &self.to_face
        {
            v.push(output(t.feature, "body"));
        }
        if self.source == Source::Sketch && !self.regions.is_empty() {
            v.push(output(self.sketch()?, "faces"));
        }
        v.extend(target_inputs(self.operation, self.target)?);
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        keep_target(self.operation, self.target)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let plans = self.tool_plans(ctx)?;
        let taper = self.taper(ctx)?;
        let mut warnings: Vec<String> = Vec::new();
        let mut built = Vec::new();
        for plan in plans {
            built.push(self.build(ctx, plan, taper, &mut warnings)?);
        }
        // 平面ごとの道具を 1 つずつ効かせる(新しいボディなら足し合わせる)
        let op = self.operation;
        let mut body = match self.target {
            Some(t) if op != Operation::New => Some(body_of(ctx, t)?),
            _ => None,
        };
        let kernel = ctx.kernel();
        let tag = tag(ctx);
        let union = |a, b| kernel.boolean(BooleanOp::Union, a, b).map_err(kernel_err);
        for b in &built {
            let tool = match b.exact {
                Some(tool) => tool,
                None if op == Operation::New => apply_recipe(ctx, &b.recipe, None, &tag)?,
                None => {
                    body = Some(apply_recipe(ctx, &b.recipe, body, &tag)?);
                    continue;
                }
            };
            body = Some(match (op.boolean(), body) {
                (None, None) => tool,
                (None, Some(prev)) => union(prev, tool)?,
                (Some(bo), Some(t)) => kernel.boolean(bo, t, tool).map_err(kernel_err)?,
                (Some(_), None) => {
                    return Err(EvalError::InvalidData(
                        "結合・切り取り・交差には対象のボディ(target)が必要です".into(),
                    ));
                }
            });
        }
        let body = body.ok_or_else(|| EvalError::InvalidData("道具がありません".into()))?;
        if op == Operation::Join
            && let Some(t) = self.target
            && let Some(w) = kmb_cad::detached_join(kernel, body_of(ctx, t)?, body)
        {
            warnings.push(w);
        }
        let mut preview: Option<kmb_core::ToolPreview> = None;
        let mut parts = Vec::new();
        for b in built {
            match &mut preview {
                None => preview = Some(b.preview),
                Some(p) => {
                    p.lines.extend(b.preview.lines);
                    p.faces.extend(b.preview.faces);
                    p.profile.extend(b.preview.profile);
                    p.arrows.extend(b.preview.arrows);
                }
            }
            parts.push(b.recipe);
        }
        let recipe = if parts.len() == 1 {
            parts.remove(0)
        } else {
            Recipe::Many {
                parts: parts
                    .into_iter()
                    .flat_map(|p| match p {
                        Recipe::Many { parts } => parts,
                        p => vec![p],
                    })
                    .collect(),
            }
        };
        let mut out: Outputs = [
            ("body".to_string(), Value::Shape(body)),
            ("recipe".to_string(), kmb_core::Payload::to_value(&recipe)),
        ]
        .into();
        if let Some(preview) = preview {
            out.insert(
                kmb_core::TOOL_PREVIEW.into(),
                kmb_core::Payload::to_value(&preview),
            );
        }
        let mut seen = std::collections::HashSet::new();
        warnings.retain(|w| seen.insert(w.clone()));
        if !warnings.is_empty() {
            kmb_core::add_warnings(&mut out, warnings);
        }
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(EXTRUDE, "押し出し")
            .icon("extrude")
            .hint("スケッチの輪郭か、ボディの平らな面を押し出す")
            .aliases("おしだし extrude")
            .field(
                Field::choice(
                    "source",
                    "押し出す物",
                    &[("sketch", "スケッチのプロファイル"), ("faces", "平らな面")],
                )
                .help("「平らな面」はボディの平らな面の縁を輪郭にする(面を選んでから押し出しを押しても同じ)"),
            )
            .field(Field::output("profile", "プロファイル", "profile").only("source", "sketch", "sketch"))
            .field(
                Field::pick("faces", "面", &[RefKind::PlanarFace], 1, None)
                    .only("source", "faces", "sketch")
                    .help("押し出す平らな面(1 つのボディの面をいくつでも。平面の違う面は、それぞれの面の向きへ押し出す)。もう一度クリックすると外す"),
            )
            .field(
                Field::choice(
                    "thin",
                    "薄い押し出し",
                    &[
                        ("none", "使わない"),
                        ("center", "壁を線の真ん中に"),
                        ("inside", "壁を内側に(開いた線は左)"),
                        ("outside", "壁を外側に(開いた線は右)"),
                    ],
                )
                .help("輪郭の線を壁の厚さで太らせて押し出す。スケッチの全部を使う時は、開いた線も壁にする"),
            )
            .field(
                Field::opt_expr("thickness", "壁の厚さ").length()
                    .unless("thin", "none")
                    .help("空なら 2 mm"),
            )
            .field(
                Field::choice(
                    "start",
                    "開始",
                    &[("profile", "プロファイル平面"), ("offset", "オフセット")],
                )
                .help("「オフセット」にすると、輪郭の平面から法線の向きにずらした所から押し出す"),
            )
            .field(
                Field::opt_expr("start_offset", "開始のオフセット").length()
                    .only("start", "offset", "profile")
                    .help("輪郭の平面から始まりまで(法線の向きが正、逆向きは負)"),
            )
            .field(Field::choice(
                "extent",
                "範囲",
                &[
                    ("distance", "距離"),
                    ("all", "すべて"),
                    ("to_face", "対象まで"),
                ],
            )
            .help("「対象まで」にすると、止める平らな面か頂点をクリックする手順に入る。向きが「両方向」の時は表側の止め方(裏側は「裏側の範囲」)")
            .pick_on("to_face", EXTRUDE_TO_FACE))
            // 「すべて」の時も距離の値は残す(隠すと値が消え、距離へ戻した時に打ち直しになる)
            .field(
                Field::expr("distance", "距離").length()
                    .help("範囲が「すべて」「対象まで」の時は使わない"),
            )
            .field(
                Field::info("to_face.face", "対象の面")
                    .help("「対象の面までにする」を押して、平らな面か頂点をクリックして選ぶ"),
            )
            .field(
                Field::info("to_face.vertex", "対象の頂点")
                    .help("頂点を選んだ時は、その点を通る、輪郭と平行な平面まで押し出す"),
            )
            .field(
                Field::opt_expr("to_offset", "面からのオフセット").length()
                    .only("extent", "to_face", "distance")
                    .help("空なら面でちょうど止める。正は面を越えて先へ、負は面の手前で止める"),
            )
            .field(Field::choice(
                "direction",
                "方向",
                &[
                    ("normal", "法線の向き"),
                    ("reverse", "逆向き"),
                    ("symmetric", "対称"),
                    ("two_sides", "両方向"),
                ],
            ))
            .field(
                Field::opt_expr("distance2", "裏側の距離").length()
                    .only("direction", "two_sides", "normal")
                    .help("向きが「両方向」の時、スケッチの裏側へ押し出す距離。裏側の範囲が「すべて」の時は使わない"),
            )
            .field(
                Field::choice("extent2", "裏側の範囲", &[("distance", "距離"), ("all", "すべて")])
                    .only("direction", "two_sides", "normal")
                    .help("向きが「両方向」の時の裏側の止め方。「すべて」は裏側の対象のボディを全部貫く(表側は「範囲」で決める)"),
            )
            .field(
                Field::opt_expr("taper", "テーパー角度").angle()
                    .help("空ならまっすぐ。壁の傾き。正は先へ行くほど細く、負は太くなる(穴の壁は逆に傾く)。SMLib のカーネルだけ"),
            )
            .fields({
                // 演算を手で選んだら印を付ける(もう自動で替えない)
                let [op, target] = operation_fields();
                [op.marks("op_manual"), target]
            })
            .upgrade(upgrade),
        |d| Ok(Box::new(parse::<Extrude>(d)?)),
    );
}

use kmb_cad::regions::{
    self, candidates, centroid, chosen_profile, faces_of, outline, profile_of, region_sketches,
    selected_profile,
};
use kmb_core::{Action, Arg, Context, Extension, Gesture, Handle, Outcome, Place};

/// 領域を選ばずに押し出す時のスケッチ: 選んでいるスケッチか、押し出せる輪郭が 1 つだけならそれ。
/// 閉じた輪郭が無く、開いた線だけのスケッチ(選んでいるか、それしか無い時)なら、それ(薄い押し出しにする)。
fn default_sketch(cx: &Context<'_>) -> Option<FeatureId> {
    selected_profile(cx)
        .or_else(|| match candidates(cx)[..] {
            [one] => Some(one),
            _ => None,
        })
        .or_else(|| cx.selected.filter(|id| open_lines(cx, *id).is_some()))
        .or_else(
            || match (candidates(cx).is_empty(), &open_sketches(cx)[..]) {
                (true, [one]) => Some(*one),
                _ => None,
            },
        )
}

/// 機能 `id` が、閉じた輪郭が無く線だけのスケッチなら、その線(薄い押し出しでだけ押し出せる)。
fn open_lines(cx: &Context<'_>, id: FeatureId) -> Option<kmb_core::Curves> {
    if kmb_cad::regions::has_profile(cx, id) {
        return None;
    }
    kmb_core::Curves::from_value(cx.output(id, "curves")?).filter(|c| !c.segments.is_empty())
}

/// 閉じた輪郭が無く線だけのスケッチ(古い順)。
fn open_sketches(cx: &Context<'_>) -> Vec<FeatureId> {
    cx.candidates("curves")
        .into_iter()
        .map(FeatureId)
        .filter(|id| open_lines(cx, *id).is_some())
        .collect()
}

/// 線だけのスケッチを、既定の厚さで真ん中に壁を付けた輪郭(押し出す高さを決める間の下絵)。
fn open_preview(cx: &Context<'_>, id: FeatureId) -> Result<kmb_core::Profile, String> {
    let c = open_lines(cx, id).ok_or("スケッチに線がありません")?;
    let walls = kmb_cad::thin::walls(
        c.plane,
        &kmb_cad::thin::chains(&c.segments),
        THICKNESS,
        kmb_cad::thin::Wall::Center,
    )?;
    Ok(kmb_core::Profile {
        plane: c.plane,
        regions: walls.into_iter().flat_map(|p| p.regions).collect(),
    })
}

/// 押し出すスケッチと領域の名前(空なら輪郭の全部)。指定の `regions`・`profile`
/// ([`regions::chosen_in_args`])、選んでいるスケッチ、押し出せる輪郭が 1 つだけならそれ。
fn chosen(cx: &Context<'_>) -> Option<(FeatureId, Vec<String>)> {
    if let Some(id) = cx.arg("profile").and_then(|v| v.as_u64()).map(FeatureId)
        && open_lines(cx, id).is_some()
    {
        return Some((id, Vec::new()));
    }
    if cx
        .arg("regions")
        .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty()))
        || cx.arg("profile").is_some()
    {
        return regions::chosen_in_args(cx);
    }
    default_sketch(cx).map(|id| (id, Vec::new()))
}

/// スケッチを押し出す。面のスケッチならその面のボディに、ほかは最後に見えるボディに結合し、ボディが無ければ新しいボディにする。
/// 距離 `distance`(mm)を指定すればその距離で。負なら逆向きに押し出し、ボディがあれば切り取る。
/// 閉じた輪郭が無く線だけのスケッチなら、線の真ん中に厚さ 2 mm の壁を付けた薄い押し出しにする。
/// 押し出しの高さの既定(mm)。
const DISTANCE: f64 = 10.0;

fn add_extrude(
    cx: &Context<'_>,
    sketch: FeatureId,
    regions: Vec<String>,
) -> Result<Outcome, kmb_core::Problem> {
    let open = regions.is_empty() && open_lines(cx, sketch).is_some();
    if !open {
        chosen_profile(cx, sketch, &regions)?;
    }
    // 面のスケッチなら、その面のボディ(の今の形)に結合する。原点の平面のスケッチは最後に見える
    // ボディ
    let bodies = cx.visible_bodies();
    let target = cx
        .reads_now(sketch)
        .into_iter()
        .find(|f| bodies.iter().any(|b| b.0 == *f))
        .or(bodies.last().map(|b| b.0));
    let d = cx.arg_f64("distance")?.unwrap_or(DISTANCE);
    if d.abs() < 1e-6 {
        return Err("距離が 0 です".into());
    }
    let data = Extrude {
        profile: Some(sketch),
        source: Source::Sketch,
        faces: Vec::new(),
        regions,
        distance: format!("{} mm", round(d.abs())),
        direction: if d < 0.0 {
            Direction::Reverse
        } else {
            Direction::Normal
        },
        distance2: None,
        extent: Extent::Distance,
        to_face: None,
        to_offset: None,
        operation: match (target.is_some(), d < 0.0) {
            (false, _) => Operation::New,
            (true, false) => Operation::Join,
            (true, true) => Operation::Cut,
        },
        target,
        thin: if open { Thin::Center } else { Thin::None },
        ..Extrude::default()
    };
    Ok(Outcome::insert(
        "押し出しを追加",
        EXTRUDE,
        format!("押し出し{}", cx.count_of(EXTRUDE) + 1),
        serde_json::to_value(data).expect("JSON にできる"),
    ))
}

/// 平らな面を押し出す時の面と、その縁の輪郭(平面の違う面があれば、最初の面の平面の輪郭)。
/// 選んでいる面(CLI・MCP は引数 `faces`、参照の 1 つか並び)が、1 つのボディの平らな面の時だけ。スケッチを選んでいる時、
/// スケッチや領域を指定した時、画面で選んだ面に平らな面が 1 つも無い時は無し(スケッチの押し出しにする)。
/// 面を一緒に押し出せない時(平らでない、別のボディ)は、黙ってスケッチへ移らず理由を返す。
fn chosen_faces(
    cx: &Context<'_>,
) -> Option<Result<(Vec<kmb_core::Ref>, kmb_core::Profile), String>> {
    if selected_profile(cx).is_some() || cx.arg("profile").is_some() || cx.arg("regions").is_some()
    {
        return None;
    }
    let refs: Vec<kmb_core::Ref> = match cx.arg("faces") {
        Some(v) => Field::info("faces", "").refs(&serde_json::json!({ "faces": v })),
        None => {
            let refs: Vec<kmb_core::Ref> = cx
                .picked_faces()
                .into_iter()
                .map(|p| kmb_core::Ref::from(p.clone()))
                .collect();
            if !refs.iter().any(|r| planar_face(cx, r)) {
                return None;
            }
            refs
        }
    };
    if refs.is_empty() {
        return None;
    }
    Some(faces_profile(cx, refs))
}

/// 参照 `r` がボディの平らな面か。
fn planar_face(cx: &Context<'_>, r: &kmb_core::Ref) -> bool {
    let kmb_core::Ref::Face { feature, face } = r else {
        return false;
    };
    let Some(kmb_core::Value::Shape(h)) = cx.output(*feature, "body") else {
        return false;
    };
    cx.kernel
        .faces(*h)
        .is_ok_and(|v| v.iter().any(|f| &f.name == face && f.planar.is_some()))
}

/// 面 `refs` が 1 つのボディの平らな面なら、面と最初の面の平面の縁の輪郭。でなければ理由。
fn faces_profile(
    cx: &Context<'_>,
    refs: Vec<kmb_core::Ref>,
) -> Result<(Vec<kmb_core::Ref>, kmb_core::Profile), String> {
    let feature = refs
        .first()
        .ok_or("押し出す平らな面を選んでください")?
        .feature();
    let mut names = Vec::new();
    for r in &refs {
        match r {
            kmb_core::Ref::Face { feature: f, face } if *f == feature => names.push(face.clone()),
            kmb_core::Ref::Face { .. } => {
                return Err(
                    "押し出す面は 1 つのボディから選んでください(別のボディの面が混ざっています)"
                        .into(),
                );
            }
            _ => return Err("押し出す物には平らな面だけを選んでください".into()),
        }
    }
    let Some(kmb_core::Value::Shape(h)) = cx.output(feature, "body") else {
        return Err(format!("{feature} はボディではありません"));
    };
    let mut groups = kmb_cad::face_profiles(cx.kernel.as_ref(), *h, &names)?;
    Ok((refs, groups.remove(0).profile.profile))
}

/// 平らな面を押し出す。面のボディに結合する(距離 `distance` が負なら切り取る)。
fn add_face_extrude(
    cx: &Context<'_>,
    faces: Vec<kmb_core::Ref>,
) -> Result<Outcome, kmb_core::Problem> {
    let target = faces.first().map(|r| r.feature());
    let d = cx.arg_f64("distance")?.unwrap_or(DISTANCE);
    if d.abs() < 1e-6 {
        return Err("距離が 0 です".into());
    }
    let data = Extrude {
        source: Source::Faces,
        faces,
        distance: format!("{} mm", round(d.abs())),
        direction: if d < 0.0 {
            Direction::Reverse
        } else {
            Direction::Normal
        },
        operation: if d < 0.0 {
            Operation::Cut
        } else {
            Operation::Join
        },
        target,
        ..Extrude::default()
    };
    Ok(Outcome::insert(
        "面の押し出しを追加",
        EXTRUDE,
        format!("押し出し{}", cx.count_of(EXTRUDE) + 1),
        serde_json::to_value(data).expect("JSON にできる"),
    ))
}

/// 値を 0.01 mm に丸めて、余分な 0 を付けない。
fn round(v: f64) -> String {
    let s = format!("{:.2}", (v * 100.0).round() / 100.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 押し出す道具の形の線: 輪郭を `from` と `to`(法線の向きの位置、mm)に置いた 2 つと、
/// 輪郭の角から何本かの縦の線(編集パネルで直している間、操作の色で描く)。
/// 斜めの面 `stop` で止める時は、終わりの輪郭をその面の上に置く。
fn tool_lines(
    p: &kmb_core::Profile,
    from: f64,
    to: f64,
    stop: Option<&StopPlane>,
) -> Vec<Vec<[f64; 3]>> {
    let n = p.plane.normal();
    let at = |q: [f64; 3], s: f64| [0, 1, 2].map(|i| q[i] + n[i] * s);
    // 点 q の終わり(斜めの面なら、法線の向きに進んで面に当たる所)
    let end = |q: [f64; 3]| match stop {
        Some(s) => {
            let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let c = dot(n, s.normal);
            at(q, dot([0, 1, 2].map(|i| s.point[i] - q[i]), s.normal) / c)
        }
        None => at(q, to),
    };
    let start = |q: [f64; 3]| if stop.is_some() { q } else { at(q, from) };
    let loops = outline(p);
    let mut out: Vec<Vec<[f64; 3]>> = Vec::new();
    out.extend(loops.iter().map(|l| l.iter().map(|q| start(*q)).collect()));
    out.extend(loops.iter().map(|l| l.iter().map(|q| end(*q)).collect()));
    for l in &loops {
        let step = (l.len() / 4).max(1);
        for q in l.iter().step_by(step) {
            out.push(vec![start(*q), end(*q)]);
        }
    }
    out
}

/// [`tool_lines`] の壁を傾けた物: 始めと終わりの輪郭(内へ寄せた)と、角からの斜めの線。
fn tapered_lines(
    cap: &kmb_cad::prism::PrismCap,
    from: f64,
    to: f64,
    lean: f64,
) -> Vec<Vec<[f64; 3]>> {
    let (a, b) = (cap.rings_at(from, lean), cap.rings_at(to, lean));
    let mut out: Vec<Vec<[f64; 3]>> = Vec::new();
    for l in a.iter().chain(&b) {
        // 輪郭の線は閉じて描く
        out.push(l.iter().chain(l.first()).copied().collect());
    }
    for (la, lb) in a.iter().zip(&b) {
        let step = (la.len() / 4).max(1);
        for k in (0..la.len()).step_by(step) {
            out.push(vec![la[k], lb[k]]);
        }
    }
    out
}

const DISTANCE_LABEL: &str =
    "マウスを動かして高さを決め、クリックで確定。逆向きにするとボディを切り取る";

/// 押し出しの高さを決める手順: 輪郭の重心から法線の向きにマウスで。
fn distance_handle(p: &kmb_core::Profile) -> Handle {
    let outline = outline(p);
    let dir = p.plane.normal();
    let len = dir.iter().map(|d| d * d).sum::<f64>().sqrt();
    // 引いている間に色付きの立体で見せる道具(値 1 の柱。画面が値の倍に伸ばす)
    let shape = kmb_cad::prism::PrismCap::new(&outline, dir).shape(len, "押し出す道具");
    Handle {
        outline,
        shape: Some(std::sync::Arc::new(shape)),
        ..Handle::new(
            "distance",
            DISTANCE_LABEL,
            Gesture::Distance {
                origin: centroid(p),
                dir: p.plane.normal(),
            },
        )
    }
}

/// 押し出す領域を選ぶ手順(候補はスケッチの線で分けた領域の全部。クリックで足す・外す)。
fn regions_handle(cx: &Context<'_>) -> Handle {
    let choices = regions::region_choices(cx, &region_sketches(cx));
    let whole = default_sketch(cx).is_some();
    Handle {
        choices,
        optional: whole,
        // ボディの平らな面も押し出せる(面をクリックすると面から高さへ)
        faces: true,
        ..Handle::new(
            "regions",
            if whole {
                "押し出す領域かボディの平らな面をクリックすると高さへ(いくつも選ぶ時は Ctrl を押しながら)。選ばずに Enter でスケッチ全体"
            } else {
                "押し出す領域かボディの平らな面をクリックすると高さへ(いくつも選ぶ時は Ctrl を押しながら)"
            },
            Gesture::Regions,
        )
    }
}

/// 領域を選ぶ手順が要るか: 候補の領域が 2 つ以上ある時(1 つなら自動で選ぶ)。
fn needs_regions(cx: &Context<'_>) -> bool {
    region_sketches(cx)
        .into_iter()
        .map(|id| faces_of(cx, id).len())
        .sum::<usize>()
        > 1
}

/// 選んでいる押し出しの範囲を、クリックした平らな面までにする操作。
pub const EXTRUDE_TO_FACE: &str = "cad.extrude_to_face";

/// 選んでいる押し出しの、押し出す領域を選び直す操作(作った後に 3D のビューで領域を足す・外す)。
pub const EXTRUDE_REGIONS: &str = "cad.extrude_regions";

/// 領域を選び直せる押し出し(ID、データ、輪郭のスケッチ)。スケッチの領域が 2 つ以上ある時だけ。
fn repickable(cx: &Context<'_>) -> Option<(FeatureId, Extrude, FeatureId)> {
    let (id, e) = selected_extrude(cx)?;
    let sketch = e.profile.filter(|_| e.source == Source::Sketch)?;
    regions::can_repick(cx, sketch).then_some((id, e, sketch))
}

/// 選んでいる押し出し(ID とデータ)。
fn selected_extrude(cx: &Context<'_>) -> Option<(FeatureId, Extrude)> {
    let id = cx.selected?;
    let e = cx.data.feature(id)?;
    if e.type_id != EXTRUDE {
        return None;
    }
    Some((id, serde_json::from_value(e.data.clone()).ok()?))
}

/// 押し出し `id` の範囲を、クリックした面(選んだ面、CLI・MCP は引数 `face` と `feature`)までにする。
/// 面は押し出しより前の形から探す(結合した後のボディの面を選んでも、元の対象のボディの同じ面を使う)。
fn extrude_to_face(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let (id, mut e) = selected_extrude(cx).ok_or("押し出しを選んでいません")?;
    let (pick, given) =
        kmb_cad::stop_at::stop_pick(cx).ok_or("対象の平らな面か頂点をクリックしてください")?;
    let at = |f: FeatureId| cx.data.index_of(f).unwrap_or(usize::MAX);
    let has = |f: FeatureId| {
        matches!(cx.output(f, "body"), Some(kmb_core::Value::Shape(b))
            if kmb_cad::stop_at::has(cx.kernel.as_ref(), *b, &pick))
    };
    let feature = [given, e.target]
        .into_iter()
        .flatten()
        .find(|f| at(*f) < at(id) && has(*f))
        .ok_or("この押し出しより後で出来た面は選べません。押し出しより前からある平らな面か頂点を選んでください")?;
    // 押し出す先がボディの中を通るなら切り取り、通らないなら結合にする(矢印をボディへ
    // 引くと切り取りになる)。新しいボディ・交差はそのまま。
    // 決め直すのは、押し出す向きが反対の側へ変わる時だけ(同じ側なら始めた演算を保つ。貫いても
    // 切り取りのまま)。演算を手で選んでいれば決め直さない
    let mut label = "押し出しを対象の面までにする";
    if let (Some(t), Operation::Join | Operation::Cut, false) = (e.target, e.operation, e.op_manual)
        && let Some(kmb_core::Value::Shape(target)) = cx.output(t, "body")
        && let Some(kmb_core::Value::Shape(fb)) = cx.output(feature, "body")
        && let Some(profile) = extrude_profile(cx, &e)
        && let Ok(stop) = face_stop(cx.kernel.as_ref(), *fb, &pick, &profile)
        && side_now(cx, &e, &profile).is_none_or(|now| now != stop.near.signum())
    {
        let into = goes_into(cx.kernel.as_ref(), *target, &profile, stop.near);
        let op = if into {
            Operation::Cut
        } else {
            Operation::Join
        };
        if op != e.operation {
            e.operation = op;
            label = if into {
                "押し出しを対象の面までの切り取りにする"
            } else {
                "押し出しを対象の面までの結合にする"
            };
        }
    }
    let (face, vertex) = pick.parts();
    e.extent = Extent::ToFace;
    e.to_face = Some(ToFace {
        feature,
        face,
        vertex,
    });
    Ok(Outcome::Change {
        label: label.into(),
        edits: vec![kmb_core::Edit::SetData {
            id,
            data: serde_json::to_value(&e).map_err(|e| e.to_string())?,
        }],
    })
}

/// 押し出し `e` が今押し出している側(輪郭の平面の法線の向きなら 1、逆なら -1)。両側へ押し出して
/// いる時は `None`(どちらの側の面でも決め直す)。
fn side_now(cx: &Context<'_>, e: &Extrude, profile: &kmb_core::Profile) -> Option<f64> {
    if e.extent == Extent::ToFace
        && let Some(t) = &e.to_face
        && let Some(kmb_core::Value::Shape(b)) = cx.output(t.feature, "body")
        && let Ok(s) = face_stop(cx.kernel.as_ref(), *b, &t.pick(), profile)
    {
        return Some(s.near.signum());
    }
    match e.direction {
        Direction::Normal => Some(1.0),
        Direction::Reverse => Some(-1.0),
        Direction::Symmetric | Direction::TwoSides => None,
    }
}

/// 押し出し `e` の輪郭(スケッチの輪郭か、平らな面の縁)を、開始のずらしの分だけ動かした物
/// (押し出しが始まる所。面までの距離も、結合か切り取りかの判定もここから測る。10-26 k15)。
fn extrude_profile(cx: &Context<'_>, e: &Extrude) -> Option<kmb_core::Profile> {
    let mut p = sketch_or_face_profile(cx, e)?;
    p.plane = p.plane.offset(start_shift(cx, e));
    Some(p)
}

/// 開始のずらし(mm)。式は今のパラメータの値で読む。読めなければ 0(ずらしの欄の誤りは計算の方が出す)。
fn start_shift(cx: &Context<'_>, e: &Extrude) -> f64 {
    let (Start::Offset, Some(src)) = (e.start, e.start_offset.as_deref()) else {
        return 0.0;
    };
    let values = cx.report.param_values();
    kmb_core::parse_expr(src)
        .ok()
        .and_then(|x| x.eval(&|n| values.get(n).copied()).ok())
        .filter(|q| q.dim.is_none() || q.dim == kmb_core::Dim::LENGTH)
        .map_or(0.0, |q| q.value)
}

/// 押し出し `e` の、輪郭の平面のままの輪郭。
fn sketch_or_face_profile(cx: &Context<'_>, e: &Extrude) -> Option<kmb_core::Profile> {
    match e.source {
        Source::Sketch => profile_of(cx, e.profile?),
        Source::Faces => {
            let (feature, names) = e.face_source().ok()?;
            let Some(kmb_core::Value::Shape(h)) = cx.output(feature, "body") else {
                return None;
            };
            Some(
                kmb_cad::face_profiles(cx.kernel.as_ref(), *h, &names)
                    .ok()?
                    .into_iter()
                    .next()?
                    .profile
                    .profile,
            )
        }
    }
}

/// 押し出しの操作。
pub struct ExtrudeCommands;

impl Extension for ExtrudeCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let sketch = selected_profile(cx).is_some();
        // 選んだ面(引数の面も)を一緒に押し出せない時は、その理由で止める(画面のプレス/プルも聞く)
        let why = match chosen_faces(cx) {
            Some(Err(why)) => Some(why),
            Some(Ok(_)) => None,
            None => (candidates(cx).is_empty() && open_sketches(cx).is_empty()).then(|| {
                "閉じた輪郭のあるスケッチか、ボディの平らな面を選んでいません".to_string()
            }),
        };
        vec![
            Action::new(EXTRUDE_SKETCH, "押し出し", "cad")
                .arg(Arg::length("distance", "高さ(mm。負なら逆へ)").default(DISTANCE))
                .arg(Arg::pick(
                    "faces",
                    "押し出すボディの平らな面(省くとスケッチの領域)",
                    &[kmb_core::RefKind::PlanarFace],
                ))
                .args(regions::args())
                .role(kmb_interchange::role::action::EXTRUDE)
                .menu("作成")
                .icon("extrude")
                .aliases("おしだし extrude")
                .hint("押してから押し出す領域かボディの平らな面をクリックする(重なった形は分かれた領域ごとに選べる。いくつも選ぶ時は Ctrl を押しながら)。高さをマウスか数字で決めて Enter。ボディの平らな面を選んでから押すと、その面を押し出す。線だけのスケッチは厚さ 2 mm の壁にして押し出す(薄い押し出し)")
                .at(&[Place::Toolbar])
                .at(if sketch { &[Place::FeatureMenu] } else { &[] })
                .disabled_if(why.as_deref()),
            Action::new(EXTRUDE_TO_FACE, "対象の面までにする", "cad")
                .args(kmb_cad::stop_at::args())
                .menu("作成")
                .icon("extrude")
                .aliases("おしだし たいしょう まで めん extrude to object")
                .hint("選んでいる押し出しの範囲を、クリックした平らな面までにする(斜めの面なら、道具の終わりをその面で切る)。押し出す先がボディの中を通れば切り取りに、通らなければ結合にする")
                .keep_edit()
                .at(match selected_extrude(cx) {
                    // 範囲が「対象まで」なら編集の欄にも出す(面の選び直し)
                    Some((_, e)) if e.extent == Extent::ToFace => {
                        &[Place::FeatureMenu, Place::Panel]
                    }
                    Some(_) => &[Place::FeatureMenu],
                    None => &[],
                })
                .disabled_if(
                    selected_extrude(cx)
                        .is_none()
                        .then_some("押し出しを選んでから使う"),
                ),
            Action::new(EXTRUDE_REGIONS, "領域を選び直す", "cad")
                .args(regions::args())
                .menu("作成")
                .icon("extrude")
                .aliases("おしだし りょういき 輪郭 プロファイル えらびなおす extrude profile region")
                .hint("選んでいる押し出しの、押し出す領域をクリックで足す・外す(今の領域を選んだまま始まる)。Enter で決める")
                .keep_edit()
                .at(if repickable(cx).is_some() {
                    &[Place::FeatureMenu, Place::Panel]
                } else {
                    &[]
                })
                .disabled_if(
                    repickable(cx)
                        .is_none()
                        .then_some("線で分けた領域が 2 つ以上あるスケッチの押し出しを選んでから使う"),
                ),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        Some(match id {
            EXTRUDE_REGIONS => (|| {
                let (id, mut e, sketch) =
                    repickable(cx).ok_or("領域を選び直せる押し出しを選んでいません")?;
                e.regions = regions::repicked(cx, sketch)?;
                Ok(Outcome::Change {
                    label: "押し出す領域を選び直す".into(),
                    edits: vec![kmb_core::Edit::SetData {
                        id,
                        data: serde_json::to_value(&e).map_err(|e| e.to_string())?,
                    }],
                })
            })(),
            EXTRUDE_SKETCH => match chosen_faces(cx) {
                Some(Ok((faces, _))) => add_face_extrude(cx, faces),
                Some(Err(why)) => Err(why.into()),
                None => chosen(cx)
                    .ok_or_else(|| {
                        "押し出す領域(閉じた輪郭のあるスケッチ)か、ボディの平らな面を選んでいません"
                            .to_string()
                    })
                    .map_err(Into::into)
                    .and_then(|(s, keys)| add_extrude(cx, s, keys)),
            },
            EXTRUDE_TO_FACE => extrude_to_face(cx),
            _ => return None,
        })
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if id == EXTRUDE_REGIONS {
            let (_, e, sketch) = repickable(cx)?;
            return Some(vec![regions::repick_handle(
                cx,
                sketch,
                &e.regions,
                "押し出す領域をクリック(もう一度で外す)。Enter で決める",
            )]);
        }
        if id == EXTRUDE_TO_FACE {
            selected_extrude(cx)?;
            return Some(vec![Handle {
                vertices: true,
                ..Handle::new(
                    "face",
                    "押し出しを止める平らな面(斜めの面でもよい)か頂点をクリック",
                    Gesture::Face,
                )
            }]);
        }
        if id != EXTRUDE_SKETCH {
            return None;
        }
        // 平らな面を選んでいれば、その面から高さを決める(前へ引くと結合、逆へ引くと切り取り)
        if let Some(Ok((_, p))) = chosen_faces(cx) {
            return Some(vec![Handle {
                ops: Some([kmb_core::ToolOp::Join, kmb_core::ToolOp::Cut]),
                ..distance_handle(&p)
            }]);
        }
        // 領域が 2 つ以上あれば、領域を選ぶ手順から始める(1 つなら自動で選んで高さから)。
        // 選んだ後は、選んだ領域で高さの手順を出し直す
        let pick_first = needs_regions(cx);
        let mut v = Vec::new();
        if pick_first {
            v.push(regions_handle(cx));
        }
        let decided = !pick_first || cx.arg("regions").is_some();
        match chosen(cx).filter(|_| decided) {
            Some((s, keys)) => v.push(
                match chosen_profile(cx, s, &keys).or_else(|e| {
                    // 線だけのスケッチは、薄い押し出しの壁を下絵にする
                    if keys.is_empty() && open_lines(cx, s).is_some() {
                        open_preview(cx, s)
                    } else {
                        Err(e)
                    }
                }) {
                    Ok(p) => Handle {
                        // 前へ引くとボディに結合、逆へ引くと切り取り(ボディがなければ新しいボディ)。
                        // 線をその色で描く(add_extrude と同じ決め方)
                        ops: Some(if cx.visible_bodies().is_empty() {
                            [kmb_core::ToolOp::New; 2]
                        } else {
                            [kmb_core::ToolOp::Join, kmb_core::ToolOp::Cut]
                        }),
                        ..distance_handle(&p)
                    },
                    // 選んだ領域では押し出せない: 理由を手順の説明に出す(決めると同じ理由で断る)
                    Err(e) => Handle::new(
                        "distance",
                        &format!("{e}。Esc でやめて選び直す"),
                        Gesture::Distance {
                            origin: [0.0; 3],
                            dir: [0.0, 0.0, 1.0],
                        },
                    ),
                },
            ),
            None if pick_first => v.push(Handle::new(
                "distance",
                DISTANCE_LABEL,
                Gesture::Distance {
                    origin: [0.0; 3],
                    dir: [0.0, 0.0, 1.0],
                },
            )),
            None => return None,
        }
        Some(v)
    }
}

/// この crate の名乗り(登録する種類と操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-extrude", register).with_extension(ExtrudeCommands)
}
