//! 下絵(キャンバス、`cad.canvas@1`)。写真や図面の絵(PNG・JPEG・BMP)を原点の平面か平らな面に置き、
//! スケッチで上からなぞる時の下絵にする。絵は文書に入れて持つので、元の
//! ファイルを動かしても開ける(STEP の読み込みと同じ)。形・加工・書き出しには使わず、3D のビューと
//! スケッチの編集が下に描くだけ([`kmb_core::Picture`] の出力)。
//!
//! 大きさは「キャリブレーション」で合わせる: 絵の上の 2 点をクリックし、その間の本当の長さを打つと、
//! 1 点目を動かさずに絵の幅を変える。
use kmb_cad::plane::PlaneRef;
use kmb_cad::{Exprs, angle, expr_refs, length, parse, rename};
use kmb_core::kernel::{self as k, P3};
use kmb_core::{
    Action, Arg, Context, Descriptor, EvalCtx, EvalError, Extension, Feature, FeatureId, Field,
    Gesture, Handle, Input, InputRef, Outcome, Outputs, Payload, Picture, Place, Registry,
};
use kmb_sketch::SketchPlane;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CANVAS: &str = "cad.canvas@1";
/// 絵のファイルを選んで下絵を置く操作。
pub const CANVAS_INSERT: &str = "cad.canvas_insert";
/// 下絵を置く平面を、原点の平面か平らな面のクリックで選び直す操作。
pub const CANVAS_PLANE: &str = "cad.canvas_plane";
/// 2 点と距離で下絵の大きさを合わせる操作。
pub const CANVAS_CALIBRATE: &str = "cad.canvas_calibrate";

/// 文書に入れる絵の長い辺の上限(画素)。超える絵は縮めて入れる。
pub const MAX_SIDE: u32 = 4096;

fn zero() -> String {
    "0 mm".into()
}

fn zero_deg() -> String {
    "0 deg".into()
}

fn width_default() -> String {
    "100 mm".into()
}

fn half() -> String {
    "0.5".into()
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 例: `{"plane": "xy", "file": "図.png", "image": "<base64>", "px": [800, 600], "x": "0 mm",
/// "y": "0 mm", "angle": "0 deg", "width": "100 mm", "opacity": "0.5"}`。`x`・`y` は絵の中心
/// (平面の座標)、`width` は絵の横の長さ(縦は絵の縦横の比で決まる)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canvas {
    #[serde(default)]
    pub plane: SketchPlane,
    /// 元のファイルの名前(表示用。読み直しには使わない)
    #[serde(default)]
    pub file: String,
    /// 絵のファイルの中身(base64)
    pub image: String,
    /// 絵の幅と高さ(画素)
    pub px: [u32; 2],
    #[serde(default = "zero")]
    pub x: String,
    #[serde(default = "zero")]
    pub y: String,
    #[serde(default = "zero_deg")]
    pub angle: String,
    #[serde(default = "width_default")]
    pub width: String,
    #[serde(default = "half")]
    pub opacity: String,
    /// 左右を反転する
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip_h: bool,
    /// 上下を反転する
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip_v: bool,
}

impl Exprs for Canvas {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![
            &mut self.x,
            &mut self.y,
            &mut self.angle,
            &mut self.width,
            &mut self.opacity,
        ]
    }
}

impl Canvas {
    /// 絵のファイルの中身から作る(大きすぎる絵は縮める)。
    pub fn from_bytes(file: &str, bytes: &[u8]) -> Result<Canvas, String> {
        let (bytes, w, h) = kmb_image::fit(bytes, MAX_SIDE)?;
        Ok(Canvas {
            plane: SketchPlane::Xy,
            file: file.into(),
            image: kmb_format::base64::encode(&bytes),
            px: [w, h],
            x: zero(),
            y: zero(),
            angle: zero_deg(),
            width: width_default(),
            opacity: half(),
            flip_h: false,
            flip_v: false,
        })
    }

    /// 平面の座標での中心・回転(rad)・幅(mm)。
    fn placement(&self, ctx: &EvalCtx<'_>) -> Result<([f64; 2], f64, f64), EvalError> {
        let width = length(ctx, &self.width, "幅")?;
        if width <= 0.0 {
            return Err(EvalError::InvalidData(format!(
                "幅 `{}` は 0 より大きくしてください",
                self.width
            )));
        }
        Ok((
            [length(ctx, &self.x, "X")?, length(ctx, &self.y, "Y")?],
            angle(ctx, &self.angle, "角度")?,
            width,
        ))
    }
}

/// 中心 `c`・回転 `a`(rad)・幅 `w`・高さ `h` の絵の、左下・右下・右上・左上の角(平面の座標)。
/// 反転は角の並びを入れ替えて表す(絵の左下の画素が来る角が最初)。
pub fn corners(c: [f64; 2], a: f64, w: f64, h: f64, flip_h: bool, flip_v: bool) -> [[f64; 2]; 4] {
    let (sx, sy) = (
        if flip_h { -1.0 } else { 1.0 },
        if flip_v { -1.0 } else { 1.0 },
    );
    let (s, co) = a.sin_cos();
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(u, v)| {
        let (px, py) = (u * sx * w / 2.0, v * sy * h / 2.0);
        [c[0] + px * co - py * s, c[1] + px * s + py * co]
    })
}

fn key_of(image: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    image.hash(&mut h);
    h.finish()
}

impl Feature for Canvas {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.plane.inputs();
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let plane = kmb_sketch::resolve_plane(&self.plane, ctx)?;
        let (c, a, w) = self.placement(ctx)?;
        let [pw, ph] = self.px;
        if pw == 0 || ph == 0 {
            return Err(EvalError::InvalidData("絵の幅か高さが 0 です".into()));
        }
        let h = w * ph as f64 / pw as f64;
        let q = ctx.expr(&self.opacity)?;
        if !q.dim.is_none() || !(0.0..=1.0).contains(&q.value) {
            return Err(EvalError::InvalidData(format!(
                "不透明度 `{}` は 0 から 1(0% から 100%)にしてください",
                self.opacity
            )));
        }
        let picture = Picture {
            plane,
            corners: corners(c, a, w, h, self.flip_h, self.flip_v),
            image: self.image.clone(),
            key: key_of(&self.image),
            opacity: q.value,
        };
        Ok([(kmb_core::PICTURE.to_string(), picture.to_value())].into())
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        Descriptor::new(CANVAS, kmb_core::category::SOLID, "キャンバス")
            .menu("挿入")
            .icon("canvas")
            .hint("写真や図面の絵を平面に置き、スケッチでなぞる下絵にする(形には使わない)")
            .aliases(
                "きゃんばす キャンバス canvas したえ 下絵 がぞう 画像 しゃしん 写真 picture image",
            )
            .field(Field::info("file", "ファイル"))
            .field(
                Field::expr("x", "中心 X")
                    .length()
                    .help("絵の中心(平面の座標)"),
            )
            .field(Field::expr("y", "中心 Y").length())
            .field(Field::expr("angle", "角度").angle())
            .field(
                Field::expr("width", "幅").length().help(
                    "絵の横の長さ。縦は絵の縦横の比で決まる。キャリブレーションでも合わせられる",
                ),
            )
            .field(Field::expr("opacity", "不透明度").ratio())
            .field(Field::bool("flip_h", "水平方向に反転"))
            .field(Field::bool("flip_v", "垂直方向に反転"))
            .no_raw_editor(),
        |d| Ok(Box::new(parse::<Canvas>(d)?)),
    );
}

/// 選んでいる機能が下絵なら、その番号とデータ。
fn selected(cx: &Context<'_>) -> Option<(FeatureId, Canvas)> {
    let id = cx.selected?;
    let entry = cx.data.feature(id)?;
    (entry.type_id == CANVAS)
        .then(|| serde_json::from_value(entry.data.clone()).ok())
        .flatten()
        .map(|c| (id, c))
}

/// 選んでいる下絵の、計算した絵(平面と角)。
fn picture(cx: &Context<'_>, id: FeatureId) -> Option<Picture> {
    cx.output(id, kmb_core::PICTURE)
        .and_then(Picture::from_value)
}

/// 絵のファイルを読んで下絵を足す。平らな面を選んでいればその面に、ほかは XY 平面に置く。
fn insert(cx: &Context<'_>) -> Result<Outcome, String> {
    let path = cx
        .file
        .map(std::path::Path::to_path_buf)
        .or_else(|| cx.arg_str("path").map(std::path::PathBuf::from))
        .ok_or("絵のファイルを選んでいません")?;
    let bytes =
        std::fs::read(&path).map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut canvas = Canvas::from_bytes(&file, &bytes).map_err(|e| format!("{file}: {e}"))?;
    if let Some(p) = cx.picked
        && kmb_cad::plane::has_planar_face(cx, p.feature, &p.face)
    {
        canvas.plane = SketchPlane::face(p.feature, p.face.clone());
    }
    log::info!(
        "下絵 {file} を置く({}×{} 画素、{} バイト)",
        canvas.px[0],
        canvas.px[1],
        bytes.len()
    );
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "キャンバス".into());
    Ok(Outcome::insert(
        "キャンバスを置く",
        CANVAS,
        name,
        serde_json::to_value(canvas).map_err(|e| e.to_string())?,
    ))
}

/// 平面を、手順で選んだ値(`plane` が xy・yz・zx か face)にする。
fn choose_plane(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, mut canvas) = selected(cx).ok_or("キャンバスを選んでいません")?;
    canvas.plane = match PlaneRef::from_choice(cx, id, "キャンバスの平面")? {
        PlaneRef::OriginXy => SketchPlane::Xy,
        PlaneRef::OriginYz => SketchPlane::Yz,
        PlaneRef::OriginZx => SketchPlane::Zx,
        PlaneRef::Face {
            feature: Some(f),
            face: Some(face),
        } => SketchPlane::face(f, face),
        _ => return Err("平面は原点の平面か平らな面で選んでください".into()),
    };
    Ok(Outcome::Change {
        label: "キャンバスの平面を選ぶ".into(),
        edits: vec![kmb_core::Edit::SetData {
            id,
            data: serde_json::to_value(canvas).map_err(|e| e.to_string())?,
        }],
    })
}

fn point_arg(cx: &Context<'_>, key: &str) -> Result<P3, String> {
    let v = cx
        .arg(key)
        .ok_or(format!("{key} に点 [x, y, z] を指定します"))?;
    serde_json::from_value(v.clone()).map_err(|_| format!("{key} は [x, y, z] の数で書きます"))
}

/// 2 点(世界の座標)とその間の本当の長さから、1 点目を動かさずに絵の幅と中心を変える。
fn calibrate(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, mut canvas) = selected(cx).ok_or("キャンバスを選んでいません")?;
    let pic = picture(cx, id).ok_or("キャンバスがまだ計算できていません")?;
    let (a, b) = (
        pic.plane.to_local(point_arg(cx, "from")?),
        pic.plane.to_local(point_arg(cx, "to")?),
    );
    let measured = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    if measured < 1e-6 {
        return Err("2 つの点が重なっています。離れた 2 点を選んでください".into());
    }
    let real = cx
        .arg_f64("distance")?
        .ok_or("distance に 2 点の間の本当の長さ(mm)を指定します")?;
    if real <= 0.0 {
        return Err("長さは 0 より大きくしてください".into());
    }
    let s = real / measured;
    // 今の中心と幅(角の平均と、下の辺の長さ)
    let c = pic.corners;
    let center = [(c[0][0] + c[2][0]) / 2.0, (c[0][1] + c[2][1]) / 2.0];
    let width = ((c[1][0] - c[0][0]).powi(2) + (c[1][1] - c[0][1]).powi(2)).sqrt();
    let moved = [a[0] + (center[0] - a[0]) * s, a[1] + (center[1] - a[1]) * s];
    let mm = |v: f64| format!("{} mm", (v * 1e4).round() / 1e4);
    canvas.x = mm(moved[0]);
    canvas.y = mm(moved[1]);
    canvas.width = mm(width * s);
    log::info!("キャンバス {id} の大きさを {s:.4} 倍にした(2 点の間 {measured:.3} → {real:.3} mm)");
    Ok(Outcome::Change {
        label: "キャンバスのキャリブレーション".into(),
        edits: vec![kmb_core::Edit::SetData {
            id,
            data: serde_json::to_value(canvas).map_err(|e| e.to_string())?,
        }],
    })
}

/// 下絵を置く・平面を選ぶ・大きさを合わせる操作。
pub struct CanvasCommands;

impl Extension for CanvasCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let on = selected(cx).is_some();
        let at: &[Place] = if on {
            &[Place::FeatureMenu, Place::Panel]
        } else {
            &[]
        };
        let off = (!on).then_some("キャンバスを選んでから使う");
        vec![
            Action::new(CANVAS_INSERT, "キャンバス…", "cad")
                .arg(Arg::file("path", "絵のファイル(省くとファイルを選ぶ窓を出す)"))
                .menu("挿入")
                .icon("canvas")
                .aliases("きゃんばす キャンバス canvas したえ 下絵 がぞう 画像 しゃしん 写真 picture image")
                .hint("写真や図面の絵を置き、スケッチでなぞる下絵にする。平らな面を選んでいればその面に置く")
                .at(&[Place::Toolbar])
                .input(Input::OpenFile {
                    title: "キャンバスの絵を選ぶ".into(),
                    filter: "絵(PNG・JPEG・BMP)".into(),
                    extensions: kmb_image::KINDS
                        .iter()
                        .flat_map(|e| [e.to_string(), e.to_uppercase()])
                        .collect(),
                }),
            Action::new(CANVAS_PLANE, "平面を選ぶ", "cad")
                .args(kmb_cad::plane::args())
                .menu("挿入")
                .icon("canvas")
                .aliases("きゃんばす へいめん 平面 canvas plane")
                .hint("キャンバスを置く平面を、原点の XY・YZ・ZX 平面の四角か、ボディの平らな面をクリックして決める")
                .keep_edit()
                .at(at)
                .disabled_if(off),
            Action::new(CANVAS_CALIBRATE, "キャリブレーション", "cad")
                .arg(Arg::vector("from", "絵の上の 1 点目(世界の座標 mm。手順の値)").required())
                .arg(Arg::vector("to", "絵の上の 2 点目(世界の座標 mm。手順の値)").required())
                .arg(Arg::length("distance", "2 点の間の本当の長さ").required())
                .menu("挿入")
                .icon("measure")
                .aliases("きゃりぶれーしょん キャリブレーション calibrate 大きさ 合わせる 尺度")
                .hint("絵の上の長さが分かっている 2 点をクリックし、その本当の長さを打って絵の大きさを合わせる")
                .keep_edit()
                .at(at)
                .disabled_if(off),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        Some(match id {
            CANVAS_INSERT => insert(cx),
            CANVAS_PLANE => choose_plane(cx),
            CANVAS_CALIBRATE => calibrate(cx),
            _ => return None,
        })
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        match id {
            CANVAS_PLANE => {
                selected(cx)?;
                Some(vec![Handle {
                    choices: ["xy", "yz", "zx"]
                        .iter()
                        .enumerate()
                        .map(|(i, v)| kmb_core::HandleChoice::origin_plane(i, json!(v)))
                        .collect(),
                    faces: true,
                    ..Handle::new(
                        "plane",
                        "キャンバスを置く原点の平面(XY・YZ・ZX の四角)か、ボディの平らな面をクリック",
                        Gesture::Choice,
                    )
                }])
            }
            CANVAS_CALIBRATE => {
                let (fid, _) = selected(cx)?;
                let pic = picture(cx, fid)?;
                let normal = k::cross(pic.plane.x_axis, pic.plane.y_axis);
                let outline = {
                    let mut w = pic.world_corners().to_vec();
                    w.push(w[0]);
                    vec![w]
                };
                let point = |key: &str, label: &str| Handle {
                    outline: outline.clone(),
                    ..Handle::new(
                        key,
                        label,
                        Gesture::Point {
                            origin: pic.plane.origin,
                            normal,
                            snap: 0.0,
                        },
                    )
                };
                let mut v = vec![
                    point("from", "絵の上の、長さが分かっている所の 1 点目をクリック"),
                    point("to", "2 点目をクリック"),
                ];
                if let (Ok(a), Ok(b)) = (point_arg(cx, "from"), point_arg(cx, "to")) {
                    let d = k::sub(b, a);
                    let len = k::dot(d, d).sqrt();
                    if len > 1e-9 {
                        v.push(Handle {
                            outline: vec![vec![a, b]],
                            ..Handle::new(
                                "distance",
                                "2 点の間の本当の長さを打って Enter",
                                Gesture::Distance {
                                    origin: a,
                                    dir: k::scale(d, 1.0 / len),
                                },
                            )
                        });
                    }
                } else {
                    v.push(Handle::new(
                        "distance",
                        "2 点の間の本当の長さを打って Enter",
                        Gesture::Distance {
                            origin: pic.plane.origin,
                            dir: pic.plane.x_axis,
                        },
                    ));
                }
                Some(v)
            }
            _ => None,
        }
    }
}

/// この crate の名乗り(登録する種類と操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-sketch-canvas", register).with_extension(CanvasCommands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_turn_and_flip_about_the_center() {
        let c = corners([10.0, 0.0], 0.0, 4.0, 2.0, false, false);
        assert_eq!(c, [[8.0, -1.0], [12.0, -1.0], [12.0, 1.0], [8.0, 1.0]]);
        // 左右の反転: 絵の左下の画素は右下の角に来る
        let f = corners([10.0, 0.0], 0.0, 4.0, 2.0, true, false);
        assert_eq!(f[0], [12.0, -1.0]);
        // 90 度回すと、幅の向きが Y になる
        let r = corners(
            [0.0, 0.0],
            std::f64::consts::FRAC_PI_2,
            4.0,
            2.0,
            false,
            false,
        );
        assert!(
            (r[1][0] - 1.0).abs() < 1e-12 && (r[1][1] - 2.0).abs() < 1e-12,
            "{r:?}"
        );
    }
}
