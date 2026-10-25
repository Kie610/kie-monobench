//! G-code の動きの行を書く所(内蔵のポストが共通に使う)。
//!
//! 同じ値をくり返し書かない(変わった軸と送りだけ)。円弧は半周以下に分け、XY を必ず書く。

use crate::{Diagnostic, PostError, Severity, num};
use kmb_toolpath::{ArcDir, Move, P3, arc_sweep};
use std::f64::consts::PI;
use std::fmt::Write;

/// 出力中の機械の状態。
#[derive(Default)]
pub struct Writer {
    pos: Option<P3>,
    motion: Option<&'static str>,
    feed: Option<f64>,
    /// 次の送りの行に付ける S の値(レーザーの出力)。付けたら `None` に戻す
    power: Option<String>,
    /// 最後に書いた S の値(同じ値をくり返し書かない)
    written_power: Option<String>,
}

impl Writer {
    /// 位置が分からなくなった(工具交換・停止の後)。次の早送りは Z を先に動かす。
    pub fn forget(&mut self) {
        *self = Writer {
            power: self.power.take(),
            written_power: self.written_power.take(),
            ..Writer::default()
        };
    }

    /// 次の早送りの行で G0 を書き直す(停止の後など)。
    pub fn forget_motion(&mut self) {
        self.motion = None;
    }

    /// 次の送りの行に `S<s>` を付ける(レーザーの出力。前に書いた値と同じなら付けない)。
    pub fn power_on_next_feed(&mut self, s: String) {
        self.power = (self.written_power.as_ref() != Some(&s)).then_some(s);
    }

    /// 出力を書いたことを忘れる(レーザーを止めた後。次は必ず S を書く)。
    pub fn forget_power(&mut self) {
        self.power = None;
        self.written_power = None;
    }

    /// Z だけを早送りで上げる。主軸や工具の命令の後でも読み違えないよう、G0 を毎回書く。
    pub fn retract(&mut self, out: &mut String, z: f64) {
        let _ = writeln!(out, "G0 Z{}", num(z));
        self.motion = Some("G0");
        if let Some(p) = &mut self.pos {
            p[2] = z;
        }
    }

    /// 動き 1 つ。
    pub fn motion(&mut self, out: &mut String, mv: &Move) -> Result<(), PostError> {
        match *mv {
            Move::Rapid { to } => {
                if self.pos.is_none() {
                    // 位置が分からない時は、まず Z を上げてから XY へ
                    let _ = writeln!(out, "G0 Z{}", num(to[2]));
                    let _ = writeln!(out, "X{} Y{}", num(to[0]), num(to[1]));
                    self.motion = Some("G0");
                    self.pos = Some(to);
                } else {
                    self.word(out, "G0", to, None);
                }
            }
            Move::Linear { to, feed } => self.word(out, "G1", to, Some(feed)),
            Move::Arc {
                to,
                center,
                dir,
                feed,
            } => {
                let Some(from) = self.pos else {
                    return Err(PostError(vec![Diagnostic {
                        severity: Severity::Error,
                        message: "位置が決まる前に円弧があります".into(),
                    }]));
                };
                for (a, b) in split_arc(from, to, center, dir) {
                    let g = match dir {
                        ArcDir::Cw => "G2",
                        ArcDir::Ccw => "G3",
                    };
                    let ij = format!("I{} J{}", num(center[0] - a[0]), num(center[1] - a[1]));
                    // 円弧は XY を必ず書く(省くと全周と読まれることがある)
                    self.pos = Some(a);
                    self.word_arc(out, g, b, feed, &ij);
                }
            }
        }
        Ok(())
    }

    /// G0 か G1 の 1 行。変わった軸だけを書く。
    fn word(&mut self, out: &mut String, motion: &'static str, to: P3, feed: Option<f64>) {
        let mut words: Vec<String> = Vec::new();
        for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
            if self.pos.is_none_or(|p| num(p[i]) != num(to[i])) {
                words.push(format!("{axis}{}", num(to[i])));
            }
        }
        self.pos = Some(to);
        if words.is_empty() {
            return;
        }
        if self.motion != Some(motion) {
            words.insert(0, motion.to_string());
            self.motion = Some(motion);
        }
        if let Some(f) = feed
            && self.feed != Some(f)
        {
            words.push(format!("F{}", num(f)));
            self.feed = Some(f);
        }
        if feed.is_some()
            && let Some(s) = self.power.take()
        {
            words.push(format!("S{s}"));
            self.written_power = Some(s);
        }
        out.push_str(&words.join(" "));
        out.push('\n');
    }

    fn word_arc(&mut self, out: &mut String, g: &'static str, to: P3, feed: f64, ij: &str) {
        let mut line = String::new();
        if self.motion != Some(g) {
            line.push_str(g);
            line.push(' ');
            self.motion = Some(g);
        }
        let _ = write!(line, "X{} Y{}", num(to[0]), num(to[1]));
        if self.pos.is_none_or(|p| num(p[2]) != num(to[2])) {
            let _ = write!(line, " Z{}", num(to[2]));
        }
        let _ = write!(line, " {ij}");
        if self.feed != Some(feed) {
            let _ = write!(line, " F{}", num(feed));
            self.feed = Some(feed);
        }
        if let Some(s) = self.power.take() {
            let _ = write!(line, " S{s}");
            self.written_power = Some(s);
        }
        out.push_str(&line);
        out.push('\n');
        self.pos = Some(to);
    }
}

/// コメントの 1 行(括弧はコメントの中に書けないので除く)。
pub fn comment(out: &mut String, s: &str) {
    let s: String = s.chars().filter(|c| *c != '(' && *c != ')').collect();
    let _ = writeln!(out, "({s})");
}

/// 円弧を半周以下に分ける(Z はらせんに沿って配る)。
pub fn split_arc(from: P3, to: P3, center: [f64; 2], dir: ArcDir) -> Vec<(P3, P3)> {
    let sweep = arc_sweep(from, to, center, dir);
    let n = (sweep.abs() / PI - 1e-9).ceil().max(1.0) as usize;
    let r = (from[0] - center[0]).hypot(from[1] - center[1]);
    let a0 = (from[1] - center[1]).atan2(from[0] - center[0]);
    let mut pts = vec![from];
    for k in 1..n {
        let t = k as f64 / n as f64;
        let a = a0 + sweep * t;
        pts.push([
            center[0] + r * a.cos(),
            center[1] + r * a.sin(),
            from[2] + (to[2] - from[2]) * t,
        ]);
    }
    pts.push(to);
    pts.windows(2).map(|w| (w[0], w[1])).collect()
}
