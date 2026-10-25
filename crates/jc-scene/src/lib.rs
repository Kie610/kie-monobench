//! レンダリングの光源とデカール。どちらもタイムラインの機能で、形は変えず、
//! 出力([`jc_look::decor::Lamp`]・[`jc_look::decor::Decal`])を 3D のビューとレンダリングが読む。
//!
//! - 光源(`render.light@1`): 点光源かスポットライト。位置・照らす点・色・明るさ(lm)・広がりの角・
//!   縁のぼかし・大きさ。位置は式で書けるので、寸法に合わせて動かせる。
//! - デカール(`render.decal@1`): 絵(PNG・JPEG・BMP)を 1 つの面に貼る。絵は文書に base64 で写して
//!   持つ(元のファイルを動かしても開ける。1 枚 4 MB まで)。面の中心からのずれ・幅・回転・不透明度・
//!   反転。面が無くなったら別の面に付け替えず、この機能を誤りにする。
//!
//! レンダリングの作業空間の「設定」の「光源」「デカール…」で足す。
use jc_cad::{Exprs, angle, expr_refs, length, output, parse, rename};
use jc_core::kernel::ShapeHandle;
use jc_core::{
    Action, Arg, Context, Descriptor, EvalCtx, EvalError, Extension, Feature, Field, Input,
    InputRef, Outcome, Outputs, Payload, Place, Ref, RefKind, Registry, Value,
};
use jc_look::decor::{self, Lamp};
use serde::{Deserialize, Serialize};

pub const LIGHT: &str = "render.light@1";
pub const DECAL: &str = "render.decal@1";
/// 光源を足す操作。
pub const LIGHT_INSERT: &str = "render.light_insert";
/// 選んだ面に絵のファイルを貼る操作。
pub const DECAL_INSERT: &str = "render.decal_insert";

/// デカールの絵のファイルの大きさの上限(Kie 2026-10-22。文書が大きくなりすぎないように)。
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
/// 文書に入れる絵の長い辺の上限(画素。超える絵は縮めて入れる)。
pub const MAX_SIDE: u32 = 4096;

fn zero() -> String {
    "0 mm".into()
}

fn zero_deg() -> String {
    "0 deg".into()
}

fn white() -> String {
    "#ffffff".into()
}

fn bulb() -> String {
    "800".into()
}

fn cone() -> String {
    "45 deg".into()
}

fn blur() -> String {
    "10 deg".into()
}

fn size() -> String {
    "5 mm".into()
}

fn width_default() -> String {
    "50 mm".into()
}

fn one() -> String {
    "1".into()
}

fn point() -> String {
    "point".into()
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// `#rrggbb`(# は省いてもよい)か `r,g,b`(0〜255)。
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim();
    if s.contains(',') {
        let v: Vec<u8> = s
            .split(',')
            .map(|p| p.trim().parse::<u8>())
            .collect::<Result<_, _>>()
            .ok()?;
        return <[u8; 3]>::try_from(v).ok();
    }
    let h = s.trim_start_matches('#');
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

/// 光源。例: `{"kind": "spot", "x": "100 mm", "y": "-150 mm", "z": "200 mm", "aim_x": "0 mm",
/// "aim_y": "0 mm", "aim_z": "0 mm", "color": "#ffe8c0", "lumens": "800", "cone": "45 deg",
/// "blur": "10 deg", "size": "5 mm"}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Light {
    /// point(点光源)か spot(スポットライト)
    #[serde(default = "point")]
    pub kind: String,
    #[serde(default = "zero")]
    pub x: String,
    #[serde(default = "zero")]
    pub y: String,
    #[serde(default = "zero")]
    pub z: String,
    /// スポットが照らす点
    #[serde(default = "zero")]
    pub aim_x: String,
    #[serde(default = "zero")]
    pub aim_y: String,
    #[serde(default = "zero")]
    pub aim_z: String,
    #[serde(default = "white")]
    pub color: String,
    /// 明るさ(lm。単位の無い数)
    #[serde(default = "bulb")]
    pub lumens: String,
    #[serde(default = "cone")]
    pub cone: String,
    #[serde(default = "blur")]
    pub blur: String,
    /// 光源の大きさ(半径。影の縁のぼけ)
    #[serde(default = "size")]
    pub size: String,
}

impl Exprs for Light {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![
            &mut self.x,
            &mut self.y,
            &mut self.z,
            &mut self.aim_x,
            &mut self.aim_y,
            &mut self.aim_z,
            &mut self.lumens,
            &mut self.cone,
            &mut self.blur,
            &mut self.size,
        ]
    }
}

fn bad(s: String) -> EvalError {
    EvalError::InvalidData(s)
}

impl Light {
    fn spot(&self) -> Result<bool, EvalError> {
        match self.kind.as_str() {
            "point" => Ok(false),
            "spot" => Ok(true),
            k => Err(bad(format!("光源の種類 {k} は使えません(point か spot)"))),
        }
    }

    /// 計算した光源。
    pub fn lamp(&self, ctx: &EvalCtx<'_>) -> Result<Lamp, EvalError> {
        let spot = self.spot()?;
        let position = [
            length(ctx, &self.x, "位置 X")?,
            length(ctx, &self.y, "位置 Y")?,
            length(ctx, &self.z, "位置 Z")?,
        ];
        let aim = [
            length(ctx, &self.aim_x, "照らす点 X")?,
            length(ctx, &self.aim_y, "照らす点 Y")?,
            length(ctx, &self.aim_z, "照らす点 Z")?,
        ];
        let d = [0, 1, 2].map(|i| aim[i] - position[i]);
        let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if spot && n < 1e-6 {
            return Err(bad(
                "照らす点が光源の位置と重なっています。離してください".into()
            ));
        }
        let direction = if n < 1e-6 {
            [0.0, 0.0, -1.0]
        } else {
            d.map(|v| v / n)
        };
        let q = ctx.expr(&self.lumens)?;
        if !q.dim.is_none() || !(0.0..=1.0e7).contains(&q.value) {
            return Err(bad(format!(
                "明るさ `{}` は 0 から 10000000 の lm の数で書いてください",
                self.lumens
            )));
        }
        let cone = angle(ctx, &self.cone, "広がりの角")?.to_degrees();
        if !(1.0..=179.0).contains(&cone) {
            return Err(bad(format!(
                "広がりの角 `{}` は 1〜179 度にしてください",
                self.cone
            )));
        }
        let blur = angle(ctx, &self.blur, "ぼかし")?.to_degrees();
        if !(0.0..=cone / 2.0).contains(&blur) {
            return Err(bad(format!(
                "ぼかし `{}` は 0 から広がりの角の半分({:.1} 度)にしてください",
                self.blur,
                cone / 2.0
            )));
        }
        let radius = length(ctx, &self.size, "大きさ")?;
        if !(0.0..=10_000.0).contains(&radius) {
            return Err(bad(format!(
                "大きさ `{}` は 0 から 10000 mm にしてください",
                self.size
            )));
        }
        let color = parse_color(&self.color).ok_or_else(|| {
            bad(format!(
                "色 `{}` は #rrggbb か r,g,b(0〜255)で書いてください",
                self.color
            ))
        })?;
        Ok(Lamp {
            spot,
            position,
            direction,
            color,
            lumens: q.value,
            cone_deg: cone,
            blur_deg: blur,
            radius,
        })
    }
}

impl Feature for Light {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        expr_refs(self)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let lamp = self.lamp(ctx)?;
        Ok([(decor::LAMP.to_string(), lamp.to_value())].into())
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

/// デカール。例: `{"face": {"face": {"feature": 2, "face": "f2:side:e1"}}, "file": "ロゴ.png",
/// "image": "<base64>", "px": [400, 200], "x": "0 mm", "y": "0 mm", "angle": "0 deg",
/// "width": "40 mm", "opacity": "1"}`。`x`・`y` は面の中心からのずれ(面の平面の右と上)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decal {
    #[serde(default, with = "jc_core::one_or_many")]
    pub face: Vec<Ref>,
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
    #[serde(default = "one")]
    pub opacity: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip_h: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip_v: bool,
}

impl Exprs for Decal {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let Decal {
            x,
            y,
            angle,
            width,
            opacity,
            ..
        } = self;
        vec![x, y, angle, width, opacity]
    }
}

fn key_of(image: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    image.hash(&mut h);
    h.finish()
}

impl Decal {
    /// 絵のファイルの中身から作る(4 MB を超えるファイルは断り、大きすぎる絵は縮める)。
    pub fn from_bytes(file: &str, bytes: &[u8], face: Ref) -> Result<Decal, String> {
        if bytes.len() > MAX_BYTES {
            return Err(format!(
                "絵のファイルは 4 MB までです(今 {:.1} MB)。縮めてから貼ってください",
                bytes.len() as f64 / 1024.0 / 1024.0
            ));
        }
        let (bytes, w, h) = jc_image::fit(bytes, MAX_SIDE)?;
        Ok(Decal {
            face: vec![face],
            file: file.into(),
            image: jc_format::base64::encode(&bytes),
            px: [w, h],
            x: zero(),
            y: zero(),
            angle: zero_deg(),
            width: width_default(),
            opacity: one(),
            flip_h: false,
            flip_v: false,
        })
    }

    /// 貼る面(ボディの機能と面の名前)。
    fn target(&self) -> Result<(jc_core::FeatureId, String), EvalError> {
        match self.face.as_slice() {
            [Ref::Face { feature, face }] => Ok((*feature, face.clone())),
            [] => Err(bad("デカールを貼る面を選んでください".into())),
            _ => Err(bad("デカールは面を 1 つだけ選んでください".into())),
        }
    }
}

impl Feature for Decal {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = Vec::new();
        if let Ok((f, _)) = self.target() {
            v.push(output(f, "body"));
        }
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let (body, face) = self.target()?;
        let shape: ShapeHandle = jc_cad::body_of(ctx, body)?;
        let faces = ctx.kernel().faces(shape).map_err(jc_cad::kernel_err)?;
        // 面が無くなったら、別の面に付け替えずに止める(黙って壊さない)
        if !faces.iter().any(|f| f.name == face) {
            return Err(bad(format!(
                "デカールを貼った面 {face} がボディにありません。面を選び直してください"
            )));
        }
        let width = length(ctx, &self.width, "幅")?;
        if width <= 0.0 {
            return Err(bad(format!(
                "幅 `{}` は 0 より大きくしてください",
                self.width
            )));
        }
        let [pw, ph] = self.px;
        if pw == 0 || ph == 0 {
            return Err(bad("絵の幅か高さが 0 です".into()));
        }
        let q = ctx.expr(&self.opacity)?;
        if !q.dim.is_none() || !(0.0..=1.0).contains(&q.value) {
            return Err(bad(format!(
                "不透明度 `{}` は 0 から 1(0% から 100%)にしてください",
                self.opacity
            )));
        }
        let out = decor::Decal {
            body,
            face,
            image: self.image.clone(),
            key: key_of(&self.image),
            px: self.px,
            offset: [length(ctx, &self.x, "X")?, length(ctx, &self.y, "Y")?],
            width,
            angle: angle(ctx, &self.angle, "回転")?,
            opacity: q.value,
            flip_h: self.flip_h,
            flip_v: self.flip_v,
        };
        Ok([(decor::DECAL.to_string(), out.to_value())].into())
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        Descriptor::new(LIGHT, jc_core::category::SOLID, "光源")
            .icon("bulb")
            .hint("点光源かスポットライトを置く(レンダリングとキャンバス内レンダリングで照らし、影を落とす)")
            .aliases("こうげん 光源 らいと ライト light でんきゅう 電球 すぽっと スポット spot lamp")
            .field(Field::choice(
                "kind",
                "種類",
                &[("point", "点光源"), ("spot", "スポットライト")],
            ))
            .field(Field::expr("x", "位置 X").length())
            .field(Field::expr("y", "位置 Y").length())
            .field(Field::expr("z", "位置 Z").length())
            .field(
                Field::expr("aim_x", "照らす点 X")
                    .length()
                    .only("kind", "spot", "point"),
            )
            .field(
                Field::expr("aim_y", "照らす点 Y")
                    .length()
                    .only("kind", "spot", "point"),
            )
            .field(
                Field::expr("aim_z", "照らす点 Z")
                    .length()
                    .only("kind", "spot", "point"),
            )
            .field(Field::text("color", "色").help("#rrggbb か r,g,b(0〜255)"))
            .field(
                Field::expr("lumens", "明るさ")
                    .plain("lm")
                    .help("光束(lm)。800 lm は 60 W の電球ほど。30 cm 離すと約 700 lx"),
            )
            .field(
                Field::expr("cone", "広がりの角")
                    .angle()
                    .only("kind", "spot", "point"),
            )
            .field(
                Field::expr("blur", "縁のぼかし")
                    .angle()
                    .only("kind", "spot", "point"),
            )
            .field(
                Field::expr("size", "大きさ(半径)")
                    .length()
                    .help("大きいほど影の縁がぼける。0 は点"),
            ),
        |d| Ok(Box::new(parse::<Light>(d)?)),
    );
    reg.register_described(
        Descriptor::new(DECAL, jc_core::category::SOLID, "デカール")
            .icon("decal")
            .role(jc_interchange::role::kind::DECAL)
            .hint("絵を面に貼る(形は変えない。3D のビューとレンダリングに出る)")
            .aliases(
                "でかーる デカール decal しーる シール ろご ロゴ がぞう 画像 えをはる 絵を貼る",
            )
            .field(Field::pick("face", "面", &[RefKind::Face], 1, Some(1)))
            .field(Field::info("file", "ファイル"))
            .field(
                Field::expr("x", "ずらす X")
                    .length()
                    .help("面の中心から、面の右へずらす量"),
            )
            .field(
                Field::expr("y", "ずらす Y")
                    .length()
                    .help("面の中心から、面の上へずらす量"),
            )
            .field(
                Field::expr("width", "幅")
                    .length()
                    .help("絵の横の長さ。縦は絵の縦横の比で決まる"),
            )
            .field(Field::expr("angle", "回転").angle())
            .field(Field::expr("opacity", "不透明度").ratio())
            .field(Field::bool("flip_h", "水平方向に反転"))
            .field(Field::bool("flip_v", "垂直方向に反転"))
            .no_raw_editor(),
        |d| Ok(Box::new(parse::<Decal>(d)?)),
    );
}

/// 見えているボディ全体の箱(無ければ `None`)。
fn bodies_box(cx: &Context<'_>) -> Option<([f64; 3], [f64; 3])> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (_, h) in cx.visible_bodies() {
        let Ok((a, b)) = cx.kernel.bounds(h) else {
            continue;
        };
        for k in 0..3 {
            lo[k] = lo[k].min(a[k]);
            hi[k] = hi[k].max(b[k]);
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

fn mm(v: f64) -> String {
    format!("{} mm", (v * 10.0).round() / 10.0)
}

/// 光源を足す。ボディの手前の左上に置き、ボディの中心を照らす。明るさはボディの中心で約 1000 lx
/// (既定の環境の光と同じくらい)。
fn insert_light(cx: &Context<'_>) -> Result<Outcome, String> {
    let spot = match cx.arg_str("kind").unwrap_or("point") {
        "point" => false,
        "spot" => true,
        k => return Err(format!("光源の種類 {k} は使えません(point か spot)")),
    };
    let (lo, hi) = bodies_box(cx).unwrap_or(([-25.0; 3], [25.0; 3]));
    let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
    let s = (0..3).map(|k| hi[k] - lo[k]).fold(50.0, f64::max);
    let pos = [c[0] - 0.6 * s, c[1] - 0.9 * s, hi[2] + 0.9 * s];
    let d_m = (0..3).map(|k| (pos[k] - c[k]).powi(2)).sum::<f64>().sqrt() / 1000.0;
    let lumens = (1000.0 * 4.0 * std::f64::consts::PI * d_m * d_m / 10.0).round() * 10.0;
    let light = Light {
        kind: if spot { "spot" } else { "point" }.into(),
        x: mm(pos[0]),
        y: mm(pos[1]),
        z: mm(pos[2]),
        aim_x: mm(c[0]),
        aim_y: mm(c[1]),
        aim_z: mm(c[2]),
        color: white(),
        lumens: format!("{}", lumens.max(10.0)),
        cone: cone(),
        blur: blur(),
        size: mm((s * 0.03).max(1.0)),
    };
    log::info!(
        "光源を足す({}、位置 {:?}、{} lm)",
        light.kind,
        pos,
        light.lumens
    );
    Ok(Outcome::insert(
        "光源を足す",
        LIGHT,
        if spot {
            "スポットライト"
        } else {
            "点光源"
        }
        .into(),
        serde_json::to_value(light).map_err(|e| e.to_string())?,
    ))
}

/// 貼る面: 引数 `face`、無ければ選んでいる面。
fn decal_face(cx: &Context<'_>) -> Result<Ref, String> {
    if let Some(v) = cx.arg("face") {
        let r: Ref = match v {
            serde_json::Value::Array(a) if a.len() == 1 => serde_json::from_value(a[0].clone()),
            v => serde_json::from_value(v.clone()),
        }
        .map_err(|e| format!("face は面 1 つで書きます: {e}"))?;
        return match r {
            Ref::Face { .. } => Ok(r),
            _ => Err("face には面を渡します".into()),
        };
    }
    let p = cx
        .picked
        .ok_or("デカールを貼る面を選んでから押してください")?;
    Ok(Ref::Face {
        feature: p.feature,
        face: p.face.clone(),
    })
}

/// 面の大きさから、初めの幅(面の面積の平方根の 6 割、1 mm 単位)。
fn first_width(cx: &Context<'_>, face: &Ref) -> Option<f64> {
    let Ref::Face { feature, face } = face else {
        return None;
    };
    let Some(Value::Shape(h)) = cx.output(*feature, "body") else {
        return None;
    };
    let f = cx
        .kernel
        .faces(*h)
        .ok()?
        .into_iter()
        .find(|f| f.name == *face)?;
    Some((f.area.max(0.0).sqrt() * 0.6).round().max(1.0))
}

/// 絵のファイルを読んで、選んだ面にデカールを貼る。
fn insert_decal(cx: &Context<'_>) -> Result<Outcome, String> {
    let face = decal_face(cx)?;
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
    let mut decal =
        Decal::from_bytes(&file, &bytes, face.clone()).map_err(|e| format!("{file}: {e}"))?;
    if let Some(w) = first_width(cx, &face) {
        decal.width = mm(w);
    }
    log::info!(
        "デカール {file} を貼る({}×{} 画素、{} バイト、幅 {})",
        decal.px[0],
        decal.px[1],
        bytes.len(),
        decal.width
    );
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "デカール".into());
    Ok(Outcome::insert(
        "デカールを貼る",
        DECAL,
        name,
        serde_json::to_value(decal).map_err(|e| e.to_string())?,
    ))
}

/// 光源とデカールを足す操作(レンダリングの「設定」)。
pub struct SceneCommands;

impl Extension for SceneCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        vec![
            Action::new(LIGHT_INSERT, "光源", "render")
                .arg(Arg::choice(
                    "kind",
                    "種類(既定は点光源)",
                    &[("point", "点光源"), ("spot", "スポットライト")],
                ))
                .menu("設定")
                .icon("bulb")
                .aliases("こうげん 光源 らいと ライト light でんきゅう 電球 すぽっと スポット spot lamp")
                .hint("点光源かスポットライトを、ボディの手前の上に置く。位置・照らす点・明るさ(lm)・色は編集パネルで直す")
                .at(&[Place::Toolbar]),
            Action::new(DECAL_INSERT, "デカール…", "render")
                .arg(Arg::file("path", "絵のファイル(省くとファイルを選ぶ窓を出す)"))
                .arg(Arg::pick("face", "貼る面(省くと選んでいる面)", &[RefKind::Face]))
                .menu("設定")
                .icon("decal")
                .aliases("でかーる デカール decal しーる シール ろご ロゴ がぞう 画像 えをはる 絵を貼る")
                .hint("選んでいる面に絵(PNG・JPEG・BMP、4 MB まで)を貼る。絵は文書に入れて持つ")
                .at(&[Place::Toolbar])
                .input(Input::OpenFile {
                    title: "デカールの絵を選ぶ".into(),
                    filter: "絵(PNG・JPEG・BMP)".into(),
                    extensions: jc_image::KINDS
                        .iter()
                        .flat_map(|e| [e.to_string(), e.to_uppercase()])
                        .collect(),
                }),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        Some(match id {
            LIGHT_INSERT => insert_light(cx),
            DECAL_INSERT => insert_decal(cx),
            _ => return None,
        })
    }
}

pub fn module() -> jc_core::Module {
    jc_core::Module::new("jc-scene", register).with_extension(SceneCommands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_read_as_hex_or_three_numbers() {
        assert_eq!(parse_color("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_color("10, 20,30"), Some([10, 20, 30]));
        assert_eq!(parse_color("1,2"), None);
        assert_eq!(parse_color("#12345"), None);
    }

    #[test]
    fn a_too_large_picture_is_refused() {
        let face = Ref::Face {
            feature: jc_core::FeatureId(1),
            face: "f1:end".into(),
        };
        let big = vec![0u8; MAX_BYTES + 1];
        let e = Decal::from_bytes("x.png", &big, face).unwrap_err();
        assert!(e.contains("4 MB"), "{e}");
    }
}
