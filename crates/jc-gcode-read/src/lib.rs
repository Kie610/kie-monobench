//! 書き出した G-code を読み直して、経路([`Toolpath`])に戻す。
//!
//! シミュレーションで「機械に送る G-code そのもの」を削るために使う。経路から直接削った形と
//! 比べれば、ポスト(G-code の書き方)の誤りが分かる。
//!
//! 読めるのは、このソフトのポストが書く範囲の G-code:
//!
//! - `G0`・`G1`・`G2`・`G3`(XY 平面の円弧。中心は `I`・`J` で始点から。Z が変わればらせん)
//! - `G17`・`G21`・`G90`・`G54`〜`G59`、`G4`(待ち)、`F`、`S`
//! - `M3`・`M4`・`M5`、`M6 T<n>`(`T<n> M6` も)、`M491`(工具長の測定)、`M7`・`M9`、`M30`
//! - 汎用のポストの `%`、`O` 番号の行、工具長の補正 `G43 H<n>`・`G49`。Grbl 系の工具交換
//!   (`M6` の代わりに書く「工具交換: T<n> …」のコメントと `M0`)
//! - Carvera のレーザー(`M321` で工具ライブラリのレーザーに替え、`M322` で戻す)と、
//!   高さの補正(`G32` の行と `M370`。動きはない)
//! - 丸括弧と `;` のコメント。工具交換の後のコメントは、そこから先の経路の名前になる
//!
//! 読めないもの(相対座標 `G91`、インチ `G20`、XY 以外の平面、`R` の円弧、固定サイクル)は、
//! 黙って違う形にせず、行の番号つきの誤りにする。

use jc_cam_tool::Tool;
use jc_toolpath::{ArcDir, Move, P3, Toolpath};

/// G-code を読む時に要るもの。
pub struct ReadOptions<'a> {
    /// 工具番号から工具の形を引く一覧
    pub tools: &'a [Tool],
    /// 経路の安全高さ(シミュレーションは使わない。経路の項目を埋めるため)
    pub safe_z: f64,
    pub retract_z: f64,
}

/// 読んだ経路。工具交換かコメントで区切る。早送りしかない区切りは捨てる。
pub fn read(text: &str, opt: &ReadOptions<'_>) -> Result<Vec<Toolpath>, String> {
    let mut r = Reader {
        opt,
        out: Vec::new(),
        cur: None,
        pending: String::new(),
        pos: [None; 3],
        motion: None,
        feed: None,
        rpm: 0.0,
    };
    for (n, line) in text.lines().enumerate() {
        r.line(line)
            .map_err(|e| format!("{} 行目「{}」: {e}", n + 1, line.trim()))?;
    }
    r.close();
    Ok(r.out)
}

struct Reader<'a, 'b> {
    opt: &'b ReadOptions<'a>,
    out: Vec<Toolpath>,
    /// 今の経路(工具が決まってから)
    cur: Option<Toolpath>,
    /// 次の経路の名前(最後に読んだコメント)
    pending: String,
    pos: [Option<f64>; 3],
    motion: Option<u32>,
    feed: Option<f64>,
    rpm: f64,
}

impl Reader<'_, '_> {
    fn close(&mut self) {
        if let Some(tp) = self.cur.take()
            && tp.moves.iter().any(|m| !matches!(m, Move::Rapid { .. }))
        {
            self.out.push(tp);
        }
    }

    /// 工具番号 `n` の工具。
    fn tool(&self, n: u32) -> Result<Tool, String> {
        self.opt
            .tools
            .iter()
            .find(|x| x.number == n)
            .cloned()
            .ok_or(format!("工具 T{n} の形が分かりません"))
    }

    /// 工具 `tool` で、名前 `name` の新しい経路を始める。
    fn start(&mut self, tool: Tool, name: String) {
        self.close();
        self.cur = Some(Toolpath {
            name,
            tool,
            rpm: self.rpm,
            safe_z: self.opt.safe_z,
            retract_z: self.opt.retract_z,
            moves: Vec::new(),
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
        });
    }

    fn line(&mut self, line: &str) -> Result<(), String> {
        let (code, comments) = split_comments(line)?;
        for c in comments {
            let c = c.trim().to_string();
            if c.is_empty() {
                continue;
            }
            // Grbl 系のポストは M6 の代わりにこのコメントと M0 を書く
            if let Some(rest) = c.strip_prefix("工具交換: T") {
                let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
                let n = n
                    .parse()
                    .map_err(|_| format!("「{c}」の工具番号が読めません"))?;
                let tool = self.tool(n)?;
                self.start(tool, self.pending.clone());
                continue;
            }
            // 工具が決まった後のコメントは、次の操作の名前。同じ工具のまま新しい経路にする
            let used = self
                .cur
                .as_ref()
                .map(|tp| (!tp.moves.is_empty(), tp.tool.clone()));
            match used {
                Some((true, tool)) => self.start(tool, c.clone()),
                Some((false, _)) => {
                    if let Some(tp) = &mut self.cur {
                        tp.name = c.clone();
                    }
                }
                None => {}
            }
            self.pending = c;
        }
        let words = parse_words(&code)?;
        if words.is_empty() {
            return Ok(());
        }
        // 高さの補正(Carvera の G32。A・B・H・I・J・R は測る範囲と点の数)は動きではない
        if words.iter().any(|w| w.0 == 'G' && w.1 == 32.0) {
            return Ok(());
        }
        let get = |l: char| words.iter().find(|w| w.0 == l).map(|w| w.1);
        let mut motion_word = None;
        for &(l, v) in &words {
            match l {
                'G' => {
                    let g = code_of(v)?;
                    match g {
                        0..=3 => motion_word = Some(g),
                        4 | 17 | 21 | 43 | 49 | 90 | 94 => {}
                        54..=59 => {}
                        20 => return Err("インチ(G20)は読めません".into()),
                        18 | 19 => return Err("XY 以外の平面(G18・G19)は読めません".into()),
                        91 => return Err("相対座標(G91)は読めません".into()),
                        80..=89 => return Err("固定サイクルは読めません".into()),
                        _ => return Err(format!("G{g} は読めません")),
                    }
                }
                'M' => {
                    let m = code_of(v)?;
                    match m {
                        6 => {
                            let t = get('T').ok_or("M6 に工具番号 T がありません")?;
                            let tool = self.tool(code_of(t)?)?;
                            let name = self.pending.clone();
                            self.start(tool, name);
                        }
                        // Carvera のレーザーに替える(工具ライブラリのレーザー)。戻す M322 の後は
                        // 次の M6 で工具が決まる
                        321 => {
                            let tool = self
                                .opt
                                .tools
                                .iter()
                                .find(|x| x.kind == jc_cam_tool::ToolKind::Laser)
                                .cloned()
                                .ok_or("M321 のレーザーが工具にありません")?;
                            let name = self.pending.clone();
                            self.start(tool, name);
                        }
                        // 600 は Carvera の一時停止(手動 NC の停止)。動きはない
                        3 | 4 | 5 | 7 | 8 | 9 | 30 | 2 | 0 | 1 | 322 | 370 | 491 | 600 => {}
                        _ => return Err(format!("M{m} は読めません")),
                    }
                }
                'S' => {
                    self.rpm = v;
                    if let Some(tp) = &mut self.cur {
                        tp.rpm = v;
                    }
                }
                'F' => self.feed = Some(v),
                'X' | 'Y' | 'Z' | 'I' | 'J' | 'T' | 'P' | 'N' | 'H' => {}
                'R' => return Err("R の円弧は読めません".into()),
                other => return Err(format!("{other} の語は読めません")),
            }
        }
        // 待ち(G4)の P は動きではない
        if words.iter().any(|w| w.0 == 'G' && w.1 == 4.0) {
            return Ok(());
        }
        if let Some(g) = motion_word {
            self.motion = Some(g);
        }
        let axes = [get('X'), get('Y'), get('Z')];
        if axes.iter().all(Option::is_none) {
            return Ok(());
        }
        let from = self.pos;
        for (p, a) in self.pos.iter_mut().zip(axes) {
            if let Some(v) = a {
                *p = Some(v);
            }
        }
        let motion = self.motion.ok_or("動きの種類(G0〜G3)が決まっていません")?;
        // 3 軸の位置がそろうまでは動きにしない(最初の G0 Z5 の後の X Y など)
        let (Some(to), Some(start)) = (full(self.pos), full(from)) else {
            return Ok(());
        };
        let modal_feed = self.feed;
        let Some(tp) = &mut self.cur else {
            return Ok(());
        };
        let feed = || modal_feed.ok_or("送り F が決まっていません");
        let m = match motion {
            0 => Move::Rapid { to },
            1 => Move::Linear { to, feed: feed()? },
            _ => {
                let (i, j) = (get('I').unwrap_or(0.0), get('J').unwrap_or(0.0));
                if i == 0.0 && j == 0.0 {
                    return Err("円弧の中心 I・J がありません".into());
                }
                Move::Arc {
                    to,
                    center: [start[0] + i, start[1] + j],
                    dir: if motion == 2 { ArcDir::Cw } else { ArcDir::Ccw },
                    feed: feed()?,
                }
            }
        };
        if tp.moves.is_empty() {
            // 経路の始めの位置(シミュレーションは最初の動きの行き先を始めの位置にする)
            tp.moves.push(Move::Rapid { to: start });
        }
        tp.moves.push(m);
        Ok(())
    }
}

fn full(p: [Option<f64>; 3]) -> Option<P3> {
    Some([p[0]?, p[1]?, p[2]?])
}

fn code_of(v: f64) -> Result<u32, String> {
    if v < 0.0 || v.fract() != 0.0 {
        return Err(format!("{v} は番号として読めません"));
    }
    Ok(v as u32)
}

/// 行をコードとコメントに分ける。丸括弧の中と、`;` から行の終わりまでがコメント。
fn split_comments(line: &str) -> Result<(String, Vec<String>), String> {
    let mut code = String::new();
    let mut comments = Vec::new();
    let mut paren: Option<String> = None;
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        match (&mut paren, ch) {
            (None, '(') => paren = Some(String::new()),
            (None, ';') => {
                comments.push(chars.by_ref().collect());
                break;
            }
            (None, c) => code.push(c),
            (Some(_), ')') => comments.push(paren.take().unwrap_or_default()),
            (Some(s), c) => s.push(c),
        }
    }
    if paren.is_some() {
        return Err("コメントの括弧が閉じていません".into());
    }
    Ok((code, comments))
}

/// `X10.5 Y-2 F600` のような語の列。
fn parse_words(code: &str) -> Result<Vec<(char, f64)>, String> {
    let s: Vec<char> = code
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    // `%` と O 番号の行(プログラムの始まり)は動きではない
    if s.first() == Some(&'%') || s.first() == Some(&'O') {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let l = s[i];
        if !l.is_ascii_alphabetic() {
            return Err(format!("「{l}」は読めません"));
        }
        i += 1;
        let start = i;
        while i < s.len() && (s[i].is_ascii_digit() || s[i] == '.' || s[i] == '-' || s[i] == '+') {
            i += 1;
        }
        let num: String = s[start..i].iter().collect();
        let v: f64 = num
            .parse()
            .map_err(|_| format!("{l} の後の数「{num}」が読めません"))?;
        out.push((l, v));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_cam_tool::ToolLibrary;

    fn opt(tools: &[Tool]) -> ReadOptions<'_> {
        ReadOptions {
            tools,
            safe_z: 5.0,
            retract_z: 1.0,
        }
    }

    #[test]
    /// 手動 NC の停止(Carvera の M600)は動きのない行として読み飛ばす。
    fn moves_arcs_and_tool_changes_are_read_back() {
        let lib = ToolLibrary::starter();
        let g = "(見本)\n(四角)\nG21\nG90\nG17\nG54\nM6 T1\nM491\nM3 S10000\nG4 P2\nG0 Z5\nX0 Y0\nZ1\n\
                 G1 Z-1 F200\nX10 F600\nY10\nG0 Z5\n(らせん)\nG0 Z20\nM5\nM600\nM6 T3\nM3 S12000\nG0 Z5\n\
                 X22 Y20\nZ1\nG1 Z0 F150\nG3 X18 Y20 Z-0.3 I-2 J0 F400\nX22 Y20 Z-0.6 I2 J0\nG0 Z5\nM30\n";
        let paths = read(g, &opt(&lib.tools)).unwrap();
        let names: Vec<&str> = paths.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["四角", "らせん"]);
        assert_eq!(paths[0].tool.number, 1);
        assert_eq!(paths[1].tool.number, 3);
        assert_eq!(paths[1].rpm, 12000.0);
        let a = &paths[0].moves;
        assert_eq!(
            a[0],
            Move::Rapid {
                to: [0.0, 0.0, 5.0]
            }
        );
        assert_eq!(
            a[3],
            Move::Linear {
                to: [10.0, 0.0, -1.0],
                feed: 600.0
            }
        );
        let Move::Arc {
            to,
            center,
            dir,
            feed,
        } = paths[1].moves[paths[1].moves.len() - 3]
        else {
            panic!("{:?}", paths[1].moves)
        };
        assert_eq!(
            (to, center, dir, feed),
            ([18.0, 20.0, -0.3], [20.0, 20.0], ArcDir::Ccw, 400.0)
        );
        // 2 本目の円弧は modal の G3 で、中心は始点 (18, 20) から I2
        assert!(matches!(
            paths[1].moves[paths[1].moves.len() - 2],
            Move::Arc {
                center: [20.0, 20.0],
                ..
            }
        ));
    }

    #[test]
    fn what_cannot_be_read_is_an_error_with_the_line() {
        let lib = ToolLibrary::starter();
        for (g, want) in [
            ("M6 T1\nG91\n", "2 行目"),
            ("M6 T1\nG20\n", "インチ"),
            ("M6 T42\n", "T42"),
            ("M6 T1\nG0 X0 Y0 Z5\nG2 X1 Y1 R1 F100\n", "R の円弧"),
            ("M6 T1\nG0 X0 Y0 Z5\nG1 X1\n", "送り F"),
            ("(閉じない\n", "括弧"),
        ] {
            let e = read(g, &opt(&lib.tools)).unwrap_err();
            assert!(e.contains(want), "{g:?}: {e}");
        }
    }
}
