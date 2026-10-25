//! 3D のビューに重ねる物の描き方の役割の語彙。コアは印の型
//! ([`kmb_core::LineInk`] など)だけを持ち、何の線・箱・色かはここの定数で決める。モジュールは
//! 重ねる物にこの印を付けて渡し、画面のテーマは印ごとに色・太さ・破線を決める(何の線かは知らない)。
//! 印の中身の文字は見分けるためだけの物で、画面にもファイルにも出ない。

/// 重ねる線の描き方の役割。
pub mod line {
    use kmb_core::LineInk;

    /// 主の線(加工の経路の切削など)
    pub const MAIN: LineInk = LineInk("main");
    /// 移る線(早送りなど。破線)
    pub const TRAVEL: LineInk = LineInk("travel");
    /// 目立たせた移る線(全退避の早送りなど。移る線の色で太く、細かい破線。削っていない間の
    /// 無駄を見つけやすいように)
    pub const TRAVEL_LOUD: LineInk = LineInk("travel_loud");
    /// 薄い参照の線(これからの経路、文字の外接の枠、避ける範囲など。削らない)
    pub const FAINT: LineInk = LineInk("faint");
    /// 周りの物の線(機械のベッド・ネジ穴・L 字の基準・工具長センサー、固定具、形にない線)
    pub const SURROUND: LineInk = LineInk("surround");
    /// 作図の線(スケッチ。3D のビューで水色に描き、ボディの白・灰色と見分ける)
    pub const CONSTRUCTION: LineInk = LineInk("construction");
    /// 寄る線(経路の入り: らせん・斜め・真下・下穴・ストックの外から材料へ下りる動き。少し太い)
    pub const APPROACH: LineInk = LineInk("approach");
    /// 沿い始め・離れる線(経路のリード: 壁に沿い始める・離れる円弧と直線)
    pub const EASE: LineInk = LineInk("ease");
    /// つなぐ線(経路のつなぎ: 削った所や材料の上を送りで移る動き。細かい破線)
    pub const CONNECT: LineInk = LineInk("connect");

    /// ここにある線の役割の全部(画面のテーマの試験が、どれにも色があるかを確かめる)。
    pub const ALL: [LineInk; 9] = [
        MAIN,
        TRAVEL,
        TRAVEL_LOUD,
        FAINT,
        SURROUND,
        CONSTRUCTION,
        APPROACH,
        EASE,
        CONNECT,
    ];
}

/// 重ねる箱の描き方の役割。
pub mod boxes {
    use kmb_core::BoxInk;

    /// 作業の箱(ストックなど。実線。画面は原点の軸の長さも最初のこの箱に合わせる)
    pub const WORK: BoxInk = BoxInk("work");
    /// 限りの箱(機械の作業範囲など。破線)
    pub const LIMIT: BoxInk = BoxInk("limit");

    pub const ALL: [BoxInk; 2] = [WORK, LIMIT];
}

/// 半透明の平面の色の役割(加工の高さ)。
pub mod plane {
    use kmb_core::PlaneInk;

    /// 加工の安全高さ・送り高さ・トップ高さ・ボトム高さ
    pub const SAFE: PlaneInk = PlaneInk("safe");
    pub const FEED: PlaneInk = PlaneInk("feed");
    pub const TOP: PlaneInk = PlaneInk("top");
    pub const BOTTOM: PlaneInk = PlaneInk("bottom");

    pub const ALL: [PlaneInk; 4] = [SAFE, FEED, TOP, BOTTOM];

    /// 作業平面(基準面と同じ灰色。加工の高さの色とは別に、画面のテーマが決める)
    pub const DATUM: PlaneInk = PlaneInk("datum");
}

/// 部品の代わりに描く形の面の色の役割。
pub mod shape {
    use kmb_core::ShapeInk;

    /// 比べる時: 許容内・削り不足・削り過ぎ
    pub const WITHIN: ShapeInk = ShapeInk::Named("within");
    pub const UNDER: ShapeInk = ShapeInk::Named("under");
    pub const OVER: ShapeInk = ShapeInk::Named("over");
    /// 工具: 刃・シャンク・ホルダー
    pub const CUTTER: ShapeInk = ShapeInk::Named("cutter");
    pub const SHANK: ShapeInk = ShapeInk::Named("shank");
    pub const HOLDER: ShapeInk = ShapeInk::Named("holder");

    pub const ALL: [ShapeInk; 6] = [WITHIN, UNDER, OVER, CUTTER, SHANK, HOLDER];
}

/// 3D のビューに置く印(シミュレーションで見つかった所)。
pub mod mark {
    use kmb_core::MarkKind;

    /// 部品に食い込んだ所(工具の先端の位置)
    pub const GOUGE: MarkKind = MarkKind("gouge");
    /// 部品より高く残った所(残った面の上)
    pub const LEFTOVER: MarkKind = MarkKind("leftover");

    pub const ALL: [MarkKind; 2] = [GOUGE, LEFTOVER];
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// 同じ種類の印どうしで、中身の文字が重ならない(重なると画面が別の役割の色で描く)。
    #[test]
    fn marks_of_one_kind_are_distinct() {
        fn distinct<T: Ord + Copy>(all: &[T]) -> bool {
            all.iter().copied().collect::<BTreeSet<_>>().len() == all.len()
        }
        assert!(distinct(&super::line::ALL));
        assert!(distinct(&super::boxes::ALL));
        assert!(distinct(&super::plane::ALL));
        assert!(distinct(&super::mark::ALL));
        let shape: Vec<&str> = super::shape::ALL
            .iter()
            .map(|s| match s {
                kmb_core::ShapeInk::Named(n) => *n,
                _ => unreachable!(),
            })
            .collect();
        assert!(distinct(&shape));
    }
}
