//! WASM のプラグイン(Component Model)を読み込む。今はポストだけ。
//!
//! インタフェースは `wit/post.wit`。プラグインには WASI を渡さないので、ファイル、
//! ネットワーク、時計には触れない。1 回の呼び出しで使える計算量(fuel)とメモリにも上限を置く。
//! プラグインが止まらない、メモリを使いすぎる、panic する、のどれでも、ホストは誤りとして返す。

use jc_post::{Diagnostic, MachineProfile, Post, PostError, Severity, check_common};
use jc_toolpath::Program;
use std::path::Path;
use std::sync::Mutex;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "post",
    });
}

use bindings::jisaku_cadcam::post::types;

/// 1 回の呼び出しで使える計算量(基準部品の G-code には十分に余る量)
pub const FUEL_PER_CALL: u64 = 20_000_000_000;
/// プラグインが使えるメモリ
pub const MEMORY_LIMIT: usize = 256 << 20;

struct State {
    limits: StoreLimits,
}

/// 読み込んだポストのプラグイン。
pub struct WasmPost {
    id: String,
    describe: String,
    fuel: u64,
    engine: Engine,
    component: Component,
    inner: Mutex<(Store<State>, bindings::Post)>,
}

/// 新しい実行環境でプラグインを始める。
fn instantiate(
    engine: &Engine,
    component: &Component,
) -> Result<(Store<State>, bindings::Post), String> {
    // 何も渡さない(WASI もない)
    let linker = Linker::<State>::new(engine);
    let mut store = store(engine);
    store.set_fuel(FUEL_PER_CALL).map_err(|e| e.to_string())?;
    let post = bindings::Post::instantiate(&mut store, component, &linker)
        .map_err(|e| format!("プラグインを始められません: {e}"))?;
    Ok((store, post))
}

fn store(engine: &Engine) -> Store<State> {
    let mut store = Store::new(
        engine,
        State {
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_LIMIT)
                .instances(4)
                .build(),
        },
    );
    store.limiter(|s| &mut s.limits);
    store
}

fn plugin_err(message: String) -> PostError {
    PostError(vec![Diagnostic {
        severity: Severity::Error,
        message,
    }])
}

fn from_wit(d: types::Diagnostic) -> Diagnostic {
    Diagnostic {
        severity: match d.severity {
            types::Severity::Error => Severity::Error,
            types::Severity::Warning => Severity::Warning,
        },
        message: d.message,
    }
}

impl WasmPost {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("{} を読めません: {e}", path.display()))?;
        Self::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).map_err(|e| e.to_string())?;
        let component = Component::new(&engine, bytes)
            .map_err(|e| format!("WASM のコンポーネントとして読めません: {e}"))?;
        let (mut store, post) = instantiate(&engine, &component)?;
        let id = post
            .call_id(&mut store)
            .map_err(|e| format!("id を呼べません: {e}"))?;
        let describe = post
            .call_describe(&mut store)
            .map_err(|e| format!("describe を呼べません: {e}"))?;
        Ok(Self {
            id,
            describe,
            fuel: FUEL_PER_CALL,
            engine,
            component,
            inner: Mutex::new((store, post)),
        })
    }

    pub fn describe(&self) -> &str {
        &self.describe
    }

    /// 1 回の呼び出しで使える計算量を変える。
    pub fn set_fuel_per_call(&mut self, fuel: u64) {
        self.fuel = fuel;
    }

    /// プラグインを呼ぶ。毎回、計算量を満タンに戻す。
    fn call<T>(
        &self,
        p: &Program,
        m: &MachineProfile,
        f: impl FnOnce(&bindings::Post, &mut Store<State>, &str, &str) -> wasmtime::Result<T>,
    ) -> Result<T, PostError> {
        let mut program = serde_json::to_value(p).map_err(|e| plugin_err(e.to_string()))?;
        tools_for_plugins(&mut program);
        ops_for_plugins(&mut program);
        let program = program.to_string();
        let machine = serde_json::to_string(m).map_err(|e| plugin_err(e.to_string()))?;
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let (store, post) = &mut *g;
        store
            .set_fuel(self.fuel)
            .map_err(|e| plugin_err(e.to_string()))?;
        match f(post, store, &program, &machine) {
            Ok(v) => Ok(v),
            Err(e) => {
                let why = if store.get_fuel().is_ok_and(|f| f == 0) {
                    "計算量の上限を超えました".to_string()
                } else {
                    format!("{e:#}")
                };
                // 途中で止まったインスタンスには入り直せないので、作り直しておく
                if let Ok(fresh) = instantiate(&self.engine, &self.component) {
                    *g = fresh;
                }
                Err(plugin_err(format!(
                    "プラグイン {} が止まりました: {why}",
                    self.id
                )))
            }
        }
    }
}

impl Post for WasmPost {
    fn id(&self) -> &str {
        &self.id
    }

    fn check(&self, p: &Program, m: &MachineProfile) -> Vec<Diagnostic> {
        let mut d = check_common(p, m);
        d.extend(check_new_ops(p, &self.id));
        match self.call(p, m, |post, store, a, b| post.call_check(store, a, b)) {
            Ok(v) => d.extend(v.into_iter().map(from_wit)),
            Err(e) => d.extend(e.0),
        }
        d
    }

    fn emit(&self, p: &Program, m: &MachineProfile) -> Result<String, PostError> {
        let diags = self.check(p, m);
        if diags.iter().any(|d| d.severity == Severity::Error) {
            return Err(PostError(diags));
        }
        self.call(p, m, |post, store, a, b| post.call_emit(store, a, b))?
            .map_err(|v| PostError(v.into_iter().map(from_wit).collect()))
    }
}

#[cfg(test)]
mod tests;

/// プラグインに渡す工具の形を、インタフェースを決めた時の形(工具ごとに切削条件が 1 つ)に保つ。
/// 工具の型が変わっても、作り済みのプラグインが読めるように(`cam.tools@2` で形を変えた)。
/// 足した項目(シャンク径など)は消し、材料ごとの条件は既定(最初)のものを `cutting` にする。
fn tools_for_plugins(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(o) => {
            if o.contains_key("flute_length") && o.contains_key("presets") {
                let cutting = o
                    .get("presets")
                    .and_then(|p| p.get(0))
                    .cloned()
                    .map(|mut c| {
                        if let Some(c) = c.as_object_mut() {
                            c.remove("material");
                        }
                        c
                    });
                const KEEP: [&str; 7] = [
                    "number",
                    "name",
                    "kind",
                    "diameter",
                    "flute_length",
                    "flutes",
                    "tip_angle",
                ];
                o.retain(|k, _| KEEP.contains(&k.as_str()));
                // V ビットは後から足した種類。先端の形が同じドリルとして渡す
                if o.get("kind").and_then(|k| k.as_str()) == Some("v_bit") {
                    o.insert("kind".into(), "drill".into());
                }
                // テーパーも後から足した種類。先が平らなので平として渡す
                if o.get("kind").and_then(|k| k.as_str()) == Some("taper_end") {
                    o.insert("kind".into(), "flat_end".into());
                }
                // ブルノーズは平、テーパーボールは先が球なのでボールとして渡す
                match o.get("kind").and_then(|k| k.as_str()) {
                    Some("bull_nose") => {
                        o.insert("kind".into(), "flat_end".into());
                    }
                    Some("taper_ball") => {
                        o.insert("kind".into(), "ball_end".into());
                    }
                    _ => {}
                }
                if let Some(c) = cutting {
                    o.insert("cutting".into(), c);
                }
            } else {
                o.values_mut().for_each(tools_for_plugins);
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(tools_for_plugins),
        _ => {}
    }
}

/// インタフェース(`post@0.1.0`)を決めた後に足した命令のうち、プラグインに渡せないものの誤り。
/// 停止(手動 NC の停止・任意停止)は、黙ってコメントにすると機械が止まらずにクランプへ
/// 向かうおそれがあるので、誤りにして書き出さない(内蔵のポストを使ってもらう)。
fn check_new_ops(p: &Program, id: &str) -> Vec<Diagnostic> {
    use jc_toolpath::Op;
    let stops = p
        .ops
        .iter()
        .filter(|o| matches!(o, Op::Stop { .. }))
        .count();
    if stops == 0 {
        return Vec::new();
    }
    vec![Diagnostic {
        severity: Severity::Error,
        message: format!(
            "ポストのプラグイン {id} には停止(手動 NC)の命令を渡せません({stops} 個)。内蔵のポストで書き出すか、停止を抑制してください"
        ),
    }]
}

/// プラグインに渡す命令を、インタフェースを決めた時の種類だけにする(作り済みのプラグインが
/// 知らない種類で読めなくならないように)。知らせ(`message`)はコメントにする。停止(`stop`)は
/// [`check_new_ops`] が誤りにして出力しないが、検査のために渡す時はコメントにしておく。
fn ops_for_plugins(v: &mut serde_json::Value) {
    let Some(ops) = v.get_mut("ops").and_then(|o| o.as_array_mut()) else {
        return;
    };
    for op in ops {
        let text = match op.get("op").and_then(|k| k.as_str()) {
            Some("message") => format!(
                "知らせ: {}",
                op.get("text").and_then(|t| t.as_str()).unwrap_or_default()
            ),
            Some("stop") => "停止(このプラグインには渡せない)".to_string(),
            _ => continue,
        };
        *op = serde_json::json!({"op": "comment", "text": text});
    }
}
