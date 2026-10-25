//! デザインの概要とプロジェクトの中身の控え。どちらもファイルを読むので裏のスレッドで読み、読み終えたら
//! 画面を描き直す(ネットワークのドライブでも画面を止めない。縮小図の控えと同じ考え)。
//!
//! 仕分けでファイルが変わったら [`Loads::refresh`] で古い印を付け、読み直す間は前の物を出す
//! (出し直しのたびに「読んでいます…」に戻してちらつかせない)。読み直している間は
//! [`Loads::loading`] が真になり、画面の台本はそれが終わるまで次の操作を待つ(前の物で確かめない)。

use kmb_session::library::{Summary, summary};
use kmb_session::projects::{Folder, ModelInfo, scan};
use kmb_session::tidy::{self, Entry};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// 同時に読むスレッドの数(フォルダに多くのデザインがあっても、スレッドを並べすぎない)。
const AT_ONCE: usize = 4;

/// 読んだ物の控え(鍵はパス)。
pub(crate) struct Loads<T> {
    /// 読んだ物と、読んだ時の世代
    done: HashMap<PathBuf, (T, u64)>,
    /// 読んでいる物
    loading: HashMap<PathBuf, Loading<T>>,
    /// 世代([`Loads::refresh`] で 1 増やす。古い物は読み直す)
    generation: u64,
    read: fn(&Path) -> T,
    /// 最後に読み終えた物を受け取った描画の番号(egui の `cumulative_pass_nr`)
    received: Option<u64>,
    /// 空きが無くて読み始められなかった物があった、最後の描画の番号
    deferred: Option<u64>,
}

/// 読んでいる物 1 つ。
struct Loading<T> {
    rx: Receiver<T>,
    /// 読み始めた時の世代
    generation: u64,
    /// 読むスレッドが持つ印(数が 1 に戻ればスレッドは終わっている)
    running: Arc<()>,
    /// 最後に求められた描画の番号(求められなくなった物は、終わった後に待たない)
    asked: u64,
}

impl<T: Send + 'static> Loads<T> {
    fn new(read: fn(&Path) -> T) -> Self {
        Self {
            done: HashMap::new(),
            loading: HashMap::new(),
            generation: 0,
            read,
            received: None,
            deferred: None,
        }
    }

    /// `path` の物(まだ読んでいなければ読み始めて `None`。古ければ読み直しつつ前の物)。
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<&T> {
        let pass = ctx.cumulative_pass_nr();
        if let Some(l) = self.loading.get_mut(path) {
            l.asked = pass;
            match l.rx.try_recv() {
                Ok(v) => {
                    let g = l.generation;
                    self.loading.remove(path);
                    self.done.insert(path.to_path_buf(), (v, g));
                    self.received = Some(pass);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.loading.remove(path);
                }
            }
        } else if self
            .done
            .get(path)
            .is_none_or(|(_, g)| *g < self.generation)
        {
            // 空きが無ければ待つ(読み終えたスレッドが描き直させるので、次のフレームで始まる)
            if self.loading.len() < AT_ONCE {
                let (tx, rx) = channel();
                let p = path.to_path_buf();
                let ctx = ctx.clone();
                let read = self.read;
                let running = Arc::new(());
                let token = running.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(read(&p));
                    drop(token);
                    ctx.request_repaint();
                });
                self.loading.insert(
                    path.to_path_buf(),
                    Loading {
                        rx,
                        generation: self.generation,
                        running,
                        asked: pass,
                    },
                );
            } else {
                self.deferred = Some(pass);
            }
        }
        self.done.get(path).map(|(v, _)| v)
    }

    /// 読んだ物に古い印を付ける(次に要る時に読み直す)。
    pub fn refresh(&mut self) {
        self.generation += 1;
    }

    /// 描画 `pass` の時点で、読み直しの途中か(出している物がもうすぐ変わるか)。読むスレッドが
    /// 走っている・読み終えた物をまだ出していない・空きを待っている物がある、のどれか。受け取った
    /// 直後の 2 描画も含める(画面の部品の一覧は描いた次の描画で届く)。求められなくなった物
    /// (別のフォルダへ移った後の概要など)は、スレッドが終わった後は待たない。
    pub fn loading(&self, pass: u64) -> bool {
        let recent = |p: Option<u64>| p.is_some_and(|p| p + 2 >= pass);
        recent(self.received)
            || recent(self.deferred)
            || self
                .loading
                .values()
                .any(|l| Arc::strong_count(&l.running) > 1 || l.asked + 2 >= pass)
    }
}

pub(crate) type Summaries = Loads<Summary>;

impl Default for Summaries {
    fn default() -> Self {
        Loads::new(summary)
    }
}

/// プロジェクト 1 つの中身(フォルダの木と、デザインの分類と整理に使う値)。
pub(crate) struct Tree {
    pub root: Folder,
    pub infos: HashMap<PathBuf, ModelInfo>,
    /// デザインごとの指紋・版の数・形の数(整理の頁で使う)
    pub entries: Vec<Entry>,
}

/// 探すフォルダの深さ(プロジェクトの中の階層)。
const DEPTH: usize = 6;

fn read_tree(root: &Path) -> Tree {
    let root = scan(root, DEPTH);
    // ファイルは 1 回だけ読み、分類と指紋を一緒に作る
    let entries: Vec<Entry> = root
        .all_models()
        .into_iter()
        .filter_map(|m| tidy::entry(&m.path))
        .collect();
    let infos = entries
        .iter()
        .map(|e| (e.path.clone(), e.info.clone()))
        .collect();
    Tree {
        root,
        infos,
        entries,
    }
}

pub(crate) type Trees = Loads<Tree>;

impl Default for Trees {
    fn default() -> Self {
        Loads::new(read_tree)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slow(_: &Path) -> u32 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        7
    }

    fn pass(ctx: &egui::Context) -> u64 {
        // 描く物は使わない(文字の絵の差分も捨てる)
        ctx.run_ui(Default::default(), |_| {})
            .textures_delta
            .clear();
        ctx.cumulative_pass_nr()
    }

    /// 確かめること: 読み直している間と、受け取った直後は読み込み中。求められなくなった物は、
    /// スレッドが終われば待たない(別のフォルダへ移った後の概要などで、台本が止まり続けない)。
    #[test]
    fn loading_lasts_until_the_new_value_is_shown_but_not_for_values_nobody_asks_for() {
        let ctx = egui::Context::default();
        let mut l: Loads<u32> = Loads::new(slow);
        let (a, b) = (Path::new("a"), Path::new("b"));
        assert!(!l.loading(pass(&ctx)));
        assert!(l.get(&ctx, a).is_none());
        assert!(l.loading(ctx.cumulative_pass_nr()), "読んでいる途中");
        while Arc::strong_count(&l.loading[a].running) > 1 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // 読み終えたがまだ出していない(求められている)間は、読み込み中
        assert!(l.loading(ctx.cumulative_pass_nr()));
        let p = pass(&ctx);
        assert_eq!(l.get(&ctx, a), Some(&7));
        assert!(
            l.loading(p + 2),
            "受け取った直後は、部品の一覧が届くまで待つ"
        );
        assert!(!l.loading(p + 3));
        // b は読み始めた後で求められなくなった: スレッドが終われば待たない
        assert!(l.get(&ctx, b).is_none());
        while Arc::strong_count(&l.loading[b].running) > 1 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let mut p = ctx.cumulative_pass_nr();
        for _ in 0..3 {
            p = pass(&ctx);
        }
        assert!(!l.loading(p), "求められなくなった物は待たない");
    }
}
