//! 描く物: ボディの網と面の境目の線、色・外観・材質・光源・隠す・薄くする・光らせるの状態、拾う時の箱の木。

use super::*;

/// 表示するボディの網と、面の境目の線。
pub struct SceneBody {
    pub feature: FeatureId,
    pub mesh: Mesh,
    /// 面のボディは裏からも描く。測定用の網は片面のまま保つ。
    pub two_sided: bool,
    /// 元の網の全ての辺が逆向きに対を持つ。断面の拾いに使い、帯と剛体移動では保つ。
    pub(super) section_closed_boundary: bool,
    pub(super) edges: Vec<[[f32; 3]; 2]>,
    /// 面ごとの決まった色(`mesh.face_names` の順。`None` と足りない面はボディの色)
    pub colors: Vec<Option<Color32>>,
    /// ボディの外観の色(材料・外観を付けたボディ。M26)。`None` なら既定の色を明るさで見分ける
    pub tint: Option<Color32>,
    /// ボディの材質(外観・材料を付けたボディ。木目の芯はこのボディの箱に合わせてある)
    pub look: Option<kmb_render::Look>,
    /// 面の外観の材質(`mesh.face_names` の順。`None` と足りない面はボディの色と材質。木目の芯は
    /// このボディの箱に合わせてある)
    pub face_looks: Vec<Option<kmb_render::Look>>,
    /// 面を拾うための三角形の箱の木(大きな網は別のスレッドで先に作り、小さな網は初めて拾う時に作る)
    pub(super) bvh: std::sync::Arc<PickTree>,
    /// アニメーションで動かしている時の、動かす前の網と動き([`Scene::set_poses`])
    rest: Option<(Mesh, kmb_core::kernel::Matrix4)>,
    /// ねじの陰影の帯に切る前の網(ねじのあるボディだけ。[`Scene::set_thread_marks`])
    plain: Option<Mesh>,
}

#[derive(Default)]
pub struct Scene {
    /// 表示しているボディ(隠したものは入らない。描く・拾う・全体に合わせるのはこれだけ)
    pub bodies: Vec<SceneBody>,
    /// 隠したボディの網(表示に戻す時に作り直さないよう取っておく)
    stowed: Vec<SceneBody>,
    /// 隠すボディ(画面の状態。文書には入れない)
    hide: HashSet<FeatureId>,
    key: Vec<(FeatureId, ShapeHandle)>,
    /// 作り直すたびに増える。描いた絵を使い回すかの判断に使う
    pub(super) generation: u64,
    pub(super) min: [f32; 3],
    pub(super) max: [f32; 3],
    /// 網の細かさ(面からのずれの上限 mm。`None` は [`kmb_measure::MESH_TOLERANCE`]。U26)
    tolerance: Option<f64>,
    /// 断面で切って見る平面(M48。画面の状態で、形は変えない)
    pub section: Option<Section>,
    /// ボディの外観の色([`SceneBody::tint`]。網を作り直しても残す)
    body_colors: std::collections::BTreeMap<FeatureId, Color32>,
    /// ボディの材質([`SceneBody::look`]。網を作り直しても残す)
    body_looks: std::collections::BTreeMap<FeatureId, kmb_render::Look>,
    /// 面の外観の材質(ボディの最後の機能 → 面の名前 → 材質。[`SceneBody::face_looks`]。網を作り直しても残す)
    face_looks: FaceLooks,
    /// 写実の見た目(画面が毎フレーム入れる)
    pub effects: crate::Effects,
    /// アニメーションのボディ(最後の機能)ごとの動き(画面の状態。網を作り直しても掛け直す)
    poses: std::collections::BTreeMap<FeatureId, kmb_core::kernel::Matrix4>,
    /// 淡く描くボディ(最後の機能。部品をアクティブにした時の、ほかの部品のボディ。画面の状態)
    faded: std::collections::BTreeSet<FeatureId>,
    /// しばらく光らせるボディ(最後の機能)、寄せる色、強さの段([`GLOW_STEPS`] 段。画面の状態)
    glow: Option<(std::collections::BTreeSet<FeatureId>, Color32, u8)>,
    /// 文書の点光源とスポットライト(写実の見た目とキャンバス内レンダリングで照らす)
    pub(super) lamps: Vec<kmb_render::lamp::Lamp>,
    /// ねじの陰影の印(ボディの最後の機能 → 印。網を作り直しても掛け直す)
    threads: ThreadMarks,
}

/// ボディ(最後の機能)ごとの、ねじの陰影の印。
pub type ThreadMarks = std::collections::BTreeMap<FeatureId, Vec<kmb_interchange::ThreadMark>>;

/// ボディ(最後の機能)ごとの、面の名前 → 面の外観の材質。
pub type FaceLooks =
    std::collections::BTreeMap<FeatureId, std::collections::BTreeMap<String, kmb_render::Look>>;

/// 光らせる強さの段の数(段が替わる時だけ面の色を作り直す)。
pub(super) const GLOW_STEPS: f32 = 6.0;
/// いちばん強く光らせる時に、面の色を光の色へ寄せる割合。
pub(super) const GLOW_MIX: f32 = 0.6;

/// 淡く描くボディの色を白(暗い画面は背景)へ寄せる割合。
pub(super) const FADE: f32 = 0.7;
/// 淡く描くボディの陰を平らにする割合(GPU のシェーダーの 254 と同じ値)。
pub(super) const FADE_FLAT: f32 = 0.7;

/// 質感で描かない面(外観の無いボディ・選んだ面など)の光。明るさの倍率は
/// 環境の光 + 主の光 + 視点からの光で、主の光は視点の向きに世界の左上の向きを混ぜる
/// (どの向きから見ても上の面が明るく、横の面との差が付く)。白いつやを足し、平らな面の中には
/// 映り込みの濃淡(手前が明るく奥が暗い)を付ける。GPU のシェーダーの `flat_shade` と同じ式。
#[derive(Clone, Copy, Debug)]
pub struct FlatLight {
    /// 視点の向き(注視点から視点へ)
    pub eye: [f32; 3],
    /// 主の光の向き
    pub key: [f32; 3],
    /// 濃淡の元にする仮の視点の中心と距離(mm)
    pub center: [f32; 3],
    pub dist: f32,
}

/// 環境の光・主の光・視点からの光の強さ。
pub const FLAT_AMBIENT: f32 = 0.42;
pub const FLAT_KEY: f32 = 0.38;
pub const FLAT_FRONT: f32 = 0.25;
/// 白いつや(Blinn-Phong の指数と強さ)
pub const FLAT_SHINE: (f32, f32) = (40.0, 0.25);
/// 平らな面の中の濃淡の強さと、仮の視点までの距離(場面の半径の倍数)
pub const FLAT_ENV: (f32, f32) = (0.4, 2.4);

impl FlatLight {
    /// 視点の向き `eye`、回す中心 `center`、場面の半径 `reach`(mm)の光。
    pub fn new(eye: [f32; 3], center: [f32; 3], reach: f32) -> FlatLight {
        let key = normalize([eye[0] * 0.8 - 0.3, eye[1] * 0.8 + 0.2, eye[2] * 0.8 + 0.6]);
        FlatLight {
            eye,
            key,
            center,
            dist: reach.max(1e-3) * FLAT_ENV.1,
        }
    }

    /// 環境マップで引く時の、面の点 `p` から視点への向き。平行投影でも、面の中の濃淡の仮の視点へ
    /// 向ける(平らな面の明るさが視点を回した時に急に変わらない。GPU の `env_view` と同じ)。
    pub fn env_view(&self, p: [f32; 3], eye_pos: Option<[f32; 3]>) -> [f32; 3] {
        if let Some(eye_pos) = eye_pos {
            return normalize(sub(eye_pos, p));
        }
        let d = sub(p, self.center);
        let along = dot(d, self.eye);
        let lat = [0, 1, 2].map(|k| d[k] - self.eye[k] * along);
        normalize([0, 1, 2].map(|k| self.eye[k] * self.dist - lat[k]))
    }

    /// 位置 `p`・法線 `n` の面の明るさの倍率と、足す白いつや(0〜1)。`vis` は環境光の遮蔽の
    /// 見える割合(1 は遮られていない)。
    pub fn shade(&self, p: [f32; 3], n: [f32; 3], vis: f32) -> (f32, f32) {
        let mut n = normalize(n);
        if dot(n, self.eye) < 0.0 {
            n = n.map(|v| -v);
        }
        let diff = dot(n, self.key).max(0.0);
        let front = dot(n, self.eye).max(0.0);
        let h = normalize([
            self.key[0] + self.eye[0],
            self.key[1] + self.eye[1],
            self.key[2] + self.eye[2],
        ]);
        let shine = dot(n, h).max(0.0).powf(FLAT_SHINE.0) * FLAT_SHINE.1;
        // 映り込みの濃淡: 画面の上の位置から仮の視点へ向かう向きで反射を見て、平行投影の向きで
        // 見た時との差だけを足す(面ごとの平均の明るさは変えない)
        let d = sub(p, self.center);
        let along = dot(d, self.eye);
        let lat = [0, 1, 2].map(|k| d[k] - self.eye[k] * along);
        let vv = normalize([0, 1, 2].map(|k| self.eye[k] * self.dist - lat[k]));
        let rz = |v: [f32; 3]| 2.0 * dot(n, v) * n[2] - v[2];
        let env = (smooth01(rz(vv)) - smooth01(rz(self.eye))) * FLAT_ENV.0;
        let k = FLAT_AMBIENT + FLAT_KEY * diff + FLAT_FRONT * front + env
            - (FLAT_AMBIENT + FLAT_FRONT * front) * (1.0 - vis);
        (k, shine)
    }
}

/// 色の明るさ(0〜255、BT.601)。
pub fn luma(c: Color32) -> f32 {
    0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b())
}

/// 0〜1 のなめらかな段(smoothstep(0, 1, x))。
pub(super) fn smooth01(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// ボディの稜線の色(ボディの色に応じて、暗い画面は白い側へ、明るい画面は暗い側へ寄せる)。
/// ただし暗い画面の明るいボディ(白・アルミ・黄など)は暗い側へ、明るい画面の暗いボディ(黒)は
/// 白い側へ寄せる(画面の明暗だけで決めると、暗い画面の白いボディでは線と面がほぼ同じ明るさになって
/// 辺が消える)。
pub fn line_color(body: Color32, dark: bool) -> Color32 {
    let light_line = if dark {
        luma(body) < 190.0
    } else {
        luma(body) < 80.0
    };
    if light_line {
        crate::mix(body, Color32::WHITE, 0.55)
    } else {
        crate::mix(body, Color32::from_rgb(40, 44, 52), 0.75)
    }
}

/// 断面の平面。`normal` の向きの側(`dot(p, normal) > offset`)を描かず、拾わない。
/// 切り口は斜めの縞で塗る(M11)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    /// 長さ 1 の法線
    pub normal: [f32; 3],
    pub offset: f32,
    /// 切り口をボディごとの色で塗る(偽なら全部のボディで同じ [`cap_color`])
    pub by_body: bool,
    /// ボディの外側も切り口と同じ色で塗る(`by_body` の時だけ効く)
    pub outside: bool,
}

/// 断面でボディごとに塗る色(外観の色の無いボディに、並びの順に割り当てる)。
pub const SECTION_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x5b, 0x9b, 0xd5),
    Color32::from_rgb(0xe0, 0x8a, 0x4a),
    Color32::from_rgb(0x6a, 0xb0, 0x6a),
    Color32::from_rgb(0xc8, 0x6a, 0xb8),
    Color32::from_rgb(0xd6, 0xc0, 0x4a),
    Color32::from_rgb(0x4a, 0xb8, 0xb0),
    Color32::from_rgb(0xc8, 0x5a, 0x5a),
    Color32::from_rgb(0x8a, 0x7a, 0xd0),
];

/// 切り口の縞の間隔と太さ(point。画面に対して 45 度の向き)。
pub const HATCH_GAP: f32 = 6.0;
pub const HATCH_WIDTH: f32 = 1.4;

/// 画素 (`x`, `y`)(画素の真ん中の位置)が切り口の縞にかかる割合(0〜1。縁はなめらかにする)。
/// GPU のシェーダーの `hatch` と同じ式。
pub fn hatch(x: f32, y: f32, ppp: f32) -> f32 {
    let t = (x + y) / std::f32::consts::SQRT_2 / ppp.max(1e-3);
    let d = ((t / HATCH_GAP).fract() - 0.5).abs() * HATCH_GAP;
    ((HATCH_WIDTH / 2.0 - d) * ppp + 0.5).clamp(0.0, 1.0)
}

/// 切り口の裏の面を手前へ寄せる量(稜線を寄せる量に対する割合)。2 つのボディが接する面では、
/// 片方の切り口の裏の面ともう片方の表の面がちょうど重なり、どちらが手前か決まらず縞が乱れていた。
pub const CAP_BIAS: f32 = 0.3;

/// 切り口の縞の色(地の色を暗くする)。
pub fn hatch_color(cap: Color32) -> Color32 {
    shade(cap, 0.6)
}

impl Section {
    /// 点 `p` が切り取って見えない側にあるか。
    pub fn removes(&self, p: [f32; 3]) -> bool {
        dot(p, self.normal) > self.offset
    }

    pub(super) fn hash_into(&self, h: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.normal.map(f32::to_bits).hash(h);
        self.offset.to_bits().hash(h);
        (self.by_body, self.outside).hash(h);
    }
}

impl Scene {
    /// ボディが前と同じなら作り直さない。
    pub fn update(&mut self, kernel: &dyn Kernel, bodies: &[(FeatureId, ShapeHandle)]) -> bool {
        if !self.needs(bodies) {
            return false;
        }
        self.set(bodies, build_meshes_at(kernel, bodies, self.tolerance()));
        self.set_body_kinds(kernel);
        true
    }

    /// 作った網のボディの種類を反映する。裏側の三角形を測定用の網へ混ぜない。
    pub fn set_body_kinds(&mut self, kernel: &dyn Kernel) {
        let mut changed = false;
        for b in self.bodies.iter_mut().chain(&mut self.stowed) {
            let two_sided = self
                .key
                .iter()
                .find(|(f, _)| *f == b.feature)
                .is_some_and(|(_, h)| {
                    kernel.body_kind(*h).ok() == Some(kmb_core::kernel::BodyKind::Sheet)
                });
            changed |= b.two_sided != two_sided;
            b.two_sided = two_sided;
        }
        if changed {
            self.sort_hidden();
        }
    }

    /// 網の細かさ(mm)。
    pub fn tolerance(&self) -> f64 {
        self.tolerance.unwrap_or(kmb_measure::MESH_TOLERANCE)
    }

    /// 網の細かさを替える。替わったら今のボディの網を作り直して真。
    pub fn set_tolerance(&mut self, kernel: &dyn Kernel, tolerance: f64) -> bool {
        if (self.tolerance() - tolerance).abs() < 1e-12 {
            return false;
        }
        self.tolerance = Some(tolerance);
        let bodies = self.key.clone();
        log::info!("網の細かさを {tolerance} mm にして作り直す");
        self.set(&bodies, build_meshes_at(kernel, &bodies, tolerance));
        self.set_body_kinds(kernel);
        true
    }

    /// 今表示しているボディ。
    pub fn key(&self) -> &[(FeatureId, ShapeHandle)] {
        &self.key
    }

    /// `bodies` を表示するのに網を作り直す必要があるか。
    pub fn needs(&self, bodies: &[(FeatureId, ShapeHandle)]) -> bool {
        self.key != bodies
    }

    /// 作っておいた網に置き換える。
    pub fn set(&mut self, bodies: &[(FeatureId, ShapeHandle)], meshes: Vec<(FeatureId, Mesh)>) {
        self.key = bodies.to_vec();
        self.stowed.clear();
        self.bodies = meshes
            .into_iter()
            .map(|(feature, mesh)| {
                let section_closed_boundary = has_closed_boundary(&mesh);
                let (mesh, plain) = banded(mesh, self.threads.get(&feature));
                (feature, mesh, plain, section_closed_boundary)
            })
            .map(
                |(feature, mesh, plain, section_closed_boundary)| SceneBody {
                    feature,
                    two_sided: false,
                    section_closed_boundary,
                    edges: feature_edges(&mesh),
                    look: self.body_looks.get(&feature).map(|l| fit_look(l, &mesh)),
                    face_looks: fit_face_looks(self.face_looks.get(&feature), &mesh),
                    mesh,
                    colors: Vec::new(),
                    tint: self.body_colors.get(&feature).copied(),
                    bvh: Default::default(),
                    rest: None,
                    plain,
                },
            )
            .collect();
        self.apply_poses();
        for b in &self.bodies {
            b.prepare_bvh();
        }
        self.sort_hidden();
    }

    /// アニメーションでボディ(最後の機能)を動かして見せる(形は変えない。空なら動かす前に戻す)。
    /// 描く・拾う・全体に合わせるのは動かした網。変わったら真。
    pub fn set_poses(
        &mut self,
        poses: std::collections::BTreeMap<FeatureId, kmb_core::kernel::Matrix4>,
    ) -> bool {
        if self.poses == poses {
            return false;
        }
        self.poses = poses;
        self.apply_poses();
        for b in &self.bodies {
            b.prepare_bvh();
        }
        self.sort_hidden();
        true
    }

    /// 今の動き([`Self::set_poses`])を網に掛ける(動きの変わったボディだけ)。
    fn apply_poses(&mut self) {
        for b in self.bodies.iter_mut().chain(self.stowed.iter_mut()) {
            let want = self.poses.get(&b.feature).copied();
            if b.rest.as_ref().map(|(_, m)| *m) == want {
                continue;
            }
            let rest = match b.rest.take() {
                Some((m, _)) => m,
                None => std::mem::take(&mut b.mesh),
            };
            match want {
                Some(m) => {
                    b.mesh = kmb_interchange::animation::transform_mesh(&rest, &m);
                    b.rest = Some((rest, m));
                }
                None => b.mesh = rest,
            }
            b.edges = feature_edges(&b.mesh);
            b.look = self
                .body_looks
                .get(&b.feature)
                .map(|l| fit_look(l, &b.mesh));
            b.face_looks = fit_face_looks(self.face_looks.get(&b.feature), &b.mesh);
            b.bvh = Default::default();
        }
    }

    /// ねじの陰影の印を替える(ボディの最後の機能 → 印)。印の替わったボディだけ網を帯に切り直す。
    /// 変わったら真。
    pub fn set_thread_marks(&mut self, marks: ThreadMarks) -> bool {
        if self.threads == marks {
            return false;
        }
        for b in self.bodies.iter_mut().chain(self.stowed.iter_mut()) {
            if self.threads.get(&b.feature) == marks.get(&b.feature) {
                continue;
            }
            // 帯に切る前の網(動かしていれば動かす前の網)から切り直し、動きは後で掛け直す
            let rest = b.rest.take().map(|(m, _)| m);
            let base = match (b.plain.take(), rest) {
                (Some(p), _) => p,
                (None, Some(r)) => r,
                (None, None) => std::mem::take(&mut b.mesh),
            };
            let (mesh, plain) = banded(base, marks.get(&b.feature));
            b.mesh = mesh;
            b.plain = plain;
            b.edges = feature_edges(&b.mesh);
            b.look = self
                .body_looks
                .get(&b.feature)
                .map(|l| fit_look(l, &b.mesh));
            b.face_looks = fit_face_looks(self.face_looks.get(&b.feature), &b.mesh);
            b.bvh = Default::default();
        }
        self.threads = marks;
        self.apply_poses();
        for b in &self.bodies {
            b.prepare_bvh();
        }
        self.sort_hidden();
        true
    }

    /// アニメーションで動かしているか。
    pub fn is_posed(&self) -> bool {
        !self.poses.is_empty()
    }

    /// 最初のボディの面ごとの色を決める(加工の途中の形の塗り分け。色の役は `inks` で色にする)。
    pub fn set_face_colors(&mut self, colors: &[Option<kmb_core::ShapeInk>], inks: &Inks) {
        if let Some(b) = self.bodies.first_mut() {
            b.colors = colors.iter().map(|c| c.map(|c| inks.shape(c))).collect();
        }
    }

    /// ボディ(最後の機能の ID)ごとの外観の色を替える(網は作り直さない)。無いボディは既定の色。
    /// 変わったら真。
    pub fn set_body_colors(
        &mut self,
        colors: std::collections::BTreeMap<FeatureId, Color32>,
    ) -> bool {
        if self.body_colors == colors {
            return false;
        }
        for b in self.bodies.iter_mut().chain(self.stowed.iter_mut()) {
            b.tint = colors.get(&b.feature).copied();
        }
        self.body_colors = colors;
        true
    }

    /// ボディ(最後の機能の ID)ごとの外観の色([`Self::set_body_colors`] で替えた物)。
    pub fn body_colors(&self) -> &std::collections::BTreeMap<FeatureId, Color32> {
        &self.body_colors
    }

    /// ボディ(最後の機能の ID)ごとの材質を替える(網は作り直さない)。無いボディは材質なし。
    /// 変わったら真。
    pub fn set_body_looks(
        &mut self,
        looks: std::collections::BTreeMap<FeatureId, kmb_render::Look>,
    ) -> bool {
        if self.body_looks == looks {
            return false;
        }
        for b in self.bodies.iter_mut().chain(self.stowed.iter_mut()) {
            b.look = looks.get(&b.feature).map(|l| fit_look(l, &b.mesh));
        }
        self.body_looks = looks;
        true
    }

    /// ボディ(最後の機能の ID)の網(隠したボディも。動かしている時は動かした後の網)。
    pub fn mesh_of(&self, feature: FeatureId) -> Option<&Mesh> {
        self.bodies
            .iter()
            .chain(self.stowed.iter())
            .find(|b| b.feature == feature)
            .map(|b| &b.mesh)
    }

    /// 点光源とスポットライトを替える。変わったら真。
    pub fn set_lamps(&mut self, lamps: Vec<kmb_render::lamp::Lamp>) -> bool {
        if self.lamps == lamps {
            return false;
        }
        self.lamps = lamps;
        true
    }

    /// 点光源とスポットライト。
    pub fn lamps(&self) -> &[kmb_render::lamp::Lamp] {
        &self.lamps
    }

    /// ボディ(最後の機能の ID)ごとの面の外観の材質を替える(網は作り直さない)。変わったら真。
    pub fn set_face_looks(&mut self, looks: FaceLooks) -> bool {
        if self.face_looks == looks {
            return false;
        }
        for b in self.bodies.iter_mut().chain(self.stowed.iter_mut()) {
            b.face_looks = fit_face_looks(looks.get(&b.feature), &b.mesh);
        }
        self.face_looks = looks;
        true
    }

    /// 表示している `index` 番目のボディの `face` 番目の面を写実で描く時の面の外観の材質
    /// (面の外観が無いか、写実で描かないなら `None`)。
    pub fn face_look(&self, index: usize, face: usize) -> Option<&kmb_render::Look> {
        let fx = self.effects;
        if !fx.materials
            || self.section.is_some_and(|s| s.by_body && s.outside)
            || self.is_faded(index)
            || self.glows(index)
        {
            return None;
        }
        self.bodies.get(index)?.face_looks.get(face)?.as_ref()
    }

    /// 表示している `index` 番目のボディの `face` 番目の面を塗る、面の外観の色(面の外観が無いか、
    /// 断面でボディごとに塗り分けている時は `None`)。淡く描く・光らせるのはボディの色と同じ。
    pub fn face_tint(&self, index: usize, face: usize, colors: &Colors) -> Option<Color32> {
        if self.section.is_some_and(|s| s.by_body && s.outside) {
            return None;
        }
        let b = self.bodies.get(index)?;
        let l = b.face_looks.get(face)?.as_ref()?;
        let c = Color32::from_rgb(l.color[0], l.color[1], l.color[2]);
        if self.faded.contains(&b.feature) {
            return Some(self.fade_with(c, colors));
        }
        Some(self.glow_tint(index, c))
    }

    /// 表示している `index` 番目のボディを写実で描く時の材質(写実で描かないなら `None`)。
    /// 材質の無いボディは、レンダリングの作業空間では外観の色の塗装、外観も無ければ既定の材料
    /// (鋼)の質感にする(書き出すレンダリングと同じ)。ほかの作業空間では質感で描かず、
    /// [`FlatLight`] の光で描く(既定の色は鋼の灰色)。
    pub fn body_look(&self, index: usize, colors: &Colors) -> Option<kmb_render::Look> {
        let fx = self.effects;
        if !fx.materials || self.section.is_some_and(|s| s.by_body && s.outside) {
            return None;
        }
        let b = self.bodies.get(index)?;
        if self.faded.contains(&b.feature) {
            return None;
        }
        match &b.look {
            Some(l) => Some(l.clone()),
            None if fx.all_bodies => Some(self.default_look(index, colors)),
            None => None,
        }
    }

    /// 写実で描く一式(表示しているボディ。材質の無いボディは外観の色の塗装か既定の材料)。断面は無視する。
    pub fn render_scene(&self, colors: &Colors) -> kmb_render::RenderScene {
        let objects = self
            .bodies
            .iter()
            .enumerate()
            .flat_map(|(i, b)| {
                let look = b
                    .look
                    .clone()
                    .unwrap_or_else(|| self.default_look(i, colors));
                // ボディの材質だけでなく、面の外観とデカールも描く
                kmb_render::Object::with_faces(b.mesh.clone(), look, &b.face_looks)
            })
            .collect();
        kmb_render::RenderScene::new(objects).with_lamps(self.lamps.clone())
    }

    /// 材質の無いボディを写実で描く材質: 外観の色があればその塗装、無ければ既定の材料
    /// (既定の灰色の塗装で描くと、書き出すレンダリング(既定の材料の鋼)と見た目が違ってしまう)。
    fn default_look(&self, index: usize, colors: &Colors) -> kmb_render::Look {
        match self.bodies.get(index).and_then(|b| b.tint) {
            Some(_) => {
                let c = self.body_color(index, colors);
                kmb_render::Look::plain([c.r(), c.g(), c.b()])
            }
            None => kmb_render::Look::named(None, Some(kmb_look::material::DEFAULT_MATERIAL)),
        }
    }

    /// 淡く描く色: 背景へ寄せる。明るい背景はボディの灰色と近く、背景へ寄せても変わらないので白へ寄せる。
    fn fade_with(&self, c: Color32, colors: &Colors) -> Color32 {
        let bg = colors.background;
        let light = u32::from(bg.r()) + u32::from(bg.g()) + u32::from(bg.b()) > 3 * 128;
        crate::mix(c, if light { Color32::WHITE } else { bg }, FADE)
    }

    /// 表示している `index` 番目のボディを塗る色(外観の色か、既定の色を明るさで見分けた色)。
    /// 断面で外側も塗り分ける時は、切り口と同じ色。既定の鋼を環境マップで描くボディは、順番で
    /// 見分けずに全部同じ鋼の色(Kie 2026-10-21。金属は地の色を掛け算するので、
    /// 2 つ目の濃い灰色が黒っぽく沈んだ)。
    pub fn body_color(&self, index: usize, colors: &Colors) -> Color32 {
        if self.section.is_some_and(|s| s.by_body && s.outside) {
            return self.section_color(index);
        }
        if self.steel_env(index) {
            return colors.body;
        }
        if self.glows(index) {
            let c = self
                .bodies
                .get(index)
                .and_then(|b| b.tint)
                .unwrap_or(colors.body);
            return self.glow_tint(index, c);
        }
        let c = self
            .bodies
            .get(index)
            .and_then(|b| b.tint)
            .unwrap_or_else(|| body_tint(colors.body, colors.edge, index));
        match self.bodies.get(index) {
            // 淡く描くボディは色を抜く(形は見えるが目立たない)
            Some(b) if self.faded.contains(&b.feature) => self.fade_with(c, colors),
            _ => c,
        }
    }

    /// しばらく光らせるボディ(最後の機能)と色・強さ(0〜1)を替える(網は作り直さない)。強さは
    /// 段に丸め、段が替わった時だけ真(面の色を作り直す回数を抑える)。空か 0 で光らせない。
    pub fn set_glow(
        &mut self,
        bodies: std::collections::BTreeSet<FeatureId>,
        color: Color32,
        amount: f32,
    ) -> bool {
        let step = (amount.clamp(0.0, 1.0) * GLOW_STEPS).ceil() as u8;
        let glow = (!bodies.is_empty() && step > 0).then_some((bodies, color, step));
        if self.glow == glow {
            return false;
        }
        self.glow = glow;
        true
    }

    /// 表示している `index` 番目のボディを今光らせているか。
    pub fn glows(&self, index: usize) -> bool {
        self.glow_amount(index).is_some()
    }

    fn glow_amount(&self, index: usize) -> Option<(Color32, f32)> {
        let (set, color, step) = self.glow.as_ref()?;
        let b = self.bodies.get(index)?;
        set.contains(&b.feature)
            .then_some((*color, f32::from(*step) / GLOW_STEPS))
    }

    /// 面の色 `c` を、光らせているボディなら光の色へ寄せる(面ごとの決まった色にも掛ける)。
    pub fn glow_tint(&self, index: usize, c: Color32) -> Color32 {
        match self.glow_amount(index) {
            Some((g, k)) => crate::mix(c, g, GLOW_MIX * k),
            None => c,
        }
    }

    /// 光らせているボディ・色・段(描いた絵を使い回すかの指紋に入れる)。
    pub fn glow(&self) -> Option<&(std::collections::BTreeSet<FeatureId>, Color32, u8)> {
        self.glow.as_ref()
    }

    /// 淡く描くボディ(最後の機能)を替える(網は作り直さない)。変わったら真。
    pub fn set_faded(&mut self, faded: std::collections::BTreeSet<FeatureId>) -> bool {
        if self.faded == faded {
            return false;
        }
        self.faded = faded;
        true
    }

    /// 表示している `index` 番目のボディを、既定の鋼として環境マップの映り込みで描くか(設計の画面で、
    /// 外観も材料の質感も無く、淡くも断面の塗り分けでもないボディ。Kie 2026-10-21)。
    pub fn steel_env(&self, index: usize) -> bool {
        let fx = self.effects;
        if fx.all_bodies || !fx.materials || self.section.is_some_and(|s| s.by_body && s.outside) {
            return false;
        }
        self.bodies.get(index).is_some_and(|b| {
            b.look.is_none() && b.tint.is_none() && !self.faded.contains(&b.feature)
        }) && !self.glows(index)
    }

    /// 表示している `index` 番目のボディを淡く描くか。
    pub fn is_faded(&self, index: usize) -> bool {
        self.bodies
            .get(index)
            .is_some_and(|b| self.faded.contains(&b.feature))
    }

    /// 淡く描くボディ(最後の機能)。
    pub fn faded(&self) -> &std::collections::BTreeSet<FeatureId> {
        &self.faded
    }

    /// 表示している `index` 番目のボディの切り口の地の色(縞は [`hatch_color`])。
    pub fn cap_color(&self, index: usize, colors: &Colors) -> Color32 {
        match self.section {
            Some(s) if s.by_body => self.section_color(index),
            _ => cap_color(colors),
        }
    }

    /// 断面でボディごとに塗る色。外観の色があればその色、無ければ文書の中の並びで決まる色
    /// (隠したボディがあっても色は変わらない)。
    fn section_color(&self, index: usize) -> Color32 {
        let Some(b) = self.bodies.get(index) else {
            return SECTION_COLORS[0];
        };
        b.tint.unwrap_or_else(|| {
            let k = self
                .key
                .iter()
                .position(|k| k.0 == b.feature)
                .unwrap_or(index);
            SECTION_COLORS[k % SECTION_COLORS.len()]
        })
    }

    /// 隠すボディを替える(網は作り直さない)。変わったら真。
    pub fn set_hidden(&mut self, hide: &HashSet<FeatureId>) -> bool {
        if self.hide == *hide {
            return false;
        }
        self.hide = hide.clone();
        self.sort_hidden();
        true
    }

    /// 隠すボディを `stowed` へ、見せるボディを `bodies` へ分け(元の順を保つ)、外形を測り直す。
    fn sort_hidden(&mut self) {
        let mut all: Vec<SceneBody> = std::mem::take(&mut self.bodies);
        all.append(&mut self.stowed);
        let order = |f: FeatureId| self.key.iter().position(|k| k.0 == f).unwrap_or(usize::MAX);
        all.sort_by_key(|b| order(b.feature));
        let (stowed, shown): (Vec<_>, Vec<_>) = all
            .into_iter()
            .partition(|b| self.hide.contains(&b.feature));
        self.bodies = shown;
        self.stowed = stowed;
        // 番号はどの場面とも重ならない(GPU に置いた網をこの番号で探す)
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.generation = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for b in &self.bodies {
            for p in &b.mesh.positions {
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
        }
        self.min = min;
        self.max = max;
    }

    pub fn is_empty(&self) -> bool {
        self.bodies.iter().all(|b| b.mesh.triangles.is_empty())
    }

    /// 網を作り直したり隠すボディを替えたりするたびに変わる番号(ほかの場面とも重ならない)。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 表示しているボディの外形(軸に平行な箱)。ボディが無ければ最小が最大より大きい。
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        (self.min, self.max)
    }
}

/// 材質の木目の芯を、ボディの網の箱に合わせる。
/// 面の外観の材質を、網の面の並びにし、木目の芯をボディの箱に合わせる。
pub(super) fn fit_face_looks(
    looks: Option<&std::collections::BTreeMap<String, kmb_render::Look>>,
    mesh: &Mesh,
) -> Vec<Option<kmb_render::Look>> {
    let Some(looks) = looks.filter(|l| !l.is_empty()) else {
        return Vec::new();
    };
    mesh.face_names
        .iter()
        .enumerate()
        .map(|(i, n)| looks.get(n).map(|l| fit_look(l, mesh).fit_decal(mesh, i)))
        .collect()
}

pub(super) fn fit_look(l: &kmb_render::Look, mesh: &Mesh) -> kmb_render::Look {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    if min[0] > max[0] {
        return l.clone();
    }
    l.clone().frame_for(min, max)
}

/// 面を拾うための箱の木と、別のスレッドで作り始めたか。
#[derive(Default)]
pub(super) struct PickTree {
    pub(super) tree: std::sync::OnceLock<crate::pick_bvh::Bvh>,
    /// 網の点を囲む箱(視線が通らないボディは木を作らずに飛ばす。同じ部品を何百個置いても、
    /// 初めて拾う時に全部の木を作らない)
    pub(super) bounds: std::sync::OnceLock<([f32; 3], [f32; 3])>,
    pub(super) started: std::sync::atomic::AtomicBool,
}

/// これより小さな網の箱の木は、初めて拾う時にその場で作っても気にならない(数 ms)。
pub(super) const BIG_MESH: usize = 20_000;

impl SceneBody {
    /// 面の境目の線分。
    pub fn edge_segments(&self) -> &[[[f32; 3]; 2]] {
        &self.edges
    }

    /// 面を拾うための箱の木(網を作り直すまで使い回す)。大きな網の木を別のスレッドで作っている間は
    /// `None`(拾う側は木を使わず三角形を全部調べる)。出来上がるまで画面のスレッドが待つと、
    /// 大きな形を開いた直後にマウスを動かした時に画面が止まる。
    fn bvh(&self) -> Option<&crate::pick_bvh::Bvh> {
        if let Some(t) = self.bvh.tree.get() {
            return Some(t);
        }
        if self.mesh.triangles.len() < BIG_MESH {
            return Some(self.bvh.tree.get_or_init(|| build_bvh(&self.mesh)));
        }
        // 隠していた間に作り直した網など、まだ作り始めていなければここで始める
        self.prepare_bvh();
        None
    }

    /// 視線と交わる三角形ごとに `hit` を呼ぶ(木が出来ていれば木を通し、無ければ全部調べる)。
    pub(super) fn ray(&self, o: [f32; 3], d: [f32; 3], t_min: f32, hit: impl FnMut(u32, f32)) {
        let (min, max) = *self.bvh.bounds.get_or_init(|| {
            self.mesh.positions.iter().fold(
                ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
                |(lo, hi), p| {
                    (
                        [0, 1, 2].map(|k| lo[k].min(p[k])),
                        [0, 1, 2].map(|k| hi[k].max(p[k])),
                    )
                },
            )
        });
        if !crate::pick_bvh::ray_hits_box(min, max, o, d, t_min) {
            return;
        }
        match self.bvh() {
            Some(t) => t.ray(&self.mesh, o, d, t_min, hit),
            None => crate::pick_bvh::Bvh::ray_all(&self.mesh, o, d, t_min, hit),
        }
    }

    /// 大きな網の箱の木を、別のスレッドで作り始める(形を変えた直後のマウスの動きを止めない)。
    /// 1 つの網につき 1 回だけ。
    fn prepare_bvh(&self) {
        if self.mesh.triangles.len() < BIG_MESH
            || self
                .bvh
                .started
                .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let cell = self.bvh.clone();
        let mesh = Mesh {
            positions: self.mesh.positions.clone(),
            triangles: self.mesh.triangles.clone(),
            ..Mesh::default()
        };
        std::thread::spawn(move || {
            cell.tree.get_or_init(|| build_bvh(&mesh));
        });
    }
}

/// 同じ座標の点を合わせ、境界が向き付きで閉じた網だけを認める。
/// 許容差で穴を塞がず、不明な接続は開いた網と同じ扱いにする。
fn has_closed_boundary(mesh: &Mesh) -> bool {
    use std::collections::HashMap;
    if mesh.triangles.is_empty() {
        return false;
    }
    let mut points = HashMap::with_capacity(mesh.positions.len());
    let mut ids = Vec::with_capacity(mesh.positions.len());
    for p in &mesh.positions {
        if p.iter().any(|v| !v.is_finite()) {
            return false;
        }
        let key = p.map(|v| if v == 0.0 { 0 } else { v.to_bits() });
        let next = points.len();
        ids.push(*points.entry(key).or_insert(next));
    }
    let mut faces = Vec::with_capacity(mesh.triangles.len());
    let mut edges = Vec::with_capacity(mesh.triangles.len() * 3);
    for tri in &mesh.triangles {
        let mut vertices = [0; 3];
        for i in 0..3 {
            let Some(&id) = ids.get(tri[i] as usize) else {
                return false;
            };
            vertices[i] = id;
        }
        if vertices[0] == vertices[1] || vertices[1] == vertices[2] || vertices[2] == vertices[0] {
            return false;
        }
        let [a, b, c] = tri.map(|i| mesh.positions[i as usize]);
        let normal = cross(sub(b, a), sub(c, a));
        if normal == [0.0; 3] || normal.iter().any(|v| !v.is_finite()) {
            return false;
        }
        let mut face = vertices;
        face.sort_unstable();
        faces.push(face);
        for [a, b] in [
            [vertices[0], vertices[1]],
            [vertices[1], vertices[2]],
            [vertices[2], vertices[0]],
        ] {
            edges.push(([a.min(b), a.max(b)], a < b));
        }
    }
    faces.sort_unstable();
    if faces.windows(2).any(|pair| pair[0] == pair[1]) {
        return false;
    }
    edges.sort_unstable_by_key(|edge| edge.0);
    let (pairs, remainder) = edges.as_chunks::<2>();
    let mut previous = None;
    for pair in pairs {
        if pair[0].0 != pair[1].0 || pair[0].1 == pair[1].1 || previous == Some(pair[0].0) {
            return false;
        }
        previous = Some(pair[0].0);
    }
    remainder.is_empty()
}

/// ねじの印 `marks` の円筒を帯に切った網と、切る前の網(切った時だけ)。
fn banded(mesh: Mesh, marks: Option<&Vec<kmb_interchange::ThreadMark>>) -> (Mesh, Option<Mesh>) {
    match marks.and_then(|m| super::thread_bands(&mesh, m)) {
        Some(b) => (b, Some(mesh)),
        None => (mesh, None),
    }
}

pub(super) fn build_bvh(m: &Mesh) -> crate::pick_bvh::Bvh {
    let started = std::time::Instant::now();
    let b = crate::pick_bvh::Bvh::build(m);
    log::debug!(
        "面を拾う箱の木: 三角形 {} 個、{:.1} ms",
        m.triangles.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    b
}

/// 表示用の網を作る(測る計算と同じ許容差 [`kmb_measure::MESH_TOLERANCE`])。別のスレッドからも呼べる。
pub fn build_meshes(
    kernel: &dyn Kernel,
    bodies: &[(FeatureId, ShapeHandle)],
) -> Vec<(FeatureId, Mesh)> {
    kmb_measure::meshes(kernel, bodies)
}

/// 表示用の網を、許容差 `tolerance`(mm)で作る(描画の設定の「網の細かさ」)。
pub fn build_meshes_at(
    kernel: &dyn Kernel,
    bodies: &[(FeatureId, ShapeHandle)],
    tolerance: f64,
) -> Vec<(FeatureId, Mesh)> {
    kmb_measure::meshes_at(kernel, bodies, tolerance)
}

/// 測る計算は、表示しているボディの網で測る。
impl kmb_measure::Meshes for Scene {
    fn mesh(&self, feature: FeatureId) -> Option<&Mesh> {
        self.bodies
            .iter()
            .find(|b| b.feature == feature)
            .map(|b| &b.mesh)
    }
}

/// 稜線の折れ線を線分に分ける。
pub(super) fn feature_edges(m: &Mesh) -> Vec<[[f32; 3]; 2]> {
    m.edges
        .iter()
        .flat_map(|l| l.windows(2).map(|w| [w[0], w[1]]))
        .collect()
}

#[cfg(test)]
mod sheet_tests {
    use super::*;
    #[test]
    fn sheet_back_is_visible_without_doubling_the_measurement_mesh() {
        struct SheetKernel;
        impl Kernel for SheetKernel {
            fn name(&self) -> &str {
                "sheet-test"
            }
            fn body_kind(
                &self,
                _: ShapeHandle,
            ) -> Result<kmb_core::kernel::BodyKind, kmb_core::KernelError> {
                Ok(kmb_core::kernel::BodyKind::Sheet)
            }
        }
        let mesh = Mesh {
            positions: vec![[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.]],
            normals: vec![[0., 0., 1.]; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            triangle_faces: vec![0, 0],
            face_names: vec!["face".into()],
            edges: vec![],
        };
        let mut scene = Scene::default();
        scene.set(
            &[(FeatureId(1), ShapeHandle(1))],
            vec![(FeatureId(1), mesh)],
        );
        let white = Color32::WHITE;
        let colors = Colors::from_look(Palette {
            dark: true,
            canvas: Color32::BLACK,
            ink: white,
            ink2: white,
            ink3: white,
            err: white,
            warn: white,
            design: white,
            cam: white,
            shade3: white,
            accent: white,
            inks: Inks::DARK,
        });
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(128., 128.));
        let mut cam = Camera::default();
        cam.set_view(0., -90.);
        cam.fit_box([0., 0., 0.], [10., 10., 0.], rect);
        let draw = |scene: &Scene| {
            render_guided(
                scene,
                &cam,
                [128, 128],
                1.,
                &[],
                None,
                &colors,
                Shading::Shaded,
                Guides {
                    grid: false,
                    axes: false,
                },
                &[],
                &Behind::default(),
            )
        };
        let before = draw(&scene);
        scene.set_body_kinds(&SheetKernel);
        assert!(scene.bodies[0].two_sided);
        let after = draw(&scene);
        assert!(
            before
                .pixels
                .iter()
                .zip(&after.pixels)
                .filter(|(a, b)| a != b)
                .count()
                > 1000,
            "裏面の面積が見える"
        );
        assert_eq!(
            scene.bodies[0].mesh.triangles.len(),
            2,
            "測定の面積は片面のまま"
        );
    }
}
