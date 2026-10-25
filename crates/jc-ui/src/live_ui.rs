//! 動いている画面に Claude がつなぐ(候補 21)。設定の「AI からの接続を受ける」を入れると、
//! jc-session の `live` の待ち受けを立て、MCP の `app` の道具から届いた要求をフレームの間に
//! 今のタブの文書へ当てる。AI の変更は人と同じ取り消しの履歴に「Claude: 」付きで入る。
//! 人が機能を編集している間(スケッチ・編集の欄・押し出しの手順など)は、文書を変える要求を断る
//! (読むだけの要求は通す)。下の帯に「Claude 待ち受け中・接続中・操作中」を出し、そこから切れる。
//!
//! 道具(`tool`): status・screenshot・selection・set_param・add_param・set_value・undo・redo。
//! 画面の部品を人と同じ入力で操作する ui_find・ui_tree・ui_click・ui_type・ui_key・ui_select・
//! ui_pick・ui_expect と、コマンド欄のコマンドを流す command(1 行)・script(何行も)。これらは
//! コマンド欄のスクリプトとして流し、流し終えたら行ごとの結果で答える(`crate::cmdline`)。
//! 画面の無い形(jc-mcp の Session)と道具の中身を 1 か所にまとめる作り直しは、まだしていない。
//!
//! 文書を変えた要求は、再計算の後に、変えた機能をタイムラインとブラウザで、そのボディを 3D のビューで
//! 少しの間光らせ、下の帯に道具の名前を出す(AI が何を変えたかを人が目で追える)。

use crate::Workspace;
use jc_core::{FeatureEntry, FeatureId};
use jc_session::live::{self, Listener, Request};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// AI の頼みへの答えの口。
type Reply = mpsc::Sender<Result<Value, String>>;

/// 接続の状態(タブを替えても続く。窓に 1 つ)。
#[derive(Default)]
pub(crate) struct LiveLink {
    listener: Option<Listener>,
    rx: Option<mpsc::Receiver<Request>>,
    /// 変更の後、再計算が終わってから答える要求
    waiting: Vec<mpsc::Sender<Result<Value, String>>>,
    /// 流し終えたら答えるスクリプト(コマンド欄の番号)
    /// 流しているスクリプトの (番号, 答えの口, 失敗したら戻す取り消しの深さ)
    scripts: Vec<(u64, Reply, Option<usize>)>,
    /// 最後に要求が届いた時
    seen: Option<Instant>,
    /// 最後に文書を変えた時
    wrote: Option<Instant>,
    /// 最後に文書を変えた道具(下の帯に出す)
    pub(crate) wrote_by: Option<String>,
    /// 印を付ける前に、再計算(とスクリプト)が終わるのを待つ要求
    pending: Vec<Pending>,
    /// 今光らせている、AI が変えた所
    pub(crate) mark: Option<AiMark>,
    /// 待ち受けを立てられなかった理由(設定に出す)
    pub failed: Option<String>,
}

/// 「接続中」と見なす長さ(要求ごとにつなぐので、最後の要求からの時間で見る)。
const SEEN_FOR: Duration = Duration::from_secs(60);
/// 「操作中」と見なす長さ。
const WROTE_FOR: Duration = Duration::from_secs(5);
/// AI が変えた所を光らせる長さ。終わりの方で薄くする
const MARK_FOR: Duration = Duration::from_millis(1800);
const MARK_FADE: f32 = 0.6;

/// 文書を変えた要求の、変える前の機能(再計算が終わってから比べて印を付ける)。
struct Pending {
    before: Vec<FeatureEntry>,
    /// 変える前の計算の結果(パラメータだけ変えた時に、形の変わった機能を見る)
    results: Vec<jc_core::FeatureResult>,
    depth: usize,
    /// スクリプトで流した要求は、流し終えてから比べる
    script: Option<u64>,
}

/// AI が変えた所(要求 1 つ分)。
pub(crate) struct AiMark {
    /// 足した・直した機能(パラメータだけ変えた時や取り消しでは、結果の変わった機能)
    pub features: BTreeSet<FeatureId>,
    /// その機能を含むボディ(最初に作った機能)と、3D のビューのボディ(最後の機能)
    pub roots: BTreeSet<FeatureId>,
    pub bodies: BTreeSet<FeatureId>,
    pub at: Instant,
}

/// 下の帯に出す接続の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveState {
    Off,
    Listening,
    Connected,
    Writing,
}

impl Workspace {
    /// AI の接続の状態。
    pub fn live_state(&self) -> LiveState {
        let l = &self.live;
        if l.listener.is_none() {
            LiveState::Off
        } else if l.wrote.is_some_and(|t| t.elapsed() < WROTE_FOR) {
            LiveState::Writing
        } else if l.seen.is_some_and(|t| t.elapsed() < SEEN_FOR) {
            LiveState::Connected
        } else {
            LiveState::Listening
        }
    }

    /// 待ち受けの番号(立てていなければ `None`)。
    pub fn live_port(&self) -> Option<u16> {
        self.live.listener.as_ref().map(|l| l.ticket.port)
    }

    /// 接続を切る(設定も切る。次に入れるまで待ち受けない)。
    pub fn live_disconnect(&mut self) {
        self.win.layout.ai_live = false;
        self.live_tick();
        self.win.message = Some("Claude との接続を切りました".into());
    }

    /// 毎フレーム: 設定に合わせて待ち受けを立てる・止める。届いた要求を処理する。
    pub(crate) fn live_tick(&mut self) {
        let want = self.win.layout.ai_live && self.win.data_dir.is_some();
        if !want && self.live.listener.is_some() {
            self.live.listener = None;
            self.live.rx = None;
            for w in self.live.waiting.drain(..) {
                let _ = w.send(Err("画面が接続を切りました".into()));
            }
            for (_, w, _) in self.live.scripts.drain(..) {
                let _ = w.send(Err("画面が接続を切りました".into()));
            }
        }
        if want && self.live.listener.is_none() && self.live.failed.is_none() {
            let (tx, rx) = mpsc::channel();
            let ctx = self.win.repaint.clone();
            let dir = self.win.data_dir.clone().expect("want なので有る");
            match Listener::start(&dir, tx, move || {
                if let Some(c) = &ctx {
                    c.request_repaint();
                }
            }) {
                Ok(l) => {
                    self.live.listener = Some(l);
                    self.live.rx = Some(rx);
                }
                Err(e) => {
                    log::warn!("AI の接続: {e}");
                    self.live.failed = Some(e);
                }
            }
        }
        let finished: Vec<u64> = self
            .live
            .scripts
            .iter()
            .map(|(b, ..)| *b)
            .filter(|b| self.cmdline.done.iter().any(|(d, _)| d == b))
            .collect();
        for b in finished {
            let r = self.script_result(b).expect("終えた");
            if let Some(i) = self.live.scripts.iter().position(|(x, ..)| *x == b) {
                let (_, w, back_to) = self.live.scripts.remove(i);
                let mut v = r.to_json();
                // AI の道のまとめて実行と同じく、途中で失敗したら頼む前の形へ戻す
                if !r.ok
                    && let Some(depth) = back_to
                    && self.cur.work.doc().undo_depth() > depth
                {
                    log::info!("AI の接続: 台本が失敗したので頼む前へ戻した");
                    self.go_to_history(depth);
                    v["rolled_back"] = json!(true);
                }
                let _ = w.send(Ok(v));
            }
        }
        if !self.live.waiting.is_empty() && !self.is_busy() {
            let s = self.live_status();
            for w in self.live.waiting.drain(..) {
                let _ = w.send(Ok(s.clone()));
            }
        }
        if !self.live.pending.is_empty() && !self.is_busy() {
            let (ready, rest): (Vec<Pending>, Vec<Pending>) =
                self.live.pending.drain(..).partition(|p| {
                    p.script
                        .is_none_or(|b| self.cmdline.done.iter().any(|(d, _)| *d == b))
                });
            self.live.pending = rest;
            for p in ready {
                self.mark_changes(p);
            }
        }
        let reqs: Vec<Request> = match &self.live.rx {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        for r in reqs {
            self.live.seen = Some(Instant::now());
            log::info!("AI の接続: {} {}", r.tool, r.args);
            let before = self.cur.work.doc().data().features.clone();
            let results = self.cur.work.report().features.clone();
            let depth = self.cur.work.doc().undo_depth();
            let by = self.tool_caption(&r.tool, &r.args);
            // MCP から来た頼みは、操作がファイルを読む引数を AI に許したフォルダの中に限る
            self.cmdline.ai_roots = r.roots.clone();
            let answer = self.live_request(&r.tool, &r.args);
            self.cmdline.ai_roots = None;
            match answer {
                Ok(Answer::Now(v)) => {
                    let _ = r.reply.send(Ok(v));
                }
                Ok(Answer::AfterRecompute) => {
                    self.live.wrote = Some(Instant::now());
                    self.live.wrote_by = Some(by);
                    self.live.waiting.push(r.reply);
                    self.live.pending.push(Pending {
                        before,
                        results,
                        depth,
                        script: None,
                    });
                }
                Ok(Answer::AfterScript(b)) => {
                    self.live.wrote = Some(Instant::now());
                    self.live.wrote_by = Some(by);
                    // keep_on_error が true なら、失敗しても途中までの変更を残す
                    let back_to =
                        (!r.args["keep_on_error"].as_bool().unwrap_or(false)).then_some(depth);
                    self.live.scripts.push((b, r.reply, back_to));
                    self.live.pending.push(Pending {
                        before,
                        results,
                        depth,
                        script: Some(b),
                    });
                }
                Ok(Answer::Later(job)) => {
                    let reply = r.reply;
                    let spawned = std::thread::Builder::new()
                        .name(format!("AI の {}", r.tool))
                        .spawn(move || {
                            let _ = reply.send(job());
                        });
                    if let Err(e) = spawned {
                        log::warn!("AI の接続: スレッドを作れません: {e}");
                    }
                }
                Err(e) => {
                    log::info!("AI の接続: 断った: {e}");
                    let _ = r.reply.send(Err(e));
                }
            }
        }
        if (!self.live.waiting.is_empty()
            || !self.live.scripts.is_empty()
            || !self.live.pending.is_empty())
            && let Some(c) = &self.win.repaint
        {
            c.request_repaint_after(Duration::from_millis(50));
        }
    }

    /// 再計算の後に、要求の前と比べて変えた所の印を付ける。文書が変わっていなければ付けない。
    fn mark_changes(&mut self, p: Pending) {
        if self.cur.work.doc().undo_depth() == p.depth {
            return;
        }
        let data = self.cur.work.doc().data();
        let mut features: BTreeSet<FeatureId> = data
            .features
            .iter()
            .filter(|f| !p.before.contains(f))
            .map(|f| f.id)
            .collect();
        if features.is_empty() {
            // パラメータだけ変えた(機能の中身は同じ)時は、結果の変わった機能。「計算し直した機能」
            // では、前に計算した値へ戻した時(取り消し)に結果を使い回すので何も光らなかった
            let same = |a: &jc_core::FeatureStatus, b: &jc_core::FeatureStatus| match (a, b) {
                (
                    jc_core::FeatureStatus::Ok { outputs: x, .. },
                    jc_core::FeatureStatus::Ok { outputs: y, .. },
                ) => x == y,
                _ => a == b,
            };
            features = self
                .cur
                .work
                .report()
                .features
                .iter()
                .filter(|f| {
                    !p.results
                        .iter()
                        .any(|b| b.id == f.id && same(&b.status, &f.status))
                })
                .map(|f| f.id)
                .collect();
        }
        if features.is_empty() {
            return;
        }
        let roots: BTreeSet<FeatureId> = features.iter().map(|f| self.body_root(*f)).collect();
        let bodies = self
            .body_roots()
            .into_iter()
            .filter(|(_, root)| roots.contains(root))
            .map(|(last, _)| last)
            .collect();
        log::info!(
            "AI の接続: 変えた所を光らせる(機能 {:?})",
            features.iter().map(|f| f.0).collect::<Vec<_>>()
        );
        self.live.mark = Some(AiMark {
            features,
            roots,
            bodies,
            at: Instant::now(),
        });
        if let Some(c) = &self.win.repaint {
            c.request_repaint();
        }
    }

    /// AI が変えた所の光の強さ(0〜1)。光らせ終えていれば `None`。
    fn mark_glow(&self, ctx: &egui::Context) -> Option<(&AiMark, f32)> {
        let m = self.live.mark.as_ref()?;
        let t = m.at.elapsed();
        if t >= MARK_FOR {
            return None;
        }
        ctx.request_repaint();
        Some((m, ((MARK_FOR - t).as_secs_f32() / MARK_FADE).min(1.0)))
    }

    /// 機能 `id`(ブラウザの行ならそのボディ `root` も)を、AI が変えたので今光らせる強さ。
    pub(crate) fn ai_glow(
        &self,
        ctx: &egui::Context,
        id: FeatureId,
        root: Option<FeatureId>,
    ) -> Option<f32> {
        let (m, k) = self.mark_glow(ctx)?;
        (m.features.contains(&id) || root.is_some_and(|r| m.roots.contains(&r))).then_some(k)
    }

    /// 毎フレーム: AI が変えたボディを 3D のビューで光らせる(差し色へ寄せ、終わりの方で薄くする)。
    pub(crate) fn sync_ai_glow(&mut self, ctx: &egui::Context) {
        let (bodies, k) = match self.mark_glow(ctx) {
            Some((m, k)) => (m.bodies.clone(), k),
            None => (BTreeSet::new(), 0.0),
        };
        self.cur.scene.set_glow(bodies, self.win.look.ws(), k);
    }

    /// 下の帯に出す道具の名前(何を変えたかが分かる引数を添える)。
    fn tool_caption(&self, tool: &str, a: &Value) -> String {
        let s = |k: &str| a[k].as_str().unwrap_or_default().to_string();
        let what = match tool {
            "run" => {
                let id = s("id");
                self.actions()
                    .into_iter()
                    .find(|x| x.id == id)
                    .map_or(id, |x| x.label)
            }
            "set_param" | "add_param" => format!("{} = {}", s("name"), s("expr")),
            "set_value" => format!("{} = {}", s("key"), s("expr")),
            "ui_click" | "ui_type" | "ui_select" | "ui_expect" | "ui_find" => s("target"),
            "ui_key" => s("keys"),
            "ui_pick" => s("face"),
            "command" => s("line"),
            _ => String::new(),
        };
        if what.is_empty() {
            tool.to_string()
        } else {
            format!("{tool}({what})")
        }
    }

    /// 人が機能を編集している間か(文書を変える要求を断る)。
    fn human_editing(&self) -> Option<&'static str> {
        if self.cur.gesture.is_some() {
            Some("人が形を作る手順の途中")
        } else if self.cur.creating.is_some() {
            Some("人が機能を作っている途中")
        } else if self.cur.editing.is_some() {
            Some("人が機能を編集している途中")
        } else if self.is_start_screen() {
            Some("開始画面を出している(文書を開いていない)")
        } else {
            None
        }
    }

    /// 要求 1 つを処理する。
    pub(crate) fn live_request(&mut self, tool: &str, a: &Value) -> Result<Answer, String> {
        let s = |k: &str| a[k].as_str().map(str::to_string);
        let need = |k: &str| s(k).ok_or_else(|| format!("{tool} には {k} が要ります"));
        let writes = matches!(
            tool,
            "set_param" | "add_param" | "set_value" | "undo" | "redo" | "run"
        );
        if writes && let Some(why) = self.human_editing() {
            return Err(format!(
                "{why}なので、文書は変えません(読むだけの道具は使えます)。人が終えてからもう一度"
            ));
        }
        let before = self.cur.work.doc().undo_depth();
        match tool {
            "status" => return Ok(Answer::Now(self.live_status())),
            // ふだんは待ち受けが画面へ送らずに答える。画面に届いた時も同じ答え
            "running" => {
                return Ok(Answer::Now(
                    json!({ "running": jc_session::live::running_json(&self.live_running()) }),
                ));
            }
            "selection" => return Ok(Answer::Now(self.live_selection())),
            "screenshot" => return Ok(Answer::Later(self.live_screenshot(a))),
            // 画面の部品を名前で操作する(人と同じ入力で。コマンド欄のコマンドとして流す)
            "ui_find" | "ui_tree" | "ui_click" | "ui_type" | "ui_key" | "ui_select" | "ui_pick"
            | "ui_expect" | "command" | "script" => {
                let text = ui_tool_script(tool, a)?;
                return self.run_script(&text).map(Answer::AfterScript);
            }
            "set_param" => {
                let (name, expr) = (need("name")?, need("expr")?);
                let p = self
                    .cur
                    .work
                    .doc()
                    .data()
                    .params
                    .iter()
                    .find(|p| p.name == name)
                    .ok_or_else(|| format!("パラメータ {name} はありません"))?
                    .id;
                let info = param_info(a)?;
                let data = self.cur.work.doc().data();
                let mut tx = jc_session::edit::set_param_expr(data, p, &expr);
                tx.ops.extend(info.op(data, p));
                tx.label = format!("Claude: {name} を {expr} に");
                let ok = self.apply(tx);
                self.ok_or_message(ok)?;
            }
            "add_param" => {
                let (name, expr) = (need("name")?, need("expr")?);
                if self
                    .cur
                    .work
                    .report()
                    .model_params
                    .iter()
                    .any(|p| p.name == name)
                    || self
                        .cur
                        .work
                        .doc()
                        .data()
                        .params
                        .iter()
                        .any(|p| p.name == name)
                {
                    return Err(format!("{name} という名前はもうあります"));
                }
                let info = param_info(a)?;
                let mut tx = self
                    .cur
                    .work
                    .prepare(|d| jc_session::edit::add_param_info(d, &name, &expr, &info));
                tx.label = format!("Claude: パラメータ {name} を足す");
                let ok = self.apply(tx);
                self.ok_or_message(ok)?;
            }
            "set_value" => {
                let (key, expr) = (need("key")?, need("expr")?);
                let id =
                    jc_core::FeatureId(a["feature"].as_u64().ok_or_else(|| {
                        "set_value には feature(機能の ID)が要ります".to_string()
                    })?);
                let mut tx = jc_session::params::set_feature_value(
                    self.cur.work.doc().data(),
                    &self.win.registry,
                    self.cur.work.report(),
                    id,
                    &key,
                    &expr,
                )?;
                tx.label = format!("Claude: {key} を {expr} に");
                let ok = self.apply(tx);
                self.ok_or_message(ok)?;
            }
            "undo" => self.undo(),
            "redo" => self.redo(),
            // 操作の一覧(引数の宣言つき)と、操作を ID で実行する(画面のボタンと同じ道。CLI の
            // `jc actions`・`jc run` と同じ引数の形)
            "actions" => return Ok(Answer::Now(json!({ "actions": self.actions() }))),
            "run" => {
                let id = need("id")?;
                if let Some(why) = self
                    .actions()
                    .into_iter()
                    .find(|x| x.id == id)
                    .and_then(|x| x.disabled)
                {
                    return Err(format!("操作 {id} は今使えません: {why}"));
                }
                let args = a.get("args").cloned().unwrap_or(Value::Null);
                let added = self.try_run_action_args(&id, args)?;
                if self.cur.work.doc().undo_depth() == before {
                    // 書き出しなど文書を変えない操作
                    return Ok(Answer::Now(
                        json!({ "ran": id, "message": self.win.message.clone() }),
                    ));
                }
                log::info!("コマンドで {id} を実行し、{added:?} を足した");
            }
            _ => {
                return Err(format!(
                    "道具 {tool} は知りません({})",
                    jc_session::live::APP_TOOLS.join("・")
                ));
            }
        }
        if !matches!(tool, "undo" | "redo") && self.cur.work.doc().undo_depth() == before {
            return Err("文書は変わりませんでした".into());
        }
        Ok(Answer::AfterRecompute)
    }

    fn ok_or_message(&self, ok: bool) -> Result<(), String> {
        if ok {
            Ok(())
        } else {
            Err(self
                .win
                .message
                .clone()
                .unwrap_or("変えられませんでした".into()))
        }
    }

    /// 今のタブの文書の要約(MCP の status に合わせた形の一部と、画面の物)。
    pub(crate) fn live_status(&self) -> Value {
        let data = self.cur.work.doc().data();
        let params: Vec<Value> = data
            .params
            .iter()
            .map(|p| {
                let mut o = json!({"name": p.name, "expr": p.expr});
                match self
                    .cur
                    .work
                    .report()
                    .params
                    .iter()
                    .find(|r| r.id == p.id)
                    .map(|r| &r.status)
                {
                    Some(jc_core::ParamStatus::Ok { value }) => {
                        o["value"] = json!(value.to_string())
                    }
                    Some(jc_core::ParamStatus::Error { message }) => o["error"] = json!(message),
                    None => {}
                }
                o
            })
            .collect();
        let features: Vec<Value> = data
            .features
            .iter()
            .map(|f| {
                let mut o = json!({"id": f.id.0, "name": f.name, "type": f.type_id});
                let (state, why) = match self.cur.work.report().feature(f.id) {
                    Some(jc_core::FeatureStatus::Ok { .. }) => ("ok", None),
                    Some(jc_core::FeatureStatus::Error { message }) => {
                        ("error", Some(message.clone()))
                    }
                    Some(jc_core::FeatureStatus::Blocked { by, .. }) => {
                        ("blocked", Some(format!("{by} が使えません")))
                    }
                    Some(jc_core::FeatureStatus::Suppressed) => ("suppressed", None),
                    Some(jc_core::FeatureStatus::RolledBack) => ("rolled_back", None),
                    None => ("unknown", None),
                };
                o["status"] = json!(state);
                if let Some(w) = why {
                    o["reason"] = json!(w);
                }
                o
            })
            .collect();
        let values: Vec<Value> = jc_session::params::feature_values(
            data,
            &self.win.registry,
            self.cur.work.report(),
        )
        .iter()
        .map(|v| json!({"feature": v.feature.0, "key": v.key, "label": v.label, "expr": v.expr}))
        .collect();
        json!({
            "document": self.document_name(),
            "file": self.cur.work.path().map(|p| p.display().to_string()),
            "modified": self.cur.modified,
            "tabs": self.tab_titles().iter().map(|(t, m)| json!({"name": t, "modified": m})).collect::<Vec<_>>(),
            "active_tab": self.active_tab(),
            "errors": self.cur.work.report().error_count(),
            "params": params,
            "features": features,
            "values": values,
            "selection": self.live_selection(),
            "human_editing": self.human_editing(),
            "can_undo": self.cur.work.doc().can_undo(),
            "can_redo": self.cur.work.doc().can_redo(),
            "running": jc_session::live::running_json(&self.live_running()),
        })
    }

    /// AI から受けて、まだ答えていない要求(接続をまたいで。道具の名前と、受けてからの長さ)。
    pub(crate) fn live_running(&self) -> Vec<(String, Duration)> {
        self.live
            .listener
            .as_ref()
            .map(|l| l.running())
            .unwrap_or_default()
    }

    /// 人が選んでいる面・辺・頂点(機能の ID と名前。MCP の measure・screenshot にそのまま渡せる)。
    fn live_selection(&self) -> Value {
        json!(self.picked_refs())
    }

    /// 画面の今の向きで、見えているボディを絵にする(選んだ面は差し色)。網にして描くのは重いので、
    /// 今の値だけを取り、画面のスレッドの外で計算する仕事を返す(窓を固めない)。
    fn live_screenshot(&self, a: &Value) -> Box<dyn FnOnce() -> Result<Value, String> + Send> {
        let w = a["width"].as_u64().unwrap_or(800).clamp(64, 2048) as u32;
        let h = a["height"].as_u64().unwrap_or(600).clamp(64, 2048) as u32;
        let tol = self.win.layout.render.mesh.tolerance();
        let bodies = self.visible_bodies();
        let kernel = self.cur.work.kernel().clone();
        let (yaw, pitch) = (
            self.cur.camera.yaw.to_degrees(),
            self.cur.camera.pitch.to_degrees(),
        );
        let picked = self.picked_faces();
        Box::new(move || {
            let meshes: Vec<_> = bodies
                .into_iter()
                .filter_map(|(f, s)| Some((f, kernel.mesh(s, tol).ok()?)))
                .collect();
            if meshes.is_empty() {
                return Err("見えているボディがありません".into());
            }
            let b: Vec<_> = meshes.iter().map(|(f, m)| (*f, m)).collect();
            Ok(live::screenshot(&b, (w, h), yaw, pitch, picked))
        })
    }

    /// 下の帯の「Claude」の札(待ち受けていなければ何も出さない)。押すと切る。
    pub(crate) fn live_badge(&mut self, ui: &mut egui::Ui) {
        let l = crate::theme::look(ui.ctx());
        let (text, color) = match self.live_state() {
            LiveState::Off => return,
            LiveState::Listening => ("Claude 待ち受け中", l.pal.ink2),
            LiveState::Connected => ("Claude 接続中", l.ws()),
            LiveState::Writing => ("Claude 操作中", l.pal.warn),
        };
        let text = match (&self.live.wrote_by, self.live_state()) {
            (Some(by), LiveState::Writing) => format!("{text}: {by}"),
            _ => text.to_string(),
        };
        if self.live_state() != LiveState::Listening {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        let r = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("● {text}"))
                        .size(crate::theme::fs(11.5))
                        .color(color),
                )
                .small()
                .frame(false),
            )
            .on_hover_text(format!(
                "Claude(AI)からの接続を受けています(この PC の中だけ、ポート {})。{}\n押すと切ります(設定の「AI との接続」でまた入れられます)",
                self.live_port().unwrap_or(0),
                running_text(&self.live_running()),
            ));
        if r.clicked() {
            self.live_disconnect();
        }
    }

    /// 設定の「AI との接続」の節。
    pub(crate) fn live_settings(&mut self, ui: &mut egui::Ui) {
        if ui
            .checkbox(&mut self.win.layout.ai_live, "AI からの接続を受ける")
            .on_hover_text(
                "入れると、この PC の中だけで Claude(MCP の app の道具)からの接続を待ちます。\n\
                 起動ごとに合い言葉を作り、データフォルダの live.json に書きます(閉じると消します)",
            )
            .changed()
        {
            self.live.failed = None;
        }
        ui.weak(
            "Claude は今のタブの文書の状態・選んでいる面や辺・画面の向きの絵を読み、パラメータや機能の値を変えられます。\
             変更は取り消しの履歴に「Claude: 」付きで入り、Ctrl+Z で戻せます。人が機能を編集している間は、文書を変える要求を断ります",
        );
        match (&self.live.failed, self.live_port()) {
            (Some(e), _) => {
                ui.colored_label(crate::theme::look(ui.ctx()).pal.err, e);
            }
            (None, Some(p)) => {
                ui.weak(format!("待ち受け中: 127.0.0.1:{p}"));
            }
            _ => {}
        }
    }
}

/// 要求の答え方。
pub(crate) enum Answer {
    /// すぐ答える
    Now(Value),
    /// 変えたので、再計算が終わってから status で答える
    AfterRecompute,
    /// コマンド欄でスクリプトとして流し、流し終えたら結果で答える
    AfterScript(u64),
    /// 画面のスレッドの外で計算して答える(絵を描くなど重い読むだけの道具。窓を固めない)
    Later(Box<dyn FnOnce() -> Result<Value, String> + Send>),
}

/// 動かしている要求の一覧の文(無ければ空)。
fn running_text(list: &[(String, Duration)]) -> String {
    if list.is_empty() {
        return String::new();
    }
    let items: Vec<String> = list
        .iter()
        .map(|(tool, d)| format!("{tool}({:.1} 秒)", d.as_secs_f64()))
        .collect();
    format!("\n動かしている要求: {}", items.join("、"))
}

/// 画面の部品の道具を、コマンド欄の 1 行(script は何行も)にする。
fn ui_tool_script(tool: &str, a: &Value) -> Result<String, String> {
    let s = |k: &str| a[k].as_str().map(str::to_string);
    let need = |k: &str| s(k).ok_or_else(|| format!("{tool} には {k} が要ります"));
    let q = |t: String| format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\""));
    Ok(match tool {
        "ui_find" => format!("find {}", q(need("target")?)),
        "ui_tree" => format!("tree {}", q(s("filter").unwrap_or_default())),
        "ui_click" => {
            let verb = match s("button").as_deref() {
                Some("right") => "rclick",
                Some("double") => "dclick",
                _ => "click",
            };
            format!("{verb} {}", q(need("target")?))
        }
        "ui_type" => format!(
            "type {} {} {}",
            q(need("target")?),
            q(need("text")?),
            s("commit").unwrap_or_else(|| "tab".into())
        ),
        "ui_key" => format!("key {}", need("keys")?),
        "ui_select" => format!("select {} {}", q(need("target")?), q(need("option")?)),
        "ui_pick" => format!("pick {}", need("face")?),
        "ui_expect" => match (s("equals"), s("contains"), s("state")) {
            // 値も引用符で囲む(囲まないと " で書き方の誤り、続く空白は 1 つに詰まり、空の値は読めない)
            (Some(v), _, _) => format!("expect {} = {}", q(need("target")?), q(v)),
            (_, Some(v), _) => format!("expect {} ~ {}", q(need("target")?), q(v)),
            (_, _, Some(st)) => format!("expect {} {st}", q(need("target")?)),
            _ => format!("expect {}", q(need("target")?)),
        },
        "command" => need("line")?,
        _ => match &a["lines"] {
            Value::Array(v) => v
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n"),
            _ => need("text")?,
        },
    })
}

/// AI の set_param・add_param の引数のコメントと単位(省けば今のまま)。
fn param_info(a: &serde_json::Value) -> Result<jc_session::edit::ParamInfo, String> {
    jc_session::edit::ParamInfo::parse(a["comment"].as_str(), a["unit"].as_str())
}

#[cfg(test)]
mod expect_tests {
    use super::*;

    /// 比べる値に引用符・続く空白・空があっても、そのまま比べる行になる。
    #[test]
    fn expected_values_keep_quotes_spaces_and_emptiness() {
        let line = |v: &str| {
            ui_tool_script(
                "ui_expect",
                &serde_json::json!({"target": "欄", "equals": v}),
            )
            .unwrap()
        };
        for v in [r#"5 "x""#, "a  b", ""] {
            let cmd = crate::cmdline::parse(&line(v)).unwrap();
            assert_eq!(
                cmd,
                Some(crate::cmdline::Cmd::Expect(
                    crate::drive::Query::parse("欄"),
                    crate::cmdline::Check::Eq(v.into())
                )),
                "{v:?}"
            );
        }
    }
}
