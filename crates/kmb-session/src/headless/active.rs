//! アクティブな部品(作業する部品、F7): アクティブな部品がある間に作ったボディと、文書の直下に
//! 作った部品を、計算の後にその部品へ入れる。画面のブラウザの丸も CLI・MCP もここを通るので、
//! どこから頼んでも同じ部品に入る。アクティブな部品は開いている間だけの状態で、文書には入れない。
//! ボディを写す機能(ミラー・パターン・移動/コピーの写し)が作ったボディは、アクティブな部品より
//! 先に、元のボディの部品へ入れる。

use super::Headless;
use kmb_core::FeatureId;
use std::collections::HashMap;

/// アクティブな部品の状態。
#[derive(Debug, Clone, Default)]
pub(super) struct Active {
    /// アクティブな部品(`None` は文書の直下)
    pub(super) id: Option<u64>,
    /// アクティブな部品があった間に足した機能と部品(番号 → その時のアクティブな部品)。計算の後に
    /// ボディになった物と直下に作った部品を、その部品へ入れる
    pub(super) born_in: HashMap<u64, u64>,
    /// ここより前の番号は `born_in` に入れるか決めた(文書の `next_id` を追う)
    pub(super) born_from: u64,
    /// 入れている途中(入れる変更の再計算から、また入れに来ない)
    settling: bool,
}

/// [`Headless::settle_active`] の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// 何も変えなかった
    Nothing,
    /// 新しいボディ・部品をアクティブな部品へ入れた(数)
    Moved(usize),
    /// アクティブな部品が無くなったので、文書の直下に戻した
    Cleared,
}

impl Headless {
    /// アクティブな部品(`None` は文書の直下)。
    pub fn active_component(&self) -> Option<u64> {
        self.active.id
    }

    /// 部品 `id` をアクティブにする(`None` で文書の直下へ戻す)。これから作るボディと部品が、
    /// その部品に入る。無い部品は断る。変わったら真。
    pub fn activate_component(&mut self, id: Option<u64>) -> Result<bool, String> {
        let data = self.doc.data();
        let name = match id {
            Some(c) => match data.component(c) {
                Some(k) => Some(k.name.clone()),
                None => return Err(format!("部品 {c} がありません")),
            },
            None => None,
        };
        // ここまでに足した物は、前のアクティブのまま決める(入れるのは、これから作る物だけ)
        self.skip_born();
        if self.active.id == id {
            return Ok(false);
        }
        match name {
            Some(n) => log::info!("部品「{n}」をアクティブにした"),
            None => log::info!("文書の直下をアクティブにした"),
        }
        self.active.id = id;
        Ok(true)
    }

    /// 今までに割り当てた番号は、アクティブな部品へ入れない(複製した部品は作った部品のまま)。
    pub(crate) fn skip_born(&mut self) {
        self.active.born_from = self.active.born_from.max(self.doc.data().next_id);
    }

    /// 計算の後に、アクティブな部品があった間に足した物を、その部品へ入れる: 部品に入っていない
    /// 新しいボディと、文書の直下に作った新しい部品。直前の変更と同じ取り消し単位にする。
    /// CLI・MCP はその場の再計算の後に自分で呼ぶ。画面は裏の計算を取り込んだ後に呼ぶ。
    pub fn settle_active(&mut self) -> Settled {
        if self.active.settling {
            return Settled::Nothing;
        }
        let next = self.doc.data().next_id;
        let from = self.active.born_from;
        let copies = self.copies_since(from);
        let Some(active) = self.active.id else {
            self.active.born_from = self.active.born_from.max(next);
            return self.put_in(copies, "写しを元の部品へ入れる");
        };
        if self.doc.data().component(active).is_none() {
            // 部品を解いた・取り消しで部品が無くなった
            log::info!("アクティブな部品が無くなったので、文書の直下をアクティブに戻す");
            self.active.id = None;
            self.active.born_in.clear();
            self.active.born_from = self.active.born_from.max(next);
            return Settled::Cleared;
        }
        {
            let data = self.doc.data();
            let born = data
                .features
                .iter()
                .map(|f| f.id.0)
                .chain(data.components.iter().map(|c| c.id))
                .filter(|n| *n >= from);
            for n in born {
                self.active.born_in.insert(n, active);
            }
        }
        self.active.born_from = from.max(next);
        // 写しは元の部品へ入れるので、アクティブな部品へは入れない
        for (root, _) in &copies {
            self.active.born_in.remove(&root.0);
        }
        if self.active.born_in.is_empty() {
            return self.put_in(copies, "写しを元の部品へ入れる");
        }
        let data = self.doc.data();
        let reg = &self.modules.registry;
        let roots: Vec<FeatureId> = kmb_core::visible_bodies(data, reg, &self.report)
            .into_iter()
            .map(|(id, _)| kmb_core::body_root(data, reg, id))
            .collect();
        let mut list = data.components.clone();
        let mut moved = put_copies(&mut list, &copies);
        for root in roots {
            // ボディになった機能は、ここで決める(部品から外した後にまた入れない)
            let Some(to) = self.active.born_in.remove(&root.0) else {
                continue;
            };
            if data.component_of(root).is_some() {
                continue;
            }
            if let Some(c) = list.iter_mut().find(|c| c.id == to) {
                c.bodies.push(root);
                moved += 1;
            }
        }
        for c in &mut list {
            let Some(to) = self.active.born_in.remove(&c.id) else {
                continue;
            };
            if c.parent.is_none()
                && c.id != to
                && !data.component_tree(c.id).contains(&to)
                && data.component(to).is_some()
            {
                c.parent = Some(to);
                moved += 1;
            }
        }
        if moved == 0 || list == data.components {
            return Settled::Nothing;
        }
        let name = data
            .component(active)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        log::info!("新しいボディと部品 {moved} 個を、アクティブな部品「{name}」へ入れる");
        self.settle_list(list, moved, "アクティブな部品へ入れる")
    }

    /// 番号 `from` 以後に足した機能のうち、ボディを写す機能が作った見えているボディ(まだ部品に
    /// 入っていない物)と、元のボディの部品。元は、機能が受け継がずに残すボディ(`keeps`)か、
    /// ボディを読まない機能なら最初に読む機能(パターンの写しを分けた機能が読むパターン)の、
    /// 最初に作った機能でたどる。
    fn copies_since(&self, from: u64) -> Vec<(FeatureId, u64)> {
        let data = self.doc.data();
        let reg = &self.modules.registry;
        let source = |f: &kmb_core::FeatureEntry| -> Option<FeatureId> {
            let feature = reg.instantiate(&f.type_id, &f.data)?.ok()?;
            let inputs = feature.inputs().ok()?;
            let body = inputs.iter().find_map(|i| match i {
                kmb_core::InputRef::Output { feature, slot } if slot == "body" => Some(*feature),
                _ => None,
            });
            match body {
                Some(u) => feature.keeps().contains(&u).then_some(u),
                None => inputs.iter().find_map(|i| match i {
                    kmb_core::InputRef::Output { feature, .. } => Some(*feature),
                    _ => None,
                }),
            }
        };
        kmb_core::visible_bodies(data, reg, &self.report)
            .into_iter()
            .map(|(id, _)| kmb_core::body_root(data, reg, id))
            .filter(|root| root.0 >= from && data.component_of(*root).is_none())
            .filter_map(|root| {
                let src = source(data.feature(root)?)?;
                let home = data.component_of(kmb_core::body_root(data, reg, src))?;
                Some((root, home.id))
            })
            .collect()
    }

    /// 写しを元の部品へ入れる(アクティブな部品の無い時)。
    fn put_in(&mut self, copies: Vec<(FeatureId, u64)>, label: &str) -> Settled {
        let mut list = self.doc.data().components.clone();
        let moved = put_copies(&mut list, &copies);
        if moved == 0 {
            return Settled::Nothing;
        }
        self.settle_list(list, moved, label)
    }

    /// 部品の木を `list` にし、直前の変更と同じ取り消し単位にする。
    fn settle_list(
        &mut self,
        list: Vec<kmb_core::Component>,
        moved: usize,
        fallback: &str,
    ) -> Settled {
        let depth = self.doc.undo_depth();
        let label = self
            .doc
            .undo_labels()
            .last()
            .map_or_else(|| fallback.to_string(), |l| l.to_string());
        self.active.settling = true;
        let done = self.apply(crate::edit::set_components(&label, list));
        self.active.settling = false;
        if let Err(e) = done {
            log::warn!("アクティブな部品へ入れられなかった: {e}");
            return Settled::Nothing;
        }
        if depth > 0 {
            self.squash_since(depth - 1, &label);
        }
        Settled::Moved(moved)
    }
}

/// 写し(最初に作った機能、元の部品)を部品の木 `list` へ入れる。入れた数。
fn put_copies(list: &mut [kmb_core::Component], copies: &[(FeatureId, u64)]) -> usize {
    let mut moved = 0;
    for (root, to) in copies {
        if let Some(c) = list.iter_mut().find(|c| c.id == *to) {
            log::info!("写しのボディ {root} を元の部品「{}」へ入れる", c.name);
            c.bodies.push(*root);
            moved += 1;
        }
    }
    moved
}
