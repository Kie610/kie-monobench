//! リンクで組み込む部品(jc-asm の link): 挿入、元が変わった印、最新にする・取り消す、固定、
//! リンクを切る、元を消しても開ける、輪になる組み込みを断る。

use super::tests::{assert_ok, rect_sketch, registry};
use super::*;
use jc_asm::link::{
    self, LINK_BREAK, LINK_INSERT, LINK_PIN, LINK_RELINK, LINK_UPDATE, LinkCommands, state,
};
use jc_core::LinkStatus as LinkState;
use jc_core::{
    Context, Document, Extension, FeatureId, Kernel, Outcome, RecomputeReport, body_root,
    visible_bodies,
};
use jc_kernel_smlib::SmlibKernel;
use jc_samples::sample::DocBuilder;
use jc_sketch::SketchPlane;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 長方形 `[x, y, w, h]` を高さ `t` に押し出した箱を足す。
fn block(b: &mut DocBuilder, name: &str, [x, y, w, h]: [f64; 4], t: f64) -> FeatureId {
    let s = rect_sketch(b, SketchPlane::Xy, x, y, w, h);
    b.feature(
        EXTRUDE,
        name,
        Extrude {
            profile: Some(s),
            distance: format!("{t} mm"),
            ..Default::default()
        },
    )
}

/// 試験ごとの空のフォルダ。
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jc-link-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Bench {
    doc: Document,
    reg: jc_core::Registry,
    k: Arc<dyn Kernel>,
    report: RecomputeReport,
}

impl Bench {
    fn new(doc: Document) -> Self {
        let mut t = Bench {
            doc,
            reg: registry(),
            k: Arc::new(SmlibKernel::new()),
            report: Default::default(),
        };
        t.recompute();
        t
    }

    fn recompute(&mut self) {
        self.report = self.doc.recompute(&self.reg, &*self.k);
        assert_ok(&self.report);
    }

    /// 操作 `id` を(選んだ機能、ファイル)で実行して適用する。足した機能を返す。
    fn run(
        &mut self,
        id: &str,
        selected: Option<FeatureId>,
        file: Option<&Path>,
    ) -> Result<Vec<FeatureId>, String> {
        self.run_in(id, selected, file, None)
    }

    /// [`Bench::run`] を、文書を `doc_path` に保存してあるとして。
    fn run_in(
        &mut self,
        id: &str,
        selected: Option<FeatureId>,
        file: Option<&Path>,
        doc_path: Option<&Path>,
    ) -> Result<Vec<FeatureId>, String> {
        let cx = Context {
            selected,
            file,
            doc_path,
            ..Context::new(self.doc.data(), &self.report, &self.reg, &self.k)
        };
        let Some(r) = LinkCommands::default().run(id, &cx) else {
            panic!("{id} は jc-asm の操作");
        };
        let Outcome::Change { label, edits } = r? else {
            panic!("{id} は文書を変える");
        };
        let added = self.doc.apply_edits(&label, edits).unwrap();
        self.recompute();
        Ok(added)
    }

    /// 操作 `id` を実行して適用し、変更の名前を返す(計算の誤りを許す: 見つからないボディの確かめ用)。
    fn run_label(&mut self, id: &str, selected: FeatureId, file: &Path) -> Result<String, String> {
        let cx = Context {
            selected: Some(selected),
            file: Some(file),
            ..Context::new(self.doc.data(), &self.report, &self.reg, &self.k)
        };
        let Some(r) = LinkCommands::default().run(id, &cx) else {
            panic!("{id} は jc-asm の操作");
        };
        let Outcome::Change { label, edits } = r? else {
            panic!("{id} は文書を変える");
        };
        self.doc.apply_edits(&label, edits).unwrap();
        self.report = self.doc.recompute(&self.reg, &*self.k);
        Ok(label)
    }

    /// 見えているボディの(最初の機能、外形の箱の高さ)。
    fn heights(&self) -> Vec<(FeatureId, f64)> {
        let data = self.doc.data();
        visible_bodies(data, &self.reg, &self.report)
            .into_iter()
            .map(|(id, h)| {
                let (lo, hi) = self.k.bounds(h).unwrap();
                (body_root(data, &self.reg, id), hi[2] - lo[2])
            })
            .collect()
    }

    fn link(&self, id: FeatureId) -> jc_asm::Link {
        serde_json::from_value(self.doc.data().feature(id).unwrap().data.clone()).unwrap()
    }

    fn state_of(&self, id: FeatureId, path: &Path) -> LinkState {
        let src = std::fs::read_to_string(path).map_err(|e| e.to_string());
        state(
            &self.link(id),
            src.as_deref().map_err(Clone::clone),
            &self.reg,
        )
        .0
    }
}

/// 蓋の文書(板 2 枚)を箱の文書にリンクで挿入する。
/// - リンク 1 つとボディ 2 つの機能が入り、ボディは「蓋」の部品にまとまり、元のボディの名前が付く
/// - 元の板を厚くし、板を 1 枚足すと「古い」。固定すると「固定」
/// - 最新にすると厚さが変わり、増えたボディは同じ部品に入る。取り消すと前の写しに戻る
/// - 元のファイルを消しても写しで計算できる
/// - リンクを切ると、最新にする操作は断る
#[test]
fn linked_part_is_inserted_flagged_updated_pinned_and_broken() {
    let dir = scratch("update");
    let lid_path = dir.join("蓋.json");
    let mut b = DocBuilder::new();
    block(&mut b, "板", [0.0, 0.0, 30.0, 20.0], 2.0);
    block(&mut b, "つまみ", [13.0, 8.0, 4.0, 4.0], 6.0);
    let lid = b.finish("蓋");
    std::fs::write(&lid_path, lid.to_json()).unwrap();

    let mut b = DocBuilder::new();
    block(&mut b, "箱", [40.0, 0.0, 30.0, 20.0], 10.0);
    let mut t = Bench::new(b.finish("箱"));

    let added = t.run(LINK_INSERT, None, Some(&lid_path)).unwrap();
    assert_eq!(added.len(), 3, "リンクとボディ 2 つ");
    let link_id = added[0];
    assert_eq!(t.heights().len(), 3);
    let data = t.doc.data();
    let comp = data.component_of(added[1]).expect("部品に入る");
    assert_eq!(comp.name, "蓋");
    assert_eq!(comp.bodies, vec![added[1], added[2]]);
    assert_eq!(
        data.body_names.get(&added[1]).map(String::as_str),
        Some("ボディ1")
    );
    assert_eq!(t.state_of(link_id, &lid_path), LinkState::Latest);
    let plate = |t: &Bench| {
        t.heights()
            .into_iter()
            .find(|(r, _)| *r == added[1])
            .unwrap()
            .1
    };
    assert!((plate(&t) - 2.0).abs() < 1e-3);

    // 元を直す: 板を 5 mm に、ボディを 1 つ足す
    let mut b = DocBuilder::new();
    block(&mut b, "板", [0.0, 0.0, 30.0, 20.0], 5.0);
    block(&mut b, "つまみ", [13.0, 8.0, 4.0, 4.0], 6.0);
    block(&mut b, "脚", [0.0, 0.0, 2.0, 2.0], 1.0);
    std::fs::write(&lid_path, b.finish("蓋").to_json()).unwrap();
    assert_eq!(t.state_of(link_id, &lid_path), LinkState::Stale);
    t.run(LINK_PIN, Some(added[2]), None).unwrap();
    assert_eq!(t.state_of(link_id, &lid_path), LinkState::Pinned);
    t.run(LINK_PIN, Some(link_id), None).unwrap();
    assert_eq!(t.state_of(link_id, &lid_path), LinkState::Stale);

    let more = t.run(LINK_UPDATE, Some(added[1]), Some(&lid_path)).unwrap();
    assert_eq!(more.len(), 1, "増えたボディ 1 つ");
    assert_eq!(t.state_of(link_id, &lid_path), LinkState::Latest);
    assert!((plate(&t) - 5.0).abs() < 1e-3);
    assert_eq!(t.heights().len(), 4);
    let comp = t.doc.data().component_of(more[0]).expect("同じ部品に入る");
    assert_eq!(comp.name, "蓋");
    assert_eq!(comp.bodies.len(), 3);

    // 取り消すと前の写し
    assert!(t.doc.undo());
    t.recompute();
    assert!((plate(&t) - 2.0).abs() < 1e-3);
    assert_eq!(t.heights().len(), 3);
    assert!(t.doc.redo());
    t.recompute();

    // 元を消しても、写しで計算できる(見つからない、と知らせる)
    std::fs::remove_file(&lid_path).unwrap();
    assert_eq!(t.state_of(link_id, &lid_path), LinkState::Missing);
    assert!(link::resolve(&t.link(link_id), None).is_none());
    let mut again = Document::from_json(&t.doc.to_json()).unwrap();
    let r = again.recompute(&t.reg, &*t.k);
    assert_ok(&r);
    assert_eq!(visible_bodies(again.data(), &t.reg, &r).len(), 4);

    // リンクを切ると、最新にしない
    t.run(LINK_BREAK, Some(link_id), None).unwrap();
    assert_eq!(t.state_of(link_id, &lid_path), LinkState::Broken);
    assert!(t.run(LINK_UPDATE, Some(link_id), None).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 元のファイルが動いた時は、文書と同じフォルダの同じ名前を探す。
#[test]
fn moved_source_is_found_beside_the_document() {
    let dir = scratch("resolve");
    let mut b = DocBuilder::new();
    block(&mut b, "板", [0.0, 0.0, 10.0, 10.0], 1.0);
    let src = dir.join("板.json");
    std::fs::write(&src, b.finish("板").to_json()).unwrap();
    let mut t = Bench::new(DocBuilder::new().finish("空"));
    let added = t.run(LINK_INSERT, None, Some(&src)).unwrap();
    let mut l = t.link(added[0]);
    assert_eq!(link::resolve(&l, None).as_deref(), Some(src.as_path()));
    l.path = "/どこにもない/板.json".into();
    assert_eq!(link::resolve(&l, None), None);
    let found = link::resolve(&l, Some(&dir.join("組立.json")));
    assert_eq!(found.as_deref(), Some(src.as_path()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 組み込むと輪になる(元の文書が、この文書をリンクで組み込んでいる)時は断る。
#[test]
fn inserting_a_document_that_links_back_is_refused() {
    let dir = scratch("cycle");
    let a = dir.join("A.json");
    let bpath = dir.join("B.json");
    let mut b = DocBuilder::new();
    block(&mut b, "板", [0.0, 0.0, 10.0, 10.0], 1.0);
    std::fs::write(&bpath, b.finish("B").to_json()).unwrap();
    // A は B を組み込む
    let mut t = Bench::new(DocBuilder::new().finish("A"));
    t.run(LINK_INSERT, None, Some(&bpath)).unwrap();
    std::fs::write(&a, t.doc.to_json()).unwrap();
    // B に A を組み込もうとすると、A の中に B がいるので断る
    let mut tb =
        Bench::new(Document::from_json(&std::fs::read_to_string(&bpath).unwrap()).unwrap());
    let e = tb
        .run_in(LINK_INSERT, None, Some(&a), Some(&bpath))
        .unwrap_err();
    assert!(e.contains("輪"), "{e}");
    // 自分自身も断る
    let e = tb
        .run_in(LINK_INSERT, None, Some(&bpath), Some(&bpath))
        .unwrap_err();
    assert!(e.contains("輪"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 文書 `doc` のボディ(最初の機能)に名前を付けて JSON にする。
fn named(doc: Document, names: &[(FeatureId, &str)]) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    let map = v
        .as_object_mut()
        .unwrap()
        .entry("body_names")
        .or_insert_with(|| serde_json::json!({}));
    for (id, n) in names {
        map[id.0.to_string()] = (*n).into();
    }
    v.to_string()
}

/// 元のファイルを選び直す(10-20 a7)。
/// - 別のファイルに替えても、同じ名前のボディは付いて来る(元の機能の番号が違っても)
/// - 新しい元に無いボディは機能を残して誤りにし、変更の名前に並べる
/// - 新しい元に増えたボディは同じ部品に入る
/// - 元が見つからないリンクでも選び直しを出す(最新にするは出さない)
#[test]
fn relink_follows_bodies_by_name_and_lists_missing_ones() {
    let dir = scratch("relink");
    let old_path = dir.join("蓋.json");
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", [0.0, 0.0, 30.0, 20.0], 2.0);
    let knob = block(&mut b, "つまみ", [13.0, 8.0, 4.0, 4.0], 3.0);
    let src = named(b.finish("蓋"), &[(plate, "板"), (knob, "つまみ")]);
    std::fs::write(&old_path, src).unwrap();

    let mut b = DocBuilder::new();
    block(&mut b, "箱", [40.0, 0.0, 30.0, 20.0], 10.0);
    let mut t = Bench::new(b.finish("箱"));
    let added = t.run(LINK_INSERT, None, Some(&old_path)).unwrap();
    let link_id = added[0];

    // 元を消すと、右クリックに選び直しが出て、最新にするは出ない
    std::fs::remove_file(&old_path).unwrap();
    let cx = Context {
        selected: Some(link_id),
        ..Context::new(t.doc.data(), &t.report, &t.reg, &t.k)
    };
    let infos = LinkCommands::default().links(&cx);
    assert_eq!(infos[0].status, LinkState::Missing);
    assert!(infos[0].actions.iter().any(|a| a == LINK_RELINK));
    assert!(!infos[0].actions.iter().any(|a| a == LINK_UPDATE));

    // 新しい元: 脚を先に作るので、板の機能の番号は前と違う。つまみは無い
    let new_path = dir.join("新しい蓋.json");
    let mut b = DocBuilder::new();
    let leg = block(&mut b, "脚", [0.0, 0.0, 2.0, 2.0], 1.0);
    let plate2 = block(&mut b, "板", [0.0, 0.0, 30.0, 20.0], 5.0);
    assert_ne!(plate2, plate, "番号で対応しない形にする");
    let src = named(b.finish("蓋"), &[(leg, "脚"), (plate2, "板")]);
    std::fs::write(&new_path, src).unwrap();

    let label = t.run_label(LINK_RELINK, added[1], &new_path).unwrap();
    assert!(label.contains("見つからないボディ: つまみ"), "{label}");
    let r = &t.report;
    assert!(
        matches!(
            r.feature(added[1]),
            Some(jc_core::FeatureStatus::Ok { .. })
        ),
        "板は名前で付いて来る"
    );
    assert!(
        matches!(
            r.feature(added[2]),
            Some(jc_core::FeatureStatus::Error { .. })
        ),
        "つまみは誤りで残る"
    );
    let h: Vec<(FeatureId, f64)> = t.heights();
    let plate_h = h.iter().find(|(id, _)| *id == added[1]).unwrap().1;
    assert!((plate_h - 5.0).abs() < 1e-3, "新しい元の板の厚さ");
    let comp = t.doc.data().component_of(added[1]).unwrap();
    assert_eq!(comp.bodies.len(), 3, "増えた脚も同じ部品に入る");
    assert_eq!(t.link(link_id).file, "新しい蓋.json");
    assert_eq!(t.state_of(link_id, &new_path), LinkState::Latest);

    // 取り消すと前の元に戻る
    assert!(t.doc.undo());
    assert_eq!(t.link(link_id).file, "蓋.json");
    let _ = std::fs::remove_dir_all(&dir);
}
