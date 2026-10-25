//! 契約テストの共通部品。各機能モジュールは自分のテストから [`check_feature`] を呼ぶ。
//! jc-core の公開の項目が変わると、`cargo xtask verify` がこれを使う crate を全部選んで流す(版の番号は
//! 公開まで上げない。2026-10-21)。

pub mod kernel;
pub mod skip;

use jc_core::{
    Document, FeatureEntry, FeatureId, FeatureStatus, NullKernel, Op, Param, Registry, Transaction,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// 上流の機能に割り当てる ID。`Case::data` から参照する時に使う。
/// パラメータの ID は 1 から順に使われ、上流の機能はその後ろに並ぶため、
/// パラメータの数に依存しないよう大きな値を予約しておく。
pub const UPSTREAM_IDS: [u64; 4] = [1001, 1002, 1003, 1004];
const TARGET_ID: u64 = 2001;

pub struct Case<'a> {
    pub type_id: &'a str,
    /// (名前, 式)
    pub params: &'a [(&'a str, &'a str)],
    /// 対象の前に置く機能(種類, データ)。ID は [`UPSTREAM_IDS`] の順に付く
    pub upstream: &'a [(&'a str, serde_json::Value)],
    /// 正しく評価できるデータ
    pub data: serde_json::Value,
    /// 誤りとして扱われるべきデータ(panic してはならない)
    pub bad_data: &'a [serde_json::Value],
}

fn build(case: &Case<'_>, data: serde_json::Value) -> Document {
    let mut doc = Document::new();
    // 予約した ID まで割り当てを進める
    while doc.data().next_id <= TARGET_ID {
        doc.alloc_feature_id();
    }
    let mut ops = Vec::new();
    for (i, (name, expr)) in case.params.iter().enumerate() {
        ops.push(Op::AddParam {
            index: None,
            param: Param::new(
                jc_core::ParamId(i as u64 + 1),
                name.to_string(),
                expr.to_string(),
            ),
        });
    }
    for (i, (type_id, d)) in case.upstream.iter().enumerate() {
        ops.push(Op::InsertFeature {
            index: None,
            entry: entry(UPSTREAM_IDS[i], type_id, d.clone()),
        });
    }
    ops.push(Op::InsertFeature {
        index: None,
        entry: entry(TARGET_ID, case.type_id, data),
    });
    doc.apply(Transaction::new("契約テスト", ops))
        .expect("契約テストの文書を組み立てられる");
    doc
}

fn entry(id: u64, type_id: &str, data: serde_json::Value) -> FeatureEntry {
    FeatureEntry {
        id: FeatureId(id),
        type_id: type_id.into(),
        name: type_id.into(),
        data,
        suppressed: false,
    }
}

/// 機能の契約を確かめる。破られていれば panic する(テストの失敗になる)。
///
/// 1. 種類が登録されている
/// 2. 正しいデータで評価が成功する(宣言していない入力を読んでいない)
/// 3. 同じ文書を二度再計算しても、保存して読み直しても、出力が変わらない
/// 4. 不正なデータは panic せず、誤りとして報告される
pub fn check_feature(registry: &Registry, case: &Case<'_>) {
    assert!(
        registry.contains(case.type_id),
        "{} が登録されていません",
        case.type_id
    );

    let mut doc = build(case, case.data.clone());
    let r1 = doc.recompute(registry, &NullKernel);
    let out1 = match r1.feature(FeatureId(TARGET_ID)) {
        Some(FeatureStatus::Ok { outputs, .. }) => outputs.clone(),
        other => panic!("{} の評価が成功しません: {other:?}", case.type_id),
    };
    let r2 = Document::from_json(&doc.to_json())
        .expect("保存した文書を読み込める")
        .recompute(registry, &NullKernel);
    match r2.feature(FeatureId(TARGET_ID)) {
        Some(FeatureStatus::Ok { outputs, .. }) => {
            assert_eq!(
                outputs, &out1,
                "{} の出力が保存と読込で変わりました",
                case.type_id
            )
        }
        other => panic!("{} が読み込み後に評価できません: {other:?}", case.type_id),
    }

    for bad in case.bad_data {
        let result = catch_unwind(AssertUnwindSafe(|| {
            build(case, bad.clone()).recompute(registry, &NullKernel)
        }));
        let report = result
            .unwrap_or_else(|_| panic!("{} が不正なデータで panic しました: {bad}", case.type_id));
        assert!(
            matches!(
                report.feature(FeatureId(TARGET_ID)),
                Some(FeatureStatus::Error { .. })
            ),
            "{} が不正なデータ {bad} を誤りとして報告しません",
            case.type_id
        );
    }
}

/// モジュールが 3D のビューに出す網([`jc_core::OverlayShape`])の約束(コア API 0.13.0 から):
/// 面の色は色の役([`jc_core::ShapeInk`])で渡し、色そのものは画面のテーマが決める。
/// - 色の役の数は網の面の数を超えない(足りない面は画面の途中の形の色)
/// - 凡例に出す色の役は、どれかの面に使っている
/// - 凡例の名前は空でない
pub fn check_overlay_shape(shape: &jc_core::OverlayShape) {
    assert!(
        shape.face_colors.len() <= shape.mesh.face_names.len(),
        "面の色の役 {} 個が、網の面 {} 個より多い",
        shape.face_colors.len(),
        shape.mesh.face_names.len()
    );
    for (ink, name) in &shape.legend {
        assert!(!name.trim().is_empty(), "凡例の名前が空({ink:?})");
        assert!(
            shape.face_colors.contains(&Some(*ink)),
            "凡例の {name}({ink:?})を使う面がない"
        );
    }
}
