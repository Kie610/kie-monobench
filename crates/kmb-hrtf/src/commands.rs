//! HRTF の操作(CLI・MCP・コマンド欄に出す): 処理の手順のファイルを流し、書き出す物を返す。
//!
//! 手順のファイルの `inputs` は「名前 → 読む SOFA のパス」。パスは手順のファイルのフォルダからの
//! 相対で、外へ出る形(絶対のパス・`..`)は断る。読むファイルを引数 `recipe` のフォルダの中に
//! 閉じ込めるので、AI に許したフォルダの確かめ(`ArgKind::File`)が入力にもそのまま効く。

use std::collections::BTreeMap;
use std::path::{Component, Path};

use kmb_core::{Action, Arg, Context, Extension, Outcome, Problem, ProblemKind};

use crate::{ErrorKind, HrtfError, Recipe, RunOptions, run_with};

/// 処理の手順を流す。
pub const PROCESS: &str = "hrtf.process";

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
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        (id == PROCESS).then(|| process(cx))
    }
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
    use super::civil;

    /// 確かめること: 秒から暦の日時に直せる(閏年の 2 月 29 日と年の変わり目)。
    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), "1970-01-01 00:00:00");
        assert_eq!(civil(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(civil(1_798_761_599), "2026-12-31 23:59:59");
    }
}
