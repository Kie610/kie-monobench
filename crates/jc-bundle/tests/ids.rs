//! 組み立て全体の名前(機能の種類・操作・設定の ID)の検査。
//!
//! - ID が一意で、役割の印も 1 つの物にしか付かない。操作が後に続ける操作(`then`)・前に並ぶ
//!   操作(`before`)・段の帯の操作(`Stage.action`)が、実際にある操作を指す。
//! - 名前の持ち主の表 `xtask/names.txt`(ID → 名乗った crate)が、今の登録と合っている。
//!   `cargo xtask verify` の層の点検は、この表で「他の層の名前を書いていないか」を調べる
//!   (組み立てずに読むため、表をファイルに置く)。登録を変えたら
//!   `JC_BLESS=1 cargo test -p jc-bundle --test ids` で書き直す。
//!
//! 操作は選んでいる物で変わるので、見本の文書 3 つで、何も選ばない時と、機能を 1 つずつ選んだ時の
//! 操作を集める(選んだ面や辺で初めて出る操作は、ここでは集まらない)。見本の再計算が時間の大半
//! なので、1 つのテストで 1 回だけ再計算し、4 つの確かめで使い回す。

use jc_bundle::Sample;
use jc_core::{
    ActionRole, Context, Document, Extension, FeatureId, Module, Modules, RecomputeReport, Registry,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// 画面が 1 つを探す役割でなく、操作の振る舞いを示す印(いくつの操作に付いてもよい)。
/// 一覧は jc-interchange の `role::MARKERS`。
fn is_marker(r: &ActionRole) -> bool {
    jc_interchange::role::MARKERS.contains(r)
}

/// 全部の文脈で集めた操作。
struct Seen {
    /// 操作の ID → 出したモジュール
    actions: BTreeMap<String, BTreeSet<&'static str>>,
    /// 同じ文脈で同じ ID を 2 つ以上のモジュールが出した所
    clashes: BTreeSet<String>,
    /// 同じ文脈で 1 つの役割が 2 つ以上の操作に付いた所
    roles: BTreeSet<String>,
    /// 指す先(指した操作、欄、指された ID)のうち、同じ文脈で操作の一覧にも無く、どのモジュールも
    /// 引き受けない(`run` が `None`)物
    missing: BTreeSet<(String, &'static str, String)>,
}

/// 再計算した見本の文書 3 つ。
struct Samples {
    m: Modules,
    docs: Vec<(Sample, Document, RecomputeReport)>,
}

impl Samples {
    fn new() -> Samples {
        let m = jc_bundle::modules();
        let docs = Sample::ALL
            .into_iter()
            .map(|s| {
                let mut doc = s.make()();
                let report = doc.recompute(&m.registry, m.kernel.as_ref());
                (s, doc, report)
            })
            .collect();
        Samples { m, docs }
    }
}

/// 見本の文書 3 つの、何も選ばない文脈と、機能を 1 つずつ選んだ文脈。
fn contexts(samples: &Samples, f: &mut dyn FnMut(&str, &Context<'_>)) {
    let m = &samples.m;
    for (s, doc, report) in &samples.docs {
        let ids: Vec<Option<FeatureId>> = std::iter::once(None)
            .chain(doc.data().features.iter().map(|e| Some(e.id)))
            .collect();
        for sel in ids {
            let cx = Context {
                selected: sel,
                ..Context::new(doc.data(), report, &m.registry, &m.kernel)
            };
            let label = match sel {
                None => format!("{}(何も選ばない)", s.label()),
                Some(id) => format!("{}(機能 {} を選ぶ)", s.label(), id.0),
            };
            f(&label, &cx);
        }
    }
}

fn extensions(modules: &[Module]) -> Vec<(&'static str, Arc<dyn Extension>)> {
    modules
        .iter()
        .flat_map(|m| m.extensions.iter().map(|e| (m.name, e.clone())))
        .collect()
}

impl Seen {
    fn new() -> Seen {
        Seen {
            actions: BTreeMap::new(),
            clashes: BTreeSet::new(),
            roles: BTreeSet::new(),
            missing: BTreeSet::new(),
        }
    }

    /// 文脈 `cx` で拡張 `exts` が出した操作 `acts`(拡張と同じ並び)と段の帯の行き先 `stages` を数える。
    fn add(
        &mut self,
        label: &str,
        cx: &Context<'_>,
        exts: &[(&'static str, Arc<dyn Extension>)],
        acts: &[Vec<jc_core::Action>],
        stages: &[(String, String)],
    ) {
        let mut here: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
        let mut roles: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut targets: Vec<(String, &'static str, String)> = vec![];
        for ((name, _), acts) in exts.iter().zip(acts) {
            for a in acts {
                here.entry(a.id.clone()).or_default().push(name);
                self.actions.entry(a.id.clone()).or_default().insert(name);
                if let Some(r) = &a.role
                    && !is_marker(r)
                {
                    roles
                        .entry(r.0.to_string())
                        .or_default()
                        .insert(a.id.clone());
                }
                if let Some(t) = &a.then {
                    targets.push((a.id.clone(), "then", t.clone()));
                }
                if let Some(t) = &a.before {
                    targets.push((a.id.clone(), "before", t.clone()));
                }
            }
        }
        for (from, to) in stages {
            targets.push((from.clone(), "Stage.action", to.clone()));
        }
        // 一覧に無い ID(`cam.stage.show:3` のように番号を付けた物)は、引き受けるモジュールがあればよい
        // (Extensions::run と同じく、最初に `Some` を返した物が実行する)
        for t in targets {
            let claimed =
                here.contains_key(&t.2) || exts.iter().any(|(_, e)| e.run(&t.2, cx).is_some());
            if !claimed {
                self.missing.insert(t);
            }
        }
        for (id, by) in here.into_iter().filter(|(_, by)| by.len() > 1) {
            self.clashes
                .insert(format!("{label}: 操作 {id} を {} が出す", by.join("・")));
        }
        for (r, ids) in roles.into_iter().filter(|(_, ids)| ids.len() > 1) {
            let ids: Vec<String> = ids.into_iter().collect();
            self.roles.insert(format!(
                "{label}: 役割 {r} が {} に付いている",
                ids.join("・")
            ));
        }
    }
}

/// 全部の文脈で集めた物。操作を出すのが時間の大半なので、文脈ごとに 1 回だけ出して使い回す。
struct Collected {
    /// 組み立てのモジュールで
    seen: Seen,
    /// 組み立てに試験用の [`Clashing`] を足して(検査が重なりと無い先を見つけるかを見る)
    clashing: Seen,
    /// 操作に付いた役割の印(印も含む)
    roles: BTreeSet<ActionRole>,
}

fn collect(samples: &Samples) -> Collected {
    let modules = jc_bundle::all();
    let exts = extensions(&modules);
    let mut more = exts.clone();
    more.push(("jc-test-clashing", Arc::new(Clashing) as Arc<dyn Extension>));
    let mut c = Collected {
        seen: Seen::new(),
        clashing: Seen::new(),
        roles: BTreeSet::new(),
    };
    contexts(samples, &mut |label, cx| {
        let mut acts: Vec<Vec<jc_core::Action>> = exts.iter().map(|(_, e)| e.actions(cx)).collect();
        let stages: Vec<(String, String)> = exts
            .iter()
            .filter_map(|(name, e)| Some((name, e.stages(cx)?)))
            .flat_map(|(name, band)| {
                band.stages
                    .into_iter()
                    .map(move |st| (format!("{name} の段の帯"), st.action))
            })
            .collect();
        c.roles.extend(acts.iter().flatten().filter_map(|a| a.role));
        c.seen.add(label, cx, &exts, &acts, &stages);
        acts.push(Clashing.actions(cx));
        c.clashing.add(label, cx, &more, &acts, &stages);
    });
    c
}

/// 確かめること: 操作・設定の ID と役割が重ならず、指す先がある(と、その検査が重なりを実際に
/// 見つける)。名前の持ち主の表が今の登録と合う。役割の語彙はどれも誰かが名乗る。
#[test]
fn names_and_roles_of_the_bundle() {
    let samples = Samples::new();
    let c = collect(&samples);
    action_and_setting_ids_and_roles_are_unique_and_targets_exist(&c.seen);
    the_name_owner_table_for_the_layer_check_is_up_to_date(&c.seen);
    clashing_ids_roles_and_missing_targets_are_found(&c.clashing);
    every_role_in_the_vocabulary_is_claimed(&samples, &c.roles);
}

fn action_and_setting_ids_and_roles_are_unique_and_targets_exist(s: &Seen) {
    // 操作の ID が重なると、先に並んだモジュールが黙って勝つ(Extensions::run は最初に引き受けた物)。
    // 役割が重なると、画面がどちらを呼ぶかが並びで決まる
    let modules = jc_bundle::all();
    assert!(
        s.actions.len() > 50,
        "集めた操作が少ない: {}",
        s.actions.len()
    );
    let mut bad: Vec<String> = s.clashes.iter().chain(&s.roles).cloned().collect();
    for (from, field, to) in &s.missing {
        bad.push(format!("{from} の {field} が無い操作 {to} を指す"));
    }

    // 設定は文脈に依らないので、全部のモジュールで 1 回ずつ
    let mut settings: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut roles: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, e) in extensions(&modules) {
        for st in e.settings() {
            settings.entry(st.id.clone()).or_default().push(name);
            if let Some(r) = st.role {
                roles
                    .entry(r.0.to_string())
                    .or_default()
                    .push(st.id.clone());
            }
        }
    }
    for (id, by) in settings.iter().filter(|(_, by)| by.len() > 1) {
        bad.push(format!("設定 {id} を {} が出す", by.join("・")));
    }
    for (r, ids) in roles.iter().filter(|(_, ids)| ids.len() > 1) {
        bad.push(format!("役割 {r} が設定 {} に付いている", ids.join("・")));
    }
    assert!(bad.is_empty(), "名前の重なり・無い先:\n{}", bad.join("\n"));
}

/// 名前の持ち主の表(ID と、名乗ったモジュールの crate)。
fn owners(seen: &Seen, modules: &[Module]) -> BTreeMap<String, BTreeSet<&'static str>> {
    let mut out: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();
    for m in modules {
        let mut r = Registry::new();
        (m.register)(&mut r);
        for d in r.descriptors() {
            out.entry(d.type_id.clone()).or_default().insert(m.name);
        }
        for e in &m.extensions {
            for st in e.settings() {
                out.entry(st.id).or_default().insert(m.name);
            }
        }
    }
    for (id, by) in &seen.actions {
        out.entry(id.clone()).or_default().extend(by);
    }
    out
}

const NAMES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../xtask/names.txt");

fn the_name_owner_table_for_the_layer_check_is_up_to_date(seen: &Seen) {
    let mut text = String::from(
        "# 名前(機能の種類・操作・設定の ID)と、それを名乗るモジュールの crate。\n\
         # cargo xtask verify の層の点検が読む。手で直さず、\n\
         # JC_BLESS=1 cargo test -p jc-bundle --test ids で書き直す(crates/jc-bundle/tests/ids.rs)。\n",
    );
    for (id, by) in owners(seen, &jc_bundle::all()) {
        let by: Vec<&str> = by.into_iter().collect();
        text += &format!("{id}\t{}\n", by.join(","));
    }
    let now = std::fs::read_to_string(NAMES)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if now == text {
        // 合っている
    } else if std::env::var("JC_BLESS").is_ok_and(|v| v == "1") {
        std::fs::write(NAMES, &text).unwrap();
    } else {
        stale(&now, &text);
    }
}

fn stale(now: &str, text: &str) {
    let old: BTreeSet<&str> = now.lines().collect();
    let new: BTreeSet<&str> = text.lines().collect();
    let gone: Vec<&&str> = old.difference(&new).collect();
    let added: Vec<&&str> = new.difference(&old).collect();
    panic!(
        "xtask/names.txt が今の登録と違う。JC_BLESS=1 cargo test -p jc-bundle --test ids で書き直す\n\
         無くなった行: {gone:?}\n増えた行: {added:?}"
    );
}

/// 試験用のモジュール: 既にある操作の ID と役割を重ねて出し、無い操作を後に続ける。
struct Clashing;

impl Extension for Clashing {
    fn actions(&self, _cx: &Context<'_>) -> Vec<jc_core::Action> {
        let mut a = jc_core::Action::new("io.export_stl", "重なり", "io")
            .role(jc_interchange::role::action::EXTRUDE);
        a.then = Some("no.such.action".into());
        vec![a]
    }

    fn run(&self, _id: &str, _cx: &Context<'_>) -> Option<Result<jc_core::Outcome, String>> {
        None
    }
}

fn clashing_ids_roles_and_missing_targets_are_found(s: &Seen) {
    // 上の検査が、実際に重なりと無い先を見つけること(検査が黙って通らないこと)
    assert!(
        s.clashes
            .iter()
            .any(|c| c.contains("io.export_stl") && c.contains("jc-test-clashing")),
        "{:?}",
        s.clashes
    );
    assert!(
        s.roles.iter().any(|r| r.contains("役割 extrude")),
        "{:?}",
        s.roles
    );
    assert!(
        s.missing
            .iter()
            .any(|(_, f, t)| *f == "then" && t == "no.such.action"),
        "{:?}",
        s.missing
    );
}

/// 役割の印の語彙(jc-interchange の `role`)は、どれも組み立ての中のどこかが名乗る(名乗る物の無い
/// 印で探すと、画面・CLI・MCP は黙って何もしない)。
fn every_role_in_the_vocabulary_is_claimed(samples: &Samples, actions: &BTreeSet<ActionRole>) {
    let modules = jc_bundle::all();
    let exts = extensions(&modules);
    let settings: BTreeSet<_> = exts
        .iter()
        .flat_map(|(_, e)| e.settings())
        .filter_map(|g| g.role)
        .collect();
    let kinds: BTreeSet<_> = samples
        .m
        .registry
        .descriptors()
        .into_iter()
        .filter_map(|d| d.role)
        .collect();
    use jc_interchange::role;
    // テンプレートから作る操作は、保存したテンプレートがある時だけ出る(見本の文書には無い)
    let mut missing: Vec<&str> = role::action::ALL
        .iter()
        .filter(|r| **r != role::action::USE_TEMPLATE && !actions.contains(r))
        .map(|r| r.0)
        .collect();
    missing.extend(
        role::kind::ALL
            .iter()
            .filter(|r| !kinds.contains(r))
            .map(|r| r.0),
    );
    missing.extend(
        role::settings::ALL
            .iter()
            .filter(|r| !settings.contains(r))
            .map(|r| r.0),
    );
    assert!(missing.is_empty(), "名乗る物の無い役割の印: {missing:?}");
}
