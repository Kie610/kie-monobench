//! 画面の色と、よく使う見た目の部品(アイコンのボタン、札、キーの印、区切りの見出し)。
//!
//! 色は画面の試作の値に合わせる。中立の灰に、作業空間ごとの色
//! (設計は青緑、加工は銅色)を差す。明るい色と暗い色の両方を持ち、PC の明暗の設定に従う。
//!
//! テーマ([`Theme`])は、明暗 2 組の色と、大きさ・角の丸み・詰め具合・動きの倍率を持つ。同梱の
//! テーマ(標準・ハイコントラスト)と、利用者のテーマ(データフォルダの `themes/*.json`、
//! [`crate::themes`])から設定の「画面」で選ぶ。既定は標準(倍率は全部 1)。

use crate::Space;
use crate::icons;
use egui::{Color32, CornerRadius, FontId, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};
pub use kmb_ui_view3d::{Inks, Scale, corner, fs, icon as icon_px, inks_now, mono, prop, scale};

/// 色の一揃い(試作の CSS の変数と同じ名前)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub chrome: Color32,
    pub chrome2: Color32,
    pub panel: Color32,
    pub paper: Color32,
    pub edge: Color32,
    pub ink: Color32,
    pub ink2: Color32,
    pub ink3: Color32,
    pub canvas: Color32,
    pub grid: Color32,
    pub design: Color32,
    pub design_soft: Color32,
    pub cam: Color32,
    pub cam_soft: Color32,
    /// レンダリングの作業空間の色(紫)
    pub render: Color32,
    pub render_soft: Color32,
    /// アニメーションの作業空間の色(こがね色)
    pub anim: Color32,
    pub anim_soft: Color32,
    /// 解析の作業空間の色と、その淡い色
    pub sim: Color32,
    pub sim_soft: Color32,
    /// スケッチを直している間の差し色(スケッチの線と同じ水色の系統。設計の青緑と見分ける)
    pub sketch: Color32,
    pub sketch_soft: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    pub sel: Color32,
    /// 道具のアイコンの土台の線(灰)。調べる・出すの道具は全部この色
    pub icon_base: Color32,
    /// 役の色: 道具のアイコンの変わる部分だけに差す(見出し・縁には使わない)。作る(形・経路を生む)
    pub role_make: Color32,
    /// 役の色: 変える(今ある物を直す)
    pub role_modify: Color32,
    /// 役の色: 増やす(写す・並べる)
    pub role_repeat: Color32,
    /// 役の色: 決める(関係・条件を決める)
    pub role_define: Color32,
    /// 形の面(明るい所・影)
    pub shade1: Color32,
    pub shade2: Color32,
    pub shade3: Color32,
    pub bed: Color32,
    pub hole: Color32,
    pub dark: bool,
    /// 意味の名前を付けた色(軸・道具の立体・スケッチ・加工の再生など)
    pub inks: Inks,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const LIGHT: Palette = Palette {
    chrome: hex(0xf7f8f7),
    chrome2: hex(0xeceeed),
    panel: hex(0xfbfcfb),
    paper: hex(0xffffff),
    edge: hex(0xcfd6d4),
    ink: hex(0x1f2725),
    ink2: hex(0x66726f),
    ink3: hex(0x9aa4a2),
    canvas: hex(0xe6eae9),
    grid: hex(0xd3d9d7),
    design: hex(0x17766a),
    design_soft: hex(0xd9eeea),
    cam: hex(0xa65a2c),
    cam_soft: hex(0xf5e4d8),
    render: hex(0x6a4fb0),
    render_soft: hex(0xe8e2f5),
    anim: hex(0x9a6a00),
    anim_soft: hex(0xf6ecd2),
    sim: hex(0x6a4fa6),
    sim_soft: hex(0xe8e1f4),
    sketch: hex(0x1f7bbd),
    sketch_soft: hex(0xdcebf7),
    ok: hex(0x2f7d3c),
    warn: hex(0x9a6a00),
    err: hex(0xb3352b),
    sel: hex(0xc7e6e0),
    icon_base: hex(0x7b8784),
    role_make: hex(0x2b8ae6),
    role_modify: hex(0xde7008),
    role_repeat: hex(0xd4489a),
    role_define: hex(0x0f9a7c),
    shade1: hex(0xc9d0ce),
    shade2: hex(0xb3bbb9),
    shade3: hex(0xdfe4e2),
    bed: hex(0xcfd4d6),
    hole: hex(0x7d878b),
    dark: false,
    inks: Inks::LIGHT,
};

pub const DARK: Palette = Palette {
    chrome: hex(0x1d2423),
    chrome2: hex(0x232b2a),
    panel: hex(0x1a2120),
    paper: hex(0x1a201f),
    edge: hex(0x34403e),
    ink: hex(0xe2e8e6),
    ink2: hex(0xa3b0ad),
    ink3: hex(0x6f7c79),
    canvas: hex(0x2a3231),
    grid: hex(0x354140),
    design: hex(0x4fc0ae),
    design_soft: hex(0x1c3934),
    cam: hex(0xe0915f),
    cam_soft: hex(0x3d2b20),
    render: hex(0xb39ddb),
    render_soft: hex(0x2e2640),
    anim: hex(0xe0b450),
    anim_soft: hex(0x3a2f14),
    sim: hex(0xb39ae8),
    sim_soft: hex(0x2e2640),
    sketch: hex(0x5fb2ea),
    sketch_soft: hex(0x1b3243),
    ok: hex(0x6cc77a),
    warn: hex(0xe2b04a),
    err: hex(0xef7a6e),
    sel: hex(0x24504a),
    icon_base: hex(0xa3b0ad),
    role_make: hex(0x5cb6ee),
    role_modify: hex(0xf0a54a),
    role_repeat: hex(0xe391c3),
    role_define: hex(0x45c9a8),
    shade1: hex(0x56625f),
    shade2: hex(0x46514f),
    shade3: hex(0x66726f),
    bed: hex(0x3b4446),
    hole: hex(0x171c1d),
    dark: true,
    inks: Inks::DARK,
};

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// ハイコントラスト(明るい): 白い地に黒い字と線。差し色は濃くして白の上で 4.5:1 以上にする。
pub const HC_LIGHT: Palette = Palette {
    chrome: hex(0xffffff),
    chrome2: hex(0xebebeb),
    panel: hex(0xffffff),
    paper: hex(0xffffff),
    edge: hex(0x1a1a1a),
    ink: hex(0x000000),
    ink2: hex(0x1f1f1f),
    ink3: hex(0x4d4d4d),
    canvas: hex(0xf4f4f4),
    grid: hex(0xa6a6a6),
    design: hex(0x00594d),
    design_soft: hex(0xc6ece5),
    cam: hex(0x8a3a00),
    cam_soft: hex(0xf6d9c4),
    render: hex(0x4b2a99),
    render_soft: hex(0xe4dcf6),
    anim: hex(0x6e4a00),
    anim_soft: hex(0xf3e6c4),
    sim: hex(0x4b2a99),
    sim_soft: hex(0xe0d6f5),
    sketch: hex(0x004c99),
    sketch_soft: hex(0xcfe3f7),
    ok: hex(0x005c12),
    warn: hex(0x6e4700),
    err: hex(0xa80000),
    sel: hex(0x9ad6ca),
    icon_base: hex(0x1f1f1f),
    role_make: hex(0x0050b3),
    role_modify: hex(0xa34700),
    role_repeat: hex(0xa3006b),
    role_define: hex(0x00705a),
    shade1: hex(0xa3a3a3),
    shade2: hex(0x8a8a8a),
    shade3: hex(0xc4c4c4),
    bed: hex(0xbdbdbd),
    hole: hex(0x4a4a4a),
    dark: false,
    inks: Inks {
        body: Color32::from_gray(214),
        axis: [rgb(200, 0, 0), rgb(0, 135, 0), rgb(0, 70, 215)],
        axis_text: rgb(0, 0, 0),
        origin_axis: [rgb(200, 0, 0), rgb(0, 135, 0), rgb(0, 70, 215)],
        axis_mark: [rgb(200, 0, 0), rgb(0, 135, 0), rgb(0, 70, 215)],
        sketch_line: rgb(0, 98, 185),
        path_entry: rgb(160, 0, 140),
        path_lead: rgb(150, 95, 0),
        gouge: rgb(205, 0, 0),
        leftover: rgb(120, 0, 205),
        sketch_draw: rgb(0, 95, 180),
        sketch_modify: rgb(0, 115, 45),
        sketch_constraint: rgb(170, 0, 95),
        sketch_pick: rgb(190, 80, 0),
        sketch_free: rgb(0, 75, 190),
        sketch_ref: rgb(110, 30, 160),
        sketch_linked: rgb(115, 45, 185),
        sketch_section: rgb(200, 85, 0),
        sketch_note: rgb(150, 85, 0),
        sketch_solved: rgb(0, 110, 40),
        group: [
            rgb(0x2a, 0x3a, 0x9c),
            rgb(0x84, 0x22, 0x72),
            rgb(0x4a, 0x5a, 0x00),
        ],
        ..Inks::LIGHT
    },
};

/// ハイコントラスト(暗い): 黒い地に白い字と明るい線。差し色は明るくして黒の上で 7:1 前後にする。
pub const HC_DARK: Palette = Palette {
    chrome: hex(0x000000),
    chrome2: hex(0x1a1a1a),
    panel: hex(0x050505),
    paper: hex(0x000000),
    edge: hex(0xbfbfbf),
    ink: hex(0xffffff),
    ink2: hex(0xebebeb),
    ink3: hex(0xb8b8b8),
    canvas: hex(0x0d0d0d),
    grid: hex(0x474747),
    design: hex(0x40f0d4),
    design_soft: hex(0x00382f),
    cam: hex(0xffa552),
    cam_soft: hex(0x452100),
    render: hex(0xc9b3ff),
    render_soft: hex(0x2b1d4d),
    anim: hex(0xffd27a),
    anim_soft: hex(0x3d2c08),
    sim: hex(0xd0b8ff),
    sim_soft: hex(0x2a1650),
    sketch: hex(0x66ccff),
    sketch_soft: hex(0x002f4d),
    ok: hex(0x66f27a),
    warn: hex(0xffd84d),
    err: hex(0xff7466),
    sel: hex(0x00594d),
    icon_base: hex(0xebebeb),
    role_make: hex(0x66b8ff),
    role_modify: hex(0xffb054),
    role_repeat: hex(0xff9be0),
    role_define: hex(0x40e0bd),
    shade1: hex(0x7a7a7a),
    shade2: hex(0x616161),
    shade3: hex(0x949494),
    bed: hex(0x3d3d3d),
    hole: hex(0x000000),
    dark: true,
    inks: Inks {
        body: Color32::from_gray(176),
        axis: [rgb(255, 95, 95), rgb(95, 235, 95), rgb(115, 165, 255)],
        axis_text: rgb(0, 0, 0),
        origin_axis: [rgb(255, 95, 95), rgb(95, 235, 95), rgb(115, 165, 255)],
        axis_mark: [rgb(255, 95, 95), rgb(95, 235, 95), rgb(115, 165, 255)],
        sketch_line: rgb(105, 215, 255),
        path_entry: rgb(255, 130, 235),
        path_lead: rgb(255, 210, 70),
        gouge: rgb(255, 60, 60),
        leftover: rgb(200, 130, 255),
        sketch_draw: rgb(90, 185, 255),
        sketch_modify: rgb(95, 225, 130),
        sketch_constraint: rgb(255, 115, 190),
        sketch_pick: rgb(255, 180, 80),
        sketch_free: rgb(135, 185, 255),
        sketch_ref: rgb(215, 165, 255),
        sketch_linked: rgb(195, 145, 255),
        sketch_section: rgb(255, 145, 45),
        sketch_note: rgb(255, 195, 65),
        sketch_solved: rgb(95, 225, 125),
        group: [
            rgb(0xa6, 0xb2, 0xff),
            rgb(0xf0, 0x9e, 0xe0),
            rgb(0xc8, 0xe0, 0x70),
        ],
        ..Inks::DARK
    },
};

/// 色覚の型に合わせたテーマ(U27)。赤と緑・青と黄の区別に頼らないよう、見分けたい色の組(状態、軸、
/// 加工の比較、道具の立体、スケッチの要素、操作の色、役の色)を置き換える。色は Okabe-Ito の 8 色
/// (カラーユニバーサルデザイン)を元に、明るい地では濃く、暗い地では明るくした。どの組も、色覚の型の
/// 見え方を真似た後(Machado ほか 2009 の行列)に色の差が 20(CIE76)以上あることを試験で確かめる。
const fn cvd(base: Palette, ok: u32, warn: u32, err: u32, c: &CvdInks) -> Palette {
    let mut p = base;
    p.ok = hex(ok);
    p.warn = hex(warn);
    p.err = hex(err);
    p.role_make = hex(c.roles[0]);
    p.role_modify = hex(c.roles[1]);
    p.role_repeat = hex(c.roles[2]);
    p.role_define = hex(c.roles[3]);
    let i = &mut p.inks;
    i.axis = [hex(c.axis[0]), hex(c.axis[1]), hex(c.axis[2])];
    i.origin_axis = i.axis;
    i.axis_mark = i.axis;
    i.deviation = [
        hex(c.deviation[0]),
        hex(c.deviation[1]),
        hex(c.deviation[2]),
    ];
    i.ghost_join = hex(c.join);
    i.ghost_join_edge = hex(c.join);
    i.ghost_cut = hex(c.cut);
    i.ghost_cut_edge = hex(c.cut);
    i.ghost_intersect = hex(c.intersect);
    i.ghost_intersect_edge = hex(c.intersect_edge);
    i.gouge = hex(c.gouge);
    i.leftover = hex(c.leftover);
    i.sketch_free = hex(c.free);
    i.sketch_ref = hex(c.reference);
    i.sketch_linked = hex(c.reference);
    i.sketch_pick = hex(c.pick);
    i.sketch_draw = hex(c.draw);
    i.sketch_modify = hex(c.modify);
    i.sketch_constraint = hex(c.constraint);
    i.group = [hex(c.group[0]), hex(c.group[1]), hex(c.group[2])];
    let mut k = 0;
    while k < 8 {
        i.op[k] = hex(c.op[k]);
        k += 1;
    }
    p
}

/// 色覚の型のテーマで置き換える意味の名前の色。
struct CvdInks {
    axis: [u32; 3],
    deviation: [u32; 3],
    join: u32,
    cut: u32,
    intersect: u32,
    intersect_edge: u32,
    gouge: u32,
    leftover: u32,
    free: u32,
    reference: u32,
    pick: u32,
    draw: u32,
    modify: u32,
    constraint: u32,
    group: [u32; 3],
    op: [u32; 8],
    /// 役の色(作る・変える・増やす・決める)
    roles: [u32; 4],
}

/// 1 型・2 型向け(赤と緑の区別がつきにくい)の明るい地。良い=青、誤り=赤紫、注意=暗い黄。
pub const CVD_RG_LIGHT: Palette = cvd(
    LIGHT,
    0x0072b2,
    0x8a6e00,
    0xb4005a,
    &CvdInks {
        axis: [0xd55e00, 0x009e73, 0x0072b2],
        deviation: [0xf0e442, 0x0072b2, 0xd55e00],
        join: 0x0072b2,
        cut: 0xd55e00,
        intersect: 0x56b4e9,
        intersect_edge: 0x2a8bc4,
        gouge: 0xd55e00,
        leftover: 0x0072b2,
        free: 0x0072b2,
        reference: 0xcc79a7,
        pick: 0xe69f00,
        draw: 0x0072b2,
        modify: 0xe69f00,
        constraint: 0xcc79a7,
        group: [0x0072b2, 0xe69f00, 0xcc79a7],
        op: [
            0x0072b2, 0xe69f00, 0x56b4e9, 0xd55e00, 0x009e73, 0xf0e442, 0xcc79a7, 0x5a5a5a,
        ],
        roles: [0x197fe5, 0xcc6600, 0xcc3298, 0x0f8960],
    },
);

/// 1 型・2 型向けの暗い地。
pub const CVD_RG_DARK: Palette = cvd(
    DARK,
    0x4fa8ff,
    0xf0e442,
    0xff9ad5,
    &CvdInks {
        axis: [0xff8a3d, 0x2fc9a0, 0x5aa0ff],
        deviation: [0xf0e442, 0x3d8bff, 0xff7a1a],
        join: 0x2f7fd8,
        cut: 0xff7a1a,
        intersect: 0x56b4e9,
        intersect_edge: 0x56b4e9,
        gouge: 0xff7a1a,
        leftover: 0x56b4e9,
        free: 0x4fa8ff,
        reference: 0xf0a0d0,
        pick: 0xf5b000,
        draw: 0x4fa8ff,
        modify: 0xe69f00,
        constraint: 0xf0a0d0,
        group: [0x7fb0ff, 0xe6a23c, 0xf0a0d0],
        op: [
            0x3d8bff, 0xe69f00, 0x8fd3ff, 0xff6a1a, 0x2fc9a0, 0xf0e442, 0xe09cc4, 0x808080,
        ],
        roles: [0x32baff, 0xea9947, 0xdb70a5, 0x8cf2bf],
    },
);

/// 3 型向け(青と黄の区別がつきにくい)の明るい地。良い=青緑、誤り=赤、注意=茶。
pub const CVD_TRI_LIGHT: Palette = cvd(
    LIGHT,
    0x00796b,
    0x8a4f00,
    0xc8102e,
    &CvdInks {
        axis: [0xd81b3c, 0x00897b, 0x5e35b1],
        deviation: [0xb0b0b0, 0x00897b, 0xd81b60],
        join: 0x3a3ad0,
        cut: 0xe0003c,
        intersect: 0x26c6da,
        intersect_edge: 0x00838f,
        gouge: 0xe0003c,
        leftover: 0x00897b,
        free: 0x00796b,
        reference: 0xad1457,
        pick: 0xef6c00,
        draw: 0x00796b,
        modify: 0xef6c00,
        constraint: 0xad1457,
        group: [0x00796b, 0xc2185b, 0x6d4c41],
        op: [
            0xd81b3c, 0x00897b, 0xf06292, 0x4dd0e1, 0x8e24aa, 0xef6c00, 0x546e7a, 0x00acc1,
        ],
        roles: [0x2d5bb7, 0xe55d19, 0xd147a3, 0x11a088],
    },
);

/// 3 型向けの暗い地。
pub const CVD_TRI_DARK: Palette = cvd(
    DARK,
    0x4dd0c0,
    0xffb074,
    0xff5c7a,
    &CvdInks {
        axis: [0xff5c7a, 0x4dd0c0, 0xb39ddb],
        deviation: [0xc8c8c8, 0x26c6da, 0xff4081],
        join: 0x8c8cff,
        cut: 0xff3d60,
        intersect: 0x4dd0e1,
        intersect_edge: 0x4dd0e1,
        gouge: 0xff3d60,
        leftover: 0x4dd0c0,
        free: 0x4dd0c0,
        reference: 0xff80ab,
        pick: 0xffd180,
        draw: 0x4dd0c0,
        modify: 0xffd180,
        constraint: 0xff80ab,
        group: [0x4dd0c0, 0xf06292, 0xbcaaa4],
        op: [
            0xff5c7a, 0x4dd0c0, 0xf48fb1, 0x26a69a, 0xce93d8, 0xffa040, 0x90a4ae, 0x80deea,
        ],
        roles: [0x99baff, 0xe88c30, 0xe598cb, 0x47d1a3],
    },
);

impl Palette {
    /// 色ごとに、名前(テーマのファイルの鍵)と色を渡す(画面の色、続けて意味の名前の色)。
    pub fn each_mut(&mut self, f: &mut dyn FnMut(&str, &mut Color32)) {
        f("chrome", &mut self.chrome);
        f("chrome2", &mut self.chrome2);
        f("panel", &mut self.panel);
        f("paper", &mut self.paper);
        f("edge", &mut self.edge);
        f("ink", &mut self.ink);
        f("ink2", &mut self.ink2);
        f("ink3", &mut self.ink3);
        f("canvas", &mut self.canvas);
        f("grid", &mut self.grid);
        f("design", &mut self.design);
        f("design_soft", &mut self.design_soft);
        f("cam", &mut self.cam);
        f("cam_soft", &mut self.cam_soft);
        f("render", &mut self.render);
        f("render_soft", &mut self.render_soft);
        f("anim", &mut self.anim);
        f("anim_soft", &mut self.anim_soft);
        f("sim", &mut self.sim);
        f("sim_soft", &mut self.sim_soft);
        f("sketch", &mut self.sketch);
        f("sketch_soft", &mut self.sketch_soft);
        f("ok", &mut self.ok);
        f("warn", &mut self.warn);
        f("err", &mut self.err);
        f("sel", &mut self.sel);
        f("icon_base", &mut self.icon_base);
        f("role_make", &mut self.role_make);
        f("role_modify", &mut self.role_modify);
        f("role_repeat", &mut self.role_repeat);
        f("role_define", &mut self.role_define);
        f("shade1", &mut self.shade1);
        f("shade2", &mut self.shade2);
        f("shade3", &mut self.shade3);
        f("bed", &mut self.bed);
        f("hole", &mut self.hole);
        self.inks.each_mut(f);
    }

    /// 色の名前と色の一覧(名前の順)。
    pub fn entries(&self) -> Vec<(String, Color32)> {
        let mut out = vec![];
        self.clone()
            .each_mut(&mut |k, c| out.push((k.to_string(), *c)));
        out
    }

    /// 道具のアイコンの色(土台と役の色)。
    pub fn icon_inks(&self) -> icons::IconInks {
        icons::IconInks {
            base: self.icon_base,
            make: self.role_make,
            modify: self.role_modify,
            repeat: self.role_repeat,
            define: self.role_define,
        }
    }

    /// 作業空間の色。
    pub fn ws(&self, s: Space) -> Color32 {
        match s {
            Space::Design => self.design,
            Space::Machining => self.cam,
            Space::Render => self.render,
            Space::Animation => self.anim,
            Space::Simulation => self.sim,
        }
    }

    /// 作業空間の淡い色(見出しの帯、選んだ札の地)。
    pub fn ws_soft(&self, s: Space) -> Color32 {
        match s {
            Space::Design => self.design_soft,
            Space::Machining => self.cam_soft,
            Space::Render => self.render_soft,
            Space::Animation => self.anim_soft,
            Space::Simulation => self.sim_soft,
        }
    }
}

/// 2 色を混ぜる(`k` は b の割合。3D のビューの部品と同じ式)。
pub use kmb_ui_view3d::mix;

/// テーマ: 明暗 2 組の色と、大きさ・角・詰め具合・動きの倍率。設定の「画面」から作る。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub light: Palette,
    pub dark: Palette,
    /// 字・アイコン・角の丸み・動きの倍率
    pub scale: Scale,
    /// 部品の間の広さの倍率(詰める・普通・広い)
    pub density: f32,
}

impl Theme {
    /// 標準のテーマ。
    pub const STANDARD: Theme = Theme {
        light: LIGHT,
        dark: DARK,
        scale: Scale::ONE,
        density: 1.0,
    };
    /// ハイコントラストのテーマ。
    pub const HIGH_CONTRAST: Theme = Theme {
        light: HC_LIGHT,
        dark: HC_DARK,
        scale: Scale::ONE,
        density: 1.0,
    };

    /// 1 型・2 型向けのテーマ(U27)。
    pub const CVD_RG: Theme = Theme {
        light: CVD_RG_LIGHT,
        dark: CVD_RG_DARK,
        scale: Scale::ONE,
        density: 1.0,
    };
    /// 3 型向けのテーマ(U27)。
    pub const CVD_TRI: Theme = Theme {
        light: CVD_TRI_LIGHT,
        dark: CVD_TRI_DARK,
        scale: Scale::ONE,
        density: 1.0,
    };

    /// 明暗のどちらかの色。
    pub fn palette(&self, dark: bool) -> Palette {
        if dark { self.dark } else { self.light }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::STANDARD
    }
}

/// 今の明暗の色と、今の作業空間(毎フレーム [`apply`] が覚える)。
#[derive(Clone, Copy, PartialEq)]
pub struct Look {
    pub pal: Palette,
    pub space: Space,
    /// スケッチを直している(差し色をスケッチの水色にして、設計の画面と見分ける)
    pub sketch: bool,
    /// 字・アイコン・角の丸み・動きの倍率
    pub scale: Scale,
    /// 部品の間の広さの倍率
    pub density: f32,
}

impl Look {
    /// 押し出しなどのプレビューの、操作ごとの線と札の色(新しいボディは灰色、結合は青、
    /// 切り取りは赤、交差は黄。Kie 2026-10-25)。切り取りの赤は失敗の知らせの色と分ける。
    pub fn tool_op(&self, op: kmb_core::ToolOp) -> Color32 {
        match op {
            kmb_core::ToolOp::New => self.pal.ink2,
            kmb_core::ToolOp::Join => self.pal.inks.ghost_join_edge,
            kmb_core::ToolOp::Cut => self.pal.inks.ghost_cut_edge,
            kmb_core::ToolOp::Intersect => self.pal.warn,
        }
    }

    /// 差し色(作業空間の色。スケッチを直している間はスケッチの色)。
    pub fn ws(&self) -> Color32 {
        if self.sketch {
            self.pal.sketch
        } else {
            self.pal.ws(self.space)
        }
    }
    pub fn ws_soft(&self) -> Color32 {
        if self.sketch {
            self.pal.sketch_soft
        } else {
            self.pal.ws_soft(self.space)
        }
    }
}

impl Default for Look {
    /// 明るい画面の設計の作業空間(色を決める前の 3D のビューなど)。
    fn default() -> Self {
        Look {
            pal: LIGHT,
            space: Space::Design,
            sketch: false,
            scale: Scale::ONE,
            density: 1.0,
        }
    }
}

/// 3D のビューの色の元。
impl From<&Look> for crate::view3d::Palette {
    fn from(l: &Look) -> Self {
        let p = &l.pal;
        crate::view3d::Palette {
            dark: p.dark,
            canvas: p.canvas,
            ink: p.ink,
            ink2: p.ink2,
            ink3: p.ink3,
            err: p.err,
            warn: p.warn,
            design: p.design,
            cam: p.cam,
            shade3: p.shade3,
            accent: l.ws(),
            inks: p.inks,
        }
    }
}

fn look_id() -> egui::Id {
    egui::Id::new("kmb_look")
}

/// 今の見た目(まだ決めていなければ PC の明暗で決める)。
pub fn look(ctx: &egui::Context) -> Look {
    ctx.data(|d| d.get_temp::<Look>(look_id()))
        .unwrap_or_else(|| Look {
            pal: if ctx.global_style().visuals.dark_mode {
                DARK
            } else {
                LIGHT
            },
            space: Space::Design,
            sketch: false,
            scale: Scale::ONE,
            density: 1.0,
        })
}

/// 明暗と作業空間(とスケッチを直しているか)とテーマに合わせて egui の見た目を決める。毎フレーム
/// 呼ぶ(変わった時だけ入れ替える)。大きさの倍率は描く糸に入れ、3D のビューとスケッチにも色を渡す。
pub fn apply(ctx: &egui::Context, theme: &Theme, space: Space, sketch: bool) {
    let dark = ctx.theme() == egui::Theme::Dark;
    let look_of = |dark: bool| Look {
        pal: theme.palette(dark),
        space,
        sketch,
        scale: theme.scale,
        density: theme.density,
    };
    let now = look_of(dark);
    kmb_ui_view3d::set_scale(theme.scale);
    kmb_ui_view3d::set_inks(ctx, now.pal.inks);
    let before = ctx.data(|d| d.get_temp::<Look>(look_id()));
    if before == Some(now) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(look_id(), now));
    // 動きの元の長さ(アプリは egui の既定 0.2 秒、試験の枠は 0)を初めに覚え、それに倍率を掛ける
    let id = egui::Id::new("jc_motion_natural");
    let natural = ctx.data(|d| d.get_temp(id)).unwrap_or_else(|| {
        let s = ctx.global_style();
        let n = (s.animation_time, s.scroll_animation);
        ctx.data_mut(|d| d.insert_temp(id, n));
        n
    });
    for t in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(t, |s| style(s, &look_of(t == egui::Theme::Dark), natural));
    }
}

/// 角の丸み `r` に倍率を掛けた egui の角。
fn round(r: f32, k: f32) -> CornerRadius {
    CornerRadius::same((r * k).round().clamp(0.0, 255.0) as u8)
}

fn style(s: &mut egui::Style, l: &Look, natural: (f32, egui::style::ScrollAnimation)) {
    use egui::TextStyle;
    let p = &l.pal;
    let ws = l.ws();
    let soft = l.ws_soft();
    let t = l.scale.text;
    let d = l.density;
    let c = l.scale.corner;
    s.text_styles = [
        (TextStyle::Small, FontId::proportional(10.5 * t)),
        (TextStyle::Body, FontId::proportional(12.5 * t)),
        (TextStyle::Button, FontId::proportional(12.5 * t)),
        (TextStyle::Heading, FontId::proportional(15.0 * t)),
        (TextStyle::Monospace, FontId::monospace(12.0 * t)),
    ]
    .into();
    s.spacing.item_spacing = Vec2::new(6.0, 5.0) * d;
    s.spacing.button_padding = Vec2::new(8.0, 3.0) * d;
    s.spacing.interact_size.y = 22.0 * t;
    s.spacing.menu_margin = egui::Margin::same((4.0 * d).round() as i8);
    // 動き: 開閉と切替のアニメーションと、巻物の滑らかな移り
    s.animation_time = natural.0 * l.scale.motion;
    s.scroll_animation = if l.scale.motion == 0.0 {
        egui::style::ScrollAnimation::none()
    } else {
        natural.1
    };
    let v = &mut s.visuals;
    *v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.override_text_color = None;
    v.panel_fill = p.chrome;
    v.window_fill = p.panel;
    v.faint_bg_color = p.chrome2;
    v.extreme_bg_color = p.chrome;
    v.code_bg_color = p.chrome2;
    v.text_edit_bg_color = Some(p.chrome);
    v.window_stroke = Stroke::new(1.0, p.edge);
    v.window_corner_radius = round(8.0, c);
    v.menu_corner_radius = round(6.0, c);
    v.window_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 90 } else { 40 }),
    };
    v.popup_shadow = v.window_shadow;
    v.hyperlink_color = ws;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.err;
    v.selection.bg_fill = p.sel;
    v.selection.stroke = Stroke::new(1.0, ws);
    v.weak_text_color = Some(p.ink3);
    let w = &mut v.widgets;
    let r = round(4.0, c);
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.edge);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.ink);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = p.chrome;
    w.inactive.weak_bg_fill = p.chrome;
    w.inactive.bg_stroke = Stroke::new(1.0, p.edge);
    w.inactive.fg_stroke = Stroke::new(1.0, p.ink);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = p.chrome2;
    w.hovered.weak_bg_fill = p.chrome2;
    w.hovered.bg_stroke = Stroke::new(1.0, mix(p.edge, ws, 0.5));
    w.hovered.fg_stroke = Stroke::new(1.0, p.ink);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = soft;
    w.active.weak_bg_fill = soft;
    w.active.bg_stroke = Stroke::new(1.0, ws);
    w.active.fg_stroke = Stroke::new(1.0, ws);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;
    w.open.bg_fill = soft;
    w.open.weak_bg_fill = soft;
    w.open.bg_stroke = Stroke::new(1.0, ws);
    w.open.fg_stroke = Stroke::new(1.0, ws);
    w.open.corner_radius = r;
}

/// 試作の `.sec`: 小さな灰色の見出しと下線。右に添え書き(なければ空)。
pub fn section(ui: &mut Ui, title: &str, right: &str) {
    let l = look(ui.ctx());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(fs(10.5)).color(l.pal.ink3));
        if !right.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(right).size(fs(10.5)).color(l.pal.ink3));
            });
        }
    });
    let r = ui.max_rect();
    let y = ui.cursor().top() - 2.0;
    ui.painter()
        .hline(r.x_range(), y, Stroke::new(1.0, l.pal.edge));
}

/// 名前で指したアイコンを描く(画面の拡張が使う。絵は [`crate::icons`] が持つ)。
pub fn paint_icon(painter: &egui::Painter, rect: Rect, name: &str, color: Color32) {
    icons::paint(painter, rect, name, color, 1.5);
}

/// 角の丸い札(`.chip`)。`color` があればその色の線と字。
pub fn chip(ui: &mut Ui, text: &str, color: Option<Color32>) -> Response {
    let l = look(ui.ctx());
    let c = color.unwrap_or(l.pal.ink2);
    let line = color.unwrap_or(l.pal.edge);
    let g = ui.painter().layout_no_wrap(text.to_string(), prop(11.0), c);
    let size = Vec2::new(g.size().x + 16.0, fs(20.0));
    let (rect, r) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_stroke(
        rect,
        corner(fs(10.0)),
        Stroke::new(1.0, line),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(rect.center() - g.size() / 2.0, g, c);
    r
}

/// 押せる札(`.kinds button`、`.chip.on`)。
pub fn chip_toggle(ui: &mut Ui, on: bool, text: &str) -> Response {
    let l = look(ui.ctx());
    let (c, line) = if on {
        (l.ws(), l.ws())
    } else {
        (l.pal.ink2, l.pal.edge)
    };
    ui.add(
        egui::Button::new(RichText::new(text).size(fs(10.5)).color(c))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, line))
            .corner_radius(round(99.0, scale().corner))
            .min_size(Vec2::new(0.0, fs(18.0))),
    )
}

/// キーの印(下の帯の `左`・`右` など。試作の `.mh b`)。
pub fn kbd(ui: &mut Ui, key: &str) {
    let l = look(ui.ctx());
    let _ = key_cap(ui, key, prop(10.5), l.pal.panel, l.pal.ink, l.pal.edge);
}

/// 押している間だけ色が付く修飾キーの印(Shift・Ctrl・Alt。試作の `.mods i`)。
pub fn modifier(ui: &mut Ui, key: &str, on: bool) {
    let l = look(ui.ctx());
    let (fill, fg, line) = if on {
        (l.ws(), l.pal.chrome, l.ws())
    } else {
        (Color32::TRANSPARENT, l.pal.ink3, l.pal.edge)
    };
    let r = key_cap(ui, key, mono(10.5), fill, fg, line);
    r.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{key} キー"))
    });
}

/// 枠つきのキーの印。字が帯用の高さ16を超える時だけ、上下の余白を加えた高さにする。
pub(crate) fn key_cap(
    ui: &mut Ui,
    key: &str,
    font: FontId,
    fill: Color32,
    fg: Color32,
    line: Color32,
) -> Response {
    let g = ui.painter().layout_no_wrap(key.to_string(), font, fg);
    let height = if g.size().y > fs(16.0) {
        g.size().y + fs(4.0)
    } else {
        fs(16.0)
    };
    let (rect, r) = ui.allocate_exact_size(Vec2::new(g.size().x + 10.0, height), Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect(
            rect,
            corner(3.0),
            fill,
            Stroke::new(1.0, line),
            egui::StrokeKind::Inside,
        );
        let pos = rect.center() - g.size() / 2.0;
        p.galley(pos, g, fg);
    }
    r
}

/// 作業空間の色で塗った主なボタン(`.btn.pri`)。`key` は右に小さく出すキー(Enter など)。
pub fn primary(ui: &mut Ui, text: &str, key: &str, enabled: bool) -> Response {
    let l = look(ui.ctx());
    let mut job = egui::text::LayoutJob::default();
    let fg = if enabled { l.pal.chrome } else { l.pal.ink3 };
    job.append(text, 0.0, egui::TextFormat::simple(prop(12.5), fg));
    if !key.is_empty() {
        job.append(
            key,
            6.0,
            egui::TextFormat::simple(mono(10.0), fg.gamma_multiply(0.75)),
        );
    }
    let fill = if enabled { l.ws() } else { l.pal.chrome2 };
    ui.add_enabled(
        enabled,
        egui::Button::new(job)
            .fill(fill)
            .stroke(Stroke::new(1.0, if enabled { l.ws() } else { l.pal.edge }))
            .min_size(Vec2::new(0.0, fs(28.0))),
    )
}

/// 縁取りのボタン(`.btn`)。
pub fn secondary(ui: &mut Ui, text: &str, key: &str, enabled: bool) -> Response {
    let l = look(ui.ctx());
    let mut job = egui::text::LayoutJob::default();
    let fg = if enabled { l.pal.ink } else { l.pal.ink3 };
    job.append(text, 0.0, egui::TextFormat::simple(prop(12.5), fg));
    if !key.is_empty() {
        job.append(key, 6.0, egui::TextFormat::simple(mono(10.0), l.pal.ink3));
    }
    ui.add_enabled(
        enabled,
        egui::Button::new(job)
            .fill(l.pal.chrome)
            .stroke(Stroke::new(1.0, l.pal.edge))
            .min_size(Vec2::new(0.0, fs(28.0))),
    )
}

/// アイコンだけの小さなボタン(上の帯の保存・取り消しなど)。`name` は読み上げと検索に使う名前。
pub fn icon_button(ui: &mut Ui, icon: &str, name: &str, enabled: bool, on: bool) -> Response {
    let l = look(ui.ctx());
    let size = Vec2::new(30.0, 26.0) * band();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(size, sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    if ui.is_rect_visible(rect) {
        if on || (enabled && r.hovered()) {
            ui.painter().rect_filled(rect, corner(4.0), l.pal.edge);
        }
        let c = if enabled { l.pal.ink } else { l.pal.ink3 };
        icons::paint_exact(
            ui.painter(),
            egui::Rect::from_center_size(rect.center(), Vec2::splat(icon_px(18.0))),
            icon,
            c,
            1.6,
        );
    }
    r
}

/// 文字とアイコンのボタン(`.abtn`、ナビの帯)。名前が「 ▾」で終われば下向きの印を描く。
pub fn icon_text_button(
    ui: &mut Ui,
    icon: &str,
    text: &str,
    color: Color32,
    size: f32,
) -> Response {
    icon_text_button_fit(ui, icon, text, color, size, false)
}

/// [`icon_text_button`] で、`icon_only` ならアイコンだけを出すもの(名前は読み上げと検索に残す)。
/// 字が大きくて並びが入らない時に使う。
pub fn icon_text_button_fit(
    ui: &mut Ui,
    icon: &str,
    text: &str,
    color: Color32,
    size: f32,
    icon_only: bool,
) -> Response {
    let l = look(ui.ctx());
    let (text, chev) = match text.strip_suffix(" ▾") {
        Some(t) => (t, true),
        None => (text, false),
    };
    let shown = if icon_only { "" } else { text };
    let galley = ui
        .painter()
        .layout_no_wrap(shown.to_string(), prop(size), color);
    let iw = icon_px(15.0);
    let gap = if icon_only { 0.0 } else { 4.0 };
    let w = 6.0 + iw + gap + galley.size().x + if chev { icon_px(16.0) } else { 0.0 } + 7.0;
    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, 24.0 * band()), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    if ui.is_rect_visible(rect) {
        if r.hovered() {
            ui.painter().rect_filled(rect, corner(4.0), l.pal.chrome2);
        }
        let ic = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 6.0 + iw / 2.0, rect.center().y),
            Vec2::splat(iw),
        );
        icons::paint_exact(ui.painter(), ic, icon, color, 1.6);
        let gx = ic.right() + gap;
        let gw = galley.size().x;
        ui.painter().galley(
            egui::pos2(gx, rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
        if chev {
            icons::paint_exact(
                ui.painter(),
                egui::Rect::from_center_size(
                    egui::pos2(gx + gw + icon_px(9.0), rect.center().y + 1.0),
                    Vec2::splat(icon_px(11.0)),
                ),
                "chev",
                color,
                1.6,
            );
        }
    }
    r
}

/// メニューを開くボタン(アイコン、名前、下向きの印)。`egui::Popup::menu` に渡す。
pub fn menu_trigger(ui: &mut Ui, icon: &str, text: &str) -> Response {
    let l = look(ui.ctx());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), prop(12.5), l.pal.ink);
    let w = 7.0 + icon_px(16.0) + 4.0 + galley.size().x + 4.0 + icon_px(12.0) + 6.0;
    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, 26.0 * band()), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&r));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if r.hovered() || open {
            p.rect_filled(rect, corner(4.0), l.pal.edge);
        }
        let ic = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 7.0 + icon_px(8.0), rect.center().y),
            Vec2::splat(icon_px(16.0)),
        );
        icons::paint_exact(p, ic, icon, l.pal.ink, 1.6);
        let tx = ic.right() + 4.0;
        p.galley(
            egui::pos2(tx, rect.center().y - galley.size().y / 2.0),
            galley.clone(),
            l.pal.ink,
        );
        let cv = egui::Rect::from_center_size(
            egui::pos2(
                tx + galley.size().x + 4.0 + icon_px(6.0),
                rect.center().y + 1.0,
            ),
            Vec2::splat(icon_px(12.0)),
        );
        icons::paint_exact(p, cv, "chev", l.pal.ink2, 1.6);
    }
    r
}

/// ツールバーの大きなボタンの名前の行の高さ(試作の 10.5 × 1.15)。
const CMD_LINE: f32 = 12.0;

/// 字とアイコンの両方が入る、決まった高さの帯と部品に掛ける倍率(字とアイコンの大きい方)。
pub fn band() -> f32 {
    let s = scale();
    s.text.max(s.icon)
}

/// ツールバーの大きなボタンの大きさ(1 倍なら試作と同じ 62×54。アイコンと 2 行の名前に合わせて広げる)。
pub fn tile_size() -> Vec2 {
    let s = scale();
    Vec2::new(
        62.0 * band(),
        54.0 + 26.0 * (s.icon - 1.0) + 2.0 * CMD_LINE * (s.text - 1.0),
    )
}

/// ツールバーのまとまりの帯の高さ(上のすき間 3、大きなボタン、下のまとまりの名前の行 19。
/// 1 倍なら 76)。名前の行も字の倍率で高くする。
pub fn band_height() -> f32 {
    3.0 + tile_size().y + fs(19.0)
}

/// ツールバーの大きなボタン(アイコンの下に名前、2 行まで)。
pub fn command_button(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    enabled: bool,
    pinned: bool,
) -> Response {
    command_tile(ui, icon, label, enabled, pinned, false)
}

/// 右クリックのメニュー。egui の `Response::context_menu` の代わりに必ずこれを使う。
///
/// egui のメニューはどのボタンのクリックでも閉じるので、素早く 2 回右クリックすると、2 回目
/// (出たメニューの角か、元の物の上)でメニューが一瞬で消えた(10-25 u5)。出ているメニューの上か
/// 元の物の上の右クリックでは閉じない(元の物の上なら押した所へ出し直す)。
pub trait RightMenu {
    fn right_menu(&self, add: impl FnOnce(&mut Ui)) -> Option<egui::InnerResponse<()>>;
}

impl RightMenu for Response {
    fn right_menu(&self, add: impl FnOnce(&mut Ui)) -> Option<egui::InnerResponse<()>> {
        let id = egui::Popup::default_response_id(self);
        let (right, at) = self.ctx.input(|i| {
            (
                i.pointer.button_clicked(egui::PointerButton::Secondary),
                i.pointer.interact_pos(),
            )
        });
        let on_menu = || {
            let shown = self.ctx.read_response(id);
            at.is_some_and(|p| shown.is_some_and(|r| r.rect.contains(p)))
        };
        let again = right
            && egui::Popup::is_id_open(&self.ctx, id)
            && (self.secondary_clicked() || on_menu());
        let menu = egui::Popup::context_menu(self);
        let menu = if again {
            menu.close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        } else {
            menu
        };
        menu.show(add)
    }
}

/// 右クリックのメニューの 1 行(左にアイコン、名前、右に同じ操作のキー)。`ui.button` の代わりに
/// 使う(10-19 M17: アイコンが無く、何が起こるか分かりにくかった)。`icon` が空でも名前の頭を
/// ほかの行とそろえ、`keys` が空ならキーを出さない。名前は読み上げと画面の要素の操作に残す。
/// 行の幅はメニューの幅いっぱいに広げ、まとまりの間は `ui.separator()` で区切る。
pub fn menu_row(ui: &mut Ui, icon: &str, label: &str, keys: &str, enabled: bool) -> Response {
    row_of_menu(ui, icon, label, keys, enabled, false)
}

/// 取り消しても戻すのに手間のかかる行(中身ごと消すなど)。名前を誤りの色で出す。
pub fn menu_row_danger(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    keys: &str,
    enabled: bool,
) -> Response {
    row_of_menu(ui, icon, label, keys, enabled, true)
}

fn row_of_menu(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    keys: &str,
    enabled: bool,
    danger: bool,
) -> Response {
    let l = look(ui.ctx());
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let tc = match (enabled, danger) {
        (false, _) => l.pal.ink3,
        (true, true) => l.pal.err,
        (true, false) => l.pal.ink,
    };
    let name = ui
        .painter()
        .layout_no_wrap(label.to_string(), prop(12.5), tc);
    let key = ui
        .painter()
        .layout_no_wrap(keys.to_string(), prop(12.0), l.pal.ink3);
    let iw = icon_px(16.0);
    let gap = if keys.is_empty() { 0.0 } else { 28.0 };
    let w = 6.0 + iw + 8.0 + name.size().x + gap + key.size().x + 8.0;
    let (rect, r) = ui.allocate_at_least(Vec2::new(w, 22.0 * band()), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if enabled && r.hovered() {
            p.rect_filled(rect, corner(3.0), l.pal.chrome2);
        }
        if !icon.is_empty() {
            icons::paint_exact_inks(
                p,
                Rect::from_center_size(
                    egui::pos2(rect.left() + 6.0 + iw / 2.0, rect.center().y),
                    Vec2::splat(iw),
                ),
                icon,
                &tool_inks(&l, enabled, false),
                1.5,
            );
        }
        let y = rect.center().y;
        p.galley(
            egui::pos2(rect.left() + 14.0 + iw, y - name.size().y / 2.0),
            name,
            tc,
        );
        if !keys.is_empty() {
            let x = rect.right() - 8.0 - key.size().x;
            p.galley(egui::pos2(x, y - key.size().y / 2.0), key, l.pal.ink3);
        }
    }
    r
}

/// 右クリックのメニューの、横に一覧を開く 1 行([`menu_row`] と同じ見た目で、右に矢印)。
/// `ui.menu_button` の代わりに使う。
pub fn menu_sub<R>(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let r = menu_row(ui, icon, label, "⏵", true);
    egui::containers::menu::SubMenu::new()
        .show(ui, &r, add)
        .map(|i| i.inner)
}

/// ツールバーの ▾ の一覧の 1 行(アイコンと名前)。`on` は使っている道具。
pub fn menu_item(ui: &mut Ui, icon: &str, label: &str, enabled: bool, on: bool) -> Response {
    let l = look(ui.ctx());
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(Vec2::new(196.0 * band(), 24.0 * band()), sense);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, label));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let accent = l.ws();
        if on {
            p.rect(
                rect,
                corner(3.0),
                mix(l.pal.panel, accent, 0.2),
                Stroke::new(1.0, accent),
                egui::StrokeKind::Inside,
            );
        } else if enabled && r.hovered() {
            p.rect_filled(rect, corner(3.0), l.pal.chrome2);
        }
        let ic = tool_inks(&l, enabled, false);
        icons::paint_exact_inks(
            p,
            Rect::from_center_size(
                egui::pos2(rect.left() + 5.5 + icon_px(17.0) / 2.0, rect.center().y),
                Vec2::splat(icon_px(17.0)),
            ),
            icon,
            &ic,
            1.5,
        );
        let tc = if enabled { l.pal.ink } else { l.pal.ink3 };
        p.text(
            egui::pos2(rect.left() + 13.0 + icon_px(17.0), rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            prop(12.5),
            tc,
        );
    }
    r
}

/// ツールバーの道具の絵の色。土台の灰と役の色の 2 層で、使えない時は `ink3`、使っている道具は
/// 作業空間の色の 1 色。
pub fn tool_inks(l: &Look, enabled: bool, on: bool) -> icons::IconInks {
    match (enabled, on) {
        (false, _) => icons::IconInks::mono(l.pal.ink3),
        (true, true) => icons::IconInks::mono(l.ws()),
        (true, false) => l.pal.icon_inks(),
    }
}

/// まとまりの大きなボタン。`on` はいま使っている道具(作業空間の色の濃い枠と塗りで囲み、下に印の
/// 線を引く)。
pub fn command_tile(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    enabled: bool,
    pinned: bool,
    on: bool,
) -> Response {
    let l = look(ui.ctx());
    // 字とアイコンの倍率で広げる(1 倍なら試作と同じ 62×54)
    let (ts, is) = (scale().text, scale().icon);
    let Vec2 { x: w, y: h } = tile_size();
    let line_h = CMD_LINE * ts;
    let ic = 26.0 * is;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, h), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        // 使っている道具: 作業空間の色の淡い塗りと枠、下の太い線
        let accent = l.ws();
        if on {
            p.rect(
                rect,
                corner(4.0),
                mix(l.pal.panel, accent, 0.2),
                Stroke::new(1.5, accent),
                egui::StrokeKind::Inside,
            );
            p.line_segment(
                [
                    rect.left_bottom() + Vec2::new(8.0, -2.0),
                    rect.right_bottom() + Vec2::new(-8.0, -2.0),
                ],
                Stroke::new(3.0, accent),
            );
        } else if enabled && r.hovered() {
            p.rect_filled(rect, corner(4.0), l.pal.chrome2);
        }
        let c = tool_inks(&l, enabled, on);
        // 試作の `.cmd`: 上下 4 の余白、アイコン 26、間 2、名前(10.5、行の高さ 1.15、2 行まで)。
        // 中身の高さで、帯の上 3 から 55 の中の上下の真ん中に置く(2 行の名前は上へ寄る)
        let tc = if enabled { l.pal.ink2 } else { l.pal.ink3 };
        let mut job = egui::text::LayoutJob::simple(label.to_string(), prop(10.5), tc, w - 4.0);
        job.halign = egui::Align::Center;
        job.wrap.max_rows = 2;
        for s in &mut job.sections {
            s.format.line_height = Some(line_h);
        }
        let g = ui.fonts_mut(|f| f.layout_job(job));
        let rows = g.rows.len().max(1) as f32;
        let top = rect.top() + (h + 1.0 - (4.0 + ic + 2.0 + rows * line_h + 4.0)) / 2.0;
        icons::paint_exact_inks(
            p,
            egui::Rect::from_center_size(
                egui::pos2(rect.center().x, top + 4.0 + ic / 2.0),
                Vec2::splat(ic),
            ),
            icon,
            &c,
            1.5,
        );
        p.galley(egui::pos2(rect.center().x, top + 4.0 + ic + 2.0), g, tc);
        if pinned {
            p.circle_filled(rect.right_top() + Vec2::new(-5.0, 5.0), 2.5, l.ws());
        }
    }
    r
}

/// つながった切替(`.seg`、`.viewbtns`)。押した番号を返す。
pub fn segmented(ui: &mut Ui, items: &[(&str, bool, bool)], fill: Color32) -> Option<usize> {
    let l = look(ui.ctx());
    let font = prop(11.5);
    let widths: Vec<f32> = items
        .iter()
        .map(|(t, _, _)| {
            ui.painter()
                .layout_no_wrap(t.to_string(), font.clone(), l.pal.ink)
                .size()
                .x
                + 18.0
        })
        .collect();
    let total = widths.iter().sum::<f32>() + (items.len().saturating_sub(1)) as f32;
    let hgt = fs(24.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, hgt), Sense::hover());
    let p = ui.painter();
    let rr = corner(5.0);
    p.rect_filled(rect, rr, l.pal.edge);
    let mut out = None;
    let mut x = rect.left();
    let n = items.len();
    for (i, ((text, on, enabled), w)) in items.iter().zip(&widths).enumerate() {
        let r = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(*w, hgt));
        x += w + 1.0;
        let sense = if *enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let resp = ui.interact(r, ui.id().with(("seg", text, i)), sense);
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, *enabled, *on, *text)
        });
        let k = rr.round() as u8;
        let round = CornerRadius {
            nw: if i == 0 { k } else { 0 },
            sw: if i == 0 { k } else { 0 },
            ne: if i + 1 == n { k } else { 0 },
            se: if i + 1 == n { k } else { 0 },
        };
        let bg = if *enabled && resp.hovered() {
            l.pal.chrome2
        } else {
            fill
        };
        let p = ui.painter();
        p.rect_filled(r.shrink2(Vec2::new(0.0, 1.0)), round, bg);
        let c = match (*on, *enabled) {
            (_, false) => l.pal.ink3.gamma_multiply(0.7),
            (true, _) => l.ws(),
            _ => l.pal.ink2,
        };
        let g = p.layout_no_wrap(text.to_string(), font.clone(), c);
        let pos = r.center() - g.size() / 2.0;
        p.galley(pos, g.clone(), c);
        if *on {
            p.galley(pos + Vec2::new(0.5, 0.0), g, c);
        }
        if resp.clicked() {
            out = Some(i);
        }
    }
    out
}

#[cfg(test)]
mod key_cap_tests {
    use super::*;

    /// 確かめること: 小さいキーの印は帯の高さを保ち、大きい字も枠の内側に収める。
    #[test]
    fn review_imported_key_caps_fit_large_text_and_keep_small_height() {
        let mut h = egui_kittest::Harness::builder().build_ui_state(
            |ui, sizes: &mut Vec<(f32, f32, f32)>| {
                sizes.clear();
                for size in [10.5, 20.0, 40.0] {
                    let font = FontId::proportional(size);
                    let glyph =
                        ui.painter()
                            .layout_no_wrap("F8".into(), font.clone(), Color32::WHITE);
                    let response = key_cap(
                        ui,
                        "F8",
                        font,
                        Color32::BLACK,
                        Color32::WHITE,
                        Color32::GRAY,
                    );
                    sizes.push((size, response.rect.height(), glyph.size().y));
                }
            },
            Vec::new(),
        );
        h.run_steps(2);
        let sizes = h.state();
        assert_eq!(sizes[0].1, fs(16.0));
        for &(size, height, glyph) in sizes {
            assert!(
                height >= glyph,
                "文字{size}: 枠の高さ{height}に字の高さ{glyph}が収まりません"
            );
        }
    }
}
