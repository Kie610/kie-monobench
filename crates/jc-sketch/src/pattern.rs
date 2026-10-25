//! 後から直せるパターン(円形状・矩形状)。定義([`PatternDef`])と、作った写しの番号([`PatternCopy`])を
//! [`Sketch::patterns`] に覚えておき、定義を直すと前の写しを消して作り直す
//! ([`Sketch::edit_pattern`])。並びは 1 つの拘束([`ConstraintKind::Pattern`])で、ソルバが写しを
//! 元を定義どおりに写した位置に保つ(元を動かすと写しが付いて来る。写しを
//! 引くと元も動く)。写しには拘束を写さない(元の寸法を変えると写しも追う。拘束の印が写しの
//! 数だけ増えない)。

use crate::arrange::rotate_about;
use crate::geom::{EPS, add, len, scale, sub};
use crate::model::{ConstraintId, ConstraintKind, Entity, EntityId, Sketch};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;
use std::fmt;

/// パターンの番号。スケッチの中でだけ一意。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PatternId(pub u32);

impl fmt::Display for PatternId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}", self.0)
    }
}

/// 円形状パターンの中心。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternCenter {
    /// スケッチの点(作り直す時の位置を使う)
    Point(EntityId),
    /// 座標(mm)
    At([f64; 2]),
}

/// 円形状パターンの角度の決め方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AngularSpacing {
    /// 総角度 `angle` に等分する。一周(360°)なら一周を `count` 等分、それより狭ければ両端を
    /// 含めて等分する
    #[default]
    Total,
    /// 1 つごとの角度が `angle`
    Step,
    /// 元の両側に対称に並べる。`count` は片側の数(元を含む)で、全部で `2 * count - 1` 個。
    /// `angle` は両側を合わせた総角度
    Symmetric,
}

/// 円形状パターンの定義。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircularPattern {
    /// 並べる元の要素(点・曲線)
    pub entities: Vec<EntityId>,
    pub center: PatternCenter,
    /// 数(元を含む)。決め方が対称なら片側の数
    pub count: usize,
    #[serde(default)]
    pub spacing: AngularSpacing,
    /// 角度(rad、反時計回りが正)。意味は [`AngularSpacing`] による
    pub angle: f64,
    /// 抑制した(作らない)写しの番号(元が 0、反時計回りに 1, 2, …、対称の時計回りの側は負)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suppressed: Vec<i32>,
}

/// 矩形状パターンの向き。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternAxis {
    /// X 軸から反時計回りの角度(rad)
    Angle(f64),
    /// 線分の向き(始点から終点へ。作り直す時の位置を使う)
    Line(EntityId),
}

/// 矩形状パターンの距離の型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistanceType {
    /// 隣どうしの間隔が `distance`
    #[default]
    Spacing,
    /// 端から端までの範囲が `distance`(対称なら両側を合わせた範囲)
    Extent,
}

/// 矩形状パターンの 1 方向。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PatternDirection {
    pub axis: PatternAxis,
    /// 数(元を含む)。1 ならこの方向には並べない。対称なら片側の数
    pub count: usize,
    /// 距離(mm)。負なら逆向き
    pub distance: f64,
    #[serde(default)]
    pub distance_type: DistanceType,
    /// 元の両側に並べる(片側の数が `count`、全部で `2 * count - 1` 列)
    #[serde(default, skip_serializing_if = "is_false")]
    pub symmetric: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 矩形状パターンの定義。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RectangularPattern {
    pub entities: Vec<EntityId>,
    pub directions: [PatternDirection; 2],
    /// 抑制した写しの番号(方向 1 と方向 2 の何番目か。元が [0, 0])
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suppressed: Vec<[i32; 2]>,
}

/// パターンの定義。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatternDef {
    Circular(CircularPattern),
    Rectangular(RectangularPattern),
}

impl PatternDef {
    /// 並べる元の要素。
    pub fn entities(&self) -> &[EntityId] {
        match self {
            PatternDef::Circular(c) => &c.entities,
            PatternDef::Rectangular(r) => &r.entities,
        }
    }

    fn entities_mut(&mut self) -> &mut Vec<EntityId> {
        match self {
            PatternDef::Circular(c) => &mut c.entities,
            PatternDef::Rectangular(r) => &mut r.entities,
        }
    }

    /// 写しの番号 `index` を抑制する。
    fn suppress(&mut self, index: [i32; 2]) {
        match self {
            PatternDef::Circular(c) => {
                if !c.suppressed.contains(&index[0]) {
                    c.suppressed.push(index[0]);
                }
            }
            PatternDef::Rectangular(r) => {
                if !r.suppressed.contains(&index) {
                    r.suppressed.push(index);
                }
            }
        }
    }
}

/// 作った写し 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternCopy {
    /// 写しの番号(円形状は [k, 0]、矩形状は [方向 1, 方向 2])
    pub index: [i32; 2],
    /// 写しの要素(点と曲線)
    pub entities: Vec<EntityId>,
    /// 写しの要素それぞれの元の要素(`entities` と同じ並び)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from: Vec<EntityId>,
    /// 古い書き方: 写しと一緒に作った拘束(元の拘束の写し)。読むと外す([`Sketch::upgrade`])
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<ConstraintId>,
}

/// 元の要素と写しの要素の組。
pub(crate) type Links = Vec<(EntityId, EntityId)>;

/// 写し 1 つの置き方(ソルバの式に使う)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Placement {
    /// 中心のまわりに角度(rad)だけ回す
    Turn(PatternCenter, f64),
    /// 向きそれぞれに、長さ(mm)だけずらす
    Shift([(PatternAxis, f64); 2]),
}

/// パターン(定義と、作った写し)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub id: PatternId,
    pub def: PatternDef,
    #[serde(default)]
    pub copies: Vec<PatternCopy>,
}

/// 写しの置き方。
enum Place {
    Shift([f64; 2]),
    Turn([f64; 2], f64),
}

/// 1 方向の並びの番号(元の 0 を含む)。
fn indices(count: usize, symmetric: bool) -> Vec<i32> {
    let n = count.max(1) as i32;
    if symmetric {
        (-(n - 1)..n).collect()
    } else {
        (0..n).collect()
    }
}

impl Sketch {
    /// パターンの定義。
    pub fn pattern(&self, id: PatternId) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id)
    }

    /// 要素が属するパターンと、写しならその番号(元の要素なら `None`)。どれにも属さなければ `None`。
    pub fn pattern_of(&self, entity: EntityId) -> Option<(PatternId, Option<[i32; 2]>)> {
        for p in &self.patterns {
            if let Some(c) = p.copies.iter().find(|c| c.entities.contains(&entity)) {
                return Some((p.id, Some(c.index)));
            }
        }
        self.patterns
            .iter()
            .find(|p| p.def.entities().contains(&entity))
            .map(|p| (p.id, None))
    }

    /// 円形状パターンを足す。写しを作り、定義を覚える。作れない(元がない、数が 0、角度が 0、
    /// 中心の点がない)なら何もせず `None`。
    pub fn add_circular_pattern(&mut self, def: CircularPattern) -> Option<PatternId> {
        self.add_pattern(PatternDef::Circular(def))
    }

    /// 矩形状パターンを足す。作れない(元がない、数が 0、距離が 0、向きの線分がない)なら `None`。
    pub fn add_rectangular_pattern(&mut self, def: RectangularPattern) -> Option<PatternId> {
        self.add_pattern(PatternDef::Rectangular(def))
    }

    /// パターンを足す([`Sketch::add_circular_pattern`]・[`Sketch::add_rectangular_pattern`])。
    /// 並びの拘束([`ConstraintKind::Pattern`])も付ける。
    pub fn add_pattern(&mut self, def: PatternDef) -> Option<PatternId> {
        let copies = self.generate(&def)?;
        let id = PatternId(self.patterns.iter().map(|p| p.id.0).max().unwrap_or(0) + 1);
        self.patterns.push(Pattern { id, def, copies });
        self.add_constraint(ConstraintKind::Pattern { pattern: id });
        Some(id)
    }

    /// 並びの拘束を付けたパターンの番号。
    pub fn pattern_constraint(&self, id: PatternId) -> Option<ConstraintId> {
        self.constraints
            .iter()
            .find(|c| matches!(c.kind, ConstraintKind::Pattern { pattern } if pattern == id))
            .map(|c| c.id)
    }

    /// パターンの定義を直す。前の写しを消して作り直す。同じ番号の写しが残る時は、写しに後から
    /// 付けた拘束・寸法と、写しの点につないだ図形を新しい写しへ付け替える。消える写しに関わる
    /// 拘束は落ち、消える写しの点を使う図形には同じ位置に新しい点を置く。作れない定義なら
    /// 何もせず偽を返す。
    pub fn edit_pattern(&mut self, id: PatternId, def: PatternDef) -> bool {
        let Some(at) = self.patterns.iter().position(|p| p.id == id) else {
            return false;
        };
        let old = self.patterns.remove(at);
        let Some(copies) = self.generate(&def) else {
            self.patterns.insert(at, old);
            return false;
        };
        // 同じ番号の写しの、同じ並びの要素どうしを対応させる
        let mut m: BTreeMap<EntityId, EntityId> = BTreeMap::new();
        for oc in &old.copies {
            if let Some(nc) = copies.iter().find(|c| c.index == oc.index)
                && nc.entities.len() == oc.entities.len()
            {
                m.extend(oc.entities.iter().copied().zip(nc.entities.iter().copied()));
            }
        }
        let old_ents: BTreeSet<EntityId> = old
            .copies
            .iter()
            .flat_map(|c| c.entities.iter().copied())
            .collect();
        let new_ents: BTreeSet<EntityId> = copies
            .iter()
            .flat_map(|c| c.entities.iter().copied())
            .collect();
        let own: BTreeSet<ConstraintId> = old
            .copies
            .iter()
            .flat_map(|c| c.constraints.iter().copied())
            .collect();
        // 写しの点を使うほかの図形は、新しい写しの点(なければ同じ位置の新しい点)へ付け替える
        let mut repoint = m.clone();
        let users: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|e| !old_ents.contains(&e.id()) && !new_ents.contains(&e.id()))
            .filter(|e| e.points().iter().any(|p| old_ents.contains(p)))
            .map(Entity::id)
            .collect();
        for u in &users {
            let pts = self.entity(*u).map(Entity::points).unwrap_or_default();
            for p in pts {
                if old_ents.contains(&p)
                    && !repoint.contains_key(&p)
                    && let Some(pos) = self.pos(p)
                {
                    let n = self.add_point(pos[0], pos[1]);
                    repoint.insert(p, n);
                }
            }
        }
        for e in &mut self.entities {
            if users.contains(&e.id()) {
                *e = e.with_points(e.id(), &|p| repoint.get(&p).copied().unwrap_or(p));
            }
        }
        // 写しに後から付けた拘束を付け替える(対応のない写しに関わるものは、消す時に落ちる)
        let all: BTreeMap<EntityId, EntityId> = self
            .entities
            .iter()
            .map(|e| (e.id(), m.get(&e.id()).copied().unwrap_or(e.id())))
            .collect();
        for c in &mut self.constraints {
            if own.contains(&c.id) {
                continue;
            }
            let r = crate::edit::refs(&c.kind);
            if !r.iter().any(|x| old_ents.contains(x)) || !r.iter().all(|x| all.contains_key(x)) {
                continue;
            }
            if r.iter().all(|x| !old_ents.contains(x) || m.contains_key(x))
                && let Some(k) = crate::arrange::remap(&c.kind, &all)
            {
                c.kind = k;
            }
        }
        let gone: Vec<EntityId> = old_ents.into_iter().collect();
        self.remove_entities(&gone);
        self.patterns.insert(at, Pattern { id, def, copies });
        true
    }

    /// パターンの定義を消す。`keep_copies` なら写しは普通の要素として残し、偽なら写しも消す。
    /// 並びの拘束も外す。パターンがなければ偽。
    pub fn remove_pattern(&mut self, id: PatternId, keep_copies: bool) -> bool {
        let Some(at) = self.patterns.iter().position(|p| p.id == id) else {
            return false;
        };
        let p = self.patterns.remove(at);
        self.constraints
            .retain(|c| !matches!(c.kind, ConstraintKind::Pattern { pattern } if pattern == id));
        if !keep_copies {
            let gone: Vec<EntityId> = p
                .copies
                .iter()
                .flat_map(|c| c.entities.iter().copied())
                .collect();
            self.remove_entities(&gone);
        }
        true
    }

    /// 定義から、写しの番号と置き方(抑制したものを除く。中心や向きは今の位置で決める)。
    /// 作れない定義なら `None`。
    fn placements(&self, def: &PatternDef) -> Option<Vec<([i32; 2], Place)>> {
        let out = self
            .layout(def)?
            .into_iter()
            .map(|(index, pl)| {
                let place = match pl {
                    Placement::Turn(c, a) => Place::Turn(
                        match c {
                            PatternCenter::Point(p) => self.pos(p)?,
                            PatternCenter::At(p) => p,
                        },
                        a,
                    ),
                    Placement::Shift(dirs) => {
                        let mut d = [0.0, 0.0];
                        for (axis, amount) in dirs {
                            d = add(d, scale(self.axis_unit(axis)?, amount));
                        }
                        Place::Shift(d)
                    }
                };
                Some((index, place))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(out)
    }

    /// 向きの単位ベクトル(今の位置)。線分が無いか長さが 0 なら `None`。
    fn axis_unit(&self, axis: PatternAxis) -> Option<[f64; 2]> {
        match axis {
            PatternAxis::Angle(a) => Some([a.cos(), a.sin()]),
            PatternAxis::Line(l) => {
                let Some(Entity::Line { a, b, .. }) = self.entity(l) else {
                    return None;
                };
                let v = sub(self.pos(*b)?, self.pos(*a)?);
                let n = len(v);
                (n >= EPS).then(|| scale(v, 1.0 / n))
            }
        }
    }

    /// 定義から、写しの番号と置き方(抑制したものを除く。中心の点や向きの線分は位置でなく
    /// 番号のまま)。数や角度が作れない定義なら `None`。
    pub(crate) fn layout(&self, def: &PatternDef) -> Option<Vec<([i32; 2], Placement)>> {
        match def {
            PatternDef::Circular(c) => {
                if let PatternCenter::Point(p) = c.center {
                    self.pos(p)?;
                }
                if c.count == 0 || !c.angle.is_finite() {
                    return None;
                }
                let n = c.count;
                if n > 1 && c.angle.abs() < 1e-9 {
                    return None;
                }
                let full = c.angle.abs() >= TAU - 1e-9;
                let (step, ks) = match c.spacing {
                    AngularSpacing::Total if full => (c.angle / n as f64, indices(n, false)),
                    AngularSpacing::Total => (c.angle / (n.max(2) - 1) as f64, indices(n, false)),
                    AngularSpacing::Step => (c.angle, indices(n, false)),
                    AngularSpacing::Symmetric => {
                        (c.angle / (2 * (n.max(2) - 1)) as f64, indices(n, true))
                    }
                };
                Some(
                    ks.into_iter()
                        .filter(|k| *k != 0 && !c.suppressed.contains(k))
                        .map(|k| ([k, 0], Placement::Turn(c.center, step * k as f64)))
                        .collect(),
                )
            }
            PatternDef::Rectangular(r) => {
                let mut dirs = vec![];
                for d in &r.directions {
                    if d.count == 0 || !d.distance.is_finite() {
                        return None;
                    }
                    self.axis_unit(d.axis)?;
                    let n = d.count;
                    if n > 1 && d.distance.abs() < 1e-9 {
                        return None;
                    }
                    let step = match (d.distance_type, d.symmetric) {
                        (DistanceType::Spacing, _) => d.distance,
                        (DistanceType::Extent, false) => d.distance / (n.max(2) - 1) as f64,
                        (DistanceType::Extent, true) => d.distance / (2 * (n.max(2) - 1)) as f64,
                    };
                    dirs.push((d.axis, step, indices(n, d.symmetric)));
                }
                let mut out = vec![];
                for &i in &dirs[0].2 {
                    for &j in &dirs[1].2 {
                        let index = [i, j];
                        if index == [0, 0] || r.suppressed.contains(&index) {
                            continue;
                        }
                        out.push((
                            index,
                            Placement::Shift([
                                (dirs[0].0, dirs[0].1 * i as f64),
                                (dirs[1].0, dirs[1].1 * j as f64),
                            ]),
                        ));
                    }
                }
                Some(out)
            }
        }
    }

    /// 定義どおりに写しを作る。作れない定義なら何もせず `None`。
    fn generate(&mut self, def: &PatternDef) -> Option<Vec<PatternCopy>> {
        let ids: Vec<EntityId> = def
            .entities()
            .iter()
            .copied()
            .filter(|e| self.entity(*e).is_some())
            .collect();
        if ids.is_empty() {
            return None;
        }
        let places = self.placements(def)?;
        let from = self.copy_sources(&ids);
        let mut out = vec![];
        for (index, place) in places {
            let before = self.constraints.iter().map(|c| c.id.0).max().unwrap_or(0);
            let entities = match place {
                Place::Shift(d) => self.copy_mapped(&ids, &|p| add(p, d), 0.0),
                Place::Turn(c, a) => self.copy_mapped(&ids, &rotate_about(c, a), a),
            };
            // 写しの形は並びの拘束が元から決めるので、写した拘束は要らない
            self.constraints.retain(|c| c.id.0 <= before);
            out.push(PatternCopy {
                index,
                from: if from.len() == entities.len() {
                    from.clone()
                } else {
                    vec![]
                },
                entities,
                constraints: vec![],
            });
        }
        Some(out)
    }

    /// [`Sketch::copy_mapped`] が写す要素の、写す前の要素(写しと同じ並び: 点、続いて曲線)。
    fn copy_sources(&self, ids: &[EntityId]) -> Vec<EntityId> {
        let mut v: Vec<EntityId> = self
            .points_of(ids)
            .into_iter()
            .filter(|p| self.pos(*p).is_some())
            .collect();
        v.extend(
            ids.iter()
                .copied()
                .filter(|id| self.entity(*id).is_some_and(Entity::is_curve)),
        );
        v
    }

    /// 古い書き方のパターンを読み替える([`Sketch::upgrade`])。写しに写した拘束を外し、
    /// 写しと元の対応を付け、並びの拘束を足す。
    pub(crate) fn patterns_upgrade(&mut self) {
        if self.patterns.is_empty() {
            return;
        }
        let mut owned: BTreeSet<ConstraintId> = BTreeSet::new();
        let mut patterns = std::mem::take(&mut self.patterns);
        for p in &mut patterns {
            let ids: Vec<EntityId> = p
                .def
                .entities()
                .iter()
                .copied()
                .filter(|e| self.entity(*e).is_some())
                .collect();
            let from = self.copy_sources(&ids);
            for c in &mut p.copies {
                owned.extend(c.constraints.drain(..));
                if c.from.len() != c.entities.len() && from.len() == c.entities.len() {
                    c.from = from.clone();
                }
            }
        }
        self.constraints.retain(|c| !owned.contains(&c.id));
        let ids: Vec<PatternId> = patterns.iter().map(|p| p.id).collect();
        self.patterns = patterns;
        for id in ids {
            if self.pattern_constraint(id).is_none() {
                self.add_constraint(ConstraintKind::Pattern { pattern: id });
            }
        }
        // 写しが消えて並びのなくなった拘束
        let live: BTreeSet<PatternId> = self.patterns.iter().map(|p| p.id).collect();
        self.constraints.retain(
            |c| !matches!(c.kind, ConstraintKind::Pattern { pattern } if !live.contains(&pattern)),
        );
    }

    /// 並びの拘束の式に使う、写しごとの置き方と(元の要素, 写しの要素)の組。
    pub(crate) fn pattern_links(&self, id: PatternId) -> Option<Vec<(Placement, Links)>> {
        let p = self.pattern(id)?;
        let layout = self.layout(&p.def)?;
        Some(
            p.copies
                .iter()
                .filter_map(|c| {
                    let (_, place) = layout.iter().find(|(i, _)| *i == c.index)?;
                    let pairs: Vec<(EntityId, EntityId)> = c
                        .from
                        .iter()
                        .copied()
                        .zip(c.entities.iter().copied())
                        .collect();
                    (!pairs.is_empty()).then_some((*place, pairs))
                })
                .collect(),
        )
    }

    /// 要素を消す時に、パターンの定義を付いて行かせる([`Sketch::remove_entities`] から、
    /// 要素を消す前に呼ぶ)。中心の点が消えれば座標に、向きの線分が消えれば角度にする。
    /// 写しがまるごと消えたら、その番号を抑制にする。元がなくなった定義は消す(写しは残る)。
    pub(crate) fn patterns_forget(&mut self, gone: &BTreeSet<EntityId>) {
        if self.patterns.is_empty() {
            return;
        }
        let mut patterns = std::mem::take(&mut self.patterns);
        for p in &mut patterns {
            match &mut p.def {
                PatternDef::Circular(c) => {
                    if let PatternCenter::Point(q) = c.center
                        && gone.contains(&q)
                    {
                        c.center = PatternCenter::At(self.pos(q).unwrap_or([0.0, 0.0]));
                    }
                }
                PatternDef::Rectangular(r) => {
                    for d in &mut r.directions {
                        if let PatternAxis::Line(l) = d.axis
                            && gone.contains(&l)
                        {
                            let v = match self.entity(l) {
                                Some(Entity::Line { a, b, .. }) => self
                                    .pos(*a)
                                    .zip(self.pos(*b))
                                    .map(|(a, b)| sub(b, a))
                                    .unwrap_or([1.0, 0.0]),
                                _ => [1.0, 0.0],
                            };
                            d.axis = PatternAxis::Angle(v[1].atan2(v[0]));
                        }
                    }
                }
            }
            p.def.entities_mut().retain(|e| !gone.contains(e));
            let mut emptied = vec![];
            for c in &mut p.copies {
                let had = !c.entities.is_empty();
                if c.from.len() == c.entities.len() {
                    // 元が消えた写しの要素は、写しから外れて普通の要素として残る
                    let (from, entities) = c
                        .from
                        .iter()
                        .copied()
                        .zip(c.entities.iter().copied())
                        .filter(|(f, e)| !gone.contains(f) && !gone.contains(e))
                        .unzip();
                    (c.from, c.entities) = (from, entities);
                } else {
                    c.entities.retain(|e| !gone.contains(e));
                }
                if had && c.entities.is_empty() {
                    emptied.push(c.index);
                }
            }
            p.copies.retain(|c| !c.entities.is_empty());
            for i in emptied {
                p.def.suppress(i);
            }
        }
        patterns.retain(|p| !p.def.entities().is_empty());
        let live: BTreeSet<PatternId> = patterns.iter().map(|p| p.id).collect();
        self.constraints.retain(
            |c| !matches!(c.kind, ConstraintKind::Pattern { pattern } if !live.contains(&pattern)),
        );
        self.patterns = patterns;
    }

    /// 番号を詰めた時に、パターンの中の番号も付け替える([`Sketch::renumber_after`] から呼ぶ)。
    pub(crate) fn patterns_renumber(
        &mut self,
        m: &BTreeMap<EntityId, EntityId>,
        cm: &BTreeMap<u32, u32>,
    ) {
        let g = |id: &mut EntityId| {
            if let Some(n) = m.get(id) {
                *id = *n;
            }
        };
        for p in &mut self.patterns {
            p.def.entities_mut().iter_mut().for_each(g);
            match &mut p.def {
                PatternDef::Circular(c) => {
                    if let PatternCenter::Point(q) = &mut c.center {
                        g(q);
                    }
                }
                PatternDef::Rectangular(r) => {
                    for d in &mut r.directions {
                        if let PatternAxis::Line(l) = &mut d.axis {
                            g(l);
                        }
                    }
                }
            }
            for c in &mut p.copies {
                c.entities.iter_mut().for_each(g);
                c.from.iter_mut().for_each(g);
                for k in &mut c.constraints {
                    if let Some(n) = cm.get(&k.0) {
                        k.0 = *n;
                    }
                }
            }
        }
    }
}
