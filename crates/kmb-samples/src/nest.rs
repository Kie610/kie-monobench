//! 板への配置の見本。部品と配置は文書のデータで、寸法を直すと作り直す。

use crate::sample::{DocBuilder,SketchBuilder};
use kmb_cam_commands::{Nest,NestMethod,NestPart,NestRotate,Origin,SETUP,Setup,StockMode,TOOLS,ToolLibrary};
use kmb_core::{Document,FeatureId};

fn extrude(b:&mut DocBuilder,name:&str,sketch:SketchBuilder)->FeatureId {
    let profile=b.sketch(&format!("{name}の輪郭"),sketch);
    b.feature(kmb_cad_extrude::EXTRUDE,name,kmb_cad_extrude::Extrude {
        profile:Some(profile),distance:"5 mm".into(),..Default::default()
    })
}

fn finish(mut b:DocBuilder,parts:Vec<NestPart>,size:[f64;2],method:NestMethod)->Document {
    let tools=b.feature(TOOLS,"工具",ToolLibrary::starter());
    let mut setup=Setup::new(parts[0].body);
    setup.stock.mode=StockMode::Fixed;
    setup.stock.width=format!("{} mm",size[0]);setup.stock.depth=format!("{} mm",size[1]);setup.stock.height="5 mm".into();
    setup.origin=Origin::FrontLeft;
    let mut nest=Nest::new(parts);nest.method=method;nest.edge="5 mm".into();nest.spacing="8 mm".into();
    setup.nest=Some(nest);
    let setup=b.feature(SETUP,"板への配置",setup);
    let contour:kmb_strategy_contour2d::Contour=serde_json::from_value(serde_json::json!({"setup":setup,"tools":tools,"tool":1})).expect("加工の既定");
    b.feature(kmb_strategy_contour2d::CONTOUR,"部品の外形",contour);
    b.finish("板への配置")
}

/// 大きさと個数の違う 5 種類の長方形。`two_sheets` なら小さい板を 2 枚使う。
pub fn nesting_boxes_document(two_sheets:bool)->Document {
    let mut b=DocBuilder::new();b.param("部品の幅","36 mm");
    let mut parts=Vec::new();
    for (i,(w,h,count)) in [(36.0,24.0,3),(28.0,20.0,2),(22.0,18.0,2),(18.0,14.0,3),(14.0,10.0,2)].into_iter().enumerate() {
        let mut s=SketchBuilder::xy();let corner=s.fixed_point(0.0,0.0);
        let width=if i==0 { "部品の幅".into() } else { format!("{w} mm") };
        s.rectangle(corner,&width,&format!("{h} mm"),[0.0,0.0,w,h]);
        let body=extrude(&mut b,&format!("部品 {}",i+1),s);
        parts.push(NestPart { body,count,up:Default::default(),rotate:NestRotate::Free });
    }
    finish(b,parts,if two_sheets { [100.0,80.0] } else { [200.0,150.0] },NestMethod::Boxes)
}

/// L 字と円を、形どおりに配置する見本。
pub fn nesting_shapes_document()->Document {
    let mut b=DocBuilder::new();
    let mut s=SketchBuilder::xy();
    let pts:[[f64;2];6]=[[0.0,0.0],[36.0,0.0],[36.0,10.0],[10.0,10.0],[10.0,36.0],[0.0,36.0]];
    let pts=pts.map(|p|s.fixed_point(p[0],p[1]));
    for i in 0..pts.len() { s.line(pts[i],pts[(i+1)%pts.len()]); }
    let l=extrude(&mut b,"L 字の部品",s);
    let mut s=SketchBuilder::xy();let center=s.fixed_point(10.0,10.0);s.circle(center,"20 mm",10.0);
    let circle=extrude(&mut b,"円の部品",s);
    finish(b,vec![NestPart { body:l,count:3,up:Default::default(),rotate:NestRotate::Free },NestPart { body:circle,count:4,up:Default::default(),rotate:NestRotate::Free }],[150.0,120.0],NestMethod::Shapes)
}
