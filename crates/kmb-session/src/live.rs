//! 動いている画面と AI のつなぎ(候補 21)。画面の設定「AI からの接続を受ける」を入れると、
//! 画面がこの PC の中だけ(127.0.0.1)で待ち受け、MCP の `app` の道具がそこへ要求を送る。
//!
//! - 待ち受けの番号と起動ごとの合い言葉は、データフォルダの `live.json` に書く(同じ PC の同じ
//!   利用者だけが読める所。Unix では 0600)。画面を閉じるか接続を切ると消す。
//! - 1 つの接続で 1 つの要求: `{"token", "tool", "args", "roots"}` の 1 行を送り、`{"ok": 値}` か
//!   `{"error": 理由}` の 1 行を受ける。要求は画面の 1 フレームの間に処理する(文書は画面の物)。
//! - 合い言葉が違う要求は、違うことと待ち受けている画面のプロセスの番号だけを答えて閉じる(合い言葉は
//!   言わない)。頼む側はそれと `live.json` の中身を比べ、どの鍵が古いかを言う([`call`] の誤り)。
//! - MCP から来た要求は `roots`(AI に許したフォルダ)を添える。画面はその要求で動く操作の
//!   ファイルの引数を、その中に限る([`crate::roots::Roots::confine_args`])。

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// 動いている画面の道具の名前(MCP の `app` の `tool`)。画面の振り分け・知らない道具の文・MCP の
/// 説明は、試験でこれと突き合わせる。
pub const APP_TOOLS: &[&str] = &[
    "status",
    "screenshot",
    "selection",
    "set_param",
    "add_param",
    "set_value",
    "undo",
    "redo",
    "actions",
    "run",
    "ui_find",
    "ui_tree",
    "ui_click",
    "ui_type",
    "ui_key",
    "ui_select",
    "ui_pick",
    "ui_expect",
    "command",
    "script",
    "running",
];

/// 待ち受けの番号と合い言葉(`live.json`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ticket {
    pub port: u16,
    pub token: String,
    /// 画面のプロセスの番号(古いファイルを見分ける)
    pub pid: u32,
}

/// バイト列を base64 にする(画面の絵を JSON で運ぶ。MCP の絵も同じ)。
pub fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// 画面の向きでボディを絵にし、`screenshot` の答え(PNG を base64 で)にする。
pub fn screenshot(
    bodies: &[(kmb_core::FeatureId, &kmb_core::Mesh)],
    size: (u32, u32),
    yaw_deg: f32,
    pitch_deg: f32,
    faces: Vec<kmb_core::FacePick>,
) -> Value {
    let b: Vec<kmb_render::picture::Body> = bodies
        .iter()
        .map(|(f, m)| kmb_render::picture::Body {
            feature: *f,
            mesh: m,
        })
        .collect();
    let p = kmb_render::picture::render(
        &b,
        &kmb_render::picture::Options {
            width: size.0,
            height: size.1,
            yaw_deg,
            pitch_deg,
            faces,
            edges: Vec::new(),
        },
    );
    json!({
        "png": base64(&p.png()),
        "width": p.width,
        "height": p.height,
        "yaw": yaw_deg,
        "pitch": pitch_deg,
        "px_per_mm": (p.px_per_mm * 1000.0).round() / 1000.0,
        "bodies": b.iter().map(|b| b.feature.0).collect::<Vec<_>>(),
    })
}

/// `live.json` の場所。
pub fn ticket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("live.json")
}

/// 起動ごとの合い言葉(32 桁の 16 進)。OS の乱数で種を作る `RandomState` を 2 つ使う。
pub fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let part = || {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.write_u32(std::process::id());
        h.finish()
    };
    format!("{:016x}{:016x}", part(), part())
}

/// 合い言葉を比べる(長さが同じなら、どこで違っても同じ時間で)。
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn write_ticket(data_dir: &Path, t: &Ticket) -> Result<(), String> {
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let p = ticket_path(data_dir);
    let s = serde_json::to_string_pretty(t).expect("JSON にできる");
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&p)
            .map_err(|e| format!("{} に書けません: {e}", p.display()))?;
        f.write_all(s.as_bytes()).map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        // Windows は利用者ごとの %APPDATA% の下なので、ほかの利用者からは読めない
        std::fs::write(&p, s).map_err(|e| format!("{} に書けません: {e}", p.display()))
    }
}

/// 画面へ届いた要求 1 つ。`reply` へ答えを送る。
pub struct Request {
    pub tool: String,
    pub args: Value,
    /// AI に許したフォルダ(MCP から来た頼み)。あれば、ファイルを読む引数はこの中に限る
    pub roots: Option<crate::roots::Roots>,
    pub reply: mpsc::Sender<Result<Value, String>>,
}

/// 画面の待ち受け。落とすと止まり、`live.json` を消す。
pub struct Listener {
    pub ticket: Ticket,
    data_dir: PathBuf,
    stop: Arc<AtomicBool>,
    running: Running,
}

/// 受けて、まだ答えていない要求(道具の名前と受けた時)。接続は要求ごとに別なので、接続をまたいで
/// 1 つの一覧に持つ。
#[derive(Clone, Default)]
struct Running(Arc<Mutex<Vec<(u64, String, Instant)>>>);

impl Running {
    /// 一覧に足し、落とすと外れる印を返す。
    fn enter(&self, tool: &str) -> RunningGuard {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut v) = self.0.lock() {
            v.push((id, tool.to_string(), Instant::now()));
        }
        RunningGuard(self.clone(), id)
    }

    /// 道具の名前と、受けてからの長さ(古い順)。
    fn list(&self) -> Vec<(String, Duration)> {
        self.0
            .lock()
            .map(|v| {
                v.iter()
                    .map(|(_, t, at)| (t.clone(), at.elapsed()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// 頼みの一覧の答え(`running` の道具と、画面の `status` の `running`)。
pub fn running_json(list: &[(String, Duration)]) -> Value {
    json!(
        list.iter()
            .map(|(tool, d)| json!({
                "tool": tool,
                "seconds": (d.as_secs_f64() * 10.0).round() / 10.0,
            }))
            .collect::<Vec<_>>()
    )
}

struct RunningGuard(Running, u64);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        if let Ok(mut v) = (self.0).0.lock() {
            v.retain(|(id, _, _)| *id != self.1);
        }
    }
}

impl Listener {
    /// 待ち受けを始める。届いた要求は `tx` へ送り、`wake` で画面を起こす。
    pub fn start(
        data_dir: &Path,
        tx: mpsc::Sender<Request>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Listener, String> {
        let l =
            TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("待ち受けられません: {e}"))?;
        l.set_nonblocking(true).map_err(|e| e.to_string())?;
        let ticket = Ticket {
            port: l.local_addr().map_err(|e| e.to_string())?.port(),
            token: new_token(),
            pid: std::process::id(),
        };
        write_ticket(data_dir, &ticket)?;
        let stop = Arc::new(AtomicBool::new(false));
        let (stop2, token) = (stop.clone(), ticket.token.clone());
        let wake = Arc::new(wake);
        let running = Running::default();
        let running2 = running.clone();
        std::thread::spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                match l.accept() {
                    Ok((s, _)) => {
                        let (tx, token, wake) = (tx.clone(), token.clone(), wake.clone());
                        let running = running2.clone();
                        std::thread::spawn(move || serve(s, &token, &tx, &*wake, &running));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        log::warn!("AI の接続: 受けられません: {e}");
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
        });
        log::info!("AI の接続: 127.0.0.1:{} で待ち受け", ticket.port);
        Ok(Listener {
            ticket,
            data_dir: data_dir.to_path_buf(),
            stop,
            running,
        })
    }

    /// 受けて、まだ答えていない要求(道具の名前と、受けてからの長さ。古い順)。
    pub fn running(&self) -> Vec<(String, Duration)> {
        self.running.list()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let p = ticket_path(&self.data_dir);
        // ほかの画面が書き直した物は消さない
        if read_ticket(&self.data_dir).is_some_and(|t| t == self.ticket) {
            let _ = std::fs::remove_file(&p);
        }
        log::info!("AI の接続: 待ち受けをやめた");
    }
}

/// 要求の答えを待つ長さ(重い再計算も入る)。
const ANSWER_WAIT: Duration = Duration::from_secs(120);

/// 要求 `tool` の答えを待つ長さ。画面の手順(`script` の `lines` か `text`、`command` の `line`)は、`until` が
/// 待つ長さ(書いていなければ 120 秒)を足す。足さないと、`until "済み" 300s` の手順は答えが
/// 切れた後も画面で流れ続け、結果が捨てられる。
fn answer_wait(tool: &str, args: &Value) -> Duration {
    let lines: Vec<&str> = match tool {
        "command" => args["line"].as_str().into_iter().collect(),
        _ => match &args["lines"] {
            Value::Array(v) => v.iter().filter_map(Value::as_str).collect(),
            _ => args["text"].as_str().into_iter().collect(),
        },
    };
    let until: f64 = lines
        .iter()
        .flat_map(|l| l.lines())
        .filter(|l| l.split_whitespace().next() == Some("until"))
        .map(|l| {
            l.split_whitespace()
                .last()
                .and_then(|w| w.strip_suffix('s'))
                .and_then(|n| n.parse::<f64>().ok())
                .filter(|t| t.is_finite() && *t >= 0.0)
                .unwrap_or(120.0)
        })
        .sum();
    ANSWER_WAIT + Duration::from_secs_f64(until.min(86_400.0))
}

fn serve(
    s: TcpStream,
    token: &str,
    tx: &mpsc::Sender<Request>,
    wake: &dyn Fn(),
    running: &Running,
) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut line = String::new();
    let Ok(mut out) = s.try_clone() else {
        return;
    };
    if BufReader::new(s)
        .take(16 << 20)
        .read_line(&mut line)
        .is_err()
    {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(&line) else {
        return;
    };
    if !same(v["token"].as_str().unwrap_or(""), token) {
        log::warn!("AI の接続: 合い言葉の違う要求を断った");
        let _ = writeln!(
            out,
            "{}",
            json!({ "error": "合い言葉が違います", "refused": "token", "pid": std::process::id() })
        );
        return;
    }
    // 頼みの一覧は画面へ送らずここで答える(画面が固まった時こそ知りたいので)
    if v["tool"] == "running" {
        let _ = writeln!(
            out,
            "{}",
            json!({ "ok": { "running": running_json(&running.list()) } })
        );
        return;
    }
    let (rtx, rrx) = mpsc::channel();
    let wait = answer_wait(v["tool"].as_str().unwrap_or(""), &v["args"]);
    let mark = running.enter(v["tool"].as_str().unwrap_or(""));
    let req = Request {
        tool: v["tool"].as_str().unwrap_or("").to_string(),
        args: v.get("args").cloned().unwrap_or(json!({})),
        roots: v["roots"].as_array().and_then(|a| {
            crate::roots::Roots::from_dirs(
                a.iter()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from)
                    .collect(),
            )
        }),
        reply: rtx,
    };
    let answer = if tx.send(req).is_ok() {
        wake();
        match rrx.recv_timeout(wait) {
            Ok(Ok(v)) => json!({ "ok": v }),
            Ok(Err(e)) => json!({ "error": e }),
            Err(_) => {
                // 時間切れでも、画面はまだこの頼みを持っている。答えるか捨てるまで一覧に残す
                std::thread::spawn(move || {
                    let _ = rrx.recv();
                    drop(mark);
                });
                json!({ "error": "画面が答えません(重い計算の途中か、止まっています)" })
            }
        }
    } else {
        json!({ "error": "画面が接続を閉じています" })
    };
    let _ = writeln!(out, "{answer}");
}

/// `live.json` を読む(無いか読めなければ `None`)。
pub fn read_ticket(data_dir: &Path) -> Option<Ticket> {
    serde_json::from_str(&std::fs::read_to_string(ticket_path(data_dir)).ok()?).ok()
}

/// 動いている画面へ要求を送り、答えを受ける(人が書いた試験の台本など、フォルダを限らない頼み)。
pub fn call(data_dir: &Path, tool: &str, args: Value) -> Result<Value, String> {
    send(data_dir, json!({ "tool": tool, "args": args }))
}

/// [`call`] の、AI に許したフォルダを添える版(MCP の `app` の道具が使う)。画面は、この頼みで動く
/// 操作のファイルの引数を `roots` の中に限る。
pub fn call_confined(
    data_dir: &Path,
    tool: &str,
    args: Value,
    roots: &crate::roots::Roots,
) -> Result<Value, String> {
    let dirs: Vec<String> = roots
        .dirs()
        .iter()
        .map(|d| d.display().to_string())
        .collect();
    send(
        data_dir,
        json!({ "tool": tool, "args": args, "roots": dirs }),
    )
}

/// 入れ直す手順(つながらない誤りの終わりに添える)。
const RETRY: &str =
    "画面で「設定 → 一般 → AI との接続」の「AI からの接続を受ける」を入れ直してください";

/// 読んだ `live.json` の出どころ(つながらない時の誤りに添える): 場所、書いた画面の番号と待ち受けの
/// 番号、書いてからの長さ。
fn ticket_origin(data_dir: &Path, t: &Ticket) -> String {
    let p = ticket_path(data_dir);
    let age = std::fs::metadata(&p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.elapsed().ok())
        .map(|d| match d.as_secs() {
            s if s < 120 => format!("、{s} 秒前に書いた"),
            s if s < 7200 => format!("、{} 分前に書いた", s / 60),
            s => format!("、{} 時間前に書いた", s / 3600),
        })
        .unwrap_or_default();
    format!(
        "{}(画面のプロセス {}、127.0.0.1:{}{age})",
        p.display(),
        t.pid,
        t.port
    )
}

fn send(data_dir: &Path, mut req: Value) -> Result<Value, String> {
    let tool = req["tool"].as_str().unwrap_or_default().to_string();
    let path = ticket_path(data_dir);
    let t = match read_ticket(data_dir) {
        Some(t) => t,
        None if path.exists() => {
            return Err(format!(
                "{} を読めません(壊れています)。{RETRY}",
                path.display()
            ));
        }
        None => {
            return Err(format!(
                "動いている画面が見つかりません({} がありません)。画面で「設定 → 一般 → AI との接続」の\
                 「AI からの接続を受ける」を入れてください(画面の無い形の道具はそのまま使えます)。\
                 入れているのに見つからない時は、画面とこの道具のデータフォルダが違います",
                path.display()
            ));
        }
    };
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], t.port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| {
        format!(
            "画面につながりません({e})。古いのは {}: そこで待ち受けている画面がありません\
             (画面を閉じたか落ちた後に残った物)。{RETRY}",
            ticket_origin(data_dir, &t)
        )
    })?;
    let _ = s.set_read_timeout(Some(
        answer_wait(&tool, &req["args"]) + Duration::from_secs(5),
    ));
    req["token"] = json!(t.token);
    writeln!(s, "{req}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s)
        .read_line(&mut line)
        .map_err(|e| format!("画面の答えを読めません: {e}"))?;
    let v: Value = serde_json::from_str(&line).map_err(|_| {
        format!(
            "画面が答えずに閉じました(合い言葉が古いかもしれません。読んだのは {})",
            ticket_origin(data_dir, &t)
        )
    })?;
    if v["refused"] == "token" {
        return Err(stale_token(data_dir, &t, v["pid"].as_u64()));
    }
    match v.get("error") {
        Some(e) => Err(e.as_str().unwrap_or("失敗しました").to_string()),
        None => Ok(v.get("ok").cloned().unwrap_or(Value::Null)),
    }
}

/// 合い言葉を断られた時の誤り: `live.json` の合い言葉が古い。待ち受けている画面の番号と比べて、
/// 別の画面の物か、同じ画面の前の接続の物かを言う。
fn stale_token(data_dir: &Path, t: &Ticket, listening: Option<u64>) -> String {
    let origin = ticket_origin(data_dir, t);
    let why = match listening {
        Some(p) if p != u64::from(t.pid) => format!(
            "127.0.0.1:{} で待ち受けているのは別の画面(プロセス {p})です。live.json を書いた画面は\
             閉じたか落ちて、番号だけが別の画面に使われています",
            t.port
        ),
        Some(_) => "同じ画面ですが、接続を入れ直す前の合い言葉です".to_string(),
        None => "待ち受けている画面の合い言葉と違います".to_string(),
    };
    format!("合い言葉が古いので断られました。古いのは {origin} の合い言葉です: {why}。{RETRY}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_script_with_until_waits_as_long_as_its_untils() {
        use super::{ANSWER_WAIT, answer_wait};
        use serde_json::json;
        use std::time::Duration;
        let lines =
            json!({"lines": ["click \"▶  実行\"", "until \"済み\" 300s", "until \"表示\""]});
        assert_eq!(
            answer_wait("script", &lines),
            ANSWER_WAIT + Duration::from_secs(420)
        );
        assert_eq!(
            answer_wait("ui_click", &json!({"target": "熱"})),
            ANSWER_WAIT
        );
        let cmd = json!({"line": "until \"済み\" 30s"});
        assert_eq!(
            answer_wait("command", &cmd),
            ANSWER_WAIT + Duration::from_secs(30)
        );
    }

    use super::*;

    #[test]
    fn a_request_with_the_token_reaches_the_screen_and_others_are_refused() {
        let d = std::env::temp_dir().join(format!("kmb-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(
            call(&d, "status", json!({}))
                .unwrap_err()
                .contains("AI からの接続を受ける")
        );
        let (tx, rx) = mpsc::channel::<Request>();
        let l = Listener::start(&d, tx, || {}).unwrap();
        assert_eq!(read_ticket(&d), Some(l.ticket.clone()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(ticket_path(&d))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(m & 0o777, 0o600);
        }
        // 画面の側(フレームの間に要求を処理する)
        let screen = std::thread::spawn(move || {
            for r in rx.iter().take(2) {
                let a = if r.tool == "echo" {
                    Ok(r.args.clone())
                } else {
                    Err(format!("{} は知らない", r.tool))
                };
                r.reply.send(a).unwrap();
            }
        });
        assert_eq!(call(&d, "echo", json!({"x": 1})).unwrap(), json!({"x": 1}));
        assert_eq!(call(&d, "nope", json!({})).unwrap_err(), "nope は知らない");
        // 合い言葉が違えば断り、どの鍵が古いか(live.json の場所と書いた画面)を言う
        let mut bad = l.ticket.clone();
        bad.token = new_token();
        write_ticket(&d, &bad).unwrap();
        let e = call(&d, "echo", json!({})).unwrap_err();
        assert!(e.contains("合い言葉が古い"), "{e}");
        assert!(
            e.contains("接続を入れ直す前"),
            "同じ画面の前の合い言葉: {e}"
        );
        assert!(e.contains(&ticket_path(&d).display().to_string()), "{e}");
        // 別の画面が書いた live.json(番号だけ今の画面と同じ)
        bad.pid = l.ticket.pid + 1;
        write_ticket(&d, &bad).unwrap();
        let e = call(&d, "echo", json!({})).unwrap_err();
        assert!(e.contains(&format!("プロセス {}", l.ticket.pid)), "{e}");
        assert!(e.contains("別の画面"), "{e}");
        write_ticket(&d, &l.ticket).unwrap();
        screen.join().unwrap();
        // 止めると live.json を消す
        let port = l.ticket.port;
        drop(l);
        assert!(read_ticket(&d).is_none());
        // 落ちた画面が残した live.json は、待ち受けが無いので古いと言う
        std::thread::sleep(Duration::from_millis(100));
        write_ticket(&d, &bad).unwrap();
        let e = call(&d, "echo", json!({})).unwrap_err();
        assert!(e.contains("待ち受けている画面がありません"), "{port}: {e}");
        // 壊れた live.json と、無い時の探した場所
        std::fs::write(ticket_path(&d), "{").unwrap();
        assert!(call(&d, "echo", json!({})).unwrap_err().contains("壊れて"));
        std::fs::remove_file(ticket_path(&d)).unwrap();
        assert!(
            call(&d, "echo", json!({}))
                .unwrap_err()
                .contains("live.json がありません")
        );
        assert_ne!(new_token(), new_token());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 画面が固まっていても、頼みの一覧は待ち受けが答える。答えの無い頼みは画面が手放すまで載る。
    #[test]
    fn the_running_list_answers_while_the_screen_is_stuck() {
        use super::*;
        let d = std::env::temp_dir().join(format!("kmb-live-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (tx, rx) = mpsc::channel::<Request>();
        let _l = Listener::start(&d, tx, || {}).unwrap();
        // 画面は頼みを受けても答えない(固まっている)
        let d2 = d.clone();
        let stuck = std::thread::spawn(move || call(&d2, "status", json!({})));
        let tools = || -> Vec<String> {
            call(&d, "running", json!({})).unwrap()["running"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["tool"].as_str().unwrap().to_string())
                .collect()
        };
        let t0 = Instant::now();
        while tools().is_empty() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(tools(), ["status"]);
        // 画面が頼みを手放すと答えが返り、一覧から外れる
        drop(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        assert!(stuck.join().unwrap().is_err());
        let t0 = Instant::now();
        while !tools().is_empty() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(tools().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
