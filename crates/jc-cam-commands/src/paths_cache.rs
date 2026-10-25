//! 経路の読み直しの写し。加工の経路は出力の JSON から読み直すので、数十万の動きがあると重い。
//! 画面は毎コマ(重ね描き・段の帯・操作の一覧・要約)経路を求めるので、再計算の結果の番号
//! ([`jc_core::RecomputeReport::stamp`])と文書の並びが同じ間は、読み直したものを使い回す。

use crate::SetupPaths;
use crate::commands::fingerprint;
use jc_core::{Context, FeatureId};
use std::sync::{Arc, Mutex};

/// 読み直した全セットアップと、その経路の指紋。
struct Entry {
    stamp: u64,
    sig: u64,
    all: Arc<Vec<SetupPaths>>,
    prints: Arc<Vec<u64>>,
}

#[derive(Default)]
pub(crate) struct PathsCache(Mutex<Option<Entry>>);

/// 読み直したセットアップの 1 つ(全体の写しの中を指す。中身は `Deref` で読む)。
#[derive(Clone)]
pub(crate) struct SetupRef {
    all: Arc<Vec<SetupPaths>>,
    prints: Arc<Vec<u64>>,
    i: usize,
}

impl std::ops::Deref for SetupRef {
    type Target = SetupPaths;
    fn deref(&self) -> &SetupPaths {
        &self.all[self.i]
    }
}

impl SetupRef {
    /// 経路の指紋([`fingerprint`] と同じ値)。
    pub(crate) fn print(&self) -> u64 {
        self.prints[self.i]
    }
}

/// 文書のうち、セットアップの読み直しに効くもの(機能の番号・種類・名前・どのセットアップか)の指紋。
fn data_sig(cx: &Context<'_>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in &cx.data.features {
        f.id.0.hash(&mut h);
        f.type_id.hash(&mut h);
        f.name.hash(&mut h);
        f.data.get("setup").map(|v| v.to_string()).hash(&mut h);
    }
    h.finish()
}

impl PathsCache {
    /// 全セットアップの経路。結果と文書が前と同じなら読み直さない。
    pub(crate) fn all(&self, cx: &Context<'_>) -> (Arc<Vec<SetupPaths>>, Arc<Vec<u64>>) {
        let stamp = cx.report.stamp();
        let sig = data_sig(cx);
        let mut c = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // 番号 0 はまだ計算していない空の結果(試験などで作り直す)なので、使い回さない
        if let Some(e) = c.as_ref()
            && stamp != 0
            && e.stamp == stamp
            && e.sig == sig
        {
            return (e.all.clone(), e.prints.clone());
        }
        let all = crate::setups(cx.data, cx.report, cx.registry);
        let prints: Vec<u64> = all.iter().map(|s| fingerprint(&s.paths)).collect();
        let e = Entry {
            stamp,
            sig,
            all: Arc::new(all),
            prints: Arc::new(prints),
        };
        let out = (e.all.clone(), e.prints.clone());
        *c = Some(e);
        out
    }

    /// セットアップ `setup` の経路。
    pub(crate) fn get(&self, cx: &Context<'_>, setup: FeatureId) -> Option<SetupRef> {
        let (all, prints) = self.all(cx);
        let i = all.iter().position(|s| s.setup == setup)?;
        Some(SetupRef { all, prints, i })
    }
}
