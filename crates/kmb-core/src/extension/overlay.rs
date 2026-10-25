//! 3D のビューへの重ね描き、進み具合の表示、時間の帯。

/// 重ねる線の描き方の役割の印。画面はこの印だけで色・太さ・破線を決め、何の線かは知らない
/// (加工の経路か、機械か、スケッチかは、モジュールが印を選んで渡す)。印の語彙(早送り・作図の線など)は
/// コアに置かず、層をまたぐ受け渡しの型(kmb-interchange の `ink`)が定数で持つ。画面のテーマが知らない
/// 印は主の線で描く。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LineInk(pub &'static str);

/// 重ねる線の種類: 描き方の役割と、凡例に出す名前(例: 「早送り」)。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LineKind {
    pub ink: LineInk,
    pub legend: std::borrow::Cow<'static, str>,
}

impl LineKind {
    pub const fn new(ink: LineInk, legend: &'static str) -> Self {
        LineKind {
            ink,
            legend: std::borrow::Cow::Borrowed(legend),
        }
    }
}

/// 重ねる箱の描き方の役割の印(語彙は kmb-interchange の `ink`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BoxInk(pub &'static str);

/// 重ねる箱の種類: 描き方の役割と、凡例に出す名前。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BoxKind {
    pub ink: BoxInk,
    pub legend: std::borrow::Cow<'static, str>,
}

impl BoxKind {
    pub const fn new(ink: BoxInk, legend: &'static str) -> Self {
        BoxKind {
            ink,
            legend: std::borrow::Cow::Borrowed(legend),
        }
    }
}

/// 3D のビューに重ねるもの(世界座標)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlay {
    pub lines: Vec<(Vec<[f64; 3]>, LineKind)>,
    /// (最小、最大、種類)
    pub boxes: Vec<([f64; 3], [f64; 3], BoxKind)>,
    /// 作業座標系の原点など
    pub points: Vec<[f64; 3]>,
    /// 作業座標系の X・Y・Z の向き(世界座標の単位ベクトル)。無ければ世界の軸の向きで描く
    pub axes: Option<[[f64; 3]; 3]>,
    /// シミュレーションで見つかった所(位置、種類、量 mm)
    pub marks: Vec<([f64; 3], MarkKind, f64)>,
    /// 部品の代わりに描く形(加工の途中の形)。あれば画面はボディを描かず、これを描く
    pub shape: Option<std::sync::Arc<OverlayShape>>,
    /// 半透明の平面と札(加工の高さなど)
    pub planes: Vec<OverlayPlane>,
    /// 平面に置いた絵(下絵)
    pub pictures: Vec<OverlayPicture>,
}

/// 3D のビューに重ねる絵(下絵。[`crate::Picture`] を画面が読んだ物)。
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayPicture {
    /// 絵の左下・右下・右上・左上の角(世界座標)
    pub corners: [[f64; 3]; 4],
    /// 絵が変わったら変わる値(画面が貼る絵を覚えておく鍵)
    pub key: u64,
    /// 絵の画素(幅, 高さ, RGBA。上の行から)
    pub rgba: std::sync::Arc<(u32, u32, Vec<u8>)>,
    /// 不透明度(0〜1)
    pub opacity: f32,
}

/// 3D のビューに重ねる、半透明の平面と札(例: 加工の安全高さ)。
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayPlane {
    /// 角(世界座標、順に回る 4 点)
    pub corners: [[f64; 3]; 4],
    pub ink: PlaneInk,
    /// 札(例: 「安全高さ Z15.0」)。画面は最初の角の近くに出す
    pub label: String,
}

/// [`OverlayPlane`] の色の役の印(語彙は kmb-interchange の `ink`)。色そのものは画面のテーマが決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlaneInk(pub &'static str);

/// 3D のビューに部品の代わりに描く網(世界座標)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OverlayShape {
    /// 中身が変わったら変わる値(画面が描いた絵を使い回すかの判断に使う)
    pub key: u64,
    /// 何の形か(例: 「ポケット の後」)
    pub label: String,
    pub mesh: crate::Mesh,
    /// 網の面ごとの色の役(`mesh.face_names` と同じ順)。`None` と、足りない面は画面の
    /// 途中の形の色。空なら全部その色
    pub face_colors: Vec<Option<ShapeInk>>,
    /// 色の凡例(色の役と名前)。画面はビューの隅に並べる
    pub legend: Vec<(ShapeInk, String)>,
    /// 2 つ目からの凡例(見出しと、色の役と名前)。画面は最初の凡例の下に並べる
    /// (流れの表面の圧力と、流線の速さなど)
    pub more_legends: Vec<(String, Vec<(ShapeInk, String)>)>,
    /// 解き方の札(例: 「3D・格子 1.6 mm・Re 13,000」)。画面はビューの右上に出す。空なら出さない
    pub badge: String,
}

/// [`OverlayShape`] の面の色の役。色そのものは画面のテーマが決める(明暗やハイコントラストに合わせる)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeInk {
    /// 操作ごとの色の `n` 番目(テーマの色の数で回す)
    Operation(u32),
    /// 名前の付いた色の役(比べる時の許容内・削り過ぎ、工具の刃など。語彙は kmb-interchange の `ink`)
    Named(&'static str),
    /// テーマを通さない色(sRGB)
    Rgb([u8; 3]),
}

/// 「実行」で走らせる重い計算の様子([`Extension::run_state`])。色・形は画面が決める。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunState {
    pub tone: RunTone,
    /// 短い札(「未実行」「計算中 42%」「古い」「済み」「失敗」)。ブラウザの名前の横に出す
    pub badge: String,
    /// 札の下の 1 行(「1:38 で計算(50 コマ)」「条件が変わったので結果は古いです」)
    pub detail: String,
    /// 計算中の進み(0〜1)
    pub fraction: Option<f32>,
    /// 計算の段(済んだ・今・まだ と、その名前)
    pub steps: Vec<(StepMark, String)>,
    /// 推移のグラフ(抗力の推移など)。計算中と結果に出す
    pub series: Option<Series>,
    /// 実行の操作と、その名前(「実行」「実行(解き直す)」)。使えなければ理由
    pub run: String,
    pub run_label: String,
    pub run_disabled: Option<String>,
    /// 計算中だけ: 中止の操作と、その名前(「中止(ここまでのコマは残す)」)
    pub cancel: Option<(String, String)>,
}

/// [`RunState`] の色の役。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RunTone {
    /// まだ解いていない
    #[default]
    Idle,
    /// 計算中
    Busy,
    /// 条件や形が変わり、結果が古い
    Stale,
    /// 今の条件で解いた結果がある
    Done,
    /// 失敗した・中止した
    Failed,
}

/// 計算の段の印。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepMark {
    Done,
    Now,
    Todo,
}

/// 推移のグラフ 1 本(横は時刻 s か回数)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Series {
    /// 例: 「抗力(N)」
    pub name: String,
    /// 横の軸の名前(例: 「時刻 s」)
    pub x_name: String,
    pub points: Vec<[f64; 2]>,
    /// グラフの下の 1 行(落ち着いたかの目安など)
    pub note: String,
}

/// 結果のコマ(時刻やモード)の帯([`Extension::frames`])。画面はビューの下に出し、
/// 押された・引かれたコマへ `seek` の操作で移す。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameBar {
    /// コマの名前(「0.21 s」「1 次 123 Hz」)。数がコマの数
    pub labels: Vec<String>,
    /// 今のコマ
    pub at: usize,
    /// 終わりの名前(「0.50 s」。空なら最後のコマの名前)
    pub total: String,
    /// 時刻のコマで、再生できるか(モードは送るだけ)
    pub playable: bool,
    /// 再生の速さ ×1 の時、1 秒に進めるコマの数
    pub fps: f64,
    /// コマを移す操作の頭。後ろにコマの番号(0 から)を付けて実行する
    pub seek: String,
}

/// 加工の「段の帯」: 素材から各操作の後の形までを並べ、選んだ段の形を 3D のビューに出す。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StageBand {
    /// 帯の見出し(例: セットアップの名前)
    pub title: String,
    pub stages: Vec<Stage>,
    /// 今 3D のビューに出している段
    pub selected: Option<usize>,
    /// 部品の形に戻す操作(段を選んでいる時だけ)
    pub clear: Option<String>,
    /// 帯の下の短い説明(例: 「シミュレーションを走らせると形が出ます」)
    pub note: String,
    /// 動きの単位の再生を始める操作(シミュレーションがまだなら走らせてから始める)
    pub play: Option<String>,
    /// 動きの単位の再生(再生している時だけ)。画面は再生の帯を出し、時刻を動かす操作を実行する
    pub playback: Option<Playback>,
}

/// 動きの単位の再生の様子と操作。時刻はすべて秒(機械の時間の目安)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Playback {
    /// 全体の時間
    pub total: f64,
    /// 今の時刻
    pub at: f64,
    /// 操作ごとの始まりの時刻と名前(前の操作・次の操作と、時間の軸の目盛りに使う)
    pub marks: Vec<(f64, String)>,
    /// 今の様子の行(名前と値。例: 「工具の位置」「X 1.00 Y 2.00 Z -3.00」)
    pub info: Vec<(String, String)>,
    /// 時刻を動かす操作の頭。後ろに秒(例: `12.5`)を付けて実行する
    pub seek: String,
    /// 前の動きと次の動きへ移る操作
    pub prev_move: String,
    pub next_move: String,
    /// ストックの色の付け方の選び(名前、操作、選んでいるか)
    pub colorings: Vec<(String, String, bool)>,
    /// 再生の帯で出す・隠すを切り替える線(名前と、その名前で切り替える描き方の役割)
    pub line_toggles: Vec<(String, LineInk)>,
    /// 再生をやめて部品の形に戻す操作
    pub stop: String,
}

/// 帯の 1 段。押すと `action` を実行する。
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub label: String,
    pub action: String,
    pub hint: String,
}

/// 3D のビューに置く印の種類(シミュレーションで見つかった所など。語彙は kmb-interchange の `ink`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MarkKind(pub &'static str);

impl Overlay {
    pub fn extend(&mut self, o: Overlay) {
        self.lines.extend(o.lines);
        self.boxes.extend(o.boxes);
        self.points.extend(o.points);
        if o.axes.is_some() {
            self.axes = o.axes;
        }
        self.marks.extend(o.marks);
        if o.shape.is_some() {
            self.shape = o.shape;
        }
        self.planes.extend(o.planes);
        self.pictures.extend(o.pictures);
    }
}
