//! 段取り表。機械の前で見る紙(Markdown)に、ストックの大きさと置き方、原点、
//! 使う工具の順、加工ごとの条件(回転数・送り・切り込み・送り幅)、長さ、高さ、突き出し、
//! 時間の目安をまとめる。手動 NC の停止なども加工の順に並べる。
//!
//! 同じ内容に、上から見た経路の図(SVG)を付けた HTML の加工指示書も書ける
//! ([`setup_sheet_html`]。ブラウザで開いて印刷する)。

use crate::{ManualAt, SetupInfo, SetupPaths};
use kmb_cam_setup::Flip;
use kmb_cam_tool::ToolKind;
/// HTML に入れる文字の置き換え
use kmb_format::svg::esc;
use kmb_post::MachineProfile;
use std::fmt::Write;

fn kind_label(k: ToolKind) -> &'static str {
    match k {
        ToolKind::FlatEnd => "フラットエンドミル",
        ToolKind::BallEnd => "ボールエンドミル",
        ToolKind::Drill => "ドリル",
        ToolKind::VBit => "V ビット",
        ToolKind::TaperEnd => "テーパー",
        ToolKind::BullNose | ToolKind::TaperBall | ToolKind::Laser | ToolKind::ThreadMill => {
            k.label()
        }
    }
}

/// 高さの補正の説明(補正しなければ `None`)。
fn leveling_text(s: &SetupPaths) -> Option<String> {
    let l = s.info.leveling?;
    let grid = |g: &kmb_toolpath::LevelGrid| {
        format!(
            "X{:.1}〜{:.1}・Y{:.1}〜{:.1} の範囲を {}×{} 点",
            g.x,
            g.x + g.width,
            g.y,
            g.y + g.length,
            g.nx,
            g.ny
        )
    };
    Some(match l.mode {
        kmb_cam_setup::LevelingMode::None => return None,
        kmb_cam_setup::LevelingMode::Gcode => match s.level_grid() {
            Some(g) => format!(
                "G-code の先頭でプローブが {} 測る。プローブを付けてから始める",
                grid(&g)
            ),
            None => "G-code の先頭で測る(削る動きがない)".into(),
        },
        kmb_cam_setup::LevelingMode::Controller => match s.level_grid() {
            Some(g) => format!(
                "コントローラーで {} 測ってから始める(G-code では測らない)",
                grid(&g)
            ),
            None => "コントローラーで測ってから始める".into(),
        },
    })
}

/// 表に出す長さ(-0.00 と出ないように、ごく小さい値は 0 にする)。
fn tidy(v: f64) -> f64 {
    if v.abs() < 5e-3 { 0.0 } else { v }
}

/// 分を「1 時間 5 分」「3 分 20 秒」のように読みやすくする。
pub fn duration(min: f64) -> String {
    let s = (min * 60.0).round() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    match (h, m) {
        (0, 0) => format!("{s} 秒"),
        (0, _) => format!("{m} 分 {s} 秒"),
        _ => format!("{h} 時間 {m} 分"),
    }
}

/// 経路の、削っていない間の動きの要約(空の動きの時間と、つなぎの種類ごとの数)。
pub fn link_summary(p: &crate::Toolpath, motion: &kmb_toolpath::Motion) -> String {
    use kmb_toolpath::LinkKind;
    let l = &p.links;
    let mut s = format!(
        "空の動き {}、全退避 {} 回",
        duration(p.air_minutes(motion)),
        l.count(LinkKind::Full)
    );
    let parts: Vec<String> = [
        (LinkKind::StayDown, "下げたまま"),
        (LinkKind::Lift, "持ち上げ"),
        (LinkKind::Low, "最小退避"),
    ]
    .into_iter()
    .filter(|&(k, _)| l.count(k) > 0)
    .map(|(k, name)| format!("{name} {}", l.count(k)))
    .collect();
    if !parts.is_empty() {
        s.push_str(&format!("(つなぎ: {})", parts.join("・")));
    }
    s
}

/// セットアップ `s` の段取り表(Markdown)。時間は `machine` の早送りの速さと加減速で見積もる。
pub fn setup_sheet(s: &SetupPaths, machine: &MachineProfile) -> String {
    let i = &s.info;
    let size = [
        i.stock_max[0] - i.stock_min[0],
        i.stock_max[1] - i.stock_min[1],
        i.stock_max[2] - i.stock_min[2],
    ];
    let mut o = String::new();
    let _ = writeln!(o, "# 段取り表: {}\n", s.name);
    let _ = writeln!(o, "| 項目 | 内容 |\n| --- | --- |");
    let _ = writeln!(o, "| 機械 | {} |", machine.name);
    let material = if i.material.is_empty() {
        "指定なし"
    } else {
        i.material.as_str()
    };
    let _ = writeln!(o, "| 材料 | {material} |");
    let _ = writeln!(
        o,
        "| ストック | 幅 {:.2} × 奥行 {:.2} × 厚さ {:.2} mm |",
        size[0], size[1], size[2]
    );
    let flip = match (i.rot, i.flip) {
        (Some(_), _) => placement_text(i),
        (None, Flip::None) => "表のまま".to_string(),
        (None, Flip::AroundX) => "X 軸のまわりに裏返す(手前と奥が入れ替わる)".to_string(),
        (None, Flip::AroundY) => "Y 軸のまわりに裏返す(左と右が入れ替わる)".to_string(),
    };
    let _ = writeln!(o, "| 置き方 | {flip} |");
    let _ = writeln!(o, "| 作業座標系 | G{} |", 53 + i.wcs);
    if let Some(l) = leveling_text(s) {
        let _ = writeln!(o, "| 高さの補正 | {l} |");
    }
    let _ = writeln!(
        o,
        "| 原点 | X0 Y0: ストックの左端から {:.2} mm、手前から {:.2} mm。Z0: ストックの上面から {:.2} mm 下 |",
        tidy(-i.stock_min[0]),
        tidy(-i.stock_min[1]),
        tidy(i.stock_max[2])
    );
    let _ = writeln!(
        o,
        "| 高さ | 横の早送り Z{:.2}、工具交換 Z{:.2} |",
        i.safe_z, i.clearance_z
    );

    append_sections(&mut o,s,machine,false);
    // 固定具の位置(ストックの左手前の角から)。経路はそのまわりを避けている
    if !i.fixtures.is_empty() {
        let _ = writeln!(o, "\n## 固定具\n");
        let _ = writeln!(
            o,
            "ストックの左手前の下の角から測った、固定具の箱の範囲(経路はまわり 2 mm を避ける)。\n\n| 番号 | X | Y | 高さ(ストックの底から) |\n| --- | --- | --- | --- |"
        );
        for (n, (lo, hi)) in i.fixtures.iter().enumerate() {
            let _ = writeln!(
                o,
                "| {} | {:.1} 〜 {:.1} mm | {:.1} 〜 {:.1} mm | {:.1} mm |",
                n + 1,
                tidy(lo[0] - i.stock_min[0]),
                tidy(hi[0] - i.stock_min[0]),
                tidy(lo[1] - i.stock_min[1]),
                tidy(hi[1] - i.stock_min[1]),
                tidy(hi[2] - lo[2])
            );
        }
    }

    // 工具は使う順に、1 本 1 回だけ
    let _ = writeln!(o, "\n## 工具(使う順)\n");
    let _ = writeln!(
        o,
        "| 番号 | 名前 | 種類 | 径 | 刃長 | 突き出しの目安 | 使う加工 |\n| --- | --- | --- | --- | --- | --- | --- |"
    );
    let mut seen: Vec<u32> = Vec::new();
    for p in &s.paths {
        let t = &p.tool;
        if seen.contains(&t.number) {
            continue;
        }
        seen.push(t.number);
        let uses: Vec<&str> = s
            .paths
            .iter()
            .filter(|q| q.tool.number == t.number)
            .map(|q| q.name.as_str())
            .collect();
        // 最も深い所まで、刃の上の部分がストックの上面に触れないだけ出す
        let stick = s
            .paths
            .iter()
            .filter(|q| q.tool.number == t.number)
            .flat_map(|q| q.moves.iter().map(|m| m.to()[2]))
            .fold(f64::INFINITY, f64::min);
        let _ = writeln!(
            o,
            "| T{} | {} | {} | {:.3} mm | {:.1} mm | {:.1} mm 以上 | {} |",
            t.number,
            t.name,
            kind_label(t.kind),
            t.diameter,
            t.flute_length,
            (i.stock_max[2] - stick + 2.0).max(t.flute_length),
            uses.join("、")
        );
    }

    let _ = writeln!(o, "\n## 加工の順\n");
    let _ = writeln!(
        o,
        "| 順 | 加工 | 工具 | 回転数 | 送り | 下降の送り | 切り込み | 送り幅 | 最も深い Z | 最も高い Z | 切削の長さ | 早送りの長さ | 突き出し | 時間の目安 | 削っていない間の動き |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    let rows = rows(s, machine);
    for r in &rows {
        match r {
            Row::Op(r) => {
                let _ = writeln!(
                    o,
                    "| {} | {} | T{} | {} | {} | {} | {} | {} | {:.2} | {:.2} | {:.0} mm | {:.0} mm | {} | {} | {} |",
                    r.n,
                    r.name,
                    r.tool,
                    r.rpm(),
                    r.feed(),
                    r.plunge(),
                    r.stepdown(),
                    r.stepover(),
                    r.zmin,
                    r.zmax,
                    r.cut,
                    r.rapid,
                    r.stick,
                    duration(r.minutes),
                    r.links
                );
            }
            Row::Manual(m) => {
                let _ = writeln!(
                    o,
                    "| — | 手動 NC: {} | | | | | | | | | | | | | |",
                    m.manual.describe()
                );
            }
        }
    }
    let t = Totals::of(&rows);
    let _ = writeln!(
        o,
        "\n合計の時間の目安: {}(工具交換と工具長の測定は含まない)。切削の長さ {:.0} mm、早送りの長さ {:.0} mm",
        duration(t.minutes),
        t.cut,
        t.rapid
    );
    let _ = writeln!(
        o,
        "\n送り・下降の送り・切り込み・送り幅は、工具の既定と加工の欄の上書きを合わせた値(パターンは元の加工の経路から読んだ送り)。突き出しは、工具に決めていればその値、なければ最も深い所まで刃の上がストックに触れない目安。"
    );
    if !s.failed.is_empty() {
        let _ = writeln!(o, "\n## 経路ができていない加工\n");
        for (id, m) in &s.failed {
            let _ = writeln!(o, "- {id}: {m}");
        }
    }
    o
}

/// 部品を置く向きの説明(上へ向けた部品の面と、部品の軸が機械のどちらを向くか)。
fn placement_text(i: &SetupInfo) -> String {
    let m = i.matrix();
    let axis = |v: [f64; 3]| -> &'static str {
        let names = [("+X", "-X"), ("+Y", "-Y"), ("+Z", "-Z")];
        let k = (0..3)
            .max_by(|a, b| v[*a].abs().total_cmp(&v[*b].abs()))
            .unwrap_or(0);
        if v[k] >= 0.0 { names[k].0 } else { names[k].1 }
    };
    // 上へ向けた部品の向きは、行列の 3 行目(部品の座標で見た機械の +Z)
    let up = axis(m[2]);
    let x = kmb_cam_setup::placement::apply(&m, [1.0, 0.0, 0.0]);
    let y = kmb_cam_setup::placement::apply(&m, [0.0, 1.0, 0.0]);
    format!(
        "部品の {up} の面を上に向ける(部品の X は機械の {}、Y は機械の {} を向く)",
        axis(x),
        axis(y)
    )
}

/// 加工の順の表の 1 行。
enum Row<'a> {
    Op(OpRow),
    Manual(&'a ManualAt),
}

/// 加工 1 つの行。切削条件が分からない欄は `None`(「—」と出す)。
struct OpRow {
    n: usize,
    name: String,
    tool: u32,
    rpm: f64,
    feed: Option<f64>,
    plunge: Option<f64>,
    stepdown: Option<f64>,
    /// 送り幅(mm)と、工具径に対する比
    stepover: Option<(f64, f64)>,
    zmin: f64,
    zmax: f64,
    cut: f64,
    rapid: f64,
    stick: String,
    minutes: f64,
    /// 削っていない間の動きの要約([`link_summary`])
    links: String,
}

fn or_dash(v: Option<String>) -> String {
    v.unwrap_or_else(|| "—".into())
}

impl OpRow {
    fn rpm(&self) -> String {
        format!("{:.0} rpm", self.rpm)
    }
    fn feed(&self) -> String {
        or_dash(self.feed.map(|f| format!("{f:.0} mm/min")))
    }
    fn plunge(&self) -> String {
        or_dash(self.plunge.map(|f| format!("{f:.0} mm/min")))
    }
    fn stepdown(&self) -> String {
        or_dash(self.stepdown.map(|v| format!("{v:.2} mm")))
    }
    fn stepover(&self) -> String {
        or_dash(
            self.stepover
                .map(|(mm, k)| format!("{mm:.2} mm(径の {:.0}%)", k * 100.0)),
        )
    }
}

/// 加工の順の行(手動 NC は、入る位置に挟む)。
fn rows<'a>(s: &'a SetupPaths, machine: &MachineProfile) -> Vec<Row<'a>> {
    let top = s.info.stock_max[2];
    let mut out = Vec::new();
    for (n, p) in s.paths.iter().enumerate() {
        out.extend(s.manual.iter().filter(|m| m.before == n).map(Row::Manual));
        let (zmin, zmax) = p
            .moves
            .iter()
            .map(|m| m.to()[2])
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), z| {
                (a.min(z), b.max(z))
            });
        let (cut, rapid) = p.lengths();
        let cutting = s.sources.get(n).and_then(|src| src.cutting);
        // 条件が出力にない時(パターンなど)は、経路のいちばん速い切削の送り
        let path_feed =
            p.moves
                .iter()
                .filter_map(|m| match m {
                    kmb_toolpath::Move::Linear { feed, .. }
                    | kmb_toolpath::Move::Arc { feed, .. } => Some(*feed),
                    _ => None,
                })
                .reduce(f64::max);
        let t = &p.tool;
        let stick = match t.stickout {
            Some(v) => format!("{v:.1} mm(工具の設定)"),
            None => format!(
                "{:.1} mm 以上(目安)",
                (top - zmin + 2.0).max(t.flute_length)
            ),
        };
        out.push(Row::Op(OpRow {
            n: n + 1,
            name: p.name.clone(),
            tool: t.number,
            rpm: p.rpm,
            feed: cutting.map(|c| c.feed).or(path_feed),
            plunge: cutting.map(|c| c.plunge),
            stepdown: cutting.map(|c| c.stepdown),
            stepover: cutting.map(|c| (c.stepover * t.diameter, c.stepover)),
            zmin,
            zmax,
            cut,
            rapid,
            stick,
            minutes: p.minutes(&machine.motion()),
            links: link_summary(p, &machine.motion()),
        }));
    }
    out.extend(
        s.manual
            .iter()
            .filter(|m| m.before >= s.paths.len())
            .map(Row::Manual),
    );
    out
}

/// 合計(時間・切削の長さ・早送りの長さ)。
struct Totals {
    minutes: f64,
    cut: f64,
    rapid: f64,
}

impl Totals {
    fn of(rows: &[Row<'_>]) -> Totals {
        let mut t = Totals {
            minutes: 0.0,
            cut: 0.0,
            rapid: 0.0,
        };
        for r in rows {
            if let Row::Op(r) = r {
                t.minutes += r.minutes;
                t.cut += r.cut;
                t.rapid += r.rapid;
            }
        }
        t
    }
}

/// 経路の色(加工ごとに順に使う)。
const COLORS: [&str; 8] = [
    "#1f77b4", "#d62728", "#2ca02c", "#9467bd", "#ff7f0e", "#17becf", "#8c564b", "#e377c2",
];

/// 上から見た図(SVG)。ストック、固定具、原点、加工ごとの切削の線(色分け)と早送り(点線)。
/// 線の点は、図の大きさの 1/800 より近い点を間引く(3D の細かい経路で HTML が膨らまないように)。
fn top_view(s: &SetupPaths) -> String {
    let i = &s.info;
    let (mut lo, mut hi) = (
        [i.stock_min[0], i.stock_min[1]],
        [i.stock_max[0], i.stock_max[1]],
    );
    for p in &s.paths {
        for m in &p.moves {
            let q = m.to();
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    let pad = 0.05 * (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
    let (x0, y0, w, h) = (
        lo[0] - pad,
        lo[1] - pad,
        hi[0] - lo[0] + 2.0 * pad,
        hi[1] - lo[1] + 2.0 * pad,
    );
    let min_d = w.max(h) / 800.0;
    let stroke = w.max(h) / 400.0;
    // SVG の y は下向きなので、y を反転して上を奥にする
    let pt = |x: f64, y: f64| format!("{:.2},{:.2}", x, y0 + h - (y - y0));
    let mut o = String::new();
    let _ = write!(
        o,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0:.2} {y0:.2} {w:.2} {h:.2}" class="view" role="img" aria-label="上から見た経路">"#
    );
    let rect = |o: &mut String, a: [f64; 2], b: [f64; 2], class: &str| {
        let _ = write!(
            o,
            r#"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" class="{class}" stroke-width="{stroke:.3}"/>"#,
            a[0],
            y0 + h - (b[1] - y0),
            b[0] - a[0],
            b[1] - a[1]
        );
    };
    rect(&mut o, lo_of(i.stock_min), lo_of(i.stock_max), "stock");
    for (a, b) in &i.fixtures {
        rect(&mut o, lo_of(*a), lo_of(*b), "fixture");
    }
    for (k, p) in s.paths.iter().enumerate() {
        let Some(first) = p.moves.first() else {
            continue;
        };
        let color = COLORS[k % COLORS.len()];
        let mut cur = first.to();
        let mut line: Vec<String> = vec![pt(cur[0], cur[1])];
        let mut last = cur;
        let mut rapid = true;
        let flush = |o: &mut String, line: &mut Vec<String>, rapid: bool| {
            if line.len() > 1 {
                let style = if rapid {
                    format!("stroke=\"#999\" stroke-dasharray=\"{:.2}\"", stroke * 3.0)
                } else {
                    format!(r#"stroke="{color}""#)
                };
                let _ = write!(
                    o,
                    r#"<polyline points="{}" fill="none" {style} stroke-width="{:.3}"/>"#,
                    line.join(" "),
                    if rapid { stroke * 0.5 } else { stroke }
                );
            }
            line.clear();
        };
        for m in &p.moves[1..] {
            let r = matches!(m, kmb_toolpath::Move::Rapid { .. });
            if r != rapid {
                flush(&mut o, &mut line, rapid);
                line.push(pt(last[0], last[1]));
                rapid = r;
            }
            for q in m.points(cur, min_d.max(0.05)) {
                if (q[0] - last[0]).hypot(q[1] - last[1]) >= min_d {
                    line.push(pt(q[0], q[1]));
                    last = q;
                }
            }
            cur = m.to();
        }
        if (cur[0] - last[0]).hypot(cur[1] - last[1]) > 0.0 {
            line.push(pt(cur[0], cur[1]));
        }
        flush(&mut o, &mut line, rapid);
    }
    // 原点(X0 Y0)
    let c = stroke * 6.0;
    let _ = write!(
        o,
        r#"<g class="origin" stroke-width="{:.3}"><line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}"/><line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}"/></g>"#,
        stroke * 1.5,
        -c,
        y0 + h + y0,
        c,
        y0 + h + y0,
        0.0,
        y0 + h + y0 - c,
        0.0,
        y0 + h + y0 + c
    );
    o.push_str("</svg>");
    o
}

fn lo_of(p: [f64; 3]) -> [f64; 2] {
    [p[0], p[1]]
}

/// セットアップ `s` の加工指示書(HTML)。段取り表と同じ項目に、上から見た経路の図を付ける。
/// 外のファイルを読まない 1 枚の HTML で、ブラウザで開いて印刷する。
pub fn setup_sheet_html(s: &SetupPaths, machine: &MachineProfile) -> String {
    let i = &s.info;
    let size = [0, 1, 2].map(|k| i.stock_max[k] - i.stock_min[k]);
    let rows = rows(s, machine);
    let t = Totals::of(&rows);
    let mut o = String::new();
    let _ = write!(
        o,
        r#"<!doctype html>
<html lang="ja"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>加工指示書: {name}</title>
<style>
:root {{ --fg: #1a1a1a; --muted: #666; --line: #ccc; --bg: #fff; --head: #f2f2f2; }}
@media (prefers-color-scheme: dark) {{ :root {{ --fg: #e8e8e8; --muted: #aaa; --line: #555; --bg: #1c1c1c; --head: #2a2a2a; }} }}
body {{ font-family: system-ui, sans-serif; color: var(--fg); background: var(--bg); margin: 16px; }}
h1 {{ font-size: 1.4em; }} h2 {{ font-size: 1.1em; margin-top: 1.6em; }}
table {{ border-collapse: collapse; font-size: 0.85em; }}
th, td {{ border: 1px solid var(--line); padding: 3px 6px; text-align: left; vertical-align: top; }}
th {{ background: var(--head); }}
td.num {{ text-align: right; white-space: nowrap; }}
tr.manual td {{ font-style: italic; }}
.wrap {{ overflow-x: auto; }}
.view {{ width: 100%; max-width: 720px; height: auto; border: 1px solid var(--line); background: #fff; }}
.view .stock {{ fill: #f6efe2; stroke: #8a6d3b; }}
.view .fixture {{ fill: #f2c4c4; stroke: #a33; }}
.view .origin line {{ stroke: #000; }}
.legend span {{ display: inline-block; margin-right: 1em; }}
.legend i {{ display: inline-block; width: 1.2em; height: 0.3em; vertical-align: middle; margin-right: 0.3em; }}
.note {{ color: var(--muted); font-size: 0.85em; }}
@media print {{ body {{ margin: 0; }} .wrap {{ overflow: visible; }} }}
</style></head><body>
<h1>加工指示書: {name}</h1>
<table>
<tr><th>機械</th><td>{machine}</td></tr>
<tr><th>材料</th><td>{material}</td></tr>
<tr><th>ストック</th><td>幅 {sx:.2} × 奥行 {sy:.2} × 厚さ {sz:.2} mm</td></tr>
<tr><th>作業座標系</th><td>G{wcs}</td></tr>
{leveling}<tr><th>原点</th><td>X0 Y0: ストックの左端から {ox:.2} mm、手前から {oy:.2} mm。Z0: ストックの上面から {oz:.2} mm 下</td></tr>
<tr><th>高さ</th><td>横の早送り Z{safe:.2}、工具交換 Z{clear:.2}</td></tr>
<tr><th>合計</th><td>時間の目安 {time}(工具交換と工具長の測定は含まない)、切削の長さ {cut:.0} mm、早送りの長さ {rapid:.0} mm</td></tr>
</table>
"#,
        name = esc(&s.name),
        machine = esc(&machine.name),
        material = if i.material.is_empty() {
            "指定なし".to_string()
        } else {
            esc(&i.material)
        },
        sx = size[0],
        sy = size[1],
        sz = size[2],
        wcs = 53 + i.wcs,
        leveling = leveling_text(s)
            .map(|l| format!("<tr><th>高さの補正</th><td>{}</td></tr>\n", esc(&l)))
            .unwrap_or_default(),
        ox = tidy(-i.stock_min[0]),
        oy = tidy(-i.stock_min[1]),
        oz = tidy(i.stock_max[2]),
        safe = i.safe_z,
        clear = i.clearance_z,
        time = duration(t.minutes),
        cut = t.cut,
        rapid = t.rapid,
    );
    let _ = write!(
        o,
        "<h2>上から見た経路</h2>\n{}\n<p class=\"legend\">",
        top_view(s)
    );
    for (k, p) in s.paths.iter().enumerate() {
        let _ = write!(
            o,
            r#"<span><i style="background:{}"></i>{} {}</span>"#,
            COLORS[k % COLORS.len()],
            k + 1,
            esc(&p.name)
        );
    }
    o.push_str(
        "<span><i style=\"background:#999\"></i>早送り</span></p>\n<p class=\"note\">奥が上。茶色の枠がストック、薄い赤で塗った箱が固定具、十字が原点(X0 Y0)。</p>\n",
    );
    o.push_str("<h2>加工の順</h2>\n<div class=\"wrap\"><table>\n<tr><th>順</th><th>加工</th><th>工具</th><th>回転数</th><th>送り</th><th>下降の送り</th><th>切り込み</th><th>送り幅</th><th>最も深い Z</th><th>最も高い Z</th><th>切削の長さ</th><th>早送りの長さ</th><th>突き出し</th><th>時間の目安</th><th>削っていない間の動き</th></tr>\n");
    for r in &rows {
        match r {
            Row::Op(r) => {
                let tool = s.paths.get(r.n - 1).map_or(String::new(), |p| {
                    format!("T{} {}", r.tool, esc(&p.tool.name))
                });
                let _ = writeln!(
                    o,
                    r#"<tr><td class="num">{}</td><td>{}</td><td>{tool}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{:.2}</td><td class="num">{:.2}</td><td class="num">{:.0} mm</td><td class="num">{:.0} mm</td><td>{}</td><td class="num">{}</td><td>{}</td></tr>"#,
                    r.n,
                    esc(&r.name),
                    r.rpm(),
                    r.feed(),
                    r.plunge(),
                    r.stepdown(),
                    r.stepover(),
                    r.zmin,
                    r.zmax,
                    r.cut,
                    r.rapid,
                    esc(&r.stick),
                    duration(r.minutes),
                    esc(&r.links)
                );
            }
            Row::Manual(m) => {
                let _ = writeln!(
                    o,
                    r#"<tr class="manual"><td>—</td><td colspan="14">手動 NC: {}</td></tr>"#,
                    esc(&m.manual.describe())
                );
            }
        }
    }
    o.push_str("</table></div>\n");
    // 工具(使う順に 1 本 1 行)
    o.push_str("<h2>工具(使う順)</h2>\n<div class=\"wrap\"><table>\n<tr><th>番号</th><th>名前</th><th>種類</th><th>径</th><th>刃長</th><th>突き出し</th></tr>\n");
    let mut seen: Vec<u32> = Vec::new();
    for p in &s.paths {
        let t = &p.tool;
        if seen.contains(&t.number) {
            continue;
        }
        seen.push(t.number);
        let _ = writeln!(
            o,
            r#"<tr><td>T{}</td><td>{}</td><td>{}</td><td class="num">{:.3} mm</td><td class="num">{:.1} mm</td><td>{}</td></tr>"#,
            t.number,
            esc(&t.name),
            kind_label(t.kind),
            t.diameter,
            t.flute_length,
            t.stickout
                .map_or("決めていない".to_string(), |v| format!("{v:.1} mm"))
        );
    }
    o.push_str("</table></div>\n");
    if !s.failed.is_empty() {
        o.push_str("<h2>経路ができていない加工</h2>\n<ul>\n");
        for (id, m) in &s.failed {
            let _ = writeln!(o, "<li>{id}: {}</li>", esc(m));
        }
        o.push_str("</ul>\n");
    }
    append_sections(&mut o,s,machine,true);
    o.push_str("<p class=\"note\">kieMonoBench が書いた加工指示書。送り・切り込み・送り幅は工具の既定と加工の欄の上書きを合わせた値。</p>\n</body></html>\n");
    o
}

fn append_sections(out:&mut String,s:&SetupPaths,machine:&MachineProfile,html:bool) {
    let Some(sections)=&s.info.sections else { return; };
    for sec in &sections.list {
        let size=[0,1,2].map(|k|sec.stock_max[k]-sec.stock_min[k]);
        let name=match sections.output {
            kmb_cam_setup::sections::SectionOutput::Files=>crate::section_file_name(&s.name,Some(&sec.label),&machine.post),
            kmb_cam_setup::sections::SectionOutput::Stops=>crate::section_file_name(&s.name,None,&machine.post),
        };
        let stock=format!("幅 {:.2} × 奥行 {:.2} × 厚さ {:.2} mm",size[0],size[1],size[2]);
        if html {
            let _=writeln!(out,"<h2>{}</h2><p>{}</p><p>ファイル: {}</p>",esc(&sec.label),stock,esc(&name));
        } else {
            let _=writeln!(out,"\n## {}\n\n{stock}\n\nファイル: {name}\n",sec.label);
        }
        if let Some(sheet)=s.nest.as_ref().and_then(|n|n.sheets.iter().find(|n|n.index==sec.index)) {
            let utilization=format!("使った面積: {:.1}%",sheet.utilization*100.0);
            let mut counts:std::collections::BTreeMap<kmb_core::FeatureId,u32>=Default::default();
            for part in &sheet.parts { *counts.entry(part.body).or_default()+=1; }
            if html { let _=writeln!(out,"<p>{utilization}</p><table><tr><th>部品</th><th>個数</th></tr>"); }
            else { let _=writeln!(out,"{utilization}\n\n| 部品 | 個数 |\n| --- | --- |"); }
            for (body,count) in counts {
                let name=s.nest_names.get(&body).cloned().unwrap_or_else(||body.to_string());
                if html { let _=writeln!(out,"<tr><td>{}</td><td>{count}</td></tr>",esc(&name)); }
                else { let _=writeln!(out,"| {} | {count} |",name.replace('|',"\\|")); }
            }
            if html { out.push_str("</table>\n"); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 受け入れ (e): 段取り表に、加工ごとの条件・長さ・高さ・突き出しと、手動 NC の行が出る。
    #[test]
    fn the_sheet_lists_the_cutting_conditions_lengths_and_heights() {
        let mut s = crate::tests::setup_of(&[
            ("ポケット", 1, kmb_strategy_pocket2d::POCKET),
            ("外形", 3, kmb_strategy_contour2d::CONTOUR),
        ]);
        s.sources[0].cutting = Some(kmb_cam_tool::Cutting {
            rpm: 10000.0,
            feed: 600.0,
            plunge: 200.0,
            stepdown: 0.5,
            stepover: 0.4,
        });
        s.paths[1].tool.stickout = Some(18.0);
        s.manual.push(crate::ManualAt {
            before: 1,
            id: kmb_core::FeatureId(200),
            manual: kmb_toolpath::Manual::Stop {
                optional: false,
                text: "クランプ".into(),
            },
        });
        let m = kmb_post_carvera_air::profile();
        let md = setup_sheet(&s, &m);
        assert!(md.contains("| 送り | 下降の送り | 切り込み | 送り幅 | 最も深い Z | 最も高い Z | 切削の長さ | 早送りの長さ | 突き出し |"), "{md}");
        // 平 6 mm の 40% は 2.4 mm。切削は 6 + 3 = 9 mm、早送りは 6 mm
        assert!(md.contains("| 1 | ポケット | T1 | 10000 rpm | 600 mm/min | 200 mm/min | 0.50 mm | 2.40 mm(径の 40%) | -1.00 | 5.00 | 9 mm | 6 mm |"), "{md}");
        assert!(md.contains("| 18.0 mm(工具の設定) |"), "{md}");
        // 条件が出力にない加工は、経路の送りを出し、分からない欄は —
        assert!(
            md.contains("| 2 | 外形 | T3 | 10000 rpm | 600 mm/min | — | — | — |"),
            "{md}"
        );
        let stop = md.find("手動 NC: 停止: クランプ").expect(&md);
        assert!(md.find("| 1 | ポケット").unwrap() < stop && stop < md.find("| 2 | 外形").unwrap());
        assert!(md.contains("切削の長さ 18 mm、早送りの長さ 12 mm"), "{md}");
        let html = setup_sheet_html(&s, &m);
        assert!(html.starts_with("<!doctype html>"), "{html}");
        assert!(
            html.contains("<svg") && html.contains("<polyline"),
            "{html}"
        );
        assert!(
            html.contains("0.50 mm") && html.contains("手動 NC: 停止: クランプ"),
            "{html}"
        );
        assert!(html.contains("<title>加工指示書: セットアップ 1</title>"));
        // 名前の記号は HTML の文字に置き換える
        s.paths[0].name = "<b>&".into();
        assert!(setup_sheet_html(&s, &m).contains("&lt;b&gt;&amp;"));
    }

    #[test]
    fn durations_are_easy_to_read() {
        assert_eq!(duration(0.5), "30 秒");
        assert_eq!(duration(3.0 + 20.0 / 60.0), "3 分 20 秒");
        assert_eq!(duration(65.0), "1 時間 5 分");
    }
}
