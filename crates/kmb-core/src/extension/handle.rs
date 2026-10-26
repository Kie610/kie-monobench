//! 3D のビューでマウスを動かして値を決める手順と、マウスの動きを値にする方法。

use super::*;

/// 3D のビューでマウスを動かして値を決める手順の 1 つ(押し出しの高さ、穴の位置と径など)。
/// 画面は手順の形だけを知り、決めた値を操作の指定(`args` の `key`)に入れて実行する。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Handle {
    /// 決めた値を入れる指定の名前(例: `distance`)
    pub key: String,
    /// 手順の説明(例: 「マウスを上下に動かし、クリックで高さを決める」)
    pub label: String,
    pub gesture: Gesture,
    /// 値の目安を示す線(押し出す輪郭など。世界座標)。距離の手順では値だけ動かして描く
    #[serde(skip)]
    pub outline: Vec<Vec<[f64; 3]>>,
    /// 点の手順で、近くに来たら吸い付く点(ベッドのネジ穴など。世界座標)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snap_to: Vec<[f64; 3]>,
    /// [`Gesture::Choice`] の候補
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<HandleChoice>,
    /// 省ける手順(数字を打たずに Enter を押すと、値なしで次へ進む。例: 穴の深さを省くと貫通)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    /// [`Gesture::Choice`] で、候補のほかにボディの平らな面のクリックも受け付ける。面をクリックすると
    /// その面を選んだ面にし、値は `"face"` になる(スケッチを描く平面に、原点の平面と面の両方を選べる)。
    /// [`Gesture::Edges`] では、辺のほかに面のクリックも受ける(フィレット・面取りの、面のまわりの辺)。
    /// 値の並びに面を `{"kind": "face", "feature": 機能, "face": 面}` で入れる(辺は今までの形のまま)。
    /// [`Gesture::Regions`] では、候補の外のボディの平らな面のクリックも受ける。面をクリックすると
    /// その面を選んだ面にして、操作の手順を始めからやり直す(押し出しは、選んだ面から高さを決める)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub faces: bool,
    /// [`Gesture::Point`] で、マウスの所に出す座標と打つ座標の座標系(加工のセットアップの座標など)。
    /// 空なら文書の座標。値は世界座標のまま(吸い付く格子はこの座標系で揃える)
    #[serde(skip)]
    pub frame: Option<Frame>,
    /// [`Gesture::Point`] で、`outline` の内側を薄く塗る(置ける所を見せる。入る位置など)。
    /// 偽なら線だけ
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fill: bool,
    /// [`Gesture::Regions`] で、始めから選んでいる候補の値(前に選んだ物。外せる)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub picked: Vec<serde_json::Value>,
    /// [`Gesture::Distance`] で、値が 0 以上の時と負の時に道具をどう効かせるか(押し出しなら
    /// 前へ引くと結合、逆へ引くと切り取り)。画面は線をその演算の色で描く
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ops: Option<[crate::ToolOp; 2]>,
    /// この手順の間、原点の平面を目安として見せる(選べない)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_origin: bool,
    /// [`Gesture::Distance`] で、値が 1 の時の道具の形(押し出す輪郭を `dir` 1 つ分伸ばした柱など。
    /// 外から見て左回り)。画面は輪郭の平面(`origin` を通り `dir` に垂直)からの距離を値の倍にして
    /// 色付きの立体で描く(値が負なら裏返す)。形を作る計算は機能が持つ
    #[serde(skip)]
    pub shape: Option<std::sync::Arc<OverlayShape>>,
    /// [`Gesture::Edges`] で、クリックした辺から接してつながる辺も入る(フィレットの「つながる辺も
    /// 入れる」)。画面はつながる辺も選んだ辺と同じに描く
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub chain: bool,
    /// [`Gesture::Face`] で、ボディの頂点のクリックも受ける(頂点の近くを押すと頂点を選ぶ)。
    /// 値は `{"vertex": 頂点の名前, "feature": 機能の番号}`(「対象まで」で点を選ぶ時)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vertices: bool,
    /// [`Gesture::Distance`] で、マウスをボディの平らな面に載せると、距離をその面の平面までにする
    /// (輪郭の平面にある面と、押し出す向きに沿う面は除く)。決めると `key` の指定にその面までの
    /// 距離を、この名前の指定に `{"feature": 機能, "face": 面}` を入れる(「対象まで」の範囲)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_key: Option<String>,
}

/// 点の座標系。世界座標 = `origin` + x·`axes[0]` + y·`axes[1]` + z·`axes[2]`(軸は互いに直交する長さ 1)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Frame {
    pub origin: [f64; 3],
    pub axes: [[f64; 3]; 3],
}

impl Frame {
    /// この座標系の点を世界座標に。
    pub fn to_world(&self, l: [f64; 3]) -> [f64; 3] {
        let mut w = self.origin;
        for (k, a) in self.axes.iter().enumerate() {
            for i in 0..3 {
                w[i] += a[i] * l[k];
            }
        }
        w
    }

    /// 世界座標の点をこの座標系に。
    pub fn to_local(&self, w: [f64; 3]) -> [f64; 3] {
        let d = [
            w[0] - self.origin[0],
            w[1] - self.origin[1],
            w[2] - self.origin[2],
        ];
        self.axes.map(|a| a[0] * d[0] + a[1] * d[1] + a[2] * d[2])
    }

    /// 世界座標の向きをこの座標系に。
    pub fn dir_to_local(&self, v: [f64; 3]) -> [f64; 3] {
        self.axes.map(|a| a[0] * v[0] + a[1] * v[1] + a[2] * v[2])
    }
}

impl Handle {
    /// 線・候補・吸い付く点のない手順。
    pub fn new(key: &str, label: &str, gesture: Gesture) -> Self {
        Handle {
            key: key.into(),
            label: label.into(),
            gesture,
            outline: Vec::new(),
            snap_to: Vec::new(),
            choices: Vec::new(),
            optional: false,
            faces: false,
            frame: None,
            fill: false,
            picked: Vec::new(),
            ops: None,
            show_origin: false,
            shape: None,
            chain: false,
            vertices: false,
            stop_key: None,
        }
    }

    /// 値 `v` の時に道具をどう効かせるか([`Handle::ops`] がなければ `None`)。
    pub fn op_for(&self, v: f64) -> Option<crate::ToolOp> {
        self.ops
            .map(|[ahead, back]| if v < 0.0 { back } else { ahead })
    }
}

/// [`Gesture::Choice`] の候補の 1 つ。
/// 値が `sketch`(機能の番号)を持つオブジェクトなら、そのスケッチの物とみなし、画面は 3D のビューで
/// 隠しているスケッチの候補を出さない。
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct HandleChoice {
    /// 選んだ時に指定へ入れる値
    pub value: serde_json::Value,
    /// 利用者に見せる名前(例: スケッチの名前)
    pub label: String,
    /// 3D のビューに描く線(世界座標)。閉じた線なら、その内側をクリックしても選べる
    #[serde(skip)]
    pub outline: Vec<Vec<[f64; 3]>>,
    /// [`Gesture::Regions`] で一緒に選べる組(押し出しならスケッチの番号)。
    /// 別の組の候補をクリックすると、それまで選んだ候補を外す
    #[serde(skip)]
    pub group: u64,
    /// `outline` を、原点のまわりで画面の上の大きさが一定の物として描く。線は長さ 1 を
    /// 原点の目安の大きさ(ビューが決める画素数)とした座標で持つ(原点の平面と軸は、
    /// 拡大・縮小しても画面の上の大きさが変わらない)
    #[serde(skip)]
    pub sized: bool,
    /// 線の色(原点の軸の赤・緑・青など)。なければ画面の既定の色
    #[serde(skip)]
    pub color: Option<[u8; 3]>,
}

impl HandleChoice {
    /// 原点の XY・YZ・ZX 平面(`k` = 0・1・2)の候補。軸の正の向きへ広がる正方形を原点から少し浮かせる
    /// (スケッチの平面を選ぶ時と同じ形。大きさは画面の上で一定)。
    pub fn origin_plane(k: usize, value: serde_json::Value) -> Self {
        let plane = [crate::Plane::XY, crate::Plane::YZ, crate::Plane::ZX][k];
        let (a, b) = (crate::Plane::ORIGIN_GAP, 1.0 + crate::Plane::ORIGIN_GAP);
        let corner = |u: f64, v: f64| plane.to_world([u, v]).map(f64::abs);
        HandleChoice {
            value,
            label: format!("原点の {} 平面", ["XY", "YZ", "ZX"][k]),
            outline: vec![vec![
                corner(a, a),
                corner(b, a),
                corner(b, b),
                corner(a, b),
                corner(a, a),
            ]],
            sized: true,
            ..Default::default()
        }
    }

    /// 原点の X・Y・Z 軸(`k` = 0・1・2)の候補。原点から正の向きへ、原点の平面と同じ長さの色の線。
    pub fn origin_axis(k: usize, value: serde_json::Value) -> Self {
        let mut tip = [0.0; 3];
        tip[k] = 1.0;
        HandleChoice {
            value,
            label: format!("原点の {} 軸", ["X", "Y", "Z"][k]),
            outline: vec![vec![[0.0; 3], tip]],
            sized: true,
            color: Some([[220, 80, 80], [70, 180, 80], [80, 125, 230]][k]),
            ..Default::default()
        }
    }
}

/// マウスの動きを値にする方法。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Gesture {
    /// `origin` から `dir` の向きの距離(mm、負もある)。値は数
    Distance { origin: [f64; 3], dir: [f64; 3] },
    /// 平面の上の点(`origin` を通り `normal` に垂直)。値は [x, y, z]
    Point {
        origin: [f64; 3],
        normal: [f64; 3],
        /// 吸い付く間隔(mm、0 なら吸い付かない)
        snap: f64,
    },
    /// `center` からの距離の 2 倍(直径、mm)。`normal` の平面の上で測る
    Diameter { center: [f64; 3], normal: [f64; 3] },
    /// `origin` を通る向き `axis` の軸まわりの角度(度、0 より大きく 360 以下。右ねじの向きが正)。
    /// 点 `from` を回した円の、マウスにいちばん近い所で決める(0 度は `from`)。値は数
    Angle {
        origin: [f64; 3],
        axis: [f64; 3],
        from: [f64; 3],
    },
    /// 3D のビューで平らな面をクリックして選ぶ。画面はその面を「選んだ面」にし、
    /// 値は面の名前(文字列)。後の手順は選んだ面で出し直す(コマンドを先に押してから面を選べる)
    Face,
    /// [`Gesture::Face`] と同じだが、曲面(円筒など)の面も選べる(ねじを付ける面など)
    AnyFace,
    /// 3D のビューで辺をクリックして足す・外す。足したら次へ進む(Ctrl を押していれば進まずに足し続け、
    /// Enter で次へ)。画面はクリックした辺を
    /// 「選んだ辺」にし、値は選んだ辺の一覧(`[{"feature": 7, "faces": ["f2:end", "f2:side:e1"]}]`)
    Edges,
    /// 手順の候補([`Handle::choices`])の線を描き、クリックした候補の値にする(押し出す輪郭など)
    Choice,
    /// 手順の候補([`Handle::choices`])をクリックで足す・外す。足したら次へ進む(Ctrl を押していれば
    /// 進まずに足し続け、Enter で次へ。押し出す領域など)。
    /// 値は選んだ候補の値の一覧。[`Handle::optional`] なら、選ばずに Enter で値なし(`null`)
    Regions,
}
