//! 物理材料と外観の表(M26)。設計でボディに付ける材料と、加工のストックの材料欄は同じ名前で引く
//! (加工の切削の目安は jc-cam-tool の `MATERIALS` が同じ名前で持つ)。
//!
//! 密度は一般的な値で、品種で変わる(木材は樹種で 0.4〜0.9 g/cm³ ほど)。確定ではない(A)。
//!
//! ボディに付けた材料は、文書の区画 [`BodyMaterials`](`cad.materials`。CAD のモジュール jc-cad が
//! 名乗る)に入る。

use jc_core::{DocumentData, FeatureId, SectionType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 文書の区画 `cad.materials`: ボディに付けた物理材料の名前([`PHYS_MATERIALS`])。鍵はボディを最初に
/// 作った機能(後から機能を足しても変わらない)。無いボディは既定の材料([`DEFAULT_MATERIAL`])。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyMaterials {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bodies: BTreeMap<FeatureId, String>,
    /// このアプリの知らない項目(新しいアプリが足した物)。読んだまま書き戻す
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl SectionType for BodyMaterials {
    const KEY: &'static str = "cad.materials";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "ボディの材料";

    fn check(&self) -> Result<(), String> {
        if self
            .bodies
            .values()
            .any(|n| n.trim().is_empty() || n.trim() != n)
        {
            return Err("材料の名前が空か、前後に空白があります".into());
        }
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.bodies.is_empty() && self.rest.is_empty()
    }
}

impl BodyMaterials {
    /// ボディ `root` に付けた材料(付けていなければ `None`)。
    pub fn get(&self, root: FeatureId) -> Option<&str> {
        self.bodies.get(&root).map(String::as_str)
    }

    /// ボディ `root` の物理材料の名前: 付けた材料、無ければ既定の材料。
    pub fn material_of(&self, root: FeatureId) -> &str {
        self.get(root).unwrap_or(DEFAULT_MATERIAL)
    }

    /// ボディ `root` の材料を `name` にする。空か `None` なら外す。
    pub fn set(&mut self, root: FeatureId, name: Option<&str>) {
        match name.map(str::trim) {
            Some(n) if !n.is_empty() => {
                self.bodies.insert(root, n.to_string());
            }
            _ => {
                self.bodies.remove(&root);
            }
        }
    }
}

/// 文書のボディの材料。区画を読めなければログに残して空(どのボディも既定の材料)。
pub fn of(data: &DocumentData) -> BodyMaterials {
    data.section().unwrap_or_else(|e| {
        log::warn!("{e}");
        BodyMaterials::default()
    })
}

/// 文書のボディ `root` の物理材料の名前(付けていなければ既定の材料)。
pub fn material_of(data: &DocumentData, root: FeatureId) -> String {
    of(data).material_of(root).to_string()
}

/// 物理材料。密度で質量を出し、色はボディの外観の既定にする。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysMaterial {
    pub name: &'static str,
    /// 密度(g/cm³)
    pub density: f64,
    /// 外観の既定の色(sRGB)
    pub color: [u8; 3],
}

/// 知っている物理材料。先頭の 6 つは加工の材料(jc-cam-tool の `MATERIALS`)と同じ名前。
pub const PHYS_MATERIALS: &[PhysMaterial] = &[
    PhysMaterial {
        name: "木材",
        density: 0.6,
        color: [196, 160, 112],
    },
    PhysMaterial {
        name: "MDF",
        density: 0.75,
        color: [170, 138, 100],
    },
    PhysMaterial {
        name: "アクリル",
        density: 1.19,
        color: [200, 222, 232],
    },
    PhysMaterial {
        name: "樹脂",
        density: 1.05,
        color: [226, 226, 216],
    },
    PhysMaterial {
        name: "アルミ",
        density: 2.70,
        color: [196, 200, 206],
    },
    PhysMaterial {
        name: "真鍮",
        density: 8.50,
        color: [204, 172, 92],
    },
    PhysMaterial {
        name: "鋼",
        density: 7.85,
        // 鋼の明るさと色味(青みの無い灰色)
        color: [160, 160, 160],
    },
    PhysMaterial {
        name: "ステンレス",
        density: 7.93,
        color: [178, 182, 186],
    },
    PhysMaterial {
        name: "銅",
        density: 8.96,
        color: [196, 120, 82],
    },
    PhysMaterial {
        name: "PLA",
        density: 1.24,
        color: [236, 236, 230],
    },
];

/// 物理材料を付けていないボディの材料。
/// 質量はこの密度で出し、外観も無ければこの色で塗る。加工のストックの材料の既定にはしない。
pub const DEFAULT_MATERIAL: &str = "鋼";

/// 材料の別名(英語の名前など)と、それが指す材料。
const MATERIAL_ALIASES: &[(&str, &str)] = &[
    ("wood", "木材"),
    ("hardwood", "木材"),
    ("木", "木材"),
    ("acrylic", "アクリル"),
    ("pmma", "アクリル"),
    ("plastic", "樹脂"),
    ("abs", "樹脂"),
    ("プラスチック", "樹脂"),
    ("aluminum", "アルミ"),
    ("aluminium", "アルミ"),
    ("アルミニウム", "アルミ"),
    ("brass", "真鍮"),
    ("steel", "鋼"),
    ("鉄", "鋼"),
    ("stainless", "ステンレス"),
    ("copper", "銅"),
];

/// 材料の名前をそろえる(前後の空白と大文字・小文字を無視し、別名は元の名前にする)。
pub fn material_key(name: &str) -> String {
    let n = name.trim().to_lowercase();
    MATERIAL_ALIASES
        .iter()
        .find(|(a, _)| *a == n)
        .map_or(n, |(_, m)| m.to_lowercase())
}

/// 材料の名前が同じか(「aluminum」と「アルミ」のような別名も同じとみなす)。
pub fn same_material(a: &str, b: &str) -> bool {
    material_key(a) == material_key(b)
}

/// 名前(別名も)で物理材料を引く。
pub fn phys_material(name: &str) -> Option<&'static PhysMaterial> {
    PHYS_MATERIALS.iter().find(|m| same_material(m.name, name))
}

/// 外観(ボディの色)。物理材料の色に加えて、塗装の色を選べる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    pub name: &'static str,
    pub color: [u8; 3],
}

/// 塗装の外観(つやのある塗装。ライブラリの「塗装」の先頭)。
pub const PAINTS: &[Appearance] = &[
    Appearance {
        name: "赤",
        color: [200, 70, 64],
    },
    Appearance {
        name: "橙",
        color: [226, 140, 60],
    },
    Appearance {
        name: "黄",
        color: [230, 200, 80],
    },
    Appearance {
        name: "緑",
        color: [96, 164, 96],
    },
    Appearance {
        name: "青",
        color: [80, 128, 200],
    },
    Appearance {
        name: "紫",
        color: [146, 104, 186],
    },
    Appearance {
        name: "白",
        color: [240, 240, 236],
    },
    Appearance {
        name: "黒",
        color: [52, 52, 56],
    },
];

/// 外観のライブラリの分類(並べる順)。透けるガラス・液体は描けないので置かない。
pub const CATEGORIES: &[&str] = &["金属", "塗装", "樹脂", "木", "ゴム", "石", "その他"];

/// 外観のライブラリの 1 つ(JC の決まった外観。文書の「このデザイン内」の外観で上書きできる)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LibraryLook {
    pub name: &'static str,
    /// [`CATEGORIES`] のどれか
    pub category: &'static str,
    /// 色(sRGB。木目なら早材の色)
    pub color: [u8; 3],
    pub finish: Finish,
}

const fn metal(roughness: f32) -> Finish {
    Finish {
        metallic: 1.0,
        roughness,
        grain: None,
    }
}

const fn plain(roughness: f32) -> Finish {
    Finish {
        metallic: 0.0,
        roughness,
        grain: None,
    }
}

const fn wood(dark: [u8; 3], ring: f32) -> Finish {
    Finish {
        metallic: 0.0,
        roughness: 0.6,
        grain: Some(Grain { dark, ring }),
    }
}

const fn look(
    name: &'static str,
    category: &'static str,
    color: [u8; 3],
    finish: Finish,
) -> LibraryLook {
    LibraryLook {
        name,
        category,
        color,
        finish,
    }
}

/// 外観のライブラリ。先頭は物理材料と同じ名前・色の外観([`PHYS_MATERIALS`] の順)、次に前からある
/// 木 2 つと塗装 8 つ、その後に分類ごとの外観。色と粗さは一般的な見た目に合わせた目安(A)。
pub const LIBRARY: &[LibraryLook] = &[
    // 物理材料と同じ名前の外観(材料の色と質感)
    look("木材", "木", [196, 160, 112], wood([150, 108, 66], 3.2)),
    look("MDF", "木", [170, 138, 100], plain(0.8)),
    look("アクリル", "樹脂", [200, 222, 232], plain(0.08)),
    look("樹脂", "樹脂", [226, 226, 216], plain(0.5)),
    look("アルミ", "金属", [196, 200, 206], metal(0.32)),
    look("真鍮", "金属", [204, 172, 92], metal(0.25)),
    look("鋼", "金属", [160, 160, 160], metal(0.32)),
    look("ステンレス", "金属", [178, 182, 186], metal(0.25)),
    look("銅", "金属", [196, 120, 82], metal(0.25)),
    look("PLA", "樹脂", [236, 236, 230], plain(0.5)),
    // 前からある木と塗装
    look("ウォルナット", "木", [132, 92, 64], wood([70, 44, 28], 2.6)),
    look(
        "メープル",
        "木",
        [226, 200, 160],
        wood([196, 160, 112], 4.0),
    ),
    look("赤", "塗装", [200, 70, 64], PLAIN),
    look("橙", "塗装", [226, 140, 60], PLAIN),
    look("黄", "塗装", [230, 200, 80], PLAIN),
    look("緑", "塗装", [96, 164, 96], PLAIN),
    look("青", "塗装", [80, 128, 200], PLAIN),
    look("紫", "塗装", [146, 104, 186], PLAIN),
    look("白", "塗装", [240, 240, 236], PLAIN),
    look("黒", "塗装", [52, 52, 56], PLAIN),
    // 金属
    look("アルミ - 艶消し", "金属", [186, 190, 196], metal(0.55)),
    look("アルミ - 鏡面", "金属", [212, 214, 218], metal(0.08)),
    look("アルミ - 黒アルマイト", "金属", [44, 44, 48], metal(0.4)),
    look("アルミ - 赤アルマイト", "金属", [206, 52, 58], metal(0.4)),
    look("アルミ - 青アルマイト", "金属", [64, 112, 204], metal(0.4)),
    look("クロムめっき", "金属", [222, 224, 228], metal(0.05)),
    look("亜鉛めっき", "金属", [182, 186, 190], metal(0.45)),
    look("チタン", "金属", [170, 166, 160], metal(0.35)),
    look("鋳鉄", "金属", [108, 108, 110], metal(0.7)),
    look("金", "金属", [230, 190, 90], metal(0.2)),
    look("銀", "金属", [222, 222, 220], metal(0.15)),
    // 塗装
    look("灰", "塗装", [128, 128, 130], PLAIN),
    look("紺", "塗装", [40, 56, 104], PLAIN),
    look("茶", "塗装", [120, 80, 50], PLAIN),
    look("桃", "塗装", [230, 150, 170], PLAIN),
    look("黒 - つや消し", "塗装", [40, 40, 42], plain(0.85)),
    look("白 - つや消し", "塗装", [232, 232, 228], plain(0.85)),
    // 樹脂
    look("ABS - 黒", "樹脂", [36, 36, 38], plain(0.5)),
    look("ABS - 白", "樹脂", [232, 232, 226], plain(0.5)),
    look("PLA - 黒", "樹脂", [30, 30, 32], plain(0.45)),
    look("ナイロン", "樹脂", [232, 228, 214], plain(0.65)),
    look("POM", "樹脂", [238, 236, 230], plain(0.35)),
    // 木
    look("オーク", "木", [200, 160, 110], wood([150, 110, 70], 3.6)),
    look("桜", "木", [190, 120, 90], wood([140, 80, 55], 3.0)),
    look("杉", "木", [214, 170, 120], wood([170, 110, 70], 5.0)),
    look("合板", "木", [214, 184, 140], wood([180, 145, 100], 4.5)),
    // ゴム
    look("ゴム - 黒", "ゴム", [30, 30, 30], plain(0.9)),
    look("ゴム - 灰", "ゴム", [96, 96, 98], plain(0.9)),
    look("シリコーン - 白", "ゴム", [236, 236, 232], plain(0.6)),
    look("シリコーン - 赤", "ゴム", [196, 60, 56], plain(0.6)),
    // 石
    look("コンクリート", "石", [150, 150, 146], plain(0.95)),
    look("大理石 - 白", "石", [236, 234, 228], plain(0.15)),
    look("花崗岩", "石", [120, 116, 112], plain(0.4)),
    look("スレート", "石", [70, 74, 80], plain(0.6)),
    look("れんが", "石", [160, 80, 60], plain(0.9)),
    // その他
    look("紙 - 白", "その他", [244, 244, 240], plain(0.95)),
    look("段ボール", "その他", [190, 150, 100], plain(0.95)),
    look("革 - 茶", "その他", [120, 70, 40], plain(0.6)),
    look("布 - 紺", "その他", [40, 50, 90], plain(1.0)),
    look("炭素繊維", "その他", [40, 40, 44], plain(0.3)),
    look("鏡", "その他", [240, 240, 240], metal(0.02)),
];

/// 名前でライブラリの外観を引く(物理材料の別名も)。
pub fn library_look(name: &str) -> Option<&'static LibraryLook> {
    LIBRARY.iter().find(|l| same_material(l.name, name))
}

/// 外観の一覧(ライブラリの順。先頭は物理材料の色、木、塗装)。
pub fn appearances() -> Vec<Appearance> {
    LIBRARY
        .iter()
        .map(|l| Appearance {
            name: l.name,
            color: l.color,
        })
        .collect()
}

/// 名前で外観を引く(物理材料の名前ならその色)。文書の「このデザイン内」の外観も引くなら
/// [`crate::appearance::Appearances::resolve`]。
pub fn appearance(name: &str) -> Option<Appearance> {
    library_look(name).map(|l| Appearance {
        name: l.name,
        color: l.color,
    })
}

/// 質量(g)。体積は mm³、密度は g/cm³。
pub fn mass_of(volume_mm3: f64, density: f64) -> f64 {
    volume_mm3 * 1e-3 * density
}

/// 見た目の質感(写実の描き方・レンダリング・glTF が使う)。物理ベースの金属・粗さの考え方。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finish {
    /// 金属か(0 は塗装・木・樹脂、1 は金属)
    pub metallic: f32,
    /// 面の粗さ(0 は鏡、1 はつやなし)
    pub roughness: f32,
    /// 木目(年輪)。`None` なら一様な色
    pub grain: Option<Grain>,
}

/// 木目。年輪は木の芯(ボディの外の軸)を中心にした筒で、ボディを長い向きに貫く(板の木取り)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grain {
    /// 年輪の濃い所(晩材)の色(sRGB)。明るい所は外観の色
    pub dark: [u8; 3],
    /// 年輪の間隔(mm)
    pub ring: f32,
}

/// 塗装・樹脂の既定(金属でない、少しつやがある)。
pub const PLAIN: Finish = Finish {
    metallic: 0.0,
    roughness: 0.45,
    grain: None,
};

/// 外観か物理材料の名前の質感(ライブラリの質感)。知らない名前は [`PLAIN`]。
pub fn finish(name: &str) -> Finish {
    library_look(name).map_or(PLAIN, |l| l.finish)
}

/// 木目の向きと芯の位置(ボディの箱から決める)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrainFrame {
    /// 木の芯の軸の上の 1 点
    pub center: [f32; 3],
    /// 芯の向き(X か Y。0 なら X)
    pub axis: usize,
}

impl GrainFrame {
    /// 箱 `min`〜`max` の板の木取り: 芯は横に長い向きに通し、板の下の、厚みの 6 倍ほど離れた所に置く
    /// (板目の模様になる)。少し横にずらして年輪の弧が左右に偏るようにする。
    pub fn of_box(min: [f32; 3], max: [f32; 3]) -> GrainFrame {
        let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        let axis = if size[1] > size[0] { 1 } else { 0 };
        let across = 1 - axis;
        let mut center = [
            (min[0] + max[0]) / 2.0,
            (min[1] + max[1]) / 2.0,
            (min[2] + max[2]) / 2.0,
        ];
        let thick = size[2].max(1.0);
        center[2] = min[2] - 6.0 * thick - 40.0;
        center[across] += 0.23 * size[across].max(10.0);
        GrainFrame { center, axis }
    }
}

/// 3 次元の値のゆらぎ(-1〜1、格子の点の乱数をなめらかにつなぐ)。GPU のシェーダーの `noise3` と同じ式。
pub fn noise3(p: [f32; 3]) -> f32 {
    fn hash(i: [i32; 3]) -> f32 {
        let mut h = (i[0].wrapping_mul(374_761_393))
            .wrapping_add(i[1].wrapping_mul(668_265_263))
            .wrapping_add(i[2].wrapping_mul(1_274_126_177)) as u32;
        h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        h ^= h >> 16;
        (h & 0xffff) as f32 / 32767.5 - 1.0
    }
    let f = p.map(f32::floor);
    let i = f.map(|v| v as i32);
    let t = [0, 1, 2].map(|k| {
        let x = p[k] - f[k];
        x * x * (3.0 - 2.0 * x)
    });
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx: i32, dy: i32, dz: i32| hash([i[0] + dx, i[1] + dy, i[2] + dz]);
    let x00 = lerp(c(0, 0, 0), c(1, 0, 0), t[0]);
    let x10 = lerp(c(0, 1, 0), c(1, 1, 0), t[0]);
    let x01 = lerp(c(0, 0, 1), c(1, 0, 1), t[0]);
    let x11 = lerp(c(0, 1, 1), c(1, 1, 1), t[0]);
    lerp(lerp(x00, x10, t[1]), lerp(x01, x11, t[1]), t[2])
}

/// 木目の点 `p`(mm)の、晩材の濃さ(0 は明るい早材、1 は濃い晩材)。GPU のシェーダーの `grain_of` と同じ式。
/// 年輪は芯からの距離の繰り返しで、ゆらぎで幹のうねりを、芯の向きに細長い筋で導管を足す。
pub fn grain_amount(p: [f32; 3], frame: &GrainFrame, grain: &Grain) -> f32 {
    let along = frame.axis;
    let across = 1 - along;
    let d = [
        p[0] - frame.center[0],
        p[1] - frame.center[1],
        p[2] - frame.center[2],
    ];
    let r = (d[across] * d[across] + d[2] * d[2]).sqrt();
    // うねり: 芯の向きにはゆっくり、断面の向きには少し速く
    let mut q = [0.0; 3];
    q[along] = d[along] * 0.012;
    q[across] = d[across] * 0.03;
    q[2] = d[2] * 0.03;
    let wobble = noise3(q) * 2.2 + noise3(q.map(|v| v * 3.1)) * 0.5;
    let ring = r / grain.ring.max(0.1) + wobble;
    let t = ring - ring.floor();
    // 晩材は年輪の終わりの細い帯(急に濃くなり、次の年輪の始めでまた明るくなる)
    let late = smooth(0.55, 0.85, t) * (1.0 - smooth(0.93, 1.0, t));
    // 導管の筋(芯の向きに細長い)
    let mut s = [0.0; 3];
    s[along] = d[along] * 0.08;
    s[across] = d[across] * 1.6;
    s[2] = d[2] * 1.6;
    let streak = (noise3(s) * 0.5 + 0.5).powi(3) * 0.35;
    (late * 0.85 + streak).clamp(0.0, 1.0)
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 木目の点 `p` の色(sRGB の 0〜1)。`base` は外観の色(早材)。
pub fn grain_color(p: [f32; 3], frame: &GrainFrame, base: [u8; 3], grain: &Grain) -> [f32; 3] {
    let k = grain_amount(p, frame, grain);
    [0, 1, 2].map(|i| {
        let a = base[i] as f32 / 255.0;
        let b = grain.dark[i] as f32 / 255.0;
        a + (b - a) * k
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_names_are_unique_and_sorted_into_known_categories() {
        for (i, l) in LIBRARY.iter().enumerate() {
            assert!(
                CATEGORIES.contains(&l.category),
                "{} の分類 {}",
                l.name,
                l.category
            );
            assert!(
                LIBRARY[..i].iter().all(|o| !same_material(o.name, l.name)),
                "{} が重なる",
                l.name
            );
            // 別名と同じ名前は、別名の指す材料に吸われて引けなくなる
            assert_eq!(library_look(l.name).map(|x| x.name), Some(l.name));
        }
        assert!(LIBRARY.len() >= 50);
    }

    #[test]
    fn material_names_keep_their_colors_and_finishes() {
        for m in PHYS_MATERIALS {
            assert_eq!(
                appearance(m.name).map(|a| a.color),
                Some(m.color),
                "{}",
                m.name
            );
        }
        for p in PAINTS {
            assert_eq!(appearance(p.name).map(|a| a.color), Some(p.color));
            assert_eq!(finish(p.name), PLAIN);
        }
        assert!(finish("wood").grain.is_some());
        assert_eq!(finish("steel").metallic, 1.0);
        assert_eq!(finish("知らない"), PLAIN);
    }

    #[test]
    fn finishes_tell_metal_from_wood_and_grain_makes_rings() {
        assert_eq!(finish("アルミ").metallic, 1.0);
        assert_eq!(finish("aluminum").metallic, 1.0, "別名でも引ける");
        assert!(finish("木材").grain.is_some());
        assert!(finish("ウォルナット").grain.is_some());
        assert_eq!(finish("赤"), PLAIN);
        assert!(appearance("ウォルナット").is_some(), "木の外観も一覧にある");

        // ゆらぎは -1〜1 で、同じ点なら同じ値
        for k in 0..200 {
            let p = [k as f32 * 0.37, k as f32 * -1.13, k as f32 * 0.071];
            let n = noise3(p);
            assert!((-1.0..=1.0).contains(&n));
            assert_eq!(n, noise3(p));
        }
        // 板(100×40×10)の上面を横切ると、明るい所と濃い所が何度も入れ替わる(年輪が見える)
        let frame = GrainFrame::of_box([0.0, 0.0, 0.0], [100.0, 40.0, 10.0]);
        assert_eq!(frame.axis, 0, "長い向き(X)に木目を通す");
        let g = finish("木材").grain.unwrap();
        let vals: Vec<f32> = (0..400)
            .map(|i| grain_amount([50.0, i as f32 * 0.1, 10.0], &frame, &g))
            .collect();
        let flips = vals
            .windows(2)
            .filter(|w| (w[0] > 0.5) != (w[1] > 0.5))
            .count();
        assert!(flips >= 2, "年輪の入れ替わりが {flips} 回");
        assert!(vals.iter().all(|v| (0.0..=1.0).contains(v)));
    }
}
