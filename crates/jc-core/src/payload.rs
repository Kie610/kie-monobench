//! 機能の出力に入れる型つきのデータ([`Value::Custom`])の読み書き。
//!
//! 型の名前(`TYPE`)と JSON で読み書きする。モジュールどうしは、相手の crate ではなく
//! コアの型(輪郭 [`crate::Profile`]、線 [`crate::Curves`]、点 [`Points`]、3 次元の線 [`Wires`])を通して受け渡す。

use crate::Value;
use crate::kernel::Plane;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;
use std::fmt;

/// [`Value::Custom`] に入れる型。
pub trait Payload: Serialize + DeserializeOwned {
    /// 型の名前(例: `geom.profile@1`)。形を変える時は版を上げる
    const TYPE: &'static str;

    fn to_value(&self) -> Value {
        Value::Custom {
            type_key: Self::TYPE.into(),
            data: serde_json::to_value(self).expect("出力は JSON にできる"),
        }
    }

    /// 型の名前が違うか、形が読めなければ `None`。
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            // 写さずに読む(操作の一覧が 1 コマごとに全部の輪郭を読む。10-24 v10)
            Value::Custom { type_key, data } if type_key == Self::TYPE => {
                <Self as serde::Deserialize>::deserialize(data).ok()
            }
            _ => None,
        }
    }
}

/// 点の集まりの出力の型の名前。
pub const POINTS_TYPE: &str = "geom.points@1";

/// 点の番号(スケッチの要素の番号と同じ数)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PointId(pub u32);

impl fmt::Display for PointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{}", self.0)
    }
}

/// 平面の上の番号つきの点。スケッチが出し、穴や回転の軸のように点を使う機能が読む。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Points {
    pub plane: Plane,
    /// 点の位置(平面の座標)
    pub points: BTreeMap<PointId, [f64; 2]>,
    /// 線や円に使われていない単独の点(穴の位置や文字の基準になる)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lone: Vec<PointId>,
}

impl Payload for Points {
    const TYPE: &'static str = POINTS_TYPE;
}

/// 番号つきの線の出力の型の名前。
pub const LINES_TYPE: &str = "geom.lines@1";

/// 番号つきの線分・円弧・円(平面の座標。補助線も)。スケッチが `paths` に出し、スイープの経路や
/// ロフトのガイドレールのように線を番号で選ぶ機能が読む。同じ番号が続くのは、1 つの要素
/// (楕円・スプライン)を何区間かにした物。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Lines(pub Vec<(u32, crate::Segment)>);

impl Payload for Lines {
    const TYPE: &'static str = LINES_TYPE;
}

impl Payload for crate::Profile {
    const TYPE: &'static str = crate::PROFILE_TYPE;
}

impl Payload for crate::ProfileFaces {
    const TYPE: &'static str = crate::PROFILE_FACES_TYPE;
}

impl Payload for crate::Curves {
    const TYPE: &'static str = crate::CURVES_TYPE;
}

/// 3 次元の線の出力の型の名前。
pub const WIRES_TYPE: &str = "geom.wires@1";

/// 3 次元の折れ線(世界座標)。3D スケッチが出し、画面は 3D のビューに描く。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub points: Vec<[f64; 3]>,
    /// 補助線(形にしない線。3D のビューには出さない)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub construction: bool,
}

/// 3 次元の折れ線の集まり。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Wires(pub Vec<Wire>);

impl Payload for Wires {
    const TYPE: &'static str = WIRES_TYPE;
}

/// 警告の一覧(`warnings` の出力)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Warnings(pub Vec<String>);

impl Payload for Warnings {
    const TYPE: &'static str = "core.warnings";
}

/// 形を作る機能の道具の線を入れる出力の名前(押し出しなど)。
pub const TOOL_PREVIEW: &str = "tool_preview";

/// 道具の形をボディにどう効かせるか(押し出しの「操作」)。プレビューの色分けに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOp {
    New,
    Join,
    Cut,
    Intersect,
}

impl ToolOp {
    /// ブーリアンの演算から(`None` は新しいボディ)。
    pub fn from_boolean(op: Option<crate::BooleanOp>) -> Self {
        match op {
            None => ToolOp::New,
            Some(crate::BooleanOp::Union) => ToolOp::Join,
            Some(crate::BooleanOp::Subtract) => ToolOp::Cut,
            Some(crate::BooleanOp::Intersect) => ToolOp::Intersect,
        }
    }

    /// 利用者に見せる名前(押し出しの「操作」の欄と同じ)。
    pub fn label(self) -> &'static str {
        match self {
            ToolOp::New => "新規ボディ",
            ToolOp::Join => "結合",
            ToolOp::Cut => "切り取り",
            ToolOp::Intersect => "交差",
        }
    }
}

/// 道具の形の線と立体(世界座標)と演算(`tool_preview` の出力)。編集パネルで直している間、
/// 3D のビューが演算の色で描く。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolPreview {
    pub op: ToolOp,
    pub lines: Vec<Vec<[f64; 3]>>,
    /// 道具の立体の三角形(外から見て左回り)。切り取りは赤、交差は黄の半透明で塗る
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<[[f64; 3]; 3]>,
    /// 押し出す輪郭(スケッチの面の上、閉じた折れ線)。直している間も青く塗る
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile: Vec<Vec<[f64; 3]>>,
    /// 押し出す向きの矢印(輪郭の重心から各側の端まで。両側・対称なら 2 本)。
    /// 線の終わりに矢印を描き、向きと長さが分かるようにする
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arrows: Vec<[[f64; 3]; 2]>,
}

impl Payload for ToolPreview {
    const TYPE: &'static str = "geom.tool_preview@1";
}

/// 下絵の絵を入れる出力の名前(キャンバス)。
pub const PICTURE: &str = "picture";

/// 平面に置く絵(下絵)。形には使わず、3D のビューとスケッチの編集が下に描くだけ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Picture {
    /// 絵を置く平面
    pub plane: Plane,
    /// 絵の左下・右下・右上・左上の角(平面の座標 mm)。絵の左下の画素が最初の角に来る
    pub corners: [[f64; 2]; 4],
    /// 絵のファイル(PNG・JPEG・BMP)の中身の base64
    pub image: String,
    /// 絵の中身が変わったら変わる値(画面が絵を読み直すかの判断に使う)
    pub key: u64,
    /// 不透明度(0〜1)
    pub opacity: f64,
}

impl Payload for Picture {
    const TYPE: &'static str = "geom.picture@1";
}

impl Picture {
    /// 出力 `v` が絵なら、その `key`(絵の中身を読まずに、画面が覚えた絵を使えるか見る)。
    pub fn key_of(v: &Value) -> Option<u64> {
        match v {
            Value::Custom { type_key, data } if type_key == Self::TYPE => data.get("key")?.as_u64(),
            _ => None,
        }
    }

    /// 出力 `v` が絵なら、絵の中身を除いた置き方(平面・角・不透明度・key)。画面は毎フレーム読むので、
    /// 大きな絵の中身は写さない。
    pub fn placement_of(v: &Value) -> Option<(Plane, [[f64; 2]; 4], f64, u64)> {
        let Value::Custom { type_key, data } = v else {
            return None;
        };
        if type_key != Self::TYPE {
            return None;
        }
        Some((
            serde_json::from_value(data.get("plane")?.clone()).ok()?,
            serde_json::from_value(data.get("corners")?.clone()).ok()?,
            data.get("opacity")?.as_f64()?,
            data.get("key")?.as_u64()?,
        ))
    }

    /// 出力 `v` が絵なら、絵のファイルの中身の base64(写さずに読む)。
    pub fn image_of(v: &Value) -> Option<&str> {
        match v {
            Value::Custom { type_key, data } if type_key == Self::TYPE => {
                data.get("image")?.as_str()
            }
            _ => None,
        }
    }

    /// 角の世界の座標。
    pub fn world_corners(&self) -> [[f64; 3]; 4] {
        self.corners.map(|c| self.plane.to_world(c))
    }
}
