//! 描く・直す道具の種類と、その案内。

use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    Select,
    Line,
    Circle,
    /// 単独の点(穴の位置など)
    Point,
    /// 押した所に点を置き、その点を基準にした文字を作る(スケッチを終えて文字の機能へ。K32)
    Text,
    /// 対角の 2 隅
    Rectangle,
    /// 中心と 1 つの隅
    CenterRectangle,
    /// 対角の 2 隅で描き、角を丸めた矩形
    RoundedRectangle,
    /// 中心、始点、終点(反時計回り)
    Arc,
    /// 線分の端から、線に接して続く円弧
    TangentArc,
    /// 押した区間を、ほかの線との交点まで消す(押したまま引くと、なぞった区間をまとめて消す)
    Trim,
    /// 押した端を、次に当たる線まで延ばす
    Extend,
    /// 押した所を通る区間を、交点で切り離す(消さない)
    Break,
    /// 選んでおいた要素を写す対称の線を選ぶ
    MirrorAxis,
    /// 2 本の線分の角を円弧で丸める
    Fillet,
    /// 両端の中心と幅で描く長穴
    Slot,
    /// 中心と頂点で描く正多角形
    Polygon,
    /// 要素をクリックして寸法を付ける(D)。種類は選んだ要素で決まる
    Dimension,
    /// 1 辺の両端と、向かいの辺の位置で描く傾いた矩形
    ThreePointRectangle,
    /// 直径の両端で描く円
    TwoPointCircle,
    /// 円周の上の 3 点で描く円
    ThreePointCircle,
    /// 3 つ(線分・円・円弧)に接する円
    TangentCircle,
    /// 2 つ(線分・円・円弧)に接する円
    TwoTangentCircle,
    /// 中心、長軸の端、短軸の大きさで描く楕円
    Ellipse,
    /// 点を通るスプライン(フィット点)
    FitSpline,
    /// 制御点のスプライン
    ControlSpline,
    /// 始点、終点、両端の接線が交わる点と形の値(rho)で描く円錐曲線
    Conic,
    /// 2 つの曲線の端を、接する(G1)か曲率もそろう(G2)スプラインでつなぐ
    Blend,
    /// 中点と一方の端で描く線分
    MidpointLine,
    /// 1 辺の両端と置く側で描く正多角形
    EdgePolygon,
    /// 中心線の円弧を 3 点で決める長穴
    ThreePointArcSlot,
    /// 始点、終点、途中の 1 点で描く円弧
    ThreePointArc,
    /// 中心と辺の中点で描く外接の正多角形
    OuterPolygon,
    /// 両端の先(全長)と幅で描く長穴
    OverallSlot,
    /// 中心、端の中心、幅で描く長穴
    CenterSlot,
    /// 円弧の中心、始まり、終わり、幅で描く、円弧に沿う長穴
    ArcSlot,
    /// 2 本の線分の角を落とす
    Chamfer,
    /// 選んだ要素を直線に並べる
    LinearPattern,
    /// 選んだ要素を点のまわりに並べる
    CircularPattern,
    /// 選んだ要素を動かす
    Move,
    /// 選んだ要素を回す
    Rotate,
    /// 選んだ要素を拡大縮小する
    Scale,
    /// 拘束を先に選び、要素を順にクリックして付ける(名前は帯の拘束と同じ。constraint_tool.rs)
    Constraint(&'static str),
    /// つながった線を、カーソルの側へ距離だけずらして写す(O。offset_tool.rs)
    Offset,
    /// 要素をクリックして、長さ・距離・角度を測る(I。寸法は付けない)
    Measure,
    /// ボディの辺・角・円の中心をクリックしてスケッチへ写す
    Project,
    /// ボディをスケッチの平面で切った断面をクリックして線にする
    Intersect,
    /// 面の上のスケッチで、原点にする面の角(円の中心)をクリックする(パネルの「原点」から)
    Origin,
}

impl Tool {
    /// 今の道具で次にすることの案内。
    pub fn hint(self, step: usize) -> &'static str {
        match (self, step) {
            (Tool::Select, _) => {
                "クリックで選ぶ(Shift で足す、Ctrl で足し外し)。右クリックでメニュー。領域を選んで E で押し出す。線・点はドラッグで動かし、空いた所からのドラッグで囲む。長押しか Alt+クリックで重なりから選ぶ"
            }
            (Tool::Line, 0) => "始点をクリック",
            (Tool::Line, _) => {
                "終点をクリック。長さ・角度は打って Tab で固定(Enter で作る)。続けて描け、始点に戻る・ダブルクリック・何も打たない Enter で区切る"
            }
            (Tool::Circle, 0) => "中心をクリック",
            (Tool::Circle, _) => "円周の位置をクリック、または直径を打って Enter",
            (Tool::Point, _) => "点を置く位置をクリック",
            (Tool::Text, _) => {
                "文字を置く位置をクリック(そこに点を置き、スケッチを終えて文字を作る。点は拘束で動かせる)"
            }
            (Tool::Rectangle, 0) => "1 つ目の隅をクリック",
            (Tool::Rectangle, _) => {
                "対角の隅をクリック。幅・高さは打って Tab で次の欄(固定)、Enter で作る"
            }
            (Tool::RoundedRectangle, 0) => "1 つ目の隅をクリック",
            (Tool::RoundedRectangle, _) => {
                "対角の隅をクリック。幅・高さ・角の半径は打って Tab で次の欄(半径の既定は短い辺の 1/5)、Enter で作る"
            }
            (Tool::CenterRectangle, 0) => "中心をクリック",
            (Tool::CenterRectangle, _) => {
                "隅をクリック。幅・高さは打って Tab で次の欄(固定)、Enter で作る"
            }
            (Tool::Arc, 0) => "中心をクリック",
            (Tool::Arc, 1) => "始点をクリック、または半径を打って Enter",
            (Tool::Arc, _) => {
                "終点をクリック。始点からカーソルを動かした向き(時計回り・反時計回り)に描く。角度も打てる"
            }
            (Tool::TangentArc, 0) => "線分か円弧の端をクリック",
            (Tool::Project, _) => {
                "写すボディの辺・角・円の中心か面をクリック(パネルで写す物「面・辺・点」「シルエット」「辺と角すべて」と投影リンクを選ぶ)"
            }
            (Tool::Intersect, _) => {
                "スケッチの平面で切ったボディの断面(色の付いた線)をクリックすると、その線を写す(パネルで「ボディ全体」と投影リンクを選ぶ)"
            }
            (Tool::TangentArc, _) => "円弧の終わりをクリック",
            (Tool::Origin, _) => {
                "原点にする面の角(角の無い面は円の中心)をクリック。描いた形は動かない。Esc でやめる"
            }
            (Tool::Trim, _) => {
                "消したい区間をクリック(交点で区切る)。押したまま引くと、なぞった区間をまとめて消す"
            }
            (Tool::Extend, _) => "延ばす線の端の近くをクリック(次に当たる線まで延ばす)",
            (Tool::Break, _) => "分けたい区間をクリック(交点で切り離す)",
            (Tool::MirrorAxis, _) => {
                "対称の線にする線分をクリック(中心線があれば Enter でそれを使う。Shift+クリックで選び直す)"
            }
            (Tool::Slot, 0) => "片方の端の中心をクリック",
            (Tool::Slot, 1) => "もう片方の端の中心をクリック、または長さを打って Enter",
            (Tool::Slot, _) => "幅の位置をクリック、または幅を打って Enter",
            (Tool::Polygon, 0) => "中心をクリック",
            (Tool::Polygon, _) => "頂点をクリック。辺の数(既定 6)と半径は打って Tab で移る",
            (Tool::Dimension, _) => {
                "寸法を付ける要素をクリック(線分、円、点 2 つ、点と線分、線分 2 本)し、置く位置をクリックして値を打ち Enter"
            }
            (Tool::Constraint(_), _) => {
                "拘束を付ける要素を順にクリック(付けた後も続く。Esc で終わる)"
            }
            (Tool::Fillet, _) => {
                "丸める角(2 本の線分が交わる点)か、線分を 2 本クリック。半径は数字を打つ(空なら寸法の値の欄か 2 mm)"
            }
            (Tool::Measure, _) => {
                "測る要素をクリック(1 つか 2 つ。3 つ目で測り直す)。結果は下に出る。寸法は付けない"
            }
            (Tool::Offset, 0) => {
                "ずらす線をクリック(端でつながった線は全部ずれる)。選んでから O でもよい"
            }
            (Tool::Offset, _) => {
                "カーソルの側へずらす。距離を打って Enter か、クリックで作る。右クリックで片側・両側・対称"
            }
            (t, step) => crate::more_tools::hint(t, step),
        }
    }
}

/// 線を描く道具か(補助線で描く切り替えが効く)。
pub(crate) fn draws(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::Line
            | Tool::Circle
            | Tool::Rectangle
            | Tool::CenterRectangle
            | Tool::RoundedRectangle
            | Tool::Arc
            | Tool::TangentArc
            | Tool::Slot
            | Tool::Polygon
            | Tool::ThreePointRectangle
            | Tool::TwoPointCircle
            | Tool::ThreePointCircle
            | Tool::TangentCircle
            | Tool::TwoTangentCircle
            | Tool::MidpointLine
            | Tool::EdgePolygon
            | Tool::ThreePointArcSlot
            | Tool::Ellipse
            | Tool::FitSpline
            | Tool::ControlSpline
            | Tool::Conic
            | Tool::Blend
            | Tool::ThreePointArc
            | Tool::OuterPolygon
            | Tool::OverallSlot
            | Tool::CenterSlot
            | Tool::ArcSlot
    )
}

/// 形を 1 つ作ったら終える道具か。描く道具は続ける(Kie 2026-10-15。使っている道具は
/// リボンと図の左上の案内で見せる)。選んだ要素を並べる・動かす道具は、選びが変わるので 1 回で終える。
pub(crate) fn finishes_on_make(tool: Tool) -> bool {
    crate::more_tools::needs_selection(tool)
}

/// キーの行(画面のキーの表 `jc_ui::keys` の名前)と道具(`None` は押し出し)。既定は多くの CAD で
/// 使われている 1 文字(L・C・R・P・T・D・O・E)で、設定の「キーの割り当て」で付け替えられる。
pub(crate) const KEY_TOOLS: [(&str, Option<Tool>); 8] = [
    ("sketch_line", Some(Tool::Line)),
    ("sketch_circle", Some(Tool::Circle)),
    ("sketch_rect", Some(Tool::Rectangle)),
    ("sketch_point", Some(Tool::Point)),
    ("sketch_trim", Some(Tool::Trim)),
    ("sketch_dimension", Some(Tool::Dimension)),
    ("sketch_offset", Some(Tool::Offset)),
    ("extrude", None),
];

/// 道具に割り当てたキーの名前(無ければ `None`)。
pub(crate) fn tool_key(t: Tool) -> Option<String> {
    KEY_TOOLS
        .iter()
        .find(|(_, x)| *x == Some(t))
        .map(|(id, _)| jc_ui::keys::label(id))
        .filter(|k| !k.is_empty())
}

/// 寸法の値を求める関数(式 → 値)。ドラッグ中の解き直しに使う。
pub(crate) type DimValues = HashMap<jc_sketch::ConstraintId, f64>;

/// 面の上の線分(スケッチの平面の座標)。
pub(crate) type Segments = [[[f64; 2]; 2]];

/// ボディをスケッチの平面で切った断面(交差の道具とスライス)。
pub(crate) struct Section {
    /// 線分・円弧・円に直し、元の面を付けた断面(交差で写す)
    pub items: Vec<jc_sketch::RefItem>,
    /// 網を切った細かい線分(描くのと、クリックの当たりに使う)
    pub segs: Vec<[[f64; 2]; 2]>,
}

/// 道具ごとの吸い付く先。描く道具は点(原点・角・中心)と中点に、選ぶ・寸法の道具は
/// 既存の形の角・辺にも当たる。並べる・動かす道具や直す道具には足さない。
pub(crate) fn snap_wants(tool: Tool) -> crate::snap::Wants {
    // 描く道具はみな、原点・角・中点・交点へ吸い付く(ブレンド曲線は曲線の端だけを選ぶ)
    let draw = (draws(tool) || matches!(tool, Tool::Point | Tool::Text)) && tool != Tool::Blend;
    let pick = matches!(
        tool,
        Tool::Select | Tool::Dimension | Tool::Constraint(_) | Tool::Project
    );
    crate::snap::Wants {
        points: draw || pick,
        edges: pick,
        midpoints: draw,
        crossings: draw,
    }
}
