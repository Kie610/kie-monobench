//! 確かめること: 派生した写しの配置、寸法の再計算、元の面と辺の参照の全写しへの適用。

use kmb_cam_setup::{SETUP,Setup};
use kmb_cam_setup::nest::{NestLayout,SHEET_GAP};
use kmb_core::{FeatureId,FeatureStatus,Op,Payload,Transaction};
use kmb_toolpath::Move;

fn layout(report:&kmb_core::RecomputeReport,id:FeatureId)->NestLayout {
    let Some(FeatureStatus::Ok { outputs,.. })=report.feature(id) else { panic!("配置の評価: {:?}",report.feature(id)); };
    NestLayout::from_value(&outputs["nest"]).expect("配置の出力")
}

#[test]
fn rectangle_samples_have_one_and_two_sheets_and_hold_named_copies() {
    let reg=kmb_bundle::registry();let kernel=kmb_bundle::kernel();
    for (two,count) in [(false,1),(true,2)] {
        let mut doc=crate::nesting_boxes_document(two);
        let id=doc.data().features.iter().find(|f|f.type_id==SETUP).unwrap().id;
        let report=doc.recompute(&reg,kernel.as_ref());
        assert_eq!(report.error_count(),0,"{:?}",report.features);
        let layout=layout(&report,id);
        assert_eq!(layout.sheets.len(),count,"{layout:?}");
        assert!(layout.unplaced.is_empty());
        let Some(FeatureStatus::Ok { outputs,.. })=report.feature(id) else { unreachable!(); };
        assert_eq!(outputs.keys().filter(|k|k.starts_with("nest.")).count(),12);
        let setup=kmb_cam_setup::SetupInfo::from_value(&outputs["setup"]).unwrap();
        assert_eq!(setup.more_bodies.len(),11);
        assert_eq!(setup.sections.as_ref().unwrap().list.len(),count);
        for sheet in &layout.sheets { for part in &sheet.parts {
            let n=layout.sheets.iter().take_while(|s|s.index<sheet.index).map(|s|s.parts.len()).sum::<usize>()
                +sheet.parts.iter().position(|p|p==part).unwrap()+1;
            let kmb_core::Value::Shape(handle)=outputs[&format!("nest.{n}")] else { panic!("写し"); };
            let Some(FeatureStatus::Ok { outputs:source_outputs,.. })=report.feature(part.body) else { panic!("元のボディ"); };
            let kmb_core::Value::Shape(original)=source_outputs["body"] else { panic!("元の形"); };
            assert_eq!(kernel.face_names(handle).unwrap(),kernel.face_names(original).unwrap());
            let (min,max)=kernel.bounds(handle).unwrap();
            assert!(min[0]>=f64::from(sheet.index-1)*(sheet.size[0]+SHEET_GAP)-1e-5);
            assert!(max[0]<=f64::from(sheet.index-1)*(sheet.size[0]+SHEET_GAP)+sheet.size[0]+1e-5);
        } }
    }
}

#[test]
fn changing_a_part_dimension_recomputes_layout_without_duplicating_operations() {
    let mut doc=crate::nesting_boxes_document(true);
    let reg=kmb_bundle::registry();let kernel=kmb_bundle::kernel();
    let id=doc.data().features.iter().find(|f|f.type_id==SETUP).unwrap().id;
    let before=doc.recompute(&reg,kernel.as_ref());
    let old=layout(&before,id);
    let parameter=doc.data().param_by_name("部品の幅").unwrap().id;
    let features=doc.data().features.len();
    doc.apply(Transaction::new("部品の幅",vec![Op::SetParamExpr { id:parameter,expr:"52 mm".into() }])).unwrap();
    let after=doc.recompute(&reg,kernel.as_ref());
    assert_eq!(after.error_count(),0,"{:?}",after.features);
    let new=layout(&after,id);
    assert_ne!(serde_json::to_string(&old).unwrap(),serde_json::to_string(&new).unwrap());
    assert_eq!(doc.data().features.len(),features);
}

#[test]
fn one_contour_operation_reaches_every_nested_part() {
    let mut doc=crate::nesting_boxes_document(true);
    let reg=kmb_bundle::registry();let kernel=kmb_bundle::kernel();
    let id=doc.data().features.iter().find(|f|f.type_id==SETUP).unwrap().id;
    let report=doc.recompute(&reg,kernel.as_ref());
    let layout=layout(&report,id);
    let setups=kmb_cam_commands::setups(doc.data(),&report,&reg);let s=&setups[0];
    assert_eq!(s.paths.len(),1);
    let radius=s.paths[0].tool.radius();
    for sheet in &layout.sheets { for part in &sheet.parts {
        let x=f64::from(sheet.index-1)*(sheet.size[0]+SHEET_GAP)+part.position[0];
        let y=part.position[1];
        assert!(s.paths[0].moves.iter().any(|m| {
            !matches!(m,Move::Rapid { .. }) && m.to()[2]<=-4.99 &&
                m.to()[0]>=x-radius-1e-4 && m.to()[0]<=x+part.size[0]+radius+1e-4 &&
                m.to()[1]>=y-radius-1e-4 && m.to()[1]<=y+part.size[1]+radius+1e-4
        }),"板 {} の部品 {} の {} 個目に加工が届きません",sheet.index,part.part,part.instance);
    } }
}

#[test]
fn shape_sample_preserves_instances_and_shows_its_actual_grid() {
    let mut doc=crate::nesting_shapes_document();let reg=kmb_bundle::registry();let kernel=kmb_bundle::kernel();
    let id=doc.data().features.iter().find(|f|f.type_id==SETUP).unwrap().id;
    let report=doc.recompute(&reg,kernel.as_ref());
    assert_eq!(report.error_count(),0,"{:?}",report.features);
    let layout=layout(&report,id);
    assert_eq!(layout.sheets.iter().map(|s|s.parts.len()).sum::<usize>(),7);
    assert!(layout.unplaced.is_empty());assert_eq!(layout.grid,1.0);
}

#[test]
fn nesting_rejects_incompatible_setup_and_only_blocks_its_downstream() {
    let mut doc=crate::nesting_boxes_document(false);let reg=kmb_bundle::registry();let kernel=kmb_bundle::kernel();
    let entry=doc.data().features.iter().find(|f|f.type_id==SETUP).unwrap();let id=entry.id;
    let mut setup:Setup=serde_json::from_value(entry.data.clone()).unwrap();setup.flip=kmb_cam_setup::Flip::AroundX;
    doc.apply(Transaction::new("向き",vec![Op::SetFeatureData { id,data:serde_json::to_value(setup).unwrap() }])).unwrap();
    let report=doc.recompute(&reg,kernel.as_ref());
    assert!(matches!(report.feature(id),Some(FeatureStatus::Error { .. })));
    assert!(doc.data().features.iter().filter(|f|f.type_id==kmb_cad_extrude::EXTRUDE).all(|f|matches!(report.feature(f.id),Some(FeatureStatus::Ok { .. }))));
}
