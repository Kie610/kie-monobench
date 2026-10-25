//! 動画のコマを描く所(形の変わる物の動画とアニメーションの段の帯)と、動画を書き出す操作の結果。

use super::*;

/// 動画の中身と、MP4 なら中の符号(連番 PNG は番号の名前)。
pub fn encode_video(
    format: VideoFormat,
    frames: &[Rgba],
    fps: u32,
) -> Result<(Files, Option<Mp4Codec>), String> {
    match format {
        VideoFormat::PngSequence => Ok((png_frames(frames), None)),
        _ => crate::video::encode_with_codec(format, frames, fps as f32, ""),
    }
}

/// 動画を書き出す操作の結果。`make` がコマを描く。画面(保存先 `file` あり)は別のスレッドで描いて
/// 選んだファイルへ書き、CLI・MCP(保存先なし)はその場で描いて中身を返す。`what` は知らせる名前。
pub fn video_outcome(
    file: Option<&Path>,
    format: VideoFormat,
    fps: u32,
    what: &'static str,
    make: impl FnOnce() -> Result<Vec<Rgba>, String> + Send + 'static,
) -> Result<Outcome, String> {
    let run = move || -> Result<(Files, Value), String> {
        let t = std::time::Instant::now();
        let frames = make()?;
        let drawn = t.elapsed().as_secs_f64();
        let (files, codec) = encode_video(format, &frames, fps)?;
        let (w, h) = frames.first().map_or((0, 0), |f| (f.width, f.height));
        let info = json!({
            "frames": frames.len(), "fps": fps, "width": w, "height": h,
            "format": format.extension(),
            "codec": codec.as_ref().map(Mp4Codec::label),
            "note": codec.as_ref().and_then(Mp4Codec::note),
            "draw_seconds": drawn, "seconds": t.elapsed().as_secs_f64(),
        });
        log::info!(
            "{what}: {} 枚を {:.1} 秒で描き、{:.1} 秒で書き出した",
            frames.len(),
            drawn,
            t.elapsed().as_secs_f64()
        );
        Ok((files, info))
    };
    let seq = format == VideoFormat::PngSequence;
    match file {
        Some(path) => {
            let path = path.to_path_buf();
            Ok(Outcome::Job(Box::new(
                move |_: &jc_core::JobControl| match run()
                    .and_then(|(files, info)| write_out(&path, &files, seq).map(|n| (n, info)))
                {
                    Ok((n, info)) => jc_core::JobReport {
                        ok: true,
                        text: if seq {
                            format!("{what}を連番 PNG {n} 枚で書きました: {}", path.display())
                        } else {
                            format!(
                                "{what}を書きました: {}({} 枚、{}{:.1} 秒かかった){}",
                                path.display(),
                                info["frames"],
                                info["codec"]
                                    .as_str()
                                    .map(|c| format!("{c}、"))
                                    .unwrap_or_default(),
                                info["seconds"].as_f64().unwrap_or(0.0),
                                info["note"]
                                    .as_str()
                                    .map(|n| format!("。{n}"))
                                    .unwrap_or_default()
                            )
                        },
                        info,
                    },
                    Err(e) => jc_core::JobReport::failed(e),
                },
            )))
        }
        None => {
            let (files, info) = run()?;
            if seq {
                Ok(Outcome::Files { files, info })
            } else {
                let (_, contents) = files.into_iter().next().ok_or("動画を作れません")?;
                Ok(Outcome::File {
                    suffix: format!("{what}.{}", format.extension()),
                    contents,
                    info,
                })
            }
        }
    }
}

/// 形の変わる物(削る様子など)の動画のコマ。`frame(k)` は k 枚目に描く(網, 外観か材料の名前)の
/// 並び(世界座標。名前が空なら灰色)。視点は引数 `camera`(画面のビュー)、無ければ 1 枚目の形に合わせた等角。
pub fn shape_frames(
    count: usize,
    mut frame: impl FnMut(usize) -> Vec<(jc_core::Mesh, String)>,
    args: &Value,
    s: &Scene,
    transparent: bool,
) -> Vec<Rgba> {
    let (w, h) = (s.video_width, s.video_height);
    let opt = raster_options(s, transparent);
    let mut cam: Option<Camera> = camera_arg(args);
    (0..count)
        .map(|k| {
            let objects = frame(k)
                .into_iter()
                .map(|(m, name)| {
                    // 名前の無い物(材料を決めていないストック)は、明るい背景で削った所が見えるよう少し濃い灰色
                    let look = if name.is_empty() {
                        Look::plain(UNNAMED_COLOR)
                    } else {
                        Look::named(Some(&name), Some(&name))
                    };
                    Object::new(m, look)
                })
                .collect();
            let scene = RenderScene::new(objects);
            let c = *cam.get_or_insert_with(|| {
                let (min, max) = scene.bounds();
                let (yaw, pitch) = view_angles(args);
                Camera::fit(
                    min,
                    max,
                    yaw,
                    pitch,
                    s.look.fov_y_deg(),
                    w as f32 / h as f32,
                )
            });
            let env = s.look.env(s.env, c.toward_eye());
            let img = jc_render::raster::render(&scene, &c, &env, &opt);
            Rgba {
                width: img.width,
                height: img.height,
                pixels: img.rgba,
            }
        })
        .collect()
}

/// 動く物の動画のコマ。`objects` は動かす前の(ボディ, 網, 材質, 面ごとの材質)、`poses` はコマごとの
/// ボディの動き。面の外観は外観の段でも替えない(段はボディの外観を替える)。
/// 視点は引数 `camera`(画面のビュー)、無ければ全部のコマの形を包む箱に合わせた等角。
pub fn posed_frames(
    objects: &[(jc_core::FeatureId, jc_core::Mesh, Look, Vec<Option<Look>>)],
    lamps: &[jc_render::lamp::Lamp],
    frames: &[Frame],
    args: &Value,
    s: &Scene,
    transparent: bool,
) -> Vec<Rgba> {
    let (w, h) = (s.video_width, s.video_height);
    let opt = raster_options(s, transparent);
    let scene_at = |f: &Frame| {
        RenderScene::new(
            objects
                .iter()
                .filter(|(id, _, _, _)| !f.hidden.contains(id))
                .flat_map(|(id, m, look, faces)| {
                    let mesh = match f.poses.get(id) {
                        Some(p) => jc_interchange::animation::transform_mesh(m, p),
                        None => m.clone(),
                    };
                    Object::with_faces(mesh, f.looks.get(id).unwrap_or(look).clone(), faces)
                })
                .collect(),
        )
        .with_lamps(lamps.to_vec())
    };
    let cam = camera_arg(args).unwrap_or_else(|| {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in frames {
            let (a, b) = scene_at(p).bounds();
            for i in 0..3 {
                lo[i] = lo[i].min(a[i]);
                hi[i] = hi[i].max(b[i]);
            }
        }
        let (yaw, pitch) = view_angles(args);
        Camera::fit(lo, hi, yaw, pitch, s.look.fov_y_deg(), w as f32 / h as f32)
    });
    let env = s.look.env(s.env, cam.toward_eye());
    frames
        .iter()
        .map(|p| {
            // ビューの段の視点はコマごと(光の環境も視点の向きに合わせる)
            let moved = p.view.as_ref().map(|v| {
                let c = view_camera(v);
                let e = s.look.env(s.env, c.toward_eye());
                (c, e)
            });
            let (cam, env) = moved.as_ref().map_or((&cam, &env), |(c, e)| (c, e));
            let img = jc_render::raster::render(&scene_at(p), cam, env, &opt);
            Rgba {
                width: img.width,
                height: img.height,
                pixels: img.rgba,
            }
        })
        .collect()
}

/// 段の帯のコマの時刻(秒。初めと終わりを含む)。
pub fn animation_times(total: f64, fps: u32) -> Vec<f64> {
    let n = ((total * fps as f64).round() as usize).max(1) + 1;
    (0..n).map(|k| total * k as f64 / (n - 1) as f64).collect()
}

pub(crate) fn animation_video(cx: &Context<'_>) -> Result<Outcome, String> {
    let steps = &jc_interchange::animation::steps(cx.data);
    if steps.is_empty() {
        return Err(
            "アニメーションの段がありません。「分解の移動」「自動分解」か「ジョイントの動き」で段を足してください"
                .into(),
        );
    }
    let format = video_format(cx)?;
    let s = scene_with_args(cx)?;
    // 隠したボディも網を作っておく(表示/非表示の段で見せることがある)。隠したボディは
    // コマの `hidden` に入れて描かない
    named_bodies(cx, false)?;
    let exclude: std::collections::BTreeSet<jc_core::FeatureId> = cx
        .arg("exclude")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_u64)
                .map(jc_core::FeatureId)
                .collect()
        })
        .unwrap_or_default();
    let mut objects = Vec::new();
    for (id, h) in cx.visible_bodies() {
        let mesh = cx.kernel.mesh(h, TOLERANCE).map_err(|e| e.to_string())?;
        let faces = body_face_looks(cx, id, &mesh);
        objects.push((id, mesh, body_look(cx, id), faces));
    }
    let lamps = lamps(cx);
    let total = jc_interchange::animation::duration(steps);
    let frames: Vec<Frame> = animation_times(total, s.fps)
        .into_iter()
        .map(|t| frame_at(cx, &exclude, t))
        .collect();
    log::info!(
        "アニメーションの動画: 段 {} 個、{total:.1} 秒、{} 枚",
        steps.len(),
        frames.len()
    );
    let args = cx.args.clone();
    let transparent = s.transparent() && format != VideoFormat::Mp4;
    let fps = s.fps;
    video_outcome(cx.file, format, fps, "アニメーション", move || {
        Ok(posed_frames(
            &objects,
            &lamps,
            &frames,
            &args,
            &s,
            transparent,
        ))
    })
}

/// アニメーションの動画の 1 コマの様子。
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// ボディ(最後の機能)ごとの動き
    pub poses: std::collections::BTreeMap<jc_core::FeatureId, jc_core::kernel::Matrix4>,
    /// 描かないボディ(最後の機能)
    pub hidden: std::collections::BTreeSet<jc_core::FeatureId>,
    /// 外観の段で替えたボディ(最後の機能)の材質
    pub looks: std::collections::BTreeMap<jc_core::FeatureId, Look>,
    /// ビューの段の視点(無ければ引数の視点か、全部のコマを包む箱に合わせる)
    pub view: Option<jc_interchange::animation::ViewPose>,
}

/// 時刻 `t` のコマの様子(画面で隠したボディ `exclude` は、表示/非表示の段で見せない限り描かない)。
pub(crate) fn frame_at(
    cx: &Context<'_>,
    exclude: &std::collections::BTreeSet<jc_core::FeatureId>,
    t: f64,
) -> Frame {
    use jc_interchange::animation as anim;
    let steps = &anim::steps(cx.data);
    let shown = anim::shown_at(cx.data, steps, t);
    let blends = anim::appearances_at(cx.data, steps, t);
    let mut hidden = std::collections::BTreeSet::new();
    let mut looks = std::collections::BTreeMap::new();
    let all = jc_look::appearance::of(cx.data);
    let materials = jc_look::material::of(cx.data);
    let color_of = |name: &str| all.resolve(name).map(|a| a.color);
    for (id, _) in cx.visible_bodies() {
        let root = jc_core::body_root(cx.data, cx.registry, id);
        let hide = shown.get(&root).map_or(exclude.contains(&id), |v| !v);
        if hide {
            hidden.insert(id);
        }
        let Some(b) = blends.get(&root) else {
            continue;
        };
        let m = materials.material_of(root);
        let own_name = all.appearance_of(cx.data, root);
        let look = if b.f <= 0.0 || b.f >= 1.0 {
            Look::of(&all, b.shown().or(own_name), Some(m))
        } else {
            // 替えている途中は、前の色と替えた先の色を混ぜた塗装
            let own = own_name
                .and_then(color_of)
                .or_else(|| jc_look::material::phys_material(m).map(|p| p.color))
                .unwrap_or([150, 150, 150]);
            let from = b.from.as_deref().and_then(color_of).unwrap_or(own);
            let to = color_of(&b.to).unwrap_or(from);
            Look::plain(b.mix(from, to))
        };
        looks.insert(id, look);
    }
    Frame {
        poses: anim::body_poses(cx.data, cx.registry, cx.report, steps, t),
        hidden,
        looks,
        view: anim::view_at(steps, t),
    }
}

/// ビューの段の視点の、写実の絵の視点(画面のカメラと同じ式。見える高さで大きさを合わせる)。
pub fn view_camera(v: &jc_interchange::animation::ViewPose) -> Camera {
    let (r, u, e) = jc_render::camera::basis(v.yaw as f32, v.pitch as f32);
    let t = v.target.map(|x| x as f32);
    let h = v.height as f32;
    if v.perspective {
        let fov = jc_render::camera::FOV_DEG;
        let dist = 0.5 * h / (fov.to_radians() / 2.0).tan();
        let eye = [0, 1, 2].map(|i| t[i] + e[i] * dist);
        Camera::from_basis(eye, r, u, e, fov, 0.0)
    } else {
        // 平行投影の視点は、場面より十分後ろに置く(光線の起点にする。画面のカメラと同じ)
        let back = h * 50.0 + 1000.0;
        let eye = [0, 1, 2].map(|i| t[i] + e[i] * back);
        Camera::from_basis(eye, r, u, e, 0.0, h)
    }
}
