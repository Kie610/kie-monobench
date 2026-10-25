//! 今の視点から見えるボディの線を DXF か SVG にする操作(図面に近い書き出し)。画面は保存先を選ばせ、
//! CLI・MCP は指定の場所に書く。表示中のスケッチの線も書く。引数 `exclude` の
//! ボディとスケッチは書かない([`Context::export_bodies`])。

use crate::view_dxf::{View, view_to_dxf};
use crate::view_svg::view_to_svg;
use kmb_core::kernel::P3;
use kmb_core::{Action, Arg, Context, Curves, Extension, Input, Outcome, Payload, Place, Wires};
use serde_json::json;

/// 今の視点から見えるボディの線を 2D の DXF に書き出す。引数 `view` は視点(画面は今の視点を渡す。
/// CLI・MCP は `"top"`・`"front"`・`"iso"` などの名前か、`{"eye": [..], "up": [..]}`)、
/// `hidden` が真なら隠れ線も破線の画層に書く、`tolerance` は曲面を網にする許容差 mm。
pub const EXPORT_VIEW_DXF: &str = "io.export_view_dxf";
/// [`EXPORT_VIEW_DXF`] を、隠れ線も破線で書く形で。
pub const EXPORT_VIEW_DXF_HIDDEN: &str = "io.export_view_dxf_hidden";
/// 今の視点から見えるボディの線を SVG に書き出す(レーザー加工機の LightBurn などで使う。1 単位 = 1 mm)。
/// 引数は [`EXPORT_VIEW_DXF`] と同じ(`view`・`hidden`・`tolerance`)。
pub const EXPORT_VIEW_SVG: &str = "io.export_view_svg";

/// 3MF の書き出し(kmb-io-stl)。視点の DXF はこの前に並べる(ファイルのメニューの並びを保つ)。
const EXPORT_3MF: &str = "io.export_3mf";

/// 今の視点の線の許容差の既定(mm)。
const VIEW_TOLERANCE: f64 = 0.02;

/// 視点の書き出しの操作。
pub struct ViewCommands;

/// 引数 `view` の視点。
fn view_arg(cx: &Context<'_>) -> Result<View, String> {
    let bad = || {
        "view は top・bottom・front・back・right・left・iso か、{\"eye\": [x, y, z], \"up\": [x, y, z]} で指定します".to_string()
    };
    let v = cx.arg("view").ok_or_else(bad)?;
    if let Some(name) = v.as_str() {
        return View::named(name).ok_or_else(bad);
    }
    let vec3 = |k: &str| -> Option<[f64; 3]> {
        let a = v.get(k)?.as_array()?;
        let c: Vec<f64> = a.iter().filter_map(|x| x.as_f64()).collect();
        (c.len() == 3).then(|| [c[0], c[1], c[2]])
    };
    View::new(vec3("eye").ok_or_else(bad)?, vec3("up").ok_or_else(bad)?).ok_or_else(bad)
}

fn view_dxf(cx: &Context<'_>, hidden_default: bool) -> Result<Outcome, String> {
    let view = view_arg(cx)?;
    let hidden = cx
        .arg("hidden")
        .and_then(|v| v.as_bool())
        .unwrap_or(hidden_default);
    let tol = cx.arg_f64("tolerance")?.unwrap_or(VIEW_TOLERANCE);
    if !(tol > 0.0001 && tol <= 1.0) {
        return Err("許容差は 0.0001〜1 mm にしてください".into());
    }
    let meshes = cx
        .export_bodies()?
        .iter()
        .map(|h| cx.kernel.mesh(*h, tol))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let sketches = sketch_lines(cx, tol);
    let r = view_to_dxf(&meshes, &sketches, &view, hidden, tol);
    if r.visible == 0 {
        return Err("見える線がありません".into());
    }
    Ok(Outcome::File {
        suffix: "視点.dxf".into(),
        contents: r.dxf.into_bytes(),
        info: json!({"bodies": meshes.len(), "sketch_lines": sketches.len(), "visible": r.visible, "hidden": r.hidden}),
    })
}

fn view_svg(cx: &Context<'_>) -> Result<Outcome, String> {
    let view = view_arg(cx)?;
    let hidden = cx.arg("hidden").and_then(|v| v.as_bool()) == Some(true);
    let tol = cx.arg_f64("tolerance")?.unwrap_or(VIEW_TOLERANCE);
    if !(tol > 0.0001 && tol <= 1.0) {
        return Err("許容差は 0.0001〜1 mm にしてください".into());
    }
    let meshes = cx
        .export_bodies()?
        .iter()
        .map(|h| cx.kernel.mesh(*h, tol))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let sketches = sketch_lines(cx, tol);
    let r = view_to_svg(&meshes, &sketches, &view, hidden, tol);
    if r.visible == 0 {
        return Err("見える線がありません".into());
    }
    log::info!(
        "今の視点を SVG に書き出す: 見える線 {}、隠れ線 {}",
        r.visible,
        r.hidden
    );
    Ok(Outcome::File {
        suffix: "視点.svg".into(),
        contents: r.svg.into_bytes(),
        info: json!({"bodies": meshes.len(), "sketch_lines": sketches.len(), "visible": r.visible, "hidden": r.hidden}),
    })
}

/// 一緒に書くスケッチの線(世界座標の折れ線)。止めていないスケッチのうち、引数 `exclude` に無い
/// 物(画面は目を閉じたスケッチを渡す)。補助線と単独の点は書かない(3D のビューと同じく、補助線は
/// スケッチの出力に入らない)。`tol` は円弧を折れ線にする許容差 mm。
fn sketch_lines(cx: &Context<'_>, tol: f64) -> Vec<Vec<P3>> {
    let exclude: Vec<u64> = cx
        .arg("exclude")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for f in &cx.data.features {
        let is_sketch = cx
            .registry
            .descriptor(&f.type_id)
            .is_some_and(|d| d.category == kmb_core::category::SKETCH);
        if !is_sketch || f.suppressed || exclude.contains(&f.id.0) {
            continue;
        }
        // 3D スケッチは世界座標の折れ線
        if let Some(w) = cx
            .output(f.id, "lines")
            .and_then(<Wires as Payload>::from_value)
        {
            out.extend(
                w.0.into_iter()
                    .filter(|w| !w.construction)
                    .map(|w| w.points),
            );
            continue;
        }
        let Some(c) = cx.output(f.id, "curves").and_then(Curves::from_value) else {
            continue;
        };
        for seg in &c.segments {
            let mut pts: Vec<P3> = seg
                .polyline(seg.steps_per_turn(tol))
                .into_iter()
                .map(|q| c.plane.to_world(q))
                .collect();
            pts.push(c.plane.to_world(seg.end_point()));
            out.push(pts);
        }
    }
    out
}

/// 視点の書き出しが受け取る引数。
fn view_args() -> Vec<Arg> {
    vec![
        Arg::list(
            "exclude",
            "書かないボディとスケッチ(機能の番号の並び。画面は目を閉じたボディとスケッチを渡す)",
        ),
        Arg::value(
            "view",
            "視点。top・bottom・front・back・right・left・iso か {\"eye\": [x, y, z], \"up\": [x, y, z]}",
        )
        .required(),
        Arg::flag("hidden", "隠れ線も破線で書く"),
        Arg::length("tolerance", "線の許容差(0.0001〜1 mm)").default(VIEW_TOLERANCE),
    ]
}

impl Extension for ViewCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let why = cx.export_bodies().err();
        let save = |title: &str, filter: &str, ext: &[&str]| Input::SaveFile {
            title: title.into(),
            filter: filter.into(),
            extensions: ext.iter().map(|e| e.to_string()).collect(),
        };
        vec![
            Action::new(EXPORT_VIEW_DXF, "今のビューを DXF に書き出す…", "cad")
                .menu("書き出し")
                .icon("dxfout")
                .aliases("でぃーえっくすえふ dxf 2d 図面 投影 してん れーざー")
                .hint("今の視点から見えるボディの線(稜線と曲面の輪郭)と表示中のスケッチの線を、1 mm = 1 単位の 2D の DXF にする。透視の視点も平行に投影して書く。隠れ線は書かない")
                .at(&[Place::Menu, Place::FileMenu])
                .input(save("今の視点を DXF に書き出す", "DXF", &["dxf"]))
                .args(view_args())
                .disabled_if(why.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES)
                .before(EXPORT_3MF),
            Action::new(EXPORT_VIEW_DXF_HIDDEN, "今の視点を DXF に書き出す(隠れ線も)…", "cad")
                .menu("書き出し")
                .icon("dxfout")
                .aliases("でぃーえっくすえふ dxf 2d 図面 投影 かくれせん 破線")
                .hint("今の視点の DXF に、隠れて見えない線も破線の画層 HIDDEN に分けて書く(見える線は画層 VISIBLE)")
                .at(&[Place::Menu, Place::FileMenu])
                .input(save("今の視点を DXF に書き出す(隠れ線も)", "DXF", &["dxf"]))
                .args(view_args())
                .disabled_if(why.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES)
                .before(EXPORT_3MF),
            Action::new(EXPORT_VIEW_SVG, "今のビューを SVG に書き出す…", "cad")
                .menu("書き出し")
                .icon("svgout")
                .aliases("えすぶいじー svg 2d 投影 してん れーざー lightburn らいとばーん")
                .hint("今の視点から見えるボディの線と表示中のスケッチの線を、1 mm = 1 単位の SVG にする(LightBurn などのレーザー加工機のソフトで同じ大きさで開ける)。線だけで塗らず、つながる線は閉じた輪郭にまとめる。隠れ線は書かない")
                .at(&[Place::Menu, Place::FileMenu])
                .input(save("今のビューを SVG に書き出す", "SVG", &["svg"]))
                .args(view_args())
                .disabled_if(why.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        match id {
            EXPORT_VIEW_DXF => Some(view_dxf(cx, false)),
            EXPORT_VIEW_DXF_HIDDEN => Some(view_dxf(cx, true)),
            EXPORT_VIEW_SVG => Some(view_svg(cx)),
            _ => None,
        }
    }
}
