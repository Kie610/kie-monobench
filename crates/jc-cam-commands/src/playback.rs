//! シミュレーションの動きの単位の再生。段の帯の「再生」で始め、時刻を動かすたびに、その時刻まで
//! 削った形と工具を 3D のビューに出す。
//!
//! 削る計算は [`jc_sim::Player`](粗い格子)。画面は [`jc_core::Playback`] の再生の帯を出し、
//! 時刻の操作(`cam.play.seek:<秒>`)を実行するだけ。経路が変わったら再生は古いので出さない。

use crate::commands::CamCommands;
use crate::paths_cache::SetupRef;
use crate::{SetupInfo, world_mesh};
use jc_core::{
    Action, Context, FeatureId, Input, JobReport, LineKind, Outcome, OverlayShape, Place, Playback,
    ShapeInk,
};
use jc_sim::{Coloring, Player};
use serde_json::json;
use std::sync::Arc;

/// 面ごとの色の役(`None` は既定の色)と、凡例。色そのものは画面のテーマが決める。
type Colors = (Vec<Option<ShapeInk>>, Vec<(ShapeInk, String)>);
/// 経路の線(点の並びと線の種類)。
type Lines = Vec<(Vec<[f64; 3]>, LineKind)>;

/// 再生を始める(シミュレーションの格子とは別に、粗い格子で用意する)。
pub const PLAY: &str = "cam.play.start";
/// 時刻を動かす。後ろに秒を付ける(例: `cam.play.seek:12.5`)。
pub const PLAY_SEEK: &str = "cam.play.seek:";
/// 前の動き・次の動き。
pub const PLAY_PREV: &str = "cam.play.prev";
pub const PLAY_NEXT: &str = "cam.play.next";
/// 色の付け方(後ろに番号: 0 材料、1 操作、2 比較)。
pub const PLAY_COLOR: &str = "cam.play.color:";
/// 再生をやめる。
pub const PLAY_STOP: &str = "cam.play.stop";
/// 削った形(再生している時はその時刻の形、ほかは最後のシミュレーションの最後の形)を
/// STL に書き出す。
pub const EXPORT_STOCK: &str = "cam.export_stock";

/// 削る様子の動画(.mp4・.gif・連番 .png)。始めから終わりまでを、レンダリングのシーンの設定の
/// 長さに縮めて描く(コマ落としの再生)。
pub const EXPORT_SIM_VIDEO: &str = "cam.export_sim_video";

/// 再生の格子の、長い辺のセルの数。
const PLAY_CELLS: usize = 300;
/// 再生の格子の大きさの下限(mm)。
const PLAY_MIN_CELL: f64 = 0.1;

/// 比べる時の色の役: 許容内・削り不足・削り過ぎ。
const DEVIATION_INKS: [ShapeInk; 3] = [
    jc_interchange::ink::shape::WITHIN,
    jc_interchange::ink::shape::UNDER,
    jc_interchange::ink::shape::OVER,
];
/// 工具の色の役: 刃・シャンク・ホルダー。
const TOOL_INKS: [ShapeInk; 3] = [
    jc_interchange::ink::shape::CUTTER,
    jc_interchange::ink::shape::SHANK,
    jc_interchange::ink::shape::HOLDER,
];

/// 用意した再生。
pub(crate) struct PlayRun {
    setup: FeatureId,
    print: u64,
    info: SetupInfo,
    player: Player,
    coloring: Coloring,
    /// 最後に作った形(時刻と色の付け方で決まる鍵)
    shape: Option<(u64, Arc<OverlayShape>)>,
}

impl PlayRun {
    fn tol(&self) -> f64 {
        (self.player.shown().cell * 0.5).max(0.05)
    }

    fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.print.hash(&mut h);
        self.player.state().at.to_bits().hash(&mut h);
        (self.coloring as u8).hash(&mut h);
        h.finish()
    }

    /// 今の時刻の形と工具(世界座標)。
    fn shape(&mut self) -> Arc<OverlayShape> {
        let key = self.key();
        if let Some((k, s)) = &self.shape
            && *k == key
        {
            return s.clone();
        }
        let p = &self.player;
        let mut mesh = p.mesh(self.coloring, self.tol());
        let names = mesh.face_names.clone();
        let (mut colors, mut legend): Colors = match self.coloring {
            Coloring::Material => (vec![], vec![]),
            Coloring::Operation => {
                let c: Vec<Option<ShapeInk>> = std::iter::once(None)
                    .chain((0..names.len() - 1).map(|k| Some(ShapeInk::Operation(k as u32))))
                    .collect();
                let l = names
                    .iter()
                    .skip(1)
                    .enumerate()
                    .map(|(k, n)| (ShapeInk::Operation(k as u32), n.clone()))
                    .collect();
                (c, l)
            }
            Coloring::Deviation => {
                let t = self.tol();
                let c = std::iter::once(None)
                    .chain(DEVIATION_INKS.iter().map(|c| Some(*c)))
                    .collect();
                let l = vec![
                    (DEVIATION_INKS[0], format!("許容内(±{t:.2} mm)")),
                    (DEVIATION_INKS[1], "削り不足".to_string()),
                    (DEVIATION_INKS[2], "削り過ぎ".to_string()),
                ];
                (c, l)
            }
        };
        // 工具を同じ網に足す(面の番号は後ろへずらす)
        colors.resize(names.len(), None);
        let st = p.state();
        let tool = &p.paths()[st.path.min(p.paths().len().saturating_sub(1))].tool;
        let t = jc_sim::tool_mesh(tool, st.tip);
        let (vbase, fbase) = (mesh.positions.len() as u32, mesh.face_names.len() as u32);
        mesh.positions.extend(t.positions);
        mesh.normals.extend(t.normals);
        mesh.triangles
            .extend(t.triangles.iter().map(|tr| tr.map(|v| v + vbase)));
        mesh.triangle_faces
            .extend(t.triangle_faces.iter().map(|f| f + fbase));
        for (k, n) in t.face_names.into_iter().enumerate() {
            mesh.face_names.push(n);
            colors.push(Some(TOOL_INKS[k.min(2)]));
        }
        if self.coloring != Coloring::Material {
            legend.push((TOOL_INKS[0], format!("工具 T{} {}", tool.number, tool.name)));
        }
        let mesh = world_mesh(&self.info, mesh);
        let shape = Arc::new(OverlayShape {
            key,
            label: format!("再生 {}", clock(st.at)),
            mesh,
            face_colors: colors,
            legend,
            ..Default::default()
        });
        self.shape = Some((key, shape.clone()));
        shape
    }
}

/// 秒を「分:秒.1 桁」にする。
fn clock(s: f64) -> String {
    let m = (s / 60.0).floor();
    format!("{}:{:04.1}", m as u64, s - m * 60.0)
}

impl CamCommands {
    fn play_fresh(&self, s: &SetupRef) -> bool {
        self.play
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|r| r.setup == s.setup && r.print == s.print())
    }

    /// 段の帯に出す再生の様子(今の経路の再生を用意してある時だけ)。
    pub(crate) fn playback(&self, s: &SetupRef) -> Option<Playback> {
        if !self.play_fresh(s) {
            return None;
        }
        let run = self.play.lock().unwrap_or_else(|e| e.into_inner());
        let r = run.as_ref()?;
        let p = &r.player;
        let st = p.state();
        let tp = &p.paths()[st.path.min(p.paths().len().saturating_sub(1))];
        let w = st.tip;
        let info = vec![
            (
                "時刻".into(),
                format!("{} / {}", clock(st.at), clock(p.total())),
            ),
            (
                "操作".into(),
                format!(
                    "{} {}({} / {})",
                    st.path + 1,
                    tp.name,
                    st.path + 1,
                    p.paths().len()
                ),
            ),
            (
                "工具".into(),
                format!("T{} {}、{:.0} rpm", tp.tool.number, tp.tool.name, tp.rpm),
            ),
            (
                "動き".into(),
                if st.rapid {
                    format!("早送り {:.0} mm/min", st.feed)
                } else {
                    format!("切削 {:.0} mm/min", st.feed)
                },
            ),
            (
                "工具の位置".into(),
                format!("X {:.2}  Y {:.2}  Z {:.2}", w[0], w[1], w[2]),
            ),
            (
                "動きの番号".into(),
                format!("{} / {}", st.index + 1, tp.moves.len()),
            ),
        ];
        let names = ["材料", "操作", "比較"];
        let colorings = [Coloring::Material, Coloring::Operation, Coloring::Deviation]
            .iter()
            .enumerate()
            .map(|(k, c)| {
                (
                    names[k].to_string(),
                    format!("{PLAY_COLOR}{k}"),
                    *c == r.coloring,
                )
            })
            .collect();
        let marks = p
            .starts()
            .into_iter()
            .zip(p.paths())
            .map(|(t, tp)| (t, tp.name.clone()))
            .collect();
        Some(Playback {
            total: p.total(),
            at: st.at,
            marks,
            info,
            seek: PLAY_SEEK.into(),
            prev_move: PLAY_PREV.into(),
            next_move: PLAY_NEXT.into(),
            colorings,
            line_toggles: [
                &crate::commands::CUT,
                &crate::commands::RAPID,
                &crate::commands::AHEAD,
            ]
            .map(|k| (k.legend.to_string(), k.ink))
            .to_vec(),
            stop: PLAY_STOP.into(),
        })
    }

    /// 再生しているなら、その形と、今の操作の経路(済んだ所はそのまま、残りは薄い線)。
    pub(crate) fn play_overlay(&self, s: &SetupRef) -> Option<(Arc<OverlayShape>, Lines)> {
        if !self.play_fresh(s) {
            return None;
        }
        let mut run = self.play.lock().unwrap_or_else(|e| e.into_inner());
        let r = run.as_mut()?;
        let shape = r.shape();
        let st = r.player.state().clone();
        let tp = r.player.paths().get(st.path)?;
        let mut lines = play_lines(tp, &r.info, st.index, st.tip);
        if let Some(m) = crate::machine_shape(&r.info) {
            head_lines(&m, &tp.tool, st.tip, &r.info, &mut lines);
        }
        Some((shape, lines))
    }

    /// 削った形の網(世界座標)と名前。再生していればその時刻の形、なければ最後の
    /// シミュレーションの最後の段。どちらも今の経路のものだけ。
    fn cut_shape(&self, cx: &Context<'_>) -> Option<(jc_core::Mesh, String)> {
        let s = self.band_setup(cx)?;
        if self.play_fresh(&s) {
            let run = self.play.lock().unwrap_or_else(|e| e.into_inner());
            let r = run.as_ref()?;
            let m = r.player.shown().to_mesh(r.info.stock_min[2]);
            return Some((
                world_mesh(&r.info, m),
                format!(
                    "{}-{}",
                    s.name,
                    clock(r.player.state().at).replace(':', "分")
                ),
            ));
        }
        let sim = self.sim.lock().unwrap_or_else(|e| e.into_inner());
        let m = sim
            .as_ref()
            .filter(|m| m.setup == s.setup && m.print == s.print())?;
        let last = m.stages.last()?;
        Some((last.mesh.clone(), format!("{}-削った後", s.name)))
    }

    /// 再生まわりの、メニューに出す操作。
    pub(crate) fn play_actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let why = self
            .cut_shape(cx)
            .is_none()
            .then_some("シミュレーションか再生をしてから使う");
        vec![
            Action::new(EXPORT_STOCK, "削った形を STL で書き出す…", "cam")
                .menu("出力")
                .icon("export")
                .aliases("すとっく stock stl けずったかたち しみゅれーしょん")
                .hint("シミュレーションで削った形(再生している時はその時刻の形)を STL にする。格子の粗さの網")
                .at(&[Place::FeatureMenu])
                .input(Input::SaveFile {
                    title: "削った形を書き出す".into(),
                    filter: "STL".into(),
                    extensions: vec!["stl".into()],
                })
                .disabled_if(why),
            Action::new(EXPORT_SIM_VIDEO, "削る様子の動画…", "cam")
                .args(
                    jc_io_render::commands::scene_args()
                        .into_iter()
                        .chain([jc_io_render::commands::format_arg()]),
                )
                .menu("確かめる")
                .icon("play")
                .aliases("どうが 動画 しみゅれーしょん さいせい mp4 gif けずる 様子 タイムラプス")
                .hint("選んだセットアップを始めから終わりまで削る様子を、今のビューの視点で動画にする(.mp4・.gif・.png は連番)。長さと大きさはレンダリングの「シーンの設定」")
                .input(Input::SaveFile {
                    title: "削る様子の動画を書き出す".into(),
                    filter: "動画(MP4・GIF・連番 PNG)".into(),
                    extensions: vec!["mp4".into(), "gif".into(), "png".into()],
                })
                .disabled_if(self.sim_video_why(cx))
                .role(jc_interchange::role::action::EXPORT_BODIES),
        ]
    }

    /// 削る様子の動画を作れない理由。
    fn sim_video_why(&self, cx: &Context<'_>) -> Option<&'static str> {
        match self.band_setup(cx) {
            None => Some("セットアップがありません"),
            Some(s) if s.paths.is_empty() => Some("このセットアップには経路がありません"),
            Some(_) => None,
        }
    }

    /// 削る様子の動画。経路の再生の格子で始めから削り、時刻を等しく分けたコマを描く。
    fn sim_video(&self, cx: &Context<'_>) -> Result<Outcome, String> {
        let format = jc_io_render::commands::video_format(cx)?;
        let s = self.band_setup(cx).ok_or("セットアップがありません")?;
        if s.paths.is_empty() {
            return Err("このセットアップには経路がありません".into());
        }
        let all = (*self.paths.all(cx).0).clone();
        let i = all
            .iter()
            .position(|x| x.setup == s.setup)
            .ok_or("セットアップの計算ができていません")?;
        let size = [0, 1].map(|k| s.info.stock_max[k] - s.info.stock_min[k]);
        let cell = (size[0].max(size[1]) / PLAY_CELLS as f64).max(PLAY_MIN_CELL);
        let kernel = cx.kernel.clone();
        let motion = self.machine_for(&s.info).motion();
        let info = s.info.clone();
        let scene = jc_io_render::commands::Scene::from_json(
            cx.args,
            &jc_io_render::commands::scene(cx.data, cx.registry),
        )?;
        let args = cx.args.clone();
        let transparent = scene.transparent() && format != jc_io_render::VideoFormat::Mp4;
        // ストックは材料の名前の見た目(知らない名前なら既定の色)、工具は鋼
        let stock = if info.material.is_empty() {
            String::new()
        } else {
            info.material.clone()
        };
        let fps = scene.fps;
        jc_io_render::commands::video_outcome(
            cx.file,
            format,
            fps,
            "削る様子の動画",
            move || {
                let mut player = crate::player(kernel.as_ref(), &all, i, cell, &motion)?;
                let total = player.total();
                let n = ((scene.seconds * scene.fps as f32).round() as usize).max(2);
                let tol = (player.shown().cell * 0.5).max(0.05);
                Ok(jc_io_render::commands::shape_frames(
                    n,
                    |k| {
                        player.seek(total * k as f64 / (n - 1) as f64);
                        let st = player.state();
                        let tool = &player.paths()
                            [st.path.min(player.paths().len().saturating_sub(1))]
                        .tool;
                        let tool_mesh = world_mesh(&info, jc_sim::tool_mesh(tool, st.tip));
                        let cut = world_mesh(&info, player.mesh(Coloring::Material, tol));
                        vec![(cut, stock.clone()), (tool_mesh, "鋼".to_string())]
                    },
                    &args,
                    &scene,
                    transparent,
                ))
            },
        )
    }

    pub(crate) fn play_run(&self, cx: &Context<'_>, id: &str) -> Result<Outcome, String> {
        if id == EXPORT_STOCK {
            let (mesh, name) = self
                .cut_shape(cx)
                .ok_or("シミュレーションか再生をしてから使う")?;
            let mut out = Vec::new();
            jc_io_stl::write_binary(std::slice::from_ref(&mesh), &mut out)
                .map_err(|e| e.to_string())?;
            return Ok(Outcome::File {
                suffix: format!("{name}.stl"),
                contents: out,
                info: json!({"triangles": mesh.triangles.len()}),
            });
        }
        if id == PLAY {
            return self.play_start(cx);
        }
        if id == EXPORT_SIM_VIDEO {
            return self.sim_video(cx);
        }
        if id == PLAY_STOP {
            *self.play.lock().unwrap_or_else(|e| e.into_inner()) = None;
            return Ok(Outcome::Message(
                "再生をやめて、部品の形に戻しました".into(),
            ));
        }
        let mut run = self.play.lock().unwrap_or_else(|e| e.into_inner());
        let r = run
            .as_mut()
            .ok_or("再生を始めていません(段の帯の「再生」から始める)")?;
        if let Some(n) = id.strip_prefix(PLAY_COLOR) {
            r.coloring = match n {
                "0" => Coloring::Material,
                "1" => Coloring::Operation,
                "2" => Coloring::Deviation,
                _ => return Err(format!("色の付け方 {n} はありません")),
            };
            return Ok(Outcome::Message(String::new()));
        }
        let p = &mut r.player;
        let t = if let Some(t) = id.strip_prefix(PLAY_SEEK) {
            t.trim()
                .parse::<f64>()
                .map_err(|_| format!("時刻が読めません: {t}"))?
        } else if id == PLAY_NEXT {
            // 今の動きの終わり。もう終わりにいれば次の動きの終わり
            let k = p.current_step();
            let end = p.step_end(k);
            if end > p.state().at + 1e-9 {
                end
            } else {
                p.step_end(k + 1)
            }
        } else if id == PLAY_PREV {
            // 今の動きの始め。もう始めにいれば前の動きの始め
            let k = p.current_step();
            let start = p.step_start(k);
            if start < p.state().at - 1e-9 {
                start
            } else {
                p.step_start(k.saturating_sub(1))
            }
        } else {
            return Err(format!("操作 {id} はありません"));
        };
        p.seek(t);
        Ok(Outcome::Message(String::new()))
    }

    fn play_start(&self, cx: &Context<'_>) -> Result<Outcome, String> {
        let s = self.band_setup(cx).ok_or("セットアップがありません")?;
        if s.paths.is_empty() {
            return Err("このセットアップには経路がありません".into());
        }
        // 段の形を出していたらやめる(再生の形と重ならないように)
        if let Some(m) = self.sim.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            m.selected = None;
        }
        let all = (*self.paths.all(cx).0).clone();
        let i = all
            .iter()
            .position(|x| x.setup == s.setup)
            .ok_or("セットアップの計算ができていません")?;
        let size = [0, 1].map(|k| s.info.stock_max[k] - s.info.stock_min[k]);
        let cell = (size[0].max(size[1]) / PLAY_CELLS as f64).max(PLAY_MIN_CELL);
        let kernel = cx.kernel.clone();
        let store = self.play.clone();
        let motion = self.machine_for(&s.info).motion();
        let (setup, print, info) = (s.setup, s.print(), s.info.clone());
        Ok(Outcome::Job(Box::new(
            move |_: &jc_core::JobControl| match crate::player(
                kernel.as_ref(),
                &all,
                i,
                cell,
                &motion,
            ) {
                Ok(mut player) => {
                    player.seek(0.0);
                    let total = player.total();
                    let moves = player.move_count();
                    *store.lock().unwrap_or_else(|e| e.into_inner()) = Some(PlayRun {
                        setup,
                        print,
                        info,
                        player,
                        coloring: Coloring::Material,
                        shape: None,
                    });
                    JobReport {
                        ok: true,
                        text: format!(
                            "再生の用意ができました(動き {moves} 個、{}、格子 {cell:.2} mm)",
                            clock(total)
                        ),
                        info: json!({"total_seconds": total, "moves": moves, "cell": cell}),
                    }
                }
                Err(e) => JobReport::failed(e),
            },
        )))
    }
}

/// 再生している経路の線(世界座標)。工具が `index` 番目の動きの `tip` にいる時、済んだ動きは
/// その種類の線、今の動きは工具の所で分けて、済んだ側は同じ種類・残りと先の動きは薄い線にする。
/// 今の動きも円弧なら円弧に沿って描き、工具の所へ直線で結ばない。
fn play_lines(tp: &jc_toolpath::Toolpath, info: &SetupInfo, index: usize, tip: [f64; 3]) -> Lines {
    use crate::commands::{AHEAD, line_kind, lines_of_range};
    let mut lines = Vec::new();
    let n = tp.moves.len();
    if n == 0 {
        return lines;
    }
    let k = index.min(n - 1);
    let w = |p: [f64; 3]| info.to_world(p);
    lines_of_range(tp, info, 0..k, &mut lines);
    if k > 0 {
        let from = tp.moves[k - 1].to();
        let kind = line_kind(tp, &tp.roles(), info, k, from);
        let mut pts = vec![from];
        pts.extend(tp.moves[k].points(from, 0.5));
        // 工具にいちばん近い区間で分ける
        let near = (0..pts.len() - 1)
            .min_by(|&a, &b| {
                let d = |i: usize| seg_dist(pts[i], pts[i + 1], tip);
                d(a).total_cmp(&d(b))
            })
            .unwrap_or(0);
        let done: Vec<_> = pts[..=near].iter().copied().chain([tip]).map(w).collect();
        let rest: Vec<_> = [tip]
            .into_iter()
            .chain(pts[near + 1..].iter().copied())
            .map(w)
            .collect();
        lines.push((done, kind));
        lines.push((rest, AHEAD));
    }
    let mut later = Vec::new();
    lines_of_range(tp, info, k..n, &mut later);
    lines.extend(later.into_iter().map(|(p, _)| (p, AHEAD)));
    lines
}

/// 工具の先端が `tip` にある時の主軸の頭(丸い主軸と Z の台の箱)の線(世界座標)。透けて見えるよう
/// 線だけで描く。
fn head_lines(
    m: &jc_sim::MachineShape,
    tool: &jc_cam_tool::Tool,
    tip: [f64; 3],
    info: &SetupInfo,
    out: &mut Lines,
) {
    use crate::commands::MACHINE;
    let w = |p: [f64; 3]| info.to_world(p);
    let nose = tip[2] + jc_sim::nose_height(tool);
    let [sd, sl] = m.spindle;
    if sd > 0.0 {
        let r = sd / 2.0;
        let ring = |z: f64| -> Vec<[f64; 3]> {
            (0..=32)
                .map(|k| {
                    let a = k as f64 / 32.0 * std::f64::consts::TAU;
                    w([tip[0] + r * a.cos(), tip[1] + r * a.sin(), z])
                })
                .collect()
        };
        out.push((ring(nose), MACHINE));
        out.push((ring(nose + sl), MACHINE));
        for k in 0..4 {
            let a = k as f64 * std::f64::consts::FRAC_PI_2;
            let (x, y) = (tip[0] + r * a.cos(), tip[1] + r * a.sin());
            out.push((vec![w([x, y, nose]), w([x, y, nose + sl])], MACHINE));
        }
    }
    let [cw, cd, ch] = m.carriage;
    if cw > 0.0 && cd > 0.0 {
        let z0 = nose + sl.max(0.0);
        let (x0, x1) = (tip[0] - cw / 2.0, tip[0] + cw / 2.0);
        let (y0, y1) = (tip[1] - cd / 2.0, tip[1] + cd / 2.0);
        for z in [z0, z0 + ch] {
            out.push((
                [[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]
                    .iter()
                    .map(|p| w([p[0], p[1], z]))
                    .collect(),
                MACHINE,
            ));
        }
        for [x, y] in [[x0, y0], [x1, y0], [x1, y1], [x0, y1]] {
            out.push((vec![w([x, y, z0]), w([x, y, z0 + ch])], MACHINE));
        }
    }
}

/// 点 `p` から線分 `a`-`b` までの距離。
fn seg_dist(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> f64 {
    let ab = [0, 1, 2].map(|i| b[i] - a[i]);
    let ap = [0, 1, 2].map(|i| p[i] - a[i]);
    let len2: f64 = ab.iter().map(|v| v * v).sum();
    let t = if len2 > 0.0 {
        ((0..3).map(|i| ab[i] * ap[i]).sum::<f64>() / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    jc_toolpath::dist([0, 1, 2].map(|i| a[i] + ab[i] * t), p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_toolpath::{ArcDir, Move, Role, Span, Toolpath};

    /// 下りて、半円の入り(反時計回り)、直線で進む経路。
    fn half_circle() -> Toolpath {
        Toolpath {
            name: "半円".into(),
            tool: jc_cam_tool::ToolLibrary::starter().tools[0].clone(),
            rpm: 10000.0,
            safe_z: 5.0,
            retract_z: 1.0,
            edge_break: 0.0,
            dwells: vec![],
            links: Default::default(),
            moves: vec![
                Move::Rapid {
                    to: [10.0, 0.0, 1.0],
                },
                Move::Linear {
                    to: [10.0, 0.0, -1.0],
                    feed: 200.0,
                },
                Move::Arc {
                    to: [-10.0, 0.0, -1.0],
                    center: [0.0, 0.0],
                    dir: ArcDir::Ccw,
                    feed: 600.0,
                },
                Move::Linear {
                    to: [-10.0, 5.0, -1.0],
                    feed: 600.0,
                },
            ],
            spans: vec![Span {
                start: 2,
                end: 3,
                role: Role::Entry,
            }],
        }
    }

    /// 点 `p` から経路(細かく分けた折れ線)までの距離。
    fn off_path(tp: &Toolpath, p: [f64; 3]) -> f64 {
        let mut pts = vec![tp.moves[0].to()];
        for m in &tp.moves[1..] {
            let from = *pts.last().unwrap();
            pts.extend(m.points(from, 0.05));
        }
        pts.iter()
            .map(|q| jc_toolpath::dist(*q, p))
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn the_lines_while_playing_follow_the_arc_and_keep_its_kind() {
        // 半円の入りの途中(90 度の所)に工具がいる。「前の点 → 工具」「工具 → 次の点」を直線で
        // 結ぶと、円弧を横切る線が工具につながって見え、入りの線も切削の色になってしまう
        let tp = half_circle();
        let i = crate::tests::info(jc_cam_setup::Flip::None);
        let tip = [0.0, 10.0, -1.0];
        let lines = play_lines(&tp, &i, 2, tip);
        let local = |p: [f64; 3]| [p[0] - i.origin[0], p[1] - i.origin[1], p[2] - i.origin[2]];
        for (pts, kind) in &lines {
            for w in pts.windows(2) {
                for t in [0.25, 0.5, 0.75] {
                    let q = local([0, 1, 2].map(|j| w[0][j] + (w[1][j] - w[0][j]) * t));
                    let d = off_path(&tp, q);
                    assert!(d < 0.05, "{kind:?} の線が経路から {d:.2} mm 外れる: {q:?}");
                }
            }
        }
        // 工具の所までの円弧は入りの線、その先は薄い線
        let at_tip = |p: &[f64; 3]| jc_toolpath::dist(local(*p), tip) < 1e-9;
        let before = lines
            .iter()
            .find(|(p, _)| p.last().is_some_and(at_tip))
            .expect("工具の所で終わる線");
        assert_eq!(before.1, crate::commands::ENTRY);
        let after = lines
            .iter()
            .find(|(p, _)| p.first().is_some_and(at_tip))
            .expect("工具の所から始まる線");
        assert_eq!(after.1, crate::commands::AHEAD);
    }
}
