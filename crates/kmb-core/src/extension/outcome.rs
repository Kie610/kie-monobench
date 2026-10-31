//! 操作の結果(文書への変更・表・書き出しなど)。

use super::*;

/// 文書への変更の 1 つ。足す機能の ID は、適用する側が割り当てる。
/// 同じ変更の中で先に足す機能を指すには、データに `{"$new": k}`(k 番目に足す機能)と書く。
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// ロールバック位置(なければ末尾)に機能を足す
    Insert {
        type_id: String,
        name: String,
        data: serde_json::Value,
    },
    SetData {
        id: FeatureId,
        data: serde_json::Value,
    },
    /// 機能を抑制する・抑制を外す(文字を線に分解した後の元の文字など)
    SetSuppressed { id: FeatureId, suppressed: bool },
    /// 読むボディをデータの番号の形のまま読むかを変える([`crate::FeatureEntry::pin_bodies`])
    SetPinBodies { id: FeatureId, pin: bool },
    /// パラメータの式を変える(構成表の行を当てるなど)
    SetParamExpr { id: crate::ParamId, expr: String },
    /// 部品の木を丸ごと置き換える(部品を作る・ボディを移すなど)。番号が 0 の部品には、適用する側が
    /// 新しい番号を割り当てる(親を 0 で指すことはできない)
    SetComponents { components: Vec<crate::Component> },
    /// この変更で足した機能のボディを部品に入れ、名前を付ける。`bodies` は(何番目に足した機能か、
    /// ボディの名前)。`component` が `None` なら、新しい部品 `name` を `parent` の中に作って入れる
    /// (リンクで組み込んだ部品のように、足したばかりで番号のまだ無いボディをまとめるため)
    AddBodies {
        component: Option<u64>,
        name: String,
        parent: Option<u64>,
        bodies: Vec<(usize, String)>,
    },
    /// この変更で足した何番目の機能かを指定し、部品に入れずにボディの名前を付ける。
    NameBodies { bodies: Vec<(usize, String)> },
    /// モジュールの区画を置き換える(`None` なら外す)。モジュールは
    /// [`DocumentData::set_section`](crate::DocumentData::set_section) で作る
    SetSection {
        key: String,
        section: Option<crate::section::Section>,
    },
    /// ロールバック位置をタイムラインの `rollback` 番目の前にする(`None` なら最後まで進める)。
    /// 機能の数より後ろの位置は、適用の検査で断られる
    SetRollback { rollback: Option<usize> },
}

/// 操作の結果。
pub enum Outcome {
    /// 文書を変える(1 回の取り消し単位)。最後に足した機能を選ぶ
    Change {
        label: String,
        edits: Vec<Edit>,
    },
    /// ファイルを書き出す。保存先を選ばせた操作([`Input::SaveFile`])なら選んだファイルへ、
    /// それ以外は、画面は文書の隣に `<文書名>-<suffix>` で置き、CLI・MCP は指定の場所に書く。
    /// `info` は CLI・MCP 向けの要約(行数、時間など。なければ `Null`)
    File {
        suffix: String,
        contents: Vec<u8>,
        info: serde_json::Value,
    },
    /// いくつものファイルを書き出す(ボディごとの書き出しなど)。`files` は名前の後ろ(例: `ボディ1.stl`)と
    /// 中身。保存先を選ばせた操作なら選んだファイルの名前に `-<名前の後ろ>` を付けた所へ、それ以外は
    /// 文書の隣に `<文書名>-<名前の後ろ>` で置く。CLI・MCP は指定の場所に同じ付け方で書く
    Files {
        files: Vec<(String, Vec<u8>)>,
        info: serde_json::Value,
    },
    /// 知らせるだけ
    Message(String),
    /// 読むための文(G-code の確認など)。画面は窓に出し、CLI・MCP は文を返す
    Text {
        title: String,
        body: String,
    },
    /// 表(部品表など)。画面は列を選べる表の窓に出し、CLI・MCP は既定の列の文([`Table::text`])を返す
    Table {
        title: String,
        table: Table,
    },
    Job(Job),
}

/// [`Outcome::Table`] の中身。値は出す形の文字にしておく(桁と単位は作る側が決める)。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Table {
    /// 表を作った操作の id。列の選び方を覚える鍵にもする。画面は、窓を開いている間に文書が変わると
    /// この操作で作り直す
    pub key: String,
    /// 表を書き出す操作の id(あれば窓に「書き出す…」を出す)
    pub export: Option<String>,
    pub columns: Vec<TableColumn>,
    /// 行ごとの値(`columns` と同じ並び)
    pub rows: Vec<Vec<String>>,
    /// 表の下に出す注記(空なら出さない)
    pub note: String,
    /// 行が無い時に出す文(空なら「行がありません」)
    pub empty: String,
    /// 表の上に出す切り替え。画面は、切り替えると表を作った操作を、全部の切り替えの値を
    /// 引数(`{id: 真偽}`)にして作り直す。CLI・MCP は同じ引数で渡す
    pub options: Vec<TableOption>,
    /// 行ごとに 3D のビューで色を付けて見せる立体(`rows` と同じ並び。空なら見せない)
    #[serde(skip)]
    pub marks: Vec<TableMark>,
    /// `marks` を全部見せるかの切り替えの名前(例:「すべての干渉を表示」)。切ると選んだ行だけ見せる
    pub marks_label: String,
}

/// 表の切り替え 1 つ(例: 干渉の「一致した面を含める」)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TableOption {
    /// 引数の名前
    pub id: String,
    pub label: String,
    /// 指した時の説明
    pub hint: String,
    /// 今の値(この表を作った時の値)
    pub on: bool,
}

/// 表の 1 行を 3D のビューで見せる立体(干渉の重なった所など)。座標は mm。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableMark {
    /// 三角形(外から見て左回り)
    pub tris: Vec<[[f32; 3]; 3]>,
    /// 稜線
    pub edges: Vec<[[f32; 3]; 2]>,
}

/// 表の 1 列。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TableColumn {
    /// 列の鍵(選んだ列を覚えるのに使う。見出しを変えても変えない)
    pub id: String,
    /// 見出し(単位を含む)
    pub label: String,
    /// 数の列(右に寄せる)
    pub numeric: bool,
    /// 何も選んでいない時に出すか
    pub shown: bool,
}

impl TableColumn {
    pub fn new(id: &str, label: &str, numeric: bool, shown: bool) -> Self {
        TableColumn {
            id: id.into(),
            label: label.into(),
            numeric,
            shown,
        }
    }
}

impl Table {
    /// 既定で出す列だけを、列をそろえた文にする(CLI・MCP 向け)。空の値は「-」。
    pub fn text(&self) -> String {
        let cols: Vec<usize> = (0..self.columns.len())
            .filter(|&i| self.columns[i].shown)
            .collect();
        let cell = |r: &[String], i: usize| -> String {
            match r.get(i).map(String::as_str) {
                None | Some("") => "-".into(),
                Some(v) => v.into(),
            }
        };
        // 全角の字は 2 桁に数える(等幅の字で見て列がそろうように)
        let width = |s: &str| {
            s.chars()
                .map(|c| if c.is_ascii() { 1 } else { 2 })
                .sum::<usize>()
        };
        let w: Vec<usize> = cols
            .iter()
            .map(|&i| {
                self.rows
                    .iter()
                    .map(|r| width(&cell(r, i)))
                    .chain([width(&self.columns[i].label)])
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let line = |vals: Vec<String>| -> String {
            let mut t = String::new();
            for (k, (&i, v)) in cols.iter().zip(&vals).enumerate() {
                let pad = " ".repeat(w[k] - width(v));
                if k > 0 {
                    t += "  ";
                }
                if self.columns[i].numeric {
                    t += &pad;
                    t += v;
                } else {
                    t += v;
                    t += &pad;
                }
            }
            t.trim_end().to_string() + "\n"
        };
        let mut t = line(
            cols.iter()
                .map(|&i| self.columns[i].label.clone())
                .collect(),
        );
        for r in &self.rows {
            t += &line(cols.iter().map(|&i| cell(r, i)).collect());
        }
        if self.rows.is_empty() && !self.empty.is_empty() {
            t += &self.empty;
            t += "\n";
        }
        if !self.note.is_empty() {
            t += "\n";
            t += &self.note;
        }
        t
    }
}

/// [`Outcome::Files`] の 1 つを書く場所: `base`(選んだファイル)の名前に `-<tail>` を付ける
/// (`部品.stl` と `ボディ1.stl` なら `部品-ボディ1.stl`)。ファイルの名前に使えない文字は `_` にする。
pub fn sibling_path(base: &std::path::Path, tail: &str) -> std::path::PathBuf {
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tail: String = tail
        .chars()
        .map(|c| {
            if "\\/:*?\"<>|".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = if stem.is_empty() {
        tail
    } else {
        format!("{stem}-{tail}")
    };
    base.with_file_name(name)
}

impl Outcome {
    /// ログに書く短い要約(中身の大きいデータは書かない)。
    pub fn brief(&self) -> String {
        match self {
            Outcome::Change { label, edits } => format!("変更「{label}」(編集 {} 件)", edits.len()),
            Outcome::File {
                suffix, contents, ..
            } => format!("ファイル {suffix} ({} バイト)", contents.len()),
            Outcome::Files { files, .. } => format!(
                "ファイル {} 個 ({} バイト)",
                files.len(),
                files.iter().map(|f| f.1.len()).sum::<usize>()
            ),
            Outcome::Message(m) => format!("知らせ「{m}」"),
            Outcome::Text { title, body } => {
                format!("文「{title}」({} 文字)", body.chars().count())
            }
            Outcome::Table { title, table } => {
                format!("表「{title}」({} 行)", table.rows.len())
            }
            Outcome::Job(_) => "仕事".into(),
        }
    }
}

impl std::fmt::Debug for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Change { label, edits } => f
                .debug_struct("Change")
                .field("label", label)
                .field("edits", edits)
                .finish(),
            Outcome::File {
                suffix, contents, ..
            } => f
                .debug_struct("File")
                .field("suffix", suffix)
                .field("len", &contents.len())
                .finish(),
            Outcome::Files { files, .. } => f
                .debug_struct("Files")
                .field("names", &files.iter().map(|x| &x.0).collect::<Vec<_>>())
                .finish(),
            Outcome::Message(m) => f.debug_tuple("Message").field(m).finish(),
            Outcome::Text { title, body } => f
                .debug_struct("Text")
                .field("title", title)
                .field("len", &body.len())
                .finish(),
            Outcome::Table { title, table } => f
                .debug_struct("Table")
                .field("title", title)
                .field("rows", &table.rows.len())
                .finish(),
            Outcome::Job(_) => f.write_str("Job"),
        }
    }
}

impl Outcome {
    /// 機能を 1 つ足す変更。
    pub fn insert(label: &str, type_id: &str, name: String, data: serde_json::Value) -> Self {
        Outcome::Change {
            label: label.into(),
            edits: vec![Edit::Insert {
                type_id: type_id.into(),
                name,
                data,
            }],
        }
    }
}
