//! 絵と動画の操作(レンダリングの作業空間の「レンダリング」「書き出し」と、ファイルのメニュー)。
//!
//! - glTF(.glb)を書き出す: ボディの網と材質(色・金属・粗さ)。ほかの 3D のソフトで開ける
//! - 静止画(.png): 光線追跡で描き、ノイズを取る(写真のような絵)
//! - 回す動画(.mp4・.gif・連番 .png): ボディのまわりを 1 周する。コマは速い描き方(影あり)。
//!   MP4 は何も入れずに書ける(Windows は OS の H.264、使えない時とほかの OS は AV1)
//! - アニメーションの動画(.mp4・.gif・連番 .png): 作業空間「アニメーション」の段の帯(分解・
//!   ジョイントの動き・ビュー・表示/非表示・外観。[`kmb_interchange::animation`])を初めから終わりまで。
//!   ビューの段があればコマごとにその時刻の視点、無ければ画面のビューのまま
//!
//! 画面は保存先を選ばせ、時間のかかる絵と動画は別のスレッドで描いて選んだファイルへ書く。
//! CLI・MCP(保存先を渡さない時)はその場で描いて中身を返す。引数は書き出しの操作と同じ
//! (`exclude` は隠したボディ、`view` は今の視点の向き)に、`camera`(画面のビューの視点そのもの)を足す。

use crate::{Mp4Codec, Rgba, VideoFormat};
use kmb_core::{Action, Arg, Context, Extension, Input, Outcome, Place, ShapeHandle};
use kmb_render::{Background, Camera, EnvKind, Look, Object, RenderScene, SceneLook};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::RwLock;

mod frames;
mod scene;

pub use frames::*;
pub use scene::*;

/// ボディを glTF(.glb)に書き出す。
pub const EXPORT_GLTF: &str = "render.export_gltf";
/// 光線追跡の静止画(.png)。
pub const RENDER_STILL: &str = "render.still";
/// ボディのまわりを 1 周する動画(.mp4・.gif・連番 .png)。
pub const RENDER_TURNTABLE: &str = "render.turntable";
/// 段の帯(分解とジョイントの動き)の動画(.mp4・.gif・連番 .png)。
pub const ANIMATION_VIDEO: &str = "render.animation_video";
/// シーンの設定(環境・背景・大きさ・画質・動画の長さ)の名前。
pub const SCENE_SETTINGS: &str = "render.scene";

/// 書き出すファイルの(名前の後ろ, 中身)の並び。
type Files = Vec<(String, Vec<u8>)>;

/// 形の変わる物の動画で、材質の名前の無い物の色。
const UNNAMED_COLOR: [u8; 3] = [168, 172, 178];

/// 網にする許容差(mm)。
const TOLERANCE: f64 = 0.02;

/// 絵と動画の操作。
pub struct RenderCommands {
    pub(crate) store: std::sync::Arc<SceneStore>,
}

/// 再計算で誤りになったデカールの機能の数。デカールは形を変えないので、静止画はその分を除いて
/// 描ける(誤りの機能は出力が無いので、描く物の一式には入らない)。
fn decal_errors(cx: &Context<'_>) -> usize {
    cx.report
        .features
        .iter()
        .filter(|f| matches!(f.status, kmb_core::FeatureStatus::Error { .. }))
        .filter(|f| {
            cx.data
                .features
                .iter()
                .find(|e| e.id == f.id)
                .and_then(|e| cx.registry.descriptor(&e.type_id))
                .is_some_and(|d| d.role == Some(kmb_interchange::role::kind::DECAL))
        })
        .count()
}

/// 書き出すボディ(名前と形)。隠したボディ(引数 `exclude`)は除く。再計算に誤りがあれば断る
/// (`decals_ok` なら、誤りがデカールだけの時は断らない)。
fn named_bodies(
    cx: &Context<'_>,
    decals_ok: bool,
) -> Result<Vec<(String, kmb_core::FeatureId, ShapeHandle)>, String> {
    if let Some(why) = bodies_problem(cx, decals_ok) {
        return Err(why);
    }
    let exclude = excluded(cx);
    let all = cx.visible_bodies();
    let ids: Vec<kmb_core::FeatureId> = all.iter().map(|(id, _)| *id).collect();
    let labels = kmb_core::body_labels(cx.data, cx.registry, &ids);
    Ok(labels
        .into_iter()
        .zip(all.iter())
        .filter(|(_, (id, _))| !exclude.contains(&id.0))
        .map(|(name, (id, h))| (name, *id, *h))
        .collect())
}

/// 引数 `exclude` の機能の番号(書かないボディ)。
fn excluded(cx: &Context<'_>) -> Vec<u64> {
    cx.arg("exclude")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

/// [`named_bodies`] が描けない理由。ボディの名前は作らない(操作の一覧は 1 コマごとに聞くので、
/// ボディが多いと名前づけだけで重い。10-24 v10)。
fn bodies_problem(cx: &Context<'_>, decals_ok: bool) -> Option<String> {
    let skipped = if decals_ok { decal_errors(cx) } else { 0 };
    let n = cx.report.error_count() - skipped;
    if n > 0 {
        return Some(format!("再計算で誤りが {n} 件あります"));
    }
    let exclude = excluded(cx);
    let all = cx.visible_bodies();
    all.iter().all(|(id, _)| exclude.contains(&id.0)).then(|| {
        if all.is_empty() {
            "ボディがありません".into()
        } else {
            "見えているボディがありません(隠したボディは描きません)".into()
        }
    })
}

/// ボディの材質(ボディか入っている部品の外観、無ければ物理材料。材料を付けていなければ既定の
/// 材料の鋼)。部品の外観も受け継ぐ(3D のビューと同じ色にするため)。
fn body_look(cx: &Context<'_>, id: kmb_core::FeatureId) -> Look {
    let root = kmb_core::body_root(cx.data, cx.registry, id);
    let looks = kmb_look::appearance::of(cx.data);
    let m = kmb_look::material::material_of(cx.data, root);
    Look::of(&looks, looks.appearance_of(cx.data, root), Some(&m))
}

/// ボディの網 `mesh` の面ごとの材質(`mesh.face_names` の順。面の外観の無い面は `None`)。
pub fn face_looks(
    data: &kmb_core::DocumentData,
    root: kmb_core::FeatureId,
    mesh: &kmb_core::Mesh,
) -> Vec<Option<Look>> {
    let looks = kmb_look::appearance::of(data);
    if !looks.faces.contains_key(&root) {
        return Vec::new();
    }
    mesh.face_names
        .iter()
        .map(|f| {
            looks
                .face(root, f)
                .and_then(|n| looks.resolve(n))
                .map(|r| Look::resolved(&r))
        })
        .collect()
}

/// 止めていない機能の出力 `slot`(タイムラインの順)。
fn outputs<'a>(cx: &Context<'a>, slot: &'a str) -> impl Iterator<Item = &'a kmb_core::Value> + 'a {
    let data = cx.data;
    let cx = *cx;
    data.features
        .iter()
        .filter(|f| !f.suppressed)
        .filter_map(move |f| cx.output(f.id, slot))
}

/// 文書の光源の機能の、描く光源。
fn lamps(cx: &Context<'_>) -> Vec<kmb_render::lamp::Lamp> {
    kmb_render::lamp::lamps_of(outputs(cx, kmb_look::decor::LAMP))
}

/// ボディ `id` の網の、面ごとの材質(面の外観とデカール)。
fn body_face_looks(
    cx: &Context<'_>,
    id: kmb_core::FeatureId,
    mesh: &kmb_core::Mesh,
) -> Vec<Option<Look>> {
    let root = kmb_core::body_root(cx.data, cx.registry, id);
    let faces = face_looks(cx.data, root, mesh);
    // 読んだ絵の控えは組み立てごとに 1 つ(モジュールの共有の値)。無ければその場で読む
    let images = cx
        .registry
        .service::<kmb_render::decal::DecalImages>()
        .unwrap_or_default();
    let decals = kmb_render::decal::decals_of(
        outputs(cx, kmb_look::decor::DECAL),
        |b| kmb_core::body_root(cx.data, cx.registry, b),
        &images,
        &decode,
    );
    kmb_render::decal::with_decals(
        faces,
        &mesh.face_names,
        &body_look(cx, id),
        decals.get(&root),
    )
}

/// デカールの絵のファイル(base64)を画素にする。
fn decode(base64: &str, max_side: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let bytes = kmb_format::base64::decode(base64).ok_or("base64 を読めません")?;
    kmb_image::decode_rgba(&bytes, max_side)
}

/// 描く物の一式と、ボディの名前(`decals_ok` は [`named_bodies`])。
fn render_scene(cx: &Context<'_>, decals_ok: bool) -> Result<(RenderScene, Vec<String>), String> {
    let mut objects = Vec::new();
    let mut names = Vec::new();
    for (name, id, h) in named_bodies(cx, decals_ok)? {
        let mesh = cx.kernel.mesh(h, TOLERANCE).map_err(|e| e.to_string())?;
        let faces = body_face_looks(cx, id, &mesh);
        objects.extend(Object::with_faces(mesh, body_look(cx, id), &faces));
        names.push(name);
    }
    Ok((RenderScene::new(objects).with_lamps(lamps(cx)), names))
}

fn vec3(v: Option<&Value>) -> Option<[f32; 3]> {
    let a = v?.as_array()?;
    let c: Vec<f32> = a
        .iter()
        .filter_map(|x| x.as_f64().map(|f| f as f32))
        .collect();
    (c.len() == 3 && c.iter().all(|x| x.is_finite())).then(|| [c[0], c[1], c[2]])
}

/// 引数の視点の向き(ヨー・ピッチ、度)。`camera` か `view` の向き、無ければ等角。
fn view_angles(args: &Value) -> (f32, f32) {
    let toward = vec3(args.pointer("/camera/toward")).or_else(|| vec3(args.pointer("/view/eye")));
    match toward {
        Some(t) => {
            let n = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-9);
            let pitch = (t[2] / n).clamp(-1.0, 1.0).asin().to_degrees();
            let yaw = t[1].atan2(t[0]).to_degrees();
            (yaw, pitch.clamp(-89.0, 89.0))
        }
        None => (-60.0, 30.0),
    }
}

/// 引数 `camera`(画面のビューの視点)から。無いか正しくなければ `None`。
fn camera_arg(args: &Value) -> Option<Camera> {
    let c = args.get("camera")?;
    let eye = vec3(c.get("eye"))?;
    let right = vec3(c.get("right"))?;
    let up = vec3(c.get("up"))?;
    let toward = vec3(c.get("toward"))?;
    let fov = c.get("fov").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let h = c.get("ortho_height").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    if !(fov > 0.0 || h > 0.0) {
        return None;
    }
    let mut cam = Camera::from_basis(eye, right, up, toward, fov, h);
    if let Some(s) = c.get("shift").and_then(Value::as_array)
        && s.len() == 2
    {
        cam.shift = [0, 1].map(|i| s[i].as_f64().unwrap_or(0.0) as f32);
    }
    // 焦点の距離(被写界深度。画面のビューの回す中心まで)
    cam.focus = c.get("focus").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Some(cam)
}

/// 動画の速い描き方の指定(MP4 は透明を持てないので、`transparent` が偽なら透明の背景を環境にする)。
fn raster_options(s: &Scene, transparent: bool) -> kmb_render::raster::RasterOptions {
    kmb_render::raster::RasterOptions {
        width: s.video_width,
        height: s.video_height,
        supersample: 2,
        // 透明にするかは呼ぶ側が決める(MP4 は透明を持てない)
        background: match s.look.background {
            _ if transparent => Background::Transparent,
            Background::Transparent => Background::Studio,
            b => b,
        },
        shadows: true,
        ground: s.look.ground.show,
    }
}

/// 静止画を描く(光線追跡とノイズ取り)。
pub fn render_still(scene: &RenderScene, args: &Value, s: &Scene) -> kmb_render::Image {
    render_still_with(scene, args, s, |_| true)
}

/// [`render_still`] と同じだが、1 回回すごとに `after(回した数)` を呼ぶ。偽を返すとそこまでで仕上げる。
pub fn render_still_with(
    scene: &RenderScene,
    args: &Value,
    s: &Scene,
    after: impl FnMut(u32) -> bool,
) -> kmb_render::Image {
    let view_aspect = args
        .get("camera")
        .and_then(|c| c.get("aspect"))
        .and_then(Value::as_f64)
        .map(|a| a as f32);
    let (width, height) = s.still_size(view_aspect);
    let aspect = width as f32 / height as f32;
    let mut cam = camera_arg(args).unwrap_or_else(|| {
        let (min, max) = scene.bounds();
        let (yaw, pitch) = view_angles(args);
        Camera::fit(min, max, yaw, pitch, s.look.fov_y_deg(), aspect)
    });
    s.look.apply_camera(&mut cam);
    let env = s.look.env(s.env, cam.toward_eye());
    let opt = kmb_render::trace::TraceOptions {
        width,
        height,
        background: s.look.background,
        ground: s.look.ground,
        ..Default::default()
    };
    kmb_render::trace::render_with(scene, &cam, &env, &opt, s.passes, true, after)
}

/// 回す動画のコマを描く(速い描き方。影あり)。今の視点の向きから始めて Z のまわりに 1 周する。
/// 透視か平行かは画面のビューに合わせる。
pub fn turntable_frames(
    scene: &RenderScene,
    args: &Value,
    s: &Scene,
    transparent: bool,
) -> Vec<Rgba> {
    let (w, h) = (s.video_width, s.video_height);
    let (min, max) = scene.bounds();
    let (yaw, pitch) = view_angles(args);
    let fov = match camera_arg(args) {
        Some(c) if !c.perspective() => 0.0,
        _ => s.look.fov_y_deg(),
    };
    let start = Camera::fit(min, max, yaw, pitch, fov, w as f32 / h as f32);
    let center = [0, 1, 2].map(|i| (min[i] + max[i]) / 2.0);
    let n = ((s.seconds * s.fps as f32).round() as usize).max(2);
    let opt = raster_options(s, transparent);
    (0..n)
        .map(|k| {
            let cam = start.orbit(center, 360.0 * k as f32 / n as f32);
            let env = s.look.env(s.env, cam.toward_eye());
            let img = kmb_render::raster::render(scene, &cam, &env, &opt);
            Rgba {
                width: img.width,
                height: img.height,
                pixels: img.rgba,
            }
        })
        .collect()
}

pub fn format_arg() -> Arg {
    Arg::choice(
        "format",
        "動画の形式(保存先の拡張子が無い時。省くと mp4)",
        &[("mp4", "MP4"), ("gif", "GIF"), ("png", "連番 PNG")],
    )
}

/// 保存先の拡張子(保存先を渡さない CLI・MCP は引数 `format`)の動画の形式。無ければ MP4。
pub fn video_format(cx: &Context<'_>) -> Result<VideoFormat, String> {
    let ext = cx
        .file
        .and_then(|f| f.extension())
        .and_then(|e| e.to_str())
        .or_else(|| cx.arg_str("format"))
        .filter(|e| !e.is_empty())
        .map(str::to_string);
    match ext {
        None => Ok(VideoFormat::Mp4),
        Some(e) => VideoFormat::from_extension(&e)
            .ok_or_else(|| format!("動画の形式 {e} は使えません(mp4・gif・png)")),
    }
}

/// 中身を `path` に書く(連番 PNG は `path` の名前に番号を付けて並べる)。書いた数を返す。
fn write_out(path: &Path, files: &[(String, Vec<u8>)], seq: bool) -> Result<usize, String> {
    for (tail, bytes) in files {
        let out = if seq {
            kmb_core::sibling_path(path, tail)
        } else {
            path.to_path_buf()
        };
        std::fs::write(&out, bytes).map_err(|e| format!("{} に書けません: {e}", out.display()))?;
        log::info!("書き出した: {}", out.display());
    }
    Ok(files.len())
}

/// 連番 PNG の(番号の名前, 中身)。
fn png_frames(frames: &[Rgba]) -> Vec<(String, Vec<u8>)> {
    frames
        .iter()
        .enumerate()
        .map(|(i, f)| (format!("{:04}.png", i + 1), crate::png_seq::png_bytes(f)))
        .collect()
}

fn gltf(cx: &Context<'_>) -> Result<Outcome, String> {
    let mut bodies = Vec::new();
    let mut triangles = 0;
    for (name, id, h) in named_bodies(cx, false)? {
        let mesh = cx.kernel.mesh(h, TOLERANCE).map_err(|e| e.to_string())?;
        triangles += mesh.triangles.len();
        // 面の外観のあるボディは材質ごとに分け、2 つ目からは「名前 (2)」のように番号を付ける
        let faces = body_face_looks(cx, id, &mesh);
        let parts = Object::with_faces(mesh, body_look(cx, id), &faces);
        let many = parts.len() > 1;
        for (k, part) in parts.into_iter().enumerate() {
            let look = part.look;
            // 木目は網に描けないので、早材と晩材の間の色にする(写実の絵は木目を描く)
            let rgb = match &look.finish.grain {
                Some(g) => [0, 1, 2]
                    .map(|k| (look.color[k] as f32 * 0.65 + g.dark[k] as f32 * 0.35) / 255.0),
                None => look.color.map(|c| c as f32 / 255.0),
            };
            bodies.push(crate::GltfBody {
                name: if many && k > 0 {
                    format!("{name} ({})", k + 1)
                } else {
                    name.clone()
                },
                mesh: part.mesh,
                color: [rgb[0], rgb[1], rgb[2], 1.0],
                metallic: look.metallic(),
                roughness: look.roughness(),
            });
        }
    }
    let mut out = Vec::new();
    crate::write_glb(&bodies, &mut out).map_err(|e| e.to_string())?;
    log::info!(
        "glTF を書き出す: ボディ {} 個、三角形 {triangles} 個",
        bodies.len()
    );
    Ok(Outcome::File {
        suffix: "形状.glb".into(),
        contents: out,
        info: json!({"bodies": bodies.len(), "triangles": triangles}),
    })
}

fn still(cx: &Context<'_>) -> Result<Outcome, String> {
    let (scene, _) = render_scene(cx, true)?;
    // 誤りがデカールだけなら、その分を除いて描き、除いたことを知らせる
    let skipped = decal_errors(cx);
    if skipped > 0 {
        log::warn!("誤りのデカール {skipped} 件を除いて静止画を描く");
    }
    let s = scene_with_args(cx)?;
    let keep = s.gallery_keep;
    let args = cx.args.clone();
    // 描いて PNG にする。`ctl` があれば 1 回ごとに進みを知らせ、中止なら `None`、仕上げならそこまでで描く
    let make = move |ctl: Option<&kmb_core::JobControl>| {
        let t = std::time::Instant::now();
        let mut done = 0;
        let img = render_still_with(&scene, &args, &s, |k| {
            done = k;
            let Some(ctl) = ctl else { return true };
            ctl.progress(kmb_core::JobProgress {
                fraction: k as f32 / s.passes.max(1) as f32,
                note: format!("光線追跡 {k}/{} 回", s.passes),
                finish: Some("ここで仕上げる".into()),
            });
            !ctl.cancelled() && !ctl.finishing()
        });
        if ctl.is_some_and(|c| c.cancelled()) {
            log::info!("静止画のレンダリングを中止した({done}/{} 回)", s.passes);
            return None;
        }
        let png = crate::png_seq::png_bytes(&Rgba {
            width: img.width,
            height: img.height,
            pixels: img.rgba,
        });
        let info = json!({
            "width": s.width, "height": s.height, "passes": s.passes, "passes_done": done,
            "seconds": t.elapsed().as_secs_f64(), "skipped_decals": skipped,
        });
        Some((png, info))
    };
    match cx.file {
        // 画面: 別のスレッドで描いて、選んだファイルへ書く
        Some(path) => {
            let path = path.to_path_buf();
            Ok(Outcome::Job(Box::new(move |ctl: &kmb_core::JobControl| {
                let Some((png, info)) = make(Some(ctl)) else {
                    return kmb_core::JobReport::failed(
                        "静止画のレンダリングを中止しました(ファイルは書いていません)".into(),
                    );
                };
                match write_out(&path, &[(String::new(), png)], false) {
                    Ok(_) => {
                        let text = format!(
                            "静止画を書きました: {}({:.1} 秒{})",
                            path.display(),
                            info["seconds"].as_f64().unwrap_or(0.0),
                            match (info["passes_done"].as_u64(), info["passes"].as_u64()) {
                                (Some(d), Some(n)) if d < n =>
                                    format!("、{n} 回のうち {d} 回で仕上げた"),
                                _ => String::new(),
                            }
                        ) + &skipped_note(skipped);
                        // 画面はこの絵を文書のレンダリング ギャラリーにも残す
                        let mut info = info;
                        info["note"] = json!(format!(
                            "光線追跡 {} 回・{:.1} 秒",
                            info["passes_done"].as_u64().unwrap_or(0),
                            info["seconds"].as_f64().unwrap_or(0.0)
                        ));
                        info["image"] = json!(path.display().to_string());
                        info["gallery_keep"] = json!(keep);
                        kmb_core::JobReport {
                            ok: true,
                            text,
                            info,
                        }
                    }
                    Err(e) => kmb_core::JobReport::failed(e),
                }
            })))
        }
        None => {
            let (png, info) = make(None).ok_or("中止しました")?;
            Ok(Outcome::File {
                suffix: "レンダリング.png".into(),
                contents: png,
                info,
            })
        }
    }
}

/// 静止画の知らせに足す、除いたデカールの断り(除いていなければ空)。
fn skipped_note(skipped: usize) -> String {
    if skipped == 0 {
        return String::new();
    }
    format!("。誤りのデカール {skipped} 件は除いて描きました(貼った面を選び直すと戻ります)")
}

/// シーンの設定に、引数で渡した項目(`width` など、設定と同じ名前)を重ねる。
fn scene_with_args(cx: &Context<'_>) -> Result<Scene, String> {
    Scene::from_json(cx.args, &scene(cx.data, cx.registry))
}

fn turntable(cx: &Context<'_>) -> Result<Outcome, String> {
    let format = video_format(cx)?;
    let (scene, _) = render_scene(cx, false)?;
    let s = scene_with_args(cx)?;
    let args = cx.args.clone();
    // MP4 は透明を持てない
    let transparent = s.transparent() && format != VideoFormat::Mp4;
    let fps = s.fps;
    video_outcome(cx.file, format, fps, "回す動画", move || {
        Ok(turntable_frames(&scene, &args, &s, transparent))
    })
}

impl Extension for RenderCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let why = bodies_problem(cx, false);
        let why_still = bodies_problem(cx, true);
        let save = |title: &str, filter: &str, ext: &[&str]| Input::SaveFile {
            title: title.into(),
            filter: filter.into(),
            extensions: ext.iter().map(|e| e.to_string()).collect(),
        };
        vec![
            Action::new(RENDER_STILL, "レンダリング…", "render")
                .menu("レンダリング")
                .icon("render")
                .aliases("れんだりんぐ render 静止画 写真 png 絵 光線追跡 レイトレース")
                .hint("今のビューを写真のような静止画(PNG)にする。光線追跡で影と映り込みを描き、ノイズを取る。大きさと画質は「シーンの設定」")
                .at(&[Place::Toolbar, Place::FileMenu])
                .input(save("静止画を書き出す", "PNG", &["png"]))
                .args(scene_args())
                .disabled_if(why_still.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES),
            Action::new(RENDER_TURNTABLE, "回す動画…", "render")
                .menu("レンダリング")
                .icon("orbit")
                .aliases("どうが 動画 ターンテーブル turntable mp4 gif 回転 アニメーション")
                .hint("ボディのまわりを 1 周する動画にする。保存の拡張子で形式を選ぶ(.mp4・.gif・.png は連番)")
                .at(&[Place::Toolbar, Place::FileMenu])
                .input(save("回す動画を書き出す", "動画(MP4・GIF・連番 PNG)", &["mp4", "gif", "png"]))
                .args(scene_args().into_iter().chain([format_arg()]))
                .disabled_if(why.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES),
            crate::appearance::action(),
            Action::new(EXPORT_GLTF, "glTF を書き出す…", "render")
                .menu("書き出し")
                .icon("export")
                .aliases("じーえるてぃーえふ gltf glb 3d 書き出し")
                .hint("見えているボディを glTF(.glb)にする。外観と材料の色・金属・粗さも書く(単位は m、上は Y)")
                .at(&[Place::Toolbar, Place::FileMenu])
                .input(save("glTF を書き出す", "glTF", &["glb"]))
                .args([Arg::list("exclude", "書かないボディ(機能の番号の並び。画面は目を閉じたボディを渡す)")])
                .disabled_if(why.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES),
            Action::new(ANIMATION_VIDEO, "アニメーションの動画…", "anim")
                .menu("公開")
                .icon("render")
                .aliases("あにめーしょん アニメーション どうが 動画 分解 ぶんかい mp4 gif 公開")
                .hint("段の帯(分解とジョイントの動き)を初めから終わりまで動画にする。視点は今のビューのまま。保存の拡張子で形式を選ぶ(.mp4・.gif・.png は連番)。大きさと 1 秒の枚数は「シーンの設定」")
                .at(&[Place::Toolbar])
                .input(save("アニメーションの動画を書き出す", "動画(MP4・GIF・連番 PNG)", &["mp4", "gif", "png"]))
                .args(scene_args().into_iter().chain([format_arg()]))
                .disabled_if(why.as_deref().or(kmb_interchange::animation::steps(cx.data)
                    .is_empty()
                    .then_some("段がありません(「分解の移動」「自動分解」か「ジョイントの動き」で足す)")))
                .role(kmb_interchange::role::action::EXPORT_BODIES),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        match id {
            crate::appearance::APPEARANCE => Some(crate::appearance::run(cx)),
            EXPORT_GLTF => Some(gltf(cx)),
            RENDER_STILL => Some(still(cx)),
            RENDER_TURNTABLE => Some(turntable(cx)),
            ANIMATION_VIDEO => Some(animation_video(cx)),
            _ => None,
        }
    }

    fn settings(&self) -> Vec<kmb_core::Settings> {
        vec![settings_group(&self.store.get())]
    }

    fn set_settings(&self, id: &str, value: &Value) -> Option<Result<(), String>> {
        (id == SCENE_SETTINGS)
            .then(|| Scene::from_json(value, &Scene::default()).map(|s| self.store.set(s)))
    }

    fn document_settings(&self, id: &str, data: &kmb_core::DocumentData) -> Option<Value> {
        (id == SCENE_SETTINGS)
            .then(|| document_scene(data))
            .flatten()
            .map(|s| s.to_json())
    }

    fn document_settings_edit(
        &self,
        id: &str,
        value: &Value,
        data: &kmb_core::DocumentData,
    ) -> Option<Result<kmb_core::Edit, String>> {
        (id == SCENE_SETTINGS).then(|| {
            let scene = Scene::from_json(value, &Scene::default())?;
            let Value::Object(m) = scene.to_json() else {
                unreachable!("シーンの設定は表");
            };
            data.set_section(&SceneSection(m))
        })
    }
}

#[cfg(test)]
mod tests;
