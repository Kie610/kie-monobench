//! ポストのプラグインの見本。Grbl 系の書き方(G21/G90/G17/G54、G2/G3 と I/J、M6 T、M3 S、
//! M30)で G-code を書く。実機では確かめていない(U)。プラグインの書き方の見本として使う。

use jc_post::{MachineProfile, num};
use jc_toolpath::{ArcDir, Move, Op, Program};
use std::fmt::Write;

wit_bindgen::generate!({
    path: "../../crates/jc-plugin-host/wit",
    world: "post",
});

use jisaku_cadcam::post::types::Severity;

struct Example;

fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        message: message.into(),
    }
}

fn parse(program: &str, machine: &str) -> Result<(Program, MachineProfile), Vec<Diagnostic>> {
    let p = serde_json::from_str(program)
        .map_err(|e| vec![error(format!("プログラムが読めません: {e}"))])?;
    let m = serde_json::from_str(machine)
        .map_err(|e| vec![error(format!("機械プロファイルが読めません: {e}"))])?;
    Ok((p, m))
}

impl Guest for Example {
    fn id() -> String {
        "example-grbl".into()
    }

    fn describe() -> String {
        "見本: Grbl 系の G-code(実機未確認)".into()
    }

    fn check(program: String, machine: String) -> Vec<Diagnostic> {
        match parse(&program, &machine) {
            Err(d) => d,
            Ok((p, _)) if !(1..=6).contains(&p.wcs) => {
                vec![error(format!("作業座標系 {} は G54〜G59 の外です", p.wcs))]
            }
            Ok(_) => vec![],
        }
    }

    fn emit(program: String, machine: String) -> Result<String, Vec<Diagnostic>> {
        let (p, m) = parse(&program, &machine)?;
        let mut out = String::new();
        let _ = writeln!(out, "({} / {})", Self::id(), m.name);
        let _ = writeln!(out, "G21 G90 G17 G{}", 53 + p.wcs);
        let mut pos: Option<[f64; 3]> = None;
        for op in &p.ops {
            match op {
                Op::Comment { text } => {
                    let t: String = text.chars().filter(|c| *c != '(' && *c != ')').collect();
                    let _ = writeln!(out, "({t})");
                }
                Op::ToolChange { tool } => {
                    let _ = writeln!(out, "M6 T{}", tool.number);
                    pos = None;
                }
                Op::Spindle { rpm } => {
                    let _ = writeln!(out, "M3 S{}", num(*rpm));
                }
                Op::SpindleStop => out.push_str("M5\n"),
                Op::Dwell { seconds } => {
                    let _ = writeln!(out, "G4 P{}", num(*seconds));
                }
                // ホスト(jc-plugin-host)は、後から足した知らせをコメントにし、停止を含む
                // プログラムは渡さない。ここに来るのは、ホストを通さずに呼んだ時だけ
                Op::Message { text } => {
                    let t: String = text.chars().filter(|c| *c != '(' && *c != ')').collect();
                    let _ = writeln!(out, "(知らせ: {t})");
                }
                Op::Stop { .. } => {
                    return Err(vec![error("見本のプラグインは停止の命令を書けません")]);
                }
                Op::Retract { z } => {
                    let _ = writeln!(out, "G0 Z{}", num(*z));
                    if let Some(q) = &mut pos {
                        q[2] = *z;
                    }
                }
                Op::Move(mv) => {
                    let to = mv.to();
                    match *mv {
                        Move::Rapid { .. } => {
                            if pos.is_none() {
                                let _ = writeln!(out, "G0 Z{}", num(to[2]));
                            }
                            let _ =
                                writeln!(out, "G0 X{} Y{} Z{}", num(to[0]), num(to[1]), num(to[2]));
                        }
                        Move::Linear { feed, .. } => {
                            let _ = writeln!(
                                out,
                                "G1 X{} Y{} Z{} F{}",
                                num(to[0]),
                                num(to[1]),
                                num(to[2]),
                                num(feed)
                            );
                        }
                        Move::Arc {
                            center, dir, feed, ..
                        } => {
                            let Some(from) = pos else {
                                return Err(vec![error("位置が決まる前に円弧があります")]);
                            };
                            let g = match dir {
                                ArcDir::Cw => "G2",
                                ArcDir::Ccw => "G3",
                            };
                            let _ = writeln!(
                                out,
                                "{g} X{} Y{} Z{} I{} J{} F{}",
                                num(to[0]),
                                num(to[1]),
                                num(to[2]),
                                num(center[0] - from[0]),
                                num(center[1] - from[1]),
                                num(feed)
                            );
                        }
                    }
                    pos = Some(to);
                }
            }
        }
        out.push_str("M30\n");
        Ok(out)
    }
}

export!(Example);
