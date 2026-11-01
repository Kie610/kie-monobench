//! 線のアイコン(24×24 の枠に描いた SVG の道筋)。形は画面の試作と同じ。
//!
//! どの操作にどのアイコンを使うかは、モジュールが操作と種類の `icon` に名前で書く
//! (画面はその名前の絵を描くだけ)。知らない名前は、名前の頭の 1 文字を丸で囲んで出す。
//!
//! 絵は 2 層で描ける: 土台の線([`PATHS`])と、道具が変える部分([`ACCENTS`]、役の色で描く)。
//! 変わる部分の無い絵(調べる・出すの道具と、まだ分けていない絵)は土台だけの 1 層。決まりは
//! Design System「JC の見た目」のアイコンの節(土台は `icon_base` の太さ 1.5、変わる部分は役の色の
//! 太さ 2。使えない時は両方を 1 色)。

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use std::collections::HashMap;
use std::sync::OnceLock;

/// 名前と土台の道筋(SVG の path の d)。
const PATHS: &[(&str, &str)] = &[
    // 試作にないもの
    (
        "help",
        "M12 4a8 8 0 1 0 0 16a8 8 0 1 0 0-16M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .8-1 1.5v.7M12 17h.01",
    ),
    ("chevl", "M15 6l-6 6 6 6"),
    // 閉じた目(3D のビューで隠しているもの。まぶたの線とまつげ)
    (
        "eyeoff",
        "M2 10c3 4 6 6 10 6s7-2 10-6M5 13l-2 3M9 15.5l-1 3M15 15.5l1 3M19 13l2 3",
    ),
    ("chevr", "M9 6l6 6-6 6"),
    ("check", "M5 12l5 5 9-10"),
    ("warn", "M12 3l9 16H3zM12 9v5M12 17h.01"),
    ("origin", "M12 12h.01M4 20l8-8M12 12V3M12 12h9"),
    // 組立: 部品(重ねた箱)、ジョイント(2 枚の板と軸)、干渉(重なった四角)、部品表(表)
    ("component", "M3 11h9v9H3z"),
    ("joint", "M3 7h7v10H3zM14 7h7v10h-7z"),
    (
        "interference",
        "M3 3h11v11H3zM10 10h11v11H10zM10 10h4v4h-4z",
    ),
    ("table", "M3 5h18v14H3zM3 10h18M3 15h18M9 5v14"),
    // アニメーション: 自動分解(真ん中の箱から 4 方へ矢印)、ワンステップ(揃った 3 本の帯)、
    // 順次(段々にずれた 3 本の帯)、トレイルライン(箱と、元の位置へ続く点線)
    (
        "explode",
        "M9 9h6v6H9zM12 2v4M10 4l2-2 2 2M12 22v-4M10 20l2 2 2-2M2 12h4M4 10l-2 2 2 2M22 12h-4M20 10l2 2-2 2",
    ),
    ("onestep", "M4 6h16M4 12h16M4 18h16"),
    ("sequential", "M3 6h7M9 12h7M15 18h6"),
    (
        "trail",
        "M14 4h6v6h-6zM4 20l1.5-1.5M7.5 16.5l1.5-1.5M11 13l1.5-1.5",
    ),
    // 規格部品(頭の付いたボルト)
    ("fastener", "M6 3h12v5H6zM9 8h6v13H9z"),
    // リンクで組み込んだ部品(横に並べてつないだ 2 つの輪)
    (
        "link",
        "M5 9h5a3 3 0 0 1 0 6H5a3 3 0 0 1 0-6zM14 9h5a3 3 0 0 1 0 6h-5a3 3 0 0 1 0-6z",
    ),
    // 試作の形
    ("lock", "M6 11h12v9H6zM9 11V8a3 3 0 0 1 6 0v3"),
    ("pin", "M9 3h6l-1 6 3 3H7l3-3zM12 12v9"),
    ("sketch", "M13 5l3 3-8 8H5v-3z"),
    ("sketchFace", "M4 4h10v10H4z"),
    // 3D スケッチ(立方体の角の 3 本の軸と鉛筆)
    ("sketch3d", "M5 4v14h14M5 18l-2 3"),
    // 3D スケッチの拘束と寸法: 軸に沿う(X・Y・Z の文字と矢印)
    ("alongx", "M6 4l6 8M12 4l-6 8"),
    ("alongy", "M6 4l3 4 3-4M9 8v4"),
    ("alongz", "M6 4h6l-6 8h6"),
    // 平面の上・平面に平行・平面との距離・平面との角度・面の角度
    ("onplane", "M3 16l6-6h12l-6 6z"),
    ("parplane", "M3 18l5-4h13l-5 4z"),
    ("pdist", "M3 19l5-4h13l-5 4z"),
    ("pangle", "M3 19l5-4h13l-5 4z"),
    ("fangle", "M3 19l5-4h13l-5 4zM8 15l4-11h9l-5 11"),
    // 軸の向きの差(2 点の高さの差など)
    ("axisdist", "M5 4v16M19 4v16"),
    // 作図面(XY・YZ・ZX)
    ("wplane", "M9 10V4M3 16V10l6-6"),
    ("extrude", "M4 16l8 4 8-4-8-4z"),
    ("revolve", "M12 3v18"),
    // スイープ: 下の丸い断面と、それを運ぶ曲がった経路の矢印
    ("sweep", "M3 18a3 3 0 1 0 6 0a3 3 0 1 0-6 0"),
    // ロフト: 下の四角と上の楕円を、外側の 2 本の線でつなぐ
    ("loft", "M3 17h10v4H3zM14 6a4 2 0 1 0 8 0a4 2 0 1 0-8 0"),
    ("hole", "M4 6h16v12H4z"),
    // ねじ: 軸と斜めのねじ山
    ("thread", "M8 3h8v18H8z"),
    ("fillet", "M4 20V11M11 4h9v16H4"),
    ("chamfer", "M4 20V10M10 4h10v16H4"),
    // シェル: 上の開いた、厚みのある箱
    ("shell", "M4 7v13h16V7"),
    // プレス/プル: 面と、上下の矢印
    ("presspull", "M4 13h16v7H4z"),
    ("boolean", "M3 12a5 5 0 1 0 10 0a5 5 0 1 0-10 0"),
    ("linpat", "M4 4h6v6H4z"),
    ("circpat", "M10 3h4v4h-4z"),
    ("mirror", "M12 3v18M9 7L4 17h5z"),
    // ボディを分割: 斜めの平面で離れた 2 つの塊
    ("split", "M3 12V4h18v5z"),
    // 移動/コピー: 4 方向の矢印
    (
        "move",
        "M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3",
    ),
    // キャンバス(下絵): 額の中の山と日
    ("canvas", "M3 5h18v14H3zM3 16l5-5 4 4 3-3 6 6M15 9h.01"),
    // 光源: 電球と口金と光の筋
    (
        "bulb",
        "M8 14a6 6 0 1 1 8 0c-1 1-1.5 2-1.5 3h-5c0-1-.5-2-1.5-3zM9.5 20h5M12 6v3",
    ),
    // デカール: 角のめくれた貼り紙
    ("decal", "M4 4h16v10l-6 6H4zM14 20v-6h6M8 10l2-2 3 3"),
    // 尺度: 小さな箱と、それを広げた大きな枠と、斜めの矢印
    ("scale", "M3 14h7v7H3z"),
    ("draft", "M4 20h16M7 20V4h6M13 4v6"),
    ("replace_face", "M3 20h12v-8H3z"),
    ("delete_face", "M4 20V9l5-5h11v16zM4 9h5V4"),
    // 面をオフセット: 曲がった面と、外へ出す矢印と、動かした先の面
    ("offset_face", "M3 20c4-7 14-7 18 0"),
    ("textf", "M3 18l4-12h14l-4 12z"),
    ("step", "M12 3v11M8 10l4 4 4-4M4 16v4h16v-4"),
    // DXF(図面の紙と線)・SVG(曲線と端の点)
    ("dxf", "M5 3h9l5 5v13H5zM14 3v5h5M8 17l3-5 2 3 3-4"),
    ("svg", "M4 18C8 4 16 20 20 6M3 17h2v2H3zM19 5h2v2h-2z"),
    ("measure", "M3 15L15 3l6 6L9 21zM7 11l2 2M10 8l2 2M13 5l2 2"),
    // 2 点の間の寸法(測る道具)
    (
        "distance",
        "M4 6v12M20 6v12M4 12h16M7.5 9L4 12l3.5 3M16.5 9L20 12l-3.5 3",
    ),
    // 断面(箱を横に切った線と、切り口の斜線)
    ("section", "M4 8l8-4 8 4v8l-8 4-8-4zM2 14l10-5 10 5"),
    ("param", "M4 7h16M4 12h16M4 17h16"),
    ("setup", "M4 9l8-4 8 4v7l-8 4-8-4zM4 9l8 4 8-4M12 13v7"),
    ("face", "M3 14h18v6H3z"),
    ("pocket", "M3 5h18v15H3z"),
    ("contour", "M8 16V10l2-2h6v8z"),
    ("adaptive", "M3 5h18v15H3z"),
    ("chamferop", "M4 7h10M20 13v5H4V7"),
    ("engrave", "M3 6h2M19 6h2"),
    ("vcarve", "M9 6h6"),
    // トレース(線の上をなぞる波)とねじ切り(2 本の壁と斜めの山)。画板「JC の画面」の加工の案と同じ
    ("trace", "M3 20h18"),
    ("threadmill", "M7 4v16M17 4v16"),
    ("raster", "M3 20c3-10 15-10 18 0"),
    ("waterline", "M3 20c3-10 15-10 18 0"),
    ("pencil", "M4 4v16h16"),
    ("rough3d", "M3 20c3-10 15-10 18 0"),
    ("flat3d", "M3 17h6l3-7 3 7h6"),
    ("spiral3d", "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18"),
    ("radial3d", "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18"),
    // ランプ(3D): 曲面の壁を斜めに下りる線(画板「JC の画面」の「傾斜」の絵)
    ("ramp3d", "M3 20c3-10 15-10 18 0"),
    // スキャロップ(3D): 曲面に沿って同じ間隔で重なる弧(画板「JC の画面」の絵)
    ("scallop3d", "M3 20c3-10 15-10 18 0"),
    // モーフィング スパイラル(3D): 外形に沿って縮む渦巻き(画板「JC の画面」の「モーフ スパイラル」の絵)
    ("morph3d", "M4 6h16v13H4z"),
    // 投影(3D): 曲面へ上から落とす矢印(画板「JC の画面」の絵)
    ("project3d", "M3 20c3-10 15-10 18 0"),
    ("drill", "M4 15h16v6H4z"),
    ("pins", "M4 15h16v6H4z"),
    ("simulate", "M4 17h16v4H4zM11 3h2v8l-1 3-1-3z"),
    ("gcode", "M6 3h9l4 4v14H6zM9 11h7M9 15h7"),
    ("sheet", "M4 4h16v16H4zM4 9h16M10 9v11"),
    ("tools", "M10 3h4v8h-4zM8 11h8l-1 10h-6z"),
    ("rest", "M3 6h18v13H3zM9 10h6v5H9zM13 3v4"),
    // 加工の間の停止(手動 NC): 丸の中の一時停止の印
    ("manual_nc", "M6 3h9l4 4v14H6zM15 3v4h4"),
    // 加工のパターン: 並べた 4 つの四角
    (
        "pattern",
        "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z",
    ),
    (
        "gear",
        "M18.7 10L21.3 10.2L21.3 13.8L18.7 14L18.2 15.3L19.9 17.3L17.3 19.9L15.3 18.2L14 18.7L13.8 21.3L10.2 21.3L10 18.7L8.7 18.2L6.7 19.9L4.1 17.3L5.8 15.3L5.3 14L2.7 13.8L2.7 10.2L5.3 10L5.8 8.7L4.1 6.7L6.7 4.1L8.7 5.8L10 5.3L10.2 2.7L13.8 2.7L14 5.3L15.3 5.8L17.3 4.1L19.9 6.7L18.2 8.7zM12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6",
    ),
    ("bell", "M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 20h4"),
    ("search", "M10 4a6 6 0 1 0 0 12a6 6 0 1 0 0-12M15 15l5 5"),
    ("undo", "M9 14L4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3"),
    ("redo", "M15 14l5-5-5-5M20 9H10a6 6 0 0 0 0 12h3"),
    ("save", "M5 4h11l3 3v13H5zM8 4v5h7V4M8 20v-6h8v6"),
    ("hist", "M12 7v5l3 3M4 12a8 8 0 1 0 2.3-5.7M4 4v4h4"),
    ("file", "M4 6h16M4 12h16M4 18h16"),
    ("chev", "M7 10l5 5 5-5"),
    ("play", "M7 4l12 8-12 8z"),
    ("pause", "M7 5h3v14H7zM14 5h3v14h-3z"),
    ("first", "M5 5v14M19 5v14l-7-7zM12 5v14l-7-7z"),
    ("last", "M19 5v14M5 5v14l7-7zM12 5v14l7-7z"),
    ("prev", "M18 5v14l-9-7zM6 5v14"),
    ("next", "M6 5v14l9-7zM18 5v14"),
    ("back", "M15 5l-7 7 7 7"),
    ("fwd", "M9 5l7 7-7 7"),
    ("orbit", "M12 5a7 3 0 1 0 0.1 0M12 3v4M5 12c0 4 3 7 7 7"),
    (
        "pan",
        "M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3",
    ),
    (
        "zoom",
        "M10 4a6 6 0 1 0 0 12a6 6 0 1 0 0-12M15 15l5 5M7 10h6M10 7v6",
    ),
    ("fit", "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"),
    // 正面から見る(面の四角と、斜めから面の真ん中へ向かう矢印)
    (
        "lookat",
        "M9 9h11v11H9zM3 3l11.5 11.5M14.5 14.5h-5M14.5 14.5v-5",
    ),
    ("display", "M3 5h18v12H3zM8 21h8"),
    // 表示の形と投影(陰をつけた箱)
    (
        "visual",
        "M4 8l8-4 8 4v8l-8 4-8-4zM4 8l8 4 8-4M12 12v8M15 14.5v3M17.5 13v3",
    ),
    (
        "eye",
        "M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6",
    ),
    ("x", "M6 6l12 12M18 6L6 18"),
    ("folder", "M3 6h6l2 2h10v11H3z"),
    // データパネル(3×3 の升)
    (
        "apps",
        "M4 4h4v4H4zM10 4h4v4h-4zM16 4h4v4h-4zM4 10h4v4H4zM10 10h4v4h-4zM16 10h4v4h-4zM4 16h4v4H4zM10 16h4v4h-4zM16 16h4v4h-4z",
    ),
    ("plus", "M12 5v14M5 12h14"),
    ("box", "M4 8l8-4 8 4v8l-8 4-8-4zM4 8l8 4 8-4M12 12v8"),
    // ボディ(中の詰まった立体。箱の上と左の面に陰の線。ブラウザのボディの行)
    (
        "body",
        "M4 8l8-4 8 4v8l-8 4-8-4zM4 8l8 4 8-4M12 12v8M7 6.5l8 4M10 5l8 4M6.7 9.4v8M9.3 10.7v8",
    ),
    ("line", "M5 19h.01M19 5h.01"),
    ("rect", "M4 6h.01M20 18h.01"),
    ("crect", "M12 12h.01"),
    ("rrect", "M4 6h.01M20 18h.01"),
    ("circle", "M12 12h.01"),
    ("arc", "M5 18h.01M19 8h.01"),
    ("slot", "M8 12h.01M16 12h.01"),
    ("polygon", "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18"),
    ("point", "M12 12h.01"),
    ("trim", "M3 12h5M16 12h5M8 5v14M16 5v14"),
    ("skfillet", "M5 19v-7M12 5h7"),
    (
        "skmirror",
        "M12 3v3M12 9v2M12 14v2M12 19v2M9 6c-4 3-4 9 0 12",
    ),
    ("construct", "M4 20L20 4"),
    ("fix", "M5 11h14v9H5z"),
    ("horiz", "M4 12h16M4 9v6M20 9v6"),
    ("vert", "M12 4v16M9 4h6M9 20h6"),
    ("coinc", "M4 19l8-7M12 12l8-7"),
    ("parallel", "M6 20L12 4"),
    ("perp", "M5 19h14"),
    ("equal", "M4 6h7M13 18h7"),
    ("tangent", "M12 3a5 5 0 1 0 0 10a5 5 0 1 0 0-10"),
    ("midpoint", "M4 12h16"),
    ("online", "M4 16L20 8"),
    ("dim", "M4 14v7M20 14v7"),
    // スケッチの道具(▾ の一覧にもアイコンを出すため、道具ごとに分ける)
    ("rect3", "M3 14h.01M12 5h.01M20 13h.01"),
    ("circle2", "M5 12h14"),
    ("circle3", "M12 5h.01M5.9 15.5h.01M18.1 15.5h.01"),
    ("tcircle", "M12 3l9.5 16.5H2.5z"),
    ("arc3", "M4 17h.01M12 9h.01M20 17h.01"),
    ("tarc", "M3 18h8"),
    ("slotov", "M1.5 12h2M20.5 12h2"),
    ("slotc", "M12 12h.01M8 12h8"),
    ("slotarc", "M12 17h.01"),
    ("polyout", "M12 7.5a4.5 4.5 0 1 0 0 9a4.5 4.5 0 1 0 0-9"),
    ("skchamfer", "M4 20V11M11 4h9"),
    ("project", "M4 15l8-4 8 4-8 4z"),
    // 交差(箱を平面で切った線)
    ("intersect", "M6 4h10l3 3v13H9l-3-3z"),
    // 交差曲線(線を伸ばして表と交える)
    ("isectcurve", "M12 6v9M4 19l8-4 8 4"),
    // サーフェスに投影(線を曲がった表へ写す)
    ("projsurf", "M4 4h16M3 19c5-4 13-4 18 0"),
    ("ellipse", "M12 12h.01"),
    ("spline", "M3 18h.01M14 12h.01M21 6h.01"),
    ("splinectl", "M3 18L8 5l8 14 5-13"),
    ("conic", "M4 19L4 4h16"),
    ("blend", "M3 16h6M15 8h6"),
    ("midline", "M12 12h.01M10 10l4 4"),
    ("tcircle2", "M3 20h18M3 20L13 3"),
    ("polyedge", "M7 21h10"),
    ("slotarc3", "M12 8h.01"),
    ("extend", "M4 20l9-9M13 4h7v7"),
    ("curvature", "M3 18h8"),
    ("skbreak", "M4 20l6.5-6.5M13.5 10.5L20 4"),
    ("skrotate", "M12 12h.01"),
    ("skscale", "M4 13h7v7H4z"),
    ("skoffset", "M4 20V10a6 6 0 0 1 6-6h10"),
    ("skoffin", "M4 20V10a6 6 0 0 1 6-6h10"),
    ("dashline", "M4 20h.01M20 4h.01"),
    ("select", "M6 3l12 9-5.5 1 3.2 6.2-2.2 1.1-3.1-6.3L6 18z"),
    ("trash", "M5 7h14M9.5 7V4.5h5V7M7 7l1 13h8l1-13"),
    // スケッチの拘束(図の上の印とリボン)
    ("oncircle", "M12 7a6 6 0 1 0 0 12a6 6 0 1 0 0-12"),
    ("concentric", "M12 4a8 8 0 1 0 0 16a8 8 0 1 0 0-16"),
    ("collinear", "M3 19l6.5-5"),
    ("finish", "M5 12l5 5 9-10"),
    ("stock", "M4 8l8-4 8 4v8l-8 4-8-4z"),
    ("path", "M4 18c4 0 4-12 8-12s4 12 8 12"),
    // 解析の研究: 壁に付けた梁を上から押す・温度計・壁から揺れる波・筋交いの骨組み
    (
        "fea_static",
        "M4 4v16M4 9h14v4H4M16 2v5M14 5l2 2 2-2M7 18c4 0 8-1 11-3",
    ),
    (
        "fea_thermal",
        "M10 4a2 2 0 0 1 4 0v10a4 4 0 1 1-4 0zM12 10v7M17 5h3M17 9h3",
    ),
    ("fea_modal", "M4 4v16M4 12c3-7 5-7 8 0s5 7 8 0"),
    ("fea_topology", "M3 6h18v12H3zM3 18l6-12 6 12 6-12"),
    // 流れの研究: 翼の断面と上向きの矢印・物を回り込む流れの線・箱の中の流れ
    (
        "flow_wing",
        "M3 15c4-4 12-5 18-1-6 1-13 2-18 1zM12 10V3M9 6l3-3 3 3",
    ),
    (
        "flow_2d",
        "M2 7h6c3 0 4 3 8 3h6M2 17h6c3 0 4-3 8-3h6M12 12a2 2 0 1 0 .01 0",
    ),
    (
        "flow_3d",
        "M4 8l8-4 8 4v8l-8 4-8-4zM4 8l8 4 8-4M12 12v8M1 12h4",
    ),
    // 音の放射・散乱: 箱の 1 面から外へ広がる音の弧
    (
        "wave_radiate",
        "M3 7h6v10H3zM12 9a4 4 0 0 1 0 6M15 6a8 8 0 0 1 0 12M18 3a12 12 0 0 1 0 18",
    ),
    // 音の仕事を解く: 球から外へ広がる音の弧
    (
        "wave_solve",
        "M7 12a3 3 0 1 0 .01 0M13 7a7 7 0 0 1 0 10M17 4a11 11 0 0 1 0 16",
    ),
    ("tool", "M10 3h4v10h-4zM9 13h6l-3 8z"),
    ("gouge", "M12 3l9 16H3zM12 9v5M12 17h.01"),
    ("remain", "M4 18h16M6 14h3v4H6zM13 11h4v7h-4z"),
    ("time", "M12 4a8 8 0 1 0 0 16a8 8 0 1 0 0-16M12 8v4l3 2"),
    ("lib", "M4 4h4v16H4zM10 4h4v16h-4zM16 5l4 1-3 15-4-1z"),
    ("copy", "M8 8h11v12H8zM5 16V4h11"),
    // 右クリックのメニュー: 編集(鉛筆)、名前変更(字の枠と字を打つ所の縦線)、貼り付け(紙挟み)、
    // 抑制(斜線の入った丸)、上へ・下へ、ほかを選ぶ(重ねた板)、繰り返す(回る 2 本の矢印)、
    // 選択表示(点線の枠の中の箱)、グループから出す(枠から出る矢印)
    ("edit", "M4 20l1-4L16 5l3 3L8 19zM14 7l3 3"),
    ("rename", "M3 8h11v8H3zM18 5v14M16 5h4M16 19h4"),
    ("paste", "M9 3h6v3H9zM9 4.5H5V21h14V4.5h-4"),
    (
        "suppress",
        "M12 4a8 8 0 1 0 0 16a8 8 0 1 0 0-16M6.5 6.5l11 11",
    ),
    ("up", "M12 19V5M6 11l6-6 6 6"),
    ("down", "M12 5v14M6 13l6 6 6-6"),
    ("layers", "M12 4l9 5-9 5-9-5zM3 14l9 5 9-5"),
    (
        "repeat",
        "M5 11a7 7 0 0 1 12-4.5M17 3v4h-4M19 13a7 7 0 0 1-12 4.5M7 21v-4h4",
    ),
    (
        "isolate",
        "M9 9h6v6H9zM3 6V3h3M10 3h4M18 3h3v3M21 10v4M21 18v3h-3M14 21h-4M6 21H3v-3M3 14v-4",
    ),
    ("ungroup", "M14 5H4v14h10M9 12h12M17 8l4 4-4 4"),
    // お気に入り(星)
    (
        "star",
        "M12 3l2.6 5.6 6.1.7-4.5 4.2 1.2 6L12 16.6l-5.4 2.9 1.2-6-4.5-4.2 6.1-.7z",
    ),
    // 物理材料(分銅)
    (
        "material",
        "M8 8h8l3 12H5zM12 4a2 2 0 1 0 0 4a2 2 0 1 0 0-4",
    ),
    // 複製(重ねた 2 枚と足す印)、切り取り(鋏)
    ("duplicate", "M4 4h10v10H4zM10 10h10v10H10zM15 13v4M13 15h4"),
    (
        "cut",
        "M6 3.5a2.5 2.5 0 1 0 0 5a2.5 2.5 0 1 0 0-5M6 15.5a2.5 2.5 0 1 0 0 5a2.5 2.5 0 1 0 0-5M8 7.5l12 9M8 16.5l12-9",
    ),
    ("export", "M12 15V3M8 7l4-4 4 4M4 14v6h16v-6"),
    // レンダリング: 床に置いた玉と、その照り
    (
        "render",
        "M12 4a7 7 0 1 0 0.1 0M8.5 8.5a4 4 0 0 1 3-2M3 21h18M7 21c1-1.5 3-2 5-2s4 .5 5 2",
    ),
    ("download", "M12 3v12M7 10l5 5 5-5M4 19h16"),
    // アニメーションの外観の段(しずくと、つやの線)
    (
        "appearance",
        "M12 3c3 4 6 7.5 6 11a6 6 0 0 1-12 0c0-3.5 3-7 6-11zM9 14a3 3 0 0 0 3 3",
    ),
    // 外観の窓(色見本の札を扇に広げた形)
    ("swatch", "M4 4h6v16H4z"),
    // アニメーションのビューの段(撮る道具: 箱とレンズの向き)
    ("camera", "M3 7h12v10H3zM15 10.5l6-3.5v10l-6-3.5"),
    // 画板(2026-10-21 の「JC の画面」のアイコンの板)で足した絵
    ("volume", "M4 8l8-4 8 4v8l-8 4-8-4zM4 8l8 4 8-4M12 12v8"), // 体積を計測
    ("fixture", "M4 13h16v7H4z"),                               // 固定具を置く
    ("holeop", "M4 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0"),            // 穴から加工を作る
    ("campat", "M3 4h7v7H3zM5 7.5h3"),                          // 加工のパターン
    ("gcodeview", "M6 3h9l4 4v14H6zM15 3v4h4M9 11h7M9 15h7"),   // G-code を見る
    ("gcodecheck", "M6 3h9l4 4v14H6zM15 3v4h4M9 14l2 2 4-4"),   // G-code で確かめる
    (
        "gcodeout",
        "M6 3h9l4 4v14H6zM15 3v4h4M12 18v-7M9.5 13.5L12 11l2.5 2.5",
    ), // G-code を書き出す
    (
        "gcodeas",
        "M6 3h9l4 4v14H6zM15 3v4h4M9 17h.01M12 17h.01M15 17h.01",
    ), // G-code を名前を付けて書き出す
    (
        "gcodesort",
        "M6 3h9l4 4v14H6zM15 3v4h4M9 11h7M9 14h5M9 17h3",
    ), // 工具の順に並べ替えて書き出す
    (
        "sheethtml",
        "M4 4h16v16H4zM4 9h16M9 13l-2 2 2 2M15 13l2 2-2 2",
    ), // 加工指示書(HTML)を書き出す
    ("machine", "M4 20h16M6 20V5h12v15M10 5v6M9 11h2"),         // 機械の設定
    ("hv", "M3 19h12M19 3v12"),                                 // 水平/垂直
    ("symmetric", "M12 3v18M4 7l4 10"),                         // 対称
    ("movecopy", "M3 12h7v7H3z"),                               // 移動/コピー
    (
        "stepout",
        "M4 9v11h11M13 11l7-7M15 4h5v5M6 14l3-1.5 3 1.5v3.5l-3 1.5-3-1.5zM6 14l3 1.5 3-1.5M9 15.5V19",
    ), // STEP を書き出す
    (
        "stlout",
        "M4 9v11h11M13 11l7-7M15 4h5v5M6 19l3-6 3 6zM9 13v6",
    ), // STL を書き出す
    (
        "3mfout",
        "M4 9v11h11M13 11l7-7M15 4h5v5M6 19h6M6 16.5h6M6 14h6",
    ), // 3MF を書き出す
    ("dxfout", "M4 9v11h11M13 11l7-7M15 4h5v5M6 19l3-5 2 3 1.5-2"), // DXF を書き出す
    (
        "svgout",
        "M4 9v11h11M13 11l7-7M15 4h5v5M6 19c1-5 4-5 6-6M6 19h.01M12 13h.01",
    ), // SVG を書き出す
    ("splitbody", "M4 5h16v14H4z"),                             // ボディを分割
    ("datumPlane", "M3 19l4-4h14l-4 4z"),                       // 作業平面
    ("datumAxis", "M3 19l4-4h14l-4 4z"),                        // 作業軸
    ("datumPoint", "M3 19l4-4h14l-4 4z"),                       // 作業点
    ("skoffboth", "M8 20v-9a3 3 0 0 1 3-3h9"),                  // 両側へオフセット
    ("skmove", "M12 3v18M3 12h18"),                             // 移動(スケッチ)
    ("polycons", "M12 3l8 6-3 10H7L4 9z"),                      // ポリゴン(拘束)
    ("autocons", "M4 20L16 8"),                                 // 自動拘束
    ("wplaneadd", "M3 16l6-6h12l-6 6z"),                        // 作図面を作る
    ("replane", "M4 4h10v10H4z"),                               // スケッチ平面を再定義
    ("rigidgroup", "M3 11h9v9H3zM12 4h9v9h-9z"),                // 剛性グループ
];

/// 名前と、その絵の変わる部分(役と道筋)。名前は [`PATHS`] にある物だけ。役は道具と 1 対 1 の
/// 絵に付ける(同じ絵を違う役の道具に使わない)。
const ACCENTS: &[(&str, IconRole, &str)] = &[
    ("sketch", IconRole::Make, "M4 20c4-3 8 1 16-3"), // スケッチを作成
    ("sketch3d", IconRole::Make, "M8 15c2-7 6-1 11-9"), // 3D スケッチを追加
    ("textf", IconRole::Make, "M10 9h7M13.5 9v6"),    // 面に文字
    ("extrude", IconRole::Make, "M12 3v9M9 6l3-3 3 3"), // 押し出し
    ("hole", IconRole::Make, "M9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0"), // 穴
    (
        "thread",
        IconRole::Make,
        "M8 7l8-2M8 11l8-2M8 15l8-2M8 19l8-2",
    ), // ねじ
    (
        "revolve",
        IconRole::Make,
        "M7 8c-3 1-3 7 0 8M17 8c3 1 3 7 0 8",
    ), // 回転
    ("presspull", IconRole::Modify, "M4 9h16M12 3v4M10 5l2-2 2 2"), // プレス/プル
    ("fillet", IconRole::Modify, "M4 11a7 7 0 0 1 7-7"), // フィレット
    ("chamfer", IconRole::Modify, "M4 10l6-6"),       // 面取り
    (
        "boolean",
        IconRole::Modify,
        "M11 12a5 5 0 1 0 10 0a5 5 0 1 0-10 0",
    ), // ブーリアン
    ("shell", IconRole::Modify, "M8 7v9h8V7"),        // シェル
    ("param", IconRole::Modify, "M9 5v4M15 10v4M7 15v4"), // パラメータ
    (
        "linpat",
        IconRole::Repeat,
        "M14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z",
    ), // 矩形状パターン
    (
        "circpat",
        IconRole::Repeat,
        "M17 10h4v4h-4zM10 17h4v4h-4zM3 10h4v4H3zM12 12h.01",
    ), // 円形状パターン
    ("mirror", IconRole::Repeat, "M15 7l5 10h-5z"),   // ミラー
    ("joint", IconRole::Define, "M10 10h4M10 14h4M12 8v8"), // ジョイント
    ("line", IconRole::Make, "M5 19L19 5"),           // 線分
    ("rect", IconRole::Make, "M4 6h16v12H4z"),        // 長方形
    (
        "circle",
        IconRole::Make,
        "M12 5a7 7 0 1 0 0 14a7 7 0 1 0 0-14",
    ), // 円
    ("arc", IconRole::Make, "M5 18a9 9 0 0 1 14-10"), // 円弧
    (
        "slot",
        IconRole::Make,
        "M8 7h8a5 5 0 0 1 0 10H8a5 5 0 0 1 0-10",
    ), // 長穴
    ("skmirror", IconRole::Repeat, "M15 6c4 3 4 9 0 12"), // ミラー(スケッチ)
    ("trim", IconRole::Modify, "M10 12h1M13 12h1"),   // トリム
    ("skfillet", IconRole::Modify, "M5 12a7 7 0 0 1 7-7"), // フィレット(スケッチ)
    (
        "coinc",
        IconRole::Define,
        "M10 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0",
    ), // 一致
    ("parallel", IconRole::Define, "M12 20l6-16"),    // 平行
    ("equal", IconRole::Define, "M9 11h6M9 14h6"),    // 等しい
    (
        "dim",
        IconRole::Define,
        "M4 18h16M7 16l-3 2 3 2M17 16l3 2-3 2M10 10h4",
    ), // スケッチ寸法
    ("setup", IconRole::Define, "M2 22v-5M2 22h5"),   // セットアップ
    ("fixture", IconRole::Define, "M6 13V9h4v4M14 13V9h4v4"), // 固定具を置く
    ("contour", IconRole::Make, "M5 19V9l4-4h10v14z"), // 輪郭
    ("pocket", IconRole::Make, "M7 9h10v7H7zM10 12.5h4"), // ポケット
    ("face", IconRole::Make, "M4 5h16v3H4v3h16"),     // フェイス
    (
        "adaptive",
        IconRole::Make,
        "M6 13c0-3 4-3 4 0s4 3 4 0 4-3 4 0",
    ), // 負荷制御粗取り
    ("rough3d", IconRole::Make, "M5 20v-4h3v-3h3v-2h2v2h3v3h3v4"), // 3D 粗取り
    ("raster", IconRole::Make, "M6 20V9M10 20V6M14 20V6M18 20V9"), // 走査線(3D)
    ("waterline", IconRole::Make, "M5.5 16h13M8.5 12h7"), // 等高線(3D)
    ("drill", IconRole::Make, "M10 3h4v5l-2 6-2-6z"), // 穴あけ
    ("pins", IconRole::Make, "M7 8v10M17 8v10M5.5 8h3M15.5 8h3"), // ピン穴
    ("holeop", IconRole::Make, "M12 12h8M17 9l3 3-3 3"), // 穴から加工を作る
    ("manual_nc", IconRole::Make, "M10 11v6M14 11v6"), // 手動 NC
    (
        "campat",
        IconRole::Repeat,
        "M14 4h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z",
    ), // 加工のパターン
    ("hv", IconRole::Define, "M6 15h6M15 6v6"),       // 水平/垂直
    ("tangent", IconRole::Define, "M3 13h18"),        // 接線
    ("perp", IconRole::Define, "M12 19V5M12 15h4v4"), // 直交
    ("fix", IconRole::Define, "M8 11V8a4 4 0 0 1 8 0v3"), // 固定
    (
        "midpoint",
        IconRole::Define,
        "M10 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0",
    ), // 中点
    (
        "concentric",
        IconRole::Define,
        "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6",
    ), // 同心円
    ("collinear", IconRole::Define, "M14.5 10l6.5-5"), // 同一直線上
    ("symmetric", IconRole::Define, "M20 7l-4 10"),   // 対称
    ("curvature", IconRole::Define, "M11 18c5 0 7-6 10-12"), // 曲率
    ("skoffset", IconRole::Modify, "M8 20v-9a3 3 0 0 1 3-3h9"), // オフセット(スケッチ)
    ("movecopy", IconRole::Modify, "M14 5h7v7h-7zM10 12l4-4"), // 移動/コピー
    (
        "scallop3d",
        IconRole::Make,
        "M7 20c1.5-6 8.5-6 10 0M10.5 20c.5-2.5 2.5-2.5 3 0",
    ), // スキャロップ
    ("splitbody", IconRole::Modify, "M2 14l20-5"),    // ボディを分割
    ("datumPlane", IconRole::Make, "M3 10l4-4h14l-4 4z"), // 作業平面
    ("datumAxis", IconRole::Make, "M12 3v14"),        // 作業軸
    (
        "datumPoint",
        IconRole::Make,
        "M10 9a2 2 0 1 0 4 0a2 2 0 1 0-4 0",
    ), // 作業点
    ("midline", IconRole::Make, "M3 19L21 5"),        // 中点の線分
    ("rect3", IconRole::Make, "M3 14l9-9 8 8-9 9z"),  // 3 点指定の長方形
    ("crect", IconRole::Make, "M4 6h16v12H4z"),       // 中心指定の長方形
    (
        "rrect",
        IconRole::Make,
        "M8 6h8a4 4 0 0 1 4 4v4a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4v-4a4 4 0 0 1 4-4",
    ), // 角の丸い長方形
    (
        "circle2",
        IconRole::Make,
        "M12 5a7 7 0 1 0 0 14a7 7 0 1 0 0-14",
    ), // 2 点の円
    (
        "circle3",
        IconRole::Make,
        "M12 5a7 7 0 1 0 0 14a7 7 0 1 0 0-14",
    ), // 3 点の円
    (
        "tcircle",
        IconRole::Make,
        "M12 9.8a4.4 4.4 0 1 0 0 8.8a4.4 4.4 0 1 0 0-8.8",
    ), // 3 つに接する円
    (
        "tcircle2",
        IconRole::Make,
        "M12.5 11a4.5 4.5 0 1 0 0 9a4.5 4.5 0 1 0 0-9",
    ), // 2 つに接する円
    ("arc3", IconRole::Make, "M4 17a8 8 0 0 1 16 0"), // 3 点の円弧
    ("tarc", IconRole::Make, "M11 18a6 6 0 0 0 6-6V5"), // 接線円弧
    (
        "slotov",
        IconRole::Make,
        "M8 8h8a4 4 0 0 1 0 8H8a4 4 0 0 1 0-8",
    ), // 全長の長穴
    (
        "slotc",
        IconRole::Make,
        "M8 7h8a5 5 0 0 1 0 10H8a5 5 0 0 1 0-10",
    ), // 中心からの長穴
    (
        "slotarc",
        IconRole::Make,
        "M3 17a9 9 0 0 1 18 0M8 17a4 4 0 0 1 8 0M3 17h5M16 17h5",
    ), // 円弧の長穴
    (
        "slotarc3",
        IconRole::Make,
        "M3 17a9 9 0 0 1 18 0M8 17a4 4 0 0 1 8 0M3 17h5M16 17h5",
    ), // 3 点円弧の長穴
    ("polyout", IconRole::Make, "M12 3l8 6-3 10H7L4 9z"), // 外接の多角形
    ("polygon", IconRole::Make, "M12 3l8 6-3 10H7L4 9z"), // 内接の多角形
    ("polyedge", IconRole::Make, "M7 19h10l3-10-8-6-8 6z"), // 辺の多角形
    (
        "ellipse",
        IconRole::Make,
        "M3 12a9 5 0 1 0 18 0a9 5 0 1 0-18 0",
    ), // 楕円
    ("spline", IconRole::Make, "M3 18c4-12 8 4 11-6s5-6 7-6"), // 通る点のスプライン
    ("splinectl", IconRole::Make, "M3 18c3-8 7-6 9-2s6 0 9-10"), // 制御点のスプライン
    ("conic", IconRole::Make, "M4 19c2-10 6-14 16-15"), // 円錐曲線
    (
        "point",
        IconRole::Make,
        "M12 10a2 2 0 1 0 0 4a2 2 0 1 0 0-4",
    ), // 点
    ("blend", IconRole::Make, "M9 16c3 0 3-8 6-8"),   // ブレンド曲線(新しい曲線を作るので作る)
    ("project", IconRole::Make, "M12 3v8M9 8l3 3 3-3"), // 投影
    ("intersect", IconRole::Make, "M3 12h18"),        // 交差
    ("skchamfer", IconRole::Modify, "M4 11l7-7"),     // 面取り(スケッチ)
    ("extend", IconRole::Modify, "M15 9l5-5"),        // 延長
    ("skbreak", IconRole::Modify, "M9 9l6 6"),        // 破断
    (
        "skrotate",
        IconRole::Modify,
        "M19 12a7 7 0 1 1-2.1-5M19 3v4h-4",
    ), // 回転(スケッチ)
    ("skscale", IconRole::Modify, "M11 13l9-9M14 4h6v6"), // 拡大縮小(スケッチ)
    (
        "skoffin",
        IconRole::Modify,
        "M8 20v-9a3 3 0 0 1 3-3h9M14 14l-3 3M14 17v-3h-3",
    ), // 内へオフセット
    (
        "dashline",
        IconRole::Modify,
        "M4 20l2.5-2.5M9 15l2.5-2.5M14 10l2.5-2.5M19 5l1-1",
    ), // 補助線の切り替え
    (
        "skoffboth",
        IconRole::Modify,
        "M4 20V10a6 6 0 0 1 6-6h10M12 20v-6a2 2 0 0 1 2-2h6",
    ), // 両側へオフセット
    (
        "skmove",
        IconRole::Modify,
        "M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3",
    ), // 移動(スケッチ)
    (
        "online",
        IconRole::Define,
        "M10 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0",
    ), // 線上
    (
        "oncircle",
        IconRole::Define,
        "M10 7a2 2 0 1 0 4 0a2 2 0 1 0-4 0",
    ), // 円周上
    (
        "polycons",
        IconRole::Define,
        "M12 3h.01M20 9h.01M17 19h.01M7 19h.01M4 9h.01",
    ), // ポリゴン(拘束)
    (
        "autocons",
        IconRole::Define,
        "M18 3v5M15.5 5.5h5M19 12v3M17.5 13.5h3",
    ), // 自動拘束
    ("alongx", IconRole::Define, "M3 18h16M16 15l3 3-3 3"), // X に沿う
    ("alongy", IconRole::Define, "M3 18h16M16 15l3 3-3 3"), // Y に沿う
    ("alongz", IconRole::Define, "M3 18h16M16 15l3 3-3 3"), // Z に沿う
    (
        "onplane",
        IconRole::Define,
        "M12 4v6M10 13a2 2 0 1 0 4 0a2 2 0 1 0-4 0",
    ), // 平面の上
    ("parplane", IconRole::Define, "M6 9h12"),        // 平面に平行
    ("pdist", IconRole::Define, "M12 3v13M9 6l3-3 3 3"), // 平面との距離
    ("pangle", IconRole::Define, "M9 17l9-12M14 17a5 5 0 0 0-2-4"), // 平面との角度
    ("fangle", IconRole::Define, "M13 17a4 4 0 0 0 1-4"), // 面の角度
    (
        "axisdist",
        IconRole::Define,
        "M5 12h14M8 9l-3 3 3 3M16 9l3 3-3 3",
    ), // 軸の向きの差
    ("wplane", IconRole::Define, "M3 16l6-6h12l-6 6z"), // 作図面を選ぶ
    ("wplaneadd", IconRole::Make, "M17 2v6M14 5h6"),  // 作図面を作る
    ("isectcurve", IconRole::Make, "M4 5c4-3 8 3 16 0"), // 交差曲線
    ("projsurf", IconRole::Make, "M12 4v8M9 9l3 3 3-3"), // サーフェスに投影
    ("sweep", IconRole::Make, "M6 15c0-7 4-10 13-10M16 2l3 3-3 3"), // スイープ
    ("loft", IconRole::Make, "M3 17l11-11M13 17l9-11"), // ロフト
    ("sketchFace", IconRole::Make, "M13 20l7-7M16 11l3 3"), // 面にスケッチ
    ("replane", IconRole::Modify, "M10 10h10v10H10z"), // スケッチ平面を再定義
    ("component", IconRole::Make, "M12 4h9v9h-9z"),   // 新しい部品
    ("rigidgroup", IconRole::Define, "M7.5 15.5l9-7"), // 剛性グループ
    ("fastener", IconRole::Make, "M9 12l6-1M9 16l6-1"), // 規格部品
    ("split", IconRole::Modify, "M3 15l18-3v8H3z"),   // 割る平面を選ぶ
    ("draft", IconRole::Modify, "M13 4l4 16"),        // 勾配
    (
        "scale",
        IconRole::Modify,
        "M3 10V3h18v18h-7M10 14l9-9M14 5h5v5",
    ), // 尺度
    (
        "replace_face",
        IconRole::Modify,
        "M3 5h18M9 12V7M6.5 9.5 9 7l2.5 2.5",
    ), // 面を置換
    ("delete_face", IconRole::Modify, "M13 10l5 5M18 10l-5 5"), // 面を削除
    (
        "offset_face",
        IconRole::Modify,
        "M5 13c4-5 10-5 14 0M12 12V3M9 6l3-3 3 3",
    ), // 面をオフセット
    ("swatch", IconRole::Modify, "M10 8l5-3 5 13-10 3M7 16.5v.1"), // 外観
    ("engrave", IconRole::Make, "M5 6l7 12 7-12"),    // 彫刻・スロット・レーザー
    ("vcarve", IconRole::Make, "M5 6l7 12 7-12"),     // V 彫り
    ("trace", IconRole::Make, "M4 16c4-8 8 4 16-6"),  // トレース
    (
        "threadmill",
        IconRole::Make,
        "M7 7l10 2M7 11l10 2M7 15l10 2",
    ), // ねじ切り
    ("chamferop", IconRole::Make, "M14 7l6 6"),       // 面取り(加工)
    ("pencil", IconRole::Make, "M8 4v8a4 4 0 0 0 4 4h8"), // ペンシル(3D)
    ("flat3d", IconRole::Make, "M3 21h6M15 21h6"),    // 平坦部(3D)
    (
        "spiral3d",
        IconRole::Make,
        "M12 12a1.5 1.5 0 0 1 3 0a3 3 0 0 1-6 0a4.5 4.5 0 0 1 9 0a6 6 0 0 1-12 0",
    ), // 渦巻き(3D)
    (
        "radial3d",
        IconRole::Make,
        "M12 12V3M12 12l8-4M12 12l8 4M12 12v9M12 12l-8 4M12 12L4 8",
    ), // 放射状(3D)
    ("ramp3d", IconRole::Make, "M6 18l12-6M8 14l8-4"), // ランプ(3D)
    ("morph3d", IconRole::Make, "M7 9h10v7H7v-5h7v3h-4"), // モーフィング スパイラル(3D)
    ("project3d", IconRole::Make, "M12 3v9M9 9l3 3 3-3"), // 投影(3D)
];

/// 道具の役。アイコンの変わる部分をこの色で描く(調べる・出すの道具は役を持たず 1 層)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconRole {
    /// 形・経路を新しく生む(押し出し・穴・線分・輪郭)
    Make,
    /// 今ある物を直す(フィレット・面取り・トリム)
    Modify,
    /// 写す・並べる(パターン・ミラー)
    Repeat,
    /// 関係・条件を決める(ジョイント・拘束・寸法・セットアップ)
    Define,
}

/// アイコンを描く色(土台と、役ごとの変わる部分の色)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconInks {
    pub base: Color32,
    pub make: Color32,
    pub modify: Color32,
    pub repeat: Color32,
    pub define: Color32,
}

impl IconInks {
    /// 土台も変わる部分も 1 色(使えない時・使っている道具・道具でない絵)。
    pub const fn mono(c: Color32) -> IconInks {
        IconInks {
            base: c,
            make: c,
            modify: c,
            repeat: c,
            define: c,
        }
    }

    /// 役の色。
    pub fn role(&self, r: IconRole) -> Color32 {
        match r {
            IconRole::Make => self.make,
            IconRole::Modify => self.modify,
            IconRole::Repeat => self.repeat,
            IconRole::Define => self.define,
        }
    }
}

/// 名前の絵の元の道筋: 土台と、あれば変わる部分の役と道筋(画板の絵と比べる試験が使う)。
pub(crate) fn source(name: &str) -> Option<(&'static str, Option<(IconRole, &'static str)>)> {
    let base = PATHS.iter().find(|(k, _)| *k == name)?.1;
    let accent = ACCENTS
        .iter()
        .find(|(k, ..)| *k == name)
        .map(|(_, r, d)| (*r, *d));
    Some((base, accent))
}

/// 役を持たず、土台だけの 1 層で描く道具の絵(調べる・出す・取り込む・見る向き・選ぶ・消す・終える)。
/// ツールバーの道具の絵は、変わる部分([`ACCENTS`])を持つか、ここにあるかのどちらか 1 つ
/// (試験は [`crate::board::layer_gaps`] を使う。新しい道具に絵を付ける時に、役の色を付け忘れない)。
pub(crate) const ONE_LAYER: &[&str] = &[
    // 面を張る操作はボディの絵を共用する。
    "body",
    // 調べる
    "measure",
    "section",
    "volume",
    "interference",
    "table",
    "simulate",
    "gcodeview",
    "gcodecheck",
    // 出す
    "stepout",
    "stlout",
    "3mfout",
    "dxfout",
    "svgout",
    "gcodeout",
    "gcodeas",
    "gcodesort",
    "sheet",
    "sheethtml",
    "export",
    "play",
    // 取り込む(読み込んだ物を置くだけで、形を作る道具ではない)
    "step",
    "svg",
    "dxf",
    "canvas",
    "link",
    // 管理・見る向き・選ぶ・消す・終える
    "tools",
    "machine",
    "lookat",
    "select",
    "trash",
    "finish",
];

/// 道筋を折れ線にしたもの(24×24 の座標)。
struct Strokes {
    lines: Vec<(Vec<Pos2>, bool)>,
    dots: Vec<Pos2>,
}

/// 1 つの絵: 土台と、あれば変わる部分。
struct Icon {
    base: Strokes,
    accent: Option<(IconRole, Strokes)>,
}

fn table() -> &'static HashMap<&'static str, Icon> {
    static T: OnceLock<HashMap<&'static str, Icon>> = OnceLock::new();
    T.get_or_init(|| build(PATHS, ACCENTS))
}

/// 土台の表と変わる部分の表から絵の表を作る(変わる部分の名前が土台に無ければ飛ばす。試験で落とす)。
fn build(
    paths: &[(&'static str, &str)],
    accents: &[(&'static str, IconRole, &str)],
) -> HashMap<&'static str, Icon> {
    let mut t: HashMap<&'static str, Icon> = paths
        .iter()
        .map(|(k, d)| {
            (
                *k,
                Icon {
                    base: parse(d),
                    accent: None,
                },
            )
        })
        .collect();
    for (k, role, d) in accents {
        if let Some(i) = t.get_mut(k) {
            i.accent = Some((*role, parse(d)));
        }
    }
    t
}

/// `rect` の中にアイコンを 1 色で描く。
pub fn paint(p: &Painter, rect: Rect, name: &str, color: Color32, width: f32) {
    paint_inks(p, rect, name, &IconInks::mono(color), width);
}

/// `rect` の中にアイコンを描く。土台は `inks.base`、変わる部分は役の色で少し太く描く。
pub fn paint_inks(p: &Painter, rect: Rect, name: &str, inks: &IconInks, width: f32) {
    // 設定の「アイコンの大きさ」で、真ん中を変えずに広げる(1 倍ならそのまま)
    let s = kmb_ui_view3d::scale().icon;
    let rect = if s == 1.0 {
        rect
    } else {
        Rect::from_center_size(rect.center(), rect.size() * s)
    };
    paint_exact_inks(p, rect, name, inks, width);
}

/// [`paint`] と同じだが、アイコンの倍率を掛けない(倍率を掛けた枠を自分で決める部品が使う)。
pub fn paint_exact(p: &Painter, rect: Rect, name: &str, color: Color32, width: f32) {
    paint_exact_inks(p, rect, name, &IconInks::mono(color), width);
}

/// 変わる部分の線の太さの、土台に対する比(土台 1.5 に対して 2)。
const ACCENT_WIDTH: f32 = 4.0 / 3.0;

/// [`paint_inks`] と同じだが、アイコンの倍率を掛けない。
pub fn paint_exact_inks(p: &Painter, rect: Rect, name: &str, inks: &IconInks, width: f32) {
    draw(p, rect, name, table().get(name), inks, width);
}

/// 絵 `icon` を描く(`None` なら名前の頭の 1 文字)。
fn draw(p: &Painter, rect: Rect, name: &str, icon: Option<&Icon>, inks: &IconInks, width: f32) {
    let Some(icon) = icon else {
        // 知らない名前: 頭の 1 文字
        let k = rect.width().min(rect.height()) / 24.0;
        p.circle_stroke(
            rect.center(),
            9.0 * k,
            Stroke::new(width * k.max(0.75), inks.base),
        );
        let c = name
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            c,
            egui::FontId::proportional(11.0 * k),
            inks.base,
        );
        return;
    };
    p.extend(shapes(rect, icon, inks, width));
}

/// 絵の線と点(土台、続けて変わる部分)。
fn shapes(rect: Rect, icon: &Icon, inks: &IconInks, width: f32) -> Vec<Shape> {
    let k = rect.width().min(rect.height()) / 24.0;
    let o = rect.center() - Vec2::splat(12.0 * k);
    let at = |q: Pos2| o + q.to_vec2() * k;
    let mut out = vec![];
    let mut add = |s: &Strokes, stroke: Stroke| {
        for (pts, closed) in &s.lines {
            let pts: Vec<Pos2> = pts.iter().map(|q| at(*q)).collect();
            out.push(if *closed {
                Shape::closed_line(pts, stroke)
            } else {
                Shape::line(pts, stroke)
            });
        }
        for d in &s.dots {
            out.push(Shape::circle_filled(
                at(*d),
                stroke.width * 0.7,
                stroke.color,
            ));
        }
    };
    let stroke = Stroke::new(width * k.max(0.75), inks.base);
    add(&icon.base, stroke);
    if let Some((role, a)) = &icon.accent {
        add(
            a,
            Stroke::new(stroke.width * ACCENT_WIDTH, inks.role(*role)),
        );
    }
    out
}

/// SVG の道筋を読む(M L H V C S Q A Z と、その小文字)。曲線は短い直線に分ける。
fn parse(d: &str) -> Strokes {
    let mut toks: Vec<Tok> = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() {
            toks.push(Tok::Cmd(c));
            i += 1;
        } else if c == '-' || c == '.' || c.is_ascii_digit() {
            let s = i;
            let mut dot = c == '.';
            i += 1;
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == '.' && !dot {
                    dot = true;
                    i += 1;
                } else {
                    break;
                }
            }
            toks.push(Tok::Num(d[s..i].parse().unwrap_or(0.0)));
        } else {
            i += 1;
        }
    }
    let mut icon = Strokes {
        lines: Vec::new(),
        dots: Vec::new(),
    };
    let mut cur: Vec<Pos2> = Vec::new();
    let mut pos = Pos2::ZERO;
    let mut start = Pos2::ZERO;
    let mut last_ctrl: Option<Pos2> = None;
    let mut cmd = 'M';
    let mut k = 0;
    let flush = |cur: &mut Vec<Pos2>, icon: &mut Strokes, closed: bool| {
        if cur.len() >= 2 {
            let len: f32 = cur.windows(2).map(|w| (w[1] - w[0]).length()).sum();
            if len < 0.2 && !closed {
                icon.dots.push(cur[0]);
            } else {
                icon.lines.push((std::mem::take(cur), closed));
            }
        }
        cur.clear();
    };
    let num = |k: &mut usize| -> Option<f32> {
        match toks.get(*k) {
            Some(Tok::Num(v)) => {
                *k += 1;
                Some(*v)
            }
            _ => None,
        }
    };
    while k < toks.len() {
        if let Tok::Cmd(c) = toks[k] {
            cmd = c;
            k += 1;
            if c == 'z' || c == 'Z' {
                flush(&mut cur, &mut icon, true);
                pos = start;
                last_ctrl = None;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { pos.to_vec2() } else { Vec2::ZERO };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let (Some(x), Some(y)) = (num(&mut k), num(&mut k)) else {
                    break;
                };
                flush(&mut cur, &mut icon, false);
                pos = Pos2::new(x, y) + base;
                start = pos;
                cur.push(pos);
                // 続く数は直線
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' => {
                let (Some(x), Some(y)) = (num(&mut k), num(&mut k)) else {
                    break;
                };
                pos = Pos2::new(x, y) + base;
                cur.push(pos);
                last_ctrl = None;
            }
            'H' => {
                let Some(x) = num(&mut k) else { break };
                pos.x = if rel { pos.x + x } else { x };
                cur.push(pos);
                last_ctrl = None;
            }
            'V' => {
                let Some(y) = num(&mut k) else { break };
                pos.y = if rel { pos.y + y } else { y };
                cur.push(pos);
                last_ctrl = None;
            }
            'C' | 'S' => {
                let c1 = if cmd.eq_ignore_ascii_case(&'C') {
                    let (Some(x), Some(y)) = (num(&mut k), num(&mut k)) else {
                        break;
                    };
                    Pos2::new(x, y) + base
                } else {
                    last_ctrl.map_or(pos, |c| pos + (pos - c))
                };
                let (Some(x2), Some(y2), Some(x), Some(y)) =
                    (num(&mut k), num(&mut k), num(&mut k), num(&mut k))
                else {
                    break;
                };
                let c2 = Pos2::new(x2, y2) + base;
                let end = Pos2::new(x, y) + base;
                for s in 1..=10 {
                    let t = s as f32 / 10.0;
                    let u = 1.0 - t;
                    let q = pos.to_vec2() * (u * u * u)
                        + c1.to_vec2() * (3.0 * u * u * t)
                        + c2.to_vec2() * (3.0 * u * t * t)
                        + end.to_vec2() * (t * t * t);
                    cur.push(q.to_pos2());
                }
                last_ctrl = Some(c2);
                pos = end;
            }
            'Q' => {
                let (Some(x1), Some(y1), Some(x), Some(y)) =
                    (num(&mut k), num(&mut k), num(&mut k), num(&mut k))
                else {
                    break;
                };
                let c1 = Pos2::new(x1, y1) + base;
                let end = Pos2::new(x, y) + base;
                for s in 1..=8 {
                    let t = s as f32 / 8.0;
                    let u = 1.0 - t;
                    let q = pos.to_vec2() * (u * u)
                        + c1.to_vec2() * (2.0 * u * t)
                        + end.to_vec2() * (t * t);
                    cur.push(q.to_pos2());
                }
                last_ctrl = None;
                pos = end;
            }
            'A' => {
                let (Some(rx), Some(ry), Some(rot), Some(large), Some(sweep), Some(x), Some(y)) = (
                    num(&mut k),
                    num(&mut k),
                    num(&mut k),
                    num(&mut k),
                    num(&mut k),
                    num(&mut k),
                    num(&mut k),
                ) else {
                    break;
                };
                let end = Pos2::new(x, y) + base;
                arc(&mut cur, pos, end, rx, ry, rot, large != 0.0, sweep != 0.0);
                pos = end;
                last_ctrl = None;
            }
            _ => {
                k += 1;
            }
        }
        if cur.is_empty() {
            cur.push(pos);
        }
    }
    flush(&mut cur, &mut icon, false);
    icon
}

#[derive(Clone, Copy)]
enum Tok {
    Cmd(char),
    Num(f32),
}

/// SVG の楕円弧(端点の形)を折れ線にする(SVG 1.1 付録 F.6 の換算)。
#[allow(clippy::too_many_arguments)]
fn arc(
    out: &mut Vec<Pos2>,
    p0: Pos2,
    p1: Pos2,
    rx: f32,
    ry: f32,
    rot: f32,
    large: bool,
    sweep: bool,
) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-6 || ry < 1e-6 || (p1 - p0).length() < 1e-6 {
        out.push(p1);
        return;
    }
    let phi = rot.to_radians();
    let (sp, cp) = phi.sin_cos();
    let d = (p0 - p1) / 2.0;
    let x1 = cp * d.x + sp * d.y;
    let y1 = -sp * d.x + cp * d.y;
    let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lam > 1.0 {
        rx *= lam.sqrt();
        ry *= lam.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = (num / den).max(0.0).sqrt();
    if large == sweep {
        co = -co;
    }
    let cx1 = co * rx * y1 / ry;
    let cy1 = -co * ry * x1 / rx;
    let m = (p0.to_vec2() + p1.to_vec2()) / 2.0;
    let c = Pos2::new(cp * cx1 - sp * cy1 + m.x, sp * cx1 + cp * cy1 + m.y);
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = ang(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dt = ang(
        (x1 - cx1) / rx,
        (y1 - cy1) / ry,
        (-x1 - cx1) / rx,
        (-y1 - cy1) / ry,
    );
    let tau = std::f32::consts::TAU;
    if !sweep && dt > 0.0 {
        dt -= tau;
    } else if sweep && dt < 0.0 {
        dt += tau;
    }
    let n = ((dt.abs() / tau) * 32.0).ceil().max(2.0) as usize;
    for s in 1..=n {
        let t = t1 + dt * s as f32 / n as f32;
        let (st, ct) = t.sin_cos();
        out.push(Pos2::new(
            c.x + rx * ct * cp - ry * st * sp,
            c.y + rx * ct * sp + ry * st * cp,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn each_strokes(i: &Icon) -> Vec<&Strokes> {
        let mut v = vec![&i.base];
        v.extend(i.accent.as_ref().map(|a| &a.1));
        v
    }

    #[test]
    fn every_icon_reads_into_lines_inside_its_box() {
        for (name, _) in PATHS {
            for s in each_strokes(&table()[name]) {
                assert!(!s.lines.is_empty() || !s.dots.is_empty(), "{name}");
                for (pts, _) in &s.lines {
                    for p in pts {
                        assert!(
                            p.x > -2.0 && p.x < 26.0 && p.y > -2.0 && p.y < 26.0,
                            "{name} {p:?}"
                        );
                    }
                }
            }
        }
        // 円は弧 2 つで閉じる(円の絵の円は変わる部分)
        let c = &table()["circle"].accent.as_ref().unwrap().1;
        let n: usize = c.lines.iter().map(|l| l.0.len()).sum();
        assert!(n > 20);
    }

    /// 変わる部分は土台の表にある絵に 1 つずつ付き、名前は重ならない。
    #[test]
    fn every_accent_belongs_to_one_drawn_icon() {
        let mut seen = std::collections::HashSet::new();
        for (name, _, _) in ACCENTS {
            assert!(PATHS.iter().any(|p| p.0 == *name), "{name} の土台が無い");
            assert!(seen.insert(*name), "{name} の変わる部分が 2 つある");
        }
        let mut names = std::collections::HashSet::new();
        for (name, _) in PATHS {
            assert!(names.insert(*name), "{name} の絵が 2 つある");
        }
    }

    /// 線の色と太さ(点は色と半径)。
    fn looks(v: &[Shape]) -> Vec<(Color32, f32)> {
        v.iter()
            .map(|s| match s {
                Shape::Path(p) => match p.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => (c, p.stroke.width),
                    _ => panic!("線の色が 1 色でない"),
                },
                Shape::Circle(c) => (c.fill, c.radius),
                other => panic!("思わない形 {other:?}"),
            })
            .collect()
    }

    /// 土台は土台の色、変わる部分は役の色で少し太く描く。1 色の時は両方がその色。
    #[test]
    fn the_accent_is_drawn_after_the_base_in_its_role_color() {
        let t = build(
            &[("t", "M4 16l8 4 8-4-8-4z"), ("one", "M4 4h16")],
            &[("t", IconRole::Modify, "M12 3v9M9 6l3-3 3 3")],
        );
        let inks = IconInks {
            base: Color32::GRAY,
            make: Color32::BLUE,
            modify: Color32::from_rgb(240, 165, 74),
            repeat: Color32::RED,
            define: Color32::GREEN,
        };
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(24.0));
        let v = looks(&shapes(rect, &t["t"], &inks, 1.5));
        assert_eq!(v.len(), 3, "{v:?}");
        assert_eq!(v[0], (Color32::GRAY, 1.5));
        assert_eq!(v[1], (inks.modify, 2.0));
        assert_eq!(v[2], (inks.modify, 2.0));
        // 1 色(使えない時など)
        let off = Color32::DARK_GRAY;
        let v = looks(&shapes(rect, &t["t"], &IconInks::mono(off), 1.5));
        assert!(v.iter().all(|x| x.0 == off), "{v:?}");
        // 変わる部分の無い絵は土台だけ
        let v = looks(&shapes(rect, &t["one"], &inks, 1.5));
        assert_eq!(v, vec![(Color32::GRAY, 1.5)]);
        // 土台に無い名前の変わる部分は飛ばす(表の試験で落とす)
        let t = build(&[("a", "M4 4h16")], &[("b", IconRole::Make, "M4 4h16")]);
        assert!(t["a"].accent.is_none() && !t.contains_key("b"));
    }
}
