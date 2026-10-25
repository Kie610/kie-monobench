//! 文書ファイル(JSON)を操作する CLI。1 回の実行が 1 つのトランザクションになる。

//! 文書の扱いとモジュールの操作は、MCP と同じ [`jc_session::Headless`] に任せる。

use clap::{Parser, Subcommand};
use jc_bundle::Sample;
use jc_core::{Document, FeatureId, Problem, ProblemKind};
use jc_session::{Headless, Ran, edit};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "kmb",
    version,
    about = "kieMonoBench の文書を操作する",
    after_help = "終了の番号: 0 成功、1 入力の誤り、2 再計算の誤り、3 確かめで問題を見つけた(食い込みなど)、4 今は使えない操作、5 ファイルを読めない・書けない"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 空の文書を作る
    New { file: PathBuf },
    /// パラメータを操作する
    Param {
        #[command(subcommand)]
        command: ParamCommand,
    },
    /// 機能を操作する
    Feature {
        #[command(subcommand)]
        command: FeatureCommand,
    },
    /// 部品の木を操作する(並べ替え・ボディを移す・複製)
    Component {
        #[command(subcommand)]
        command: ComponentCommand,
    },
    /// 名前を付けた視点を操作する
    View {
        #[command(subcommand)]
        command: ViewCommand,
    },
    /// ロールバック位置を設定する(先頭から効かせる機能の数。`end` で解除)
    Rollback { file: PathBuf, position: String },
    /// タイムラインの機能をグループにまとめる(見せ方だけで、計算は変わらない)。番号を出す
    Group {
        file: PathBuf,
        /// グループの名前
        #[arg(long)]
        name: String,
        /// まとめる機能の ID
        #[arg(required = true)]
        ids: Vec<u64>,
        /// 続いていなければ、間の機能も含める
        #[arg(long)]
        gaps: bool,
        /// 続いていなければ、選んだ機能を寄せてからまとめる
        #[arg(long, conflicts_with = "gaps")]
        gather: bool,
    },
    /// グループを解く(中身の機能は残す)
    Ungroup { file: PathBuf, id: u64 },
    /// 再計算して結果を表示する
    Recompute {
        file: PathBuf,
        /// 結果を JSON で出力する
        #[arg(long)]
        json: bool,
    },
    /// STEP ファイルを読み、タイムラインの最後に足す(形だけのボディになる)
    Import {
        file: PathBuf,
        step: PathBuf,
        /// Y が上で作られた形を、X 軸のまわりに 90 度回して Z が上に立てる
        #[arg(long)]
        stand_up: bool,
    },
    /// 設計の属性(部品名・部品番号・文書の長さの単位 units など)を表示する。`鍵=値` を並べると変える(空の値は消す)。
    /// 鍵は title・part_number・revision・designer・organization・description・material・
    /// copyright・license・project(作品)・genre(ジャンル)・tags(タグ。「,」で区切る)・
    /// units(文書の長さの単位。mm・cm・m・in。画面の表示と数だけ打った長さの単位で、形は mm のまま)。
    /// それ以外は自分で足す項目。部品名などは G-code の頭の注に入る
    Props { file: PathBuf, values: Vec<String> },
    /// ボディに物理材料を付けて保存する(空の名前で外す)。`--body` は機能の ID で、省くと全ボディ
    Material {
        file: PathBuf,
        name: String,
        #[arg(long)]
        body: Option<u64>,
    },
    /// 最後に残ったボディの体積・表面積・外形・重心と、材料を付けていれば質量を表示する
    Mass {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// 見えているボディを書き出す。拡張子で形式を決める(.stl・.step / .stp・.3mf・.glb・
    /// 今の視点の .dxf と .svg・レンダリングの .png と動画)
    Export {
        file: PathBuf,
        out: PathBuf,
        /// 使う書き出しの操作の ID(隠れ線つきの DXF など、拡張子だけでは選ばれない物)
        #[arg(long)]
        action: Option<String>,
        /// 視点(.dxf・.svg の線と、レンダリング)。top・bottom・front・back・right・left・iso か、
        /// `{"eye": [x, y, z], "up": [x, y, z]}`
        #[arg(long)]
        view: Option<String>,
        /// 三角形・線の許容差(mm)。省くと操作の既定(画面から書き出した時と同じ)
        #[arg(long)]
        tolerance: Option<f64>,
        /// STL を ASCII 形式で書く
        #[arg(long)]
        ascii: bool,
        /// ボディごとに別のファイルにする(out の名前の後ろに -<ボディの名前> を付ける)
        #[arg(long)]
        each: bool,
    },
    /// 基準部品の文書を作る
    Sample {
        file: PathBuf,
        /// 加工(工具、セットアップ、加工の機能)も入れる
        #[arg(long)]
        cam: bool,
        /// 面取りとフィレットを足し、走査線と等高線の 3D 仕上げも入れる(`--cam` を含む)
        #[arg(long = "3d")]
        three_d: bool,
    },
    /// セットアップの G-code を Carvera Air 向けに書き出す(出力先がなければ標準出力)
    Gcode {
        file: PathBuf,
        out: Option<PathBuf>,
        /// セットアップの機能の ID(省くと最初のセットアップ)
        #[arg(long)]
        setup: Option<u64>,
        /// ポストのプラグイン(WASM のコンポーネント)。省くと Carvera Air
        #[arg(long)]
        post: Option<PathBuf>,
    },
    /// セットアップの段取り表(Markdown)を書き出す(出力先がなければ標準出力)
    Sheet {
        file: PathBuf,
        out: Option<PathBuf>,
        #[arg(long)]
        setup: Option<u64>,
    },
    /// セットアップの経路でストックを削ってみて、食い込みと削り残しを表示する
    Sim {
        file: PathBuf,
        #[arg(long)]
        setup: Option<u64>,
        /// 格子の大きさ(mm)。省くと画面と同じ(設定の格子。荒削りは荒い格子)
        #[arg(long)]
        cell: Option<f64>,
    },
    /// 文書で使える操作の一覧(ID・名前・使えない理由と、受け取る引数の型・単位・既定)を表示する
    Actions {
        file: PathBuf,
        /// 選んでいるとする機能の ID(選んだ機能についての操作も出す)
        #[arg(long)]
        selected: Option<u64>,
        /// JSON で出力する
        #[arg(long)]
        json: bool,
    },
    /// 操作を ID で実行する(`kmb actions` の一覧の ID)。文書を変える操作は保存し、書き出しは `--out` へ書く
    Run {
        file: PathBuf,
        id: String,
        /// 引数 `鍵=値`(いくつでも)。値は JSON として読めなければ文字。長さは `5 mm`・`0.2 in` とも書ける。
        /// 操作が宣言しない鍵や型の違いは誤り
        #[arg(long = "arg", value_name = "鍵=値")]
        args: Vec<String>,
        /// 選んでいるとする機能の ID
        #[arg(long)]
        selected: Option<u64>,
        /// 操作が読むファイル(読み込みの操作)
        #[arg(long)]
        input: Option<PathBuf>,
        /// 書き出す先(書き出しの操作。ボディごとの書き出しは名前の後ろに -<ボディの名前> を付ける)
        #[arg(long)]
        out: Option<PathBuf>,
        /// 作るボディと部品を入れる部品の番号(画面で部品をアクティブにしてから作るのと同じ)
        #[arg(long)]
        component: Option<u64>,
    },
    /// 登録されている機能の種類を表示する
    Types,
    /// PC 全体の設定(機械など。データフォルダの settings.json を当てた値)を JSON で表示する
    Settings,
}

#[derive(Subcommand)]
enum ParamCommand {
    Add {
        file: PathBuf,
        name: String,
        expr: String,
        /// 表のコメントの列(何のための値か)
        #[arg(long)]
        comment: Option<String>,
        /// 単位: length・angle・unitless・any
        #[arg(long)]
        unit: Option<String>,
    },
    Set {
        file: PathBuf,
        name: String,
        expr: String,
        /// 表のコメントの列(何のための値か)
        #[arg(long)]
        comment: Option<String>,
        /// 単位: length・angle・unitless・any
        #[arg(long)]
        unit: Option<String>,
    },
    Rename {
        file: PathBuf,
        name: String,
        new_name: String,
    },
    /// パラメータを消す(式で使われていれば断る。`param restore` で戻せる)
    Rm { file: PathBuf, name: String },
    /// 消したパラメータを戻す
    Restore { file: PathBuf, name: String },
    /// パラメータの表を CSV か JSON に書き出す(`out` の拡張子で決める。`--id` を省くと全部の表)
    Export {
        file: PathBuf,
        out: PathBuf,
        #[arg(long, default_value_t = 0)]
        id: u64,
    },
    /// 書き出した表を読み込む(同じ名前のパラメータは上書き、無ければ足す。寸法・機能の値は
    /// この文書にある物だけ直す)
    Import { file: PathBuf, table: PathBuf },
    /// 機能の値(押し出しの距離など)の式を変える。例: `kmb param value part.json 5 distance 12`
    Value {
        file: PathBuf,
        /// 機能の ID
        feature: u64,
        /// 値の欄(`kmb param views part.json --id 0` の行の `機能の ID:欄` の欄)
        key: String,
        /// 式(数だけなら今の単位を補う)
        expr: String,
    },
    /// パラメータの表を JSON で表示する(`--id 0` で全部の表)
    Views {
        file: PathBuf,
        #[arg(long)]
        id: Option<u64>,
    },
    /// パラメータの表を作る。番号を出す。行はパラメータか寸法の名前、機能の値は `機能の ID:欄`
    ViewNew {
        file: PathBuf,
        /// 表の名前(省くとボディの名前から付ける)
        #[arg(long)]
        name: Option<String>,
        /// このボディ(機能の ID)に関わる行を決まりで入れる
        #[arg(long)]
        body: Option<u64>,
        rows: Vec<String>,
    },
    /// パラメータの表に行を入れる
    ViewAdd {
        file: PathBuf,
        id: u64,
        #[arg(required = true)]
        rows: Vec<String>,
    },
    /// パラメータの表から行を外す(パラメータは残る)
    ViewHide {
        file: PathBuf,
        id: u64,
        #[arg(required = true)]
        rows: Vec<String>,
    },
    /// パラメータの表の名前を変える
    ViewRename {
        file: PathBuf,
        id: u64,
        name: String,
    },
    /// パラメータの表を消す(パラメータは残る)
    ViewRm { file: PathBuf, id: u64 },
}

#[derive(Subcommand)]
enum FeatureCommand {
    /// 種類(`type_id`)の一覧は `kmb types`、データの形は種類ごとの記述子。
    Add {
        file: PathBuf,
        type_id: String,
        data: String,
        #[arg(long)]
        name: Option<String>,
        /// 作るボディと部品を入れる部品の番号(画面で部品をアクティブにしてから作るのと同じ)
        #[arg(long)]
        component: Option<u64>,
    },
    Set {
        file: PathBuf,
        id: u64,
        data: String,
    },
    Suppress {
        file: PathBuf,
        id: u64,
        /// 抑制を解除する
        #[arg(long)]
        off: bool,
    },
    Rm {
        file: PathBuf,
        id: u64,
    },
    /// 機能を写してタイムラインの今の位置に貼る。面・辺の選ぶ欄は空にする(`feature set` で選び直す)。
    /// 貼った機能の ID を出す。例: `kmb feature paste part.json 3 4`
    Paste {
        file: PathBuf,
        #[arg(required = true)]
        ids: Vec<u64>,
    },
    /// 機能をまとめて、機能 `target` の前(`--after` なら後ろ)へ動かす。例: `kmb feature move part.json 7 3 4`
    Move {
        file: PathBuf,
        target: u64,
        #[arg(required = true)]
        ids: Vec<u64>,
        #[arg(long)]
        after: bool,
    },
}

#[derive(Subcommand)]
enum ComponentCommand {
    /// 部品を中の部品ごと移す。先を付けなければ文書の直下へ。例: `kmb component move part.json 3 --into 2`
    Move {
        file: PathBuf,
        id: u64,
        /// この部品の中へ入れる
        #[arg(long, conflicts_with_all = ["before", "after"])]
        into: Option<u64>,
        /// この部品のすぐ前(同じ親の並び)へ
        #[arg(long, conflicts_with = "after")]
        before: Option<u64>,
        /// この部品のすぐ後ろ(同じ親の並び)へ
        #[arg(long)]
        after: Option<u64>,
    },
    /// ボディ(そのボディを作った・変えたどれかの機能の ID)を部品へ移す。`--to` を省くと部品から外す。
    /// 例: `kmb component bodies part.json 5 7 --to 2`
    Bodies {
        file: PathBuf,
        #[arg(required = true)]
        ids: Vec<u64>,
        #[arg(long)]
        to: Option<u64>,
    },
    /// 部品を中の部品ごと複製する(中のボディは元の形を追う写し、横へずらして置く)。作った部品の番号を出す
    Duplicate { file: PathBuf, id: u64 },
}

#[derive(Subcommand)]
enum ViewCommand {
    /// 名前を付けた視点の一覧を JSON で表示する
    List { file: PathBuf },
    /// 視点に名前を付けて保存する。番号を出す。例: `kmb view save part.json 斜め --target 0,0,0 --yaw -60 --pitch 30 --height 100`
    Save {
        file: PathBuf,
        /// 視点の名前(空なら「視点N」)
        #[arg(default_value = "")]
        name: String,
        /// 画面の真ん中に来る点(mm。x,y,z)
        #[arg(
            long,
            value_delimiter = ',',
            num_args = 3,
            allow_negative_numbers = true
        )]
        target: Vec<f64>,
        /// 水平の回り(度。-90 で前から)
        #[arg(long, allow_negative_numbers = true)]
        yaw: f64,
        /// 見下ろす角度(度、-90〜90)
        #[arg(long, allow_negative_numbers = true)]
        pitch: f64,
        /// 画面の上での傾き(度)
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        roll: f64,
        /// 画面の縦に見える高さ(mm)
        #[arg(long)]
        height: f64,
        /// 透視投影で見る
        #[arg(long)]
        perspective: bool,
    },
    /// 視点の名前を変える
    Rename {
        file: PathBuf,
        id: u64,
        name: String,
    },
    /// 視点を消す
    Rm { file: PathBuf, id: u64 },
}

/// 表のファイルの形(拡張子が csv か json)。
fn table_format(p: &Path) -> Result<jc_session::param_io::Format, String> {
    p.extension()
        .and_then(|e| e.to_str())
        .and_then(jc_session::param_io::Format::from_extension)
        .ok_or_else(|| format!("{} の拡張子は .csv か .json にしてください", p.display()))
}

/// 文書を開く(再計算する)。
fn open(path: &Path) -> Result<Headless, Problem> {
    let modules = jc_bundle::modules();
    jc_session::apply_user_settings(&modules);
    Headless::open(modules, path)
}

/// 文書を読み、組み立てた変更([`jc_session::edit`])を適用して保存する。
fn apply(
    path: &Path,
    f: impl FnOnce(&jc_core::DocumentData) -> jc_core::Transaction,
) -> Result<(), Problem> {
    change(path, |w| w.apply(f(w.doc().data())))
}

/// パラメータのコメントと単位の引数を読む。
fn param_info(
    comment: Option<String>,
    unit: Option<String>,
) -> Result<jc_session::edit::ParamInfo, Problem> {
    jc_session::edit::ParamInfo::parse(comment.as_deref(), unit.as_deref())
        .map_err(|e| Problem::new(ProblemKind::Input, e))
}

/// 文書を開き、`f` で変えて保存する(1 回の実行が 1 つのトランザクション)。`f` の結果を返す。
fn change<T>(
    path: &Path,
    f: impl FnOnce(&mut Headless) -> Result<T, String>,
) -> Result<T, Problem> {
    let mut w = open(path)?;
    let r = f(&mut w)?;
    w.save(None)?;
    Ok(r)
}

/// `--component` の部品をアクティブにする(この後に作るボディと部品がその部品に入る)。
fn activate(w: &mut Headless, component: Option<u64>) -> Result<(), String> {
    match component {
        Some(c) => w.activate_component(Some(c)).map(|_| ()),
        None => Ok(()),
    }
}

fn json_arg(s: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(s).map_err(|e| format!("データが JSON として読めません: {e}"))
}

fn write(out: &Path, contents: &[u8]) -> Result<(), Problem> {
    jc_session::write_output(out, contents)
}

/// セットアップの操作(G-code、段取り表、シミュレーション)を実行する。
/// 経路を作れなかった加工があれば、再計算の結果も見せる。
fn run_setup(
    w: &mut Headless,
    role: jc_core::ActionRole,
    setup: Option<u64>,
    args: serde_json::Value,
) -> Result<Ran, Problem> {
    let action = w.action_for_role(&role)?;
    let setup = setup.map(FeatureId).or_else(|| w.first_setup());
    let r = w.run(&action, setup, None, &args);
    let failed = w.status()["setups"].as_array().is_some_and(|a| {
        a.iter()
            .any(|s| s["failed"].as_array().is_some_and(|f| !f.is_empty()))
    });
    if r.is_err() || failed {
        print!("{}", w.report_text());
    }
    r
}

fn run(cli: Cli) -> Result<ExitCode, Problem> {
    match cli.command {
        Command::New { file } => {
            if file.exists() {
                return Err(Problem::new(
                    ProblemKind::File,
                    format!("{} は既にあります", file.display()),
                ));
            }
            jc_session::save(&file, &Document::new())?;
        }
        Command::Param { command } => match command {
            ParamCommand::Add {
                file,
                name,
                expr,
                comment,
                unit,
            } => {
                let info = param_info(comment, unit)?;
                change(&file, |w| w.add_param_info(&name, &expr, &info))?;
            }
            ParamCommand::Set {
                file,
                name,
                expr,
                comment,
                unit,
            } => {
                let info = param_info(comment, unit)?;
                change(&file, |w| w.set_param_info(&name, Some(&expr), &info))?;
            }
            ParamCommand::Rename {
                file,
                name,
                new_name,
            } => {
                // 参照している式も書き換える(MCP と同じ)
                change(&file, |w| w.rename_param(&name, &new_name))?;
            }
            ParamCommand::Rm { file, name } => {
                change(&file, |w| w.remove_param(&name))?;
            }
            ParamCommand::Restore { file, name } => {
                change(&file, |w| w.restore_param(&name))?;
            }
            ParamCommand::Export { file, out, id } => {
                let w = open(&file)?;
                let text = w.export_params(id, table_format(&out)?)?;
                write(&out, text.as_bytes())?;
            }
            ParamCommand::Import { file, table } => {
                let text = std::fs::read_to_string(&table)
                    .map_err(|e| format!("{} を読めません: {e}", table.display()))?;
                let done = change(&file, |w| w.import_params(&text, table_format(&table)?))?;
                println!("{}", done.summary());
            }
            ParamCommand::Value {
                file,
                feature,
                key,
                expr,
            } => {
                change(&file, |w| w.set_value(FeatureId(feature), &key, &expr))?;
            }
            ParamCommand::Views { file, id } => {
                let w = open(&file)?;
                let v = w.param_views(id)?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&v).expect("JSON にできる")
                );
            }
            ParamCommand::ViewNew {
                file,
                name,
                body,
                rows,
            } => {
                let id = change(&file, |w| {
                    w.add_param_view(name.as_deref(), body.map(FeatureId), &rows)
                })?;
                println!("{id}");
            }
            ParamCommand::ViewAdd { file, id, rows } => {
                change(&file, |w| w.param_view_rows(id, &rows, true))?;
            }
            ParamCommand::ViewHide { file, id, rows } => {
                change(&file, |w| w.param_view_rows(id, &rows, false))?;
            }
            ParamCommand::ViewRename { file, id, name } => {
                change(&file, |w| w.rename_param_view(id, &name))?;
            }
            ParamCommand::ViewRm { file, id } => {
                change(&file, |w| w.remove_param_view(id))?;
            }
        },
        Command::Feature { command } => match command {
            FeatureCommand::Add {
                file,
                type_id,
                data,
                name,
                component,
            } => {
                let data = json_arg(&data)?;
                let mut w = open(&file)?;
                activate(&mut w, component)?;
                let id = w
                    .add_feature(&type_id, name.as_deref(), data)
                    .map_err(|e| e.replace("(types で一覧)", "(`kmb types` で一覧)"))?;
                w.save(None)?;
                println!("{id}");
            }
            FeatureCommand::Set { file, id, data } => {
                let data = json_arg(&data)?;
                apply(&file, |d| edit::set_feature_data(d, FeatureId(id), data))?
            }
            FeatureCommand::Suppress { file, id, off } => {
                apply(&file, |_| edit::set_suppressed(&[FeatureId(id)], !off))?
            }
            FeatureCommand::Rm { file, id } => {
                apply(&file, |d| edit::remove_features(d, &[FeatureId(id)]))?
            }
            FeatureCommand::Paste { file, ids } => {
                let ids: Vec<FeatureId> = ids.into_iter().map(FeatureId).collect();
                let (made, cleared) = change(&file, |w| w.paste_features(&ids))?;
                for id in made {
                    println!("{}", id.0);
                }
                if cleared > 0 {
                    eprintln!(
                        "選ぶ欄 {cleared} 個を空にしました。feature set で面や辺を選び直してください"
                    );
                }
            }
            FeatureCommand::Move {
                file,
                target,
                ids,
                after,
            } => {
                let ids: Vec<FeatureId> = ids.into_iter().map(FeatureId).collect();
                change(&file, |w| w.move_features(&ids, FeatureId(target), after))?;
            }
        },
        Command::Group {
            file,
            name,
            ids,
            gaps,
            gather,
        } => {
            let mode = match (gaps, gather) {
                (true, _) => edit::GroupMode::IncludeGaps,
                (_, true) => edit::GroupMode::Gather,
                _ => edit::GroupMode::Exact,
            };
            let ids: Vec<FeatureId> = ids.into_iter().map(FeatureId).collect();
            let g = change(&file, |w| w.group(&ids, &name, mode))?;
            println!("{g}");
        }
        Command::Component { command } => match command {
            ComponentCommand::Move {
                file,
                id,
                into,
                before,
                after,
            } => {
                use jc_session::components::Place;
                let at = match (before, after) {
                    (Some(c), _) => Place::Before(c),
                    (_, Some(c)) => Place::After(c),
                    _ => Place::Into(into),
                };
                change(&file, |w| w.move_component(id, at))?;
            }
            ComponentCommand::Bodies { file, ids, to } => {
                let ids: Vec<FeatureId> = ids.into_iter().map(FeatureId).collect();
                change(&file, |w| w.move_bodies(&ids, to))?;
            }
            ComponentCommand::Duplicate { file, id } => {
                let made = change(&file, |w| w.duplicate_component(id))?;
                println!("{made}");
            }
        },
        Command::View { command } => match command {
            ViewCommand::List { file } => {
                let w = open(&file)?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&w.doc().data().named_views)
                        .expect("JSON にできる")
                );
            }
            ViewCommand::Save {
                file,
                name,
                target,
                yaw,
                pitch,
                roll,
                height,
                perspective,
            } => {
                let target: [f64; 3] = target.try_into().map_err(|_| {
                    Problem::new(ProblemKind::Input, "--target は x,y,z の 3 つです")
                })?;
                let view = jc_core::NamedView {
                    id: 0,
                    name,
                    target,
                    yaw,
                    pitch,
                    roll,
                    height,
                    perspective,
                };
                let id = change(&file, |w| w.save_named_view(view))?;
                println!("{id}");
            }
            ViewCommand::Rename { file, id, name } => {
                change(&file, |w| w.rename_named_view(id, &name))?;
            }
            ViewCommand::Rm { file, id } => {
                change(&file, |w| w.remove_named_view(id))?;
            }
        },
        Command::Ungroup { file, id } => {
            change(&file, |w| w.ungroup(id))?;
        }
        Command::Rollback { file, position } => {
            // 画面・MCP と同じ操作を通す(範囲の外は操作が範囲を書いて断る)
            let position = match position.as_str() {
                "end" => serde_json::Value::Null,
                n => json!(
                    n.parse::<i64>()
                        .map_err(|_| format!("位置は整数か end です: {n}"))?
                ),
            };
            let mut w = open(&file)?;
            let id = w.action_for_role(&jc_interchange::role::action::ROLL_BACK)?;
            w.run(&id, None, None, &json!({ "position": position }))?;
            w.save(None)?;
        }
        Command::Recompute { file, json } => {
            let w = open(&file)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(w.report()).expect("結果は JSON にできる")
                );
            } else {
                print!("{}", w.report_text());
            }
            if w.report().error_count() > 0 {
                return Ok(ExitCode::from(ProblemKind::Compute.exit_code()));
            }
        }
        Command::Import {
            file,
            step,
            stand_up,
        } => {
            let mut w = open(&file)?;
            let import = w.action_for_role(&jc_interchange::role::action::IMPORT_STEP)?;
            let Ran::Changed(added) =
                w.run(&import, None, Some(&step), &serde_json::Value::Null)?
            else {
                return Err("読み込めませんでした".into());
            };
            if stand_up && let Some(id) = added.last() {
                w.stand_up(*id, true)?;
            }
            w.save(None)?;
            if let Some(id) = added.last() {
                println!("{id}");
            }
        }
        Command::Props { file, values } => {
            let mut w = open(&file)?;
            if !values.is_empty() {
                let pairs = values
                    .iter()
                    .map(|v| {
                        v.split_once('=')
                            .map(|(k, x)| (k.trim().to_string(), x.to_string()))
                            .ok_or_else(|| format!("`鍵=値` の形で書いてください: {v}"))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                w.set_properties(&pairs)?;
                w.save(None)?;
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&w.properties()).expect("JSON にできる")
            );
        }
        Command::Material { file, name, body } => {
            let mut w = open(&file)?;
            w.set_body_material(body.map(jc_core::FeatureId), &name)?;
            w.save(None)?;
            for b in w.mass()? {
                match (b.material, b.mass) {
                    (Some(m), Some(g)) => println!("{}: {m}、{g:.3} g", b.name),
                    _ => println!("{}: 材料なし", b.name),
                }
            }
        }
        Command::Mass { file, json } => {
            let w = open(&file)?;
            let all = w.mass().inspect_err(|_| print!("{}", w.report_text()))?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&all).expect("結果は JSON にできる")
                );
            } else {
                let r3 = |p: [f64; 3]| p.map(|v| (v * 1e3).round() / 1e3 + 0.0);
                for b in &all {
                    let m = &b.props;
                    println!("{}:", b.name);
                    println!("  体積   {:.3} mm³", m.volume);
                    println!("  表面積 {:.3} mm²", m.area);
                    println!(
                        "  外形   {:.3} × {:.3} × {:.3} mm(最小 {:?}、最大 {:?})",
                        m.max[0] - m.min[0],
                        m.max[1] - m.min[1],
                        m.max[2] - m.min[2],
                        r3(m.min),
                        r3(m.max)
                    );
                    if let Some(c) = m.centroid {
                        println!("  重心   {:?} mm", r3(c));
                    }
                    if let (Some(name), Some(d), Some(g)) = (&b.material, b.density, b.mass) {
                        println!("  材料   {name}({d} g/cm³)");
                        println!("  質量   {g:.3} g");
                    }
                }
            }
        }
        Command::Export {
            file,
            out,
            action,
            view,
            tolerance,
            ascii,
            each,
        } => {
            let mut w = open(&file)?;
            let action = w.save_action_for(&out, action.as_deref())?;
            // 利用者が書いた指定だけを渡す(受け取らない操作に渡すと誤りにする)
            let mut args = w.export_args(&action, &out);
            if ascii {
                args["ascii"] = json!(true);
            }
            if each {
                args["each"] = json!(true);
            }
            if let Some(t) = tolerance {
                args["tolerance"] = json!(t);
            }
            if let Some(v) = view {
                args["view"] = if v.trim_start().starts_with('{') {
                    json_arg(&v)?
                } else {
                    json!(v)
                };
            }
            match w.run(&action, None, None, &args) {
                Ok(Ran::File { contents, .. }) => write(&out, &contents)?,
                Ok(Ran::Files { files, .. }) => {
                    for p in Headless::write_files(&out, &files)? {
                        println!("{}", p.display());
                    }
                }
                Ok(_) => return Err("書き出せませんでした".into()),
                Err(e) => {
                    if w.report().error_count() > 0 {
                        print!("{}", w.report_text());
                    }
                    return Err(e);
                }
            }
        }
        Command::Sample { file, cam, three_d } => {
            if file.exists() {
                return Err(Problem::new(
                    ProblemKind::File,
                    format!("{} は既にあります", file.display()),
                ));
            }
            let kind = if three_d {
                Sample::Cam3d
            } else if cam {
                Sample::Cam
            } else {
                Sample::Part
            };
            jc_session::save(&file, &kind.make()())?;
        }
        Command::Gcode {
            file,
            out,
            setup,
            post,
        } => {
            let mut w = open(&file)?;
            let mut args = json!({});
            if let Some(p) = &post {
                args["post"] = json!(p.display().to_string());
            }
            let Ran::File { contents, info, .. } = run_setup(
                &mut w,
                jc_interchange::role::action::EXPORT_GCODE,
                setup,
                args,
            )?
            else {
                return Err("G-code を作れませんでした".into());
            };
            match out {
                Some(o) => {
                    write(&o, &contents)?;
                    println!(
                        "{} に {} 行を書きました。工具 {} 本、加工時間の目安 {:.1} 分",
                        o.display(),
                        info["lines"],
                        info["tools"].as_array().map_or(0, Vec::len),
                        info["minutes"].as_f64().unwrap_or_default()
                    );
                }
                None => print!("{}", String::from_utf8_lossy(&contents)),
            }
        }
        Command::Sheet { file, out, setup } => {
            let mut w = open(&file)?;
            let Ran::File { contents, .. } = run_setup(
                &mut w,
                jc_interchange::role::action::SETUP_SHEET,
                setup,
                serde_json::Value::Null,
            )?
            else {
                return Err("段取り表を作れませんでした".into());
            };
            match out {
                Some(o) => {
                    write(&o, &contents)?;
                    println!("{} に段取り表を書きました", o.display());
                }
                None => print!("{}", String::from_utf8_lossy(&contents)),
            }
        }
        Command::Sim { file, setup, cell } => {
            let mut w = open(&file)?;
            // 省いた時は渡さない(操作の既定と設定に任せ、画面・MCP と同じ結果にする)
            let args = cell.map_or(serde_json::Value::Null, |c| json!({"cell": c}));
            let Ran::Report(r) =
                run_setup(&mut w, jc_interchange::role::action::SIMULATE, setup, args)?
            else {
                return Err("シミュレーションできませんでした".into());
            };
            println!("{}", r.info["summary"].as_str().unwrap_or(&r.text));
            for o in r.info["removed_by_operation"]
                .as_array()
                .into_iter()
                .flatten()
            {
                println!(
                    "  {}: {:.1} mm³",
                    o["name"].as_str().unwrap_or_default(),
                    o["volume"].as_f64().unwrap_or_default()
                );
            }
            // 食い込みなどを見つけた(壊れた入力の 1 と分ける)
            if !r.ok {
                return Ok(ExitCode::from(ProblemKind::Found.exit_code()));
            }
        }
        Command::Actions {
            file,
            selected,
            json,
        } => {
            let w = open(&file)?;
            let actions = w.actions(selected.map(FeatureId));
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&actions).expect("JSON にできる")
                );
            } else {
                print!("{}", jc_session::run_args::actions_text(&actions));
            }
        }
        Command::Run {
            file,
            id,
            args,
            selected,
            input,
            out,
            component,
        } => {
            let mut w = open(&file)?;
            activate(&mut w, component)?;
            let args = jc_session::run_args::parse_pairs(&args)?;
            let r = w.run(&id, selected.map(FeatureId), input.as_deref(), &args);
            if r.is_err() && w.report().error_count() > 0 {
                print!("{}", w.report_text());
            }
            match r? {
                Ran::Changed(added) => {
                    w.save(None)?;
                    for a in added {
                        println!("{a}");
                    }
                }
                Ran::File { contents, .. } => match out {
                    Some(o) => write(&o, &contents)?,
                    None => match String::from_utf8(contents) {
                        Ok(t) => print!("{t}"),
                        Err(_) => {
                            return Err(
                                "中身が文字でないので --out で書き出す先を指定します".into()
                            );
                        }
                    },
                },
                Ran::Files { files, .. } => {
                    let o = out.ok_or("いくつものファイルを書くので --out で名前を指定します")?;
                    for p in Headless::write_files(&o, &files)? {
                        println!("{}", p.display());
                    }
                }
                Ran::Message(m) => println!("{m}"),
                Ran::Report(r) => {
                    println!("{}", r.text);
                    if !r.ok {
                        return Ok(ExitCode::from(ProblemKind::Found.exit_code()));
                    }
                }
            }
        }
        Command::Types => {
            for d in jc_bundle::registry().descriptors() {
                println!("{}\t{}\t{}", d.type_id, d.category, d.label);
            }
        }
        Command::Settings => {
            let modules = jc_bundle::modules();
            jc_session::apply_user_settings(&modules);
            let v = serde_json::to_string_pretty(&modules.extensions.settings())
                .map_err(|e| e.to_string())?;
            println!("{v}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    // ログとクラッシュ報告を手元に残す(送信しない)。残せなくても動く
    jc_session::logging::init(jc_session::default_data_dir().as_deref(), "kmb-cli");
    log::info!("引数: {:?}", std::env::args().skip(1).collect::<Vec<_>>());
    let _crash = jc_session::default_data_dir().and_then(|d| {
        jc_session::crash::install(&d, "kmb-cli")
            .map_err(|e| eprintln!("クラッシュ報告を残せません: {e}"))
            .ok()
    });
    // 引数の誤りは clap に任せると 2(ここでは計算の誤り)で終わるので、入力の誤りの番号にする。
    // --help・--version は標準出力に出して 0
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() {
                ExitCode::from(ProblemKind::Input.exit_code())
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    match run(cli) {
        Ok(code) => code,
        // 終了の番号は誤りの種類で分ける(1 入力・2 計算・3 見つけた問題・4 使えない・5 ファイル)
        Err(e) => {
            eprintln!("誤り: {e}");
            ExitCode::from(e.kind.exit_code())
        }
    }
}
