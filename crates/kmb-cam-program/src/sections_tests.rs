use super::*;
use kmb_cam_setup::sections::{Rect2,Section,SectionKind,SectionOutput,Sections};
use kmb_toolpath::{Move,Op};

fn two_sheets()->SetupPaths {
    let mut s=super::tests::setup_of(&[("部品",1,"cam.face@1")]);
    let mut other=s.paths[0].clone();
    let xf=kmb_toolpath::Xform2::translate(110.0,0.0);
    other.moves=other.moves.iter().map(|m|xf.apply(m)).collect();
    s.paths[0].moves.extend(other.moves);
    s.info.stock_max[0]=120.0;
    s.info.sections=Some(Sections { kind:SectionKind::Sheets,output:SectionOutput::Files,
        list:(0..2).map(|k| {
            let x=f64::from(k)*110.0;
            let window=Rect2 { min:[x-50.0,-50.0],max:[x+60.0,60.0] };
            Section { index:k+1,label:format!("板 {}",k+1),cut:window,own:window,shift:[-x,0.0],
                stock_min:[x,0.0,-5.0],stock_max:[x+10.0,10.0,0.0] }
        }).collect() });
    s
}

#[test]
fn section_paths_are_in_sheet_one_coordinates_and_files_match_single_exports() {
    let s=two_sheets();
    let machine=kmb_post_carvera_air::profile();
    let post=kmb_post_carvera_air::CarveraAir;
    let files=gcode_files(&s,&post,&machine).unwrap();
    assert_eq!(files.len(),2);
    for (k,(name,text)) in files.iter().enumerate() {
        let section=s.section(k as u32+1);
        assert!(name.ends_with(&format!(" 板{}.nc",k+1)));
        assert_eq!(*text,gcode(&section,&post,&machine).unwrap());
        assert!(section.paths.iter().flat_map(|p|&p.moves).all(|m| {
            let p=m.to();p[0]>=0.0 && p[0]<=10.0 && p[1]>=0.0 && p[1]<=10.0
        }));
        assert!(section.info.sections.is_none());
    }
    assert_eq!(s.paths.len(),1,"加工は板ごとに足さない");
}

#[test]
fn stops_output_has_one_safe_stop_between_each_sheet_and_same_cut_coordinates() {
    let mut s=two_sheets();
    let expected:Vec<_>=(1..=2).flat_map(|k|s.section(k).program().ops).filter(|o|matches!(o,Op::Move(Move::Linear { .. }|Move::Arc { .. }))).collect();
    s.info.sections.as_mut().unwrap().output=SectionOutput::Stops;
    let program=s.program();
    assert_eq!(program.ops.iter().filter(|o|matches!(o,Op::Stop { .. })).count(),1);
    let at=program.ops.iter().position(|o|matches!(o,Op::Stop { .. })).unwrap();
    assert!(program.ops[..at].iter().rev().any(|o|matches!(o,Op::SpindleStop)));
    assert!(matches!(program.ops[at-1],Op::Message { .. }));
    assert_eq!(program.ops.into_iter().filter(|o|matches!(o,Op::Move(Move::Linear { .. }|Move::Arc { .. }))).collect::<Vec<_>>(),expected);
    assert_eq!(gcode_files(&s,&kmb_post_carvera_air::CarveraAir,&kmb_post_carvera_air::profile()).unwrap().len(),1);
}

#[test]
fn fixed_path_is_neither_clipped_nor_translated_for_any_sheet() {
    let mut s=two_sheets();
    s.sources[0].section_fixed=true;
    for k in 1..=2 { assert_eq!(s.section(k).paths,s.paths); }
}

#[test]
fn simulation_uses_sheet_union_with_empty_gap_and_world_placed_paths() {
    let s=two_sheets();let stock=initial_stock(&s.info,1.0);
    let height=|x:usize|stock.z[5*stock.nx+x];
    assert_eq!(height(5),0.0);assert_eq!(height(50),-5.0);assert_eq!(height(115),0.0);
    let paths=s.simulation_paths();
    assert_eq!(paths.len(),1,"加工の段は増やさない");
    assert!(paths[0].moves.iter().any(|m|m.to()[0]<10.0));
    assert!(paths[0].moves.iter().any(|m|m.to()[0]>110.0));
}

#[test]
fn section_headings_and_file_names_appear_in_both_sheets() {
    let s=two_sheets();let machine=kmb_post_carvera_air::profile();
    let md=setup_sheet(&s,&machine);let html=setup_sheet_html(&s,&machine);
    for n in 1..=2 {
        assert!(md.contains(&format!("## 板 {n}")));
        assert!(html.contains(&format!("<h2>板 {n}</h2>")));
        assert!(md.contains(&format!("板{n}.nc")));assert!(html.contains(&format!("板{n}.nc")));
    }
}
