//! 形を使わない試験(部品の木の操作、行列、部品表の CSV)。形を動かす試験は kmb-solid の asm_tests。

use super::*;
use kmb_core::{Document, DocumentData, Op, Transaction};

fn comp(id: u64, name: &str, parent: Option<u64>, bodies: &[u64]) -> Component {
    Component {
        id,
        name: name.into(),
        parent,
        bodies: bodies.iter().map(|b| FeatureId(*b)).collect(),
    }
}

/// 部品の木: ボディを移す・部品を解くと、中身は親へ。外観は近い部品から受け継ぐ。
#[test]
fn component_tree_moves_bodies_and_dissolves_into_the_parent() {
    let mut d = DocumentData {
        next_id: 100,
        ..Default::default()
    };
    d.components = vec![
        comp(10, "引き出し", None, &[1]),
        comp(11, "前板", Some(10), &[2]),
        comp(12, "取っ手", Some(11), &[3]),
    ];
    assert_eq!(d.component_tree(10), vec![10, 11, 12]);
    assert_eq!(
        d.component_bodies(10),
        vec![FeatureId(1), FeatureId(2), FeatureId(3)]
    );
    assert_eq!(
        Part::Component { id: 11 }.roots(&d),
        vec![FeatureId(2), FeatureId(3)]
    );
    assert_eq!(Part::of_body(&d, FeatureId(3)), Part::Component { id: 12 });
    assert_eq!(
        Part::of_body(&d, FeatureId(9)),
        Part::Body { root: FeatureId(9) }
    );
    // 外観: ボディに無ければ近い部品の物
    let mut looks = kmb_look::appearance::Appearances::default();
    looks.set_component(d.components[0].id, Some("赤"));
    assert_eq!(looks.appearance_of(&d, FeatureId(3)), Some("赤"));
    looks.set_component(d.components[2].id, Some("青"));
    assert_eq!(looks.appearance_of(&d, FeatureId(3)), Some("青"));
    // 移す
    let moved = d.components_moving(&[FeatureId(3)], Some(10));
    assert_eq!(moved[0].bodies, vec![FeatureId(1), FeatureId(3)]);
    assert!(moved[2].bodies.is_empty());
    // 解く: 前板のボディは引き出しへ、取っ手は引き出しの子へ
    let gone = d.components_without(11);
    assert_eq!(gone.len(), 2);
    assert_eq!(gone[0].bodies, vec![FeatureId(1), FeatureId(2)]);
    assert_eq!(gone[1].parent, Some(10));
}

/// 部品の木の決まりを守らない変更は断り、取り消すと前の木に戻る。
#[test]
fn set_components_checks_the_tree_and_undoes() {
    let mut doc = Document::new();
    for _ in 0..20 {
        doc.alloc_group_id();
    }
    let ok = vec![comp(3, "箱", None, &[1]), comp(4, "蓋", Some(3), &[2])];
    doc.apply(Transaction::new(
        "部品",
        vec![Op::SetComponents {
            components: ok.clone(),
        }],
    ))
    .unwrap();
    assert_eq!(doc.data().components, ok);
    // 輪・親が無い・2 つの部品に同じボディ・割り当てていない番号
    for bad in [
        vec![comp(3, "a", Some(4), &[]), comp(4, "b", Some(3), &[])],
        vec![comp(3, "a", Some(9), &[])],
        vec![comp(3, "a", None, &[1]), comp(4, "b", None, &[1])],
        vec![comp(500, "a", None, &[])],
        vec![comp(3, " ", None, &[])],
    ] {
        assert!(
            doc.apply(Transaction::new(
                "だめ",
                vec![Op::SetComponents { components: bad }]
            ))
            .is_err()
        );
    }
    assert_eq!(doc.data().components, ok);
    // 番号 0 は新しい番号にする
    doc.apply_edits(
        "新しい部品",
        vec![Edit::SetComponents {
            components: vec![comp(0, "新", None, &[])],
        }],
    )
    .unwrap();
    assert!(doc.data().components[0].id >= 20);
    doc.undo();
    assert_eq!(doc.data().components, ok);
    // 保存して読み直しても同じ
    let json = serde_json::to_string(doc.data()).unwrap();
    let back: DocumentData = serde_json::from_str(&json).unwrap();
    assert_eq!(back.components, ok);
}

/// 回して動かす行列の逆と積。
#[test]
fn rigid_inverse_undoes_a_rotation_and_a_shift() {
    let m = joint::mul(
        &k::translation([3.0, -2.0, 5.0]),
        &k::rotation([1.0, 1.0, 0.0], [0.0, 0.3, 1.0], 0.7),
    );
    let back = joint::rigid_inverse(&m);
    let p = [4.0, -1.5, 2.0];
    let q = k::transform_point(&back, k::transform_point(&m, p));
    for i in 0..3 {
        assert!((p[i] - q[i]).abs() < 1e-9, "{p:?} {q:?}");
    }
}

/// 部品表の CSV: 頭に BOM、カンマを含む名前は囲む、質量の無い行は空。
#[test]
fn bom_csv_quotes_names_and_leaves_unknown_mass_empty() {
    let rows = vec![
        BomRow {
            names: vec!["棚板, 上".into(), "棚板2".into()],
            kind: "部品".into(),
            count: 2,
            material: "パイン".into(),
            size: [300.0, 200.0, 18.0],
            volume: 1_080_000.0,
            mass: Some(540.0),
        },
        BomRow {
            names: vec!["ボディ3".into()],
            kind: "ボディ".into(),
            count: 1,
            material: String::new(),
            size: [10.0, 10.0, 10.0],
            volume: 1000.0,
            mass: None,
        },
    ];
    let csv = bom_csv(&rows);
    assert!(csv.starts_with('\u{feff}'));
    let lines: Vec<&str> = csv.trim_end().split("\r\n").collect();
    assert_eq!(
        lines[1],
        "1,\"棚板, 上、棚板2\",2,パイン,300.0,200.0,18.0,1080.000,540.0"
    );
    assert_eq!(lines[2], "2,ボディ3,1,,10.0,10.0,10.0,1.000,");
}

/// 部品表の表: 列の鍵と既定で出す列。合計質量は数を掛け、質量の無い行は空。文は既定の列だけで、
/// 数の列は右に寄せる。
#[test]
fn bom_table_has_selectable_columns() {
    let rows = vec![
        BomRow {
            names: vec!["棚板".into()],
            kind: "部品".into(),
            count: 2,
            material: "パイン".into(),
            size: [300.0, 200.0, 18.0],
            volume: 1_080_000.0,
            mass: Some(540.0),
        },
        BomRow {
            names: vec!["ボディ3".into()],
            kind: "ボディ".into(),
            count: 1,
            material: String::new(),
            size: [10.0, 10.0, 10.0],
            volume: 1000.0,
            mass: None,
        },
    ];
    let t = bom_table(&rows);
    assert_eq!(t.key, crate::BOM);
    let ids: Vec<&str> = t.columns.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "item",
            "name",
            "kind",
            "count",
            "material",
            "length",
            "width",
            "thickness",
            "volume",
            "mass",
            "total_mass"
        ]
    );
    let hidden: Vec<&str> = t
        .columns
        .iter()
        .filter(|c| !c.shown)
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(hidden, ["kind", "total_mass"]);
    assert!(t.rows.iter().all(|r| r.len() == t.columns.len()));
    assert_eq!(t.rows[0][10], "1080.0");
    assert_eq!(t.rows[1][9], "");
    assert_eq!(t.rows[1][10], "");
    let text = t.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "番号  名前     数  材料    長さ mm  幅 mm  厚さ mm  体積 cm³  質量 g"
    );
    assert_eq!(
        lines[1],
        "   1  棚板      2  パイン    300.0  200.0     18.0    1080.00   540.0"
    );
    assert_eq!(
        lines[2],
        "   2  ボディ3   1  -          10.0   10.0     10.0       1.00       -"
    );
    assert!(!lines[0].contains("種類"), "既定では隠す列");
}

/// ジョイントのデータは往復し、知らない項目は断る。
#[test]
fn joint_data_round_trips() {
    let j = joint::new_joint(
        Part::Component { id: 4 },
        Some(Part::Body { root: FeatureId(1) }),
        vec![FeatureId(7)],
        &Origin {
            feature: FeatureId(7),
            point: [0.0, 0.0, 10.0],
            axis: [2.0, 0.0, 0.0],
        },
    );
    assert_eq!(j.axis, [1.0, 0.0, 0.0]);
    let v = serde_json::to_value(&j).unwrap();
    let back: Joint = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(back, j);
    let mut bad = v;
    bad["what"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Joint>(bad).is_err());
}

/// 自動分解の向きと距離: 中心からいちばんずれた軸の側へ、同じ向きの物は近い順に長さを積む。
/// 中心に重なる物は +Z。同じくらいのずれなら Z を選ぶ。
#[test]
fn spread_moves_away_from_the_center_and_stacks_the_same_direction() {
    use crate::anim::spread;
    let b = |lo: [f64; 3], hi: [f64; 3]| (lo, hi);
    // 横に 3 つ並んだ箱(幅 10)。真ん中は中心に重なる
    let row = [
        b([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]),
        b([10.0, 0.0, 0.0], [20.0, 10.0, 10.0]),
        b([20.0, 0.0, 0.0], [30.0, 10.0, 10.0]),
    ];
    assert_eq!(
        spread(&row, 1.0),
        vec![[-10.0, 0.0, 0.0], [0.0, 0.0, 10.0], [10.0, 0.0, 0.0]]
    );
    // 縦に 3 段(高さ 5・5・20、全体の中心は z = 20)。下の 2 つは近い順に積む: 2 段目 -5、
    // 1 段目 -(5+5)。目盛 2 で倍
    let stack = [
        b([0.0, 0.0, 0.0], [10.0, 10.0, 5.0]),
        b([0.0, 0.0, 15.0], [10.0, 10.0, 20.0]),
        b([0.0, 0.0, 20.0], [10.0, 10.0, 40.0]),
    ];
    assert_eq!(
        spread(&stack, 2.0),
        vec![[0.0, 0.0, -20.0], [0.0, 0.0, -10.0], [0.0, 0.0, 40.0]]
    );
    // 斜めにずれた物(X と Z に同じだけ)は Z
    let diag = [
        b([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]),
        b([8.0, 0.0, 8.0], [10.0, 2.0, 10.0]),
    ];
    assert_eq!(spread(&diag, 1.0), vec![[0.0, 0.0, -2.0], [0.0, 0.0, 2.0]]);
}

/// リンクの元を移した時: 保存した絶対パスに無ければ、元のフォルダの移した記録をたどって探す
/// (記録の形は kmb-session の organize の試験と同じ文字列)。続けて移した物もたどる。
#[test]
fn a_moved_source_is_found_by_following_the_moved_records() {
    use crate::link::{Link, resolve};
    let d = std::env::temp_dir().join(format!("kmb-asm-moved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let (a, b, c) = (d.join("a"), d.join("b"), d.join("c"));
    for f in [&a, &b, &c] {
        std::fs::create_dir_all(f.join(".jc")).unwrap();
    }
    let at = |p: &std::path::Path| p.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        a.join(".jc/moved.json"),
        format!(
            r#"{{"moves":[{{"from":"天板.json","to":"{}","at":1792700000}}]}}"#,
            at(&b.join("天板.json"))
        ),
    )
    .unwrap();
    std::fs::write(
        b.join(".jc/moved.json"),
        format!(
            r#"{{"moves":[{{"from":"天板.json","to":"{}","at":1792700001}}]}}"#,
            at(&c.join("天板2.json"))
        ),
    )
    .unwrap();
    let link = Link {
        path: a.join("天板.json").to_string_lossy().to_string(),
        file: "天板.json".into(),
        doc: serde_json::json!({}),
        pinned: false,
        broken: false,
    };
    assert_eq!(resolve(&link, None), None, "元がどこにも無い");
    std::fs::write(c.join("天板2.json"), "{}").unwrap();
    assert_eq!(resolve(&link, None), Some(c.join("天板2.json")));
    // 記録をたどるより、保存した絶対パスにあればそちら
    std::fs::write(a.join("天板.json"), "{}").unwrap();
    assert_eq!(resolve(&link, None), Some(a.join("天板.json")));
    // フォルダごと移した物は、上のフォルダの記録をたどる(d/c を d/e へ移した)
    std::fs::rename(&c, d.join("e")).unwrap();
    std::fs::create_dir_all(d.join(".jc")).unwrap();
    std::fs::write(
        d.join(".jc/moved.json"),
        format!(
            r#"{{"moves":[{{"from":"c","to":"{}","at":1792700002}}]}}"#,
            at(&d.join("e"))
        ),
    )
    .unwrap();
    std::fs::remove_file(a.join("天板.json")).unwrap();
    assert_eq!(resolve(&link, None), Some(d.join("e").join("天板2.json")));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_snap_joint_between_touching_faces_keeps_the_part_where_it_is() {
    // 10-20 a5: 既に接している面どうしに合わせるジョイントを作っても、部品 1 を回して動かさない
    use joint::{Frame, touching_alignment};
    let r = 30f64.to_radians();
    let to = Frame {
        origin: [1.0, 2.0, 3.0],
        z: [0.0, 0.0, 1.0],
        x: [1.0, 0.0, 0.0],
    };
    for (z, flip) in [([0.0, 0.0, -1.0], false), ([0.0, 0.0, 1.0], true)] {
        let from = Frame {
            origin: to.origin,
            z,
            x: [r.cos(), r.sin(), 0.0],
        };
        let (a, f) = touching_alignment(&from, &to).expect("接している");
        assert_eq!(f, flip);
        // 合わせる行列(Joint::placement と同じ組み方)が動かさない
        let mut local = kmb_core::kernel::rotation([0.0; 3], [0.0, 0.0, 1.0], a.to_radians());
        if !f {
            local = joint::mul(
                &local,
                &kmb_core::kernel::rotation([0.0; 3], [1.0, 0.0, 0.0], std::f64::consts::PI),
            );
        }
        let m = joint::mul(
            &to.matrix(),
            &joint::mul(&local, &joint::rigid_inverse(&from.matrix())),
        );
        for (i, row) in m.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((v - want).abs() < 1e-9, "{a} {f}: {m:?}");
            }
        }
    }
    // 離れていれば今までどおりそろえる
    let apart = Frame {
        origin: [1.0, 2.0, 13.0],
        ..to
    };
    assert_eq!(touching_alignment(&apart, &to), None);
}
