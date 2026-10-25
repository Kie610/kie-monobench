//! 操作の引数の宣言([`Action::args`](crate::Action::args))。
//!
//! 操作は受け取る引数(鍵・型・単位・既定)を宣言する。[`Extensions::run`](crate::Extensions::run)
//! は実行の前に宣言で引数を確かめ(知らない鍵・型の違いは誤り)、省いた引数に既定を入れる。
//! 画面・CLI・MCP・コマンド欄は、この宣言から引数の一覧と既定を出す。
//!
//! 長さの引数の数は mm、角度は deg で読む。文字で `"0.5 in"`・`"90 deg"` のように単位つきの式を
//! 渡すと、宣言の単位に直して数にする(数だけを打つ欄の単位の規則 [`FieldUnit`] と同じ考え)。

use crate::{Choice, FieldUnit, RefKind};
use serde::Serialize;
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::collections::BTreeSet;

/// 引数の型。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArgKind {
    /// 数(単位は [`Arg::unit`])
    Number,
    /// 整数(個数・番号)
    Integer,
    Bool,
    Text,
    /// 読むファイルのパス(文字)。AI に許したフォルダを持つ道(MCP)は、外を指すパスを断る
    File,
    /// 決まった値から選ぶ
    Choice {
        choices: Vec<Choice>,
    },
    /// 3 つの数の組(点・向き。長さなら mm)
    Vector,
    /// 3D のビューで選ぶ物([`crate::Ref`] か、その並び)
    Pick {
        accept: Vec<RefKind>,
    },
    /// 機能の番号
    Feature,
    /// 並び(中身は操作の説明のとおり)
    List,
    /// 入れ子の値(中身は操作の説明のとおり)
    Object,
    /// 形の決まらない値(名前か入れ子の値を受ける視点など。中身は操作の説明のとおり)
    Value,
}

/// 引数 1 つの宣言。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Arg {
    pub key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub help: String,
    #[serde(flatten)]
    pub kind: ArgKind,
    /// 数の単位(長さは mm、角度は deg で読む)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<FieldUnit>,
    /// 省いた時の値(`None` なら省いたまま。設定の値を使うなど、省いたことに意味がある引数)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
}

impl Arg {
    pub fn new(key: &str, kind: ArgKind, help: &str) -> Self {
        Arg {
            key: key.into(),
            help: help.into(),
            kind,
            unit: None,
            default: None,
            required: false,
        }
    }

    /// 長さ(数は mm)。
    pub fn length(key: &str, help: &str) -> Self {
        Arg::number(key, help, FieldUnit::Length)
    }

    /// 角度(数は deg)。
    pub fn angle(key: &str, help: &str) -> Self {
        Arg::number(key, help, FieldUnit::Angle)
    }

    /// 単位 `unit` の数。
    pub fn number(key: &str, help: &str, unit: FieldUnit) -> Self {
        Arg {
            unit: Some(unit),
            ..Arg::new(key, ArgKind::Number, help)
        }
    }

    pub fn integer(key: &str, help: &str) -> Self {
        Arg::new(key, ArgKind::Integer, help)
    }

    pub fn flag(key: &str, help: &str) -> Self {
        Arg::new(key, ArgKind::Bool, help)
    }

    pub fn text(key: &str, help: &str) -> Self {
        Arg::new(key, ArgKind::Text, help)
    }

    /// 読むファイルのパス。パスを読む引数は文字の引数にせず、これで宣言する。
    pub fn file(key: &str, help: &str) -> Self {
        Arg::new(key, ArgKind::File, help)
    }

    /// `(値, 名前)` から選ぶ。
    pub fn choice(key: &str, help: &str, choices: &[(&str, &str)]) -> Self {
        let choices = choices
            .iter()
            .map(|(v, l)| Choice {
                value: (*v).into(),
                label: (*l).into(),
            })
            .collect();
        Arg::new(key, ArgKind::Choice { choices }, help)
    }

    pub fn vector(key: &str, help: &str) -> Self {
        Arg::new(key, ArgKind::Vector, help)
    }

    pub fn pick(key: &str, help: &str, accept: &[RefKind]) -> Self {
        Arg::new(
            key,
            ArgKind::Pick {
                accept: accept.to_vec(),
            },
            help,
        )
    }

    pub fn feature(key: &str, help: &str) -> Self {
        Arg::new(key, ArgKind::Feature, help)
    }

    pub fn list(key: &str, help: &str) -> Self {
        Arg::new(key, ArgKind::List, help)
    }

    pub fn object(key: &str, help: &str) -> Self {
        Arg::new(key, ArgKind::Object, help)
    }

    pub fn value(key: &str, help: &str) -> Self {
        Arg::new(key, ArgKind::Value, help)
    }

    /// 省いた時の値。
    pub fn default(mut self, v: impl Into<Value>) -> Self {
        self.default = Some(v.into());
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// 値 `v` を宣言で確かめ、単位つきの式を数に直した値を返す(`null` は省いたのと同じでそのまま)。
    fn check(&self, v: &Value) -> Result<Value, String> {
        let k = &self.key;
        if v.is_null() {
            return Ok(Value::Null);
        }
        let bad = |what: &str| Err(format!("引数 {k} は{what}で指定します(渡した値: {v})"));
        match &self.kind {
            ArgKind::Number => match v {
                Value::Number(_) => Ok(v.clone()),
                Value::String(s) => self.number_of(s).map(Value::from),
                _ => bad("数"),
            },
            ArgKind::Integer | ArgKind::Feature => match v.as_f64() {
                Some(n) if n.fract() == 0.0 && (self.kind == ArgKind::Integer || n >= 0.0) => {
                    Ok(v.clone())
                }
                _ => bad(if self.kind == ArgKind::Feature {
                    "機能の番号"
                } else {
                    "整数"
                }),
            },
            ArgKind::Bool => match v {
                Value::Bool(_) => Ok(v.clone()),
                _ => bad("true か false"),
            },
            ArgKind::Text | ArgKind::File => match v {
                Value::String(_) => Ok(v.clone()),
                _ => bad("文字"),
            },
            ArgKind::Choice { choices } => match v.as_str() {
                Some(s) if choices.iter().any(|c| c.value == s) => Ok(v.clone()),
                _ => {
                    let names: Vec<&str> = choices.iter().map(|c| c.value.as_str()).collect();
                    bad(&format!(" {} のどれか", names.join("・")))
                }
            },
            ArgKind::Vector => match v.as_array() {
                Some(a) if a.len() == 3 && a.iter().all(Value::is_number) => Ok(v.clone()),
                _ => bad(" 3 つの数の組 [x, y, z] "),
            },
            ArgKind::Pick { .. } => match v {
                Value::Object(_) | Value::Array(_) => Ok(v.clone()),
                _ => bad("選ぶ物({\"kind\": …} か、その並び)"),
            },
            ArgKind::List => match v {
                Value::Array(_) => Ok(v.clone()),
                _ => bad("並び"),
            },
            ArgKind::Object => match v {
                Value::Object(_) => Ok(v.clone()),
                _ => bad(" {鍵: 値} の形"),
            },
            ArgKind::Value => Ok(v.clone()),
        }
    }

    /// 単位つきの式 `s` を、宣言の単位の数にする(長さは mm、角度は deg)。
    fn number_of(&self, s: &str) -> Result<f64, String> {
        let k = &self.key;
        let q = kmb_expr::eval_str(s, &|_| None).map_err(|e| format!("引数 {k} の式 {s}: {e}"))?;
        let want = match self.unit {
            Some(FieldUnit::Length) => kmb_expr::Dim::LENGTH,
            Some(FieldUnit::Angle) => kmb_expr::Dim::ANGLE,
            _ => kmb_expr::Dim::NONE,
        };
        // 数だけなら宣言の単位(長さは mm、角度は deg)
        if q.dim.is_none() {
            return Ok(q.value);
        }
        if q.dim != want {
            return Err(format!("引数 {k} の式 {s} は単位が合いません({})", q.dim));
        }
        Ok(if want == kmb_expr::Dim::ANGLE {
            q.value.to_degrees()
        } else {
            q.value
        })
    }
}

/// 操作 `id` に渡す引数 `given` を宣言 `decl` で確かめる。知らない鍵・型の違い・無い必須の引数は
/// 誤り。単位つきの式は数に直し、省いた引数には既定を入れた値を返す(`null` は `{}` と同じ)。
pub(crate) fn check_args(id: &str, decl: &[Arg], given: &Value) -> Result<Value, String> {
    let empty = Map::new();
    let given = match given {
        Value::Null => &empty,
        Value::Object(m) => m,
        _ => return Err(format!("操作 {id} の引数は {{鍵: 値}} の形で渡します")),
    };
    let mut out = Map::new();
    for (k, v) in given {
        let Some(a) = decl.iter().find(|a| &a.key == k) else {
            let names: Vec<&str> = decl.iter().map(|a| a.key.as_str()).collect();
            return Err(if names.is_empty() {
                format!("操作 {id} は引数を受け取りません(渡した引数: {k})")
            } else {
                format!(
                    "操作 {id} に引数 {k} はありません(使える引数: {})",
                    names.join("・")
                )
            });
        };
        out.insert(k.clone(), a.check(v)?);
    }
    for a in decl {
        if out.contains_key(&a.key) {
            continue;
        }
        if a.required {
            return Err(format!("操作 {id} には引数 {} が要ります", a.key));
        }
        if let Some(d) = &a.default {
            out.insert(a.key.clone(), d.clone());
        }
    }
    Ok(Value::Object(out))
}

thread_local! {
    /// 実行中の操作が読んだ引数の鍵(読んだ記録を取っている間だけ `Some`)。
    static READ: RefCell<Option<BTreeSet<String>>> = const { RefCell::new(None) };
}

/// 引数の鍵 `key` を読んだことを記録する([`crate::Context::arg`] が呼ぶ)。
pub(crate) fn note_read(key: &str) {
    READ.with(|r| {
        if let Some(s) = r.borrow_mut().as_mut() {
            s.insert(key.into());
        }
    });
}

/// `f` を走らせる間に、操作が読んだ引数の鍵を集める(同じスレッドで読んだ物だけ)。
pub(crate) fn recording<T>(f: impl FnOnce() -> T) -> (T, BTreeSet<String>) {
    let before = READ.with(|r| r.borrow_mut().replace(BTreeSet::new()));
    let out = f();
    let read = READ.with(|r| std::mem::replace(&mut *r.borrow_mut(), before));
    (out, read.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decl() -> Vec<Arg> {
        vec![
            Arg::length("tolerance", "許容差").default(0.02),
            Arg::angle("angle", "角度"),
            Arg::flag("ascii", "ASCII"),
            Arg::choice("unit", "単位", &[("mm", "mm"), ("in", "in")]),
            Arg::integer("count", "個数"),
            Arg::text("name", "名前").required(),
        ]
    }

    #[test]
    fn unknown_keys_and_wrong_types_are_errors() {
        let e = check_args("io.x", &decl(), &json!({"name": "a", "tolerence": 0.1})).unwrap_err();
        assert!(e.contains("引数 tolerence はありません"), "{e}");
        assert!(e.contains("tolerance"), "使える引数を挙げる: {e}");
        let e = check_args("io.x", &decl(), &json!({"name": "a", "tolerance": true})).unwrap_err();
        assert!(e.contains("tolerance は数で"), "{e}");
        let e = check_args("io.x", &decl(), &json!({"name": "a", "unit": "ft"})).unwrap_err();
        assert!(e.contains("mm・in"), "{e}");
        let e = check_args("io.x", &decl(), &json!({"name": "a", "count": 1.5})).unwrap_err();
        assert!(e.contains("整数"), "{e}");
        let e = check_args("io.x", &decl(), &json!({})).unwrap_err();
        assert!(e.contains("引数 name が要ります"), "{e}");
        let e = check_args("io.x", &[], &json!({"a": 1})).unwrap_err();
        assert!(e.contains("引数を受け取りません"), "{e}");
    }

    #[test]
    fn defaults_fill_and_units_convert() {
        let v = check_args("io.x", &decl(), &json!({"name": "a"})).unwrap();
        assert_eq!(v, json!({"name": "a", "tolerance": 0.02}));
        let v = check_args(
            "io.x",
            &decl(),
            &json!({"name": "a", "tolerance": "0.001 in", "angle": "0.5 * 180 deg"}),
        )
        .unwrap();
        assert!(
            (v["tolerance"].as_f64().unwrap() - 0.0254).abs() < 1e-12,
            "{v}"
        );
        assert!((v["angle"].as_f64().unwrap() - 90.0).abs() < 1e-9, "{v}");
        // 数だけの式は宣言の単位(mm)
        let v = check_args("io.x", &decl(), &json!({"name": "a", "tolerance": "0.5"})).unwrap();
        assert_eq!(v["tolerance"], json!(0.5));
        let e =
            check_args("io.x", &decl(), &json!({"name": "a", "tolerance": "3 deg"})).unwrap_err();
        assert!(e.contains("単位が合いません"), "{e}");
        // null は省いたのと同じ扱いで残す(手順で値なしに決めた印)
        let v = check_args("io.x", &decl(), &json!({"name": "a", "angle": null})).unwrap();
        assert_eq!(v["angle"], Value::Null);
        assert_eq!(check_args("io.x", &[], &Value::Null).unwrap(), json!({}));
    }

    #[test]
    fn recording_collects_reads_on_this_thread() {
        let ((), read) = recording(|| {
            note_read("a");
            note_read("b");
        });
        assert_eq!(read.into_iter().collect::<Vec<_>>(), ["a", "b"]);
        note_read("c"); // 記録していない間は何もしない
        let ((), read) = recording(|| {});
        assert!(read.is_empty());
    }
}
