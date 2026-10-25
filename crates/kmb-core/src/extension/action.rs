//! 操作の名乗り(出す場所・入力・役割の印・設定の欄)。

use super::*;

/// 操作を出す場所。コマンド検索には全部出す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// ツールバーの前に出すボタン(`menu` のまとまりの中。利用者が外せる)
    Toolbar,
    /// ツールバーの `menu` のまとまりの ▾ の一覧だけ(前には出さない)。場所が空の時と同じだが、
    /// 右クリックなどほかの場所を足しても ▾ の一覧から消えないように、はっきり書く時に使う
    Menu,
    /// タイムラインの機能の右クリック(選んでいる機能についての操作)
    FeatureMenu,
    /// 機能の設定欄のボタン(選んでいる機能についての操作)
    Panel,
    /// 3D のビューの右クリック(選んでいる面についての操作)
    FaceMenu,
    /// 「ファイル」のメニュー(読み込み、書き出し)
    FileMenu,
    /// タイムライン(加工では段の帯)の右端のボタン
    Timeline,
}

/// 操作が実行の前に求めるもの。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Input {
    None,
    /// ファイルを 1 つ選ばせる(`extensions` は `step` のように点なし)
    OpenFile {
        title: String,
        filter: String,
        extensions: Vec<String>,
    },
    /// 保存先を選ばせる。結果の [`Outcome::File`] は、選んだファイルへ書く
    SaveFile {
        title: String,
        filter: String,
        extensions: Vec<String>,
    },
    /// 名前を打たせる(`default` を初めに入れておく)。画面は打った名前を指定 `name` に、
    /// 選んでいる機能の番号の一覧(タイムラインの順)を指定 `selection` に入れて実行する
    Name {
        title: String,
        label: String,
        default: String,
    },
}

/// 画面・CLI・MCP が操作を探す時の役割の印。画面は機能の名前を書かず、この印で探す(名前や形を
/// 変えても、印を持つ側だけを直せばよい)。コアは印の型と探し方だけを持ち、どんな役割があるかは
/// 知らない。語彙(押し出し・ボディを書き出すなど)は kmb-interchange の `role` が定数で持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ActionRole(pub &'static str);

/// 画面が機能の種類を探す時の役割の印(面を作る機能・ボディを写すなど)。語彙は kmb-interchange の
/// `role::kind`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct KindRole(pub &'static str);

/// 画面が設定のまとまりを探す時の役割の印(機械・レンダリングのシーン)。語彙は kmb-interchange の
/// `role::settings`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SettingsRole(pub &'static str);

/// PC 全体の設定の 1 まとまり(機械など)。文書には入らず、データフォルダに保存する。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    /// 保存に使う名前(例: `cam.machine`)
    pub id: String,
    /// 画面の見出し(例: 「機械」)
    pub label: String,
    /// 置く作業空間([`Action::group`] と同じ語。`cam` なら設定の窓の加工の頁、ほかは設計の頁)
    pub group: String,
    /// 変えられない事柄の説明(ポストの名前など)
    pub notes: Vec<String>,
    /// 変えられる欄(数・真偽・文字)
    pub fields: Vec<crate::Field>,
    /// 今の値
    pub value: serde_json::Value,
    /// 初めの値(「初めの値に戻す」に使う)
    pub default: serde_json::Value,
    /// 役割の印(画面が探す設定だけ)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<SettingsRole>,
}

/// 操作の 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Action {
    /// 例: `cam.add.pocket`
    pub id: String,
    pub label: String,
    /// 検索に使う別名(読み、英語。空白で区切る)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub aliases: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hint: String,
    /// 作業空間(例: `cad` は設計、`cam` は加工、`io` はファイル)
    pub group: String,
    /// ツールバーのまとまりの名前(例: 「作成」「2D」)。空ならコマンド検索とメニューだけに出す
    #[serde(skip_serializing_if = "String::is_empty")]
    pub menu: String,
    /// アイコンの名前(例: `extrude`)。絵は画面が持つ。空なら名前の頭の文字で出す
    #[serde(skip_serializing_if = "String::is_empty")]
    pub icon: String,
    pub places: Vec<Place>,
    pub input: Input,
    /// 使えない理由(使えるなら `None`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<String>,
    /// 画面で、この操作が機能を足したすぐ後に続けて始める操作(穴を作った後に同じ穴を足す、など)。
    /// 足した機能は「作っている途中」のまま、それを選んだ文脈で始める
    #[serde(skip_serializing_if = "Option::is_none")]
    pub then: Option<String>,
    /// 画面で、3D のビューの手順を決めて実行した後、同じ操作をもう一度始める(Esc か、
    /// 省ける手順を Enter で値なしにすると終える)。続けてクリックで足していく操作に使う
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub repeat: bool,
    /// 画面で、作っている途中・直している途中の機能をそのままにして 3D のビューの手順を始める
    /// (選んでいる機能のデータだけを変える操作。押し出しの対象の面を選ぶ、など)
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub keep_edit: bool,
    /// 役割の印(画面が名前でなく役割で探す操作だけ)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<ActionRole>,
    /// 役割の印が名前ごとに付く操作の、その名前(テンプレートから作る操作のテンプレートの名前。
    /// 画面は 1 つの一覧に名前で出す)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_name: Option<String>,
    /// 並べる時に、この操作を操作 `before` のすぐ前に置く(別のモジュールの操作のまとまりの中に
    /// 並べる時。その操作が無ければモジュールの順のまま)。並べ終えた一覧には要らないので書き出さない
    #[serde(skip)]
    pub before: Option<String>,
    /// 受け取る引数の宣言([`crate::args`])。ここに無い鍵を渡すと誤りになる
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<Arg>,
    /// この操作が置き換えた機能の種類(欄で足す種類から、クリックで作る操作に替えた時など)。画面は、
    /// 利用者が前の種類をツールバーの前に出していれば、この操作を前に出す
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replaces: Vec<String>,
}

impl Action {
    pub fn new(id: &str, label: &str, group: &str) -> Self {
        Action {
            id: id.into(),
            label: label.into(),
            aliases: String::new(),
            hint: String::new(),
            group: group.into(),
            menu: String::new(),
            icon: String::new(),
            places: Vec::new(),
            input: Input::None,
            disabled: None,
            then: None,
            repeat: false,
            keep_edit: false,
            role: None,
            role_name: None,
            before: None,
            args: Vec::new(),
            replaces: Vec::new(),
        }
    }

    /// この操作が機能の種類 `type_id` を置き換えた([`Action::replaces`])。
    pub fn replaces(mut self, type_id: &str) -> Self {
        self.replaces.push(type_id.into());
        self
    }

    /// 役割の印を付ける([`Action::role`])。
    pub fn role(mut self, r: ActionRole) -> Self {
        self.role = Some(r);
        self
    }

    /// 名前ごとに付く役割の印を付ける([`Action::role`]・[`Action::role_name`])。
    pub fn role_named(mut self, r: ActionRole, name: &str) -> Self {
        self.role = Some(r);
        self.role_name = Some(name.to_string());
        self
    }

    /// 引数 `a` を受け取る([`Action::args`])。
    pub fn arg(mut self, a: Arg) -> Self {
        self.args.push(a);
        self
    }

    /// 引数 `a` をまとめて受け取る([`Action::args`])。
    pub fn args(mut self, a: impl IntoIterator<Item = Arg>) -> Self {
        self.args.extend(a);
        self
    }

    /// 操作 `id` のすぐ前に並べる([`Action::before`])。
    pub fn before(mut self, id: &str) -> Self {
        self.before = Some(id.into());
        self
    }

    /// 作っている途中・直している途中のまま始める([`Action::keep_edit`])。
    pub fn keep_edit(mut self) -> Self {
        self.keep_edit = true;
        self
    }

    /// 機能を足したすぐ後に、続けて操作 `id` を始める([`Action::then`])。
    pub fn then(mut self, id: &str) -> Self {
        self.then = Some(id.into());
        self
    }

    /// 実行した後、同じ操作をもう一度始める([`Action::repeat`])。
    pub fn repeat(mut self) -> Self {
        self.repeat = true;
        self
    }

    pub fn aliases(mut self, a: &str) -> Self {
        self.aliases = a.into();
        self
    }

    pub fn hint(mut self, h: &str) -> Self {
        self.hint = h.into();
        self
    }

    /// ツールバーのまとまり `m` に入れる。[`Place::Toolbar`] もあれば、初めから前に出す。
    pub fn menu(mut self, m: &str) -> Self {
        self.menu = m.into();
        self
    }

    /// アイコンの名前。
    pub fn icon(mut self, i: &str) -> Self {
        self.icon = i.into();
        self
    }

    pub fn at(mut self, places: &[Place]) -> Self {
        self.places.extend_from_slice(places);
        self
    }

    /// 実行の前に求める物。[`Input::Name`] なら、画面が渡す引数 `name`・`selection` も受け取る。
    pub fn input(mut self, i: Input) -> Self {
        if let Input::Name { label, .. } = &i {
            self.args.extend([
                Arg::text("name", label),
                Arg::list("selection", "選んでいる機能の番号の並び(タイムラインの順)"),
            ]);
        }
        self.input = i;
        self
    }

    /// `why` が `Some` なら使えない操作にする。
    pub fn disabled_if(mut self, why: Option<&str>) -> Self {
        self.disabled = why.map(String::from);
        self
    }

    pub fn is_at(&self, place: Place) -> bool {
        self.places.contains(&place)
    }
}
