//! 汎用の 3 軸のポスト(Fanuc 系と Grbl 系)。
//!
//! どちらも mm(G21)、絶対座標(G90)、XY 平面(G17)、作業座標は G54〜G59 で、円弧は G2/G3 と
//! I、J(半周以下に分ける)。固定サイクルと工具径補正は使わない(戦略が展開した動きのまま書く)。
//!
//! - Fanuc 系: 頭と終わりに `%`、O 番号の行、`T<n> M6` の後に `G43 H<n>`、待ちは `G4 X<秒>`、
//!   停止は `M0`、任意停止は `M1`
//! - Grbl 系: 工具交換の命令がない(Grbl 1.1 は M6 を受け付けない)ので、コメントと `M0` で止まって
//!   人が替える。待ちは `G4 P<秒>`。レーザーは機械の値の命令(既定は `M4 S<出力>` と `M5`)

use crate::writer::{Writer, comment};
use crate::{
    Diagnostic, MachineProfile, Post, PostError, Severity, check_common, check_wcs, machines, num,
};
use jc_toolpath::{Op, Program};
use std::fmt::Write;

/// 書き方の系統。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Fanuc,
    Grbl,
}

/// 汎用のポスト。
pub struct Generic {
    flavor: Flavor,
}

impl Generic {
    pub fn fanuc() -> Generic {
        Generic {
            flavor: Flavor::Fanuc,
        }
    }

    pub fn grbl() -> Generic {
        Generic {
            flavor: Flavor::Grbl,
        }
    }

    /// 機械の値の `post` の ID のポスト(汎用でなければ `None`)。
    pub fn by_id(id: &str) -> Option<Generic> {
        match id {
            machines::GENERIC_FANUC => Some(Generic::fanuc()),
            machines::GENERIC_GRBL => Some(Generic::grbl()),
            _ => None,
        }
    }
}

impl Post for Generic {
    fn id(&self) -> &str {
        match self.flavor {
            Flavor::Fanuc => machines::GENERIC_FANUC,
            Flavor::Grbl => machines::GENERIC_GRBL,
        }
    }

    fn check(&self, p: &Program, m: &MachineProfile) -> Vec<Diagnostic> {
        let mut d = check_common(p, m);
        d.extend(check_wcs(p, m));
        if !m.optional_stop
            && p.ops
                .iter()
                .any(|o| matches!(o, Op::Stop { optional: true }))
        {
            d.push(Diagnostic {
                severity: Severity::Warning,
                message: "任意停止を書けない機械なので、必ず止まる停止(M0)にしました".into(),
            });
        }
        d
    }

    fn emit(&self, p: &Program, m: &MachineProfile) -> Result<String, PostError> {
        let diags = self.check(p, m);
        if diags.iter().any(|d| d.severity == Severity::Error) {
            return Err(PostError(diags));
        }
        let fanuc = self.flavor == Flavor::Fanuc;
        let mut out = String::new();
        if fanuc {
            out.push_str("%\n");
        }
        // 書けない機械では、組み立てが入れた頭のコメント(「プログラム番号 O1001」)だけになる
        if let Some(n) = p.number.filter(|_| m.program_number) {
            let _ = writeln!(out, "O{n:04}");
        }
        comment(&mut out, &format!("kieMonoBench / {}", m.name));
        comment(
            &mut out,
            &format!(
                "時間の目安 {:.1} 分、工具交換の時間は含まない",
                p.minutes(&m.motion())
            ),
        );
        for t in p.tools() {
            comment(
                &mut out,
                &format!(
                    "T{} {} D{} シャンク{}",
                    t.number,
                    t.name,
                    num(t.diameter),
                    num(t.shank())
                ),
            );
        }
        for d in &diags {
            comment(&mut out, &format!("注意: {}", d.message));
        }
        let _ = writeln!(out, "G21\nG90\nG17\nG{}", 53 + p.wcs);
        if let (Some(g), Some(l)) = (&p.leveling, &m.leveling) {
            comment(&mut out, "プローブで表面の高さを測って補正する");
            let _ = writeln!(out, "{}", l.probe_line(g));
        }
        let mut w = Writer::default();
        let mut spinning = false;
        let mut in_laser = false;
        let mut burning = false;
        // 今の出力の S の値(0 なら焼いていない)
        let mut power = String::new();
        for op in &p.ops {
            match op {
                Op::Comment { text } => comment(&mut out, text),
                Op::ToolChange { tool } => {
                    let laser = tool.kind == jc_cam_tool::ToolKind::Laser;
                    if laser != in_laser
                        && let Some(l) = &m.laser
                    {
                        for line in if laser { &l.enter } else { &l.exit } {
                            let _ = writeln!(out, "{line}");
                        }
                    }
                    in_laser = laser;
                    if !(laser && m.laser.as_ref().is_some_and(|l| !l.enter.is_empty())) {
                        if fanuc {
                            let _ = writeln!(out, "T{0} M6\nG43 H{0}", tool.number);
                        } else {
                            comment(
                                &mut out,
                                &format!(
                                    "工具交換: T{} {} に替えて、Z の原点を合わせ直してから再開する",
                                    tool.number, tool.name
                                ),
                            );
                            out.push_str("M0\n");
                        }
                    }
                    w.forget();
                }
                Op::Spindle { rpm } => {
                    let _ = writeln!(out, "M3 S{}", num(*rpm));
                    if m.air && !spinning {
                        out.push_str("M7\n");
                    }
                    spinning = true;
                }
                Op::SpindleStop => {
                    out.push_str("M5\n");
                    if m.air && spinning {
                        out.push_str("M9\n");
                    }
                    spinning = false;
                }
                Op::Dwell { seconds } if burning && !power.is_empty() => {
                    // 貫通: 止まったまま焼く(検査で、止まって焼けない機械は誤りにしてある)
                    let lines = m.laser.as_ref().and_then(|l| l.pierce.as_ref());
                    match lines.filter(|l| !l.is_empty()) {
                        Some(lines) => {
                            for line in lines {
                                let line =
                                    line.replace("{s}", &power).replace("{t}", &num(*seconds));
                                let _ = writeln!(out, "{line}");
                            }
                        }
                        None => {
                            let _ = writeln!(out, "G4 P{}", num(*seconds));
                        }
                    }
                }
                Op::Dwell { seconds } => {
                    if fanuc {
                        let _ = writeln!(out, "G4 X{}", num(*seconds));
                    } else {
                        let _ = writeln!(out, "G4 P{}", num(*seconds));
                    }
                }
                Op::Message { text } => comment(&mut out, &format!("知らせ: {text}")),
                Op::Stop { optional } => {
                    out.push_str(if *optional && m.optional_stop {
                        "M1\n"
                    } else {
                        "M0\n"
                    });
                    w.forget_motion();
                }
                Op::Retract { z } => w.retract(&mut out, *z),
                Op::Move(mv) => w.motion(&mut out, mv)?,
                Op::LaserOn { power: p } => {
                    let (power, now) = (p, &mut power);
                    // 検査で、レーザーのない機械は誤りにしてある。焼き始めの命令は最初だけ書き、
                    // 出力の変わり目(はみ出しの S0 など)は送りの行の S だけにする
                    if let Some(l) = &m.laser {
                        let s = num(power * l.s_max);
                        if !burning && !l.on.is_empty() {
                            let _ = writeln!(out, "{} S{s}", l.on);
                        }
                        burning = true;
                        w.power_on_next_feed(s.clone());
                        *now = if *power > 0.0 { s } else { String::new() };
                    }
                }
                Op::LaserOff => {
                    if let Some(l) = m.laser.as_ref().filter(|l| burning && !l.off.is_empty()) {
                        let _ = writeln!(out, "{}", l.off);
                    }
                    burning = false;
                    power.clear();
                    w.forget_power();
                }
            }
        }
        if in_laser && let Some(l) = &m.laser {
            for line in &l.exit {
                let _ = writeln!(out, "{line}");
            }
        }
        if p.leveling.is_some()
            && let Some(l) = m.leveling.as_ref().filter(|l| !l.clear.is_empty())
        {
            let _ = writeln!(out, "{}", l.clear);
        }
        out.push_str("M30\n");
        if fanuc {
            out.push_str("%\n");
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_cam_tool::{ToolKind, ToolLibrary};
    use jc_toolpath::{Move, Toolpath};

    fn paths() -> Vec<Toolpath> {
        let lib = ToolLibrary::starter();
        let cut = |name: &str, k: usize, rpm: f64| Toolpath {
            name: name.into(),
            tool: lib.tools[k].clone(),
            rpm,
            safe_z: 5.0,
            retract_z: 1.0,
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
            moves: vec![
                Move::Rapid {
                    to: [0.0, 0.0, 5.0],
                },
                Move::Linear {
                    to: [0.0, 0.0, -1.0],
                    feed: 200.0,
                },
                Move::Linear {
                    to: [10.0, 0.0, -1.0],
                    feed: 600.0,
                },
                Move::Rapid {
                    to: [10.0, 0.0, 5.0],
                },
            ],
        };
        vec![cut("四角", 0, 10000.0), cut("溝", 2, 12000.0)]
    }

    fn program(wcs: u32) -> Program {
        let mut p = Program::build("見本", wcs, 20.0, &paths());
        p.number = Some(1001);
        p
    }

    #[test]
    fn fanuc_writes_the_program_number_tool_lengths_and_the_wcs() {
        let m = machines::generic_fanuc();
        let g = Generic::fanuc().emit(&program(2), &m).unwrap();
        let lines: Vec<&str> = g.lines().collect();
        assert_eq!(&lines[..2], &["%", "O1001"], "{g}");
        assert!(g.contains("\nG55\n"), "{g}");
        assert!(g.contains("\nT1 M6\nG43 H1\n"), "{g}");
        assert!(g.contains("\nT3 M6\nG43 H3\n"), "{g}");
        assert!(g.contains("\nG4 X2\n"), "{g}");
        assert!(g.ends_with("M30\n%\n"), "{g}");
        // 作業座標系は機械の範囲まで
        assert!(Generic::fanuc().emit(&program(7), &m).is_err());
    }

    #[test]
    fn grbl_stops_for_a_tool_change_and_writes_no_program_number() {
        let m = machines::generic_grbl();
        let g = Generic::grbl().emit(&program(1), &m).unwrap();
        assert!(g.starts_with("(kieMonoBench"), "{g}");
        assert!(!g.contains("M6"), "{g}");
        assert!(
            g.contains("(工具交換: T3 平 2 mm に替えて、Z の原点を合わせ直してから再開する)\nM0\n"),
            "{g}"
        );
        assert!(g.contains("\nG4 P2\n"), "{g}");
        assert!(!g.contains('%'), "{g}");
    }

    #[test]
    fn grbl_burns_with_m4_and_the_power_on_the_feed_line() {
        let mut ps = paths();
        let mut laser = ps[0].clone();
        laser.name = "焼く".into();
        laser.tool.kind = ToolKind::Laser;
        laser.tool.number = 9;
        laser.rpm = 40.0;
        ps.push(laser);
        let p = Program::build("見本", 1, 20.0, &ps);
        let g = Generic::grbl().emit(&p, &machines::generic_grbl()).unwrap();
        let at = g.find("(焼く)").expect(&g);
        let tail = &g[at..];
        assert!(!tail.contains("M3 S"), "{tail}");
        assert!(tail.contains("M4 S400\n"), "{tail}");
        assert!(tail.contains("Z-1 F200 S400\n"), "{tail}");
        assert!(tail.contains("M5\n"), "{tail}");
        // レーザーのない機械は誤り
        let e = Generic::fanuc()
            .emit(&p, &machines::generic_fanuc())
            .unwrap_err();
        assert!(e.to_string().contains("レーザーがありません"), "{e}");
    }
}
