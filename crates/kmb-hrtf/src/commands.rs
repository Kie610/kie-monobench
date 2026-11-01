//! HRTF の操作(CLI・MCP・コマンド欄に出す): 処理の手順のファイルを流し、書き出す物を返す。
//!
//! 手順のファイルの `inputs` は「名前 → 読む SOFA のパス」。パスは手順のファイルのフォルダからの
//! 相対で、外へ出る形(絶対のパス・`..`)は断る。読むファイルを引数 `recipe` のフォルダの中に
//! 閉じ込めるので、AI に許したフォルダの確かめ(`ArgKind::File`)が入力にもそのまま効く。
//!
//! `hrtf.compute` は頭の網の仕事の JSON([`HeadJob`])を解き、HRIR を SOFA で書き出す(研究の画面の
//! 外から計算する道。画面では「実行」の ▾ の一覧からファイルを選ぶ)。

use std::collections::BTreeMap;
use std::path::{Component, Path};

use kmb_core::{Action, Arg, Context, Extension, Input, Outcome, Place, Problem, ProblemKind};

use crate::sofa::SofaSpec;
use crate::{ErrorKind, HeadJob, HrtfError, Recipe, RunOptions, run_with};

/// 処理の手順を流す。
pub const PROCESS: &str = "hrtf.process";

/// 頭の網の仕事を解いて SOFA を書く。
pub const COMPUTE: &str = "hrtf.compute";

/// 書き出す SOFA の名前の既定。
const DEFAULT_OUT: &str = "HRTF.sofa";

/// HRTF の操作。
pub struct HrtfCommands;

impl Extension for HrtfCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        vec![
            Action::new(PROCESS, "HRTF の形式をそろえる", "fea")
                .aliases("hrtf sofa とうぶでんたつかんすう でんたつかんすう そろえる へんかん convert regrid resample")
                .hint("処理の手順(JSON)で、SOFA の HRTF を読み、サンプリング周波数・方向の格子・立ち上がり・位相をそろえて、SOFA や分けた表に書き出す")
                .arg(
                    Arg::file(
                        "recipe",
                        "処理の手順のファイル(JSON。inputs に名前と SOFA のパス(手順のフォルダからの相対)、steps に段の並び)",
                    )
                    .required(),
                ),
            Action::new(COMPUTE, "頭の網から HRTF を計算…", "fea")
                .arg(Arg::file(
                    "job",
                    "仕事の JSON(頭の網 m・左右の耳の入口・方向・周波数)。省くとファイルを選ぶ窓を出す",
                ))
                .arg(
                    Arg::text("out", "書き出す SOFA の名前(仕事のファイルの名前に -<名前> を付けて書く)")
                        .default(DEFAULT_OUT),
                )
                .menu("実行")
                .icon("hrtf_solve")
                .aliases("hrtf sofa とうぶでんたつかんすう でんたつかんすう みみ あたま 頭部伝達関数 頭 耳 境界要素法 bem binaural head ear")
                .hint("頭の網の仕事の JSON を境界要素法で解き、全方向の HRIR を SOFA で書き出す")
                .at(&[Place::Menu])
                .input(Input::OpenFile {
                    title: "頭の網の仕事を選ぶ".into(),
                    filter: "頭の網の仕事(JSON)".into(),
                    extensions: vec!["json".into(), "JSON".into()],
                }),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        match id {
            PROCESS => Some(process(cx)),
            COMPUTE => Some(compute(cx)),
            _ => None,
        }
    }
}

fn compute(cx: &Context<'_>) -> Result<Outcome, Problem> {
    let path = cx
        .file
        .map(Path::to_path_buf)
        .or_else(|| cx.arg_str("job").map(std::path::PathBuf::from))
        .ok_or("仕事の JSON を選んでいません(引数 job)")?;
    let out = cx
        .arg_str("out")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_OUT)
        .to_string();
    let text = std::fs::read_to_string(&path).map_err(|e| {
        Problem::new(
            ProblemKind::File,
            format!("{} を読めません: {e}", path.display()),
        )
    })?;
    let job: HeadJob = serde_json::from_str(&text)
        .map_err(|e| format!("{} は頭の網の仕事の形ではありません: {e}", path.display()))?;
    let t = std::time::Instant::now();
    let head = crate::compute(&job, &()).map_err(problem)?;
    let seconds = t.elapsed().as_secs_f64();
    let spec = SofaSpec {
        now: Some(utc_now()),
        ..SofaSpec::default()
    };
    let bytes = crate::sofa::write(&head.set, &spec).map_err(problem)?;
    let tr = &head.transfer;
    let worst = tr
        .notes
        .iter()
        .map(|n| n.elements_per_wavelength)
        .fold(f64::INFINITY, f64::min);
    let mut warnings = Vec::new();
    if worst < 6.0 {
        warnings.push(format!(
            "1 波長に入る要素が 6 個を下回る周波数があります(いちばん少ない所で {worst:.1} 個)。網を細かくすると確かになります"
        ));
    }
    log::info!(
        "{COMPUTE}: {} を解いた(三角形 {:?}、周波数 {}、方向 {}、{seconds:.1} 秒)",
        path.display(),
        tr.mesh_triangles,
        tr.frequencies.len(),
        tr.dirs.len()
    );
    let info = serde_json::json!({
        "triangles": tr.mesh_triangles,
        "ear_triangles": [tr.ear_triangles[0].len(), tr.ear_triangles[1].len()],
        "frequencies": tr.frequencies.len(),
        "directions": tr.dirs.len(),
        "taps": head.set.taps(),
        "fs": head.set.fs,
        "iterations": tr.notes.iter().map(|n| n.iterations).max().unwrap_or(0),
        "seconds": seconds,
        "warnings": warnings,
    });
    Ok(Outcome::Files {
        files: vec![(out, bytes)],
        info,
    })
}

fn problem(e: HrtfError) -> Problem {
    let kind = match e.kind {
        ErrorKind::Input => ProblemKind::Input,
        ErrorKind::Compute => ProblemKind::Compute,
        ErrorKind::Unsupported => ProblemKind::Unavailable,
    };
    Problem::new(kind, e.message)
}

fn process(cx: &Context<'_>) -> Result<Outcome, Problem> {
    let path = cx
        .arg("recipe")
        .and_then(|v| v.as_str())
        .ok_or("recipe に手順のファイルを渡す")?;
    let path = Path::new(path);
    let text = std::fs::read_to_string(path).map_err(|e| {
        Problem::new(
            ProblemKind::File,
            format!("{} を読めない: {e}", path.display()),
        )
    })?;
    let recipe = Recipe::from_json(&text).map_err(problem)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut sets = BTreeMap::new();
    for (name, rel) in &recipe.inputs {
        let rel_path = Path::new(rel);
        if rel_path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(Problem::new(
                ProblemKind::Input,
                format!(
                    "inputs の {name} のパス「{rel}」は手順のフォルダからの相対にする(絶対のパスと .. は使わない)"
                ),
            ));
        }
        let file = dir.join(rel_path);
        let bytes = std::fs::read(&file).map_err(|e| {
            Problem::new(
                ProblemKind::File,
                format!("{} を読めない: {e}", file.display()),
            )
        })?;
        let set =
            crate::sofa::read(&bytes).map_err(|e| problem(e.at(&format!("{}", file.display()))))?;
        log::info!(
            "HRTF {name} を読んだ({} 方向、{} tap、{} Hz)",
            set.dirs.len(),
            set.taps(),
            set.fs
        );
        sets.insert(name.clone(), set);
    }
    let opts = RunOptions {
        now: Some(utc_now()),
    };
    let files = run_with(&recipe, &mut sets, &opts).map_err(problem)?;
    log::info!(
        "HRTF の手順を流した(段 {} 個、書き出し {} 個)",
        recipe.steps.len(),
        files.len()
    );
    let info = serde_json::json!({
        "sets": sets
            .iter()
            .map(|(k, s)| (k.clone(), serde_json::json!({
                "dirs": s.dirs.len(),
                "taps": s.taps(),
                "fs": s.fs,
                "history": s.history,
            })))
            .collect::<serde_json::Map<_, _>>(),
    });
    if files.is_empty() {
        return Ok(Outcome::Message(format!(
            "手順を流した(書き出す段が無いのでファイルは作っていない。集まり {} 個)",
            sets.len()
        )));
    }
    Ok(Outcome::Files {
        files: files.into_iter().map(|f| (f.name, f.bytes)).collect(),
        info,
    })
}

/// 今の UTC の日時(SOFA の決まりの書き方 `yyyy-mm-dd HH:MM:SS`)。
fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    civil(secs)
}

/// 1970-01-01 からの秒を暦の日時にする(先発グレゴリオ暦)。
fn civil(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dir, Directions};
    use std::sync::Arc;

    /// 確かめること: 頭の網の仕事(剛い球)を解き、書いた SOFA が読めて方向の数とサンプリング周波数が
    /// そろう。仕事のファイルが無ければファイルの誤り。
    #[test]
    fn compute_writes_a_sofa_named_after_out() {
        let dir = std::env::temp_dir().join(format!("kmb-hrtf-compute-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mesh = kmb_wave::icosphere(0.0875, 1);
        let job = HeadJob {
            ears: [[0.0, 0.0875, 0.0], [0.0, -0.0875, 0.0]].map(|e| {
                // 入口は表面の点: 一番近い頂点
                *mesh
                    .vertices
                    .iter()
                    .min_by(|a, b| {
                        let d = |p: &[f64; 3]| (0..3).map(|k| (p[k] - e[k]).powi(2)).sum::<f64>();
                        d(a).total_cmp(&d(b))
                    })
                    .unwrap()
            }),
            directions: Directions::List {
                dirs: vec![
                    Dir {
                        az: 90.0,
                        el: 0.0,
                        dist: 1.0,
                    },
                    Dir {
                        az: 270.0,
                        el: 0.0,
                        dist: 1.0,
                    },
                ],
            },
            fs: 8000.0,
            taps: 32,
            max_hz: 1000.0,
            lead: 4.0,
            ear_radius: 0.0,
            ..serde_json::from_value(serde_json::json!({
                "mesh": mesh, "ears": [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]
            }))
            .unwrap()
        };
        let path = dir.join("球.json");
        std::fs::write(&path, serde_json::to_vec(&job).unwrap()).unwrap();
        let data = kmb_core::DocumentData::default();
        let report = kmb_core::RecomputeReport::default();
        let registry = kmb_core::Registry::new();
        let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(kmb_core::NullKernel);
        let run = |args: serde_json::Value| {
            let mut cx = Context::new(&data, &report, &registry, &kernel);
            cx.args = &args;
            HrtfCommands.run(COMPUTE, &cx).unwrap()
        };
        let o = run(serde_json::json!({"job": path.to_str().unwrap(), "out": "球.sofa"})).unwrap();
        let Outcome::Files { files, info } = o else {
            panic!("ファイルを返さなかった: {o:?}")
        };
        assert_eq!(files[0].0, "球.sofa");
        let set = crate::sofa::read(&files[0].1).unwrap();
        assert_eq!(set.dirs.len(), 2);
        assert_eq!(set.fs, 8000.0);
        assert_eq!(info["directions"], 2);
        let e =
            run(serde_json::json!({"job": dir.join("無い.json").to_str().unwrap()})).unwrap_err();
        assert_eq!(e.kind, ProblemKind::File);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 確かめること: 秒から暦の日時に直せる(閏年の 2 月 29 日と年の変わり目)。
    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), "1970-01-01 00:00:00");
        assert_eq!(civil(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(civil(1_798_761_599), "2026-12-31 23:59:59");
    }
}
