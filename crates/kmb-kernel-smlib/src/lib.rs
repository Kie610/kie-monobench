//! SMLib(NVIDIA usd-brep 1.0.0 のカーネル)による形状カーネル。kmb-core の [`Kernel`] を実装する。
//!
//! SMLib は C++ の API なので、C の薄い中継層(shim/jc_smlib.cpp)を通して呼ぶ。
//! 中継層は形の操作と問い合わせだけを行い、面の名前の決め方はここで行う。
//!
//! 面の名前: 面ごとに整数のタグ(SMLib の属性)を付け、タグと名前の対応をここで持つ。
//! SMLib はブーリアンで面が分かれたり移ったりしても属性を引き継ぐので、名前もついてくる。
//! 1 つの面が分かれた時は、truck 版と同じく位置の順に `#0`、`#1` … を付ける。
//! タグを失った面は、入力の平面と同じ平面かどうかで名前を探す(見つからなければ `unnamed`)。
//!
//! 円の側面: truck 版は円を 2 つに分けて `.a`、`.b` の 2 面にするが、こちらは 1 面
//! (`f1:side:e9`)。文書が名前で指すのは平らな面だけなので、保存済みの文書には響かない。
//!
//! STEP: 書き出しは中継層(shim/jc_step.cpp、smlibpy の書き出しを移したもの)で行う。
//! 読み込みは、まず自前の読み手(step_in.rs)で面と辺を SMLib の面に写して立体を組む(足し引き・
//! 穴・フィレットに使える)。組めない STEP(対応していない曲面・閉じない殻など)は
//! [`SmlibKernel::with_step_import`] で渡した別のカーネル(組み立てでは truck)に任せ、その形は
//! そのカーネルが持つ(網として表示・計測・加工に使える)。

mod ffi;
mod names;
mod skin;
mod step_in;

// SMLib 本体の静的ライブラリをリンクさせる(Rust から直接は使わない)
extern crate smlib_sys;

use ffi::{Brep, JcFace, JcPlane, JcSegment};
use kmb_core::kernel::{
    self as k, BooleanOp, EdgeBlend, EdgeInfo, FaceInfo, Kernel, KernelError, MassProps, Mesh, P2,
    P3, PlanarFace, Profile, ProfileEdge, ProfileLoop, Seam, Segment, ShapeHandle, dedupe_names,
};
use names::{
    adopt, blend_names, offset_slab_names, points, polyline_distance, prefer_target_names,
    shell_names,
};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// 体積の計算の相対精度(SmApiBrepComputeVolume)。
const VOLUME_ACCURACY: f64 = 1e-7;
/// 網の、隣り合う三角形の法線の角度の上限(度)。usd-brep の KNOWN_ISSUES にある
/// 「フィレット近くで三角形が裏返る」を避けるため 15° にする。
const MESH_ANGLE_DEG: f64 = 15.0;

struct Shape {
    brep: Brep,
    /// 面ごとの名前。`brep.faces(..)` の順。
    names: Vec<String>,
}

#[derive(Default)]
struct Store {
    next: u64,
    shapes: HashMap<u64, Shape>,
    tag_names: Vec<String>,
    tags: HashMap<String, i64>,
    /// 作った網(形と許容差ごと)。形は作った後に変わらないので、同じ網を加工ごとに作り直さない
    meshes: HashMap<(u64, u64), Mesh>,
    /// 面の一覧(形ごと)。面積は SMLib が積分で求めるので、穴の多い形では 1 回に 1 秒を超える。
    /// ツールバーが毎フレーム使える操作を調べる時にも呼ばれるので、形ごとに 1 回だけ求める
    faces: HashMap<u64, Vec<FaceInfo>>,
    /// 体積(形ごと)。面取り・フィレットはブーリアンのたびに前後の体積を比べるので、
    /// 前の結果の体積を求め直さない(SMLib は積分で求めるので、1 回に 0.1 秒ほどかかる)
    volumes: HashMap<u64, f64>,
    /// 体積・表面積・外形・重心(形ごと)。AI が設計を調べるたびにボディごとの質量を聞くので、
    /// ボディが千あると 1 回に数十秒かかっていた。形は作った後に変わらないので 1 回だけ求める
    masses: HashMap<u64, MassProps>,
}

impl Store {
    fn tag(&mut self, name: &str) -> i64 {
        if let Some(&t) = self.tags.get(name) {
            return t;
        }
        let t = self.tag_names.len() as i64;
        self.tag_names.push(name.to_string());
        self.tags.insert(name.to_string(), t);
        t
    }

    fn get(&self, h: ShapeHandle) -> Result<&Shape, KernelError> {
        self.shapes.get(&h.0).ok_or(KernelError::UnknownShape(h))
    }

    /// 名前をタグとして形状に書き込み、しまう。
    fn put(&mut self, mut brep: Brep, names: Vec<String>) -> Result<ShapeHandle, KernelError> {
        let tags: Vec<i64> = names.iter().map(|n| self.tag(n)).collect();
        brep.set_tags(&tags).map_err(failed("面の名前付け"))?;
        self.next += 1;
        let id = self.next;
        self.shapes.insert(id, Shape { brep, names });
        Ok(ShapeHandle(id))
    }
}

/// SMLib による [`Kernel`] の実装。
#[derive(Default)]
pub struct SmlibKernel {
    store: Mutex<Store>,
    /// STEP の読み込みを任せるカーネル
    step_import: Option<Box<dyn Kernel>>,
    /// 読み込んだ形: こちらの番号 → 任せたカーネルの番号
    imported: Mutex<HashMap<u64, ShapeHandle>>,
    /// 出現の表([`Kernel::instance`])。出現は写して動かした形を持つ(B-rep は共有しない)
    instances: Mutex<k::Instances>,
}

impl Drop for SmlibKernel {
    fn drop(&mut self) {
        let _g = ffi::lock();
        self.store
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .shapes
            .clear();
    }
}

fn failed(what: &'static str) -> impl Fn(String) -> KernelError {
    move |m| KernelError::Failed(format!("{what}: {m}"))
}

/// ロフトの断面を、穴を除いた外側の断面と、穴ごとの断面の並び(反時計回りに直した穴)に分ける。
/// 穴は断面ごとに同じ数とし、前の断面の穴にいちばん近い穴を同じ穴とみなす。
fn hollow(sections: &[k::Profile]) -> Result<(Vec<k::Profile>, Vec<Vec<k::Profile>>), String> {
    let count = |p: &k::Profile| p.regions.first().map_or(0, |r| r.holes.len());
    let n = sections.first().map_or(0, count);
    let mut outer = sections.to_vec();
    for p in &mut outer {
        for r in &mut p.regions {
            r.holes.clear();
        }
    }
    if n == 0 {
        // 最初の断面に穴が無く、ほかにある時も数の違いとして止める
        if let Some(i) = sections.iter().position(|p| count(p) != 0) {
            return Err(format!(
                "断面 {} には穴が {} つ、断面 1 には穴がありません。中空にする時はどの断面にも同じ数の穴を描いてください",
                i + 1,
                count(&sections[i])
            ));
        }
        return Ok((outer, vec![]));
    }
    let centroid = |p: &k::Profile, l: &k::ProfileLoop| -> P3 {
        let pts = l.polyline();
        let mut c = [0.0; 3];
        for q in &pts {
            let w = p.plane.to_world(*q);
            for d in 0..3 {
                c[d] += w[d] / pts.len() as f64;
            }
        }
        c
    };
    let as_profile = |p: &k::Profile, l: &k::ProfileLoop| {
        let mut l = l.clone();
        if l.signed_area() < 0.0 {
            l.reverse();
        }
        k::Profile {
            plane: p.plane,
            regions: vec![k::Region {
                outer: l,
                holes: vec![],
            }],
        }
    };
    let mut holes: Vec<Vec<k::Profile>> = sections[0].regions[0]
        .holes
        .iter()
        .map(|l| vec![as_profile(&sections[0], l)])
        .collect();
    let mut last: Vec<P3> = sections[0].regions[0]
        .holes
        .iter()
        .map(|l| centroid(&sections[0], l))
        .collect();
    for (i, p) in sections.iter().enumerate().skip(1) {
        if count(p) != n {
            return Err(format!(
                "断面 {} には穴が {} つ、断面 1 には {n} つあります。中空にする時はどの断面にも同じ数の穴を描いてください",
                i + 1,
                count(p)
            ));
        }
        let mut free: Vec<(P3, &k::ProfileLoop)> = p.regions[0]
            .holes
            .iter()
            .map(|l| (centroid(p, l), l))
            .collect();
        for (j, at) in last.iter_mut().enumerate() {
            let dist = |c: P3| {
                let d = k::sub(c, *at);
                k::dot(d, d)
            };
            let best = (0..free.len())
                .min_by(|a, b| dist(free[*a].0).total_cmp(&dist(free[*b].0)))
                .expect("穴の数は同じ");
            let (c, l) = free.swap_remove(best);
            holes[j].push(as_profile(p, l));
            *at = c;
        }
    }
    Ok((outer, holes))
}

impl SmlibKernel {
    pub fn new() -> Self {
        Self::default()
    }

    /// STEP の読み込みを別のカーネルに任せる。読み込んだ形への問い合わせ(面・網・体積など)も
    /// そのカーネルが答える。足し引きは、任せたカーネルが対応していればそのカーネルの中で行う。
    pub fn with_step_import(mut self, kernel: impl Kernel + 'static) -> Self {
        self.step_import = Some(Box::new(kernel));
        self
    }

    /// 読み込んだ形なら、任せたカーネルとその中の番号。
    /// 形を行列で写して動かした複製(面の名前の末尾に `suffix` を足す)。
    fn moved_copy(
        &self,
        shape: ShapeHandle,
        m: &k::Matrix4,
        suffix: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if let Some(why) = k::placement_problem(m) {
            return Err(KernelError::Failed(why.into()));
        }
        if let Some((k, inner)) = self.imported(shape) {
            let h = k.transform(inner, m, suffix)?;
            return Ok(self.adopt_imported(h));
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        let (mut brep, names) = {
            let src = s.get(shape)?;
            let names: Vec<String> = src.names.iter().map(|n| format!("{n}{suffix}")).collect();
            (src.brep.copy().map_err(failed("形状の複製"))?, names)
        };
        brep.transform(m).map_err(failed("移動"))?;
        // 移動で面の並びは変わらない(面の数で確かめる)
        let count = brep.faces(0).map_err(failed("面の列挙"))?.len();
        if count != names.len() {
            return Err(KernelError::Failed("移動で面の数が変わりました".into()));
        }
        s.put(brep, names)
    }

    fn imported(&self, h: ShapeHandle) -> Option<(&dyn Kernel, ShapeHandle)> {
        let inner = *self
            .imported
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&h.0)?;
        Some((self.step_import.as_deref()?, inner))
    }

    /// 任せたカーネルの形をこちらの番号で包む。
    fn adopt_imported(&self, inner: ShapeHandle) -> ShapeHandle {
        let mut s = self.lock();
        s.next += 1;
        let id = s.next;
        self.imported
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, inner);
        ShapeHandle(id)
    }

    /// 持っている形状の数。テストと動作確認用。
    pub fn shape_count(&self) -> usize {
        self.lock().shapes.len()
    }

    /// 閉じた正しい立体か、立体の数、空洞の数。動作確認用。
    pub fn check(&self, h: ShapeHandle) -> Result<(bool, i64, i64), KernelError> {
        let _g = ffi::lock();
        let s = self.lock();
        s.get(h)?.brep.check().map_err(failed("立体の検査"))
    }

    /// スイープ・ロフトの曲面の網を縫って立体にし、面に名前を付ける。
    fn skin_solid(
        &self,
        surfaces: Vec<skin::Surface>,
        tag: &str,
        what: &'static str,
    ) -> Result<ShapeHandle, KernelError> {
        let n = surfaces.len() as i64;
        let patches: Vec<ffi::Patch> = surfaces
            .iter()
            .enumerate()
            .map(|(i, s)| s.patch(i as i64))
            .collect();
        let _g = ffi::lock();
        let brep = ffi::patch_solid(&patches, n, n + 1).map_err(failed(what))?;
        let faces = brep
            .faces(ffi::FACE_INNER_POINT)
            .map_err(failed("面の列挙"))?;
        let names: Vec<String> = faces
            .iter()
            .map(|f| match f.tag {
                t if t == n => format!("{tag}:start"),
                t if t == n + 1 => format!("{tag}:end"),
                t if (0..n).contains(&t) => surfaces[t as usize].name.clone(),
                _ => format!("{tag}:unnamed"),
            })
            .collect();
        let names = dedupe_names(names, &points(&faces));
        self.lock().put(brep, names)
    }

    /// STEP を自前で読み、SMLib の面で閉じた立体に組む(M56)。面の名前は `step{殻}:f{面}`。
    fn import_step_solid(&self, step: &str) -> Result<ShapeHandle, String> {
        let model = step_in::read(step)?;
        let faces: Vec<ffi::StepFace> = model
            .faces
            .iter()
            .enumerate()
            .map(|(i, f)| step_face(f, i as i64))
            .collect();
        let _g = ffi::lock();
        let brep = ffi::step_solid(&faces, STEP_TOLERANCE).map_err(|(i, e)| match i {
            Some(i) => format!("面 {}: {e}", model.faces[i].name),
            None => e,
        })?;
        // 空洞の殻は、書いた CAD によって面の向きの付け方が違う(空洞を空洞として読む直しの
        // 記録 docs/機能/2026-10-22-STEPの空洞を空洞として読む-44mk8x.md)。数が合わなければ網で読む
        let (manifold, solids, voids) = brep.check()?;
        if !manifold || solids < 1 || voids != model.voids.len() as i64 {
            return Err(format!(
                "縫った形が正しい立体になりません(多様体 {manifold}、立体 {solids}、空洞 {voids}、STEP の空洞 {})",
                model.voids.len()
            ));
        }
        let found = brep.faces(ffi::FACE_INNER_POINT)?;
        let names: Vec<String> = found
            .iter()
            .map(|f| {
                usize::try_from(f.tag)
                    .ok()
                    .and_then(|t| model.faces.get(t))
                    .map_or_else(|| "step:unnamed".to_string(), |m| m.name.clone())
            })
            .collect();
        let names = dedupe_names(names, &points(&found));
        log::info!(
            "STEP を SMLib の立体に組みました(面 {} 枚 → {} 枚)",
            model.faces.len(),
            found.len()
        );
        self.lock().put(brep, names).map_err(|e| e.to_string())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// シェルの結果を確かめる。SMLib は厚みで内側の面同士が重なる形(穴とポケットが近い等)で、
/// 体積が 0 の不正な形を返すことがある。その時は崩れた形を出さずに理由を返す。
fn check_shell(src: &ffi::Brep, r: &ffi::Brep, thickness: f64) -> Result<(), KernelError> {
    let bad = || KernelError::Failed("厚みで内側の面同士が重なる所があります".into());
    let (manifold, solids, _) = r.check().map_err(failed("立体の検査"))?;
    if !manifold || solids < 1 {
        return Err(bad());
    }
    let v = r.mass(VOLUME_ACCURACY).map_err(failed("体積"))?.volume;
    let v0 = src.mass(VOLUME_ACCURACY).map_err(failed("体積"))?.volume;
    // 内側へ厚みを付けた殻は元より小さい。外側なら 0 より大きければよい
    if !v.is_finite() || v <= 1e-9 || (thickness > 0.0 && v >= v0) {
        return Err(bad());
    }
    Ok(())
}

fn jc_plane(p: &k::Plane) -> JcPlane {
    JcPlane {
        origin: p.origin,
        x_axis: k::normalize(p.x_axis),
        y_axis: k::normalize(p.y_axis),
    }
}

/// 輪郭を中継層の区間の並びにする。ループごとの区間の数も返す。
fn segments(loops: &[&ProfileLoop]) -> (Vec<JcSegment>, Vec<u32>) {
    let mut segs = Vec::new();
    let mut lens = Vec::new();
    for lp in loops {
        for e in &lp.edges {
            let mut s = JcSegment::default();
            match e.segment {
                Segment::Line { a, b } => {
                    s.kind = ffi::SEG_LINE;
                    (s.a, s.b) = (a, b);
                }
                Segment::Arc {
                    center,
                    radius,
                    start,
                    end,
                } => {
                    s.kind = ffi::SEG_ARC;
                    s.center = center;
                    s.radius = radius;
                    (s.start, s.end) = (start, end);
                }
                Segment::Circle { center, radius } => {
                    s.kind = ffi::SEG_CIRCLE;
                    s.center = center;
                    s.radius = radius;
                }
            }
            segs.push(s);
        }
        lens.push(lp.edges.len() as u32);
    }
    (segs, lens)
}

/// 回転軸に辺で 2 か所以上載るループ(半円の輪を直径の軸で回す時など)を、軸から軸までの線ごとに
/// 軸の上の線で閉じたループに分け、囲む面積の大きい順に並べる(中のものは引かれ、さらに中のものは
/// 足される。`jc_revolve`)。分けないと SMLib は軸で切れた殻をそれぞれ別の立体にし、中空の球が
/// 外の球と内の球の 2 つの立体になって、空洞の分が体積に足されていた(10-25 k8)。
/// 軸に載る所が 1 か所以下なら分けない(殻は 1 つに閉じる)。
fn split_on_axis(lp: &ProfileLoop, on_axis: &dyn Fn(P2) -> bool) -> Vec<ProfileLoop> {
    let n = lp.edges.len();
    let axis: Vec<bool> = lp
        .edges
        .iter()
        .map(|e| matches!(e.segment, Segment::Line { a, b } if on_axis(a) && on_axis(b)))
        .collect();
    let Some(first) = (0..n).find(|&i| axis[(i + n - 1) % n] && !axis[i]) else {
        return vec![lp.clone()];
    };
    let mut chains: Vec<Vec<ProfileEdge>> = Vec::new();
    let mut cur = Vec::new();
    for i in (first..first + n).map(|i| i % n) {
        if !axis[i] {
            cur.push(lp.edges[i].clone());
        } else if !cur.is_empty() {
            chains.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        chains.push(cur);
    }
    if chains.len() < 2 {
        return vec![lp.clone()];
    }
    let mut out: Vec<ProfileLoop> = chains
        .into_iter()
        .map(|mut edges| {
            let (from, to) = (edges[0].from_point(), edges[edges.len() - 1].to_point());
            edges.push(ProfileEdge {
                name: format!("{}:axis", edges[0].name),
                segment: Segment::Line { a: to, b: from },
                reversed: false,
            });
            ProfileLoop { edges }
        })
        .collect();
    out.sort_by(|a, b| b.signed_area().abs().total_cmp(&a.signed_area().abs()));
    out
}

fn all_loops(profile: &Profile) -> Vec<&ProfileLoop> {
    profile
        .regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .collect()
}

fn nearest_edge<'a>(loops: &[&'a ProfileLoop], q: [f64; 2]) -> Option<(&'a str, f64)> {
    loops
        .iter()
        .flat_map(|l| &l.edges)
        .map(|e| (e.name.as_str(), e.segment.distance(q)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// STEP の面を切り抜く時の許容差(mm)。
const STEP_TOLERANCE: f64 = 1e-3;

/// 読んだ STEP の面を中継層の形にする。
fn step_face(f: &step_in::StepFace, tag: i64) -> ffi::StepFace {
    use step_in::Surf;
    let fr = |f: &step_in::Frame| {
        let mut m = [0.0; 12];
        for (i, v) in [f.o, f.x, f.y, f.z].iter().enumerate() {
            m[i * 3..i * 3 + 3].copy_from_slice(v);
        }
        m
    };
    let (surface, frame) = match &f.surface {
        Surf::Plane(a) => (ffi::StepSurface::Plane, fr(a)),
        Surf::Cylinder(a, r) => (ffi::StepSurface::Cylinder(*r), fr(a)),
        Surf::Cone(a, r, t) => {
            // 円すいは面の高さの範囲だけの有限の円すいにする。無限の円すいのまま切り抜くと、
            // 先の点まで届く面の面積と体積が 1% ほど大きくなった(試験の cone-point*.step。
            // 有限にすると合う)。高さは軸に沿った距離で、頂点は -r / tan(t)。
            let apex = -r / t.tan();
            let height = |p: &[f64]| k::dot(k::sub([p[0], p[1], p[2]], a.o), a.z);
            let hs = f
                .loops
                .iter()
                .flatten()
                .flat_map(|c| c.pts.iter().map(|p| height(p)))
                .chain(f.points.iter().map(|p| height(p)));
            let (mut lo, mut hi) =
                hs.fold((f64::MAX, f64::MIN), |(lo, hi), h| (lo.min(h), hi.max(h)));
            // 縁が 1 つの高さだけ(底の円だけの、先の点まで届く面)なら頂点まで
            if hi - lo < STEP_TOLERANCE {
                lo = lo.min(apex);
                hi = hi.max(apex);
            }
            // 切り抜く曲線が曲面の端に乗らないよう少し広げる(頂点より先には広げない)
            lo = (lo - STEP_TOLERANCE * 10.0).max(apex);
            hi += STEP_TOLERANCE * 10.0;
            let mut b = fr(a);
            for (v, z) in b.iter_mut().zip(a.z) {
                *v += z * lo;
            }
            let radius = |h: f64| (r + h * t.tan()).max(0.0);
            (ffi::StepSurface::Cone(radius(lo), radius(hi), hi - lo), b)
        }
        Surf::Sphere(a, r) => (ffi::StepSurface::Sphere(*r), fr(a)),
        Surf::Torus(a, r1, r2) => (ffi::StepSurface::Torus(*r1, *r2), fr(a)),
        Surf::BSpline(s) => (
            ffi::StepSurface::BSpline(ffi::Patch {
                degree_u: s.degree_u as u32,
                degree_v: s.degree_v as u32,
                knots_u: s.knots_u.clone(),
                knots_v: s.knots_v.clone(),
                points: s.pts.clone(),
                tag: 0,
                caps: 0,
            }),
            [0.0; 12],
        ),
    };
    ffi::StepFace {
        surface,
        frame,
        same_sense: f.same_sense,
        loops: f
            .loops
            .iter()
            .map(|l| {
                l.iter()
                    .map(|c| ffi::Curve {
                        degree: c.degree as u32,
                        knots: c.knots.clone(),
                        points: c.pts.clone(),
                    })
                    .collect()
            })
            .collect(),
        points: f.points.clone(),
        tag,
    }
}

fn near(a: P3, b: P3) -> bool {
    let d = k::sub(a, b);
    k::dot(d, d) < 1e-12
}

impl Kernel for SmlibKernel {
    fn name(&self) -> &str {
        "smlib"
    }

    fn extrude(
        &self,
        profile: &Profile,
        from: f64,
        to: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if !(to - from).is_finite() || to <= from {
            return Err(KernelError::Failed(
                "押し出しの距離は 0 より大きくしてください".into(),
            ));
        }
        if profile.regions.is_empty() {
            return Err(KernelError::Failed("閉じた輪郭がありません".into()));
        }
        let loops = all_loops(profile);
        let (segs, lens) = segments(&loops);
        let plane = profile.plane;
        let n = k::normalize(plane.normal());
        let _g = ffi::lock();
        let brep =
            ffi::extrude(&jc_plane(&plane), &segs, &lens, from, to).map_err(failed("押し出し"))?;
        // 平面の代表点は外形の箱の中心: 線分を押し出した側面(平行四辺形)では面の中心になる
        let faces = brep
            .faces(ffi::FACE_INNER_POINT_CURVED)
            .map_err(failed("面の列挙"))?;
        let names: Vec<String> = faces
            .iter()
            .map(|f| {
                if f.planar != 0 && k::dot(f.normal, n).abs() > 1.0 - 1e-9 {
                    let h = k::dot(k::sub(f.plane_point, plane.origin), n);
                    if (h - from).abs() < 1e-6 {
                        return format!("{tag}:start");
                    }
                    if (h - to).abs() < 1e-6 {
                        return format!("{tag}:end");
                    }
                }
                match nearest_edge(&loops, plane.to_local(f.point)) {
                    Some((e, d)) if d < 1e-3 => format!("{tag}:side:{e}"),
                    _ => format!("{tag}:face"),
                }
            })
            .collect();
        let names = dedupe_names(names, &points(&faces));
        self.lock().put(brep, names)
    }

    fn extrude_taper(
        &self,
        profile: &Profile,
        from: f64,
        to: f64,
        taper: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if !(to - from).is_finite() || to <= from {
            return Err(KernelError::Failed(
                "押し出しの距離は 0 より大きくしてください".into(),
            ));
        }
        if from > 1e-9 || to < -1e-9 {
            return Err(KernelError::Failed(
                "テーパーの押し出しは輪郭の平面から始めてください".into(),
            ));
        }
        if !taper.is_finite() || taper.abs() >= std::f64::consts::FRAC_PI_2 {
            return Err(KernelError::Failed(
                "テーパー角は -90 度から 90 度の間にしてください".into(),
            ));
        }
        if profile.regions.is_empty() {
            return Err(KernelError::Failed("閉じた輪郭がありません".into()));
        }
        // 外周は離れるほど細く、穴は逆に太く。SMLib の角度は押し出す向きから測り、正は内へ、負は外へ
        // 傾く(ヘッダーの「90〜180 は外」は当たらない。2026-10-19 に体積で確かめた)
        let deg = taper.to_degrees();
        let mut loops = Vec::new();
        let mut drafts = Vec::new();
        let mut holes = Vec::new();
        for r in &profile.regions {
            loops.push(&r.outer);
            drafts.push(deg);
            holes.push(0);
            for h in &r.holes {
                loops.push(h);
                drafts.push(-deg);
                holes.push(1);
            }
        }
        let (segs, lens) = segments(&loops);
        let plane = profile.plane;
        let n = k::normalize(plane.normal());
        let _g = ffi::lock();
        // 輪郭の平面から表へ・裏へ、それぞれ傾けて作り、両方あれば足す
        let mut brep: Option<ffi::Brep> = None;
        for h in [to, from] {
            if h.abs() < 1e-9 {
                continue;
            }
            let piece = ffi::extrude_taper(&jc_plane(&plane), &segs, &lens, h, &drafts, &holes)
                .map_err(failed("テーパーの押し出し"))?;
            // 輪郭の細い所(幅 2 の腕など)の壁が途中で出会うと、SMLib は成功と返して閉じていない
            // 形を出す。そのまま切り取りに使うと何も削らずに通っていた(10-25 k1)
            let (manifold, solids, _) = piece.check().map_err(failed("立体の検査"))?;
            if !manifold || solids < 1 {
                return Err(KernelError::Failed(
                    "テーパーの押し出しが閉じた立体になりません(輪郭の細い所の壁が途中で出会う時に起きます。テーパー角か距離を小さくしてください)".into(),
                ));
            }
            brep = Some(match brep {
                None => piece,
                Some(mut b) => {
                    b.boolean(piece, 0)
                        .map_err(failed("テーパーの押し出しの和"))?;
                    b
                }
            });
        }
        let brep = brep.ok_or_else(|| KernelError::Failed("押し出しの距離が 0 です".into()))?;
        let faces = brep
            .faces(ffi::FACE_INNER_POINT_CURVED)
            .map_err(failed("面の列挙"))?;
        let tan = taper.tan().abs();
        let names: Vec<String> = faces
            .iter()
            .map(|f| {
                let h = k::dot(k::sub(f.point, plane.origin), n);
                if f.planar != 0 && k::dot(f.normal, n).abs() > 1.0 - 1e-9 {
                    let at = k::dot(k::sub(f.plane_point, plane.origin), n);
                    if (at - from).abs() < 1e-6 {
                        return format!("{tag}:start");
                    }
                    if (at - to).abs() < 1e-6 {
                        return format!("{tag}:end");
                    }
                }
                // 傾いた側面の中の点は、輪郭の線から高さ × tan(傾き) ほど離れる
                let q = plane.to_local(k::sub(f.point, k::scale(n, h)));
                match nearest_edge(&loops, q) {
                    Some((e, d)) if d < h.abs() * tan + 1e-3 => format!("{tag}:side:{e}"),
                    _ => format!("{tag}:face"),
                }
            })
            .collect();
        let names = dedupe_names(names, &points(&faces));
        self.lock().put(brep, names)
    }

    fn revolve(
        &self,
        profile: &Profile,
        axis_origin: P3,
        axis_dir: P3,
        angle: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if !angle.is_finite() || angle <= 1e-9 {
            return Err(KernelError::Failed(
                "回転の角度は 0 より大きくしてください".into(),
            ));
        }
        if profile.regions.is_empty() {
            return Err(KernelError::Failed("閉じた輪郭がありません".into()));
        }
        let plane = profile.plane;
        let a = k::normalize(axis_dir);
        if k::dot(a, plane.normal()).abs() > 1e-6
            || k::dot(k::sub(axis_origin, plane.origin), plane.normal()).abs() > 1e-6
        {
            return Err(KernelError::Failed(
                "回転軸はスケッチの平面の上に置いてください".into(),
            ));
        }
        // 平面の中で軸に直交する向き。輪郭がある側を正にする
        let mut u = k::cross(plane.normal(), a);
        let pts: Vec<P3> = profile
            .regions
            .iter()
            .flat_map(|r| r.outer.polyline())
            .map(|p| plane.to_world(p))
            .collect();
        let sum: f64 = pts.iter().map(|p| k::dot(k::sub(*p, axis_origin), u)).sum();
        if sum < 0.0 {
            u = k::scale(u, -1.0);
        }
        if pts
            .iter()
            .any(|p| k::dot(k::sub(*p, axis_origin), u) < -1e-9)
        {
            return Err(KernelError::Failed(
                "輪郭が回転軸をまたいでいます(軸の両側に輪郭があります。軸を「自動」か、輪郭の辺が乗る軸にしてください)".into(),
            ));
        }
        let full = angle >= std::f64::consts::TAU - 1e-9;
        let end_plane_normal = {
            let r = k::rotation(axis_origin, a, angle);
            k::sub(
                k::transform_point(&r, plane.normal()),
                k::transform_point(&r, [0.0; 3]),
            )
        };
        let loops = all_loops(profile);
        let (o2, a2) = {
            let o = plane.to_local(axis_origin);
            let t = plane.to_local(k::add(axis_origin, a));
            (o, [t[0] - o[0], t[1] - o[1]])
        };
        let on_axis = |p: P2| ((p[0] - o2[0]) * a2[1] - (p[1] - o2[1]) * a2[0]).abs() < 1e-6;
        let parts: Vec<ProfileLoop> = loops
            .iter()
            .flat_map(|l| split_on_axis(l, &on_axis))
            .collect();
        let (segs, lens) = segments(&parts.iter().collect::<Vec<_>>());
        let _g = ffi::lock();
        let brep = ffi::revolve(&jc_plane(&plane), &segs, &lens, axis_origin, a, angle)
            .map_err(failed("回転"))?;
        let faces = brep
            .faces(ffi::FACE_INNER_POINT)
            .map_err(failed("面の列挙"))?;
        // 名前の決め方は kmb-kernel-truck と同じ: 面の点を回転の前の平面へ戻し、近い区間を探す
        let names: Vec<String> = faces
            .iter()
            .map(|f| {
                let p = f.point;
                let d = k::sub(p, axis_origin);
                let h = k::dot(d, a);
                let radial = k::sub(d, k::scale(a, h));
                let r = k::dot(radial, radial).sqrt();
                let on_start = k::dot(d, plane.normal()).abs() < 1e-3 && k::dot(d, u) > 0.0;
                let on_end = !full && k::dot(d, end_plane_normal).abs() < 1e-3;
                let q = plane.to_local(k::add(axis_origin, k::add(k::scale(a, h), k::scale(u, r))));
                let (best, dist) = nearest_edge(&loops, q).unwrap_or(("", f64::INFINITY));
                if dist > 1e-2 && (on_start || on_end) {
                    if on_start {
                        format!("{tag}:start")
                    } else {
                        format!("{tag}:end")
                    }
                } else {
                    format!("{tag}:side:{best}")
                }
            })
            .collect();
        let names = dedupe_names(names, &points(&faces));
        self.lock().put(brep, names)
    }

    fn sweep(
        &self,
        profile: &Profile,
        path: &k::PlanarPath,
        twist: f64,
        scale: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let surfaces =
            skin::sweep(profile, path, twist, scale, tag).map_err(KernelError::Failed)?;
        self.skin_solid(surfaces, tag, "スイープ")
    }

    fn loft(
        &self,
        sections: &[Profile],
        rails: &[k::PlanarPath],
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let (outer, holes) = hollow(sections).map_err(KernelError::Failed)?;
        let surfaces = skin::loft(&outer, rails, tag).map_err(KernelError::Failed)?;
        let mut h = self.skin_solid(surfaces, tag, "ロフト")?;
        // 穴は穴どうしを別にロフトして引く(ガイドレールは外側だけを導く)
        for (j, hole) in holes.iter().enumerate() {
            let surfaces = skin::loft(hole, &[], tag)
                .map_err(|e| KernelError::Failed(format!("穴 {} のロフト: {e}", j + 1)))?;
            let tool = self.skin_solid(surfaces, &format!("{tag}:hole{}", j + 1), "ロフトの穴")?;
            h = self.boolean(BooleanOp::Subtract, h, tool)?;
        }
        Ok(h)
    }

    fn loft_to_point(
        &self,
        sections: &[Profile],
        point: P3,
        at_start: bool,
        rails: &[k::PlanarPath],
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let (outer, holes) = hollow(sections).map_err(KernelError::Failed)?;
        if !holes.is_empty() {
            return Err(KernelError::Failed(
                "点へのロフトには穴の無い輪郭を選んでください".into(),
            ));
        }
        let surfaces = skin::loft_to_point(&outer, point, at_start, rails, tag)
            .map_err(KernelError::Failed)?;
        self.skin_solid(surfaces, tag, "点へのロフト")
    }

    fn boolean(
        &self,
        op: BooleanOp,
        target: ShapeHandle,
        tool: ShapeHandle,
    ) -> Result<ShapeHandle, KernelError> {
        match (self.imported(target), self.imported(tool)) {
            (None, None) => {}
            (Some((k, a)), Some((_, b))) => {
                let h = k.boolean(op, a, b)?;
                return Ok(self.adopt_imported(h));
            }
            _ => {
                return Err(KernelError::Failed(
                    "STEP から読み込んだ形と、ここで作った形は足し引きできません".into(),
                ));
            }
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        let (mut result, tool_copy) = {
            let t = s.get(target)?;
            let u = s.get(tool)?;
            // 入力は他の機能も使うので、複製に対して演算する(失敗すると壊れることがある)
            (
                t.brep.copy().map_err(failed("形状の複製"))?,
                u.brep.copy().map_err(failed("形状の複製"))?,
            )
        };
        let code = match op {
            BooleanOp::Union => 0,
            BooleanOp::Subtract => 1,
            BooleanOp::Intersect => 2,
        };
        result
            .boolean(tool_copy, code)
            .map_err(failed("ブーリアン"))?;
        // 代表点(同じ名前の面の並べ替えにだけ使う)は、安い外形の箱の中心で足りる
        let faces = result.faces(0).map_err(failed("面の列挙"))?;
        // SMLib は成功と返しても、立体を 1 つも囲まない形(殻が裏返り体積が 0)を返すことがある。
        // 幅 0.01 mm ほどの細い面の縁を、相手の面が横切る時など(docs/機能/ の「ブーリアンの結果が
        // 立体か確かめる」)。空でないのに立体が無ければ、壊れた形を出さずに失敗にする
        if !faces.is_empty() {
            let (_, solids, _) = result.check().map_err(failed("立体の検査"))?;
            if solids < 1 {
                return Err(KernelError::Failed(
                    "ブーリアンの結果が閉じた立体になりませんでした(ごく細い面の近くで交わっています)"
                        .into(),
                ));
            }
        }
        let mut sources = Vec::new();
        if std::env::var_os("JC_SMLIB_DEBUG").is_some() {
            eprintln!(
                "boolean {op:?}: 面 {} 枚、タグの無い面 {} 枚",
                faces.len(),
                faces.iter().filter(|f| f.tag < 0).count()
            );
        }
        if faces.iter().any(|f| f.tag < 0) {
            // タグを失った面があれば、入力の同じ平面を探す
            for h in [target, tool] {
                let sh = s.get(h)?;
                let f = sh.brep.faces(0).map_err(failed("面の列挙"))?;
                sources.push((f, sh.names.clone()));
            }
        }
        let src: Vec<(&[JcFace], &[String])> = sources
            .iter()
            .map(|(f, n)| (f.as_slice(), n.as_slice()))
            .collect();
        let mut names = adopt(&s, &faces, &src);
        prefer_target_names(&s, &result, &faces, &mut names, target, tool)?;
        s.put(result, names)
    }

    fn transform(
        &self,
        shape: ShapeHandle,
        m: &k::Matrix4,
        suffix: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if suffix.is_empty() && k::is_rigid(m) {
            return self.instance(shape, m);
        }
        self.moved_copy(shape, m, suffix)
    }

    fn instance(&self, shape: ShapeHandle, m: &k::Matrix4) -> Result<ShapeHandle, KernelError> {
        k::place_instance(&self.instances, shape, m, |b, m| self.moved_copy(b, m, ""))
    }

    fn instance_of(&self, shape: ShapeHandle) -> Option<(ShapeHandle, k::Matrix4)> {
        self.instances
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .of(shape)
    }

    fn extrude_faces(
        &self,
        shape: ShapeHandle,
        faces: &[String],
        from: f64,
        to: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if self.imported(shape).is_some() {
            return Err(KernelError::Unsupported(
                "STEP から読み込んだ形の面の縁のままの押し出し".into(),
            ));
        }
        if !(to - from).is_finite() || to <= from {
            return Err(KernelError::Failed(
                "押し出しの距離は 0 より大きくしてください".into(),
            ));
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        let (brep, n, origin) = {
            let src = s.get(shape)?;
            let idx = faces
                .iter()
                .map(|f| {
                    src.names
                        .iter()
                        .position(|m| m == f)
                        .ok_or_else(|| KernelError::Failed(format!("押し出す面 {f} がありません")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let info = src.brep.faces(0).map_err(failed("面の列挙"))?;
            let first = &info[idx[0]];
            if first.planar == 0 {
                return Err(KernelError::Failed(format!(
                    "面 {} は平らではありません",
                    faces[0]
                )));
            }
            let b = src
                .brep
                .extrude_faces(&idx, from, to)
                .map_err(failed("面の押し出し"))?;
            (b, k::normalize(first.normal), first.plane_point)
        };
        let out = brep
            .faces(ffi::FACE_INNER_POINT_CURVED)
            .map_err(failed("面の列挙"))?;
        let mut side = 0;
        let names: Vec<String> = out
            .iter()
            .map(|f| {
                if f.planar != 0 && k::dot(f.normal, n).abs() > 1.0 - 1e-9 {
                    let h = k::dot(k::sub(f.plane_point, origin), n);
                    if (h - from).abs() < 1e-6 {
                        return format!("{tag}:start");
                    }
                    if (h - to).abs() < 1e-6 {
                        return format!("{tag}:end");
                    }
                }
                side += 1;
                format!("{tag}:side:s{side}")
            })
            .collect();
        let names = dedupe_names(names, &points(&out));
        s.put(brep, names)
    }

    fn shell(
        &self,
        shape: ShapeHandle,
        open: &[String],
        thickness: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if self.imported(shape).is_some() {
            return Err(KernelError::Failed(
                "STEP から読み込んだ形はシェルにできません".into(),
            ));
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        let (result, src_faces, src_names) = {
            let src = s.get(shape)?;
            let idx =
                open.iter()
                    .map(|n| {
                        src.names.iter().position(|m| m == n).ok_or_else(|| {
                            KernelError::Failed(format!("開ける面 {n} がありません"))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            let r = src
                .brep
                .shell(&idx, -thickness)
                .map_err(|m| KernelError::Failed(format!("シェルを作れませんでした。{m}")))?;
            check_shell(&src.brep, &r, thickness)?;
            let f = src.brep.faces(0).map_err(failed("面の列挙"))?;
            (r, f, src.names.clone())
        };
        let faces = result.faces(0).map_err(failed("面の列挙"))?;
        let names = shell_names(&s, &faces, &src_faces, &src_names, tag);
        s.put(result, names)
    }

    fn offset_faces(
        &self,
        shape: ShapeHandle,
        faces: &[String],
        distance: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if self.imported(shape).is_some() {
            return Err(KernelError::Failed(
                "STEP から読み込んだ形は面をオフセットできません".into(),
            ));
        }
        if faces.is_empty() {
            return Err(KernelError::Failed("オフセットする面がありません".into()));
        }
        if distance.abs() < 1e-9 {
            return Ok(shape);
        }
        // 選んだ面だけを残して残りを開けたシェルが、面と動かした先の間の板になる(SMLib に面を 1 つ
        // だけ動かす関数は無い)。外へなら板を足し、内へなら板を引く
        let slab = {
            let _g = ffi::lock();
            let mut s = self.lock();
            let (slab, slab_faces, src_faces, src_names) = {
                let src = s.get(shape)?;
                let keep = faces
                    .iter()
                    .map(|n| {
                        src.names.iter().position(|m| m == n).ok_or_else(|| {
                            KernelError::Failed(format!("オフセットする面 {n} がありません"))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let open: Vec<usize> = (0..src.names.len()).filter(|i| !keep.contains(i)).collect();
                // 前のオフセットでできた面(SMLib が B-spline で近似した面)は、凸の辺を延ばす指定では
                // 板が崩れることがある。崩れたら指定を外して作り直す
                let mut found = None;
                for flags in [3, 2, 1, 0] {
                    let Ok(slab) = src.brep.shell_with(&open, distance, flags) else {
                        continue;
                    };
                    let (manifold, solids, _) = slab.check().map_err(failed("立体の検査"))?;
                    let v = slab.mass(VOLUME_ACCURACY).map_err(failed("体積"))?.volume;
                    if std::env::var_os("JC_SMLIB_DEBUG").is_some() {
                        eprintln!(
                            "offset slab {flags}: manifold {manifold} solids {solids} volume {v}"
                        );
                    }
                    if manifold && solids >= 1 && v.is_finite() && v > 1e-9 {
                        found = Some(slab);
                        break;
                    }
                }
                let Some(slab) = found else {
                    return Err(KernelError::Failed(
                        "面を動かした形を作れません(動かす量が大きすぎるか、まわりの面と重なります)"
                            .into(),
                    ));
                };
                let f = slab.faces(0).map_err(failed("面の列挙"))?;
                let sf = src.brep.faces(0).map_err(failed("面の列挙"))?;
                (slab, f, sf, src.names.clone())
            };
            let names = offset_slab_names(&s, &slab_faces, &src_faces, &src_names, faces, tag);
            s.put(slab, names)?
        };
        let op = if distance > 0.0 {
            BooleanOp::Union
        } else {
            BooleanOp::Subtract
        };
        let r = self.boolean(op, shape, slab);
        let _g = ffi::lock();
        let mut s = self.lock();
        s.shapes.remove(&slab.0);
        let h = r.map_err(|e| KernelError::Failed(format!("面を動かした形の足し引き: {e}")))?;
        // 板の元の位置の面が残った所(まわりと面一でない所)は、オフセットでできた面と名付ける
        let base = format!("{tag}:offset:base:");
        if s.get(h)?.names.iter().any(|n| n.starts_with(&base)) {
            let sh = s.shapes.remove(&h.0).expect("今あった");
            let names: Vec<String> = sh
                .names
                .iter()
                .map(|n| {
                    if n.starts_with(&base) {
                        format!("{tag}:offset")
                    } else {
                        n.clone()
                    }
                })
                .collect();
            let pts = points(&sh.brep.faces(0).map_err(failed("面の列挙"))?);
            return s.put(sh.brep, dedupe_names(names, &pts));
        }
        Ok(h)
    }

    fn rename_faces(
        &self,
        shape: ShapeHandle,
        renames: &[(String, String)],
    ) -> Result<ShapeHandle, KernelError> {
        if let Some((k, inner)) = self.imported(shape) {
            let h = k.rename_faces(inner, renames)?;
            return Ok(self.adopt_imported(h));
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        let (brep, mut names) = {
            let src = s.get(shape)?;
            (
                src.brep.copy().map_err(failed("形状の複製"))?,
                src.names.clone(),
            )
        };
        for (from, to) in renames {
            let n = names
                .iter_mut()
                .find(|n| *n == from)
                .ok_or_else(|| KernelError::Failed(format!("面 {from} がありません")))?;
            *n = to.clone();
        }
        s.put(brep, names)
    }

    fn remove_blend_faces(
        &self,
        shape: ShapeHandle,
        faces: &[String],
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if self.imported(shape).is_some() {
            return Err(KernelError::Failed(
                "STEP から読み込んだ形の丸めは消せません".into(),
            ));
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        let brep = {
            let src = s.get(shape)?;
            let at = faces
                .iter()
                .map(|n| {
                    src.names
                        .iter()
                        .position(|m| m == n)
                        .ok_or_else(|| KernelError::Failed(format!("消す面 {n} がありません")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            src.brep
                .delete_fillet_faces(&at)
                .map_err(failed("丸めの面の削除"))?
        };
        let (manifold, solids, _) = brep.check().map_err(failed("立体の検査"))?;
        if !manifold || solids != 1 {
            return Err(KernelError::Failed(
                "丸めの面を消した後の形が閉じた 1 つの立体になりません".into(),
            ));
        }
        let f = brep
            .faces(ffi::FACE_INNER_POINT)
            .map_err(failed("面の列挙"))?;
        let names: Vec<String> = f
            .iter()
            .map(|x| {
                (x.tag >= 0)
                    .then(|| s.tag_names.get(x.tag as usize).cloned())
                    .flatten()
                    .unwrap_or_else(|| format!("{tag}:healed"))
            })
            .collect();
        let names = dedupe_names(names, &points(&f));
        s.put(brep, names)
    }

    fn face_names(&self, shape: ShapeHandle) -> Result<Vec<String>, KernelError> {
        if let Some((k, inner)) = self.imported(shape) {
            return k.face_names(inner);
        }
        Ok(self.lock().get(shape)?.names.clone())
    }

    fn faces(&self, shape: ShapeHandle) -> Result<Vec<FaceInfo>, KernelError> {
        if let Some((k, inner)) = self.imported(shape) {
            return k.faces(inner);
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        if let Some(f) = s.faces.get(&shape.0) {
            return Ok(f.clone());
        }
        let sh = s.get(shape)?;
        let faces = sh.brep.faces(ffi::FACE_AREA).map_err(failed("面の列挙"))?;
        let out: Vec<FaceInfo> = faces
            .iter()
            .zip(&sh.names)
            .map(|(f, n)| FaceInfo {
                name: n.clone(),
                planar: (f.planar != 0).then_some(PlanarFace {
                    point: f.plane_point,
                    normal: f.normal,
                }),
                area: f.area,
            })
            .collect();
        s.faces.insert(shape.0, out.clone());
        Ok(out)
    }

    fn edges(&self, shape: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
        if let Some((k, inner)) = self.imported(shape) {
            return k.edges(inner);
        }
        let _g = ffi::lock();
        let s = self.lock();
        let sh = s.get(shape)?;
        let edges = sh.brep.edges().map_err(failed("辺の列挙"))?;
        // 両側の平面の法線から、角か継ぎ目かを求める
        let planes = sh.brep.faces(0).map_err(failed("面の列挙"))?;
        let seam = |f0: u32, f1: u32| match (planes.get(f0 as usize), planes.get(f1 as usize)) {
            (Some(a), Some(b)) if a.planar != 0 && b.planar != 0 => {
                Seam::from_normals(a.normal, b.normal)
            }
            _ => Seam::Unknown,
        };
        let mut out: Vec<EdgeInfo> = edges
            .iter()
            .filter_map(|e| {
                let mut faces = [
                    sh.names.get(e.f0 as usize)?.clone(),
                    sh.names.get(e.f1 as usize)?.clone(),
                ];
                let (mut a, mut b) = (e.a, e.b);
                if faces[0] > faces[1] {
                    faces.swap(0, 1);
                    std::mem::swap(&mut a, &mut b);
                }
                Some(EdgeInfo {
                    curve: Vec::new(),
                    faces,
                    a,
                    b,
                    convex: e.convex != 0,
                    seam: seam(e.f0, e.f1),
                    arc: None,
                })
            })
            .collect();
        out.sort_by(|x, y| x.faces.cmp(&y.faces));
        Ok(out)
    }

    fn arc_edges(&self, shape: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
        if let Some((k, inner)) = self.imported(shape) {
            return k.arc_edges(inner);
        }
        let _g = ffi::lock();
        let s = self.lock();
        let sh = s.get(shape)?;
        let edges = sh.brep.arc_edges().map_err(failed("円の辺の列挙"))?;
        let mut out: Vec<EdgeInfo> = edges
            .iter()
            .filter_map(|e| {
                Some(EdgeInfo {
                    curve: Vec::new(),
                    faces: [
                        sh.names.get(e.f0 as usize)?.clone(),
                        sh.names.get(e.f1 as usize)?.clone(),
                    ],
                    a: e.a,
                    b: e.b,
                    convex: e.convex != 0,
                    // 平面とそれに垂直な円筒の境目は、いつも直角の角
                    seam: Seam::Corner,
                    arc: Some(kmb_core::ArcEdge {
                        center: e.center,
                        axis: e.axis,
                        radius: e.radius,
                        sweep: e.sweep,
                        inside: e.inside != 0,
                    }),
                })
            })
            .collect();
        out.sort_by(|x, y| x.faces.cmp(&y.faces));
        Ok(out)
    }

    fn curve_edges(&self, shape: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
        if let Some((k, inner)) = self.imported(shape) {
            return k.curve_edges(inner);
        }
        let _g = ffi::lock();
        let s = self.lock();
        let sh = s.get(shape)?;
        let all = sh.brep.all_edges().map_err(failed("辺の列挙"))?;
        let straight = sh.brep.edges().map_err(failed("辺の列挙"))?;
        let arcs = sh.brep.arc_edges().map_err(failed("円の辺の列挙"))?;
        let same_faces = |f: [usize; 2], g0: u32, g1: u32| {
            let g = [g0 as usize, g1 as usize];
            f == g || f == [g[1], g[0]]
        };
        let ends =
            |a: P3, b: P3, p: P3, q: P3| (near(a, p) && near(b, q)) || (near(a, q) && near(b, p));
        let mut out: Vec<EdgeInfo> = all
            .into_iter()
            .filter(|e| e.points.len() >= 2)
            .filter(|e| {
                let (p, q) = (e.points[0], e.points[e.points.len() - 1]);
                !straight
                    .iter()
                    .any(|s| same_faces(e.faces, s.f0, s.f1) && ends(s.a, s.b, p, q))
                    && !arcs
                        .iter()
                        .any(|a| same_faces(e.faces, a.f0, a.f1) && ends(a.a, a.b, p, q))
            })
            .filter_map(|e| {
                let mut faces = [
                    sh.names.get(e.faces[0])?.clone(),
                    sh.names.get(e.faces[1])?.clone(),
                ];
                let mut curve = e.points;
                if faces[0] > faces[1] {
                    faces.swap(0, 1);
                    curve.reverse();
                }
                Some(EdgeInfo {
                    faces,
                    a: curve[0],
                    b: *curve.last()?,
                    convex: e.convex,
                    seam: match e.smooth {
                        Some(true) => Seam::Smooth,
                        Some(false) => Seam::Corner,
                        None => Seam::Unknown,
                    },
                    arc: None,
                    curve,
                })
            })
            .collect();
        out.sort_by(|x, y| x.faces.cmp(&y.faces));
        Ok(out)
    }

    fn blend_edges(
        &self,
        shape: ShapeHandle,
        blends: &[EdgeBlend],
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if self.imported(shape).is_some() {
            return Err(KernelError::Failed(
                "STEP から読み込んだ形の辺は丸められません".into(),
            ));
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        let (mut brep, jb, blended, radii) = {
            let src = s.get(shape)?;
            let index = |n: &str| {
                src.names
                    .iter()
                    .position(|m| m == n)
                    .ok_or_else(|| KernelError::Failed(format!("面 {n} がありません")))
            };
            let all = src.brep.all_edges().map_err(failed("辺の列挙"))?;
            let mut jb = Vec::with_capacity(blends.len());
            let mut radii: Vec<f64> = Vec::new();
            let mut blended = Vec::with_capacity(blends.len());
            for b in blends {
                let (f0, f1) = (index(&b.faces[0])?, index(&b.faces[1])?);
                let (law, start, first, count, size) = match &b.law {
                    k::BlendLaw::Constant => (0, [0.0; 3], 0, 0, b.size),
                    k::BlendLaw::Chord => (1, [0.0; 3], 0, 0, b.size),
                    k::BlendLaw::Variable { start, radii: r } => {
                        let first = radii.len() / 2;
                        radii.extend(r.iter().flat_map(|&(at, v)| [at, v]));
                        let max = r.iter().map(|x| x.1).fold(0.0, f64::max);
                        (2, *start, first as u32, r.len() as u32, max)
                    }
                };
                jb.push(ffi::JcBlend {
                    f0: f0 as u32,
                    f1: f1 as u32,
                    point: b.point,
                    size: b.size,
                    flat: i32::from(b.flat),
                    law,
                    g2: i32::from(b.g2 && !b.flat),
                    weight: b.weight,
                    start,
                    first,
                    count,
                });
                // 名前付けに使う、辺の折れ線(同じ面の組の辺のうち、点に一番近いもの)
                let line = all
                    .iter()
                    .filter(|e| e.faces == [f0, f1] || e.faces == [f1, f0])
                    .min_by(|x, y| {
                        polyline_distance(&x.points, b.point)
                            .0
                            .total_cmp(&polyline_distance(&y.points, b.point).0)
                    })
                    .map(|e| e.points.clone())
                    .unwrap_or_else(|| vec![b.point]);
                blended.push((&b.faces, line, size));
            }
            (
                src.brep.copy().map_err(failed("形状の複製"))?,
                jb,
                blended,
                radii,
            )
        };
        // 途中で変わる半径は、SMLib が閉じた 1 つの立体のまま反対側の小片だけを返すことがある
        // (60×40×8 の板の辺を半径 1 mm・途中 0.5 mm で丸めると、体積 27 mm³ の棒になった)。
        // 体積の変わり方で確かめる(一定の半径では起きていないので、計算の時間を足さない)
        let variable = blends
            .iter()
            .any(|b| matches!(b.law, k::BlendLaw::Variable { .. }));
        let before = if variable {
            let v = match s.volumes.get(&shape.0) {
                Some(&v) => v,
                None => s
                    .get(shape)?
                    .brep
                    .volume(VOLUME_ACCURACY)
                    .map_err(failed("体積の計算"))?,
            };
            Some(v)
        } else {
            None
        };
        brep.blend_edges(&jb, &radii)
            .map_err(failed("フィレット"))?;
        let (manifold, solids, _) = brep.check().map_err(failed("立体の検査"))?;
        if !manifold || solids != 1 {
            return Err(KernelError::Failed(
                "フィレット: 丸めた後の形が閉じた 1 つの立体になりません".into(),
            ));
        }
        if let Some(before) = before {
            // 丸めで変わる体積は、辺ごとに 半径² × 辺の長さ より小さい(凸の辺で削るのは約 0.21 倍)
            let room: f64 = blended
                .iter()
                .map(|(_, line, size)| {
                    let len: f64 = line
                        .windows(2)
                        .map(|w| {
                            let d = k::sub(w[1], w[0]);
                            k::dot(d, d).sqrt()
                        })
                        .sum();
                    size * size * len.max(*size)
                })
                .sum();
            let after = brep.volume(VOLUME_ACCURACY).map_err(failed("体積の計算"))?;
            if (after - before).abs() > room + 1e-6 * before.abs().max(1.0) {
                return Err(KernelError::Failed(format!(
                    "フィレット: 途中で変わる半径の丸めが形を壊しました(体積が {before:.1} から {after:.1} mm³ に)。半径か途中の半径を変えてください"
                )));
            }
        }
        let faces = brep
            .faces(ffi::FACE_INNER_POINT)
            .map_err(failed("面の列挙"))?;
        let names = blend_names(&s, &faces, &blended, tag);
        s.put(brep, names)
    }

    fn mass(&self, shape: ShapeHandle) -> Result<MassProps, KernelError> {
        if let Some((k, inner)) = self.imported(shape) {
            return k.mass(inner);
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        if let Some(&m) = s.masses.get(&shape.0) {
            return Ok(m);
        }
        let m = s
            .get(shape)?
            .brep
            .mass(VOLUME_ACCURACY)
            .map_err(failed("体積の計算"))?;
        s.volumes.insert(shape.0, m.volume);
        let props = MassProps {
            volume: m.volume,
            area: m.area,
            min: m.min,
            max: m.max,
            centroid: (m.has_centroid != 0).then_some(m.centroid),
        };
        s.masses.insert(shape.0, props);
        Ok(props)
    }

    fn volume(&self, shape: ShapeHandle) -> Result<f64, KernelError> {
        if let Some((k, inner)) = self.imported(shape) {
            return k.volume(inner);
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        if let Some(&v) = s.volumes.get(&shape.0) {
            return Ok(v);
        }
        let v = s
            .get(shape)?
            .brep
            .volume(VOLUME_ACCURACY)
            .map_err(failed("体積の計算"))?;
        s.volumes.insert(shape.0, v);
        Ok(v)
    }

    fn bounds(&self, shape: ShapeHandle) -> Result<(P3, P3), KernelError> {
        if let Some((k, inner)) = self.imported(shape) {
            return k.bounds(inner);
        }
        let _g = ffi::lock();
        let s = self.lock();
        s.get(shape)?.brep.bounds().map_err(failed("外形の箱"))
    }

    fn mesh(&self, shape: ShapeHandle, tolerance: f64) -> Result<Mesh, KernelError> {
        if let Some((k, inner)) = self.imported(shape) {
            return k.mesh(inner, tolerance);
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        let key = (shape.0, tolerance.to_bits());
        if let Some(m) = s.meshes.get(&key) {
            return Ok(m.clone());
        }
        let sh = s.get(shape)?;
        let m = sh
            .brep
            .mesh(tolerance, MESH_ANGLE_DEG)
            .map_err(failed("テッセレーション"))?;
        let three = |v: &[f32]| -> Vec<[f32; 3]> { v.as_chunks::<3>().0.to_vec() };
        let mut edges = Vec::with_capacity(m.edge_counts.len());
        let pts = three(&m.edge_points);
        let mut i = 0usize;
        for &c in &m.edge_counts {
            let c = c as usize;
            if c >= 2 && i + c <= pts.len() {
                edges.push(pts[i..i + c].to_vec());
            }
            i += c;
        }
        let nfaces = sh.names.len() as u32;
        let mesh = Mesh {
            positions: three(&m.positions),
            normals: three(&m.normals),
            triangles: m.triangles.as_chunks::<3>().0.to_vec(),
            triangle_faces: m
                .tri_face
                .iter()
                .map(|&f| if f < nfaces { f } else { 0 })
                .collect(),
            face_names: sh.names.clone(),
            edges,
        };
        s.meshes.insert(key, mesh.clone());
        Ok(mesh)
    }

    fn export_step(&self, shapes: &[ShapeHandle]) -> Result<String, KernelError> {
        let names: Vec<String> = (1..=shapes.len()).map(|i| format!("body{i}")).collect();
        self.export_step_named(shapes, &names, "kieMonoBench")
    }

    fn export_step_named(
        &self,
        shapes: &[ShapeHandle],
        names: &[String],
        product: &str,
    ) -> Result<String, KernelError> {
        let imported: Vec<_> = shapes.iter().map(|h| self.imported(*h)).collect();
        if imported.iter().all(Option::is_some) && !shapes.is_empty() {
            let (k, _) = imported[0].expect("すべて読み込んだ形");
            let inner: Vec<ShapeHandle> = imported.iter().map(|x| x.expect("同上").1).collect();
            return k.export_step_named(&inner, names, product);
        }
        if imported.iter().any(Option::is_some) {
            return Err(KernelError::Failed(
                "STEP から読み込んだ形は、ここで作った形と一緒に STEP へ書き出せません".into(),
            ));
        }
        let _g = ffi::lock();
        let s = self.lock();
        let breps = shapes
            .iter()
            .map(|h| s.get(*h).map(|x| &x.brep))
            .collect::<Result<Vec<_>, _>>()?;
        let fallback: Vec<String>;
        let names = if names.len() == breps.len() {
            names
        } else {
            fallback = (1..=breps.len()).map(|i| format!("body{i}")).collect();
            &fallback
        };
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        ffi::export_step(&breps, &names, product).map_err(failed("STEP の書き出し"))
    }

    fn import_step(&self, step: &str) -> Result<ShapeHandle, KernelError> {
        let why = match self.import_step_solid(step) {
            Ok(h) => return Ok(h),
            Err(e) => e,
        };
        let Some(k) = self.step_import.as_deref() else {
            return Err(KernelError::Failed(format!(
                "STEP を立体に組めません: {why}"
            )));
        };
        log::info!("STEP を SMLib の立体に組めないので網で読みます: {why}");
        // 網でも読めない時は、両方の理由を返す(網の読み手の理由だけでは、立体に組めなかった
        // 本当の理由が見えない)
        let h = k.import_step(step).map_err(|e| {
            KernelError::Failed(format!(
                "STEP を立体に組めません: {why}。網でも読めません: {e}"
            ))
        })?;
        Ok(self.adopt_imported(h))
    }

    fn retain(&self, live: &HashSet<ShapeHandle>) {
        let live = &self
            .instances
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(live);
        {
            let mut imported = self.imported.lock().unwrap_or_else(|e| e.into_inner());
            imported.retain(|id, _| live.contains(&ShapeHandle(*id)));
            if let Some(k) = &self.step_import {
                k.retain(&imported.values().copied().collect());
            }
        }
        let _g = ffi::lock();
        let mut s = self.lock();
        s.shapes.retain(|id, _| live.contains(&ShapeHandle(*id)));
        s.meshes
            .retain(|(id, _), _| live.contains(&ShapeHandle(*id)));
        s.volumes.retain(|id, _| live.contains(&ShapeHandle(*id)));
        s.masses.retain(|id, _| live.contains(&ShapeHandle(*id)));
    }
}
