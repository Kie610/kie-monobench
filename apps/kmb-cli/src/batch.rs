//! 文書の窓を使わない変換と、文書の命令を並べた台本。操作と変更は Headless に任せる。
use super::{Headless, Path, Problem, ProblemKind, Ran, json, show_output_warnings, write};
use kmb_core::{Action, FeatureId, Input};
use kmb_interchange::role::action as roles;

fn unavailable(message: impl Into<String>) -> Problem {
    Problem::new(ProblemKind::Unavailable, message)
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn writes(action: &Action, path: &Path) -> bool {
    matches!(&action.input, Input::SaveFile { extensions, .. }
        if extensions.iter().any(|s| s.eq_ignore_ascii_case(&extension(path))))
}

fn output(
    w: &mut Headless,
    action: &str,
    selected: Option<FeatureId>,
    path: &Path,
    pairs: &[String],
) -> Result<(), Problem> {
    let mut args = w.export_args(action, path);
    let given = kmb_session::run_args::parse_pairs(pairs)?;
    args.as_object_mut()
        .expect("書き出しの引数は表")
        .extend(given.as_object().expect("引数は表").clone());
    match w.run(action, selected, None, &args)? {
        Ran::File { contents, info, .. } => {
            show_output_warnings(&info);
            write(path, &contents)
        }
        Ran::Files { files, info } => {
            show_output_warnings(&info);
            Headless::write_files(path, &files).map_err(|e| Problem::new(ProblemKind::File, e))?;
            Ok(())
        }
        _ => Err(unavailable("指定した操作はファイルを書き出しません")),
    }
}

pub(super) fn convert(input: &Path, out: &Path, pairs: &[String]) -> Result<(), Problem> {
    let role = match extension(input).as_str() {
        "step" | "stp" => roles::IMPORT_STEP,
        "dxf" => roles::INSERT_DXF,
        "svg" => roles::INSERT_SVG,
        _ => {
            return Err(unavailable(
                "この入力の形式は変換できません(STEP・DXF・SVG)",
            ));
        }
    };
    let mut w = fresh();
    let read = w.action_for_role(&role).map_err(unavailable)?;
    let Ran::Changed(ids) = w.run(&read, None, Some(input), &json!({}))? else {
        return Err(unavailable("変換のための形を読み込めませんでした"));
    };
    check(&w)?;
    let selected = ids.last().copied();
    let action = if role == roles::IMPORT_STEP {
        w.save_action_for(out, None).map_err(unavailable)?
    } else {
        let candidates: Vec<_> = w
            .actions(selected)
            .into_iter()
            .filter(|a| writes(a, out) && a.role != Some(roles::EXPORT_BODIES))
            .collect();
        let enabled: Vec<_> = candidates.iter().filter(|a| a.disabled.is_none()).collect();
        match enabled.as_slice() {
            [a] => a.id.clone(),
            [] => {
                return Err(unavailable(
                    candidates
                        .first()
                        .and_then(|a| a.disabled.clone())
                        .unwrap_or_else(|| {
                            "この輪郭の入力と出力の組み合わせは変換できません".into()
                        }),
                ));
            }
            _ => {
                return Err(unavailable(
                    "この拡張子の輪郭の書き出しが複数あり、変換先を決められません",
                ));
            }
        }
    };
    output(&mut w, &action, selected, out, pairs)
}

fn fresh() -> Headless {
    let modules = kmb_bundle::modules();
    kmb_session::apply_user_settings(&modules);
    Headless::new(modules)
}

fn check(w: &Headless) -> Result<(), Problem> {
    if w.report().error_count() > 0 {
        Err(Problem::new(ProblemKind::Compute, w.report_text()))
    } else {
        Ok(())
    }
}

/// 引用した空白を 1 つの値にし、引用の中のバックスラッシュは引用符と自身だけを逃がす。
fn words(line: &str) -> Result<Vec<String>, Problem> {
    let mut result = Vec::new();
    let mut token = None::<String>;
    let mut quote = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            quote = !quote;
            token.get_or_insert_default();
        } else if quote && c == '\\' && chars.get(i + 1).is_some_and(|c| matches!(c, '"' | '\\')) {
            i += 1;
            token.get_or_insert_default().push(chars[i]);
        } else if !quote && c.is_whitespace() {
            if let Some(t) = token.take() {
                result.push(t);
            }
        } else {
            token.get_or_insert_default().push(c);
        }
        i += 1;
    }
    if quote {
        return Err("引用符が閉じていません".into());
    }
    if let Some(t) = token {
        result.push(t);
    }
    Ok(result)
}

fn line(w: &mut Headless, selected: &mut Option<FeatureId>, text: &str) -> Result<(), Problem> {
    let args = words(text)?;
    let Some(verb) = args.first() else {
        return Ok(());
    };
    let a = &args[1..];
    let need = |n| {
        if a.len() >= n {
            Ok(())
        } else {
            Err(Problem::from(format!("{verb} の引数が足りません")))
        }
    };
    let exact = |n| {
        if a.len() == n {
            Ok(())
        } else {
            Err(Problem::from(format!("{verb} の引数は {n} 個です")))
        }
    };
    match verb.to_ascii_lowercase().as_str() {
        "add_param" | "set_param" => {
            need(2)?;
            let expr = a[1..].join(" ");
            if verb.eq_ignore_ascii_case("add_param") {
                w.add_param(&a[0], &expr)?;
            } else {
                w.set_param(&a[0], &expr)?;
            }
        }
        "set_value" => {
            need(3)?;
            let id = a[0]
                .parse::<u64>()
                .map_err(|_| Problem::from("機能の ID は整数で書いてください"))?;
            w.set_value(FeatureId(id), &a[1], &a[2..].join(" "))?;
        }
        "run" => {
            need(1)?;
            let given = kmb_session::run_args::parse_pairs(&a[1..])?;
            match w.run(&a[0], *selected, None, &given)? {
                Ran::Changed(ids) => {
                    if let Some(id) = ids.last() {
                        *selected = Some(*id);
                    }
                }
                Ran::Message(t) => println!("{t}"),
                Ran::Report(r) => {
                    println!("{}", r.text);
                    if !r.ok {
                        return Err(Problem::new(ProblemKind::Found, r.text));
                    }
                }
                Ran::File { .. } | Ran::Files { .. } => {
                    return Err(unavailable(
                        "台本の書き出し先は --export で指定してください",
                    ));
                }
            }
        }
        "undo" => {
            exact(0)?;
            w.undo()?;
            *selected = None;
        }
        "redo" => {
            exact(0)?;
            w.redo()?;
            *selected = None;
        }
        "status" => {
            exact(0)?;
            println!("{}", w.status());
        }
        "actions" => {
            exact(0)?;
            print!(
                "{}",
                kmb_session::run_args::actions_text(&w.actions(*selected))
            );
        }
        "echo" => println!("{}", a.join(" ")),
        "wait" => {
            exact(0)?;
            w.recompute();
        }
        "click" | "rclick" | "dclick" | "hover" | "scroll" | "type" | "key" | "select" | "pick"
        | "drag" | "expect" | "until" | "find" | "tree" | "pane" | "shot" => {
            return Err(unavailable(format!(
                "{verb} は動いている画面の台本で使ってください"
            )));
        }
        _ => return Err(Problem::from(format!("台本のコマンド {verb} は知りません"))),
    }
    check(w)
}

pub(super) fn script(
    path: &Path,
    document: Option<&Path>,
    save: Option<&Path>,
    export: Option<&Path>,
) -> Result<(), Problem> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Problem::new(ProblemKind::File, format!("{}: {e}", path.display())))?;
    let mut w = match document {
        Some(p) => super::open(p)?,
        None => fresh(),
    };
    let mut selected = None;
    for (n, text) in text.lines().enumerate() {
        let text = text.trim().trim_start_matches('\u{feff}');
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        if let Err(mut e) = line(&mut w, &mut selected, text) {
            e.message = format!("{}:{} 行目: {}", path.display(), n + 1, e.message);
            return Err(e);
        }
    }
    check(&w)?;
    if let Some(out) = export {
        let action = w.save_action_for(out, None).map_err(unavailable)?;
        output(&mut w, &action, selected, out, &[])?;
    }
    if let Some(out) = save {
        w.save(Some(out))
            .map_err(|e| Problem::new(ProblemKind::File, e))?;
    }
    Ok(())
}
