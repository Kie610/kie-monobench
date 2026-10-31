//! IGES の面と境界。読めない面を数え、読めた面を独立に組み立てられる形へ直す。
mod geometry;
mod topology;
use crate::step_in::{Nurbs, Surf};
use geometry::{Surface, checked, moved};
use kmb_core::kernel::{self as k, P3};
use kmb_format::iges::{self, Entry, File, Params};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

struct Face {
    pub surface: Arc<Surface>,
    /// 曲面の局所座標。uv の時は曲面のパラメータ。
    pub loops: Vec<Vec<Nurbs>>,
    pub points: Vec<P3>,
    pub uv: bool,
    pub same_sense: bool,
    pub place: k::Matrix4,
    pub name: String,
}
struct Model {
    pub faces: Vec<Face>,
    pub warnings: Vec<String>,
    pub tolerance: f64,
}
struct Boundary {
    curves: Vec<Nurbs>,
    points: Vec<P3>,
    uv: bool,
}
struct Data {
    file: File,
    work: Cell<usize>,
    controls: Cell<usize>,
    used: RefCell<BTreeSet<u32>>,
    stack: RefCell<Vec<u32>>,
    surfaces: RefCell<BTreeMap<u32, Arc<Surface>>>,
    matrices: RefCell<BTreeMap<(u32, bool), k::Matrix4>>,
    vertices: RefCell<BTreeMap<u32, Arc<Vec<P3>>>>,
}
struct Pop<'a>(&'a RefCell<Vec<u32>>);
impl Drop for Pop<'_> {
    fn drop(&mut self) {
        self.0.borrow_mut().pop();
    }
}
impl Data {
    fn charge(&self, controls: usize) -> Result<(), String> {
        let total = self
            .controls
            .get()
            .checked_add(controls)
            .filter(|n| *n <= 1_000_000)
            .ok_or("展開した曲線・曲面の制御点が100万点を超えます。面を分けて保存してください")?;
        self.controls.set(total);
        Ok(())
    }
    fn with<T>(&self, id: u32, f: impl FnOnce(&Entry) -> Result<T, String>) -> Result<T, String> {
        let work = self
            .work
            .get()
            .checked_add(1)
            .filter(|n| *n <= 1_000_000)
            .ok_or("要素の参照の展開が100万回を超えます。繰り返しを減らして保存してください")?;
        self.work.set(work);
        {
            let mut stack = self.stack.borrow_mut();
            if stack.len() >= 64 || stack.contains(&id) {
                return Err(format!(
                    "要素 {id} の参照が循環しているか、64段を超えています"
                ));
            }
            stack.push(id);
        }
        let _guard = Pop(&self.stack);
        self.used.borrow_mut().insert(id);
        let entry = self
            .file
            .entries
            .get(&id)
            .ok_or_else(|| format!("参照先の要素 {id} がありません"))?;
        f(entry).map_err(|e| format!("要素 {id}(種類 {}): {e}", entry.kind))
    }
    fn matrix(&self, id: u32, parameter: bool) -> Result<k::Matrix4, String> {
        if id == 0 {
            return Ok(k::IDENTITY);
        }
        if let Some(matrix) = self.matrices.borrow().get(&(id, parameter)).copied() {
            return Ok(matrix);
        }
        let matrix = self.with(id, |e| {
            if e.kind != 124 {
                return Err("変換の要素124を参照してください".into());
            }
            let mut p = Params::new(e);
            let mut m = k::IDENTITY;
            for row in &mut m[..3] {
                for value in row.iter_mut() {
                    *value = p.number()?;
                }
                if !parameter {
                    row[3] *= self.file.global.mm;
                }
            }
            let m = k::mat_mul(&self.matrix(e.transform, parameter)?, &m);
            validate_matrix(&m)?;
            Ok(m)
        })?;
        self.matrices.borrow_mut().insert((id, parameter), matrix);
        Ok(matrix)
    }
    fn mark_curve(&self, id: u32, depth: usize) {
        if id == 0 || depth >= 64 {
            return;
        }
        if !self.used.borrow_mut().insert(id) {
            return;
        }
        if let Some(e) = self.file.entries.get(&id)
            && e.kind == 102
        {
            let mut p = Params::new(e);
            if let Ok(n) = p.count(100_000) {
                for _ in 0..n {
                    if let Ok(child) = p.integer() {
                        self.mark_curve(child, depth + 1);
                    }
                }
            }
        }
    }
    fn boundary(&self, id: u32, surface: u32) -> Result<Boundary, String> {
        if self
            .file
            .entries
            .get(&id)
            .is_some_and(|e| matches!(e.kind, 100 | 102 | 110 | 126))
        {
            return Ok(Boundary {
                curves: self.curve(id, false)?,
                points: Vec::new(),
                uv: false,
            });
        }
        self.with(id,|e|{

            let mut p=Params::new(e);

            let (mut curves,uv)=match e.kind {

                142=>{

                    p.integer()?;
if p.integer()?!=surface{
return Err("境が別の曲面を指しています".into());
}

                    let parameter=p.integer()?;
let model=p.integer()?;
p.integer()?;

                    if model!=0{
self.mark_curve(parameter,0);
(self.curve(model,false)?,false)}

                    else if parameter!=0{
(self.curve(parameter,true)?,true)}
else{
return Err("境の曲線がありません".into());
}

                }

                141=>{

                    p.integer()?;
p.integer()?;
if p.integer()?!=surface{
return Err("境が別の曲面を指しています".into());
}

                    let count=p.count(100_000)?;
let mut curves=Vec::new();
let mut mode=None;

                    for _ in 0..count{

                        let model=p.integer()?;
let sense=p.integer()?;
let n=p.count(100_000)?;

                        let mut parameter=Vec::with_capacity(n);
for _ in 0..n{
parameter.push(p.integer()?);
}

                        let (mut part,uv)=if model!=0{
for &id in &parameter{
self.mark_curve(id,0);
}
(self.curve(model,false)?,false)}
else{

                            let mut part=Vec::new();
for id in parameter{
part.extend(self.curve(id,true)?);
}
 (part,true)
                        }
;

                        if mode.is_some_and(|m|m!=uv){
return Err("1つの境に3Dとパラメータの曲線が混在しています。3Dの境で保存してください".into());
}
mode=Some(uv);

                        match sense{
1=>{
}
,2=>{
part.reverse();
part=part.iter().map(crate::step_in::reverse).collect();
}
,_=>return Err("境の向きの印が不正です".into())}

                        curves.extend(part);

                    }
(curves,mode.unwrap_or(false))
                }

                100|102|110|126=>(self.curve(id,false)?,false),
                kind=>return Err(format!("境の種類 {kind} には対応していません")),
            }
;

            let matrix=self.matrix(e.transform,uv)?;

            // 曲線そのものの要素は curve が変換を適用する。
            if matches!(e.kind,141|142){
curves=curves.iter().map(|c|moved(c,&matrix)).collect::<Result<_,_>>()?;
}

            Ok(Boundary{
curves,points:Vec::new(),uv}
)
        }
)
    }
    fn finish_face(
        &self,
        surface: Arc<Surface>,
        mut boundaries: Vec<Boundary>,
        same_sense: bool,
        parent: k::Matrix4,
        name: String,
    ) -> Result<Face, String> {
        let surface_controls = match &surface.geo {
            Surf::BSpline(s) => s.pts.len(),
            _ => 0,
        };
        self.charge(surface_controls)?;
        let uv = boundaries.first().is_some_and(|b| b.uv);
        if boundaries.iter().any(|b| b.uv != uv) {
            return Err(
                "面の境に3Dとパラメータの曲線が混在しています。3Dの境で保存してください".into(),
            );
        }
        let mut uv = uv;
        if uv {
            if boundaries
                .iter()
                .flat_map(|b| &b.curves)
                .flat_map(|c| &c.pts)
                .any(|p| p[2].abs() > 1e-9)
            {
                return Err("面のパラメータの線は2次元で指定してください".into());
            }
            if !surface.parameterized {
                return Err("パラメータが定義されていない曲面にパラメータの境があります".into());
            }
            if matches!(&surface.geo, Surf::Plane(_)) {
                for b in &mut boundaries {
                    b.curves = b
                        .curves
                        .iter()
                        .map(|c| surface.lift_plane(c, self.file.global.mm))
                        .collect::<Result<_, _>>()?;
                    b.uv = false;
                }
                uv = false;
            } else if !matches!(&surface.geo, Surf::BSpline(_)) {
                return Err("この解析曲面のパラメータの境だけはまだ読めません。3Dの境を付けて保存してください".into());
            }
        }
        let inverse = inverse(&surface.place)?;
        let mut loops = Vec::new();
        let mut points = Vec::new();
        for boundary in boundaries {
            if !boundary.curves.is_empty() {
                check_loop(
                    &boundary.curves,
                    if uv {
                        1e-8
                    } else {
                        self.file.global.tolerance_mm.max(1e-7)
                    },
                )?;
                loops.push(if uv {
                    boundary.curves
                } else {
                    boundary
                        .curves
                        .iter()
                        .map(|c| moved(c, &inverse))
                        .collect::<Result<_, _>>()?
                });
            }
            for point in boundary.points {
                points.push(if uv {
                    point
                } else {
                    checked(k::transform_point(&inverse, point))?
                });
            }
        }
        if loops.is_empty()
            && points.is_empty()
            && !matches!(&surface.geo, Surf::Sphere(_, _) | Surf::Torus(_, _, _))
        {
            return Err("面の境に有効な輪がありません。境を閉じて保存してください".into());
        }
        let place = k::mat_mul(&parent, &surface.place);
        validate_matrix(&place)?;
        Ok(Face {
            surface,
            loops,
            points,
            uv,
            same_sense,
            place,
            name,
        })
    }
    fn face(
        &self,
        id: u32,
        same_sense: bool,
        parent: k::Matrix4,
        name: String,
    ) -> Result<Face, String> {
        let entry = self
            .file
            .entries
            .get(&id)
            .ok_or_else(|| format!("面の要素 {id} がありません"))?;
        if matches!(entry.kind, 108 | 128 | 190 | 192 | 194 | 196 | 198) {
            let surface = self.surface(id)?;
            let boundaries = if surface.boundary != 0 {
                vec![Boundary {
                    curves: self
                        .curve(surface.boundary, false)?
                        .iter()
                        .map(|c| moved(c, &surface.place))
                        .collect::<Result<_, _>>()?,
                    points: Vec::new(),
                    uv: false,
                }]
            } else if matches!(&surface.geo, Surf::Sphere(_, _) | Surf::Torus(_, _, _)) {
                Vec::new()
            } else {
                vec![Boundary {
                    curves: surface.natural(false)?,
                    points: Vec::new(),
                    uv: false,
                }]
            };
            return self.finish_face(surface, boundaries, same_sense, parent, name);
        }
        self.with(id, |e| {
            let mut p = Params::new(e);
            let (surface, boundaries) = match e.kind {
                144 => {
                    let source = p.integer()?;
                    let outer = p.integer()?;
                    let count = p.count(100_000)?;
                    let boundary = p.integer()?;
                    let surface = self.surface(source)?;
                    let mut inner = Vec::with_capacity(count);
                    for _ in 0..count {
                        inner.push(self.boundary(p.integer()?, source)?);
                    }
                    let first = match outer {
                        0 => {
                            let uv = inner.first().is_some_and(|b| b.uv);
                            let curves = if surface.boundary != 0 && !uv {
                                self.curve(surface.boundary, false)?
                                    .iter()
                                    .map(|c| moved(c, &surface.place))
                                    .collect::<Result<_, _>>()?
                            } else {
                                surface.natural(uv)?
                            };
                            Boundary {
                                curves,
                                points: Vec::new(),
                                uv,
                            }
                        }
                        1 => self.boundary(boundary, source)?,
                        _ => return Err("外側の境の印は0か1で指定してください".into()),
                    };
                    let mut boundaries = vec![first];
                    boundaries.extend(inner);
                    (surface, boundaries)
                }
                143 => {
                    p.flag()?;
                    let source = p.integer()?;
                    let n = p.count(100_000)?;
                    if n == 0 {
                        return Err("境のある面には1本以上の輪が必要です".into());
                    }
                    let mut loops = Vec::with_capacity(n);
                    for _ in 0..n {
                        loops.push(self.boundary(p.integer()?, source)?);
                    }
                    (self.surface(source)?, loops)
                }
                510 => {
                    let source = p.integer()?;
                    let count = p.count(100_000)?;
                    let outer = p.flag()?;
                    let mut boundaries = Vec::with_capacity(count);
                    for _ in 0..count {
                        boundaries.push(self.topology_loop(p.integer()?)?);
                    }
                    if !outer && !boundaries.is_empty() {
                        return Err("外側の輪が無い位相の面にはまだ対応していません".into());
                    }
                    let surface = self.surface(source)?;
                    if boundaries.is_empty()
                        && !matches!(&surface.geo, Surf::Sphere(_, _) | Surf::Torus(_, _, _))
                    {
                        return Err("面に境の輪がありません".into());
                    }
                    (surface, boundaries)
                }
                kind => return Err(format!("面の種類 {kind} には対応していません")),
            };
            self.finish_face(
                surface,
                boundaries,
                same_sense,
                k::mat_mul(&parent, &self.matrix(e.transform, false)?),
                name,
            )
        })
    }
}
fn validate_matrix(m: &k::Matrix4) -> Result<(), String> {
    if let Some(why) = k::placement_problem(m) {
        return Err(why.into());
    }
    if m.iter().flatten().any(|v| v.abs() > 1e30) {
        return Err("変換の倍率か距離が大きすぎます".into());
    }
    Ok(())
}
fn inverse(m: &k::Matrix4) -> Result<k::Matrix4, String> {
    validate_matrix(m)?;
    let mut a = [[0.; 6]; 3];
    for (i, row) in a.iter_mut().enumerate() {
        row[..3].copy_from_slice(&m[i][..3]);
        row[i + 3] = 1.;
    }
    for column in 0..3 {
        let pivot = (column..3)
            .max_by(|&x, &y| a[x][column].abs().total_cmp(&a[y][column].abs()))
            .ok_or("変換を逆にできません")?;
        a.swap(column, pivot);
        let d = a[column][column];
        if d == 0. || !d.is_finite() {
            return Err("変換を逆にできません".into());
        }
        for x in &mut a[column] {
            *x /= d;
        }
        let row = a[column];
        for (i, other) in a.iter_mut().enumerate() {
            if i != column {
                let k = other[column];
                for j in 0..6 {
                    other[j] -= k * row[j];
                }
            }
        }
    }
    let mut out = k::IDENTITY;
    for i in 0..3 {
        out[i][..3].copy_from_slice(&a[i][3..]);
        out[i][3] = -(0..3).map(|j| out[i][j] * m[j][3]).sum::<f64>();
    }
    validate_matrix(&out)?;
    Ok(out)
}
fn check_loop(curves: &[Nurbs], tolerance: f64) -> Result<(), String> {
    if curves.is_empty() {
        return Err("境の輪に曲線がありません".into());
    }
    for (i, curve) in curves.iter().enumerate() {
        let next = &curves[(i + 1) % curves.len()];
        let end = crate::step_in::eval(curve, curve.knots[curve.pts.len()]);
        let start = crate::step_in::eval(next, next.knots[next.degree]);
        let d = k::sub(end, start);
        let gap = d[0].hypot(d[1]).hypot(d[2]);
        if !gap.is_finite() || gap > tolerance {
            return Err(format!(
                "境の区間{}と{}の間に隙間 {gap} があります",
                i + 1,
                (i + 1) % curves.len() + 1
            ));
        }
    }
    Ok(())
}
fn known(kind: u32) -> bool {
    matches!(
        kind,
        100 | 102
            | 108
            | 110
            | 116
            | 123
            | 124
            | 126
            | 128
            | 141
            | 142
            | 143
            | 144
            | 186
            | 190
            | 192
            | 194
            | 196
            | 198
            | 502
            | 504
            | 508
            | 510
            | 514
    )
}
fn read(text: &str) -> Result<Model, String> {
    let data = Data {
        file: iges::read(text)?,
        work: Cell::new(0),
        controls: Cell::new(0),
        used: RefCell::new(BTreeSet::new()),
        stack: RefCell::new(Vec::new()),
        surfaces: RefCell::new(BTreeMap::new()),
        matrices: RefCell::new(BTreeMap::new()),
        vertices: RefCell::new(BTreeMap::new()),
    };
    let mut model = Model {
        faces: Vec::new(),
        warnings: Vec::new(),
        tolerance: data.file.global.tolerance_mm.max(1e-7),
    };
    if data.file.global.tolerance_mm < 1e-7 {
        model
            .warnings
            .push("ファイルの精度が細かすぎるので、0.0000001 mm の精度で面を組みます".into());
    }
    let shells: BTreeMap<_, _> = data
        .file
        .entries
        .values()
        .filter(|e| e.kind == 514)
        .enumerate()
        .map(|(i, e)| (e.id, i + 1))
        .collect();
    for e in data
        .file
        .entries
        .values()
        .filter(|e| e.kind == 186 && !e.physically_dependent)
    {
        let result = data.with(e.id, |e| {
            let mut p = Params::new(e);
            let outer = p.integer()?;
            let orientation = p.flag()?;
            let count = p.count(100_000)?;
            let place = data.matrix(e.transform, false)?;
            data.shell(outer, orientation, place, &shells, &mut model)?;
            for _ in 0..count {
                let id = p.integer()?;
                let orientation = p.flag()?;
                data.shell(id, orientation, place, &shells, &mut model)?;
            }
            Ok(())
        });
        if let Err(why) = result {
            model
                .warnings
                .push(format!("立体を読めませんでした: {why}"));
        }
    }
    for e in data
        .file
        .entries
        .values()
        .filter(|e| e.kind == 514 && !e.physically_dependent)
    {
        if !data.used.borrow().contains(&e.id)
            && let Err(why) = data.shell(e.id, true, k::IDENTITY, &shells, &mut model)
        {
            model.warnings.push(format!("殻を読めませんでした: {why}"));
        }
    }
    // 切り抜きの元の曲面を、切り抜いた面とは別の面として読み込まない。
    // 境の読み込みが失敗しても、元の無傷の曲面で穴を塞がない。
    let referenced_surfaces: BTreeSet<_> = data
        .file
        .entries
        .values()
        .filter_map(|e| {
            let mut p = Params::new(e);
            match e.kind {
                144 | 510 => p.integer().ok(),
                143 => {
                    p.integer().ok()?;
                    p.integer().ok()
                }
                _ => None,
            }
        })
        .collect();
    let mut number = 0;
    for e in data.file.entries.values().filter(|e| {
        matches!(
            e.kind,
            108 | 128 | 143 | 144 | 190 | 192 | 194 | 196 | 198 | 510
        ) && !e.physically_dependent
            && !referenced_surfaces.contains(&e.id)
    }) {
        if data.used.borrow().contains(&e.id) {
            continue;
        }
        number += 1;
        let name = format!("iges{}:f{number}", shells.len() + 1);
        match data.face(e.id, true, k::IDENTITY, name.clone()) {
            Ok(face) => model.faces.push(face),
            Err(why) => model
                .warnings
                .push(format!("面 {name} を読めませんでした: {why}")),
        }
    }
    let unused = data
        .file
        .entries
        .values()
        .filter(|e| matches!(e.kind, 100 | 102 | 110 | 126) && !data.used.borrow().contains(&e.id))
        .count();
    if unused > 0 {
        model.warnings.push(format!(
            "面に使わない線・曲線 {unused} 要素は読みませんでした"
        ));
    }
    let orphan = data
        .file
        .entries
        .values()
        .filter(|e| {
            e.physically_dependent
                && matches!(
                    e.kind,
                    108 | 128 | 143 | 144 | 190 | 192 | 194 | 196 | 198 | 510
                )
                && !data.used.borrow().contains(&e.id)
        })
        .count();
    if orphan > 0 {
        model.warnings.push(format!(
            "親を読めず位置を決められない面 {orphan} 要素は読みませんでした"
        ));
    }
    let mut unsupported = BTreeMap::new();
    for e in data.file.entries.values().filter(|e| !known(e.kind)) {
        *unsupported.entry(e.kind).or_insert(0usize) += 1;
    }
    for (kind, count) in unsupported {
        model.warnings.push(format!(
            "未対応の種類 {kind} を {count} 要素読みませんでした"
        ));
    }
    if model.faces.is_empty() {
        return Err(format!(
            "IGES に読み込める面・立体がありません。{}",
            model.warnings.join("。")
        ));
    }
    Ok(model)
}

/// 独立に作れた面を全部残して縫い、失敗と開いた縁を位置つきで報告する。
pub(super) fn import(
    kernel: &crate::SmlibKernel,
    text: &str,
) -> Result<(k::ShapeHandle, Vec<String>), String> {
    use crate::{ffi, names::points, step_face, step_in};
    let mut model = read(text)?;
    let _guard = ffi::lock();
    let mut faces = Vec::new();
    for (i, face) in model.faces.iter().enumerate() {
        let definition = step_in::StepFace {
            surface: face.surface.geo.clone(),
            loops: face.loops.clone(),
            points: face.points.clone(),
            same_sense: face.same_sense,
            name: face.name.clone(),
        };
        let result = ffi::iges_face(&step_face(&definition, i as i64), face.uv, model.tolerance)
            .and_then(|mut brep| {
                brep.transform(&face.place)?;
                Ok(brep)
            });
        match result {
            Ok(brep) => faces.push(brep),
            Err(why) => {
                let first = face
                    .loops
                    .first()
                    .and_then(|l| l.first())
                    .and_then(|c| c.pts.first());
                let location = if face.uv {
                    format!("境のパラメータ {:?}", first.map(|p| [p[0], p[1]]))
                } else {
                    format!(
                        "境の位置 {:?} mm",
                        first.map(|p| k::transform_point(&face.place, [p[0], p[1], p[2]]))
                    )
                };
                model.warnings.push(format!(
                    "面 {} を作れませんでした({location}): {why}",
                    face.name
                ));
            }
        }
    }
    if faces.is_empty() {
        return Err(format!(
            "面を1枚も作れませんでした。{}",
            model.warnings.join("。")
        ));
    }
    let expected = faces
        .iter()
        .map(|b| b.faces(0).map(|f| f.len()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum::<usize>();
    let candidate = ffi::iges_stitch(&faces, model.tolerance, 1);
    let brep = match candidate {
        Ok(brep) if brep.check()?.0 && brep.faces(0)?.len() == expected => brep,
        other => {
            if let Err(why) = other {
                model.warnings.push(format!(
                    "立体への縫い合わせに失敗しました: {why}。読めた面を残します"
                ));
            }
            match ffi::iges_stitch(&faces, model.tolerance, 0) {
                Ok(brep) if brep.faces(0)?.len() == expected => brep,
                other => {
                    let why = other
                        .err()
                        .unwrap_or_else(|| "縫い合わせで面の数が変わりました".into());
                    model.warnings.push(format!(
                        "面の縫い合わせを行えませんでした: {why}。独立した面を全部残します"
                    ));
                    ffi::iges_stitch(&faces, model.tolerance, -1)?
                }
            }
        }
    };
    let found = brep.faces(ffi::FACE_INNER_POINT)?;
    let names: Vec<_> = found
        .iter()
        .map(|f| {
            usize::try_from(f.tag)
                .ok()
                .and_then(|t| model.faces.get(t))
                .map(|f| f.name.clone())
                .ok_or_else(|| "縫った面の元を特定できません".to_string())
        })
        .collect::<Result<_, _>>()?;
    let names = k::dedupe_names(names, &points(&found));
    let all = brep.all_edges()?;
    let mut rims = vec![None; all.len()];
    let mut open: BTreeMap<usize, Vec<(usize, P3)>> = BTreeMap::new();
    for (i, e) in all.iter().enumerate() {
        if e.faces[1] == u32::MAX as usize {
            // 球の極など、1点に縮む境は開いた縁ではない。
            if let Some(first) = e.points.first()
                && e.points.iter().all(|p| {
                    let d = k::sub(*p, *first);
                    d[0].hypot(d[1]).hypot(d[2]) <= model.tolerance
                })
            {
                continue;
            }
            let at = *e
                .points
                .get(e.points.len() / 2)
                .ok_or("開いた縁の位置を求められません")?;
            open.entry(e.faces[0]).or_default().push((i, at));
        }
    }
    for (&face, edges) in &mut open {
        let name = names.get(face).ok_or("開いた縁の面がありません")?;
        edges.sort_by(|a, b| {
            a.1.iter()
                .zip(b.1)
                .map(|(a, b)| a.total_cmp(&b))
                .find(|c| !c.is_eq())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (ordinal, (i, _)) in edges.iter().enumerate() {
            rims[*i] = Some(format!("{name}:rim:{}", ordinal + 1));
        }
    }
    if !open.is_empty() {
        let locations = open
            .keys()
            .map(|&i| format!("{} {:?} mm", names[i], found[i].point))
            .collect::<Vec<_>>()
            .join("、");
        model.warnings.push(format!(
            "縫って閉じなかった面 {} 枚(開いた縁 {} 本): {locations}。面のボディとして残しました",
            open.len(),
            open.values().map(Vec::len).sum::<usize>()
        ));
    }
    log::info!(
        "IGES の面 {} 枚を組みました(注意 {} 件)",
        found.len(),
        model.warnings.len()
    );
    if open.is_empty() {
        rims.clear();
    }
    let mut store = kernel.lock();
    let handle = store.put(brep, names).map_err(|e| e.to_string())?;
    store
        .shapes
        .get_mut(&handle.0)
        .ok_or("形を保存できませんでした")?
        .rims = rims;
    Ok((handle, model.warnings))
}

#[cfg(test)]
mod tests;
