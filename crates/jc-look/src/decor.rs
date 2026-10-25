//! 形を変えずに見た目だけに効く機能の出力: 光源([`Lamp`])とデカール([`Decal`])。
//!
//! どちらもタイムラインの機能が出し、レンダリングと 3D のビューが
//! 型で読む。機能の名前は知らずに、出力の名前([`LAMP`]・[`DECAL`])と型で探す。

use jc_core::{FeatureId, Payload, Value};
use serde::{Deserialize, Serialize};

/// 光源の出力の名前。
pub const LAMP: &str = "lamp";
/// デカールの出力の名前。
pub const DECAL: &str = "decal";

/// 点光源かスポットライト。位置は世界の座標(mm)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lamp {
    /// スポットライトか(偽なら点光源。どの向きにも同じ明るさ)
    pub spot: bool,
    /// 光源の位置(mm)
    pub position: [f64; 3],
    /// スポットの向き(長さ 1)。点光源では使わない
    pub direction: [f64; 3],
    /// 光の色(sRGB)
    pub color: [u8; 3],
    /// 明るさ(lm。点光源の全体の光束。スポットも同じ光度で、円すいの外を切る)
    pub lumens: f64,
    /// スポットの広がりの角(度。円すいの頂角)
    pub cone_deg: f64,
    /// スポットの縁のぼかし(度。広がりの角の内側をこの幅でなめらかに暗くする)
    pub blur_deg: f64,
    /// 光源の大きさ(半径 mm。影の縁のぼけ)
    pub radius: f64,
}

impl Payload for Lamp {
    const TYPE: &'static str = "render.lamp@1";
}

/// 面に貼る絵。どこに貼るかの面(ボディと面の名前)と、面の上の置き方を持つ。置き方の面の平面
/// (中心と向き)は、描く側が今の網から決める(後の機能で面が動いても付いていく)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decal {
    /// 面を選んだ時のボディ(ボディの最初の機能は描く側が引く)
    pub body: FeatureId,
    /// 面の名前
    pub face: String,
    /// 絵のファイル(PNG・JPEG・BMP)の中身の base64
    pub image: String,
    /// 絵の中身が変わったら変わる値(描く側が覚えた絵を使えるか見る)
    pub key: u64,
    /// 絵の幅と高さ(画素)
    pub px: [u32; 2],
    /// 面の中心からずらす量(面の平面の右と上、mm)
    pub offset: [f64; 2],
    /// 絵の横の長さ(mm。縦は絵の縦横の比)
    pub width: f64,
    /// 回転(rad。面の外から見て左回りが正)
    pub angle: f64,
    /// 不透明度(0〜1)
    pub opacity: f64,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
}

impl Payload for Decal {
    const TYPE: &'static str = "render.decal@1";
}

impl Decal {
    /// 出力 `v` がデカールなら、絵の中身を除いた物(画面は文書が変わるたびに読むので、大きな絵の
    /// 中身は写さない。中身は [`Decal::image_of`])。
    pub fn without_image(v: &Value) -> Option<Decal> {
        let Value::Custom { type_key, data } = v else {
            return None;
        };
        if type_key != Self::TYPE {
            return None;
        }
        let mut small: serde_json::Map<String, serde_json::Value> = data
            .as_object()?
            .iter()
            .filter(|(k, _)| k.as_str() != "image")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        small.insert("image".into(), serde_json::Value::String(String::new()));
        serde_json::from_value(serde_json::Value::Object(small)).ok()
    }

    /// 出力 `v` がデカールなら、絵のファイルの中身の base64(写さずに読む)。
    pub fn image_of(v: &Value) -> Option<&str> {
        match v {
            Value::Custom { type_key, data } if type_key == Self::TYPE => {
                data.get("image")?.as_str()
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decal_is_read_without_copying_its_picture() {
        let d = Decal {
            body: FeatureId(3),
            face: "f2:end".into(),
            image: "QUJD".into(),
            key: 7,
            px: [4, 2],
            offset: [1.0, 2.0],
            width: 30.0,
            angle: 0.5,
            opacity: 0.8,
            flip_h: false,
            flip_v: true,
        };
        let v = d.to_value();
        assert_eq!(Decal::image_of(&v), Some("QUJD"));
        let light = Decal::without_image(&v).unwrap();
        assert_eq!(light.image, "");
        assert_eq!(light.face, "f2:end");
        assert_eq!(Decal::from_value(&v), Some(d));
        let lamp = Lamp {
            spot: false,
            position: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
            color: [255; 3],
            lumens: 800.0,
            cone_deg: 45.0,
            blur_deg: 10.0,
            radius: 5.0,
        };
        assert!(Decal::without_image(&lamp.to_value()).is_none());
    }
}
