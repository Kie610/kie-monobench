//! 確かめること: 丸めた面と解析的な網の半径、当てはまらない面の扱い。

use super::*;
use serde_json::json;

fn patch(n: usize, m: usize, point: impl Fn(f64,f64)->P) -> Vec<(FeatureId,Mesh)> {
    let mut mesh=Mesh::default();mesh.face_names.push("surface".into());
    for j in 0..=m { for i in 0..=n {
        mesh.positions.push(point(i as f64/n as f64,j as f64/m as f64).map(|x|x as f32));
    }}
    for j in 0..m { for i in 0..n {
        let a=(j*(n+1)+i)as u32;let b=a+1;let d=a+(n+1)as u32;let c=d+1;
        mesh.triangles.extend([[a,b,c],[a,c,d]]);mesh.triangle_faces.extend([0,0]);
    }}
    vec![(FeatureId(1),mesh)]
}

fn resolved(scene: &Vec<(FeatureId,Mesh)>) -> Geo {
    resolve(scene,&Target::Face(Pick{feature:scene[0].0,face:scene[0].1.face_names[0].clone()})).unwrap()
}

fn radius_rows(g: &Geo, radius: f64) {
    let rows=describe(g);
    assert!(rows.iter().any(|(k,v)|k=="半径"&&*v==format!("{radius:.3} mm")),"{:?}: {rows:?}",g.kind);
    assert!(rows.iter().any(|(k,v)|k=="直径"&&*v==format!("{:.3} mm",2.0*radius)),"{rows:?}");
}

#[test]
fn curved_radius_cylindrical_fillet_patch_already_reports_its_radius() {
    let scene=patch(24,3,|u,v| {
        let a=u*std::f64::consts::FRAC_PI_2;
        [4.0*a.cos()+6.0,4.0*a.sin()+2.0,20.0*v]
    });
    let g=resolved(&scene);assert!(matches!(g.kind,Kind::Cylinder(_)));radius_rows(&g,4.0);
}

#[test]
fn curved_radius_spherical_corner_patch_reports_radius_and_diameter() {
    let scene=patch(24,16,|u,v| {
        let a=u*std::f64::consts::FRAC_PI_2;let b=0.08+v*1.45;
        [6.0+4.0*b.sin()*a.cos(),7.0+4.0*b.sin()*a.sin(),8.0+4.0*b.cos()]
    });
    radius_rows(&resolved(&scene),4.0);
}

#[test]
fn curved_radius_toroidal_rim_reports_the_tube_radius_and_diameter() {
    let scene=patch(64,12,|u,v| {
        let a=u*std::f64::consts::TAU;let b=v*std::f64::consts::FRAC_PI_2;
        let (s,c)=0.7_f64.sin_cos();let p=[(16.0+4.0*b.cos())*a.cos(),(16.0+4.0*b.cos())*a.sin(),4.0*b.sin()];
        [3.0+c*p[0]+s*p[2],-4.0+p[1],5.0-s*p[0]+c*p[2]]
    });
    radius_rows(&resolved(&scene),4.0);
}

#[test]
fn curved_radius_an_ellipsoid_remains_an_unclassified_surface() {
    let scene=patch(20,16,|u,v| {
        let a=u*std::f64::consts::FRAC_PI_2;let b=0.1+v*1.4;
        [5.0*b.sin()*a.cos(),3.0*b.sin()*a.sin(),2.0*b.cos()]
    });
    let g=resolved(&scene);assert!(matches!(g.kind,Kind::Surface{..}));
    assert_eq!(describe(&g).len(),1);assert_eq!(describe(&g)[0].0,"面積(網で)");
}

fn model(mut doc: kmb_core::Document)->Vec<(FeatureId,Mesh)> {
    let modules=kmb_bundle::modules();let report=doc.recompute(&modules.registry,modules.kernel.as_ref());
    assert_eq!(report.error_count(),0,"{:?}",report.features);
    let bodies=kmb_core::visible_bodies(doc.data(),&modules.registry,&report);
    meshes(modules.kernel.as_ref(),&bodies)
}

#[test]
fn curved_radius_three_rounded_box_edges_include_a_measurable_spherical_face() {
    let scene=model(kmb_samples::corner_document("solid.fillet@1","ball"));
    let (id,mesh)=scene.last().unwrap();
    let found:Vec<_>=mesh.face_names.iter().filter_map(|name|{
        let g=resolve(&scene,&Target::Face(Pick{feature:*id,face:name.clone()}))?;
        println!("face {name}: {:?}",g.kind);
        format!("{:?}",g.kind).starts_with("Sphere").then_some(g)
    }).collect();
    assert!(!found.is_empty(),"3本の辺の丸めの球面に半径が出る");
    for g in found {assert!(describe(&g).iter().any(|(k,_)|k=="半径"));}
}

#[test]
fn curved_radius_a_rounded_cylinder_rim_includes_a_measurable_toroidal_face() {
    let mut b=kmb_samples::sample::DocBuilder::new();
    let mut s=kmb_samples::sample::SketchBuilder::xy();let center=s.fixed_point(0.0,0.0);let circle=s.circle(center,"20 mm",10.0);
    let sketch=b.feature("cad.sketch@1","円の輪郭",s.build());
    let cylinder=b.feature("solid.extrude@1","円柱",json!({"profile":sketch,"distance":"20 mm","operation":"new","direction":"normal"}));
    b.feature("solid.fillet@1","縁の丸め",json!({"target":cylinder,"edges":[[format!("f{}:end",cylinder.0),format!("f{}:side:e{}",cylinder.0,circle.0)]],"radius":"2 mm"}));
    let scene=model(b.finish("丸い縁"));let(id,mesh)=scene.last().unwrap();
    let found:Vec<_>=mesh.face_names.iter().filter_map(|name|{
        let g=resolve(&scene,&Target::Face(Pick{feature:*id,face:name.clone()}))?;
        println!("face {name}: {:?}",g.kind);
        format!("{:?}",g.kind).starts_with("Torus").then_some(g)
    }).collect();
    assert!(!found.is_empty(),"円柱の縁の丸めの円環面に半径が出る");
    for g in found {assert!(describe(&g).iter().any(|(k,_)|k=="半径"));}
}
