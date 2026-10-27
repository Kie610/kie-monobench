//! 出現([`Kernel::instance`])の表。カーネルが持ち、出現の元と行列を覚える。

use super::*;
use std::collections::HashMap;
use std::sync::Mutex;

/// 行列を表の鍵にする(数の並びのビット)。
fn bits(m: &Matrix4) -> [u64; 12] {
    let mut b = [0; 12];
    for (i, v) in m.iter().take(3).flatten().enumerate() {
        b[i] = v.to_bits();
    }
    b
}

/// カーネルが作った出現の表: 出現 → (元の形, 行列)と、(元の形, 行列) → 出現。
/// 同じ元を同じ行列で置くと同じ出現を返す(再計算のたびに作り直さない)。
#[derive(Debug, Default)]
pub struct Instances {
    of: HashMap<ShapeHandle, (ShapeHandle, Matrix4)>,
    made: HashMap<(ShapeHandle, [u64; 12]), ShapeHandle>,
}

impl Instances {
    /// 出現 `h` の元と行列。出現でなければ `None`。
    pub fn of(&self, h: ShapeHandle) -> Option<(ShapeHandle, Matrix4)> {
        self.of.get(&h).copied()
    }

    /// `live` に、そこにある出現の元を足した物。表からは `live` に無い出現を捨てる。
    /// カーネルの [`Kernel::retain`] はこれで残す形を決める(出現が生きている間は元も残す)。
    pub fn retain(&mut self, live: &HashSet<ShapeHandle>) -> HashSet<ShapeHandle> {
        self.of.retain(|h, _| live.contains(h));
        self.made.retain(|_, h| live.contains(h));
        let mut out = live.clone();
        out.extend(self.of.values().map(|(b, _)| *b));
        out
    }
}

/// [`Kernel::instance`] の中身。行列を確かめ、出現の出現を 1 段に畳み、同じ置き方なら前の出現を
/// 返す。新しく作る時は `copy`(元を行列で写して動かす。面の名前はそのまま)で形を作って表に記す。
/// 畳んだ行列が単位行列なら元をそのまま返す。
pub fn place_instance(
    table: &Mutex<Instances>,
    shape: ShapeHandle,
    m: &Matrix4,
    copy: impl FnOnce(ShapeHandle, &Matrix4) -> Result<ShapeHandle, KernelError>,
) -> Result<ShapeHandle, KernelError> {
    if !is_rigid(m) {
        return Err(KernelError::Failed(
            placement_problem(m)
                .unwrap_or("出現の行列は回転と鏡映と平行移動だけです(拡大縮小は使えません)")
                .into(),
        ));
    }
    let lock = || table.lock().unwrap_or_else(|e| e.into_inner());
    let (base, m) = {
        let t = lock();
        let (base, m) = match t.of(shape) {
            Some((b, inner)) => (b, mat_mul(m, &inner)),
            None => (shape, *m),
        };
        if near_identity(&m) {
            return Ok(base);
        }
        if let Some(h) = t.made.get(&(base, bits(&m))) {
            return Ok(*h);
        }
        (base, m)
    };
    // 写す間は表を離す(カーネルが中で出現を問うことがある)
    let h = copy(base, &m)?;
    let mut t = lock();
    t.of.insert(h, (base, m));
    t.made.insert((base, bits(&m)), h);
    Ok(h)
}
