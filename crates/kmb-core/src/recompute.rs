use crate::document::DocumentData;
use crate::feature::{EvalCtx, InputRef, Outputs};
use crate::{FeatureId, Kernel, ParamId, Quantity, Registry, ShapeHandle, Value};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ParamStatus {
    Ok {
        value: Quantity,
    },
    /// 計算できた文字のパラメータ([`crate::ParamUnit::Text`])の値
    Text {
        value: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FeatureStatus {
    Ok {
        outputs: Outputs,
        reused: bool,
    },
    Error {
        message: String,
    },
    /// 依存先が失敗・抑制されているため評価しなかった
    Blocked {
        by: String,
        /// 止めた上流の機能(パラメータが原因なら `None`)
        #[serde(skip_serializing_if = "Option::is_none")]
        feature: Option<FeatureId>,
    },
    Suppressed,
    /// ロールバック位置より後ろにある
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParamResult {
    pub id: ParamId,
    pub name: String,
    #[serde(flatten)]
    pub status: ParamStatus,
}

/// モデルのパラメータ([`crate::Feature::model_params`])の計算の結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelParamResult {
    /// 持ち主の機能
    pub feature: FeatureId,
    /// 機能の中で変わらない鍵([`crate::ModelParam::key`])
    pub key: String,
    pub name: String,
    pub expr: String,
    pub label: String,
    #[serde(flatten)]
    pub status: ParamStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeatureResult {
    pub id: FeatureId,
    pub name: String,
    #[serde(flatten)]
    pub status: FeatureStatus,
}

/// 再計算の結果。キャッシュの使用状況(`reused`)を除けば、同じ文書からは毎回同じ結果になる。
#[derive(Debug, Clone, Serialize, Default)]
pub struct RecomputeReport {
    pub params: Vec<ParamResult>,
    /// 機能が持つ名前の付いた式(スケッチの寸法の `d1` など)。名前で参照できる
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub model_params: Vec<ModelParamResult>,
    pub features: Vec<FeatureResult>,
    pub evaluated: usize,
    pub reused: usize,
    /// 再計算ごとに違う番号(写しは同じ番号)。結果から作る重いもの(加工の経路の読み直しなど)を、
    /// 番号が同じ間だけ使い回すのに使う。比べる時は見ない
    #[serde(skip)]
    stamp: u64,
    /// 機能ごとの計算にかかった時間(ミリ秒)。使い回した機能は、その結果を計算した時の時間。
    /// 毎回変わるので、比べる時は見ない
    #[serde(skip)]
    times: HashMap<FeatureId, f64>,
    /// ボディの系譜の表(機能が読むボディをどの形に解いたか)。文書から決まるので、比べる時は見ない
    #[serde(skip)]
    pub bodies: std::sync::Arc<crate::BodyTable>,
    /// そろえた面の名前と前の決まりの名前の表([`crate::FaceAliases`])。前の文書の参照を書き換えるのに
    /// 使う。形から決まるので、比べる時は見ない
    #[serde(skip)]
    pub face_aliases: std::sync::Arc<crate::FaceAliases>,
}

impl PartialEq for RecomputeReport {
    fn eq(&self, o: &Self) -> bool {
        self.params == o.params
            && self.model_params == o.model_params
            && self.features == o.features
            && self.evaluated == o.evaluated
            && self.reused == o.reused
    }
}

/// 次の再計算の番号(0 は空の結果に残す)。
fn next_stamp() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl RecomputeReport {
    /// 再計算ごとに違う番号(写しは同じ番号、まだ計算していない空の結果は 0)。
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    pub fn error_count(&self) -> usize {
        self.params
            .iter()
            .map(|p| &p.status)
            .chain(self.model_params.iter().map(|p| &p.status))
            .filter(|s| matches!(s, ParamStatus::Error { .. }))
            .count()
            + self
                .features
                .iter()
                .filter(|f| matches!(f.status, FeatureStatus::Error { .. }))
                .count()
    }

    /// 名前のパラメータ(利用者のものか、機能が持つモデルのもの)の計算の結果。
    pub fn param(&self, name: &str) -> Option<&ParamStatus> {
        self.params
            .iter()
            .find(|p| p.name == name)
            .map(|p| &p.status)
            .or_else(|| {
                self.model_params
                    .iter()
                    .find(|p| p.name == name)
                    .map(|p| &p.status)
            })
    }

    /// 計算できたパラメータの値(利用者のものとモデルのもの。名前 → 値)。
    pub fn param_values(&self) -> HashMap<&str, Quantity> {
        let user = self.params.iter().map(|p| (p.name.as_str(), &p.status));
        let model = self
            .model_params
            .iter()
            .map(|p| (p.name.as_str(), &p.status));
        let mut out = HashMap::new();
        // 名前が重なれば利用者のパラメータを使う(モデルの方は誤りになっている)
        for (n, st) in user.chain(model) {
            if let ParamStatus::Ok { value } = st {
                out.entry(n).or_insert(*value);
            }
        }
        out
    }

    /// 計算できた文字のパラメータの値(名前 → 文字)。
    pub fn text_values(&self) -> HashMap<&str, &str> {
        let mut out = HashMap::new();
        for p in &self.params {
            if let ParamStatus::Text { value } = &p.status {
                out.entry(p.name.as_str()).or_insert(value.as_str());
            }
        }
        out
    }

    pub fn feature(&self, id: FeatureId) -> Option<&FeatureStatus> {
        self.features.iter().find(|f| f.id == id).map(|f| &f.status)
    }

    /// 機能 `id` の計算にかかった時間(ミリ秒)。計算しなかった機能(抑制・止まった物など)は `None`。
    /// 前の結果を使い回した機能は、その結果を計算した時の時間(タイムラインで重い機能を探すのに使う)。
    pub fn time_ms(&self, id: FeatureId) -> Option<f64> {
        self.times.get(&id).copied()
    }
}

/// 機能ごとに、入力のハッシュと出力を覚えておく。新しい順に [`Cache::KEEP`] 個まで持ち、
/// 値を変えた直後の取り消し(前の入力に戻す)でも計算し直さない。
///
/// 再計算を別のスレッドで行う時は [`crate::Document::take_cache`] で取り出し、
/// [`recompute_detached`] に渡した後で [`crate::Document::put_cache`] で戻す。
#[derive(Default)]
pub struct Cache {
    /// (入力のハッシュ、出力、計算にかかった時間 ms、そろえた面の名前と前の決まりの名前)
    entries: HashMap<FeatureId, Vec<(u64, Outputs, f64, Aliases)>>,
    /// 結果のほかに、カーネルに残させる形状(画面がまだ出している前の形。M27)
    held: HashSet<ShapeHandle>,
    /// 前の再計算が出現の最後の形として置いた形(結果に入るがキャッシュに無い)
    placed: HashSet<ShapeHandle>,
}

impl Cache {
    /// 機能ごとに覚えておく結果の数(今のものと、1 つ前のもの)。
    pub const KEEP: usize = 2;

    /// 入力のハッシュが `key` の結果。見つかったものは一番新しいものとして並べ直す。
    /// 計算にかかった時間(ms)も返す。
    fn get(&mut self, id: FeatureId, key: u64) -> Option<(&Outputs, f64, &Aliases)> {
        let list = self.entries.get_mut(&id)?;
        let i = list.iter().position(|(k, ..)| *k == key)?;
        let hit = list.remove(i);
        list.insert(0, hit);
        Some((&list[0].1, list[0].2, &list[0].3))
    }

    fn insert(&mut self, id: FeatureId, key: u64, out: Outputs, ms: f64, aliases: Aliases) {
        let list = self.entries.entry(id).or_default();
        list.retain(|(k, ..)| *k != key);
        list.insert(0, (key, out, ms, aliases));
        list.truncate(Self::KEEP);
    }

    /// キャッシュが持っている形状(と [`Cache::hold`] で残させた形状)。これ以外はカーネルが捨ててよい。
    pub(crate) fn shapes(&self) -> HashSet<ShapeHandle> {
        self.entries
            .values()
            .flatten()
            .flat_map(|(_, out, ..)| out.values())
            .filter_map(|v| match v {
                Value::Shape(h) => Some(*h),
                _ => None,
            })
            .chain(self.held.iter().copied())
            .chain(self.placed.iter().copied())
            .collect()
    }

    /// 次の再計算で、結果に無くてもカーネルに残させる形状を決める(前のものと置き換える)。
    /// 計算に失敗した間も、画面が前に計算できた形を出し続けるのに使う(M27)。
    pub fn hold(&mut self, shapes: impl IntoIterator<Item = ShapeHandle>) {
        self.held = shapes.into_iter().collect();
    }
}

/// 文書から切り離して再計算する。カーネルはキャッシュにない形状を捨てる。
pub fn recompute_detached(
    data: &DocumentData,
    registry: &Registry,
    kernel: &dyn Kernel,
    cache: &mut Cache,
) -> RecomputeReport {
    let never = AtomicBool::new(false);
    recompute_cancellable(data, registry, kernel, cache, &never)
        .expect("やめる印を立てないので、必ず最後まで計算する")
}

/// [`recompute_detached`] と同じだが、`cancel` が立ったら次の機能の前でやめて `None` を返す
/// (取り消しを続けて押した時など、もう要らない計算を早く終えるため)。評価し終えた機能の結果は
/// キャッシュに残るので、次の再計算で使い回せる。やめた時はカーネルの形状を捨てない
/// (画面がまだ前の結果の形状を使っている)。
pub fn recompute_cancellable(
    data: &DocumentData,
    registry: &Registry,
    kernel: &dyn Kernel,
    cache: &mut Cache,
    cancel: &AtomicBool,
) -> Option<RecomputeReport> {
    recompute_watched(data, registry, kernel, cache, cancel, &AtomicUsize::new(0))
}

/// [`recompute_cancellable`] と同じだが、機能を 1 つ通るたびに `done` を 1 つ進める
/// (大きな文書を開く間に「何個のうち何個」を出すため。U8)。始めに 0 にする。
pub fn recompute_watched(
    data: &DocumentData,
    registry: &Registry,
    kernel: &dyn Kernel,
    cache: &mut Cache,
    cancel: &AtomicBool,
    done: &AtomicUsize,
) -> Option<RecomputeReport> {
    done.store(0, Ordering::Relaxed);
    let report = run(data, registry, kernel, cache, cancel, done)?;
    kernel.retain(&cache.shapes());
    Some(report)
}

/// 入れ子の計算([`EvalCtx::recompute_nested`])の深さの上限。写しの中の写しは有限なので輪には
/// ならないが、深すぎる入れ子で止まらないように。
pub const NESTED_DEPTH: usize = 8;

thread_local! {
    static DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// 入れ子の深さを 1 つ進め、抜ける時(panic でも)に戻す。
struct Nest;

impl Drop for Nest {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get() - 1));
    }
}

/// 機能の計算の中(や操作の中)で、別の文書を計算する。キャッシュは使わず(外の機能のキャッシュが、
/// 写しが同じ間は結果を使い回す)、カーネルの形も捨てない(作った形は、外の文書の次の再計算が
/// 結果に無い物として捨てる)。入れ子は [`NESTED_DEPTH`] 段まで。
pub fn recompute_nested(
    data: &DocumentData,
    registry: &Registry,
    kernel: &dyn Kernel,
) -> Result<RecomputeReport, crate::EvalError> {
    let depth = DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get()
    });
    let _nest = Nest;
    if depth > NESTED_DEPTH {
        return Err(crate::EvalError::Other(format!(
            "組み込んだ部品の入れ子が {NESTED_DEPTH} 段を超えました"
        )));
    }
    let never = AtomicBool::new(false);
    let t = Instant::now();
    let report = run(
        data,
        registry,
        kernel,
        &mut Cache::default(),
        &never,
        &AtomicUsize::new(0),
    )
    .expect("やめる印を立てないので、必ず最後まで計算する");
    log::info!(
        "組み込んだ文書を計算した(入れ子 {depth} 段目、機能 {} 個、誤り {}、{:.1} ms)",
        data.features.len(),
        report.error_count(),
        t.elapsed().as_secs_f64() * 1e3
    );
    Ok(report)
}

pub(crate) fn run(
    data: &DocumentData,
    registry: &Registry,
    kernel: &dyn Kernel,
    cache: &mut Cache,
    cancel: &AtomicBool,
    done: &AtomicUsize,
) -> Option<RecomputeReport> {
    let started = Instant::now();
    let (params, model_params) = eval_params(data, &model_params(data, registry));
    for (name, st) in params
        .iter()
        .map(|p| (&p.name, &p.status))
        .chain(model_params.iter().map(|p| (&p.name, &p.status)))
    {
        if let ParamStatus::Error { message } = st {
            log::warn!("パラメータ {name} の誤り: {message}");
        }
    }
    let mut ok_params: HashMap<String, Quantity> = HashMap::new();
    for (name, st) in params
        .iter()
        .map(|p| (&p.name, &p.status))
        .chain(model_params.iter().map(|p| (&p.name, &p.status)))
    {
        if let ParamStatus::Ok { value } = st {
            ok_params.entry(name.clone()).or_insert(*value);
        }
    }
    let ok_texts: HashMap<String, String> = params
        .iter()
        .filter_map(|p| match &p.status {
            ParamStatus::Text { value } => Some((p.name.clone(), value.clone())),
            _ => None,
        })
        .collect();

    let position: HashMap<FeatureId, usize> = data
        .features
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id, i))
        .collect();
    let mut outputs: HashMap<FeatureId, Outputs> = HashMap::new();
    let mut features = Vec::with_capacity(data.features.len());
    let (mut evaluated, mut reused) = (0, 0);
    let mut times: HashMap<FeatureId, f64> = HashMap::new();
    let active = data.active_len();
    let bodies = crate::BodyTable::build(data, registry);
    let mut aliases = crate::FaceAliases::default();
    let mut placing = crate::occurrence::Placing::new(&bodies);

    // 別のスレッドで計算を待つ機能(互いに依存しない、形を作らない種類)。依存する機能か、
    // 形を作る機能が来たら、先に計算し終える(結果は機能の順に並べるので、順に計算した時と同じ)
    let mut pending: Vec<Job> = Vec::new();

    for (i, entry) in data.features.iter().enumerate() {
        done.store(i, Ordering::Relaxed);
        if cancel.load(Ordering::Relaxed) {
            log::info!(
                "再計算をやめた(新しい変更が来た): 機能 {i}/{} の前、評価 {evaluated}、{:.1} ms",
                data.features.len(),
                started.elapsed().as_secs_f64() * 1e3
            );
            return None;
        }
        let t = Instant::now();
        // 同時に計算できる種類なら、組み立てて入力を調べる(待っている機能を使うなら、先に済ませる)
        // (組み立てや入力で誤る・panic する時は、1 つずつの計算に任せて誤りを記録する)
        // (出現のある文書は、出現の値を渡すので 1 つずつ計算する)
        let concurrent = (i < active
            && !entry.suppressed
            && !placing.active()
            && registry.has_mark(&entry.type_id, crate::CONCURRENT))
        .then(|| {
            catch_unwind(AssertUnwindSafe(|| {
                registry
                    .instantiate(&entry.type_id, &entry.data)?
                    .ok()?
                    .inputs()
                    .ok()
                    .map(|v| {
                        v.iter()
                            .map(|i| bodies.resolve(entry.id, i))
                            .collect::<Vec<_>>()
                    })
            }))
            .ok()
            .flatten()
        })
        .flatten();
        let waits = |inputs: &[InputRef]| {
            inputs.iter().any(|input| {
                let f = match input {
                    InputRef::Output { feature, .. } => *feature,
                    InputRef::Param { .. } | InputRef::Units => return false,
                };
                pending.iter().any(|j| j.id == f)
            })
        };
        if concurrent.as_deref().is_none_or(waits) {
            run_jobs(
                &mut pending,
                data,
                registry,
                &bodies,
                kernel,
                &ok_params,
                &ok_texts,
                cache,
                &mut outputs,
                &mut features,
                &mut times,
                &mut aliases,
            );
        }
        if let Some(inputs) = concurrent {
            let mut hasher = DefaultHasher::new();
            entry.type_id.hash(&mut hasher);
            entry.data.to_string().hash(&mut hasher);
            let checked = inputs.iter().try_for_each(|input| {
                check_input(
                    input,
                    i,
                    &position,
                    &params,
                    &model_params,
                    &outputs,
                    data,
                    &mut hasher,
                )
            });
            let key = hasher.finish();
            let status = match checked {
                Err(st) => Some(st),
                Ok(()) => cache.get(entry.id, key).map(|(out, ms, pairs)| {
                    reused += 1;
                    times.insert(entry.id, ms);
                    aliases.extend(pairs);
                    FeatureStatus::Ok {
                        outputs: out.clone(),
                        reused: true,
                    }
                }),
            };
            match status {
                Some(status) => finish(entry, status, &t, &mut outputs, &mut features),
                None => {
                    evaluated += 1;
                    pending.push(Job {
                        index: i,
                        slot: features.len(),
                        id: entry.id,
                        key,
                    });
                    // 計算し終えたら置き換える
                    features.push(FeatureResult {
                        id: entry.id,
                        name: entry.name.clone(),
                        status: FeatureStatus::RolledBack,
                    });
                }
            }
            continue;
        }
        let status = if i >= active {
            FeatureStatus::RolledBack
        } else if entry.suppressed {
            if let Some(out) = pass_through(entry, i, registry, &bodies, &position, &outputs) {
                outputs.insert(entry.id, out);
            }
            FeatureStatus::Suppressed
        } else {
            let mut run_one = || -> FeatureStatus {
                let feature = match registry.instantiate(&entry.type_id, &entry.data) {
                    None => {
                        return error(format!("未登録の機能の種類です: {}", entry.type_id));
                    }
                    Some(Err(e)) => return error(e.to_string()),
                    Some(Ok(f)) => f,
                };
                let inputs = match feature.inputs() {
                    Ok(v) => v,
                    Err(e) => return error(e.to_string()),
                };
                let own = feature.instance_of();
                let placed = match placing.inputs(
                    kernel,
                    data,
                    &bodies,
                    entry.id,
                    i,
                    &inputs,
                    own.as_ref(),
                    &outputs,
                ) {
                    Ok(p) => p,
                    Err(st) => return st,
                };
                let source = own.and_then(|o| match bodies.resolve(entry.id, &o) {
                    InputRef::Output { feature, slot } => Some((feature, slot)),
                    _ => None,
                });
                let mut after = |out: &Outputs| {
                    placing.after(
                        kernel,
                        data,
                        registry,
                        &bodies,
                        entry.id,
                        &placed,
                        source.clone(),
                        &outputs,
                        out,
                    )
                };
                let mut hasher = DefaultHasher::new();
                entry.type_id.hash(&mut hasher);
                entry.data.to_string().hash(&mut hasher);
                for input in &inputs {
                    if let Err(st) = check_input(
                        &bodies.resolve(entry.id, input),
                        i,
                        &position,
                        &params,
                        &model_params,
                        &outputs,
                        data,
                        &mut hasher,
                    ) {
                        return st;
                    }
                }
                crate::occurrence::Placing::hash(kernel, &placed, &mut hasher);
                let key = hasher.finish();
                if let Some((out, ms, pairs)) = cache.get(entry.id, key) {
                    reused += 1;
                    times.insert(entry.id, ms);
                    aliases.extend(pairs);
                    if let Err(m) = after(out) {
                        return error(m);
                    }
                    return FeatureStatus::Ok {
                        outputs: out.clone(),
                        reused: true,
                    };
                }
                let mut ctx = EvalCtx {
                    id: entry.id,
                    declared: &inputs,
                    params: &ok_params,
                    texts: &ok_texts,
                    outputs: &outputs,
                    redirect: bodies.redirects(entry.id),
                    placed: &placed,
                    kernel,
                    registry,
                    data,
                };
                evaluated += 1;
                let result = feature.evaluate(&mut ctx);
                let ms = t.elapsed().as_secs_f64() * 1e3;
                times.insert(entry.id, ms);
                match result {
                    Ok(mut out) => {
                        let pairs = shorten_names(kernel, entry.id, &mut out, &aliases);
                        aliases.extend(&pairs);
                        cache.insert(entry.id, key, out.clone(), ms, pairs);
                        if let Err(m) = after(&out) {
                            return error(m);
                        }
                        FeatureStatus::Ok {
                            outputs: out,
                            reused: false,
                        }
                    }
                    Err(e) => error(e.to_string()),
                }
            };
            // カーネルや機能の panic で文書全体を止めない
            catch_unwind(AssertUnwindSafe(&mut run_one)).unwrap_or_else(|payload| {
                error(format!(
                    "評価中に panic しました: {}",
                    panic_message(&payload)
                ))
            })
        };
        finish(entry, status, &t, &mut outputs, &mut features);
    }
    run_jobs(
        &mut pending,
        data,
        registry,
        &bodies,
        kernel,
        &ok_params,
        &ok_texts,
        cache,
        &mut outputs,
        &mut features,
        &mut times,
        &mut aliases,
    );
    cache.entries.retain(|id, _| position.contains_key(id));
    cache.placed = placing.finish(kernel, data, &bodies, &mut outputs, &mut features);
    pin_warnings(data, &bodies, &mut features);

    let report = RecomputeReport {
        params,
        model_params,
        features,
        evaluated,
        reused,
        stamp: next_stamp(),
        times,
        bodies: std::sync::Arc::new(bodies),
        face_aliases: std::sync::Arc::new(aliases),
    };
    log::info!(
        "再計算: 機能 {} 個、評価 {}、再利用 {}、誤り {}、{:.1} ms",
        data.features.len(),
        evaluated,
        reused,
        report.error_count(),
        started.elapsed().as_secs_f64() * 1e3
    );
    Some(report)
}

/// 出力の名前の頭。`moved:<機能の番号>` の形は、その機能が出したボディを、ボディの名前(最後の
/// 機能)を変えずに動かした形(ジョイント)。再計算は、それより後の機能が読むその機能の `body` と、
/// 結果の並びのその機能の `body` を、この形に置き換える(見えるボディも動いた形になる)。
/// 1 つの機能が出せるボディは `body` の 1 つなので、いくつものボディを一緒に動かすのに使う。
pub const MOVED: &str = "moved:";

/// 機能 `id` のボディを動かした形を入れる出力の名前([`MOVED`])。
pub fn moved_slot(id: FeatureId) -> String {
    format!("{MOVED}{}", id.0)
}

/// 出力 `out` の [`MOVED`] の形で、動かしたボディの `body` を置き換える。
fn relocate(
    out: &Outputs,
    outputs: &mut HashMap<FeatureId, Outputs>,
    features: &mut [FeatureResult],
) {
    for (slot, v) in out {
        let Some(id) = slot
            .strip_prefix(MOVED)
            .and_then(|n| n.parse::<u64>().ok())
            .map(FeatureId)
        else {
            continue;
        };
        if !matches!(v, Value::Shape(_)) {
            continue;
        }
        if let Some(o) = outputs.get_mut(&id)
            && o.contains_key("body")
        {
            o.insert("body".into(), v.clone());
        }
        if let Some(r) = features.iter_mut().rev().find(|r| r.id == id)
            && let FeatureStatus::Ok { outputs: o, .. } = &mut r.status
            && o.contains_key("body")
        {
            o.insert("body".into(), v.clone());
        }
    }
}

/// 同時に計算する機能 1 つ([`crate::CONCURRENT`] の印の種類)。
struct Job {
    /// タイムラインの位置
    index: usize,
    /// 結果の並び(`features`)の中の位置
    slot: usize,
    id: FeatureId,
    /// 入力のハッシュ(キャッシュの鍵)
    key: u64,
}

/// そろえた面の名前と、前の決まり(そろえない)での名前の組。
type Aliases = Vec<(String, String)>;

/// 機能 `id` の結果の形の面の名前をそろえる([`crate::face_names`]。2 回以上割った面を
/// `元~f{id}#k` に)。そろえた名前と前の決まりの名前の組を返す。`aliases` はそれより前の機能の組。
fn shorten_names(
    kernel: &dyn Kernel,
    id: FeatureId,
    out: &mut Outputs,
    aliases: &crate::FaceAliases,
) -> Aliases {
    let tag = format!("f{}", id.0);
    let mut done: HashMap<ShapeHandle, ShapeHandle> = HashMap::new();
    let mut pairs = Vec::new();
    for v in out.values_mut() {
        let Value::Shape(h) = v else {
            continue;
        };
        if let Some(n) = done.get(h) {
            *h = *n;
            continue;
        }
        let Ok(names) = kernel.face_names(*h) else {
            continue;
        };
        let list: Vec<(String, String)> = crate::face_names::shorten(&names, &tag)
            .into_iter()
            .map(|(i, new)| (names[i].clone(), new))
            .collect();
        if list.is_empty() {
            done.insert(*h, *h);
            continue;
        }
        // 付け替えは名前で面を探すので、新しい名前が今ある名前と重なる時は仮の名前を挟む
        let renamed = if list.iter().any(|(_, to)| names.contains(to)) {
            let tmp: Vec<(String, String)> = list
                .iter()
                .enumerate()
                .map(|(i, (from, _))| (from.clone(), format!("{tag}\u{0}{i}")))
                .collect();
            let back: Vec<(String, String)> = tmp
                .iter()
                .zip(&list)
                .map(|((_, t), (_, to))| (t.clone(), to.clone()))
                .collect();
            kernel
                .rename_faces(*h, &tmp)
                .and_then(|m| kernel.rename_faces(m, &back))
        } else {
            kernel.rename_faces(*h, &list)
        };
        match renamed {
            Ok(n) => {
                pairs.extend(
                    list.iter()
                        .map(|(old, new)| (new.clone(), aliases.legacy(old))),
                );
                done.insert(*h, n);
                *h = n;
            }
            Err(e) => log::warn!("機能 {tag} の面の名前をそろえられなかった: {e}"),
        }
    }
    pairs
}

/// 機能の結果を記録する(ログ、後ろの機能が読む出力、結果の並び)。
fn finish(
    entry: &crate::FeatureEntry,
    status: FeatureStatus,
    t: &Instant,
    outputs: &mut HashMap<FeatureId, Outputs>,
    features: &mut Vec<FeatureResult>,
) {
    log_status(entry, &status, t.elapsed().as_secs_f64() * 1e3);
    // 計算しなかった機能(ロールバックの棒の後ろ、抑制、誤り、止まった物)も前の結果を残す。
    // 棒を戻す・抑制を解く・誤りになった変更を取り消す時に計算し直さないため。
    // 入力が変わっていれば鍵が合わず使われない
    if let FeatureStatus::Ok { outputs: out, .. } = &status {
        outputs.insert(entry.id, out.clone());
        relocate(out, outputs, features);
    }
    features.push(FeatureResult {
        id: entry.id,
        name: entry.name.clone(),
        status,
    });
}

fn log_status(entry: &crate::FeatureEntry, status: &FeatureStatus, ms: f64) {
    match status {
        FeatureStatus::Ok { reused: false, .. } => {
            log::debug!("評価した: {} ({}) {ms:.1} ms", entry.name, entry.type_id)
        }
        FeatureStatus::Error { message } => {
            log::warn!("機能 {} ({}) の誤り: {message}", entry.name, entry.type_id)
        }
        FeatureStatus::Blocked { by, .. } => {
            log::debug!("機能 {} は止まった: {by}", entry.name)
        }
        _ => {}
    }
}

/// 待っている機能を計算する。2 つ以上なら別々のスレッドで同時に(CPU の数まで)。
/// 結果は機能の順に記録する。
#[allow(clippy::too_many_arguments)]
fn run_jobs(
    pending: &mut Vec<Job>,
    data: &DocumentData,
    registry: &Registry,
    bodies: &crate::BodyTable,
    kernel: &dyn Kernel,
    params: &HashMap<String, Quantity>,
    texts: &HashMap<String, String>,
    cache: &mut Cache,
    outputs: &mut HashMap<FeatureId, Outputs>,
    features: &mut [FeatureResult],
    times: &mut HashMap<FeatureId, f64>,
    aliases: &mut crate::FaceAliases,
) {
    if pending.is_empty() {
        return;
    }
    let jobs = std::mem::take(pending);
    let ready: &HashMap<FeatureId, Outputs> = outputs;
    let one = |job: &Job| -> (Result<Outputs, String>, f64) {
        let t = Instant::now();
        let entry = &data.features[job.index];
        let run = || -> Result<Outputs, String> {
            let feature = match registry.instantiate(&entry.type_id, &entry.data) {
                None => return Err(format!("未登録の機能の種類です: {}", entry.type_id)),
                Some(Err(e)) => return Err(e.to_string()),
                Some(Ok(f)) => f,
            };
            let declared = feature.inputs().map_err(|e| e.to_string())?;
            let mut ctx = EvalCtx {
                id: job.id,
                declared: &declared,
                params,
                texts,
                outputs: ready,
                redirect: bodies.redirects(job.id),
                placed: &Default::default(),
                kernel,
                registry,
                data,
            };
            feature.evaluate(&mut ctx).map_err(|e| e.to_string())
        };
        // カーネルや機能の panic で文書全体を止めない
        let result = catch_unwind(AssertUnwindSafe(run)).unwrap_or_else(|payload| {
            Err(format!(
                "評価中に panic しました: {}",
                panic_message(&payload)
            ))
        });
        (result, t.elapsed().as_secs_f64() * 1e3)
    };
    let started = Instant::now();
    let results: Vec<(Result<Outputs, String>, f64)> = if jobs.len() == 1 {
        vec![one(&jobs[0])]
    } else {
        par_map(&jobs, one)
    };
    if jobs.len() > 1 {
        log::debug!(
            "同時に計算した: {} 個、{:.1} ms(1 つずつなら {:.1} ms)",
            jobs.len(),
            started.elapsed().as_secs_f64() * 1e3,
            results.iter().map(|r| r.1).sum::<f64>()
        );
    }
    for (job, (result, ms)) in jobs.iter().zip(results) {
        let entry = &data.features[job.index];
        times.insert(job.id, ms);
        let status = match result {
            Ok(mut out) => {
                let pairs = shorten_names(kernel, job.id, &mut out, aliases);
                aliases.extend(&pairs);
                cache.insert(job.id, job.key, out.clone(), ms, pairs);
                FeatureStatus::Ok {
                    outputs: out,
                    reused: false,
                }
            }
            Err(e) => error(e),
        };
        log_status(entry, &status, ms);
        if let FeatureStatus::Ok { outputs: out, .. } = &status {
            outputs.insert(job.id, out.clone());
            relocate(out, outputs, features);
        }
        features[job.slot].status = status;
    }
}

/// 同時に計算する機能の数の上限。既定は CPU の数。環境変数 `JC_RECOMPUTE_THREADS` で決められる
/// (1 なら 1 つずつ。速さを比べる時や、調べる時)。
fn threads() -> usize {
    std::env::var("JC_RECOMPUTE_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
}

/// `items` のそれぞれに `f` を、[`threads`] までのスレッドで同時に当てる。結果は `items` の順。
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = threads().min(items.len());
    let next = AtomicUsize::new(0);
    let mut all: Vec<(usize, R)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(k) else { break };
                        out.push((k, f(item)));
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    all.sort_by_key(|(k, _)| *k);
    all.into_iter().map(|(_, r)| r).collect()
}

/// 抑制した機能が後ろへ渡す出力。元の出力がないものは渡さない。
fn pass_through(
    entry: &crate::FeatureEntry,
    index: usize,
    registry: &Registry,
    bodies: &crate::BodyTable,
    position: &HashMap<FeatureId, usize>,
    outputs: &HashMap<FeatureId, Outputs>,
) -> Option<Outputs> {
    let Some(Ok(feature)) = registry.instantiate(&entry.type_id, &entry.data) else {
        return None;
    };
    let out: Outputs = feature
        .pass_through()
        .into_iter()
        .filter_map(|(name, from)| {
            let InputRef::Output { feature, slot } = bodies.resolve(entry.id, &from) else {
                return None;
            };
            if position.get(&feature).is_none_or(|&p| p >= index) {
                return None;
            }
            outputs.get(&feature)?.get(&slot).map(|v| (name, v.clone()))
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

/// 固定の印が付き、そのボディの最新の形と違う形を読んでいる機能に、警告を足す。
fn pin_warnings(data: &DocumentData, bodies: &crate::BodyTable, features: &mut [FeatureResult]) {
    let name = |id: FeatureId| data.feature(id).map_or(id.to_string(), |f| f.name.clone());
    for r in features {
        let (Some(list), FeatureStatus::Ok { outputs, .. }) =
            (bodies.stale().get(&r.id), &mut r.status)
        else {
            continue;
        };
        let warnings = list
            .iter()
            .map(|((read, _), (tip, _))| {
                format!(
                    "この機能は前の版の読み方で、機能「{}」の時点のボディを読んでいます(後から足した機能「{}」が入っていません)。「最新の形を読む」で付いて来ます",
                    name(*read),
                    name(*tip)
                )
            })
            .collect();
        crate::add_warnings(outputs, warnings);
    }
}

fn error(message: String) -> FeatureStatus {
    FeatureStatus::Error { message }
}

/// 入力が使えるかを確かめ、使えるならその値をハッシュに加える。
#[allow(clippy::too_many_arguments)]
fn check_input(
    input: &InputRef,
    index: usize,
    position: &HashMap<FeatureId, usize>,
    params: &[ParamResult],
    model_params: &[ModelParamResult],
    outputs: &HashMap<FeatureId, Outputs>,
    data: &DocumentData,
    hasher: &mut DefaultHasher,
) -> Result<(), FeatureStatus> {
    let upstream = |feature: FeatureId| -> Result<&Outputs, FeatureStatus> {
        match position.get(&feature) {
            None => Err(error(format!("参照先の機能 {feature} がありません"))),
            Some(&p) if p >= index => Err(error(format!(
                "参照先の機能 {feature} がタイムラインで後ろにあります"
            ))),
            Some(_) => outputs.get(&feature).ok_or_else(|| {
                let name = data
                    .feature(feature)
                    .map_or(String::new(), |f| f.name.clone());
                FeatureStatus::Blocked {
                    by: format!("{feature} {name}"),
                    feature: Some(feature),
                }
            }),
        }
    };
    match input {
        InputRef::Param { name } => {
            let status = params
                .iter()
                .find(|p| &p.name == name)
                .map(|p| &p.status)
                .or_else(|| {
                    model_params
                        .iter()
                        .find(|p| &p.name == name)
                        .map(|p| &p.status)
                });
            match status {
                None => Err(error(format!("未定義のパラメータです: {name}"))),
                Some(ParamStatus::Error { .. }) => Err(FeatureStatus::Blocked {
                    by: format!("パラメータ {name}"),
                    feature: None,
                }),
                Some(ParamStatus::Ok { value }) => {
                    name.hash(hasher);
                    hash_json(value, hasher);
                    Ok(())
                }
                Some(ParamStatus::Text { value }) => {
                    name.hash(hasher);
                    value.hash(hasher);
                    Ok(())
                }
            }
        }
        InputRef::Units => {
            hash_json(&data.properties.units, hasher);
            Ok(())
        }
        InputRef::Output { feature, slot } => {
            let out = upstream(*feature)?;
            let v = out
                .get(slot)
                .ok_or_else(|| error(format!("{feature} に出力 {slot} がありません")))?;
            hash_json(v, hasher);
            Ok(())
        }
    }
}

fn hash_json<T: Serialize>(v: &T, hasher: &mut DefaultHasher) {
    serde_json::to_string(v)
        .expect("出力は JSON にできる")
        .hash(hasher);
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "(内容不明)".to_string()
    }
}

/// 機能が持つモデルのパラメータ(タイムラインの順。組み立てられない機能は飛ばす)。
/// 抑制した機能やロールバックより後ろの機能のものも含む。
pub fn model_params(
    data: &DocumentData,
    registry: &Registry,
) -> Vec<(FeatureId, crate::ModelParam)> {
    data.features
        .iter()
        .filter_map(|f| match registry.instantiate(&f.type_id, &f.data) {
            Some(Ok(feature)) => Some((f.id, feature.model_params())),
            _ => None,
        })
        .flat_map(|(id, ps)| ps.into_iter().map(move |p| (id, p)))
        .collect()
}

/// パラメータ(利用者のものと、機能が持つモデルのもの)を、1 つの名前の空間で依存順に
/// 評価する。循環しているもの、名前が重なったもの(後の方)は誤りにする。文字のパラメータ
/// ([`crate::ParamUnit::Text`])は差し込みの文字として、数は文書の長さの単位で文字にする。
fn eval_params(
    data: &DocumentData,
    model: &[(FeatureId, crate::ModelParam)],
) -> (Vec<ParamResult>, Vec<ModelParamResult>) {
    // 利用者のパラメータが先、モデルのパラメータが後。利用者のものは、数だけの式に単位を補う
    let completed: Vec<String> = data.params.iter().map(|p| p.completed_expr()).collect();
    let entries: Vec<Entry> = data
        .params
        .iter()
        .zip(&completed)
        .map(|(p, e)| Entry {
            name: &p.name,
            src: e,
            text: p.unit == crate::ParamUnit::Text,
        })
        .chain(model.iter().map(|(_, p)| Entry {
            name: &p.name,
            src: &p.expr,
            text: false,
        }))
        .collect();
    let mut status = eval_entries(&entries, data.properties.units).into_iter();
    let user = data
        .params
        .iter()
        .zip(status.by_ref())
        .map(|(p, s)| ParamResult {
            id: p.id,
            name: p.name.clone(),
            // 単位の列と式の次元が合わなければ誤り
            status: match s {
                ParamStatus::Ok { value } => match p.unit.check(value.dim) {
                    Ok(()) => ParamStatus::Ok { value },
                    Err(message) => ParamStatus::Error { message },
                },
                e => e,
            },
        })
        .collect();
    let model = model
        .iter()
        .zip(status)
        .map(|((f, p), s)| ModelParamResult {
            feature: *f,
            key: p.key.clone(),
            name: p.name.clone(),
            expr: p.expr.clone(),
            label: p.label.clone(),
            status: s,
        })
        .collect();
    (user, model)
}

/// 名前と式の並びを、1 つの名前の空間で依存順に評価する(パラメータの計算の中身)。
/// 循環しているもの、名前が前のものと重なったものは誤りにする。画面が、直したスケッチの
/// 寸法が循環しないかを置く前に確かめるのにも使う。
pub fn eval_exprs(exprs: &[(&str, &str)]) -> Vec<ParamStatus> {
    let entries: Vec<Entry> = exprs
        .iter()
        .map(|(name, src)| Entry {
            name,
            src,
            text: false,
        })
        .collect();
    eval_entries(&entries, kmb_expr::LengthUnit::Mm)
}

/// 計算するパラメータ 1 つ。`text` なら `src` は差し込みの文字。
struct Entry<'a> {
    name: &'a str,
    src: &'a str,
    text: bool,
}

/// 式か差し込みの文字を読んだもの。
enum Parsed {
    Num(kmb_expr::Expr),
    Text(kmb_expr::Template),
}

impl Parsed {
    fn references(&self) -> Vec<String> {
        match self {
            Parsed::Num(e) => e.references(),
            Parsed::Text(t) => t.references(),
        }
    }
}

fn eval_entries(exprs: &[Entry], unit: kmb_expr::LengthUnit) -> Vec<ParamStatus> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Visiting,
        Done,
    }
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut status: Vec<Option<ParamStatus>> = vec![None; exprs.len()];
    let mut marks = vec![Mark::New; exprs.len()];
    for (i, e) in exprs.iter().enumerate() {
        if index.contains_key(e.name) {
            status[i] = Some(ParamStatus::Error {
                message: format!("同じ名前のパラメータがあります: {}", e.name),
            });
            marks[i] = Mark::Done;
        } else {
            index.insert(e.name, i);
        }
    }

    struct Walk<'a, 'b> {
        exprs: &'b [Entry<'a>],
        index: &'b HashMap<&'a str, usize>,
        marks: &'b mut [Mark],
        status: &'b mut [Option<ParamStatus>],
        unit: kmb_expr::LengthUnit,
    }

    fn visit(i: usize, w: &mut Walk) {
        if w.marks[i] == Mark::Done {
            return;
        }
        if w.marks[i] == Mark::Visiting {
            w.status[i] = Some(ParamStatus::Error {
                message: "参照が循環しています".into(),
            });
            return;
        }
        w.marks[i] = Mark::Visiting;
        let entry = &w.exprs[i];
        let parsed = if entry.text {
            kmb_expr::Template::parse(entry.src).map(Parsed::Text)
        } else {
            kmb_expr::parse(entry.src).map(Parsed::Num)
        };
        let result = match parsed {
            Err(e) => ParamStatus::Error {
                message: e.to_string(),
            },
            Ok(parsed) => {
                let mut failed = None;
                for name in parsed.references() {
                    match w.index.get(name.as_str()) {
                        None => {
                            failed = Some(format!("未定義の名前: {name}"));
                            break;
                        }
                        Some(&j) => {
                            // 数の式は文字のパラメータを読めない(差し込みの文字だけが読む)
                            if w.exprs[j].text && matches!(parsed, Parsed::Num(_)) {
                                failed = Some(kmb_expr::ExprError::TextParam(name).to_string());
                                break;
                            }
                            visit(j, w);
                            if w.marks[j] == Mark::Visiting {
                                failed = Some("参照が循環しています".into());
                                break;
                            }
                            if let Some(ParamStatus::Error { .. }) = w.status[j] {
                                failed = Some(format!("依存先 {name} が誤りです"));
                                break;
                            }
                        }
                    }
                }
                match failed {
                    Some(message) => ParamStatus::Error { message },
                    None => {
                        let status = &*w.status;
                        let index = w.index;
                        let value = |n: &str| index.get(n).and_then(|&j| status[j].as_ref());
                        let evaluated = match &parsed {
                            Parsed::Num(e) => e
                                .eval(&|n| match value(n) {
                                    Some(ParamStatus::Ok { value }) => Some(*value),
                                    _ => None,
                                })
                                .map(|value| ParamStatus::Ok { value }),
                            Parsed::Text(t) => t
                                .render(
                                    &|n| match value(n) {
                                        Some(ParamStatus::Ok { value }) => {
                                            Some(kmb_expr::Value::Num(*value))
                                        }
                                        Some(ParamStatus::Text { value }) => {
                                            Some(kmb_expr::Value::Text(value.clone()))
                                        }
                                        _ => None,
                                    },
                                    w.unit,
                                )
                                .map(|value| ParamStatus::Text { value }),
                        };
                        evaluated.unwrap_or_else(|e| ParamStatus::Error {
                            message: e.to_string(),
                        })
                    }
                }
            }
        };
        w.marks[i] = Mark::Done;
        // 循環の検出で先に誤りが入っていれば、それを残す
        if w.status[i].is_none() {
            w.status[i] = Some(result);
        }
    }

    let mut walk = Walk {
        exprs,
        index: &index,
        marks: &mut marks,
        status: &mut status,
        unit,
    };
    for i in 0..exprs.len() {
        visit(i, &mut walk);
    }
    status
        .into_iter()
        .map(|s| s.expect("すべてのパラメータを訪問した"))
        .collect()
}

/// 最後まで残ったボディ。後の機能が `body` を読んで自分も `body` を出す
/// (結合や切り取りに使われた)ボディは除く。表示や書き出しに使う。
pub fn visible_bodies(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
) -> Vec<(FeatureId, ShapeHandle)> {
    visible_bodies_before(data, registry, report, None)
}

/// 機能 `until` の手前(タイムラインで前)までで残ったボディ。`None` なら最後まで。
/// スケッチの編集中に、それより前にできた形を下に描くのに使う。
pub fn visible_bodies_before(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    until: Option<FeatureId>,
) -> Vec<(FeatureId, ShapeHandle)> {
    let mut bodies = Vec::new();
    let mut consumed: HashSet<FeatureId> = HashSet::new();
    // 抑制した機能が後ろへそのまま渡すボディの元(抑制した機能 → 元のボディの機能)。
    // 後ろの機能がそれを読めば元のボディを使い切ったことにし、読まなければ元のボディが残る。
    // (渡した時点で使い切ったことにすると、最後の機能を抑制した時にボディが消える)
    let mut passed: HashMap<FeatureId, FeatureId> = HashMap::new();
    let origin = |passed: &HashMap<FeatureId, FeatureId>, id: FeatureId| {
        passed.get(&id).copied().unwrap_or(id)
    };
    for r in report
        .features
        .iter()
        .take_while(|r| until.is_none_or(|u| r.id != u))
    {
        if r.status == FeatureStatus::Suppressed {
            if let Some(entry) = data.feature(r.id)
                && let Some(Ok(f)) = registry.instantiate(&entry.type_id, &entry.data)
            {
                for (out, from) in f.pass_through() {
                    if let InputRef::Output { feature, slot } = report.bodies.resolve(r.id, &from)
                        && out == "body"
                        && slot == "body"
                    {
                        passed.insert(r.id, origin(&passed, feature));
                    }
                }
            }
            continue;
        }
        let FeatureStatus::Ok { outputs, .. } = &r.status else {
            continue;
        };
        let Some(Value::Shape(h)) = outputs.get("body") else {
            continue;
        };
        bodies.push((r.id, *h));
        let Some(entry) = data.feature(r.id) else {
            continue;
        };
        if let Some(Ok(f)) = registry.instantiate(&entry.type_id, &entry.data)
            && let Ok(inputs) = f.inputs()
        {
            let keeps = f.keeps();
            // 出現は元を読むだけ(元は見えるボディに残る)
            let own = f.instance_of();
            for i in inputs {
                if own.as_ref() == Some(&i) {
                    continue;
                }
                if let InputRef::Output { feature, slot } = report.bodies.resolve(r.id, &i)
                    && slot == "body"
                    && !keeps.contains(&feature)
                    && !keeps.contains(&i_feature(&i))
                {
                    consumed.insert(origin(&passed, feature));
                }
            }
        }
    }
    bodies.retain(|(id, _)| !consumed.contains(id));
    bodies
}

/// ボディを最初に作った機能(ボディの名前の鍵)。機能が読む `body` を前へたどる(元を残す機能の
/// `keeps` に当たったら、そこで止める: 移動/コピーの写しは別のボディ)。
pub fn body_root(data: &DocumentData, registry: &Registry, id: FeatureId) -> FeatureId {
    root_in(&crate::BodyTable::build(data, registry), data, registry, id)
}

/// [`body_root`] を、作った表 `bodies` で求める。
fn root_in(
    bodies: &crate::BodyTable,
    data: &DocumentData,
    registry: &Registry,
    mut id: FeatureId,
) -> FeatureId {
    for _ in 0..data.features.len() {
        let Some(f) = data.feature(id) else { break };
        // 元を残す機能(移動/コピーのコピー)は、ここから別のボディ
        match reads_body(bodies, registry, f) {
            Some(u) => id = u,
            None => break,
        }
    }
    id
}

/// 機能 `id` が読む機能(読む順。読み込めない機能なら空)。読むボディは今のそのボディの最後の
/// 機能にする(読んだ後で同じボディに足した機能があれば、その後の形)。面のスケッチから足す機能
/// (穴・押し出し)が、スケッチの面のボディを対象にするのに使う。
pub fn reads_now(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    id: FeatureId,
) -> Vec<FeatureId> {
    let Some(f) = data.feature(id) else {
        return Vec::new();
    };
    let Some(Ok(feature)) = registry.instantiate(&f.type_id, &f.data) else {
        return Vec::new();
    };
    let visible = visible_bodies(data, registry, report);
    feature
        .inputs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i {
            InputRef::Output { feature, slot } if slot == "body" => {
                let root = body_root(data, registry, feature);
                let tip = visible
                    .iter()
                    .map(|b| b.0)
                    .find(|b| body_root(data, registry, *b) == root);
                Some(tip.unwrap_or(feature))
            }
            InputRef::Output { feature, .. } => Some(feature),
            _ => None,
        })
        .collect()
}

/// 見えているボディ `bodies`(最後の機能の ID)の名前: 付けた名前か、なければ「ボディN」。
/// N は文書でボディを作った機能([`body_roots`])のうち何番目か(ブラウザのボディの番号と同じ)。
/// 後でほかのボディに結合して見えなくなったボディも数えるので、ほかのボディを結合で使っても
/// 番号は変わらない。並びは `bodies` の順。
pub fn body_labels(data: &DocumentData, registry: &Registry, bodies: &[FeatureId]) -> Vec<String> {
    let table = crate::BodyTable::build(data, registry);
    let roots: Vec<FeatureId> = bodies
        .iter()
        .map(|b| root_in(&table, data, registry, *b))
        .collect();
    let all = body_roots(&table, data, registry, &roots);
    roots
        .iter()
        .map(|r| {
            data.body_names.get(r).cloned().unwrap_or_else(|| {
                let n = all.iter().position(|a| a == r).map_or(0, |i| i + 1);
                format!("ボディ{n}")
            })
        })
        .collect()
}

/// 文書でボディを作った機能(ボディの名前の鍵)のすべて、タイムラインの順: ソリッドの機能のうち
/// 読むボディが無い物(か、元を残す機能の写し)と、`extra`(見えているボディの元。ソリッドの区分で
/// ない機能が作ったボディも数える)。
fn body_roots(
    bodies: &crate::BodyTable,
    data: &DocumentData,
    registry: &Registry,
    extra: &[FeatureId],
) -> Vec<FeatureId> {
    let mut v: Vec<FeatureId> = data
        .features
        .iter()
        .filter(|f| registry.category(&f.type_id) == Some(crate::descriptor::category::SOLID))
        .filter(|f| reads_body(bodies, registry, f).is_none())
        .map(|f| f.id)
        .chain(extra.iter().copied())
        .collect();
    v.sort_by_key(|id| data.index_of(*id).unwrap_or(usize::MAX));
    v.dedup();
    v
}

/// 機能 `f` が読んで受け継ぐボディ(元を残す機能の写しなら無し)。読み込めなければ無し。
/// 読むボディは表 `bodies` で解いた形(その手前の最新の形)。
fn reads_body(
    bodies: &crate::BodyTable,
    registry: &Registry,
    f: &crate::FeatureEntry,
) -> Option<FeatureId> {
    let feature = registry.instantiate(&f.type_id, &f.data)?.ok()?;
    let raw = feature
        .inputs()
        .unwrap_or_default()
        .into_iter()
        .find(|i| matches!(i, InputRef::Output { slot, .. } if slot == "body"))?;
    // 元を残す機能の写しと、出現は新しいボディ
    if feature.keeps().contains(&i_feature(&raw)) || feature.instance_of().is_some() {
        return None;
    }
    Some(i_feature(&bodies.resolve(f.id, &raw)))
}

/// 入力の機能(パラメータ・単位なら番号 0。ボディの入力にだけ使う)。
fn i_feature(i: &InputRef) -> FeatureId {
    match i {
        InputRef::Output { feature, .. } => *feature,
        InputRef::Param { .. } | InputRef::Units => FeatureId(0),
    }
}

/// 機能 `from` が出したボディを、タイムラインの最後まで受け継いだ機能。後の機能が `body` を読んで
/// 自分も `body` を出すたび(抑制した機能はそのまま後ろへ渡すたび)に、その機能へ移る。
/// 画面はボディを最後の機能の ID で隠すので、途中の段階のボディが隠れているかを調べるのに使う。
pub fn body_heir(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    from: FeatureId,
) -> FeatureId {
    let mut cur = from;
    for r in report.features.iter().skip_while(|r| r.id != from).skip(1) {
        let Some(entry) = data.feature(r.id) else {
            continue;
        };
        let Some(Ok(f)) = registry.instantiate(&entry.type_id, &entry.data) else {
            continue;
        };
        let reads = |i: &InputRef, body_only: bool| matches!(report.bodies.resolve(r.id, i), InputRef::Output { feature, slot } if feature == cur && (!body_only || slot == "body"));
        let takes = match &r.status {
            _ if f.keeps().contains(&cur) => false,
            FeatureStatus::Suppressed => f.pass_through().iter().any(|(_, i)| reads(i, false)),
            FeatureStatus::Ok { outputs, .. }
                if matches!(outputs.get("body"), Some(Value::Shape(_))) =>
            {
                f.inputs()
                    .unwrap_or_default()
                    .iter()
                    .any(|i| reads(i, true))
            }
            _ => false,
        };
        if takes {
            cur = r.id;
        }
    }
    cur
}
