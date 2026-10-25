//! CLI の `kmb run`・`kmb actions` とコマンド欄の `run` が共有する、操作の引数の読み方と一覧の文。
//! 引数の確かめ(知らない鍵・型・単位)は操作の宣言([`kmb_core::Action::args`])で核が行う。

use kmb_core::{Action, Arg, ArgKind, FieldUnit};
use serde_json::{Map, Value};

/// `鍵=値` の並びを引数の JSON にする。値は JSON として読めればその値(数・true・配列・`{…}`)、
/// 読めなければ文字(`5 mm`・`top` など。単位つきの長さは宣言の単位に直る)。
pub fn parse_pairs<S: AsRef<str>>(pairs: &[S]) -> Result<Value, String> {
    let mut m = Map::new();
    for p in pairs {
        let p = p.as_ref();
        let (k, v) = p
            .split_once('=')
            .ok_or_else(|| format!("引数 {p} は 鍵=値 の形で書きます"))?;
        let k = k.trim();
        if k.is_empty() {
            return Err(format!("引数 {p} に鍵がありません"));
        }
        let v = v.trim();
        let value = serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.into()));
        if m.insert(k.into(), value).is_some() {
            return Err(format!("引数 {k} を 2 回書いています"));
        }
    }
    Ok(Value::Object(m))
}

/// 引数 1 つの説明(`  cell(長さ mm): 格子の大きさ…`)。
fn arg_line(a: &Arg) -> String {
    let kind = match &a.kind {
        ArgKind::Number => match &a.unit {
            Some(FieldUnit::Length) => "長さ mm".into(),
            Some(FieldUnit::Angle) => "角度 deg".into(),
            Some(FieldUnit::Plain(u)) => format!("数 {u}"),
            _ => "数".into(),
        },
        ArgKind::Integer => "整数".into(),
        ArgKind::Bool => "true か false".into(),
        ArgKind::Text => "文字".into(),
        ArgKind::File => "ファイルのパス".into(),
        ArgKind::Choice { choices } => {
            let v: Vec<&str> = choices.iter().map(|c| c.value.as_str()).collect();
            v.join("・")
        }
        ArgKind::Vector => "[x, y, z]".into(),
        ArgKind::Pick { .. } => "選ぶ物".into(),
        ArgKind::Feature => "機能の番号".into(),
        ArgKind::List => "並び".into(),
        ArgKind::Object => "{…}".into(),
        ArgKind::Value => "値".into(),
    };
    let mut s = format!("  {}({kind}", a.key);
    if a.required {
        s.push_str("、必須");
    }
    if let Some(d) = &a.default {
        s.push_str(&format!("、既定 {d}"));
    }
    s.push(')');
    if !a.help.is_empty() {
        s.push_str(": ");
        s.push_str(&a.help);
    }
    s
}

/// 操作の一覧の文(1 行目に ID と名前と使えない理由、続けて引数を 1 行ずつ)。
pub fn actions_text(actions: &[Action]) -> String {
    let mut out = String::new();
    for a in actions {
        out.push_str(&format!("{}\t{}", a.id, a.label));
        if let Some(why) = &a.disabled {
            out.push_str(&format!("\t(使えない: {why})"));
        }
        out.push('\n');
        for arg in &a.args {
            out.push_str(&arg_line(arg));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pairs_read_json_or_text() {
        let v = parse_pairs(&[
            "tolerance=0.05",
            "view=top",
            "each=true",
            "d=5 mm",
            "p=[1, 2, 3]",
        ])
        .unwrap();
        assert_eq!(
            v,
            json!({"tolerance": 0.05, "view": "top", "each": true, "d": "5 mm", "p": [1, 2, 3]})
        );
        assert!(parse_pairs(&["tolerance"]).unwrap_err().contains("鍵=値"));
        assert!(parse_pairs(&["a=1", "a=2"]).unwrap_err().contains("2 回"));
    }

    #[test]
    fn the_list_shows_args_with_kind_and_default() {
        let a = Action::new("io.x", "書き出す", "io")
            .arg(Arg::length("tolerance", "許容差").default(0.02))
            .arg(Arg::flag("ascii", "ASCII").required())
            .disabled_if(Some("ボディがありません"));
        let t = actions_text(&[a]);
        assert!(
            t.starts_with("io.x\t書き出す\t(使えない: ボディがありません)\n"),
            "{t}"
        );
        assert!(
            t.contains("  tolerance(長さ mm、既定 0.02): 許容差\n"),
            "{t}"
        );
        assert!(t.contains("  ascii(true か false、必須): ASCII\n"), "{t}");
    }
}
