//! CPU で描く: 三角形を深度バッファで塗り、辺・格子・原点を重ねた絵を作る。

use super::*;

pub(super) fn lerp_color(c: Color32, k: f32) -> Color32 {
    shade(c, k)
}

/// 深度つきでソフトウェア描画する。`size` は画素数、`ppp` は 1 point あたりの画素数。
/// 裏向きの三角形は描かない(閉じたソリッドなので見えない)。
/// `shading` が線だけなら、面は選んだ面とマウスの下の面だけ塗り、辺は隠れた所も描く。
#[allow(clippy::too_many_arguments)]
pub fn render(
    scene: &Scene,
    cam: &Camera,
    size: [usize; 2],
    ppp: f32,
    picked: &[Pick],
    hover: Option<&Pick>,
    colors: &Colors,
    shading: Shading,
) -> egui::ColorImage {
    render_guided(
        scene,
        cam,
        size,
        ppp,
        picked,
        hover,
        colors,
        shading,
        Guides::default(),
        &[],
        &Behind::default(),
    )
}

/// 背景の絵(上の `background` から下の `background_low` へ縦に変える。GPU の `fs_back` と同じ)。
pub fn backdrop(colors: &Colors, w: usize, h: usize) -> Vec<Color32> {
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        let t = (y as f32 + 0.5) / h.max(1) as f32;
        let c = crate::mix(colors.background, colors.background_low, t);
        out.extend(std::iter::repeat_n(c, w));
    }
    out
}

/// [`render`] に、形の下の目安(格子・軸)を足して描く。目安は形より先に描くので、形に隠れる。
#[allow(clippy::too_many_arguments)]
pub fn render_guided(
    scene: &Scene,
    cam: &Camera,
    size: [usize; 2],
    ppp: f32,
    picked: &[Pick],
    hover: Option<&Pick>,
    colors: &Colors,
    shading: Shading,
    guides: Guides,
    ghosts: &[Ghost],
    behind: &Behind,
) -> egui::ColorImage {
    let [w, h] = size;
    let mut buf = Raster {
        w,
        h,
        color: backdrop(colors, w, h),
        depth: vec![f32::NEG_INFINITY; w * h],
    };
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(w as f32 / ppp, h as f32 / ppp));
    let (_, _, eye) = cam.basis();
    let eye_pos = cam.eye_position(rect);
    let near = cam.near_limit(rect);
    let dist = cam.distance(rect);
    // 深度バッファに書く値: 平行投影は近さそのもの、透視投影は視点からの距離の逆数
    // (どちらも画面の上で直線に変わり、大きいほど手前)
    let key = |z: f32| match dist {
        Some(d) => 1.0 / (d - z).max(d * NEAR * 0.5),
        None => z,
    };
    let to_px = |p: [f32; 3]| {
        let (q, d) = cam.project(p, rect);
        (Pos2::new(q.x * ppp, q.y * ppp), d)
    };
    // 場面の半径(対角の半分。GPU の描画と同じ)
    let reach = {
        let d = sub(scene.max, scene.min).map(|v| v.max(0.0));
        let r = 0.5 * dot(d, d).sqrt();
        if r.is_finite() { r.max(1.0) } else { 1.0 }
    };
    let flat = FlatLight::new(eye, cam.target, reach);
    let light = flat.key;
    if guides.any() {
        draw_guides(&mut buf, cam, rect, ppp, colors, guides);
    }
    let shade_of = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| {
        let (k, shine) = flat.shade(a, cross(sub(b, a), sub(c, a)), 1.0);
        k + shine
    };
    // 透かす道具の立体と稜線(奥行きを見ない)
    let ghost_free = |buf: &mut Raster, g: &Ghost| {
        for t in &g.tris {
            if near.is_some_and(|zmax| t.iter().any(|p| to_px(*p).1 > zmax)) {
                continue;
            }
            let l = dot(normalize(cross(sub(t[1], t[0]), sub(t[2], t[0]))), light);
            buf.blend(t.map(|p| to_px(p).0), g.color, ghost_shade(l));
        }
        for [p, q] in &g.edges {
            if let Some((p, q)) = cam.clip_segment(*p, *q, rect) {
                buf.line(to_px(p).0, to_px(q).0, g.edge_color);
            }
        }
    };
    let size3 = sub(scene.max, scene.min);
    let bias = 0.003 * dot(size3, size3).sqrt();
    // 写実の見た目の環境の光(主の光は視点の水平の向きについてくる)
    let mut env = kmb_render::Env::new(scene.effects.env, eye);
    env.lamps = scene.lamps.clone();
    // 設計の画面は、景色を世界に固定した環境マップで描く(Kie 2026-10-21。GPU の shade_env と同じ)
    let envmap = !scene.effects.all_bodies;
    let env_frame = kmb_render::EnvFrame::world();
    let section = scene.section;
    let hidden_alpha = crate::render::current().hidden_alpha;
    // 稜線の太さ(描く先の画素の本数。1 本なら今までと同じ)
    let edge_copies = {
        let r = crate::render::current();
        (r.edge_px * r.scale).round().max(1.0) as usize
    };
    for (bi, b) in scene.bodies.iter().enumerate() {
        let m = &b.mesh;
        let body_color = scene.body_color(bi, colors);
        let look = scene
            .body_look(bi, colors)
            .filter(|_| shading.faces() && !scene.glows(bi));
        let steel = shading.faces() && scene.steel_env(bi);
        let faded = scene.is_faded(bi);
        let cap = scene.cap_color(bi, colors);
        let stripe = hatch_color(cap);
        let pts: Vec<(Pos2, f32)> = m.positions.iter().map(|p| to_px(*p)).collect();
        // 白いつやは色の掛け算に直して入れる(塗りは明るさの倍率だけを持つ)
        let lum = (luma(body_color) / 255.0).max(0.05);
        let intensity: Vec<f32> = m
            .normals
            .iter()
            .zip(&m.positions)
            .map(|(n, p)| {
                let (k, shine) = flat.shade(*p, *n, 1.0);
                k + shine / lum
            })
            .collect();
        for (t, face) in m.triangles.iter().zip(&m.triangle_faces) {
            let wpos = |i: usize| m.positions[t[i] as usize];
            if section.is_some_and(|sec| (0..3).all(|i| sec.removes(wpos(i)))) {
                continue;
            }
            let gn = cross(sub(wpos(1), wpos(0)), sub(wpos(2), wpos(0)));
            if dot(gn, cam.toward_eye(wpos(0), eye_pos)) <= 0.0 {
                // 断面で切った時は、裏の面を切り口の蓋の色で平らに、斜めの縞を入れて塗る
                // (切り口が開いて見えない)
                if section.is_some() {
                    for tri in clip_section_with(section, [0, 1, 2].map(|i| (wpos(i), 1.0))) {
                        for tri in clip_near_with(cam, rect, tri) {
                            buf.fill_by(
                                // 切り口は少し手前に寄せる(別のボディの、ちょうど重なる面と競ってちらつかない)
                                tri.map(|(p, l)| {
                                    let (q, z) = to_px(p);
                                    (q, key(z + bias * CAP_BIAS), l)
                                }),
                                |x, y, _| crate::mix(cap, stripe, hatch(x, y, ppp)),
                            );
                        }
                    }
                }
                continue;
            }
            let name = &m.face_names[*face as usize];
            let is =
                |p: Option<&Pick>| p.is_some_and(|p| p.feature == b.feature && p.face == *name);
            // 面の外観のある面は、その色と材質で塗る(加工の塗り分けの色が先)
            let mut tri_look = look.as_ref();
            let mut from_face = false;
            let base = if picked.iter().any(|p| is(Some(p))) {
                colors.picked
            } else if is(hover) {
                colors.hover
            } else if shading.faces() {
                match b.colors.get(*face as usize).copied().flatten() {
                    Some(c) => scene.glow_tint(bi, c),
                    None => match scene.face_tint(bi, *face as usize, colors) {
                        Some(c) => {
                            tri_look = scene.face_look(bi, *face as usize);
                            from_face = true;
                            c
                        }
                        None => body_color,
                    },
                }
            } else {
                continue;
            };
            let textured = if from_face {
                tri_look.is_some()
            } else {
                (steel || look.is_some()) && base == body_color
            };
            let k = |i: usize| {
                let x = intensity.get(t[i] as usize).copied().unwrap_or(1.0);
                if faded { x + (1.0 - x) * FADE_FLAT } else { x }
            };
            let v = [0, 1, 2].map(|i| pts[t[i] as usize]);
            if section.is_some_and(|sec| (0..3).any(|i| sec.removes(wpos(i)))) {
                // 断面にかかる三角形は、切り取る側を落としてから塗る
                for tri in clip_section_with(section, [0, 1, 2].map(|i| (wpos(i), k(i)))) {
                    for tri in clip_near_with(cam, rect, tri) {
                        buf.fill(
                            tri.map(|(p, l)| {
                                let (q, z) = to_px(p);
                                (q, key(z), l)
                            }),
                            base,
                        );
                    }
                }
            } else if near.is_some_and(|zmax| v.iter().any(|q| q.1 > zmax)) {
                // 視点に近すぎる所を切った残りを塗る
                for tri in clip_near_with(cam, rect, [0, 1, 2].map(|i| (wpos(i), k(i)))) {
                    let tri = tri.map(|(p, l)| {
                        let (q, z) = to_px(p);
                        (q, key(z), l)
                    });
                    buf.fill(tri, base);
                }
            } else if textured {
                // 材質の質感: 画素ごとに位置と法線を出して塗る(透視投影は奥行きで重みを直す)
                let keys = [0, 1, 2].map(|i| key(v[i].1));
                let nrm = |i: usize| {
                    m.normals
                        .get(t[i] as usize)
                        .copied()
                        .unwrap_or([0.0, 0.0, 1.0])
                };
                buf.fill_with([0, 1, 2].map(|i| (v[i].0, keys[i], k(i))), |_, _, _, e| {
                    let e = if dist.is_some() {
                        let s = e[0] * keys[0] + e[1] * keys[1] + e[2] * keys[2];
                        [0, 1, 2].map(|i| e[i] * keys[i] / s.max(1e-12))
                    } else {
                        e
                    };
                    let at = |f: &dyn Fn(usize) -> [f32; 3]| {
                        [0, 1, 2].map(|c| e[0] * f(0)[c] + e[1] * f(1)[c] + e[2] * f(2)[c])
                    };
                    let p = at(&wpos);
                    let mut n = normalize(at(&nrm));
                    let view = if envmap {
                        flat.env_view(p, eye_pos)
                    } else {
                        normalize(cam.toward_eye(p, eye_pos))
                    };
                    if dot(n, view) < 0.0 {
                        n = n.map(|x| -x);
                    }
                    let lin = match tri_look {
                        // 設計の画面は環境マップ、レンダリングの作業空間は選んだ環境の光
                        Some(look) if envmap => {
                            let (albedo, metal) = look.surface_at(p, n);
                            kmb_render::EnvMap::viewport().shade(
                                &env_frame,
                                albedo,
                                metal,
                                look.roughness(),
                                n,
                                view,
                            )
                        }
                        Some(look) => {
                            kmb_render::raster::shade_surface(&env, look, p, n, view, 1.0)
                        }
                        None => kmb_render::EnvMap::viewport().shade(
                            &env_frame,
                            [base.r(), base.g(), base.b()].map(kmb_render::math::srgb_to_linear),
                            1.0,
                            kmb_render::envmap::STEEL_ROUGHNESS,
                            n,
                            view,
                        ),
                    };
                    let c = kmb_render::math::tonemap(lin);
                    Color32::from_rgb(
                        kmb_render::math::linear_to_srgb(c[0]),
                        kmb_render::math::linear_to_srgb(c[1]),
                        kmb_render::math::linear_to_srgb(c[2]),
                    )
                });
            } else {
                buf.fill([0, 1, 2].map(|i| (v[i].0, key(v[i].1), k(i))), base);
            }
        }
    }
    // 稜線は全部のボディの面を塗った後に描く(後のボディの面に隠れた辺も点線にできる)
    for (bi, b) in scene.bodies.iter().enumerate().filter(|_| shading.edges()) {
        let line = line_color(scene.body_color(bi, colors), colors.dark);
        // 稜線は少し手前に寄せて、隠れていない部分だけ描く(線だけの表示では全部描く)。
        // 陰影と隠れ線では、隠れた部分を薄い点線で描く
        let see_through = !shading.faces();
        let dashed = shading.hidden_dashed();
        let (dash_on, dash_period) = (
            (DASH_ON * ppp).round().max(1.0) as usize,
            ((DASH_ON + DASH_OFF) * ppp).round().max(2.0) as usize,
        );
        let hidden_ink = colors.edge;
        for [p, q] in &b.edges {
            let Some((p, q)) = clip_section_segment(section, *p, *q) else {
                continue;
            };
            let Some((p, q)) = cam.clip_segment(p, q, rect) else {
                continue;
            };
            let ((a, da), (c, dc)) = (to_px(p), to_px(q));
            let (ka, kc) = (key(da + bias), key(dc + bias));
            let steps = (c - a).length().ceil().clamp(1.0, 100_000.0) as usize;
            // 太い線は、線と直角に 1 画素ずつずらして重ねる
            let side = (c - a).normalized().rot90();
            for o in 0..edge_copies {
                let shift = side * (o as f32 - (edge_copies - 1) as f32 / 2.0);
                for s in 0..=steps {
                    let t = s as f32 / steps as f32;
                    let x = a.x + (c.x - a.x) * t + shift.x;
                    let y = a.y + (c.y - a.y) * t + shift.y;
                    if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
                        continue;
                    }
                    let i = y as usize * w + x as usize;
                    if see_through || ka + (kc - ka) * t >= buf.depth[i] {
                        buf.color[i] = line;
                    } else if dashed && o == 0 && s % dash_period < dash_on {
                        buf.color[i] = crate::mix(buf.color[i], hidden_ink, hidden_alpha);
                    }
                }
            }
        }
    }
    // できる形の見本(結合・新しいボディ): ボディと同じく奥行きを見て塗る
    for g in ghosts.iter().filter(|g| g.layer == GhostLayer::Solid) {
        for t in &g.tris {
            let gn = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            if dot(gn, cam.toward_eye(t[0], eye_pos)) <= 0.0 {
                continue;
            }
            let v = t.map(&to_px);
            if near.is_some_and(|zmax| v.iter().any(|q| q.1 > zmax)) {
                continue;
            }
            let k = shade_of(t[0], t[1], t[2]);
            buf.fill(v.map(|(q, z)| (q, key(z), k)), g.color);
        }
        for [p, q] in &g.edges {
            let Some((p, q)) = cam.clip_segment(*p, *q, rect) else {
                continue;
            };
            let ((a, da), (c, dc)) = (to_px(p), to_px(q));
            let (ka, kc) = (key(da + bias), key(dc + bias));
            let steps = (c - a).length().ceil().clamp(1.0, 100_000.0) as usize;
            for s in 0..=steps {
                let t = s as f32 / steps as f32;
                let (x, y) = (a.x + (c.x - a.x) * t, a.y + (c.y - a.y) * t);
                if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
                    continue;
                }
                let i = y as usize * w + x as usize;
                if ka + (kc - ka) * t >= buf.depth[i] {
                    buf.color[i] = g.edge_color;
                }
            }
        }
    }
    // 本体の奥のスケッチ・作業平面: 面は手前に見える所だけ透かして塗り、線は手前の所を普通に、
    // 隠れた所を隠れた辺と同じ点線で描く(GPU の描画と同じ。Q6 v1)
    for (t, c) in &behind.tris {
        let v = t.map(&to_px);
        if near.is_some_and(|zmax| v.iter().any(|q| q.1 > zmax)) {
            continue;
        }
        buf.blend_front(v.map(|(q, z)| (q, key(z + bias))), *c);
    }
    let (dash_on, dash_period) = (
        (DASH_ON * ppp).round().max(1.0) as usize,
        ((DASH_ON + DASH_OFF) * ppp).round().max(2.0) as usize,
    );
    for ([p, q], c, width) in &behind.lines {
        let Some((p, q)) = cam.clip_segment(*p, *q, rect) else {
            continue;
        };
        let ((a, da), (e, de)) = (to_px(p), to_px(q));
        let (ka, ke) = (key(da + bias), key(de + bias));
        let steps = (e - a).length().ceil().clamp(1.0, 100_000.0) as usize;
        let copies = (width * ppp).round().max(1.0) as usize;
        let side = (e - a).normalized().rot90();
        for o in 0..copies {
            let shift = side * (o as f32 - (copies - 1) as f32 / 2.0);
            for s in 0..=steps {
                let t = s as f32 / steps as f32;
                let x = a.x + (e.x - a.x) * t + shift.x;
                let y = a.y + (e.y - a.y) * t + shift.y;
                if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
                    continue;
                }
                let i = y as usize * w + x as usize;
                if ka + (ke - ka) * t >= buf.depth[i] {
                    buf.color[i] = *c;
                } else if s % dash_period < dash_on {
                    buf.color[i] = crate::mix(buf.color[i], *c, hidden_alpha);
                }
            }
        }
    }
    for g in ghosts.iter().filter(|g| g.layer == GhostLayer::Over) {
        ghost_free(&mut buf, g);
    }
    egui::ColorImage::new(size, buf.color)
}

/// 格子と軸を、深度を書かずに描く(後から塗る形が上に重なる)。
pub(super) fn draw_guides(
    buf: &mut Raster,
    cam: &Camera,
    rect: Rect,
    ppp: f32,
    colors: &Colors,
    g: Guides,
) {
    guide_segments(cam, rect, colors, g, &mut |a, b, c| {
        if let Some((p, q)) = cam.clip_segment(a, b, rect) {
            let (p, q) = (cam.project(p, rect).0, cam.project(q, rect).0);
            buf.line(
                Pos2::new(p.x * ppp, p.y * ppp),
                Pos2::new(q.x * ppp, q.y * ppp),
                c,
            );
        }
    });
}

/// 格子を描く原点の平面(平面に沿う 2 つの軸の番号)。ふだんは XY 平面(真横から見ると 1 本の線に
/// 潰れるので描かない)。真横から見て YZ 平面か ZX 平面と向き合う時(視線との角が 10 度より小さい。
/// 右・前などの向き)は、その平面に描く(M10)。
pub(crate) fn grid_plane(cam: &Camera) -> Option<(usize, usize)> {
    let (_, _, eye) = cam.basis();
    const FACING: f32 = 0.985;
    if eye[2].abs() > 0.08 {
        Some((0, 1))
    } else if eye[0].abs() >= FACING {
        Some((1, 2))
    } else if eye[1].abs() >= FACING {
        Some((0, 2))
    } else {
        None
    }
}

/// 格子の真ん中(平面の 2 つの座標、`major` に丸める)と半分の大きさ。真ん中は回す中心の真下でなく、
/// 視線の軸が格子の平面に当たる所にする(回す中心が部品の上にあると、寄った時に真下の格子が画面の
/// 外へ出て格子が消えていた。10-20 u10)。浅い角度で遠くへ行き過ぎないよう、真下から半分の大きさまでに
/// 留める。斜めに見るほど平面は縮んで見えるので、半分の大きさを広げる(3 倍まで)。
fn grid_center(cam: &Camera, a: usize, b: usize, half: f32, major: f32) -> (f32, f32, f32) {
    let (_, _, e) = cam.basis();
    let k = 3 - a - b;
    let tilt = e[k].abs().max(1e-3);
    let half = ((half / tilt.max(1.0 / 3.0) / major).ceil() * major).max(major);
    // 回す中心から視線の軸に沿って平面まで(平面は原点を通る)
    let t = -cam.target[k] / if e[k] == 0.0 { 1e-3 } else { e[k] };
    let (mut sa, mut sb) = (t * e[a], t * e[b]);
    let len = (sa * sa + sb * sb).sqrt();
    let limit = half * 0.5;
    if len > limit {
        (sa, sb) = (sa * limit / len, sb * limit / len);
    }
    let round = |v: f32| (v / major).round() * major;
    (round(cam.target[a] + sa), round(cam.target[b] + sb), half)
}

/// 格子の線の薄れ方: 薄れ始める所と消える所。`round` は格子の真ん中からの距離(半分の大きさに対する
/// 割合)、`deep` は透視の時の視点からの距離(回す中心までの距離に対する割合)。細かい線は遠くで詰まって
/// 縞に見えるので、太い線より手前で消す。透視で低く見ると、端が地平線の手前で横一線に切れて見えていた
/// (10-27 v3)。平面の上の距離だけで薄めると、遠い所は画面の上で数画素に縮むので、視点からの距離でも
/// 薄める。真上から見る平行投影では画面の角は 0.4 ほどなので変わらない。
#[derive(Clone, Copy)]
struct Fade {
    round: (f32, f32),
    deep: (f32, f32),
}
const GRID_FADE_MINOR: Fade = Fade {
    round: (0.45, 0.8),
    deep: (1.2, 3.0),
};
const GRID_FADE_MAJOR: Fade = Fade {
    round: (0.55, 1.0),
    deep: (1.5, 4.5),
};
/// XY 平面の格子を薄くし始める視線の高さと、消えきる高さ(視線の向きの Z。`grid_plane` が描かなくなる
/// 0.08 で消えきる)
const GRID_LOW: (f32, f32) = (0.08, 0.14);
/// 1 本の線を分ける数(区間ごとに 1 つの濃さで引く。同じ濃さの続く区間はまとめる)
const GRID_PIECES: usize = 24;

/// `lo` から `hi` へ 0 から 1 へ滑らかに上がる値(`lo > hi` なら下がる)。
fn smooth(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 格子の真ん中から `v` 離れた 1 本の線(真ん中からの距離 `r1` の円の内側)を区間に分け、区間の
/// 真ん中の濃さ `k`(線に沿った位置から 0〜1)を透明度にして `put` へ渡す(線に沿った位置の両端と色)。
fn fade_line(
    r1: f32,
    v: f32,
    c: Color32,
    k: &dyn Fn(f32) -> f32,
    put: &mut dyn FnMut(f32, f32, Color32),
) {
    if v.abs() >= r1 {
        return;
    }
    let s1 = (r1 * r1 - v * v).sqrt();
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    let at = |j: usize| -s1 + 2.0 * s1 * j as f32 / GRID_PIECES as f32;
    // 同じ濃さの続く区間は 1 本にまとめる(始まりと濃さ)
    let mut run: Option<(f32, u8)> = None;
    for j in 0..=GRID_PIECES {
        let alpha = if j < GRID_PIECES {
            (255.0 * k((at(j) + at(j + 1)) * 0.5)).round() as u8
        } else {
            0
        };
        match run {
            Some((_, a)) if a == alpha && j < GRID_PIECES => {}
            _ => {
                if let Some((from, a)) = run
                    && a > 0
                {
                    put(from, at(j), Color32::from_rgba_unmultiplied(r, g, b, a));
                }
                run = Some((at(j), alpha));
            }
        }
    }
}

/// 格子と軸の線分(空間の両端と色)を `line` へ渡す。CPU と GPU の描画で同じ線を使う。格子の線は
/// 透明度を持つ(遠くで薄れる)。
pub(crate) fn guide_segments(
    cam: &Camera,
    rect: Rect,
    colors: &Colors,
    g: Guides,
    line: &mut dyn FnMut([f32; 3], [f32; 3], Color32),
) {
    let step = grid_step(cam.zoom, 14.0);
    let major = grid_major(step);
    // 見えている範囲を覆う大きさ(画面の対角の 1.3 倍)。太い線の間隔に丸める
    let diag = (rect.width().powi(2) + rect.height().powi(2)).sqrt() / cam.zoom;
    let half = ((diag * 1.3 / major).ceil() * major).max(major);
    let round = |v: f32| (v / major).round() * major;
    let (cx, cy) = (round(cam.target[0]), round(cam.target[1]));
    let minor_c = crate::mix(colors.background, colors.edge, 0.10);
    let major_c = crate::mix(colors.background, colors.edge, 0.22);
    if g.grid
        && let Some((a, b)) = grid_plane(cam)
    {
        let (ca, cb, half) = grid_center(cam, a, b, half, major);
        // 線の数は画面の対角を `step` の画素(14)で割った数ほど。拡大が壊れても 2000 本で打ち切る
        let n = ((half / step).round() as i32).clamp(0, 2000);
        let at = |va: f32, vb: f32| {
            let mut p = [0.0; 3];
            p[a] = va;
            p[b] = vb;
            p
        };
        // 平面を低く見るほど全体を薄くし、描かなくなる角度で消えきる(急に消えないように)
        let (_, _, e) = cam.basis();
        let low = if (a, b) == (0, 1) {
            smooth(GRID_LOW.0, GRID_LOW.1, e[2].abs())
        } else {
            1.0
        };
        let eye = cam.eye_position(rect);
        let dist = cam.distance(rect).unwrap_or(1.0);
        // 平面の上の点(2 つの座標)の濃さ
        let alpha = |fade: Fade, pa: f32, pb: f32| {
            let r = ((pa - ca).powi(2) + (pb - cb).powi(2)).sqrt();
            let deep = eye.map_or(1.0, |o| {
                let p = at(pa, pb);
                let d = (0..3).map(|i| (p[i] - o[i]).powi(2)).sum::<f32>().sqrt();
                smooth(fade.deep.1 * dist, fade.deep.0 * dist, d)
            });
            low * smooth(fade.round.1 * half, fade.round.0 * half, r) * deep
        };
        if low > 0.0 {
            for i in -n..=n {
                let v = i as f32 * step;
                let on_major = ((v / major).round() * major - v).abs() < step * 0.01;
                let (c, fade) = if on_major {
                    (major_c, GRID_FADE_MAJOR)
                } else {
                    (minor_c, GRID_FADE_MINOR)
                };
                let r1 = fade.round.1 * half;
                let (x, y) = (ca + v, cb + v);
                // 原点を通る線が X・Y の軸に重なる時は、軸の色の線だけ引く(Z の軸は短いので格子を残す)
                if !(g.axes && b != 2 && x.abs() < step * 0.01) {
                    let k = |s: f32| alpha(fade, x, cb + s);
                    fade_line(r1, v, c, &k, &mut |s, t, c| {
                        line(at(x, cb + s), at(x, cb + t), c)
                    });
                }
                if !(g.axes && a != 2 && y.abs() < step * 0.01) {
                    let k = |s: f32| alpha(fade, ca + s, y);
                    fade_line(r1, v, c, &k, &mut |s, t, c| {
                        line(at(ca + s, y), at(ca + t, y), c)
                    });
                }
            }
        }
    }
    if g.axes {
        // X・Y は格子の端まで、Z は原点の平面と同じ高さまで上へ
        let lo = |c: f32| (c - half).min(-major);
        let hi = |c: f32| (c + half).max(major);
        let axis = colors.inks.axis;
        line([lo(cx), 0.0, 0.0], [hi(cx), 0.0, 0.0], axis[0]);
        line([0.0, lo(cy), 0.0], [0.0, hi(cy), 0.0], axis[1]);
        line([0.0, 0.0, 0.0], [0.0, 0.0, origin_size(cam, rect)], axis[2]);
    }
}

/// 描いている絵と深度。
pub(super) struct Raster {
    w: usize,
    h: usize,
    color: Vec<Color32>,
    depth: Vec<f32>,
}

impl Raster {
    /// 深度を見ずに線を引く(画素の位置)。透明度のある色は下の色と混ぜる(同じ画素は 1 度だけ)。
    fn line(&mut self, a: Pos2, b: Pos2, c: Color32) {
        // 絵の枠で切ってから引く(格子の線は画面の外へ長く伸びる)
        let d = b - a;
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for (p, q) in [
            (-d.x, a.x),
            (d.x, self.w as f32 - 1.0 - a.x),
            (-d.y, a.y),
            (d.y, self.h as f32 - 1.0 - a.y),
        ] {
            if p.abs() < 1e-9 {
                if q < 0.0 {
                    return;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
            }
        }
        if t0 > t1 {
            return;
        }
        let (a, b) = (a + d * t0, a + d * t1);
        let n = ((b - a).length().ceil() as usize).clamp(1, 100_000);
        let k = f32::from(c.a()) / 255.0;
        let [r, g, bl, _] = c.to_srgba_unmultiplied();
        let mut last = usize::MAX;
        for i in 0..=n {
            let p = a + (b - a) * (i as f32 / n as f32);
            if p.x >= 0.0 && p.y >= 0.0 && (p.x as usize) < self.w && (p.y as usize) < self.h {
                let at = p.y as usize * self.w + p.x as usize;
                if at == last {
                    continue;
                }
                last = at;
                self.color[at] = if c.a() == 255 {
                    c
                } else {
                    let d = self.color[at];
                    let f = |x: u8, y: u8| (y as f32 + (x as f32 - y as f32) * k).round() as u8;
                    Color32::from_rgb(f(r, d.r()), f(g, d.g()), f(bl, d.b()))
                };
            }
        }
    }

    /// 三角形(画素の位置、深度の値、明るさ)を塗る。手前のものだけ残す。
    /// 三角形を透かして塗る(奥行きは見ず、書かない)。`base` の透明度で混ぜ、`k` は明るさ。
    fn blend(&mut self, v: [Pos2; 3], base: Color32, k: f32) {
        let (w, h) = (self.w, self.h);
        let (p0, p1, p2) = (v[0], v[1], v[2]);
        let area = (p1.x - p0.x) * (p2.y - p0.y) - (p1.y - p0.y) * (p2.x - p0.x);
        if area.abs() < 1e-12 {
            return;
        }
        let c = lerp_color(base, k);
        let a = f32::from(base.a()) / 255.0;
        let x0 = p0.x.min(p1.x).min(p2.x).floor().max(0.0) as usize;
        let x1 = (p0.x.max(p1.x).max(p2.x).ceil() as isize).min(w as isize - 1);
        let y0 = p0.y.min(p1.y).min(p2.y).floor().max(0.0) as usize;
        let y1 = (p0.y.max(p1.y).max(p2.y).ceil() as isize).min(h as isize - 1);
        if x1 < 0 || y1 < 0 {
            return;
        }
        for y in y0..=y1 as usize {
            let py = y as f32 + 0.5;
            for x in x0..=x1 as usize {
                let px = x as f32 + 0.5;
                let e0 = ((p2.x - p1.x) * (py - p1.y) - (p2.y - p1.y) * (px - p1.x)) / area;
                let e1 = ((p0.x - p2.x) * (py - p2.y) - (p0.y - p2.y) * (px - p2.x)) / area;
                if e0 < 0.0 || e1 < 0.0 || 1.0 - e0 - e1 < 0.0 {
                    continue;
                }
                let i = y * w + x;
                let d = self.color[i];
                let mix =
                    |s: u8, t: u8| (f32::from(t) * (1.0 - a) + f32::from(s) * a).round() as u8;
                self.color[i] =
                    Color32::from_rgb(mix(c.r(), d.r()), mix(c.g(), d.g()), mix(c.b(), d.b()));
            }
        }
    }

    /// 三角形(画素の位置、深度の値)を、手前に見える所だけ `base` の透明度で混ぜて塗る(深度は書かない)。
    fn blend_front(&mut self, v: [(Pos2, f32); 3], base: Color32) {
        let (w, h) = (self.w, self.h);
        let (p0, p1, p2) = (v[0].0, v[1].0, v[2].0);
        let area = (p1.x - p0.x) * (p2.y - p0.y) - (p1.y - p0.y) * (p2.x - p0.x);
        if area.abs() < 1e-12 {
            return;
        }
        let [r, g, b, a] = base.to_srgba_unmultiplied();
        let a = f32::from(a) / 255.0;
        let x0 = p0.x.min(p1.x).min(p2.x).floor().max(0.0) as usize;
        let x1 = (p0.x.max(p1.x).max(p2.x).ceil() as isize).min(w as isize - 1);
        let y0 = p0.y.min(p1.y).min(p2.y).floor().max(0.0) as usize;
        let y1 = (p0.y.max(p1.y).max(p2.y).ceil() as isize).min(h as isize - 1);
        if x1 < 0 || y1 < 0 {
            return;
        }
        for y in y0..=y1 as usize {
            let py = y as f32 + 0.5;
            for x in x0..=x1 as usize {
                let px = x as f32 + 0.5;
                let e0 = ((p2.x - p1.x) * (py - p1.y) - (p2.y - p1.y) * (px - p1.x)) / area;
                let e1 = ((p0.x - p2.x) * (py - p2.y) - (p0.y - p2.y) * (px - p2.x)) / area;
                let e2 = 1.0 - e0 - e1;
                if e0 < 0.0 || e1 < 0.0 || e2 < 0.0 {
                    continue;
                }
                let i = y * w + x;
                if e0 * v[0].1 + e1 * v[1].1 + e2 * v[2].1 < self.depth[i] {
                    continue;
                }
                let d = self.color[i];
                let mix =
                    |s: u8, t: u8| (f32::from(t) * (1.0 - a) + f32::from(s) * a).round() as u8;
                self.color[i] = Color32::from_rgb(mix(r, d.r()), mix(g, d.g()), mix(b, d.b()));
            }
        }
    }

    fn fill(&mut self, v: [(Pos2, f32, f32); 3], base: Color32) {
        self.fill_by(v, |_, _, k| lerp_color(base, k));
    }

    /// 三角形を塗る。色は画素の真ん中の位置と明るさから `color` で決める。手前のものだけ残す。
    fn fill_by(&mut self, v: [(Pos2, f32, f32); 3], color: impl Fn(f32, f32, f32) -> Color32) {
        self.fill_with(v, |x, y, l, _| color(x, y, l));
    }

    /// 三角形を塗る。色は画素の真ん中の位置・明るさ・3 つの角の重み(画面の上の重心座標)から決める。
    fn fill_with(
        &mut self,
        v: [(Pos2, f32, f32); 3],
        color: impl Fn(f32, f32, f32, [f32; 3]) -> Color32,
    ) {
        let (w, h) = (self.w, self.h);
        let (p0, p1, p2) = (v[0].0, v[1].0, v[2].0);
        let area = (p1.x - p0.x) * (p2.y - p0.y) - (p1.y - p0.y) * (p2.x - p0.x);
        if area.abs() < 1e-12 {
            return;
        }
        let x0 = p0.x.min(p1.x).min(p2.x).floor().max(0.0) as usize;
        let x1 = (p0.x.max(p1.x).max(p2.x).ceil() as isize).min(w as isize - 1);
        let y0 = p0.y.min(p1.y).min(p2.y).floor().max(0.0) as usize;
        let y1 = (p0.y.max(p1.y).max(p2.y).ceil() as isize).min(h as isize - 1);
        if x1 < 0 || y1 < 0 {
            return;
        }
        for y in y0..=y1 as usize {
            let py = y as f32 + 0.5;
            for x in x0..=x1 as usize {
                let px = x as f32 + 0.5;
                let e0 = ((p2.x - p1.x) * (py - p1.y) - (p2.y - p1.y) * (px - p1.x)) / area;
                let e1 = ((p0.x - p2.x) * (py - p2.y) - (p0.y - p2.y) * (px - p2.x)) / area;
                let e2 = 1.0 - e0 - e1;
                if e0 < 0.0 || e1 < 0.0 || e2 < 0.0 {
                    continue;
                }
                let z = e0 * v[0].1 + e1 * v[1].1 + e2 * v[2].1;
                let i = y * w + x;
                if z > self.depth[i] {
                    self.depth[i] = z;
                    self.color[i] = color(
                        px,
                        py,
                        e0 * v[0].2 + e1 * v[1].2 + e2 * v[2].2,
                        [e0, e1, e2],
                    );
                }
            }
        }
    }
}

/// 絵に線を引く(画面を開かずに確かめる時に使う)。
pub fn draw_line(img: &mut egui::ColorImage, a: Pos2, b: Pos2, color: Color32) {
    let [w, h] = img.size;
    let n = ((b - a).length().ceil() as usize).max(1);
    for i in 0..=n {
        let p = a + (b - a) * (i as f32 / n as f32);
        let (x, y) = (p.x.round() as isize, p.y.round() as isize);
        if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
            img.pixels[y as usize * w + x as usize] = color;
        }
    }
}
