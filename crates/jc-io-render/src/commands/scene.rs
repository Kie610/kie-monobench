//! シーンの設定(環境・背景・大きさ・画質・動画の長さ)と、文書の区画 `render.scene`、それに重ねる引数。

use super::*;

/// シーンの設定。
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub env: EnvKind,
    /// 環境の明るさ・回転・背景・床と、カメラの焦点距離・露出・被写界深度
    pub look: SceneLook,
    /// 静止画の縦横比の鍵(jc_render::settings::ASPECTS)。決まった比なら高さは幅から決める。
    /// 既定はカスタム(幅と高さのまま。縦横比の無かった前の版と同じ大きさの絵になる)
    pub aspect: String,
    /// 静止画の大きさ(画素)
    pub width: u32,
    pub height: u32,
    /// 静止画の画質(光線追跡を回す数)
    pub passes: u32,
    /// 動画の大きさ(画素)
    pub video_width: u32,
    pub video_height: u32,
    /// 動画の長さ(秒。1 周にかける時間)
    pub seconds: f32,
    /// 1 秒あたりの枚数
    pub fps: u32,
    /// 描いた静止画をギャラリー(版の履歴の横)に残す枚数。超えたら古い絵から消す。0 なら残さない
    pub gallery_keep: u32,
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            env: EnvKind::StudioLight,
            look: SceneLook::default(),
            aspect: "custom".into(),
            width: 1600,
            height: 1000,
            passes: 64,
            video_width: 960,
            video_height: 540,
            seconds: 6.0,
            fps: 30,
            gallery_keep: 50,
        }
    }
}

impl Scene {
    /// 透明の背景か。
    pub fn transparent(&self) -> bool {
        self.look.background == Background::Transparent
    }

    /// 静止画の大きさ(画素)。縦横比が決まった比なら高さを幅から、`viewport` なら画面のビューの比
    /// `view_aspect`(横÷縦。分からなければ幅と高さのまま)で決める。
    pub fn still_size(&self, view_aspect: Option<f32>) -> (u32, u32) {
        let r =
            jc_render::settings::aspect_ratio(&self.aspect, self.width, self.height, view_aspect);
        if self.aspect == "custom" {
            return (self.width, self.height);
        }
        let h = (self.width as f32 / r)
            .round()
            .clamp(16.0, jc_render::MAX_SIZE as f32) as u32;
        (self.width, h)
    }

    pub(crate) fn to_json(&self) -> Value {
        let mut v = json!({
            "env": self.env.key(),
            "aspect": self.aspect,
            "width": self.width,
            "height": self.height,
            "passes": self.passes,
            "video_width": self.video_width,
            "video_height": self.video_height,
            "seconds": self.seconds,
            "fps": self.fps,
            "gallery_keep": self.gallery_keep,
        });
        if let Some(m) = v.as_object_mut() {
            self.look.write_json(m);
        }
        v
    }

    /// JSON から。書いていない項目は `base` の値。
    pub fn from_json(v: &Value, base: &Scene) -> Result<Scene, String> {
        let int = |k: &str, cur: u32, lo: u32, hi: u32, what: &str| -> Result<u32, String> {
            match v.get(k) {
                None | Some(Value::Null) => Ok(cur),
                Some(x) => match x.as_f64() {
                    Some(n) if n.fract() == 0.0 && n >= lo as f64 && n <= hi as f64 => Ok(n as u32),
                    _ => Err(format!("{what}は {lo}〜{hi} の整数にしてください")),
                },
            }
        };
        let env = match v.get("env").and_then(Value::as_str) {
            None => base.env,
            Some(n) => EnvKind::from_name(n).ok_or_else(|| {
                let names: Vec<&str> = EnvKind::ALL.iter().map(|e| e.key()).collect();
                format!("env は {} のどれかです", names.join("・"))
            })?,
        };
        let look = SceneLook::from_json(v, &base.look)?;
        let aspect = match v.get("aspect").and_then(Value::as_str) {
            None => base.aspect.clone(),
            Some(a) if jc_render::settings::ASPECTS.iter().any(|x| x.0 == a) => a.to_string(),
            Some(_) => {
                let keys: Vec<&str> = jc_render::settings::ASPECTS.iter().map(|x| x.0).collect();
                return Err(format!("aspect は {} のどれかです", keys.join("・")));
            }
        };
        let seconds = match v.get("seconds") {
            None | Some(Value::Null) => base.seconds,
            Some(x) => match x.as_f64() {
                Some(s) if (0.5..=60.0).contains(&s) => s as f32,
                _ => return Err("動画の長さは 0.5〜60 秒にしてください".into()),
            },
        };
        let max = jc_render::MAX_SIZE;
        Ok(Scene {
            env,
            look,
            aspect,
            width: int("width", base.width, 16, max, "静止画の幅")?,
            height: int("height", base.height, 16, max, "静止画の高さ")?,
            passes: int("passes", base.passes, 1, 4096, "画質(回す数)")?,
            video_width: int("video_width", base.video_width, 16, 1920, "動画の幅")?,
            video_height: int("video_height", base.video_height, 16, 1080, "動画の高さ")?,
            seconds,
            fps: int("fps", base.fps, 1, 50, "1 秒あたりの枚数")?,
            gallery_keep: int(
                "gallery_keep",
                base.gallery_keep,
                0,
                10_000,
                "ギャラリーに残す枚数",
            )?,
        })
    }
}

/// シーンの設定の入れ物。組み立てた物(画面の窓・CLI・MCP の 1 回)ごとに 1 つ持ち、
/// [`jc_core::Registry::provide`] で置く(プロセス全体の static に置かない)。
#[derive(Default)]
pub struct SceneStore(RwLock<Option<Scene>>);

impl SceneStore {
    /// 今のシーンの設定。
    pub fn get(&self) -> Scene {
        self.0
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_default()
    }

    pub(crate) fn set(&self, s: Scene) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = Some(s);
    }
}

/// 文書の区画 `render.scene`: 文書ごとのシーンの設定。
/// 中身は設定の窓と同じ JSON([`Scene::from_json`] の形)。区画の無い文書は、PC 全体の値
/// (前回使った設定、[`SceneStore`])を使う。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SceneSection(pub serde_json::Map<String, Value>);

impl jc_core::SectionType for SceneSection {
    const KEY: &'static str = "render.scene";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "シーンの設定";

    fn check(&self) -> Result<(), String> {
        Scene::from_json(&Value::Object(self.0.clone()), &Scene::default()).map(|_| ())
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 文書 `data` に書いたシーンの設定(書いていなければ `None`。読めなければログに残して `None`)。
pub fn document_scene(data: &jc_core::DocumentData) -> Option<Scene> {
    match data.section::<SceneSection>() {
        Ok(s) if s.0.is_empty() => None,
        Ok(s) => Scene::from_json(&Value::Object(s.0), &Scene::default())
            .inspect_err(|e| log::warn!("文書のシーンの設定を使えません: {e}"))
            .ok(),
        Err(e) => {
            log::warn!("{e}");
            None
        }
    }
}

/// 文書 `data` のシーンの設定。文書に無ければ、組み立てた物の PC 全体の値(前回使った設定。
/// このモジュールが組み立てに無ければ既定)。
pub fn scene(data: &jc_core::DocumentData, registry: &jc_core::Registry) -> Scene {
    document_scene(data).unwrap_or_else(|| {
        registry
            .service::<SceneStore>()
            .map_or_else(Scene::default, |s| s.get())
    })
}

/// シーンの設定と同じ名前で重ねる引数([`Scene::from_json`])と、画面のビューの視点。描く操作
/// (静止画・動画。加工の削る様子の動画も)が受け取る。
pub fn scene_args() -> Vec<Arg> {
    let env: Vec<(&str, &str)> = EnvKind::ALL.iter().map(|e| (e.key(), e.key())).collect();
    let n = |k: &str, help: &str| Arg::integer(k, help);
    vec![
        Arg::choice("env", "光の環境(省くとシーンの設定)", &env),
        Arg::choice(
            "background",
            "背景(省くとシーンの設定)",
            &[
                ("studio", "スタジオ"),
                ("color", "単色"),
                ("transparent", "透明"),
            ],
        ),
        Arg::text(
            "background_color",
            "単色の背景の色 #rrggbb(省くとシーンの設定)",
        ),
        Arg::number(
            "brightness",
            "環境の明るさ(lx。省くとシーンの設定)",
            jc_core::FieldUnit::Plain("lx".into()),
        ),
        Arg::number(
            "exposure",
            "露出(EV。省くとシーンの設定)",
            jc_core::FieldUnit::Plain("EV".into()),
        ),
        n("width", "静止画の幅(画素)"),
        n("height", "静止画の高さ(画素)"),
        n("passes", "画質(光線追跡を回す数 1〜4096)"),
        n("video_width", "動画の幅(画素)"),
        n("video_height", "動画の高さ(画素)"),
        Arg::number(
            "seconds",
            "動画の長さ(0.5〜60 秒)",
            jc_core::FieldUnit::Plain("s".into()),
        ),
        n("fps", "1 秒あたりの枚数(1〜50)"),
        n(
            "gallery_keep",
            "ギャラリーに残す枚数(0〜10000。0 なら残さない。省くとシーンの設定)",
        ),
        Arg::object(
            "camera",
            "画面のビューの視点 {eye, right, up, toward, fov, ortho_height, shift}(画面が渡す)",
        ),
        Arg::object("view", "視点の向き {eye, up}(camera が無い時。省くと等角)"),
        Arg::list(
            "exclude",
            "描かないボディ(機能の番号の並び。画面は目を閉じたボディを渡す)",
        ),
    ]
}

pub(crate) fn settings_group(current: &Scene) -> jc_core::Settings {
    let envs: Vec<(&str, &str)> = EnvKind::ALL.iter().map(|e| (e.key(), e.label())).collect();
    let aspects: Vec<(&str, &str)> = jc_render::settings::ASPECTS
        .iter()
        .map(|a| (a.0, a.1))
        .collect();
    jc_core::Settings {
        id: SCENE_SETTINGS.into(),
        label: "レンダリング".into(),
        group: "render".into(),
        notes: vec![
            "静止画は光線追跡で描き、ノイズを取る。画質(回す数)を上げるほどきれいで時間がかかる"
                .into(),
            "シーンの設定は文書ごとに文書へ保存する(取り消せる)。まだ変えていない文書と新しい文書は、前回使った設定"
                .into(),
            "回す動画はボディのまわりを 1 周する。コマは速い描き方(影あり)".into(),
            "MP4 は何も入れずに書ける。Windows では OS の H.264(どこでも再生できる)、使えない Windows\
             (N 版など)とほかの OS では AV1(使用料なし。Windows では Microsoft Store の無料の\
             「AV1 Video Extension」か VLC などで再生)"
                .into(),
        ],
        fields: vec![
            jc_core::Field::choice("env", "環境", &envs),
            jc_core::Field::number("brightness", "明るさ(lx)").plain("lx").help(
                "環境の光の強さ(0〜100000 lx、既定 1000)。単色の背景の色には効かない",
            ),
            jc_core::Field::number("env_rotation", "環境の回転(°)").plain("°").help(
                "光の向きを Z のまわりに回す(-360〜360 度。光は視点の水平の向きについてくる)",
            ),
            jc_core::Field::choice(
                "background",
                "背景",
                &[
                    ("studio", "環境"),
                    ("color", "単色"),
                    ("transparent", "透明(PNG・GIF)"),
                ],
            ),
            jc_core::Field::text("background_color", "背景の色")
                .help("#rrggbb(例 #c8c8c8)")
                .only("background", "color", "studio"),
            jc_core::Field::bool("ground", "グラウンド面")
                .help("床を描いて影を落とす(透明の背景では描かない)"),
            jc_core::Field::bool("ground_reflection", "反射")
                .help("床に映り込む(静止画とキャンバス内レンダリング。動画は映らない)"),
            jc_core::Field::number("ground_roughness", "粗さ")
                .unitless()
                .help("床の映り込みのぼけ(0〜1。0 は鏡)"),
            jc_core::Field::number("focal_length", "焦点距離(mm)").plain("mm").help(
                "35 mm 判の焦点距離(5〜1000 mm、既定 45)。レンダリングの作業空間の透視投影のビューにも効く",
            ),
            jc_core::Field::number("exposure", "露出(EV)").plain("EV").help(
                "-15〜25 EV(既定 9.5)。1 上げると半分の明るさ",
            ),
            jc_core::Field::bool("depth_of_field", "被写界深度").help(
                "回す中心に焦点を合わせ、手前と奥をぼかす(光線追跡の静止画とキャンバス内レンダリング。透視投影の時だけ)",
            ),
            jc_core::Field::number("blur", "ぼかし")
                .unitless()
                .help("被写界深度のぼけの強さ(0〜10、既定 1)"),
            jc_core::Field::choice("aspect", "縦横比", &aspects),
            jc_core::Field::integer("width", "静止画の幅(画素)").plain("画素"),
            jc_core::Field::integer("height", "静止画の高さ(画素)")
                .plain("画素")
                .only("aspect", "custom", "custom"),
            jc_core::Field::integer("passes", "静止画の画質(回す数)").unitless(),
            jc_core::Field::integer("video_width", "動画の幅(画素)").plain("画素"),
            jc_core::Field::integer("video_height", "動画の高さ(画素)").plain("画素"),
            jc_core::Field::number("seconds", "動画の長さ(秒、1 周)").plain("秒"),
            jc_core::Field::integer("fps", "1 秒あたりの枚数").unitless(),
            jc_core::Field::integer("gallery_keep", "ギャラリーに残す枚数")
                .unitless()
                .help("描いた静止画を版の履歴の横に残す枚数(0〜10000、既定 50)。超えたら古い絵から消す。0 なら残さない"),
        ],
        value: current.to_json(),
        default: Scene::default().to_json(),
        role: Some(jc_interchange::role::settings::RENDER_SCENE),
    }
}
