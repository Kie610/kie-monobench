use super::*;
use truck_geometry::prelude::IntersectionCurve;
use truck_meshalgo::prelude::*;
use truck_topology::*;

/// Only solids consisting of faces whose surface is implemented this trait can be used for set operations.
pub trait ShapeOpsSurface:
    ParametricSurface3D
    + ParameterDivision2D
    + SearchParameter<D2, Point = Point3>
    + SearchNearestParameter<D2, Point = Point3>
    + Invertible
    + Send
    + Sync {
}
impl<S> ShapeOpsSurface for S where S: ParametricSurface3D
        + ParameterDivision2D
        + SearchParameter<D2, Point = Point3>
        + SearchNearestParameter<D2, Point = Point3>
        + Invertible
        + Send
        + Sync
{
}

/// Only solids consisting of edges whose curve is implemented this trait can be used for set operations.
pub trait ShapeOpsCurve<S: ShapeOpsSurface>:
    ParametricCurve3D
    + ParameterDivision1D<Point = Point3>
    + Cut
    + Invertible
    + From<IntersectionCurve<PolylineCurve<Point3>, S>>
    + SearchParameter<D1, Point = Point3>
    + SearchNearestParameter<D1, Point = Point3>
    + Send
    + Sync {
}
impl<C, S: ShapeOpsSurface> ShapeOpsCurve<S> for C where C: ParametricCurve3D
        + ParameterDivision1D<Point = Point3>
        + Cut
        + Invertible
        + From<IntersectionCurve<PolylineCurve<Point3>, S>>
        + SearchParameter<D1, Point = Point3>
        + SearchNearestParameter<D1, Point = Point3>
        + Send
        + Sync
{
}

/// jisaku-cadcam: the region a closed shell bounds, for placing the faces that no intersection
/// divides (a whole shell that meets nothing of the other operand).
///
/// The meshes of a meshed shell follow each face's surface, not the face's orientation, so
/// counting the crossings by the meshes alone (as truck did) counted every reversed face (a
/// `not()`ed solid: a cut's tool, a body turned outward) the wrong way, and read a shell facing
/// inward (a void) as the solid inside it. The crossings are counted here as each face is
/// oriented, and a shell facing inward bounds what lies outside it.
struct ShellRegion<'a> {
    shell: &'a MeshedShell,
    inward: bool,
}

type MeshedShell = Shell<Point3, PolylineCurve<Point3>, Option<PolygonMesh>>;

impl<'a> ShellRegion<'a> {
    fn new(shell: &'a MeshedShell) -> Option<Self> {
        // six times the volume, each face counted as it is oriented
        let mut volume = 0.0;
        for face in shell.face_iter() {
            let poly = face.surface()?;
            let pos = poly.positions();
            let mut v = 0.0;
            for f in poly.face_iter() {
                let p0 = pos[f[0].pos].to_vec();
                for i in 2..f.len() {
                    v += p0.dot(pos[f[i - 1].pos].to_vec().cross(pos[f[i].pos].to_vec()));
                }
            }
            volume += if face.orientation() { v } else { -v };
        }
        Some(Self {
            shell,
            inward: volume < 0.0,
        })
    }

    fn contains(&self, pt: Point3) -> Option<bool> {
        let dir = hash::take_one_unit(pt);
        let mut count = 0;
        for face in self.shell.face_iter() {
            let c = face.surface()?.signed_crossing_faces(pt, dir);
            count += if face.orientation() { c } else { -c };
        }
        // outward: 1 inside, 0 outside; inward: 0 outside the void, -1 inside it
        Some(if self.inward { count >= 0 } else { count >= 1 })
    }
}

fn process_one_pair_of_shells<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
    shell0: &Shell<Point3, C, S>,
    shell1: &Shell<Point3, C, S>,
    tol: f64,
) -> Option<[Shell<Point3, C, S>; 2]> {
    nonpositive_tolerance!(tol);
    let poly_shell0 = shell0.triangulation(tol);
    let poly_shell1 = shell1.triangulation(tol);
    let loops_store::LoopsStoreQuadruple {
        geom_loops_store0: loops_store0,
        geom_loops_store1: loops_store1,
        ..
    } = loops_store::create_loops_stores(shell0, &poly_shell0, shell1, &poly_shell1, tol)?;
    let mut cls0 = divide_face::divide_faces(shell0, &loops_store0, tol)?;
    cls0.integrate_by_component();
    let mut cls1 = divide_face::divide_faces(shell1, &loops_store1, tol)?;
    cls1.integrate_by_component();
    let [mut and0, mut or0, unknown0] = cls0.and_or_unknown();
    let region1 = ShellRegion::new(&poly_shell1)?;
    unknown0.into_iter().try_for_each(|face| {
        let pt = face.boundaries()[0].vertex_iter().next().unwrap().point();
        if region1.contains(pt)? {
            and0.push(face);
        } else {
            or0.push(face);
        }
        Some(())
    })?;
    let [mut and1, mut or1, unknown1] = cls1.and_or_unknown();
    let region0 = ShellRegion::new(&poly_shell0)?;
    unknown1.into_iter().try_for_each(|face| {
        let pt = face.boundaries()[0].vertex_iter().next().unwrap().point();
        if region0.contains(pt)? {
            and1.push(face);
        } else {
            or1.push(face);
        }
        Some(())
    })?;
    and0.append(&mut and1);
    or0.append(&mut or1);
    Some([and0, or0])
}

/// AND operation between two solids.
pub fn and<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
    solid0: &Solid<Point3, C, S>,
    solid1: &Solid<Point3, C, S>,
    tol: f64,
) -> Option<Solid<Point3, C, S>> {
    let mut iter0 = solid0.boundaries().iter();
    let mut iter1 = solid1.boundaries().iter();
    let shell0 = iter0.next().unwrap();
    let shell1 = iter1.next().unwrap();
    let [mut and_shell, _] = process_one_pair_of_shells(shell0, shell1, tol)?;
    for shell in iter0 {
        let [res, _] = process_one_pair_of_shells(&and_shell, shell, tol)?;
        and_shell = res;
    }
    for shell in iter1 {
        let [res, _] = process_one_pair_of_shells(&and_shell, shell, tol)?;
        and_shell = res;
    }
    let boundaries = and_shell.connected_components();
    Some(Solid::new(boundaries))
}

/// OR operation between two solids.
pub fn or<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
    solid0: &Solid<Point3, C, S>,
    solid1: &Solid<Point3, C, S>,
    tol: f64,
) -> Option<Solid<Point3, C, S>> {
    let mut iter0 = solid0.boundaries().iter();
    let mut iter1 = solid1.boundaries().iter();
    let shell0 = iter0.next().unwrap();
    let shell1 = iter1.next().unwrap();
    let [_, mut or_shell] = process_one_pair_of_shells(shell0, shell1, tol)?;
    for shell in iter0 {
        let [_, res] = process_one_pair_of_shells(&or_shell, shell, tol)?;
        or_shell = res;
    }
    for shell in iter1 {
        let [_, res] = process_one_pair_of_shells(&or_shell, shell, tol)?;
        or_shell = res;
    }
    let boundaries = or_shell.connected_components();
    Some(Solid::new(boundaries))
}

#[cfg(test)]
mod tests;
