//! Carvera Air 向けのポスト。
//!
//! 書式の根拠は機械の対応 G-code の一覧。ほかのポストのコードは参照していない。
//!
//! - 単位は mm(G21)、絶対座標(G90)、XY 平面(G17)。作業座標は G54
//! - 円弧は G2/G3 と I、J(始点からの中心の位置)。半周を超える円弧は半周以下に分ける
//! - 工具交換は `M6 T<n>`。既定では交換の後に `M491` で工具長を測り直す
//!   (Carvera Air の手動交換で工具長が自動で測られるかは未確認のため。U)
//! - 待ちは `G4 P<秒>`、エアは `M7`/`M9`、終わりは `M30`
//! - 固定サイクル(G81 など)は使わない
//! - 手動 NC の停止は `M600`(A、実機は未確認)。`M0`・`M1` は機械の対応 G-code の
//!   一覧に無いので使わない。任意停止も、止まる側に倒して
//!   `M600` にし、注意を出す。止める前に主軸を止めて退避するのは [`Program::build_with`] の仕事
//! - 知らせはコメントにする(Carvera Air は画面を持たず。U)
//! - O 番号の行は一覧に無いのでコメントにする
//! - レーザーは `M321` で替え(工具を外し、レーザーの位置を合わせる)、焼く送りの行に `S<0〜1>` を
//!   付け、`M322` で戻す。高さの補正は先頭に `G32 R1 X Y A B I J H`、終わりに `M370`(どちらも
//!   機械の値の命令。A、実機は未確認)
//!
//! 機械の値(作業範囲・ベッド・レーザーと補正の命令)は [`machine`](mod@machine) にある。組み立ては
//! [`library_entry`] をマシンライブラリに入れる。

use kmb_post::writer::{Writer, comment};
use kmb_post::{
    Diagnostic, MachineProfile, Post, PostError, Severity, check_common, check_wcs, num,
};
use kmb_toolpath::{Op, Program};
use std::fmt::Write;

pub struct CarveraAir;

mod machine;
pub use machine::ID;

/// Carvera Air の値(マシンライブラリの既定の機械)。
pub fn profile() -> MachineProfile {
    machine::profile()
}

/// Carvera Air のベッド。
pub fn bed() -> kmb_post::Bed {
    machine::bed()
}

/// マシンライブラリの 1 台(機械の値とポスト)。
pub fn library_entry() -> kmb_post::machines::Machine {
    kmb_post::machines::Machine::new(profile(), CarveraAir)
}

impl Post for CarveraAir {
    fn id(&self) -> &str {
        ID
    }

    fn check(&self, p: &Program, m: &MachineProfile) -> Vec<Diagnostic> {
        let mut d = check_common(p, m);
        if p.wcs != 1 {
            d.push(Diagnostic {
                severity: Severity::Error,
                message: "Carvera の対応表にあるのは G54 だけです".into(),
            });
        } else {
            d.extend(check_wcs(p, m));
        }
        if p.ops
            .iter()
            .any(|o| matches!(o, Op::Stop { optional: true }))
        {
            d.push(Diagnostic {
                severity: Severity::Warning,
                message:
                    "任意停止(M1)は Carvera の対応表に無いので、必ず止まる停止(M600)にしました"
                        .into(),
            });
        }
        d
    }

    fn emit(&self, p: &Program, m: &MachineProfile) -> Result<String, PostError> {
        let diags = self.check(p, m);
        if diags.iter().any(|d| d.severity == Severity::Error) {
            return Err(PostError(diags));
        }
        let mut out = String::new();
        comment(&mut out, &format!("kieMonoBench / {}", m.name));
        comment(
            &mut out,
            &format!(
                "時間の目安 {:.1} 分、工具交換と工具長の測定は含まない",
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
        out.push_str("G21\nG90\nG17\nG54\n");
        if let (Some(g), Some(l)) = (&p.leveling, &m.leveling) {
            comment(&mut out, "プローブで表面の高さを測って補正する");
            let _ = writeln!(out, "{}", l.probe_line(g));
        }
        let mut w = Writer::default();
        let mut spinning = false;
        let mut in_laser = false;
        let mut burning = false;
        let laser = m.laser.as_ref();
        for op in &p.ops {
            match op {
                Op::Comment { text } => comment(&mut out, text),
                Op::ToolChange { tool } => {
                    let to_laser = tool.kind == kmb_cam_tool::ToolKind::Laser;
                    if let Some(l) = laser.filter(|_| to_laser != in_laser) {
                        for line in if to_laser { &l.enter } else { &l.exit } {
                            let _ = writeln!(out, "{line}");
                        }
                    }
                    in_laser = to_laser;
                    if !to_laser {
                        let _ = writeln!(out, "M6 T{}", tool.number);
                        if m.measure_after_tool_change {
                            out.push_str("M491\n");
                        }
                    }
                    // 交換の後は位置が分からないので、次の早送りは Z を先に動かす
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
                Op::Dwell { seconds } => {
                    let _ = writeln!(out, "G4 P{}", num(*seconds));
                }
                Op::Message { text } => comment(&mut out, &format!("知らせ: {text}")),
                Op::Stop { .. } => {
                    // 止まって再開を待つ。再開の後は早送りの G0 から書き直す
                    out.push_str("M600\n");
                    w.forget_motion();
                }
                Op::Retract { z } => w.retract(&mut out, *z),
                Op::Move(mv) => w.motion(&mut out, mv)?,
                Op::LaserOn { power } => {
                    // 検査で、レーザーのない機械は誤りにしてある。焼き始めの命令は最初だけ書き、
                    // 出力の変わり目(はみ出しの S0 など)は送りの行の S だけにする
                    if let Some(l) = laser {
                        let s = num(power * l.s_max);
                        if !burning && !l.on.is_empty() {
                            let _ = writeln!(out, "{} S{s}", l.on);
                        }
                        burning = true;
                        w.power_on_next_feed(s);
                    }
                }
                Op::LaserOff => {
                    if let Some(l) = laser.filter(|l| burning && !l.off.is_empty()) {
                        let _ = writeln!(out, "{}", l.off);
                    }
                    burning = false;
                    w.forget_power();
                }
            }
        }
        if let Some(l) = laser.filter(|_| in_laser) {
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
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_cam_tool::ToolLibrary;
    use kmb_toolpath::{ArcDir, Move, Toolpath};

    fn sample() -> Program {
        Program::build("見本", 1, 20.0, &sample_paths())
    }

    fn sample_paths() -> Vec<Toolpath> {
        let lib = ToolLibrary::starter();
        let square = Toolpath {
            name: "四角".into(),
            tool: lib.tools[0].clone(),
            rpm: 10000.0,
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
                Move::Rapid {
                    to: [0.0, 0.0, 1.0],
                },
                Move::Linear {
                    to: [0.0, 0.0, -1.0],
                    feed: 200.0,
                },
                Move::Linear {
                    to: [10.0, 0.0, -1.0],
                    feed: 600.0,
                },
                Move::Linear {
                    to: [10.0, 10.0, -1.0],
                    feed: 600.0,
                },
                Move::Rapid {
                    to: [10.0, 10.0, 5.0],
                },
            ],
        };
        let helix = Toolpath {
            name: "らせん".into(),
            tool: lib.tools[2].clone(),
            rpm: 12000.0,
            safe_z: 5.0,
            retract_z: 1.0,
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
            moves: vec![
                Move::Rapid {
                    to: [22.0, 20.0, 5.0],
                },
                Move::Rapid {
                    to: [22.0, 20.0, 1.0],
                },
                Move::Linear {
                    to: [22.0, 20.0, 0.0],
                    feed: 150.0,
                },
                Move::Arc {
                    to: [22.0, 20.0, -0.6],
                    center: [20.0, 20.0],
                    dir: ArcDir::Ccw,
                    feed: 400.0,
                },
                Move::Rapid {
                    to: [22.0, 20.0, 5.0],
                },
            ],
        };
        vec![square, helix]
    }

    #[test]
    fn gcode_snapshot() {
        let g = CarveraAir.emit(&sample(), &profile()).unwrap();
        insta::assert_snapshot!(g);
    }

    #[test]
    fn full_circles_are_split_and_arcs_keep_their_radius() {
        let g = CarveraAir.emit(&sample(), &profile()).unwrap();
        let arcs: Vec<&str> = g.lines().filter(|l| l.contains(" I")).collect();
        assert_eq!(arcs.len(), 2, "{g}");
        assert!(
            arcs[0].starts_with("G3 X18 Y20 Z-0.3 I-2 J0"),
            "{}",
            arcs[0]
        );
        assert!(arcs[1].starts_with("X22 Y20 Z-0.6 I2 J0"), "{}", arcs[1]);
    }

    #[test]
    fn out_of_range_programs_are_refused() {
        let mut p = sample();
        p.ops.push(Op::Spindle { rpm: 20000.0 });
        let e = CarveraAir.emit(&p, &profile()).unwrap_err();
        assert!(e.to_string().contains("回転数"), "{e}");
        let mut p = sample();
        if let Some(Op::Move(Move::Linear { to, .. })) = p
            .ops
            .iter_mut()
            .find(|o| matches!(o, Op::Move(Move::Linear { .. })))
        {
            to[0] = 400.0;
        }
        assert!(CarveraAir.emit(&p, &profile()).is_err());
    }

    /// 手動 NC の停止は、主軸を止めて退避した後の `M600` になり、再開の後に主軸を回し直す。
    #[test]
    fn a_manual_stop_becomes_m600_between_a_spindle_stop_and_a_restart() {
        let paths = sample_paths();
        let manual = [(
            1,
            kmb_toolpath::Manual::Stop {
                optional: false,
                text: "切りくずを払う".into(),
            },
        )];
        let p = Program::build_with("見本", 1, 20.0, &paths, &manual);
        let g = CarveraAir.emit(&p, &profile()).unwrap();
        let lines: Vec<&str> = g.lines().collect();
        let at = lines.iter().position(|l| *l == "M600").expect(&g);
        assert_eq!(
            &lines[at - 4..at],
            &[
                "G0 Z20",
                "M5",
                "(停止: 切りくずを払う)",
                "(知らせ: 切りくずを払う)"
            ],
            "{g}"
        );
        // 再開の後: 次の加工の名前、工具交換、主軸を回し直して待つ
        assert_eq!(lines[at + 1], "(らせん)", "{g}");
        let after = &lines[at + 1..];
        let spin = after.iter().position(|l| l.starts_with("M3 ")).unwrap();
        assert_eq!(after[spin + 1], "G4 P2", "{g}");
        // 横の動きと切削は、主軸を回し直した後
        let first_move = after
            .iter()
            .position(|l| l.starts_with('X') || l.starts_with("G1"))
            .unwrap();
        assert!(spin < first_move, "{g}");
        // 任意停止も止まる側の M600 にし、注意を出す
        let manual = [(
            1,
            kmb_toolpath::Manual::Stop {
                optional: true,
                text: String::new(),
            },
        )];
        let p = Program::build_with("見本", 1, 20.0, &paths, &manual);
        let g = CarveraAir.emit(&p, &profile()).unwrap();
        assert!(g.contains("\nM600\n"), "{g}");
        assert!(g.contains("(注意: 任意停止"), "{g}");
        assert!(!g.contains("M1\n") && !g.contains("M0\n"), "{g}");
    }

    #[test]
    fn tools_are_checked_against_the_collets() {
        let p = sample();
        let d = CarveraAir.check(&p, &profile());
        // 平 6 mm はシャンク 6 mm。別売りのコレットへの交換を知らせる
        assert!(
            d.iter()
                .any(|d| d.severity == Severity::Warning && d.message.contains("交換")),
            "{d:?}"
        );
        assert!(!d.iter().any(|d| d.message.contains("T3")), "{d:?}");
        let mut p = sample();
        for op in &mut p.ops {
            if let Op::ToolChange { tool } = op {
                tool.shank_diameter = Some(5.0);
            }
        }
        let d = CarveraAir.check(&p, &profile());
        assert!(
            d.iter().any(|d| d.message.contains("合うコレット")),
            "{d:?}"
        );
        // 注意だけなので出力はできる
        assert!(CarveraAir.emit(&p, &profile()).is_ok());
    }
}
