//! 画面の配置(作業空間、パネルの幅と開閉、ツールバーのピン、マウスの割り当て)。
//! PC ごとに覚え、データフォルダの `layout.json` に書く。文書には入れない。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// 作業空間。設計(形を作る)、加工(経路を作る)、レンダリング(写実の絵と動画)、
/// アニメーション(分解とジョイントの動きの段の帯と、その動画)、解析(強さ・熱・揺れを調べる)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Space {
    #[default]
    Design,
    Machining,
    Render,
    Animation,
    Simulation,
}

impl Space {
    /// 作業空間の一覧に並べる順
    pub const ALL: [Space; 5] = [
        Space::Design,
        Space::Machining,
        Space::Render,
        Space::Animation,
        Space::Simulation,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Space::Design => "設計",
            Space::Machining => "加工",
            Space::Render => "レンダリング",
            Space::Animation => "アニメーション",
            Space::Simulation => "解析",
        }
    }

    /// 作業空間の色(設計は青緑、加工は銅色、解析は紫)。
    pub fn color(self) -> egui::Color32 {
        match self {
            Space::Design => egui::Color32::from_rgb(38, 166, 154),
            Space::Machining => egui::Color32::from_rgb(196, 120, 64),
            Space::Render => egui::Color32::from_rgb(124, 92, 196),
            Space::Animation => egui::Color32::from_rgb(196, 146, 32),
            Space::Simulation => egui::Color32::from_rgb(128, 100, 200),
        }
    }

    /// この作業空間の操作の `group`(モジュールの操作の分け方)。
    pub fn action_group(self) -> &'static str {
        match self {
            Space::Design => "cad",
            Space::Machining => "cam",
            Space::Render => "render",
            Space::Animation => "anim",
            Space::Simulation => "fea",
        }
    }
}

/// ホイールを押してドラッグした時の動き。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddleDrag {
    /// 平行に動かす(既定)
    #[default]
    Pan,
    /// 回す
    Orbit,
}

/// 画面の明るさ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Brightness {
    /// PC の明暗の設定に合わせる(既定)
    #[default]
    System,
    Light,
    Dark,
}

impl Brightness {
    pub fn preference(self) -> egui::ThemePreference {
        match self {
            Brightness::System => egui::ThemePreference::System,
            Brightness::Light => egui::ThemePreference::Light,
            Brightness::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// 3D のビューの表示の形・投影・描画の設定(3D のビューの部品が持つ)。
pub use kmb_ui_view3d::{Projection, RenderSettings, Shading};

/// 投影を読む。前の版は「透視投影か」の真偽(`perspective`)で持っていた。
fn projection_compat<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Projection, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        Old(bool),
        New(Projection),
    }
    Ok(match Either::deserialize(d)? {
        Either::Old(true) => Projection::Perspective,
        Either::Old(false) => Projection::Parallel,
        Either::New(p) => p,
    })
}

/// 部品の間の広さ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    Normal,
    Roomy,
}

impl Density {
    pub const ALL: [Density; 3] = [Density::Compact, Density::Normal, Density::Roomy];

    pub fn label(self) -> &'static str {
        match self {
            Density::Compact => "詰める",
            Density::Normal => "普通",
            Density::Roomy => "広い",
        }
    }

    /// 部品の間の広さの倍率。
    pub fn factor(self) -> f32 {
        match self {
            Density::Compact => 0.6,
            Density::Normal => 1.0,
            Density::Roomy => 1.5,
        }
    }
}

/// 角の丸み。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corners {
    Square,
    Slight,
    #[default]
    Normal,
    Round,
}

impl Corners {
    pub const ALL: [Corners; 4] = [
        Corners::Square,
        Corners::Slight,
        Corners::Normal,
        Corners::Round,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Corners::Square => "四角",
            Corners::Slight => "少し",
            Corners::Normal => "普通",
            Corners::Round => "丸い",
        }
    }

    /// 角の丸みの倍率。
    pub fn factor(self) -> f32 {
        match self {
            Corners::Square => 0.0,
            Corners::Slight => 0.5,
            Corners::Normal => 1.0,
            Corners::Round => 2.0,
        }
    }
}

/// 画面の動き(開閉・切替のアニメーションと、視点の向きの移り変わり)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    #[default]
    Full,
    Reduced,
    Off,
}

impl Motion {
    pub const ALL: [Motion; 3] = [Motion::Full, Motion::Reduced, Motion::Off];

    pub fn label(self) -> &'static str {
        match self {
            Motion::Full => "する",
            Motion::Reduced => "減らす",
            Motion::Off => "しない",
        }
    }

    /// 動きの時間の倍率。
    pub fn factor(self) -> f32 {
        match self {
            Motion::Full => 1.0,
            Motion::Reduced => 0.5,
            Motion::Off => 0.0,
        }
    }
}

/// 画面の見た目の設定(設定の「画面」。U38)。UI 全体の大きさは egui の表示倍率(Ctrl+= と Ctrl+-
/// でも変わり、窓の位置と一緒に覚える)なので、ここには持たない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// テーマ。`standard`(既定)・`high_contrast`、利用者のテーマは `user:<ファイルの名前>`
    pub theme: String,
    /// 字の大きさの倍率(0.5〜2.5)
    pub text_scale: f32,
    /// アイコンの大きさの倍率(0.5〜2.5)
    pub icon_scale: f32,
    pub density: Density,
    pub corners: Corners,
    /// 字の書体のファイル(`None` なら OS の字から自動で選ぶ)
    pub font: Option<String>,
    pub motion: Motion,
    /// ツールバーのボタンに名前を出すか(出さなければアイコンだけ。名前は指を置くと出る。
    /// Kie 2026-10-21: 既定は出す)
    pub tool_labels: bool,
    /// アイコンの変わる部分を役の色(作る・変える・増やす・決める)で描くか。切ると土台も
    /// 変わる部分も字の色の 1 色(前の見た目)
    pub role_colors: bool,
}

/// 字とアイコンの倍率の範囲(UI の大きさと同じ 50〜250 %。Kie 2026-10-19)。
pub const SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=2.5;

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: crate::themes::STANDARD.to_string(),
            text_scale: 1.0,
            icon_scale: 1.0,
            density: Density::Normal,
            corners: Corners::Normal,
            font: None,
            motion: Motion::Full,
            tool_labels: true,
            role_colors: true,
        }
    }
}

impl Appearance {
    /// 大きさの倍率(範囲の外の値は範囲に収める)。
    pub fn scale(&self) -> kmb_ui_view3d::Scale {
        let clamp = |v: f32| {
            if v.is_finite() {
                v.clamp(*SCALE_RANGE.start(), *SCALE_RANGE.end())
            } else {
                1.0
            }
        };
        kmb_ui_view3d::Scale {
            text: clamp(self.text_scale),
            icon: clamp(self.icon_scale),
            corner: self.corners.factor(),
            motion: self.motion.factor(),
        }
    }
}

/// 新しい文書の既定の値(候補 19)。新しい文書・新しいタブ・起動の時の無題の文書の設計の属性に入れる。
/// 見本と開いたファイルには入れない。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NewDoc {
    /// 長さの単位
    pub units: kmb_core::LengthUnit,
    /// 作者
    pub designer: String,
    /// 所属
    pub organization: String,
    /// 著作権
    pub copyright: String,
    /// 使用条件
    pub license: String,
}

impl NewDoc {
    /// 文字の欄(設計の属性の鍵, 画面の名前)。
    pub const TEXTS: [(&'static str, &'static str); 4] = [
        ("designer", "作者"),
        ("organization", "所属"),
        ("copyright", "著作権"),
        ("license", "使用条件"),
    ];

    pub fn text_mut(&mut self, key: &str) -> Option<&mut String> {
        Some(match key {
            "designer" => &mut self.designer,
            "organization" => &mut self.organization,
            "copyright" => &mut self.copyright,
            "license" => &mut self.license,
            _ => return None,
        })
    }

    /// 新しい文書の設計の属性。
    pub fn properties(&self) -> kmb_core::Properties {
        let mut p = kmb_core::Properties {
            units: self.units,
            ..Default::default()
        };
        for (key, _) in NewDoc::TEXTS {
            let v = match key {
                "designer" => &self.designer,
                "organization" => &self.organization,
                "copyright" => &self.copyright,
                _ => &self.license,
            };
            p.set(key, v);
        }
        p
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub space: Space,
    pub browser_open: bool,
    pub browser_width: f32,
    /// 選んだだけでも編集パネルを出すか(切なら、道具を使う間だけ出す。前の `panel_open` は読まない)
    pub panel_always: bool,
    pub panel_width: f32,
    pub timeline_open: bool,
    /// コマンド欄を出すか(初めは隠す)
    #[serde(default)]
    pub commands_open: bool,
    /// データパネル(最近・プロジェクトのデザインの細い一覧)を出すか(初めは隠す。上の帯の左端のボタン)
    #[serde(default)]
    pub data_open: bool,
    /// ツールバーのまとまりごとの前に出すボタン(並び順)。ないまとまりは既定のまま。
    /// 鍵は「作業空間/タブ/まとまり」、値は項目の鍵([`crate::ribbon::Item::key`])
    pub pins: BTreeMap<String, Vec<String>>,
    /// タブの並び順(作業空間ごと)。ないものは既定の順
    pub tab_order: BTreeMap<String, Vec<String>>,
    pub middle_drag: MiddleDrag,
    /// 回し方: 真ならフリーオービット(上下の軸に縛らない)、偽なら拘束オービット(Z が上。既定)
    pub free_orbit: bool,
    /// 3D マウスの速さ・割り当て(設定の「マウス」)
    pub mouse3d: crate::mouse3d::Mouse3dSettings,
    pub brightness: Brightness,
    /// 3D のビューの表示の形
    pub shading: Shading,
    /// 写実の見た目(材質の質感と環境の光。表示の「効果」)
    pub effects: Effects,
    /// 3D のビューの投影(平行・透視・自動)。前の版の `perspective`(真偽)も読む
    #[serde(alias = "perspective", deserialize_with = "projection_compat")]
    pub projection: Projection,
    /// 3D のビューに XY 平面の格子を描くか
    pub grid: bool,
    /// 3D のビューに原点と軸を描くか(空の文書では基準面も)
    pub origin: bool,
    /// 測る道具で、距離の X・Y・Z ごとの成分を出すか(表の行とビューの線)
    pub measure_xyz: bool,
    /// 3D のビューの右下に画面の速さ(fps と 1 回の処理の時間)を出すか
    pub show_fps: bool,
    /// 自動保存の間隔(分。0 なら自動保存しない。U11)
    pub autosave_minutes: u32,
    /// 画面の見た目(テーマ・大きさ・角・詰め具合・字の書体・動き。U38)
    pub appearance: Appearance,
    /// 付け替えたキー(キーの行の名前 → キーの名前の並び。空の並びは外した行。U36)。
    /// 書いていない行は既定の割り当て([`crate::keys`])
    pub keys: BTreeMap<String, Vec<String>>,
    /// 設定の窓で開いている頁(例「general/appearance」。U39)
    pub settings_page: String,
    /// 描画の設定(3D の描き方・描画装置・細かさ・アンチエイリアス・網の細かさなど。U26)
    pub render: RenderSettings,
    /// STEP を読み込んだ後に、Y が上の形を立てるかを聞くか(U12)
    pub ask_step_up: bool,
    /// 画面の部品の並び(`egui_dock` の並びの JSON。`None` は既定の並び。N4)
    pub dock: Option<serde_json::Value>,
    /// 窓の帯を Windows の標準にするか(既定はアプリが描く帯。N4)
    pub system_title_bar: bool,
    /// Claude(MCP の app の道具)からの接続を受けるか(候補 21。既定は受けない)
    pub ai_live: bool,
    /// 新しい文書の既定の値(単位・作者など。候補 19)
    pub new_doc: NewDoc,
    /// アプリの外の窓へ出した部品と、その窓の場所と大きさ(画面の論理の px で x, y, 幅, 高さ)
    pub outer: std::collections::BTreeMap<crate::dock_ui::Pane, [f32; 4]>,
    /// 表の窓で出す列(表の種類の鍵 → 出す列の鍵を出す順に)。無い表は既定の列
    pub table_columns: BTreeMap<String, Vec<String>>,
    /// 外観の窓の「お気に入り」(外観の名前。足した順)
    pub favorite_looks: Vec<String>,
}

/// 写実の見た目(表示の「効果」)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Effects {
    /// 外観・材料を付けたボディを、材質の質感(木目・金属の映り込み・つや)で描く
    pub materials: bool,
    /// 環境の光の名前(`kmb_render::EnvKind::key`)
    pub env: String,
}

impl Default for Effects {
    fn default() -> Self {
        Effects {
            materials: true,
            env: kmb_render::EnvKind::StudioLight.key().into(),
        }
    }
}

impl Effects {
    pub fn env_kind(&self) -> kmb_render::EnvKind {
        kmb_render::EnvKind::from_name(&self.env).unwrap_or_default()
    }
}

/// 設定で選べる自動保存の間隔(分。0 はしない)。
pub const AUTOSAVE_CHOICES: [u32; 8] = [0, 1, 2, 5, 10, 15, 30, 60];

impl Default for Layout {
    fn default() -> Self {
        Layout {
            space: Space::Design,
            browser_open: true,
            browser_width: 250.0,
            panel_always: false,
            panel_width: 312.0,
            timeline_open: true,
            commands_open: false,
            data_open: false,
            pins: BTreeMap::new(),
            tab_order: BTreeMap::new(),
            middle_drag: MiddleDrag::Pan,
            free_orbit: false,
            mouse3d: Default::default(),
            brightness: Brightness::System,
            shading: Shading::ShadedEdges,
            effects: Effects::default(),
            projection: Projection::Parallel,
            grid: true,
            origin: true,
            measure_xyz: true,
            show_fps: false,
            autosave_minutes: 5,
            appearance: Appearance::default(),
            keys: BTreeMap::new(),
            settings_page: String::new(),
            render: RenderSettings::default(),
            ask_step_up: true,
            dock: None,
            system_title_bar: false,
            ai_live: false,
            new_doc: NewDoc::default(),
            outer: std::collections::BTreeMap::new(),
            table_columns: BTreeMap::new(),
            favorite_looks: Vec::new(),
        }
    }
}

const FILE: &str = "layout.json";

/// 前の名前と今の名前(まとまり・タブ・拘束の名前)。docs/用語表.md。
const RENAMED_WORDS: &[(&str, &str)] = &[
    ("描く", "作成"),
    ("直す", "修正"),
    ("3D 描く", "3D 作成"),
    ("3D 直す", "3D 修正"),
    ("調べる", "検査"),
    ("読み込み", "挿入"),
    ("直角", "直交"),
    ("接する", "接線"),
    ("同心", "同心円"),
    ("同一直線", "同一直線上"),
];

impl Layout {
    /// データフォルダから読む。なければ・読めなければ既定の配置。
    pub fn load(dir: &Path) -> Layout {
        let mut l: Layout = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        l.rename_words();
        l
    }

    /// 画面の言葉を用語表(docs/用語表.md)に合わせた時(2026-10-19)、前の名前で覚えた
    /// ピンとタブの並びを今の名前へ読み替える。読み替えないと、利用者が前に出したボタンが外れる。
    fn rename_words(&mut self) {
        let part = |s: &str| {
            RENAMED_WORDS
                .iter()
                .find(|(old, _)| *old == s)
                .map_or(s.to_string(), |(_, new)| new.to_string())
        };
        let key = |k: &str| k.split('/').map(part).collect::<Vec<_>>().join("/");
        let item = |k: &str| match k.strip_prefix("c:") {
            Some(name) => format!("c:{}", part(name)),
            None => k.to_string(),
        };
        self.pins = std::mem::take(&mut self.pins)
            .into_iter()
            .map(|(k, v)| (key(&k), v.iter().map(|i| item(i)).collect()))
            .collect();
        for names in self.tab_order.values_mut() {
            for n in names.iter_mut() {
                *n = part(n);
            }
        }
    }

    /// 書き込みは kmb-session の置き換え書き(途中で落ちても前の配置が残る)。
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{} を作れません: {e}", dir.display()))?;
        let s = serde_json::to_string_pretty(self).expect("JSON にできる");
        kmb_session::write_atomic(&dir.join(FILE), &s, false)
    }

    /// 配置を既定に戻す(作業空間とマウス・キーの割り当て、明るさと見た目、3D の表示の形と投影は残す)。
    pub fn reset_arrangement(&mut self) {
        *self = Layout {
            space: self.space,
            middle_drag: self.middle_drag,
            free_orbit: self.free_orbit,
            mouse3d: self.mouse3d.clone(),
            brightness: self.brightness,
            shading: self.shading,
            effects: self.effects.clone(),
            projection: self.projection,
            grid: self.grid,
            origin: self.origin,
            show_fps: self.show_fps,
            autosave_minutes: self.autosave_minutes,
            appearance: self.appearance.clone(),
            keys: self.keys.clone(),
            settings_page: self.settings_page.clone(),
            render: self.render.clone(),
            ask_step_up: self.ask_step_up,
            system_title_bar: self.system_title_bar,
            new_doc: self.new_doc.clone(),
            ..Layout::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pins_and_tab_order_saved_with_the_old_words() {
        let dir = std::env::temp_dir().join(format!("kmb-layout-words-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FILE),
            r#"{"pins": {"design/スケッチ/直す": ["c:直角", "trim"], "design/ソリッド/読み込み": []},
               "tab_order": {"design": ["調べる", "ソリッド"]}}"#,
        )
        .unwrap();
        let l = Layout::load(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(l.pins["design/スケッチ/修正"], vec!["c:直交", "trim"]);
        assert!(l.pins.contains_key("design/ソリッド/挿入"));
        assert_eq!(l.tab_order["design"], vec!["検査", "ソリッド"]);
    }

    #[test]
    fn remembers_widths_and_pins_and_resets_only_the_arrangement() {
        let dir = std::env::temp_dir().join(format!("kmb-layout-{}", std::process::id()));
        let mut l = Layout {
            space: Space::Machining,
            browser_width: 310.0,
            middle_drag: MiddleDrag::Orbit,
            shading: Shading::Wireframe,
            projection: Projection::Perspective,
            grid: false,
            ..Layout::default()
        };
        l.pins
            .insert("design/ソリッド/作成".into(), vec!["a:cad.extrude".into()]);
        l.save(&dir).unwrap();
        let back = Layout::load(&dir);
        assert_eq!(back, l);
        let mut r = back.clone();
        r.reset_arrangement();
        assert_eq!(r.browser_width, Layout::default().browser_width);
        assert!(r.pins.is_empty());
        assert_eq!(
            (r.space, r.middle_drag),
            (Space::Machining, MiddleDrag::Orbit)
        );
        assert_eq!(
            (r.shading, r.projection),
            (Shading::Wireframe, Projection::Perspective)
        );
        assert!(!r.grid && r.origin);
        // 前の版のファイル(表示の形と投影、格子と原点がない)は既定で読む
        let old: Layout = serde_json::from_str(r#"{"browser_width": 280.0}"#).unwrap();
        assert_eq!(
            (old.shading, old.projection, old.grid, old.origin),
            (Shading::ShadedEdges, Projection::Parallel, true, true)
        );
        // 前の版の「透視投影か」の真偽を読む。新しい版は 3 つの名前で書く
        let old: Layout = serde_json::from_str(r#"{"perspective": true}"#).unwrap();
        assert_eq!(old.projection, Projection::Perspective);
        let auto = Layout {
            projection: Projection::Auto,
            ..Layout::default()
        };
        let json = serde_json::to_string(&auto).unwrap();
        assert!(json.contains(r#""projection":"auto""#), "{json}");
        assert_eq!(serde_json::from_str::<Layout>(&json).unwrap(), auto);
        // 自動保存の間隔のない前の版は 5 分。配置を既定に戻しても間隔は残す
        assert_eq!(old.autosave_minutes, 5);
        let mut a = Layout {
            autosave_minutes: 15,
            ..Layout::default()
        };
        a.keys.insert("extrude".into(), vec!["W".into()]);
        a.reset_arrangement();
        assert_eq!(a.autosave_minutes, 15);
        assert_eq!(
            a.keys["extrude"],
            vec!["W".to_string()],
            "キーの割り当ては残す"
        );
        // 壊れたファイルは既定の配置にする
        std::fs::write(dir.join(FILE), "{").unwrap();
        assert_eq!(Layout::load(&dir), Layout::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
