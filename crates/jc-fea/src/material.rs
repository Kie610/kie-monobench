//! 解析に使う材料の値。名前はコアの物理材料の表([`jc_look::material::PHYS_MATERIALS`])と同じで、
//! 密度はそちらから引く。
//!
//! 値は一般的な文献値の目安(A)で、品種・樹種・造形の向きで大きく変わる。木・MDF・PLA は
//! 向きで強さが大きく違う(直交異方性)が、ここでは等方性として扱い、注意を出す。木の値は
//! 米国森林製品研究所の『Wood Handbook』(FPL-GTR-282)の広葉樹の繊維方向の値の範囲から取った。

/// 解析の材料。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeaMaterial {
    pub name: &'static str,
    /// 縦弾性係数(MPa)
    pub e: f64,
    /// ポアソン比
    pub nu: f64,
    /// 安全率に使う強さ(MPa)。金属は降伏応力、樹脂・木は引張か曲げの強さ
    pub strength: f64,
    /// 強さの種類(画面に出す)
    pub strength_kind: &'static str,
    /// 熱伝導率 W/(m·K)
    pub k: f64,
    /// 向きで強さが大きく違う材料(等方性で扱う注意を出す)
    pub anisotropic: bool,
}

pub const FEA_MATERIALS: &[FeaMaterial] = &[
    FeaMaterial {
        name: "アルミ",
        e: 69_000.0,
        nu: 0.33,
        strength: 276.0,
        strength_kind: "降伏応力(6061-T6)",
        k: 167.0,
        anisotropic: false,
    },
    FeaMaterial {
        name: "鋼",
        e: 205_000.0,
        nu: 0.29,
        strength: 245.0,
        strength_kind: "降伏応力(一般構造用)",
        k: 50.0,
        anisotropic: false,
    },
    FeaMaterial {
        name: "ステンレス",
        e: 193_000.0,
        nu: 0.29,
        strength: 205.0,
        strength_kind: "降伏応力(304)",
        k: 16.0,
        anisotropic: false,
    },
    FeaMaterial {
        name: "真鍮",
        e: 100_000.0,
        nu: 0.34,
        strength: 200.0,
        strength_kind: "降伏応力",
        k: 120.0,
        anisotropic: false,
    },
    FeaMaterial {
        name: "銅",
        e: 117_000.0,
        nu: 0.34,
        strength: 70.0,
        strength_kind: "降伏応力(焼きなまし)",
        k: 390.0,
        anisotropic: false,
    },
    FeaMaterial {
        name: "木材",
        e: 11_000.0,
        nu: 0.3,
        strength: 80.0,
        strength_kind: "曲げ強さ(繊維方向)",
        k: 0.15,
        anisotropic: true,
    },
    FeaMaterial {
        name: "MDF",
        e: 3_500.0,
        nu: 0.25,
        strength: 30.0,
        strength_kind: "曲げ強さ",
        k: 0.12,
        anisotropic: false,
    },
    FeaMaterial {
        name: "アクリル",
        e: 3_000.0,
        nu: 0.37,
        strength: 70.0,
        strength_kind: "引張強さ",
        k: 0.19,
        anisotropic: false,
    },
    FeaMaterial {
        name: "樹脂",
        e: 2_200.0,
        nu: 0.35,
        strength: 40.0,
        strength_kind: "引張強さ(ABS)",
        k: 0.17,
        anisotropic: false,
    },
    FeaMaterial {
        name: "PLA",
        e: 3_500.0,
        nu: 0.36,
        strength: 50.0,
        strength_kind: "引張強さ(層の向きで大きく下がる)",
        k: 0.13,
        anisotropic: true,
    },
];

/// 研究を足した時の材料(物理材料の既定の鋼にそろえる。違うと、材料を選ばない時に質量・部品表と
/// 別の材料で解く)。
pub const DEFAULT_MATERIAL: &str = "鋼";

/// 名前(コアの別名も)で材料を引く。
pub fn fea_material(name: &str) -> Option<&'static FeaMaterial> {
    let name = if name.trim().is_empty() {
        DEFAULT_MATERIAL
    } else {
        name
    };
    FEA_MATERIALS
        .iter()
        .find(|m| jc_look::material::same_material(m.name, name))
}

/// 密度(g/cm³、コアの物理材料の表から)。
pub fn density(m: &FeaMaterial) -> f64 {
    jc_look::material::phys_material(m.name).map_or(1.0, |p| p.density)
}

/// 選ぶ欄の候補。
pub fn choices() -> Vec<(&'static str, &'static str)> {
    FEA_MATERIALS.iter().map(|m| (m.name, m.name)).collect()
}

/// 向きの注意(等方性で扱う材料)。
pub fn anisotropy_note(m: &FeaMaterial) -> Option<String> {
    m.anisotropic.then(|| {
        format!(
            "{} は向きで強さが大きく違います(木は繊維の向きと直角で縦弾性係数が 10 倍以上違い、3D プリントは層の間で弱い)。ここでは繊維・層の向きの値で等方性として扱うので、直角の向きの力には甘い結果になります",
            m.name
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_material_has_a_density_in_the_core_table() {
        for m in FEA_MATERIALS {
            assert!(
                jc_look::material::phys_material(m.name).is_some(),
                "{} がコアの物理材料の表に無い",
                m.name
            );
        }
        assert_eq!(fea_material("aluminum").unwrap().name, "アルミ");
        assert_eq!(fea_material("").unwrap().name, DEFAULT_MATERIAL);
        // 物理材料の表に無い名前は解析の表にも無い(質量・部品表と同じく、知らない材料として止まる)
        assert!(jc_look::material::phys_material("謎の合金").is_none());
        assert!(fea_material("謎の合金").is_none());
    }

    /// 研究の既定の材料は、質量・部品表・描き方の既定の物理材料と同じ(食い違うと、同じ部品で
    /// 質量と解析の材料が別になる)。
    #[test]
    fn the_default_material_is_the_core_default() {
        assert!(jc_look::material::same_material(
            DEFAULT_MATERIAL,
            jc_look::material::DEFAULT_MATERIAL
        ));
    }
}
