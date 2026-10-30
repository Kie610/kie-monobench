//! マシンライブラリ(機械の値と、値が選ぶポストの対の一覧)。セットアップは機械を ID で選び、
//! 選んだ機械の値を文書に写して持つ(経路とポストが同じ値を使い、文書だけで結果が決まる)。
//!
//! 加工の核(セットアップ・工具・戦略・経路)は機械に寄せず、機械ごとの違い(作業範囲、G-code の
//! 方言、付属品、切削条件の表)は機械の値と、値が選ぶポストに閉じ込める。どの機械を一覧に入れるか
//! は組み立て(kmb-bundle)が決めて [`install`] する。ここに置くのは汎用の機械だけで、値は
//! よくある小型の機械の目安(使う機械に合わせて設定で直す。A)。

use crate::{Laser, LaserDrive, MachineProfile, Post};
use std::sync::Arc;

pub const GENERIC_FANUC: &str = "generic-fanuc";
pub const GENERIC_GRBL: &str = "generic-grbl";
pub const SCULPFUN_G20: &str = "sculpfun-g20";
/// LightBurn に取り込む SVG を書くポスト(G-code で動かない、ガルバノのレーザー向け)
pub const LIGHTBURN_SVG: &str = "lightburn-svg";

/// ライブラリの 1 台: 機械の値と、その機械の値が選ぶポスト。
#[derive(Clone)]
pub struct Machine {
    pub profile: MachineProfile,
    pub post: Arc<dyn Post + Send + Sync>,
}

impl Machine {
    pub fn new(profile: MachineProfile, post: impl Post + Send + Sync + 'static) -> Self {
        Machine {
            profile,
            post: Arc::new(post),
        }
    }
}

/// ファイルからポストを読む関数(WASM のポストのプラグイン。読み手は組み立てが渡す)。
pub type PostLoader = Arc<dyn Fn(&std::path::Path) -> Result<Box<dyn Post>, String> + Send + Sync>;

/// 機械の一覧(一覧の順。最初が新しいセットアップの既定)と、ポストのプラグインの読み手。
#[derive(Clone, Default)]
pub struct MachineLibrary {
    machines: Vec<Machine>,
    loader: Option<PostLoader>,
}

impl MachineLibrary {
    pub fn new(machines: Vec<Machine>) -> Self {
        MachineLibrary {
            machines,
            loader: None,
        }
    }

    /// ポストのプラグインをファイルから読む関数を足す(加工の操作は読み手の実装を知らない)。
    pub fn with_loader(mut self, loader: PostLoader) -> Self {
        self.loader = Some(loader);
        self
    }

    /// ファイル `path` のポストのプラグインを読む。読み手が無い組み立てでは誤り。
    pub fn load_post(&self, path: &std::path::Path) -> Result<Box<dyn Post>, String> {
        match &self.loader {
            Some(f) => f(path),
            None => Err("この組み立てはポストのプラグインを読めません".into()),
        }
    }

    /// 汎用の機械(組み立てが自分の機械の後ろに足す)。
    pub fn generic() -> Vec<Machine> {
        vec![
            Machine::new(generic_fanuc(), crate::generic::Generic::fanuc()),
            Machine::new(generic_grbl(), crate::generic::Generic::grbl()),
            Machine::new(sculpfun_g20(), crate::svg::LightBurnSvg),
        ]
    }

    pub fn profiles(&self) -> Vec<MachineProfile> {
        self.machines.iter().map(|m| m.profile.clone()).collect()
    }

    /// ID の機械の値。知らない ID なら `None`。
    pub fn get(&self, id: &str) -> Option<MachineProfile> {
        self.machines
            .iter()
            .find(|m| m.profile.id == id)
            .map(|m| m.profile.clone())
    }

    /// 機械の値 `post`(ポストの ID)が選ぶポスト。知らない ID なら `None`。
    pub fn post(&self, post: &str) -> Option<Arc<dyn Post + Send + Sync>> {
        self.machines
            .iter()
            .find(|m| m.post.id() == post)
            .map(|m| m.post.clone())
    }

    /// 画面の選び(ID と名前)。
    pub fn choices(&self) -> Vec<(String, String)> {
        self.machines
            .iter()
            .map(|m| (m.profile.id.clone(), m.profile.name.clone()))
            .collect()
    }

    /// 新しいセットアップの既定の機械の ID(一覧の最初)。
    pub fn default_id(&self) -> &str {
        self.machines.first().map_or("", |m| m.profile.id.as_str())
    }
}

/// 汎用の 3 軸(Fanuc 系の書き方: O 番号、`T<n> M6` と `G43 H<n>`、G54〜G59、M0・M1)。
/// 値は中くらいの機械の目安(A)。
pub fn generic_fanuc() -> MachineProfile {
    MachineProfile {
        id: GENERIC_FANUC.into(),
        post: GENERIC_FANUC.into(),
        name: "汎用 3 軸(Fanuc 系)".into(),
        travel: [500.0, 400.0, 200.0],
        max_rpm: 24000.0,
        min_rpm: 0.0,
        tool_numbers: (1, 99),
        rapid_feed: 5000.0,
        acceleration: 0.0,
        power_w: 0.0,
        junction_deviation: 0.0,
        max_rate: [0.0; 3],
        measure_after_tool_change: false,
        air: false,
        collets: Vec::new(),
        bed: None,
        frame: None,
        atc_slots: 0,
        outer: [0.0; 3],
        home: String::new(),
        wcs_count: 6,
        program_number: true,
        optional_stop: true,
        laser: None,
        leveling: None,
        slot_stepdown: Vec::new(),
    }
}

/// 汎用の 3 軸(Grbl 系の書き方: 工具交換の命令がないので止まって待つ、G54〜G59、
/// レーザーは `$32=1` の時の M4 と S0〜1000)。値は小型の機械の目安(A)。
pub fn generic_grbl() -> MachineProfile {
    MachineProfile {
        id: GENERIC_GRBL.into(),
        post: GENERIC_GRBL.into(),
        name: "汎用 3 軸(Grbl 系)".into(),
        travel: [400.0, 400.0, 100.0],
        max_rpm: 12000.0,
        min_rpm: 0.0,
        tool_numbers: (1, 99),
        rapid_feed: 3000.0,
        acceleration: 0.0,
        power_w: 0.0,
        junction_deviation: 0.0,
        max_rate: [0.0; 3],
        measure_after_tool_change: false,
        air: false,
        collets: Vec::new(),
        bed: None,
        frame: None,
        atc_slots: 0,
        outer: [0.0; 3],
        home: String::new(),
        wcs_count: 6,
        program_number: false,
        optional_stop: true,
        laser: Some(Laser {
            s_max: 1000.0,
            enter: Vec::new(),
            exit: Vec::new(),
            on: "M4".into(),
            off: "M5".into(),
            // M4 は止まると消えるので、止まって焼く間だけ M3(出力一定)に替える
            pierce: Some(vec!["M3 S{s}".into(), "G4 P{t}".into(), "M4 S{s}".into()]),
            drive: LaserDrive::Gantry,
            notes: vec![
                "Grbl の設定でレーザーの扱い($32=1)にしておく。S の最大($30)が 1000 の時の値"
                    .into(),
            ],
        }),
        leveling: None,
        slot_stepdown: Vec::new(),
    }
}

/// SCULPFUN G20(20 W のファイバーレーザー、ガルバノ)。加工範囲 150×150 mm、光の点 0.01 mm、
/// 最高 10000 mm/s、周波数 30〜60 kHz は販売元の製品の頁の検索の要約から(A、2026-10-20。頁は
/// この作業場から読めなかった)。G-code では動かず、LightBurn(Pro)が制御盤を動かすので、
/// 書き出しは LightBurn に取り込む SVG にする。焼き始め・終わり・角の遅れとレンズの補正は
/// LightBurn の機械の設定に任せる(二重に補正しないため、経路は形の補正だけを持つ)。
/// 早送りの値は鏡の跳ぶ速さの目安 4000 mm/s(A)。上下は手で合わせる台なので Z は広く取る(A)。
pub fn sculpfun_g20() -> MachineProfile {
    MachineProfile {
        id: SCULPFUN_G20.into(),
        post: LIGHTBURN_SVG.into(),
        name: "SCULPFUN G20(ガルバノのレーザー)".into(),
        travel: [150.0, 150.0, 300.0],
        // 主軸は無い(回す加工は誤りにする)
        max_rpm: 0.0,
        min_rpm: 0.0,
        tool_numbers: (1, 99),
        rapid_feed: 240_000.0,
        acceleration: 0.0,
        power_w: 0.0,
        junction_deviation: 0.0,
        max_rate: [0.0; 3],
        measure_after_tool_change: false,
        air: false,
        collets: Vec::new(),
        bed: None,
        frame: None,
        atc_slots: 0,
        outer: [0.0; 3],
        home: "加工範囲の中心".into(),
        wcs_count: 1,
        program_number: false,
        optional_stop: false,
        laser: Some(Laser {
            s_max: 100.0,
            enter: Vec::new(),
            exit: Vec::new(),
            on: String::new(),
            off: String::new(),
            // SVG には書けない。LightBurn のレイヤーの「始めの止まり」で焼く(書き出しの注意にする)
            pierce: Some(Vec::new()),
            drive: LaserDrive::Galvo,
            notes: vec![
                "LightBurn で SVG を取り込み、レイヤーを「線」にして切り口の補正(Kerf Offset)は 0 にする。補正と進入はこの書き出しで済んでいる".into(),
                "出力・速さ・周波数は LightBurn のレイヤーで決める(SVG は加工ごとのレイヤーの色だけを持つ)。回数は同じ線を重ねて書く".into(),
            ],
        }),
        leveling: None,
        slot_stepdown: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_library_without_a_loader_refuses_plugins() {
        let lib = MachineLibrary::new(MachineLibrary::generic());
        let e = lib.load_post(std::path::Path::new("x.wasm")).err();
        assert!(e.is_some_and(|e| e.contains("プラグインを読めません")));
        let lib = lib.with_loader(Arc::new(|_| {
            Ok(Box::new(crate::generic::Generic::grbl()) as Box<dyn Post>)
        }));
        assert_eq!(
            lib.load_post(std::path::Path::new("x.wasm"))
                .map(|p| p.id().to_string()),
            Ok(GENERIC_GRBL.to_string())
        );
    }
    use crate::SlotStepdown;

    #[test]
    fn the_library_has_unique_ids_and_the_default_comes_first() {
        let library = MachineLibrary::new(MachineLibrary::generic());
        let all = library.profiles();
        assert_eq!(all[0].id, library.default_id());
        let mut ids: Vec<&str> = all.iter().map(|m| m.id.as_str()).collect();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
        assert_eq!(library.choices().len(), all.len());
        for (m, (id, name)) in all.iter().zip(library.choices()) {
            assert_eq!(
                (m.id.as_str(), m.name.as_str()),
                (id.as_str(), name.as_str())
            );
        }
        assert!(library.get("どこにもない").is_none());
        // 機械の値が選ぶポストが一覧にある
        for m in &all {
            assert_eq!(library.post(&m.post).unwrap().id(), m.post);
        }
    }

    #[test]
    fn the_slot_depth_follows_the_diameter_classes() {
        let m = MachineProfile {
            slot_stepdown: vec![SlotStepdown {
                material: "アルミ".into(),
                depths: [0.2, 0.15, 0.1],
            }],
            ..generic_fanuc()
        };
        assert_eq!(m.slot_stepdown("アルミ", 3.175), Some(0.2));
        assert_eq!(m.slot_stepdown("アルミ", 2.0), Some(0.15));
        assert_eq!(m.slot_stepdown("アルミ", 1.0), Some(0.1));
        assert_eq!(m.slot_stepdown("知らない材料", 3.0), None);
        assert_eq!(generic_fanuc().slot_stepdown("アルミ", 3.175), None);
    }
}
