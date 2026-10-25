//! 描画の設定(U26。ゲームのグラフィック設定の考え)。設定の窓の「描画」で選び、画面の設定
//! (jc-ui の `layout.json`)に入る。
//!
//! すぐ効く物(3D の描き方・描く細かさ・アンチエイリアス・動かす間は軽く・網の細かさ・隠れた辺の
//! 濃さ)は、画面が毎フレーム [`set_current`] で入れ、3D のビューが [`current`] で読む(画面を描く
//! 糸ごとに 1 つ。試験が互いに混ざらない)。描画装置と垂直同期は窓を開く前に決まるので、
//! 次の起動から効く(jc-app が起動の前に設定のファイルを読む)。

use serde::{Deserialize, Serialize};
use std::cell::Cell;

/// 3D のビューを描く所。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View3dMode {
    /// ソフトウェアの描画装置(WARP・llvmpipe)なら CPU、ほかは GPU(既定)
    #[default]
    Auto,
    Gpu,
    Cpu,
}

impl View3dMode {
    pub const ALL: [View3dMode; 3] = [View3dMode::Auto, View3dMode::Gpu, View3dMode::Cpu];

    pub fn label(self) -> &'static str {
        match self {
            View3dMode::Auto => "自動",
            View3dMode::Gpu => "GPU",
            View3dMode::Cpu => "CPU",
        }
    }
}

/// 描画装置の選び方(次の起動から)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    /// 描画の部品(wgpu)の既定(高性能の GPU を選ぶ)
    #[default]
    Auto,
    /// 2 つの GPU がある PC で、高性能の方
    HighPerformance,
    /// 2 つの GPU がある PC で、省電力の方(ノート PC の内蔵 GPU)
    LowPower,
    /// ソフトウェアの描画装置(GPU のドライバが壊れた時の逃げ道。遅い)
    Software,
}

impl Device {
    pub const ALL: [Device; 4] = [
        Device::Auto,
        Device::HighPerformance,
        Device::LowPower,
        Device::Software,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Device::Auto => "自動",
            Device::HighPerformance => "高性能の GPU",
            Device::LowPower => "省電力の GPU",
            Device::Software => "ソフトウェア",
        }
    }
}

/// 描画の API(次の起動から)。Windows で、DirectX 12 か Vulkan か、DirectX 12 のシェーダーを
/// どちらの作り方(DXC・FXC)で作るかを選ぶ。ほかの OS では DirectX 12 を選んでいても自動で選ぶ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Api {
    /// DirectX 12。シェーダーを DXC(新しい作り方。アプリの横の dxcompiler.dll)で作る(既定)
    #[default]
    Dx12Dxc,
    /// DirectX 12。シェーダーを FXC(Windows に入っている古い作り方)で作る
    Dx12Fxc,
    Vulkan,
}

impl Api {
    pub const ALL: [Api; 3] = [Api::Dx12Dxc, Api::Dx12Fxc, Api::Vulkan];

    pub fn label(self) -> &'static str {
        match self {
            Api::Dx12Dxc => "DirectX 12(DXC)",
            Api::Dx12Fxc => "DirectX 12(FXC)",
            Api::Vulkan => "Vulkan",
        }
    }

    /// 環境変数 `JC_RENDERER` で選ぶ時の名前。
    pub fn key(self) -> &'static str {
        match self {
            Api::Dx12Dxc => "dxc",
            Api::Dx12Fxc => "fxc",
            Api::Vulkan => "vulkan",
        }
    }

    /// [`Api::key`] から戻す(大文字と小文字は区別しない)。
    pub fn from_key(key: &str) -> Option<Api> {
        let k = key.trim().to_ascii_lowercase();
        Api::ALL.into_iter().find(|a| a.key() == k)
    }
}

/// 形の網の細かさ(面からのずれの上限)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshDetail {
    Coarse,
    /// 0.05 mm(測る道具・MCP の網と同じ)
    #[default]
    Standard,
    Fine,
    VeryFine,
}

impl MeshDetail {
    pub const ALL: [MeshDetail; 4] = [
        MeshDetail::Coarse,
        MeshDetail::Standard,
        MeshDetail::Fine,
        MeshDetail::VeryFine,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MeshDetail::Coarse => "粗い(0.1 mm)",
            MeshDetail::Standard => "標準(0.05 mm)",
            MeshDetail::Fine => "細かい(0.02 mm)",
            MeshDetail::VeryFine => "とても細かい(0.01 mm)",
        }
    }

    /// 網の面からのずれの上限(mm)。
    pub fn tolerance(self) -> f64 {
        match self {
            MeshDetail::Coarse => 0.1,
            MeshDetail::Standard => jc_measure::MESH_TOLERANCE,
            MeshDetail::Fine => 0.02,
            MeshDetail::VeryFine => 0.01,
        }
    }
}

/// まとめて選ぶ質。下の項目を替えると「自分で」になる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Light,
    Standard,
    Quality,
    Custom,
}

impl Preset {
    pub const CHOICES: [Preset; 3] = [Preset::Light, Preset::Standard, Preset::Quality];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Light => "軽さ優先",
            Preset::Standard => "標準",
            Preset::Quality => "見た目優先",
            Preset::Custom => "自分で",
        }
    }
}

/// 選べる描く細かさ(%)。
pub const SCALE_CHOICES: [u32; 5] = [50, 75, 100, 150, 200];
/// 選べるアンチエイリアス(MSAA の数。1 はなし)。
pub const MSAA_CHOICES: [u32; 2] = [1, 4];

/// 描画の設定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, from = "Stored")]
pub struct RenderSettings {
    /// 3D のビューを描く所(すぐ替わる)
    pub view3d: View3dMode,
    /// 描画装置(次の起動から。安全モードが勝つ)
    pub device: Device,
    /// 描画の API(次の起動から。安全モードとソフトウェアの描画装置では使わない)
    pub api: Api,
    /// 描く細かさ(画面の画素に対する %)。下げると速く、上げると線がなめらか
    pub scale: u32,
    /// GPU で描く時のアンチエイリアス(MSAA の数。1 はなし)
    pub msaa: u32,
    /// CPU で描く時、回す・動かす間は細かさを半分にする(止まると元の細かさで描き直す)
    pub dynamic: bool,
    /// 形の網の細かさ
    pub mesh: MeshDetail,
    /// GPU で描く時、面の入り隅を暗くする(環境光の遮蔽。CPU の描画には無い)
    pub ao: bool,
    /// 「シェーディング、隠線エッジ表示」の隠れた辺の濃さ(%)
    pub hidden_dim: u8,
    /// 稜線の太さ(画面の画素)。既定は 1(Kie 2026-10-21)
    pub edge_width: f32,
    /// 垂直同期(次の起動から。切ると fps の上限が外れる)
    pub vsync: bool,
}

impl RenderSettings {
    pub const DEFAULT: RenderSettings = RenderSettings {
        view3d: View3dMode::Auto,
        device: Device::Auto,
        api: Api::Dx12Dxc,
        scale: 100,
        msaa: 4,
        dynamic: true,
        mesh: MeshDetail::Standard,
        ao: true,
        hidden_dim: 40,
        edge_width: 1.0,
        vsync: true,
    };

    /// 今の値に合うまとめての質(どれにも合わなければ「自分で」)。
    pub fn preset(&self) -> Preset {
        Preset::CHOICES
            .into_iter()
            .find(|p| {
                let mut s = self.clone();
                s.apply(*p);
                s == *self
            })
            .unwrap_or(Preset::Custom)
    }

    /// まとめての質 `p` の値にする(描き方・装置・隠れた辺・稜線の太さ・垂直同期は変えない)。
    pub fn apply(&mut self, p: Preset) {
        let (scale, msaa, dynamic, mesh, ao) = match p {
            Preset::Light => (75, 1, true, MeshDetail::Coarse, false),
            Preset::Standard => (100, 4, true, MeshDetail::Standard, true),
            Preset::Quality => (150, 4, false, MeshDetail::Fine, true),
            Preset::Custom => return,
        };
        self.scale = scale;
        self.msaa = msaa;
        self.dynamic = dynamic;
        self.mesh = mesh;
        self.ao = ao;
    }

    /// 描く細かさ(倍率)。読めない値は 1 倍。
    pub fn scale_factor(&self) -> f32 {
        if (25..=400).contains(&self.scale) {
            self.scale as f32 / 100.0
        } else {
            1.0
        }
    }

    /// GPU の MSAA の数(1 か 4 にそろえる)。
    pub fn samples(&self) -> u32 {
        if self.msaa >= 4 { 4 } else { 1 }
    }

    /// 隠れた辺の濃さ(0〜1)。
    pub fn hidden_alpha(&self) -> f32 {
        f32::from(self.hidden_dim.min(100)) / 100.0
    }

    /// 稜線の太さ(画面の画素、0.5〜4)。読めない値は既定。
    pub fn edge_px(&self) -> f32 {
        if self.edge_width.is_finite() {
            self.edge_width
                .clamp(Self::EDGE_WIDTH.0, Self::EDGE_WIDTH.1)
        } else {
            Self::DEFAULT.edge_width
        }
    }

    /// 稜線の太さの範囲(画面の画素)。
    pub const EDGE_WIDTH: (f32, f32) = (0.5, 4.0);
}

/// 保存した設定の読み込み。`ao` は 8e4f09e で足した項目で、それより前の保存には無い。無い時に
/// 既定の真で埋めると、軽さ優先を選んでいた人にも入り隅の陰が付き、質の表示が「自分で」になる。
/// そこで、無い時は他の値が軽さ優先と合えば切る。
#[derive(Deserialize)]
#[serde(default)]
struct Stored {
    view3d: View3dMode,
    device: Device,
    api: Api,
    scale: u32,
    msaa: u32,
    dynamic: bool,
    mesh: MeshDetail,
    ao: Option<bool>,
    hidden_dim: u8,
    edge_width: f32,
    vsync: bool,
}

impl Default for Stored {
    fn default() -> Self {
        let d = RenderSettings::DEFAULT;
        Stored {
            view3d: d.view3d,
            device: d.device,
            api: d.api,
            scale: d.scale,
            msaa: d.msaa,
            dynamic: d.dynamic,
            mesh: d.mesh,
            ao: None,
            hidden_dim: d.hidden_dim,
            edge_width: d.edge_width,
            vsync: d.vsync,
        }
    }
}

impl From<Stored> for RenderSettings {
    fn from(s: Stored) -> Self {
        let mut out = RenderSettings {
            view3d: s.view3d,
            device: s.device,
            api: s.api,
            scale: s.scale,
            msaa: s.msaa,
            dynamic: s.dynamic,
            mesh: s.mesh,
            ao: s.ao.unwrap_or(true),
            hidden_dim: s.hidden_dim,
            edge_width: s.edge_width,
            vsync: s.vsync,
        };
        if s.ao.is_none() {
            let mut light = out.clone();
            light.apply(Preset::Light);
            light.ao = true;
            if light == out {
                out.ao = false;
            }
        }
        out
    }
}

impl Default for RenderSettings {
    fn default() -> Self {
        RenderSettings::DEFAULT
    }
}

thread_local! {
    static CURRENT: Cell<Option<Current>> = const { Cell::new(None) };
}

/// 3D のビューが毎フレーム読む値(`RenderSettings` のうち、すぐ効く物)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Current {
    pub view3d: View3dMode,
    pub scale: f32,
    pub samples: u32,
    pub dynamic: bool,
    pub hidden_alpha: f32,
    pub ao: bool,
    /// 稜線の太さ(画面の画素)
    pub edge_px: f32,
}

impl From<&RenderSettings> for Current {
    fn from(s: &RenderSettings) -> Self {
        Current {
            view3d: s.view3d,
            scale: s.scale_factor(),
            samples: s.samples(),
            dynamic: s.dynamic,
            hidden_alpha: s.hidden_alpha(),
            ao: s.ao,
            edge_px: s.edge_px(),
        }
    }
}

/// 今の描画の設定を入れる(画面が毎フレーム)。
pub fn set_current(s: &RenderSettings) {
    CURRENT.with(|c| c.set(Some(Current::from(s))));
}

/// 今の描画の設定(入れていなければ既定)。
pub fn current() -> Current {
    CURRENT
        .with(|c| c.get())
        .unwrap_or_else(|| Current::from(&RenderSettings::DEFAULT))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 入り隅の陰の項目が無い古い保存: 軽さ優先なら陰を切って軽さ優先のまま、ほかは既定の真。
    #[test]
    fn old_settings_without_ao_keep_their_preset() {
        let light = r#"{"scale":75,"msaa":1,"dynamic":true,"mesh":"coarse"}"#;
        let s: RenderSettings = serde_json::from_str(light).unwrap();
        assert!(!s.ao);
        assert_eq!(s.preset(), Preset::Light);
        let standard: RenderSettings = serde_json::from_str("{}").unwrap();
        assert!(standard.ao);
        assert_eq!(standard.preset(), Preset::Standard);
        // 明示した値はそのまま
        let on = r#"{"scale":75,"msaa":1,"dynamic":true,"mesh":"coarse","ao":true}"#;
        assert!(serde_json::from_str::<RenderSettings>(on).unwrap().ao);
    }

    #[test]
    fn presets_set_the_quality_items_and_custom_is_anything_else() {
        let mut s = RenderSettings::default();
        assert_eq!(s.preset(), Preset::Standard, "既定は標準");
        s.apply(Preset::Light);
        assert_eq!((s.scale, s.samples(), s.mesh), (75, 1, MeshDetail::Coarse));
        assert!(!s.ao, "軽いは入り隅の陰を描かない");
        assert_eq!(s.preset(), Preset::Light);
        // まとめての質に入らない項目は残す
        s.view3d = View3dMode::Cpu;
        s.hidden_dim = 70;
        assert_eq!(s.preset(), Preset::Light);
        s.apply(Preset::Quality);
        assert_eq!((s.view3d, s.hidden_dim), (View3dMode::Cpu, 70));
        s.scale = 200;
        assert_eq!(s.preset(), Preset::Custom, "1 つ替えると自分で");
        // 読めない値は安全な値で使う
        s.scale = 0;
        s.msaa = 3;
        assert_eq!((s.scale_factor(), s.samples()), (1.0, 1));
        // 前の版の設定(描画の欄が無い)は既定で読む
        let old: RenderSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old, RenderSettings::default());
    }

    #[test]
    fn the_current_settings_are_per_thread() {
        let s = RenderSettings {
            scale: 50,
            ..RenderSettings::default()
        };
        set_current(&s);
        assert_eq!(current().scale, 0.5);
        let other = std::thread::spawn(|| current().scale).join().unwrap();
        assert_eq!(other, 1.0, "ほかの糸は既定のまま");
    }

    #[test]
    fn the_api_defaults_to_dxc_and_is_kept_by_presets() {
        // 前の版の設定(API の欄が無い)は DirectX 12(DXC)
        let old: RenderSettings = serde_json::from_str(r#"{"vsync": false}"#).unwrap();
        assert_eq!(old.api, Api::Dx12Dxc);
        let mut s = RenderSettings {
            api: Api::Vulkan,
            ..RenderSettings::default()
        };
        assert!(
            serde_json::to_string(&s)
                .unwrap()
                .contains(r#""api":"vulkan""#)
        );
        s.apply(Preset::Light);
        assert_eq!(s.api, Api::Vulkan, "まとめての質は API を変えない");
        assert_eq!(s.preset(), Preset::Light);
        for a in Api::ALL {
            assert_eq!(Api::from_key(&a.key().to_uppercase()), Some(a));
        }
        assert_eq!(Api::from_key("cpu"), None);
    }
}
