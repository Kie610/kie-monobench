//! C の中継層(shim/jc_smlib.h)の宣言と、それを包む安全な型。
//!
//! SMLib は 1 つの文脈を全体で共有するので、ここの関数はすべて [`lock`] を取った状態で呼ぶ。
//! [`Brep`] の後始末(`Drop`)も錠を取らずに SMLib を呼ぶので、錠を持っている間に捨てること。
#![allow(unsafe_code)]

use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::{Mutex, MutexGuard};

#[repr(C)]
pub struct JcBrep {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct JcSegment {
    pub kind: i32,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub center: [f64; 2],
    pub radius: f64,
    pub start: f64,
    pub end: f64,
}

pub const SEG_LINE: i32 = 0;
pub const SEG_ARC: i32 = 1;
pub const SEG_CIRCLE: i32 = 2;

/// 面積も求める
pub const FACE_AREA: i32 = 1;
/// 代表点を面の内側の点にする(無ければ外形の箱の中心)
pub const FACE_INNER_POINT: i32 = 2;
/// 曲面だけ代表点を面の内側の点にする
pub const FACE_INNER_POINT_CURVED: i32 = 4;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct JcPlane {
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
}

/// 立体を組む曲面 1 枚(jc_smlib.h の JcPatch)。
#[repr(C)]
pub struct JcPatch {
    pub degree_u: u32,
    pub degree_v: u32,
    pub count_u: u32,
    pub count_v: u32,
    pub knots_u: *const f64,
    pub knots_v: *const f64,
    pub points: *const f64,
    pub tag: i64,
    pub caps: i32,
}

/// STEP の辺の曲線(jc_smlib.h の JcCurve)。
#[repr(C)]
pub struct JcCurve {
    pub degree: u32,
    pub count: u32,
    pub knots: *const f64,
    pub points: *const f64,
}

/// STEP の面(jc_smlib.h の JcStepFace)。
#[repr(C)]
pub struct JcStepFace {
    pub kind: i32,
    pub frame: [f64; 12],
    pub r1: f64,
    pub r2: f64,
    pub h: f64,
    pub patch: JcPatch,
    pub same_sense: i32,
    pub loop_count: u32,
    pub loop_sizes: *const u32,
    pub curves: *const JcCurve,
    pub point_count: u32,
    pub points: *const f64,
    pub tag: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct JcFace {
    pub tag: i64,
    pub planar: i32,
    pub normal: [f64; 3],
    pub plane_point: [f64; 3],
    pub point: [f64; 3],
    pub area: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct JcEdge {
    pub f0: u32,
    pub f1: u32,
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub convex: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct JcArcEdge {
    pub f0: u32,
    pub f1: u32,
    pub center: [f64; 3],
    pub axis: [f64; 3],
    pub radius: f64,
    pub sweep: f64,
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub inside: i32,
    pub convex: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct JcMassProps {
    pub volume: f64,
    pub area: f64,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub centroid: [f64; 3],
    pub has_centroid: i32,
}

#[repr(C)]
pub struct JcMesh {
    pub vertex_count: usize,
    pub positions: *mut f32,
    pub normals: *mut f32,
    pub triangle_count: usize,
    pub triangles: *mut u32,
    pub tri_face: *mut u32,
    pub edge_count: usize,
    pub edge_counts: *mut u32,
    pub edge_points: *mut f32,
}

/// 2 つの面の境目の辺(形を問わない)。折れ線は `jc_all_edges` の点の並びの `first..first + count`。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JcAnyEdge {
    pub f0: u32,
    pub f1: u32,
    pub convex: i32,
    pub smooth: i32,
    pub first: u32,
    pub count: u32,
}

/// 丸める(面取りする)辺 1 本。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JcBlend {
    pub f0: u32,
    pub f1: u32,
    pub point: [f64; 3],
    pub size: f64,
    pub flat: i32,
    /// 0 = 一定、1 = 弦の長さ、2 = 途中で変わる(`radii` の `first..first + count` の組)
    pub law: i32,
    pub g2: i32,
    pub weight: f64,
    pub start: [f64; 3],
    pub first: u32,
    pub count: u32,
}

/// 辺 1 本(面の添字の組、出っ張りか、なめらかな継ぎ目か、折れ線)。
#[derive(Debug, Clone)]
pub struct AnyEdge {
    pub faces: [usize; 2],
    pub convex: bool,
    /// なめらかな継ぎ目か(`None` は面の法線を求められず分からない)
    pub smooth: Option<bool>,
    pub points: Vec<[f64; 3]>,
}

#[repr(C)]
pub struct JcBoundary {
    pub kind: i32,
    pub count: u32,
    pub points: *const f64,
    pub brep: *const JcBrep,
    pub f0: u32,
    pub f1: u32,
    pub center: [f64; 3],
    pub axis: [f64; 3],
    pub sweep: f64,
}

unsafe extern "C" {
    fn jc_planar_sheet(
        plane: *const JcPlane,
        segments: *const JcSegment,
        lens: *const u32,
        loops: usize,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_patch_sheet(
        boundary: *const JcBoundary,
        count: usize,
        planar: i32,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_is_sheet(brep: *const JcBrep, out: *mut i32) -> i32;
    fn jc_last_error() -> *const c_char;
    fn jc_all_edges(
        brep: *const JcBrep,
        out: *mut *mut JcAnyEdge,
        count: *mut usize,
        points: *mut *mut f64,
        point_count: *mut usize,
    ) -> i32;
    fn jc_blend_edges(
        brep: *mut JcBrep,
        blends: *const JcBlend,
        count: usize,
        radii: *const f64,
    ) -> i32;
    fn jc_extrude(
        plane: *const JcPlane,
        segments: *const JcSegment,
        loop_lens: *const u32,
        loop_count: usize,
        from: f64,
        to: f64,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_extrude_taper(
        plane: *const JcPlane,
        segments: *const JcSegment,
        loop_lens: *const u32,
        loop_count: usize,
        height: f64,
        drafts: *const f64,
        holes: *const i32,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_revolve(
        plane: *const JcPlane,
        segments: *const JcSegment,
        loop_lens: *const u32,
        loop_count: usize,
        axis_origin: *const f64,
        axis_dir: *const f64,
        angle: f64,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_patch_solid(
        patches: *const JcPatch,
        count: usize,
        start_tag: i64,
        end_tag: i64,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_step_solid(
        faces: *const JcStepFace,
        count: usize,
        tolerance: f64,
        failed: *mut i32,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_iges_face(face: *const JcStepFace, uv: i32, tolerance: f64, out: *mut *mut JcBrep)
    -> i32;
    fn jc_iges_stitch(
        faces: *const *const JcBrep,
        count: usize,
        tolerance: f64,
        mode: i32,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_brep_copy(brep: *const JcBrep, out: *mut *mut JcBrep) -> i32;
    fn jc_brep_free(brep: *mut JcBrep);
    fn jc_boolean(target: *mut JcBrep, tool: *mut JcBrep, op: i32) -> i32;
    fn jc_transform(brep: *mut JcBrep, m: *const f64) -> i32;
    fn jc_extrude_faces(
        brep: *const JcBrep,
        faces: *const usize,
        count: usize,
        from: f64,
        to: f64,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_shell(
        brep: *const JcBrep,
        open: *const usize,
        open_count: usize,
        offset: f64,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_delete_fillet_faces(
        brep: *const JcBrep,
        faces: *const usize,
        count: usize,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_shell_with(
        brep: *const JcBrep,
        open: *const usize,
        open_count: usize,
        offset: f64,
        flags: i32,
        out: *mut *mut JcBrep,
    ) -> i32;
    fn jc_face_count(brep: *const JcBrep, out: *mut usize) -> i32;
    fn jc_faces(brep: *const JcBrep, flags: i32, out: *mut JcFace, cap: usize) -> i32;
    fn jc_set_face_tags(brep: *mut JcBrep, tags: *const i64, count: usize) -> i32;
    fn jc_edges(brep: *const JcBrep, out: *mut *mut JcEdge, count: *mut usize) -> i32;
    fn jc_free_buffer(p: *mut std::ffi::c_void);
    fn jc_face_distance(brep: *const JcBrep, face: usize, p: *const f64, out: *mut f64) -> i32;
    fn jc_arc_edges(brep: *const JcBrep, out: *mut *mut JcArcEdge, count: *mut usize) -> i32;
    fn jc_mass(brep: *const JcBrep, relative_accuracy: f64, out: *mut JcMassProps) -> i32;
    fn jc_volume(brep: *const JcBrep, relative_accuracy: f64, out: *mut f64) -> i32;
    fn jc_bounds(brep: *const JcBrep, min: *mut f64, max: *mut f64) -> i32;
    fn jc_check(brep: *const JcBrep, manifold: *mut i32, solids: *mut i64, voids: *mut i64) -> i32;
    fn jc_mesh(brep: *const JcBrep, chord: f64, angle_deg: f64, out: *mut *mut JcMesh) -> i32;
    fn jc_mesh_free(mesh: *mut JcMesh);
    fn jc_export_step(
        breps: *const *const JcBrep,
        names: *const *const c_char,
        count: usize,
        product: *const c_char,
        out: *mut *mut c_char,
    ) -> i32;
}

static SMLIB: Mutex<()> = Mutex::new(());

/// SMLib を呼ぶための錠。
pub fn lock() -> MutexGuard<'static, ()> {
    SMLIB.lock().unwrap_or_else(|e| e.into_inner())
}

fn status(code: i32) -> Result<(), String> {
    if code == 0 {
        return Ok(());
    }
    // SAFETY: jc_last_error は、このスレッドの最後の失敗の文(NUL 終端)を返す
    let msg = unsafe { CStr::from_ptr(jc_last_error()) };
    Err(msg.to_string_lossy().into_owned())
}

/// SMLib の形状(SmBrep)を 1 つ持つ。
pub struct Brep(*mut JcBrep);

// SAFETY: SMLib の呼び出しはすべて錠の中で行うので、スレッドをまたいで渡してよい
unsafe impl Send for Brep {}
unsafe impl Sync for Brep {}

impl Drop for Brep {
    fn drop(&mut self) {
        // SAFETY: 自分が持つ形状を 1 回だけ消す
        unsafe { jc_brep_free(self.0) }
    }
}

fn take(code: i32, out: *mut JcBrep) -> Result<Brep, String> {
    status(code)?;
    if out.is_null() {
        return Err("SMLib が形状を返しませんでした".into());
    }
    Ok(Brep(out))
}

pub fn extrude(
    plane: &JcPlane,
    segments: &[JcSegment],
    loop_lens: &[u32],
    from: f64,
    to: f64,
) -> Result<Brep, String> {
    debug_assert_eq!(loop_lens.iter().sum::<u32>() as usize, segments.len());
    let mut out = std::ptr::null_mut();
    // SAFETY: 配列の長さは呼び出し側で揃えてある
    let code = unsafe {
        jc_extrude(
            plane,
            segments.as_ptr(),
            loop_lens.as_ptr(),
            loop_lens.len(),
            from,
            to,
            &mut out,
        )
    };
    take(code, out)
}

/// 壁を傾けた押し出し(`drafts` はループごとの SMLib の角度、度。0 でまっすぐ、正は内へ、負は外へ)。
/// `holes` はループごとの穴の印(0 でなければ引く)。
pub fn extrude_taper(
    plane: &JcPlane,
    segments: &[JcSegment],
    loop_lens: &[u32],
    height: f64,
    drafts: &[f64],
    holes: &[i32],
) -> Result<Brep, String> {
    debug_assert_eq!(loop_lens.iter().sum::<u32>() as usize, segments.len());
    assert_eq!(drafts.len(), loop_lens.len());
    assert_eq!(holes.len(), loop_lens.len());
    let mut out = std::ptr::null_mut();
    // SAFETY: 配列の長さは呼び出し側で揃えてある(角度はループの数だけ)
    let code = unsafe {
        jc_extrude_taper(
            plane,
            segments.as_ptr(),
            loop_lens.as_ptr(),
            loop_lens.len(),
            height,
            drafts.as_ptr(),
            holes.as_ptr(),
            &mut out,
        )
    };
    take(code, out)
}

pub fn planar_sheet(plane: &JcPlane, segments: &[JcSegment], lens: &[u32]) -> Result<Brep, String> {
    let mut out = std::ptr::null_mut();
    let code = unsafe {
        jc_planar_sheet(
            plane,
            segments.as_ptr(),
            lens.as_ptr(),
            lens.len(),
            &mut out,
        )
    };
    take(code, out)
}

pub fn patch_sheet(boundary: &[JcBoundary], planar: bool) -> Result<Brep, String> {
    let mut out = std::ptr::null_mut();
    let code = unsafe {
        jc_patch_sheet(
            boundary.as_ptr(),
            boundary.len(),
            i32::from(planar),
            &mut out,
        )
    };
    take(code, out)
}

/// 曲面 1 枚の中身(制御点は u ごとに v の並びで、x, y, z, 重み)。
pub struct Patch {
    pub degree_u: u32,
    pub degree_v: u32,
    pub knots_u: Vec<f64>,
    pub knots_v: Vec<f64>,
    pub points: Vec<[f64; 4]>,
    pub tag: i64,
    /// 1: u の最小の境目を始まりの端の面に、2: 最大の境目を終わりの端の面に使う
    pub caps: i32,
}

/// 曲面の網を縫って閉じた立体にする(jc_patch_solid)。
pub fn patch_solid(patches: &[Patch], start_tag: i64, end_tag: i64) -> Result<Brep, String> {
    let raw: Vec<JcPatch> = patches
        .iter()
        .map(|p| {
            let count_u = (p.knots_u.len() - p.degree_u as usize - 1) as u32;
            let count_v = (p.knots_v.len() - p.degree_v as usize - 1) as u32;
            debug_assert_eq!((count_u * count_v) as usize, p.points.len());
            JcPatch {
                degree_u: p.degree_u,
                degree_v: p.degree_v,
                count_u,
                count_v,
                knots_u: p.knots_u.as_ptr(),
                knots_v: p.knots_v.as_ptr(),
                points: p.points.as_ptr().cast(),
                tag: p.tag,
                caps: p.caps,
            }
        })
        .collect();
    let mut out = std::ptr::null_mut();
    // SAFETY: 各曲面の配列は patches が持ち、この呼び出しの間は生きている
    let code = unsafe { jc_patch_solid(raw.as_ptr(), raw.len(), start_tag, end_tag, &mut out) };
    take(code, out)
}

/// STEP の面の曲面(`frame` は原点・x・y・z の軸)。
pub enum StepSurface {
    Plane,
    Cylinder(f64),
    /// 原点(底)の半径、上の半径、高さ(有限の円すい)
    Cone(f64, f64, f64),
    Sphere(f64),
    Torus(f64, f64),
    BSpline(Patch),
}

/// 辺の曲線(制御点は x, y, z, 重み)。
pub struct Curve {
    pub degree: u32,
    pub knots: Vec<f64>,
    pub points: Vec<[f64; 4]>,
}

/// STEP の面 1 枚(輪の曲線は輪を回る向き、1 本目の輪が外)。
pub struct StepFace {
    pub surface: StepSurface,
    pub frame: [f64; 12],
    pub same_sense: bool,
    pub loops: Vec<Vec<Curve>>,
    pub points: Vec<[f64; 3]>,
    pub tag: i64,
}

/// STEP の面を切り抜いて縫い、閉じた立体にする(jc_step_solid)。失敗した面があればその添字も返す。
pub fn step_solid(faces: &[StepFace], tolerance: f64) -> Result<Brep, (Option<usize>, String)> {
    with_step_faces(faces, |raw| {
        let mut out = std::ptr::null_mut();
        let mut failed = -1i32;
        // SAFETY: 面・輪・曲線の配列は faces と上の Vec が持ち、この呼び出しの間は生きている
        let code =
            unsafe { jc_step_solid(raw.as_ptr(), raw.len(), tolerance, &mut failed, &mut out) };
        take(code, out).map_err(|e| ((failed >= 0).then_some(failed as usize), e))
    })
}

fn with_step_faces<T>(faces: &[StepFace], f: impl FnOnce(&[JcStepFace]) -> T) -> T {
    let empty = Patch {
        degree_u: 0,
        degree_v: 0,
        knots_u: Vec::new(),
        knots_v: Vec::new(),
        points: Vec::new(),
        tag: 0,
        caps: 0,
    };
    let sizes: Vec<Vec<u32>> = faces
        .iter()
        .map(|f| f.loops.iter().map(|l| l.len() as u32).collect())
        .collect();
    let curves: Vec<Vec<JcCurve>> = faces
        .iter()
        .map(|f| {
            f.loops
                .iter()
                .flatten()
                .map(|c| JcCurve {
                    degree: c.degree,
                    count: c.points.len() as u32,
                    knots: c.knots.as_ptr(),
                    points: c.points.as_ptr().cast(),
                })
                .collect()
        })
        .collect();
    let raw: Vec<JcStepFace> = faces
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let (kind, r1, r2, h, patch) = match &f.surface {
                StepSurface::Plane => (0, 0.0, 0.0, 0.0, &empty),
                StepSurface::Cylinder(r) => (1, *r, 0.0, 0.0, &empty),
                StepSurface::Cone(a, b, h) => (2, *a, *b, *h, &empty),
                StepSurface::Sphere(r) => (3, *r, 0.0, 0.0, &empty),
                StepSurface::Torus(a, b) => (4, *a, *b, 0.0, &empty),
                StepSurface::BSpline(p) => (5, 0.0, 0.0, 0.0, p),
            };
            let count_u = p_count(&patch.knots_u, patch.degree_u);
            let count_v = p_count(&patch.knots_v, patch.degree_v);
            JcStepFace {
                kind,
                frame: f.frame,
                r1,
                r2,
                h,
                patch: JcPatch {
                    degree_u: patch.degree_u,
                    degree_v: patch.degree_v,
                    count_u,
                    count_v,
                    knots_u: patch.knots_u.as_ptr(),
                    knots_v: patch.knots_v.as_ptr(),
                    points: patch.points.as_ptr().cast(),
                    tag: 0,
                    caps: 0,
                },
                same_sense: f.same_sense as i32,
                loop_count: sizes[i].len() as u32,
                loop_sizes: sizes[i].as_ptr(),
                curves: curves[i].as_ptr(),
                point_count: f.points.len() as u32,
                points: f.points.as_ptr().cast(),
                tag: f.tag,
            }
        })
        .collect();
    f(&raw)
}

/// IGES の面を1枚ずつ作り、失敗を面ごとに報告できるようにする。
pub fn iges_face(face: &StepFace, uv: bool, tolerance: f64) -> Result<Brep, String> {
    with_step_faces(std::slice::from_ref(face), |raw| {
        let mut out = std::ptr::null_mut();
        // SAFETY: 曲面・曲線の配列は with_step_faces と face が呼び出し中保持する。
        let code = unsafe { jc_iges_face(raw.as_ptr(), i32::from(uv), tolerance, &mut out) };
        take(code, out)
    })
}

/// 全面を複製して縫う。入力の面は変更しない。
pub fn iges_stitch(faces: &[Brep], tolerance: f64, mode: i32) -> Result<Brep, String> {
    let raw: Vec<_> = faces.iter().map(Brep::raw).collect();
    let mut out = std::ptr::null_mut();
    // SAFETY: 入力の B-rep とポインタの配列は呼び出し中保持し、出力は独立に所有する。
    let code = unsafe { jc_iges_stitch(raw.as_ptr(), raw.len(), tolerance, mode, &mut out) };
    take(code, out)
}

fn p_count(knots: &[f64], degree: u32) -> u32 {
    (knots.len() as u32).saturating_sub(degree + 1)
}

pub fn revolve(
    plane: &JcPlane,
    segments: &[JcSegment],
    loop_lens: &[u32],
    axis_origin: [f64; 3],
    axis_dir: [f64; 3],
    angle: f64,
) -> Result<Brep, String> {
    debug_assert_eq!(loop_lens.iter().sum::<u32>() as usize, segments.len());
    let mut out = std::ptr::null_mut();
    // SAFETY: 同上
    let code = unsafe {
        jc_revolve(
            plane,
            segments.as_ptr(),
            loop_lens.as_ptr(),
            loop_lens.len(),
            axis_origin.as_ptr(),
            axis_dir.as_ptr(),
            angle,
            &mut out,
        )
    };
    take(code, out)
}

impl Brep {
    pub fn raw(&self) -> *const JcBrep {
        self.0
    }

    pub fn is_sheet(&self) -> Result<bool, String> {
        let mut sheet = 0;
        let code = unsafe { jc_is_sheet(self.0, &mut sheet) };
        status(code)?;
        Ok(sheet != 0)
    }

    pub fn copy(&self) -> Result<Brep, String> {
        let mut out = std::ptr::null_mut();
        // SAFETY: self.0 は生きている形状
        let code = unsafe { jc_brep_copy(self.0, &mut out) };
        take(code, out)
    }

    /// `self` をその場で書き換える。`tool` は消費される。失敗したときは `self` が
    /// 壊れていることがあるので、捨てること。
    pub fn boolean(&mut self, tool: Brep, op: i32) -> Result<(), String> {
        let t = tool.0;
        std::mem::forget(tool);
        // SAFETY: tool の持ち主は中継層へ移る(成功・失敗にかかわらず中継層が消す)
        status(unsafe { jc_boolean(self.0, t, op) })
    }

    pub fn transform(&mut self, m: &[[f64; 4]; 4]) -> Result<(), String> {
        let flat: Vec<f64> = m.iter().flatten().copied().collect();
        // SAFETY: 16 個の値
        status(unsafe { jc_transform(self.0, flat.as_ptr()) })
    }

    /// シェル(`self` は変えない)。`open` は取り除く面の添字、`offset` は SMLib の向きの厚み。
    /// 平らな面 `faces`(添字、同じ平面)の縁をそのまま、面の外向きの法線へ `from`〜`to` だけ押し出す。
    pub fn extrude_faces(&self, faces: &[usize], from: f64, to: f64) -> Result<Brep, String> {
        let mut out = std::ptr::null_mut();
        // SAFETY: self.0 は生きている形状、faces は faces.len() 個の添字
        let code =
            unsafe { jc_extrude_faces(self.0, faces.as_ptr(), faces.len(), from, to, &mut out) };
        take(code, out)
    }

    pub fn shell(&self, open: &[usize], offset: f64) -> Result<Brep, String> {
        let mut out = std::ptr::null_mut();
        // SAFETY: self.0 は生きている形状、open は open.len() 個の添字
        let code = unsafe { jc_shell(self.0, open.as_ptr(), open.len(), offset, &mut out) };
        take(code, out)
    }

    /// 丸めの面 `faces`(面の添字)を消し、両側の面を延ばして鋭い辺に戻した形(元は変えない)。
    pub fn delete_fillet_faces(&self, faces: &[usize]) -> Result<Brep, String> {
        let mut out = std::ptr::null_mut();
        // SAFETY: self.0 は生きている形状、faces は faces.len() 個の添字
        let code = unsafe { jc_delete_fillet_faces(self.0, faces.as_ptr(), faces.len(), &mut out) };
        take(code, out)
    }

    /// [`Brep::shell`] の SMLib の指定(`flags`: 1 = 凸の辺を延ばす、2 = 自己交差を扱う)を選べる版。
    pub fn shell_with(&self, open: &[usize], offset: f64, flags: i32) -> Result<Brep, String> {
        let mut out = std::ptr::null_mut();
        // SAFETY: self.0 は生きている形状、open は open.len() 個の添字
        let code =
            unsafe { jc_shell_with(self.0, open.as_ptr(), open.len(), offset, flags, &mut out) };
        take(code, out)
    }

    /// 面の一覧。`flags` は [`FACE_AREA`] と [`FACE_INNER_POINT`] の組み合わせ。
    pub fn faces(&self, flags: i32) -> Result<Vec<JcFace>, String> {
        let mut n = 0usize;
        // SAFETY: 面の数を数えてから、その大きさの配列に書かせる
        status(unsafe { jc_face_count(self.0, &mut n) })?;
        let mut v = vec![JcFace::default(); n];
        status(unsafe { jc_faces(self.0, flags, v.as_mut_ptr(), n) })?;
        Ok(v)
    }

    pub fn set_tags(&mut self, tags: &[i64]) -> Result<(), String> {
        // SAFETY: 面の数だけのタグ(中継層が数を確かめる)
        status(unsafe { jc_set_face_tags(self.0, tags.as_ptr(), tags.len()) })
    }

    pub fn edges(&self) -> Result<Vec<JcEdge>, String> {
        let mut p = std::ptr::null_mut();
        let mut n = 0usize;
        // SAFETY: 中継層が malloc した配列を写してから free する
        status(unsafe { jc_edges(self.0, &mut p, &mut n) })?;
        if p.is_null() {
            return Ok(vec![]);
        }
        let v = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
        unsafe { jc_free_buffer(p.cast()) };
        Ok(v)
    }

    /// 面 `face`(`faces` の添字)から点 `p` までの距離。
    pub fn face_distance(&self, face: usize, p: [f64; 3]) -> Result<f64, String> {
        let mut d = 0.0;
        // SAFETY: 3 要素の配列を読み、出力へ書くだけ
        status(unsafe { jc_face_distance(self.0, face, p.as_ptr(), &mut d) })?;
        Ok(d)
    }

    /// 2 つの面の境目の辺すべて。
    pub fn all_edges(&self) -> Result<Vec<AnyEdge>, String> {
        let (mut p, mut n) = (std::ptr::null_mut(), 0usize);
        let (mut q, mut m) = (std::ptr::null_mut(), 0usize);
        // SAFETY: 中継層が malloc した 2 つの配列を写してから free する
        status(unsafe { jc_all_edges(self.0, &mut p, &mut n, &mut q, &mut m) })?;
        if p.is_null() {
            return Ok(vec![]);
        }
        let edges = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
        let pts = unsafe { std::slice::from_raw_parts(q, m * 3) }.to_vec();
        unsafe {
            jc_free_buffer(p.cast());
            jc_free_buffer(q.cast());
        }
        Ok(edges
            .iter()
            .map(|e| AnyEdge {
                faces: [e.f0 as usize, e.f1 as usize],
                convex: e.convex != 0,
                smooth: match e.smooth {
                    1 => Some(true),
                    0 => Some(false),
                    _ => None,
                },
                points: (e.first..e.first + e.count)
                    .map(|i| {
                        let i = i as usize * 3;
                        [pts[i], pts[i + 1], pts[i + 2]]
                    })
                    .collect(),
            })
            .collect())
    }

    /// 辺を丸める・面取りする(その場で変える。失敗すると形が壊れることがあるので、複製に掛ける)。
    /// `radii` は途中で変わる半径の (位置, 半径) の組を平たく並べたもの([`JcBlend::first`])。
    pub fn blend_edges(&mut self, blends: &[JcBlend], radii: &[f64]) -> Result<(), String> {
        // SAFETY: 配列を読むだけ。形状は中継層がその場で変える
        status(unsafe { jc_blend_edges(self.0, blends.as_ptr(), blends.len(), radii.as_ptr()) })
    }

    pub fn arc_edges(&self) -> Result<Vec<JcArcEdge>, String> {
        let mut p = std::ptr::null_mut();
        let mut n = 0usize;
        // SAFETY: 中継層が malloc した配列を写してから free する
        status(unsafe { jc_arc_edges(self.0, &mut p, &mut n) })?;
        if p.is_null() {
            return Ok(vec![]);
        }
        let v = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
        unsafe { jc_free_buffer(p.cast()) };
        Ok(v)
    }

    pub fn mass(&self, relative_accuracy: f64) -> Result<JcMassProps, String> {
        let mut m = JcMassProps::default();
        // SAFETY: 出力の構造体へ書くだけ
        status(unsafe { jc_mass(self.0, relative_accuracy, &mut m) })?;
        Ok(m)
    }

    pub fn volume(&self, relative_accuracy: f64) -> Result<f64, String> {
        let mut v = 0.0;
        // SAFETY: 出力の数へ書くだけ
        status(unsafe { jc_volume(self.0, relative_accuracy, &mut v) })?;
        Ok(v)
    }

    pub fn bounds(&self) -> Result<([f64; 3], [f64; 3]), String> {
        let (mut lo, mut hi) = ([0.0; 3], [0.0; 3]);
        // SAFETY: 3 要素の配列へ書くだけ
        status(unsafe { jc_bounds(self.0, lo.as_mut_ptr(), hi.as_mut_ptr()) })?;
        Ok((lo, hi))
    }

    /// (閉じた正しい立体か, 立体の数, 空洞の数)
    pub fn check(&self) -> Result<(bool, i64, i64), String> {
        let (mut m, mut s, mut v) = (0i32, 0i64, 0i64);
        // SAFETY: 出力へ書くだけ
        status(unsafe { jc_check(self.0, &mut m, &mut s, &mut v) })?;
        Ok((m != 0, s, v))
    }

    pub fn mesh(&self, chord: f64, angle_deg: f64) -> Result<MeshData, String> {
        let mut p = std::ptr::null_mut();
        // SAFETY: 中継層が作った網を写してから消す
        status(unsafe { jc_mesh(self.0, chord, angle_deg, &mut p) })?;
        if p.is_null() {
            return Err("SMLib が網を返しませんでした".into());
        }
        let m = unsafe { &*p };
        let slice_f =
            |ptr: *mut f32, n: usize| unsafe { std::slice::from_raw_parts(ptr, n) }.to_vec();
        let slice_u =
            |ptr: *mut u32, n: usize| unsafe { std::slice::from_raw_parts(ptr, n) }.to_vec();
        let edge_counts = slice_u(m.edge_counts, m.edge_count);
        let total: usize = edge_counts.iter().map(|&c| c as usize).sum();
        let out = MeshData {
            positions: slice_f(m.positions, m.vertex_count * 3),
            normals: slice_f(m.normals, m.vertex_count * 3),
            triangles: slice_u(m.triangles, m.triangle_count * 3),
            tri_face: slice_u(m.tri_face, m.triangle_count),
            edge_counts,
            edge_points: slice_f(m.edge_points, total * 3),
        };
        unsafe { jc_mesh_free(p) };
        Ok(out)
    }
}

/// 中継層の網を写したもの。
pub struct MeshData {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub triangles: Vec<u32>,
    pub tri_face: Vec<u32>,
    pub edge_counts: Vec<u32>,
    pub edge_points: Vec<f32>,
}

/// 形状を STEP(AP214、mm)の文字列にする。`names` は立体ごとの名前。
pub fn export_step(breps: &[&Brep], names: &[&str], product: &str) -> Result<String, String> {
    // NUL を含む名前は空にする(ASCII 以外の字は中継層が STEP の書き方 `\X2\` にする)
    let cstr = |s: &str| std::ffi::CString::new(s).unwrap_or_default();
    let names: Vec<std::ffi::CString> = names.iter().map(|n| cstr(n)).collect();
    let name_ptrs: Vec<*const c_char> = names.iter().map(|n| n.as_ptr()).collect();
    let ptrs: Vec<*const JcBrep> = breps.iter().map(|b| b.0 as *const JcBrep).collect();
    let product = cstr(product);
    let mut out: *mut c_char = std::ptr::null_mut();
    // SAFETY: 配列の長さは揃えてあり、文字列は呼び出しの間だけ生きていればよい
    let code = unsafe {
        jc_export_step(
            ptrs.as_ptr(),
            if name_ptrs.len() == ptrs.len() {
                name_ptrs.as_ptr()
            } else {
                std::ptr::null()
            },
            ptrs.len(),
            product.as_ptr(),
            &mut out,
        )
    };
    status(code)?;
    if out.is_null() {
        return Err("SMLib が STEP を返しませんでした".into());
    }
    // SAFETY: 中継層が malloc した NUL 終端の文字列を写してから消す
    let text = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { jc_free_buffer(out.cast()) };
    Ok(text)
}
