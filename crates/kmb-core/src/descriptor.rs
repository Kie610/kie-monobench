//! 機能の種類の説明(記述子)。画面、CLI、MCP は、機能の中身を知らずに
//! これだけを見て、追加のボタンや設定欄を作る。
//!
//! 記述子は機能のモジュールが登録の時に一緒に渡す。説明のない種類は
//! [`category::OTHER`] になり、画面では JSON で編集する。

use serde::{Deserialize, Serialize};

/// 機能の区分。文字列なので、プラグインも自分で名乗れる。
pub mod category {
    /// スケッチ(輪郭や点を出す)
    pub const SKETCH: &str = "cad.sketch";
    /// ボディを作る・変える
    pub const SOLID: &str = "cad.solid";
    /// 工具ライブラリ
    pub const TOOLS: &str = "cam.tools";
    /// セットアップ
    pub const SETUP: &str = "cam.setup";
    /// 加工(経路を出す)
    pub const OPERATION: &str = "cam.operation";
    /// 解析の研究(強度・熱・固有振動など。形を読んで結果を出す)
    pub const STUDY: &str = "fea.study";
    /// 動作確認用
    pub const DEBUG: &str = "debug";
    /// 説明のない種類
    pub const OTHER: &str = "other";
}

/// 選択肢の 1 つ。`value` は保存する値、`label` は画面に出す名前。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

/// 候補の一覧の出力の型の名前([`Suggest::Output`] が読む)。
pub const CHOICES_TYPE: &str = "core.choices@1";

/// 候補の一覧を出力の値にする。
pub fn choices_value(list: Vec<Choice>) -> crate::Value {
    crate::Value::Custom {
        type_key: CHOICES_TYPE.into(),
        data: serde_json::to_value(list).expect("JSON にできる"),
    }
}

/// 出力の値から候補の一覧を読む(型が違えば `None`)。
pub fn choices_of(v: &crate::Value) -> Option<Vec<Choice>> {
    match v {
        crate::Value::Custom { type_key, data } if type_key == CHOICES_TYPE => {
            serde_json::from_value(data.clone()).ok()
        }
        _ => None,
    }
}

/// 原点の平面を指す選択肢の値(XY・YZ・ZX の順)。
const ORIGIN_PLANE_VALUES: [&str; 3] = ["origin_xy", "origin_yz", "origin_zx"];

/// 設定欄の種類。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    /// 式の文字列(例: `"width / 2"`)。`optional` なら空にすると項目を消す
    Expr {
        optional: bool,
    },
    /// 数。`integer` なら整数。`optional` なら空にすると項目を消す
    Number {
        integer: bool,
        optional: bool,
    },
    Bool,
    /// 自由な文字列(例: 材料の名前)。空にすると項目を消す
    Text,
    /// ファイルのパス(例: ポストのプラグイン)。AI(MCP)から来た値は、許したフォルダの中に限る
    File,
    /// 決まった値から選ぶ
    Choice {
        choices: Vec<Choice>,
    },
    /// 前にある機能の ID。その機能が出力 `slot` を持つものだけを選べる
    Output {
        slot: String,
        optional: bool,
    },
    /// 3 つの数の組(向きや点。例: `[1, 0, 0]`)
    Vector,
    /// 3D のビューで選ぶ物(面・辺・頂点・ボディ)。`accept` の種類だけを受け、`min` 個から
    /// `max` 個(`None` ならいくつでも)まで。値は 1 つなら [`crate::Ref`] そのもの、2 つ以上なら並び
    /// (ボディは機能の番号で書くので、前のボディの欄 `"target": 2` と同じ形)
    Pick {
        accept: Vec<crate::RefKind>,
        min: usize,
        max: Option<usize>,
    },
    /// 表示するだけの文字列
    Info,
    /// 行の並び(1 つの機能に違う値の組をいくつも持つ)。
    /// データは `key` の並びで、行ごとに `fields`(行の中の `key`)を持つ。行の欄は
    /// `key.行番号.欄` の欄として扱う([`Descriptor::field_at`])。行の最初の選ぶ欄が空になった行は消す
    /// (1 行目だけは残す)
    Rows {
        fields: Vec<Field>,
    },
}

/// 設定欄の 1 つ。`key` はデータの中の場所で、`.` で区切って入れ子をたどる(例: `stock.margin_top`)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Field {
    pub key: String,
    pub label: String,
    #[serde(flatten)]
    pub kind: FieldKind,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub help: String,
    /// 他の項目の値によって使う・使わないが変わる欄
    /// (例: 押し出しの対象は、演算が「新規ボディ」でない時だけ)。
    /// 使わない間は値を消し、使い始めた時に空なら画面は直前の候補を入れる
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    /// 一覧から選べる候補(打つこともできる)。数や文字の欄に付ける
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggest: Option<Suggest>,
    /// 画面で、この欄を値 `value` にした時に続けて始める操作(選んだ機能を直している途中のまま。
    /// 例: 押し出しの範囲を「対象まで」にしたら、対象の面をクリックする手順に入る)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pick: Option<PickOn>,
    /// 画面でこの欄を手で替えたら、データのこの場所(`.` で入れ子)に `true` を入れる
    /// (例: 押し出しの演算を手で選んだ印。印があれば、機能はもう演算を自動で替えない)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marks: Option<String>,
    /// 選ぶ欄の値のうち、画面の一覧に出さないもの(前の版で作った文書だけが持つ値。読む・計算する
    /// のは今までどおりで、その値の文書では今の値として名前を出す)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<String>,
    /// 式の欄の横に出すスライダ([`Field::slider`])
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slider: Option<Slider>,
    /// 数だけを打った時に補う既定の単位([`FieldUnit`])。式と数の欄は必ず持つ(持たない欄は
    /// 単位を補わない。試験が、組み込みの欄がみな持つことを確かめる)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<FieldUnit>,
    /// 必須の「前にある機能」の欄([`FieldKind::Output`])を、足す時に、欄 `follows` に入れた機能が
    /// 最初に読む同じ出力の機能で埋める(直前の候補より先)。面の上のスケッチから足す穴は、その
    /// スケッチを置いた面のボディを対象にする。[`Descriptor::follow`]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follows: Option<String>,
    /// 選ぶ欄([`FieldKind::Choice`])の値 `origin_xy`・`origin_yz`・`origin_zx` が原点の平面。画面は、
    /// この欄を持つ機能を直している間、原点の平面を 3D のビューに出し、選んでいる平面を色付けする
    ///
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub origin_planes: bool,
}

/// 欄の既定の単位: 数だけを打った時に、その欄が何の量として読むか。画面は欄の名前から単位を
/// 推し量らず、これに従う(長さは設定の単位に従う)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "suffix", rename_all = "snake_case")]
pub enum FieldUnit {
    /// 長さ。設定の長さの単位(文書の単位: mm・cm・m・in・ft)で補う。送りの速さ(mm/min)のように、
    /// 長さの式で書いて別の量として読む欄もこれ
    Length,
    /// 角度(deg で補う)
    Angle,
    /// 割合(0.3 のように数で書く。30% と打てば 0.3 にする)
    Ratio,
    /// 式に単位を書けない量(℃・W・N・MPa・m/s・s・rpm・% など)。数だけで書き、その単位で読む。
    /// 中の字は画面に出す単位の名前
    Plain(String),
    /// 単位の無い数(個数・倍率・番号など。例外の印)
    Unitless,
    /// 式そのものを書く欄(例外の印。数だけを打っても単位を補わない)
    Formula,
}

impl FieldUnit {
    /// 単位の名前(長さは `length` の単位で。単位の無い物は空)。
    pub fn suffix(&self, length: crate::LengthUnit) -> String {
        match self {
            FieldUnit::Length => length.name().into(),
            FieldUnit::Angle => "deg".into(),
            FieldUnit::Plain(s) => s.clone(),
            FieldUnit::Ratio | FieldUnit::Unitless | FieldUnit::Formula => String::new(),
        }
    }
}

/// 式の欄を引いて変えるスライダ(ジョイントを動かして確かめる時など)。範囲は欄 `min`・`max`
/// の値(空なら `lo`・`hi`)。引くと式を `<数> <unit>` に書き換える。欄 `min`・`max` に値があれば、
/// 画面はその外の値を赤くして受け付けない。欄 `rest` に値があれば、引いて離すとその値に戻す。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slider {
    pub min: String,
    pub max: String,
    pub lo: f64,
    pub hi: f64,
    pub unit: String,
    /// 引いて離した時に戻す値の欄(空なら戻さない。[`Field::rest`])
    #[serde(skip_serializing_if = "String::is_empty")]
    pub rest: String,
}

/// 行の並びの欄([`FieldKind::Rows`])の行 `row` の呼び名(「選択セット 1」)。
pub fn row_name(row: usize) -> String {
    format!("選択セット {}", row + 1)
}

/// 欄を決まった値にした時に始める操作([`Field::pick`])。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PickOn {
    pub value: String,
    pub action: String,
}

/// 欄の候補。`value` を欄に入れ、`label` を一覧に出す。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum Suggest {
    /// 決まった候補(例: 材料の名前)
    Values { values: Vec<Choice> },
    /// 欄 `field`(機能の ID の欄)が指す機能の出力 `slot` にある候補。出力は
    /// [`choices_value`] で作る(例: 工具ライブラリの工具の一覧)
    Output { field: String, slot: String },
    /// この機能そのものの出力 `slot` にある候補(例: ねじの面の径に近い順の呼び)
    Own { slot: String },
}

/// 欄を使う条件: データの `key`(なければ `default`)が `value` と等しい(`equal`)か、等しくないか。
/// `value` は `|` で区切って幾つも書ける(どれかと等しければ等しい。例: `revolute|cylindrical`)。
/// `and` があれば、その条件も満たす時だけ使う([`Field::and_only`])。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct When {
    pub key: String,
    pub value: String,
    pub equal: bool,
    pub default: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub and: Option<Box<When>>,
}

impl When {
    fn holds(&self, data: &serde_json::Value) -> bool {
        let f = Field::info(&self.key, "");
        let cur = f
            .get(data)
            .and_then(|x| x.as_str())
            .unwrap_or(&self.default);
        self.value.split('|').any(|v| v == cur) == self.equal
            && self.and.as_ref().is_none_or(|a| a.holds(data))
    }
}

impl Field {
    fn new(key: &str, label: &str, kind: FieldKind) -> Self {
        Field {
            key: key.into(),
            label: label.into(),
            kind,
            help: String::new(),
            when: None,
            suggest: None,
            pick: None,
            marks: None,
            hidden: Vec::new(),
            slider: None,
            unit: None,
            follows: None,
            origin_planes: false,
        }
    }

    /// 選択肢の値の `origin_xy`・`origin_yz`・`origin_zx` を原点の平面として見せる([`Field::origin_planes`])。
    /// [`Field::choice`] が選択肢から決めるので、ふつうは呼ばなくてよい。
    pub fn origin_planes(mut self) -> Self {
        self.origin_planes = true;
        self
    }

    /// 原点の平面の欄([`Field::origin_planes`])で、今の値が指す原点の平面(XY・YZ・ZX の 0・1・2)。
    pub fn origin_plane_of(&self, data: &serde_json::Value) -> Option<usize> {
        if !self.origin_planes {
            return None;
        }
        ORIGIN_PLANE_VALUES
            .iter()
            .position(|v| self.get(data).and_then(|x| x.as_str()) == Some(v))
    }

    /// 足す時に、欄 `key` の機能が読む機能でこの欄を埋める([`Field::follows`])。
    pub fn follows(mut self, key: &str) -> Self {
        self.follows = Some(key.into());
        self
    }

    /// 既定の単位を長さにする(設定の長さの単位で補う。[`FieldUnit::Length`])。
    pub fn length(mut self) -> Self {
        self.unit = Some(FieldUnit::Length);
        self
    }

    /// 既定の単位を角度(deg)にする。
    pub fn angle(mut self) -> Self {
        self.unit = Some(FieldUnit::Angle);
        self
    }

    /// 割合の欄にする(30% と打てば 0.3)。
    pub fn ratio(mut self) -> Self {
        self.unit = Some(FieldUnit::Ratio);
        self
    }

    /// 式に書けない単位 `suffix`(℃・W など)の量の欄にする。数だけで書く。
    pub fn plain(mut self, suffix: &str) -> Self {
        self.unit = Some(FieldUnit::Plain(suffix.into()));
        self
    }

    /// 単位の無い数の欄にする(個数・倍率など。例外の印)。
    pub fn unitless(mut self) -> Self {
        self.unit = Some(FieldUnit::Unitless);
        self
    }

    /// 式そのものを書く欄にする(例外の印)。
    pub fn formula(mut self) -> Self {
        self.unit = Some(FieldUnit::Formula);
        self
    }

    /// 式の欄の横にスライダを出す。範囲は欄 `min`・`max` の値で、空なら `lo`・`hi`(単位 `unit`)。
    pub fn slider(mut self, min: &str, max: &str, lo: f64, hi: f64, unit: &str) -> Self {
        self.slider = Some(Slider {
            min: min.into(),
            max: max.into(),
            lo,
            hi,
            unit: unit.into(),
            rest: String::new(),
        });
        self
    }

    /// スライダ([`Field::slider`])を引いて離した時に、欄 `rest` の値へ戻す(空の間は戻さない)。
    pub fn rest(mut self, rest: &str) -> Self {
        if let Some(s) = self.slider.as_mut() {
            s.rest = rest.into();
        }
        self
    }

    /// 選ぶ欄の値 `value` を、画面の一覧に出さない([`Field::hidden`])。
    pub fn hide_choice(mut self, value: &str) -> Self {
        self.hidden.push(value.into());
        self
    }

    /// 画面でこの欄を手で替えたら、データの `key` に `true` を入れる([`Field::marks`])。
    pub fn marks(mut self, key: &str) -> Self {
        self.marks = Some(key.into());
        self
    }

    /// 欄を値 `value` にしたら、操作 `action` を直している途中のまま始める([`Field::pick`])。
    pub fn pick_on(mut self, value: &str, action: &str) -> Self {
        self.pick = Some(PickOn {
            value: value.into(),
            action: action.into(),
        });
        self
    }

    /// この機能の出力 `slot` にある候補を付ける([`Suggest::Own`])。
    pub fn suggest_own(mut self, slot: &str) -> Self {
        self.suggest = Some(Suggest::Own { slot: slot.into() });
        self
    }

    pub fn expr(key: &str, label: &str) -> Self {
        Self::new(key, label, FieldKind::Expr { optional: false })
    }

    pub fn opt_expr(key: &str, label: &str) -> Self {
        Self::new(key, label, FieldKind::Expr { optional: true })
    }

    pub fn number(key: &str, label: &str) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Number {
                integer: false,
                optional: false,
            },
        )
    }

    pub fn opt_number(key: &str, label: &str) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Number {
                integer: false,
                optional: true,
            },
        )
    }

    pub fn integer(key: &str, label: &str) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Number {
                integer: true,
                optional: false,
            },
        )
    }

    pub fn opt_integer(key: &str, label: &str) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Number {
                integer: true,
                optional: true,
            },
        )
    }

    pub fn bool(key: &str, label: &str) -> Self {
        Self::new(key, label, FieldKind::Bool)
    }

    /// `choices` は (保存する値, 表示名) の組。
    /// 選ぶ欄。選択肢に原点の平面(`origin_xy`・`origin_yz`・`origin_zx`)があれば、原点の平面の欄
    /// ([`Field::origin_planes`])になる(機能ごとに印を付け忘れて、ビューに平面が出ない事の無いように)。
    pub fn choice(key: &str, label: &str, choices: &[(&str, &str)]) -> Self {
        let mut f = Self::new(
            key,
            label,
            FieldKind::Choice {
                choices: choices
                    .iter()
                    .map(|(v, l)| Choice {
                        value: (*v).into(),
                        label: (*l).into(),
                    })
                    .collect(),
            },
        );
        f.origin_planes = choices.iter().any(|(v, _)| ORIGIN_PLANE_VALUES.contains(v));
        f
    }

    pub fn output(key: &str, label: &str, slot: &str) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Output {
                slot: slot.into(),
                optional: false,
            },
        )
    }

    pub fn opt_output(key: &str, label: &str, slot: &str) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Output {
                slot: slot.into(),
                optional: true,
            },
        )
    }

    pub fn text(key: &str, label: &str) -> Self {
        Self::new(key, label, FieldKind::Text)
    }

    /// ファイルのパスの欄([`FieldKind::File`])。
    pub fn file(key: &str, label: &str) -> Self {
        Self::new(key, label, FieldKind::File)
    }

    pub fn vector(key: &str, label: &str) -> Self {
        Self::new(key, label, FieldKind::Vector)
    }

    pub fn info(key: &str, label: &str) -> Self {
        Self::new(key, label, FieldKind::Info)
    }

    /// 行の並びの欄([`FieldKind::Rows`])。`fields` の `key` は行の中の名前。
    pub fn rows(key: &str, label: &str, fields: Vec<Field>) -> Self {
        Self::new(key, label, FieldKind::Rows { fields })
    }

    /// 行の並びの欄の、行 `row` の欄(`key` は `並び.行番号.欄`、名前は「N 行目」)。
    pub fn row_field(&self, row: usize, sub: &Field) -> Field {
        let mut f = sub.clone();
        f.key = format!("{}.{row}.{}", self.key, sub.key);
        f.label = row_name(row);
        // 行の中の欄を見る条件(`@欄`)は、この行の欄を見る
        let mut w = f.when.as_mut();
        while let Some(cur) = w {
            if let Some(k) = cur.key.strip_prefix('@') {
                cur.key = format!("{}.{row}.{k}", self.key);
            }
            w = cur.and.as_deref_mut();
        }
        f
    }

    /// 行の並びの欄の行の欄 `sub` を、どれかの行で使うか(行の中の欄を見る条件 `@欄` は行ごとに見る)。
    pub fn applies_in_some_row(&self, sub: &Field, data: &serde_json::Value) -> bool {
        if !sub.row_relative() {
            return sub.applies(data);
        }
        (0..self.row_count(data)).any(|i| self.row_field(i, sub).applies(data))
    }

    /// 行の中の欄を見る条件(`@欄`)を持つか。
    pub fn row_relative(&self) -> bool {
        let mut w = self.when.as_ref();
        while let Some(cur) = w {
            if cur.key.starts_with('@') {
                return true;
            }
            w = cur.and.as_deref();
        }
        false
    }

    /// 行の並びの欄の行 `row` の見出し。番号と中身(選んだ物の数と最初の値)を書く
    /// (例: 「選択セット 1(辺 2・半径 3 mm)」)。
    pub fn row_title(&self, row: usize, data: &serde_json::Value) -> String {
        let FieldKind::Rows { fields } = &self.kind else {
            return row_name(row);
        };
        let mut parts = Vec::new();
        for s in fields {
            let rf = self.row_field(row, s);
            match &s.kind {
                FieldKind::Pick { .. } => {
                    let refs = rf.refs(data);
                    if refs.is_empty() {
                        parts.push("未選択".to_string());
                    }
                    let mut kinds: Vec<(crate::RefKind, usize)> = Vec::new();
                    for r in &refs {
                        match kinds.iter_mut().find(|(k, _)| *k == r.kind()) {
                            Some((_, n)) => *n += 1,
                            None => kinds.push((r.kind(), 1)),
                        }
                    }
                    parts.extend(kinds.iter().map(|(k, n)| format!("{} {n}", k.label())));
                }
                FieldKind::Expr { .. } if rf.applies(data) => {
                    if let Some(v) = rf.get(data).and_then(|v| v.as_str()) {
                        parts.push(format!("{} {v}", s.label));
                    }
                    // 値の欄は最初の 1 つだけ(見出しが長くならないように)
                    break;
                }
                _ => {}
            }
        }
        format!("{}({})", row_name(row), parts.join("・"))
    }

    /// 行の並びの欄の行の数(並びでなければ 0)。
    pub fn row_count(&self, data: &serde_json::Value) -> usize {
        self.get(data)
            .and_then(|v| v.as_array())
            .map_or(0, |a| a.len())
    }

    /// 3D のビューで選ぶ欄([`FieldKind::Pick`])。`max` が `None` ならいくつでも。
    pub fn pick(
        key: &str,
        label: &str,
        accept: &[crate::RefKind],
        min: usize,
        max: Option<usize>,
    ) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Pick {
                accept: accept.to_vec(),
                min,
                max,
            },
        )
    }

    /// 選ぶ欄の、データに入っている物(1 つでも並びでも読む。読めない物は飛ばす)。
    pub fn refs(&self, data: &serde_json::Value) -> Vec<crate::Ref> {
        match self.get(data) {
            None | Some(serde_json::Value::Null) => vec![],
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect(),
            Some(v) => serde_json::from_value(v.clone()).into_iter().collect(),
        }
    }

    /// 選ぶ欄に物を入れる(空なら項目を消し、1 つなら値そのもの、2 つ以上なら並び)。
    pub fn set_refs(&self, data: &mut serde_json::Value, refs: &[crate::Ref]) {
        let v = match refs {
            [] => None,
            [one] => Some(serde_json::to_value(one).expect("JSON にできる")),
            _ => Some(serde_json::to_value(refs).expect("JSON にできる")),
        };
        self.set(data, v);
    }

    /// 選ぶ欄に、あと何個入るか(選ぶ欄でなければ 0)。
    pub fn room(&self, data: &serde_json::Value) -> usize {
        match &self.kind {
            FieldKind::Pick { max, .. } => {
                max.map_or(usize::MAX, |m| m.saturating_sub(self.refs(data).len()))
            }
            _ => 0,
        }
    }

    pub fn help(mut self, help: &str) -> Self {
        self.help = help.into();
        self
    }

    /// 一覧から選べる決まった候補を付ける(`values` は欄に入れる値で、一覧にもそのまま出す)。
    pub fn suggest(mut self, values: &[&str]) -> Self {
        self.suggest = Some(Suggest::Values {
            values: values
                .iter()
                .map(|v| Choice {
                    value: (*v).into(),
                    label: (*v).into(),
                })
                .collect(),
        });
        self
    }

    /// 欄 `field` が指す機能の出力 `slot` にある候補を付ける([`Suggest::Output`])。
    pub fn suggest_from(mut self, field: &str, slot: &str) -> Self {
        self.suggest = Some(Suggest::Output {
            field: field.into(),
            slot: slot.into(),
        });
        self
    }

    /// データの `key` が `value` の時は使わない欄にする(`key` がなければ `value` とみなす)。
    pub fn unless(mut self, key: &str, value: &str) -> Self {
        self.when = Some(When {
            key: key.into(),
            value: value.into(),
            equal: false,
            default: value.into(),
            and: None,
        });
        self
    }

    /// データの `key`(なければ `default`)が `value` の時だけ使う欄にする(`value` は `|` で区切って
    /// 幾つも書ける。[`When`])。行の並びの行の欄では、
    /// `key` を `@欄` と書くと同じ行の欄を見る(例: 行の半径の種類が「途中で変わる」の時だけ出す終わりの半径)。
    pub fn only(mut self, key: &str, value: &str, default: &str) -> Self {
        self.when = Some(When {
            key: key.into(),
            value: value.into(),
            equal: true,
            default: default.into(),
            and: None,
        });
        self
    }

    /// 今の条件([`Field::only`] など)に加えて、データの `key`(なければ `default`)が `value` の時
    /// だけ使う欄にする(例: 動かし方が回転で、軸が「通る点と向き」の時だけ出す欄)。
    pub fn and_only(mut self, key: &str, value: &str, default: &str) -> Self {
        let add = When {
            key: key.into(),
            value: value.into(),
            equal: true,
            default: default.into(),
            and: None,
        };
        match &mut self.when {
            None => self.when = Some(add),
            Some(w) => {
                let mut cur = w;
                while cur.and.is_some() {
                    cur = cur.and.as_mut().expect("確かめた");
                }
                cur.and = Some(Box::new(add));
            }
        }
        self
    }

    /// この欄を今のデータで使うか。
    pub fn applies(&self, data: &serde_json::Value) -> bool {
        self.when.as_ref().is_none_or(|w| w.holds(data))
    }

    /// データの中のこの欄の値。
    pub fn get<'a>(&self, data: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        self.key
            .split('.')
            .try_fold(data, |v, k| match (v, k.parse::<usize>()) {
                // 行の並び([`FieldKind::Rows`])の行は番号でたどる
                (serde_json::Value::Array(a), Ok(i)) => a.get(i),
                _ => v.get(k),
            })
    }

    /// データの中のこの欄に値を入れる(`None` なら項目を消す)。途中の入れ子は作る。
    pub fn set(&self, data: &mut serde_json::Value, value: Option<serde_json::Value>) {
        let mut parts: Vec<&str> = self.key.split('.').collect();
        let last = parts.pop().expect("key は空でない");
        let mut cur = data;
        for p in parts {
            if let (true, Ok(i)) = (cur.is_array(), p.parse::<usize>()) {
                // 行の並びの行。無い行は空の行で足す(行の中身は [`Descriptor::ensure_row`] で入れる)
                let a = cur.as_array_mut().expect("並び");
                while a.len() <= i {
                    a.push(serde_json::json!({}));
                }
                cur = &mut a[i];
                continue;
            }
            if !cur.get(p).is_some_and(|v| v.is_object() || v.is_array()) {
                cur[p] = serde_json::json!({});
            }
            cur = cur.get_mut(p).expect("今入れた");
        }
        match value {
            Some(v) => cur[last] = v,
            None => {
                if let Some(o) = cur.as_object_mut() {
                    o.remove(last);
                }
            }
        }
    }
}

/// 参照が切れた機能を、3D のビューで選んだ面に付け替える方法。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Retarget {
    /// データの `key` に面の名前を入れる
    Face { key: String },
    /// データの `key` に面の名前 1 つの並びを入れ、`clear` の項目を消す(辺の選び直し)
    Faces { key: String, clear: Vec<String> },
    /// データの `key` に `{"kind": "face", "feature": 機能, "face": 面}` を入れる(面の上の平面。
    /// 前の値の `origin` は残す)
    Plane { key: String },
    /// 選ぶ欄 `key` を、その面 1 つ(`{"kind": "face", ...}`)にする(行の並びの行の欄でもよい)
    Pick { key: String },
}

impl Retarget {
    /// 今のデータで付け替えられるか(面の上の平面は、面を参照している時だけ)。
    pub fn applies(&self, data: &serde_json::Value) -> bool {
        match self {
            Retarget::Plane { key } => Field::info(key, "")
                .get(data)
                .and_then(|p| p.get("face"))
                .is_some(),
            _ => true,
        }
    }

    /// `data` の参照を、機能 `feature` の面 `face` に付け替える。
    pub fn apply(&self, data: &mut serde_json::Value, feature: u64, face: &str) {
        let set =
            |data: &mut serde_json::Value, key: &str, v| Field::info(key, "").set(data, Some(v));
        match self {
            Retarget::Face { key } => set(data, key, serde_json::json!(face)),
            Retarget::Faces { key, clear } => {
                set(data, key, serde_json::json!([face]));
                for c in clear {
                    Field::info(c, "").set(data, None);
                }
            }
            Retarget::Pick { key } => set(
                data,
                key,
                serde_json::json!({"kind": "face", "feature": feature, "face": face}),
            ),
            Retarget::Plane { key } => {
                // 面の上のスケッチの原点の決め方(`origin`)は残す。角が新しい面に無ければ、
                // スケッチが面の左下の角に置き直して知らせる
                let origin = Field::info(key, "")
                    .get(data)
                    .and_then(|p| p.get("origin"))
                    .cloned();
                let mut v = serde_json::json!({"kind": "face", "feature": feature, "face": face});
                if let Some(o) = origin {
                    v["origin"] = o;
                }
                set(data, key, v)
            }
        }
    }
}

/// 機能の種類の説明。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Descriptor {
    pub type_id: String,
    pub category: String,
    /// 画面に出す名前(例: 「押し出し」)
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hint: String,
    /// 設定欄(上から順に出す)。ここにない項目は「詳細(JSON)」で編集する
    pub fields: Vec<Field>,
    /// 「詳細(JSON)」の欄を出すか。中身が大きいもの(読み込んだファイルなど)は出さない
    pub raw_editor: bool,
    /// 追加する時の初めのデータ。あれば画面やコマンド検索から追加できる。
    /// 必須の「前にある機能」の欄([`FieldKind::Output`])は、追加する側が直前の候補で埋める
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<serde_json::Value>,
    /// 検索に使う別名(読み、英語。空白で区切る)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub aliases: String,
    /// 参照の付け替え(面を参照する種類だけ)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retarget: Option<Retarget>,
    /// ツールバーのまとまりの名前(例: 「作成」)。空ならコマンド検索とメニューだけに出す
    #[serde(skip_serializing_if = "String::is_empty")]
    pub menu: String,
    /// ツールバーの前に初めから出すか(利用者が外せる)
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// アイコンの名前(例: `fillet`)。絵は画面が持つ
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// ツールバーとメニューでの並び(小さい順。同じなら種類の名前の順)
    #[serde(skip_serializing_if = "is_zero")]
    pub order: i32,
    /// ブラウザでこの機能を下に並べる親を指す欄(例: 加工の `setup`)。なければ種類ごとのまとまりに並べる
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// CLI・MCP から新しく作る時、データに無い欄へ入れる値(画面から作った時と既定をそろえる)。
    /// 保存済みの文書を読む時は使わない(省いた欄は前からの serde の既定のまま)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_defaults: Option<serde_json::Value>,
    /// 前の形のデータを、欄が読める今の形に直す(例: 1 組だけだったフィレットを行の並びに)。
    /// 画面は直す前にこれを通す。機能の計算は前の形もそのまま読む
    #[serde(skip)]
    pub upgrade: Option<Upgrade>,
    /// 役割の印(画面が名前でなく役割で探す種類だけ。[`crate::KindRole`])
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<crate::KindRole>,
    /// この種類で計算に失敗した時の直し方の例(画面は共通の例の後に並べる)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<Fix>,
    /// 先に選んでいた物がボディだけの時に入れる欄の値(欄の名前, 値)。パターンを、
    /// ボディを選んでから始めると種類が「ボディ」になる、など
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_bodies: Option<(String, String)>,
    /// 画面や CLI・MCP からこの種類を足す時、続けて同じ取り消し単位で足す種類とその初めのデータ
    /// (データの `{"$new": 0}` は先に足したこの種類の機能)。1 つの機能から 2 つのボディを出す
    /// 物(ボディの分割の残りの側など)に使う。足した後に直すのは、先に足したこの種類の方
    pub companion: Option<(String, serde_json::Value)>,
}

/// 直し方の例の 1 つ([`Descriptor::fixes`])。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fix {
    /// 誤りの文がこれを含む時だけ出す(空なら、この種類の誤りすべてに出す)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub when: String,
    pub text: String,
}

/// [`Descriptor::upgrade`] の直し方。比べる時はどれも同じとみなす(関数は比べられないため)。
#[derive(Clone, Copy)]
pub struct Upgrade(pub fn(&mut serde_json::Value));

impl std::fmt::Debug for Upgrade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Upgrade")
    }
}

impl PartialEq for Upgrade {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Descriptor {
    pub fn new(type_id: &str, category: &str, label: &str) -> Self {
        Descriptor {
            type_id: type_id.into(),
            category: category.into(),
            label: label.into(),
            hint: String::new(),
            fields: Vec::new(),
            raw_editor: true,
            template: None,
            aliases: String::new(),
            retarget: None,
            menu: String::new(),
            pinned: false,
            icon: String::new(),
            order: 0,
            parent: None,
            create_defaults: None,
            upgrade: None,
            role: None,
            fixes: Vec::new(),
            on_bodies: None,
            companion: None,
        }
    }

    /// 足す時に続けて足す種類([`Descriptor::companion`])。
    pub fn companion(mut self, type_id: &str, data: serde_json::Value) -> Self {
        self.companion = Some((type_id.into(), data));
        self
    }

    /// 先に選んでいた物がボディだけなら、欄 `key` を `value` にしてから振り分ける([`Descriptor::on_bodies`])。
    pub fn on_bodies(mut self, key: &str, value: &str) -> Self {
        self.on_bodies = Some((key.into(), value.into()));
        self
    }

    /// 役割の印を付ける([`Descriptor::role`])。
    pub fn role(mut self, r: crate::KindRole) -> Self {
        self.role = Some(r);
        self
    }

    /// 直し方の例を足す。`when` を含む誤りの時だけ出す(空ならいつも)。
    pub fn fix(mut self, when: &str, text: &str) -> Self {
        self.fixes.push(Fix {
            when: when.into(),
            text: text.into(),
        });
        self
    }

    /// 前の形のデータの直し方([`Descriptor::upgrade`])。
    pub fn upgrade(mut self, f: fn(&mut serde_json::Value)) -> Self {
        self.upgrade = Some(Upgrade(f));
        self
    }

    /// `data` を今の形にしたもの(直し方がなければそのまま)。
    pub fn upgraded(&self, data: &serde_json::Value) -> serde_json::Value {
        let mut d = data.clone();
        if let Some(Upgrade(f)) = self.upgrade {
            f(&mut d);
        }
        d
    }

    /// 欄 `key`。行の並びの行の欄(`並び.行番号.欄`)は、行の欄を行の番号つきで返す
    /// (まだ無い行の番号でもよい。足す行の欄)。
    pub fn field_at(&self, key: &str) -> Option<Field> {
        if let Some(f) = self.fields.iter().find(|f| f.key == key) {
            return Some(f.clone());
        }
        self.fields.iter().find_map(|f| {
            let FieldKind::Rows { fields } = &f.kind else {
                return None;
            };
            let rest = key.strip_prefix(&f.key)?.strip_prefix('.')?;
            let (row, sub) = rest.split_once('.')?;
            let row: usize = row.parse().ok()?;
            let sub = fields.iter().find(|s| s.key == sub)?;
            Some(f.row_field(row, sub))
        })
    }

    /// 選ぶ欄すべて(行の並びは、今ある行ごとの選ぶ欄に開く)。
    pub fn pick_fields(&self, data: &serde_json::Value) -> Vec<Field> {
        let mut out = Vec::new();
        for f in &self.fields {
            match &f.kind {
                FieldKind::Pick { .. } => out.push(f.clone()),
                FieldKind::Rows { fields } => {
                    for row in 0..f.row_count(data) {
                        for s in fields
                            .iter()
                            .filter(|s| matches!(s.kind, FieldKind::Pick { .. }))
                        {
                            out.push(f.row_field(row, s));
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// 欄 `key` が行の並びのまだ無い行なら、前の行を写して(選ぶ欄は空にして)足す
    /// (前の行の半径のまま新しい行ができるのと同じ)。
    pub fn ensure_row(&self, data: &mut serde_json::Value, key: &str) {
        for f in &self.fields {
            let FieldKind::Rows { fields } = &f.kind else {
                continue;
            };
            let Some(rest) = key.strip_prefix(&f.key).and_then(|r| r.strip_prefix('.')) else {
                continue;
            };
            let Some(row) = rest.split('.').next().and_then(|r| r.parse::<usize>().ok()) else {
                continue;
            };
            let n = f.row_count(data);
            if row < n {
                return;
            }
            let mut last = f
                .get(data)
                .and_then(|v| v.as_array())
                .and_then(|a| a.last().cloned())
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(o) = last.as_object_mut() {
                for s in fields
                    .iter()
                    .filter(|s| matches!(s.kind, FieldKind::Pick { .. }))
                {
                    o.remove(&s.key);
                }
            }
            let mut rows = f
                .get(data)
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            while rows.len() <= row {
                rows.push(last.clone());
            }
            f.set(data, Some(serde_json::Value::Array(rows)));
            return;
        }
    }

    /// 最初の選ぶ欄が空の行を消す(1 行目だけになっても 1 行は残す)。消した行があれば真。
    pub fn prune_rows(&self, data: &mut serde_json::Value) -> bool {
        let mut pruned = false;
        for f in &self.fields {
            let FieldKind::Rows { fields } = &f.kind else {
                continue;
            };
            let Some(pick) = fields
                .iter()
                .find(|s| matches!(s.kind, FieldKind::Pick { .. }))
            else {
                continue;
            };
            let Some(rows) = f.get(data).and_then(|v| v.as_array()).cloned() else {
                continue;
            };
            let filled: Vec<_> = rows
                .iter()
                .filter(|r| !pick.refs(r).is_empty())
                .cloned()
                .collect();
            let keep = if filled.is_empty() {
                rows.into_iter().take(1).collect()
            } else {
                filled
            };
            if Some(keep.len()) != f.get(data).and_then(|v| v.as_array()).map(|a| a.len()) {
                pruned = true;
                f.set(data, Some(serde_json::Value::Array(keep)));
            }
        }
        pruned
    }

    /// ツールバーのまとまり `m` に入れる。
    pub fn menu(mut self, m: &str) -> Self {
        self.menu = m.into();
        self
    }

    /// アイコンの名前。
    pub fn icon(mut self, i: &str) -> Self {
        self.icon = i.into();
        self
    }

    /// ツールバーの前に初めから出す。
    pub fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }

    /// ツールバーとメニューでの並びを決める(小さい順)。
    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// ブラウザで、欄 `key`(前の機能を指す欄)の機能の下に並べる。
    pub fn parent(mut self, key: &str) -> Self {
        self.parent = Some(key.into());
        self
    }

    /// CLI・MCP から新しく作る時の、欄の既定([`Descriptor::create_defaults`])。
    pub fn create_defaults(mut self, data: serde_json::Value) -> Self {
        self.create_defaults = Some(data);
        self
    }

    /// 新しく作るデータ `data` の無い欄に、[`Descriptor::create_defaults`] の値を入れる。
    pub fn fill_create_defaults(&self, data: &mut serde_json::Value) {
        let (Some(serde_json::Value::Object(d)), serde_json::Value::Object(m)) =
            (&self.create_defaults, data)
        else {
            return;
        };
        for (k, v) in d {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }

    /// 追加する時の初めのデータ。
    pub fn template(mut self, data: serde_json::Value) -> Self {
        self.template = Some(data);
        self
    }

    /// 初めのデータの必須の参照を、`candidates(slot)`(前にある候補、古い順)で埋める。
    /// 同じ出力を求める欄が 2 つあれば(例: ブーリアンの対象と道具)、後ろの欄ほど新しい候補にする。
    /// 埋められない欄があれば、その欄の名前を返す。
    pub fn instantiate(
        &self,
        candidates: &dyn Fn(&str) -> Vec<u64>,
    ) -> Result<serde_json::Value, String> {
        let mut data = self.template.clone().unwrap_or(serde_json::json!({}));
        let mut used: Vec<u64> = Vec::new();
        for f in self.fields.iter().rev() {
            let FieldKind::Output {
                slot,
                optional: false,
            } = &f.kind
            else {
                continue;
            };
            if !f.applies(&data) || f.get(&data).is_some_and(|v| !v.is_null()) {
                continue;
            }
            let pick = candidates(slot)
                .into_iter()
                .rev()
                .find(|c| !used.contains(c))
                .ok_or_else(|| f.label.clone())?;
            used.push(pick);
            f.set(&mut data, Some(serde_json::Value::from(pick)));
        }
        Ok(data)
    }

    /// 先に選んだ物 `refs` を、選ぶ欄([`FieldKind::Pick`])へ振り分ける。選んだ順に見ていき、
    /// 種類が合って空きのある最初の欄に入れる。どの欄も受けない面・辺・頂点は、ボディを受ける欄に
    /// 空きがあれば、その物のボディとして入れる(同じボディは 1 度だけ)。入らなかった物を返す。
    /// `planar` は面が平らか(平らな面だけを受ける欄に使う)。
    pub fn distribute(
        &self,
        data: &mut serde_json::Value,
        refs: &[crate::Ref],
        planar: &dyn Fn(&crate::Ref) -> bool,
    ) -> Vec<crate::Ref> {
        let picks: Vec<&Field> = self
            .fields
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::Pick { .. }))
            .collect();
        let accepts = |f: &Field, r: &crate::Ref| match &f.kind {
            FieldKind::Pick { accept, .. } => r.accepted_by(accept, planar),
            _ => false,
        };
        let mut left = Vec::new();
        for r in refs {
            let direct = picks
                .iter()
                .find(|f| f.applies(data) && f.room(data) > 0 && accepts(f, r));
            let (field, item) = match direct {
                Some(f) => (Some(*f), r.clone()),
                None => {
                    // ボディは、どれかの欄にもう入っていれば足さない(対象を道具にもしない)。
                    // 機能を受ける欄には、面を作った機能を入れる
                    let feature_field = picks.iter().any(|f| {
                        matches!(&f.kind, FieldKind::Pick { accept, .. }
                            if accept.contains(&crate::RefKind::Feature))
                    });
                    let creator = match r {
                        crate::Ref::Face { face, .. } if feature_field => {
                            crate::face_feature(face).unwrap_or(r.feature())
                        }
                        _ => r.feature(),
                    };
                    let body = crate::Ref::Body(creator);
                    let taken = picks.iter().any(|f| {
                        let is_feature = matches!(&f.kind, FieldKind::Pick { accept, .. }
                            if accept.contains(&crate::RefKind::Feature));
                        is_feature == feature_field && f.refs(data).contains(&body)
                    });
                    let f = picks.iter().find(|f| {
                        !taken && f.applies(data) && f.room(data) > 0 && accepts(f, &body)
                    });
                    (f.copied(), body)
                }
            };
            match field {
                Some(f) => {
                    let mut now = f.refs(data);
                    if !now.contains(&item) {
                        now.push(item);
                        f.set_refs(data, &now);
                    }
                    // 機能の欄に入れた面のボディを、空いている「足すボディ」の欄(ボディだけを受け、
                    // 空でもよい 1 つの欄)に入れる(画面のクリックと同じ)
                    let feature = matches!(&f.kind, FieldKind::Pick { accept, .. }
                        if accept.contains(&crate::RefKind::Feature));
                    if feature
                        && let Some(b) = picks.iter().find(|b| {
                            matches!(&b.kind, FieldKind::Pick { accept, min: 0, max: Some(1) }
                                if accept == &[crate::RefKind::Body])
                                && b.applies(data)
                                && b.refs(data).is_empty()
                        })
                    {
                        b.set_refs(data, &[crate::Ref::Body(r.feature())]);
                    }
                }
                None => left.push(r.clone()),
            }
        }
        left
    }

    /// [`Descriptor::instantiate`] と同じだが、先に選んだ物 `refs` を選ぶ欄へ振り分けてから、
    /// 空いたまま残った必須のボディの欄を直前の候補で埋める。データ、入らなかった物、候補で埋めた
    /// 欄の `key` を返す(画面は、候補で埋めた欄を最初のクリックで選び直させる)。
    pub fn instantiate_with(
        &self,
        candidates: &dyn Fn(&str) -> Vec<u64>,
        refs: &[crate::Ref],
        planar: &dyn Fn(&crate::Ref) -> bool,
    ) -> Result<(serde_json::Value, Vec<crate::Ref>, Vec<String>), String> {
        let mut data = self.instantiate(candidates)?;
        if let Some((key, value)) = &self.on_bodies
            && !refs.is_empty()
            && refs.iter().all(|r| matches!(r, crate::Ref::Body(_)))
        {
            Field::info(key, "").set(&mut data, Some(serde_json::json!(value)));
        }
        let left = self.distribute(&mut data, refs, planar);
        let mut first = data.clone();
        match self.fill_required(&mut first, candidates) {
            Ok(auto) => Ok((first, left, auto)),
            // 道具の作り方を持つ機能が無い(最後がフィレットなど)時は、ボディを並べる形で始める
            Err(label) => match &self.on_bodies {
                Some((key, value))
                    if Field::info(key, "").get(&data) != Some(&serde_json::json!(value)) =>
                {
                    Field::info(key, "").set(&mut data, Some(serde_json::json!(value)));
                    let auto = self.fill_required(&mut data, candidates)?;
                    Ok((data, left, auto))
                }
                _ => Err(label),
            },
        }
    }

    /// [`Field::follows`] の欄を、相手の欄の機能が最初に読む候補(`reads` が返す機能のうち、
    /// `candidates(slot)` にある物)で埋め直す。読む物が候補に無ければ今の値のまま。
    /// 替えた欄の `key` を返す。
    pub fn follow(
        &self,
        data: &mut serde_json::Value,
        candidates: &dyn Fn(&str) -> Vec<u64>,
        reads: &dyn Fn(u64) -> Vec<u64>,
    ) -> Vec<String> {
        let mut changed = Vec::new();
        for f in &self.fields {
            let (Some(key), FieldKind::Output { slot, .. }) = (&f.follows, &f.kind) else {
                continue;
            };
            let Some(other) = Field::info(key, "").get(data).and_then(|v| v.as_u64()) else {
                continue;
            };
            let cands = candidates(slot);
            let Some(pick) = reads(other).into_iter().find(|r| cands.contains(r)) else {
                continue;
            };
            if f.get(data).and_then(|v| v.as_u64()) != Some(pick) {
                f.set(data, Some(serde_json::Value::from(pick)));
                changed.push(f.key.clone());
            }
        }
        changed
    }

    /// 空いたまま残った必須のボディ・機能の欄を、まだ使っていない新しい候補から埋める
    /// (後ろの欄ほど新しい候補)。候補で埋めた欄の `key` を返し、埋められなければ欄の名前を返す。
    fn fill_required(
        &self,
        data: &mut serde_json::Value,
        candidates: &dyn Fn(&str) -> Vec<u64>,
    ) -> Result<Vec<String>, String> {
        let mut used: Vec<crate::Ref> = self
            .fields
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::Pick { .. }))
            .flat_map(|f| f.refs(data))
            .collect();
        let mut auto = Vec::new();
        for f in self.fields.iter().rev() {
            let FieldKind::Pick { accept, min, .. } = &f.kind else {
                continue;
            };
            // ボディは見えているボディ、機能は道具の作り方を持つ機能の候補から。面も受ける欄
            // (シェルの「面/ボディ」)は面を選ぶのが主なので埋めない
            let slot = if accept.contains(&crate::RefKind::Face) {
                continue;
            } else if accept.contains(&crate::RefKind::Body) {
                "body"
            } else if accept.contains(&crate::RefKind::Feature) {
                "recipe"
            } else {
                continue;
            };
            if !f.applies(data) {
                continue;
            }
            let mut now = f.refs(data);
            if now.len() < *min {
                auto.insert(0, f.key.clone());
            }
            while now.len() < *min {
                let next = candidates(slot)
                    .into_iter()
                    .rev()
                    .map(|c| crate::Ref::Body(crate::FeatureId(c)))
                    .find(|c| !used.contains(c))
                    .ok_or_else(|| f.label.clone())?;
                used.push(next.clone());
                now.push(next);
            }
            f.set_refs(data, &now);
        }
        Ok(auto)
    }

    pub fn hint(mut self, hint: &str) -> Self {
        self.hint = hint.into();
        self
    }

    pub fn aliases(mut self, aliases: &str) -> Self {
        self.aliases = aliases.into();
        self
    }

    pub fn retarget(mut self, r: Retarget) -> Self {
        self.retarget = Some(r);
        self
    }

    pub fn field(mut self, f: Field) -> Self {
        self.fields.push(f);
        self
    }

    pub fn fields(mut self, f: impl IntoIterator<Item = Field>) -> Self {
        self.fields.extend(f);
        self
    }

    pub fn no_raw_editor(mut self) -> Self {
        self.raw_editor = false;
        self
    }

    /// 説明のない種類のための既定の記述子。
    pub fn unknown(type_id: &str) -> Self {
        Descriptor::new(type_id, category::OTHER, type_id)
    }
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}
