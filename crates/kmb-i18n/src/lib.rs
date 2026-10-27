//! 画面の文言の訳。文言は ID(小文字とハイフン。頭は分野)で引き、言語ごとの Fluent のファイル
//! (`locales/<言語>/<分野>.ftl`)から文を出す。ファイルは組み込むので、ネットも置き場所も要らない。
//!
//! ID の決まり(`locales/README.md` が訳す人向けの正本):
//! - 置き場ごとに別の ID にする(同じ字でもボタンと見出しは訳し分けることがある)。共通の ID
//!   (`common-`)は OK・キャンセルのように働きまで同じ物だけ。
//! - 意味が変わったら ID を替える。字の誤りの直しだけなら替えない(替えると全言語で未訳に戻る)。
//! - 文を切って継ぎ合わせない。数や名前は `{ $name }` の差し込み口で渡す。
//!
//! 足りない訳は、選んだ言語 → 英語 → 日本語 → ID の順に探す(未訳の言語は英語で出る)。
//! 日本語と英語は全 ID を持つ(試験が確かめる)。

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use std::borrow::Cow;

/// 画面の言語。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Ja,
    En,
    ZhHans,
    ZhHant,
    Ko,
}

impl Lang {
    pub const ALL: [Lang; 5] = [Lang::Ja, Lang::En, Lang::ZhHans, Lang::ZhHant, Lang::Ko];

    /// 言語の符号(BCP 47。ファイルのフォルダの名前と設定に書く値)。
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ja => "ja",
            Lang::En => "en",
            Lang::ZhHans => "zh-Hans",
            Lang::ZhHant => "zh-Hant",
            Lang::Ko => "ko",
        }
    }

    /// その言語での言語の名前(言語の一覧に出す。今の言語に訳さない)。
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Ja => "日本語",
            Lang::En => "English",
            Lang::ZhHans => "简体中文",
            Lang::ZhHant => "繁體中文",
            Lang::Ko => "한국어",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// OS の言語の名前(`ja-JP`・`zh-TW`・`zh-Hant-HK` など)から。日本語・中国語・韓国語でなければ英語。
    pub fn from_locale(locale: &str) -> Lang {
        let parts: Vec<String> = locale
            .split(['-', '_', '.'])
            .map(|p| p.to_ascii_lowercase())
            .collect();
        match parts.first().map(String::as_str) {
            Some("ja") => Lang::Ja,
            Some("ko") => Lang::Ko,
            Some("zh") => {
                let traditional = parts[1..]
                    .iter()
                    .any(|p| matches!(p.as_str(), "hant" | "tw" | "hk" | "mo"));
                if traditional {
                    Lang::ZhHant
                } else {
                    Lang::ZhHans
                }
            }
            _ => Lang::En,
        }
    }
}

/// 設定で選ぶ言語。「自動」は OS の言語に合わせる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Choice {
    #[default]
    Auto,
    Fixed(Lang),
}

impl Choice {
    /// 設定に書く値(`auto` か言語の符号)。
    pub fn code(self) -> &'static str {
        match self {
            Choice::Auto => "auto",
            Choice::Fixed(l) => l.code(),
        }
    }

    /// 設定の値から。知らない値は「自動」(設定のせいで起動を止めない)。
    pub fn parse(code: &str) -> Choice {
        match Lang::from_code(code.trim()) {
            Some(l) => Choice::Fixed(l),
            None => {
                if !code.trim().is_empty() && code.trim() != "auto" {
                    log::warn!("知らない言語の設定「{code}」。自動にする");
                }
                Choice::Auto
            }
        }
    }

    /// 一覧に出す順(自動が先頭)。
    pub fn all() -> impl Iterator<Item = Choice> {
        std::iter::once(Choice::Auto).chain(Lang::ALL.into_iter().map(Choice::Fixed))
    }
}

/// 組み込みのファイル(言語・分野・中身)。分野を足したら 5 言語ぶん並べる(試験が、フォルダの
/// ファイルがみなここにあることを確かめる)。
macro_rules! files {
    ($($area:literal),*) => {
        &[$(
            (Lang::Ja, $area, include_str!(concat!("../locales/ja/", $area, ".ftl"))),
            (Lang::En, $area, include_str!(concat!("../locales/en/", $area, ".ftl"))),
            (Lang::ZhHans, $area, include_str!(concat!("../locales/zh-Hans/", $area, ".ftl"))),
            (Lang::ZhHant, $area, include_str!(concat!("../locales/zh-Hant/", $area, ".ftl"))),
            (Lang::Ko, $area, include_str!(concat!("../locales/ko/", $area, ".ftl"))),
        )*]
    };
}

pub const FILES: &[(Lang, &str, &str)] = files!(
    "common", "settings", "frame", "menu", "toolbar", "action", "feature", "browser", "timeline",
    "view", "review"
);

/// 全言語の文言。作るのは起動の時に 1 回(画面は窓の状態に持つ)。
pub struct Catalog {
    bundles: Vec<(Lang, FluentBundle<FluentResource>)>,
    os: Lang,
}

impl Catalog {
    /// 組み込みのファイルと OS の言語から。
    pub fn new() -> Catalog {
        let os = sys_locale::get_locale()
            .map(|l| Lang::from_locale(&l))
            .unwrap_or(Lang::En);
        Catalog::with_os(os)
    }

    /// OS の言語を決めて作る(試験用)。
    pub fn with_os(os: Lang) -> Catalog {
        let bundles = Lang::ALL
            .into_iter()
            .map(|lang| {
                let id = lang.code().parse().expect("言語の符号は BCP 47");
                let mut b = FluentBundle::new_concurrent(vec![id]);
                // 差し込んだ値を方向の印(U+2068・U+2069)で囲まない(画面の字体に無く □ になる)
                b.set_use_isolating(false);
                for (_, area, src) in FILES.iter().filter(|(l, _, _)| *l == lang) {
                    let res = match FluentResource::try_new(src.to_string()) {
                        Ok(r) => r,
                        Err((r, errs)) => {
                            log::error!("{}/{area}.ftl を読めない所がある: {errs:?}", lang.code());
                            r
                        }
                    };
                    if let Err(errs) = b.add_resource(res) {
                        log::error!("{}/{area}.ftl の ID が重なっている: {errs:?}", lang.code());
                    }
                }
                (lang, b)
            })
            .collect();
        log::info!("画面の文言を読んだ(OS の言語 {})", os.code());
        Catalog { bundles, os }
    }

    /// OS の言語(「自動」の時の言語)。
    pub fn os_lang(&self) -> Lang {
        self.os
    }

    /// 選んだ言語の、実際に使う言語。
    pub fn resolve(&self, c: Choice) -> Lang {
        match c {
            Choice::Auto => self.os,
            Choice::Fixed(l) => l,
        }
    }

    fn bundle(&self, lang: Lang) -> &FluentBundle<FluentResource> {
        &self
            .bundles
            .iter()
            .find(|(l, _)| *l == lang)
            .expect("全言語を持つ")
            .1
    }

    /// その言語が ID の訳を持つか(探す順をたどらない)。
    pub fn has(&self, lang: Lang, id: &str) -> bool {
        self.bundle(lang)
            .get_message(id)
            .is_some_and(|m| m.value().is_some())
    }

    /// 文言。
    pub fn text(&self, lang: Lang, id: &str) -> String {
        self.format(lang, id, None)
    }

    /// 差し込み口のある文言(`args` は名前と値)。
    pub fn text_with(&self, lang: Lang, id: &str, args: &[(&str, Arg)]) -> String {
        let mut a = FluentArgs::new();
        for (k, v) in args {
            match v {
                Arg::Text(s) => a.set(*k, FluentValue::from(s.as_str())),
                Arg::Number(n) => a.set(*k, FluentValue::from(*n)),
            }
        }
        self.format(lang, id, Some(&a))
    }

    /// 文言があれば、その文(探す順は [`Catalog::text`] と同じ)。無ければ `None`(モジュールが付けた
    /// 名前のように、訳の無い時の字を呼ぶ側が持つ時)。
    pub fn find(&self, lang: Lang, id: &str) -> Option<String> {
        self.lookup(lang, id, None)
    }

    fn format(&self, lang: Lang, id: &str, args: Option<&FluentArgs>) -> String {
        self.lookup(lang, id, args).unwrap_or_else(|| {
            log::warn!("文言 {id} がどの言語にも無い");
            id.to_string()
        })
    }

    fn lookup(&self, lang: Lang, id: &str, args: Option<&FluentArgs>) -> Option<String> {
        for l in [lang, Lang::En, Lang::Ja] {
            let b = self.bundle(l);
            if let Some(p) = b.get_message(id).and_then(|m| m.value()) {
                let mut errs = vec![];
                let s: Cow<str> = b.format_pattern(p, args, &mut errs);
                if !errs.is_empty() {
                    log::warn!("文言 {id}({})の差し込みが足りない: {errs:?}", l.code());
                }
                return Some(s.into_owned());
            }
        }
        None
    }
}

impl Default for Catalog {
    fn default() -> Self {
        Catalog::new()
    }
}

/// 差し込み口に渡す値。
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Text(String),
    Number(f64),
}

impl From<&str> for Arg {
    fn from(s: &str) -> Arg {
        Arg::Text(s.to_string())
    }
}

impl From<String> for Arg {
    fn from(s: String) -> Arg {
        Arg::Text(s)
    }
}

impl From<f64> for Arg {
    fn from(n: f64) -> Arg {
        Arg::Number(n)
    }
}

impl From<usize> for Arg {
    fn from(n: usize) -> Arg {
        Arg::Number(n as f64)
    }
}

/// 操作(モジュールの `Action`)の名前の文言の ID。操作の ID(`cad.add.extrude`)の `.` と `_` を
/// `-` にして頭に `action-` を付ける。説明はその後ろに `-tip`。
pub fn action_id(action: &str) -> String {
    format!(
        "action-{}",
        action.replace(['.', '_'], "-").to_ascii_lowercase()
    )
}

/// 機能の種類(`solid.hole@1`)の名前の文言の ID。版(`@1`)を外し、`.` と `_` を `-` にして頭に
/// `feature-` を付ける。説明はその後ろに `-tip`。
pub fn feature_id(type_id: &str) -> String {
    let kind = type_id.split('@').next().unwrap_or(type_id);
    format!(
        "feature-{}",
        kind.replace(['.', '_'], "-").to_ascii_lowercase()
    )
}

/// Fluent のファイルの中の文言の ID(行頭の `名前 =`。`-` で始まる用語は数えない)。
pub fn ids(src: &str) -> Vec<&str> {
    src.lines()
        .filter_map(|l| {
            let (name, _) = l.split_once('=')?;
            let name = name.trim_end();
            let ok = name.starts_with(|c: char| c.is_ascii_alphabetic())
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            ok.then_some(name)
        })
        .collect()
}

#[cfg(test)]
mod tests;
