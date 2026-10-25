//! 見本の全部の面と辺に設計の操作を掛けて、落ちないかを試す。
//!
//! 1 手 = 見本のボディの面 1 つか辺 1 本を選んだ状態で、その時に使える設計の操作(押し出し・
//! 押す引く・穴・シェル・フィレット・面取りなど、ファイルや名前を聞かない物)を 1 つ実行し、
//! 足した機能を計算して、取り消す。確かめること:
//! - 落ちない(パニックしない)。
//! - 取り消すと、ボディの体積が元に戻る。
//! - 前に通った手(基準の一覧 `matrix_baseline.txt`)が、計算の誤りにならない(SMLib の時だけ)。
//!
//! 普段の検査は手の名前の安定なハッシュで 8 手に 1 つだけ試す(数秒)。全部は
//! `cargo test -p jc-samples -- --ignored matrix_full`(公開前とカーネルを直した時)。
//! 基準の一覧は全部を試す時に `JC_MATRIX_BLESS=1` で書き直す。

use crate::{
    corner_document, cylinder_cut_from_bottom_document, fillet_then_chamfer_document,
    reference_cam_3d_document, reference_document,
};
use jc_core::{
    Context, Document, EdgePick, FacePick, FeatureStatus, Input, Modules, Op, Outcome, ShapeHandle,
    Transaction,
};
use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

thread_local! {
    static MODULES: Modules = jc_bundle::modules();
}

fn modules() -> Modules {
    MODULES.with(|m| m.clone())
}

/// 普段の検査で試す割合(8 手に 1 つ)。
const SAMPLE_EVERY: u64 = 8;

const BASELINE: &str = include_str!("../tests/matrix_baseline.txt");

/// 名前の安定なハッシュ(FNV-1a。Rust の版や実行ごとに変わらない)。
fn stable_hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// 1 つの見本を試した結果。
#[derive(Default)]
struct Outcomes {
    tried: usize,
    /// 足した機能が計算できた手
    ok: BTreeSet<String>,
    /// 計算の誤りになった手(落ちてはいない)
    failed: BTreeSet<String>,
    /// 落ちた手・取り消しで戻らなかった手(試験の失敗)
    broken: Vec<String>,
}

fn volumes(doc: &mut Document, m: &Modules) -> Vec<(u64, i64)> {
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    jc_core::visible_bodies(doc.data(), &m.registry, &report)
        .into_iter()
        .map(|(f, h)| {
            let v = m.kernel.mass(h).map_or(-1.0, |p| p.volume);
            // 1e-3 mm³ の単位で比べる(計算の揺れを吸う)
            (f.0, (v * 1e3).round() as i64)
        })
        .collect()
}

/// 3D のビューで選んでいる物(面 1 つか辺 1 本)。
enum Pick {
    Face(FacePick),
    Edge([EdgePick; 1]),
}

impl Pick {
    /// 選びを入れた文脈。
    fn context<'a>(&'a self, cx: Context<'a>) -> Context<'a> {
        match self {
            Pick::Face(f) => Context {
                picked: Some(f),
                ..cx
            },
            Pick::Edge(e) => Context { edges: e, ..cx },
        }
    }

    fn label(&self) -> String {
        match self {
            Pick::Face(f) => format!("面 {}", f.face),
            Pick::Edge([e]) => format!("辺 {}|{}", e.faces[0], e.faces[1]),
        }
    }
}

/// 選び `pick` で使える設計の操作。
fn actions(doc: &mut Document, m: &Modules, pick: &Pick) -> Vec<String> {
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = pick.context(Context::new(doc.data(), &report, &m.registry, &m.kernel));
    m.extensions
        .actions(&cx)
        .into_iter()
        .filter(|a| a.group == "cad" && a.disabled.is_none() && matches!(a.input, Input::None))
        .map(|a| a.id)
        .collect()
}

/// 1 手を行う。足した機能が計算できたら真、誤りなら偽。文書を変えない操作は `None`。
fn one_move(
    doc: &mut Document,
    m: &Modules,
    action: &str,
    pick: &Pick,
) -> Result<Option<bool>, String> {
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = pick.context(Context::new(doc.data(), &report, &m.registry, &m.kernel));
    let Ok(Outcome::Change { label, edits }) = m.extensions.run(action, &cx) else {
        return Ok(None);
    };
    let added = doc
        .apply_edits(&label, edits)
        .map_err(|e| format!("変更を適用できない: {e}"))?;
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    let ok = added
        .iter()
        .all(|id| matches!(report.feature(*id), Some(FeatureStatus::Ok { .. })));
    if !doc.undo() {
        return Err("取り消せない".into());
    }
    Ok(Some(ok))
}

/// 見本 `name` の面と辺を総なめにする。`take` で試す手を選ぶ。
fn sweep(name: &str, make: fn() -> Document, take: &dyn Fn(&str) -> bool) -> Outcomes {
    let m = modules();
    let mut doc = make();
    let before = volumes(&mut doc, &m);
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    let bodies: Vec<(jc_core::FeatureId, ShapeHandle)> =
        jc_core::visible_bodies(doc.data(), &m.registry, &report);
    let mut out = Outcomes::default();
    for (body, h) in bodies {
        let faces: Vec<String> = m
            .kernel
            .faces(h)
            .map(|f| f.into_iter().map(|f| f.name).collect())
            .unwrap_or_default();
        let mut edges: Vec<[String; 2]> = Vec::new();
        for list in [
            m.kernel.edges(h),
            m.kernel.arc_edges(h),
            m.kernel.curve_edges(h),
        ] {
            for e in list.unwrap_or_default() {
                if !edges.contains(&e.faces) {
                    edges.push(e.faces);
                }
            }
        }
        let picks: Vec<Pick> = faces
            .into_iter()
            .map(|face| {
                Pick::Face(FacePick {
                    feature: body,
                    face,
                })
            })
            .chain(edges.into_iter().map(|faces| {
                Pick::Edge([EdgePick {
                    feature: body,
                    faces,
                }])
            }))
            .collect();
        for pick in &picks {
            for action in actions(&mut doc, &m, pick) {
                let key = format!("{name} / {} / {action}", pick.label());
                if !take(&key) {
                    continue;
                }
                out.tried += 1;
                let r = catch_unwind(AssertUnwindSafe(|| one_move(&mut doc, &m, &action, pick)));
                match r {
                    Ok(Ok(Some(true))) => {
                        out.ok.insert(key);
                    }
                    Ok(Ok(Some(false))) => {
                        out.failed.insert(key);
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(e)) => out.broken.push(format!("{key}: {e}")),
                    Err(_) => {
                        out.broken.push(format!("{key}: 落ちた(パニック)"));
                        // 文書が途中の形かもしれないので、作り直して続ける
                        doc = make();
                    }
                }
            }
        }
    }
    if volumes(&mut doc, &m) != before {
        out.broken
            .push(format!("{name}: 全部を取り消しても体積が元に戻らない"));
    }
    out
}

/// CAM の機能の手前まで巻き戻す(手ごとに工具経路を計算し直さないため。設計の操作はその位置に入る)。
fn without_cam(mut doc: Document) -> Document {
    let first_cam = doc
        .data()
        .features
        .iter()
        .position(|f| f.type_id.starts_with("cam."));
    if let Some(n) = first_cam {
        doc.apply(Transaction::new(
            "CAM の手前まで",
            vec![Op::SetRollback { rollback: Some(n) }],
        ))
        .expect("巻き戻せる");
    }
    doc
}

/// 見本と名前。
fn sample(name: &str) -> fn() -> Document {
    match name {
        "基準部品" => reference_document,
        "3D の見本" => || without_cam(reference_cam_3d_document()),
        "丸めの後の面取り" => fillet_then_chamfer_document,
        "フィレット" => || corner_document(jc_cad_edge::FILLET, "ball"),
        "下から円柱" => cylinder_cut_from_bottom_document,
        _ => unreachable!("見本 {name} は無い"),
    }
}

const SAMPLES: [&str; 5] = [
    "基準部品",
    "3D の見本",
    "丸めの後の面取り",
    "フィレット",
    "下から円柱",
];

/// 見本 `name` を試し、落ちた手と、前に通ったのに誤りになった手を失敗にする。
fn check(name: &str, full: bool) -> Outcomes {
    let t = std::time::Instant::now();
    let take = |k: &str| full || stable_hash(k).is_multiple_of(SAMPLE_EVERY);
    let out = sweep(name, sample(name), &take);
    eprintln!(
        "{name}: {} 手(通った {}、計算の誤り {}、落ちた {})、{:.1} 秒",
        out.tried,
        out.ok.len(),
        out.failed.len(),
        out.broken.len(),
        t.elapsed().as_secs_f64()
    );
    assert!(
        out.broken.is_empty(),
        "落ちた手:\n{}",
        out.broken.join("\n")
    );
    if modules().kernel.name() == "smlib" {
        let regressed: Vec<&str> = BASELINE
            .lines()
            .filter(|l| l.starts_with(&format!("{name} / ")) && out.failed.contains(*l))
            .collect();
        assert!(
            regressed.is_empty(),
            "前に通った手が計算の誤りになった:\n{}",
            regressed.join("\n")
        );
    }
    out
}

macro_rules! matrix {
    ($sampled:ident, $full:ident, $name:expr) => {
        #[test]
        fn $sampled() {
            check($name, false);
        }

        #[test]
        #[ignore = "全部の手は時間がかかるので、公開前とカーネルを直した時に --ignored で流す"]
        fn $full() {
            let out = check($name, true);
            if std::env::var_os("JC_MATRIX_BLESS").is_some() {
                bless($name, &out.ok);
            }
        }
    };
}

/// 基準の一覧のうち見本 `name` の行を、今通った手に置き換える。
fn bless(name: &str, ok: &BTreeSet<String>) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/matrix_baseline.txt");
    let now = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: BTreeSet<String> = now
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with(&format!("{name} / ")))
        .map(str::to_string)
        .collect();
    lines.extend(ok.iter().cloned());
    let head = "# 見本の全部の面と辺の試験(matrix_tests.rs)で通った手。\
                JC_MATRIX_BLESS=1 cargo test -p jc-samples -- --ignored matrix_full で書き直す\n";
    let body: Vec<String> = lines.into_iter().collect();
    std::fs::write(path, format!("{head}{}\n", body.join("\n"))).unwrap();
}

matrix!(matrix_reference, matrix_full_reference, SAMPLES[0]);
matrix!(matrix_reference_3d, matrix_full_reference_3d, SAMPLES[1]);
matrix!(
    matrix_fillet_chamfer,
    matrix_full_fillet_chamfer,
    SAMPLES[2]
);
matrix!(matrix_corner, matrix_full_corner, SAMPLES[3]);
matrix!(matrix_cylinder, matrix_full_cylinder, SAMPLES[4]);

#[test]
fn hash_is_stable() {
    // 版をまたいで同じ手を選ぶため、値を固定して確かめる
    assert_eq!(stable_hash(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(stable_hash("a"), 0xaf63_dc4c_8601_ec8c);
}
