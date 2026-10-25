//! 機能の知らせの文を、内部の言葉(カーネルの状態の番号、面の内部の名前、データの読み込みの文)を
//! 除いて読みやすく直し、見出しと直し方の例を添える(機能ごとに
//! 「計算に失敗しました」と原因・直し方を並べる)。元の文は確かめる窓の「元の文」と
//! 「文を写す」に残す(不具合の報告に使えるように)。

use crate::Workspace;
use crate::timeline_ui::Health;
use kmb_core::{FeatureId, FeatureStatus};

/// 前に付く、種類だけを言う言葉(見出しで言い直すので文からは外す)。
const PREFIXES: &[&str] = &["データが不正です: ", "形状カーネルの誤り: "];

/// 文を読みやすく直す。`face` は面の内部の名前を見せる名前にする(分からなければ `None`)。
pub(crate) fn plain(raw: &str, face: &dyn Fn(&str) -> Option<String>) -> String {
    raw.lines()
        .map(|l| plain_line(l, face))
        .collect::<Vec<_>>()
        .join("\n")
}

fn plain_line(raw: &str, face: &dyn Fn(&str) -> Option<String>) -> String {
    let mut s = raw.trim().to_string();
    // 種類の言葉は何重にも付くことがある
    while let Some(p) = PREFIXES.iter().find(|p| s.starts_with(**p)) {
        s = s[p.len()..].to_string();
    }
    if let Some(rest) = s.strip_prefix("宣言していない入力を読みました: ") {
        s = format!("内部の誤りです(不具合として知らせてください): {rest}");
    }
    s = drop_status(&s);
    s = s
        .replace(
            "SMLib の中で例外が起きました: ",
            "形の計算の途中で問題が起きました: ",
        )
        .replace(
            "SMLib の中で不明な例外が起きました",
            "形の計算の途中で問題が起きました",
        );
    // 「ブーリアン: ブーリアン に失敗しました」のように、何をしたかが二度出る
    if let Some((what, rest)) = s.split_once(": ")
        && rest.starts_with(what)
    {
        s = rest.to_string();
    }
    s = s
        .replace(" に失敗しました", "に失敗しました")
        .replace(
            "ブーリアンに失敗しました",
            "ボディの足し引き(結合・切り取り・交差)に失敗しました",
        )
        .replace("テッセレーション", "画面に出す網の計算");
    s = serde_words(&s);
    faces(&s, face)
}

/// 「(SMLib の状態 1001)」「(状態 1007)」を外す。
fn drop_status(s: &str) -> String {
    let mut out = s.to_string();
    for open in ["(SMLib の状態 ", "(状態 "] {
        while let Some(i) = out.find(open) {
            let tail = &out[i + open.len()..];
            let Some(j) = tail.find(')') else { break };
            if !tail[..j]
                .trim()
                .chars()
                .all(|c| c.is_ascii_digit() || c == '-')
            {
                break;
            }
            out = format!("{}{}", &out[..i], &tail[j + ')'.len_utf8()..]);
        }
    }
    out.trim_end().to_string()
}

/// データの読み込み(serde)の英語の文を日本語にする。
fn serde_words(s: &str) -> String {
    let quoted = |s: &str, key: &str| -> Option<(usize, usize, String)> {
        let i = s.find(key)?;
        let tail = &s[i + key.len()..];
        let j = tail.find('`')?;
        Some((i, i + key.len() + j + 1, tail[..j].to_string()))
    };
    if let Some((i, end, name)) = quoted(s, "missing field `") {
        return format!("{}設定の欄「{name}」が空です{}", &s[..i], &s[end..]);
    }
    if let Some((i, _, name)) = quoted(s, "unknown variant `") {
        return format!("{}選べない値「{name}」が入っています", &s[..i]);
    }
    if let Some(i) = s.find("invalid type: ") {
        return format!("{}欄の値の種類が違います", &s[..i]);
    }
    s.to_string()
}

/// 「面 f2:end」の内部の名前を、見せる名前(「上の面(押し出し1)」)にする。
fn faces(s: &str, face: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("面 ") {
        out += &rest[..i];
        let tail = &rest[i + "面 ".len()..];
        let end = tail
            .find(|c: char| c.is_whitespace() || "、。()「」".contains(c))
            .unwrap_or(tail.len());
        let token = &tail[..end];
        if is_face_name(token) {
            out += &face(token).unwrap_or_else(|| "ある面".into());
            rest = &tail[end..];
            // 「上の面(押し出し1) と 横の面(…)」の空白を詰める
            if let Some(r) = rest.strip_prefix(" と ") {
                out += "と";
                rest = r;
                // 「面 a と b の間」の b にも「面 」は付かない
                let end = rest
                    .find(|c: char| c.is_whitespace() || "、。()「」".contains(c))
                    .unwrap_or(rest.len());
                if is_face_name(&rest[..end]) {
                    out += &face(&rest[..end]).unwrap_or_else(|| "ある面".into());
                    rest = rest[end..].strip_prefix(' ').unwrap_or(&rest[end..]);
                }
            } else if let Some(r) = rest.strip_prefix(' ') {
                rest = r;
            }
        } else {
            out += "面 ";
            rest = tail;
        }
    }
    out += rest;
    out
}

/// 面の内部の名前か(`f8:end`、`f2:side:e15` のように、`f` と番号と `:` で始まる)。
fn is_face_name(t: &str) -> bool {
    let Some(r) = t.strip_prefix('f') else {
        return false;
    };
    let digits = r.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && r[digits..].starts_with(':')
}

/// 見出し。
pub(crate) fn headline(h: &Health) -> &'static str {
    match h {
        Health::Error(m) if m.contains("設定の欄「") => "設定の欄が空です",
        Health::Error(_) => "計算に失敗しました",
        Health::Blocked { .. } => "前の機能が計算できていません",
        Health::Warning(_) => "計算はできました。確かめてほしい点があります",
        _ => "",
    }
}

/// 直し方の例。直した後の文から選ぶ共通の例の後に、機能の種類が持つ例(`own`、
/// [`kmb_core::Descriptor::fixes`])を並べる。
pub(crate) fn fixes(own: &[kmb_core::Fix], h: &Health) -> Vec<String> {
    let m = match h {
        Health::Error(m) => m.as_str(),
        Health::Warning(_) => {
            return vec![
                "形や経路は作れています。意図どおりか確かめ、必要なら機能を編集して直す".into(),
            ];
        }
        Health::Blocked { .. } => {
            return vec![
                "「原因のフィーチャを選ぶ」で止めている機能へ移り、そちらを直す(直すとこの機能も計算し直す)".into(),
            ];
        }
        _ => return Vec::new(),
    };
    let mut v: Vec<&str> = Vec::new();
    if m.contains("設定の欄「") {
        v.push("機能を編集して、空の欄を埋める(選び直す)");
    }
    if m.contains("参照が切れ") || m.contains("見つかりません") || m.contains("ありません")
    {
        v.push("前の機能を変えた・抑制した・消した時は、面や辺を選び直す(編集パネルの「選んだ面に付け替える」も使える)");
    }
    if m.contains("足し引き") || m.contains("結果が空") {
        v.push("面がぴったり重なる・接するだけの所がないか見て、寸法を少し変えるか位置をずらす");
        v.push("対象のボディと演算(結合・切り取り・交差)が意図どおりか確かめる");
    }
    v.extend(
        own.iter()
            .filter(|f| f.when.is_empty() || m.contains(&f.when))
            .map(|f| f.text.as_str()),
    );
    if v.is_empty() {
        v.push("機能を編集して、値や選んだ面・辺を直す");
        v.push("前の機能を変えた・抑制した・消した時は、そちらも確かめる");
    }
    v.into_iter().map(String::from).collect()
}

impl Workspace {
    /// 機能の具合(知らせの文は読みやすく直したもの)。
    pub(crate) fn health(&self, id: FeatureId) -> Health {
        match Health::of(self.cur.work.report().feature(id)) {
            Health::Error(m) => Health::Error(self.field_labels(id, self.plain_text(&m))),
            Health::Warning(m) => Health::Warning(self.field_labels(id, self.plain_text(&m))),
            h => h,
        }
    }

    /// 「設定の欄「tool」」の内部の名前を、パネルに出す欄の名前(「ツールボディ」)にする。
    fn field_labels(&self, id: FeatureId, mut s: String) -> String {
        let desc = self
            .cur
            .work
            .doc()
            .data()
            .feature(id)
            .and_then(|f| self.win.registry.descriptor(&f.type_id));
        for f in desc.iter().flat_map(|d| &d.fields) {
            s = s.replace(&format!("欄「{}」", f.key), &format!("欄「{}」", f.label));
        }
        s
    }

    /// 知らせの文を読みやすく直す(面の名前はいま見えている形から引く)。
    pub(crate) fn plain_text(&self, raw: &str) -> String {
        plain(raw, &|f| {
            let l = self.face_label(f);
            // 形からも持ち主からも分からない面は、内部の名前を見せない
            (!l.contains(f)).then_some(l)
        })
    }

    /// 元の文(直していないもの。読みやすく直した文と同じなら `None`)。
    pub(crate) fn raw_text(&self, id: FeatureId) -> Option<String> {
        let raw = match self.cur.work.report().feature(id) {
            Some(FeatureStatus::Error { message }) => message.clone(),
            Some(FeatureStatus::Ok { outputs, .. }) => kmb_core::warnings_of(outputs).join("\n"),
            _ => return None,
        };
        (!raw.is_empty() && raw != self.plain_text(&raw)).then_some(raw)
    }

    /// 状態の印と、読みやすく直した説明(タイムライン・ブラウザ・パネルの説明に使う)。
    pub(crate) fn status_line(&self, id: FeatureId) -> (&'static str, String) {
        let st = self.cur.work.report().feature(id);
        let (mark, detail) = crate::timeline_ui::status_mark(st);
        match st {
            Some(FeatureStatus::Error { .. }) | Some(FeatureStatus::Ok { .. }) => {
                (mark, self.field_labels(id, self.plain_text(&detail)))
            }
            _ => (mark, detail),
        }
    }

    /// 注意・エラーのある機能を、タイムラインの順に(赤が先)。
    pub(crate) fn problems(&self) -> Vec<(FeatureId, Health)> {
        let mut v: Vec<_> = self
            .cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .map(|f| (f.id, Health::of(self.cur.work.report().feature(f.id))))
            .filter(|(_, h)| h.is_problem())
            .collect();
        v.sort_by_key(|(_, h)| !h.is_red());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn kernel_words_are_removed() {
        assert_eq!(
            plain(
                "形状カーネルの誤り: ブーリアン: ブーリアン に失敗しました(SMLib の状態 1001)",
                &none
            ),
            "ボディの足し引き(結合・切り取り・交差)に失敗しました"
        );
        assert_eq!(
            plain("データが不正です: missing field `a`", &none),
            "設定の欄「a」が空です"
        );
        assert_eq!(
            plain(
                "形状カーネルの誤り: 押し出し: 押し出し に失敗しました(状態 7)",
                &none
            ),
            "押し出しに失敗しました"
        );
    }

    #[test]
    fn face_names_become_readable() {
        let label = |f: &str| match f {
            "f2:end" => Some("上の面(押し出し1)".to_string()),
            "f2:side:e15" => Some("横の面(押し出し1)".to_string()),
            _ => None,
        };
        assert_eq!(
            plain(
                "データが不正です: 面 f2:end と f2:side:e15 の間の辺に、半径 30 mm は大きすぎます。",
                &label
            ),
            "上の面(押し出し1)と横の面(押し出し1)の間の辺に、半径 30 mm は大きすぎます。"
        );
        // 分からない面は内部の名前を見せない。面の名前でない「面 」は残す
        assert_eq!(
            plain("面 f9:x のまわりに丸められる辺がありません", &none),
            "ある面のまわりに丸められる辺がありません"
        );
        assert_eq!(plain("面 と面が重なる", &none), "面 と面が重なる");
    }

    #[test]
    fn fixes_follow_the_kind_and_the_cause() {
        // 種類が持つ例は、誤りの文が `when` を含む時だけ、共通の例の後に出る
        let own = kmb_core::Descriptor::new("x", "solid", "x")
            .fix("大きすぎ", "半径を小さくする")
            .fix("", "写す平面を確かめる")
            .fixes;
        let big = Health::Error("上の面の間の辺に、半径 30 mm は大きすぎます".into());
        assert_eq!(
            fixes(&own, &big),
            ["半径を小さくする", "写す平面を確かめる"]
        );
        let bool_fail =
            Health::Error("ボディの足し引き(結合・切り取り・交差)に失敗しました".into());
        let f = fixes(&own, &bool_fail);
        assert!(f[0].contains("重なる"), "{f:?}");
        assert_eq!(f.last().map(String::as_str), Some("写す平面を確かめる"));
        assert!(!f.iter().any(|s| s.contains("小さく")));
        assert_eq!(
            headline(&Health::Error("設定の欄「a」が空です".into())),
            "設定の欄が空です"
        );
        assert!(!fixes(&[], &Health::Error("何か".into())).is_empty());
    }
}
