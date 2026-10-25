//! 加工の使い回し: パラメータを既定として保存(その種類を新しく足す時の初めの値にする)と、
//! テンプレート(幾つかの加工をまとめて保存し、別のセットアップや別の文書で足す)。
//!
//! - 既定は PC 全体の設定 [`DEFAULTS_SETTINGS`](データフォルダの `settings.json`)に、種類ごとの
//!   データで持つ。書き出し・読み込み・初めに戻すもできる。
//! - テンプレートはデータフォルダの `templates` に 1 つ 1 ファイル(`<名前>.json`、平文)で置く
//!   (工具ライブラリの `tools.json` と同じく、PC 全体の物は文書の外に持つ)。
//! - どちらも、その文書でしか意味のない値(セットアップ・工具ライブラリ・面・線・入る位置)は
//!   持たない。使う時に、今の文書の物(選んでいる面、最後のスケッチなど)で埋める。

use crate::commands::{add_operation, last_setup, last_tools, selected_setup};
use crate::{SETUP, ToolLibrary};
use kmb_cam_setup::Strategy;
use kmb_core::{Action, Context, Edit, FeatureId, Input, Outcome, Place, Settings};
use serde_json::{Map, Value as Json, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// 選んでいる加工のパラメータを、その種類の既定にする。
pub const SAVE_DEFAULTS: &str = "cam.defaults.save";
/// 選んでいる加工の種類の既定を消す。
pub const CLEAR_DEFAULTS: &str = "cam.defaults.clear";
/// 既定を全部消す(初めの値に戻す)。
pub const RESET_DEFAULTS: &str = "cam.defaults.reset";
pub const EXPORT_DEFAULTS: &str = "cam.defaults.export";
pub const IMPORT_DEFAULTS: &str = "cam.defaults.import";
/// 既定の設定の名前。
pub const DEFAULTS_SETTINGS: &str = "cam.op_defaults";
/// 選んでいる加工(セットアップなら中の加工すべて)をテンプレートとして保存する。
pub const SAVE_TEMPLATE: &str = "cam.template.save";
/// テンプレートから作る操作の前置き(後ろにテンプレートの名前)。
pub const USE_TEMPLATE: &str = "cam.template.use:";
/// テンプレートを消す操作の前置き(後ろにテンプレートの名前)。
pub const DELETE_TEMPLATE: &str = "cam.template.delete:";
/// テンプレートを置くフォルダ(データフォルダの中)。
pub const TEMPLATE_DIR: &str = "templates";

/// 書き出したファイルの印(読み込む時に、別の JSON と取り違えないように)。
const DEFAULTS_FORMAT: &str = "kie-monobench/加工の既定";
const TEMPLATE_FORMAT: &str = "kie-monobench/加工のテンプレート";
/// 改名の前に書いた加工の既定のファイルの形式の名前(読むだけ)。
const OLD_DEFAULTS_FORMAT: &str = "jisaku-cadcam/加工の既定";

/// その文書の物を指す欄。既定にもテンプレートにも入れず、使う時に今の文書の物で埋める。
const DOCUMENT_KEYS: [&str; 6] = ["setup", "tools", "face", "curves", "profile", "entry"];
/// 既定に入れない欄(工具の番号はライブラリで変わり、ピン穴の線は裏返しの軸で決まる。前の工具
/// の直径を既定にすると、足す加工がみな残り削りになる)。
const NOT_DEFAULT_KEYS: [&str; 5] = ["tool", "rest_tool", "rest_diameter", "rest_corner", "line"];

/// テンプレートの中の加工の 1 つ。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TemplateOp {
    pub type_id: String,
    pub name: String,
    /// 文書の物を指す欄([`DOCUMENT_KEYS`])を除いたデータ
    pub data: Json,
}

/// 加工のテンプレート(ファイルの中身)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Template {
    #[serde(default)]
    pub format: String,
    pub name: String,
    pub operations: Vec<TemplateOp>,
}

/// 既定とテンプレートの状態(画面・CLI・MCP で共有する)。
#[derive(Default)]
pub(crate) struct Reuse {
    /// 既定とテンプレートに使える戦略(戦略が名乗る)
    strategies: Arc<[Strategy]>,
    /// 種類ごとの既定のデータ
    defaults: Arc<RwLock<Map<String, Json>>>,
    data_dir: Arc<RwLock<Option<PathBuf>>>,
    /// テンプレートの名前の一覧と、読んだ時のフォルダの更新時刻(操作の一覧は毎コマ求められるので、
    /// フォルダが変わった時だけ読み直す)
    listed: Arc<RwLock<(Option<std::time::SystemTime>, Vec<String>)>>,
}

/// `data` から欄 `keys` を除いたもの。
fn without(data: &Json, keys: &[&str]) -> Map<String, Json> {
    data.as_object()
        .map(|m| {
            m.iter()
                .filter(|(k, _)| !keys.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// 足す時の初めのデータ `fresh` に、保存した値 `saved` を当てる。`saved` にない欄は、空の欄
/// (「なし」の選び)として保存したものなので外す。保存した後に種類の欄が増えて読めなければ、
/// 初めのデータに重ねるだけにし、それも読めなければ初めのデータのまま。
fn overlay(
    cx: &Context<'_>,
    type_id: &str,
    fresh: &Json,
    saved: &Map<String, Json>,
    keep: &[&str],
) -> Json {
    let ok = |d: &Json| matches!(cx.registry.instantiate(type_id, d), Some(Ok(_)));
    let mut exact: Map<String, Json> = saved.clone();
    let mut layered = fresh.as_object().cloned().unwrap_or_default();
    for (k, v) in saved {
        layered.insert(k.clone(), v.clone());
    }
    if let Some(f) = fresh.as_object() {
        for k in keep {
            match f.get(*k) {
                Some(v) => {
                    exact.insert((*k).to_string(), v.clone());
                    layered.insert((*k).to_string(), v.clone());
                }
                None => {
                    exact.remove(*k);
                    layered.remove(*k);
                }
            }
        }
    }
    let exact = Json::Object(exact);
    if ok(&exact) {
        return exact;
    }
    let layered = Json::Object(layered);
    if ok(&layered) {
        return layered;
    }
    fresh.clone()
}

/// ファイル名に使える名前(Windows で使えない字を `_` にし、前後の空白と末尾の点を除く)。
pub fn template_file_name(name: &str) -> Result<String, String> {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || r#"\/:*?"<>|"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() {
        return Err("テンプレートの名前を入れてください".into());
    }
    Ok(s)
}

/// 選んでいる物の加工(タイムラインの順)。指定 `selection`(番号の一覧)があればそれ、なければ
/// 選んでいる機能。セットアップは中の加工すべてにする。
fn chosen_operations(cx: &Context<'_>) -> Vec<FeatureId> {
    let mut picked: Vec<FeatureId> = cx
        .arg("selection")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_u64()).map(FeatureId).collect())
        .unwrap_or_default();
    if picked.is_empty() {
        picked.extend(cx.selected);
    }
    let setups: Vec<FeatureId> = picked
        .iter()
        .copied()
        .filter(|id| cx.data.feature(*id).is_some_and(|f| f.type_id == SETUP))
        .collect();
    cx.data
        .features
        .iter()
        .filter(|f| crate::is_operation(cx.registry, &f.type_id))
        .filter(|f| {
            picked.contains(&f.id)
                || f.data
                    .get("setup")
                    .and_then(|v| v.as_u64())
                    .is_some_and(|s| setups.contains(&FeatureId(s)))
        })
        .map(|f| f.id)
        .collect()
}

impl Reuse {
    pub(crate) fn new(strategies: Arc<[Strategy]>) -> Self {
        Reuse {
            strategies,
            ..Default::default()
        }
    }

    /// 種類の名前(`cam.pocket2d@1`)から、既定とテンプレートに使える戦略。
    fn kind_of(&self, type_id: &str) -> Option<&Strategy> {
        self.strategies
            .iter()
            .find(|s| s.reuse && s.type_id == type_id)
    }

    /// 種類の名前を画面の名前(「ポケット」)にする。知らない種類はそのまま。
    fn kind_label(&self, type_id: &str) -> String {
        self.kind_of(type_id)
            .map_or(type_id.to_string(), |s| s.label.to_string())
    }

    pub(crate) fn set_data_dir(&self, dir: Option<&Path>) {
        *self.data_dir.write().expect("データフォルダの鍵") = dir.map(Path::to_path_buf);
        *self.listed.write().expect("テンプレートの一覧の鍵") = (None, Vec::new());
    }

    fn template_dir(&self) -> Option<PathBuf> {
        self.data_dir
            .read()
            .expect("データフォルダの鍵")
            .as_ref()
            .map(|d| d.join(TEMPLATE_DIR))
    }

    /// 種類 `type_id` の既定(なければ `None`)。
    pub fn defaults_of(&self, type_id: &str) -> Option<Map<String, Json>> {
        self.defaults
            .read()
            .expect("既定の鍵")
            .get(type_id)?
            .as_object()
            .cloned()
    }

    /// 加工を足す変更 `o` のデータに、その種類の既定を当てる。
    pub(crate) fn with_defaults(&self, cx: &Context<'_>, mut o: Outcome) -> Outcome {
        if let Outcome::Change { edits, .. } = &mut o {
            for e in edits {
                if let Edit::Insert { type_id, data, .. } = e
                    && let Some(saved) = self.defaults_of(type_id)
                {
                    let keep: Vec<&str> = DOCUMENT_KEYS
                        .iter()
                        .chain(&NOT_DEFAULT_KEYS)
                        .copied()
                        .collect();
                    *data = overlay(cx, type_id, data, &saved, &keep);
                }
            }
        }
        o
    }

    /// 既定の設定(設定の窓に出し、`settings.json` に書く)。
    pub(crate) fn settings(&self) -> Settings {
        let map = self.defaults.read().expect("既定の鍵").clone();
        let kinds: Vec<String> = map.keys().map(|t| self.kind_label(t)).collect();
        let mut notes = vec![if kinds.is_empty() {
            "まだありません。加工の右クリックの「パラメータを既定として保存」で、その種類を新しく足す時の初めの値にする".to_string()
        } else {
            format!("保存した既定: {}", kinds.join("、"))
        }];
        notes.push(
            "書き出し・読み込みは、コマンド検索か「管理」の一覧の「加工の既定を書き出す」「加工の既定を読み込む」"
                .into(),
        );
        Settings {
            id: DEFAULTS_SETTINGS.into(),
            label: "加工の既定".into(),
            group: "cam".into(),
            notes,
            fields: vec![],
            value: Json::Object(map),
            default: json!({}),
            role: None,
        }
    }

    /// 既定を `v`(種類ごとのデータの表)にする。形が違えば何も変えない。
    pub(crate) fn set_defaults(&self, v: &Json) -> Result<(), String> {
        let map = v
            .as_object()
            .ok_or("加工の既定は、種類ごとの表にしてください")?;
        if let Some((k, _)) = map.iter().find(|(_, d)| !d.is_object()) {
            return Err(format!("{k} の既定の形が違います"));
        }
        *self.defaults.write().expect("既定の鍵") = map.clone();
        Ok(())
    }

    /// テンプレートの名前の一覧(名前の順)。フォルダが変わった時だけ読み直す。
    pub fn templates(&self) -> Vec<String> {
        let Some(dir) = self.template_dir() else {
            return Vec::new();
        };
        let stamp = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
        {
            let l = self.listed.read().expect("テンプレートの一覧の鍵");
            if l.0.is_some() && l.0 == stamp {
                return l.1.clone();
            }
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "json"))
                    .filter_map(|p| Some(p.file_stem()?.to_string_lossy().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        *self.listed.write().expect("テンプレートの一覧の鍵") = (stamp, names.clone());
        names
    }

    fn template_path(&self, name: &str) -> Result<PathBuf, String> {
        let dir = self
            .template_dir()
            .ok_or("データフォルダがないので、テンプレートを使えません")?;
        Ok(dir.join(format!("{}.json", template_file_name(name)?)))
    }

    /// テンプレート `name` を読む。
    pub fn load_template(&self, name: &str) -> Result<Template, String> {
        let p = self.template_path(name)?;
        let s = std::fs::read_to_string(&p)
            .map_err(|e| format!("{} を読めません: {e}", p.display()))?;
        serde_json::from_str(&s).map_err(|e| format!("{} を読めません: {e}", p.display()))
    }

    /// 選んでいる加工をテンプレート `name` として書く。書いたファイルを返す。
    fn save_template(
        &self,
        cx: &Context<'_>,
        name: &str,
    ) -> Result<(PathBuf, usize, bool), String> {
        let ops = chosen_operations(cx);
        if ops.is_empty() {
            return Err("テンプレートにする加工を選んでいません".into());
        }
        let file = template_file_name(name)?;
        let t = Template {
            format: TEMPLATE_FORMAT.into(),
            name: file.clone(),
            operations: ops
                .iter()
                .filter_map(|id| cx.data.feature(*id))
                .map(|f| TemplateOp {
                    type_id: f.type_id.clone(),
                    name: f.name.clone(),
                    data: Json::Object(without(&f.data, &DOCUMENT_KEYS)),
                })
                .collect(),
        };
        let path = self.template_path(&file)?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{} を作れません: {e}", d.display()))?;
        }
        let existed = path.exists();
        let text = serde_json::to_string_pretty(&t).expect("JSON にできる");
        std::fs::write(&path, text).map_err(|e| format!("{} に書けません: {e}", path.display()))?;
        Ok((path, ops.len(), existed))
    }

    /// テンプレート `name` の加工を、選んでいるセットアップ(なければ最後のセットアップ)に足す。
    fn use_template(&self, cx: &Context<'_>, name: &str) -> Result<Outcome, String> {
        let t = self.load_template(name)?;
        let setup = selected_setup(cx)
            .or_else(|| last_setup(cx))
            .ok_or("先にセットアップを足してください")?;
        let tools = last_tools(cx).ok_or("工具ライブラリがありません")?;
        let lib: Option<ToolLibrary> = cx
            .data
            .feature(tools)
            .and_then(|f| serde_json::from_value(f.data.clone()).ok());
        let has_tool = |n: &Json| {
            n.as_u64().is_some_and(|n| {
                lib.as_ref()
                    .is_some_and(|l| l.tools.iter().any(|t| t.number as u64 == n))
            })
        };
        let mut taken: Vec<String> = cx.data.features.iter().map(|f| f.name.clone()).collect();
        let mut edits = Vec::new();
        for op in &t.operations {
            let kind = self.kind_of(&op.type_id).ok_or_else(|| {
                format!(
                    "「{}」の種類 {} は、このアプリでは足せません",
                    op.name, op.type_id
                )
            })?;
            // 面・線などは、今の文書の物(選んでいる面、最後のスケッチ)で埋める
            let fresh = match add_operation(cx, kind) {
                Ok(Outcome::Change { edits, .. }) => match edits.into_iter().next() {
                    Some(Edit::Insert { data, .. }) => data,
                    _ => return Err(format!("「{}」を足せません", op.name)),
                },
                Ok(_) => return Err(format!("「{}」を足せません", op.name)),
                Err(e) => return Err(format!("テンプレートの「{}」を足せません: {e}", op.name)),
            };
            let mut saved = without(&op.data, &DOCUMENT_KEYS);
            // ライブラリにない工具の番号は、足す時の選び方(種類で選ぶ)に任せる
            for k in ["tool", "rest_tool"] {
                if saved.get(k).is_some_and(|n| !has_tool(n)) {
                    saved.remove(k);
                    if k == "tool"
                        && let Some(v) = fresh.get("tool")
                    {
                        saved.insert(k.into(), v.clone());
                    }
                }
            }
            let mut data = overlay(cx, &op.type_id, &fresh, &saved, &DOCUMENT_KEYS);
            if let Some(m) = data.as_object_mut() {
                m.insert("setup".into(), json!(setup.0));
            }
            let n = kmb_core::unique_name(&op.name, |t| taken.iter().any(|x| x == t));
            taken.push(n.clone());
            edits.push(Edit::Insert {
                type_id: op.type_id.clone(),
                name: n,
                data,
            });
        }
        if edits.is_empty() {
            return Err(format!("テンプレート「{name}」に加工がありません"));
        }
        Ok(Outcome::Change {
            label: format!("テンプレート「{name}」から作成"),
            edits,
        })
    }

    pub(crate) fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let mut v = Vec::new();
        let sel = cx.selected.and_then(|id| cx.data.feature(id));
        let op = sel.filter(|f| crate::is_operation(cx.registry, &f.type_id));
        let on_setup = sel.is_some_and(|f| f.type_id == SETUP);
        let no_dir = self
            .template_dir()
            .is_none()
            .then_some("データフォルダがないので使えません");
        if let Some(f) = op {
            let kind = self.kind_of(&f.type_id).map_or("加工", |s| s.label);
            v.push(
                Action::new(SAVE_DEFAULTS, "パラメータを既定として保存", "cam")
                    .menu("管理")
                    .aliases("きてい default でふぉると しょきち ほぞん")
                    .hint(&format!(
                        "この加工の値(切削条件・残し量・段など)を、次に足す{kind}の初めの値にする。面・工具・セットアップは保存しない"
                    ))
                    .at(&[Place::FeatureMenu]),
            );
            if self.defaults_of(&f.type_id).is_some() {
                v.push(
                    Action::new(CLEAR_DEFAULTS, &format!("{kind}の既定を消す"), "cam")
                        .menu("管理")
                        .aliases("きてい default りせっと reset")
                        .hint(&format!(
                            "保存した既定をやめ、{kind}を初めの値で足すようにする"
                        ))
                        .at(&[Place::FeatureMenu]),
                );
            }
        }
        let empty = self.defaults.read().expect("既定の鍵").is_empty();
        v.extend([
            Action::new(EXPORT_DEFAULTS, "加工の既定を書き出す…", "cam")
                .menu("管理")
                .aliases("きてい default かきだし export")
                .hint("保存した既定を JSON のファイルに書く(別の PC へ持っていく時に)")
                .input(Input::SaveFile {
                    title: "加工の既定を書き出す".into(),
                    filter: "JSON".into(),
                    extensions: vec!["json".into()],
                })
                .disabled_if(empty.then_some("まだ既定を保存していません")),
            Action::new(IMPORT_DEFAULTS, "加工の既定を読み込む…", "cam")
                .menu("管理")
                .aliases("きてい default よみこみ import")
                .hint("書き出した既定のファイルを読む。ファイルにある種類だけ置き換える")
                .input(Input::OpenFile {
                    title: "加工の既定を読み込む".into(),
                    filter: "JSON".into(),
                    extensions: vec!["json".into()],
                }),
            Action::new(RESET_DEFAULTS, "加工の既定を初めに戻す", "cam")
                .menu("管理")
                .aliases("きてい default りせっと reset")
                .hint("保存した既定をすべてやめ、どの加工も初めの値で足す")
                .disabled_if(empty.then_some("まだ既定を保存していません")),
        ]);
        if op.is_some() || on_setup {
            let default = match sel {
                Some(f) if on_setup => format!("{}の加工", f.name),
                Some(f) => f.name.clone(),
                None => String::new(),
            };
            v.push(
                Action::new(SAVE_TEMPLATE, "テンプレートとして保存…", "cam")
                    .menu("管理")
                    .aliases("てんぷれーと template ほぞん ひながた")
                    .hint("選んだ加工(セットアップなら中の加工すべて)を名前を付けて PC に保存し、別のセットアップや文書で使う。面や線は保存せず、使う時に選び直す")
                    .at(&[Place::FeatureMenu])
                    .input(Input::Name {
                        title: "テンプレートとして保存".into(),
                        label: "テンプレートの名前".into(),
                        default,
                    })
                    .disabled_if(no_dir),
            );
        }
        let why = last_setup(cx).is_none().then_some("先にセットアップを足す");
        for name in self.templates() {
            v.push(
                Action::new(
                    &format!("{USE_TEMPLATE}{name}"),
                    &format!("テンプレート「{name}」から作成"),
                    "cam",
                )
                .menu("セットアップ")
                .role_named(kmb_interchange::role::action::USE_TEMPLATE, &name)
                .aliases("てんぷれーと template さくせい")
                .hint("選んでいるセットアップ(なければ最後のセットアップ)に、テンプレートの加工を足す。面の要る加工は、先に 3D のビューで面を選ぶ")
                .disabled_if(why),
            );
            v.push(
                Action::new(
                    &format!("{DELETE_TEMPLATE}{name}"),
                    &format!("テンプレート「{name}」を消す"),
                    "cam",
                )
                .menu("管理")
                .aliases("てんぷれーと template けす さくじょ delete")
                .hint("PC に保存したテンプレートのファイルを消す(文書の加工は変わらない)"),
            );
        }
        v
    }

    pub(crate) fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        if let Some(name) = id.strip_prefix(USE_TEMPLATE) {
            return Some(self.use_template(cx, name));
        }
        if let Some(name) = id.strip_prefix(DELETE_TEMPLATE) {
            return Some(self.template_path(name).and_then(|p| {
                std::fs::remove_file(&p)
                    .map_err(|e| format!("{} を消せません: {e}", p.display()))?;
                Ok(Outcome::Message(format!(
                    "テンプレート「{name}」を消しました"
                )))
            }));
        }
        Some(match id {
            SAVE_TEMPLATE => {
                let name = cx
                    .arg_str("name")
                    .map(str::to_string)
                    .or_else(|| {
                        cx.selected
                            .and_then(|s| Some(cx.data.feature(s)?.name.clone()))
                    })
                    .unwrap_or_default();
                self.save_template(cx, &name).map(|(p, n, existed)| {
                    let name = p.file_stem().map_or(String::new(), |s| s.to_string_lossy().to_string());
                    Outcome::Message(format!(
                        "加工 {n} 個をテンプレート「{name}」に{}しました。セットアップの右クリックの「テンプレートから作成」で使えます",
                        if existed { "上書き" } else { "保存" }
                    ))
                })
            }
            SAVE_DEFAULTS => (|| {
                let f = cx
                    .selected
                    .and_then(|s| cx.data.feature(s))
                    .filter(|f| crate::is_operation(cx.registry, &f.type_id))
                    .ok_or("加工を選んでいません")?;
                let keep: Vec<&str> = DOCUMENT_KEYS
                    .iter()
                    .chain(&NOT_DEFAULT_KEYS)
                    .copied()
                    .collect();
                let saved = without(&f.data, &keep);
                self.defaults
                    .write()
                    .expect("既定の鍵")
                    .insert(f.type_id.clone(), Json::Object(saved));
                let kind = self.kind_of(&f.type_id).map_or("加工", |s| s.label);
                Ok(Outcome::Message(format!(
                    "「{}」の値を{kind}の既定にしました。次に足す{kind}はこの値で始まります(設定の「加工の既定」で戻せます)",
                    f.name
                )))
            })(),
            CLEAR_DEFAULTS => (|| {
                let t = cx.selected_type().ok_or("加工を選んでいません")?;
                self.defaults.write().expect("既定の鍵").remove(t);
                let kind = self.kind_of(t).map_or("加工", |s| s.label);
                Ok(Outcome::Message(format!(
                    "{kind}の既定を消しました。次からは初めの値で足します"
                )))
            })(),
            RESET_DEFAULTS => {
                self.defaults.write().expect("既定の鍵").clear();
                Ok(Outcome::Message("加工の既定をすべて消しました".into()))
            }
            EXPORT_DEFAULTS => {
                let map = self.defaults.read().expect("既定の鍵").clone();
                let n = map.len();
                let v = json!({"format": DEFAULTS_FORMAT, "defaults": map});
                Ok(Outcome::File {
                    suffix: "加工の既定.json".into(),
                    contents: serde_json::to_string_pretty(&v)
                        .expect("JSON にできる")
                        .into_bytes(),
                    info: json!({"kinds": n}),
                })
            }
            IMPORT_DEFAULTS => (|| {
                let p = cx.file.ok_or("読み込むファイルを選んでいません")?;
                let s = std::fs::read_to_string(p)
                    .map_err(|e| format!("{} を読めません: {e}", p.display()))?;
                let v: Json = serde_json::from_str(&s)
                    .map_err(|e| format!("{} を読めません: {e}", p.display()))?;
                let format = v.get("format").and_then(|f| f.as_str());
                if format != Some(DEFAULTS_FORMAT) && format != Some(OLD_DEFAULTS_FORMAT) {
                    return Err(format!(
                        "{} は加工の既定のファイルではありません",
                        p.display()
                    ));
                }
                let new = v.get("defaults").cloned().unwrap_or(json!({}));
                let mut all = self.defaults.read().expect("既定の鍵").clone();
                let incoming = new.as_object().ok_or("既定の形が違います")?;
                for (k, d) in incoming {
                    all.insert(k.clone(), d.clone());
                }
                self.set_defaults(&Json::Object(all))?;
                let kinds: Vec<String> = incoming.keys().map(|t| self.kind_label(t)).collect();
                Ok(Outcome::Message(format!(
                    "加工の既定を読み込みました({})",
                    kinds.join("、")
                )))
            })(),
            _ => return None,
        })
    }
}

#[cfg(test)]
#[path = "reuse_tests.rs"]
mod tests;
