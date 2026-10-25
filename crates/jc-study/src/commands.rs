//! 解析の作業空間の操作: 研究の実行・中止と様子、結果の見せ方(どの量を色で出すか、断面・流線・
//! 変形・コマ)、結果の数値、3D のビューに重ねる色分けの形。出力 `plan` を出す研究(区分が研究の
//! 機能)ならどれにも効く(強度と熱の jc-fea、流れの jc-flow)。

use crate::result::{Geometry, Role, SimResult, fmt};
use crate::run::{PLAN, Plan, Runner, Status, clock, prepared};
use crate::view::{self, Look};
use jc_core::{
    Action, Context, Extension, FeatureId, FeatureStatus, FrameBar, Outcome, Overlay, OverlayShape,
    Payload, Place, RunState, RunTone, StepMark, Value,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const RUN: &str = "study.run";
pub const CANCEL: &str = "study.cancel";
pub const VIEW_QUANTITY: &str = "study.view.quantity.";
pub const VIEW_FRAME: &str = "study.view.frame.";
pub const VIEW_SURFACE: &str = "study.view.surface";
pub const VIEW_SECTION: &str = "study.view.section";
pub const VIEW_STREAM: &str = "study.view.streamlines";
pub const VIEW_SMOOTH: &str = "study.view.smooth";
pub const VIEW_RANGE: &str = "study.view.range";
pub const VIEW_DEFORM: &str = "study.view.deform";
pub const VIEW_HIDE: &str = "study.view.hide";
pub const VIEW_NUMBERS: &str = "study.view.numbers";

/// 研究ごとの見せ方(結果が来た時に、結果の勧める見せ方で埋める)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
struct Show {
    /// 埋めた結果の番号(違う結果が来たら埋め直す)
    from: u64,
    surface: Option<usize>,
    section: bool,
    section_q: usize,
    streamlines: bool,
    smooth: bool,
    banded: bool,
    frame_only: bool,
    no_deform: bool,
    hidden: bool,
    frame: usize,
}

/// 研究の実行と結果の見せ方。
#[derive(Default)]
pub struct StudyCommands {
    runner: Runner,
    show: Mutex<HashMap<(u64, u64), Show>>,
    /// 描いた形の使い回し(鍵と形)
    drawn: Mutex<Option<(u64, Option<Arc<OverlayShape>>)>>,
}

fn is_study(cx: &Context<'_>, type_id: &str) -> bool {
    cx.registry.category(type_id) == Some(jc_core::category::STUDY)
}

/// 選んでいる研究。
fn selected(cx: &Context<'_>) -> Option<FeatureId> {
    let id = cx.selected?;
    let e = cx.data.feature(id)?;
    is_study(cx, &e.type_id).then_some(id)
}

fn plan(cx: &Context<'_>, id: FeatureId) -> Option<Plan> {
    Plan::from_value(cx.output(id, PLAN)?)
}

/// 研究が評価できない理由(条件が足りないなど)。
fn problem(cx: &Context<'_>, id: FeatureId) -> String {
    match cx.report.feature(id) {
        Some(FeatureStatus::Error { message }) => format!("条件を直してください: {message}"),
        Some(FeatureStatus::Blocked { by, .. }) => format!("{by} が使えません"),
        Some(FeatureStatus::Suppressed) => "抑制しています".into(),
        Some(FeatureStatus::RolledBack) => "ロールバックの位置より後ろです".into(),
        _ => "条件を読めていません(再計算してください)".into(),
    }
}

fn key_of(at: (u64, u64)) -> u64 {
    let mut h = crate::result::Fnv::default();
    h.eat(&at.0.to_le_bytes());
    h.eat(&at.1.to_le_bytes());
    h.0
}

/// 四面体の網の断面を塗る量(表面の量。表面を塗っていなければ、四面体の網の最初の量)。
fn tets_section(r: &SimResult, surface: Option<usize>) -> Option<usize> {
    let on_tets = |q: usize| {
        r.quantities
            .get(q)
            .and_then(|d| r.parts.get(d.part))
            .is_some_and(|p| matches!(p.geom, Geometry::Tets(_)))
    };
    surface
        .filter(|q| on_tets(*q))
        .or_else(|| (0..r.quantities.len()).find(|q| on_tets(*q)))
}

/// 量 `q` の網(四面体)の範囲。
fn tets_bounds(r: &SimResult, q: usize) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    if let Some(Geometry::Tets(t)) = r
        .quantities
        .get(q)
        .and_then(|d| r.parts.get(d.part))
        .map(|p| &p.geom)
    {
        for p in &t.points {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a] as f64);
                hi[a] = hi[a].max(p[a] as f64);
            }
        }
    }
    if lo[0] > hi[0] {
        return ([0.0; 3], [0.0; 3]);
    }
    (lo, hi)
}

impl StudyCommands {
    fn at(cx: &Context<'_>, id: FeatureId) -> (u64, u64) {
        (cx.doc, id.0)
    }

    /// いちばん新しい結果と、それが古いか。
    pub fn result(&self, cx: &Context<'_>, id: FeatureId) -> Option<(Arc<SimResult>, bool)> {
        let d = self.runner.done(Self::at(cx, id))?;
        let stale = plan(cx, id).is_none_or(|p| p.key != d.key);
        Some((d.result, stale))
    }

    fn show_of(&self, cx: &Context<'_>, id: FeatureId, r: &SimResult) -> Show {
        let at = Self::at(cx, id);
        let stamp = r.stamp();
        let Ok(mut m) = self.show.lock() else {
            return Show::default();
        };
        let s = m.entry(at).or_default();
        if s.from != stamp {
            let keep = *s;
            let q = r.start.quantity.min(r.quantities.len().saturating_sub(1));
            let on_grid = r
                .quantities
                .get(q)
                .and_then(|d| r.parts.get(d.part))
                .is_some_and(|p| matches!(p.geom, Geometry::Grid(_)));
            let first_grid = r
                .quantities
                .iter()
                .position(|d| {
                    r.parts
                        .get(d.part)
                        .is_some_and(|p| matches!(p.geom, Geometry::Grid(_)))
                })
                .unwrap_or(0);
            *s = Show {
                from: stamp,
                surface: (!on_grid && !r.quantities.is_empty()).then_some(q),
                section: on_grid || r.start.section,
                section_q: if on_grid { q } else { first_grid },
                streamlines: r.start.streamlines,
                // 見せ方の好みは同じ研究の次の結果にも残す
                smooth: keep.smooth,
                banded: keep.banded,
                frame_only: keep.frame_only,
                no_deform: keep.no_deform,
                hidden: keep.hidden,
                frame: if keep.from == 0 {
                    r.frames.len().saturating_sub(1)
                } else {
                    keep.frame.min(r.frames.len().saturating_sub(1))
                },
            };
            // 最初は最後のコマ(落ち着いた流れ)を出す
            if keep.from == 0 {
                s.frame = r.frames.len().saturating_sub(1);
            }
        }
        *s
    }

    fn set_show(&self, cx: &Context<'_>, id: FeatureId, f: impl FnOnce(&mut Show)) {
        if let Ok(mut m) = self.show.lock() {
            f(m.entry(Self::at(cx, id)).or_default());
        }
    }

    fn look(&self, cx: &Context<'_>, id: FeatureId, r: &SimResult, stale: bool) -> Look {
        let s = self.show_of(cx, id, r);
        let grid = r.parts.iter().find_map(|p| match &p.geom {
            Geometry::Grid(g) => Some(g),
            _ => None,
        });
        // 断面の平面: 2D は格子の面、3D は研究の欄(無ければ物体の真ん中の水平)
        let plane = grid.map(|g| {
            if g.flat() {
                (g.axes[2], g.origin[2] * g.signs[2])
            } else {
                plan(cx, id).and_then(|p| p.section).unwrap_or_else(|| {
                    let z = g.center(0, 0, g.n[2] / 2)[2];
                    (g.axes[2], z * g.signs[2])
                })
            }
        });
        // 四面体の網は、形を平面で切って表面と同じ量で切り口を塗る
        let tets = tets_section(r, s.surface).map(|q| {
            let plane = plan(cx, id).and_then(|p| p.section).unwrap_or_else(|| {
                let (lo, hi) = tets_bounds(r, q);
                (2, 0.5 * (lo[2] + hi[2]))
            });
            (q, plane)
        });
        let section = match tets {
            Some((q, (a, v))) => Some((q, a, v)),
            None => plane.map(|(a, v)| (s.section_q, a, v)),
        };
        Look {
            frame: s.frame,
            surface: s.surface,
            section: section.filter(|_| s.section),
            streamlines: s.streamlines,
            smooth: !s.banded,
            deform: !s.no_deform,
            legend_all: !s.frame_only,
            stale,
        }
    }

    fn shape(&self, cx: &Context<'_>, id: FeatureId) -> Option<Arc<OverlayShape>> {
        let (r, stale) = self.result(cx, id)?;
        let s = self.show_of(cx, id, &r);
        if s.hidden {
            return None;
        }
        let look = self.look(cx, id, &r, stale);
        let others: Vec<_> = if r.bodies.is_empty() {
            Vec::new()
        } else {
            cx.visible_bodies()
                .into_iter()
                .filter(|(b, _)| !r.bodies.contains(&b.0))
                .collect()
        };
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            r.stamp().hash(&mut h);
            key_of(Self::at(cx, id)).hash(&mut h);
            format!("{look:?}").hash(&mut h);
            if !others.is_empty() {
                // ほかのボディが変わったら描き直す
                cx.report.stamp().hash(&mut h);
            }
            h.finish()
        };
        if let Ok(d) = self.drawn.lock()
            && let Some((k, s)) = d.as_ref()
            && *k == key
        {
            return s.clone();
        }
        let mut shape = view::draw(&r, &look, key);
        for (_, h) in others {
            if let Ok(m) = cx.kernel.mesh(h, 0.2) {
                add_backdrop(&mut shape, &m);
            }
        }
        let shape = Some(Arc::new(shape));
        if let Ok(mut d) = self.drawn.lock() {
            *d = Some((key, shape.clone()));
        }
        shape
    }

    /// 研究 `id` を解く。`wait` ならこのスレッドで終わるまで(CLI・MCP・試験)。
    pub fn run_study(
        &self,
        cx: &Context<'_>,
        id: FeatureId,
        wait: bool,
    ) -> Result<Option<Arc<SimResult>>, String> {
        let Some(p) = plan(cx, id) else {
            return Err(problem(cx, id));
        };
        let task = prepared(cx.registry, p.key)
            .ok_or("条件を読み直してから実行してください(再計算すると読み直します)")?;
        let at = Self::at(cx, id);
        if wait {
            self.runner.solve_now(at, p.key, task.as_ref()).map(Some)
        } else {
            self.runner.start(at, p.key, p.steps.clone(), task);
            Ok(None)
        }
    }

    /// どれかの研究が計算中か。
    pub fn busy(&self) -> bool {
        self.runner.busy()
    }
}

/// 研究 `id` を今のスレッドで解いて結果を返す(試験・CLI 向け。画面の結果には残さない。
/// 画面に残すには操作 `study.run` を引数 `{"wait": true}` で使う)。
pub fn solve_now(cx: &Context<'_>, id: FeatureId) -> Result<SimResult, String> {
    let p = plan(cx, id).ok_or_else(|| problem(cx, id))?;
    let task =
        prepared(cx.registry, p.key).ok_or("解く準備が見つかりません(再計算してください)")?;
    task.solve(&crate::run::Control::new())
}

/// 結果の形に、解いていないボディの網 `m` を灰色(最後の「色なし」の面)で足す。
pub fn add_backdrop(s: &mut OverlayShape, m: &jc_core::Mesh) {
    let gray = s.face_colors.len().saturating_sub(1) as u32;
    let base = s.mesh.positions.len() as u32;
    s.mesh.positions.extend(&m.positions);
    if m.normals.len() == m.positions.len() {
        s.mesh.normals.extend(&m.normals);
    } else {
        s.mesh
            .normals
            .extend(std::iter::repeat_n([0.0, 0.0, 1.0], m.positions.len()));
    }
    for t in &m.triangles {
        s.mesh.triangles.push(t.map(|i| i + base));
        s.mesh.triangle_faces.push(gray);
    }
}

/// 結果の数値の表(読むための文)。
pub fn numbers_text(name: &str, r: &SimResult) -> String {
    let mut out = format!("{name}\n");
    for (k, v) in &r.numbers {
        out.push_str(&format!("{k}: {v}\n"));
    }
    if !r.note.is_empty() {
        out.push('\n');
        out.push_str(&r.note);
    }
    out
}

fn toggle(id: &str, on: bool, label_on: &str, label_off: &str, hint: &str) -> Action {
    let places = [Place::Panel, Place::FeatureMenu];
    Action::new(id, if on { label_on } else { label_off }, "fea")
        .menu("結果")
        .hint(hint)
        .at(&places)
}

impl Extension for StudyCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let sel = selected(cx);
        let mut out = Vec::new();
        // 実行と中止(リボンの「実行」のまとまりと、研究の右クリック)
        let running = sel.is_some_and(|id| {
            matches!(self.runner.status(Self::at(cx, id)), Status::Running { .. })
        });
        let run_why = match sel {
            None => Some("研究を選んでから実行します".to_string()),
            Some(_) if running => Some("計算中です".into()),
            Some(id) if plan(cx, id).is_none() => Some(problem(cx, id)),
            _ => None,
        };
        let places = [Place::Toolbar, Place::FeatureMenu];
        let mut run = Action::new(RUN, "実行", "fea")
            .arg(jc_core::Arg::flag("wait", "解き終わるまで待って結果を返す(CLI・MCP)"))
            .menu("実行")
            .icon("play")
            .hint("選んだ研究を解く。研究は自動では解きません。条件や形を変えると結果に「古い」が付きます")
            .aliases("じっこう とく かいせき 実行 解く solve run")
            .at(&places);
        run.disabled = run_why;
        out.push(run);
        let mut cancel = Action::new(CANCEL, "中止", "fea")
            .menu("実行")
            .icon("x")
            .hint("計算をやめる(流れは、ここまでのコマを結果に残す)")
            .aliases("ちゅうし やめる 中止 cancel stop")
            .at(&places);
        if !running {
            cancel.disabled = Some("計算していません".into());
        }
        out.push(cancel);
        let Some(id) = sel else {
            return out;
        };
        let Some((r, _)) = self.result(cx, id) else {
            return out;
        };
        let s = self.show_of(cx, id, &r);
        let places = [Place::Panel, Place::FeatureMenu];
        for (i, q) in r.quantities.iter().enumerate() {
            if q.role == Role::Tensor {
                continue;
            }
            let on_grid = r
                .parts
                .get(q.part)
                .is_some_and(|p| matches!(p.geom, Geometry::Grid(_)));
            let shown =
                !s.hidden && (s.surface == Some(i) || (on_grid && s.section && s.section_q == i));
            let label = match (shown, on_grid) {
                (true, true) => format!("● 断面: {}", q.name),
                (false, true) => format!("断面: {}", q.name),
                (true, false) => format!("● {}", q.name),
                (false, false) => q.name.clone(),
            };
            out.push(
                Action::new(&format!("{VIEW_QUANTITY}{i}"), &label, "fea")
                    .menu("結果")
                    .hint(&if on_grid {
                        format!("断面を {} で塗り分けて出す", q.name)
                    } else {
                        format!("{} を色で塗り分けて出す", q.name)
                    })
                    .at(&places),
            );
        }
        let has_grid = r.parts.iter().any(|p| matches!(p.geom, Geometry::Grid(_)));
        let has_surface_q = r.quantities.iter().any(|q| {
            r.parts
                .get(q.part)
                .is_some_and(|p| matches!(p.geom, Geometry::Surface { .. }))
        });
        let flat = r
            .parts
            .iter()
            .any(|p| matches!(&p.geom, Geometry::Grid(g) if g.flat()));
        if has_grid && has_surface_q {
            out.push(toggle(
                VIEW_SURFACE,
                s.surface.is_some(),
                "表面の色を隠す",
                "表面の色を出す",
                "物体の表面の色(表面の圧力など)を出す・灰色にする",
            ));
        }
        let has_tets = r.parts.iter().any(|p| matches!(p.geom, Geometry::Tets(_)));
        if has_tets {
            out.push(toggle(
                VIEW_SECTION,
                s.section,
                "断面を隠す",
                "断面で切る",
                "形を平面で切って、中の値を色で出す(位置は研究の「断面」の欄。解き直さずに変えられる)",
            ));
        }
        if has_grid && !flat {
            out.push(toggle(
                VIEW_SECTION,
                s.section,
                "断面を隠す",
                "断面を出す",
                "流体の所だけを塗った断面を出す(位置は研究の「断面」の欄。解き直さずに変えられる)",
            ));
        }
        if r.quantities.iter().any(|q| q.role == Role::Velocity) {
            out.push(toggle(
                VIEW_STREAM,
                s.streamlines,
                "流線を隠す",
                "流線を出す",
                "上流から流れに乗って進む線を、速さで塗って出す",
            ));
        }
        out.push(toggle(
            VIEW_SMOOTH,
            s.banded,
            "滑らかに塗る",
            "帯で塗る",
            "色を滑らかにするか、値の帯(10 段)に分けるか",
        ));
        if r.frames.len() > 1 && r.frames.iter().any(|f| f.time.is_some()) {
            out.push(toggle(
                VIEW_RANGE,
                s.frame_only,
                "色の範囲を全部の時刻で決める",
                "色の範囲を今の時刻で決める",
                "凡例の範囲を全部の時刻で揃える(既定。時刻を送っても色の意味が変わらない)か、今のコマだけで取り直すか",
            ));
        }
        if r.quantities.iter().any(|q| q.role == Role::Deform) {
            out.push(toggle(
                VIEW_DEFORM,
                !s.no_deform,
                "変形を隠す",
                "変形を出す",
                "変形・揺れの形を誇張して出す(倍率は色の名前の横に出る)",
            ));
        }
        out.push(toggle(
            VIEW_HIDE,
            !s.hidden,
            "結果を隠す",
            "結果を出す",
            "色分けをやめてボディを出す(面を選び直す時に)",
        ));
        out.push(
            Action::new(VIEW_NUMBERS, "結果の数値", "fea")
                .menu("結果")
                .hint("最大の応力・変位・安全率、温度、固有振動数、揚力・抗力などの表")
                .at(&places),
        );
        out
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        if id != RUN && id != CANCEL && !id.starts_with("study.view.") {
            return None;
        }
        let Some(fid) = selected(cx) else {
            return Some(Err("解析の研究を選んでから使う".into()));
        };
        Some(match id {
            RUN => {
                let wait = cx.arg("wait").and_then(|v| v.as_bool()) == Some(true);
                self.run_study(cx, fid, wait).map(|r| match r {
                    Some(r) => Outcome::Message(format!("解きました: {}", r.summary)),
                    None => Outcome::Message("計算を始めました。進み具合はパネルに出ます".into()),
                })
            }
            CANCEL => {
                if self.runner.cancel(Self::at(cx, fid)) {
                    Ok(Outcome::Message("計算を止めます".into()))
                } else {
                    Err("計算していません".into())
                }
            }
            VIEW_NUMBERS => match self.result(cx, fid) {
                Some((r, stale)) => {
                    let name = cx
                        .data
                        .feature(fid)
                        .map(|e| e.name.clone())
                        .unwrap_or_default();
                    let mut body = numbers_text(&name, &r);
                    if stale {
                        body.push_str(
                            "\n\n条件か形が変わったので、この結果は古いです(実行で解き直します)",
                        );
                    }
                    Ok(Outcome::Text {
                        title: format!("{name} の結果"),
                        body,
                    })
                }
                None => Err("まだ結果がありません(「実行」で解きます)".into()),
            },
            _ => {
                let Some((r, _)) = self.result(cx, fid) else {
                    return Some(Err("まだ結果がありません(「実行」で解きます)".into()));
                };
                // 見せ方の初めの値を埋めてから変える
                self.show_of(cx, fid, &r);
                let last = r.frames.len().saturating_sub(1);
                let tail = |p: &str| id.strip_prefix(p).and_then(|s| s.parse::<usize>().ok());
                if let Some(i) = tail(VIEW_QUANTITY) {
                    // 応力の 6 成分は塗る量ではない(一覧にも出さない)
                    if r.quantities.get(i).is_none_or(|q| q.role == Role::Tensor) {
                        return Some(Err(format!("{i} 番の量は塗れません")));
                    }
                    let on_grid = r
                        .quantities
                        .get(i)
                        .and_then(|q| r.parts.get(q.part))
                        .is_some_and(|p| matches!(p.geom, Geometry::Grid(_)));
                    self.set_show(cx, fid, |s| {
                        if on_grid {
                            s.section = true;
                            s.section_q = i;
                        } else {
                            s.surface = Some(i);
                        }
                        s.hidden = false;
                    });
                } else if let Some(f) = tail(VIEW_FRAME) {
                    self.set_show(cx, fid, |s| s.frame = f.min(last));
                } else {
                    let first_surface = r.quantities.iter().position(|q| {
                        r.parts
                            .get(q.part)
                            .is_some_and(|p| matches!(p.geom, Geometry::Surface { .. }))
                    });
                    match id {
                        VIEW_SURFACE => self.set_show(cx, fid, |s| {
                            s.surface = match s.surface {
                                Some(_) => None,
                                None => first_surface,
                            }
                        }),
                        VIEW_SECTION => self.set_show(cx, fid, |s| s.section = !s.section),
                        VIEW_STREAM => self.set_show(cx, fid, |s| s.streamlines = !s.streamlines),
                        VIEW_SMOOTH => self.set_show(cx, fid, |s| s.banded = !s.banded),
                        VIEW_RANGE => self.set_show(cx, fid, |s| s.frame_only = !s.frame_only),
                        VIEW_DEFORM => self.set_show(cx, fid, |s| s.no_deform = !s.no_deform),
                        VIEW_HIDE => self.set_show(cx, fid, |s| s.hidden = !s.hidden),
                        _ => return None,
                    }
                }
                Ok(Outcome::Message(String::new()))
            }
        })
    }

    fn overlay(&self, cx: &Context<'_>) -> Overlay {
        let Some(id) = selected(cx) else {
            return Overlay::default();
        };
        Overlay {
            shape: self.shape(cx, id),
            ..Overlay::default()
        }
    }

    fn frames(&self, cx: &Context<'_>) -> Option<FrameBar> {
        let id = selected(cx)?;
        let (r, _) = self.result(cx, id)?;
        if r.frames.len() < 2 {
            return None;
        }
        let s = self.show_of(cx, id, &r);
        if s.hidden {
            return None;
        }
        let playable = r.frames.iter().all(|f| f.time.is_some());
        Some(FrameBar {
            labels: r.frames.iter().map(|f| f.label.clone()).collect(),
            at: s.frame.min(r.frames.len() - 1),
            total: r.frames.last().map(|f| f.label.clone()).unwrap_or_default(),
            playable,
            fps: 12.0,
            seek: VIEW_FRAME.into(),
        })
    }

    fn run_state(&self, cx: &Context<'_>, id: FeatureId) -> Option<RunState> {
        let e = cx.data.feature(id)?;
        if !is_study(cx, &e.type_id) {
            return None;
        }
        let at = Self::at(cx, id);
        let plan = plan(cx, id);
        let mut st = RunState {
            run: RUN.into(),
            run_label: "実行".into(),
            run_disabled: plan.is_none().then(|| problem(cx, id)),
            ..RunState::default()
        };
        match self.runner.status(at) {
            Status::Running {
                secs,
                steps,
                progress,
                ..
            } => {
                let pct = (progress.fraction * 100.0).floor() as u32;
                st.tone = RunTone::Busy;
                st.badge = format!("計算中 {pct}%");
                let f = progress.fraction as f64;
                st.detail = if f > 0.03 {
                    format!(
                        "経過 {}・あと約 {}",
                        clock(secs),
                        clock(secs * (1.0 - f) / f)
                    )
                } else {
                    format!("経過 {}", clock(secs))
                };
                st.fraction = Some(progress.fraction);
                st.steps = steps
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        if i < progress.step {
                            (StepMark::Done, name.clone())
                        } else if i == progress.step {
                            let n = if progress.note.is_empty() {
                                name.clone()
                            } else {
                                format!("{name} {}", progress.note)
                            };
                            (StepMark::Now, n)
                        } else {
                            (StepMark::Todo, name.clone())
                        }
                    })
                    .collect();
                st.series = progress.series;
                st.run_disabled = Some("計算中です".into());
                let partial = plan.as_ref().is_some_and(|p| p.keeps_partial);
                st.cancel = Some((
                    CANCEL.into(),
                    if partial {
                        "中止(ここまでのコマは残す)".into()
                    } else {
                        "中止".into()
                    },
                ));
            }
            Status::Done { key, secs } => {
                let r = self.runner.done(at)?.result;
                let fresh = plan.as_ref().is_some_and(|p| p.key == key);
                st.series = r.series.first().cloned();
                let frames = if r.frames.len() > 1 {
                    format!("({} コマ)", r.frames.len())
                } else {
                    String::new()
                };
                if fresh {
                    st.tone = RunTone::Done;
                    st.badge = if r.partial { "途中まで" } else { "済み" }.into();
                    st.detail = format!("{} で計算{frames}", clock(secs));
                } else {
                    st.tone = RunTone::Stale;
                    st.badge = "古い".into();
                    st.detail = "条件か形が変わったので、出している結果は古いです".into();
                    st.run_label = "実行(解き直す)".into();
                }
            }
            Status::Idle => {
                st.tone = RunTone::Idle;
                st.badge = "未実行".into();
                st.detail = "「実行」を押すと解きます(自動では解きません)".into();
            }
        }
        if !matches!(st.tone, RunTone::Busy)
            && let Some((key, e)) = self.runner.failure(at)
            && plan.as_ref().is_some_and(|p| p.key == key)
        {
            st.tone = RunTone::Failed;
            st.badge = "失敗".into();
            st.detail = e;
        }
        Some(st)
    }

    fn summary(&self, cx: &Context<'_>, id: FeatureId) -> Option<String> {
        let (r, stale) = self.result(cx, id)?;
        let mut s = r
            .numbers
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        if !r.note.is_empty() {
            s.push_str("\n\n");
            s.push_str(&r.note);
        }
        if stale {
            s.insert_str(0, "(古い結果)\n");
        }
        Some(s)
    }

    fn describe(&self, v: &Value) -> Option<String> {
        Plan::from_value(v).map(|_| "解く準備ができた研究(「実行」で解く)".into())
    }

    fn status(&self, cx: &Context<'_>) -> Vec<(String, serde_json::Value)> {
        let list: Vec<serde_json::Value> = cx
            .data
            .features
            .iter()
            .filter(|f| is_study(cx, &f.type_id))
            .map(|f| {
                let r = self.result(cx, f.id);
                let st = self.run_state(cx, f.id);
                serde_json::json!({
                    "id": f.id.0,
                    "name": f.name,
                    "type": f.type_id,
                    "state": st.as_ref().map(|s| s.badge.clone()),
                    "summary": r.as_ref().map(|r| r.0.summary.clone()),
                    "numbers": r.as_ref().map(|r| r.0.numbers.clone()),
                    "stale": r.as_ref().map(|r| r.1),
                    "frames": r.as_ref().map(|r| r.0.frames.len()),
                })
            })
            .collect();
        if list.is_empty() {
            return Vec::new();
        }
        vec![("studies".into(), serde_json::Value::Array(list))]
    }
}

/// 数を短く(結果の表と同じ)。
pub fn short(v: f64) -> String {
    fmt(v)
}
