//! rmcp の道具の定義。中身は [`Session`] にあり、ここでは引数と結果の形だけを決める。

use crate::session::{Reply, Session};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Server {
    session: Arc<Mutex<Session>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ImportArg {
    /// 作業フォルダからの相対パス(作業フォルダと --allow で許したフォルダの外は不可)
    pub path: String,
    /// 真なら、Y が上で作られた形を X 軸のまわりに 90 度回して Z が上に立てる(他の CAD の多くは Y が上)
    #[serde(default)]
    pub stand_up: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct RollBackArg {
    /// 先頭から効かせる機能の数(0 から機能の数)。省くと最後まで進める
    #[serde(default)]
    pub position: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct PropertiesArg {
    /// 変える属性(鍵と値)。決まった鍵は title(部品名)・part_number(部品番号)・revision(改訂)・
    /// designer(作者)・organization(所属)・description(説明)・material(材料)・copyright(著作権)・
    /// license(使用条件)・project(作品)・genre(ジャンル)・tags(タグ。「,」で区切る)。
    /// それ以外の鍵は自分で足す項目。空の値は消す。省くと今の属性を返すだけ
    #[serde(default)]
    pub set: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct VersionsArg {
    /// 節目の名前を付ける版(指紋か版の番号)。省くと一覧を返すだけ
    #[serde(default)]
    pub milestone_of: Option<String>,
    /// 節目の名前(空なら節目を外す)
    #[serde(default)]
    pub milestone: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct RestoreVersionArg {
    /// 戻す版(versions の id か number)
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct MaterialArg {
    /// 物理材料の名前(木材・MDF・アクリル・樹脂・アルミ・真鍮・鋼・ステンレス・銅・PLA。英語の別名も可)。
    /// 空なら外す
    pub material: String,
    /// ボディを作った機能か、その後の機能の ID。省くと見えているボディすべて
    #[serde(default)]
    pub body: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct PathArg {
    /// 作業フォルダからの相対パス(作業フォルダと --allow で許したフォルダの外は不可)
    pub path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CrashArg {
    /// 報告のファイル名(一覧の name)。省くと一覧
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SampleArg {
    /// 加工(工具、セットアップ、加工の機能)も入れる
    #[serde(default)]
    pub cam: bool,
    /// 面取りとフィレットを足し、走査線と等高線の 3D 仕上げも入れる(cam を含む)
    #[serde(default)]
    pub three_d: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct SaveArg {
    /// 保存先。省くと開いたファイルへ上書き
    pub path: Option<String>,
    /// 別のファイルが既にあっても上書きする
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct ParamArg {
    /// パラメータ名
    pub name: String,
    /// 式。例: "60 mm"、"width / 2 + 3 mm"
    pub expr: String,
    /// パラメータの表のコメント(何のための値か)。省くと今のまま
    #[serde(default)]
    pub comment: Option<String>,
    /// 単位: length・angle・unitless・any。省くと今のまま(足す時は any)
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ValueArg {
    /// 機能の ID(status の values[].feature)
    pub feature: u64,
    /// 値の欄(status の values[].key。例: "distance")
    pub key: String,
    /// 式。数だけなら今の単位を補う。例: "12"、"thickness + 2 mm"
    pub expr: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ViewsArg {
    /// 表の番号(status の param_views[].id)。0 なら「全部」の表。省くと作った表すべて
    pub id: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct EditViewArg {
    /// "create"(作る)・"rename"(名前を変える)・"remove"(表を消す。中のパラメータは残る)・
    /// "add"(行を入れる)・"hide"(行を外す)
    pub action: String,
    /// 表の番号(create のほかは要る)
    pub id: Option<u64>,
    /// 表の名前(create・rename。create で省くとボディの名前から付ける)
    pub name: Option<String>,
    /// create: このボディ(機能の ID。topology の bodies[].id でもよい)に関わる行を決まりで入れる
    pub body: Option<u64>,
    /// 行: パラメータか寸法の名前("width"、"d1")、機能の値は "機能の ID:欄"("5:distance")
    #[serde(default)]
    pub rows: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RenameArg {
    pub name: String,
    pub new_name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct IdArg {
    /// 機能の ID(status の features[].id)
    pub id: u64,
}

#[derive(Deserialize, JsonSchema)]
pub struct AddFeatureArg {
    /// 機能の種類(types で一覧)。例: "solid.extrude@1"
    pub type_id: String,
    /// タイムラインに出す名前
    pub name: Option<String>,
    /// 機能のデータ(種類ごとの JSON。get_feature で既存のものを見られる)
    pub data: serde_json::Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct SetFeatureArg {
    pub id: u64,
    /// 新しいデータ(全体を置き換える)
    pub data: serde_json::Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct MoveFeaturesArg {
    /// 動かす機能の ID(status の features[].id)。互いの順は保ち、1 か所に並べる
    pub ids: Vec<u64>,
    /// この機能の前へ動かす
    pub target: u64,
    /// true ならこの機能の後ろへ
    #[serde(default)]
    pub after: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct SuppressArg {
    pub id: u64,
    /// true で抑制、false で解除
    pub suppressed: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct PasteFeaturesArg {
    /// 写す機能の ID(status の features[].id)。ボディを作る・変える機能だけ
    pub ids: Vec<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct MoveComponentArg {
    /// 移す部品の番号(status の components[].id)。中の部品も一緒に動く
    pub id: u64,
    /// 入れる親の部品の番号(省くと文書の直下)。before・after が無い時に使う
    #[serde(default)]
    pub into: Option<u64>,
    /// この部品のすぐ前(同じ親の並び)へ移す
    #[serde(default)]
    pub before: Option<u64>,
    /// この部品のすぐ後ろ(同じ親の並び)へ移す
    #[serde(default)]
    pub after: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ActivateComponentArg {
    /// アクティブにする部品の番号(省くと文書の直下へ戻す)
    #[serde(default)]
    pub component: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct MoveBodiesArg {
    /// 移すボディ(そのボディを作った・変えたどれかの機能の ID)
    pub ids: Vec<u64>,
    /// 入れる部品の番号(省くと部品から外す)
    #[serde(default)]
    pub component: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SaveViewArg {
    /// 視点の名前。空なら「視点N」、同じ名前があれば後ろに番号を足す
    #[serde(default)]
    pub name: String,
    /// 画面の真ん中に来る点(mm)
    pub target: [f64; 3],
    /// 水平の回り(度。X 軸から反時計回り。-90 で前から)
    pub yaw: f64,
    /// 見下ろす角度(度、-90〜90)
    pub pitch: f64,
    /// 画面の上での傾き(度、既定 0)
    #[serde(default)]
    pub roll: f64,
    /// 画面の縦に見える高さ(mm)
    pub height: f64,
    /// 透視投影で見るか(既定は平行投影)
    #[serde(default)]
    pub perspective: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct RenameViewArg {
    /// 視点の番号(named_views の id)
    pub id: u64,
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct GroupArg {
    /// まとめる機能の ID(status の features[].id)
    pub ids: Vec<u64>,
    /// グループの名前
    pub name: String,
    /// 選んだ機能が続いていない時: "exact"(断る、既定)、"gaps"(間も含める)、"gather"(寄せてからまとめる)
    #[serde(default)]
    pub mode: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ExportArg {
    /// 書き出し先。拡張子で形式を決める(.stl・.step・.stp・.3mf・.glb・.dxf・.svg(今の視点の線)・
    /// .png・.mp4・.gif(レンダリング))
    pub path: String,
    /// 使う書き出しの操作の ID(actions で一覧)。省くと、ボディの書き出しのうち拡張子が合う最初の物
    pub action: Option<String>,
    /// 視点(.dxf・.svg の線と、レンダリング)。"top"・"bottom"・"front"・"back"・"right"・"left"・"iso" か
    /// {"eye": [x, y, z], "up": [x, y, z]}
    pub view: Option<serde_json::Value>,
    /// 三角形・線の許容差(mm)。省くと画面から書き出した時と同じ
    pub tolerance: Option<f64>,
    /// 真ならボディごとに別のファイルにする(path の名前の後ろに -<ボディの名前> を付ける)
    #[serde(default)]
    pub each: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct GcodeArg {
    /// セットアップの機能の ID。省くと最初のセットアップ
    pub setup: Option<u64>,
    /// 書き出し先(.nc など)。省くと G-code を結果に入れて返す
    pub out: Option<String>,
    /// ポストのプラグイン(WASM のコンポーネント)のパス。省くと Carvera Air
    pub post: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SimArg {
    pub setup: Option<u64>,
    /// 格子の大きさ(mm)。省くと画面と同じ(設定の格子。荒削りは荒い格子)。小さいほど正確で遅い
    pub cell: Option<f64>,
}

/// 測る物 1 つ。`point` か、`body` と `face`・`edge`・`center` のどれか 1 つで指す。
#[derive(Deserialize, JsonSchema)]
pub struct MeasureTarget {
    /// ボディの機能の ID(topology の bodies[].id)
    pub body: Option<u64>,
    /// 面の名前(topology の faces[].name)
    pub face: Option<String>,
    /// 辺の番号(topology の edges[].index)
    pub edge: Option<usize>,
    /// 円・円弧の辺の中心を点として使う(辺の番号)
    pub center: Option<usize>,
    /// 点の座標 [x, y, z](mm)
    pub point: Option<[f64; 3]>,
}

impl MeasureTarget {
    pub fn pick(&self) -> kmb_session::MeasurePick {
        kmb_session::MeasurePick {
            body: self.body,
            face: self.face.clone(),
            edge: self.edge,
            center: self.center,
            point: self.point,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct MeasureArg {
    /// 測る物(1 つか 2 つ)
    pub targets: Vec<MeasureTarget>,
    /// 丸い物(円・円柱)が入っている時の測り方: "center"(中心か軸から、既定)、"min"(最短)、"max"(最長)
    pub basis: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TopologyArg {
    /// ボディの機能の ID。省くと残ったボディ全部
    pub body: Option<u64>,
}

/// 目立たせる面(topology の名前)。
#[derive(Deserialize, JsonSchema)]
pub struct FaceRef {
    /// ボディの機能の ID(topology の bodies[].id)
    pub body: u64,
    /// 面の名前(topology の faces[].name)
    pub face: String,
}

/// 目立たせる辺(topology の番号)。
#[derive(Deserialize, JsonSchema)]
pub struct EdgeRef {
    pub body: u64,
    /// 辺の番号(topology の edges[].index)
    pub edge: usize,
}

#[derive(Deserialize, JsonSchema)]
pub struct AppArg {
    /// 画面の道具: status(今のタブの文書の要約と選んでいる物)・screenshot・probe(screenshot の絵の
    /// 画素に見えている面・点・法線)・selection・
    /// set_param・add_param・set_value・undo・redo・actions(操作の一覧と引数の宣言)・run(操作を
    /// ID で実行する。画面のボタンと同じ道)。画面の部品を名前で探し、人と同じマウスと
    /// キーの入力で操作する道具: ui_tree・ui_find・ui_click・ui_type・ui_key・ui_select・ui_pick・
    /// ui_expect。コマンド欄のコマンドを流す: command(1 行)・script(何行も。途中で失敗したら
    /// 残りは流さない。コマンドの一覧はコマンド `help`)。running(受けてまだ答えていない頼みの一覧。
    /// 画面が固まっていても答え、時間切れの頼みも画面が終えるまで載せる)
    pub tool: String,
    /// 道具の引数。set_param と add_param は {name, expr, comment?, unit?}、set_value は {feature, key, expr}、
    /// run は {id, args}(args は actions の宣言のとおり。宣言に無い鍵は誤り)、
    /// screenshot は {width, height}(省くと 800×600)、probe は {x, y, width, height}(x, y は同じ
    /// width・height の screenshot の絵の画素。左上から。答えは hit・ref(selection と同じ形)・point_mm・
    /// normal。選択・視点・文書は変えない)。部品の名前 target は画面の字のまま
    /// (`~字` で含む物、`button:名前` で役割を絞る、`名前#2` で 2 番目)。ui_tree は {filter}、
    /// ui_find は {target}、ui_click は {target, button: left|right|double}、ui_type は
    /// {target(欄の名前), text, commit: tab|enter|none}、ui_key は {keys}(例 "ctrl+z")、
    /// ui_select は {target, option}、ui_pick は {face}(3D のビューの面の名前)、ui_expect は
    /// {target, equals | contains | state(exists・missing・enabled・disabled)}、command は {line}、
    /// script は {lines: [..]} か {text}。command・script・ui_* は途中で失敗すると頼む前の形へ
    /// 戻す(答えに rolled_back: true)。残すなら {keep_on_error: true}
    #[serde(default)]
    pub args: serde_json::Value,
}

/// 動いている画面へ要求を送る(答えを待つ間、ほかの道具を止めない)。
/// 画面で動く操作のファイルの引数は、`roots`(AI に許したフォルダ)の中に限る。
pub fn call_app(
    dir: Option<std::path::PathBuf>,
    roots: &kmb_session::roots::Roots,
    a: AppArg,
) -> CallToolResult {
    let r = match dir {
        Some(d) => kmb_session::live::call_confined(&d, &a.tool, a.args, roots),
        None => Err("データフォルダが分からないので、画面を探せません".into()),
    };
    match r {
        Ok(mut v) => {
            let png = v
                .as_object_mut()
                .and_then(|o| o.remove("png"))
                .and_then(|p| p.as_str().map(str::to_string));
            let mut out = vec![ContentBlock::text(
                serde_json::to_string_pretty(&v).expect("JSON にできる"),
            )];
            if let Some(png) = png {
                out.push(ContentBlock::image(png, "image/png"));
            }
            CallToolResult::success(out)
        }
        Err(e) => problem(e),
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ScreenshotArg {
    /// 視点: "iso"(等角、既定)・"iso_back"・"top"・"bottom"・"front"・"back"・"right"・"left"
    pub view: Option<String>,
    /// 向きを度で決める時のヨー(Z のまわり。-90 で前から)。pitch と一緒に使い、view より先
    pub yaw: Option<f32>,
    /// 向きを度で決める時のピッチ(上から見下ろす角度)
    pub pitch: Option<f32>,
    /// 名前を付けた視点の番号(named_views の id)。その向きで描く。yaw・pitch・view より先
    pub named_view: Option<u64>,
    /// 絵の幅(画素、既定 800、16〜2048)
    pub width: Option<u32>,
    /// 絵の高さ(画素、既定 600、16〜2048)
    pub height: Option<u32>,
    /// 描くボディの機能の ID。省くと残ったボディ全部
    pub body: Option<u64>,
    /// 選んだ色で目立たせる面
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    /// 太い差し色で目立たせる辺
    #[serde(default)]
    pub edges: Vec<EdgeRef>,
    /// PNG を書き出す先。省くと絵を結果に入れるだけ
    pub out: Option<String>,
}

/// 誤りの答え。文はそのまま読めるように content に、種類(input・compute・found・unavailable・file)は
/// structuredContent の `kind` に入れる(相手が文の字を見ずに分けられるように)。
pub(crate) fn problem(e: impl Into<kmb_core::Problem>) -> CallToolResult {
    let p: kmb_core::Problem = e.into();
    let mut r = CallToolResult::error(vec![ContentBlock::text(p.message.clone())]);
    r.structured_content = Some(serde_json::json!({"kind": p.kind, "message": p.message}));
    r
}

fn result(r: Reply) -> Result<CallToolResult, ErrorData> {
    Ok(match r {
        Ok(v) => CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&v).expect("JSON にできる"),
        )]),
        Err(e) => problem(e),
    })
}

#[derive(Deserialize, JsonSchema)]
pub struct BatchArg {
    /// 順に動かす手(1〜200 手)。例: [{"tool": "set_param", "args": {"name": "width", "expr": "70 mm"}}, {"tool": "mass"}]
    pub steps: Vec<crate::batch::Step>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ActionsArg {
    /// 選んでいるとする機能の ID(省くと何も選ばない)
    pub selected: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SettingsArg {
    /// 設定の名前(settings の id)。例: "cam.machine"
    pub id: String,
    /// 変える項目と値(settings の fields の key)。書かない項目は今の値のまま。例: {"rapid_feed": 2500}
    pub value: serde_json::Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct RunActionArg {
    /// 操作の ID(actions の id)。例: "cam.add.pocket"
    pub id: String,
    /// 選んでいるとする機能の ID
    pub selected: Option<u64>,
    /// 読み込むファイル(input が open_file の操作)
    pub file: Option<String>,
    /// 書き出し先(ファイルを作る操作)。省くと中身を結果の contents に入れる(文字のファイルだけ。
    /// 3MF・STL・PNG などは書き出し先が要る)
    pub out: Option<String>,
    /// 操作ごとの指定(例: G-code の post、シミュレーションの cell、STL の tolerance)。受け取る鍵・型・
    /// 単位・既定は actions の args の宣言のとおりで、宣言に無い鍵や型の違いは誤り。長さの数は mm、
    /// 角度は deg。文字で "0.5 in" のように単位つきでも書ける
    #[serde(default)]
    pub args: serde_json::Value,
}

impl Server {
    pub fn new(session: Session) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
            tool_router: Self::tool_router(),
        }
    }

    /// 道具の名前の一覧(試験で batch の読み方と突き合わせる)。
    #[cfg(test)]
    pub(crate) fn tool_names() -> Vec<String> {
        Self::tools().into_iter().map(|t| t.0).collect()
    }

    /// 道具の名前と説明(入出力の一覧 `crate::reference` が使う)。
    pub fn tools() -> Vec<(String, String)> {
        Self::tool_router()
            .list_all()
            .into_iter()
            .map(|t| {
                let about = t.description.as_deref().unwrap_or_default().to_string();
                (t.name.to_string(), about)
            })
            .collect()
    }

    /// 絵と説明を返す(説明は JSON の文字、絵は PNG)。
    /// 画面の道具(`app`)。文書の錠は道具の答えを待つ間は持たない。
    async fn app_tool(&self, a: AppArg) -> Result<CallToolResult, ErrorData> {
        let (dir, roots) = {
            let s = self.session.lock().unwrap_or_else(|e| e.into_inner());
            (
                s.data_dir().map(std::path::Path::to_path_buf),
                s.roots().clone(),
            )
        };
        tokio::task::spawn_blocking(move || call_app(dir, &roots, a))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))
    }

    async fn run_picture(
        &self,
        f: impl FnOnce(&mut Session) -> Result<(Vec<u8>, serde_json::Value), String> + Send + 'static,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self.session.clone();
        let r = tokio::task::spawn_blocking(move || {
            let mut s = session.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut s)
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(match r {
            Ok((png, info)) => CallToolResult::success(vec![
                ContentBlock::text(serde_json::to_string_pretty(&info).expect("JSON にできる")),
                ContentBlock::image(crate::session::base64(&png), "image/png"),
            ]),
            Err(e) => problem(e),
        })
    }

    /// 形状の計算は重いので、非同期の実行器を止めない所で動かす。
    async fn run(
        &self,
        f: impl FnOnce(&mut Session) -> Reply + Send + 'static,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self.session.clone();
        let r = tokio::task::spawn_blocking(move || {
            let mut s = session.lock().unwrap_or_else(|e| e.into_inner());
            let r = f(&mut s);
            // クラッシュ報告に、最近の操作と文書を残す
            kmb_session::crash::note(s.document(), s.work.active_component());
            r
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        result(r)
    }
}

/// 道具の表から、MCP の道具と batch の 1 手の読み方を一緒に作る(道具を足す所は表の 1 か所)。
/// 1 つの道具は `"説明" fn 名前(s, a: 引数の型) 種類 => 中身;` と書く。`s` は文書の
/// [`Session`](種類が server の時は [`Server`])、`a` は読んだ引数。種類は次のどれか。
/// - `edit`: 文書を読む・変える道具。batch に入れられる
/// - `value`: パラメータと機能の値の式を直すだけの道具。batch では続く間の作り直しを後へ回す
/// - `picture`: 絵と説明を返す道具。batch の結果には説明だけを入れる
/// - `outside "理由"`: batch に入れない道具
/// - `server "理由"`: 文書の錠の外で動く道具(中身は `Server` の上で待つ)。batch に入れない
macro_rules! tools {
    ($(
        $desc:literal
        fn $name:ident($s:ident $(, $a:ident: $arg:ty)?) $kind:ident $($why:literal)? => $body:expr;
    )*) => {
        #[tool_router(router = tool_router)]
        impl Server {
            $(
                #[tool(description = $desc)]
                async fn $name(
                    &self $(, Parameters($a): Parameters<$arg>)?
                ) -> Result<CallToolResult, ErrorData> {
                    tools!(@mcp $kind, self, $s, $body)
                }
            )*
        }

        /// batch に入れられない道具なら、その理由。
        pub(crate) fn batch_refusal(tool: &str) -> Option<&'static str> {
            match tool {
                $(stringify!($name) => tools!(@why $kind $($why)?),)*
                _ => None,
            }
        }

        /// batch の 1 手を読む。知らない道具は `None`、引数の誤りは理由を返す。
        /// batch に入れられない道具かどうかは先に [`batch_refusal`] で確かめる。
        pub(crate) fn batch_call(
            tool: &str,
            json: serde_json::Value,
        ) -> Option<Result<crate::batch::Call, String>> {
            Some(match tool {
                $(stringify!($name) => tools!(@batch $kind, tool, json, $s, $body $(, $a: $arg)?),)*
                _ => return None,
            })
        }
    };
    (@mcp picture, $me:tt, $s:ident, $body:expr) => {
        $me.run_picture(move |$s| $body).await
    };
    (@mcp server, $me:tt, $s:ident, $body:expr) => {{
        let $s = $me;
        $body.await
    }};
    (@mcp $kind:ident, $me:tt, $s:ident, $body:expr) => {
        $me.run(move |$s| $body).await
    };
    (@why outside $why:literal) => { Some($why) };
    (@why server $why:literal) => { Some($why) };
    (@why $kind:ident) => { None };
    (@batch outside, $($rest:tt)*) => { unreachable!("batch_refusal で先に断る") };
    (@batch server, $($rest:tt)*) => { unreachable!("batch_refusal で先に断る") };
    (@batch picture, $tool:ident, $json:ident, $s:ident, $body:expr $(, $a:ident: $arg:ty)?) => {
        tools!(@batch edit, $tool, $json, $s, $body.map(|(_, info)| info).map_err(Into::into) $(, $a: $arg)?)
    };
    (@batch $kind:ident, $tool:ident, $json:ident, $s:ident, $body:expr $(, $a:ident: $arg:ty)?) => {{
        $(let $a: $arg = match crate::batch::args($tool, $json) {
            Ok(a) => a,
            Err(e) => return Some(Err(e)),
        };)?
        Ok(crate::batch::Call::new(
            tools!(@values $kind),
            Box::new(move |$s: &mut Session| -> Reply { $body }),
        ))
    }};
    (@values value) => { true };
    (@values $kind:ident) => { false };
}

tools! {
    "文書の要約(パラメータ、タイムラインの各機能の状態と止まった理由、セットアップ)"
    fn status(s) edit => Ok(s.status());
    "人が見ている動いている画面(kieMonoBench)の今のタブにつなぐ。画面で設定の「AI との接続」を入れている時だけ使える。status で文書の要約と人が選んでいる面や辺、screenshot で画面の向きの絵、probe でその絵の画素に見える面・点(mm)・法線、set_param・add_param・set_value・undo・redo で文書を変える(人の取り消しの履歴に「Claude: 」付きで入る)。人が機能を編集している間は変える要求を断る。ほかの道具は画面の無い別の文書を動かす"
    fn app(s, a: AppArg) server "画面の道具は、画面の取り消しの履歴に入るので batch の外で使います" => s.app_tool(a);
    "文書ファイル(JSON)を開き、今の文書と置き換えて再計算し、status と同じ内容を返す。今の文書の保存されていない変更は確かめずに捨てるので、要るなら先に save する"
    fn open(s, a: PathArg) outside "文書を入れ替える道具は、戻せないので batch に入れられません" => s.open(&a.path);
    "基準部品の文書で始める(cam=true で加工も入れる)"
    fn sample(s, a: SampleArg) outside "文書を入れ替える道具は、戻せないので batch に入れられません" => s.sample(a.cam, a.three_d);
    "文書を JSON のファイルに保存し、保存先を saved で返す。path を省くと開いたファイル(または前に保存した場所)へ上書きし、どちらも無ければ失敗する。path に別の既存ファイルを指すと、overwrite が真の時だけ上書きする。直前の版は .bak に残る"
    fn save(s, a: SaveArg) outside "保存は batch の外で(途中の形を保存しないため)" => s.save(a.path.as_deref(), a.overwrite);
    "パラメータの式を変える(なければ足す)。comment と unit を渡すと表のコメントと単位も入れる。スケッチの寸法の名前(d1 など。status の dimensions)なら、その寸法の式を変える。形状と加工の経路は作り直される"
    fn set_param(s, a: ParamArg) value => s.set_param_info(&a.name, &a.expr, a.comment.as_deref(), a.unit.as_deref());
    "機能の値(押し出しの距離など。status の values)の式を変える。数だけなら今の単位を補う"
    fn set_value(s, a: ValueArg) value => s.set_value(a.feature, &a.key, &a.expr);
    "パラメータの表(同じパラメータを見る窓。写しは持たない)。表ごとに行(row は行の書き方、label、expr、value)。id 0 は全部の表"
    fn param_views(s, a: ViewsArg) edit => s.param_views(a.id);
    "パラメータの表を作る・名前を変える・消す、行を入れる・外す(取り消せる)。body を付けて作ると、そのボディに関わるパラメータ・寸法・機能の値が決まりで入る"
    fn edit_param_view(s, a: EditViewArg) edit => s.edit_param_view(&a);
    "パラメータ名か寸法の名前(d1 など)を変える。参照している式も書き換える"
    fn rename_param(s, a: RenameArg) edit => s.rename_param(&a.name, &a.new_name);
    "機能の種類の一覧。種類ごとに区分、表示名、データの項目(key、種類、説明)を返す。add_feature のデータはこれに従って作る"
    fn types(s) edit => Ok(s.types());
    "機能 1 つの id、name、type、suppressed と data(種類ごとの JSON)を返す。set_feature で置き換える前の下書きや、add_feature の data の例に使う"
    fn get_feature(s, a: IdArg) edit => s.feature_data(a.id);
    "タイムラインの最後に機能を 1 つ足して再計算し、status に足した機能の ID(added)を付けて返す。data の書き方は types、既存の例は get_feature で見る。省いた欄は画面から作った時と同じ既定で埋まる。対になる機能(ボディの分割の残りの側など)がある種類は、同じ取り消し単位で一緒に足す。未登録の種類は何も変えずに失敗する"
    fn add_feature(s, a: AddFeatureArg) edit => s.add_feature(&a.type_id, a.name.as_deref(), a.data);
    "STEP ファイルを読み、タイムラインの最後に足す(形だけのボディ。ブーリアンや加工の元にはなるが、フィレット等はかけられない)"
    fn import_step(s, a: ImportArg) edit => s.import_step(&a.path, a.stand_up);
    "タイムラインのロールバックの位置を動かして再計算し、status を返す(画面のタイムラインの印・CLI の rollback と同じ操作。取り消せる)。position は先頭から効かせる機能の数で、省くと最後まで進める。範囲の外は何も変えずに失敗する"
    fn roll_back(s, a: RollBackArg) edit => s.roll_back(a.position);
    "設計の属性(部品名・部品番号・改訂・作者・材料・文書の長さの単位 units など)を見る・変える(取り消せる)。部品名・部品番号・改訂・材料は G-code の頭の注に入る。units(mm・cm・m・in)は画面の表示・status の値・数だけ打った長さの単位で、道具の数の引数と measure・mass はいつも mm"
    fn properties(s, a: PropertiesArg) edit => s.properties(&a.set);
    "保存した文書の版の履歴(保存のたびに文書の横の .jc/history に残る。新しい順に number・id・saved_at(UNIX 秒)・note・milestone・features)。milestone_of と milestone で節目の名前を付ける"
    fn versions(s, a: VersionsArg) outside "版の履歴は保存したファイルの物なので、batch の外で使います" => s.versions(a.milestone_of.as_deref(), &a.milestone);
    "前の版を最新として戻して保存し、status を返す。戻したことも新しい版として残るので、versions からまた戻せる(undo の履歴は消える)"
    fn restore_version(s, a: RestoreVersionArg) outside "版の履歴は保存したファイルの物なので、batch の外で使います" => s.restore_version(&a.id);
    "機能のデータを data で丸ごと置き換えて再計算し、status を返す(一部の欄だけを足し合わせることはしない)。先に get_feature で今のデータを取り、直した全体を渡す。式 1 つだけ変えるなら set_value の方が安全。undo で戻せる"
    fn set_feature(s, a: SetFeatureArg) edit => s.set_feature(a.id, a.data);
    "機能をまとめてタイムラインの別の位置へ動かし、status を返す。使う機能より前へは動かせない(理由を返す)。undo で戻せる"
    fn move_features(s, a: MoveFeaturesArg) edit => s.move_features(&a.ids, a.target, a.after);
    "機能を抑制する、または戻す"
    fn suppress(s, a: SuppressArg) edit => s.suppress(a.id, a.suppressed);
    "機能をタイムラインから消して再計算し、status を返す。それを使っていた機能は理由つきで止まる(status の reason)。undo で戻せる。残したまま計算から外すだけなら suppress を使う"
    fn remove_feature(s, a: IdArg) edit => s.remove_feature(a.id);
    "タイムラインの機能をグループにまとめる(見せ方だけで、計算は変わらない)。status の groups に出る"
    fn group(s, a: GroupArg) edit => s.group(&a.ids, &a.name, &a.mode);
    "機能を写してタイムラインの今の位置に貼る(画面のコピー・貼り付けと同じ)。値と式は写し、面・辺の選ぶ欄は空にするので set_feature で選び直す。まとめて写すと、写した機能どうしのつながりは貼った写しの間でつなぎ直す。pasted が貼った機能の ID、cleared が空にした選ぶ欄の数。undo で戻せる"
    fn paste_features(s, a: PasteFeaturesArg) edit => s.paste_features(&a.ids);
    "部品を中の部品ごと、別の部品の中(into、省くと文書の直下)か、ある部品の前(before)・後ろ(after)へ移し、status を返す(画面の部品の木のドラッグと同じ)。自分や自分の中へは移せない(理由を返す)。undo で戻せる"
    fn move_component(s, a: MoveComponentArg) edit => s.move_component(&a);
    "部品をアクティブにし(component、省くと文書の直下へ戻す)、status を返す(画面のブラウザの部品の行の丸と同じ)。この後に作るボディと文書の直下に作る部品は、その部品に入る(作った変更と同じ取り消し単位)。今のアクティブな部品は status の active_component。文書には保存しない"
    fn activate_component(s, a: ActivateComponentArg) edit => s.activate_component(a.component);
    "ボディを部品(component、省くと部品から外す)へ移し、status を返す(画面のボディの行のドラッグと同じ)。undo で戻せる"
    fn move_bodies(s, a: MoveBodiesArg) edit => s.move_bodies(&a.ids, a.component);
    "部品を中の部品ごと複製し、status を返す(画面のブラウザの部品の右クリック「複製」と同じ)。中のボディは元の形を追う写しになり、元のすぐ後ろに並び、外形の幅と隙間だけ X へずらして置く。component が作った部品の番号。undo で戻せる"
    fn duplicate_component(s, a: IdArg) edit => s.duplicate_component(a.id);
    "名前を付けた視点の一覧(番号・名前・真ん中の点・向き・傾き・見える高さ・投影)。画面のブラウザの「名前の付いたビュー」と同じ"
    fn named_views(s) edit => Ok(s.named_views());
    "視点に名前を付けて文書に保存し、その視点を返す。screenshot の named_view で描ける。undo で戻せる"
    fn save_named_view(s, a: SaveViewArg) edit => s.save_named_view(&a);
    "名前を付けた視点の名前を変える"
    fn rename_named_view(s, a: RenameViewArg) edit => s.rename_named_view(a.id, &a.name);
    "名前を付けた視点を消す。id は named_views の id"
    fn remove_named_view(s, a: IdArg) edit => s.remove_named_view(a.id);
    "グループを解く(中身の機能は残す)。id は status の groups[].id"
    fn ungroup(s, a: IdArg) edit => s.ungroup(a.id);
    "最後の変更を取り消す"
    fn undo(s) outside "取り消し・やり直しは batch に入れられません(失敗した時は batch が戻します)" => s.undo();
    "取り消した変更をやり直す"
    fn redo(s) outside "取り消し・やり直しは batch に入れられません(失敗した時は batch が戻します)" => s.redo();
    "取り消しの履歴。done は済んだ変更(古い順、最後が次に取り消される)、undone はやり直せる変更(次にやり直すものが先頭)"
    fn history(s) edit => Ok(s.history());
    "PC 全体の設定(機械など)。fields が変えられる項目、value が今の値、default が初めの値、notes が変えられない事柄"
    fn settings(s) edit => Ok(s.settings());
    "設定を変える(この接続の間だけ。PC に残すのは画面の「設定」)。正しくない値なら何も変えずに理由を返す"
    fn set_settings(s, a: SettingsArg) outside "設定の変更は戻せないので batch に入れられません" => s.set_settings(&a.id, &a.value);
    "今使えるモジュールの操作(画面のボタンやメニューと同じもの)。disabled は使えない理由、args は受け取る引数の宣言(key・kind・unit・default・required)"
    fn actions(s, a: ActionsArg) edit => Ok(s.actions(a.selected));
    "モジュールの操作を実行する(actions の id)。文書を変える操作は status と added を返す"
    fn run_action(s, a: RunActionArg) edit => s.run_action_at(&a.id, a.selected, a.file.as_deref(), &a.args, a.out.as_deref());
    "残ったボディの名前、体積(mm³)、表面積(mm²)、外形(mm)、重心(mm)。材料を付けたボディは材料・密度(g/cm³)・質量(g)も"
    fn mass(s) edit => s.mass();
    "ボディに物理材料を付ける(取り消せる)。質量(mass)と外観の既定の色に使い、加工のセットアップの材料の既定にもなる"
    fn set_material(s, a: MaterialArg) edit => s.set_material(a.body, &a.material);
    "残ったボディの面(名前)と辺(番号)と、それぞれの形(平面・円柱・直線・円など、数値つき)。measure で測る物を指すのに使う"
    fn topology(s, a: TopologyArg) edit => s.topology(a.body);
    "面・辺・円の中心・点を 1 つか 2 つ測る(画面の測る道具と同じ値)。1 つなら長さ・直径・面積・法線など(面ならボディの体積も)、2 つなら距離・ΔX/ΔY/ΔZ・角度。rows が値の行、shapes が測った物の形"
    fn measure(s, a: MeasureArg) edit => s.measure(&a.targets, a.basis.as_deref());
    "残ったボディの絵(PNG)。画面の 3D のビューの明るい見た目で、平行投影で全体が入るように描く(格子・寸法・スケッチは描かない)。view で向き、faces・edges で topology の面と辺を目立たせる。説明に 1 mm の画素数(px_per_mm)。out で PNG を書き出す"
    fn screenshot(s, a: ScreenshotArg) picture => s.screenshot(&a);
    "見えているボディを書き出す。拡張子で形式を決める(STL・STEP・3MF・glTF の .glb・今の視点の DXF と SVG・レンダリングの .png と動画)。each が真ならボディごとに別のファイル(部品.stl → 部品-ボディ1.stl)。ほかの書き出し(隠れ線つきの DXF など)は action に操作の ID"
    fn export(s, a: ExportArg) edit => s.export(&a.path, a.action.as_deref(), a.view.as_ref(), a.tolerance, a.each);
    "セットアップの G-code を作る(既定は Carvera Air、post で WASM のポストのプラグインも使える)"
    fn gcode(s, a: GcodeArg) edit => s.gcode(a.setup, a.out.as_deref(), a.post.as_deref());
    "セットアップの経路でストックを削ってみて、食い込み、早送りでの切削、削り残しを調べる"
    fn simulate(s, a: SimArg) edit => s.simulate(a.setup, a.cell);
    "いくつもの道具を順に動かす。途中で失敗したら batch の前の形に全部戻し、何手目(step)の、どの道具(tool)が、なぜ(error)失敗したかを返す。通ったら文書を変えた手を 1 回の取り消し単位にまとめる(undo 1 回で戻る)。手の引数は道具を 1 つずつ呼ぶ時と同じ。open・sample・save・undo・redo・set_settings・batch は入れられない。書き出したファイルは戻さない"
    fn batch(s, a: BatchArg) outside "batch の中に batch は入れられません" => s.batch(a.steps);
    "このPCに残ったクラッシュ報告(送信はしていない)。name を省くと一覧(種類、時刻、版、理由)、指すとその報告(場所、逆追跡、最近の操作、再現用の文書)"
    fn crash_reports(s, a: CrashArg) edit => s.crash_reports(a.name.as_deref());
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "kie-monobench",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "kieMonoBench の文書を操作する。まず status で状態を見る。status の値は単位つきで、長さは\
                 文書の単位(status の units。既定は mm)。式に数だけを書くと、その単位を補う。\
                 道具の数の引数(径・距離・座標・measure・mass など)はいつも mm。\
                 変更するとすぐ再計算され、止まった機能は status の reason に理由が出る。",
            )
    }
}
