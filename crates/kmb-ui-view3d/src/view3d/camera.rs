//! 視点: 平行投影と透視投影、回す・動かす・寄る、決まった向きへ回すボタンと軸の玉。

use super::*;

/// 視点。ヨー(z 軸まわり)とピッチで向きを決める。平行投影か透視投影。
/// `target` は画面の中心にある点で、回す中心(動かす・拡大すると一緒に動く)。
///
/// 透視投影では、視点は回す中心から [`Camera::distance`] だけ離れた所にあり、回す中心を通って
/// 視線に垂直な平面(回す中心の平面)の上で 1 mm が `zoom` 画素になるようにする。そのため
/// 2 つの投影を切り替えても、回す中心の平面の上の物は同じ大きさ・同じ位置に見える。
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    /// 1 mm あたりの画素数(透視投影では回す中心の平面の上で)
    pub zoom: f32,
    pub pan: Vec2,
    pub target: [f32; 3],
    /// 透視投影か(偽なら平行投影)
    pub perspective: bool,
    /// 画面の上での傾き(ラジアン。視線のまわりに右と上の向きを回す)。3D のビューでは 0 のまま。
    /// 2D スケッチを 3D の視点で見る時に、スケッチの平面の X が右に来るよう使う(N1b)
    pub roll: f32,
    /// 透視投影の縦の画角(度)。ふだんは 30 度。レンダリングの作業空間ではシーンの設定の焦点距離
    /// から入れる(回す中心の平面の見かけの大きさは変えずに、遠近の強さが変わる)
    pub fov_deg: f32,
    pub(super) fitted: bool,
    /// 向きを滑らかに替えている途中(軸の玉・等角のボタン・正面から見る)
    pub(super) turn: Option<Turn>,
    /// フリーオービットで傾けた(`roll` が 3D のビューで 0 でない)。次の拘束オービットや決まった
    /// 向きのボタンで、上を Z に戻す
    pub(super) free: bool,
    /// 回すドラッグの始まりの向き([`Camera::start_drag`])。回す量を毎回足さず、始まりの向きに
    /// ここまでの全部の量を当てる(真上で止まった分を失わず、押した所へ戻すと元の向きに戻る。10-20 u9)
    pub(super) drag: Drag,
}

/// 回すドラッグの状態。
/// 右・上・視点へ向かう向き。
type Basis = ([f32; 3], [f32; 3], [f32; 3]);

#[derive(Clone, Copy, Debug, Default)]
pub(super) enum Drag {
    /// ドラッグしていない(回す量をそのまま今の向きに足す)
    #[default]
    Off,
    /// ドラッグを始めた(まだ回していない。動かす・拡大のドラッグでは向きに触らない)
    Armed,
    /// 回している。始まりの向き(拘束オービットのヨー・ピッチ、フリーオービットの右・上・視点へ
    /// 向かう向き)と、ここまでの全部の量(画素)
    On {
        free: bool,
        yaw: f32,
        pitch: f32,
        basis: Basis,
        total: Vec2,
    },
}

/// 向きを替える時間(秒。U24。どう回ったかを目で追える長さ)。
pub const TURN_SECS: f64 = 0.3;

/// 向きの移り変わり(ヨー・ピッチはラジアン)。始めた時刻は、次に描く時に入れる。
#[derive(Clone, Copy, Debug)]
pub(super) struct Turn {
    from: (f32, f32),
    to: (f32, f32),
    start: Option<f64>,
    /// 向きと一緒に回す中心・倍率も移す(正面から見るで形全体を収める)
    glide: Option<Glide>,
    /// 傾きも移す(初めと行き先、ラジアン。名前を付けた視点へ戻る時)。無ければ傾きは触らない
    roll: Option<(f32, f32)>,
}

/// 向きを替える間に一緒に移す、回す中心・画面のずらし・倍率(初めと行き先)。
#[derive(Clone, Copy, Debug)]
pub(super) struct Glide {
    from: ([f32; 3], Vec2, f32),
    to: ([f32; 3], f32),
}

/// 透視投影の縦の画角の既定(度)。
pub const FOV_DEG: f32 = 30.0;
/// 透視投影で描く、視点からのいちばん近い距離(視点から回す中心までの距離との比)。
/// これより視点に近い所は切り落とす。
pub(crate) const NEAR: f32 = 0.02;

/// 視点の操作の割り当て(設定の「マウス」)。
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    /// ホイール押しドラッグで回す(偽なら動かす)
    pub middle_orbits: bool,
    /// 左ドラッグの働き(ビューの下の帯の「回す・動かす・拡大」で替える。既定は範囲で選ぶ)
    pub left: LeftDrag,
    /// マウスの下の面を色付けしない(辺を選ぶ時。面全体が色付くと辺が見えにくい)
    pub no_face_hover: bool,
    /// マウスの下の面が属するボディの、全部の面と稜線を強調する。
    pub body_hover: bool,
    /// 空の文書の基準面の目安を描かない(スケッチを描く平面を選ぶ板と重ならないように)
    pub no_guide_planes: bool,
    /// 左ドラッグで範囲を囲まない(2D スケッチが点を引く・自分の範囲で選ぶのに使う)
    pub no_box: bool,
    /// 回す時に上下の軸に縛らない(フリーオービット。偽なら Z を上に保つ拘束オービット)
    pub free_orbit: bool,
    /// 本体の奥のスケッチ・作業平面も普通の線で描く(表示の「全部透かす」。偽なら隠れた所は薄い点線)
    pub see_through: bool,
}

/// 3D マウスの 1 コマ分の視点の量(画面の基準)。手で持った形を動かす向き: 量の向きへ形が動く。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Nav {
    /// 画面移動(画素。[`Camera::pan_by`] と同じ向き)
    pub pan: Vec2,
    /// ズーム(倍率の自然対数。正で寄る)。寄る所は画面の中心(回す中心)
    pub zoom: f32,
    /// 画面の上の軸のまわりの回転(ラジアン。正は右へ引いた時と同じ向き)
    pub spin: f32,
    /// 画面の右の軸のまわりの回転(ラジアン。正は下へ引いた時と同じ向き)
    pub tilt: f32,
    /// 視線のまわりの回転(ラジアン。フリーオービットだけ)
    pub roll: f32,
    /// ボタンで押した視点の操作(押した時の 1 コマだけ)
    pub press: Option<NavPress>,
}

/// 3D マウスのボタンに割り当てる視点の操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavPress {
    /// 全体表示
    Fit,
    /// [`VIEW_BUTTONS`] の表の向き(上・前・右・等角)
    View(usize),
}

impl Nav {
    /// 動かす量もボタンも無いか。
    pub fn is_none(&self) -> bool {
        self.pan == Vec2::ZERO
            && self.zoom == 0.0
            && self.spin == 0.0
            && self.tilt == 0.0
            && self.roll == 0.0
            && self.press.is_none()
    }

    /// 回す量があるか(回す中心の印を出す)。
    pub fn turns(&self) -> bool {
        self.spin != 0.0 || self.tilt != 0.0 || self.roll != 0.0
    }
}

/// 左ドラッグの働き。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeftDrag {
    #[default]
    Select,
    Orbit,
    Pan,
    Zoom,
}

/// 3D のビューで起きたこと(選びは作業場が決める)。
#[derive(Default)]
pub struct ViewEvents {
    /// 左クリックした位置(Alt なし)
    pub click: Option<Pos2>,
    /// 左のダブルクリックの位置(Alt なし)。2 回目のクリックと同じフレームで、`click` も入っている
    pub double_click: Option<Pos2>,
    /// クリックの時に Ctrl(macOS は Cmd)を押していたか(選ぶ欄・選ぶ手順で、次の欄へ進まずに足す)
    pub add: bool,
    /// 左ドラッグで囲んだ範囲(離した時)
    pub boxed: Option<Rect>,
    /// 範囲を右から左へ囲んだか(U30。真なら範囲にかかった物、偽なら全部入った物を選ぶ)
    pub crossing: bool,
    /// 左の長押し(動かさずに押し続けた位置。1 回だけ)
    pub long_press: Option<Pos2>,
    /// 右クリックした位置(動きが小さい時だけ。ドラッグは回す)
    pub context: Option<Pos2>,
    /// カーソルの位置
    pub hover: Option<Pos2>,
    /// 今回し中か(回す中心の印を出す)
    pub orbiting: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            yaw: -60f32.to_radians(),
            pitch: 30f32.to_radians(),
            zoom: 4.0,
            pan: Vec2::ZERO,
            target: [0.0; 3],
            perspective: false,
            roll: 0.0,
            fov_deg: FOV_DEG,
            fitted: false,
            turn: None,
            free: false,
            drag: Drag::Off,
        }
    }
}

pub(super) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(super) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `v` を、長さ 1 の軸 `axis` のまわりに `angle` ラジアン回す(ロドリゲスの式)。
pub(super) fn rotate(v: [f32; 3], axis: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    let k = cross(axis, v);
    let d = dot(axis, v) * (1.0 - c);
    [0, 1, 2].map(|i| v[i] * c + k[i] * s + axis[i] * d)
}

pub(super) fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l > 0.0 { a.map(|v| v / l) } else { a }
}

pub(super) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

impl Camera {
    /// (右、上、視点へ向かう向き)
    pub fn basis_pub(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        self.basis()
    }

    /// (右、上、視点へ向かう向き)
    pub(super) fn basis(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let (right, up, eye) = kmb_render::camera::basis_rad(self.yaw, self.pitch);
        if self.roll == 0.0 {
            return (right, up, eye);
        }
        let (sr, cr) = self.roll.sin_cos();
        (
            [0, 1, 2].map(|i| right[i] * cr + up[i] * sr),
            [0, 1, 2].map(|i| up[i] * cr - right[i] * sr),
            eye,
        )
    }

    /// 透視投影の焦点距離(画素)。ビューの高さと画角で決まる。
    fn focal(&self, rect: Rect) -> f32 {
        let fov = self.fov_deg.clamp(1.0, 150.0);
        0.5 * rect.height().max(1.0) / (fov.to_radians() / 2.0).tan()
    }

    /// 透視投影の時、視点から回す中心までの距離(mm)。平行投影なら `None`。
    pub fn distance(&self, rect: Rect) -> Option<f32> {
        self.perspective.then(|| self.focal(rect) / self.zoom)
    }

    /// 写実の絵(kmb-render)を、ビュー `rect` と同じ見え方で描く視点。
    pub fn render_camera(&self, rect: Rect) -> kmb_render::Camera {
        let (r, u, e) = self.basis();
        let shift = [
            self.pan.x / (rect.width() / 2.0).max(1.0),
            -self.pan.y / (rect.height() / 2.0).max(1.0),
        ];
        let zoom = self.zoom.max(1e-6);
        let mut c = match self.distance(rect) {
            Some(dist) => {
                let eye = [0, 1, 2].map(|i| self.target[i] + e[i] * dist);
                let mut c = kmb_render::Camera::from_basis(eye, r, u, e, self.fov_deg, 0.0);
                // 焦点は回す中心(被写界深度)
                c.focus = dist;
                c
            }
            None => {
                let h = rect.height() / zoom;
                // 平行投影の視点は、場面より十分後ろに置く(光線の起点にする)
                let back = h.max(rect.width() / zoom) * 50.0 + 1000.0;
                let eye = [0, 1, 2].map(|i| self.target[i] + e[i] * back);
                kmb_render::Camera::from_basis(eye, r, u, e, 0.0, h)
            }
        };
        c.shift = shift;
        c
    }

    /// 透視投影の時の視点の位置。平行投影なら `None`。
    pub fn eye_position(&self, rect: Rect) -> Option<[f32; 3]> {
        let (_, _, e) = self.basis();
        let dist = self.distance(rect)?;
        Some([0, 1, 2].map(|i| self.target[i] + e[i] * dist))
    }

    /// 点 `p` から視点へ向かう向き(長さは決めない。表か裏かを見るのに使う)。
    pub(super) fn toward_eye(&self, p: [f32; 3], eye: Option<[f32; 3]>) -> [f32; 3] {
        match eye {
            Some(e) => sub(e, p),
            None => self.basis().2,
        }
    }

    /// 透視投影で描く近さの上限(`project` の近さ。これより手前は視点に近すぎるので切る)。
    pub(super) fn near_limit(&self, rect: Rect) -> Option<f32> {
        self.distance(rect).map(|d| d * (1.0 - NEAR))
    }

    /// 見る向きが X・Y・Z の軸のどれかとほぼ平行か(1 度まで。真上・真下はピッチを 89.9 度で
    /// 止めるので、ちょうど平行にはならない)。投影の「自動」が平行投影にする向き。
    pub fn axis_aligned(&self) -> bool {
        let (_, _, e) = self.basis();
        e.iter().any(|v| v.abs() > 1.0f32.to_radians().cos())
    }

    /// 画面上の位置 `pos` を通る視線(始点、向き)。始点は回す中心の平面の上で、向きは長さ 1。
    /// 平行投影ではどの視線も同じ向き、透視投影では視点から放射状に出る。
    pub fn ray(&self, pos: Pos2, rect: Rect) -> ([f32; 3], [f32; 3]) {
        let (r, u, e) = self.basis();
        let c = rect.center() + self.pan;
        let (dx, dy) = ((pos.x - c.x) / self.zoom, (c.y - pos.y) / self.zoom);
        let o = [0, 1, 2].map(|i| self.target[i] + r[i] * dx + u[i] * dy);
        match self.eye_position(rect) {
            Some(eye) => (o, normalize(sub(o, eye))),
            None => (o, e.map(|v| -v)),
        }
    }

    /// 画面上の位置と近さ([`Camera::project`] の 2 つ目の値)から、空間の点。
    pub fn unproject(&self, pos: Pos2, depth: f32, rect: Rect) -> [f32; 3] {
        let (o, d) = self.ray(pos, rect);
        let (_, _, e) = self.basis();
        // 始点の近さは 0。視線を進むと近さは dot(d, e) ずつ減る
        let t = depth / dot(d, e);
        [0, 1, 2].map(|i| o[i] + d[i] * t)
    }

    /// ドラッグを始めた。この後の [`Camera::orbit`]・[`Camera::orbit_free`] は、始まりの向きに
    /// ここまでの全部の量を当てる([`Camera::end_drag`] まで)。
    pub fn start_drag(&mut self) {
        self.drag = Drag::Armed;
    }

    /// ドラッグを終えた(回す量をまた今の向きに足す)。
    pub fn end_drag(&mut self) {
        self.drag = Drag::Off;
    }

    /// ドラッグの間なら、ここまでの全部の量に `d` を足し、始まりの向きと全部の量を返す。回し方を
    /// 途中で替えた時(拘束とフリーの切り替え)は、今の向きを始まりにし直す。
    fn drag_total(&mut self, free: bool, d: Vec2) -> Option<(f32, f32, Basis, Vec2)> {
        if let Drag::On { free: f, .. } = self.drag
            && f != free
        {
            self.drag = Drag::Armed;
        }
        match &mut self.drag {
            Drag::Off => None,
            Drag::Armed => {
                let basis = self.basis();
                self.drag = Drag::On {
                    free,
                    yaw: self.yaw,
                    pitch: self.pitch,
                    basis,
                    total: d,
                };
                Some((self.yaw, self.pitch, basis, d))
            }
            Drag::On {
                yaw,
                pitch,
                basis,
                total,
                ..
            } => {
                *total += d;
                Some((*yaw, *pitch, *basis, *total))
            }
        }
    }

    /// 回す中心のまわりに回す(画素の動き)。上下の軸(Z)を保つ拘束オービット。
    pub fn orbit(&mut self, d: Vec2) {
        // 向きを替えている途中に引いたら、今の向きから回す
        self.turn = None;
        // フリーオービットで傾けた後は、上を Z に戻してから回す(拘束オービットは Z が上)
        self.level();
        // 傾けてあれば、画面の上の動きを傾ける前の向きに直す(引いた向きへ回る)
        let (sr, cr) = self.roll.sin_cos();
        let d = Vec2::new(d.x * cr + d.y * sr, d.y * cr - d.x * sr);
        let (yaw, pitch, d) = match self.drag_total(false, d) {
            Some((yaw, pitch, _, total)) => (yaw, pitch, total),
            None => (self.yaw, self.pitch, d),
        };
        self.yaw = yaw - d.x * 0.01;
        self.pitch = (pitch + d.y * 0.01).clamp(-1.55, 1.55);
    }

    /// 上下の軸に縛らずに回す(フリーオービット)。
    /// 横に引くと画面の上下の軸のまわりに、縦に引くと画面の左右の軸のまわりに回り、真上を
    /// 越えて裏へも回れる。回す量は拘束オービットと同じ。
    pub fn orbit_free(&mut self, d: Vec2) {
        self.turn = None;
        let ((r, u, e), d) = match self.drag_total(true, d) {
            Some((_, _, basis, total)) => (basis, total),
            None => (self.basis(), d),
        };
        let turn = |v: [f32; 3]| rotate(rotate(v, u, -d.x * 0.01), r, -d.y * 0.01);
        self.set_basis(turn(r), turn(e));
    }

    /// 向きを、右の向き `r` と視点へ向かう向き `e`(どちらも長さ 1、直交)にする(ヨー・ピッチ・
    /// 傾きに分ける)。真上・真下はピッチの限り(拘束オービットと同じ 89.9 度より少し先)で止める。
    fn set_basis(&mut self, r: [f32; 3], e: [f32; 3]) {
        const LIMIT: f32 = std::f32::consts::FRAC_PI_2 - 1e-4;
        let pitch = e[2].clamp(-1.0, 1.0).asin().clamp(-LIMIT, LIMIT);
        let yaw = e[1].atan2(e[0]);
        self.yaw = yaw;
        self.pitch = pitch;
        self.roll = 0.0;
        let (r0, u0, _) = self.basis();
        self.roll = dot(r, u0).atan2(dot(r, r0));
        self.free = true;
    }

    /// フリーオービットで傾けていたら、上を Z に戻す(向きは今の視線のまま)。
    fn level(&mut self) {
        if self.free {
            self.free = false;
            self.roll = 0.0;
            self.pitch = self.pitch.clamp(-1.55, 1.55);
        }
    }

    /// 平行に動かす(画素の動き)。画面の中心の点(回す中心)も一緒に動く。
    /// 透視投影でも、回す中心の平面の上の物がマウスと同じだけ動く。
    pub fn pan_by(&mut self, d: Vec2) {
        let (r, u, _) = self.basis();
        for i in 0..3 {
            self.target[i] += (-r[i] * d.x + u[i] * d.y) / self.zoom;
        }
    }

    /// `factor` 倍に拡大する。`at` の点が画面の同じ所に残るよう、回す中心も寄せる。
    /// 透視投影では視点が回す中心へ近づく(回す中心の平面の上の、`at` の点が残る)。
    pub fn zoom_at(&mut self, factor: f32, at: Pos2, rect: Rect) {
        let new = (self.zoom * factor).clamp(0.01, 1000.0);
        let c = rect.center() + self.pan;
        let d = (at - c) * (1.0 / self.zoom - 1.0 / new);
        let (r, u, _) = self.basis();
        for i in 0..3 {
            self.target[i] += r[i] * d.x - u[i] * d.y;
        }
        self.zoom = new;
    }

    /// 3D マウスの量 `nav` を当てる。回すのは今の回す中心のまわりで、`free` ならフリーオービット
    /// (今の向きの基準で 3 軸とも回す)、偽なら拘束オービット(Z を上に保ち、視線のまわりは回さない)。
    /// ズームは画面の中心(回す中心)へ寄る。ボタンの操作もここで行う。
    pub fn nav(&mut self, nav: &Nav, rect: Rect, free: bool) {
        if nav.turns() {
            self.turn = None;
            if free {
                let (r, u, e) = self.basis();
                let turn = |v: [f32; 3]| {
                    let v = rotate(v, u, -nav.spin);
                    let v = rotate(v, r, -nav.tilt);
                    rotate(v, e, nav.roll)
                };
                self.set_basis(turn(r), turn(e));
            } else {
                self.level();
                // 傾けてあれば、画面の基準の量を傾ける前の向きに直す(マウスで回すのと同じ)
                let (sr, cr) = self.roll.sin_cos();
                let spin = nav.spin * cr + nav.tilt * sr;
                let tilt = nav.tilt * cr - nav.spin * sr;
                self.yaw -= spin;
                self.pitch = (self.pitch + tilt).clamp(-1.55, 1.55);
            }
        }
        if nav.pan != Vec2::ZERO {
            self.pan_by(nav.pan);
        }
        if nav.zoom != 0.0 {
            self.turn = None;
            self.zoom_at(nav.zoom.exp(), rect.center() + self.pan, rect);
        }
        match nav.press {
            Some(NavPress::Fit) => self.refit(),
            Some(NavPress::View(i)) => {
                if let Some(b) = VIEW_BUTTONS.get(i) {
                    self.turn_to(b.yaw, b.pitch);
                    log::info!("視点(3D マウス): {}", b.label);
                }
            }
            None => {}
        }
    }

    /// 回す中心を `p` にする(`p` が画面の中心に来る)。
    pub fn set_pivot(&mut self, p: [f32; 3]) {
        self.target = p;
        self.pan = Vec2::ZERO;
    }

    /// 画面上の位置と、近さ(回す中心の平面からの視点の側への距離 mm。大きいほど手前)。
    /// 透視投影で視点より手前(後ろ)の点は、近い所の上限に寄せて写す(線は
    /// [`Camera::segment`] で切ってから写す)。
    pub fn project(&self, p: [f32; 3], rect: Rect) -> (Pos2, f32) {
        let (r, u, e) = self.basis();
        let d = sub(p, self.target);
        let c = rect.center() + self.pan;
        let z = dot(d, e);
        let k = match self.distance(rect) {
            Some(dist) => self.zoom * dist / (dist - z).max(dist * NEAR),
            None => self.zoom,
        };
        (Pos2::new(c.x + dot(d, r) * k, c.y - dot(d, u) * k), z)
    }

    /// 線分 `a`〜`b` の、描ける部分の画面上の両端(透視投影で視点に近すぎる所は切る。全部なら `None`)。
    pub fn segment(&self, a: [f32; 3], b: [f32; 3], rect: Rect) -> Option<(Pos2, Pos2)> {
        let (a, b) = self.clip_segment(a, b, rect)?;
        Some((self.project(a, rect).0, self.project(b, rect).0))
    }

    /// 線分の、透視投影で視点に近すぎる所を切る(平行投影ならそのまま)。
    pub(crate) fn clip_segment(
        &self,
        a: [f32; 3],
        b: [f32; 3],
        rect: Rect,
    ) -> Option<([f32; 3], [f32; 3])> {
        let Some(zmax) = self.near_limit(rect) else {
            return Some((a, b));
        };
        let (_, _, e) = self.basis();
        let za = dot(sub(a, self.target), e);
        let zb = dot(sub(b, self.target), e);
        let at = |t: f32| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
        match (za <= zmax, zb <= zmax) {
            (true, true) => Some((a, b)),
            (false, false) => None,
            (true, false) => Some((a, at((zmax - za) / (zb - za)))),
            (false, true) => Some((at((zmax - za) / (zb - za)), b)),
        }
    }

    /// 画面の上で直線に補間した重み `w` での近さ。透視投影では近さが画面の上で直線に
    /// 変わらないので、視点からの距離の逆数で補間する。
    pub(super) fn depth_at(&self, rect: Rect, w: &[f32], z: &[f32]) -> f32 {
        match self.distance(rect) {
            Some(dist) => {
                let inv: f32 = w
                    .iter()
                    .zip(z)
                    .map(|(w, z)| w / (dist - z).max(dist * NEAR))
                    .sum();
                dist - 1.0 / inv
            }
            None => w.iter().zip(z).map(|(w, z)| w * z).sum(),
        }
    }

    /// 全体が収まるように合わせる。
    pub fn fit(&mut self, scene: &Scene, rect: Rect) {
        if scene.is_empty() {
            return;
        }
        self.fit_box(scene.min, scene.max, rect);
    }

    /// 箱 `min`〜`max` が収まるように合わせる(ボディが無くスケッチだけの時は線の範囲)。
    pub fn fit_box(&mut self, min: [f32; 3], max: [f32; 3], rect: Rect) {
        let center = [0, 1, 2].map(|i| (min[i] + max[i]) / 2.0);
        let size = sub(max, min);
        let radius = (dot(size, size).sqrt() / 2.0).max(1.0);
        self.target = center;
        self.pan = Vec2::ZERO;
        let half = 0.45 * rect.width().min(rect.height());
        self.zoom = if self.perspective {
            // 球の見かけの半径が `half` 画素になる距離(手前に膨らむ分も入れる)
            let f = self.focal(rect);
            let angle = (half / f).atan();
            f * angle.sin() / radius
        } else {
            half / radius
        };
        // 字を大きくして 3D のビューが潰れた時(幅 0)も、拡大と同じ範囲に収める。
        // 0 のままだと格子の線の数が限りなく増えて止まる
        self.zoom = if self.zoom.is_finite() {
            self.zoom.clamp(0.01, 1000.0)
        } else {
            1.0
        };
        self.fitted = true;
    }

    /// 今の視点を、ビューの大きさに依らない形で取り出す(U20 の名前付きの視点):
    /// (画面の真ん中の点、ヨー(度)、ピッチ(度)、画面の縦に見える高さ(mm))。
    /// 向きを替えている途中なら、行き先の向き。
    pub fn snapshot(&self, rect: Rect) -> ([f32; 3], f32, f32, f32) {
        let (center, _) = self.ray(rect.center(), rect);
        let (yaw, pitch) = self.goal();
        (
            center,
            yaw.to_degrees(),
            pitch.to_degrees(),
            rect.height().max(1.0) / self.zoom,
        )
    }

    /// 今の傾き(度)。向きを替えている途中なら行き先の傾き。
    pub fn tilt_deg(&self) -> f32 {
        self.turn
            .and_then(|t| t.roll)
            .map_or(self.roll, |r| r.1)
            .to_degrees()
    }

    /// [`Camera::snapshot`] と [`Camera::tilt_deg`] で取り出した視点に戻す。向き・傾き・真ん中・
    /// 大きさを一緒に滑らかに替える(U24。真ん中だけ先に飛ばすと、回り始める前に形が跳ぶ)。
    /// 戻した後は、全体に合わせ直さない。
    pub fn restore(
        &mut self,
        center: [f32; 3],
        yaw_deg: f32,
        pitch_deg: f32,
        roll_deg: f32,
        height: f32,
        rect: Rect,
    ) {
        let zoom = (rect.height().max(1.0) / height.max(1e-3)).clamp(0.01, 1000.0);
        let roll = roll_deg.to_radians();
        let roll0 = self.roll;
        let from = (self.target, self.pan, self.zoom);
        self.turn_to(yaw_deg, pitch_deg);
        match self.turn.as_mut() {
            Some(t) if from.2 > 0.0 => {
                t.glide = Some(Glide {
                    from,
                    to: (center, zoom),
                });
                t.roll = Some((roll0, roll));
                // 傾きは行き先へ向けて移すので、turn_to が上を Z に戻した分を元に戻す
                self.roll = roll0;
            }
            _ => {
                self.turn = None;
                (self.target, self.pan, self.zoom) = (center, Vec2::ZERO, zoom);
                self.roll = roll;
            }
        }
        self.free = roll.abs() > 1e-6;
        self.fitted = true;
    }

    /// [`Camera::restore`] と同じ視点(傾きも)へ、滑らかにせずすぐ替える(スケッチを終えた時に、
    /// スケッチで見ていた向きのまま 3D のビューへ戻る。10-23 u1)。
    pub fn restore_now(
        &mut self,
        center: [f32; 3],
        yaw_deg: f32,
        pitch_deg: f32,
        roll_deg: f32,
        height: f32,
        rect: Rect,
    ) {
        self.restore(center, yaw_deg, pitch_deg, roll_deg, height, rect);
        // 向きを替える途中の始まりと終わりを続けて進め、すぐ行き先に置く
        self.step_turn(0.0);
        self.step_turn(f64::MAX);
    }

    /// [`Camera::restore`] と同じ視点へ、滑らかにせずすぐ替える(アニメーションの段)。
    pub fn place(
        &mut self,
        center: [f32; 3],
        yaw_deg: f32,
        pitch_deg: f32,
        height: f32,
        rect: Rect,
    ) {
        self.set_view(yaw_deg, pitch_deg);
        self.target = center;
        self.pan = Vec2::ZERO;
        self.zoom = (rect.height().max(1.0) / height.max(1e-3)).clamp(0.01, 1000.0);
        self.fitted = true;
    }

    /// 箱 `min`〜`max` に寄せて、回す中心をその中心にする(大きさは変えない)。
    pub fn focus(&mut self, min: [f32; 3], max: [f32; 3]) {
        self.set_pivot([0, 1, 2].map(|i| (min[i] + max[i]) / 2.0));
    }

    /// 次に描く時に全体が収まるよう合わせ直す。
    pub fn refit(&mut self) {
        self.fitted = false;
    }

    /// 今の向きが `yaw`・`pitch`(度)か。向きを替えている途中なら、行き先で比べる
    /// (押したボタンがすぐ光り、もう一度押すと反対側から見る)。
    pub fn is_view(&self, yaw_deg: f32, pitch_deg: f32) -> bool {
        if self.free {
            return false;
        }
        let (yaw, pitch) = self.goal();
        (yaw - yaw_deg.to_radians()).abs() < 1e-3 && (pitch - pitch_deg.to_radians()).abs() < 1e-3
    }

    /// 向きをすぐ替える(滑らかに替えている途中なら止める)。
    pub fn set_view(&mut self, yaw_deg: f32, pitch_deg: f32) {
        self.turn = None;
        self.level();
        self.yaw = yaw_deg.to_radians();
        self.pitch = pitch_deg.to_radians();
    }

    /// 行き先の向き(ラジアン)。替えている途中でなければ今の向き。
    fn goal(&self) -> (f32, f32) {
        self.turn.map_or((self.yaw, self.pitch), |t| t.to)
    }

    /// 向きを [`TURN_SECS`] かけて滑らかに替え始める(U24)。進めるのは [`Camera::step_turn`]。
    /// ヨーは近い回り方で回す(-170 度から 170 度へは 20 度だけ回る)。
    pub fn turn_to(&mut self, yaw_deg: f32, pitch_deg: f32) {
        use std::f32::consts::{PI, TAU};
        self.level();
        let to = (yaw_deg.to_radians(), pitch_deg.to_radians());
        let d = (to.0 - self.yaw + PI).rem_euclid(TAU) - PI;
        // 同じ向きを表す値に置き直す(見た目は変わらない)
        self.yaw = to.0 - d;
        if d.abs() < 1e-4 && (to.1 - self.pitch).abs() < 1e-4 {
            self.set_view(yaw_deg, pitch_deg);
            return;
        }
        self.turn = Some(Turn {
            from: (self.yaw, self.pitch),
            to,
            start: None,
            glide: None,
            roll: None,
        });
    }

    /// 向きを替えている途中なら、時刻 `now`(秒)の向きにする。まだ途中なら真(描き直しを頼む)。
    /// 終わりは行き先の値ちょうどに置く(ボタンの光り方と、もう一度押した時の向きが狂わない)。
    pub fn step_turn(&mut self, now: f64) -> bool {
        let Some(t) = &mut self.turn else {
            return false;
        };
        let start = *t.start.get_or_insert(now);
        // 設定の「動き」で時間を縮める(しないなら 0 秒で行き先に着く)
        let secs = crate::ink::motion(TURN_SECS);
        let k = if secs > 0.0 {
            ((now - start) / secs) as f32
        } else {
            1.0
        };
        if k >= 1.0 {
            (self.yaw, self.pitch) = t.to;
            if let Some(g) = t.glide {
                (self.target, self.pan, self.zoom) = (g.to.0, Vec2::ZERO, g.to.1);
            }
            if let Some(r) = t.roll {
                self.roll = r.1;
            }
            self.turn = None;
            return false;
        }
        // 初めと終わりをゆっくり(smoothstep)
        let e = k.max(0.0) * k.max(0.0) * (3.0 - 2.0 * k.max(0.0));
        self.yaw = t.from.0 + (t.to.0 - t.from.0) * e;
        self.pitch = t.from.1 + (t.to.1 - t.from.1) * e;
        if let Some(g) = t.glide {
            let (target, pan, zoom) = g.from;
            self.target = [0, 1, 2].map(|i| target[i] + (g.to.0[i] - target[i]) * e);
            self.pan = pan * (1.0 - e);
            // 倍率は比で移す(大きく変わる時も、縮む・広がる速さが揃う)
            self.zoom = zoom * (g.to.1 / zoom).powf(e);
        }
        if let Some((from, to)) = t.roll {
            use std::f32::consts::{PI, TAU};
            self.roll = from + ((to - from + PI).rem_euclid(TAU) - PI) * e;
        }
        true
    }

    /// 向きを替えている途中か。
    pub fn turning(&self) -> bool {
        self.turn.is_some()
    }

    /// 外向きの法線 `n`(世界の座標)の面を真正面から見る向きへ替える。
    /// 真上・真下を向く面は X が右になる向き(上の軸の玉と同じ)。
    pub fn look_along(&mut self, n: [f32; 3]) {
        if let Some((yaw, pitch)) = Self::facing(n) {
            self.turn_to(yaw, pitch);
        }
    }

    /// 「正面から見る」のボタン: [`Camera::look_along`] と同じだが、もうその面を真正面から見ていれば
    /// 裏から見る(視点のボタンをもう一度押した時と同じ)。裏へ回したら真。
    pub fn look_along_or_back(&mut self, n: [f32; 3]) -> bool {
        let front = Self::facing(n).is_some_and(|(yaw, pitch)| self.is_view(yaw, pitch));
        self.look_along(if front { n.map(|v| -v) } else { n });
        front
    }

    /// [`Camera::look_along`] と同じく向きを替え、回し終えた時に回す中心が `target`、倍率が `zoom`
    /// (1 mm あたりの画素数)になるよう、向きと一緒に滑らかに移す(正面から見るで形全体を
    /// 収める)。
    pub fn look_along_to(&mut self, n: [f32; 3], target: [f32; 3], zoom: f32) {
        self.look_along(n);
        let zoom = if zoom.is_finite() {
            zoom.clamp(0.01, 1000.0)
        } else {
            self.zoom
        };
        match self.turn.as_mut() {
            Some(t) if self.zoom > 0.0 => {
                t.glide = Some(Glide {
                    from: (self.target, self.pan, self.zoom),
                    to: (target, zoom),
                })
            }
            // もう正面を向いている(向きは替わらない): すぐ合わせる
            _ => (self.target, self.pan, self.zoom) = (target, Vec2::ZERO, zoom),
        }
    }

    /// [`Camera::look_along_or_back`] と [`Camera::look_along_to`] を合わせた物: もうその面を真正面から
    /// 見ていれば裏から見て、回し終えた時に回す中心と倍率を `target`・`zoom` にする。裏へ回したら真。
    pub fn look_along_or_back_to(&mut self, n: [f32; 3], target: [f32; 3], zoom: f32) -> bool {
        let front = Self::facing(n).is_some_and(|(yaw, pitch)| self.is_view(yaw, pitch));
        self.look_along_to(if front { n.map(|v| -v) } else { n }, target, zoom);
        front
    }

    /// [`Camera::look_along`] と同じ向きへ、滑らかにせずすぐ替える(2D スケッチが平らな図から
    /// 3D の視点へ移る時。N1b)。
    pub fn face(&mut self, n: [f32; 3]) {
        if let Some((yaw, pitch)) = Self::facing(n) {
            self.set_view(yaw, pitch);
        }
    }

    /// 外向きの法線 `n` の面を真正面から見るヨーとピッチ(度)。長さが 0 なら `None`。
    fn facing(n: [f32; 3]) -> Option<(f32, f32)> {
        let n = normalize(n);
        if dot(n, n) < 0.5 {
            return None;
        }
        let pitch = n[2].clamp(-1.0, 1.0).asin().to_degrees().clamp(-89.9, 89.9);
        let yaw = if n[2].abs() > 0.9995 {
            -90.0
        } else {
            n[1].atan2(n[0]).to_degrees()
        };
        Some((yaw, pitch))
    }

    /// 視点のボタン `b` が今の向きを指しているか。表なら `Some(false)`、反対側なら `Some(true)`。
    pub fn view_side(&self, b: &ViewButton) -> Option<bool> {
        if self.is_view(b.yaw, b.pitch) {
            Some(false)
        } else if self.is_view(b.back_yaw, b.back_pitch) {
            Some(true)
        } else {
            None
        }
    }

    /// 視点のボタン `b` を押す。もう表から見ていれば反対側から、それ以外は表から見る。
    pub fn press_view(&mut self, b: &ViewButton) {
        if self.view_side(b) == Some(false) {
            self.turn_to(b.back_yaw, b.back_pitch);
        } else {
            self.turn_to(b.yaw, b.pitch);
        }
        log::info!(
            "視点: {}",
            if self.view_side(b) == Some(true) {
                b.back
            } else {
                b.label
            }
        );
    }
}

/// 視点のボタン 1 つ。同じボタンをもう一度押すと反対側から見る。ボタンを増やさずに後ろ・左・下を見られる。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewButton {
    /// 表の名前と、反対側から見ている時の名前
    pub label: &'static str,
    pub back: &'static str,
    /// 表の向き(度)
    pub yaw: f32,
    pub pitch: f32,
    /// 反対側の向き(度)
    pub back_yaw: f32,
    pub back_pitch: f32,
}

/// 3D のビューの右上の視点のボタン。前・右は真裏から(後ろ・左)、上は真下から(X は右のまま、
/// 図面の下面図と同じ向き)、等角は回す軸のまわりに半周した斜め上から見る。
pub const VIEW_BUTTONS: [ViewButton; 4] = [
    ViewButton {
        label: "上",
        back: "下",
        yaw: -90.0,
        pitch: 89.9,
        back_yaw: -90.0,
        back_pitch: -89.9,
    },
    ViewButton {
        label: "前",
        back: "後",
        yaw: -90.0,
        pitch: 0.0,
        back_yaw: 90.0,
        back_pitch: 0.0,
    },
    ViewButton {
        label: "右",
        back: "左",
        yaw: 0.0,
        pitch: 0.0,
        back_yaw: 180.0,
        back_pitch: 0.0,
    },
    ViewButton {
        label: "等角",
        back: "等角(後)",
        yaw: -60.0,
        pitch: 30.0,
        back_yaw: 120.0,
        back_pitch: 30.0,
    },
];

/// 視点の軸の玉 1 つ(右上の軸の表示)。押すとこの軸の先から見る。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisBall {
    /// 0 = X、1 = Y、2 = Z
    pub axis: usize,
    /// 軸の正の側か
    pub positive: bool,
    /// 名前(玉の上に出す説明と、読み上げの名前)
    pub name: &'static str,
    /// この軸の先から見る向き(度)
    pub yaw: f32,
    pub pitch: f32,
}

/// 軸の玉 6 つ。正の側には X・Y・Z の文字を書き、負の側は淡くする。
/// 上と下から見る時は X が右(上は Y が上、下は Y が下)。
pub const AXIS_BALLS: [AxisBall; 6] = [
    AxisBall {
        axis: 0,
        positive: true,
        name: "右から(+X)",
        yaw: 0.0,
        pitch: 0.0,
    },
    AxisBall {
        axis: 0,
        positive: false,
        name: "左から(-X)",
        yaw: 180.0,
        pitch: 0.0,
    },
    AxisBall {
        axis: 1,
        positive: true,
        name: "後ろから(+Y)",
        yaw: 90.0,
        pitch: 0.0,
    },
    AxisBall {
        axis: 1,
        positive: false,
        name: "前から(-Y)",
        yaw: -90.0,
        pitch: 0.0,
    },
    AxisBall {
        axis: 2,
        positive: true,
        name: "上から(+Z)",
        yaw: -90.0,
        pitch: 89.9,
    },
    AxisBall {
        axis: 2,
        positive: false,
        name: "下から(-Z)",
        yaw: -90.0,
        pitch: -89.9,
    },
];

impl AxisBall {
    /// 世界の座標での軸の向き(長さ 1)
    pub fn dir(&self) -> [f32; 3] {
        let mut d = [0.0; 3];
        d[self.axis] = if self.positive { 1.0 } else { -1.0 };
        d
    }

    /// 同じ軸の反対の側の玉
    pub fn opposite(&self) -> &'static AxisBall {
        AXIS_BALLS
            .iter()
            .find(|b| b.axis == self.axis && b.positive != self.positive)
            .expect("6 つ揃っている")
    }
}

impl Camera {
    /// 軸の玉を押す。その軸の先から見る。もうその向きなら反対の側から見る。
    pub fn press_axis(&mut self, b: &AxisBall) {
        let b = if self.is_view(b.yaw, b.pitch) {
            b.opposite()
        } else {
            b
        };
        self.turn_to(b.yaw, b.pitch);
        log::info!("視点: {}", b.name);
    }

    /// 軸の玉 `b` の、軸の表示の中心から見た画面の上の位置(長さ 1 につき 1)と近さ(手前ほど大きい)
    pub fn axis_on_screen(&self, b: &AxisBall) -> (Vec2, f32) {
        let (r, u, e) = self.basis();
        let d = b.dir();
        (Vec2::new(dot(d, r), -dot(d, u)), dot(d, e))
    }
}

/// 軸の表示の半径(画素)。
pub const GIZMO_RADIUS: f32 = 56.0;

/// 視点の軸の表示。
/// X 赤・Y 緑・Z 青の玉を今の向きのまま描き、玉を押すとその軸の先から見る(2 度目は反対側)。
/// 玉のない所を引くと回す。押して向きを変える立方体は作らない(docs/避ける形.md)。
/// 返すのは、引いて回している最中か。
pub fn axis_gizmo(ui: &mut egui::Ui, cam: &mut Camera, center: Pos2, panel: Color32) -> bool {
    // 向きを替えている途中なら進める(軸の玉は 3D のビューと 3D スケッチの両方で毎回描く)
    let now = ui.input(|i| i.time);
    if cam.step_turn(now) {
        ui.ctx().request_repaint();
    }
    let id = ui.id().with("axis_gizmo");
    let inks = crate::inks(ui.ctx());
    let disc = ui.interact(
        Rect::from_center_size(center, Vec2::splat(2.0 * GIZMO_RADIUS)),
        id,
        Sense::click_and_drag(),
    );
    disc.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "視点の軸"));
    let orbiting = disc.dragged();
    if orbiting {
        cam.orbit(disc.drag_delta());
    }
    let arm = GIZMO_RADIUS - 14.0;
    // 遠い玉から描き、近い玉ほど上に重ねる(押した時も近い玉が勝つ)
    let mut order: Vec<(usize, Vec2, f32)> = AXIS_BALLS
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let (p, depth) = cam.axis_on_screen(b);
            (i, p, depth)
        })
        .collect();
    order.sort_by(|a, b| a.2.total_cmp(&b.2));
    let hovered_disc = disc.hovered() || orbiting;
    let painter = ui.painter().clone();
    if hovered_disc {
        painter.circle_filled(center, GIZMO_RADIUS, panel.gamma_multiply(0.75));
    }
    let mut pressed = None;
    let mut on_ball = false;
    for (i, p, _) in order {
        let b = &AXIS_BALLS[i];
        let at = center + p * arm;
        let size = if b.positive { 11.0 } else { 9.0 };
        let resp = ui
            .interact(
                Rect::from_center_size(at, Vec2::splat(2.0 * size + 2.0)),
                ui.id().with(("axis_ball", i)),
                Sense::click(),
            )
            .on_hover_text(b.name);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, b.name));
        let color = inks.axis[b.axis];
        let on = cam.is_view(b.yaw, b.pitch);
        if b.positive {
            painter.line_segment([center, at], egui::Stroke::new(2.0, color));
            painter.circle_filled(at, size, color);
            painter.text(
                at,
                egui::Align2::CENTER_CENTER,
                ["X", "Y", "Z"][b.axis],
                crate::prop(12.5),
                inks.axis_text,
            );
        } else {
            // 淡い色でも透かさない(真上から見た時、後ろの正の玉が透けて見えないように)
            let soft = Color32::from_rgb(
                ((color.r() as u16 + 2 * 235) / 3) as u8,
                ((color.g() as u16 + 2 * 235) / 3) as u8,
                ((color.b() as u16 + 2 * 235) / 3) as u8,
            );
            painter.circle_filled(at, size, soft);
            painter.circle_stroke(at, size, egui::Stroke::new(1.0, color));
        }
        on_ball |= resp.hovered();
        if resp.hovered() || on {
            painter.circle_stroke(at, size + 1.5, egui::Stroke::new(1.5, Color32::WHITE));
        }
        if resp.clicked() {
            pressed = Some(*b);
        }
    }
    if let Some(b) = pressed {
        cam.press_axis(&b);
        cam.step_turn(now);
        ui.ctx().request_repaint();
    }
    // 玉の上では玉の名前だけを出す(説明が 2 つ重ならないように)
    if !on_ball && !orbiting {
        disc.on_hover_text("視点の軸(玉を押すとその向きから見る。引くと回す)");
    }
    orbiting
}
