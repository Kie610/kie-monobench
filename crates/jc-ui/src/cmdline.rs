//! コマンド欄(名前と書き方は独自)。
//!
//! 1 行に 1 つのコマンドを打つ。画面の部品を名前で探して人と同じ入力で操作するコマンド
//! (`click "熱"`・`type "まわりの温度(℃)" 50`)と、文書を内部で操作するコマンド(`set_param`・
//! `undo` など。MCP の `app` と同じ中身)の両方を受ける。何行も並べたスクリプトも流せ、
//! 途中で失敗したら残りは流さない(バッチの e2e と MCP の `script` が使う)。
//! 欄そのものも画面の部品なので、`type "コマンド" "click 熱" enter` のようにコマンドで打てる。
//!
//! 画面の操作は 1 つずつ、出来事を入れてから数フレーム待ち、再計算が終わってから次へ進む
//! (人が結果を見てから次を押すのと同じ)。

use crate::Workspace;
use crate::drive::{self, Query, UiDriver, UiNode};
use egui::{Modifiers, PointerButton};
use jc_session::roots::Roots;
use serde_json::{Value, json};
use std::collections::VecDeque;

/// コマンドの一覧と説明(`help` と補完に使う)。
pub const COMMANDS: &[(&str, &str)] = &[
    ("click", "click <部品>  部品の中央を左で押す"),
    ("rclick", "rclick <部品>  右で押す(右クリックのメニュー)"),
    ("dclick", "dclick <部品>  2 回押す"),
    ("hover", "hover <部品>  上にマウスを置く"),
    (
        "scroll",
        "scroll <部品> <点>  部品の上でホイールを回し、下へ <点> だけ巻く(負なら上へ。長い一覧や G-code の窓の終わりを見る)",
    ),
    (
        "type",
        "type <欄> <文字> [tab|enter|none]  欄を押して中身を全部選び、打って確定する(既定は tab)",
    ),
    ("key", "key <キー>  例: key Enter・key Escape・key ctrl+z"),
    ("select", "select <一覧> <項目>  一覧を開いて項目を選ぶ"),
    (
        "pick",
        "pick <面の名前> [ctrl]  3D のビューでその面が見えている所を押す(例: pick f8:end)。辺は両側の面の名前を | でつなぐ(例: pick f12:end|f12:side:e3)。頂点は集まる 3 つ以上の面の名前をつなぐ(例: pick f2:end|f2:side:e1|f2:side:e2)。作る手順の候補(領域・軸の線)は 候補:<名前>(例: pick 候補:原点の Z 軸)。ctrl を付けると Ctrl を押しながら押す",
    ),
    (
        "drag",
        "drag <面の名前|部品> <右へ> <下へ>  3D のビューでその面が見えている所(面でなければ画面の部品の中央)を左で押し、画面の点で右と下へ引いて離す(部品を引いて動かす・タブを並べ替える。例: drag f3:end 120 -40、drag \"(無題)\" -150 0)",
    ),
    (
        "expect",
        "expect <部品> [exists|missing|enabled|disabled|= <値>|~ <含む字>]  確かめる(違えば失敗)",
    ),
    (
        "until",
        "until <部品> [exists|missing|enabled|disabled|= <値>|~ <含む字>] [<秒>s]  そうなるまで待つ(既定 120 秒)",
    ),
    ("find", "find <部品>  合う部品を挙げる"),
    ("tree", "tree [<含む字>]  画面の部品を全部挙げる"),
    (
        "wait",
        "wait [<フレーム数>]  再計算が終わるまで(数を書けばその数だけ)待つ",
    ),
    (
        "pane",
        "pane <部品の名前> [on|off]  ブラウザ・編集パネル・タイムライン・コマンドを出す・隠す",
    ),
    ("shot", "shot <名前>  画面の写真(バッチの e2e で撮る)"),
    ("echo", "echo <文字>  記録に字を残す"),
    ("status", "status  文書の要約(MCP の app の status と同じ)"),
    ("selection", "selection  選んでいる物"),
    ("set_param", "set_param <名前> <式>  パラメータの式を変える"),
    ("add_param", "add_param <名前> <式>  パラメータを足す"),
    (
        "set_value",
        "set_value <機能の ID> <欄> <式>  機能の値を変える",
    ),
    (
        "actions",
        "actions  使える操作の一覧(ID・使えない理由・受け取る引数)",
    ),
    (
        "run",
        "run <操作の ID> [鍵=値 …]  操作を ID で実行する(引数は actions の宣言のとおり。単位つきの値は 鍵=\"0.05 mm\" のように書く)",
    ),
    ("undo", "undo  取り消す"),
    ("redo", "redo  やり直す"),
    ("clear", "clear  コマンド欄の記録を消す"),
    ("help", "help [<コマンド>]  一覧と書き方"),
];

/// 部品の押し方。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Press {
    Left,
    Right,
    Double,
}

/// 打った後の確定のしかた。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Commit {
    Tab,
    Enter,
    None,
}

/// 確かめ方。
#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    Exists,
    Missing,
    Enabled,
    Disabled,
    Eq(String),
    Contains(String),
}

/// 読み終えたコマンド。
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Press(Query, Press),
    Hover(Query),
    /// 部品の上でホイールを回す(下へ巻く点の数)
    Scroll(Query, f32),
    Type(Query, String, Commit),
    Key(String),
    Select(Query, String),
    /// 面・辺の名前か `候補:<名前>`、Ctrl を押しながらか
    Pick(String, bool),
    /// 面の名前と、引く量(画面の点。右・下が正)
    Drag(String, f32, f32),
    Expect(Query, Check),
    /// そうなるまで待つ(秒)
    Until(Query, Check, f32),
    Find(Query),
    Tree(String),
    Wait(Option<u32>),
    Pane(String, Option<bool>),
    Shot(String),
    Echo(String),
    Clear,
    Help(Option<String>),
    /// 文書を内部で操作する(MCP の app と同じ道具)
    Internal(String, Value),
}

/// 1 行を言葉に分ける(`"..."` の中は空白も含めて 1 つ。`\"` で引用符)。
pub fn split(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut had = false;
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' if quoted && matches!(it.peek(), Some('"') | Some('\\')) => {
                cur.push(it.next().expect("見た"));
            }
            '"' => {
                quoted = !quoted;
                had = true;
            }
            c if c.is_whitespace() && !quoted => {
                if had || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    had = false;
                }
            }
            c => cur.push(c),
        }
    }
    if quoted {
        return Err("引用符 \" が閉じていません".into());
    }
    if had || !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

/// 1 行を読む。空行と `#` で始まる行は `None`。
pub fn parse(line: &str) -> Result<Option<Cmd>, String> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    let w = split(t)?;
    let verb = w[0].to_ascii_lowercase();
    let args = &w[1..];
    let usage = || {
        COMMANDS
            .iter()
            .find(|(n, _)| *n == verb)
            .map_or_else(|| verb.clone(), |(_, u)| u.to_string())
    };
    let need = |n: usize| {
        if args.len() < n {
            Err(format!("書き方: {}", usage()))
        } else {
            Ok(())
        }
    };
    let q = |i: usize| Query::parse(&args[i]);
    let rest = |i: usize| args[i..].join(" ");
    let cmd = match verb.as_str() {
        "click" | "rclick" | "dclick" => {
            need(1)?;
            let p = match verb.as_str() {
                "click" => Press::Left,
                "rclick" => Press::Right,
                _ => Press::Double,
            };
            Cmd::Press(q(0), p)
        }
        "hover" => {
            need(1)?;
            Cmd::Hover(q(0))
        }
        "scroll" => {
            need(2)?;
            let by: f32 = args[1]
                .parse()
                .map_err(|_| format!("巻く量 {} は数ではありません", args[1]))?;
            Cmd::Scroll(q(0), by)
        }
        "type" => {
            need(2)?;
            let commit = match args.get(2).map(|s| s.to_ascii_lowercase()).as_deref() {
                None | Some("tab") => Commit::Tab,
                Some("enter") => Commit::Enter,
                Some("none") => Commit::None,
                Some(o) => return Err(format!("確定のしかた {o} は知りません(tab・enter・none)")),
            };
            Cmd::Type(q(0), args[1].clone(), commit)
        }
        "key" => {
            need(1)?;
            drive::parse_key(&args[0])?;
            Cmd::Key(args[0].clone())
        }
        "select" => {
            need(2)?;
            Cmd::Select(q(0), args[1].clone())
        }
        "pick" => {
            need(1)?;
            let ctrl = match args.get(1).map(|s| s.to_ascii_lowercase()).as_deref() {
                None => false,
                Some("ctrl") => true,
                Some(o) => return Err(format!("{o} は知りません(pick <名前> [ctrl])")),
            };
            Cmd::Pick(args[0].clone(), ctrl)
        }
        "drag" => {
            need(3)?;
            let num = |a: &String| {
                a.parse::<f32>()
                    .map_err(|_| format!("引く量 {a} は数ではありません"))
            };
            Cmd::Drag(args[0].clone(), num(&args[1])?, num(&args[2])?)
        }
        "expect" | "until" => {
            need(1)?;
            // until の最後の `30s` は待つ長さ。= と ~ は比べる値の後にある時だけ(`~ 1s` の 1s は値)
            let mut args = args.to_vec();
            let mut secs = 120.0;
            let with_value = matches!(args.get(1).map(String::as_str), Some("=" | "~"));
            if verb == "until"
                && args.len() > if with_value { 3 } else { 1 }
                && let Some(t) = args
                    .last()
                    .and_then(|a| a.strip_suffix('s'))
                    .and_then(|n| n.parse::<f32>().ok())
            {
                secs = t;
                args.pop();
            }
            let args = &args[..];
            let rest = |i: usize| args[i..].join(" ");
            // 待つ長さを外した後の語で数える(外の need は外す前の語を見ている)
            let need = |n: usize| {
                if args.len() < n {
                    Err(format!("書き方: {}", usage()))
                } else {
                    Ok(())
                }
            };
            let check = match args.get(1).map(String::as_str) {
                None | Some("exists") => Check::Exists,
                Some("missing") => Check::Missing,
                Some("enabled") => Check::Enabled,
                Some("disabled") => Check::Disabled,
                Some("=") => {
                    need(3)?;
                    Check::Eq(rest(2))
                }
                Some("~") => {
                    need(3)?;
                    Check::Contains(rest(2))
                }
                Some(o) => return Err(format!("確かめ方 {o} は知りません。書き方: {}", usage())),
            };
            let q = Query::parse(&args[0]);
            if verb == "until" {
                Cmd::Until(q, check, secs)
            } else {
                Cmd::Expect(q, check)
            }
        }
        "find" => {
            need(1)?;
            Cmd::Find(q(0))
        }
        "tree" => Cmd::Tree(rest(0)),
        "wait" => Cmd::Wait(match args.first() {
            None => None,
            Some(n) => Some(n.parse().map_err(|_| format!("{n} は数ではありません"))?),
        }),
        "pane" => {
            need(1)?;
            let on = match args.get(1).map(String::as_str) {
                None => None,
                Some("on") => Some(true),
                Some("off") => Some(false),
                Some(o) => return Err(format!("{o} は on か off で書いてください")),
            };
            Cmd::Pane(args[0].clone(), on)
        }
        "shot" => {
            need(1)?;
            Cmd::Shot(args[0].clone())
        }
        "echo" => Cmd::Echo(rest(0)),
        "clear" => Cmd::Clear,
        "help" => Cmd::Help(args.first().cloned()),
        "status" | "selection" | "undo" | "redo" | "actions" => {
            Cmd::Internal(verb.clone(), json!({}))
        }
        "run" => {
            need(1)?;
            let given = jc_session::run_args::parse_pairs(&args[1..])?;
            Cmd::Internal(verb.clone(), json!({"id": args[0], "args": given}))
        }
        "set_param" | "add_param" => {
            need(2)?;
            Cmd::Internal(verb.clone(), json!({"name": args[0], "expr": rest(1)}))
        }
        "set_value" => {
            need(3)?;
            let id: u64 = args[0]
                .parse()
                .map_err(|_| format!("機能の ID {} は数で書いてください", args[0]))?;
            Cmd::Internal(
                verb.clone(),
                json!({"feature": id, "key": args[1], "expr": rest(2)}),
            )
        }
        _ => {
            let near: Vec<&str> = COMMANDS
                .iter()
                .map(|(n, _)| *n)
                // 先頭の 1 字で比べる(バイトで切ると、日本語の字の途中で切れて落ちる)
                .filter(|n| verb.chars().next().is_some_and(|c| n.starts_with(c)))
                .collect();
            return Err(format!(
                "コマンド {verb} は知りません(help で一覧{})",
                if near.is_empty() {
                    String::new()
                } else {
                    format!("。近い物: {}", near.join("・"))
                }
            ));
        }
    };
    Ok(Some(cmd))
}

/// 記録の 1 行の種類。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// 打った(流した)コマンド
    Input,
    Ok,
    Err,
}

/// 流し終えたスクリプトの結果(MCP の `script` とバッチの e2e が読む)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptResult {
    pub ok: bool,
    /// 行ごとの (コマンド, 結果)。失敗した行の後は流さないので入らない
    pub lines: Vec<(String, Result<String, String>)>,
    /// 撮るよう頼まれた写真の名前(画面の無い試験が撮る)
    pub shots: Vec<String>,
}

impl ScriptResult {
    /// 人が読む記録。
    pub fn report(&self) -> String {
        let mut s = String::new();
        for (line, r) in &self.lines {
            s += &format!("> {line}\n");
            match r {
                Ok(t) if !t.is_empty() => s += &format!("  {}\n", t.replace('\n', "\n  ")),
                Ok(_) => {}
                Err(e) => s += &format!("  失敗: {}\n", e.replace('\n', "\n  ")),
            }
        }
        s
    }

    pub fn to_json(&self) -> Value {
        json!({
            "ok": self.ok,
            "lines": self.lines.iter().map(|(l, r)| match r {
                Ok(t) => json!({"command": l, "ok": t}),
                Err(e) => json!({"command": l, "error": e}),
            }).collect::<Vec<_>>(),
        })
    }
}

struct Job {
    line: String,
    cmd: Cmd,
    batch: u64,
    /// AI から来たスクリプトなら、AI に許したフォルダ(ファイルを読む引数をこの中に限る)
    roots: Option<Roots>,
}

struct Running {
    job: Job,
    phase: u32,
    /// 次の段まで待つフレームの数
    wait: u32,
    /// 前の段で決めた部品(欄など)
    target: Option<UiNode>,
    /// 引き始めた所(drag)
    from: Option<egui::Pos2>,
    /// 待ち始めてからのフレーム(木が来ない・終わらない時に諦める)
    frames: u32,
    started: std::time::Instant,
}

/// コマンド欄の状態(窓に 1 つ。タブを替えても続く)。
#[derive(Default)]
pub(crate) struct CmdLine {
    pub input: String,
    pub log: Vec<(Kind, String)>,
    history: Vec<String>,
    /// ↑ で呼び戻している履歴の位置
    hist_at: Option<usize>,
    queue: VecDeque<Job>,
    running: Option<Running>,
    next_batch: u64,
    /// 流している途中のスクリプト(batch → 途中の結果)
    batches: Vec<(u64, ScriptResult)>,
    /// 流し終えたスクリプト
    pub(crate) done: Vec<(u64, ScriptResult)>,
    /// 記録を一番下まで巻く
    scroll: bool,
    /// 撮るよう頼まれた写真(画面の無い試験が [`Workspace::take_shot`] で受け取る)
    shot: Option<String>,
    /// AI からの頼みを流している間の、AI に許したフォルダ(操作のファイルの引数をこの中に限る)
    pub(crate) ai_roots: Option<Roots>,
}

/// 出来事を入れてから次の段までのフレーム(入力 → 部品が答える → 木が新しくなる)。
const SETTLE: u32 = 3;
/// drag で引く途中のコマの数(最後のコマで離す)
const DRAG_STEPS: u32 = 8;
/// 1 つのコマンドで木や再計算を待つ上限のフレーム。
const GIVE_UP: u32 = 600;

enum Step {
    Done(Result<String, String>),
    /// 待ってから次の段へ
    Next(u32),
    /// 何もせずにもう一度(木や再計算を待つ)
    Again,
}

impl Workspace {
    /// 内部の道具を流す。AI から来たスクリプトの行なら、その間だけ AI に許したフォルダを当てる。
    fn live_request_in(
        &mut self,
        roots: &Option<Roots>,
        tool: &str,
        args: &Value,
    ) -> Result<crate::live_ui::Answer, String> {
        let outer = std::mem::replace(&mut self.cmdline.ai_roots, roots.clone());
        let a = self.live_request(tool, args);
        self.cmdline.ai_roots = outer;
        a
    }

    /// スクリプト(何行でも)を流し始める。返す番号で終わりを [`Self::script_result`] から読む。
    /// 読めない行があれば、何も流さずにその行を返す。
    pub fn run_script(&mut self, text: &str) -> Result<u64, String> {
        let mut jobs = Vec::new();
        let batch = self.cmdline.next_batch;
        for (i, line) in text.lines().enumerate() {
            match parse(line) {
                Ok(Some(cmd)) => jobs.push(Job {
                    line: line.trim().to_string(),
                    cmd,
                    batch,
                    roots: self.cmdline.ai_roots.clone(),
                }),
                Ok(None) => {}
                Err(e) => return Err(format!("{} 行目「{}」: {e}", i + 1, line.trim())),
            }
        }
        self.cmdline.next_batch += 1;
        let empty = jobs.is_empty();
        self.cmdline.queue.extend(jobs);
        self.cmdline.batches.push((
            batch,
            ScriptResult {
                ok: true,
                ..Default::default()
            },
        ));
        if empty {
            self.finish_batch(batch);
        }
        if let Some(c) = &self.win.repaint {
            c.request_repaint();
        }
        Ok(batch)
    }

    /// 流し終えたスクリプトの結果(まだなら `None`)。読むと消える。
    pub fn script_result(&mut self, batch: u64) -> Option<ScriptResult> {
        let i = self.cmdline.done.iter().position(|(b, _)| *b == batch)?;
        Some(self.cmdline.done.remove(i).1)
    }

    /// コマンドを流している途中か。
    pub fn script_running(&self) -> bool {
        self.cmdline.running.is_some() || !self.cmdline.queue.is_empty()
    }

    /// `shot` で頼まれた写真の名前(読むと消える。試験が絵にして書き出す)。
    pub fn take_shot(&mut self) -> Option<String> {
        self.cmdline.shot.take()
    }

    /// コマンド欄の記録(試験と MCP が読む)。
    pub fn command_log(&self) -> Vec<(Kind, String)> {
        self.cmdline.log.clone()
    }

    fn finish_batch(&mut self, batch: u64) {
        if let Some(i) = self.cmdline.batches.iter().position(|(b, _)| *b == batch) {
            let r = self.cmdline.batches.remove(i);
            self.cmdline.done.push(r);
        }
    }

    /// 毎フレーム: 流しているコマンドを 1 段進める。
    pub(crate) fn cmdline_tick(&mut self, ctx: &egui::Context) {
        if self.cmdline.running.is_none() {
            let Some(job) = self.cmdline.queue.pop_front() else {
                return;
            };
            self.cmdline.log.push((Kind::Input, job.line.clone()));
            self.cmdline.scroll = true;
            log::info!("コマンド: {}", job.line);
            self.cmdline.running = Some(Running {
                job,
                phase: 0,
                wait: 0,
                target: None,
                from: None,
                frames: 0,
                started: std::time::Instant::now(),
            });
        }
        ctx.request_repaint();
        let mut r = self.cmdline.running.take().expect("上で入れた");
        r.frames += 1;
        if r.wait > 0 {
            r.wait -= 1;
            self.cmdline.running = Some(r);
            return;
        }
        // 再計算の途中と、ホームがフォルダを読み直している途中は待つ(人も結果や一覧が出てから
        // 次を押す)。待つこと自体のコマンドも同じ
        let is_ui = !matches!(
            r.job.cmd,
            Cmd::Echo(_) | Cmd::Clear | Cmd::Help(_) | Cmd::Shot(_) | Cmd::Until(..)
        );
        let busy = self.is_busy()
            || self
                .home
                .page
                .as_ref()
                .is_some_and(|p| p.loading(ctx.cumulative_pass_nr()));
        let step = if is_ui && busy && r.frames < GIVE_UP * 20 {
            Step::Again
        } else if r.frames > GIVE_UP * 20 && !matches!(r.job.cmd, Cmd::Until(..)) {
            Step::Done(Err("再計算かフォルダの読み直しが終わりません".into()))
        } else {
            self.cmd_step(ctx, &mut r)
        };
        match step {
            Step::Again => {
                if r.frames > GIVE_UP && !busy && !matches!(r.job.cmd, Cmd::Until(..)) {
                    self.end_job(r, Err("画面の部品の一覧が届きません".into()));
                } else {
                    self.cmdline.running = Some(r);
                }
            }
            Step::Next(w) => {
                r.phase += 1;
                r.wait = w;
                self.cmdline.running = Some(r);
            }
            Step::Done(res) => self.end_job(r, res),
        }
    }

    fn end_job(&mut self, r: Running, res: Result<String, String>) {
        let batch = r.job.batch;
        match &res {
            Ok(t) => {
                log::info!("コマンド: {} → {t}", r.job.line);
                if !t.is_empty() {
                    self.cmdline.log.push((Kind::Ok, t.clone()));
                }
            }
            Err(e) => {
                log::warn!("コマンド: {} → 失敗: {e}", r.job.line);
                self.cmdline.log.push((Kind::Err, e.clone()));
            }
        }
        self.cmdline.scroll = true;
        let failed = res.is_err();
        if let Some((_, b)) = self.cmdline.batches.iter_mut().find(|(b, _)| *b == batch) {
            if let Cmd::Shot(name) = &r.job.cmd {
                b.shots.push(name.clone());
            }
            b.lines.push((r.job.line.clone(), res));
            if failed {
                b.ok = false;
            }
        }
        if failed {
            // 失敗したスクリプトの残りは流さない
            let before = self.cmdline.queue.len();
            self.cmdline.queue.retain(|j| j.batch != batch);
            let skipped = before - self.cmdline.queue.len();
            if skipped > 0 {
                self.cmdline
                    .log
                    .push((Kind::Err, format!("残りの {skipped} 行は流しません")));
            }
        }
        if !self.cmdline.queue.iter().any(|j| j.batch == batch) {
            self.finish_batch(batch);
        }
    }

    fn ui_nodes(&self, ctx: &egui::Context) -> Option<Vec<UiNode>> {
        let d = ctx.plugin::<UiDriver>();
        let d = d.lock();
        d.ready().then(|| d.nodes(ctx.pixels_per_point()))
    }

    fn push_events(ctx: &egui::Context, ev: Vec<egui::Event>) {
        ctx.plugin::<UiDriver>().lock().push(ev);
    }

    /// コマンド 1 つの 1 段。
    fn cmd_step(&mut self, ctx: &egui::Context, r: &mut Running) -> Step {
        let needs_tree = matches!(
            r.job.cmd,
            Cmd::Press(..)
                | Cmd::Pick(..)
                | Cmd::Drag(..)
                | Cmd::Hover(_)
                | Cmd::Scroll(..)
                | Cmd::Type(..)
                | Cmd::Select(..)
                | Cmd::Expect(..)
                | Cmd::Until(..)
                | Cmd::Find(_)
                | Cmd::Tree(_)
        );
        let nodes = if needs_tree {
            match self.ui_nodes(ctx) {
                Some(n) => n,
                None => return Step::Again,
            }
        } else {
            Vec::new()
        };
        let screen = ctx.content_rect();
        let one = |q: &Query| q.one(&nodes, screen).cloned();
        match (&r.job.cmd, r.phase) {
            // 押す・打つ部品は、まず見える所まで巻く(パネルの下の方の欄など)
            (Cmd::Press(q, _) | Cmd::Hover(q), 0) => match one(q) {
                Ok(n) => {
                    Self::push_events(ctx, vec![drive::scroll_into_view(&n)]);
                    Step::Next(SETTLE)
                }
                Err(e) => Step::Done(Err(e)),
            },
            (Cmd::Press(q, how), 1) => {
                let n = match one(q) {
                    Ok(n) => n,
                    Err(e) => return Step::Done(Err(e)),
                };
                // 欄の名前のラベルを押す時は、横の入切か一覧の口を押す(ラベルは押しても何も起きない。
                // 人は名前を見て横の箱を押す。`type` が横の欄へ打つのと同じ)
                let n = match (n.role.as_str(), how) {
                    ("Label", Press::Left) => drive::check_for(&n, &nodes)
                        .or_else(|| drive::combo_for(&n, &nodes))
                        .cloned()
                        .unwrap_or(n),
                    _ => n,
                };
                if n.disabled {
                    return Step::Done(Err(format!("「{}」は押せない状態です", n.name)));
                }
                let at = n.rect.center();
                let ev = match how {
                    Press::Left => drive::click_at(at, PointerButton::Primary, Modifiers::NONE),
                    Press::Right => drive::click_at(at, PointerButton::Secondary, Modifiers::NONE),
                    Press::Double => {
                        let mut e = drive::click_at(at, PointerButton::Primary, Modifiers::NONE);
                        e.extend(drive::click_at(at, PointerButton::Primary, Modifiers::NONE));
                        e
                    }
                };
                Self::push_events(ctx, ev);
                r.target = Some(n);
                Step::Next(SETTLE)
            }
            (Cmd::Press(..), _) => {
                let n = r.target.as_ref().expect("前の段で決めた");
                Step::Done(Ok(format!("押した: {}", n.line())))
            }
            (Cmd::Hover(q), 1) => match one(q) {
                Ok(n) => {
                    Self::push_events(ctx, vec![egui::Event::PointerMoved(n.rect.center())]);
                    Step::Next(SETTLE)
                }
                Err(e) => Step::Done(Err(e)),
            },
            (Cmd::Hover(_), _) => Step::Done(Ok(String::new())),
            // 人と同じく、マウスを部品の上へ置いてからホイールを回す(巻く先はマウスの下の一覧)
            (Cmd::Scroll(q, by), 0) => match one(q) {
                Ok(n) => {
                    let at = n.rect.center();
                    Self::push_events(
                        ctx,
                        vec![
                            egui::Event::PointerMoved(at),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                delta: egui::vec2(0.0, -by),
                                phase: egui::TouchPhase::Move,
                                modifiers: Modifiers::NONE,
                            },
                        ],
                    );
                    Step::Next(SETTLE)
                }
                Err(e) => Step::Done(Err(e)),
            },
            (Cmd::Scroll(q, by), _) => {
                Step::Done(Ok(format!("「{}」の上で {by} 点巻いた", q.text)))
            }
            (Cmd::Type(q, _, _), 0 | 1) => {
                let n = match one(q) {
                    Ok(n) => n,
                    Err(e) => return Step::Done(Err(e)),
                };
                let Some(f) = drive::field_for(&n, &nodes).cloned() else {
                    return Step::Done(Err(format!("「{}」の近くに打てる欄がありません", n.name)));
                };
                if r.phase == 0 {
                    Self::push_events(ctx, vec![drive::scroll_into_view(&f)]);
                    return Step::Next(SETTLE);
                }
                if f.disabled {
                    return Step::Done(Err(format!("「{}」の欄は直せない状態です", n.name)));
                }
                Self::push_events(
                    ctx,
                    drive::click_at(f.rect.center(), PointerButton::Primary, Modifiers::NONE),
                );
                r.target = Some(f);
                Step::Next(SETTLE)
            }
            (Cmd::Type(_, text, _), 2) => {
                let f = r.target.as_ref().expect("前の段で決めた");
                if !nodes
                    .iter()
                    .any(|n| n.text_input && n.focused && n.id == f.id)
                {
                    let now = nodes
                        .iter()
                        .find(|n| n.focused)
                        .map_or("どこにも無い".to_string(), |n| n.line());
                    return Step::Done(Err(format!(
                        "欄を押しても、キーボードが欄へ移りません(キーボードの先: {now})"
                    )));
                }
                // 中身を全部選んでから打つ(人が欄の値を打ち直すのと同じ)。確定のキーは次の段で
                // 別のフレームに送る(同じフレームに入れると、egui は Tab でキーボードの先を
                // フレームの初めに移すので、スライダの横の数の欄は打った字を受け取らずに閉じる)
                let mut ev = drive::key_events(egui::Key::A, Modifiers::COMMAND);
                ev.extend(drive::key_events(egui::Key::Delete, Modifiers::NONE));
                ev.push(egui::Event::Text(text.clone()));
                Self::push_events(ctx, ev);
                Step::Next(SETTLE)
            }
            (Cmd::Type(_, _, commit), 3) => {
                let ev = match commit {
                    Commit::Tab => drive::key_events(egui::Key::Tab, Modifiers::NONE),
                    Commit::Enter => drive::key_events(egui::Key::Enter, Modifiers::NONE),
                    Commit::None => Vec::new(),
                };
                Self::push_events(ctx, ev);
                Step::Next(SETTLE)
            }
            (Cmd::Type(q, text, _), _) => {
                // 確定した後の欄の値(単位を補った後の字)を返す。欄が消えた時(Enter で閉じた)は打っただけ
                let now = one(q)
                    .ok()
                    .and_then(|n| drive::field_for(&n, &nodes).and_then(|f| f.value.clone()));
                Step::Done(Ok(match now {
                    Some(v) => format!("「{}」へ {text} を打った → 欄の値「{v}」", q.text),
                    None => format!("「{}」へ {text} を打った", q.text),
                }))
            }
            (Cmd::Key(k), 0) => match drive::parse_key(k) {
                Ok((key, m)) => {
                    Self::push_events(ctx, drive::key_events(key, m));
                    Step::Next(SETTLE)
                }
                Err(e) => Step::Done(Err(e)),
            },
            (Cmd::Key(_), _) => Step::Done(Ok(String::new())),
            (Cmd::Select(q, _), 0) => {
                let n = match one(q) {
                    Ok(n) => n,
                    Err(e) => return Step::Done(Err(e)),
                };
                // 欄の名前で書いた時は、その横の一覧の口
                let n = drive::combo_for(&n, &nodes).cloned().unwrap_or(n);
                Self::push_events(
                    ctx,
                    drive::click_at(n.rect.center(), PointerButton::Primary, Modifiers::NONE),
                );
                r.target = Some(n);
                Step::Next(SETTLE)
            }
            (Cmd::Select(_, item), 1) => {
                let combo = r.target.clone().expect("前の段で決めた");
                let what = if combo.name.is_empty() {
                    q_text(&r.job.cmd)
                } else {
                    combo.name.clone()
                };
                // 開いた一覧の項目のうち、一覧の口に近い物
                let pick = nodes
                    .iter()
                    .filter(|n| n.name == *item && n.rect != combo.rect && !n.disabled)
                    .min_by(|a, b| {
                        let d = |n: &UiNode| (n.rect.center() - combo.rect.center()).length();
                        d(a).total_cmp(&d(b))
                    });
                match pick {
                    Some(p) => {
                        Self::push_events(
                            ctx,
                            drive::click_at(
                                p.rect.center(),
                                PointerButton::Primary,
                                Modifiers::NONE,
                            ),
                        );
                        Step::Next(SETTLE)
                    }
                    None => Step::Done(Err(format!(
                        "「{what}」を開いても、項目「{item}」がありません"
                    ))),
                }
            }
            (Cmd::Select(q, item), _) => {
                Step::Done(Ok(format!("「{}」で {item} を選んだ", q.text)))
            }
            (Cmd::Pick(face, ctrl), 0) => {
                let parts: Vec<&str> = face.split('|').collect();
                match match (face.strip_prefix("候補:"), parts.as_slice()) {
                    (Some(label), _) => self.choice_point(label),
                    (None, [a, b]) => self.edge_point([a, b]),
                    (None, [_, _, _, ..]) => self.vertex_point(&parts),
                    (None, _) => self.face_point(face, true),
                } {
                    Some(at) => {
                        let m = if *ctrl {
                            Modifiers::COMMAND
                        } else {
                            Modifiers::NONE
                        };
                        // egui は押している修飾キーを出来事の `ModifiersChanged` から読む(クリックの出来事に
                        // 付けた値は見ない)ので、Ctrl を押してからクリックし、次の段で離す
                        let mut ev = vec![egui::Event::ModifiersChanged(m)];
                        ev.extend(drive::click_at(at, PointerButton::Primary, m));
                        Self::push_events(ctx, ev);
                        Step::Next(SETTLE)
                    }
                    None => Step::Done(Err(format!(
                        "3D のビューに {face} が見えず、その名前の部品もありません"
                    ))),
                }
            }
            (Cmd::Pick(face, ctrl), _) => {
                if *ctrl {
                    Self::push_events(ctx, vec![egui::Event::ModifiersChanged(Modifiers::NONE)]);
                }
                Step::Done(Ok(format!("{face} を押した")))
            }
            // 3D のビューの面が先。面でなければ画面の部品(タブなど)の中央を押す
            (Cmd::Drag(face, ..), 0) => match self
                .face_point(face, false)
                .or_else(|| one(&Query::parse(face)).ok().map(|n| n.rect.center()))
            {
                Some(at) => {
                    Self::push_events(
                        ctx,
                        vec![
                            egui::Event::PointerMoved(at),
                            egui::Event::PointerButton {
                                pos: at,
                                button: PointerButton::Primary,
                                pressed: true,
                                modifiers: Modifiers::NONE,
                            },
                        ],
                    );
                    r.from = Some(at);
                    // 止まったまま待つと長押し(重なりの一覧)になるので、すぐ引き始める
                    Step::Next(1)
                }
                None => Step::Done(Err(format!(
                    "3D のビューに {face} が見えず、その名前の部品もありません"
                ))),
            },
            // 人の手のように、何コマかに分けて少しずつ引く(コマごとに形を直す)
            (Cmd::Drag(_, dx, dy), p) if p <= DRAG_STEPS => {
                let from = r.from.expect("前の段で押した");
                let t = p as f32 / DRAG_STEPS as f32;
                let at = from + egui::vec2(dx * t, dy * t);
                let ev = if p < DRAG_STEPS {
                    vec![egui::Event::PointerMoved(at)]
                } else {
                    vec![egui::Event::PointerButton {
                        pos: at,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    }]
                };
                Self::push_events(ctx, ev);
                Step::Next(SETTLE)
            }
            (Cmd::Drag(face, dx, dy), _) => {
                Step::Done(Ok(format!("{face} を押して右へ {dx}・下へ {dy} 引いた")))
            }
            (Cmd::Expect(q, check), _) => Step::Done(expect(q, check, &nodes, screen)),
            (Cmd::Until(q, check, secs), _) => match expect(q, check, &nodes, screen) {
                Ok(t) => Step::Done(Ok(format!(
                    "{t}({:.1} 秒待った)",
                    r.started.elapsed().as_secs_f32()
                ))),
                Err(e) if r.started.elapsed().as_secs_f32() > *secs => {
                    Step::Done(Err(format!("{secs} 秒待っても: {e}")))
                }
                Err(_) => Step::Again,
            },
            (Cmd::Find(q), _) => {
                let f = q.find(&nodes, screen);
                if f.is_empty() {
                    Step::Done(Err(format!("画面に「{}」が見つかりません", q.text)))
                } else {
                    Step::Done(Ok(f
                        .iter()
                        .map(|n| n.line())
                        .collect::<Vec<_>>()
                        .join("\n")))
                }
            }
            (Cmd::Tree(part), _) => Step::Done(Ok(nodes
                .iter()
                .filter(|n| !n.name.is_empty() || n.value.is_some() || n.text_input)
                .filter(|n| part.is_empty() || n.name.contains(part.as_str()))
                .map(|n| n.line())
                .collect::<Vec<_>>()
                .join("\n"))),
            (Cmd::Wait(None), _) => Step::Done(Ok(String::new())),
            (Cmd::Wait(Some(n)), 0) => Step::Next(*n),
            (Cmd::Wait(Some(_)), _) => Step::Done(Ok(String::new())),
            (Cmd::Pane(name, on), _) => Step::Done(self.cmd_pane(name, *on)),
            (Cmd::Shot(name), _) => {
                self.cmdline.shot = Some(name.clone());
                Step::Done(Ok(format!("写真 {name}")))
            }
            (Cmd::Echo(t), _) => Step::Done(Ok(t.clone())),
            (Cmd::Clear, _) => {
                self.cmdline.log.clear();
                Step::Done(Ok(String::new()))
            }
            (Cmd::Help(c), _) => Step::Done(help(c.as_deref())),
            (Cmd::Internal(tool, args), 0) => {
                match self.live_request_in(&r.job.roots, tool, args) {
                    Ok(crate::live_ui::Answer::Now(v)) => Step::Done(Ok(pretty(&v))),
                    Ok(crate::live_ui::Answer::AfterRecompute) => Step::Next(1),
                    // 手順は 1 行ずつ順に流すので、裏の仕事もここで待って結果を出す
                    Ok(crate::live_ui::Answer::Later(job)) => Step::Done(job().map(|v| pretty(&v))),
                    // 内部の道具に画面の道具は入れない(parse が分けている)
                    Ok(crate::live_ui::Answer::AfterScript(_)) => {
                        Step::Done(Err("スクリプトの中からスクリプトは流せません".into()))
                    }
                    Err(e) => Step::Done(Err(e)),
                }
            }
            (Cmd::Internal(..), _) => Step::Done(Ok("変えた".into())),
        }
    }

    /// 3D のビューで面 `face` が見えている所(画面の座標)。見えている所の真ん中に近い点。
    /// `inside` なら、押して面を選ぶための点(縁にかからない点)。
    pub(crate) fn face_point(&self, face: &str, inside: bool) -> Option<egui::Pos2> {
        let rect = self.win.view_rect;
        let (nx, ny) = (96, 72);
        let at = |i: usize, j: usize| {
            egui::pos2(
                rect.min.x + rect.width() * (i as f32 + 0.5) / nx as f32,
                rect.min.y + rect.height() * (j as f32 + 0.5) / ny as f32,
            )
        };
        let mut on = vec![false; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                on[j * nx + i] =
                    crate::view3d::pick_at(&self.cur.scene, &self.cur.camera, rect, at(i, j))
                        .is_some_and(|h| h.face == face);
            }
        }
        let cells: Vec<(usize, usize)> = (0..ny)
            .flat_map(|j| (0..nx).map(move |i| (i, j)))
            .filter(|&(i, j)| on[j * nx + i])
            .collect();
        if cells.is_empty() {
            return None;
        }
        let n = cells.len() as f32;
        let mean = cells
            .iter()
            .fold(egui::Vec2::ZERO, |a, &(i, j)| a + at(i, j).to_vec2() / n)
            .to_pos2();
        let near = |c: &&(usize, usize)| (at(c.0, c.1) - mean).length();
        let best = cells.iter().min_by(|a, b| near(a).total_cmp(&near(b)))?;
        // 押して選ぶ時、真ん中に近い点が面の縁にかかる(面の中の穴の縁など。押すと辺が選ばれる)なら、
        // 上下左右も同じ面の所のうち真ん中に近い点にする。引く時は前と同じ点(引いた量の結果が変わる)
        let inner = |&&(i, j): &&(usize, usize)| {
            i > 0
                && j > 0
                && i + 1 < nx
                && j + 1 < ny
                && on[j * nx + i - 1]
                && on[j * nx + i + 1]
                && on[(j - 1) * nx + i]
                && on[(j + 1) * nx + i]
        };
        let pick = if !inside || inner(&best) {
            best
        } else {
            cells
                .iter()
                .filter(inner)
                .min_by(|a, b| near(a).total_cmp(&near(b)))
                .unwrap_or(best)
        };
        Some(at(pick.0, pick.1))
    }

    /// 作る手順の今の候補(領域・軸の線など)のうち、名前が `label` の物を押すと選べる所
    /// (候補の内側か線の近くの、押すとその候補になる点のうち、まん中に近い所)。
    pub(crate) fn choice_point(&self, label: &str) -> Option<egui::Pos2> {
        let h = self.cur.gesture.as_ref()?.current()?;
        let want = h.choices.iter().position(|c| c.label == label)?;
        let rect = self.win.view_rect;
        let (nx, ny) = (160, 120);
        let mut hits = Vec::new();
        for j in 0..ny {
            for i in 0..nx {
                let p = egui::pos2(
                    rect.min.x + rect.width() * (i as f32 + 0.5) / nx as f32,
                    rect.min.y + rect.height() * (j as f32 + 0.5) / ny as f32,
                );
                if crate::gesture::choice_at(h, &self.cur.camera, rect, p) == Some(want) {
                    hits.push(p);
                }
            }
        }
        let n = hits.len() as f32;
        let mean = hits
            .iter()
            .fold(egui::Vec2::ZERO, |a, p| a + p.to_vec2() / n)
            .to_pos2();
        hits.into_iter()
            .min_by(|a, b| (*a - mean).length().total_cmp(&(*b - mean).length()))
    }

    /// 両側の面が `faces` の辺を、3D のビューで拾える所(辺の上の点のうち、押すとその辺が拾える所)。
    pub(crate) fn edge_point(&mut self, faces: [&str; 2]) -> Option<egui::Pos2> {
        let rect = self.win.view_rect;
        let same = |f: &[String; 2]| {
            (f[0] == faces[0] && f[1] == faces[1]) || (f[0] == faces[1] && f[1] == faces[0])
        };
        let edges: Vec<jc_core::EdgeInfo> = self
            .scene_edges()
            .iter()
            .flat_map(|(_, l)| l.iter().filter(|e| same(&e.faces)).cloned())
            .collect();
        for e in &edges {
            let pts = crate::view3d::edge_polyline(e);
            let n = 24;
            for k in 0..=n {
                let t = k as f64 / n as f64 * (pts.len() - 1) as f64;
                let (i, u) = ((t.floor() as usize).min(pts.len() - 2), t - t.floor());
                let q = [0, 1, 2].map(|j| (pts[i][j] + (pts[i + 1][j] - pts[i][j]) * u) as f32);
                let p = self.cur.camera.project(q, rect).0;
                if rect.contains(p) && self.edge_hit(rect, p).is_some_and(|h| same(&h.faces)) {
                    return Some(p);
                }
            }
        }
        None
    }

    /// 面 `faces` が全部集まる頂点の、画面の点(見えていて、そこを押すとその頂点に当たる所)。
    pub(crate) fn vertex_point(&mut self, faces: &[&str]) -> Option<egui::Pos2> {
        let rect = self.win.view_rect;
        self.scene_edges();
        let all = |v: &jc_core::VertexInfo| faces.iter().all(|f| v.faces.iter().any(|g| g == f));
        let found: Vec<(jc_core::FeatureId, jc_core::VertexInfo)> = self
            .cur
            .vertex_cache
            .iter()
            .flat_map(|(f, l)| l.iter().filter(|v| all(v)).map(|v| (*f, v.clone())))
            .collect();
        found.into_iter().find_map(|(feature, v)| {
            let p = self.cur.camera.project(v.at.map(|c| c as f32), rect).0;
            (rect.contains(p)
                && self
                    .vertex_hit(rect, p)
                    .is_some_and(|h| h.feature == feature && h.vertex == v.name))
            .then_some(p)
        })
    }

    fn cmd_pane(&mut self, name: &str, on: Option<bool>) -> Result<String, String> {
        let space = self.win.layout.space;
        let pane = crate::dock_ui::Pane::HIDABLE
            .into_iter()
            .find(|p| p.label(space) == name || p.label(space).contains(name))
            .ok_or_else(|| {
                format!("部品 {name} はありません(ブラウザ・編集パネル・タイムライン・コマンド)")
            })?;
        let open = on.unwrap_or(!self.pane_open(pane));
        self.set_pane_open(pane, open);
        Ok(format!(
            "{} を{}",
            pane.label(space),
            if open { "出した" } else { "隠した" }
        ))
    }

    /// コマンド欄の部品の中身。
    pub(crate) fn cmdline_ui(&mut self, ui: &mut egui::Ui) {
        let l = crate::theme::look(ui.ctx());
        let line_h = ui.text_style_height(&egui::TextStyle::Monospace);
        let input_h = line_h + 14.0;
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("cmdline_log")
            .max_height((ui.available_height() - input_h - 8.0).max(line_h))
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (k, t) in &self.cmdline.log {
                    let (text, color) = match k {
                        Kind::Input => (format!("> {t}"), l.pal.ink),
                        Kind::Ok => (t.clone(), l.pal.ink2),
                        Kind::Err => (t.clone(), ui.visuals().error_fg_color),
                    };
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).monospace().color(color)).wrap(),
                    );
                }
                if std::mem::take(&mut self.cmdline.scroll) {
                    ui.scroll_to_cursor(Some(egui::Align::Max));
                }
            });
        ui.horizontal(|ui| {
            let label = ui.label(egui::RichText::new("コマンド").color(l.pal.ink2));
            let typed = self.cmdline.input.clone();
            let cands: Vec<&str> = if typed.contains(' ') || typed.is_empty() {
                Vec::new()
            } else {
                COMMANDS
                    .iter()
                    .map(|(n, _)| *n)
                    .filter(|n| n.starts_with(typed.as_str()))
                    .collect()
            };
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.cmdline.input)
                    .id_salt("cmdline_input")
                    .font(egui::TextStyle::Monospace)
                    .hint_text("例: click \"熱\"(help で一覧。↑ で前のコマンド)")
                    .lock_focus(true)
                    .desired_width((ui.available_width() - 8.0).max(80.0)),
            );
            let r = r.labelled_by(label.id);
            let (tab, up, down) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Tab),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                )
            });
            if r.has_focus() {
                if tab && let Some(c) = cands.first() {
                    self.cmdline.input = format!("{c} ");
                }
                let h = &self.cmdline.history;
                if up && !h.is_empty() {
                    let at = self
                        .cmdline
                        .hist_at
                        .map_or(h.len() - 1, |i| i.saturating_sub(1));
                    self.cmdline.hist_at = Some(at);
                    self.cmdline.input = h[at].clone();
                }
                if down && let Some(i) = self.cmdline.hist_at {
                    if i + 1 < h.len() {
                        self.cmdline.hist_at = Some(i + 1);
                        self.cmdline.input = h[i + 1].clone();
                    } else {
                        self.cmdline.hist_at = None;
                        self.cmdline.input.clear();
                    }
                }
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let line = std::mem::take(&mut self.cmdline.input);
                if !line.trim().is_empty() {
                    self.submit_line(line.trim());
                }
                // 続けて打てるように欄に戻す
                r.request_focus();
            }
            if !cands.is_empty() && r.has_focus() {
                r.on_hover_text(cands.join("  "));
            }
        });
    }

    /// 欄で打った 1 行を流す(流している途中のスクリプトより先に)。
    fn submit_line(&mut self, line: &str) {
        self.cmdline.history.push(line.to_string());
        self.cmdline.hist_at = None;
        match parse(line) {
            Ok(Some(cmd)) => {
                let batch = self.cmdline.next_batch;
                self.cmdline.next_batch += 1;
                self.cmdline.batches.push((
                    batch,
                    ScriptResult {
                        ok: true,
                        ..Default::default()
                    },
                ));
                self.cmdline.queue.push_front(Job {
                    line: line.to_string(),
                    cmd,
                    batch,
                    roots: None,
                });
            }
            Ok(None) => {}
            Err(e) => {
                self.cmdline.log.push((Kind::Input, line.to_string()));
                self.cmdline.log.push((Kind::Err, e));
                self.cmdline.scroll = true;
            }
        }
    }
}

/// コマンドの部品の書き方の字。
fn q_text(c: &Cmd) -> String {
    match c {
        Cmd::Press(q, _)
        | Cmd::Hover(q)
        | Cmd::Scroll(q, _)
        | Cmd::Type(q, _, _)
        | Cmd::Select(q, _)
        | Cmd::Expect(q, _)
        | Cmd::Until(q, _, _)
        | Cmd::Find(q) => q.text.clone(),
        _ => String::new(),
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn help(c: Option<&str>) -> Result<String, String> {
    match c {
        Some(c) => COMMANDS
            .iter()
            .find(|(n, _)| *n == c)
            .map(|(_, u)| u.to_string())
            .ok_or_else(|| format!("コマンド {c} は知りません")),
        None => Ok(format!(
            "{}\n部品の書き方: \"名前\"(ちょうど同じ)・~字(含む)・button:名前(役割で絞る。field は打てる欄)・名前#2(2 番目)",
            COMMANDS
                .iter()
                .map(|(_, u)| *u)
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

/// `expect` の中身。
fn expect(
    q: &Query,
    check: &Check,
    nodes: &[UiNode],
    screen: egui::Rect,
) -> Result<String, String> {
    let found = q.find(nodes, screen);
    match check {
        Check::Missing => {
            // 無いことは、書いたとおりの名前で見る(`~` の時だけ含む物)
            let found: Vec<&UiNode> = found
                .into_iter()
                .filter(|n| q.contains || n.name == q.text)
                .collect();
            // `#3` は 3 つ目が無いこと(番号を見ないと、1 つ目があるだけで落ちる)
            let k = q.nth.unwrap_or(1);
            let which = q.nth.map_or(String::new(), |k| format!("の {k} つ目"));
            return match found.get(k - 1) {
                None => Ok(format!("「{}」{which}は無い", q.text)),
                Some(n) => Err(format!("「{}」{which}があります: {}", q.text, n.line())),
            };
        }
        // `#2` は 2 つ目があること(番号を見ないと、1 つ目があるだけで通る)
        Check::Exists if found.len() >= q.nth.unwrap_or(1) => {
            return Ok(format!("ある: {}", found[q.nth.unwrap_or(1) - 1].line()));
        }
        _ => {}
    }
    let n = q.one(nodes, screen)?;
    match check {
        Check::Exists | Check::Missing => Ok(format!("ある: {}", n.line())),
        Check::Enabled if !n.disabled => Ok(format!("押せる: {}", n.line())),
        Check::Disabled if n.disabled => Ok(format!("押せない: {}", n.line())),
        Check::Enabled => Err(format!("「{}」は押せない状態です", n.name)),
        Check::Disabled => Err(format!("「{}」は押せる状態です", n.name)),
        Check::Eq(want) | Check::Contains(want) => {
            // 欄の名前で探した時は、その欄の値を見る。ほかは値か名前
            let target = if n.text_input {
                n
            } else {
                // 入切を先に見る(入切の名前の下の行の欄を拾わない)
                drive::check_for(n, nodes)
                    .or_else(|| drive::field_for(n, nodes))
                    .or_else(|| drive::combo_for(n, nodes))
                    .unwrap_or(n)
            };
            // 入切は「入」か「切」
            let have = match target.checked {
                Some(c) => (if c { "入" } else { "切" }).to_string(),
                None => target.value.clone().unwrap_or_else(|| target.name.clone()),
            };
            let ok = match check {
                Check::Eq(_) => have.trim() == want.trim(),
                _ => have.contains(want.as_str()),
            };
            if ok {
                Ok(format!("「{}」は「{have}」", q.text))
            } else {
                Err(format!(
                    "「{}」は「{have}」で、「{want}」{}ではありません",
                    q.text,
                    if matches!(check, Check::Eq(_)) {
                        ""
                    } else {
                        "を含む字"
                    }
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_splits_into_words_and_quoted_words_keep_their_spaces() {
        assert_eq!(
            split(r#"type "まわりの温度(℃)" "50 \"x\"" enter"#).unwrap(),
            vec!["type", "まわりの温度(℃)", "50 \"x\"", "enter"]
        );
        assert_eq!(split(r#"click """#).unwrap(), vec!["click", ""]);
        assert!(split(r#"click "熱"#).is_err());
    }

    #[test]
    fn commands_are_read_and_mistakes_say_how_to_write_them() {
        assert_eq!(parse("  # 見出し").unwrap(), None);
        assert_eq!(
            parse("click button:熱#2").unwrap(),
            Some(Cmd::Press(
                Query {
                    text: "熱".into(),
                    contains: false,
                    role: Some("button".into()),
                    nth: Some(2)
                },
                Press::Left
            ))
        );
        assert_eq!(
            parse(r#"expect "まわりの温度(℃)" = 50 ℃"#).unwrap(),
            Some(Cmd::Expect(
                Query::parse("まわりの温度(℃)"),
                Check::Eq("50 ℃".into())
            ))
        );
        assert!(parse("type 欄").unwrap_err().contains("書き方: type"));
        assert!(parse("clik 熱").unwrap_err().contains("click"));
        // 日本語で始まる知らない命令は、落ちずに知らないと返す
        assert!(parse("クリック 熱").unwrap_err().contains("知りません"));
        // until の `~ 1s` の 1s は含む字。待つ長さは値の後に書いた時だけ
        assert_eq!(
            parse(r#"until "表示" ~ 1s"#).unwrap(),
            Some(Cmd::Until(
                Query::parse("表示"),
                Check::Contains("1s".into()),
                120.0
            ))
        );
        assert_eq!(
            parse(r#"until "表示" ~ 済み 5s"#).unwrap(),
            Some(Cmd::Until(
                Query::parse("表示"),
                Check::Contains("済み".into()),
                5.0
            ))
        );
        assert_eq!(
            parse(r#"until "済み" 300s"#).unwrap(),
            Some(Cmd::Until(Query::parse("済み"), Check::Exists, 300.0))
        );
        assert_eq!(
            parse(r#"until "表示" = 5s"#).unwrap(),
            Some(Cmd::Until(
                Query::parse("表示"),
                Check::Eq("5s".into()),
                120.0
            ))
        );
        assert!(parse(r#"until "表示" ="#).unwrap_err().contains("書き方"));
        assert!(parse("key ctrl+nope").is_err());
        assert_eq!(
            parse(r#"scroll ~"G21" 5000"#).unwrap(),
            Some(Cmd::Scroll(Query::parse("~G21"), 5000.0))
        );
        assert!(
            parse("scroll 一覧 たくさん")
                .unwrap_err()
                .contains("数ではありません")
        );
        assert_eq!(
            parse("set_param width 70 mm").unwrap(),
            Some(Cmd::Internal(
                "set_param".into(),
                json!({"name": "width", "expr": "70 mm"})
            ))
        );
    }
}
