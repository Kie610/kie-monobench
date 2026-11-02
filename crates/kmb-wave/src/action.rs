//! 操作 `wave.solve`: 仕事の JSON([`BemJob`])を解いて、結果の JSON([`BemOut`])を書き出す。
//!
//! 研究の画面の外から境界要素法を使う道(CLI・MCP・コマンド欄、画面では「実行」の ▾ の一覧)。
//! 画面ではファイルを選ぶ窓で仕事を選び、仕事のファイルの隣に `<仕事の名前>-<out>` で書く。
//! CLI・MCP は引数 `job` で仕事を渡し、指定の場所に同じ付け方で書く。
//! 解き方の誤りの種類([`WaveErrorKind`])は、そのまま [`Problem`] の種類へ写す。

use kmb_core::{Action, Arg, Context, Extension, Input, Outcome, Place, Problem, ProblemKind};
use serde_json::json;

use crate::{BemJob, BemOut, WaveError, WaveErrorKind, solve_bem};

/// 操作の id。
pub const SOLVE: &str = "wave.solve";

/// 結果のファイルの名前の後ろの既定。
const DEFAULT_OUT: &str = "音.json";

/// 1 波長に入る要素の数がこれを下回る周波数は、粗いと知らせる。
const COARSE: f64 = 6.0;

/// 音の波の解き方の操作。
pub struct WaveCommands;

impl Extension for WaveCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        vec![
            Action::new(SOLVE, "音の仕事を解く…", "fea")
                .arg(Arg::file(
                    "job",
                    "仕事の JSON(網・境界・音源・周波数・受け取る所)。省くとファイルを選ぶ窓を出す",
                ))
                .arg(
                    Arg::text(
                        "out",
                        "結果のファイルの名前の後ろ(仕事のファイルの名前に -<後ろ> を付けて書く)",
                    )
                    .default(DEFAULT_OUT),
                )
                .menu("実行")
                .icon("wave_solve")
                .aliases("おと おんきょう はどう ほうしゃ さんらん 音 音響 波動 放射 散乱 境界要素法 bem helmholtz acoustics wave sound")
                .hint("音の仕事の JSON を境界要素法で解き、周波数ごとの表面と受け取る所の音圧を JSON で書き出す")
                .at(&[Place::Menu])
                .input(Input::OpenFile {
                    title: "音の仕事を選ぶ".into(),
                    filter: "音の仕事(JSON)".into(),
                    extensions: vec!["json".into(), "JSON".into()],
                }),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        (id == SOLVE).then(|| solve(cx))
    }
}

fn solve(cx: &Context<'_>) -> Result<Outcome, Problem> {
    let path = cx
        .file
        .map(std::path::Path::to_path_buf)
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
    let job: BemJob = serde_json::from_str(&text)
        .map_err(|e| format!("{} は音の仕事の形ではありません: {e}", path.display()))?;
    let t = std::time::Instant::now();
    let result = solve_bem(&job, &()).map_err(problem)?;
    let seconds = t.elapsed().as_secs_f64();
    let info = summary(&job, &result, seconds);
    log::info!(
        "{SOLVE}: {} を解いた(周波数 {}、{seconds:.2} 秒)",
        path.display(),
        result.frequencies.len()
    );
    let bytes = serde_json::to_vec(&result).map_err(|e| e.to_string())?;
    Ok(Outcome::Files {
        files: vec![(out, bytes)],
        info,
    })
}

/// 解き方の誤りを、種類を保って [`Problem`] にする。
fn problem(e: WaveError) -> Problem {
    let kind = match e.kind {
        WaveErrorKind::Input => ProblemKind::Input,
        WaveErrorKind::Compute => ProblemKind::Compute,
        WaveErrorKind::Unavailable => ProblemKind::Unavailable,
    };
    Problem::new(kind, e.message)
}

/// CLI・MCP 向けの要約。粗い周波数は `warnings` に入れる(画面は知らせの欄に出す)。
fn summary(job: &BemJob, out: &BemOut, seconds: f64) -> serde_json::Value {
    let coarse: Vec<f64> = out
        .frequencies
        .iter()
        .filter(|f| f.elements_per_wavelength < COARSE)
        .map(|f| f.frequency)
        .collect();
    let mut warnings = Vec::new();
    if !coarse.is_empty() {
        let list: Vec<String> = coarse.iter().map(|f| format!("{f} Hz")).collect();
        warnings.push(format!(
            "1 波長に入る要素が {COARSE} 個を下回る周波数があります({})。網を細かくすると確かになります",
            list.join("、")
        ));
    }
    if out.cancelled {
        warnings.push("中止したので、途中の周波数までの結果です".to_string());
    }
    json!({
        "triangles": job.mesh.triangles.len(),
        "frequencies": out.frequencies.len(),
        "iterations": out.frequencies.iter().map(|f| f.iterations).max().unwrap_or(0),
        "residual": out.frequencies.iter().map(|f| f.residual).fold(0.0, f64::max),
        "seconds": seconds,
        "cancelled": out.cancelled,
        "warnings": warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Boundary, Complex64, Receivers, Source, icosphere};
    use std::sync::Arc;

    fn run(args: serde_json::Value, file: Option<&std::path::Path>) -> Result<Outcome, Problem> {
        let data = kmb_core::DocumentData::default();
        let report = kmb_core::RecomputeReport::default();
        let registry = kmb_core::Registry::new();
        let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(kmb_core::NullKernel);
        let mut cx = Context::new(&data, &report, &registry, &kernel);
        cx.args = &args;
        cx.file = file;
        WaveCommands.run(SOLVE, &cx).unwrap()
    }

    fn write_job(dir: &std::path::Path, job: &BemJob) -> std::path::PathBuf {
        let p = dir.join("球.json");
        std::fs::write(&p, serde_json::to_vec(job).unwrap()).unwrap();
        p
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kmb-wave-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 剛い球に平面波を当てた仕事を解き、結果の JSON が読み戻せて受け取る所の数がそろう。
    #[test]
    fn solve_writes_the_result_named_after_out() {
        let dir = scratch("solve");
        let job = BemJob {
            mesh: icosphere(0.1, 1),
            boundaries: vec![Boundary::Rigid],
            sources: vec![Source::PlaneWave {
                direction: [0.0, 0.0, 1.0],
                amplitude: Complex64::new(1.0, 0.0),
            }],
            frequencies: vec![200.0],
            receivers: Receivers::Points {
                points: vec![[0.0, 0.0, 0.5], [0.3, 0.0, 0.0]],
            },
            field_points: Vec::new(),
            medium: Default::default(),
            solver: Default::default(),
        };
        let path = write_job(&dir, &job);
        let o = run(
            json!({"job": path.to_str().unwrap(), "out": "結果.json"}),
            None,
        )
        .unwrap();
        let Outcome::Files { files, info } = o else {
            panic!("ファイルを返さなかった: {o:?}")
        };
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "結果.json");
        let back: BemOut = serde_json::from_slice(&files[0].1).unwrap();
        assert_eq!(back.frequencies.len(), 1);
        assert_eq!(back.frequencies[0].receivers.len(), 2);
        assert_eq!(info["frequencies"], 1);
        // 200 Hz で 80 個の三角形の球は細かい(粗い知らせを出さない)
        assert_eq!(info["warnings"], json!([]));
        // 画面の選んだファイルは引数より先に使い、out を省くと既定の名前
        let o = run(json!({}), Some(&path)).unwrap();
        let Outcome::Files { files, .. } = o else {
            panic!()
        };
        assert_eq!(files[0].0, DEFAULT_OUT);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 誤りの種類は Problem の種類へ写す(読めない・形が違う・網が開いている)。
    #[test]
    fn errors_keep_their_kind() {
        let dir = scratch("errors");
        let missing = dir.join("無い.json");
        let e = run(json!({"job": missing.to_str().unwrap()}), None).unwrap_err();
        assert_eq!(e.kind, ProblemKind::File);
        let bad = dir.join("違う.json");
        std::fs::write(&bad, "{\"mesh\": 1}").unwrap();
        let e = run(json!({"job": bad.to_str().unwrap()}), None).unwrap_err();
        assert_eq!(e.kind, ProblemKind::Input);
        // 三角形 1 つの開いた網は入力の誤り
        let mut mesh = icosphere(0.1, 0);
        mesh.triangles.truncate(1);
        let job = BemJob {
            mesh,
            boundaries: vec![],
            sources: vec![],
            frequencies: vec![100.0],
            receivers: Receivers::None,
            field_points: Vec::new(),
            medium: Default::default(),
            solver: Default::default(),
        };
        let path = write_job(&dir, &job);
        let e = run(json!({"job": path.to_str().unwrap()}), None).unwrap_err();
        assert_eq!(e.kind, ProblemKind::Input);
        // 引数が無ければ入力の誤り
        let e = run(json!({}), None).unwrap_err();
        assert_eq!(e.kind, ProblemKind::Input);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wave_errors_map_to_problem_kinds() {
        assert_eq!(
            problem(WaveError::unavailable("x")).kind,
            ProblemKind::Unavailable
        );
        assert_eq!(problem(WaveError::compute("x")).kind, ProblemKind::Compute);
    }
}
