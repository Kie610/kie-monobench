//! LightBurn に取り込む SVG を書くポスト(`lightburn-svg`)。G-code で動かないガルバノの
//! レーザー(SCULPFUN G20 など)向け。
//!
//! - 焼く送りの動き(出力が 0 より大きい間)だけを、続く所ごとに 1 本の折れ線にして書く。
//!   早送りと出力 0 の送り(つなぎ・はみ出し)は書かない。円弧は折れ線に分ける
//! - 加工(経路の頭のコメント)ごとに 1 つの組にし、LightBurn のレイヤーの色で線を塗り分ける。
//!   出力・速さ・周波数は SVG に書けないので、取り込んだ後にレイヤーで決める
//! - 寸法は mm。作業座標の Y は上が正なので、SVG では Y を反転して書く。原点(作業座標の 0, 0)の
//!   位置は `viewBox` に残す(LightBurn での置き方は U、実機で確かめる)
//! - 切り口の補正と進入・退出は経路で済んでいる。LightBurn の Kerf Offset は 0 にする

use crate::{
    Diagnostic, MachineProfile, Post, PostError, Severity, check_common, machines, pierces,
};
/// 小数 4 桁まで(末尾の 0 を書かない)
use jc_format::svg::num as n;
use jc_toolpath::{Move, Op, P3, Program};
use std::fmt::Write;

/// LightBurn の初めのレイヤーの色(00 から順。A、LightBurn の画面の並びから)。
const COLORS: [&str; 10] = [
    "#000000", "#0000FF", "#FF0000", "#00E000", "#D0D000", "#FF8000", "#00E0E0", "#FF00FF",
    "#B4B4B4", "#0000A0",
];

/// 円弧を折れ線に分ける間隔(mm)
const STEP: f64 = 0.05;

pub struct LightBurnSvg;

/// 加工の名前、出力(0〜1)、焼く折れ線。
type Group = (String, f64, Vec<Vec<[f64; 2]>>);

impl Post for LightBurnSvg {
    fn id(&self) -> &str {
        machines::LIGHTBURN_SVG
    }

    fn check(&self, p: &Program, m: &MachineProfile) -> Vec<Diagnostic> {
        let mut d = check_common(p, m);
        let warn = |message: &str| Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
        };
        if p.ops.iter().any(
            |o| matches!(o, Op::ToolChange { tool } if tool.kind != jc_cam_tool::ToolKind::Laser),
        ) {
            d.push(Diagnostic {
                severity: Severity::Error,
                message: format!("{} はレーザーの加工だけを書き出せます", m.name),
            });
        }
        if pierces(p) {
            d.push(warn(
                "貫通の時間は SVG に書けません。LightBurn のレイヤーの「始めの止まり」(Start Pause)で決めてください",
            ));
        }
        d
    }

    fn emit(&self, p: &Program, m: &MachineProfile) -> Result<String, PostError> {
        let diags = self.check(p, m);
        if diags.iter().any(|d| d.severity == Severity::Error) {
            return Err(PostError(diags));
        }
        // 加工ごとの焼く線
        let mut groups: Vec<Group> = Vec::new();
        let mut name = String::new();
        let mut pos: Option<P3> = None;
        let mut power = 0.0;
        let mut line: Vec<[f64; 2]> = Vec::new();
        let flush = |groups: &mut Vec<Group>, line: &mut Vec<[f64; 2]>| {
            if line.len() >= 2
                && let Some(g) = groups.last_mut()
            {
                g.2.push(std::mem::take(line));
            }
            line.clear();
        };
        for op in &p.ops {
            match op {
                Op::Comment { text } => name = text.clone(),
                Op::LaserOn { power: w } => {
                    if *w <= 0.0 {
                        flush(&mut groups, &mut line);
                    } else if groups.last().is_none_or(|g| g.0 != name) {
                        flush(&mut groups, &mut line);
                        groups.push((name.clone(), *w, Vec::new()));
                    }
                    power = *w;
                }
                Op::LaserOff => {
                    flush(&mut groups, &mut line);
                    power = 0.0;
                }
                Op::Move(mv) => {
                    let to = mv.to();
                    let burn = power > 0.0 && !matches!(mv, Move::Rapid { .. });
                    if burn && let Some(from) = pos {
                        if line.is_empty() {
                            line.push([from[0], from[1]]);
                        }
                        let pts = match mv {
                            Move::Arc { .. } => mv.points(from, STEP),
                            _ => vec![to],
                        };
                        for q in pts {
                            let q = [q[0], q[1]];
                            if line
                                .last()
                                .is_none_or(|l| (l[0] - q[0]).hypot(l[1] - q[1]) > 1e-9)
                            {
                                line.push(q);
                            }
                        }
                    } else {
                        flush(&mut groups, &mut line);
                    }
                    pos = Some(to);
                }
                Op::ToolChange { .. } | Op::Stop { .. } => {
                    flush(&mut groups, &mut line);
                    pos = None;
                }
                _ => {}
            }
        }
        flush(&mut groups, &mut line);
        let (mut lo, mut hi) = ([0.0f64, 0.0f64], [0.0f64, 0.0f64]);
        for q in groups.iter().flat_map(|g| g.2.iter().flatten()) {
            for i in 0..2 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
            }
        }
        let (w, h) = ((hi[0] - lo[0]).max(1.0), (hi[1] - lo[1]).max(1.0));
        let mut out = String::new();
        let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        let _ = writeln!(out, "<!-- kieMonoBench / {} -->", xml(&m.name));
        if let Some(l) = &m.laser {
            for n in &l.notes {
                let _ = writeln!(out, "<!-- {} -->", xml(n));
            }
        }
        for d in &diags {
            let _ = writeln!(out, "<!-- 注意: {} -->", xml(&d.message));
        }
        let _ = writeln!(
            out,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}mm" height="{}mm" viewBox="{} {} {} {}">"#,
            n(w),
            n(h),
            n(lo[0]),
            n(-hi[1]),
            n(w),
            n(h)
        );
        let _ = writeln!(out, "<title>{}</title>", xml(&p.name));
        for (k, (name, power, lines)) in groups.iter().enumerate() {
            let _ = writeln!(
                out,
                r#"<g id="op{k}" fill="none" stroke="{}" stroke-width="0.05">"#,
                COLORS[k % COLORS.len()]
            );
            let _ = writeln!(
                out,
                "<title>{} 出力 {} %</title>",
                xml(name),
                n(power * 100.0)
            );
            for l in lines {
                let closed = l.len() > 2
                    && (l[0][0] - l[l.len() - 1][0]).hypot(l[0][1] - l[l.len() - 1][1]) < 1e-9;
                let pts: Vec<String> = l[..l.len() - usize::from(closed)]
                    .iter()
                    .map(|q| format!("{},{}", n(q[0]), n(-q[1])))
                    .collect();
                let tag = if closed { "polygon" } else { "polyline" };
                let _ = writeln!(out, r#"<{tag} points="{}"/>"#, pts.join(" "));
            }
            out.push_str("</g>\n");
        }
        out.push_str("</svg>\n");
        Ok(out)
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace("--", "- -")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_toolpath::{Role, Span, Toolpath};

    fn laser_path() -> Toolpath {
        let mut tool = jc_cam_tool::ToolLibrary::laser_tool();
        tool.number = 1;
        let z = 0.0;
        let sq = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ];
        let mut moves = vec![
            Move::Rapid {
                to: [0.0, 0.0, 5.0],
            },
            Move::Linear {
                to: [0.0, 0.0, z],
                feed: 600.0,
            },
        ];
        moves.extend(sq[1..].iter().map(|p| Move::Linear {
            to: [p[0], p[1], z],
            feed: 600.0,
        }));
        moves.push(Move::Rapid {
            to: [0.0, 0.0, 5.0],
        });
        Toolpath {
            name: "切り抜き".into(),
            tool,
            rpm: 80.0,
            safe_z: 5.0,
            retract_z: 1.0,
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![Span {
                start: 1,
                end: 2,
                role: Role::Link,
            }],
            links: Default::default(),
            moves,
        }
    }

    #[test]
    fn the_burn_moves_become_one_closed_polygon_per_shape() {
        let p = Program::build("板", 1, 20.0, &[laser_path()]);
        let m = machines::sculpfun_g20();
        let svg = LightBurnSvg.emit(&p, &m).unwrap();
        assert!(
            svg.contains(r#"width="10mm" height="10mm" viewBox="0 -10 10 10""#),
            "{svg}"
        );
        // Y は反転して書く。閉じた四角は polygon 1 つ
        assert!(
            svg.contains(r#"<polygon points="0,0 10,0 10,-10 0,-10"/>"#),
            "{svg}"
        );
        assert_eq!(
            svg.matches("<polygon").count() + svg.matches("<polyline").count(),
            1
        );
        assert!(svg.contains("切り抜き 出力 80 %"), "{svg}");
    }

    #[test]
    fn a_milling_tool_cannot_go_to_the_galvo() {
        let mut t = laser_path();
        t.tool = jc_cam_tool::ToolLibrary::starter().tools[0].clone();
        let p = Program::build("板", 1, 20.0, &[t]);
        assert!(LightBurnSvg.emit(&p, &machines::sculpfun_g20()).is_err());
    }
}
