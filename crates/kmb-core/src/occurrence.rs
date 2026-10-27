//! 出現(同じ形を別の位置に置いたボディ。[`crate::Feature::instance_of`])の読み方。
//!
//! 出現は形を写さず、元のボディの形を置き直して読む。機能 R が出現 O を読む時は、元 S の R の
//! 手前の最新の形を、O の R の手前の位置に置いた形を渡す(元を後ろで直しても付いて来る)。
//! 元を動かした分(ジョイント・移動)は出現に移さない: 形ごとに「定義の座標から動かした行列」を
//! 覚え、元の形はその逆で定義の座標に戻してから置く。どれも行列の掛け算と
//! [`crate::Kernel::instance`] だけで、B-rep を作らない。
//!
//! 文書に出現が無ければ何もしない(今までと同じ計算になる)。

use crate::bodies::{BodyId, BodyState, BodyTable};
use crate::feature::{InputRef, Outputs};
use crate::kernel::{IDENTITY, Matrix4, mat_mul, rigid_inverse};
use crate::recompute::{FeatureResult, FeatureStatus, MOVED};
use crate::{DocumentData, FeatureId, Kernel, Registry, ShapeHandle, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// 機能が読む出現の値(読む形 → 渡す値)。
pub(crate) type Placed = BTreeMap<BodyState, Value>;

/// 1 回の再計算の間の、出現の読み方の状態。
#[derive(Default)]
pub(crate) struct Placing {
    on: bool,
    /// 形ごとの、定義の座標から動かした行列(動かしただけの操作を掛け合わせた物)
    moved: HashMap<ShapeHandle, Matrix4>,
}

fn shape_in(outputs: &HashMap<FeatureId, Outputs>, s: &BodyState) -> Option<ShapeHandle> {
    match outputs.get(&s.0)?.get(&s.1)? {
        Value::Shape(h) => Some(*h),
        _ => None,
    }
}

/// 形の元と行列(出現でなければ自分と単位行列)。
fn base_of(kernel: &dyn Kernel, h: ShapeHandle) -> (ShapeHandle, Matrix4) {
    kernel.instance_of(h).unwrap_or((h, IDENTITY))
}

fn kernel_error(e: crate::KernelError) -> FeatureStatus {
    FeatureStatus::Error {
        message: format!("出現を置けませんでした({e})"),
    }
}

fn blocked(data: &DocumentData, f: FeatureId) -> FeatureStatus {
    let name = data.feature(f).map_or(String::new(), |e| e.name.clone());
    FeatureStatus::Blocked {
        by: format!("{f} {name}"),
        feature: Some(f),
    }
}

impl Placing {
    pub(crate) fn new(bodies: &BodyTable) -> Self {
        Placing {
            on: bodies.occurrences().next().is_some(),
            moved: HashMap::new(),
        }
    }

    /// 文書に出現があるか。無ければほかの関数は何もしない。
    pub(crate) fn active(&self) -> bool {
        self.on
    }

    fn moved(&self, h: ShapeHandle) -> Matrix4 {
        self.moved.get(&h).copied().unwrap_or(IDENTITY)
    }

    /// 形 `v` を定義の座標に戻した形(動かした分を戻す)。
    fn definition(
        &mut self,
        kernel: &dyn Kernel,
        v: ShapeHandle,
    ) -> Result<ShapeHandle, FeatureStatus> {
        let (base, m) = base_of(kernel, v);
        let h = kernel
            .instance(base, &mat_mul(&rigid_inverse(&self.moved(v)), &m))
            .map_err(kernel_error)?;
        self.moved.entry(h).or_insert(IDENTITY);
        Ok(h)
    }

    /// 元の形 `v` を、出現の今の形 `at` の位置に置いた形。
    fn replace(
        &mut self,
        kernel: &dyn Kernel,
        v: ShapeHandle,
        at: ShapeHandle,
    ) -> Result<ShapeHandle, FeatureStatus> {
        let (base, m) = base_of(kernel, v);
        let pos = self.moved(at);
        let place = mat_mul(&pos, &mat_mul(&rigid_inverse(&self.moved(v)), &m));
        let h = kernel.instance(base, &place).map_err(kernel_error)?;
        self.moved.insert(h, pos);
        Ok(h)
    }

    /// 出現 `occ` の元の、位置 `index` の手前での最新の形と、その前に足された機能。
    fn source_at(
        bodies: &BodyTable,
        occ: BodyId,
        index: usize,
    ) -> Option<(BodyState, Option<FeatureId>)> {
        let src = bodies.line(occ)?.of?;
        bodies.own_tip_at(src, index)
    }

    /// 機能 `id`(タイムラインの位置 `index`)が読む出現の値。入力の `inputs` は機能の宣言のまま、
    /// `instance_of` は機能の [`crate::Feature::instance_of`]。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn inputs(
        &mut self,
        kernel: &dyn Kernel,
        data: &DocumentData,
        bodies: &BodyTable,
        id: FeatureId,
        index: usize,
        inputs: &[InputRef],
        instance_of: Option<&InputRef>,
        outputs: &HashMap<FeatureId, Outputs>,
    ) -> Result<Placed, FeatureStatus> {
        let mut placed = Placed::new();
        if !self.on {
            return Ok(placed);
        }
        for input in inputs {
            let InputRef::Output { feature, slot } = bodies.resolve(id, input) else {
                continue;
            };
            let s: BodyState = (feature, slot);
            if BodyId::part_of_slot(&s.1).is_none() {
                continue;
            }
            // 出現そのもの: 元の定義の座標の形を渡す
            if instance_of == Some(input) {
                let Some((src, _)) = Self::source_at(bodies, BodyId::new(id), index) else {
                    continue;
                };
                let v = shape_in(outputs, &src).ok_or_else(|| blocked(data, src.0))?;
                let h = self.definition(kernel, v)?;
                placed.insert(s, Value::Shape(h));
                continue;
            }
            // 出現を読む: 元の今の形を出現の位置に置く
            let Some(occ) = bodies
                .body_of_state(&s)
                .filter(|b| bodies.line(*b).is_some_and(|l| l.of.is_some()))
            else {
                continue;
            };
            let (Some((src, _)), Some(at)) =
                (Self::source_at(bodies, occ, index), shape_in(outputs, &s))
            else {
                continue;
            };
            let v = shape_in(outputs, &src).ok_or_else(|| blocked(data, src.0))?;
            let h = self.replace(kernel, v, at)?;
            placed.insert(s, Value::Shape(h));
        }
        Ok(placed)
    }

    /// 出現の値をキャッシュの鍵に加える(作るたびに番号の変わる形でなく、元と行列で)。
    pub(crate) fn hash(kernel: &dyn Kernel, placed: &Placed, hasher: &mut impl Hasher) {
        for (s, v) in placed {
            s.hash(hasher);
            if let Value::Shape(h) = v {
                let (base, m) = base_of(kernel, *h);
                base.0.hash(hasher);
                for x in m.iter().flatten() {
                    x.to_bits().hash(hasher);
                }
            }
        }
    }

    /// 機能 `id` を計算した後(使い回した時も)、出した形の動かした行列を覚える。出現の形を変える
    /// 機能(丸め・穴など)なら誤りの文を返す。`outputs` は機能の結果を入れる前の物、`source` は
    /// 出現の機能が読んだ元の形([`crate::Feature::instance_of`] を解いた物)。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn after(
        &mut self,
        kernel: &dyn Kernel,
        data: &DocumentData,
        registry: &Registry,
        bodies: &BodyTable,
        id: FeatureId,
        placed: &Placed,
        source: Option<BodyState>,
        outputs: &HashMap<FeatureId, Outputs>,
        out: &Outputs,
    ) -> Result<(), String> {
        if !self.on {
            return Ok(());
        }
        let read = |s: &BodyState| match placed.get(s) {
            Some(Value::Shape(h)) => Some(*h),
            _ => shape_in(outputs, s),
        };
        // 受け継いだボディ(出現そのものは、渡した定義の座標の形)
        let from = bodies.heir_of(id).cloned().or(source);
        if let (Some(from), Some(Value::Shape(h))) = (from, out.get("body"))
            && let Some(r) = read(&from)
        {
            let (bh, mh) = base_of(kernel, *h);
            let (br, mr) = base_of(kernel, r);
            let body = bodies.body_of(id);
            let occ =
                body.filter(|b| b.root != id && bodies.line(*b).is_some_and(|l| l.of.is_some()));
            if let Some(occ) = occ
                && bh != br
            {
                let names = crate::body_labels(data, registry, &[occ.root]);
                let src = bodies.line(occ).and_then(|l| l.of).map(|s| {
                    data.component_of(s.root)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| crate::body_labels(data, registry, &[s.root]).concat())
                });
                return Err(format!(
                    "出現『{}』の形は元の部品『{}』で直します(この機能は出現の形を変えます)",
                    names.concat(),
                    src.unwrap_or_default()
                ));
            }
            let m = if bh == br {
                mat_mul(&mat_mul(&mh, &rigid_inverse(&mr)), &self.moved(r))
            } else {
                self.moved(r)
            };
            self.moved.insert(*h, m);
        }
        // 動かした別のボディ(ジョイント。[`MOVED`])
        for (slot, v) in out {
            let (Some(b), Value::Shape(h)) = (
                slot.strip_prefix(MOVED).and_then(|n| n.parse::<u64>().ok()),
                v,
            ) else {
                continue;
            };
            let InputRef::Output { feature, slot } = bodies.resolve(
                id,
                &InputRef::Output {
                    feature: FeatureId(b),
                    slot: "body".into(),
                },
            ) else {
                continue;
            };
            let Some(r) = read(&(feature, slot)) else {
                continue;
            };
            let ((bh, mh), (br, mr)) = (base_of(kernel, *h), base_of(kernel, r));
            let m = if bh == br {
                mat_mul(&mat_mul(&mh, &rigid_inverse(&mr)), &self.moved(r))
            } else {
                self.moved(r)
            };
            self.moved.insert(*h, m);
        }
        Ok(())
    }

    /// 計算の終わりに、出現の最後の形を、元の最後の形を出現の位置に置いた形にする(見えるボディ・
    /// 部品表・干渉が読む)。元が結合で無くなっていれば、無くなる手前の形で置いて出現に警告を出す。
    /// 置いた形を返す(カーネルに残させる)。
    pub(crate) fn finish(
        &mut self,
        kernel: &dyn Kernel,
        data: &DocumentData,
        bodies: &BodyTable,
        outputs: &mut HashMap<FeatureId, Outputs>,
        features: &mut [FeatureResult],
    ) -> HashSet<ShapeHandle> {
        let mut made = HashSet::new();
        if !self.on {
            return made;
        }
        let occs: Vec<BodyId> = bodies.occurrences().map(|l| l.id).collect();
        for occ in occs {
            let (Some((tip, _)), Some((src, merged))) = (
                bodies.own_tip_at(occ, usize::MAX),
                Self::source_at(bodies, occ, usize::MAX),
            ) else {
                continue;
            };
            let (Some(at), Some(v)) = (shape_in(outputs, &tip), shape_in(outputs, &src)) else {
                continue;
            };
            let h = match self.replace(kernel, v, at) {
                Ok(h) => h,
                Err(e) => {
                    log::warn!("出現 {occ} を最後の形に置けませんでした: {e:?}");
                    continue;
                }
            };
            made.insert(h);
            if let Some(o) = outputs.get_mut(&tip.0) {
                o.insert(tip.1.clone(), Value::Shape(h));
            }
            if let Some(r) = features.iter_mut().rev().find(|r| r.id == tip.0)
                && let FeatureStatus::Ok { outputs: o, .. } = &mut r.status
            {
                o.insert(tip.1.clone(), Value::Shape(h));
            }
            if let Some(by) = merged
                && let Some(r) = features.iter_mut().find(|r| r.id == occ.root)
                && let FeatureStatus::Ok { outputs: o, .. } = &mut r.status
            {
                let name = data.feature(by).map_or(by.to_string(), |e| e.name.clone());
                crate::add_warnings(
                    o,
                    vec![format!(
                        "元のボディは機能「{name}」で別のボディに足されたので、その手前の形で置いています"
                    )],
                );
            }
        }
        made
    }
}
