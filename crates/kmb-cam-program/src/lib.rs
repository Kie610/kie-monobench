//! セットアップごとのプログラムの組み立て(文書の加工の経路を並べ、手動 NC・パターン・空振りの
//! 削除を当てる)、G-code の書き出し、シミュレーションのつなぎ、段取り表([`sheet`])。
//! 文書と登録表だけで決まり、画面の操作(kmb-cam-commands)を知らない。

use kmb_cam_setup::{SETUP, SetupInfo, is_rest_data, toolpath_of};
use kmb_cam_tool::Tool;
use kmb_post::{MachineProfile, Post};
use kmb_sim::{SimOptions, SimReport, ZMap};
use kmb_toolpath::{Manual, Program, Toolpath};

mod air;
#[cfg(test)]
mod sections_tests;
pub mod sheet;
pub use sheet::{setup_sheet, setup_sheet_html};

use kmb_core::{DocumentData, FeatureId, FeatureStatus, Kernel, RecomputeReport, Registry};

/// 加工の機能か(区分 `cam.operation` で登録された種類)。
pub fn is_operation(reg: &Registry, type_id: &str) -> bool {
    reg.is(type_id, kmb_core::category::OPERATION)
}

/// 経路 `SetupPaths::paths[i]` を作った加工の機能と、その条件。
#[derive(Debug, Clone, PartialEq)]
pub struct PathSource {
    pub id: FeatureId,
    pub type_id: String,
    /// 前の操作の削り残しを削る(残り削り。パターンなら元が残り削り)。並べ替えると結果が変わる
    pub rest: bool,
    /// 経路を作った時の切削条件(パターンや古い出力では `None`)
    pub cutting: Option<kmb_cam_tool::Cutting>,
    /// シミュレーションで荒削りの(粗い)格子で削る(加工の欄「シミュレーションの精度」。パターンなら元の加工)
    pub rough_grid: bool,
    /// 部品を切り抜く加工(戦略の印 [`kmb_cam_setup::CUTS_FREE`]。パターンなら元の加工)。工具の順に
    /// 並べ替える時に前へ動かさない
    pub cuts_free: bool,
    /// 前に削った所の空振りを省く(加工の欄「削った所の空振りを省く」)
    pub skip_air: bool,
    /// 空振りを省く前の経路(省いた時だけ)
    pub original: Option<std::sync::Arc<Toolpath>>,
    /// この経路は既に区画 1 の座標で作ってあり、区画ごとに切らない。
    pub section_fixed: bool,
    pub section_entry: Option<kmb_cam_setup::EntryPlan>,
}

/// 手動 NC の命令の 1 つ。`before` 番目の経路の前に入る(経路の数なら最後の加工の後)。
#[derive(Debug, Clone, PartialEq)]
pub struct ManualAt {
    pub before: usize,
    pub id: FeatureId,
    pub manual: Manual,
}

/// 1 つのセットアップと、その経路(タイムラインの順)。
#[derive(Debug, Clone)]
pub struct SetupPaths {
    pub setup: FeatureId,
    pub name: String,
    pub info: SetupInfo,
    pub paths: Vec<Toolpath>,
    /// `paths` と同じ順の、経路を作った機能。G-code を読み直した経路([`with_gcode_paths`])では
    /// 数が合わないことがある
    pub sources: Vec<PathSource>,
    /// 手動 NC の命令(`before` の小さい順。同じ所はタイムラインの順)。シミュレーションは使わない
    pub manual: Vec<ManualAt>,
    /// 経路を作れなかった加工の機能と、その理由
    pub failed: Vec<(FeatureId, String)>,
    /// 文書の設計の属性から作る、G-code の頭の注の行(部品名・部品番号など)
    pub part: Vec<String>,
    /// 空振りを省いた時の、始めの材料の高さの上限(裏返す前に削った形を含む)。加工を選んだり
    /// 並べ替えたりした時に省き直すのに使う
    pub air_start: Option<std::sync::Arc<ZMap>>,
    pub nest: Option<kmb_cam_setup::nest::NestLayout>,
    pub nest_names: std::collections::BTreeMap<FeatureId, String>,
}

impl SetupPaths {
    /// 1 から数える区画 `k` の経路を、区画 1 の座標へ動かす。
    pub fn section(&self, k: u32) -> SetupPaths {
        let Some(sections) = &self.info.sections else {
            return self.clone();
        };
        let Some(sec) = sections.list.iter().find(|s| s.index == k) else {
            let mut out = self.clone();
            out.paths.clear();
            out.sources.clear();
            out.manual.clear();
            out.info.sections = None;
            out.failed
                .push((self.setup, format!("区画 {k} がありません")));
            return out;
        };
        let mut out = self.clone();
        out.name = format!("{} {}", self.name, sec.label.replace(' ', ""));
        out.info.sections = None;
        out.info.stock_min = [
            sec.stock_min[0] + sec.shift[0],
            sec.stock_min[1] + sec.shift[1],
            sec.stock_min[2],
        ];
        out.info.stock_max = [
            sec.stock_max[0] + sec.shift[0],
            sec.stock_max[1] + sec.shift[1],
            sec.stock_max[2],
        ];
        let mut window = sec.clone();
        if sections.kind == kmb_cam_setup::sections::SectionKind::Sheets {
            let radius = self
                .paths
                .iter()
                .map(|p| p.tool.radius())
                .fold(0.0, f64::max);
            // 板の外で工具が材料に届かない区間は、仮想配置の空間だけを通る。
            for axis in 0..2 {
                window.cut.min[axis] = window.cut.min[axis].max(sec.stock_min[axis] - radius);
                window.cut.max[axis] = window.cut.max[axis].min(sec.stock_max[axis] + radius);
            }
        }
        for (i, tp) in out.paths.iter_mut().enumerate() {
            if self.sources.get(i).is_some_and(|s| s.section_fixed) {
                continue;
            }
            let entry = self.sources.get(i).and_then(|s| s.section_entry);
            let opts = kmb_cam_setup::ClipOpts {
                clearance_z: self.info.clearance_z,
                ramp_angle: entry
                    .map_or(kmb_cam_setup::DEFAULT_ENTRY_DEG.to_radians(), |e| e.angle),
                plunge: entry.map_or_else(
                    || {
                        self.sources
                            .get(i)
                            .and_then(|s| s.cutting.as_ref())
                            .map_or_else(|| tp.tool.cutting().plunge, |c| c.plunge)
                    },
                    |e| e.plunge_feed,
                ),
                ramp_feed: entry.map(|e| e.ramp_feed),
            };
            let (mut clipped, warnings) = kmb_cam_setup::clip_to_section(tp, &window, &opts);
            if kmb_cam_setup::avoid(&mut clipped, &self.info.keep_out) > 0 {
                out.part.push(
                    "区画へ移した経路が固定具を避けるため、固定具の下に材料が残ります".into(),
                );
            }
            *tp = clipped;
            out.part.extend(warnings);
        }
        out.air_start = None;
        out
    }

    /// 模擬用の経路。区画ごとの窓で切り、世界に並べた位置へ戻して連ねる。
    pub fn simulation_paths(&self) -> std::borrow::Cow<'_, [Toolpath]> {
        let Some(sections) = &self.info.sections else {
            return std::borrow::Cow::Borrowed(&self.paths);
        };
        let mut paths = self.paths.clone();
        for tp in &mut paths {
            tp.moves.clear();
            tp.dwells.clear();
            tp.spans.clear();
            tp.links = Default::default();
        }
        for sec in &sections.list {
            let section = self.section(sec.index);
            let xf = kmb_toolpath::Xform2::translate(-sec.shift[0], -sec.shift[1]);
            for (i, tp) in section.paths.into_iter().enumerate() {
                paths[i].moves.extend(tp.moves.iter().map(|m| xf.apply(m)));
                paths[i].dwells.extend(tp.dwells.into_iter().map(|mut d| {
                    d.at = xf.point(d.at);
                    d
                }));
            }
        }
        std::borrow::Cow::Owned(paths)
    }

    pub fn simulation_fixtures(&self) -> Vec<([f64; 3], [f64; 3])> {
        let Some(sections) = &self.info.sections else {
            return self.info.fixtures.clone();
        };
        sections
            .list
            .iter()
            .flat_map(|sec| {
                self.info.fixtures.iter().map(move |(lo, hi)| {
                    (
                        [lo[0] - sec.shift[0], lo[1] - sec.shift[1], lo[2]],
                        [hi[0] - sec.shift[0], hi[1] - sec.shift[1], hi[2]],
                    )
                })
            })
            .collect()
    }
    /// 空振りを省く前の経路に戻してから、今の順で省き直す(始めの形は前と同じ)。
    pub fn redo_skip_air(&mut self) {
        let Some(start) = self.air_start.clone() else {
            return;
        };
        air::restore(self);
        air::apply(self, Some((*start).clone()));
    }

    /// `k` 番目の加工が作ったままの経路(空振りを省いたなら省く前の経路)。
    pub fn generated(&self, k: usize) -> &Toolpath {
        self.sources
            .get(k)
            .and_then(|s| s.original.as_deref())
            .unwrap_or(&self.paths[k])
    }

    pub fn program(&self) -> Program {
        if let Some(sections) = &self.info.sections {
            let mut programs = sections
                .list
                .iter()
                .map(|s| (s, self.section(s.index).program()));
            if let Some((_, mut combined)) = programs.next() {
                for (s, p) in programs {
                    combined.ops.push(kmb_toolpath::Op::Message {
                        text: format!("{}の材料を置いて原点と固定具を確かめてください", s.label),
                    });
                    combined
                        .ops
                        .push(kmb_toolpath::Op::Stop { optional: false });
                    combined.ops.extend(p.ops);
                }
                combined.name = self.name.clone();
                return combined;
            }
        }
        let manual: Vec<(usize, Manual)> = self
            .manual
            .iter()
            .map(|m| (m.before, m.manual.clone()))
            .collect();
        let mut p = Program::build_with(
            &self.name,
            self.info.wcs,
            self.info.clearance_z,
            &self.paths,
            &manual,
        );
        // 部品名などの設計の属性、プログラム番号とコメントは、名前の次のコメントにする。
        // O 番号の行を書ける機械では、ポストが頭に O 番号の行も書く
        let mut head: Vec<kmb_toolpath::Op> = self
            .part
            .iter()
            .map(|t| kmb_toolpath::Op::Comment { text: t.clone() })
            .collect();
        if let Some(n) = self.info.program {
            head.push(kmb_toolpath::Op::Comment {
                text: format!("プログラム番号 O{n:04}"),
            });
        }
        for line in self.info.comment.lines().filter(|l| !l.trim().is_empty()) {
            head.push(kmb_toolpath::Op::Comment {
                text: line.trim().to_string(),
            });
        }
        if let Some(l) = self.info.leveling {
            match l.mode {
                kmb_cam_setup::LevelingMode::Gcode => p.leveling = self.level_grid(),
                kmb_cam_setup::LevelingMode::Controller => head.push(kmb_toolpath::Op::Comment {
                    text: format!(
                        "高さの補正: コントローラーで表面を {}×{} 点測ってから始める(この G-code では測らない)",
                        l.nx, l.ny
                    ),
                }),
                kmb_cam_setup::LevelingMode::None => {}
            }
        }
        let at = p.ops.len().min(1);
        p.ops.splice(at..at, head);
        p.number = self.info.program;
        p
    }

    /// 高さの補正で測る格子: 削る動き(送り)の XY の範囲を、決めた長さだけ内へ狭めたもの。
    /// 補正しないか、削る動きがなければ `None`。
    pub fn level_grid(&self) -> Option<kmb_toolpath::LevelGrid> {
        let l = self.info.leveling?;
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for m in self.paths.iter().flat_map(|p| &p.moves) {
            if matches!(m, kmb_toolpath::Move::Rapid { .. }) {
                continue;
            }
            let to = m.to();
            for k in 0..2 {
                lo[k] = lo[k].min(to[k]);
                hi[k] = hi[k].max(to[k]);
            }
        }
        if lo[0] > hi[0] {
            return None;
        }
        // 狭めると範囲がなくなる向きは、真ん中の 1 本にする
        let span = |k: usize| {
            let w = hi[k] - lo[k] - 2.0 * l.inset;
            if w > 0.0 {
                (lo[k] + l.inset, w)
            } else {
                ((lo[k] + hi[k]) / 2.0, 0.0)
            }
        };
        let ((x, width), (y, length)) = (span(0), span(1));
        Some(kmb_toolpath::LevelGrid {
            x,
            y,
            width,
            length,
            nx: l.nx,
            ny: l.ny,
            height: l.height,
        })
    }

    /// 機能 `ids` の経路と手動 NC だけを残したもの(一部の加工だけを書き出す)。
    /// 経路を作れなかった加工も、選んだものだけを残す。選んだものが 1 つもなければ誤り。
    pub fn only(&self, ids: &[FeatureId]) -> Result<SetupPaths, String> {
        if self.sources.len() != self.paths.len() {
            return Err("経路と機能の対応が分かりません".into());
        }
        let keep: Vec<usize> = (0..self.paths.len())
            .filter(|&i| ids.contains(&self.sources[i].id))
            .collect();
        let manual: Vec<ManualAt> = self
            .manual
            .iter()
            .filter(|m| ids.contains(&m.id))
            .map(|m| ManualAt {
                before: keep.iter().filter(|&&i| i < m.before).count(),
                ..m.clone()
            })
            .collect();
        let failed: Vec<(FeatureId, String)> = self
            .failed
            .iter()
            .filter(|(id, _)| ids.contains(id))
            .cloned()
            .collect();
        if keep.is_empty() && manual.is_empty() && failed.is_empty() {
            return Err(format!("{} に選んだ加工がありません", self.name));
        }
        let names: Vec<&str> = keep.iter().map(|&i| self.paths[i].name.as_str()).collect();
        let mut out = SetupPaths {
            name: format!("{}(一部: {})", self.name, names.join("、")),
            paths: keep.iter().map(|&i| self.paths[i].clone()).collect(),
            sources: keep.iter().map(|&i| self.sources[i].clone()).collect(),
            manual,
            failed,
            ..self.clone()
        };
        // 外した加工は削らないものとして、空振りを省き直す
        if keep.len() < self.paths.len() {
            out.redo_skip_air();
        }
        Ok(out)
    }

    /// 工具の番号の順に並べ替えたもの(同じ工具の中はタイムラインの順)と、並べ替えの前と後の
    /// 工具交換の回数。並べ替えると結果が変わる時は、並べ替えずに理由を返す:
    /// 残り削り(前の操作の削り残しに依る)、手動 NC(タイムラインの位置に意味がある)、
    /// 部品を切り抜く加工(輪郭など)がほかの加工より前に動く(後の加工の時に部品が外れているおそれ)。
    /// 並べるのは工具の番号だけで、工具の摩耗やマガジンの配置は見ない(docs/避ける形.md)。
    pub fn by_tool(&self) -> Result<(SetupPaths, usize, usize), String> {
        if self.sources.len() != self.paths.len() {
            return Err("経路と機能の対応が分かりません".into());
        }
        if let Some(m) = self.manual.first() {
            return Err(format!(
                "手動 NC({})の位置が変わるので並べ替えません",
                m.manual.describe()
            ));
        }
        if let Some(i) = self.sources.iter().position(|s| s.rest) {
            return Err(format!(
                "{} は前の加工の削り残しを削る(残り削り)ので、並べ替えると結果が変わります。並べ替えません",
                self.paths[i].name
            ));
        }
        let mut order: Vec<usize> = (0..self.paths.len()).collect();
        order.sort_by_key(|&i| self.paths[i].tool.number);
        for (pos, &i) in order.iter().enumerate() {
            if !self.sources[i].cuts_free {
                continue;
            }
            // 元は輪郭より前だったのに、並べ替えで後ろへ回る加工
            if let Some(&j) = order[pos + 1..].iter().find(|&&j| j < i) {
                return Err(format!(
                    "輪郭 {} が {} より前に動き、後の加工の時に部品が切り抜かれているおそれがあるので並べ替えません",
                    self.paths[i].name, self.paths[j].name
                ));
            }
        }
        let paths: Vec<Toolpath> = order.iter().map(|&i| self.paths[i].clone()).collect();
        let (before, after) = (tool_changes(&self.paths), tool_changes(&paths));
        let mut out = SetupPaths {
            name: format!("{}(工具の順)", self.name),
            sources: order.iter().map(|&i| self.sources[i].clone()).collect(),
            paths,
            ..self.clone()
        };
        // 削る順が変わるので、空振りを省き直す
        out.redo_skip_air();
        Ok((out, before, after))
    }
}

/// 経路を順に削る時の工具交換の回数(最初の 1 本を含む)。
pub fn tool_changes(paths: &[Toolpath]) -> usize {
    let mut n = 0;
    let mut last = None;
    for p in paths.iter().filter(|p| !p.moves.is_empty()) {
        if last != Some(p.tool.number) {
            n += 1;
            last = Some(p.tool.number);
        }
    }
    n
}

/// 全部のセットアップを 1 本のプログラムにまとめる(セットアップの間に停止と付け替えのコメント)。
/// 経路を作れなかった加工があるセットアップがあれば作らない。加工のないセットアップは飛ばす。
pub fn chain_program(all: &[SetupPaths]) -> Result<Program, String> {
    if let Some((s, (id, m))) = all.iter().find_map(|s| s.failed.first().map(|f| (s, f))) {
        return Err(format!("{} の {id} の経路ができていません: {m}", s.name));
    }
    let used: Vec<&SetupPaths> = all.iter().filter(|s| !s.paths.is_empty()).collect();
    if used.is_empty() {
        return Err("加工がありません".into());
    }
    let parts: Vec<Program> = used.iter().map(|s| s.program()).collect();
    let names: Vec<&str> = used.iter().map(|s| s.name.as_str()).collect();
    Program::chain(&format!("全セットアップ: {}", names.join("、")), &parts)
}

/// まとめて書き出すセットアップの機械の ID。機械の違うセットアップは 1 本にできない。
pub fn chain_machine(all: &[SetupPaths]) -> Result<MachineProfile, String> {
    let mut used = all.iter().filter(|s| !s.paths.is_empty());
    let first = used.next().ok_or("加工がありません")?;
    // 同じ機械でも、写した値が違えば(設定を直す前と後に足したセットアップ)まとめない
    if let Some(s) = used.find(|s| s.info.machine_profile() != first.info.machine_profile()) {
        return Err(format!(
            "{} と {} は機械が違うので、1 本にまとめられません",
            first.name, s.name
        ));
    }
    Ok(first.info.machine_profile().clone())
}

/// G-code の頭の注に入れる部品の行。材料は、削るボディに付けた材料(部品表と同じ)を先に使い、
/// 付けていない時だけ設計の属性の材料を書く(2 つが違う材料を出さないように)。
fn part_lines(data: &DocumentData, reg: &Registry, setup: FeatureId) -> Vec<String> {
    let mut props = data.properties.clone();
    // セットアップの最初の入力が削るボディなので、セットアップからたどれば元のボディに着く
    let root = kmb_core::body_root(data, reg, setup);
    if let Some(m) = kmb_look::material::of(data).get(root) {
        props.material = m.to_string();
    }
    props.gcode_lines()
}

/// 文書のセットアップごとに経路を集める。
pub fn setups(data: &DocumentData, report: &RecomputeReport, reg: &Registry) -> Vec<SetupPaths> {
    let mut out: Vec<SetupPaths> = Vec::new();
    for r in &report.features {
        let Some(entry) = data.feature(r.id) else {
            continue;
        };
        if entry.type_id == SETUP {
            if let FeatureStatus::Ok { outputs, .. } = &r.status
                && let Some(info) = outputs.get("setup").and_then(SetupInfo::from_value)
            {
                let nest = outputs
                    .get("nest")
                    .and_then(<kmb_cam_setup::nest::NestLayout as kmb_core::Payload>::from_value);
                let ids: std::collections::BTreeSet<_> = nest
                    .iter()
                    .flat_map(|n| n.sheets.iter())
                    .flat_map(|s| s.parts.iter().map(|p| p.body))
                    .collect();
                let nest_names = ids
                    .into_iter()
                    .map(|id| (id, part_label(data, reg, id)))
                    .collect();
                out.push(SetupPaths {
                    setup: r.id,
                    name: entry.name.clone(),
                    info,
                    paths: vec![],
                    sources: vec![],
                    manual: vec![],
                    failed: vec![],
                    part: part_lines(data, reg, r.id),
                    air_start: None,
                    nest,
                    nest_names,
                });
            }
            continue;
        }
        if !is_operation(reg, &entry.type_id) {
            continue;
        }
        let setup = entry
            .data
            .get("setup")
            .and_then(|v| serde_json::from_value::<FeatureId>(v.clone()).ok());
        let Some(s) = out.iter_mut().find(|s| Some(s.setup) == setup) else {
            continue;
        };
        match &r.status {
            FeatureStatus::Ok { outputs, .. } => {
                if let Some((_, mut tp)) = outputs.get("toolpath").and_then(toolpath_of) {
                    tp.name = entry.name.clone();
                    s.paths.push(tp);
                    s.sources.push(PathSource {
                        id: r.id,
                        type_id: entry.type_id.clone(),
                        rest: is_rest(data, &entry.data),
                        cutting: outputs.get("cutting").and_then(kmb_cam_setup::cutting_of),
                        rough_grid: is_rough_grid(data, entry, reg),
                        cuts_free: reg
                            .has_mark(&origin(data, entry).type_id, kmb_cam_setup::CUTS_FREE),
                        skip_air: kmb_cam_setup::AirCuts::of(&entry.data).is_skip(),
                        original: None,
                        section_fixed: outputs.contains_key(kmb_cam_setup::sections::SECTION_FIXED),
                        section_entry: outputs.get(kmb_cam_setup::sections::SECTION_ENTRY).and_then(<kmb_cam_setup::sections::SectionEntry as kmb_core::Payload>::from_value).map(|e|e.0),
                    });
                }
                if let Some((_, manual)) =
                    outputs.get("manual_nc").and_then(kmb_cam_setup::manual_of)
                {
                    // 「この加工の後」を決めていれば、その加工の後。なければタイムラインの位置
                    let after = entry
                        .data
                        .get("after")
                        .and_then(|v| serde_json::from_value::<FeatureId>(v.clone()).ok())
                        .and_then(|a| s.sources.iter().position(|p| p.id == a));
                    s.manual.push(ManualAt {
                        before: after.map_or(s.paths.len(), |i| i + 1),
                        id: r.id,
                        manual,
                    });
                }
            }
            FeatureStatus::Error { message } => s.failed.push((r.id, message.clone())),
            FeatureStatus::Blocked { by, .. } => {
                s.failed.push((r.id, format!("{by} が使えません")))
            }
            _ => {}
        }
    }
    for s in &mut out {
        // 安定な並べ替えなので、同じ所の命令はタイムラインの順のまま
        s.manual.sort_by_key(|m| m.before);
    }
    skip_air_all(&mut out);
    out
}

/// 表示と段取り表で使う、現在のボディの名前。評価の派生出力には保存しない。
pub fn part_label(data: &DocumentData, reg: &Registry, body: FeatureId) -> String {
    kmb_core::body_labels(data, reg, &[body])
        .pop()
        .unwrap_or_else(|| body.to_string())
}

/// 全部のセットアップの空振りを、タイムラインの順に省く。同じボディで同じ大きさのストックの
/// 前のセットアップがあれば、その削った後の形から始める(シミュレーションと同じ引き継ぎ方)。
fn skip_air_all(all: &mut [SetupPaths]) {
    let mut ends: Vec<Option<ZMap>> = Vec::with_capacity(all.len());
    for k in 0..all.len() {
        // 失敗した加工があるセットアップは、経路が欠けているので省かない(後へも引き継がない)
        if !all[k].failed.is_empty() || all[k].paths.is_empty() {
            ends.push(None);
            continue;
        }
        let start = (0..k).rev().find_map(|j| {
            let m = ends[j].as_ref()?;
            same_stock(&all[j], &all[k])
                .then(|| air::dilate(&carried_stock(&all[j].info, m, &all[k].info, m.cell)))
        });
        let end = air::apply(&mut all[k], start);
        ends.push(Some(end));
    }
}

/// 残り削りか(前の工具を番号か直径で打っている。パターンなら元の加工が残り削り)。
fn is_rest(data: &DocumentData, d: &serde_json::Value) -> bool {
    if is_rest_data(d) {
        return true;
    }
    d.get("source")
        .and_then(|v| serde_json::from_value::<FeatureId>(v.clone()).ok())
        .and_then(|id| data.feature(id))
        .is_some_and(|f| f.type_id != SETUP && is_rest(data, &f.data))
}

/// 加工 `entry` の元の加工(パターンなら、たどった先の元の加工。ほかは自分)。
fn origin<'d>(
    data: &'d DocumentData,
    entry: &'d kmb_core::FeatureEntry,
) -> &'d kmb_core::FeatureEntry {
    let mut e = entry;
    for _ in 0..data.features.len() {
        if e.type_id != kmb_cam_setup::PATTERN {
            break;
        }
        match e
            .data
            .get("source")
            .and_then(|v| serde_json::from_value::<FeatureId>(v.clone()).ok())
            .and_then(|id| data.feature(id))
        {
            Some(src) => e = src,
            None => break,
        }
    }
    e
}

/// シミュレーションで粗い格子で削る加工か。パターンは元の加工の欄で決める。「シミュレーションの精度」が
/// 自動なら、戦略が荒削りと名乗った種類(印 [`kmb_cam_setup::ROUGH_GRID`]。荒削りの加工:
/// 負荷一定・ポケット・フェイス・3D の荒削り)。後の加工で仕上げる前提なので、粗い格子でも当たりと
/// 食い込みの見落としが少ない。
fn is_rough_grid(data: &DocumentData, entry: &kmb_core::FeatureEntry, reg: &Registry) -> bool {
    let e = origin(data, entry);
    match kmb_cam_setup::SimGrid::of(&e.data) {
        kmb_cam_setup::SimGrid::Auto => reg.has_mark(&e.type_id, kmb_cam_setup::ROUGH_GRID),
        kmb_cam_setup::SimGrid::Rough => true,
        kmb_cam_setup::SimGrid::Fine => false,
    }
}

/// 経路ごとに削る格子の大きさ(荒削りは `opt.rough()`、ほかは `opt.cell`)。G-code を読み直して
/// 経路の数が機能と合わない時は、全部を細かい格子にする。
pub fn path_cells(s: &SetupPaths, opt: &SimOptions) -> Vec<f64> {
    let aligned = s.sources.len() == s.paths.len();
    (0..s.paths.len())
        .map(|k| {
            if aligned && s.sources[k].rough_grid {
                opt.rough()
            } else {
                opt.cell
            }
        })
        .collect()
}

/// G-code を作る。加工の機能に失敗したものがあれば作らない。
pub fn gcode(s: &SetupPaths, post: &dyn Post, machine: &MachineProfile) -> Result<String, String> {
    if let Some((id, m)) = s.failed.first() {
        return Err(format!("{id} の経路ができていません: {m}"));
    }
    if s.paths.is_empty() {
        return Err(format!("{} に加工がありません", s.name));
    }
    post.emit(&s.program(), machine).map_err(|e| e.to_string())
}

/// 区画のないセットアップは 1 ファイル、区画があれば指定の分け方で書く。
pub fn gcode_files(
    s: &SetupPaths,
    post: &dyn Post,
    machine: &MachineProfile,
) -> Result<Vec<(String, String)>, String> {
    use kmb_cam_setup::sections::SectionOutput;
    if let Some(sections) = &s.info.sections {
        // 機械の範囲の点検は、区画 1 へ移した経路ごとに行う。
        for sec in &sections.list {
            let p = s.section(sec.index).program();
            let errors: Vec<_> = post
                .check(&p, machine)
                .into_iter()
                .filter(|d| d.severity == kmb_post::Severity::Error)
                .map(|d| d.message)
                .collect();
            if !errors.is_empty() {
                return Err(errors.join("、"));
            }
        }
        if sections.output == SectionOutput::Files {
            return sections
                .list
                .iter()
                .map(|sec| {
                    let section = s.section(sec.index);
                    Ok((
                        section_file_name(&s.name, Some(&sec.label), post.id()),
                        gcode(&section, post, machine)?,
                    ))
                })
                .collect();
        }
    }
    Ok(vec![(
        section_file_name(&s.name, None, post.id()),
        gcode(s, post, machine)?,
    )])
}

/// 書き出しと段取り表で共用する、区画のファイル名。
pub fn section_file_name(setup: &str, section: Option<&str>, post: &str) -> String {
    let clean = |s: &str| {
        let name: String = s
            .chars()
            .filter(|c| !c.is_whitespace() && !r#"\/:*?"<>|"#.contains(*c))
            .collect();
        if name.is_empty() {
            "setup".into()
        } else {
            name
        }
    };
    let stem = match section {
        Some(label) => format!("{} {}", clean(setup), clean(label)),
        None => clean(setup),
    };
    format!("{stem}.{}", kmb_post::file_extension(post))
}

/// 板の間の空間には材料を置かず、区画の箱の和を始めの材料にする。
pub fn initial_stock(info: &SetupInfo, cell: f64) -> ZMap {
    let mut stock = ZMap::stock(info.stock_min, info.stock_max, cell);
    if let Some(sections) = &info.sections {
        for y in 0..stock.ny {
            for x in 0..stock.nx {
                let p = stock.center(x, y);
                stock.z[y * stock.nx + x] = sections
                    .list
                    .iter()
                    .filter(|s| {
                        p[0] >= s.stock_min[0]
                            && p[0] < s.stock_max[0]
                            && p[1] >= s.stock_min[1]
                            && p[1] < s.stock_max[1]
                    })
                    .map(|s| s.stock_max[2])
                    .fold(info.stock_min[2], f64::max);
            }
        }
    }
    stock
}

/// 途中の形の粗い格子 `map`(セットアップの座標)を、3D のビューに出す網(世界座標)にする。
pub fn stage_shape(info: &SetupInfo, map: &ZMap, name: &str, key: u64) -> kmb_core::OverlayShape {
    let mesh = world_mesh(info, map.to_mesh(info.stock_min[2]));
    kmb_core::OverlayShape {
        key,
        label: if name == "素材" {
            "素材".into()
        } else {
            format!("{name} の後")
        },
        mesh,
        ..Default::default()
    }
}

/// セットアップの座標の網を世界座標にする(原点を足し、裏返したセットアップは部品の向きへ戻す)。
pub fn world_mesh(info: &SetupInfo, mut m: kmb_core::Mesh) -> kmb_core::Mesh {
    for p in &mut m.positions {
        for (v, o) in p.iter_mut().zip(info.origin) {
            *v += o as f32;
        }
    }
    info.placed_mesh_inverse(&m)
}

/// 書き出した G-code `gcode` を読み直した経路にしたセットアップ(G-code で削って確かめるため)。
/// 面取りで意図して縁を削る深さは、同じ名前の経路から写す。
pub fn with_gcode_paths(s: &SetupPaths, gcode: &str) -> Result<SetupPaths, String> {
    let program = s.program();
    let tools: Vec<Tool> = program.tools().into_iter().cloned().collect();
    let mut paths = kmb_gcode_read::read(
        gcode,
        &kmb_gcode_read::ReadOptions {
            tools: &tools,
            safe_z: s.info.safe_z,
            retract_z: s.info.retract_z,
        },
    )
    .map_err(|e| format!("G-code を読み直せません: {e}"))?;
    // 読み直した経路は書いた順。数が合えば順で、合わなければ名前で元の経路と結ぶ
    let same_count = paths.len() == s.paths.len();
    for (k, p) in paths.iter_mut().enumerate() {
        let o = if same_count {
            s.paths.get(k)
        } else {
            s.paths.iter().find(|o| o.name == p.name)
        };
        if let Some(o) = o {
            p.edge_break = o.edge_break;
        }
    }
    Ok(SetupPaths { paths, ..s.clone() })
}

/// 区画ごとに機械へ送る座標で書いて読み直し、模擬用の仮想配置の位置へ戻す。
pub fn with_section_gcode_paths(
    s: &SetupPaths,
    post: &dyn Post,
    machine: &MachineProfile,
) -> Result<SetupPaths, String> {
    let Some(sections) = &s.info.sections else {
        return with_gcode_paths(s, &gcode(s, post, machine)?);
    };
    let mut out = s.clone();
    out.sources.clear();
    for tp in &mut out.paths {
        tp.moves.clear();
        tp.dwells.clear();
        tp.spans.clear();
        tp.links = Default::default();
    }
    for sec in &sections.list {
        let section = s.section(sec.index);
        let read = with_gcode_paths(&section, &gcode(&section, post, machine)?)?;
        let xf = kmb_toolpath::Xform2::translate(-sec.shift[0], -sec.shift[1]);
        let same_count = read.paths.len() == s.paths.len();
        for (k, tp) in read.paths.into_iter().enumerate() {
            let index = if same_count {
                Some(k)
            } else {
                s.paths.iter().position(|p| p.name == tp.name)
            }
            .ok_or("読み直した区画の経路と加工の対応が分かりません")?;
            out.paths[index]
                .moves
                .extend(tp.moves.iter().map(|m| xf.apply(m)));
            out.paths[index]
                .dwells
                .extend(tp.dwells.into_iter().map(|mut d| {
                    d.at = xf.point(d.at);
                    d
                }));
        }
    }
    // 読み直すと経路の数が変わり得る。役目や元の経路で結ばず、細かい格子で確かめる。
    out.air_start = None;
    Ok(out)
}

/// セットアップの経路でストックを削ってみる。加工の機能に失敗したものがあれば行わない
/// (経路が欠けたまま「問題なし」に見えないように)。
pub fn simulate(kernel: &dyn Kernel, s: &SetupPaths, opt: SimOptions) -> Result<SimReport, String> {
    simulate_from(kernel, s, None, opt)
}

/// 前のセットアップから続けて削る。`setups[index]` より前の、同じボディで同じ大きさの
/// ストックのセットアップを順に削り、その後の形を次のセットアップの向きで引き継ぐ
/// (1 回目で切り抜いた所は、2 回目では初めから空いている)。
pub fn simulate_chain(
    kernel: &dyn Kernel,
    setups: &[SetupPaths],
    index: usize,
    opt: SimOptions,
) -> Result<SimReport, String> {
    let target = setups.get(index).ok_or("セットアップがありません")?;
    let start = carried_start(kernel, setups, index, opt)?;
    simulate_from(kernel, target, start, opt)
}

/// 同じボディで同じ大きさのストックのセットアップか(前の削った形を引き継げるか)。
fn same_stock(a: &SetupPaths, b: &SetupPaths) -> bool {
    let size = |s: &SetupPaths| [0, 1, 2].map(|k| s.info.stock_max[k] - s.info.stock_min[k]);
    // 横倒しに置いたセットアップは、上から削った柱が横向きになるので引き継がない
    a.info.body == b.info.body
        && a.info.keeps_vertical()
        && b.info.keeps_vertical()
        && size(a)
            .iter()
            .zip(size(b))
            .all(|(x, y)| (x - y).abs() < 1e-3)
}

/// `setups[index]` の始めのストック。前に同じストックのセットアップがあれば、それらを順に削った
/// 後の形を引き継ぐ(なければ `None` で、ストックの箱から)。
fn carried_start(
    kernel: &dyn Kernel,
    setups: &[SetupPaths],
    index: usize,
    opt: SimOptions,
) -> Result<Option<ZMap>, String> {
    let target = setups.get(index).ok_or("セットアップがありません")?;
    let mut carry: Option<(&SetupPaths, ZMap)> = None;
    for s in setups[..index].iter().filter(|s| same_stock(s, target)) {
        let start = carry
            .as_ref()
            .map(|(p, m)| carried_stock(&p.info, m, &s.info, opt.cell));
        let r = simulate_from(kernel, s, start, opt)?;
        carry = Some((s, r.result));
    }
    Ok(carry.map(|(p, m)| carried_stock(&p.info, &m, &target.info, opt.cell)))
}

/// 動きの単位の再生を、`setups[index]` について格子の大きさ `cell` で用意する
/// (前のセットアップから続けるなら、その削った形から始める)。
pub fn player(
    kernel: &dyn Kernel,
    setups: &[SetupPaths],
    index: usize,
    cell: f64,
    motion: &kmb_toolpath::Motion,
) -> Result<kmb_sim::Player, String> {
    let target = setups.get(index).ok_or("セットアップがありません")?;
    if let Some((id, m)) = target.failed.first() {
        return Err(format!("{id} の経路ができていません: {m}"));
    }
    let opt = SimOptions {
        cell,
        ..SimOptions::default()
    };
    let stock = carried_start(kernel, setups, index, opt)?
        .unwrap_or_else(|| initial_stock(&target.info, cell));
    let mesh = if target.info.sections.is_some() {
        target.info.part_mesh(kernel).map_err(|e| e.to_string())?
    } else {
        kernel
            .mesh(target.info.body, target.info.tolerance)
            .map_err(|e| e.to_string())?
    };
    let part = ZMap::from_mesh(
        &stock,
        &target.info.placed_mesh(&mesh),
        target.info.origin,
        f64::NEG_INFINITY,
    );
    Ok(kmb_sim::Player::new(
        stock,
        target.info.stock_min[2],
        part,
        target.simulation_paths().into_owned(),
        motion,
    ))
}

/// 前のセットアップ `prev` で削った後の高さ `map` を、次のセットアップ `next` の向きで見た
/// ストックの高さにする。上から削った形は縦の柱の集まりなので、裏返しても柱のままで、
/// 柱の上端が次の上面になる(柱がない所は、ストックの底)。
pub fn carried_stock(prev: &SetupInfo, map: &ZMap, next: &SetupInfo, cell: f64) -> ZMap {
    let mut out = ZMap::stock(next.stock_min, next.stock_max, cell);
    let bottom = prev.stock_min[2];
    // 世界座標 → 前のセットアップの作業座標
    let to_prev = |w: [f64; 3]| prev.to_setup(w);
    let to_next = |w: [f64; 3]| next.to_setup(w)[2];
    for j in 0..out.ny {
        for i in 0..out.nx {
            let c = out.center(i, j);
            let q = to_prev(next.to_world([c[0], c[1], 0.0]));
            let pi = ((q[0] - map.origin[0]) / map.cell).floor();
            let pj = ((q[1] - map.origin[1]) / map.cell).floor();
            let k = j * out.nx + i;
            let inside = pi >= 0.0 && pj >= 0.0 && (pi as usize) < map.nx && (pj as usize) < map.ny;
            let h = if inside {
                map.z[pj as usize * map.nx + pi as usize]
            } else {
                f64::NEG_INFINITY
            };
            out.z[k] = if h > bottom + 1e-6 {
                let a = to_next(prev.to_world([q[0], q[1], bottom]));
                let b = to_next(prev.to_world([q[0], q[1], h]));
                a.max(b).min(next.stock_max[2])
            } else {
                next.stock_min[2]
            };
        }
    }
    out
}

fn simulate_from(
    kernel: &dyn Kernel,
    s: &SetupPaths,
    start: Option<ZMap>,
    opt: SimOptions,
) -> Result<SimReport, String> {
    if let Some((id, m)) = s.failed.first() {
        return Err(format!("{id} の経路ができていません: {m}"));
    }
    let mesh = s.info.part_mesh(kernel).map_err(|e| e.to_string())?;
    let paths = s.simulation_paths();
    let cells = path_cells(s, &opt);
    // 始めの格子は最初の経路の大きさ(途中で大きさが変わる所で並べ直す)
    let first = cells.first().copied().unwrap_or(opt.cell);
    let mut stock = initial_stock(&s.info, first);
    // ボディをストックにしたセットアップは、そのボディの上面の形から削り始める
    if let Some(h) = s.info.stock_body {
        let m = kernel
            .mesh(h, s.info.tolerance)
            .map_err(|e| e.to_string())?;
        stock = ZMap::from_mesh(
            &stock,
            &s.info.placed_mesh(&m),
            s.info.origin,
            s.info.stock_min[2],
        );
    }
    let stock = start.unwrap_or(stock);
    let placed = s.info.placed_mesh(&mesh);
    let part_at = |g: &ZMap| ZMap::from_mesh(g, &placed, s.info.origin, f64::NEG_INFINITY);
    let machine = machine_shape(&s.info);
    let mut report = kmb_sim::simulate_on_machine(
        &stock,
        s.info.stock_min[2],
        &part_at,
        &paths,
        &cells,
        opt,
        &s.simulation_fixtures(),
        if s.info.sections.is_none() {
            machine.as_ref()
        } else {
            None
        },
    );
    if let Some(sections) = &s.info.sections
        && machine.is_some()
    {
        // 材料の形は仮想配置で削り、機械の当たりは各区画を実機の座標へ戻して確かめる。
        // ここも同じ既存の検査を実行し、仮想配置の幅を機械の travel として扱わない。
        for sec in &sections.list {
            let local = s.section(sec.index);
            let mut local_stock = initial_stock(&local.info, stock.cell);
            for y in 0..local_stock.ny {
                for x in 0..local_stock.nx {
                    let p = local_stock.center(x, y);
                    // 移動がセルの幅の整数倍でなければ、重なるセルの高い側を採る。
                    let span = |axis: usize, n: usize| {
                        let q = (p[axis] - sec.shift[axis] - stock.origin[axis]) / stock.cell;
                        (
                            ((q - 0.5 + 1e-9).floor().max(0.0) as usize).min(n),
                            ((q + 0.5 - 1e-9).ceil().max(0.0) as usize).min(n),
                        )
                    };
                    let (x0, x1) = span(0, stock.nx);
                    let (y0, y1) = span(1, stock.ny);
                    let mut height = local.info.stock_min[2];
                    for j in y0..y1 {
                        for i in x0..x1 {
                            height = height.max(stock.z[j * stock.nx + i]);
                        }
                    }
                    local_stock.z[y * local_stock.nx + x] = height;
                }
            }
            let mut local_mesh = placed.clone();
            for p in &mut local_mesh.positions {
                p[0] += sec.shift[0] as f32;
                p[1] += sec.shift[1] as f32;
            }
            let local_part =
                |g: &ZMap| ZMap::from_mesh(g, &local_mesh, local.info.origin, f64::NEG_INFINITY);
            let checked = kmb_sim::simulate_on_machine(
                &local_stock,
                local.info.stock_min[2],
                &local_part,
                &local.paths,
                &path_cells(&local, &opt),
                SimOptions {
                    stage_cells: 0,
                    ..opt
                },
                &local.info.fixtures,
                machine_shape(&local.info).as_ref(),
            );
            report.bed_depth = report.bed_depth.max(checked.bed_depth);
            report
                .machine_hits
                .extend(checked.machine_hits.into_iter().map(|mut hit| {
                    hit.event.at[0] -= sec.shift[0];
                    hit.event.at[1] -= sec.shift[1];
                    hit.event.op = format!("{} / {}", sec.label, hit.event.op);
                    hit
                }));
        }
    }
    Ok(report)
}

/// セットアップの機械の形(WCS)。ストックはベッドの上の、L 字の基準の角から `bed_offset` の所に
/// 置く。機械の値に形が無ければ `None`(模擬は機械との当たりを見ない)。
pub fn machine_shape(info: &SetupInfo) -> Option<kmb_sim::MachineShape> {
    let m = info.machine_profile();
    let f = m.frame.as_ref()?;
    let fence = info.fence_corner();
    let top = fence[2];
    // ベッドの左手前の角(L 字の角はベッドの角から `origin`。ベッドが分からなければ L 字の角)
    let o = m.bed.as_ref().map_or([0.0; 2], |b| b.origin);
    let corner = [fence[0] - o[0], fence[1] - o[1]];
    let lo = [
        corner[0] + f.travel_corner[0],
        corner[1] + f.travel_corner[1],
    ];
    Some(kmb_sim::MachineShape {
        travel: (lo, [lo[0] + m.travel[0], lo[1] + m.travel[1]]),
        nose: [top + f.nose_low, top + f.nose_low + m.travel[2]],
        spindle: f.spindle,
        carriage: f.carriage,
        bed: m.bed.as_ref().map(|b| {
            (
                [corner[0], corner[1], top - b.size[2]],
                [corner[0] + b.size[0], corner[1] + b.size[1], top],
            )
        }),
        bed_allowance: f.bed_allowance,
        clearance: (f.clearance > 0.0).then_some(top + f.clearance),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_cam_setup::Flip;
    use kmb_cam_tool::ToolLibrary;
    use kmb_post_carvera_air::CarveraAir;
    use kmb_strategy_chamfer::CHAMFER;
    use kmb_strategy_contour2d::CONTOUR;
    use kmb_strategy_drill::DRILL;
    use kmb_strategy_face::FACE;
    use kmb_strategy_pocket2d::POCKET;

    pub(crate) fn info(flip: Flip) -> SetupInfo {
        SetupInfo {
            body: kmb_core::ShapeHandle(1),
            origin: [0.0, 0.0, 5.0],
            stock_min: [0.0, 0.0, -5.0],
            stock_max: [10.0, 10.0, 0.0],
            safe_z: 5.0,
            retract_z: 1.0,
            clearance_z: 20.0,
            wcs: 1,
            material: String::new(),
            flip,
            flip_center: [5.0, 5.0, 2.5],
            tolerance: kmb_cam_setup::MESH_TOLERANCE,
            keep_out: vec![],
            fixtures: vec![],
            rot: None,
            more_bodies: vec![],
            part_min: [0.0; 3],
            part_max: [10.0, 10.0, 0.0],
            stock_body: None,
            program: None,
            comment: String::new(),
            bed_offset: [0.0; 2],
            machine: kmb_post_carvera_air::ID.into(),
            machine_values: kmb_post_carvera_air::profile(),
            leveling: None,
            sections: None,
        }
    }

    #[test]
    fn a_cut_through_area_stays_open_after_flipping() {
        let prev = info(Flip::None);
        let mut map = ZMap::stock(prev.stock_min, prev.stock_max, 0.5);
        for j in 0..map.ny {
            for i in 0..map.nx {
                // 左の 2 mm は切り抜き、ほかは上から 2 mm 削った
                let x = map.center(i, j)[0];
                map.z[j * map.nx + i] = if x < 2.0 { -5.0 } else { -2.0 };
            }
        }
        // 左右を裏返すと、切り抜いた所は右の 2 mm になり、ほかは平らな底面が上になる
        let next = info(Flip::AroundY);
        let out = carried_stock(&prev, &map, &next, 0.5);
        for j in 0..out.ny {
            for i in 0..out.nx {
                let x = out.center(i, j)[0];
                let z = out.z[j * out.nx + i];
                let want = if x > 8.0 { -5.0 } else { 0.0 };
                assert!((z - want).abs() < 1e-9, "x {x}: {z}");
            }
        }
        // 裏返さなければ、そのままの高さ
        let same = carried_stock(&prev, &map, &info(Flip::None), 0.5);
        assert_eq!(same.z, map.z);
    }

    /// 工具 `tool` で x の位置に 1 本の溝を掘る経路。
    pub(crate) fn slot(name: &str, tool: u32, x: f64) -> Toolpath {
        let lib = ToolLibrary::starter();
        let t = lib
            .tools
            .iter()
            .find(|t| t.number == tool)
            .unwrap_or(&lib.tools[0])
            .clone();
        Toolpath {
            name: name.into(),
            tool: t,
            rpm: 10000.0,
            safe_z: 5.0,
            retract_z: 1.0,
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
            moves: vec![
                kmb_toolpath::Move::Rapid { to: [x, 5.0, 5.0] },
                kmb_toolpath::Move::Linear {
                    to: [x, 5.0, -1.0],
                    feed: 200.0,
                },
                kmb_toolpath::Move::Linear {
                    to: [x, 8.0, -1.0],
                    feed: 600.0,
                },
                kmb_toolpath::Move::Rapid { to: [x, 8.0, 5.0] },
            ],
        }
    }

    /// 加工 `ops`((名前, 工具, 種類))を順に持つセットアップ。機能の番号は 101 から順。
    pub(crate) fn setup_of(ops: &[(&str, u32, &str)]) -> SetupPaths {
        SetupPaths {
            setup: FeatureId(100),
            name: "セットアップ 1".into(),
            info: info(Flip::None),
            paths: ops
                .iter()
                .enumerate()
                .map(|(i, (n, t, _))| slot(n, *t, 1.0 + i as f64))
                .collect(),
            sources: ops
                .iter()
                .enumerate()
                .map(|(i, (_, _, ty))| PathSource {
                    id: FeatureId(101 + i as u64),
                    type_id: ty.to_string(),
                    rest: false,
                    cutting: None,
                    rough_grid: false,
                    cuts_free: *ty == CONTOUR,
                    skip_air: true,
                    original: None,
                    section_fixed: false,
                    section_entry: None,
                })
                .collect(),
            manual: vec![],
            failed: vec![],
            part: vec![],
            air_start: None,
            nest: None,
            nest_names: Default::default(),
        }
    }

    fn changes(p: &Program) -> usize {
        p.ops
            .iter()
            .filter(|o| matches!(o, kmb_toolpath::Op::ToolChange { .. }))
            .count()
    }

    /// 受け入れ (a): 2 つの加工の間の停止は、主軸の停止と退避の後に M600、再開の後に主軸を回し直す。
    #[test]
    fn a_manual_stop_between_two_operations_is_written_safely() {
        let mut s = setup_of(&[("ポケット", 1, POCKET), ("外形", 1, CONTOUR)]);
        s.manual.push(ManualAt {
            before: 1,
            id: FeatureId(200),
            manual: Manual::Stop {
                optional: false,
                text: "クランプを付け替える".into(),
            },
        });
        let g = gcode(&s, &CarveraAir, &kmb_post_carvera_air::profile()).unwrap();
        let lines: Vec<&str> = g.lines().collect();
        let at = lines.iter().position(|l| *l == "M600").expect(&g);
        let pocket = lines.iter().position(|l| *l == "(ポケット)").unwrap();
        let contour = lines.iter().position(|l| *l == "(外形)").unwrap();
        assert!(pocket < at && at < contour, "停止は 2 つの加工の間: {g}");
        assert_eq!(
            &lines[at - 4..at],
            &[
                "G0 Z20",
                "M5",
                "(停止: クランプを付け替える)",
                "(知らせ: クランプを付け替える)"
            ],
            "{g}"
        );
        assert_eq!(
            &lines[at + 1..at + 4],
            &["(外形)", "M3 S10000", "G4 P2"],
            "{g}"
        );
    }

    /// 受け入れ (b): 一部の加工だけを書き出すと、選んだ加工のコメントと動きだけが入る。
    #[test]
    fn only_the_chosen_operations_are_written() {
        let mut s = setup_of(&[
            ("面", 1, FACE),
            ("ポケット", 3, POCKET),
            ("外形", 1, CONTOUR),
        ]);
        s.manual.push(ManualAt {
            before: 2,
            id: FeatureId(200),
            manual: Manual::Stop {
                optional: false,
                text: String::new(),
            },
        });
        let one = s.only(&[FeatureId(102)]).unwrap();
        assert_eq!(one.paths.len(), 1);
        assert!(one.manual.is_empty(), "選んでいない手動 NC は入れない");
        let g = gcode(&one, &CarveraAir, &kmb_post_carvera_air::profile()).unwrap();
        assert!(g.contains("(ポケット)") && g.contains("M6 T3"), "{g}");
        assert!(
            !g.contains("(面)") && !g.contains("(外形)") && !g.contains("M600"),
            "{g}"
        );
        assert!(
            !g.contains("X1 ") && !g.contains("X3 "),
            "ほかの加工の動きはない: {g}"
        );
        // 手動 NC も選べば、残した加工の番号に付け直して入る
        let two = s
            .only(&[FeatureId(102), FeatureId(103), FeatureId(200)])
            .unwrap();
        assert_eq!(two.manual[0].before, 1);
        // ほかのセットアップの加工だけを選ぶと誤り
        assert!(s.only(&[FeatureId(999)]).is_err());
        // 選んでいない加工の失敗は、書き出しを止めない
        s.failed.push((FeatureId(104), "だめ".into()));
        assert!(
            gcode(
                &s.only(&[FeatureId(101)]).unwrap(),
                &CarveraAir,
                &kmb_post_carvera_air::profile()
            )
            .is_ok()
        );
        assert!(gcode(&s, &CarveraAir, &kmb_post_carvera_air::profile()).is_err());
    }

    /// 受け入れ (d): 工具の順に並べ替えると工具交換が減り、残り削りがあると並べ替えない。
    #[test]
    fn sorting_by_tool_reduces_tool_changes_unless_the_order_matters() {
        let s = setup_of(&[
            ("面", 1, FACE),
            ("穴", 3, DRILL),
            ("ポケット", 1, POCKET),
            ("面取り", 3, CHAMFER),
        ]);
        let (sorted, before, after) = s.by_tool().unwrap();
        assert_eq!((before, after), (4, 2));
        let names: Vec<&str> = sorted.paths.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["面", "ポケット", "穴", "面取り"]);
        assert_eq!(changes(&sorted.program()), 2);
        assert_eq!(changes(&s.program()), 4);
        // 残り削りがあれば並べ替えない
        let mut rest = s.clone();
        rest.sources[2].rest = true;
        assert!(rest.by_tool().unwrap_err().contains("残り削り"));
        // 手動 NC があれば並べ替えない
        let mut man = s.clone();
        man.manual.push(ManualAt {
            before: 1,
            id: FeatureId(200),
            manual: Manual::Comment { text: "x".into() },
        });
        assert!(man.by_tool().unwrap_err().contains("手動 NC"));
        // 切り抜きの輪郭が前へ動くなら並べ替えない
        let cut = setup_of(&[("穴", 3, DRILL), ("外形", 1, CONTOUR)]);
        assert!(cut.by_tool().unwrap_err().contains("輪郭"));
        // 輪郭が最後のままなら並べ替える
        let ok = setup_of(&[
            ("面", 1, FACE),
            ("穴", 3, DRILL),
            ("ポケット", 1, POCKET),
            ("外形", 3, CONTOUR),
        ]);
        assert!(ok.by_tool().is_ok());
    }

    /// 文書の設計の属性(部品名など)は、G-code の頭の注でプログラム番号より前に出る。
    #[test]
    fn the_part_name_goes_into_the_gcode_header() {
        let mut props = kmb_core::Properties::default();
        props.set("title", "ブラケット(左)");
        props.set("part_number", "JC-001");
        props.set("material", "A5052");
        let mut s = setup_of(&[("表", 1, FACE)]);
        s.part = props.gcode_lines();
        s.info.program = Some(1001);
        let g = CarveraAir
            .emit(&s.program(), &kmb_post_carvera_air::profile())
            .unwrap();
        let at = |t: &str| g.find(t).unwrap_or_else(|| panic!("{t} が無い: {g}"));
        // 括弧はコメントの中に書けないので落ちる
        assert!(at("(部品名 ブラケット左)") < at("(部品番号 JC-001)"));
        assert!(at("(材料 A5052)") < at("(プログラム番号 O1001)"));
        // 属性が空なら何も足さない
        assert!(kmb_core::Properties::default().gcode_lines().is_empty());
    }

    /// 全セットアップを 1 本にすると、セットアップの間に停止が入り、2 本目の初めで工具を替える。
    #[test]
    fn all_setups_are_chained_into_one_program() {
        let a = setup_of(&[("表", 1, FACE)]);
        let mut b = setup_of(&[("裏", 1, FACE)]);
        b.name = "セットアップ 2".into();
        let empty = SetupPaths {
            paths: vec![],
            sources: vec![],
            ..setup_of(&[])
        };
        let p = chain_program(&[a.clone(), empty, b.clone()]).unwrap();
        let g = CarveraAir
            .emit(&p, &kmb_post_carvera_air::profile())
            .unwrap();
        let lines: Vec<&str> = g.lines().collect();
        let stops: Vec<usize> = (0..lines.len()).filter(|&i| lines[i] == "M600").collect();
        assert_eq!(stops.len(), 1, "{g}");
        assert!(
            lines[stops[0] - 2].contains("セットアップ 2 へ付け替え"),
            "{g}"
        );
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("M6 ")).count(),
            2,
            "{g}"
        );
        let mut bad = b;
        bad.failed.push((FeatureId(9), "だめ".into()));
        assert!(chain_program(&[a, bad]).is_err());
    }
}
