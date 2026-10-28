//! 板への配置。配置の計算は文書とカーネルに触らず、同じ入力から同じ順で答える。

use crate::Up;
use kmb_core::FeatureId;
use serde::{Deserialize, Serialize};

pub const SHEET_GAP: f64 = 100.0;
const MAX_CELLS: usize = 4_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SheetFrom {
    #[default]
    Stock,
    Machine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NestMethod {
    #[default]
    Boxes,
    Shapes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NestRotate {
    #[default]
    Free,
    Grain,
    Fixed,
}

impl NestRotate {
    fn angles(self, method: NestMethod) -> &'static [u32] {
        match (self, method) {
            (Self::Free, NestMethod::Shapes) => &[0, 90, 180, 270],
            (Self::Free, NestMethod::Boxes) => &[0, 90],
            (Self::Grain, NestMethod::Shapes) => &[0, 180],
            _ => &[0],
        }
    }
}

fn edge() -> String { "5 mm".into() }
fn spacing() -> String { "8 mm".into() }
fn grid() -> String { "1 mm".into() }
fn max_sheets() -> u32 { 10 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NestPart {
    pub body: FeatureId,
    pub count: u32,
    #[serde(default)]
    pub up: Up,
    #[serde(default)]
    pub rotate: NestRotate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nest {
    pub parts: Vec<NestPart>,
    #[serde(default)]
    pub sheet: SheetFrom,
    #[serde(default = "edge")]
    pub edge: String,
    #[serde(default = "spacing")]
    pub spacing: String,
    #[serde(default)]
    pub method: NestMethod,
    #[serde(default = "grid")]
    pub grid: String,
    #[serde(default = "max_sheets")]
    pub max_sheets: u32,
    /// 区画ごとのファイルか、段取りの停止を挟む 1 ファイルか。
    #[serde(default, skip_serializing_if = "is_files")]
    pub output: crate::sections::SectionOutput,
}

fn is_files(output:&crate::sections::SectionOutput)->bool { *output==crate::sections::SectionOutput::Files }

impl Nest {
    pub fn new(parts: Vec<NestPart>) -> Self {
        Self { parts, sheet: SheetFrom::Stock, edge: edge(), spacing: spacing(),
            method: NestMethod::Boxes, grid: grid(), max_sheets: max_sheets(),output:Default::default() }
    }
}

/// 上へ向けた網の XY 三角形。座標は部品の箱の手前左を (0, 0) にする。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackPart {
    pub body: FeatureId,
    pub count: u32,
    pub size: [f64; 2],
    pub triangles: Vec<[[f64; 2]; 3]>,
    pub rotate: NestRotate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackInput {
    pub parts: Vec<PackPart>,
    pub sheet: [f64; 2],
    pub edge: f64,
    pub spacing: f64,
    pub method: NestMethod,
    pub grid: f64,
    pub max_sheets: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedPart {
    /// `PackInput.parts` の 0 からの添字。
    pub part: usize,
    pub body: FeatureId,
    /// 同じ部品の何個目か(1 から)。
    pub instance: u32,
    pub angle: u32,
    /// 回した後の箱の手前左(板の座標)。
    pub position: [f64; 2],
    pub size: [f64; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnplacedReason { TooBig, NoSheet }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnplacedPart {
    pub part: usize,
    pub instance: u32,
    pub reason: UnplacedReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestSheet {
    pub index: u32,
    pub size: [f64; 2],
    pub utilization: f64,
    pub parts: Vec<PlacedPart>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct NestLayout {
    pub sheets: Vec<NestSheet>,
    pub unplaced: Vec<UnplacedPart>,
    pub grid: f64,
    pub warnings: Vec<String>,
}

impl kmb_core::Payload for NestLayout {
    const TYPE: &'static str = "cam.nest_layout@1";
}

pub type PackOutput = NestLayout;

#[derive(Clone, Copy)]
struct Rect { x: f64, y: f64, w: f64, h: f64 }

impl Rect {
    fn contains(self, b: Self) -> bool {
        b.x >= self.x - 1e-9 && b.y >= self.y - 1e-9
            && b.x + b.w <= self.x + self.w + 1e-9
            && b.y + b.h <= self.y + self.h + 1e-9
    }
}

fn rotated_size(p: &PackPart, angle: u32) -> [f64; 2] {
    if angle % 180 == 0 { p.size } else { [p.size[1], p.size[0]] }
}

/// 回転後の箱を原点へ寄せる。格子は回さず、この三角形から角度ごとに描く。
pub fn rotate_point(p: [f64; 2], size: [f64; 2], angle: u32) -> [f64; 2] {
    match angle {
        90 => [size[1] - p[1], p[0]],
        180 => [size[0] - p[0], size[1] - p[1]],
        270 => [p[1], size[0] - p[0]],
        _ => p,
    }
}

fn split_free(free: &mut Vec<Rect>, used: Rect) {
    let mut out = Vec::new();
    for r in free.drain(..) {
        if used.x >= r.x + r.w - 1e-9 || used.x + used.w <= r.x + 1e-9
            || used.y >= r.y + r.h - 1e-9 || used.y + used.h <= r.y + 1e-9 {
            out.push(r);
            continue;
        }
        if used.x > r.x { out.push(Rect { w: used.x - r.x, ..r }); }
        if used.y > r.y { out.push(Rect { h: used.y - r.y, ..r }); }
        if used.x + used.w < r.x + r.w {
            out.push(Rect { x: used.x + used.w, w: r.x + r.w - used.x - used.w, ..r });
        }
        if used.y + used.h < r.y + r.h {
            out.push(Rect { y: used.y + used.h, h: r.y + r.h - used.y - used.h, ..r });
        }
    }
    for (i, r) in out.iter().enumerate() {
        if !out.iter().enumerate().any(|(j, b)| j != i && b.contains(*r)
            && (!r.contains(*b) || j < i)) {
            free.push(*r);
        }
    }
}

/// 行ごとのビット列。衝突の検査は決まった行・列の順に行う。
struct Bits { w: usize, h: usize, stride: usize, rows: Vec<u64> }

impl Bits {
    fn new(w: usize, h: usize) -> Self {
        let stride = w.div_ceil(64);
        Self { w, h, stride, rows: vec![0; stride * h] }
    }
    fn get(&self, x: usize, y: usize) -> bool {
        self.rows[y * self.stride + x / 64] & (1 << (x % 64)) != 0
    }
    fn set(&mut self, x: usize, y: usize) {
        self.rows[y * self.stride + x / 64] |= 1 << (x % 64);
    }
    fn fits(&self, p: &Bits, x: usize, y: usize) -> bool {
        let shift=x%64;
        (0..p.h).all(|j|(0..p.stride).all(|word| {
            let bits=p.rows[j*p.stride+word];
            let at=(y+j)*self.stride+x/64+word;
            (self.rows[at] & (bits<<shift))==0
                && (shift==0 || at%self.stride+1>=self.stride || (self.rows[at+1] & (bits>>(64-shift)))==0)
        }))
    }
    fn add(&mut self, p: &Bits, x: usize, y: usize) {
        let shift=x%64;
        for j in 0..p.h { for word in 0..p.stride {
            let bits=p.rows[j*p.stride+word];let at=(y+j)*self.stride+x/64+word;
            self.rows[at]|=bits<<shift;
            if shift!=0 && at%self.stride+1<self.stride { self.rows[at+1]|=bits>>(64-shift); }
        } }
    }
}

/// 三角形とセルが少しでも重なる時は塗る。分離軸はセルの軸と三角形の辺の法線。
fn triangle_cell(t: [[f64; 2]; 3], min: [f64; 2], max: [f64; 2]) -> bool {
    let mut axes = vec![[1.0,0.0], [0.0,1.0]];
    for i in 0..3 { let a=t[i]; let b=t[(i+1)%3]; axes.push([a[1]-b[1],b[0]-a[0]]); }
    axes.into_iter().all(|n| {
        let (mut lo,mut hi) = (f64::INFINITY,f64::NEG_INFINITY);
        for p in t { let v=p[0]*n[0]+p[1]*n[1]; lo=lo.min(v); hi=hi.max(v); }
        let c=[min,[min[0],max[1]],max,[max[0],min[1]]];
        let cl=c.map(|p|p[0]*n[0]+p[1]*n[1]);
        hi >= cl.into_iter().fold(f64::INFINITY,f64::min)-1e-9
            && lo <= cl.into_iter().fold(f64::NEG_INFINITY,f64::max)+1e-9
    })
}

fn silhouette(p: &PackPart, angle: u32, g: f64, dilation: usize) -> (Bits,f64) {
    let size=rotated_size(p,angle);
    let w=(size[0]/g).ceil() as usize + 1;
    let h=(size[1]/g).ceil() as usize + 1;
    let mut core=Bits::new(w,h);
    for t in &p.triangles {
        let t=t.map(|v|rotate_point(v,p.size,angle));
        let lo=[0,1].map(|k|t.iter().map(|p|p[k]).fold(f64::INFINITY,f64::min));
        let hi=[0,1].map(|k|t.iter().map(|p|p[k]).fold(f64::NEG_INFINITY,f64::max));
        let x0=((lo[0]/g).floor().max(0.0) as usize).min(w-1);
        let y0=((lo[1]/g).floor().max(0.0) as usize).min(h-1);
        let x1=((hi[0]/g).floor().max(0.0) as usize).min(w-1);
        let y1=((hi[1]/g).floor().max(0.0) as usize).min(h-1);
        for y in y0..=y1 { for x in x0..=x1 {
            if triangle_cell(t,[x as f64*g,y as f64*g],[(x+1) as f64*g,(y+1) as f64*g]) { core.set(x,y); }
        } }
    }
    // 外から届かない空きセルは影の穴なので塗り、部品を穴の中へ入れない。
    let mut outside=Bits::new(w,h);
    let mut queue=std::collections::VecDeque::new();
    for y in 0..h { for x in 0..w {
        if (x==0 || y==0 || x==w-1 || y==h-1) && !core.get(x,y) && !outside.get(x,y) {
            outside.set(x,y); queue.push_back((x,y));
        }
    } }
    while let Some((x,y))=queue.pop_front() {
        for (a,b) in [(x.wrapping_sub(1),y),(x+1,y),(x,y.wrapping_sub(1)),(x,y+1)] {
            if a<w && b<h && !core.get(a,b) && !outside.get(a,b) {
                outside.set(a,b); queue.push_back((a,b));
            }
        }
    }
    let mut prefix=vec![0u32;(w+1)*(h+1)];
    for y in 0..h { for x in 0..w {
        let n=u32::from(core.get(x,y) || !outside.get(x,y));
        prefix[(y+1)*(w+1)+x+1]=n+prefix[y*(w+1)+x+1]+prefix[(y+1)*(w+1)+x]-prefix[y*(w+1)+x];
    } }
    let area=(f64::from(prefix[h*(w+1)+w])*g*g).min(p.size[0]*p.size[1]);
    let mut padded=Bits::new(w+2*dilation,h+2*dilation);
    for y in 0..padded.h { for x in 0..padded.w {
        let (x0,y0)=(x.saturating_sub(2*dilation).min(w),y.saturating_sub(2*dilation).min(h));
        let (x1,y1)=((x+1).min(w),(y+1).min(h));
        let positive=prefix[y1*(w+1)+x1]+prefix[y0*(w+1)+x0];
        let negative=prefix[y0*(w+1)+x1]+prefix[y1*(w+1)+x0];
        if positive>negative { padded.set(x,y); }
    } }
    (padded,area)
}

/// 入力の上限を超えたり有限でない値は、配置を作らず理由を返す。
pub fn pack(input: &PackInput) -> PackOutput {
    let mut out=NestLayout { grid: input.grid, ..NestLayout::default() };
    let finite = input.sheet.into_iter().chain([input.edge,input.spacing,input.grid]).all(f64::is_finite);
    let count: u64=input.parts.iter().map(|p|u64::from(p.count)).sum();
    if !finite || input.sheet.iter().any(|v|*v<=0.0) || input.edge<0.0
        || !(input.sheet[0]*input.sheet[1]).is_finite()
        || input.spacing<0.0 || input.grid<=0.0 || !(1..=50).contains(&input.max_sheets)
        || count>100 || input.parts.iter().any(|p|p.count==0 || p.size.iter().any(|v|!v.is_finite() || *v<=0.0)
            || !(p.size[0]*p.size[1]).is_finite()
            || p.triangles.iter().flatten().flatten().any(|v|!v.is_finite())
            || p.triangles.iter().flatten().any(|v|(0..2).any(|k|v[k] < -1e-8 || v[k]>p.size[k]+1e-8))
            || (input.method==NestMethod::Shapes && p.triangles.is_empty())) {
        out.warnings.push("板への配置の寸法・個数・格子の目を確かめてください".into());
        return out;
    }
    let available=input.sheet.map(|v|v-2.0*input.edge);
    let mut pending=Vec::new();
    for (part,p) in input.parts.iter().enumerate() { for instance in 1..=p.count { pending.push((part,instance)); } }
    pending.sort_by(|(a_index,ia),(b_index,ib)| {
        let (a,b)=(&input.parts[*a_index],&input.parts[*b_index]);
        (b.size[0]*b.size[1]).total_cmp(&(a.size[0]*a.size[1]))
            .then_with(||b.size[0].max(b.size[1]).total_cmp(&a.size[0].max(a.size[1])))
            .then(a_index.cmp(b_index)).then(ia.cmp(ib))
    });
    // 格子の総数を抑える。広げる分も含め、最悪の板の格子が上限内に入るまで粗くする。
    if input.method==NestMethod::Shapes {
        loop {
            let d=(input.spacing/out.grid).ceil()+1.0;
            let w=(available[0].max(0.0)/out.grid).ceil()+1.0+2.0*d;
            let h=(available[1].max(0.0)/out.grid).ceil()+1.0+2.0*d;
            if w*h<=MAX_CELLS as f64 { break; }
            out.grid*=2.0;
        }
        if out.grid>input.grid { out.warnings.push("格子の目を粗くして板への配置を計算しました".into()); }
    }
    let mut masks: Vec<Vec<(u32,Bits)>>=Vec::new();
    let mut areas:Vec<f64>=input.parts.iter().map(|p|p.size[0]*p.size[1]).collect();
    if input.method==NestMethod::Shapes {
        let d=(input.spacing/out.grid).ceil() as usize+1;
        for (index,p) in input.parts.iter().enumerate() {
            let variants:Vec<_>=p.rotate.angles(input.method).iter().filter_map(|&angle| {
                let s=rotated_size(p,angle);
                if s[0]>available[0] || s[1]>available[1] { return None; }
                let (mask,area)=silhouette(p,angle,out.grid,d);areas[index]=area;
                Some((angle,mask))
            }).collect();
            masks.push(variants);
        }
        pending.sort_by(|(a,ia),(b,ib)|areas[*b].total_cmp(&areas[*a])
            .then_with(||input.parts[*b].size[0].max(input.parts[*b].size[1]).total_cmp(&input.parts[*a].size[0].max(input.parts[*a].size[1])))
            .then(a.cmp(b)).then(ia.cmp(ib)));
    }
    pending.retain(|&(part,instance)| {
        let p=&input.parts[part];
        let fits=p.rotate.angles(input.method).iter().any(|&a| {
            let s=rotated_size(p,a); s[0]<=available[0]+1e-9 && s[1]<=available[1]+1e-9
        });
        if !fits { out.unplaced.push(UnplacedPart { part,instance,reason:UnplacedReason::TooBig }); }
        fits
    });
    for index in 1..=input.max_sheets {
        if pending.is_empty() { break; }
        let mut sheet=NestSheet { index,size:input.sheet,utilization:0.0,parts:Vec::new() };
        let mut free=vec![Rect { x:input.edge,y:input.edge,w:available[0]+input.spacing,h:available[1]+input.spacing }];
        let mut occupied=(input.method==NestMethod::Shapes).then(|| {
            let d=(input.spacing/out.grid).ceil() as usize+1;
            Bits::new(
            (available[0].max(0.0)/out.grid).ceil() as usize+1+2*d,
            (available[1].max(0.0)/out.grid).ceil() as usize+1+2*d)
        });
        pending.retain(|&(part,instance)| {
            let p=&input.parts[part];
            let mut choices=Vec::new();
            match input.method {
                NestMethod::Boxes => {
                    for &angle in p.rotate.angles(input.method) {
                        let size=rotated_size(p,angle);
                        for r in &free {
                            if size[0]+input.spacing<=r.w+1e-9 && size[1]+input.spacing<=r.h+1e-9 {
                                choices.push((r.y,r.x,angle));
                            }
                        }
                    }
                }
                NestMethod::Shapes => {
                    let occupied=occupied.as_mut().expect("形の格子");
                    'rows: for y in 0..occupied.h { for x in 0..occupied.w {
                        for (angle,mask) in &masks[part] {
                            let s=rotated_size(p,*angle);
                            if x+mask.w<=occupied.w && y+mask.h<=occupied.h
                                && x as f64*out.grid+s[0]<=available[0]+1e-9
                                && y as f64*out.grid+s[1]<=available[1]+1e-9
                                && occupied.fits(mask,x,y) {
                                choices.push((input.edge+y as f64*out.grid,input.edge+x as f64*out.grid,*angle));
                                break 'rows;
                            }
                        }
                    } }
                }
            }
            choices.sort_by(|a,b|a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
            let Some(&(y,x,angle))=choices.first() else { return true; };
            let size=rotated_size(p,angle);
            if let Some(occupied)=occupied.as_mut() {
                let mask=&masks[part].iter().find(|(a,_)|*a==angle).expect("選んだ角度").1;
                occupied.add(mask,((x-input.edge)/out.grid).round() as usize,((y-input.edge)/out.grid).round() as usize);
            } else {
                split_free(&mut free,Rect { x,y,w:size[0]+input.spacing,h:size[1]+input.spacing });
            }
            sheet.utilization+=areas[part]/(input.sheet[0]*input.sheet[1]);
            sheet.parts.push(PlacedPart { part,body:p.body,instance,angle,position:[x,y],size });
            false
        });
        if !sheet.parts.is_empty() { out.sheets.push(sheet); }
    }
    out.unplaced.extend(pending.into_iter().map(|(part,instance)|UnplacedPart { part,instance,reason:UnplacedReason::NoSheet }));
    out
}

pub(crate) struct PreparedNest {
    pub bodies: Vec<kmb_core::ShapeHandle>,
    pub layout: NestLayout,
    pub size: [f64; 2],
    pub thickness: f64,
    pub extent: f64,
    pub output: crate::sections::SectionOutput,
}

impl PreparedNest {
    pub fn write_outputs(&self,out:&mut kmb_core::Outputs) {
        for (k,h) in self.bodies.iter().enumerate() { out.insert(format!("nest.{}",k+1),kmb_core::Value::Shape(*h)); }
        out.insert("grid".into(),kmb_core::Value::Scalar(kmb_core::Quantity::mm(self.layout.grid)));
        out.insert("nest".into(),kmb_core::Payload::to_value(&self.layout));
    }
    pub fn sections(&self, origin: [f64; 3]) -> crate::sections::Sections {
        use crate::sections::{Rect2, Section, SectionKind, Sections};
        Sections {
            kind: SectionKind::Sheets, output: self.output,
            list: self.layout.sheets.iter().map(|s| {
                let x=f64::from(s.index-1)*(self.size[0]+SHEET_GAP);
                let min=[x-origin[0],-origin[1],-origin[2]];
                let max=[x+self.size[0]-origin[0],self.size[1]-origin[1],self.thickness-origin[2]];
                let window=Rect2 { min:[min[0]-SHEET_GAP/2.0,min[1]-SHEET_GAP/2.0],
                    max:[max[0]+SHEET_GAP/2.0,max[1]+SHEET_GAP/2.0] };
                Section { index:s.index,label:format!("板 {}",s.index),cut:window,own:window,
                    shift:[-x,0.0],stock_min:min,stock_max:max }
            }).collect(),
        }
    }
}

pub(crate) fn prepare(
    ctx: &kmb_core::EvalCtx<'_>, setup: &crate::Setup, nest: &Nest,
    tolerance: f64, machine: &kmb_post::MachineProfile,
) -> Result<PreparedNest,kmb_core::EvalError> {
    use crate::{invalid,length,placement};
    use kmb_cam_geom::MachiningModel;
    use kmb_core::{EvalError, Value};
    if nest.parts.is_empty() || nest.parts[0].body!=setup.body || !setup.more_bodies.is_empty()
        || setup.stock.mode!=crate::StockMode::Fixed || !setup.up.is_default()
        || setup.turn!=crate::Turn::None || setup.flip!=crate::Flip::None {
        return Err(invalid("板への配置は部品の先頭をボディにし、固定のストックと既定の向きで使ってください"));
    }
    if !(1..=50).contains(&nest.max_sheets) || nest.parts.iter().any(|p|p.count==0)
        || nest.parts.iter().map(|p|u64::from(p.count)).sum::<u64>()>100 {
        return Err(invalid("部品は各 1 個以上で合計 100 個まで、板は 1〜50 枚にしてください"));
    }
    if nest.output==crate::sections::SectionOutput::Stops && setup.leveling.mode==crate::LevelingMode::Gcode {
        return Err(invalid("G-code で板ごとに高さを測る時は、区画の書き出しを「板ごとのファイル」にしてください"));
    }
    let size=match nest.sheet {
        SheetFrom::Stock=>[length(ctx,&setup.stock.width,"板の幅")?,length(ctx,&setup.stock.depth,"板の奥行")?],
        SheetFrom::Machine=>[machine.travel[0]-2.0*crate::FIXTURE_MARGIN,machine.travel[1]-2.0*crate::FIXTURE_MARGIN],
    };
    let thickness=length(ctx,&setup.stock.height,"板の厚さ")?;
    let edge=length(ctx,&nest.edge,"縁の余白")?;
    let spacing=length(ctx,&nest.spacing,"部品の間")?;
    let grid=length(ctx,&nest.grid,"格子の目")?;
    if size.iter().any(|v|!v.is_finite() || *v<=0.0) || !thickness.is_finite() || thickness<=0.0
        || !edge.is_finite() || edge<0.0 || !spacing.is_finite() || spacing<0.0 || !grid.is_finite() || grid<=0.0 {
        return Err(invalid("板の寸法と格子の目は 0 より大きく、余白と間は 0 以上にしてください"));
    }
    let mut input=PackInput { parts:Vec::new(),sheet:size,edge,spacing,method:nest.method,grid,max_sheets:nest.max_sheets };
    let mut originals=Vec::new();
    let mut warnings=Vec::new();
    for p in &nest.parts {
        let h=match ctx.output(p.body,"body")? { Value::Shape(h)=>*h,_=>return Err(invalid("配置する部品はボディを選んでください")) };
        let mesh=ctx.kernel().mesh(h,tolerance).map_err(|e|EvalError::Kernel(e.to_string()))?;
        let model=MachiningModel::from_mesh(&mesh,[0.0;3]);
        let center=[0,1,2].map(|k|(model.min[k]+model.max[k])/2.0);
        let rot=p.up.matrix();
        let placed=placement::rotate_mesh(&rot,&mesh,center);
        let bounds=MachiningModel::from_mesh(&placed,[0.0;3]);
        if bounds.max[2]-bounds.min[2]>thickness+1e-6 {
            warnings.push(format!("部品 {} は板より厚くなっています",p.body));
        }
        let triangles=placed.triangles.iter().map(|t|t.map(|i| {
            let v=placed.positions[i as usize];[f64::from(v[0])-bounds.min[0],f64::from(v[1])-bounds.min[1]]
        })).collect();
        input.parts.push(PackPart { body:p.body,count:p.count,size:[bounds.max[0]-bounds.min[0],bounds.max[1]-bounds.min[1]],triangles,rotate:p.rotate });
        originals.push((h,center,rot,bounds.min));
    }
    let mut layout=pack(&input);
    let mut bodies=Vec::new();
    for sheet in &layout.sheets {
        let offset=f64::from(sheet.index-1)*(size[0]+SHEET_GAP);
        for part in &sheet.parts {
            let (h,center,up,min)=originals[part.part];
            let turn=placement::turn_matrix(f64::from(part.angle));
            let rot=placement::mul(&turn,&up);
            let after=placement::apply(&turn,min);
            let corners=[[0.0,0.0],[input.parts[part.part].size[0],0.0],input.parts[part.part].size,[0.0,input.parts[part.part].size[1]]];
            let rmin=[0,1].map(|k|corners.iter().map(|p|placement::apply(&turn,[p[0],p[1],0.0])[k]).fold(f64::INFINITY,f64::min));
            let target=[offset+part.position[0]-rmin[0]-after[0],part.position[1]-rmin[1]-after[1],-min[2]];
            let up_center=placement::apply(&up,center);
            let correction=[0,1,2].map(|k|center[k]-up_center[k]);
            let correction=placement::apply(&turn,correction);
            let mut matrix=[[0.0;4];4];
            for k in 0..3 { for j in 0..3 { matrix[k][j]=rot[k][j]; } matrix[k][3]=target[k]+correction[k]; }
            matrix[3][3]=1.0;
            bodies.push(ctx.kernel().transform(h,&matrix,"").map_err(|e|EvalError::Kernel(e.to_string()))?);
        }
    }
    if size[0]+2.0*crate::FIXTURE_MARGIN>machine.travel[0] || size[1]+2.0*crate::FIXTURE_MARGIN>machine.travel[1] {
        warnings.push("板が機械の作業範囲を超えています".into());
    }
    for (part,p) in nest.parts.iter().enumerate() {
        let n=layout.unplaced.iter().filter(|u|u.part==part).count();
        if n>0 { warnings.push(format!("部品 {} が {n} 個、板に入りませんでした",p.body)); }
    }
    layout.warnings.extend(warnings);
    let extent=if layout.sheets.is_empty() { 0.0 } else { (layout.sheets.len()-1) as f64*(size[0]+SHEET_GAP)+size[0] };
    Ok(PreparedNest { bodies,layout,size,thickness,extent,output:nest.output })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(body:u64,size:[f64;2],count:u32) -> PackPart {
        let [w,h]=size;
        PackPart { body:FeatureId(body),size,count,rotate:NestRotate::Free,
            triangles:vec![[[0.0,0.0],[w,0.0],[w,h]],[[0.0,0.0],[w,h],[0.0,h]]] }
    }
    fn input(parts:Vec<PackPart>) -> PackInput {
        PackInput { parts,sheet:[100.0,80.0],edge:5.0,spacing:8.0,method:NestMethod::Boxes,grid:1.0,max_sheets:10 }
    }
    fn assert_boxes(input:&PackInput,out:&NestLayout) {
        for sheet in &out.sheets {
            for (i,p) in sheet.parts.iter().enumerate() {
                for k in 0..2 {
                    assert!(p.position[k]>=input.edge-1e-8);
                    assert!(p.position[k]+p.size[k]<=input.sheet[k]-input.edge+1e-8);
                }
                for q in &sheet.parts[i+1..] {
                    let dx=(p.position[0]-q.position[0]-q.size[0]).max(q.position[0]-p.position[0]-p.size[0]).max(0.0);
                    let dy=(p.position[1]-q.position[1]-q.size[1]).max(q.position[1]-p.position[1]-p.size[1]).max(0.0);
                    assert!(dx.hypot(dy)+1e-8>=input.spacing,"{p:?} / {q:?}");
                }
            }
        }
    }

    #[test]
    fn boxes_keep_every_instance_spacing_and_edge_on_multiple_sheets() {
        let i=input(vec![rectangle(9,[30.0,21.0],7),rectangle(3,[21.0,17.0],5),rectangle(5,[16.0,11.0],4)]);
        let out=pack(&i);
        assert!(out.sheets.len()>1);
        assert!(out.unplaced.is_empty());
        assert_eq!(out.sheets.iter().map(|s|s.parts.len()).sum::<usize>(),16);
        assert_boxes(&i,&out);
    }

    #[test]
    fn boxes_do_not_use_the_shape_grid_even_when_its_spacing_ratio_overflows() {
        let mut i=input(vec![rectangle(1,[20.0,10.0],2)]);
        i.spacing=1.0;
        let expected=pack(&i);
        assert_eq!(expected.sheets.len(),1);
        assert!(expected.unplaced.is_empty());
        for grid in [f64::from_bits(1),f64::MIN_POSITIVE] {
            i.grid=grid;
            let actual=pack(&i);
            assert_eq!(actual.sheets,expected.sheets);
            assert_eq!(actual.unplaced,expected.unplaced);
            assert_eq!(actual.warnings,expected.warnings);
            assert_boxes(&i,&actual);
        }
    }

    #[test]
    fn empty_sheet_failure_and_sheet_limit_have_distinct_reasons() {
        let mut i=input(vec![rectangle(1,[91.0,71.0],1),rectangle(2,[90.0,70.0],2)]);
        i.max_sheets=1;
        let out=pack(&i);
        assert_eq!(out.unplaced,vec![UnplacedPart { part:0,instance:1,reason:UnplacedReason::TooBig },
            UnplacedPart { part:1,instance:2,reason:UnplacedReason::NoSheet }]);
        assert_eq!(out.sheets[0].parts.len(),1);
    }

    #[test]
    fn quarter_turn_fits_but_fixed_and_grain_do_not_turn_across_grain() {
        let mut i=input(vec![rectangle(1,[20.0,85.0],1)]);
        let out=pack(&i);
        assert_eq!(out.sheets[0].parts[0].angle,90);
        assert_boxes(&i,&out);
        for rotate in [NestRotate::Fixed,NestRotate::Grain] {
            i.parts[0].rotate=rotate;
            assert_eq!(pack(&i).unplaced[0].reason,UnplacedReason::TooBig);
        }
    }

    #[test]
    fn ties_follow_part_index_and_layout_has_deterministic_json_roundtrip() {
        let i=input(vec![rectangle(99,[25.0,20.0],2),rectangle(1,[25.0,20.0],2)]);
        let a=pack(&i);
        assert_eq!(a.sheets[0].parts[0].part,0);
        assert_eq!(a.sheets[0].parts[0].instance,1);
        let json=serde_json::to_string(&a).unwrap();
        let roundtrip:PackInput=serde_json::from_str(&serde_json::to_string(&i).unwrap()).unwrap();
        assert_eq!(json,serde_json::to_string(&pack(&roundtrip)).unwrap());
        assert_eq!(a,serde_json::from_str::<PackOutput>(&json).unwrap());
    }

    // 三角形の辺どうしの距離を使う独立の判定。配置の格子は読まない。
    fn segment_distance(a:[f64;2],b:[f64;2],c:[f64;2],d:[f64;2]) -> f64 {
        fn cross(a:[f64;2],b:[f64;2],c:[f64;2])->f64 { (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0]) }
        fn point(p:[f64;2],a:[f64;2],b:[f64;2])->f64 {
            let v=[b[0]-a[0],b[1]-a[1]];
            let t=(((p[0]-a[0])*v[0]+(p[1]-a[1])*v[1])/(v[0]*v[0]+v[1]*v[1]).max(1e-30)).clamp(0.0,1.0);
            (p[0]-a[0]-t*v[0]).hypot(p[1]-a[1]-t*v[1])
        }
        if cross(a,b,c)*cross(a,b,d)<0.0 && cross(c,d,a)*cross(c,d,b)<0.0 { 0.0 }
        else { point(a,c,d).min(point(b,c,d)).min(point(c,a,b)).min(point(d,a,b)) }
    }

    fn triangles(i:&PackInput,p:&PlacedPart)->Vec<[[f64;2];3]> {
        let (s,c)=f64::from(p.angle).to_radians().sin_cos();
        let mut ts:Vec<_>=i.parts[p.part].triangles.iter().map(|t|t.map(|q|[c*q[0]-s*q[1],s*q[0]+c*q[1]])).collect();
        let min=[0,1].map(|k|ts.iter().flatten().map(|v|v[k]).fold(f64::INFINITY,f64::min));
        for t in &mut ts { for q in t { for k in 0..2 { q[k]+=p.position[k]-min[k]; } } }
        ts
    }

    fn inside_triangle(p:[f64;2],t:[[f64;2];3])->bool {
        let side=|a:[f64;2],b:[f64;2]|(b[0]-a[0])*(p[1]-a[1])-(b[1]-a[1])*(p[0]-a[0]);
        let signs=[side(t[0],t[1]),side(t[1],t[2]),side(t[2],t[0])];
        signs.iter().all(|s|*s>=-1e-9) || signs.iter().all(|s|*s<=1e-9)
    }

    #[test]
    fn shapes_keep_true_triangle_distance_and_grain_orientation() {
        let mut l=rectangle(1,[30.0,30.0],3);
        l.rotate=NestRotate::Grain;
        l.triangles=vec![ [[0.0,0.0],[30.0,0.0],[30.0,8.0]], [[0.0,0.0],[30.0,8.0],[0.0,8.0]],
            [[0.0,8.0],[8.0,8.0],[8.0,30.0]], [[0.0,8.0],[8.0,30.0],[0.0,30.0]] ];
        let mut circle=rectangle(2,[14.0,14.0],3);
        circle.triangles=(0..24).map(|k| {
            let at=|a:f64|[7.0+7.0*a.cos(),7.0+7.0*a.sin()];
            [[7.0,7.0],at(f64::from(k)*std::f64::consts::TAU/24.0),at(f64::from(k+1)*std::f64::consts::TAU/24.0)]
        }).collect();
        let mut i=input(vec![l,circle]); i.method=NestMethod::Shapes; i.spacing=2.0;
        let out=pack(&i);
        assert!(out.unplaced.is_empty());
        for sheet in &out.sheets {
            for (k,p) in sheet.parts.iter().enumerate() {
                if p.part==0 { assert!([0,180].contains(&p.angle)); }
                for t in triangles(&i,p) { for q in t { for axis in 0..2 {
                    assert!(q[axis]>=i.edge-1e-8 && q[axis]<=i.sheet[axis]-i.edge+1e-8);
                } } }
                for q in &sheet.parts[k+1..] {
                    for a in triangles(&i,p) { for b in triangles(&i,q) {
                        assert!(!a.iter().any(|p|inside_triangle(*p,b)) && !b.iter().any(|p|inside_triangle(*p,a)));
                        for x in 0..3 { for y in 0..3 {
                            assert!(segment_distance(a[x],a[(x+1)%3],b[y],b[(y+1)%3])+1e-8>=i.spacing);
                        } }
                    } }
                }
            }
        }
        assert_eq!(serde_json::to_string(&out).unwrap(),serde_json::to_string(&pack(&i)).unwrap());
    }

    #[test]
    fn grid_budget_coarsens_and_reports_the_actual_pitch() {
        let mut i=input(vec![rectangle(1,[20.0,20.0],1)]);
        i.method=NestMethod::Shapes; i.grid=0.001;
        let out=pack(&i);
        assert!(out.grid>i.grid);
        assert!(!out.warnings.is_empty());
        assert_eq!(out.sheets.len(),1);
    }

    #[test]
    fn saved_nesting_expressions_rename_even_when_their_mode_does_not_use_them() {
        use kmb_core::Feature;
        for method in [NestMethod::Boxes,NestMethod::Shapes] { for sheet in [SheetFrom::Stock,SheetFrom::Machine] {
            let mut setup=crate::Setup::new(FeatureId(1));
            let mut nest=Nest::new(vec![NestPart { body:FeatureId(1),count:1,up:Up::PlusZ,rotate:NestRotate::Free }]);
            nest.method=method;nest.sheet=sheet;
            nest.edge="contract_p * 1".into();nest.spacing="contract_p * 1".into();nest.grid="contract_p * 1".into();
            setup.nest=Some(nest);
            assert!(setup.inputs().unwrap().contains(&kmb_core::InputRef::Param { name:"contract_p".into() }));
            let renamed=setup.rename_param("contract_p","contract_q").unwrap().unwrap();
            for key in ["edge","spacing","grid"] { assert_eq!(renamed["nest"][key],"contract_q * 1"); }
        } }
    }

    #[test]
    fn output_choice_defaults_to_files_and_stops_roundtrips_without_old_fields() {
        let mut nest=Nest::new(vec![NestPart { body:FeatureId(1),count:1,up:Up::PlusZ,rotate:NestRotate::Free }]);
        let json=serde_json::to_value(&nest).unwrap();assert!(json.get("output").is_none());
        assert_eq!(serde_json::from_value::<Nest>(json).unwrap().output,crate::sections::SectionOutput::Files);
        nest.output=crate::sections::SectionOutput::Stops;
        assert_eq!(nest,serde_json::from_value::<Nest>(serde_json::to_value(&nest).unwrap()).unwrap());
    }

    #[test]
    fn a_shadow_hole_is_reserved_instead_of_accepting_another_part() {
        let mut ring=rectangle(1,[30.0,30.0],1);ring.rotate=NestRotate::Fixed;ring.triangles.clear();
        for (x,y,w,h) in [(0.0,0.0,30.0,5.0),(0.0,25.0,30.0,5.0),(0.0,5.0,5.0,20.0),(25.0,5.0,5.0,20.0)] {
            ring.triangles.extend([[[x,y],[x+w,y],[x+w,y+h]],[[x,y],[x+w,y+h],[x,y+h]]]);
        }
        let mut i=input(vec![ring,rectangle(2,[10.0,10.0],1)]);
        i.sheet=[40.0,40.0];i.method=NestMethod::Shapes;i.spacing=0.0;i.max_sheets=1;
        let out=pack(&i);
        assert_eq!(out.sheets[0].parts.len(),1);
        assert_eq!(out.unplaced,vec![UnplacedPart { part:1,instance:1,reason:UnplacedReason::NoSheet }]);
    }
}
