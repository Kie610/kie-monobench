//! 画面の部品が描く、意味の名前を付けた色([`Inks`])と、今の大きさの倍率([`Scale`])。
//!
//! 色は画面のテーマ(jc-ui の `theme`)が明暗ごとに持ち、3D のビュー・スケッチ・工具の絵に渡す。
//! 名前はテーマのファイル(データフォルダの `themes/*.json`)の鍵と同じ([`Inks::each_mut`])。
//!
//! 大きさの倍率(字・アイコン・角の丸み・動き)は、画面を描く糸ごとに 1 つ持つ。テーマが毎フレーム
//! [`set_scale`] で入れ、部品は [`fs`]・[`prop`]・[`icon`]・[`corner`] で直書きの大きさに掛ける。
//! 既定は全部 1 倍で、その時は今までと同じ大きさになる。

use egui::{Color32, FontId};
use std::cell::Cell;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

const fn gray(v: u8) -> Color32 {
    Color32::from_gray(v)
}

/// 意味の名前を付けた色の一揃い(明るい画面と暗い画面で 1 つずつ)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inks {
    /// ボディの面(色味のない白・灰色。スケッチの水色、選んだ色と見分ける)
    pub body: Color32,
    /// 軸(X 赤・Y 緑・Z 青)。軸の玉、原点の軸、格子の軸、測る道具の成分
    pub axis: [Color32; 3],
    /// 軸の玉の文字
    pub axis_text: Color32,
    /// セットアップの原点の軸
    pub origin_axis: [Color32; 3],
    /// ビューの左下の軸の印
    pub axis_mark: [Color32; 3],
    /// 道具の立体: 結合・切り取り・交差の面、その稜線と線・札の色、押し出す輪郭
    /// (新しいボディはボディと同じ灰色で描く)
    pub ghost_join: Color32,
    pub ghost_join_edge: Color32,
    pub ghost_cut: Color32,
    pub ghost_cut_edge: Color32,
    pub ghost_intersect: Color32,
    pub ghost_intersect_edge: Color32,
    pub profile: Color32,
    /// 3D のビューのスケッチの線
    pub sketch_line: Color32,
    /// 経路の入り(らせん・斜め・真下)とリード
    pub path_entry: Color32,
    pub path_lead: Color32,
    /// シミュレーションの印: 食い込み・削り残し
    pub gouge: Color32,
    pub leftover: Color32,
    /// スケッチのツールバーのまとまり: 描く・直す・拘束
    pub sketch_draw: Color32,
    pub sketch_modify: Color32,
    pub sketch_constraint: Color32,
    /// スケッチの要素: 選んだ・まだ動ける・写した・ボディから取り込んだ
    pub sketch_pick: Color32,
    pub sketch_free: Color32,
    pub sketch_ref: Color32,
    pub sketch_linked: Color32,
    /// スケッチ: ボディの断面(スライス・交差の道具)、注意の字、完全拘束の字
    pub sketch_section: Color32,
    pub sketch_note: Color32,
    pub sketch_solved: Color32,
    /// 加工の再生: 操作ごとの色(順に使い回す)
    pub op: [Color32; 8],
    /// 加工の再生の比較: 許容内・削り不足・削り過ぎ
    pub deviation: [Color32; 3],
    /// 加工の再生の工具: 刃・シャンク・ホルダー
    pub tool_parts: [Color32; 3],
    /// 工具の絵: 刃と首、シャンク
    pub tool_flute: Color32,
    pub tool_shank: Color32,
    /// タイムラインのグループ(番号で回す)
    pub group: [Color32; 3],
    /// 範囲で選ぶ枠: 左から右(全部入った物。実線)・右から左(かかった物。破線)
    pub select_window: Color32,
    pub select_crossing: Color32,
    /// 加工の高さの面: 安全高さ・送り高さ・トップ高さ・ボトム高さ
    pub heights: [Color32; 4],
}

const AXIS: [Color32; 3] = [rgb(220, 80, 80), rgb(70, 180, 80), rgb(80, 125, 230)];
const ORIGIN_AXIS: [Color32; 3] = [rgb(230, 70, 70), rgb(70, 200, 70), rgb(80, 120, 240)];
const AXIS_MARK: [Color32; 3] = [rgb(220, 80, 80), rgb(80, 190, 80), rgb(90, 130, 230)];
const OP: [Color32; 8] = [
    rgb(66, 133, 244),
    rgb(234, 120, 50),
    rgb(52, 168, 83),
    rgb(171, 71, 188),
    rgb(0, 172, 193),
    rgb(230, 180, 20),
    rgb(233, 80, 110),
    rgb(120, 144, 156),
];
const DEVIATION: [Color32; 3] = [rgb(70, 180, 90), rgb(60, 120, 225), rgb(225, 50, 50)];
/// 加工の高さの面(安全は緑、送りは黄土、トップは青、ボトムは赤紫)
const HEIGHTS: [Color32; 4] = [
    rgb(60, 170, 90),
    rgb(215, 160, 30),
    rgb(60, 120, 220),
    rgb(200, 60, 150),
];
/// 外観の無いボディの既定の色(jc_look::material の鋼の色と同じ)。
/// 明るい画面・暗い画面で同じ。
const STEEL: Color32 = rgb(160, 160, 160);
const TOOL_PARTS: [Color32; 3] = [rgb(215, 215, 225), rgb(160, 160, 172), rgb(95, 95, 108)];

impl Inks {
    /// 明るい画面(今までの色)。
    pub const LIGHT: Inks = Inks {
        body: STEEL,
        axis: AXIS,
        axis_text: rgb(25, 25, 25),
        origin_axis: ORIGIN_AXIS,
        axis_mark: AXIS_MARK,
        ghost_join: crate::view3d::GHOST_JOIN,
        ghost_join_edge: crate::view3d::GHOST_JOIN_EDGE,
        ghost_cut: crate::view3d::GHOST_CUT,
        ghost_cut_edge: crate::view3d::GHOST_CUT_EDGE,
        ghost_intersect: crate::view3d::GHOST_INTERSECT,
        ghost_intersect_edge: crate::view3d::GHOST_INTERSECT_EDGE,
        profile: crate::view3d::PROFILE_BLUE,
        sketch_line: crate::view3d::sketch_ink(false),
        path_entry: crate::view3d::path_ink(false).0,
        path_lead: crate::view3d::path_ink(false).1,
        gouge: rgb(235, 40, 40),
        leftover: rgb(170, 80, 230),
        sketch_draw: rgb(38, 138, 206),
        sketch_modify: rgb(52, 150, 84),
        sketch_constraint: rgb(196, 66, 128),
        sketch_pick: rgb(222, 110, 18),
        sketch_free: rgb(44, 108, 196),
        sketch_ref: rgb(132, 64, 176),
        sketch_linked: rgb(150, 90, 210),
        sketch_section: rgb(230, 110, 20),
        sketch_note: rgb(190, 120, 20),
        sketch_solved: rgb(40, 140, 70),
        op: OP,
        deviation: DEVIATION,
        tool_parts: TOOL_PARTS,
        tool_flute: rgb(214, 170, 60),
        tool_shank: gray(150),
        group: [
            rgb(0x58, 0x66, 0xb8),
            rgb(0x9a, 0x4f, 0x8c),
            rgb(0x6d, 0x7d, 0x2c),
        ],
        select_window: rgb(40, 110, 220),
        select_crossing: rgb(30, 150, 70),
        heights: HEIGHTS,
    };

    /// 暗い画面(今までの色)。
    pub const DARK: Inks = Inks {
        body: STEEL,
        sketch_line: crate::view3d::sketch_ink(true),
        path_entry: crate::view3d::path_ink(true).0,
        path_lead: crate::view3d::path_ink(true).1,
        sketch_pick: rgb(255, 166, 77),
        sketch_free: rgb(112, 162, 232),
        sketch_ref: rgb(190, 140, 230),
        group: [
            rgb(0x8c, 0x98, 0xe0),
            rgb(0xcf, 0x86, 0xc0),
            rgb(0xa8, 0xba, 0x5c),
        ],
        select_window: rgb(110, 160, 245),
        select_crossing: rgb(100, 210, 120),
        // 暗い地では線と札の字が沈まないよう明るくする
        ghost_join_edge: rgb(110, 170, 255),
        ghost_cut_edge: rgb(255, 95, 95),
        ..Inks::LIGHT
    };

    /// 色ごとに、名前(テーマのファイルの鍵)と色を渡す。列の色は `axis_x`・`op_1` のように番号を付ける。
    pub fn each_mut(&mut self, f: &mut dyn FnMut(&str, &mut Color32)) {
        const XYZ: [&str; 3] = ["x", "y", "z"];
        fn list(
            f: &mut dyn FnMut(&str, &mut Color32),
            name: &str,
            cs: &mut [Color32],
            tags: &[&str],
        ) {
            for (c, t) in cs.iter_mut().zip(tags) {
                f(&format!("{name}_{t}"), c);
            }
        }
        f("body", &mut self.body);
        list(f, "axis", &mut self.axis, &XYZ);
        f("axis_text", &mut self.axis_text);
        list(f, "origin_axis", &mut self.origin_axis, &XYZ);
        list(f, "axis_mark", &mut self.axis_mark, &XYZ);
        f("ghost_join", &mut self.ghost_join);
        f("ghost_join_edge", &mut self.ghost_join_edge);
        f("ghost_cut", &mut self.ghost_cut);
        f("ghost_cut_edge", &mut self.ghost_cut_edge);
        f("ghost_intersect", &mut self.ghost_intersect);
        f("ghost_intersect_edge", &mut self.ghost_intersect_edge);
        f("profile", &mut self.profile);
        f("sketch_line", &mut self.sketch_line);
        f("path_entry", &mut self.path_entry);
        f("path_lead", &mut self.path_lead);
        f("gouge", &mut self.gouge);
        f("leftover", &mut self.leftover);
        f("sketch_draw", &mut self.sketch_draw);
        f("sketch_modify", &mut self.sketch_modify);
        f("sketch_constraint", &mut self.sketch_constraint);
        f("sketch_pick", &mut self.sketch_pick);
        f("sketch_free", &mut self.sketch_free);
        f("sketch_ref", &mut self.sketch_ref);
        f("sketch_linked", &mut self.sketch_linked);
        f("sketch_section", &mut self.sketch_section);
        f("sketch_note", &mut self.sketch_note);
        f("sketch_solved", &mut self.sketch_solved);
        list(
            f,
            "op",
            &mut self.op,
            &["1", "2", "3", "4", "5", "6", "7", "8"],
        );
        list(
            f,
            "deviation",
            &mut self.deviation,
            &["ok", "under", "over"],
        );
        list(
            f,
            "play_tool",
            &mut self.tool_parts,
            &["cutter", "shank", "holder"],
        );
        f("tool_flute", &mut self.tool_flute);
        f("tool_shank", &mut self.tool_shank);
        list(f, "group", &mut self.group, &["1", "2", "3"]);
        f("select_window", &mut self.select_window);
        f("select_crossing", &mut self.select_crossing);
        list(
            f,
            "height",
            &mut self.heights,
            &["safe", "feed", "top", "bottom"],
        );
    }

    /// モジュールが渡す面の色の役を、色にする。知らない役は操作の最初の色。
    pub fn shape(&self, ink: jc_core::ShapeInk) -> Color32 {
        use jc_core::ShapeInk as S;
        use jc_interchange::ink::shape;
        match ink {
            S::Operation(n) => self.op[n as usize % self.op.len()],
            shape::WITHIN => self.deviation[0],
            shape::UNDER => self.deviation[1],
            shape::OVER => self.deviation[2],
            shape::CUTTER => self.tool_parts[0],
            shape::SHANK => self.tool_parts[1],
            shape::HOLDER => self.tool_parts[2],
            S::Named(_) => self.op[0],
            S::Rgb([r, g, b]) => rgb(r, g, b),
        }
    }

    /// 半透明の平面(加工の高さ)の色の役を、色にする。知らない役は安全高さの色。
    pub fn height(&self, ink: jc_core::PlaneInk) -> Color32 {
        use jc_interchange::ink::plane;
        match ink {
            plane::FEED => self.heights[1],
            plane::TOP => self.heights[2],
            plane::BOTTOM => self.heights[3],
            _ => self.heights[0],
        }
    }
}

fn inks_id() -> egui::Id {
    egui::Id::new("jc_inks")
}

/// 今の画面の色(テーマがまだ決めていなければ明るい画面の色)。
pub fn inks(ctx: &egui::Context) -> Inks {
    ctx.data(|d| d.get_temp::<Inks>(inks_id()))
        .unwrap_or(Inks::LIGHT)
}

/// 今の画面の色を覚える(テーマが毎フレーム呼ぶ)。画面を描く糸にも置く([`inks_now`])。
pub fn set_inks(ctx: &egui::Context, inks: Inks) {
    INKS.with(|c| c.set(inks));
    if ctx.data(|d| d.get_temp::<Inks>(inks_id())) != Some(inks) {
        ctx.data_mut(|d| d.insert_temp(inks_id(), inks));
    }
}

/// 今の画面の色(`egui::Context` の無い所で使う。テーマがこの糸で最後に入れた色)。
pub fn inks_now() -> Inks {
    INKS.with(|c| c.get())
}

/// 大きさの倍率(設定の「画面」)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// 字の大きさ(部品の決まった寸法のうち、字に合わせる物にも掛ける)
    pub text: f32,
    /// アイコンの大きさ
    pub icon: f32,
    /// 角の丸み(0 なら四角)
    pub corner: f32,
    /// 動きの時間(0 なら動かさない)
    pub motion: f32,
}

impl Scale {
    pub const ONE: Scale = Scale {
        text: 1.0,
        icon: 1.0,
        corner: 1.0,
        motion: 1.0,
    };
}

impl Default for Scale {
    fn default() -> Self {
        Scale::ONE
    }
}

thread_local! {
    static SCALE: Cell<Scale> = const { Cell::new(Scale::ONE) };
    static INKS: Cell<Inks> = const { Cell::new(Inks::LIGHT) };
}

/// 今の倍率を入れる(テーマが毎フレーム呼ぶ。画面を描く糸ごと)。
pub fn set_scale(s: Scale) {
    SCALE.with(|c| c.set(s));
}

/// 今の倍率。
pub fn scale() -> Scale {
    SCALE.with(|c| c.get())
}

/// 字の大きさ `px` に字の倍率を掛ける。
pub fn fs(px: f32) -> f32 {
    px * scale().text
}

/// 字の倍率を掛けた文章の字。
pub fn prop(px: f32) -> FontId {
    FontId::proportional(fs(px))
}

/// 字の倍率を掛けた等幅の字。
pub fn mono(px: f32) -> FontId {
    FontId::monospace(fs(px))
}

/// アイコンの大きさ `px` にアイコンの倍率を掛ける。
pub fn icon(px: f32) -> f32 {
    px * scale().icon
}

/// 角の丸み `r` に角の倍率を掛ける。
pub fn corner(r: f32) -> f32 {
    r * scale().corner
}

/// 動きの時間 `secs` に動きの倍率を掛ける。
pub fn motion(secs: f64) -> f64 {
    secs * scale().motion as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_body_color_is_the_default_material() {
        // 材料を付けていないボディは既定の材料(鋼)として質量を出すので、色も鋼の色にそろえる
        let m = jc_look::material::phys_material(jc_look::material::DEFAULT_MATERIAL).unwrap();
        let c = rgb(m.color[0], m.color[1], m.color[2]);
        assert_eq!((Inks::LIGHT.body, Inks::DARK.body), (c, c));
    }

    #[test]
    fn every_ink_has_a_unique_name() {
        let mut names = vec![];
        Inks::LIGHT
            .clone()
            .each_mut(&mut |k, _| names.push(k.to_string()));
        let set: std::collections::HashSet<&String> = names.iter().collect();
        assert_eq!(set.len(), names.len());
        assert!(names.contains(&"axis_x".to_string()) && names.contains(&"op_8".to_string()));
    }

    #[test]
    fn module_roles_become_theme_colors() {
        use jc_core::ShapeInk as S;
        use jc_interchange::ink::{plane, shape};
        let i = Inks::LIGHT;
        assert_eq!(i.shape(S::Operation(9)), i.op[1], "操作の色は数で回す");
        assert_eq!(i.shape(shape::OVER), i.deviation[2]);
        assert_eq!(i.shape(shape::HOLDER), i.tool_parts[2]);
        assert_eq!(i.shape(S::Rgb([1, 2, 3])), Color32::from_rgb(1, 2, 3));
        // 受け渡しの型にある役は、どれも別の色になる(知らない役の色に落ちない)
        let named: std::collections::HashSet<Color32> =
            shape::ALL.iter().map(|s| i.shape(*s)).collect();
        assert_eq!(named.len(), shape::ALL.len());
        let heights: Vec<Color32> = plane::ALL.iter().map(|p| i.height(*p)).collect();
        assert_eq!(heights, i.heights.to_vec());
    }

    #[test]
    fn scale_multiplies_sizes_and_motion_off_finishes_turns_at_once() {
        assert_eq!((fs(12.5), icon(18.0), corner(4.0)), (12.5, 18.0, 4.0));
        set_scale(Scale {
            text: 1.2,
            icon: 1.5,
            corner: 0.0,
            motion: 0.0,
        });
        assert!((fs(10.0) - 12.0).abs() < 1e-5);
        assert_eq!((icon(10.0), corner(4.0)), (15.0, 0.0));
        let mut cam = crate::view3d::Camera::default();
        cam.turn_to(90.0, 0.0);
        assert!(!cam.step_turn(5.0), "動きを止めると 1 回で行き先に着く");
        assert!(cam.is_view(90.0, 0.0));
        set_scale(Scale::ONE);
        cam.turn_to(0.0, 0.0);
        assert!(cam.step_turn(5.0), "普通は途中");
    }
}
