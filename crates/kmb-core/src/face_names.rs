//! 面の名前を短く保つ。カーネルは割れた面の切れ端に位置の順の `#k` を足す([`crate::kernel::dedupe_names`])
//! ので、同じ面を割るたびに `a:end#1#0#1…` と伸びる。再計算は機能の結果のうち、2 回以上割った名前
//! (末尾が `#数#数`)を `元の名前~割った機能#k` にそろえる。名前は「元の名前 + 最後に割った機能 + 番号」で
//! 頭打ちになる。1 回だけ割った面(`元#k`)と割れていない面の名前は変えない。
//!
//! 前の版のアプリは名前をそろえなかったので、前の文書の参照は伸びた名前を持つ。再計算はそろえた
//! 名前ごとに前の決まりの名前を覚え([`FaceAliases`])、文書を開いた後に参照を書き換える
//! ([`crate::Document::rename_old_faces`])。

use std::collections::{BTreeMap, HashMap, HashSet};

/// 割った機能の印(`a:end~f12#0` の `~`)。前の決まりの名前には現れない。
pub const SPLIT_BY: char = '~';

/// 名前の末尾の `#数` を外した残りと、外した数。
fn strip_numbers(name: &str) -> (&str, usize) {
    let mut head = name;
    let mut n = 0;
    while let Some(at) = head.rfind('#') {
        let tail = &head[at + 1..];
        if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
            break;
        }
        head = &head[..at];
        n += 1;
    }
    (head, n)
}

/// 名前の文字(これ以外の文字の後ろから、別の名前が始まりうる。`f20:f2:end~f12#0|…` の `:`・`|`)。
fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// 末尾の `~機能` を外す。
fn strip_split(head: &str) -> &str {
    match head.rfind(SPLIT_BY) {
        Some(at) if head[at + 1..].bytes().all(word) && at + 1 < head.len() => &head[..at],
        _ => head,
    }
}

/// 並べる時の名前の欠片(数は数として比べ、`#10` を `#2` の後ろにする)。
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Piece<'a> {
    Num(u64),
    Text(&'a str),
}

fn order_key(tail: &str) -> Vec<Piece<'_>> {
    tail.split('#')
        .map(|p| p.parse().map_or(Piece::Text(p), Piece::Num))
        .collect()
}

/// 機能 `tag` の結果の面の名前 `names` のうち、そろえる物の (何番目、新しい名前)。2 回以上割った名前を
/// 元の名前ごとに集め、カーネルが付けた番号の順(位置の順)に `元~{tag}#k` と振る。この機能の印の付いた
/// 名前が既にあれば、それも一緒に振り直す(名前が重ならないように)。
pub fn shorten(names: &[String], tag: &str) -> Vec<(usize, String)> {
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, n) in names.iter().enumerate() {
        let (head, count) = strip_numbers(n);
        if count >= 2 {
            groups.entry(strip_split(head)).or_default().push(i);
        }
    }
    let mut out = Vec::new();
    for (base, mut idx) in groups {
        let mine = format!("{base}{SPLIT_BY}{tag}#");
        for (i, n) in names.iter().enumerate() {
            if !idx.contains(&i)
                && n.strip_prefix(&mine)
                    .is_some_and(|k| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()))
            {
                idx.push(i);
            }
        }
        idx.sort_by(|&a, &b| {
            order_key(&names[a][base.len()..]).cmp(&order_key(&names[b][base.len()..]))
        });
        for (k, i) in idx.into_iter().enumerate() {
            let new = format!("{mine}{k}");
            if names[i] != new {
                out.push((i, new));
            }
        }
    }
    out
}

/// そろえた名前と、前の決まり(そろえない)での名前の表。再計算が作る派生物。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceAliases {
    /// そろえた名前 → 前の決まりの名前
    old: HashMap<String, String>,
    /// 2 つのボディで、同じそろえた名前が違う前の名前を持った物(書き換えに使わない)
    clash: HashSet<String>,
}

impl FaceAliases {
    pub fn is_empty(&self) -> bool {
        self.old.is_empty()
    }

    pub(crate) fn extend(&mut self, pairs: &[(String, String)]) {
        for (new, old) in pairs {
            self.insert(new.clone(), old.clone());
        }
    }

    pub(crate) fn insert(&mut self, new: String, old: String) {
        match self.old.get(&new) {
            Some(o) if *o != old => {
                self.clash.insert(new);
            }
            Some(_) => {}
            None => {
                self.old.insert(new, old);
            }
        }
    }

    /// 名前 `name` の前の決まりでの名前。名前の中のそろえた名前(丸めの面の `f20:{面}|{面}` の中など)を
    /// 前の名前に戻す。
    pub fn legacy(&self, name: &str) -> String {
        if self.old.is_empty() || !name.contains(SPLIT_BY) {
            return name.to_string();
        }
        let b = name.as_bytes();
        let mut out = String::new();
        let mut done = 0;
        let mut at = 0;
        while let Some(p) = name[at..].find(SPLIT_BY).map(|p| p + at) {
            at = p + 1;
            // `~機能#数` の終わり
            let mut e = p + 1;
            while e < b.len() && word(b[e]) {
                e += 1;
            }
            if e >= b.len() || b[e] != b'#' {
                continue;
            }
            let digits = e + 1;
            e = digits;
            while e < b.len() && b[e].is_ascii_digit() {
                e += 1;
            }
            if e == digits {
                continue;
            }
            // 一番長い名前(前から)。始まりは名前の頭か、名前の文字でない文字の後ろ
            let start = (done..p)
                .filter(|&s| name.is_char_boundary(s) && (s == 0 || !word(b[s - 1])))
                .find(|&s| self.old.contains_key(&name[s..e]));
            if let Some(s) = start {
                out.push_str(&name[done..s]);
                out.push_str(&self.old[&name[s..e]]);
                done = e;
                at = e;
            }
        }
        out.push_str(&name[done..]);
        out
    }

    /// 前の決まりの名前 → そろえた名前の表(1 つに決まらない物は除く)。
    pub fn current(&self) -> HashMap<&str, &str> {
        let mut seen: HashMap<&str, Option<&str>> = HashMap::new();
        for (new, old) in &self.old {
            if self.clash.contains(new) {
                seen.insert(old, None);
                continue;
            }
            seen.entry(old)
                .and_modify(|v| {
                    if *v != Some(new.as_str()) {
                        *v = None;
                    }
                })
                .or_insert(Some(new));
        }
        seen.into_iter()
            .filter_map(|(o, n)| n.map(|n| (o, n)))
            .collect()
    }
}

/// 前の決まりの名前 `name` を、表 `current`([`FaceAliases::current`])でそろえた名前に直す。名前の中の
/// 前の名前(一番長い物)を置き換える。直す所が無ければ `None`。
pub fn rename_old(name: &str, current: &HashMap<&str, &str>) -> Option<String> {
    if current.is_empty() || !name.contains('#') {
        return None;
    }
    let b = name.as_bytes();
    let mut out = String::new();
    let mut done = 0;
    let mut s = 0;
    while s < b.len() {
        if !name.is_char_boundary(s) || (s > 0 && word(b[s - 1])) {
            s += 1;
            continue;
        }
        let end = (s + 1..=b.len())
            .rev()
            .filter(|&e| name.is_char_boundary(e) && (e == b.len() || !b[e].is_ascii_digit()))
            .find(|&e| current.contains_key(&name[s..e]));
        match end {
            Some(e) => {
                out.push_str(&name[done..s]);
                out.push_str(current[&name[s..e]]);
                done = e;
                s = e;
            }
            None => s += 1,
        }
    }
    (done > 0).then(|| {
        out.push_str(&name[done..]);
        out
    })
}

#[cfg(test)]
#[path = "face_names_tests.rs"]
mod tests;
