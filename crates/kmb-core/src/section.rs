//! 文書のモジュールごとの区画。
//!
//! モーション スタディや接触セットのように、機能のタイムラインに乗らない文書の中身は、それを扱う
//! モジュールが区画として名乗る。コアは区画の中身を知らず、鍵(例 `asm.motion`)・版・JSON の値だけを
//! 持つ。区画の変更は汎用の [`crate::Op::SetSection`] 1 つで、取り消しは前の値に戻す逆の変更。
//!
//! - 知らない区画(新しいアプリやほかのモジュールが書いた物)は、読んだまま書き戻す。
//! - 区画の版はモジュールが持つ。古い版は読む時に [`SectionType::migrate`] で今の形にし、新しい版は
//!   読まない(書き換えもしない。知らない項目を落として消さないため)。
//! - 区画の型は根に `#[serde(flatten)]` の残りの欄を持ち、区画の中の知らない項目も書き戻す。

use crate::DocumentData;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// 文書に入っている区画 1 つ(モジュールの版と中身)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// 書いたモジュールの区画の版([`SectionType::VERSION`])
    pub version: u32,
    pub data: serde_json::Value,
}

/// モジュールが名乗る区画の型。
pub trait SectionType: Serialize + DeserializeOwned + Default + Clone {
    /// 文書の中の鍵(`asm.motion` のように、名乗るモジュールの頭を付ける)
    const KEY: &'static str;
    /// 今の版(1 から。形を変えたら上げ、[`SectionType::migrate`] に 1 版ずつの移し方を足す)
    const VERSION: u32;
    /// 画面と誤りに出す名前(例: 「モーション スタディ」)
    const LABEL: &'static str;

    /// 版 `from` の中身を版 `from + 1` の形に直す。
    fn migrate(_from: u32, _data: &mut serde_json::Value) {}

    /// 中身の決まり(名前の重なりなど)。区画を書く前と読んだ後に確かめる。
    fn check(&self) -> Result<(), String> {
        Ok(())
    }

    /// 何も決めていない(区画を文書から外す)か。
    fn is_empty(&self) -> bool;
}

/// 区画を読めない理由。
#[derive(Debug, Clone, PartialEq)]
pub enum SectionError {
    /// 新しいアプリで書かれた版(このアプリは `known` まで)
    Newer {
        key: String,
        version: u32,
        known: u32,
    },
    /// 中身が型に合わないか、決まりに外れる
    Invalid { key: String, why: String },
}

impl std::fmt::Display for SectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SectionError::Newer {
                key,
                version,
                known,
            } => write!(
                f,
                "文書の区画 {key} は新しいアプリで保存されています(版 {version}、このアプリは {known} まで)。アプリを更新してください"
            ),
            SectionError::Invalid { key, why } => write!(f, "文書の区画 {key} を読めません: {why}"),
        }
    }
}

impl DocumentData {
    /// モジュールの区画 `T` の中身。区画が無ければ空の値。
    pub fn section<T: SectionType>(&self) -> Result<T, SectionError> {
        let Some(s) = self.sections.get(T::KEY) else {
            return Ok(T::default());
        };
        if s.version > T::VERSION {
            return Err(SectionError::Newer {
                key: T::KEY.into(),
                version: s.version,
                known: T::VERSION,
            });
        }
        let mut data = s.data.clone();
        for v in s.version.max(1)..T::VERSION {
            T::migrate(v, &mut data);
        }
        let invalid = |why: String| SectionError::Invalid {
            key: T::KEY.into(),
            why,
        };
        let value: T = serde_json::from_value(data).map_err(|e| invalid(e.to_string()))?;
        value.check().map_err(invalid)?;
        Ok(value)
    }

    /// 区画 `T` を `value` に置き換える変更(操作の結果に入れる)。決まりに外れる値と、今の区画を
    /// 読めない時(新しい版か、中身が型や決まりに合わない)の上書きは誤りにする(読めない区画を
    /// 空として読んだ値で書くと、中身が黙って消えるため)。空の値なら区画を外す。
    pub fn set_section<T: SectionType>(&self, value: &T) -> Result<crate::Edit, String> {
        let crate::Op::SetSection { key, section } = self.section_op(value)? else {
            unreachable!("区画の変更");
        };
        Ok(crate::Edit::SetSection { key, section })
    }

    /// [`DocumentData::set_section`] と同じ変更を、取り消しの単位([`crate::Transaction`])に入れる形で。
    pub fn section_op<T: SectionType>(&self, value: &T) -> Result<crate::Op, String> {
        if self.sections.contains_key(T::KEY) {
            self.section::<T>().map_err(|e| e.to_string())?;
        }
        value
            .check()
            .map_err(|why| format!("{}にできません: {why}", T::LABEL))?;
        let section = if value.is_empty() {
            None
        } else {
            Some(Section {
                version: T::VERSION,
                data: serde_json::to_value(value).map_err(|e| e.to_string())?,
            })
        };
        Ok(crate::Op::SetSection {
            key: T::KEY.into(),
            section,
        })
    }
}

/// 区画の鍵の決まり: 空でなく、英小文字・数字・`.`・`_` だけ。
pub(crate) fn check_key(key: &str) -> Result<(), String> {
    if key.is_empty()
        || !key
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.' || c == b'_')
    {
        return Err(format!("区画の鍵 {key:?} は英小文字・数字・. と _ で書く"));
    }
    Ok(())
}
