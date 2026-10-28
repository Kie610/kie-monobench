//! 1 つの輪郭を削る手順(閉じた輪と開いた線)と、利用者への知らせのまとめ。

use super::*;

/// 利用者に知らせることを集める(輪郭ごとに出さず、最後に 1 回ずつ)。
#[derive(Default)]
pub(super) struct Notes {
    pub(super) lead_missing: bool,
    pub(super) no_depth: bool,
    pub(super) too_small: bool,
    pub(super) helix_fallback: bool,
    pub(super) outside_fallback: bool,
    pub(super) predrill_missing: bool,
    pub(super) open_lines: bool,
    pub(super) on_unused: bool,
    pub(super) entry_far: bool,
}

impl Notes {
    pub(super) fn report(&self, w: &mut Vec<String>, kind: EntryKind) {
        if self.lead_missing {
            w.push(
                "入りと抜けの円弧が入らない輪郭があるので、そこは壁の上で下ります\
                 (円弧の半径を小さくしてください)"
                    .into(),
            );
        }
        if self.too_small {
            w.push("工具が入らない輪郭があるので、そこは削りません".into());
        }
        if self.no_depth {
            w.push("削る深さがない輪郭があるので、そこは削りません".into());
        }
        if self.helix_fallback {
            w.push(format!(
                "らせんが入らない輪郭があるので、そこは輪郭の上を斜めに下ります(入り方: {})",
                kind.label()
            ));
        }
        if self.outside_fallback {
            w.push(
                "ストックの外から削る所へまっすぐ入れない輪郭があるので、そこは輪郭の上を斜めに下ります"
                    .into(),
            );
        }
        if self.predrill_missing {
            w.push(
                "下穴から入るには、入る位置を下穴の中心に置いてください。置いていない輪郭は真下に下ります"
                    .into(),
            );
        }
        if self.entry_far {
            w.push(
                "入る位置から輪郭の始まりへ、削ってよい所だけを通って進めないので、輪郭の始まりで下ります"
                    .into(),
            );
        }
        if self.open_lines {
            w.push(
                "開いた線は工具の中心でなぞります(削る側・タブ・仕上げ・入りと抜けの円弧は使いません)"
                    .into(),
            );
        }
        if self.on_unused {
            w.push(
                "線の上をなぞる時は、仕上げ代・横の荒取り・入りと抜けの円弧・らせん・ストックの外からの入りを使いません"
                    .into(),
            );
        }
    }
}

/// 1 つの輪郭を削るのに使う値。
pub(super) struct Cut<'a> {
    pub(super) c: &'a Contour,
    pub(super) p: &'a Planner,
    pub(super) plan: &'a EntryPlan,
    pub(super) lead: Option<LeadShape>,
    pub(super) top: f64,
    pub(super) floor: f64,
    pub(super) bottom: f64,
    pub(super) entry: Option<P2>,
    pub(super) leave: f64,
    pub(super) finish: f64,
    pub(super) rough_step: f64,
    pub(super) tab: (f64, f64),
    pub(super) fin: FinishPlan,
    /// タブの付く経路を集める(手で置く手順で見せる)
    pub(super) tab_paths: &'a std::cell::RefCell<Vec<TabPath>>,
}

impl Cut<'_> {
    /// 閉じた輪 `l`(反時計回り)を削る。何か削れば真。
    pub(super) fn closed(
        &self,
        tp: &mut Toolpath,
        l: &Loop,
        tool_out: bool,
        on: bool,
        n: &mut Notes,
    ) -> bool {
        let p = self.p;
        let c = self.c;
        let shift = |d: f64| -> Vec<Loop> {
            if d.abs() < 1e-12 {
                vec![l.clone()]
            } else if tool_out {
                offset_loop_outward(l, d)
            } else {
                offset_loop_outward(l, -d)
            }
        };
        if on && (self.finish > 1e-9 || c.rough_passes > 1 || self.lead.is_some()) {
            n.on_unused = true;
        }
        let (rough_d, finish) = if on {
            (0.0, 0.0)
        } else {
            (p.r() + self.leave + self.finish, self.finish)
        };
        let rough = shift(rough_d);
        if rough.is_empty() {
            n.too_small = true;
            return false;
        }
        let free_inside = !tool_out;
        // 工具の中心が入ってよい側(線の上をなぞる時は、どちらも削る所ではないので使わない)
        let lead = if on { None } else { self.lead };
        let passes = if on { 1 } else { c.rough_passes.max(1) };
        for path in rough {
            // ダウンカットなら外側は時計回り、内側は反時計回り
            let path = c.direction.apply(if tool_out || on {
                path.reversed()
            } else {
                path
            });
            let fits = |q: P2| on || path.contains(q) == free_inside;
            // 横の荒取りの輪(遠い順、最後が `path`)
            let mut rings: Vec<Loop> = Vec::new();
            for k in (1..passes).rev() {
                let d = self.rough_step * k as f64;
                let outer = if tool_out {
                    offset_loop_outward(&path, d)
                } else {
                    offset_loop_outward(&path, -d)
                };
                for r in outer {
                    let r = c.direction.apply(if tool_out { r.reversed() } else { r });
                    rings.push(r);
                }
            }
            let last = rings.len();
            rings.push(path.clone());
            // 始まりを決める
            let use_mid =
                lead.is_some() || matches!(self.plan.kind, EntryKind::Helix | EntryKind::Outside);
            let first = match self.entry {
                Some(e) => start_at(&rings[0], e),
                None if use_mid => start_mid_line(&rings[0]),
                None => rings[0].clone(),
            };
            rings[0] = first;
            let arcs = if rings.len() == 1 {
                lead.and_then(|s| {
                    let a = leads(
                        &rings[0],
                        (s.radius, s.sweep, s.line),
                        free_on_left(&rings[0], free_inside),
                        fits,
                    );
                    n.lead_missing |= a.is_none();
                    a
                })
            } else {
                None
            };
            let way = self.way(&rings[0], arcs, free_inside, on, &fits, n);
            approach(tp, way.d);
            let first_move = tp.moves.len();
            let from = tp
                .moves
                .last()
                .map_or([way.d[0], way.d[1], tp.safe_z], |m| m.to());
            let levels = p.levels(self.top, self.bottom);
            let mut prev = tp.retract_z.min(self.top);
            let n_levels = levels.len();
            // 前の段の終わりで、入りの円弧の抜けまで行ったか
            let mut away = false;
            let start0 = [rings[0].verts[0].x, rings[0].verts[0].y];
            for (i, z) in levels.into_iter().enumerate() {
                let ramp_mode = matches!(way.down, Down::Ramp);
                // 下りる所 `d` へ戻る
                if i > 0 {
                    if away {
                        approach(tp, way.d);
                        if prev < tp.retract_z {
                            let s = tp.moves.len();
                            tp.moves.push(Move::Linear {
                                to: [way.d[0], way.d[1], prev],
                                feed: p.cutting.feed,
                            });
                            tp.mark(s, Role::Link);
                        }
                    } else if !ramp_mode {
                        let cur = tp.moves.last().map_or(start0, |m| [m.to()[0], m.to()[1]]);
                        if dist(cur, way.d) > 1e-9 {
                            // 前の段で通った道を戻る(削れている)
                            let s = tp.moves.len();
                            if dist(cur, start0) > 1e-9 {
                                tp.moves.push(Move::Linear {
                                    to: [start0[0], start0[1], prev],
                                    feed: p.cutting.feed,
                                });
                            }
                            if dist(start0, way.t) > 1e-9 {
                                tp.moves.push(Move::Linear {
                                    to: [way.t[0], way.t[1], prev],
                                    feed: p.cutting.feed,
                                });
                            }
                            tp.moves.push(Move::Linear {
                                to: [way.d[0], way.d[1], prev],
                                feed: p.cutting.feed,
                            });
                            tp.mark(s, Role::Link);
                        }
                    }
                }
                // 下りる
                let s = tp.moves.len();
                match way.down {
                    Down::Ramp => tp.moves.extend(ramp_moves(
                        &rings[0],
                        prev,
                        z,
                        self.plan.angle,
                        self.plan.ramp_feed,
                        self.plan.plunge_feed,
                    )),
                    Down::Straight(feed) => tp.moves.push(Move::Linear {
                        to: [way.d[0], way.d[1], z],
                        feed,
                    }),
                    Down::Helix { c: hc, r, a0 } => tp.moves.extend(helix_moves(
                        hc,
                        r,
                        a0,
                        (prev, z),
                        self.plan.angle,
                        self.plan.ramp_feed,
                        c.direction.is_climb(),
                    )),
                }
                if !ramp_mode && dist(way.d, way.t) > 1e-9 {
                    tp.moves.push(Move::Linear {
                        to: [way.t[0], way.t[1], z],
                        feed: p.cutting.feed,
                    });
                }
                tp.mark(s, Role::Entry);
                if let Some(a) = arcs
                    && !ramp_mode
                {
                    let s = tp.moves.len();
                    tp.moves.extend(a.lead_in(z, a_feed(lead, p)));
                    tp.mark(s, Role::Lead);
                }
                tp.moves.extend(loop_moves(&rings[0], z, p.cutting.feed));
                // 横の荒取り: 壁へ寄りながら次の輪へ
                let mut at = start0;
                for r in rings.iter().skip(1) {
                    let r = start_at(r, at);
                    let q = [r.verts[0].x, r.verts[0].y];
                    tp.moves.push(Move::Linear {
                        to: [q[0], q[1], z],
                        feed: p.cutting.feed,
                    });
                    tp.moves.extend(loop_moves(&r, z, p.cutting.feed));
                    at = q;
                }
                away = false;
                if let Some(a) = arcs
                    && (!ramp_mode || i + 1 == n_levels)
                {
                    let s = tp.moves.len();
                    tp.moves.extend(a.lead_out(z, a_feed(lead, p)));
                    tp.mark(s, Role::Lead);
                    away = true;
                }
                if last > 0 && !away {
                    // 外の輪の始まりへ、来た道を戻る(次の段で下りる所へ)
                    if i + 1 < n_levels {
                        let s = tp.moves.len();
                        tp.moves.push(Move::Linear {
                            to: [start0[0], start0[1], z],
                            feed: p.cutting.feed,
                        });
                        tp.mark(s, Role::Link);
                    }
                }
                prev = z;
            }
            self.tabs(tp, &rings[last], first_move, from);
            retract(tp);
            // 仕上げ: 底の深さで、仕上げ代を等分して外から順に
            if finish > 1e-9 {
                for off in
                    finish_offsets(p.r() + self.leave, finish, self.fin.passes, self.fin.spring)
                {
                    for fl in shift(off) {
                        let fl = c.direction.apply(if tool_out { fl.reversed() } else { fl });
                        let fl = match (self.entry, lead) {
                            (_, Some(_)) => start_mid_line(&fl),
                            (Some(e), None) => start_at(&fl, e),
                            (None, None) => fl,
                        };
                        let fits = |q: P2| fl.contains(q) == free_inside;
                        let free_left = free_on_left(&fl, free_inside);
                        let shape = lead.map(|s| (s.radius, s.sweep, s.line));
                        let a = shape.and_then(|sh| {
                            let a = leads(&fl, sh, free_left, fits);
                            n.lead_missing |= a.is_none();
                            a
                        });
                        let out_a = if self.fin.overlap > 1e-9 {
                            shape.and_then(|sh| {
                                leads(&start_after(&fl, self.fin.overlap), sh, free_left, fits)
                            })
                        } else {
                            a
                        };
                        let down = lap_start(&fl, a);
                        approach(tp, down);
                        let first_move = tp.moves.len();
                        let from = tp
                            .moves
                            .last()
                            .map_or([down[0], down[1], tp.safe_z], |m| m.to());
                        let s = tp.moves.len();
                        tp.moves.push(Move::Linear {
                            to: [down[0], down[1], self.bottom],
                            feed: p.cutting.plunge,
                        });
                        tp.mark(s, Role::Entry);
                        Lap {
                            l: &fl,
                            z: self.bottom,
                            feed: self.fin.feed,
                            lead_feed: lead.map_or(self.fin.feed, |s| s.feed),
                            leads: a,
                            out_leads: a.and(out_a),
                            overlap: self.fin.overlap,
                        }
                        .push(tp);
                        self.tabs(tp, &rings[last], first_move, from);
                        retract(tp);
                    }
                }
            }
        }
        true
    }

    /// 入り方を決める。`l` は始まりを決めた輪郭、`arcs` はその入りと抜け。
    fn way(
        &self,
        l: &Loop,
        arcs: Option<Leads>,
        free_inside: bool,
        on: bool,
        fits: &impl Fn(P2) -> bool,
        n: &mut Notes,
    ) -> Way {
        let p = self.p;
        let start = [l.verts[0].x, l.verts[0].y];
        let t = arcs.map_or(start, |a| a.start);
        let plunge = Way {
            d: t,
            t,
            down: Down::Straight(self.plan.plunge_feed),
        };
        let ramp = Way {
            d: start,
            t: start,
            down: Down::Ramp,
        };
        let free_left = free_on_left(l, free_inside);
        let kind = self.plan.kind;
        if on && matches!(kind, EntryKind::Helix | EntryKind::Outside) {
            return ramp;
        }
        match kind {
            EntryKind::Auto => {
                if self.c.ramp_angle.is_some() {
                    ramp
                } else {
                    plunge
                }
            }
            EntryKind::Ramp => ramp,
            EntryKind::Plunge => plunge,
            EntryKind::Predrill => match self.entry {
                Some(e) if segment_fits(e, t, fits) && fits(e) => Way {
                    d: e,
                    t,
                    down: Down::Straight(self.plan.plunge_feed),
                },
                Some(_) => {
                    n.entry_far = true;
                    plunge
                }
                None => {
                    n.predrill_missing = true;
                    plunge
                }
            },
            EntryKind::Helix => {
                // 入る位置があれば、そこを中心に回る
                if let Some(e) = self.entry {
                    let mut r = self.plan.helix_r;
                    for _ in 0..3 {
                        let a0 = (t[1] - e[1]).atan2(t[0] - e[0]);
                        let hs = helix_start(e, r, a0, 0.0);
                        let d = [hs[0], hs[1]];
                        if circle_fits(e, r, fits) && segment_fits(d, t, fits) {
                            return Way {
                                d,
                                t,
                                down: Down::Helix { c: e, r, a0 },
                            };
                        }
                        r /= 2.0;
                    }
                }
                // 下りる所に接する円を、入ってよい側に置く
                if let Some(nv) = free_normal(l, free_left) {
                    let mut r = self.plan.helix_r;
                    for _ in 0..3 {
                        let c = [t[0] + nv[0] * r, t[1] + nv[1] * r];
                        if circle_fits(c, r, fits) {
                            let a0 = (t[1] - c[1]).atan2(t[0] - c[0]);
                            return Way {
                                d: t,
                                t,
                                down: Down::Helix { c, r, a0 },
                            };
                        }
                        r /= 2.0;
                    }
                }
                n.helix_fallback = true;
                ramp
            }
            EntryKind::Outside => {
                let Some(nv) = free_normal(l, free_left) else {
                    n.outside_fallback = true;
                    return ramp;
                };
                let m = p.r() + 1.0;
                let (lo, hi) = (p.info.stock_min, p.info.stock_max);
                let dd = exit_distance(t, nv, [lo[0] - m, lo[1] - m], [hi[0] + m, hi[1] + m]);
                let d = [t[0] + nv[0] * dd, t[1] + nv[1] * dd];
                if dd.is_finite() && segment_fits(d, t, fits) && fits(d) {
                    Way {
                        d,
                        t,
                        // ストックの外なので、下降の送りで下りる
                        down: Down::Straight(self.plan.plunge_feed),
                    }
                } else {
                    n.outside_fallback = true;
                    ramp
                }
            }
        }
    }

    /// 開いた線 `pts` を工具の中心でなぞる。段ごとに行って戻る(持ち上げずに次の深さへ)。
    pub(super) fn open(&self, tp: &mut Toolpath, pts: &[P2], n: &mut Notes) {
        let p = self.p;
        n.open_lines = true;
        let mut pts: Vec<P2> = pts.to_vec();
        // 入る位置に近い端から
        if let Some(e) = self.entry
            && dist(pts[pts.len() - 1], e) < dist(pts[0], e)
        {
            pts.reverse();
        }
        let straight = matches!(self.plan.kind, EntryKind::Plunge | EntryKind::Predrill)
            || (self.plan.kind == EntryKind::Auto && self.c.ramp_angle.is_none());
        approach(tp, pts[0]);
        let mut prev = tp.retract_z.min(self.top);
        for z in p.levels(self.top, self.bottom) {
            let s = tp.moves.len();
            if straight {
                tp.moves.push(Move::Linear {
                    to: [pts[0][0], pts[0][1], z],
                    feed: self.plan.plunge_feed,
                });
            } else {
                tp.moves.extend(zigzag_moves(
                    &pts,
                    (prev, z),
                    self.plan.angle,
                    self.plan.ramp_feed,
                    self.plan.plunge_feed,
                ));
            }
            tp.mark(s, Role::Entry);
            tp.moves.extend(pts.iter().skip(1).map(|q| Move::Linear {
                to: [q[0], q[1], z],
                feed: p.cutting.feed,
            }));
            pts.reverse();
            prev = z;
        }
        retract(tp);
    }

    /// `first` からの動きを、タブの上で持ち上げる。
    fn tabs(&self, tp: &mut Toolpath, path: &Loop, first: usize, from: kmb_toolpath::P3) {
        let c = self.c;
        let mut pts = path.flatten(0.05);
        if let Some(q) = pts.first().copied() {
            pts.push(q);
        }
        {
            let mut seen = self.tab_paths.borrow_mut();
            if !seen.iter().any(|t| t.pts == pts) {
                seen.push(TabPath {
                    z: self.floor,
                    pts: pts.clone(),
                });
            }
        }
        let tabs = tab_positions(c, path, self.p.r());
        if tabs.is_empty() {
            return;
        }
        let (w, h) = self.tab;
        // タブの高さは底の面から測る(さらに下げた分は含めない)
        let floor = self.floor;
        // タブの幅は部品の側で測るので、工具の中心では工具の半径だけ広げる
        let starts = lift_over_tabs_shaped(
            &mut tp.moves,
            first,
            from,
            &tabs,
            w / 2.0 + self.p.r(),
            (floor + h).min(self.top),
            (c.tab_shape == TabShape::Triangle).then_some(h),
        );
        tp.remap_spans(&starts);
    }
}

/// 通常の輪郭と取残しで、等間隔・手動のタブを同じ輪郭上の位置にそろえる。
pub(super) fn tab_positions(c: &Contour, path: &Loop, radius: f64) -> Vec<P2> {
    if c.tab_points.is_empty() {
        return tab_centers(path, c.tabs);
    }
    let mut pts = path.flatten(0.05);
    if let Some(q) = pts.first().copied() {
        pts.push(q);
    }
    c.tab_points
        .iter()
        .filter_map(|q| {
            let (d, at) = nearest_on(&pts, *q)?;
            (d <= radius + 1e-6).then_some(at)
        })
        .collect()
}

/// 折れ線 `pts` の上で `q` に最も近い点と、その距離。
fn nearest_on(pts: &[P2], q: P2) -> Option<(f64, P2)> {
    pts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let ab = [b[0] - a[0], b[1] - a[1]];
            let len2 = ab[0] * ab[0] + ab[1] * ab[1];
            let t = if len2 > 1e-18 {
                (((q[0] - a[0]) * ab[0] + (q[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let at = [a[0] + ab[0] * t, a[1] + ab[1] * t];
            (dist(at, q), at)
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
}

fn a_feed(lead: Option<LeadShape>, p: &Planner) -> f64 {
    lead.map_or(p.cutting.feed, |s| s.feed)
}
