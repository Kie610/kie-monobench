//! 役割の印の語彙。画面・CLI・MCP は機能の名前を書かず、この印で
//! 操作・機能の種類・設定を探す。コアは印の型([`jc_core::ActionRole`]・[`jc_core::KindRole`]・
//! [`jc_core::SettingsRole`])と探し方だけを持ち、どんな役割があるかは知らない。
//!
//! 印を名乗るのはモジュール(`Action::role`・`Descriptor::role`・`Settings::role`)。探される印は
//! 組み立ての中でちょうど 1 つの物が名乗る(jc-bundle の tests/ids.rs。[`MARKERS`] は例外)。

use jc_core::{ActionRole, KindRole, SettingsRole};

/// 操作の役割。
pub mod action {
    use super::ActionRole;

    /// 押し出し。プレス/プルで、平らな面とスケッチの形に使う。引数 `faces`(面の参照の並び)を
    /// 渡すと、その面を一緒に押し出せない時は使えない理由を返す
    pub const EXTRUDE: ActionRole = ActionRole("extrude");
    /// 選んだ辺を丸める。プレス/プルで、辺に使う
    pub const FILLET_EDGES: ActionRole = ActionRole("fillet_edges");
    /// 面に穴を開ける(画面は H のキーで始める)
    pub const HOLE_ON_FACE: ActionRole = ActionRole("hole_on_face");
    /// 見えているボディをファイルに書き出す。画面は目を閉じて隠したボディ(引数 `exclude`)と
    /// 今の視点(引数 `view`)を渡す。いくつの操作に付いてもよい([`super::MARKERS`])
    pub const EXPORT_BODIES: ActionRole = ActionRole("export_bodies");
    /// 保存したテンプレートから作る。テンプレートの名前は `Action::role_name`(画面は 1 つの一覧に
    /// 名前で出す)。テンプレートごとに付く([`super::MARKERS`])
    pub const USE_TEMPLATE: ActionRole = ActionRole("use_template");
    /// STEP のファイルを読み込む(CLI・MCP の短い命令が使う)
    pub const IMPORT_STEP: ActionRole = ActionRole("import_step");
    /// タイムラインのロールバックの位置を動かす。引数 `position`(先頭から効かせる機能の数。省くと
    /// 最後まで進める)。CLI・MCP の短い命令が使う
    pub const ROLL_BACK: ActionRole = ActionRole("roll_back");
    /// 選んだセットアップの G-code を書き出す(CLI・MCP の短い命令が使う)
    pub const EXPORT_GCODE: ActionRole = ActionRole("export_gcode");
    /// 選んだセットアップの段取り表を書き出す(CLI の短い命令が使う)
    pub const SETUP_SHEET: ActionRole = ActionRole("setup_sheet");
    /// 選んだセットアップを削るシミュレーション(CLI・MCP の短い命令が使う)
    pub const SIMULATE: ActionRole = ActionRole("simulate");
    /// 別のファイルのデザインを、今の文書へリンクで挿入する(ファイルを選ばせる)。画面は
    /// データパネルの「現在のデザインに挿入」と、データパネルから 3D のビューへ引いて離した時に、
    /// そのファイルを渡して呼ぶ
    pub const INSERT_LINK: ActionRole = ActionRole("insert_link");
    /// DXF のファイルを、選んでいるスケッチへ挿入する(スケッチの編集器の「挿入」が使う)
    pub const INSERT_DXF: ActionRole = ActionRole("insert_dxf");
    /// SVG のファイルを、選んでいるスケッチへ挿入する(スケッチの編集器の「挿入」が使う)
    pub const INSERT_SVG: ActionRole = ActionRole("insert_svg");
    /// 選んでいるスケッチを DXF で保存する(スケッチの編集器の右クリックが使う)
    pub const EXPORT_SKETCH_DXF: ActionRole = ActionRole("export_sketch_dxf");
    /// 選んでいるスケッチの点に文字を置く(スケッチの編集器の「文字」が使う。引数 `point` に
    /// 基準にする点の番号)
    pub const TEXT_AT_POINT: ActionRole = ActionRole("text_at_point");
    /// 自動分解(アニメーション)。画面はすぐ実行せず、ビューの上に小さな道具(ワンステップ/
    /// 順次・トレイルライン・分解の目盛)を出し、引数 `sequential`(真偽)・`trail`(真偽)・`scale`
    /// (数)を変えるたびに実行し直して先に見せ、OK で文書に入れる([`super::MARKERS`])
    pub const AUTO_EXPLODE: ActionRole = ActionRole("auto_explode");
    /// ビューの段を足す(アニメーション)。画面は引数 `view`({target, yaw, pitch, height,
    /// perspective})に 3D のビューの今の視点を渡す。段の帯の「記録」を入れている間は、視点を
    /// 動かして止めるたびに画面がこの操作を呼ぶ
    pub const ANIMATION_VIEW: ActionRole = ActionRole("animation_view");
    /// モーション スタディ(設計)。画面はすぐ実行せず、スタディの窓を出す。窓のキーを置く・
    /// 消すなどもこの操作で行う。引数は `study`(名前)・`joint`・`step`・`value`・`remove`・`drop`・
    /// `seconds`・`rename`・`delete`([`crate::motion`])
    pub const MOTION_STUDY: ActionRole = ActionRole("motion_study");
    /// 外観(設計の修正・レンダリングの設定)。画面はすぐ実行せず、外観の窓を出す。窓で付ける・
    /// 外す・直す・複製する・消すもこの操作で行う。引数は `apply`・`remove`・`target`・`bodies`・
    /// `components`・`faces`・`edit`・`rename`・`color`・`metallic`・`roughness`・`duplicate`・`delete`
    pub const APPEARANCE: ActionRole = ActionRole("appearance");
    /// 新しい部品(組立)。引数 `selection`(ボディの機能の番号の並び。空なら空の部品)と `parent`
    /// (入れる親の部品の番号。省くと、選んだボディがみな同じ部品に入っていればその中)。画面の
    /// ボディの右クリックの「新しい部品にする」と、部品の右クリックの「中に新しい部品」が使う
    pub const NEW_COMPONENT: ActionRole = ActionRole("new_component");
    /// 部品を値で動かす(位置の機能を足す)。引数 `bodies`(省くと選んでいるボディ)。画面の M キーが、
    /// アクティブでない部品のボディを選んでいる時に使う
    pub const MOVE_COMPONENT: ActionRole = ActionRole("move_component");

    /// 全部(名前の重なりの試験に使う)。
    pub const ALL: &[ActionRole] = &[
        EXTRUDE,
        FILLET_EDGES,
        HOLE_ON_FACE,
        EXPORT_BODIES,
        USE_TEMPLATE,
        IMPORT_STEP,
        ROLL_BACK,
        EXPORT_GCODE,
        SETUP_SHEET,
        SIMULATE,
        INSERT_LINK,
        INSERT_DXF,
        INSERT_SVG,
        EXPORT_SKETCH_DXF,
        TEXT_AT_POINT,
        AUTO_EXPLODE,
        ANIMATION_VIEW,
        MOTION_STUDY,
        APPEARANCE,
        NEW_COMPONENT,
        MOVE_COMPONENT,
    ];
}

/// 機能の種類の役割。
pub mod kind {
    use super::KindRole;

    /// 面を作る機能。プレス/プルでその面を選ぶと、この機能を直す
    pub const EDIT_FROM_FACE: KindRole = KindRole("edit_from_face");
    /// 選んだ面を法線の向きへ動かす(面のオフセット)。プレス/プルで、押し出せない曲がった面に使う
    pub const OFFSET_FACES: KindRole = KindRole("offset_faces");
    /// ボディを写す。データ `{"body": 写すボディ(今の最後の機能), "copy": true}` で、元を残して
    /// 同じ位置に写しを新しいボディにする(元の今の形を読むので、元を直すと写しも変わる)。
    /// 画面の部品の複製が使う
    pub const COPY_BODY: KindRole = KindRole("copy_body");
    /// 部品の位置。データ `{"moving": {"kind": "component", "id": 部品}, "bodies": [動かすボディ],
    /// "x": 式, "y": 式, "z": 式}`(引く前の位置からの距離)。画面の部品の複製が、写しを横へ
    /// ずらすのに使う
    pub const COMPONENT_POSITION: KindRole = KindRole("component_position");
    /// 面に絵を貼るだけで形を変えない(デカール)。静止画の書き出しは、この種類の誤りだけなら
    /// その分を除いて描く
    pub const DECAL: KindRole = KindRole("decal");

    /// 全部(名前の重なりの試験に使う)。
    pub const ALL: &[KindRole] = &[
        EDIT_FROM_FACE,
        OFFSET_FACES,
        COPY_BODY,
        COMPONENT_POSITION,
        DECAL,
    ];
}

/// 設定の役割。
pub mod settings {
    use super::SettingsRole;

    /// 機械。画面は説明の「機械: 名前」の行から名前を出す
    pub const MACHINE: SettingsRole = SettingsRole("machine");
    /// レンダリングのシーン(値の `env` は環境の名前)。画面はレンダリングの設定を開く所と、
    /// 描く環境を決める所で使う
    pub const RENDER_SCENE: SettingsRole = SettingsRole("render_scene");

    /// 全部(名前の重なりの試験に使う)。
    pub const ALL: &[SettingsRole] = &[MACHINE, RENDER_SCENE];
}

/// 画面が 1 つを探す役割でなく、操作の振る舞いを示す印(いくつの操作に付いてもよい)。
/// 画面は `a.id == id && a.role == …` の形で確かめる。
pub const MARKERS: &[ActionRole] = &[
    action::USE_TEMPLATE,
    action::EXPORT_BODIES,
    action::AUTO_EXPLODE,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn role_names_are_distinct_within_each_vocabulary() {
        let a: BTreeSet<_> = action::ALL.iter().map(|r| r.0).collect();
        assert_eq!(a.len(), action::ALL.len());
        let k: BTreeSet<_> = kind::ALL.iter().map(|r| r.0).collect();
        assert_eq!(k.len(), kind::ALL.len());
        let s: BTreeSet<_> = settings::ALL.iter().map(|r| r.0).collect();
        assert_eq!(s.len(), settings::ALL.len());
        assert!(MARKERS.iter().all(|m| action::ALL.contains(m)));
    }
}
