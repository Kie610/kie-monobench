//! 要素の番号で作る連結成分。拘束の向きや距離に優先の順を付けない。
use crate::EntityId;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Groups(BTreeMap<EntityId, EntityId>);

impl Groups {
    pub fn root(&mut self, id: EntityId) -> EntityId {
        let mut root = id;
        while let Some(&parent) = self.0.get(&root) {
            if parent == root {
                break;
            }
            root = parent;
        }
        let mut next = id;
        while let Some(parent) = self.0.insert(next, root) {
            if parent == root {
                break;
            }
            next = parent;
        }
        root
    }

    pub fn join(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        let mut first = None;
        for id in ids {
            let root = self.root(id);
            if let Some(a) = first {
                let a = self.root(a);
                let low = a.min(root);
                self.0.insert(a.max(root), low);
                first = Some(low);
            } else {
                first = Some(root);
            }
        }
    }
}
