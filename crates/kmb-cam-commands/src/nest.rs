//! 板への配置の作成と部品の編集。すべて文書のセットアップのデータを返す。

use crate::{Setup, StockMode};
use crate::commands::{selected_setup, setup_of};
use kmb_cam_setup::nest::{Nest, NestMethod, NestPart, NestRotate, SheetFrom};
use kmb_core::{Action, Arg, Context, Edit, FeatureId, Outcome, Place, Problem, Value,Payload};

pub const CREATE: &str = "cam.nest.create";
pub const ADD_PART: &str = "cam.nest.add_part";
pub const REMOVE_PART: &str = "cam.nest.remove_part";
pub const SET_PART: &str = "cam.nest.set_part";

fn selected_bodies(cx:&Context<'_>) -> Vec<FeatureId> {
    let mut out=Vec::new();
    for id in cx.selected.into_iter().chain(cx.also_selected.iter().copied()) {
        if cx.output(id,"body").is_some() && !out.contains(&id) { out.push(id); }
    }
    out
}

pub fn actions(cx:&Context<'_>) -> Vec<Action> {
    let nested=selected_setup(cx).and_then(|id|setup_of(cx,id)).is_some_and(|s|s.nest.is_some());
    let part=||Arg::integer("part","部品の番号(1 から)").required();
    vec![
        Action::new(CREATE,"板への配置","cam").menu("セットアップ")
            .hint("選んだボディを板へ並べるセットアップを作る")
            .arg(Arg::list("bodies","配置するボディの番号。省くと選んだボディ"))
            .arg(Arg::choice("method","並べ方",&[("boxes","外形の箱"),("shapes","形どおり")]).default("boxes"))
            .arg(Arg::choice("sheet","板の大きさの取り方",&[("stock","ストック"),("machine","機械の作業範囲")]).default("stock"))
            .arg(Arg::length("spacing","部品の間").default("8 mm"))
            .arg(Arg::length("edge","縁の余白").default("5 mm"))
            .arg(Arg::length("width","板の幅").default("100 mm"))
            .arg(Arg::length("depth","板の奥行").default("100 mm"))
            .arg(Arg::length("height","板の厚さ").default("20 mm"))
            .arg(Arg::length("grid","形どおりの格子の目").default("1 mm"))
            .arg(Arg::integer("max_sheets","板の枚数の上限").default(10))
            .arg(Arg::choice("output","区画の書き出し",&[("files","板ごとのファイル"),("stops","止めを入れた 1 ファイル")]).default("files"))
            .at(&[Place::Menu]).disabled_if((selected_bodies(cx).is_empty() && cx.arg("bodies").is_none_or(serde_json::Value::is_null)).then_some("配置するボディを選んでください")),
        Action::new(ADD_PART,"配置する部品を足す","cam").menu("セットアップ")
            .hint("板への配置に、別のボディを部品として足す")
            .arg(Arg::feature("body","足すボディ").required())
            .arg(Arg::integer("count","個数").default(1))
            .at(&[Place::Menu,Place::Panel]).disabled_if((!nested).then_some("板への配置のセットアップを選んでください")),
        Action::new(REMOVE_PART,"配置する部品を外す","cam").menu("セットアップ")
            .hint("板への配置から、指定した番号の部品を外す").arg(part())
            .at(&[Place::Menu,Place::Panel]).disabled_if((!nested).then_some("板への配置のセットアップを選んでください")),
        Action::new(SET_PART,"配置する部品を直す","cam").menu("セットアップ")
            .hint("部品の個数、回してよい向き、上に向ける面を直す").arg(part())
            .arg(Arg::integer("count","個数"))
            .arg(Arg::choice("rotate","回してよい向き",&[("free","自由"),("grain","木目をそろえる"),("fixed","固定")]))
            .arg(Arg::choice("up","上に向ける面",&[("plus_z","上面"),("minus_z","下面"),("plus_x","右の側面"),("minus_x","左の側面"),("plus_y","奥の側面"),("minus_y","手前の側面")]))
            .disabled_if((!nested).then_some("板への配置のセットアップを選んでください")),
    ]
}

fn auto_part(cx:&Context<'_>,body:FeatureId,count:u32) -> Result<NestPart,Problem> {
    let h=match cx.output(body,"body") { Some(Value::Shape(h))=>*h,_=>return Err("配置するボディがありません".into()) };
    let (min,max)=cx.kernel.bounds(h).map_err(|e|Problem::from(e.to_string()))?;
    let axis=(0..3).min_by(|&a,&b|(max[a]-min[a]).total_cmp(&(max[b]-min[b]))).expect("3 軸");
    let up=match axis { 0=>kmb_cam_setup::Up::PlusX,1=>kmb_cam_setup::Up::PlusY,_=>kmb_cam_setup::Up::PlusZ };
    Ok(NestPart { body,count,up,rotate:NestRotate::Free })
}

fn integer(cx:&Context<'_>,key:&str,default:u32)->Result<u32,Problem> {
    let n=cx.arg_f64(key)?.unwrap_or(f64::from(default));
    if n>=0.0 && n<=f64::from(u32::MAX) && n.fract()==0.0 { Ok(n as u32) }
    else { Err(format!("{key} は 0 以上の整数で指定してください").into()) }
}

fn mm(cx:&Context<'_>,key:&str,default:f64)->Result<String,Problem> {
    Ok(format!("{} mm",cx.arg_f64(key)?.unwrap_or(default)))
}

pub fn run(id:&str,cx:&Context<'_>,machine:&kmb_post::MachineProfile)->Option<Result<Outcome,Problem>> {
    if ![CREATE,ADD_PART,REMOVE_PART,SET_PART].contains(&id) { return None; }
    Some((|| {
        if id==CREATE {
            let ids:Vec<FeatureId>=match cx.arg("bodies").filter(|v|!v.is_null()) {
                Some(v)=>serde_json::from_value(v.clone()).map_err(|e|Problem::from(e.to_string()))?,
                None=>selected_bodies(cx),
            };
            let body=*ids.first().ok_or("配置するボディを選んでください")?;
            let parts=ids.into_iter().map(|id|auto_part(cx,id,1)).collect::<Result<Vec<_>,_>>()?;
            let mut nest=Nest::new(parts);
            nest.method=if cx.arg_str("method")==Some("shapes") { NestMethod::Shapes } else { NestMethod::Boxes };
            nest.sheet=if cx.arg_str("sheet")==Some("machine") { SheetFrom::Machine } else { SheetFrom::Stock };
            nest.edge=mm(cx,"edge",5.0)?;nest.spacing=mm(cx,"spacing",8.0)?;nest.grid=mm(cx,"grid",1.0)?;
            nest.max_sheets=integer(cx,"max_sheets",10)?;
            if cx.arg_str("output")==Some("stops") { nest.output=kmb_cam_setup::sections::SectionOutput::Stops; }
            let mut setup=Setup::new(body);setup.set_machine(machine.clone());
            setup.stock.mode=StockMode::Fixed;
            setup.stock.width=mm(cx,"width",100.0)?;setup.stock.depth=mm(cx,"depth",100.0)?;setup.stock.height=mm(cx,"height",20.0)?;
            setup.nest=Some(nest);
            let mut edits=Vec::new();
            if crate::commands::last_tools(cx).is_none() {
                edits.push(Edit::Insert { type_id:crate::TOOLS.into(),name:"工具".into(),data:serde_json::to_value(crate::ToolLibrary::starter()).expect("JSON にできる") });
            }
            edits.push(Edit::Insert { type_id:crate::SETUP.into(),name:"板への配置".into(),data:serde_json::to_value(setup).expect("JSON にできる") });
            return Ok(Outcome::Change { label:"板への配置を作る".into(),edits });
        }
        let sid=selected_setup(cx).ok_or("板への配置のセットアップを選んでください")?;
        let mut setup=setup_of(cx,sid).ok_or("セットアップのデータが読めません")?;
        let nest=setup.nest.as_mut().ok_or("板への配置のセットアップを選んでください")?;
        if id==ADD_PART {
            let body=FeatureId(cx.arg("body").and_then(|v|v.as_u64().or_else(||v.as_f64()
                .filter(|n|*n>=0.0 && *n<u64::MAX as f64 && n.fract()==0.0).map(|n|n as u64)))
                .ok_or("足すボディの番号がありません")?);
            nest.parts.push(auto_part(cx,body,integer(cx,"count",1)?)?);
        } else {
            let index=integer(cx,"part",0)?.checked_sub(1).ok_or("部品の番号は 1 から指定してください")? as usize;
            if index>=nest.parts.len() { return Err("部品の番号がありません".into()); }
            if id==REMOVE_PART {
                if nest.parts.len()==1 { return Err("配置する部品は 1 種類以上必要です".into()); }
                nest.parts.remove(index);
            } else {
                if cx.arg("count").is_none_or(serde_json::Value::is_null) && cx.arg_str("rotate").is_none() && cx.arg_str("up").is_none() {
                    return Err("個数・回してよい向き・上に向ける面のいずれかを指定してください".into());
                }
                let part=&mut nest.parts[index];
                if cx.arg("count").is_some_and(|v|!v.is_null()) { part.count=integer(cx,"count",1)?; }
                if let Some(s)=cx.arg_str("rotate") { part.rotate=serde_json::from_value(serde_json::json!(s)).map_err(|e|Problem::from(e.to_string()))?; }
                if let Some(s)=cx.arg_str("up") { part.up=serde_json::from_value(serde_json::json!(s)).map_err(|e|Problem::from(e.to_string()))?; }
            }
        }
        if nest.parts.iter().any(|p|p.count==0) || nest.parts.iter().map(|p|u64::from(p.count)).sum::<u64>()>100 {
            return Err("部品は各 1 個以上で合計 100 個までにしてください".into());
        }
        setup.body=nest.parts[0].body;
        Ok(Outcome::Change { label:"配置する部品を直す".into(),edits:vec![Edit::SetData { id:sid,data:serde_json::to_value(setup).expect("JSON にできる") }] })
    })())
}

pub fn handles(id:&str,cx:&Context<'_>)->Option<Vec<kmb_core::Handle>> {
    use kmb_core::{Gesture,Handle,HandleChoice};
    if ![ADD_PART,REMOVE_PART].contains(&id) { return None; }
    let sid=selected_setup(cx)?;let setup=setup_of(cx,sid)?;let nest=setup.nest?;
    let mut handle=Handle::new(if id==ADD_PART { "body" } else { "part" },
        if id==ADD_PART { "配置に足すボディをクリック" } else { "配置する部品をクリック" },Gesture::Choice);
    if id==ADD_PART {
        handle.choices=cx.visible_bodies().into_iter().filter(|(id,_)|!nest.parts.iter().any(|p|p.body==*id)).filter_map(|(body,h)| {
            let (min,max)=cx.kernel.bounds(h).ok()?;
            Some(HandleChoice { value:serde_json::json!(body.0),label:cx.data.feature(body)?.name.clone(),
                outline:vec![vec![[min[0],min[1],max[2]],[max[0],min[1],max[2]],max,[min[0],max[1],max[2]],[min[0],min[1],max[2]]]],..Default::default() })
        }).collect();
    } else {
        let layout=kmb_cam_setup::nest::NestLayout::from_value(cx.output(sid,"nest")?)?;
        let info=crate::SetupInfo::from_value(cx.output(sid,"setup")?)?;
        let z=info.to_world([0.0,0.0,info.stock_max[2]])[2];
        for (i,p) in nest.parts.iter().enumerate() {
            let mut outline=Vec::new();
            for sheet in &layout.sheets { for placed in sheet.parts.iter().filter(|p|p.part==i) {
                let x=f64::from(sheet.index-1)*(sheet.size[0]+kmb_cam_setup::nest::SHEET_GAP)+placed.position[0];
                let y=placed.position[1];let [w,h]=placed.size;
                outline.push(vec![[x,y,z],[x+w,y,z],[x+w,y+h,z],[x,y+h,z],[x,y,z]]);
            } }
            handle.choices.push(HandleChoice { value:serde_json::json!(i+1),label:cx.data.feature(p.body)?.name.clone(),outline,..Default::default() });
        }
    }
    Some(vec![handle])
}

pub fn unplaced_notes(cx:&Context<'_>,sid:FeatureId)->Vec<String> {
    let mut out=Vec::new();
    if let Some(layout)=cx.output(sid,"nest").and_then(kmb_cam_setup::nest::NestLayout::from_value)
        && let Some(nest)=setup_of(cx,sid).and_then(|s|s.nest) {
        for (part,p) in nest.parts.iter().enumerate() {
            let count=layout.unplaced.iter().filter(|u|u.part==part).count();
            if count>0 {
                let name=kmb_cam_program::part_label(cx.data,cx.registry,p.body);
                out.push(format!("{name} が {count} 個、板に入りませんでした"));
            }
        }
    }
    out
}

/// 加工をまだ足していない時も、写しや網の細かさが変われば表示の形を出し直す。
pub fn overlay_key(info:&crate::SetupInfo,paths:u64)->u64 {
    use std::hash::{Hash,Hasher};
    let mut key=std::collections::hash_map::DefaultHasher::new();
    paths.hash(&mut key);info.body.hash(&mut key);info.more_bodies.hash(&mut key);
    info.tolerance.to_bits().hash(&mut key);
    key.finish()
}
